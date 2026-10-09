// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Pop-out dialog windows (About / Settings) as real child OS windows, so a
//! dialog larger than the main window is still fully visible (the in-surface
//! overlay was clipped by the main window). Each dialog owns its surface + text
//! context and is sized to its content (non-resizable).
use std::collections::HashMap;
use std::sync::Arc;

use glyphon::{Attrs, Color as GColor, Shaping, TextArea, TextBounds};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::RawWindowHandle;
use winit::window::{Window, WindowId};

use crate::config;
use crate::gfx::{FRAME_RETRY_FIRST, FRAME_RETRY_MAX, Gfx, RectInstance, RectRenderer, Retry};
use crate::pane::Rect;
use crate::settings_ui::{Action, EditCmd, SettingsDialog, View, quad};
use crate::text::{TextCtx, ui_attrs};

// A laid-out line of static dialog text (window-relative coords).
struct Line {
	text: String,
	x: f32,
	y: f32,
	color: [u8; 3],
	bold: bool,
	scale: f32,
}

// A clickable region in the About box. `button` links draw a filled box behind
// their label (the Support button); plain links are just colored text. A link
// with a `tooltip` shows that text as a flyover while the cursor is over it -
// used so the Support button can reveal the URL it opens without baking it into
// the label.
struct AboutLink {
	rect: Rect,
	// None closes the window, which is a notice's OK
	url: Option<String>,
	tooltip: Option<String>,
	button: bool,
}

enum Content {
	// the About box, or a notice laid out the same way
	About {
		lines: Vec<Line>,
		links: Vec<AboutLink>,
		// what the lines were made from, so a scale change can make them again
		source: AboutSource,
	},
	Settings(SettingsDialog),
}

// Neither box can be resized and both are laid out once at open, so a change of
// display scale has to run the layout again from scratch.
enum AboutSource {
	About(Box<wgpu::AdapterInfo>),
	// Windows says a notice with MessageBoxW, so it has no dialog window of its own
	#[cfg(not(target_os = "windows"))]
	Notice(Vec<String>),
}

impl AboutSource {
	fn is_notice(&self) -> bool {
		match self {
			AboutSource::About(_) => false,
			#[cfg(not(target_os = "windows"))]
			AboutSource::Notice(_) => true,
		}
	}
}

#[derive(Debug)]
pub enum DialogAction {
	OpenUrl(String),
	Apply,         // apply settings, keep the dialog open (live preview)
	ApplyAndClose, // OK
	Close,         // Cancel / Esc / window close
}

pub struct DialogWin {
	pub window: Arc<Window>,
	gfx: Gfx,
	text: TextCtx,
	rects: RectRenderer,
	content: Content,
	mouse: (f32, f32),
	// field-edit animation (view scroll / caret ease / blink): frame timing and
	// the wake cadence the app loop should keep while something animates
	last_frame: std::time::Instant,
	anim_wake: Option<u64>,
	/// a frame the surface refused, drawn again on this (read by the app loop)
	pub refused: Retry,
	// what the pointer is resting on, and since when: flyover help waits the same
	// DELAY here as it does in the tab strip and the menus
	tip: crate::tip::Dwell<Rect>,
	// the tip the last frame drew, so a dwell that ripens between frames is owed one
	tip_drawn: Option<Rect>,
	shaped: ShapedText,
	// the terminal window this dialog belongs to, so we can restack it beneath
	// us when we're activated (see raise_parent).
	parent: Option<RawWindowHandle>,
	// a resize snap has been asked for and not yet seen: see snap_to_natural
	snapped: bool,
	// what the screen leaves this window, physical pixels. The snap may not pull
	// it back past this. Unbounded for About, which cannot be resized.
	caps: (f32, f32),
	// The held keys, as this window last reported them. They live and die with
	// the window: a closing macOS window never hears its keys let go, and one
	// that gains the focus is told nothing until a key changes.
	mods: ModifiersState,
}

impl std::fmt::Debug for DialogWin {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DialogWin")
			.field("window", &self.window.id())
			.finish_non_exhaustive()
	}
}

impl DialogWin {
	pub fn device(&self) -> &wgpu::Device {
		&self.gfx.device
	}

	pub fn id(&self) -> WindowId {
		self.window.id()
	}

	/// Restack the terminal to sit directly beneath this dialog. Called when the
	/// dialog gains focus, so a window that got in front of the terminal can't
	/// stay wedged between them - the transient hints alone don't force this on
	/// WMs that don't raise a transient's parent with it (Compiz).
	pub fn raise_parent(&self) {
		// SILK_MODALDBG=1 traces the restack + the resulting stack order, so a WM
		// where this misbehaves (e.g. a Compiz profile that ignores the restack)
		// can be diagnosed from the terminal without a headless rig.
		let dbg = std::env::var_os("SILK_MODALDBG").is_some();
		restack_parent_below(&self.window, self.parent.as_ref(), dbg, self.kind());
	}

	fn kind(&self) -> &'static str {
		match &self.content {
			Content::About { source, .. } if source.is_notice() => "Notice",
			Content::About { .. } => "About",
			Content::Settings(_) => "Settings",
		}
	}

	fn make(
		el: &ActiveEventLoop,
		title: String,
		w: f32,
		h: f32,
		resizable: bool,
		parent: Option<RawWindowHandle>,
		warm: Option<&crate::gfx::DialogGpu>,
	) -> anyhow::Result<(Arc<Window>, Gfx, TextCtx, RectRenderer)> {
		#[allow(unused_mut)] // reassigned on linux/windows/macos below
		let mut attrs = Window::default_attributes()
			.with_title(title)
			.with_window_icon(crate::app::load_icon())
			.with_resizable(resizable)
			.with_inner_size(winit::dpi::PhysicalSize::new(
				w.ceil().max(1.0) as u32,
				h.ceil().max(1.0) as u32,
			));
		// Tie the dialog to the terminal window so the WM keeps it above its
		// parent and groups them. Windows: MUST be owner semantics, not winit's
		// generic parent_window - that creates a WS_CHILD window there, which
		// embeds the dialog inside the terminal's client area (clipped when the
		// dialog is bigger than the terminal) and never gets its own keyboard
		// activation (text fields dead). An owned popup floats above the owner,
		// takes focus normally, and stays off the taskbar. macOS: parent_window
		// is child-window-of semantics, which is what we want there. X11:
		// parent_window means literal X reparenting (an embedded child, unmanaged
		// by the WM), so DON'T pass it there; WM_TRANSIENT_FOR is set after
		// creation instead (below).
		#[cfg(target_os = "windows")]
		if let Some(RawWindowHandle::Win32(h)) = parent {
			use winit::platform::windows::WindowAttributesExtWindows;
			attrs = attrs.with_owner_window(h.hwnd.get());
		}
		#[cfg(target_os = "macos")]
		if parent.is_some() {
			// SAFETY: the handle comes from the live main window on this same thread.
			attrs = unsafe { attrs.with_parent_window(parent) };
		}
		// X11: create unmapped so WM_TRANSIENT_FOR + the modal/dialog hints are all
		// set BEFORE the WM maps the window and fixes its stacking group. A post-map
		// property write is read too late by Compiz et al - that's why re-selecting
		// the dialog raised it alone and left the parent buried. The caller shows the
		// window after the final resize (see new_about / new_settings).
		// Windows: an owned popup gets no auto-placement and appears at the screen
		// origin, so it must be created hidden, centered over the terminal, drawn
		// once, then revealed by the caller - otherwise it flashes at (0,0) then
		// jumps to center.
		#[cfg(any(target_os = "linux", target_os = "windows"))]
		{
			attrs = attrs.with_visible(false);
		}
		let window = Arc::new(el.create_window(attrs)?);
		set_transient_for(&window, parent.as_ref());
		// PRIMARY (no GL): the main window may hold a glutin GL/EGL context, and a
		// second wgpu GL instance would panic in EGL teardown. Dialogs are opaque,
		// so Vulkan/Metal/DX12 is all they need. The warm context (see DialogGpu)
		// has already paid for the instance/adapter/device; without it, or if its
		// adapter can't present here, build one the slow way.
		let warmed = warm.and_then(|gpu| Gfx::with_dialog_gpu(window.clone(), gpu));
		let mut gfx = match warmed {
			Some(gfx) => gfx,
			None => Gfx::with_backends(
				window.clone(),
				wgpu::Backends::PRIMARY,
				crate::gfx::wanted(),
			)?,
		};
		// adopt the size winit actually gave us
		let size = window.inner_size();
		gfx.resize(size.width, size.height);
		let scale = config::display_scale(window.scale_factor());
		let text = TextCtx::new(&gfx.device, &gfx.queue, gfx.format, scale);
		let rects = RectRenderer::new(&gfx.device, gfx.format);
		Ok((window, gfx, text, rects))
	}

	pub fn new_about(
		el: &ActiveEventLoop,
		adapter: &wgpu::AdapterInfo,
		parent: Option<RawWindowHandle>,
		warm: Option<&crate::gfx::DialogGpu>,
	) -> anyhow::Result<Self> {
		// provisional window so we have a TextCtx to measure with
		let (window, mut gfx, mut text, rects) = Self::make(
			el,
			format!("About {}", config::APP_NAME),
			560.0,
			360.0,
			false,
			parent,
			warm,
		)?;
		let (lines, links, size) = layout_about(&mut text, adapter);
		let requested_size =
			winit::dpi::PhysicalSize::new(size.0.ceil() as u32, size.1.ceil() as u32);
		if let Some(applied) = crate::app::request_size(&window, requested_size) {
			gfx.resize(applied.width, applied.height);
		}
		// mapped last, at the final size, with the transient hints already in place
		#[cfg(target_os = "linux")]
		window.set_visible(true);
		Ok(Self {
			window,
			gfx,
			text,
			rects,
			content: Content::About {
				lines,
				links,
				source: AboutSource::About(Box::new(adapter.clone())),
			},
			mouse: (0.0, 0.0),
			last_frame: std::time::Instant::now(),
			anim_wake: None,
			refused: Retry::default(),
			tip: crate::tip::Dwell::default(),
			tip_drawn: None,
			shaped: ShapedText::default(),
			parent,
			snapped: false,
			caps: (f32::MAX, f32::MAX),
			mods: ModifiersState::empty(),
		})
	}

	/// A message and an OK button, standing in for the system's message box on a
	/// platform that has none SilkTerm can count on.
	#[cfg(not(target_os = "windows"))]
	pub fn new_notice(
		el: &ActiveEventLoop,
		title: String,
		paras: &[String],
		parent: Option<RawWindowHandle>,
		warm: Option<&crate::gfx::DialogGpu>,
	) -> anyhow::Result<Self> {
		let (window, mut gfx, mut text, rects) =
			Self::make(el, title, 480.0, 200.0, false, parent, warm)?;
		let (lines, links, size) = layout_notice(&mut text, paras);
		let requested_size =
			winit::dpi::PhysicalSize::new(size.0.ceil() as u32, size.1.ceil() as u32);
		if let Some(applied) = crate::app::request_size(&window, requested_size) {
			gfx.resize(applied.width, applied.height);
		}
		#[cfg(target_os = "linux")]
		window.set_visible(true);
		Ok(Self {
			window,
			gfx,
			text,
			rects,
			content: Content::About {
				lines,
				links,
				source: AboutSource::Notice(paras.to_vec()),
			},
			mouse: (0.0, 0.0),
			last_frame: std::time::Instant::now(),
			anim_wake: None,
			refused: Retry::default(),
			tip: crate::tip::Dwell::default(),
			tip_drawn: None,
			shaped: ShapedText::default(),
			parent,
			snapped: false,
			caps: (f32::MAX, f32::MAX),
			mods: ModifiersState::empty(),
		})
	}

	/// `resume` is the view a recently closed Settings window was left on (see
	/// `App::settings_view`); None opens at the top of the first tab. `sized` is a
	/// size the user dragged it to earlier this session, which outlives the view
	/// and is never written anywhere.
	pub fn new_settings(
		el: &ActiveEventLoop,
		parent: Option<RawWindowHandle>,
		resume: Option<View>,
		sized: Option<(f32, f32)>,
		warm: Option<&crate::gfx::DialogGpu>,
		base: config::Settings,
	) -> anyhow::Result<Self> {
		// provisional window first: sizing needs a TextCtx to measure labels in
		// the real UI font (same pattern as About)
		let (window, mut gfx, mut text, rects) =
			Self::make(el, "Settings".into(), 560.0, 800.0, true, parent, warm)?;
		let scale = config::display_scale(window.scale_factor());
		let (label_w, btn_w, row_btn_w, value_w, tab_ws, label_ws) =
			crate::settings_ui::chrome_widths(&mut text, scale);
		// Cap the window to the part of the screen it can actually occupy - the
		// monitor minus the taskbar, minus the frame the WM puts round it - and to
		// ~1010 DIP tall. A tab that doesn't fit scrolls instead of pushing the
		// footer buttons off the bottom. SettingsDialog divides these by the scale
		// factor on the way in, so every figure here is physical.
		let (max_w, max_h) = Self::settings_caps(&window, parent, scale);
		// laid out at the origin
		let mut dialog = SettingsDialog::new(
			0.0,
			0.0,
			text.ui_line_h,
			label_w,
			btn_w,
			row_btn_w,
			value_w,
			tab_ws,
			label_ws,
			max_w,
			max_h,
			scale,
		);
		dialog.start_from(base);
		dialog.set_sees_hidden(!on_wayland(el));
		if let Some(view) = resume {
			dialog.restore(view);
		}
		let (min_w, min_h) = dialog.min_size();
		window.set_min_inner_size(Some(winit::dpi::PhysicalSize::new(
			min_w.ceil() as u32,
			min_h.ceil() as u32,
		)));
		// The size the user last dragged it to wins over the natural one, but it
		// still has to fit the screen this window came up on. It was kept in DIP,
		// so a reopen at another scale is the same apparent size.
		let (w, h) = match sized {
			Some((sw, sh)) => (
				(sw * scale).clamp(min_w, max_w.max(min_w)),
				(sh * scale).clamp(min_h, max_h.max(min_h)),
			),
			None => dialog.size(),
		};
		dialog.set_size(w, h);
		let requested_size = winit::dpi::PhysicalSize::new(w.ceil() as u32, h.ceil() as u32);
		if let Some(applied) = crate::app::request_size(&window, requested_size) {
			gfx.resize(applied.width, applied.height);
			dialog.set_size(applied.width as f32, applied.height as f32);
		}
		// mapped last, at the final size, with the transient hints already in place
		#[cfg(target_os = "linux")]
		window.set_visible(true);
		Ok(Self {
			window,
			gfx,
			text,
			rects,
			content: Content::Settings(dialog),
			mouse: (0.0, 0.0),
			last_frame: std::time::Instant::now(),
			anim_wake: None,
			refused: Retry::default(),
			tip: crate::tip::Dwell::default(),
			tip_drawn: None,
			shaped: ShapedText::default(),
			parent,
			snapped: false,
			caps: (max_w, max_h),
			mods: ModifiersState::empty(),
		})
	}

	/// (orig, edited, `use_system_font`) for the app to apply, if this is Settings.
	pub fn settings_values(&self) -> Option<(config::Settings, config::Settings, bool)> {
		match &self.content {
			Content::Settings(dialog) => Some((
				dialog.orig().clone(),
				dialog.edited().clone(),
				dialog.use_system_font(),
			)),
			Content::About { .. } => None,
		}
	}

	/// A background shell scan arrived while this dialog was open. Fold it into the
	/// settings BOTH copies hold: the edited one so the user sees what turned up,
	/// and the baseline so the fold does not read as an edit they made.
	pub fn fold_shells(&mut self, found: &[crate::shells::Found]) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.fold_shells(found);
		}
	}

	/// The tab + scroll this dialog is sitting on, for a later reopen to resume.
	pub fn settings_view(&self) -> Option<View> {
		match &self.content {
			Content::Settings(dialog) => Some(dialog.view()),
			Content::About { .. } => None,
		}
	}

	/// The size it is sitting at, in DIP. Kept for the rest of the session so a
	/// reopen comes back the size it was dragged to - and on a monitor at another
	/// scale, the same apparent size rather than the same count of pixels.
	pub fn settings_size(&self) -> Option<(f32, f32)> {
		match &self.content {
			Content::Settings(_) => {
				let size = self.window.inner_size();
				let scale = crate::settings_ui::sane_scale(config::display_scale(
					self.window.scale_factor(),
				));
				Some((size.width as f32 / scale, size.height as f32 / scale))
			}
			Content::About { .. } => None,
		}
	}

	/// After an Apply, reset the settings baseline to the applied values so a later
	/// Apply diffs against the live state (see `SettingsDialog::commit_baseline`).
	pub fn commit_baseline(&mut self) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.commit_baseline();
		}
	}

	/// Config keys the user hit "revert to default" on since the last Apply; the
	/// app comments them out in config.shcl (`config::revert_keys`).
	pub fn take_reverted(&mut self) -> Vec<&'static str> {
		match &mut self.content {
			Content::Settings(dialog) => dialog.take_reverted(),
			Content::About { .. } => Vec::new(),
		}
	}

	/// True when the move changed what the window shows, so it is owed a frame.
	pub fn set_cursor(&mut self, x: f32, y: f32) -> bool {
		let from = std::mem::replace(&mut self.mouse, (x, y));
		pointer_moved(
			&mut self.content,
			&mut self.text,
			&mut self.tip,
			from,
			(x, y),
		)
	}

	pub fn mouse_down(
		&mut self,
		clip: Option<&mut crate::clipboard::Clipboard>,
	) -> Option<DialogAction> {
		let (mx, my) = self.mouse;
		match &mut self.content {
			Content::About { links, .. } => links
				.iter()
				.find(|link| link.rect.contains(mx, my))
				.map(|link| {
					link.url
						.clone()
						.map_or(DialogAction::Close, DialogAction::OpenUrl)
				}),
			Content::Settings(dialog) => {
				let (w, h) = dialog.size();
				// ignore clicks outside the panel (would otherwise Cancel)
				if mx < 0.0 || my < 0.0 || mx > w || my > h {
					return None;
				}
				// disjoint field borrow: measure via the text context (click-to-caret)
				let attrs = ui_attrs();
				let text = &mut self.text;
				let mut measure = |s: &str| text.measure_ui_text(s, &attrs);
				let action = dialog.mouse_down(mx, my, &mut measure);
				if let Action::Edit(cmd) = action {
					edit_cmd(dialog, cmd, clip);
					return None;
				}
				map_action(action)
			}
		}
	}

	/// Right mouse button: pop the field context menu (Settings only). `paste_ok`
	/// grays the Paste item when the clipboard holds nothing.
	pub fn mouse_right(&mut self, paste_ok: bool) {
		let (mx, my) = self.mouse;
		if let Content::Settings(dialog) = &mut self.content {
			let attrs = ui_attrs();
			let text = &mut self.text;
			let mut measure = |s: &str| text.measure_ui_text(s, &attrs);
			dialog.mouse_right(mx, my, paste_ok, &mut measure);
		}
	}

	/// Shift held (from the dialog's own modifier tracking): Shift+F10 opens the
	/// field context menu like the Menu key.
	pub fn shift_held(&self) -> bool {
		match &self.content {
			Content::Settings(dialog) => dialog.shift(),
			Content::About { .. } => false,
		}
	}

	/// Keyboard Menu key: context menu at the caret of the active field edit.
	pub fn menu_key(&mut self, paste_ok: bool) {
		if let Content::Settings(dialog) = &mut self.content {
			let attrs = ui_attrs();
			let text = &mut self.text;
			let mut measure = |s: &str| text.measure_ui_text(s, &attrs);
			dialog.menu_key(paste_ok, &mut measure);
		}
	}

	pub fn mouse_up(&mut self) -> Option<DialogAction> {
		let (mx, my) = self.mouse;
		if let Content::Settings(dialog) = &mut self.content {
			map_action(dialog.mouse_up(mx, my))
		} else {
			None
		}
	}

	/// wheel scroll for an overflowing settings tab (positive dy = scroll up)
	pub fn wheel(&mut self, dx_px: f32, dy_px: f32) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.wheel(dx_px, dy_px);
		}
	}

	/// Modifier state (from `ModifiersChanged`), already read for the platform:
	/// Alt underlines button accelerators; Shift and the shortcut key steer
	/// Tab-key focus and tab switching.
	pub fn set_keys(&mut self, keys: crate::input::EditKeys) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.set_keys(keys);
		}
	}

	pub fn set_mods(&mut self, mods: ModifiersState) {
		self.mods = mods;
	}

	pub fn mods(&self) -> ModifiersState {
		self.mods
	}

	/// A press while a Keys row waits for its new chord, which takes every key
	/// until it has one. False when nothing is waiting.
	pub fn capture_key(&mut self, key: &winit::keyboard::Key) -> bool {
		match &mut self.content {
			Content::Settings(dialog) => dialog.capture_key(key, self.mods),
			Content::About { .. } => false,
		}
	}

	/// Copy or Paste from the macOS menu bar, which takes Command+C and
	/// Command+V before the dialog sees them.
	pub fn menu_edit(&mut self, cmd: EditCmd, clip: Option<&mut crate::clipboard::Clipboard>) {
		if let Content::Settings(dialog) = &mut self.content {
			edit_cmd(dialog, cmd, clip);
		}
	}

	/// The tab to the left or right, from the macOS Window menu's tab rows.
	pub fn switch_tab(&mut self, forward: bool) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.switch_tab(forward);
		}
	}

	/// Tab key: walk control focus (Ctrl = switch tabs, Shift = backwards).
	pub fn key_tab(&mut self) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.key_tab();
		}
	}
	/// Ctrl+PageUp / Ctrl+PageDown: cycle tabs (PageDown = next).
	pub fn key_page(&mut self, forward: bool) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.key_page(forward);
		}
	}
	/// Up / Down: walk control focus.
	pub fn focus_vertical(&mut self, forward: bool) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.key_vertical(forward);
		}
	}
	/// Left / Right: caret motion (editing) or adjust the focused slider/radio.
	pub fn key_horizontal(&mut self, dir: i32) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.key_horizontal(dir);
		}
	}
	/// Space: type into an active edit, activate a focused footer button, or
	/// activate the focused control.
	pub fn key_space(&mut self) -> Option<DialogAction> {
		match &mut self.content {
			Content::Settings(dialog) => map_action(dialog.key_space()),
			// a notice's one button has the focus
			Content::About { source, .. } => source.is_notice().then_some(DialogAction::Close),
		}
	}

	/// A character key: while Alt is held it's an accelerator (Cancel/Apply/OK);
	/// while the shortcut key (Ctrl, Command on a Mac) is held it's an edit
	/// shortcut (select-all/copy/cut/paste); otherwise it types into the focused
	/// field.
	pub fn key_char(
		&mut self,
		ch: char,
		clip: Option<&mut crate::clipboard::Clipboard>,
	) -> Option<DialogAction> {
		match &mut self.content {
			Content::Settings(dialog) if dialog.alt() => map_action(dialog.alt_key(ch)),
			Content::Settings(dialog) if dialog.ctrl() => {
				let cmd = match ch.to_ascii_lowercase() {
					'a' => EditCmd::SelectAll,
					'c' => EditCmd::Copy,
					'x' => EditCmd::Cut,
					'v' => EditCmd::Paste,
					_ => return None,
				};
				edit_cmd(dialog, cmd, clip);
				None
			}
			Content::Settings(dialog) => {
				dialog.char_input(ch);
				None
			}
			Content::About { .. } => None,
		}
	}

	pub fn backspace(&mut self) {
		if let Content::Settings(dialog) = &mut self.content {
			dialog.backspace();
		}
	}

	/// Home / End / Delete / Insert inside a focused settings field (Left/Right go
	/// through `key_horizontal` so they can double as slider/radio adjust when not
	/// editing). Shift+Delete = cut, Ctrl+Insert = copy, Shift+Insert = paste.
	pub fn edit_nav(
		&mut self,
		key: winit::keyboard::NamedKey,
		clip: Option<&mut crate::clipboard::Clipboard>,
	) {
		use winit::keyboard::NamedKey as N;
		if let Content::Settings(dialog) = &mut self.content {
			match key {
				N::Home => dialog.cursor_home(),
				N::End => dialog.cursor_end(),
				N::Delete if dialog.shift() => {
					if let (Some(clip), Some(text)) = (clip, dialog.selected_text()) {
						clip.set_clipboard(text);
						dialog.delete_selection();
					} else {
						dialog.delete_forward();
					}
				}
				N::Delete => dialog.delete_forward(),
				N::Insert if dialog.shift() => {
					if let Some(text) = clip.and_then(super::clipboard::Clipboard::get_clipboard) {
						dialog.insert_str(&text);
					}
				}
				N::Insert if dialog.ctrl() => {
					if let (Some(clip), Some(text)) = (clip, dialog.selected_text()) {
						clip.set_clipboard(text);
					}
				}
				_ => {}
			}
		}
	}

	pub fn key_escape(&mut self) -> Option<DialogAction> {
		match &mut self.content {
			Content::About { .. } => Some(DialogAction::Close),
			Content::Settings(dialog) => map_action(dialog.key_escape()),
		}
	}

	pub fn key_enter(
		&mut self,
		clip: Option<&mut crate::clipboard::Clipboard>,
	) -> Option<DialogAction> {
		match &mut self.content {
			Content::Settings(dialog) => {
				let action = dialog.key_enter();
				if let Action::Edit(cmd) = action {
					edit_cmd(dialog, cmd, clip);
					return None;
				}
				map_action(action)
			}
			Content::About { source, .. } => source.is_notice().then_some(DialogAction::Close),
		}
	}

	/// When the loop next owes this dialog a frame with no input: the next step
	/// of a field edit's animation (view scroll, caret ease, blink), or a resting
	/// pointer's tip coming due.
	pub fn wake_at(&self) -> Option<std::time::Instant> {
		let anim = self
			.anim_wake
			.map(|ms| self.last_frame + std::time::Duration::from_millis(ms));
		match (anim, self.tip.wake()) {
			(Some(step), Some(tip)) => Some(step.min(tip)),
			(step, tip) => step.or(tip),
		}
	}

	pub fn owes_frame(&self, now: std::time::Instant) -> bool {
		frame_due(self.wake_at(), &self.tip, self.tip_drawn, now)
	}

	pub fn resize(&mut self, w: u32, h: u32) {
		self.gfx.resize(w, h);
		if let Content::Settings(dialog) = &mut self.content {
			dialog.set_size(w as f32, h as f32);
			self.snap_to_natural(w as f32, h as f32);
		}
		self.window.request_redraw();
	}

	/// DPI/scale changed under an open dialog: dragged to a monitor at another
	/// scale, or the desktop's scaling moved. Only the boundary follows - the text
	/// context rasterizes at the new size and the chrome is measured again - while
	/// every value and unapplied edit stays put.
	pub fn set_scale(&mut self, scale_factor: f64) {
		let scale = config::display_scale(scale_factor);
		if (scale - self.text.scale).abs() < 1e-4 {
			return;
		}
		self.text = TextCtx::new(&self.gfx.device, &self.gfx.queue, self.gfx.format, scale);
		match &mut self.content {
			Content::About { .. } => self.rescale_about(),
			Content::Settings(_) => self.rescale_settings(scale),
		}
		self.window.request_redraw();
	}

	// About and the notice hold no layout of their own to adjust, so they are
	// laid out again from what they were built from and the window asked for the
	// size that comes back. Neither can be resized, so there is nothing else to
	// keep in step.
	fn rescale_about(&mut self) {
		let Content::About { source, .. } = &self.content else {
			return;
		};
		let (lines, links, size) = layout_source(&mut self.text, source);
		if let Content::About {
			lines: old_lines,
			links: old_links,
			..
		} = &mut self.content
		{
			*old_lines = lines;
			*old_links = links;
		}
		let want = winit::dpi::PhysicalSize::new(size.0.ceil() as u32, size.1.ceil() as u32);
		if let Some(applied) = crate::app::request_size(&self.window, want) {
			self.gfx.resize(applied.width, applied.height);
		}
	}

	fn rescale_settings(&mut self, scale: f32) {
		let (label_w, btn_w, row_btn_w, value_w, tab_ws, label_ws) =
			crate::settings_ui::chrome_widths(&mut self.text, scale);
		let (max_w, max_h) = Self::settings_caps(&self.window, self.parent, scale);
		self.caps = (max_w, max_h);
		let line_h = self.text.ui_line_h;
		let Content::Settings(dialog) = &mut self.content else {
			return;
		};
		dialog.rescale(
			line_h, label_w, btn_w, row_btn_w, value_w, tab_ws, label_ws, max_w, max_h, scale,
		);
		// the floor is physical, so it was wrong the moment the factor moved
		let (min_w, min_h) = dialog.min_size();
		self.window
			.set_min_inner_size(Some(winit::dpi::PhysicalSize::new(
				min_w.ceil() as u32,
				min_h.ceil() as u32,
			)));
		// Nothing guarantees a Resized. winit keeps the LOGICAL size, so an
		// ordinary window gets one with the new physical size - but a maximized or
		// tiled window keeps its physical size and sends nothing, and the layout
		// would then be drawn at the new scale inside the old window. Hand the
		// dialog what the window really measures, then ask for a size the screen
		// can still hold, since it holds fewer DIP at a higher scale.
		let now = self.window.inner_size();
		dialog.set_size(now.width as f32, now.height as f32);
		let (want_w, want_h) = size_within_caps((now.width, now.height), (max_w, max_h));
		let want = winit::dpi::PhysicalSize::new(want_w, want_h);
		// the natural size is a different number of pixels now, so let the snap
		// have another go at whatever size we end up with
		self.snapped = false;
		if want != now {
			// a platform that resizes synchronously answers here and sends no Resized
			if let Some(applied) = crate::app::request_size(&self.window, want) {
				self.resize(applied.width, applied.height);
			}
		}
	}

	// Magnetic snap: within a few DIP of the size the content wants, the window
	// settles exactly on it; drag further and it lets go. `snapped` is the guard
	// against a window manager that declines the request - without it, declining
	// once would mean asking again on every event.
	fn snap_to_natural(&mut self, w: f32, h: f32) {
		let Content::Settings(dialog) = &self.content else {
			return;
		};
		if self.window.is_maximized() {
			return;
		}
		let snap = DLG_SNAP * config::display_scale(self.window.scale_factor());
		let (nat_w, nat_h) = dialog.natural_size();
		// Only toward a size the screen can hold. Snapping back to one it cannot is
		// how the footer buttons end up behind the taskbar again.
		let want_w = if nat_w <= self.caps.0 {
			snap_to(w, nat_w, snap)
		} else {
			w
		};
		let want_h = if nat_h <= self.caps.1 {
			snap_to(h, nat_h, snap)
		} else {
			h
		};
		if (want_w - w).abs() < 0.5 && (want_h - h).abs() < 0.5 {
			self.snapped = false;
			return;
		}
		if self.snapped {
			return;
		}
		self.snapped = true;
		let size = winit::dpi::PhysicalSize::new(want_w as u32, want_h as u32);
		// A platform that resizes synchronously answers here and sends no Resized,
		// so the surface and the layout have to be told from this side.
		if let Some(applied) = crate::app::request_size(&self.window, size) {
			self.gfx.resize(applied.width, applied.height);
			if let Content::Settings(dialog) = &mut self.content {
				dialog.set_size(applied.width as f32, applied.height as f32);
			}
		}
	}

	// How big the Settings window may be: the part of the screen it can occupy,
	// less the frame the WM puts round it, and no taller than DLG_MAX_H.
	// The parent names the monitor. A dialog created hidden has never been placed,
	// so asking which monitor IT is on answers for wherever the origin happens to
	// be - the primary, not the screen the terminal is on.
	fn settings_caps(window: &Window, parent: Option<RawWindowHandle>, scale: f32) -> (f32, f32) {
		let work = parent
			.and_then(work_area_of)
			.or_else(|| work_area(window))
			.map(|(_, _, w, h)| (w as f32, h as f32));
		let monitor = window
			.current_monitor()
			.map(|m| (m.size().width as f32, m.size().height as f32));
		caps_from(usable_screen(work, monitor), decor_extra(window), scale)
	}

	pub fn render(&mut self) {
		// advance the field-edit animation (view scroll ease, caret ease, blink)
		// with real frame time before anything is laid out
		let now = std::time::Instant::now();
		let dt = (now - self.last_frame).as_secs_f32().min(0.1);
		self.last_frame = now;
		self.anim_wake = if let Content::Settings(dialog) = &mut self.content {
			let attrs = ui_attrs();
			let text = &mut self.text;
			dialog.animate(dt, &mut |s| text.measure_ui_text(s, &attrs))
		} else {
			None
		};
		let (w, h) = (self.gfx.config.width, self.gfx.config.height);
		let (scene, tip_drawn) = compose(
			&self.content,
			&mut self.text,
			&mut self.shaped,
			&mut self.tip,
			self.mouse,
			now,
			(w, h),
		);
		self.tip_drawn = tip_drawn;
		let Ok(frame) = self.gfx.begin_frame() else {
			// nothing else asks again (see the terminal's own refused frame)
			self.refused.missed(now, FRAME_RETRY_FIRST, FRAME_RETRY_MAX);
			return;
		};
		let view = self.gfx.frame_view(&frame);
		self.text.update_viewport(&self.gfx.queue, w, h);
		// The panel's own colors decide it here, not the terminal's.
		let cfg = config::settings();
		self.text.set_text_blend(
			&self.gfx.queue,
			crate::text::text_blend(cfg.dialog_fg, cfg.dialog_bg, cfg.text_dark_on_light),
		);

		let areas = text_areas(&scene.texts, &self.shaped.bufs, (w, h));
		if let Err(err) = self.text.prepare(&self.gfx.device, &self.gfx.queue, areas) {
			// same atlas-full recovery as the main window: trim so the next
			// frame re-prepares with room, instead of dropping the dialog text
			eprintln!(
				"{}: dialog text prepare failed; trimming atlas: {err:?}",
				config::APP_NAME
			);
			self.text.trim_atlas();
		}
		// open-dropdown popup text prepared into the overlay renderer (second pass)
		if scene.overlay_range.is_some() {
			let ov_areas = text_areas(&scene.overlay_texts, &self.shaped.bufs, (w, h));
			if let Err(err) = self
				.text
				.prepare_overlay(&self.gfx.device, &self.gfx.queue, ov_areas)
			{
				eprintln!(
					"{}: dialog overlay prepare failed; trimming atlas: {err:?}",
					config::APP_NAME
				);
				self.text.trim_atlas();
			}
		}
		let rect_inst = &scene.rects;
		if !rect_inst.is_empty() {
			self.rects
				.set_resolution(&self.gfx.queue, w as f32, h as f32);
			self.rects
				.upload(&self.gfx.device, &self.gfx.queue, rect_inst);
		}

		let bg = config::srgb_f32(scene.clear);
		let mut encoder = self
			.gfx
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("dialog"),
			});
		{
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("dialog pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color {
							r: bg[0] as f64,
							g: bg[1] as f64,
							b: bg[2] as f64,
							a: 1.0,
						}),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if !rect_inst.is_empty() {
				let (rect_split, rows_end) = (scene.rect_split, scene.rows_end);
				self.rects.draw(&mut pass, 0..rect_split as u32);
				// scrolled settings rows, clipped to the viewport
				if rows_end > rect_split {
					if let Some(vp) = scene.scissor_vp {
						let x = vp.x.max(0.0).min(w as f32) as u32;
						let y = vp.y.max(0.0).min(h as f32) as u32;
						let sw = vp.w.max(0.0).min(w as f32 - x as f32) as u32;
						let sh = vp.h.max(0.0).min(h as f32 - y as f32) as u32;
						if sw > 0 && sh > 0 {
							pass.set_scissor_rect(x, y, sw, sh);
							self.rects
								.draw(&mut pass, rect_split as u32..rows_end as u32);
							pass.set_scissor_rect(0, 0, w, h);
						}
					}
				}
			}
			let _ = self.text.render(&mut pass);
		}
		// second pass: the open dropdown popup on top (preserves the first pass)
		if let Some((start, end)) = scene.overlay_range {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("dialog overlay pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			self.rects.draw(&mut pass, start..end);
			let _ = self.text.render_overlay(&mut pass);
		}
		self.gfx.queue.submit(Some(encoder.finish()));
		if self.gfx.end_frame(frame).is_ok() {
			self.refused = Retry::default();
		} else {
			self.refused.missed(now, FRAME_RETRY_FIRST, FRAME_RETRY_MAX);
		}
		self.text.trim_atlas();
	}
}

// One piece of text placed for drawing, and which of the frame's shaped
// buffers it draws.
struct Placed {
	x: f32,
	y: f32,
	scale: f32,
	color: [u8; 3],
	clip: Option<Rect>,
	buf: usize,
}

// What one frame draws, worked out with no surface.
struct Scene {
	clear: [u8; 3],
	rects: Vec<RectInstance>,
	// rects before `rect_split` draw unclipped; Settings rows from there to
	// `rows_end` draw scissored to the scroll viewport
	rect_split: usize,
	rows_end: usize,
	scissor_vp: Option<Rect>,
	// an open dropdown, field menu or tip, drawn on top in a second pass
	overlay_range: Option<(u32, u32)>,
	texts: Vec<Placed>,
	overlay_texts: Vec<Placed>,
}

// Shaped dialog text, kept from one frame to the next. Shaping is most of what
// a frame costs, and a frame mostly shows what the last one did. A buffer is
// found again by everything shaping reads: the text, its attrs (font, weight,
// color) and its line box. Where it sits and what clips it are given when it
// is drawn, so neither needs a new shape. What a frame does not ask for is
// dropped at the next one.
#[derive(Default)]
struct ShapedText {
	// the text context these were shaped in; a font, size or scale change
	// builds a new one, and nothing shaped in the old one is any use
	generation: u64,
	// this frame's buffers, in the order first asked for
	bufs: Vec<glyphon::Buffer>,
	index: HashMap<ShapeKey, usize>,
	// last frame's, until this one asks for them
	spare: HashMap<ShapeKey, glyphon::Buffer>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeKey {
	text: String,
	attrs: Attrs<'static>,
	// the buffer's height, as bits, since an f32 has no Hash
	height: u32,
}

impl ShapedText {
	fn start_frame(&mut self, text: &TextCtx) {
		self.spare.clear();
		if self.generation != text.generation {
			self.generation = text.generation;
			self.bufs.clear();
			self.index.clear();
			return;
		}
		let mut last: Vec<Option<glyphon::Buffer>> = std::mem::take(&mut self.bufs)
			.into_iter()
			.map(Some)
			.collect();
		for (key, slot) in self.index.drain() {
			if let Some(buf) = last[slot].take() {
				self.spare.insert(key, buf);
			}
		}
	}

	// This frame's buffer for `line` in `attrs`, shaped only when the last frame
	// had none like it. A width change lays a kept one out again, which costs
	// far less than shaping it.
	fn buffer(
		&mut self,
		text: &mut TextCtx,
		line: &str,
		attrs: &Attrs<'static>,
		width: f32,
		height: f32,
	) -> usize {
		let key = ShapeKey {
			text: line.to_string(),
			attrs: attrs.clone(),
			height: height.to_bits(),
		};
		if let Some(&slot) = self.index.get(&key) {
			return slot;
		}
		let buf = match self.spare.remove(&key) {
			Some(mut kept) => {
				text.resize_buffer(&mut kept, width, height);
				kept
			}
			None => shape(text, line, attrs, width, height),
		};
		self.bufs.push(buf);
		self.index.insert(key, self.bufs.len() - 1);
		self.bufs.len() - 1
	}
}

fn shape(
	text: &mut TextCtx,
	line: &str,
	attrs: &Attrs,
	width: f32,
	height: f32,
) -> glyphon::Buffer {
	#[cfg(test)]
	SHAPED.with(|n| n.set(n.get() + 1));
	let mut buf = text.new_ui_buffer(width, height);
	buf.set_text(&mut text.font_system, line, attrs, Shaping::Advanced, None);
	buf.shape_until_scroll(&mut text.font_system, false);
	buf
}

// Buffers shaped on this thread, so a test can hold a frame to a number.
#[cfg(test)]
thread_local! {
	static SHAPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn text_areas<'a>(
	placed: &[Placed],
	bufs: &'a [glyphon::Buffer],
	(w, h): (u32, u32),
) -> Vec<TextArea<'a>> {
	placed
		.iter()
		.map(|p| {
			let bounds = match p.clip {
				Some(rect) => TextBounds {
					left: rect.x as i32,
					top: rect.y as i32,
					right: (rect.x + rect.w) as i32,
					bottom: (rect.y + rect.h) as i32,
				},
				None => TextBounds {
					left: 0,
					top: 0,
					right: w as i32,
					bottom: h as i32,
				},
			};
			TextArea {
				buffer: &bufs[p.buf],
				left: p.x,
				top: p.y,
				scale: p.scale,
				bounds,
				default_color: GColor::rgb(p.color[0], p.color[1], p.color[2]),
				custom_glyphs: &[],
			}
		})
		.collect()
}

// The tip under the pointer, and the rect it hangs from. A frame asks once.
fn tip_under(content: &Content, (mx, my): (f32, f32)) -> Option<(&str, Rect)> {
	match content {
		Content::About { links, .. } => links
			.iter()
			.find(|link| link.tooltip.is_some() && link.rect.contains(mx, my))
			.and_then(|link| link.tooltip.as_deref().map(|tip| (tip, link.rect))),
		Content::Settings(dialog) => dialog.hover_tip(mx, my),
	}
}

// A pointer move: true when what the window shows moved with it. Settings draws
// nothing from the pointer itself, only from what a move changes (a drag, the
// item lit in a dropdown or menu) and the tip. About lights a button under it.
fn pointer_moved(
	content: &mut Content,
	text: &mut TextCtx,
	tip: &mut crate::tip::Dwell<Rect>,
	from: (f32, f32),
	to: (f32, f32),
) -> bool {
	let changed = match content {
		Content::About { links, .. } => {
			let lit = |(x, y): (f32, f32)| {
				links
					.iter()
					.position(|link| link.button && link.rect.contains(x, y))
			};
			lit(from) != lit(to)
		}
		Content::Settings(dialog) => {
			let attrs = ui_attrs();
			dialog.mouse_move(to.0, to.1, &mut |s| text.measure_ui_text(s, &attrs))
		}
	};
	let over = tip_under(content, to).map(|(_, anchor)| anchor);
	let tip_moved = tip.point_at(over);
	changed || tip_moved
}

// Everything one frame draws, worked out with no surface, and the tip it drew.
// Flyover help waits for the pointer to rest, the same as the tab strip and the
// menus do. A dialog that answered the moment the pointer crossed a control
// would read as a different kind of tip. The wake the wait asks for is read
// through `wake_at`, since a move can start it between frames.
fn compose(
	content: &Content,
	text: &mut TextCtx,
	shaped: &mut ShapedText,
	dwell: &mut crate::tip::Dwell<Rect>,
	mouse: (f32, f32),
	now: std::time::Instant,
	size: (u32, u32),
) -> (Scene, Option<Rect>) {
	let found = tip_under(content, mouse);
	let (drawn, _) = tip_gate(dwell, found.map(|(_, anchor)| anchor), now);
	let tip = found.filter(|(_, anchor)| drawn == Some(*anchor));
	(scene(content, text, shaped, mouse, tip, size), drawn)
}

// Whether a dialog is owed a frame with no input. A tip that ripened since the
// last frame counts even once its wake has passed, since `Dwell::wake` stops
// answering when the time is up.
fn frame_due(
	wake: Option<std::time::Instant>,
	dwell: &crate::tip::Dwell<Rect>,
	drawn: Option<Rect>,
	now: std::time::Instant,
) -> bool {
	wake.is_some_and(|at| at <= now) || dwell.ripe() != drawn
}

// Everything one frame draws, with `tip` the flyover to draw, if one is up.
fn scene(
	content: &Content,
	text: &mut TextCtx,
	shaped: &mut ShapedText,
	(mx, my): (f32, f32),
	tip: Option<(&str, Rect)>,
	(w, h): (u32, u32),
) -> Scene {
	shaped.start_frame(text);
	let mut rects: Vec<RectInstance> = Vec::new();
	let mut texts: Vec<Placed> = Vec::new();
	let mut overlay_texts: Vec<Placed> = Vec::new();
	let mut overlay_range: Option<(u32, u32)> = None;
	let line_h = text.ui_line_h;
	let border_col = crate::settings_ui::dialog_border();
	match content {
		Content::About {
			lines,
			links,
			source,
		} => {
			// a notice's OK is the default button, outlined the way Settings
			// outlines its own
			let btn_border = if source.is_notice() {
				crate::settings_ui::dialog_btn_hl()
			} else {
				border_col
			};
			// filled boxes behind button-style links (the Support button),
			// brightened while hovered
			for link in links.iter().filter(|link| link.button) {
				let fill = if link.rect.contains(mx, my) {
					crate::settings_ui::dialog_btn_hl()
				} else {
					crate::settings_ui::dialog_btn()
				};
				let r = link.rect;
				let b = text.dip(ABOUT_BORDER);
				rects.push(quad(
					r.x - b,
					r.y - b,
					r.w + 2.0 * b,
					r.h + 2.0 * b,
					btn_border,
				));
				rects.push(quad(r.x, r.y, r.w, r.h, fill));
			}
			for line in lines {
				let mut attrs = ui_attrs();
				attrs.color_opt = Some(GColor::rgb(line.color[0], line.color[1], line.color[2]));
				if line.bold {
					attrs.weight = crate::text::ui_bold_weight();
				}
				let buf = shaped.buffer(text, &line.text, &attrs, w as f32, line_h);
				texts.push(Placed {
					x: line.x,
					y: line.y,
					scale: line.scale,
					color: line.color,
					clip: None,
					buf,
				});
			}
			// flyover: show the destination URL of the hovered link in a small
			// box under it (the Support label hides its URL; this reveals it).
			if let Some((tip, anchor)) = tip {
				let attrs = ui_attrs();
				let tip_w = text.measure_ui_text(tip, &attrs);
				let at =
					crate::tip::lay_out(anchor, 1, tip_w, line_h, (w as f32, h as f32), text.scale);
				let (b, f) = (at.border, at.fill);
				rects.push(quad(b.x, b.y, b.w, b.h, border_col));
				rects.push(quad(f.x, f.y, f.w, f.h, crate::settings_ui::dialog_btn()));
				let dim = crate::settings_ui::dialog_dim();
				let mut a = ui_attrs();
				a.color_opt = Some(GColor::rgb(dim[0], dim[1], dim[2]));
				let buf = shaped.buffer(text, tip, &a, w as f32, line_h);
				texts.push(Placed {
					x: at.text_x,
					y: at.text_y,
					scale: 1.0,
					color: dim,
					clip: None,
					buf,
				});
			}
			let rect_split = rects.len();
			Scene {
				clear: crate::settings_ui::dialog_bg(),
				rects,
				rect_split,
				rows_end: 0,
				scissor_vp: None,
				overlay_range,
				texts,
				overlay_texts,
			}
		}
		Content::Settings(dialog) => {
			let attrs = ui_attrs();
			let (fixed, rows) = dialog.rects(line_h, |s| text.measure_ui_text(s, &attrs));
			let rect_split = fixed.len();
			rects = fixed;
			rects.extend(rows);
			let items = dialog.texts(line_h, |s| text.measure_ui_text(s, &attrs));
			// The dialog centers its text by line box; this drops each buffer so
			// what reads as centered is the text itself.
			let ink_dy = text.ui_center_dy();
			let mut place = |text: &mut TextCtx, item: &crate::settings_ui::TextItem| {
				let mut attrs = ui_attrs();
				attrs.color_opt = Some(GColor::rgb(item.color[0], item.color[1], item.color[2]));
				if item.bold {
					attrs.weight = crate::text::ui_bold_weight();
				}
				let tall = item.scale.max(1.0);
				let buf = shaped.buffer(text, &item.text, &attrs, w as f32, line_h * tall);
				Placed {
					x: item.x,
					y: item.y + ink_dy * tall,
					scale: item.scale,
					color: item.color,
					clip: item.clip,
					buf,
				}
			};
			for item in &items {
				texts.push(place(text, item));
			}
			let rows_end = rects.len();
			// open dropdown popup / field context menu: rects appended after the
			// rows (drawn on top, unscissored, in a second pass); text goes to
			// the overlay renderer
			if dialog.overlay_open() {
				let (ov_rects, ov_texts) = dialog.overlay(&mut |s| text.measure_ui_text(s, &attrs));
				let start = rects.len() as u32;
				rects.extend(ov_rects);
				overlay_range = Some((start, rects.len() as u32));
				for item in &ov_texts {
					overlay_texts.push(place(text, item));
				}
			}
			// flyover: what a control does, or why it is grayed out. A small box
			// under it, drawn in the overlay pass so it can't bleed with the row
			// text (same as the About URL tip). It WRAPS - a sentence long enough
			// to outrun the panel would otherwise be clamped to the edge and run
			// off it, and the panel's width is not ours to grow.
			if let Some((tip, anchor)) = tip {
				let scale = text.scale;
				let avail = crate::tip::wrap_budget(w as f32, scale);
				let lines = crate::tip::wrap(tip, avail, |s| text.measure_ui_text(s, &attrs));
				let tip_w = lines
					.iter()
					.map(|l| text.measure_ui_text(l, &attrs))
					.fold(0.0f32, f32::max);
				let at = crate::tip::lay_out(
					anchor,
					lines.len(),
					tip_w,
					line_h,
					(w as f32, h as f32),
					scale,
				);
				let (b, f) = (at.border, at.fill);
				let start = overlay_range.map_or(rects.len() as u32, |(s, _)| s);
				rects.push(quad(b.x, b.y, b.w, b.h, border_col));
				rects.push(quad(f.x, f.y, f.w, f.h, crate::settings_ui::dialog_btn()));
				overlay_range = Some((start, rects.len() as u32));
				let dim = crate::settings_ui::dialog_dim();
				let mut a = ui_attrs();
				a.color_opt = Some(GColor::rgb(dim[0], dim[1], dim[2]));
				for (n, line) in lines.iter().enumerate() {
					let buf = shaped.buffer(text, line, &a, w as f32, line_h);
					overlay_texts.push(Placed {
						x: at.text_x,
						y: at.text_y + line_h * n as f32,
						scale: 1.0,
						color: dim,
						clip: None,
						buf,
					});
				}
			}
			Scene {
				clear: crate::settings_ui::dialog_bg(),
				rects,
				rect_split,
				rows_end,
				scissor_vp: Some(dialog.viewport_px()),
				overlay_range,
				texts,
				overlay_texts,
			}
		}
	}
}

// X11: make the dialog a proper transient modal of the terminal via WM hints -
// WM_TRANSIENT_FOR (winit has no API for it there; its parent_window means
// literal X reparenting), plus the EWMH dialog type and the MODAL / SKIP_TASKBAR
// states. That gives the standard Linux modal behavior (kept off the taskbar,
// stacked above and raised with its parent, retains focus) without any input
// tricks. Same throwaway-connection pattern as app::set_blur_behind. No-op off X11.
#[cfg(target_os = "linux")]
fn set_transient_for(window: &Window, parent: Option<&RawWindowHandle>) {
	use winit::raw_window_handle::HasWindowHandle;
	use x11rb::connection::Connection;
	use x11rb::protocol::xproto::{
		AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode,
	};
	use x11rb::wrapper::ConnectionExt as _;

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let xid = match handle.as_raw() {
		RawWindowHandle::Xlib(h) => h.window as u32,
		RawWindowHandle::Xcb(h) => h.window.get(),
		_ => return,
	};
	let Ok((conn, screen)) = x11rb::connect(None) else {
		return;
	};
	let root = conn.setup().roots[screen].root;

	let atom = |name: &[u8]| -> Option<u32> {
		Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
	};
	let (Some(wt), Some(wt_dialog), Some(state), Some(modal), Some(skip)) = (
		atom(b"_NET_WM_WINDOW_TYPE"),
		atom(b"_NET_WM_WINDOW_TYPE_DIALOG"),
		atom(b"_NET_WM_STATE"),
		atom(b"_NET_WM_STATE_MODAL"),
		atom(b"_NET_WM_STATE_SKIP_TASKBAR"),
	) else {
		return;
	};

	if let Some(parent_xid) = parent.and_then(|p| match p {
		RawWindowHandle::Xlib(h) => Some(h.window as u32),
		RawWindowHandle::Xcb(h) => Some(h.window.get()),
		_ => None,
	}) {
		let _ = conn.change_property32(
			PropMode::REPLACE,
			xid,
			AtomEnum::WM_TRANSIENT_FOR,
			AtomEnum::WINDOW,
			&[parent_xid],
		);
	}
	let _ = conn.change_property32(PropMode::REPLACE, xid, wt, AtomEnum::ATOM, &[wt_dialog]);
	let _ = conn.change_property32(
		PropMode::REPLACE,
		xid,
		state,
		AtomEnum::ATOM,
		&[modal, skip],
	);

	// the window is already mapped, so also request the states via the EWMH
	// client message (action ADD=1, source = application=1) for WMs that only
	// honor a state change that way rather than a bare property write.
	let add_state = |st: u32| {
		let ev = ClientMessageEvent::new(32, xid, state, [1, st, 0, 1, 0]);
		let _ = conn.send_event(
			false,
			root,
			EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
			ev,
		);
	};
	add_state(modal);
	add_state(skip);
	let _ = conn.flush();
}
#[cfg(not(target_os = "linux"))]
fn set_transient_for(_window: &Window, _parent: Option<&RawWindowHandle>) {}

// Keep the parent (terminal) window directly below `dialog` in the stack, via an
// EWMH _NET_RESTACK_WINDOW client message to the root window. xfwm4/GNOME raise a
// transient's parent with it automatically; Compiz does not, so when the dialog
// is re-activated after another window came forward, the terminal stays buried
// behind that window - this slots it back just beneath the dialog. The message
// goes to root (the only stacking path Compiz honors for a managed window: it
// reparents clients into decoration frames, so a direct XConfigureWindow on the
// client isn't redirected to the WM and does nothing). Focus is untouched.
#[cfg(target_os = "linux")]
fn restack_parent_below(dialog: &Window, parent: Option<&RawWindowHandle>, dbg: bool, kind: &str) {
	use winit::raw_window_handle::HasWindowHandle;
	use x11rb::connection::Connection;
	use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask};

	let xid = |h: &RawWindowHandle| -> Option<u32> {
		match h {
			RawWindowHandle::Xlib(x) => Some(x.window as u32),
			RawWindowHandle::Xcb(x) => Some(x.window.get()),
			_ => None,
		}
	};
	let Some(parent_xid) = parent.and_then(xid) else {
		return;
	};
	let Ok(handle) = dialog.window_handle() else {
		return;
	};
	let Some(dlg_xid) = xid(&handle.as_raw()) else {
		return;
	};
	let Ok((conn, screen)) = x11rb::connect(None) else {
		return;
	};
	let root = conn.setup().roots[screen].root;
	let atom = |name: &[u8]| -> Option<u32> {
		Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
	};
	let Some(restack) = atom(b"_NET_RESTACK_WINDOW") else {
		return;
	};
	let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
	// terminal below the dialog (source = application(1), sibling = dialog,
	// detail = Below(1)).
	let ev = ClientMessageEvent::new(32, parent_xid, restack, [1, dlg_xid, 1, 0, 0]);
	let _ = conn.send_event(false, root, mask, ev);
	let _ = conn.flush();

	if dbg {
		// read back where the WM actually put things (parent should end up right
		// below dialog). Prints "term below dialog" or the offending gap.
		let order = atom(b"_NET_CLIENT_LIST_STACKING")
			.and_then(|prop| {
				conn.get_property(false, root, prop, AtomEnum::WINDOW, 0, 1024)
					.ok()?
					.reply()
					.ok()
			})
			.map_or_else(Vec::new, |reply| {
				reply.value32().map(Iterator::collect).unwrap_or_default()
			});
		let pos = |w: u32| order.iter().position(|&x| x == w);
		let (tp, dp) = (pos(parent_xid), pos(dlg_xid));
		let ok = matches!((tp, dp), (Some(t), Some(d)) if t + 1 == d);
		eprintln!(
			"[modal] {kind}: restack term={parent_xid:#x} below dialog={dlg_xid:#x} -> \
			 term_pos={tp:?} dialog_pos={dp:?} adjacent={ok}"
		);
	}
}
#[cfg(not(target_os = "linux"))]
fn restack_parent_below(_d: &Window, _p: Option<&RawWindowHandle>, _dbg: bool, _kind: &str) {}

// Wayland tells a window neither that it is minimized nor that it is covered.
fn on_wayland(el: &ActiveEventLoop) -> bool {
	use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
	el.owned_display_handle()
		.display_handle()
		.is_ok_and(|handle| matches!(handle.as_raw(), RawDisplayHandle::Wayland(_)))
}

// Field context-menu command against the active edit; the clipboard glue lives
// here (settings_ui stays clipboard-free). The Ctrl+letter shortcuts, Command on
// a Mac, come here too.
fn edit_cmd(
	dialog: &mut SettingsDialog,
	cmd: EditCmd,
	clip: Option<&mut crate::clipboard::Clipboard>,
) {
	match cmd {
		EditCmd::Cut => {
			if let (Some(clip), Some(text)) = (clip, dialog.selected_text()) {
				clip.set_clipboard(text);
				dialog.delete_selection();
			}
		}
		EditCmd::Copy => {
			if let (Some(clip), Some(text)) = (clip, dialog.selected_text()) {
				clip.set_clipboard(text);
			}
		}
		EditCmd::Paste => {
			if let Some(text) = clip.and_then(super::clipboard::Clipboard::get_clipboard) {
				dialog.insert_str(&text);
			}
		}
		EditCmd::Delete => dialog.delete_selection(),
		EditCmd::SelectAll => dialog.select_all(),
	}
}

fn map_action(action: Action) -> Option<DialogAction> {
	match action {
		Action::Apply => Some(DialogAction::Apply),
		Action::Ok => Some(DialogAction::ApplyAndClose),
		Action::Cancel => Some(DialogAction::Close),
		// None, plus context-menu Edit cmds (handled by edit_cmd before mapping)
		Action::None | Action::Edit(_) => None,
	}
}

// Settings-window height caps, DIP (see config::dip).
const DLG_MAX_H: f32 = 1010.0;
const DLG_DECOR_HEADROOM: f32 = 38.0; // room left for the WM's own title bar
const DLG_SNAP: f32 = 12.0; // how close a resize gets before it settles on the natural size

// About-panel geometry, DIP (see config::dip).
const ABOUT_PAD: f32 = 20.0; // panel inset around the whole content column
const ABOUT_INDENT: f32 = 16.0; // indent of a detail line under its heading
const ABOUT_TIGHT_GAP: f32 = 2.0; // heading to its first detail line
const ABOUT_LOOSE_GAP: f32 = 4.0; // title to the version line
const ABOUT_BTN_PAD_X: f32 = 16.0;
const ABOUT_BTN_PAD_Y: f32 = 8.0;
const ABOUT_TIP_ROOM: f32 = 14.0; // headroom kept below the button for its flyover
const ABOUT_BORDER: f32 = 1.0; // 1px rule around the Support button
// The flyover box's own measurements are shared with the Settings dialog's, in
// tip.rs - both windows draw the same box.

// The size to ask the window for after a change of display scale: the pixels it
// has now, held to what the screen can still hold. A screen holds fewer DIP at a
// higher scale, so a window dragged from a wide monitor to a smaller one at a
// higher scale comes out taller than the screen with its footer buttons under
// the taskbar.
fn size_within_caps(now: (u32, u32), caps: (f32, f32)) -> (u32, u32) {
	let cap = |px: u32, cap: f32| {
		if cap.is_finite() && cap >= 1.0 {
			(px as f32).min(cap).round() as u32
		} else {
			px
		}
	};
	(cap(now.0, caps.0), cap(now.1, caps.1))
}

// Build the About content laid out at the window origin; returns
// (lines, clickable links, (width, height)) in physical px.
fn layout_about(
	text: &mut TextCtx,
	info: &wgpu::AdapterInfo,
) -> (Vec<Line>, Vec<AboutLink>, (f32, f32)) {
	let menu_fg = crate::settings_ui::dialog_text();
	let menu_dim = crate::settings_ui::dialog_dim();
	let menu_link = config::MENU_LINK;
	let accel = crate::gfx::acceleration(info.device_type);
	let repo_url = env!("CARGO_PKG_REPOSITORY").to_string();
	// Every measurement below is DIP, converted here rather than at a boundary -
	// the panel is small enough that one conversion per number is clearer than a
	// second coordinate space (settings_ui.rs is the one that earns that).
	let gap = text.dip(config::MENU_SEP_H);
	let indent = text.dip(ABOUT_INDENT);
	let tight = text.dip(ABOUT_TIGHT_GAP);
	let loose = text.dip(ABOUT_LOOSE_GAP);
	// which build this is, then which cross target it was compiled for
	let build = config::build_target();
	// read as the box opens, so it is what the session had reached then rather
	// than a figure that ticks while nobody is reading it
	let uptime = crate::tabtitle::elapsed(config::uptime().as_secs());
	#[rustfmt::skip]
	let content: Vec<(String, [u8; 3], f32, f32, bool, f32)> = vec![
		(format!("About {}", config::APP_NAME), menu_fg, 0.0, 0.0, true, 1.5),
		(format!("Version {}", env!("CARGO_PKG_VERSION")), menu_dim, 0.0, loose, false, 1.0),
		("Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]".into(), menu_dim, 0.0, 0.0, false, 1.0),
		(format!("License: {}", env!("CARGO_PKG_LICENSE")), menu_dim, 0.0, 0.0, false, 1.0),
		("Info".into(), menu_fg, 0.0, gap, true, 1.0),
		(format!("Build:  {}  {build}", config::BUILD_ID), menu_dim, indent, tight, false, 1.0),
		(format!("Renderer:  {}", info.name), menu_dim, indent, 0.0, false, 1.0),
		(format!("Backend:  {:?}", info.backend), menu_dim, indent, 0.0, false, 1.0),
		(format!("Acceleration:  {accel}"), menu_dim, indent, 0.0, false, 1.0),
		(format!("Uptime:  {uptime}"), menu_dim, indent, 0.0, false, 1.0),
		(repo_url.clone(), menu_link, 0.0, gap, false, 1.0),
		("Click a link to open it in your browser  ·  Esc to close".into(), menu_dim, 0.0, gap, false, 1.0),
	];

	let attrs = ui_attrs();
	let pad = text.dip(ABOUT_PAD);
	let line_h = text.ui_line_h;
	let mut content_w: f32 = 0.0;
	let mut widths = Vec::with_capacity(content.len());
	for (line_text, _, indent, _, _, scale) in &content {
		let width = indent + text.measure_ui_text(line_text, &attrs) * scale;
		widths.push(width);
		content_w = content_w.max(width);
	}

	// Support button: a filled box with a centered label; opens DONATE.md and
	// reveals that URL as a flyover on hover (config::DONATE_URL).
	let btn_label = "Support SilkTerm!";
	let (btn_pad_x, btn_pad_y) = (text.dip(ABOUT_BTN_PAD_X), text.dip(ABOUT_BTN_PAD_Y));
	let label_w = text.measure_ui_text(btn_label, &attrs);
	let btn_w = label_w + btn_pad_x * 2.0;
	let btn_h = line_h + btn_pad_y * 2.0;
	content_w = content_w.max(btn_w);
	// the button's hover flyover shows the full donate URL; size the window so it
	// isn't clipped
	content_w = content_w.max(text.measure_ui_text(config::DONATE_URL, &attrs));
	let box_w = content_w + pad * 2.0;

	let mut lines = Vec::with_capacity(content.len() + 1);
	let mut links = Vec::with_capacity(2);
	let mut y = pad;
	for (i, (line_text, color, indent, gap_before, bold, scale)) in content.into_iter().enumerate()
	{
		y += gap_before;
		let x = pad + indent;
		if color == menu_link {
			links.push(AboutLink {
				rect: Rect {
					x,
					y,
					w: widths[i],
					h: line_h,
				},
				url: Some(repo_url.clone()),
				tooltip: None,
				button: false,
			});
		}
		lines.push(Line {
			text: line_text,
			x,
			y,
			color,
			bold,
			scale,
		});
		y += line_h * scale;
	}

	// Support button below the text, centered in the content column
	y += gap * 1.5;
	let btn_x = pad + (content_w - btn_w) * 0.5;
	links.push(AboutLink {
		rect: Rect {
			x: btn_x,
			y,
			w: btn_w,
			h: btn_h,
		},
		url: Some(config::DONATE_URL.to_string()),
		tooltip: Some(config::DONATE_URL.to_string()),
		button: true,
	});
	lines.push(Line {
		text: btn_label.into(),
		x: btn_x + (btn_w - label_w) * 0.5,
		y: y + btn_pad_y,
		color: menu_fg,
		bold: true,
		scale: 1.0,
	});
	y += btn_h;

	// leave room below the button for the URL flyover to appear on hover
	let box_h = y + pad + line_h + text.dip(ABOUT_TIP_ROOM);
	(lines, links, (box_w, box_h))
}

fn layout_source(
	text: &mut TextCtx,
	source: &AboutSource,
) -> (Vec<Line>, Vec<AboutLink>, (f32, f32)) {
	match source {
		AboutSource::About(info) => layout_about(text, info),
		#[cfg(not(target_os = "windows"))]
		AboutSource::Notice(paras) => layout_notice(text, paras),
	}
}

// "Line 3", "Lines 3 and 40", or "A line" when shcl could not say which.
fn line_names(lines: &[usize], lost: usize) -> String {
	match lines {
		[] if lost == 1 => "A line".to_string(),
		[] => format!("{lost} lines"),
		[one] => format!("Line {one}"),
		many => {
			let shown: Vec<String> = many.iter().take(5).map(ToString::to_string).collect();
			match many.len() - shown.len() {
				0 => format!(
					"Lines {} and {}",
					shown[..shown.len() - 1].join(", "),
					shown[shown.len() - 1]
				),
				more => format!("Lines {} and {more} more", shown.join(", ")),
			}
		}
	}
}

/// A notice's window title and its paragraphs. The path is a paragraph of its
/// own, since it is the one part that cannot be wrapped at a space.
pub fn refusal_notice(refusal: &config::Refusal) -> (String, Vec<String>) {
	let which = line_names(&refusal.lines, refusal.lost);
	let many = refusal.lines.len() > 1 || (refusal.lines.is_empty() && refusal.lost > 1);
	let it = if many { "them" } else { "it" };
	(
		"Settings not saved".to_string(),
		vec![
			format!("{} cannot save its settings file.", config::APP_NAME),
			refusal.path.display().to_string(),
			format!("{which} cannot be read, and saving now would delete {it}."),
			"Until that is fixed, changes such as the window size, new shells and anything set in Settings are used now but not kept.".to_string(),
		],
	)
}

/// What a launch says when converting the settings file left settings behind:
/// how many, and the name the file as it was is kept under, in the same folder.
pub fn conversion_notice(loss: &config::ConversionLoss) -> (String, Vec<String>) {
	let (done, lost) = match (loss.how, loss.lost) {
		(config::Converted::Dropped(rewrite), _) => return dropped_notice(loss, rewrite),
		(config::Converted::InPlace, 1) => (
			"converted its settings file to a new format.",
			"One setting could not be converted and now does nothing.".to_string(),
		),
		(config::Converted::InPlace, n) => (
			"converted its settings file to a new format.",
			format!("{n} settings could not be converted and now do nothing."),
		),
		(config::Converted::Rewritten, 1) => (
			"could not convert its settings file to the new format, so it wrote a new one.",
			"One setting could not be copied to the new file.".to_string(),
		),
		(config::Converted::Rewritten, n) => (
			"could not convert its settings file to the new format, so it wrote a new one.",
			format!("{n} settings could not be copied to the new file."),
		),
	};
	let mut paras = vec![
		format!("{} {done}", config::APP_NAME),
		loss.path.display().to_string(),
		lost,
	];
	paras.extend(backup_para(loss));
	("Settings not converted".to_string(), paras)
}

fn backup_para(loss: &config::ConversionLoss) -> Option<String> {
	let name = loss.backup.as_ref().and_then(|b| b.file_name())?;
	Some(format!(
		"The file as it was before is kept in the same folder, as {}.",
		name.to_string_lossy()
	))
}

// A current settings file with lines that are not UTF-8 was written again
// without them. Some editors show such a line as if nothing were wrong, so the
// notice names each one, and the copy that still has them.
fn dropped_notice(
	loss: &config::ConversionLoss,
	rewrite: config::Rewrite,
) -> (String, Vec<String>) {
	let done = match rewrite {
		config::Rewrite::Kept => "wrote the file again without it",
		config::Rewrite::Template => {
			"wrote a new one from the defaults, with every setting it could still read"
		}
	};
	let which = line_names(&loss.lines, loss.lines.len());
	let were = if loss.lines.len() == 1 { "was" } else { "were" };
	let carried = match (rewrite, loss.lost) {
		(config::Rewrite::Kept, _) | (config::Rewrite::Template, 0) => String::new(),
		(config::Rewrite::Template, 1) => {
			" One setting could not be copied to the new file.".to_string()
		}
		(config::Rewrite::Template, n) => {
			format!(" {n} settings could not be copied to the new file.")
		}
	};
	let gone = format!("{which} {were} left out.{carried}");
	let mut paras = vec![
		format!(
			"{} found text that is not UTF-8 in its settings file, and {done}.",
			config::APP_NAME
		),
		loss.path.display().to_string(),
		gone,
	];
	paras.extend(backup_para(loss));
	("Settings file rewritten".to_string(), paras)
}

// Notice geometry, DIP (see config::dip). Windows draws its own message box.
#[cfg(not(target_os = "windows"))]
const NOTICE_WRAP: f32 = 440.0; // widest a paragraph runs before it wraps
#[cfg(not(target_os = "windows"))]
const NOTICE_PARA_GAP: f32 = 10.0;
#[cfg(not(target_os = "windows"))]
const NOTICE_BTN_MIN_W: f32 = 88.0;

// A notice laid out at the window origin: its paragraphs wrapped at word
// breaks, and an OK button at the bottom right. Physical px, like layout_about.
#[cfg(not(target_os = "windows"))]
fn layout_notice(text: &mut TextCtx, paras: &[String]) -> (Vec<Line>, Vec<AboutLink>, (f32, f32)) {
	let fg = crate::settings_ui::dialog_text();
	let dim = crate::settings_ui::dialog_dim();
	let attrs = ui_attrs();
	let pad = text.dip(ABOUT_PAD);
	let line_h = text.ui_line_h;
	let wrap = text.dip(NOTICE_WRAP);
	let para_gap = text.dip(NOTICE_PARA_GAP);

	let mut rows: Vec<(String, [u8; 3], f32)> = Vec::new();
	for (i, para) in paras.iter().enumerate() {
		// the path sits right under the sentence that introduces it
		let gap = match i {
			0 => 0.0,
			1 => text.dip(ABOUT_TIGHT_GAP),
			_ => para_gap,
		};
		let color = if i == 1 { dim } else { fg };
		// a path breaks after a separator, where the rest break at a space
		let (words, joiner): (Vec<&str>, &str) = if i == 1 {
			(para.split_inclusive(['/', '\\']).collect(), "")
		} else {
			(para.split(' ').collect(), " ")
		};
		let mut row = String::new();
		let mut first = true;
		for word in words {
			let tried = if row.is_empty() {
				word.to_string()
			} else {
				format!("{row}{joiner}{word}")
			};
			if !row.is_empty() && text.measure_ui_text(&tried, &attrs) > wrap {
				rows.push((
					std::mem::take(&mut row),
					color,
					if first { gap } else { 0.0 },
				));
				first = false;
				row = word.to_string();
			} else {
				row = tried;
			}
		}
		rows.push((row, color, if first { gap } else { 0.0 }));
	}

	let label = "OK";
	let (btn_pad_x, btn_pad_y) = (text.dip(ABOUT_BTN_PAD_X), text.dip(ABOUT_BTN_PAD_Y));
	let label_w = text.measure_ui_text(label, &attrs);
	let btn_w = (label_w + btn_pad_x * 2.0).max(text.dip(NOTICE_BTN_MIN_W));
	let btn_h = line_h + btn_pad_y * 2.0;
	let mut content_w = btn_w;
	for (row, _, _) in &rows {
		content_w = content_w.max(text.measure_ui_text(row, &attrs));
	}

	let mut lines = Vec::with_capacity(rows.len() + 1);
	let mut y = pad;
	for (row, color, gap) in rows {
		y += gap;
		lines.push(Line {
			text: row,
			x: pad,
			y,
			color,
			bold: false,
			scale: 1.0,
		});
		y += line_h;
	}
	// clear of the text above it, the way the Settings footer is
	y += text.dip(config::MENU_SEP_H) * 2.0;
	let btn_x = pad + content_w - btn_w;
	let links = vec![AboutLink {
		rect: Rect {
			x: btn_x,
			y,
			w: btn_w,
			h: btn_h,
		},
		url: None,
		tooltip: None,
		button: true,
	}];
	lines.push(Line {
		text: label.into(),
		x: btn_x + (btn_w - label_w) * 0.5,
		y: y + btn_pad_y,
		color: fg,
		bold: false,
		scale: 1.0,
	});
	y += btn_h;
	(lines, links, (content_w + pad * 2.0, y + pad))
}

/// The part of the screen a window can actually occupy: the monitor minus the
/// taskbar/panels/docks. winit has no API for it, so each platform is asked in
/// its own way and anything else falls back to the whole monitor. Physical
/// pixels, screen coordinates. Takes a handle rather than a window because the
/// window that WANTS the answer is often not the one to ask - see `settings_caps`.
#[cfg(target_os = "windows")]
pub fn work_area_of(handle: RawWindowHandle) -> Option<(i32, i32, i32, i32)> {
	use windows_sys::Win32::Graphics::Gdi::{
		GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
	};

	let RawWindowHandle::Win32(h) = handle else {
		return None;
	};
	// SAFETY: a dead hwnd only gets a null monitor, checked below. `info` is an
	// all-integer struct, zeroed with `cbSize` set, and outlives the call.
	unsafe {
		let monitor = MonitorFromWindow(h.hwnd.get() as *mut _, MONITOR_DEFAULTTONEAREST);
		if monitor.is_null() {
			return None;
		}
		let mut info: MONITORINFO = core::mem::zeroed();
		info.cbSize = core::mem::size_of::<MONITORINFO>() as u32;
		if GetMonitorInfoW(monitor, core::ptr::addr_of_mut!(info)) == 0 {
			return None;
		}
		let r = info.rcWork;
		Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
	}
}

/// X11 publishes it as _`NET_WORKAREA` on the root window - four CARDINALs per
/// virtual desktop, so the current desktop picks the entry. It covers the whole
/// virtual screen rather than one monitor, which is as much as the protocol
/// offers, so `usable_screen` treats it as a cap rather than an answer. Wayland
/// has no equivalent and returns None.
#[cfg(target_os = "linux")]
pub fn work_area_of(handle: RawWindowHandle) -> Option<(i32, i32, i32, i32)> {
	use x11rb::connection::Connection;
	use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

	match handle {
		RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {}
		_ => return None,
	}
	let (conn, screen) = x11rb::connect(None).ok()?;
	let root = conn.setup().roots.get(screen)?.root;
	let read = |name: &[u8], len: u32| -> Option<Vec<u32>> {
		let atom = conn.intern_atom(true, name).ok()?.reply().ok()?.atom;
		let reply = conn
			.get_property(false, root, atom, AtomEnum::CARDINAL, 0, len)
			.ok()?
			.reply()
			.ok()?;
		Some(reply.value32()?.collect())
	};
	// A window manager that publishes the work area but not the current desktop
	// still has a usable first entry; don't throw the answer away over it.
	let desktop = read(b"_NET_CURRENT_DESKTOP", 1)
		.and_then(|v| v.first().copied())
		.unwrap_or(0) as usize;
	let areas = read(b"_NET_WORKAREA", 256)?;
	let quad = areas
		.chunks_exact(4)
		.nth(desktop)
		.or_else(|| areas.chunks_exact(4).next())?;
	Some((
		quad[0] as i32,
		quad[1] as i32,
		quad[2] as i32,
		quad[3] as i32,
	))
}

/// macOS: the screen's visible frame, which leaves out the menu bar and the
/// Dock. Without it the cap was the whole display, and a dialog that tall was
/// pushed down from under the menu bar until its buttons left the screen.
#[cfg(target_os = "macos")]
pub fn work_area_of(handle: RawWindowHandle) -> Option<(i32, i32, i32, i32)> {
	use objc2::MainThreadMarker;
	use objc2_app_kit::{NSScreen, NSView};

	let RawWindowHandle::AppKit(h) = handle else {
		return None;
	};
	let mtm = MainThreadMarker::new()?;
	// SAFETY: an AppKit handle names the window's live NSView, and the marker
	// above proves this is the main thread, the only one AppKit answers on.
	let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
	let screen = view.window()?.screen()?;
	// Cocoa's origin is the bottom-left of the first screen, the one with the
	// menu bar - the same display winit measures from.
	let primary = NSScreen::screens(mtm).firstObject()?;
	let area = screen.visibleFrame();
	Some(flip_cocoa_rect(
		(area.origin.x, area.origin.y),
		(area.size.width, area.size.height),
		primary.frame().size.height,
		screen.backingScaleFactor(),
	))
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn work_area_of(_handle: RawWindowHandle) -> Option<(i32, i32, i32, i32)> {
	None
}

// Cocoa measures the screen in points from the bottom-left of the primary
// display, y up. winit measures in physical pixels from its top-left, y down.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn flip_cocoa_rect(
	origin: (f64, f64),
	size: (f64, f64),
	primary_h: f64,
	scale: f64,
) -> (i32, i32, i32, i32) {
	let top = primary_h - (origin.1 + size.1);
	(
		(origin.0 * scale).round() as i32,
		(top * scale).round() as i32,
		(size.0 * scale).round() as i32,
		(size.1 * scale).round() as i32,
	)
}

/// Where a dialog goes: centered over the terminal, then pulled back onto the
/// work area, or a tall dialog centered on a tall terminal puts its own buttons
/// under the taskbar or the Dock. With no work area to go by it only keeps off
/// the negative side of the origin.
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
pub fn dialog_origin(
	parent_pos: (i32, i32),
	parent_size: (u32, u32),
	dialog: (u32, u32),
	work: Option<(i32, i32, i32, i32)>,
) -> (i32, i32) {
	let (dlg_w, dlg_h) = (dialog.0 as i32, dialog.1 as i32);
	let x = parent_pos.0 + (parent_size.0 as i32 - dlg_w) / 2;
	let y = parent_pos.1 + (parent_size.1 as i32 - dlg_h) / 2;
	match work {
		Some((ax, ay, aw, ah)) => (
			x.clamp(ax, (ax + aw - dlg_w).max(ax)),
			y.clamp(ay, (ay + ah - dlg_h).max(ay)),
		),
		None => (x.max(0), y.max(0)),
	}
}

pub fn work_area(window: &Window) -> Option<(i32, i32, i32, i32)> {
	use winit::raw_window_handle::HasWindowHandle;
	work_area_of(window.window_handle().ok()?.as_raw())
}

// The screen a dialog may use. The work area is what a window can occupy once
// the taskbar has had its share, but on X11 it covers every monitor at once, so
// it is a cap rather than an answer: the smaller of the two is what one window
// gets. The monitor alone still counts whatever the taskbar has taken.
fn usable_screen(work: Option<(f32, f32)>, monitor: Option<(f32, f32)>) -> (f32, f32) {
	match (work, monitor) {
		(Some(work), Some(monitor)) => (work.0.min(monitor.0), work.1.min(monitor.1)),
		(Some(only), None) | (None, Some(only)) => only,
		(None, None) => (f32::MAX, f32::MAX),
	}
}

// The size caps, given what the screen leaves and what the frame costs. Split
// out from the window so the arithmetic can be tested without one.
fn caps_from(screen: (f32, f32), decor: (f32, f32), scale: f32) -> (f32, f32) {
	// The frame can only be measured once there is one, and X11 adds it at map
	// time - after the first measurement. Hence the DIP allowance to fall back on.
	let decor_h = if decor.1 > 0.0 {
		decor.1
	} else {
		DLG_DECOR_HEADROOM * scale
	};
	(
		(screen.0 - decor.0).max(1.0),
		(screen.1 - decor_h).min(DLG_MAX_H * scale).max(1.0),
	)
}

// Magnetic snap: a size within `snap` of the one the content wants settles
// exactly on it, and anything further away is left alone. A part pixel rounds
// up, since a window a fraction short of the content scrolls sideways by it.
fn snap_to(have: f32, want: f32, snap: f32) -> f32 {
	if (have - want).abs() <= snap {
		want.ceil()
	} else {
		have
	}
}

// What the window manager's own frame adds around the client area. Zero before
// the window is mapped (X11 has no frame yet), which is what the DIP fallback
// beside every caller is for.
fn decor_extra(window: &Window) -> (f32, f32) {
	let (outer, inner) = (window.outer_size(), window.inner_size());
	(
		(outer.width as f32 - inner.width as f32).max(0.0),
		(outer.height as f32 - inner.height as f32).max(0.0),
	)
}

// What a dialog draws a tip for this frame, and when the loop has to come back
// to raise one. Split out of `render` because the drawing needs a surface and
// the decision does not.
fn tip_gate(
	dwell: &mut crate::tip::Dwell<Rect>,
	over: Option<Rect>,
	now: std::time::Instant,
) -> (Option<Rect>, Option<u64>) {
	dwell.point_at(over);
	let wake = dwell
		.wake()
		.map(|due| due.saturating_duration_since(now).as_millis() as u64);
	(dwell.ripe(), wake)
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use super::{
		ABOUT_PAD, AboutSource, DLG_DECOR_HEADROOM, DLG_MAX_H, DLG_SNAP, Rect, caps_from,
		conversion_notice, dialog_origin, flip_cocoa_rect, layout_about, layout_source,
		refusal_notice, size_within_caps, snap_to, tip_gate, usable_screen,
	};
	use crate::config;
	use crate::text::{TextCtx, ui_attrs};

	// What a conversion that left settings behind says: which file, how many,
	// and the name of the copy beside it.
	// Test ID: Erf0Qkx
	#[test]
	fn a_conversion_notice_says_how_many_and_where_the_copy_is() {
		let said = |lost: usize, backup: Option<&str>| {
			conversion_notice(&config::ConversionLoss {
				path: std::path::PathBuf::from("/home/me/.config/silkterm/config.shcl"),
				backup: backup
					.map(|name| std::path::Path::new("/home/me/.config/silkterm").join(name)),
				lost,
				how: config::Converted::InPlace,
				lines: Vec::new(),
			})
		};
		let (title, paras) = said(2, Some("config_backup_20261003-142233_format-v2.shcl"));
		assert_eq!(title, "Settings not converted");
		assert_eq!(paras.len(), 4, "{paras:?}");
		assert_eq!(paras[1], "/home/me/.config/silkterm/config.shcl");
		assert_eq!(
			paras[2],
			"2 settings could not be converted and now do nothing."
		);
		assert_eq!(
			paras[3],
			"The file as it was before is kept in the same folder, as config_backup_20261003-142233_format-v2.shcl."
		);
		let (_, one) = said(1, None);
		assert_eq!(
			one[2],
			"One setting could not be converted and now does nothing."
		);
		assert_eq!(one.len(), 3, "no copy, nothing said about one");
	}

	// A file that could not be converted in place was written new, and its
	// notice says so, with what was left behind and the copy that still has it.
	// Test ID: ErgDpOZ
	#[test]
	fn a_rewritten_file_notice_says_it_was_written_new() {
		let said = |lost: usize| {
			conversion_notice(&config::ConversionLoss {
				path: std::path::PathBuf::from("/home/me/.config/silkterm/config.shcl"),
				backup: Some(std::path::PathBuf::from(
					"/home/me/.config/silkterm/config_backup_20261003-142233_format-v2.shcl",
				)),
				lost,
				how: config::Converted::Rewritten,
				lines: Vec::new(),
			})
		};
		let (title, paras) = said(3);
		assert_eq!(title, "Settings not converted");
		assert_eq!(
			paras[0],
			format!(
				"{} could not convert its settings file to the new format, so it wrote a new one.",
				config::APP_NAME
			)
		);
		assert_eq!(paras[2], "3 settings could not be copied to the new file.");
		assert!(paras[3].ends_with("config_backup_20261003-142233_format-v2.shcl."));
		assert_eq!(
			said(1).1[2],
			"One setting could not be copied to the new file."
		);
	}

	// What a refused save says: which file, which lines, and what that costs.
	// Test ID: EqGnMOw
	#[test]
	fn a_refused_save_names_the_file_and_the_lines() {
		let said = |lines: &[usize], lost: usize| {
			let refusal = crate::config::Refusal {
				path: std::path::PathBuf::from("/home/me/.config/silkterm/config.shcl"),
				lines: lines.to_vec(),
				lost,
			};
			refusal_notice(&refusal)
		};
		let (title, paras) = said(&[12], 1);
		assert_eq!(title, "Settings not saved");
		assert_eq!(paras[1], "/home/me/.config/silkterm/config.shcl");
		assert_eq!(
			paras[2],
			"Line 12 cannot be read, and saving now would delete it."
		);
		assert!(paras[3].contains("not kept"));
		assert_eq!(
			said(&[3, 40], 2).1[2],
			"Lines 3 and 40 cannot be read, and saving now would delete them."
		);
		assert_eq!(
			said(&[1, 2, 3, 4, 5, 6, 7], 7).1[2],
			"Lines 1, 2, 3, 4, 5 and 2 more cannot be read, and saving now would delete them."
		);
		assert_eq!(
			said(&[], 1).1[2],
			"A line cannot be read, and saving now would delete it."
		);
		assert_eq!(
			said(&[], 3).1[2],
			"3 lines cannot be read, and saving now would delete them."
		);
	}

	// Off since a current file that is not UTF-8 is written again without those
	// lines (2026100315581313), so no save is refused for them and `Refusal` no
	// longer says why. `a_notice_for_dropped_lines_names_them_and_the_copy` covers
	// the notice now.
	// // Lines that are not UTF-8 look fine in some editors, so the notice says
	// // what is wrong with them.
	// // Test ID: ErgK2Vb
	// #[test]
	// fn a_notice_for_lines_that_are_not_utf8_says_so() {
	// 	let said = |lines: &[usize]| {
	// 		refusal_notice(&crate::config::Refusal {
	// 			path: std::path::PathBuf::from("/home/me/.config/silkterm/config.shcl"),
	// 			lines: lines.to_vec(),
	// 			lost: lines.len(),
	// 			why: crate::config::Unreadable::NotUtf8,
	// 		})
	// 	};
	// 	let (title, paras) = said(&[3]);
	// 	assert_eq!(title, "Settings not saved");
	// 	assert_eq!(
	// 		paras[2],
	// 		"Line 3 cannot be read, since it is not UTF-8 text, and saving now would delete it."
	// 	);
	// 	assert_eq!(
	// 		said(&[3, 40]).1[2],
	// 		"Lines 3 and 40 cannot be read, since they are not UTF-8 text, and saving now would delete them."
	// 	);
	// }

	// A file written again without lines that are not UTF-8 says which lines
	// went and where the copy that has them is. One that had to start from the
	// template also says what it could not carry.
	// Test ID: ErgZJw3
	#[test]
	fn a_notice_for_dropped_lines_names_them_and_the_copy() {
		let said = |lines: &[usize], lost: usize, rewrite: config::Rewrite| {
			conversion_notice(&config::ConversionLoss {
				path: std::path::PathBuf::from("/home/me/.config/silkterm/config.shcl"),
				backup: Some(std::path::PathBuf::from(
					"/home/me/.config/silkterm/config_backup_20261003-142233_format-v3.shcl",
				)),
				lost,
				how: config::Converted::Dropped(rewrite),
				lines: lines.to_vec(),
			})
		};
		let (title, paras) = said(&[3], 1, config::Rewrite::Kept);
		assert_eq!(title, "Settings file rewritten");
		assert_eq!(
			paras,
			vec![
				format!(
					"{} found text that is not UTF-8 in its settings file, and wrote the file again without it.",
					config::APP_NAME
				),
				"/home/me/.config/silkterm/config.shcl".to_string(),
				"Line 3 was left out.".to_string(),
				"The file as it was before is kept in the same folder, as config_backup_20261003-142233_format-v3.shcl.".to_string(),
			]
		);
		let (_, paras) = said(&[3, 40], 2, config::Rewrite::Template);
		assert_eq!(
			paras[0],
			format!(
				"{} found text that is not UTF-8 in its settings file, and wrote a new one from the defaults, with every setting it could still read.",
				config::APP_NAME
			)
		);
		assert_eq!(
			paras[2],
			"Lines 3 and 40 were left out. 2 settings could not be copied to the new file."
		);
		assert_eq!(
			said(&[3], 0, config::Rewrite::Template).1[2],
			"Line 3 was left out."
		);
	}

	// A tip in a dialog waits for the pointer to rest, the way one in the tab
	// strip or a menu does. Drawing it needs a GPU, so what is pinned here is the
	// decision render asks for: whether there is a tip to draw, and when to come
	// back for one.
	// Test ID: Eq4Llx2
	#[test]
	fn a_dialog_tip_waits_for_the_pointer_to_rest() {
		let mut dwell = crate::tip::Dwell::default();
		let ok = Rect {
			x: 10.0,
			y: 20.0,
			w: 60.0,
			h: 24.0,
		};
		let (drawn, wake) = tip_gate(&mut dwell, Some(ok), Instant::now());
		assert_eq!(drawn, None, "the tip came up before the pointer had rested");
		assert!(wake.is_some(), "nothing would wake the loop to raise it");
		std::thread::sleep(crate::tip::DELAY);
		let (drawn, wake) = tip_gate(&mut dwell, Some(ok), Instant::now());
		assert_eq!(drawn, Some(ok), "a rested pointer got no tip");
		assert_eq!(wake, None, "a tip already up still asked for a wake-up");
		// crossing to another control starts the wait again
		let cancel = Rect {
			x: 80.0,
			y: 20.0,
			w: 60.0,
			h: 24.0,
		};
		assert_eq!(tip_gate(&mut dwell, Some(cancel), Instant::now()).0, None);
		// and leaving them all puts the tip away
		assert_eq!(tip_gate(&mut dwell, None, Instant::now()).0, None);
	}

	// The defect this was written for: a 1080p screen at 150% with a taskbar
	// leaves 1008 usable, and the dialog was being sized against the full 1080 -
	// so its footer buttons sat behind the taskbar with no way to reach OK.
	// Test ID: EpOQNMO
	#[test]
	fn the_height_cap_comes_off_the_usable_screen_not_the_monitor() {
		let scale = 1.5;
		let frame = 45.0; // a measured Windows title bar at 150%
		let screen = usable_screen(Some((1920.0, 1008.0)), Some((1920.0, 1080.0)));
		assert_eq!(screen, (1920.0, 1008.0), "the taskbar's share is not ours");
		let (_, usable) = caps_from(screen, (6.0, frame), scale);
		assert!(
			usable + frame <= 1008.0,
			"a window of {usable} plus its {frame} frame does not fit 1008"
		);
		// and the whole monitor would have been too tall by more than the frame
		let (_, whole) = caps_from((1920.0, 1080.0), (6.0, frame), scale);
		assert!(whole > usable);
	}

	// Before the window is mapped there is no frame to measure, so the allowance
	// stands in for one - and it is a DIP figure, so it grows with the scale.
	// Test ID: EpOQNMP
	#[test]
	fn an_unmapped_window_falls_back_to_the_dip_allowance() {
		for scale in [1.0, 1.5, 2.0] {
			let (_, h) = caps_from((1920.0, 1000.0), (0.0, 0.0), scale);
			assert_eq!(
				h,
				(1000.0 - DLG_DECOR_HEADROOM * scale).min(DLG_MAX_H * scale)
			);
		}
	}

	// Test ID: EpOQNMQ
	#[test]
	fn a_screen_that_answers_nothing_still_yields_a_usable_cap() {
		let (w, h) = caps_from(usable_screen(None, None), (0.0, 0.0), 1.0);
		assert!(w > 0.0 && h > 0.0 && h <= DLG_MAX_H);
		// no work area published: the monitor is all there is to go on
		assert_eq!(usable_screen(None, Some((800.0, 600.0))), (800.0, 600.0));
	}

	// The b26 case: a Retina screen that looks like 1440x900, a 25 point menu
	// bar and a Dock. Sized against the whole display, the dialog plus its title
	// bar was taller than what lies below the menu bar, so macOS pushed it down
	// until the buttons were off the bottom. Off the visible frame it fits, and
	// placed over a terminal near the top it still clears the menu bar.
	// Test ID: ErUj5Ic
	#[test]
	fn a_mac_dialog_fits_between_the_menu_bar_and_the_dock() {
		let scale = 2.0;
		let (menu_bar, dock) = (25.0, 70.0);
		let work = flip_cocoa_rect((0.0, dock), (1440.0, 900.0 - menu_bar - dock), 900.0, scale);
		assert_eq!(
			work,
			(0, 50, 2880, 1610),
			"below the menu bar, above the Dock"
		);
		let (ax, ay, aw, ah) = work;
		let title_bar = 28.0 * 2.0;
		let screen = usable_screen(Some((aw as f32, ah as f32)), Some((2880.0, 1800.0)));
		let (_, cap) = caps_from(screen, (0.0, title_bar), scale as f32);
		let outer = (cap + title_bar) as u32;
		assert!(outer <= ah as u32, "{outer} tall does not fit {ah}");
		let (x, y) = dialog_origin((200, 60), (1600, 1300), (1300, outer), Some(work));
		assert!(y >= ay && y + outer as i32 <= ay + ah, "y {y} runs off");
		assert!(x >= ax && x + 1300 <= ax + aw);
	}

	// A second monitor above or left of the primary has negative coordinates,
	// and a dialog on it stays there rather than jumping to the primary.
	// Test ID: ErUj5MS
	#[test]
	fn a_dialog_stays_on_a_monitor_left_of_the_primary() {
		let work = flip_cocoa_rect((-1920.0, 0.0), (1920.0, 1055.0), 1080.0, 1.0);
		assert_eq!(work, (-1920, 25, 1920, 1055));
		let (x, y) = dialog_origin((-1800, 100), (1600, 900), (800, 1000), Some(work));
		assert_eq!((x, y), (-1400, 50));
		// nothing known about the screen: only the negative side is refused
		assert_eq!(
			dialog_origin((-1800, -50), (1600, 900), (800, 1000), None),
			(0, 0)
		);
		assert_eq!(
			dialog_origin((100, 100), (1000, 800), (600, 400), None),
			(300, 300)
		);
	}

	// Test ID: EpOQNMR
	#[test]
	fn a_resize_settles_on_the_natural_size_and_lets_go_past_the_snap() {
		let want = 648.0;
		assert_eq!(snap_to(want - DLG_SNAP + 1.0, want, DLG_SNAP), want);
		assert_eq!(snap_to(want + DLG_SNAP - 1.0, want, DLG_SNAP), want);
		let far = want + DLG_SNAP + 1.0;
		assert_eq!(snap_to(far, want, DLG_SNAP), far);
		// already there: the snap is a no-op, which is what stops it looping
		assert_eq!(snap_to(want, want, DLG_SNAP), want);
		// content a fraction wider than a whole pixel gets the next pixel, or the
		// window is short of it and scrolls sideways by the fraction
		assert_eq!(snap_to(705.0, 706.4, DLG_SNAP), 707.0);
		assert_eq!(snap_to(707.0, 706.4, DLG_SNAP), 707.0);
	}

	// A scale change leaves the window at the pixels it already had, and the
	// screen holds fewer DIP at the higher scale. Nothing else pulls the window
	// back onto it.
	// Test ID: EqQh9oO
	#[test]
	fn a_window_is_capped_to_what_the_screen_can_fit_at_the_new_scale() {
		// inside the caps: left exactly as it is, no resize asked for
		assert_eq!(size_within_caps((800, 600), (1920.0, 1080.0)), (800, 600));
		// taller than the screen now holds: pulled back, width untouched
		assert_eq!(size_within_caps((800, 1400), (1920.0, 1080.0)), (800, 1080));
		// and both, dragged to a smaller monitor at a higher scale
		assert_eq!(size_within_caps((2000, 1400), (1280.0, 700.0)), (1280, 700));
		// About and the notice never measure a cap, so an unset one changes nothing
		assert_eq!(
			size_within_caps((800, 600), (f32::MAX, f32::MAX)),
			(800, 600)
		);
		// a screen that measured as nothing is not a reason to shrink to nothing
		assert_eq!(size_within_caps((800, 600), (0.0, 0.0)), (800, 600));
	}

	// A notice is whatever was laid out from a notice's paragraphs. Nothing else
	// says so, so the box's title, focus and OK all follow its source.
	// Test ID: ErstbFF
	#[test]
	fn only_a_notice_source_is_a_notice() {
		assert!(!AboutSource::About(Box::new(adapter())).is_notice());
		#[cfg(not(target_os = "windows"))]
		assert!(AboutSource::Notice(vec!["text".into()]).is_notice());
	}

	fn adapter() -> wgpu::AdapterInfo {
		crate::gfx::test_adapter("X", wgpu::DeviceType::Cpu)
	}

	// The Support button opens the donation page and shows its address as a
	// flyover, and the box is wide enough that the flyover is not cut off.
	// Test ID: Er2UJeZ
	#[test]
	fn the_about_box_has_a_support_button_with_room_for_its_address() {
		let mut text = TextCtx::new_cpu(1.0);
		let (lines, links, (box_w, _)) = layout_about(&mut text, &adapter());
		let buttons: Vec<_> = links.iter().filter(|l| l.button).collect();
		assert_eq!(buttons.len(), 1);
		assert_eq!(buttons[0].url.as_deref(), Some(config::DONATE_URL));
		assert_eq!(buttons[0].tooltip.as_deref(), Some(config::DONATE_URL));
		assert!(lines.iter().any(|l| l.text == "Support SilkTerm!"));
		let pad = text.dip(ABOUT_PAD);
		let tip = text.measure_ui_text(config::DONATE_URL, &ui_attrs());
		assert!(box_w >= tip + pad * 2.0, "{box_w} clips a {tip} flyover");
	}

	// Test ID: Er2UJea
	#[test]
	fn the_about_box_names_the_version_copyright_license_and_build() {
		let (lines, _, _) = layout_about(&mut TextCtx::new_cpu(1.0), &adapter());
		let said: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
		let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
		let build = format!("Build:  {}  {}", config::BUILD_ID, config::build_target());
		assert!(said.contains(&version.as_str()), "{said:?}");
		assert!(
			said.iter().any(|l| l.starts_with("Copyright © ")),
			"{said:?}"
		);
		assert!(said.contains(&"License: GPL-2.0-or-later"), "{said:?}");
		assert!(said.contains(&build.as_str()), "{said:?}");
		assert!(said.contains(&"Acceleration:  Software (CPU)"), "{said:?}");
	}

	// Test ID: Er2UJeb
	#[test]
	fn the_about_box_leads_with_a_bold_title_and_links_the_repository() {
		let (lines, links, _) = layout_about(&mut TextCtx::new_cpu(1.0), &adapter());
		assert_eq!(lines[0].text, format!("About {}", config::APP_NAME));
		assert!(lines[0].bold);
		assert!((lines[0].scale - 1.5).abs() < f32::EPSILON);
		let repo = env!("CARGO_PKG_REPOSITORY");
		let line = lines
			.iter()
			.find(|l| l.text == repo)
			.expect("a repository line");
		let link = links
			.iter()
			.find(|l| l.url.as_deref() == Some(repo))
			.expect("a repository link");
		assert!(!link.button);
		let r = &link.rect;
		assert!(r.w > 0.0 && r.h > 0.0);
		assert!(
			r.x <= line.x && line.x < r.x + r.w,
			"link {r:?}, line x {}",
			line.x
		);
		assert!(
			r.y <= line.y && line.y < r.y + r.h,
			"link {r:?}, line y {}",
			line.y
		);
	}

	// Both boxes are laid out again from their source when the display scale
	// changes, and what comes back has to be measured at the new scale.
	// Test ID: Er2UJec
	#[test]
	fn about_and_the_notice_lay_out_again_at_twice_the_size() {
		let sources = [
			AboutSource::About(Box::new(adapter())),
			#[cfg(not(target_os = "windows"))]
			AboutSource::Notice(
				refusal_notice(&config::Refusal {
					path: "/home/me/.config/silkterm/config.shcl".into(),
					lines: vec![12],
					lost: 1,
				})
				.1,
			),
			#[cfg(not(target_os = "windows"))]
			AboutSource::Notice(
				conversion_notice(&config::ConversionLoss {
					path: "/home/me/.config/silkterm/config.shcl".into(),
					backup: Some(
						"/home/me/.config/silkterm/config_backup_20261003-142233_format-v2.shcl"
							.into(),
					),
					lost: 2,
					how: config::Converted::InPlace,
					lines: Vec::new(),
				})
				.1,
			),
		];
		let (mut one, mut two) = (TextCtx::new_cpu(1.0), TextCtx::new_cpu(2.0));
		for source in &sources {
			let (_, _, (w1, h1)) = layout_source(&mut one, source);
			let (_, _, (w2, h2)) = layout_source(&mut two, source);
			for (at1, at2) in [(w1, w2), (h1, h2)] {
				let ratio = at2 / at1;
				assert!((1.85..2.15).contains(&ratio), "{at1} -> {at2}");
			}
		}
	}

	// The text one frame draws, as the next frame would compare it: what each
	// piece says, where, in what color and clip.
	type Drawn = Vec<(String, u32, u32, [u8; 3], Option<(u32, u32, u32, u32)>)>;
	fn drawn(scene: &super::Scene, shaped: &super::ShapedText) -> Drawn {
		scene
			.texts
			.iter()
			.chain(&scene.overlay_texts)
			.map(|p| {
				let said: String = shaped.bufs[p.buf]
					.lines
					.iter()
					.map(glyphon::BufferLine::text)
					.collect();
				let clip = p
					.clip
					.map(|r| (r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()));
				(said, p.x.to_bits(), p.y.to_bits(), p.color, clip)
			})
			.collect()
	}

	fn settings_content(text: &mut TextCtx) -> super::Content {
		let (label_w, btn_w, row_btn_w, value_w, tab_ws, label_ws) =
			crate::settings_ui::chrome_widths(text, 1.0);
		let mut dialog = crate::settings_ui::SettingsDialog::new(
			0.0,
			0.0,
			text.ui_line_h,
			label_w,
			btn_w,
			row_btn_w,
			value_w,
			tab_ws,
			label_ws,
			f32::MAX,
			700.0,
			1.0,
		);
		let (w, h) = dialog.size();
		dialog.set_size(w, h);
		super::Content::Settings(dialog)
	}

	fn shaped_so_far() -> usize {
		super::SHAPED.with(std::cell::Cell::get)
	}

	// One frame as render works it out, the pointer off the window.
	fn frame(
		content: &super::Content,
		text: &mut TextCtx,
		shaped: &mut super::ShapedText,
		size: (u32, u32),
	) -> super::Scene {
		let mut dwell = crate::tip::Dwell::default();
		super::compose(
			content,
			text,
			shaped,
			&mut dwell,
			(-1.0, -1.0),
			Instant::now(),
			size,
		)
		.0
	}

	fn switch_tab(content: &mut super::Content) {
		if let super::Content::Settings(dialog) = content {
			dialog.switch_tab(true);
		}
	}

	// A pointer move used to draw the dialog again, and every frame made and
	// shaped a new buffer for every piece of text on it. A frame that shows what
	// the last one did shapes nothing, on every tab.
	// Test ID: ErlkwlW
	#[test]
	fn a_dialog_frame_shapes_nothing_the_last_one_did() {
		let mut text = TextCtx::new_cpu(1.0);
		let mut content = settings_content(&mut text);
		let mut shaped = super::ShapedText::default();
		for tab in 0..crate::settings_ui::tab_titles().len() {
			let first = frame(&content, &mut text, &mut shaped, (800, 700));
			let first = drawn(&first, &shaped);
			assert!(!first.is_empty(), "tab {tab} drew no text");
			let before = shaped_so_far();
			let second = frame(&content, &mut text, &mut shaped, (800, 700));
			let shapes = shaped_so_far() - before;
			assert_eq!(
				shapes, 0,
				"tab {tab}: a frame with nothing changed shaped {shapes}"
			);
			assert_eq!(
				drawn(&second, &shaped),
				first,
				"tab {tab} drew something else"
			);
			switch_tab(&mut content);
		}
	}

	// What a frame shapes is exactly the text the last frame did not have, as
	// tabs, values, scrolling and the window's width change under it. A move or
	// a new clip is the same text somewhere else.
	// Test ID: Erlkwox
	#[test]
	fn a_dialog_frame_shapes_only_text_that_changed() {
		let mut text = TextCtx::new_cpu(1.0);
		let mut content = settings_content(&mut text);
		let mut shaped = super::ShapedText::default();
		let keys = |shaped: &super::ShapedText| {
			shaped
				.index
				.keys()
				.cloned()
				.collect::<std::collections::HashSet<_>>()
		};
		let _ = frame(&content, &mut text, &mut shaped, (800, 700));
		let mut seen = keys(&shaped);
		let (mut reshaped, mut kept) = (0, 0);
		for step in 0..4 * crate::settings_ui::tab_titles().len() {
			let mut width = 800;
			match step % 4 {
				0 => switch_tab(&mut content),
				1 => {
					let mut s = (*config::settings()).clone();
					s.margin += 3.0;
					s.font_size += 1.0;
					s.use_system_font_size = false;
					if let super::Content::Settings(dialog) = &mut content {
						dialog.start_from(s);
					}
				}
				2 => {
					if let super::Content::Settings(dialog) = &mut content {
						dialog.wheel(0.0, -120.0);
					}
				}
				_ => width = 640,
			}
			let before = shaped_so_far();
			let scene = frame(&content, &mut text, &mut shaped, (width, 700));
			let shapes = shaped_so_far() - before;
			let now = keys(&shaped);
			let new = now.difference(&seen).count();
			assert_eq!(
				shapes, new,
				"step {step} shaped {shapes} for {new} new pieces of text"
			);
			for p in scene.texts.iter().chain(&scene.overlay_texts) {
				assert_eq!(
					shaped.bufs[p.buf].size().0,
					Some(width as f32),
					"step {step}"
				);
			}
			if shapes > 0 {
				reshaped += 1;
			} else {
				kept += 1;
			}
			seen = now;
		}
		assert!(
			reshaped > 0 && kept > 0,
			"{reshaped} steps shaped and {kept} did not"
		);
	}

	// A kept buffer is found by everything shaping reads. A new color, weight,
	// font or line box shapes again, and so does a new text context, which is
	// what a font, size or scale change makes. A new width lays the kept shape
	// out again, the same as a fresh one.
	// Test ID: ErlkwsK
	#[test]
	fn a_kept_buffer_is_shaped_again_when_what_shaping_reads_changes() {
		let mut text = TextCtx::new_cpu(1.0);
		let mut shaped = super::ShapedText::default();
		let line_h = text.ui_line_h;
		let base = ui_attrs();
		let mut red = ui_attrs();
		red.color_opt = Some(glyphon::Color::rgb(200, 0, 0));
		let mut heavy = ui_attrs();
		heavy.weight = glyphon::Weight(if base.weight.0 == 700 { 400 } else { 700 });
		let mut serif = ui_attrs();
		serif.family = glyphon::Family::Serif;
		let asks = [
			("Label", &base, line_h),
			("Label", &red, line_h),
			("Label", &heavy, line_h),
			("Label", &serif, line_h),
			("Label", &base, line_h * 2.0),
			("Other", &base, line_h),
		];
		let ask_all = |shaped: &mut super::ShapedText, text: &mut TextCtx, width: f32| {
			shaped.start_frame(text);
			let before = shaped_so_far();
			let slots: Vec<usize> = asks
				.iter()
				.map(|(line, attrs, h)| shaped.buffer(text, line, attrs, width, *h))
				.collect();
			(slots, shaped_so_far() - before)
		};
		let (slots, shapes) = ask_all(&mut shaped, &mut text, 800.0);
		assert_eq!(
			shapes,
			asks.len(),
			"each ask differs in something shaping reads"
		);
		let unique: std::collections::HashSet<_> = slots.iter().collect();
		assert_eq!(unique.len(), asks.len());
		let (_, shapes) = ask_all(&mut shaped, &mut text, 800.0);
		assert_eq!(shapes, 0, "the same asks a frame later shaped again");
		// asked twice in one frame: one buffer serves both
		let again = shaped.buffer(&mut text, "Label", &base, 800.0, line_h);
		assert_eq!(
			again,
			shaped.buffer(&mut text, "Label", &base, 800.0, line_h)
		);
		// a narrower window keeps the shape and lays it out to the new width
		let (slots, shapes) = ask_all(&mut shaped, &mut text, 300.0);
		assert_eq!(shapes, 0, "a width change shaped again");
		let glyphs = |buf: &glyphon::Buffer| {
			buf.layout_runs()
				.flat_map(|run| run.glyphs.iter().map(|g| (g.glyph_id, g.x.to_bits())))
				.collect::<Vec<_>>()
		};
		for (slot, (line, attrs, h)) in slots.iter().zip(asks) {
			let kept = &shaped.bufs[*slot];
			assert_eq!(kept.size(), (Some(300.0), Some(h)));
			let fresh = super::shape(&mut text, line, attrs, 300.0, h);
			assert_eq!(
				glyphs(kept),
				glyphs(&fresh),
				"{line} laid out unlike a fresh shape"
			);
		}
		// a new context shapes everything again, and what a frame never asked for
		// is gone by the next
		let mut other = TextCtx::new_cpu(2.0);
		let (_, shapes) = ask_all(&mut shaped, &mut other, 800.0);
		assert_eq!(shapes, asks.len(), "a new text context reused old shapes");
		shaped.start_frame(&other);
		shaped.start_frame(&other);
		let before = shaped_so_far();
		let _ = shaped.buffer(&mut other, "Label", &base, 800.0, line_h);
		assert_eq!(
			shaped_so_far() - before,
			1,
			"a buffer outlived a frame that skipped it"
		);
	}

	// A frame used to look up the tip under the pointer twice, once to time it
	// and once to draw it.
	// Test ID: Erlkwvi
	#[test]
	fn a_dialog_frame_looks_up_the_tip_once() {
		let mut text = TextCtx::new_cpu(1.0);
		let content = settings_content(&mut text);
		let mut shaped = super::ShapedText::default();
		let mut dwell = crate::tip::Dwell::default();
		let super::Content::Settings(dialog) = &content else {
			unreachable!()
		};
		let (w, h) = dialog.size();
		let mut points = vec![(-1.0, -1.0)];
		for k in 1..40 {
			points.push((w * k as f32 / 40.0, h * k as f32 / 40.0));
		}
		for at in points {
			let before = crate::settings_ui::hover_tips();
			let _ = super::compose(
				&content,
				&mut text,
				&mut shaped,
				&mut dwell,
				at,
				Instant::now(),
				(800, 700),
			);
			let asks = crate::settings_ui::hover_tips() - before;
			assert_eq!(
				asks, 1,
				"a frame with the pointer at {at:?} looked the tip up {asks} times"
			);
		}
	}

	// A pointer move draws nothing unless something drawn moved with it: a
	// lit button, or a tip going away. A tip coming due is owed its frame by
	// the clock, not by the next move, and it is owed it even once the dwell's
	// wake has passed, since the wake stops answering then.
	// Test ID: Erlkwz5
	#[test]
	fn a_pointer_move_that_changes_nothing_drawn_needs_no_frame() {
		let mut text = TextCtx::new_cpu(1.0);
		let mut content = settings_content(&mut text);
		let mut dwell = crate::tip::Dwell::default();
		let (w, h) = match &content {
			super::Content::Settings(dialog) => dialog.size(),
			super::Content::About { .. } => unreachable!(),
		};
		let mut from = (0.0, 0.0);
		let (mut tipped, mut bare) = (None, None);
		for row in 0..30 {
			for col in 0..12 {
				let to = (w * (col as f32 + 0.5) / 12.0, h * (row as f32 + 0.5) / 30.0);
				assert!(
					!super::pointer_moved(&mut content, &mut text, &mut dwell, from, to),
					"a move to {to:?} asked for a frame"
				);
				match super::tip_under(&content, to) {
					Some((_, anchor)) => tipped = tipped.or(Some((to, anchor))),
					None => bare = bare.or(Some(to)),
				}
				from = to;
			}
		}
		let ((at, anchor), bare) = (tipped.unwrap(), bare.unwrap());
		let _ = super::pointer_moved(&mut content, &mut text, &mut dwell, from, at);
		let now = Instant::now();
		assert!(
			!super::frame_due(dwell.wake(), &dwell, None, now),
			"a tip still waiting asked for a frame"
		);
		std::thread::sleep(crate::tip::DELAY);
		assert!(
			super::frame_due(dwell.wake(), &dwell, None, Instant::now()),
			"a tip that came due got no frame"
		);
		let mut shaped = super::ShapedText::default();
		let (_, drawn) = super::compose(
			&content,
			&mut text,
			&mut shaped,
			&mut dwell,
			at,
			Instant::now(),
			(800, 700),
		);
		assert_eq!(drawn, Some(anchor));
		assert!(!super::frame_due(
			dwell.wake(),
			&dwell,
			drawn,
			Instant::now()
		));
		let inside = (at.0 + 1.0, at.1);
		assert_eq!(
			super::tip_under(&content, inside).map(|(_, r)| r),
			Some(anchor)
		);
		assert!(
			!super::pointer_moved(&mut content, &mut text, &mut dwell, at, inside),
			"a move inside the tip's control asked for a frame"
		);
		assert!(
			super::pointer_moved(&mut content, &mut text, &mut dwell, inside, bare),
			"leaving a tip up drew nothing"
		);
		// About lights its button under the pointer
		let (lines, links, _) = layout_about(&mut text, &adapter());
		let button = links.iter().find(|l| l.button).unwrap().rect;
		let mut about = super::Content::About {
			lines,
			links,
			source: AboutSource::About(Box::new(adapter())),
		};
		let mut dwell = crate::tip::Dwell::default();
		let on = (button.x + 2.0, button.y + 2.0);
		let off = (button.x - 5.0, button.y - 5.0);
		assert!(super::pointer_moved(
			&mut about, &mut text, &mut dwell, off, on
		));
		assert!(!super::pointer_moved(
			&mut about,
			&mut text,
			&mut dwell,
			on,
			(on.0 + 1.0, on.1)
		));
		assert!(super::pointer_moved(
			&mut about, &mut text, &mut dwell, on, off
		));
	}
}
