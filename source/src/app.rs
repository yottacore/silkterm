// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::index::Side;
use alacritty_terminal::selection::SelectionType;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use alacritty_terminal::term::{ClipboardType, TermMode};
use glyphon::{Buffer, Color as GColor, Shaping, TextArea, TextBounds};

use crate::bgimage::{ImageRenderer, WpProbe};
use crate::clipboard::Clipboard;
use crate::config;
use crate::gfx::{Gfx, Rebirth, RectInstance, RectRenderer, VramProbe};
use crate::input::{self, ClickSelect, CopyFrom, Hotkey, WheelRoute, is_copy_chord};
use crate::pane::{BarHit, CopyKind, Dir, Pane, PaneManager, Rect};
use crate::shells::ShellEntry;
use crate::term::{PaneId, UserEvent};
use crate::text::TextCtx;

// Delayed re-assertions of "terminal stays under the dialog" after the dialog is
// focused, to win the race against the WM's own activation restacking (Compiz).
// The window must outlast the WM's raise/focus animation (Compiz fade/zoom can
// keep re-stacking for a few hundred ms), so it spans ~1.2s - a too-short window
// let the animation re-bury the terminal after the last retry (About showed this;
// Settings happened to settle in time). Each retry is one cheap X message.
const RAISE_REASSERTS: u8 = 24;
const RAISE_REASSERT_IVL: Duration = Duration::from_millis(50);

// Reopening Settings this soon after closing it resumes the tab and scroll it
// was left on - long enough to cover "closed it, went to look at the result,
// came back", short enough that a later visit still starts from the top.
const SETTINGS_RESUME: Duration = Duration::from_mins(1);

pub struct App {
	proxy: EventLoopProxy<UserEvent>,
	state: Option<State>,
	cli: crate::cli::Cli,
	// pop-out dialog window (About/Settings), if open. Its own surface + text
	// context, so it can be larger than the main window.
	dialog: Option<crate::dialog::DialogWin>,
	dialog_dirty: bool,
	// A save that could not be written, waiting to be said, and the notice
	// saying one. It is its own window so it can stand over an open Settings.
	// Windows shows the system's message box instead, and `notice` stays None.
	notice: Option<crate::dialog::DialogWin>,
	notice_dirty: bool,
	notice_owed: Option<config::Refusal>,
	// files already reported this session (see notice_due)
	told: Vec<std::path::PathBuf>,
	// where the Settings dialog was when it last closed, and when that was
	settings_view: Option<(Instant, crate::settings_ui::View)>,
	// and the size it was dragged to, which outlives the view above and lasts
	// the whole session. Deliberately never written to the config.
	settings_size: Option<(f32, f32)>,
	// after the dialog is focused, re-assert "keep the terminal under me" a few
	// times: the WM's own activation (raising the dialog) can come just after our
	// first restack and re-bury the terminal, so a couple of delayed retries
	// settle it (see handle_dialog_event / about_to_wait).
	raise_reassert: u8,
	raise_next: Instant,
	// VT watcher spawned (once per process; GL path only)
	vt_watch: bool,
	// GPU context the pop-out dialogs draw on, warmed on a worker thread once the
	// terminal is on screen (see gfx::DialogGpu for why they can't share the
	// terminal's) and then kept, so no dialog open pays for it.
	gpu_warm: crate::gfx::GpuWarm,
	// cicd profiler stage: when SILK_PROFILE_OUT is set the app runs a workload
	// (via --shell) for SILK_PROFILE_SECS then exits, so main can dump a flamegraph.
	#[cfg(feature = "profiling")]
	profile_secs: u64,
	#[cfg(feature = "profiling")]
	profile_deadline: Option<std::time::Instant>,
}

impl App {
	pub fn new(proxy: EventLoopProxy<UserEvent>, cli: crate::cli::Cli) -> Self {
		Self {
			proxy,
			state: None,
			cli,
			dialog: None,
			dialog_dirty: false,
			notice: None,
			notice_dirty: false,
			notice_owed: None,
			told: Vec::new(),
			settings_view: None,
			settings_size: None,
			raise_reassert: 0,
			raise_next: Instant::now(),
			vt_watch: false,
			gpu_warm: crate::gfx::GpuWarm::idle(),
			#[cfg(feature = "profiling")]
			profile_secs: std::env::var("SILK_PROFILE_SECS")
				.ok()
				.and_then(|raw| raw.parse().ok())
				.unwrap_or(8),
			#[cfg(feature = "profiling")]
			profile_deadline: None,
		}
	}

	// Events for the pop-out dialog window (its own surface/input).
	fn handle_dialog_event(&mut self, event: WindowEvent) {
		use crate::dialog::DialogAction as DA;
		if env_flag("SILK_DLGDBG") {
			match &event {
				WindowEvent::KeyboardInput {
					event: k,
					is_synthetic,
					..
				} => {
					eprintln!(
						"[dlg] key {:?} {:?} synthetic={is_synthetic}",
						k.logical_key, k.state
					);
				}
				WindowEvent::Focused(f) => eprintln!("[dlg] focused {f}"),
				WindowEvent::MouseInput { state, button, .. } => {
					eprintln!("[dlg] mouse {button:?} {state:?}");
				}
				_ => {}
			}
		}
		let mut act: Option<DA> = None;
		match event {
			WindowEvent::CloseRequested => {
				self.close_dialog();
				return;
			}
			WindowEvent::Focused(true) => {
				// keep the terminal directly beneath us when we're activated, so
				// nothing stays wedged between the two (Compiz doesn't do this). Do
				// it now and arm delayed retries - the WM's own raise/animation of
				// the dialog can keep re-stacking for a while and re-bury the
				// terminal. We don't disarm on focus-out: the restack only positions
				// the terminal relative to us (never raises us), so retrying after
				// the user switched away can't pop the pair over another window -
				// and Compiz's animation briefly drops+restores focus, which would
				// otherwise kill the retries mid-flight.
				if let Some(d) = &self.dialog {
					d.raise_parent();
				}
				self.raise_reassert = RAISE_REASSERTS;
				self.raise_next = Instant::now() + RAISE_REASSERT_IVL;
			}
			WindowEvent::Resized(size) => {
				if let Some(d) = &mut self.dialog {
					d.resize(size.width, size.height);
				}
				self.dialog_dirty = true;
			}
			// Dragged to a monitor at another scale, or the desktop's scaling
			// changed. The dialog follows it in place.
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				if let Some(d) = &mut self.dialog {
					d.set_scale(scale_factor);
				}
				self.dialog_dirty = true;
			}
			WindowEvent::RedrawRequested => {
				if let Some(d) = &mut self.dialog {
					d.render();
				}
			}
			WindowEvent::CursorMoved { position, .. } => {
				if let Some(d) = &mut self.dialog {
					d.set_cursor(position.x as f32, position.y as f32);
					self.dialog_dirty = true; // slider drag feedback
				}
			}
			WindowEvent::MouseInput {
				state,
				button: MouseButton::Left,
				..
			} => {
				if let Some(d) = &mut self.dialog {
					match state {
						ElementState::Pressed => {
							// clipboard for the field context-menu commands
							let clip = self.state.as_mut().map(|s| &mut s.clipboard);
							act = d.mouse_down(clip);
						}
						ElementState::Released => act = d.mouse_up(),
					}
					self.dialog_dirty = true;
				}
			}
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Right,
				..
			} => {
				if let Some(d) = &mut self.dialog {
					// gray the menu's Paste when the clipboard holds nothing
					let paste_ok = self.state.as_mut().is_some_and(|s| {
						s.clipboard.get_clipboard().is_some_and(|t| !t.is_empty())
					});
					d.mouse_right(paste_ok);
					self.dialog_dirty = true;
				}
			}
			WindowEvent::KeyboardInput {
				event: key_event,
				is_synthetic,
				..
			} if key_is_typed(key_event.state, is_synthetic) => {
				let key_event = input::name_typed(key_event);
				if let Some(d) = &mut self.dialog {
					match &key_event.logical_key {
						Key::Named(NamedKey::Escape) => act = d.key_escape(),
						Key::Named(NamedKey::Enter) => {
							// clipboard for a context-menu item fired via Enter
							let clip = self.state.as_mut().map(|s| &mut s.clipboard);
							act = d.key_enter(clip);
						}
						Key::Named(NamedKey::ContextMenu) => {
							let paste_ok = self.state.as_mut().is_some_and(|s| {
								s.clipboard.get_clipboard().is_some_and(|t| !t.is_empty())
							});
							d.menu_key(paste_ok);
						}
						// Shift+F10: the other standard context-menu chord
						Key::Named(NamedKey::F10) if d.shift_held() => {
							let paste_ok = self.state.as_mut().is_some_and(|s| {
								s.clipboard.get_clipboard().is_some_and(|t| !t.is_empty())
							});
							d.menu_key(paste_ok);
						}
						Key::Named(NamedKey::Tab) => d.key_tab(),
						Key::Named(NamedKey::PageUp) => d.key_page(false),
						Key::Named(NamedKey::PageDown) => d.key_page(true),
						Key::Named(NamedKey::Backspace) => d.backspace(),
						Key::Named(NamedKey::Space) => act = d.key_space(),
						Key::Named(NamedKey::ArrowUp) => d.focus_vertical(false),
						Key::Named(NamedKey::ArrowDown) => d.focus_vertical(true),
						Key::Named(NamedKey::ArrowLeft) => d.key_horizontal(-1),
						Key::Named(NamedKey::ArrowRight) => d.key_horizontal(1),
						Key::Named(
							nav_key @ (NamedKey::Home
							| NamedKey::End
							| NamedKey::Delete
							| NamedKey::Insert),
						) => {
							let clip = self.state.as_mut().map(|s| &mut s.clipboard);
							d.edit_nav(*nav_key, clip);
						}
						Key::Character(typed) => {
							for c in typed.chars() {
								let clip = self.state.as_mut().map(|s| &mut s.clipboard);
								if let Some(action) = d.key_char(c, clip) {
									act = Some(action);
								}
							}
						}
						_ => {}
					}
					self.dialog_dirty = true;
				}
			}
			WindowEvent::ModifiersChanged(mods) => {
				if let Some(d) = &mut self.dialog {
					let mod_state = mods.state();
					d.set_mods(
						mod_state.alt_key(),
						mod_state.shift_key(),
						mod_state.control_key(),
					);
					self.dialog_dirty = true;
				}
			}
			WindowEvent::MouseWheel { delta, .. } => {
				if let Some(d) = &mut self.dialog {
					let (dx, dy) = match delta {
						MouseScrollDelta::LineDelta(x, y) => (x * 40.0, y * 40.0),
						MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
					};
					d.wheel(dx, dy);
					self.dialog_dirty = true;
				}
			}
			_ => {}
		}
		if let Some(action) = act {
			self.apply_dialog_action(action);
		}
	}

	// Windows: an owned popup gets no automatic placement (it appears at the
	// screen origin), so center a fresh dialog over the terminal window - then
	// pull it back onto the part of the screen a window can reach, or a tall
	// dialog centered on a tall terminal puts its own buttons under the taskbar.
	// Linux WMs place transients themselves.
	#[cfg(target_os = "windows")]
	fn center_dialog(&self) {
		let (Some(state), Some(dialog)) = (self.state.as_ref(), self.dialog.as_ref()) else {
			return;
		};
		if let Ok(pos) = state.window.outer_position() {
			let win = state.window.outer_size();
			let dlg = dialog.window.outer_size();
			let mut x = pos.x + (win.width as i32 - dlg.width as i32) / 2;
			let mut y = pos.y + (win.height as i32 - dlg.height as i32) / 2;
			// the terminal's monitor, not the dialog's: the dialog has not been
			// placed yet, so its own answer is for wherever the origin is
			let screen = {
				use winit::raw_window_handle::HasWindowHandle;
				state
					.window
					.window_handle()
					.ok()
					.map(|h| h.as_raw())
					.and_then(crate::dialog::work_area_of)
			};
			if let Some((ax, ay, aw, ah)) = screen {
				x = x.clamp(ax, (ax + aw - dlg.width as i32).max(ax));
				y = y.clamp(ay, (ay + ah - dlg.height as i32).max(ay));
			}
			dialog
				.window
				.set_outer_position(winit::dpi::PhysicalPosition::new(x.max(0), y.max(0)));
		}
	}
	// self kept for call-site parity with the Windows version above
	#[cfg(not(target_os = "windows"))]
	#[allow(clippy::unused_self)]
	fn center_dialog(&self) {}

	// Windows: the dialog is created hidden (see dialog::make), so after centering it
	// draw one frame at the final position and then show it - no origin flash, no jump.
	// Elsewhere the dialog is already mapped by new_about / new_settings.
	#[cfg(target_os = "windows")]
	fn reveal_dialog(&mut self) {
		if let Some(d) = self.dialog.as_mut() {
			d.render();
			d.window.set_visible(true);
		}
	}
	// self kept for call-site parity with the Windows version above
	#[cfg(not(target_os = "windows"))]
	#[allow(clippy::unused_self)]
	fn reveal_dialog(&self) {}

	// Drop the dialog window, remembering a Settings view on the way out so a
	// reopen within SETTINGS_RESUME picks up where it left off. Every close goes
	// through here - Cancel, OK, Esc and the window's own close button alike.
	fn close_dialog(&mut self) {
		if let Some(view) = self
			.dialog
			.as_ref()
			.and_then(super::dialog::DialogWin::settings_view)
		{
			self.settings_view = Some((Instant::now(), view));
			self.settings_size = self
				.dialog
				.as_ref()
				.and_then(super::dialog::DialogWin::settings_size);
		}
		self.dialog = None;
	}

	// Events for the notice window. It has one button, so everything that means
	// OK or close closes it.
	fn handle_notice_event(&mut self, event: WindowEvent) {
		let Some(n) = self.notice.as_mut() else {
			return;
		};
		let mut close = false;
		match event {
			WindowEvent::CloseRequested => close = true,
			WindowEvent::Resized(size) => {
				n.resize(size.width, size.height);
				self.notice_dirty = true;
			}
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				n.set_scale(scale_factor);
				self.notice_dirty = true;
			}
			WindowEvent::RedrawRequested => n.render(),
			WindowEvent::CursorMoved { position, .. } => {
				n.set_cursor(position.x as f32, position.y as f32);
				self.notice_dirty = true;
			}
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Left,
				..
			} => close = n.mouse_down(None).is_some(),
			WindowEvent::KeyboardInput {
				event: key_event,
				is_synthetic,
				..
			} if key_is_typed(key_event.state, is_synthetic) => {
				close = match &input::name_typed(key_event).logical_key {
					Key::Named(NamedKey::Escape) => n.key_escape().is_some(),
					Key::Named(NamedKey::Enter) => n.key_enter(None).is_some(),
					Key::Named(NamedKey::Space) => n.key_space().is_some(),
					_ => false,
				};
			}
			_ => {}
		}
		if close {
			self.notice = None;
		}
	}

	// While a notice is up, the windows under it take no input, and a click on
	// one brings the notice forward: the same rule a dialog holds the terminal to.
	fn notice_holds(&self, event: &WindowEvent) -> bool {
		let Some(n) = &self.notice else {
			return false;
		};
		match event {
			WindowEvent::KeyboardInput { .. }
			| WindowEvent::MouseWheel { .. }
			| WindowEvent::Ime(_) => true,
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				..
			} => {
				n.window.focus_window();
				true
			}
			_ => false,
		}
	}

	// Put an owed notice up, once nothing is saying one already. In front of
	// Settings when that is open, since an OK there is the usual way to meet it.
	fn show_notice(&mut self, event_loop: &ActiveEventLoop) {
		use winit::raw_window_handle::HasWindowHandle;
		#[cfg(target_os = "windows")]
		if NOTICE_UP.load(std::sync::atomic::Ordering::SeqCst) {
			return;
		}
		if self.notice.is_some() {
			return;
		}
		let Some(refusal) = self.notice_owed.take() else {
			return;
		};
		let (title, paras) = crate::dialog::refusal_notice(&refusal);
		let parent = self
			.dialog
			.as_ref()
			.map(|d| &d.window)
			.or(self.state.as_ref().map(|s| &s.window))
			.and_then(|w| w.window_handle().ok().map(|h| h.as_raw()));
		#[cfg(target_os = "windows")]
		{
			let _ = event_loop;
			let owner = match parent {
				Some(winit::raw_window_handle::RawWindowHandle::Win32(h)) => h.hwnd.get(),
				_ => 0,
			};
			// the path goes right under the sentence that introduces it
			let body = format!("{}\n{}\n\n{}", paras[0], paras[1], paras[2..].join("\n\n"));
			message_box(owner, &title, &body);
		}
		#[cfg(not(target_os = "windows"))]
		{
			let warm = self.gpu_warm.get();
			match crate::dialog::DialogWin::new_notice(
				event_loop,
				title,
				&paras,
				parent,
				warm.as_ref(),
			) {
				Ok(n) => {
					self.notice = Some(n);
					self.notice_dirty = true;
				}
				Err(e) => eprintln!("{}: notice window failed: {e}", config::APP_NAME),
			}
		}
	}

	fn apply_dialog_action(&mut self, action: crate::dialog::DialogAction) {
		use crate::dialog::DialogAction as DA;
		match action {
			DA::OpenUrl(u) => open_url(&u),
			DA::Close => self.close_dialog(),
			DA::Apply => {
				self.apply_dialog_settings();
			}
			DA::ApplyAndClose => {
				// Only close on OK if the save worked or cannot; if the file looked
				// open elsewhere the change applied live but wasn't written, so we
				// keep the dialog up to try again (the FYI went to stderr).
				if self.apply_dialog_settings() {
					self.close_dialog();
				}
			}
		}
	}

	// Pull the edited Settings from the dialog window and live-apply them to the
	// main window (config + persist + rebuild). The dialog has its own surface,
	// so it's unaffected.
	// Returns true when the change was written to disk, or when it never can be
	// until the file is fixed, which a notice says. False means the file looked
	// open elsewhere: applied live but not saved, and OK leaves the dialog open.
	fn apply_dialog_settings(&mut self) -> bool {
		let mut wrote = true;
		let mut refused = false;
		if let Some((orig, edited, sys)) = self
			.dialog
			.as_ref()
			.and_then(super::dialog::DialogWin::settings_values)
		{
			if let Some(state) = self.state.as_mut() {
				wrote = state.apply_settings_values(&orig, edited, sys);
			}
			if let Some(refusal) = config::take_refusal() {
				refused = true;
				if notice_due(&mut self.told, &refusal.path, true) {
					self.notice_owed = Some(refusal);
				}
			}
			// Reverted-to-default keys: after persist wrote the diffs, comment
			// them back out so the file returns to the template's default line.
			// Skip when the write was deferred (revert_keys would just no-op busy).
			if wrote {
				if let Some(reverted) = self
					.dialog
					.as_mut()
					.map(super::dialog::DialogWin::take_reverted)
				{
					config::revert_keys(&reverted);
				}
			}
			// The applied values are the new baseline, so a later Apply diffs against
			// the live state (without this, re-selecting the open-time value - e.g.
			// Bg fit back to Stretch - reads as "no change" and isn't re-applied).
			if let Some(d) = self.dialog.as_mut() {
				d.commit_baseline();
			}
			self.dialog_dirty = true;
		}
		wrote || refused
	}
}

#[derive(Clone, Copy)]
enum MenuAction {
	OpenLink,
	CopyLink,
	Copy,
	Paste,
	PasteSelection,
	ToggleReadOnly,
	ToggleCopySelect,
	ToggleCopyOutput,
	NewTab,
	// New tab running the shell at this index in the stored list (config
	// `shells.*`; see the Tabs menu's "New tab with shell").
	NewTabShell(usize),
	CloseTab,
	SplitVertical,
	SplitHorizontal,
	// Split running the shell at this index in the stored list, like NewTabShell.
	SplitShell(Dir, usize),
	Close,
	FontBigger,
	FontSmaller,
	FontReset,
	ToggleFullscreen,
	ToggleFrame,
	ToggleMenuBar,
	ToggleSingleTab,
	ToggleMinimap,
	ToggleBare,
	ToggleRemote,
	NextWallpaper,
	ReloadConfig,
	Settings,
	About,
	Quit,
}

impl MenuAction {
	// The flyover for a row that needs one. Most do not: "Copy" and "New tab"
	// say what they do, and a tip on every row would be noise the reader has to
	// learn to ignore. Empty means no tip.
	fn help(self) -> &'static str {
		match self {
			MenuAction::PasteSelection => {
				"Paste what was last highlighted with the mouse, without it having been copied first."
			}
			MenuAction::ToggleCopySelect => {
				"Send highlighted text straight to the clipboard, with no copy step. Per pane."
			}
			MenuAction::ToggleCopyOutput => {
				"Copy what a command printed, once the pane settles back at the prompt. Per pane."
			}
			MenuAction::ToggleReadOnly => {
				"Ignore anything typed at this pane, so a long job cannot be interrupted by accident."
			}
			MenuAction::ToggleFrame => {
				"The title bar and border. Turning it off takes the window manager's own buttons with it."
			}
			MenuAction::ToggleMenuBar => {
				"The menu bar. Right-clicking a pane still reaches the same items with it off."
			}
			MenuAction::ToggleSingleTab => {
				"The tab strip. Off keeps it hidden until there is a second tab."
			}
			MenuAction::ToggleMinimap => {
				"Show a miniature of the whole scroll buffer beside the text. It takes the room it uses."
			}
			MenuAction::ToggleBare => {
				"Drop the title bar, menu bar and tab strip together. Choosing it again puts back whatever was on."
			}
			MenuAction::ToggleRemote => {
				"Run as a plain terminal while the screen is somewhere else, since no effect survives the trip. Set for you when a remote session is noticed, and forgotten at the next launch."
			}
			MenuAction::NextWallpaper => {
				"Show the next picture from the wallpaper folder now. The rotation timer starts over from it."
			}
			MenuAction::ReloadConfig => {
				"Re-read the config file. Anything edited by hand since launch takes effect now."
			}
			MenuAction::NewTabShell(_) => "Open a tab running this shell instead of the usual one.",
			MenuAction::SplitShell(..) => "Split this pane and run this shell in the new half.",
			MenuAction::CopyLink => {
				"Put the link's address on the clipboard rather than opening it."
			}
			_ => "",
		}
	}
}

// One row of a menu: an action item (optionally a checkmark toggle) or a group
// separator. Separators render as a faint horizontal line, never hover/click.
// `accel` is the byte offset of the item's accelerator letter in the label
// (underlined; typing it picks the item); None = no accelerator - accelerators
// must be unique per menu, so low-priority items (and ones that already have a
// hotkey) go without.
#[derive(Clone)]
enum Entry {
	Item {
		label: String,
		action: MenuAction,
		check: Option<bool>,
		accel: Option<usize>,
	},
	// A row that opens a menu of its own to the right instead of doing
	// something. It carries its own items, so the popup can be built the moment
	// the pointer reaches the row.
	Sub {
		label: String,
		accel: Option<usize>,
		items: Vec<Entry>,
	},
	Sep,
}

// The text a row draws, if it draws any - a separator does not. Item and Sub
// rows are laid out and measured identically, so everything that walks a menu
// asks here rather than matching the two arms itself.
fn entry_label(entry: &Entry) -> Option<&str> {
	match entry {
		Entry::Item { label, .. } | Entry::Sub { label, .. } => Some(label),
		Entry::Sep => None,
	}
}

// The first accelerator letter two rows of one menu both claim, if any.
//
// Typing a letter picks the FIRST row carrying it, so a duplicate does not read
// as a duplicate - it silently makes the LATER row unreachable from the
// keyboard, which is why this is asserted where a menu is built rather than
// left to be noticed.
fn accel_clash(entries: &[Entry]) -> Option<char> {
	let mut seen: Vec<char> = Vec::new();
	for entry in entries {
		let Some((label, pos)) = entry_accel(entry) else {
			continue;
		};
		let Some(ch) = label[pos..].chars().next().map(|c| c.to_ascii_lowercase()) else {
			continue;
		};
		if seen.contains(&ch) {
			return Some(ch);
		}
		seen.push(ch);
	}
	None
}

// The label and the byte offset of its accelerator letter, for a row that has one.
fn entry_accel(entry: &Entry) -> Option<(&str, usize)> {
	match entry {
		Entry::Item {
			label,
			accel: Some(pos),
			..
		}
		| Entry::Sub {
			label,
			accel: Some(pos),
			..
		} => Some((label, *pos)),
		_ => None,
	}
}

// Byte offset of the accelerator letter: exact-case match first (so 'S' can
// pick "Selection" in "Paste Selection"), else case-insensitive.
fn accel_at(label: &str, ch: char) -> Option<usize> {
	label
		.find(ch)
		.or_else(|| label.to_ascii_lowercase().find(ch.to_ascii_lowercase()))
}

fn mi(label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: None,
		accel: None,
	}
}
fn mia(ch: char, label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: None,
		accel: accel_at(label, ch),
	}
}
// `ch` is optional because accelerators have to be unique WITHIN a menu, and a
// row that appears in two of them cannot always spell it the same way.
fn msub(ch: Option<char>, label: &str, items: Vec<Entry>) -> Entry {
	Entry::Sub {
		label: label.into(),
		accel: ch.and_then(|ch| accel_at(label, ch)),
		items,
	}
}
fn mt(on: bool, label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: Some(on),
		accel: None,
	}
}
fn mta(ch: char, on: bool, label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: Some(on),
		accel: accel_at(label, ch),
	}
}

// A "... with shell" row, or nothing at all while there is no shell to put
// under it - an empty flyout is worse than no row. The stored list supplies the
// titles and the order; only the active entries are offered, and the action
// carries the index into the WHOLE list so a disabled entry between two active
// ones cannot shift what a click runs.
fn shell_submenu(
	shells: &[ShellEntry],
	accel: Option<char>,
	label: &str,
	action: fn(usize) -> MenuAction,
) -> Vec<Entry> {
	let items: Vec<Entry> = shells
		.iter()
		.enumerate()
		.filter(|(_, shell)| shell.active)
		.map(|(i, shell)| mi(&shell.title, action(i)))
		.collect();
	if items.is_empty() {
		Vec::new()
	} else {
		vec![msub(accel, label, items)]
	}
}

// What the View menu needs to know to draw its checkmarks. Every field reads
// the same way: true means the thing is on, and its row is checked.
#[derive(Clone, Copy)]
struct ViewState {
	read_only: bool,
	fullscreen: bool,
	window_frame: bool,
	menu_bar: bool,
	tab_strip: bool,
	minimap: bool,
	bare: bool,
	remote: bool,
	// a rotation folder with something to move on to; the row is left out
	// otherwise, like the link rows on the right-click menu
	next_wallpaper: bool,
}

// The View menu, apart from the window it is asking about - so the labels, the
// order and the accelerators can be held to the style guide by test.
fn view_menu_items(on: ViewState) -> Vec<Entry> {
	let mut items = vec![
		mia(
			'I',
			"Increase font size (Ctrl+Plus)",
			MenuAction::FontBigger,
		),
		mia(
			'D',
			"Decrease font size (Ctrl+Minus)",
			MenuAction::FontSmaller,
		),
		mia('e', "Reset font size (Ctrl+0)", MenuAction::FontReset),
		Entry::Sep,
		mta('R', on.read_only, "Read-only", MenuAction::ToggleReadOnly),
		Entry::Sep,
		mta(
			'F',
			on.fullscreen,
			"Fullscreen (F11)",
			MenuAction::ToggleFullscreen,
		),
		// every toggle below names the thing itself and is checked while it is
		// showing, so the checkmarks all read one way down the column
		mta(
			'W',
			on.window_frame,
			"Window frame",
			MenuAction::ToggleFrame,
		),
		mta('M', on.menu_bar, "Menu bar", MenuAction::ToggleMenuBar),
		mta('T', on.tab_strip, "Tab strip", MenuAction::ToggleSingleTab),
		// 'M' and 'i' are both spoken for on this menu (Menu bar, Increase font
		// size), so the accelerator falls to the n
		mta('n', on.minimap, "Minimap", MenuAction::ToggleMinimap),
		mta('B', on.bare, "Bare window", MenuAction::ToggleBare),
		Entry::Sep,
		mta(
			'p',
			on.remote,
			"Temporary remote display mode",
			MenuAction::ToggleRemote,
		),
	];
	if on.next_wallpaper {
		items.extend([Entry::Sep, next_wallpaper_row()]);
	}
	items
}

// 'N' is New tab on the right-click menu and the n in Minimap on View, so both
// menus take the x
fn next_wallpaper_row() -> Entry {
	mia('x', "Next wallpaper", MenuAction::NextWallpaper)
}

// The three shell rows: a new tab, and a split either way.
fn new_tab_shells(shells: &[ShellEntry], accel: Option<char>) -> Vec<Entry> {
	shell_submenu(shells, accel, "New tab with shell", MenuAction::NewTabShell)
}
fn split_shells(shells: &[ShellEntry]) -> Vec<Entry> {
	let mut rows = shell_submenu(shells, None, "Split vertical with shell", |i| {
		MenuAction::SplitShell(Dir::Vertical, i)
	});
	rows.extend(shell_submenu(
		shells,
		None,
		"Split horizontal with shell",
		|i| MenuAction::SplitShell(Dir::Horizontal, i),
	));
	rows
}

// The menu-bar dropdowns other than View, and the right-click menu, each apart
// from the window it opens in, so the labels and accelerators can be held to
// the style guide by test the way View's are.
//
// No tab or pane action goes on File; each has a menu of its own.
fn file_menu_items() -> Vec<Entry> {
	vec![
		mia('R', "Reload config", MenuAction::ReloadConfig),
		mia('S', "Settings\u{2026} (Ctrl+,)", MenuAction::Settings),
		Entry::Sep,
		mia('Q', "Quit", MenuAction::Quit),
	]
}

fn edit_menu_items(copy_select: bool, copy_output: bool) -> Vec<Entry> {
	vec![
		mia('C', "Copy (Ctrl+Shift+C)", MenuAction::Copy),
		mia('P', "Paste (Ctrl+Shift+V)", MenuAction::Paste),
		mia('S', "Paste Selection", MenuAction::PasteSelection),
		Entry::Sep,
		mt(copy_select, "Copy on select", MenuAction::ToggleCopySelect),
		mt(copy_output, "Copy on output", MenuAction::ToggleCopyOutput),
	]
}

fn tabs_menu_items(shells: &[ShellEntry]) -> Vec<Entry> {
	let mut items = vec![mia('N', "New tab (Ctrl+Shift+T)", MenuAction::NewTab)];
	items.extend(new_tab_shells(shells, Some('S')));
	items.extend([
		Entry::Sep,
		mia('C', "Close tab (Ctrl+Shift+W)", MenuAction::CloseTab),
	]);
	items
}

fn panes_menu_items(shells: &[ShellEntry]) -> Vec<Entry> {
	let mut items = vec![
		mia('V', "Split vertical", MenuAction::SplitVertical),
		mia('H', "Split horizontal", MenuAction::SplitHorizontal),
	];
	items.extend(split_shells(shells));
	items.extend([Entry::Sep, mia('C', "Close pane", MenuAction::Close)]);
	items
}

fn help_menu_items() -> Vec<Entry> {
	vec![mia('A', "About\u{2026}", MenuAction::About)]
}

// What the right-click menu needs to know about the pane and window it opens
// over. The checkmark fields read as ViewState's do.
#[derive(Clone, Copy)]
struct CtxState {
	// a link under the click; its two rows are left out otherwise
	link: bool,
	read_only: bool,
	copy_select: bool,
	copy_output: bool,
	menu_bar: bool,
	next_wallpaper: bool,
}

fn context_menu_items(on: CtxState, shells: &[ShellEntry]) -> Vec<Entry> {
	let mut entries = Vec::new();
	// A link under the click gets its two items at the top, and only then -
	// they'd be dead weight on every other right-click.
	if on.link {
		entries.extend([
			mia('O', "Open link", MenuAction::OpenLink),
			mia('L', "Copy link", MenuAction::CopyLink),
			Entry::Sep,
		]);
	}
	// no accelerator on the shell rows: this menu already spends every letter
	// their labels offer - 'S' on "Paste Selection", 'H' on "Split
	// horizontal", 'N' on "New tab" - and a duplicate would make the older
	// item unreachable, since the first match wins
	entries.extend([
		mia('C', "Copy (Ctrl+Shift+C)", MenuAction::Copy),
		mia('P', "Paste (Ctrl+Shift+V)", MenuAction::Paste),
		mia('S', "Paste Selection", MenuAction::PasteSelection),
		Entry::Sep,
		mt(
			on.copy_select,
			"Copy on select",
			MenuAction::ToggleCopySelect,
		),
		mt(
			on.copy_output,
			"Copy on output",
			MenuAction::ToggleCopyOutput,
		),
		mta('R', on.read_only, "Read-only", MenuAction::ToggleReadOnly),
		Entry::Sep,
		mia('N', "New tab (Ctrl+Shift+T)", MenuAction::NewTab),
	]);
	entries.extend(new_tab_shells(shells, None));
	entries.extend([
		Entry::Sep,
		mia('V', "Split vertical", MenuAction::SplitVertical),
		mia('H', "Split horizontal", MenuAction::SplitHorizontal),
	]);
	entries.extend(split_shells(shells));
	entries.extend([
		Entry::Sep,
		mi("Close pane", MenuAction::Close),
		// The one window-chrome row worth repeating here: with the bar hidden
		// this menu is the only way back to it. The rest live on View.
		Entry::Sep,
		mta('M', on.menu_bar, "Menu bar", MenuAction::ToggleMenuBar),
	]);
	if on.next_wallpaper {
		entries.push(next_wallpaper_row());
	}
	entries.extend([
		Entry::Sep,
		mi("Reload config", MenuAction::ReloadConfig),
		mi("Settings\u{2026} (Ctrl+,)", MenuAction::Settings),
	]);
	entries
}

// The background shell scan came back (shells.rs). It reports what it FOUND;
// the fold into the stored list happens here, on the winit thread, against the
// list as it stands right now - so a scan cannot carry a snapshot that went
// stale while it ran. Nothing on screen changes (menus are built when they
// open), so this only has to put the list in the live settings and in the file.
// A scan that found nothing new compares equal and writes nothing at all; if the
// config looks open in another program the write is skipped and the list still
// applies for this session.
//
// A Settings dialog open at that moment is folded into as well, on BOTH of its
// copies - see `Dialog::fold_shells`.
fn fold_shells(found: &[crate::shells::Found]) {
	let orig = (*config::settings()).clone();
	let shells = crate::shells::merge(&orig.shells, found);
	if shells == orig.shells {
		return;
	}
	let mut new = orig.clone();
	new.shells = shells;
	let _ = config::persist(&orig, &new);
	config::update(new);
}

// argv for the stored shell at `index`. None when the list moved under an open
// menu, in which case the new tab falls back to the default shell rather than
// running something the user did not pick.
fn shell_argv(index: usize) -> Option<Vec<String>> {
	let command = config::settings().shells.get(index)?.command.clone();
	config::command_argv(&command)
}

// A popup's own DIP measurements at one scale factor: the padding above the
// first item and below the last, and the height of a separator row. Resolved
// once when the menu is built (see `popup`) so the draw and both hit tests read
// the same numbers without carrying a TextCtx into the geometry.
fn menu_metrics(scale: f32) -> (f32, f32) {
	(
		config::dip(config::MENU_ITEM_PAD_Y, scale),
		config::dip(config::MENU_SEP_H, scale),
	)
}

// What Close pane closes, in the cascade the menu and a dead shell both follow.
// `present` is whether the pane the menu was opened for is still there: a menu
// left standing after its pane went must not reach another tab's panes, which is
// how one could take the whole window down with live shells in it.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum CloseScope {
	Pane,
	Tab,
	Window,
	Nothing,
}

fn close_scope(present: bool, panes_in_tab: usize, tabs: usize) -> CloseScope {
	if !present {
		CloseScope::Nothing
	} else if panes_in_tab > 1 {
		CloseScope::Pane
	} else if tabs > 1 {
		CloseScope::Tab
	} else {
		CloseScope::Window
	}
}

// right-click context menu / menu-bar dropdown over a pane
struct ContextMenu {
	x: f32,
	y: f32,
	w: f32,
	item_h: f32,
	// this popup's `menu_metrics`, in physical px
	pad_y: f32,
	sep_h: f32,
	target: PaneId,
	entries: Vec<Entry>,
	hover: Option<usize>, // index into entries; never a separator
	// The submenu standing open off one of these rows, if any. It is placed
	// clear of this popup's right edge, so "the pointer is in the submenu" and
	// "the pointer is on a parent row" can never both be true.
	sub: Option<Box<ContextMenu>>,
}

impl ContextMenu {
	fn height(&self) -> f32 {
		let rows: f32 = self.entries.iter().map(|entry| self.entry_h(entry)).sum();
		rows + self.pad_y * 2.0
	}
	fn entry_h(&self, entry: &Entry) -> f32 {
		match entry {
			Entry::Sep => self.sep_h,
			_ => self.item_h,
		}
	}
	// This popup and every submenu standing open off it, outermost first.
	fn chain(&self) -> Vec<&ContextMenu> {
		let mut out = vec![self];
		let mut at = self;
		while let Some(sub) = &at.sub {
			out.push(sub);
			at = sub;
		}
		out
	}
	// The popup the keyboard and the pointer are on: the innermost open one.
	fn inner_mut(&mut self) -> &mut ContextMenu {
		match self.sub {
			Some(_) => self.sub.as_mut().expect("just matched").inner_mut(),
			None => self,
		}
	}
	// Anywhere on this popup or a submenu of it.
	fn hit_any(&self, mx: f32, my: f32) -> bool {
		self.chain().iter().any(|popup| popup.hit(mx, my))
	}
	fn row_top(&self, i: usize) -> f32 {
		self.y
			+ self.pad_y
			+ self.entries[..i]
				.iter()
				.map(|entry| self.entry_h(entry))
				.sum::<f32>()
	}
	// Anywhere on the popup, separators and padding included - a click that falls
	// on the menu belongs to the menu, whatever chrome it happens to cover.
	fn hit(&self, mx: f32, my: f32) -> bool {
		mx >= self.x && mx < self.x + self.w && my >= self.y && my < self.y + self.height()
	}
	fn item_at(&self, mx: f32, my: f32) -> Option<usize> {
		if mx < self.x || mx >= self.x + self.w {
			return None;
		}
		let mut y = self.y + self.pad_y;
		for (i, entry) in self.entries.iter().enumerate() {
			let h = self.entry_h(entry);
			if my >= y && my < y + h {
				return (!matches!(entry, Entry::Sep)).then_some(i);
			}
			y += h;
		}
		None
	}
	// Next selectable item from `from` in direction `dir` (+1 down / -1 up),
	// wrapping and skipping separators. None only if there are no items.
	fn step(&self, from: Option<usize>, dir: i32) -> Option<usize> {
		let n = self.entries.len() as i32;
		if n == 0 {
			return None;
		}
		let mut i = from.map_or(if dir > 0 { -1 } else { 0 }, |i| i as i32);
		for _ in 0..n {
			i = (i + dir).rem_euclid(n);
			if !matches!(self.entries[i as usize], Entry::Sep) {
				return Some(i as usize);
			}
		}
		None
	}
}

// Shaped chrome text, kept frame to frame: menu-bar titles + the copybox label,
// the tab close-"x", and per-tab title buffers. Re-shaping these every rendered
// frame was constant background work during any animation (even the idle cursor
// pulse). Rebuilt when the menu color changes; a tab entry re-shapes only when
// its title or the tab width changes; the whole cache is dropped on a
// text-context rebuild (buffers are tied to the FontSystem they were made with).
struct ChromeCache {
	menu_fg: [u8; 3],
	menubar: Vec<Buffer>, // MENU_BAR titles + trailing "Copy output" label
	// per shown tab: the title, the width it was shaped for, and the buffer
	tabs: Vec<(String, f32, Buffer)>,
}

// The tab strip as drawn: which tab it starts at, and per tab shown, how wide
// it is and what it says. Tabs are no longer one width apiece, so a position on
// the bar is a running total rather than a multiplication (see tabtitle).
#[derive(Default)]
struct TabLayout {
	key: (u32, usize, usize, usize, u32),
	first: usize,
	widths: Vec<f32>,
	labels: Vec<String>,
}

impl TabLayout {
	fn shown(&self) -> usize {
		self.widths.len()
	}

	fn x(&self, i: usize) -> Option<f32> {
		(i >= self.first && i < self.first + self.shown())
			.then(|| crate::tabtitle::slot_x(&self.widths, i - self.first))
	}

	fn w(&self, i: usize) -> Option<f32> {
		self.widths.get(i.checked_sub(self.first)?).copied()
	}

	fn at_x(&self, x: f32) -> Option<usize> {
		crate::tabtitle::slot_at_x(&self.widths, x).map(|slot| self.first + slot)
	}
}

// The menu bar's right-side copy-mode cluster: "Copy on [ ] select [ ] output".
// Drawing, label placement, and click hit-testing all read this one layout.
struct CopyBoxes {
	boxes: [Rect; 2],  // select, output checkbox squares
	label_x: [f32; 3], // left edge per COPYBOX_LABELS entry
	label_w: [f32; 3],
	shown: [bool; 3], // which labels there is room for at this window width
}

impl CopyBoxes {
	// Everything the cluster occupies, left edge first.
	fn left(&self) -> f32 {
		if self.shown[0] {
			self.label_x[0]
		} else {
			self.boxes[0].x
		}
	}

	// Where a click counts as hitting checkbox `i`: the box, plus its word when
	// the word is there to be aimed at.
	fn hit_range(&self, i: usize) -> (f32, f32) {
		let label = i + 1;
		let right = if self.shown[label] {
			self.label_x[label] + self.label_w[label]
		} else {
			self.boxes[i].x + self.boxes[i].w
		};
		(self.boxes[i].x, right)
	}
}

// What the cluster is measured from. Split out so the shedding order is a pure
// function of the numbers, testable without a window.
struct CopyMetrics {
	right: f32, // where the cluster's right edge sits
	label_w: [f32; 3],
	box_sz: f32,
	box_y: f32,
	box_gap: f32,  // checkbox to its own word
	pair_gap: f32, // one pair to the next
	lead_gap: f32, // "Copy on:" to the first checkbox
}

// Where menu-bar buffer `i` is drawn: (left, clip left, clip right, top). The
// titles come first, then the right-aligned copy-mode labels, and at a narrow
// width some of those are not there at all.
//
// Everything on this bar sits on ONE baseline. The copy labels used to center
// their full ink box, which reads better on its own but left them half a
// descent above the titles beside them.
fn menubar_text_slot(
	text: &TextCtx,
	menu_h: f32,
	bar_layout: &[(f32, f32)],
	copyboxes: Option<&CopyBoxes>,
	i: usize,
) -> Option<(f32, f32, f32, f32)> {
	let bar_top = text.ui_text_top(0.0, menu_h);
	if let Some(&(x, w)) = bar_layout.get(i) {
		return Some((x + text.dip(MENU_BAR_PAD), x, x + w, bar_top));
	}
	let j = i - bar_layout.len();
	let cb = copyboxes.filter(|cb| cb.shown[j])?;
	let (x, w) = (cb.label_x[j], cb.label_w[j]);
	Some((x, x, x + w, bar_top))
}

// The widest arrangement that still clears `titles_right`, or None when even the
// bare boxes cannot. Ordered widest first: whole thing, then without the lead-in,
// then boxes alone.
fn copybox_fit(m: &CopyMetrics, titles_right: f32) -> Option<CopyBoxes> {
	[[true; 3], [false, true, true], [false; 3]]
		.into_iter()
		.map(|shown| copybox_place(m, shown))
		.find(|cb| cb.left() >= titles_right)
}

// Laid out right to left, with a hidden label taking its own gap with it.
fn copybox_place(m: &CopyMetrics, shown: [bool; 3]) -> CopyBoxes {
	let mut label_w = m.label_w;
	for (w, on) in label_w.iter_mut().zip(shown) {
		if !on {
			*w = 0.0;
		}
	}
	let gap_if = |on: bool, gap: f32| if on { gap } else { 0.0 };
	let square = |x: f32| Rect {
		x,
		y: m.box_y,
		w: m.box_sz,
		h: m.box_sz,
	};
	let out_x = m.right - label_w[2];
	let out_box = square(out_x - gap_if(shown[2], m.box_gap) - m.box_sz);
	let sel_x = out_box.x - m.pair_gap - label_w[1];
	let sel_box = square(sel_x - gap_if(shown[1], m.box_gap) - m.box_sz);
	let lead_x = sel_box.x - m.lead_gap - label_w[0];
	CopyBoxes {
		boxes: [sel_box, out_box],
		label_x: [lead_x, sel_x, out_x],
		label_w,
		shown,
	}
}

// Tab strip: each tab owns its own pane split-tree. Detach/dock to other
// windows is deferred (needs multi-window support).
struct Tabs {
	list: Vec<PaneManager>,
	active: usize,
}

impl Tabs {
	fn cur(&self) -> &PaneManager {
		&self.list[self.active]
	}
	fn cur_mut(&mut self) -> &mut PaneManager {
		&mut self.list[self.active]
	}
	fn len(&self) -> usize {
		self.list.len()
	}
	// PaneIds are globally unique; the pane may live in any tab, not just the
	// active one (background-tab shells reply to ESC[6n etc. too)
	fn find_pane(&self, id: PaneId) -> Option<&Pane> {
		self.list.iter().find_map(|pm| pm.panes.get(&id))
	}
	fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
		self.list.iter_mut().find_map(|pm| pm.panes.get_mut(&id))
	}
	fn next(&mut self) {
		self.active = tab_step(self.active, self.list.len(), true);
	}
	fn prev(&mut self) {
		self.active = tab_step(self.active, self.list.len(), false);
	}
	fn move_active(&mut self, fwd: bool) {
		self.active = move_tab(&mut self.list, self.active, fwd);
	}
}

// The tab beside `i` of `n`, wrapping at both ends.
fn tab_step(i: usize, n: usize, forward: bool) -> usize {
	if forward {
		(i + 1) % n
	} else {
		(i + n - 1) % n
	}
}

// Swap the tab at `i` with its neighbour and answer where it went, so the
// active tab follows. Past either end it trades places with the far one.
fn move_tab<T>(list: &mut [T], i: usize, forward: bool) -> usize {
	if list.len() < 2 {
		return i;
	}
	let j = tab_step(i, list.len(), forward);
	list.swap(i, j);
	j
}

// Menu/tab bars auto-size to the menu (proportional) font: height = the text line
// height (cell_h) + this vertical padding, so a larger font isn't clipped (#124).
// Chrome measurements are DIP and convert at their use site - see config::dip.
const MENU_BAR_VPAD: f32 = 6.0;
const TAB_BAR_VPAD: f32 = 6.0; // text is metric-centered in the bar; descenders clear via that
const BELL_TAU_S: f32 = 0.18; // visual-bell flash fade time-constant (~0.8s to settle)
// Text scrim "Strength" is a percent; this much of it is one doubling of the
// finished halo's alpha. The scrim design doc and the dialog's comment both quote the
// number, and a test holds them to it.
const SCRIM_PCT_PER_DOUBLING: f32 = 20.0;
// Freeze knob (one line rolls it back): a minimized window builds no frames -
// PTY reading never stops - and catches up in one hard-cut frame on restore.
// Covers WMs that never report Occluded for an iconified window.
const FREEZE_MINIMIZED: bool = true;
// Warm knob (one line rolls it back): build the dialogs' GPU context on a worker
// thread once the terminal is up, instead of on the click that opens one. Off
// means every dialog open pays for its own instance + adapter + device again.
const WARM_DIALOG_GPU: bool = true;
const SIZE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500); // remember-size settle time before hitting disk
// How long after the window is genuinely on screen the background shell scan
// starts (shells.rs). Long enough to be clear of the first prompt and whatever
// the shell reads at startup, short enough that the Tabs menu has its list well
// before anyone opens it.
const SHELL_SCAN_DELAY: Duration = Duration::from_secs(3);
// The scan waits for the wallpaper to be on screen (see `wp_shown`), and a
// wallpaper on a share that never answers would otherwise hold it off for the
// life of the window - leaving the Tabs menu with no shells in it. This is the
// backstop, measured from the reveal.
const SHELL_SCAN_MAX_WAIT: Duration = Duration::from_secs(20);
// The performance benchmark waits for the wallpaper too: it is one of the
// things being timed, and starting before it is on screen would rate a lighter
// window than the one that ends up in front of the person. The backstop is for
// an image on a share that never answers.
const BENCH_DELAY: Duration = Duration::from_millis(600);
const BENCH_MAX_WAIT: Duration = Duration::from_secs(10);
// The banner goes up as soon as a run is due, so the wait for the wallpaper
// happens behind it rather than in front of a window that then stops taking
// input. And it stays up this long at least: a run that answers on the first
// rung is over in under a second, which is not enough time to notice a box has
// appeared, let alone read it.
const BENCH_BANNER_MIN: Duration = Duration::from_secs(4);
const VRAM_CHECK_IVL: Duration = Duration::from_secs(2); // GL sentinel probe tick (VT-switch texture loss)
// After a return to this console, how long until the second heal (VtHeal).
const VT_SETTLE: Duration = Duration::from_secs(3);
const CAPTURE_SETTLE: Duration = Duration::from_millis(120); // copy-output: idle-at-prompt debounce marking a command done
// Chrome geometry, all DIP (see config::dip).
const MENU_BAR_PAD: f32 = 10.0; // around each top-level title
const TAB_TIP_REFRESH: Duration = Duration::from_millis(500); // how often an open tip re-reads what it says
const TAB_TIP_PAD: f32 = 8.0; // inside the tip box, DIP
const MENU_TIP_PAD: f32 = 6.0; // inside a menu row's tip box, DIP
const MENU_TIP_GAP: f32 = 6.0; // between the popup's edge and the tip beside it, DIP
const MENU_TIP_MAX_W: f32 = 320.0; // widest a menu tip's text gets before it wraps, DIP
const BENCH_BANNER_PAD: f32 = 22.0; // inside the benchmark banner's box, DIP
const BENCH_SCRIM_ALPHA: f32 = 0.55; // how far the window dims under the banner
const TAB_TIP_GAP: f32 = 4.0; // between the tab bar and the tip below it, DIP
const TAB_CLOSE_W: f32 = 26.0; // right-edge close-button region per tab (title clips before it)
const TAB_CLOSE_M: f32 = 6.0; // balanced top/right/bottom margin around the close button box
const TAB_TITLE_PAD: f32 = 8.0; // tab title's left inset
const TAB_EDIT_INSET: f32 = 4.0; // rename box's inset inside the tab button
const TAB_EDIT_PAD: f32 = 4.0; // text inset inside the rename box (INSET + PAD == TAB_TITLE_PAD,
// so the label does not move when a rename starts)
const TAB_GAP: f32 = 1.0; // gap between adjacent tab buttons (each side of the seam)
const TAB_TOP_PAD: f32 = 2.0; // tab button's inset from the top of the bar
const CHROME_HAIRLINE: f32 = 1.0; // 1px rules: accelerator underlines, menu/checkbox borders
const COPYBOX_BOX_GAP: f32 = 6.0; // checkbox to its own word
const COPYBOX_PAIR_GAP: f32 = 14.0; // one checkbox pair to the next
const COPYBOX_LEAD_GAP: f32 = 10.0; // "Copy on:" lead-in to the first checkbox
const COPYBOX_TICK_INSET: f32 = 3.0; // checked fill's inset inside its box
const MENUBAR_TEXT_W: f32 = 240.0; // shaping width for a menu-bar title buffer
const MENU_ACCEL_DROP: f32 = 3.0; // accelerator underline's rise off the item's line box
// Input knob (one line rolls it back): a key that arrives while the window is
// unfocused is never typed. A WM hotkey grab (Ctrl+Alt+Arrow for desktop
// switching) brackets the chord with a focus-out/in pair, and winit zeroes the
// modifiers on the way out - so a grab that still passes the key through hands
// us an arrow with nothing held, which would encode as a bare arrow.
const IGNORE_KEYS_WHILE_UNFOCUSED: bool = true;

// Winit replays every key already held down whenever focus changes, flagged
// `is_synthetic`, so an app can track what is physically pressed. That is
// state, not typing - and on X11 the replay arrives BEFORE winit re-queries the
// modifiers, so a held Ctrl+Alt+Arrow comes back through it as a bare arrow.
fn key_is_typed(state: ElementState, is_synthetic: bool) -> bool {
	state == ElementState::Pressed && !is_synthetic
}

// SILK_DUMP / SILK_DLGDBG / SILK_KEYDBG are consulted per frame / per event;
// read the env once (var_os takes the env lock and scans environ every call).
// Same pattern as pane.rs scroll_dbg.
pub(crate) fn env_flag(name: &str) -> bool {
	use std::sync::OnceLock;
	static DUMP: OnceLock<bool> = OnceLock::new();
	static DLGDBG: OnceLock<bool> = OnceLock::new();
	static KEYDBG: OnceLock<bool> = OnceLock::new();
	let cell = match name {
		"SILK_DUMP" => &DUMP,
		"SILK_KEYDBG" => &KEYDBG,
		_ => &DLGDBG,
	};
	*cell.get_or_init(|| std::env::var_os(name).is_some())
}

// SILK_MAX_FPS pins the animation frame rate instead of letting vblank set it.
// Unset (every ordinary run) this is None and nothing below it changes: the GL
// path keeps swap interval 1 and a scroll ease keeps rendering on Poll.
//
// It exists for the demo recorder, which samples the X screen at a fixed rate.
// Whatever paces the app has to divide that rate evenly or frames fall off the
// sampling grid on a strict period - a source of 60 into a capture of 50 drops
// one frame in six, so every fifth stored frame carries two frames of travel,
// and a regular hitch like that is exactly what reads as the picture jumping.
// Pinning the source to the capture rate takes the host's refresh rate out of
// the answer entirely. Also useful for measuring a fixed frame budget.
fn max_fps() -> Option<f64> {
	use std::sync::OnceLock;
	static FPS: OnceLock<Option<f64>> = OnceLock::new();
	*FPS.get_or_init(|| {
		std::env::var("SILK_MAX_FPS")
			.ok()
			.and_then(|raw| raw.trim().parse::<f64>().ok())
			.filter(|fps| *fps > 0.0 && fps.is_finite())
	})
}

// The display's refresh rate, which is what a scroll ease is paced against.
// Unknown reads as 60: the common case, and a wrong guess only moves the
// budget a little.
fn refresh_hz(window: &Window) -> f32 {
	window
		.current_monitor()
		.and_then(|m| m.refresh_rate_millihertz())
		.map_or(60.0, |mhz| mhz as f32 / 1000.0)
}

// Whether a refused save gets a notice. One the user asked for, an OK or Apply
// in Settings, is answered every time, or the button would seem to do nothing.
// The others (a resize, a menu switch, shells found at launch) are said once a
// session for each file, or every resize would raise it again.
fn notice_due(told: &mut Vec<std::path::PathBuf>, path: &std::path::Path, asked: bool) -> bool {
	let first = !told.iter().any(|seen| seen == path);
	if first {
		told.push(path.to_path_buf());
	}
	asked || first
}

// The system's own message box, from a thread of its own: it runs a message
// loop until OK, and on the window's thread that would stop every pane drawing.
// Owned by `owner`, so it stays in front of it and keeps its input while up.
#[cfg(target_os = "windows")]
static NOTICE_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "windows")]
fn message_box(owner: isize, title: &str, body: &str) {
	use std::sync::atomic::Ordering;
	use windows_sys::Win32::UI::WindowsAndMessaging::{
		MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MessageBoxW,
	};
	let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
	let (title, body) = (wide(title), wide(body));
	NOTICE_UP.store(true, Ordering::SeqCst);
	std::thread::spawn(move || {
		// SAFETY: both strings are NUL-terminated and outlive the call
		unsafe {
			MessageBoxW(
				owner as windows_sys::Win32::Foundation::HWND,
				body.as_ptr(),
				title.as_ptr(),
				MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
			);
		}
		NOTICE_UP.store(false, Ordering::SeqCst);
	});
}

// A remote screen wears the Remote profile for the session. Nothing is written
// and nothing is rated: the console keeps the profile it had, and the override
// lifts at the next launch unless that one is remote too.
fn remote_override_at_launch() {
	if !crate::profile::remote_session() {
		return;
	}
	let mut live = (*config::settings()).clone();
	live.remote_override = true;
	config::update(live);
}

// A rotation folder with nothing picked from it has to be read, or the request
// answers with no picture at all: a configured folder owns the wallpaper and
// suppresses the built-in stand-in, so there is nothing left to show. A
// command-line wallpaper is exempt, since it keeps rotation out of the session.
fn needs_folder_read(
	locked: bool,
	showing: Option<&std::path::Path>,
	folder: Option<&std::path::PathBuf>,
) -> bool {
	!locked && showing.is_none() && folder.is_some()
}

// What the performance watch does with a pass of the event loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RatingStep {
	Note,
	Pause,
}

// A frame is evidence about the hardware only when this window's own eased
// rendering paced it. A benchmark is timing the same frames itself, a pinned
// rate paces itself, and a window without focus is one nobody is watching, so
// it gets no say in the profile.
#[allow(clippy::fn_params_excessive_bools)] // four independent gates, all sixteen cases tested
fn rating_step(bench: bool, scroll_anim: bool, pinned_fps: bool, focused: bool) -> RatingStep {
	if !bench && scroll_anim && !pinned_fps && focused {
		RatingStep::Note
	} else {
		RatingStep::Pause
	}
}

// Where the rotation timer goes when a tick fires. It has to move off `now`
// here rather than waiting for the worker's answer: the answer is dropped
// unless it is still the newest request, and a timer left in the past fires
// again on the very next pass, so each pass started another decode thread.
// Whether rotation has anywhere to go: not held by a command-line wallpaper,
// and the last scan found more than the one image showing.
fn rotation_live(locked: bool, count: usize, folder: bool) -> bool {
	!locked && count >= 2 && folder
}

fn rotation_next(now: Instant, live: bool, interval_s: f32) -> Option<Instant> {
	(live && interval_s > 0.0).then(|| now + Duration::from_secs_f32(interval_s))
}

// A wait that starts over once the wallpaper is on screen (the shell scan, the
// benchmark). It moves out to `due` but never past its backstop, and a wait that
// already ran stays gone: a late wallpaper must not start a second one.
fn push_back(at: Option<Instant>, cap: Option<Instant>, due: Instant) -> Option<Instant> {
	at.map(|_| cap.map_or(due, |cap| due.min(cap)))
}

// With the profile on automatic, hardware the config has not seen gets a fresh
// pick, written down against that hardware so the next launch on it leaves the
// profile where the rating left it. Answers the id a benchmark should write
// when it finishes, or None where there is nothing to time - a software
// renderer is decided here and written now, and a remote screen is left alone.
fn rate_hardware(info: &wgpu::AdapterInfo) -> Option<String> {
	let live = config::settings();
	if !live.performance_automatic || live.remote_override {
		return None;
	}
	let hardware = crate::profile::hardware_id(info);
	if !rating_due(&live, &hardware) {
		return None;
	}
	// The one-shot check counts as new hardware, and clears itself here rather
	// than with the answer: a window closed mid-run has still had its run.
	if live.performance_check_next_run {
		let kept = config::keep_rating(&config::RatingLines {
			check_next_run: Some(false),
			..config::RatingLines::default()
		});
		note_rating_not_kept(&kept);
		let mut new = (*live).clone();
		new.performance_check_next_run = false;
		config::update(new);
	}
	let live = config::settings();
	if crate::profile::worth_measuring(info) {
		// Nothing is written yet. The id goes down with the measured answer, so a
		// window closed mid-run is measured again next launch instead of leaving
		// the heaviest rung recorded as this machine's rating.
		return Some(hardware);
	}
	let pick = crate::profile::first_pick(info);
	// no banner on this path, and redoing it next launch costs nothing
	let kept = config::keep_rating(&config::RatingLines {
		profile: Some(pick.key()),
		rated_hardware: Some(&hardware),
		check_next_run: None,
	});
	note_rating_not_kept(&kept);
	let mut new = (*live).clone();
	new.rated_hardware = hardware;
	new.performance_profile = pick.key().to_string();
	config::update(new);
	None
}

// Whether this launch owes a rating. A machine already rated once is only
// re-rated when asked for; the first rating has to happen either way, or there
// is no profile at all.
fn rating_due(live: &config::Settings, hardware: &str) -> bool {
	if !live.performance_automatic || live.remote_override {
		return false;
	}
	if live.performance_check_next_run {
		return true;
	}
	if live.rated_hardware == hardware {
		return false;
	}
	live.performance_check_hardware || live.rated_hardware.is_empty()
}

// A measured answer into the settings file. Its own function so a test can run
// the same write the banner's run does.
fn keep_measured(pick: crate::profile::Profile, id: Option<&str>) -> config::Kept {
	config::keep_rating(&config::RatingLines {
		profile: Some(pick.key()),
		rated_hardware: id,
		check_next_run: None,
	})
}

// Every rating write reports a failure, since a rating that is not kept is a
// test again at the next launch, and a Windows release build shows no stderr
// (G37) - which is why the banner says it too.
fn note_rating_not_kept(kept: &config::Kept) {
	let reason = match kept {
		config::Kept::Written => return,
		config::Kept::Busy => "the settings file is open in another program",
		config::Kept::Unreadable => "the settings file has a line that cannot be read",
		config::Kept::Unplaced => {
			"the performance section of the settings file could not be updated"
		}
		config::Kept::Unwritable(why) => why.as_str(),
	};
	eprintln!(
		"{}: performance rating not saved ({reason}); the test runs again at the next launch",
		config::APP_NAME
	);
}

// What the benchmark's banner says: that a run is on, or, for its last few
// seconds, why its answer could not be kept.
fn bench_banner_lines(kept: Option<&config::Kept>) -> &'static [&'static str] {
	const AGAIN: &str = "The test runs again at the next launch.";
	match kept {
		None | Some(config::Kept::Written) => &["Testing performance", "This takes a few seconds."],
		Some(config::Kept::Busy) => &[
			"Could not save the result",
			"The settings file is open in another program.",
			AGAIN,
		],
		Some(config::Kept::Unreadable) => &[
			"Could not save the result",
			"The settings file has a line that cannot be read.",
			AGAIN,
		],
		Some(config::Kept::Unplaced) => &[
			"Could not save the result",
			"The performance section of the settings file could not be updated.",
			AGAIN,
		],
		Some(config::Kept::Unwritable(_)) => &[
			"Could not save the result",
			"The settings file cannot be written.",
			AGAIN,
		],
	}
}

// What the banner says after a run the display stalled.
const BENCH_STALLED_LINES: &[&str] = &[
	"Could not test performance",
	"The display was not drawing at its usual rate.",
	"The test runs again at the next launch.",
];

// The user's own settings with a measured profile stored in them: a benchmark
// rung while it is timed, or the answer once the run ends. A measurement replaces
// any step the display watch took, and a step left in place would sit over the
// rung and time the wrong one.
fn with_measured_profile(
	live: &config::Settings,
	profile: crate::profile::Profile,
) -> config::Settings {
	let mut next = live.clone();
	crate::profile::unapply(&mut next);
	next.performance_profile = profile.key().to_string();
	next.stepped_profile = None;
	next
}

// The live settings with the display watch's next step in force, or None when
// automatic is off or the watch has no rung left. Only the session field moves:
// a step written to the file became every later launch's profile.
fn watch_step_down(live: &config::Settings) -> Option<config::Settings> {
	if !live.performance_automatic {
		return None;
	}
	let lower = crate::profile::current(live).watched_lower()?;
	let mut next = live.clone();
	next.stepped_profile = Some(lower);
	Some(next)
}

// Next frame on a FIXED schedule, not `now + interval` - the latter adds each
// frame's own render time to the period and runs slow. Falling behind resyncs
// rather than trying to catch up in a burst.
fn pace_frame(next: &mut Option<Instant>, ivl: Duration) -> ControlFlow {
	let now = Instant::now();
	let mut at = next.unwrap_or(now) + ivl;
	if at <= now {
		at = now + ivl;
	}
	*next = Some(at);
	ControlFlow::WaitUntil(at)
}

// The times a pane asks the loop to come back at, apart from animating. A
// new one goes here too, or `pane_wake` never sees it.
trait PaneWakes {
	fn cursor_wake_at(&self) -> Option<Instant>;
	fn map_wake_at(&self) -> Option<Instant>;
}

impl PaneWakes for Pane {
	fn cursor_wake_at(&self) -> Option<Instant> {
		self.cursor_wake
	}

	fn map_wake_at(&self) -> Option<Instant> {
		self.map_wake()
	}
}

// The earliest pane wake still ahead of `now`, read after the frame, since
// drawing is what sets them. One already due is left out: the pass before the
// frame acts on those, and a window that is not drawing would spin on one.
fn pane_wake<'a, P: PaneWakes + 'a>(
	panes: impl IntoIterator<Item = &'a P>,
	now: Instant,
) -> Option<Instant> {
	panes
		.into_iter()
		.flat_map(|pane| [pane.cursor_wake_at(), pane.map_wake_at()])
		.flatten()
		.filter(|&wake| wake > now)
		.min()
}

// When the window last saw a person or a shell: input, focus either way, or
// output while it could be seen. The idle release counts from `since` (see
// `release_deadline`). `wake_owed` says something wants the window back since
// it let its device go, so the device is owed as soon as there is a screen to
// draw on.
struct IdleClock {
	since: Instant,
	wake_owed: bool,
}

impl IdleClock {
	fn new() -> Self {
		IdleClock {
			since: Instant::now(),
			wake_owed: false,
		}
	}

	// A sign of life. True when it is the one that makes the device owed.
	fn active(&mut self, released: bool) -> bool {
		self.since = Instant::now();
		self.owe(released)
	}

	fn owe(&mut self, released: bool) -> bool {
		let newly = released && !self.wake_owed;
		self.wake_owed |= released;
		newly
	}

	// A shell printing. Output nobody can see does not keep a hidden window's
	// device, or a program that prints forever would hold it for good. It is
	// still owed at the reveal, so a desktop that says nothing about showing
	// the window again cannot leave old pixels up.
	fn output(&mut self, released: bool, hidden: bool) -> bool {
		if hidden {
			self.owe(released)
		} else {
			self.active(released)
		}
	}
}

// What the window title says about the device. Nothing normally, a note while
// it is let go, another while it comes back, and a last one for a few seconds
// after. The wallpaper is the last thing a rebuild waits on, and the only part
// slow enough for anyone to see, so coming back lasts until it answers.
// Any rebuild counts, a return to this console as much as the idle release.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Conserve {
	Off,
	Saving,
	Restoring,
	Restored(Instant),
}

const RESTORED_SHOWN: Duration = Duration::from_secs(5);

impl Conserve {
	fn note(self, now: Instant) -> Option<&'static str> {
		match self {
			Conserve::Off => None,
			Conserve::Saving => Some("resource conservation mode"),
			Conserve::Restoring => Some("restoring resources ..."),
			Conserve::Restored(at) => (now < at + RESTORED_SHOWN).then_some("resources restored"),
		}
	}

	// When the title next changes on its own.
	fn wake(self) -> Option<Instant> {
		match self {
			Conserve::Restored(at) => Some(at + RESTORED_SHOWN),
			_ => None,
		}
	}

	fn wallpaper_answered(&mut self, now: Instant) {
		if *self == Conserve::Restoring {
			*self = Conserve::Restored(now);
		}
	}
}

// What the idle release reads of the window (see `release_deadline`).
struct Idle {
	focused: bool,
	hidden: bool,     // minimized, or covered where the desktop says so
	revealed: bool,   // shown at all yet
	bench_busy: bool, // a rating owed or running
	since: Instant,   // the last sign of life
}

// When an idle window may let its device go, or None while something keeps
// it: the switch off, the window not yet shown, a rating owed or running, or a
// window that has focus and is on screen. Two waits, because a hidden window is
// known to be out of sight while a merely unfocused one may be on a second
// monitor being read.
fn release_deadline(cfg: &config::Settings, idle: &Idle) -> Option<Instant> {
	let (on, when_hidden, otherwise) = idle_rule(cfg);
	if !on || !idle.revealed || idle.bench_busy || (idle.focused && !idle.hidden) {
		return None;
	}
	Some(idle.since + if idle.hidden { when_hidden } else { otherwise })
}

// The setting's answer, unless SILK_IDLE_SECS names one wait in seconds for
// both cases - which is how the release is exercised without leaving a window
// alone for half an hour.
fn idle_rule(cfg: &config::Settings) -> (bool, Duration, Duration) {
	if let Some(secs) = std::env::var("SILK_IDLE_SECS")
		.ok()
		.and_then(|raw| raw.parse::<f32>().ok())
		.filter(|secs| secs.is_finite() && *secs >= 0.0)
	{
		let wait = Duration::from_secs_f32(secs);
		return (true, wait, wait);
	}
	let minutes = |m: usize| Duration::from_secs(m as u64 * 60);
	(
		cfg.idle_release,
		minutes(cfg.idle_release_hidden_min),
		minutes(cfg.idle_release_min),
	)
}

// SILK_IDLEDBG=1: the idle release's comings and goings on stderr, stamped
// with seconds since the first call so a log can be read against a timeline.
fn idledbg(msg: &str) {
	use std::sync::OnceLock;
	static T0: OnceLock<Instant> = OnceLock::new();
	if !env_flag("SILK_IDLEDBG") {
		return;
	}
	let t = T0.get_or_init(Instant::now).elapsed().as_secs_f32();
	eprintln!("[idle {t:7.2}s] {msg}");
}

// Hand freed heap back to the OS. glibc keeps what it was given unless asked,
// so a release that dropped tens of MB would still show them as resident.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_heap() {
	// SAFETY: no arguments to get wrong, and it only touches the allocator's
	// own free lists.
	unsafe {
		libc::malloc_trim(0);
	}
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_heap() {}

// A big buffer comes straight from the OS and goes straight back. glibc's
// mmap threshold starts at 128 KB but moves: freeing a mapped buffer raises it
// to that buffer's size, after which a wallpaper's decode (several buffers of
// megabytes each) is carved out of the thread's own arena and stays resident
// there once freed, because `malloc_trim` never shrinks an arena that is not
// the main one. Measured on a 1920x993 wallpaper: about 40 MB kept per decode,
// one per rebuild after an idle release, and the first decode's 50 MB kept for
// the life of every window. Setting the threshold pins it. 4 MB keeps a
// frame's own vectors in the arena on any grid and puts only the image buffers
// on the mapping path. Pinning it also stops the trim threshold moving, so
// that is set too, high enough that the main heap's top is not given back and
// asked for again around every frame. Called before the first thread exists,
// like the environment fixes.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn tune_heap() {
	// SAFETY: plain allocator parameters, set before any other thread runs.
	unsafe {
		libc::mallopt(libc::M_MMAP_THRESHOLD, 4 << 20);
		libc::mallopt(libc::M_TRIM_THRESHOLD, 8 << 20);
	}
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(crate) fn tune_heap() {}

// VT-switch field diagnostics: `touch ~/silk_vramdbg.on` (no relaunch needed)
// makes the sentinel probes append their results to ~/silk_vramdbg.txt, so a
// desktop repro can show whether loss detection fired. The marker is re-checked
// per call - probes tick every 2s, so the stat costs nothing.
fn vramdbg(msg: &str) {
	use std::io::Write;
	let Some(home) = std::env::var_os("HOME") else {
		return;
	};
	let home = std::path::PathBuf::from(home);
	if !home.join("silk_vramdbg.on").exists() {
		return;
	}
	let path = home.join("silk_vramdbg.txt");
	// a forgotten marker must not grow the log unbounded
	if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 4_000_000) {
		return;
	}
	let epoch = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_secs());
	if let Ok(mut f) = std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(&path)
	{
		let _ = writeln!(f, "{epoch} pid={} {msg}", std::process::id());
	}
}

// A return to this console is healed twice: at once, and again once the X
// server has had time to take the display back. The watcher sees the console
// change before the mode set, so a purge that comes after the first rebuild
// would spoil it too.
#[derive(Default)]
struct VtHeal {
	again: Option<Instant>,
}

impl VtHeal {
	fn returned(&mut self, now: Instant) {
		self.again = Some(now + VT_SETTLE);
	}

	// True once, when the second heal comes due.
	fn due(&mut self, now: Instant) -> bool {
		if self.again.is_some_and(|at| now >= at) {
			self.again = None;
			return true;
		}
		false
	}
}

// Watch the active virtual console (/sys/class/tty/tty0/active). A VT switch
// away and back breaks sampling of long-lived textures in ways the readback
// probes cannot see (field logs: every witness read back intact across a switch
// that blacked the window - the driver restores readback contents while the
// sampled copies stay garbage). So detect the switch itself: the value at spawn
// is the console this display lives on; when the file returns to it after being
// elsewhere, send VtSwitched so the sampled textures are rebuilt. Only returns
// are signaled - a rebuild done while parked on another console could itself be
// purged on the way back. SILK_VTFILE overrides the watched path so a headless
// test can drive the mechanism (Xvfb has no VTs).
#[cfg(target_os = "linux")]
fn spawn_vt_watch(proxy: EventLoopProxy<UserEvent>) -> bool {
	let path = std::env::var_os("SILK_VTFILE").map_or_else(
		|| std::path::PathBuf::from("/sys/class/tty/tty0/active"),
		std::path::PathBuf::from,
	);
	let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().map(|s| s.trim().to_owned());
	// unreadable (container, odd kernel) -> no watcher; probes remain as fallback
	let Some(home_vt) = read(&path) else {
		return false;
	};
	std::thread::spawn(move || {
		let mut watch = VtWatch::new(home_vt);
		loop {
			std::thread::sleep(Duration::from_millis(500));
			let Some(cur) = read(&path) else {
				continue;
			};
			if cur != watch.last {
				vramdbg(&format!("vt switch: {} -> {cur}", watch.last));
			}
			if watch.step(cur) && proxy.send_event(UserEvent::VtSwitched).is_err() {
				return; // event loop gone - exit with the app
			}
		}
	});
	true
}

// The console this display lives on, and the one seen last.
#[cfg(any(target_os = "linux", test))]
struct VtWatch {
	home: String,
	last: String,
}

#[cfg(any(target_os = "linux", test))]
impl VtWatch {
	fn new(home: String) -> Self {
		let last = home.clone();
		Self { home, last }
	}

	// Take the console now active; true only when that is a return home.
	fn step(&mut self, cur: String) -> bool {
		let returned = cur == self.home && self.last != self.home;
		self.last = cur;
		returned
	}
}

#[cfg(not(target_os = "linux"))]
fn spawn_vt_watch(_proxy: EventLoopProxy<UserEvent>) -> bool {
	false
}

// Top and height of a tab button inside the bar: inset at the top, and the bar's
// bottom hairline left showing under it. The draw and the text centering read
// the one rule.
fn tab_button_v(bar_y: f32, tab_h: f32, scale: f32) -> (f32, f32) {
	let top = config::dip(TAB_TOP_PAD, scale);
	let rule = config::dip(CHROME_HAIRLINE, scale);
	(bar_y + top, tab_h - top - rule)
}

// The close-"x" button box within a tab: a square with equal top/right/bottom
// margins (the extra room falls to the left, separating it from the title).
// Shared by the rect draw, the glyph placement, and the click hit-test so they
// can't drift apart.
fn tab_close_box(tab_x: f32, tab_w: f32, bar_y: f32, tab_h: f32, scale: f32) -> Rect {
	let m = config::dip(TAB_CLOSE_M, scale);
	let side = (tab_h - 2.0 * m).max(config::dip(8.0, scale));
	Rect {
		x: tab_x + tab_w - m - side,
		y: bar_y + m,
		w: side,
		h: side,
	}
}
// The box a tab being renamed types into: a real text field inside the tab
// button, stopping short of the close column and as tall as a line of text plus
// its own padding, or the button itself where that is shorter.
fn tab_edit_box(tab_x: f32, tab_w: f32, bar_y: f32, tab_h: f32, line_h: f32, scale: f32) -> Rect {
	let inset = config::dip(TAB_EDIT_INSET, scale);
	let (btn_y, btn_h) = tab_button_v(bar_y, tab_h, scale);
	let h = (line_h + 2.0 * config::dip(TAB_EDIT_PAD, scale)).min(btn_h - 2.0 * inset);
	let x = tab_x + inset;
	let right = tab_x + tab_w - config::dip(TAB_CLOSE_W, scale) - inset;
	Rect {
		x,
		y: btn_y + (btn_h - h) / 2.0,
		w: (right - x).max(config::dip(8.0, scale)),
		h,
	}
}

// How much of a tab its title actually gets: the button less its own inset on
// both sides and the close-button column it must never run under. The draw and
// the fit read the one rule, or a title is shortened to a width it is not then
// given.
fn tab_title_w(tab_w: f32, scale: f32) -> f32 {
	let pad = config::dip(TAB_TITLE_PAD, scale);
	(tab_w - 2.0 * pad - config::dip(TAB_CLOSE_W, scale)).max(config::dip(8.0, scale))
}

// The command line behind a tab, for naming the shell it runs. Every pane
// resolves its own at spawn (see `spawn_pane`), so None here means nothing is
// switched on at all and the engine picked its own default - which we have no
// way to name, and must not GUESS at from the list: guessing is what had a pane
// running PowerShell labelled Command Prompt.
fn tab_command_line(command: Option<&[String]>) -> String {
	command.map_or_else(String::new, crate::shells::command_line)
}

// A tab's hover tip: what it runs, how it was started, where it is, and how
// long it has been open - the three of those a tab is too narrow to say, plus
// the one it never says. The lines are built on a timer rather than per frame:
// naming the shell resolves its program on the filesystem, and the clock at the
// bottom has to tick anyway.
struct TabTip {
	tab: usize,
	lines: Vec<String>,
	built: Instant,
}

// What a typed tab title is worth keeping, given what the tab would say on its
// own. Blank, or the same as the automatic name, means no override at all -
// both are the way back to a tab that names itself.
fn typed_title(typed: String, auto: &str) -> Option<String> {
	(!typed.trim().is_empty() && typed != auto).then_some(typed)
}

// A tab title being typed in place. `caret` and `anchor` are byte offsets into
// `text`; equal means no selection. Committing text that matches what the tab
// would have said on its own puts it back to naming the shell.
struct TabEdit {
	tab: usize,
	text: String,
	caret: usize,
	anchor: usize,
}

impl TabEdit {
	fn range(&self) -> (usize, usize) {
		(self.caret.min(self.anchor), self.caret.max(self.anchor))
	}

	// Replace the selection (or insert at the caret) and leave the caret after it.
	fn insert(&mut self, typed: &str) {
		let (from, to) = self.range();
		self.text.replace_range(from..to, typed);
		self.caret = from + typed.len();
		self.anchor = self.caret;
	}

	// Backspace (back = true) or Delete. With a selection, either just clears it.
	fn erase(&mut self, back: bool) {
		let (from, to) = self.range();
		if from != to {
			self.text.replace_range(from..to, "");
			self.caret = from;
		} else if back {
			if let Some(prev) = self.text[..self.caret].chars().next_back() {
				self.caret -= prev.len_utf8();
				self.text.remove(self.caret);
			}
		} else if self.caret < self.text.len() {
			self.text.remove(self.caret);
		}
		self.anchor = self.caret;
	}

	// Move the caret one character, to an end, and either drag the selection with
	// it or drop it.
	fn move_caret(&mut self, to: Caret, select: bool) {
		self.caret = match to {
			Caret::Left => self.text[..self.caret]
				.chars()
				.next_back()
				.map_or(0, |c| self.caret - c.len_utf8()),
			Caret::Right => self.text[self.caret..]
				.chars()
				.next()
				.map_or(self.caret, |c| self.caret + c.len_utf8()),
			Caret::Home => 0,
			Caret::End => self.text.len(),
		};
		if !select {
			self.anchor = self.caret;
		}
	}
}

#[derive(Clone, Copy)]
enum Caret {
	Left,
	Right,
	Home,
	End,
}

const MENU_BAR: [&str; 6] = ["File", "Edit", "View", "Tabs", "Panes", "Help"];

// The top-level menu Alt plus `ch` opens: the one whose title starts with it.
fn bar_menu_for(ch: char) -> Option<usize> {
	let ch = ch.to_ascii_uppercase();
	MENU_BAR.iter().position(|title| title.starts_with(ch))
}

// Which bar titles get their accelerator underlined, and on which letter: all
// of them while Alt is held, none while a dropdown is open, since the dropdown
// underlines its own rows then.
fn bar_title_underlines(
	alt_held: bool,
	open: Option<usize>,
	titles: &[&str],
) -> Vec<(usize, char)> {
	if !alt_held || open.is_some() {
		return Vec::new();
	}
	titles
		.iter()
		.enumerate()
		.filter_map(|(i, title)| title.chars().next().map(|c| (i, c)))
		.collect()
}
const COPYBOX_LABELS: [&str; 3] = ["Copy on:", "select", "output"]; // menu-bar auto-copy checkboxes

// Everything that lives on the GPU device, held together so an idle window can
// let the whole lot go at once and take it back later (see `release_gpu`).
struct Gpu {
	gfx: Gfx,
	rects: RectRenderer,
	minimap: crate::minimap::MapRenderer,
	wallpaper_img: Option<ImageRenderer>,
	scrim: crate::scrim::Scrim, // text readability scrim (used only when config.text_scrim)
}

impl Gpu {
	// The renderers first and the device last: `Gfx::release` needs every
	// handle on the device gone before it runs.
	fn release(self) -> Rebirth {
		let Self {
			gfx,
			rects,
			minimap,
			wallpaper_img,
			scrim,
		} = self;
		drop((wallpaper_img, minimap, scrim, rects));
		gfx.release()
	}
}

struct State {
	window: Arc<Window>,
	// None while the window has let its device go after a long idle, with what
	// is needed to build it again waiting in `rebirth` (see `release_gpu`).
	// `render` takes it out for the length of a frame, so nothing a frame calls
	// may ask for it.
	gpu: Option<Gpu>,
	rebirth: Option<Rebirth>,
	text: TextCtx,
	// posts worker results (wallpaper) back into this event loop
	proxy: EventLoopProxy<UserEvent>,
	tabs: Tabs,
	mods: ModifiersState,
	mouse: (f32, f32),
	mouse_btn: Option<input::MouseBtn>, // button held after a reported press (mouse-tracking apps)
	mouse_cell: Option<(usize, usize)>, // last cell reported, to de-dupe motion
	selecting: Option<PaneId>,          // pane with an in-progress drag-select
	select_edge_held: f32,              // seconds that drag has been past a pane edge
	last_click: Option<(Instant, f32, f32)>, // for multi-click detection
	click_count: u32,                   // consecutive clicks in the same spot (2=double, 3=triple)
	// (active tab, focused pane, window focused) at the last frame - a change
	// while focused pokes the focused pane's cursor, so a long-idle-parked
	// animation resumes on any window/tab/pane refocus
	cursor_focus_sig: Option<(usize, PaneId, bool)>,
	resizing: Option<Vec<bool>>, // split-tree path of the divider being dragged
	dragging_pane: Option<PaneId>, // pane being drag-reordered (Shift+drag)
	bar_dragging: Option<PaneId>, // pane whose scrollbar thumb is being dragged
	map_dragging: Option<PaneId>, // pane whose minimap marker is being dragged
	// A Ctrl+press hit a hyperlink: the release over the same link opens it,
	// a release anywhere else drops it (drag off to cancel, like the tab close
	// button). The URL is captured at press time - output can scroll it away in
	// between - and `menu_link` is the same for the right-click menu's two items.
	link_arm: Option<(PaneId, String)>,
	menu_link: Option<String>,
	cursor_icon: CursorIcon,
	clipboard: Clipboard,
	last_frame: Instant,
	// how a scroll ease is being paced, and the period a frame may not exceed
	// before it counts as a miss (profile.rs)
	rating: crate::profile::Rating,
	frame_budget: crate::profile::FrameBudget,
	// The startup benchmark: a run in flight, and when one is due. While a run
	// is in flight the window renders flat out. bench_banner is when the banner
	// went up, which is earlier than either - it covers the wait as well as the
	// run, and the window takes no input for as long as it is up (bench_layout).
	bench: Option<crate::profile::Bench>,
	bench_at: Option<Instant>,
	bench_cap: Option<Instant>,
	bench_banner: Option<Instant>,
	// Set while a run is owed or in flight; written down with the answer.
	bench_id: Option<String>,
	// Why the answer was not kept, while the banner says so.
	bench_kept: Option<config::Kept>,
	// The profile in force before the run, to go back to if it gives no answer,
	// and whether it gave none (the banner says that instead).
	bench_from: Option<crate::profile::Profile>,
	bench_stalled: bool,
	dirty: bool,
	bell_flash: f32,    // visual-bell brightness, set to 1.0 on BEL, decays to 0
	size_tracked: bool, // false until the first frame, so startup/programmatic resizes don't overwrite remembered_size
	// The window is born hidden and revealed once a real frame is on screen at its
	// final (grid-derived) size, so it never flashes the default size / blank client
	// before painting. reveal_want is the physical size to wait for when the startup
	// resize was async (None = reveal on the first frame); reveal_deadline is a hard
	// fallback so an async or WM-adjusted resize can't strand the window hidden.
	revealed: bool,
	// When the background shell scan is due: set when the window is revealed,
	// cleared when the scan is away. None the rest of the time - it runs once.
	shell_scan_at: Option<Instant>,
	reveal_want: Option<winit::dpi::PhysicalSize<u32>>,
	reveal_deadline: Instant,
	pending_size: Option<(usize, usize)>, // debounced remember-size: persisted after the size holds, not per resize tick
	pending_size_at: Instant,
	menu: Option<ContextMenu>,
	tab_close_arm: Option<usize>, // tab whose close button is held down (closes on release)
	tab_hover: crate::tip::Dwell<usize>, // tab under the pointer, and since when
	// menu row under the pointer, as (how deep in the open chain, which row)
	menu_tip: crate::tip::Dwell<(usize, usize)>,
	// the row whose tip is up. Kept because ripeness is reached by the clock
	// rather than by an event, so "has it changed" has to be asked against what
	// was last drawn, not against what the timer said a moment ago.
	menu_tip_up: Option<(usize, usize)>,
	tab_first: usize,                  // tab the strip is paged to (clamped on read)
	tab_followed: usize,               // active tab the page last followed (see rebuild_tab_layout)
	tab_layout: TabLayout,             // the strip as measured (see rebuild_tab_layout)
	tab_tip: Option<TabTip>,           // the hover tip currently up, if any
	tab_edit: Option<TabEdit>,         // tab title being renamed in place (double-click)
	tab_dbl: Option<(Instant, usize)>, // last tab-strip click, for the double
	decorated: bool,                   // window frame shown (winit has no getter, so track it)
	menu_bar: bool,                    // window menu bar (File/Edit/...) shown
	bare: bool, // frame, menu bar and tab strip all off at once (View > Bare window); never written
	bare_saved: (bool, bool), // (decorated, menu_bar) from before bare, to put back
	bar_open: Option<usize>, // which top-level menu's dropdown is open, if any
	quit: bool, // set by File->Quit; the event handler exits after applying
	win_opacity: Option<f32>, // CLI --background-opacity override (this window only)
	win_title: Option<String>, // CLI --title override (else "AppName - <tab title>")
	last_win_title: String, // last string set on the window (skip redundant set_title)
	focused: bool, // window has keyboard focus (gates copy-output: never copy from a background window)
	pending_about: bool, // request to open the About window (App acts on it; needs the event loop)
	pending_settings: bool, // request to open the Settings window
	chrome: Option<ChromeCache>, // shaped menu/tab text, reused across frames
	chrome_rev: u64, // bumped whenever a chrome buffer is (re)shaped
	// Signature of everything feeding the prepared text set, from the last frame
	// that actually prepared. A pure cursor frame matches it, and then both
	// glyphon prepares (the bulk of per-frame CPU) and the atlas trim are skipped
	// - the retained vertex buffers are still correct. None = must prepare.
	text_sig: Option<u64>,
	// same idea for the context-menu overlay (skip re-shaping an open menu)
	overlay_sig: Option<u64>,
	// The scrim's blurred source is valid for this signature. The halo depends on
	// the text alone (the cursor has its own coverage texture), so a cursor-only
	// frame reuses it instead of re-rendering and re-blurring the whole window.
	scrim_sig: Option<u64>,
	occluded: bool, // window fully hidden: skip rendering entirely until it comes back
	// last cycle's frozen state (occluded or minimized); the false edge is the
	// unfreeze - one dirty catch-up frame, hard-cut. Read and written only by
	// freeze_sync, which both render entry points go through.
	was_hidden: bool,
	// Deadline of the next animation frame while SILK_MAX_FPS pins the rate; None
	// otherwise, which is every ordinary run. See `max_fps`.
	next_frame: Option<Instant>,
	// Rotation state as of the last scan; the folder itself, the shuffle history
	// and the picking all live in the worker (wallpaper.rs), so nothing here reads
	// the filesystem.
	wp_count: usize, // images the last scan found (<2 = nothing to rotate to)
	wp_current: Option<PathBuf>, // image showing now, so order mode advances from it
	wp_next: Option<Instant>, // when to rotate next (None = no timer / startup-only)
	wp_locked: bool, // a command-line wallpaper owns this session; don't rotate
	// The window options the command line gave at launch, folded in again after a
	// reload so a reread file does not drop them.
	cli_style: crate::cli::Style,
	// Request stamp, shared with the workers: a result with an older one is
	// stale, and a worker whose stamp is no longer the newest stops early.
	wp_seq: Arc<std::sync::atomic::AtomicU64>,
	wp_pacing: crate::wallpaper::Pacing, // holds a tick while a request is working
	// A worker has answered - with an image, or with the news that there is none.
	wp_answered: bool,
	// ...and a frame has been drawn since, so whatever it said is ON SCREEN. This
	// is what the shell scan waits for: the scan is filesystem and registry work
	// and the wallpaper is hundreds of ms of decode and blur off-thread, so letting
	// them overlap puts a stall between the window appearing and the wallpaper
	// arriving in it - the one moment anyone is looking.
	wp_shown: bool,
	// The hard deadline for that wait (see SHELL_SCAN_MAX_WAIT).
	shell_scan_cap: Option<Instant>,
	vram_next: Instant, // next GL VRAM sentinel probe (VT-switch content-loss detection)
	vramloss_test: bool, // SILK_VRAMLOSS one-shot: fake a loss to exercise the rebuild path
	// The surface's size, kept here because layout still needs it while there
	// is no surface to ask.
	surface_px: (u32, u32),
	gl: bool, // born on the glutin GL path (X11): the one with a VT watcher and sentinels
	adapter_info: wgpu::AdapterInfo, // for the About dialog, which may open before a rebuild
	idle: IdleClock,
	conserve: Conserve,
	vt_heal: VtHeal,
}

// What a render entry point does, given whether the window was hidden at the
// last check and is now (see `State::freeze_sync`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
	Skip,    // nothing on screen: a frame now would bank the backlog into the ease
	CatchUp, // just shown again: one hard-cut frame
	Draw,
}

fn freeze_frame(was_hidden: bool, hidden: bool) -> Frame {
	if hidden {
		Frame::Skip
	} else if was_hidden {
		Frame::CatchUp
	} else {
		Frame::Draw
	}
}

// (window frame, menu bar) saved and shown after a bare-window toggle, `bare`
// being the new state. Coming back puts back only what is still off, so one
// switched on in the meantime stays on.
fn bare_chrome(bare: bool, saved: (bool, bool), now: (bool, bool)) -> ((bool, bool), (bool, bool)) {
	if bare {
		(now, (false, false))
	} else {
		(saved, (now.0 | saved.0, now.1 | saved.1))
	}
}

impl State {
	// Pixels reserved at the very top by the menu bar (0 when hidden).
	// Bar heights track the menu font's line height so they scale with font size.
	fn menu_bar_h(&self) -> f32 {
		self.text.ui_line_h + self.text.dip(MENU_BAR_VPAD)
	}
	// Measure the strip afresh: what each tab's label wants, what the least it
	// can be given is, which tabs that leaves on the page, and the widest label
	// form that fits the width each one ends up with.
	//
	// Kept rather than recomputed per call, because measuring every tab's label
	// on each mouse move would be paid for on each mouse move. The render pass
	// rebuilds unconditionally (a label changes when its shell does something);
	// everything else goes through `tab_layout`, which rebuilds only when one of
	// the inputs in `tab_layout_key` moved.
	fn rebuild_tab_layout(&mut self) {
		let total = self.surface_px.0 as f32;
		let scale = self.text.scale;
		// what a tab spends on itself rather than on its label
		let chrome = 2.0 * config::dip(TAB_TITLE_PAD, scale) + config::dip(TAB_CLOSE_W, scale);
		let attrs = crate::text::ui_attrs();
		let forms: Vec<Vec<String>> = (0..self.tabs.len())
			.map(|i| self.tab_label_forms(i))
			.collect();
		let mut demands = Vec::with_capacity(forms.len());
		for tab in &forms {
			let mut width_of =
				|form: Option<&String>| form.map_or(0.0, |s| self.text.measure_ui_text(s, &attrs));
			demands.push(crate::tabtitle::Demand {
				natural: width_of(tab.first()) + chrome,
				floor: width_of(tab.last()) + chrome,
			});
		}
		let floors: Vec<f32> = demands.iter().map(|d| d.floor).collect();
		// Bring the active tab onto the page when it CHANGES - and only then, so
		// a page the wheel moved to stays put. Driven from the change rather than
		// from each of the many places that set `tabs.active`, so no path misses
		// it. `tab_first` is otherwise only a preference, clamped on read, so
		// opening or closing a tab cannot strand it.
		if self.tab_followed != self.tabs.active {
			self.tab_followed = self.tabs.active;
			self.tab_first =
				crate::tabtitle::page_for(self.tab_first, self.tabs.active, &floors, total);
		}
		let first = crate::tabtitle::clamp_page(self.tab_first, &floors, total);
		let shown = crate::tabtitle::tabs_that_fit(total, &floors, first)
			.min(self.tabs.len().saturating_sub(first));
		let settings = config::settings();
		// The tab in front takes what the row can spare, so the strip has to know
		// which slot it is on this page - and nothing, when it is on another.
		let active_slot = self
			.tabs
			.active
			.checked_sub(first)
			.filter(|slot| *slot < shown);
		let widths = crate::tabtitle::widths(
			total,
			&demands[first..first + shown],
			settings.tab_regular_pct,
			settings.tab_max_pct,
			active_slot,
		);
		// The widest form that fits the space this tab ended up with, else the
		// shortest there is - which still names the shell, so it reads as a tab
		// even clipped.
		let labels = widths
			.iter()
			.enumerate()
			.map(|(slot, w)| {
				let title_w = tab_title_w(*w, scale);
				let tab = &forms[first + slot];
				tab.iter()
					.find(|form| self.text.measure_ui_text(form, &attrs) <= title_w)
					.or_else(|| tab.last())
					.cloned()
					.unwrap_or_default()
			})
			.collect();
		self.tab_layout = TabLayout {
			key: self.tab_layout_key(),
			first,
			widths,
			labels,
		};
	}

	// What the strip was measured from. A mouse move is not on the list, which
	// is the point of having one.
	fn tab_layout_key(&self) -> (u32, usize, usize, usize, u32) {
		(
			self.surface_px.0,
			self.tabs.len(),
			self.tabs.active,
			self.tab_first,
			self.text.scale.to_bits(),
		)
	}

	// The strip as drawn, measured again only if one of its inputs moved.
	fn tab_layout(&mut self) -> &TabLayout {
		if self.tab_layout.key != self.tab_layout_key() {
			self.rebuild_tab_layout();
		}
		&self.tab_layout
	}

	// Where tab `i` sits on the bar and how wide it is, or None when it is on
	// another page. Drawing and both hit tests read this one answer, or a click
	// sits on a different tab than the one under the pointer.
	fn tab_box(&mut self, i: usize) -> Option<(f32, f32)> {
		let layout = self.tab_layout();
		Some((layout.x(i)?, layout.w(i)?))
	}

	// Which tab a pointer at `x` is over - the inverse of `tab_box`, and the only
	// thing the two hit tests may use.
	fn tab_at(&mut self, x: f32) -> Option<usize> {
		self.tab_layout().at_x(x)
	}

	// The close button of tab `i`, if that tab is on the page.
	fn tab_close_box_at(&mut self, i: usize, bar_y: f32, tab_h: f32) -> Option<Rect> {
		let (x, w) = self.tab_box(i)?;
		Some(tab_close_box(x, w, bar_y, tab_h, self.text.scale))
	}

	// A wheel over the tab bar turns the page. Without it a tab past the edge
	// could only be reached from the keyboard or the Tabs menu.
	fn scroll_tab_strip(&mut self, lines: f32) {
		let first = self.tab_layout().first;
		let step = if lines > 0.0 {
			first.saturating_sub(1)
		} else {
			first.saturating_add(1)
		};
		if step != self.tab_first {
			self.tab_first = step;
			self.dirty = true;
		}
	}

	fn tab_bar_h(&self) -> f32 {
		self.text.ui_line_h + self.text.dip(TAB_BAR_VPAD)
	}
	fn menubar_h(&self) -> f32 {
		if self.menu_bar {
			self.menu_bar_h()
		} else {
			0.0
		}
	}

	// The tab bar shows for >1 tab always; for a single tab unless the user
	// opts out (hide_single_tab, View menu / config).
	fn tab_bar_visible(&self) -> bool {
		!self.bare && (self.tabs.len() > 1 || !config::settings().hide_single_tab)
	}

	// Everything above the panes: the menu bar when shown, then the tab strip when
	// visible. A row count asks for the cells below this, so the launch sizing and
	// save_window_size both have to account for it.
	fn chrome_h(&self) -> f32 {
		self.menubar_h()
			+ if self.tab_bar_visible() {
				self.tab_bar_h()
			} else {
				0.0
			}
	}

	fn area(&self) -> Rect {
		// Panes sit below the menu bar (always when shown) and the tab bar
		// (when visible), stacked in that order.
		let bar = self.chrome_h();
		Rect {
			x: 0.0,
			y: bar,
			w: self.surface_px.0 as f32,
			h: (self.surface_px.1 as f32 - bar).max(1.0),
		}
	}

	fn focus_at(&mut self, x: f32, y: f32) {
		if let Some(id) = self.tabs.cur().pane_at(x, y) {
			self.tabs.cur_mut().focused = id;
			self.update_title();
		}
	}

	// Point every pane at the pointer (only the one under it gets a position), so
	// the next build can look for a hyperlink there. Marking dirty on a pending
	// probe is what makes the underline appear at all - the frame that scans is
	// also the frame that draws the result - and it costs at most one frame per
	// cell crossed, never one per pixel.
	fn update_link_hover(&mut self, at: Option<(f32, f32)>) {
		if !config::settings().hyperlinks {
			return;
		}
		// An app watching the pointer owns it: no underline flickering through a
		// TUI that uses the mouse itself. This has to key on the app's MODE, not on
		// whether this particular event was reported - the report is throttled to
		// cell changes, so a pointer that settles inside one cell would slip
		// through and underline anyway. Shift is the local-action bypass, the same
		// one that already lets a selection through a tracking app.
		let shift = self.mods.shift_key();
		let over = at
			.and_then(|(x, y)| self.tabs.cur().pane_at(x, y))
			.filter(|id| {
				shift
					|| !self.tabs.cur().panes.get(id).is_some_and(|p| {
						p.mode
							.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG)
					})
			});
		let text = &self.text;
		let mut probing = false;
		for (id, p) in &mut self.tabs.cur_mut().panes {
			p.set_hover(at.filter(|_| over == Some(*id)), text);
			probing |= p.link_probing();
		}
		self.dirty |= probing;
	}

	// Whether the pointer is over an underlined link - the pane's own hover state,
	// so this is only the pointer shape's question. Anything that ACTS on a link
	// re-scans through link_at_pointer instead of trusting a frame-old answer.
	fn hovering_link(&self) -> bool {
		let (x, y) = self.mouse;
		self.tabs
			.cur()
			.pane_at(x, y)
			.and_then(|id| self.tabs.cur().panes.get(&id))
			.is_some_and(|p| p.link_hover.is_some())
	}

	// Fresh scan for the link under the pointer, with the pane it belongs to.
	fn link_at_pointer(&self) -> Option<(PaneId, crate::pane::LinkHit)> {
		let (x, y) = self.mouse;
		let id = self.tabs.cur().pane_at(x, y)?;
		let hit = self
			.tabs
			.cur()
			.panes
			.get(&id)?
			.link_at_px(x, y, &self.text)?;
		Some((id, hit))
	}

	// One owner for the pointer shape: a drag beats a divider, a divider beats a
	// link. Called from the pointer move AND from the frame, since a link found
	// under a pointer that has stopped moving still has to change the cursor.
	fn sync_cursor_icon(&mut self) {
		let (x, y) = self.mouse;
		let icon = if self.dragging_pane.is_some() {
			CursorIcon::Grabbing
		} else {
			match self
				.tabs
				.cur()
				.divider_at(x, y, self.area(), self.text.scale)
			{
				Some((_, Dir::Vertical)) => CursorIcon::ColResize,
				Some((_, Dir::Horizontal)) => CursorIcon::RowResize,
				None if self.hovering_link() => CursorIcon::Pointer,
				None => CursorIcon::Default,
			}
		};
		if icon != self.cursor_icon {
			self.window.set_cursor(icon);
			self.cursor_icon = icon;
		}
	}

	// Refresh every pane's scrollbar-hover flag from the pointer. Only the pane the
	// pointer is actually over can be hovered, so this also clears the one it just
	// left. Marks dirty only on a change - the fade is what needs the frames, and
	// this runs on every mouse move.
	fn update_bar_hover(&mut self, x: f32, y: f32) {
		let cfg = config::settings();
		if !cfg.scrollbar {
			return;
		}
		let over = self.tabs.cur().pane_at(x, y);
		let mut changed = false;
		let text = &self.text;
		for (id, p) in &mut self.tabs.cur_mut().panes {
			let near = over == Some(*id) && p.bar_near(x, y, text, &cfg);
			if p.bar_hover != near {
				p.bar_hover = near;
				changed = true;
			}
		}
		if changed {
			self.dirty = true;
		}
	}

	// Mouse reporting: forward a button press/release to the pane under the cursor
	// when the app has mouse tracking on (see `input::press_is_reported` for what
	// stays local). Returns true when the event was reported (and should not be
	// handled locally). Records the held button for drag + release.
	fn report_mouse_button(&mut self, button: MouseButton, state: ElementState) -> bool {
		let Some(btn) = input::mouse_btn_of(button) else {
			return false;
		};
		let (x, y) = self.mouse;
		if state == ElementState::Pressed {
			if !input::press_is_reported(btn, self.menu.is_some(), self.mods.shift_key()) {
				return false;
			}
			let cur = self.tabs.cur();
			let Some(id) = cur.pane_at(x, y) else {
				return false;
			};
			let Some(p) = cur.panes.get(&id) else {
				return false;
			};
			if !input::wants_mouse(p.mode) {
				return false;
			}
			let Some((col, row)) = p.screen_cell_at(x, y, &self.text) else {
				return false;
			};
			if let Some(seq) = input::mouse_report(p.mode, btn, true, false, col, row, self.mods) {
				p.write_input(seq);
			}
			self.mouse_btn = Some(btn);
			self.mouse_cell = Some((col, row));
			true
		} else {
			// only our business if we owned the matching press
			if self.mouse_btn.take().is_none() {
				return false;
			}
			let cur = self.tabs.cur();
			if let Some(p) = cur.pane_at(x, y).and_then(|id| cur.panes.get(&id)) {
				if input::wants_mouse(p.mode) {
					if let Some((col, row)) = p.screen_cell_at(x, y, &self.text) {
						if let Some(seq) =
							input::mouse_report(p.mode, btn, false, false, col, row, self.mods)
						{
							p.write_input(seq);
						}
					}
				}
			}
			self.mouse_cell = None;
			true
		}
	}

	// Mouse reporting: forward cursor motion when the app requests it - MOUSE_MOTION
	// (any move) or MOUSE_DRAG (only while a button is held). De-duped per cell so a
	// pixel jiggle inside one cell doesn't flood the PTY. Returns true when reported.
	fn report_mouse_motion(&mut self) -> bool {
		if self.mods.shift_key() {
			return false;
		}
		let (x, y) = self.mouse;
		let held = self.mouse_btn;
		let last = self.mouse_cell;
		let new_cell = {
			let cur = self.tabs.cur();
			let Some(id) = cur.pane_at(x, y) else {
				return false;
			};
			let Some(p) = cur.panes.get(&id) else {
				return false;
			};
			let motion = p.mode.contains(TermMode::MOUSE_MOTION);
			let drag = p.mode.contains(TermMode::MOUSE_DRAG) && held.is_some();
			if !(motion || drag) {
				return false;
			}
			let Some((col, row)) = p.screen_cell_at(x, y, &self.text) else {
				return false;
			};
			if last == Some((col, row)) {
				return false;
			}
			let btn = held.unwrap_or(input::MouseBtn::None);
			if let Some(seq) = input::mouse_report(p.mode, btn, true, true, col, row, self.mods) {
				p.write_input(seq);
			}
			(col, row)
		};
		self.mouse_cell = Some(new_cell);
		true
	}

	// Copy-output: when the focused pane's foreground command finishes, copy its
	// output text to the desktop clipboard. A pending capture only survives while
	// its pane stays the active copy target (window focused, tab active, pane
	// focused, trigger on) - anything else disarms it, so output that finished
	// while the user was elsewhere never copies late on refocus; only a command
	// launched after returning does. Runs every event-loop pass, and every way
	// eligibility can break is itself an event, so the disarm always comes before
	// a refocus could re-poll.
	fn poll_output_copy(&mut self) {
		let cur_focused = self.tabs.cur().focused;
		for pm in &mut self.tabs.list {
			for (id, pane) in &mut pm.panes {
				// ids are unique across tabs, so this is the active tab's pane too
				let in_use = *id == cur_focused;
				if !(input::copy_allowed(CopyFrom::Output, self.focused, in_use)
					&& pane.copy_output)
				{
					pane.disarm_capture();
				}
			}
		}
		let keep = self.focused.then_some(cur_focused);
		let Some(focused_id) = keep else {
			return;
		};
		let text = {
			let Some(pane) = self.tabs.cur_mut().panes.get_mut(&focused_id) else {
				return;
			};
			pane.poll_capture(CAPTURE_SETTLE)
		};
		if let Some(text) = text {
			self.clipboard.set_clipboard(text);
		}
	}

	// When the focused pane is armed for copy-output, the instant its settle timer
	// should fire, so an idle loop wakes to run the capture check.
	fn capture_wake(&self) -> Option<Instant> {
		if !self.focused {
			return None;
		}
		let focused_id = self.tabs.cur().focused;
		let p = self.tabs.cur().panes.get(&focused_id)?;
		p.copy_output
			.then(|| p.capture_deadline(CAPTURE_SETTLE))
			.flatten()
	}

	// Everything a tab could say, longest form first (see tabtitle). A `--title`
	// override is the whole answer; otherwise it is the shell's FRIENDLY name -
	// what the Shells list calls it, which is the name the user themselves gave
	// it - plus whatever that shell has to report: the command it is running,
	// the last one it ran, or, having run nothing at all, where it is.
	fn tab_label_forms(&mut self, index: usize) -> Vec<String> {
		// A rename in progress is what the tab says, so what is typed is what is
		// seen - and it is one form, never shortened, or the caret would sit off
		// the end of an abbreviated label.
		if let Some(edit) = &self.tab_edit {
			if edit.tab == index {
				return vec![edit.text.clone()];
			}
		}
		let Some(pm) = self.tabs.list.get_mut(index) else {
			return vec![config::APP_NAME.to_string()];
		};
		if let Some(title) = &pm.title_override {
			return vec![title.clone()];
		}
		// The focused pane's own title, plus the program the pane was started
		// with - that is what tells a console's own decoration apart from text
		// somebody chose.
		let focused_id = pm.focused;
		let (said, launched) = pm.panes.get(&focused_id).map_or_else(
			|| (String::new(), None),
			|pane| (pane.title.clone(), pane.launched().map(str::to_string)),
		);
		let (command, task, cwd) = pm.tab_facts();
		let settings = config::settings();
		let command_line = tab_command_line(command.as_deref());
		let friendly = crate::shells::friendly(&command_line, &settings.shells);
		let cwd = cwd.map(|dir| dir.to_string_lossy().into_owned());
		let home = config::home_dir().map(|dir| dir.to_string_lossy().into_owned());
		let task = match &task {
			crate::term::Task::Running(program) => Some(crate::tabtitle::Task::Running(program)),
			crate::term::Task::Last(program) => Some(crate::tabtitle::Task::Last(program)),
			crate::term::Task::Idle => None,
		};
		crate::tabtitle::label_forms(
			&friendly,
			crate::tabtitle::program_title(config::rights(), &said, launched.as_deref()),
			task,
			cwd.as_deref(),
			home.as_deref(),
			crate::tabtitle::Style::native(),
			crate::tabtitle::Parts {
				title: settings.tab_shows_title,
				shell: settings.tab_shows_shell,
				program: settings.tab_shows_program,
				directory: settings.tab_shows_directory,
			},
		)
	}

	// Start renaming tab `i` in place, seeded with what it says now and with all
	// of that selected, so the first thing typed replaces it.
	fn begin_tab_edit(&mut self, tab: usize) {
		let text = self
			.tabs
			.list
			.get(tab)
			.and_then(|pm| pm.title_override.clone())
			.unwrap_or_else(|| {
				self.tab_label_forms(tab)
					.into_iter()
					.next()
					.unwrap_or_default()
			});
		self.tab_edit = Some(TabEdit {
			tab,
			caret: text.len(),
			anchor: 0,
			text,
		});
		self.tab_tip = None;
		self.dirty = true;
	}

	// Take what was typed. A title matching what the tab would have said anyway
	// is dropped rather than frozen, and so is a blank one - either way the tab
	// goes back to naming itself.
	fn commit_tab_edit(&mut self) {
		let Some(edit) = self.tab_edit.take() else {
			return;
		};
		// What this tab would say with no title of its own - cleared first, since
		// that is what the label is built from.
		if let Some(pm) = self.tabs.list.get_mut(edit.tab) {
			pm.title_override = None;
		}
		let auto = self
			.tab_label_forms(edit.tab)
			.into_iter()
			.next()
			.unwrap_or_default();
		if let Some(pm) = self.tabs.list.get_mut(edit.tab) {
			pm.title_override = typed_title(edit.text, &auto);
		}
		self.update_title();
		self.dirty = true;
	}

	// Change the edit in place and redraw. Every key that types into a tab title
	// goes through here so no path forgets the redraw.
	fn edit_tab(&mut self, change: impl FnOnce(&mut TabEdit)) {
		if let Some(edit) = self.tab_edit.as_mut() {
			change(edit);
			self.dirty = true;
		}
	}

	fn cancel_tab_edit(&mut self) {
		if self.tab_edit.take().is_some() {
			self.dirty = true;
		}
	}

	// Which tab the pointer is over, and since when. Anything the pointer is
	// already busy with - a drag, an open menu - owns it instead, so no tip
	// appears underneath one.
	fn note_tab_hover(&mut self, x: f32, y: f32) {
		let busy = self.bar_dragging.is_some()
			|| self.map_dragging.is_some()
			|| self.dragging_pane.is_some()
			|| self.tab_edit.is_some()
			|| self.menu.is_some()
			|| self.bar_open.is_some()
			|| self.tab_close_arm.is_some();
		let bar_y = self.menubar_h();
		let over = if busy || !self.tab_bar_visible() || y < bar_y || y >= bar_y + self.tab_bar_h()
		{
			None
		} else {
			self.tab_at(x)
		};
		if self.tab_hover.point_at(over) && self.tab_tip.take().is_some() {
			self.dirty = true;
		}
	}

	// Every tip's clock in one place: raise one whose pointer has rested, and
	// keep an open tab tip's contents current. Returns true when the frame has
	// to be redrawn.
	fn update_tips(&mut self) -> bool {
		self.menu_tip.point_at(self.menu_tip_target());
		let up = self.menu_tip.ripe();
		let menu_changed = up != self.menu_tip_up;
		self.menu_tip_up = up;
		self.update_tab_tip() || menu_changed
	}

	// A drag-selection held past the top or bottom of its pane crawls the view
	// that way and keeps extending, so a selection can run past what is on screen.
	// Returns true while it is scrolling, which keeps frames coming - the pointer
	// is stationary, so nothing else would ask for one.
	fn autoscroll_selection(&mut self, dt: f32) -> bool {
		let Some(id) = self.selecting else {
			self.select_edge_held = 0.0;
			return false;
		};
		let (x, y) = self.mouse;
		let cell_h = self.text.cell_h;
		let held = self.select_edge_held;
		let Some(pane) = self.tabs.cur_mut().panes.get_mut(&id) else {
			return false;
		};
		let rate =
			crate::pane::edge_scroll_rate(y, pane.rect.y, pane.rect.y + pane.rect.h, cell_h, held);
		if rate == 0.0 {
			self.select_edge_held = 0.0;
			return false;
		}
		self.select_edge_held = held + dt;
		let was = pane.scroll.target_lines();
		pane.scroll.wheel(rate * dt);
		let moved = pane.scroll.target_lines() != was;
		if moved {
			pane.poke_scrollbar();
		}
		// The pointer sits still while the content moves under it, so the far end
		// of the selection has to be re-read against the rows now on screen.
		let (point, side) = pane.point_clamped(x, y, &self.text);
		pane.update_selection(point, side);
		// Pinned at either end there is nothing left to reveal, so don't ask for
		// another frame - held past the bottom, that would be a spin.
		moved
	}

	// When the loop next has to wake for any tip.
	fn tip_wake(&self) -> Option<Instant> {
		match (self.tab_tip_wake(), self.menu_tip.wake()) {
			(Some(a), Some(b)) => Some(a.min(b)),
			(only, None) | (None, only) => only,
		}
	}

	// Bring the tab tip up once the pointer has rested, and keep what it says
	// current while it is up. Returns true when the frame has to be redrawn.
	fn update_tab_tip(&mut self) -> bool {
		let limit =
			Duration::try_from_secs_f32(config::settings().tab_tip_max_s).unwrap_or_default();
		let Some(tab) = self.tab_hover.ripe_for(limit) else {
			return self.tab_hover.wake().is_none() && self.tab_tip.take().is_some();
		};
		let now = Instant::now();
		let stale = self
			.tab_tip
			.as_ref()
			.is_none_or(|tip| tip.tab != tab || now.duration_since(tip.built) >= TAB_TIP_REFRESH);
		if !stale {
			return false;
		}
		let lines = self.tab_tip_lines(tab);
		let changed = self
			.tab_tip
			.as_ref()
			.is_none_or(|tip| tip.tab != tab || tip.lines != lines);
		self.tab_tip = Some(TabTip {
			tab,
			lines,
			built: now,
		});
		changed
	}

	// The menu row a tip would describe: the innermost open popup that the
	// pointer is actually on (a parent keeps its highlight on the row its
	// submenu hangs off, so the deepest hovered one is the right answer), and
	// only when that row has something to say.
	fn menu_tip_target(&self) -> Option<(usize, usize)> {
		let root = self.menu.as_ref()?;
		let (depth, menu, row) = root
			.chain()
			.iter()
			.enumerate()
			.filter_map(|(depth, menu)| menu.hover.map(|row| (depth, *menu, row)))
			.next_back()?;
		let help = match menu.entries.get(row)? {
			Entry::Item { action, .. } => action.help(),
			_ => "",
		};
		(!help.is_empty()).then_some((depth, row))
	}

	// The menu tip's box and its wrapped lines, once the pointer has rested on a
	// row that has a tip. It stands beside the popup rather than under the row,
	// so the rows being chosen between stay readable.
	fn menu_tip_layout(&mut self) -> Option<(Rect, Vec<(f32, f32, String)>)> {
		let (depth, row) = self.menu_tip.ripe()?;
		let (anchor, help) = {
			let menu = *self.menu.as_ref()?.chain().get(depth)?;
			let help = match menu.entries.get(row)? {
				Entry::Item { action, .. } => action.help(),
				_ => "",
			};
			let anchor = Rect {
				x: menu.x,
				y: menu.row_top(row),
				w: menu.w,
				h: menu.item_h,
			};
			(anchor, help)
		};
		if help.is_empty() {
			return None;
		}
		let attrs = crate::text::ui_attrs();
		let pad = self.text.dip(MENU_TIP_PAD);
		let line_h = self.text.ui_line_h;
		let budget = self.text.dip(MENU_TIP_MAX_W);
		let lines = crate::tip::wrap(help, budget, |line| self.text.measure_ui_text(line, &attrs));
		let text_w = lines.iter().fold(0.0f32, |widest, line| {
			widest.max(self.text.measure_ui_text(line, &attrs))
		});
		let w = text_w + 2.0 * pad;
		let h = line_h * lines.len() as f32 + 2.0 * pad;
		let win = (self.surface_px.0 as f32, self.surface_px.1 as f32);
		let (x, y) = crate::tip::beside(anchor, (w, h), win, self.text.dip(MENU_TIP_GAP), pad);
		let placed = lines
			.into_iter()
			.enumerate()
			.map(|(i, line)| (x + pad, y + pad + line_h * i as f32, line))
			.collect();
		Some((Rect { x, y, w, h }, placed))
	}

	// The benchmark's banner: one box in the middle of the window, over a dimmed
	// screen. It is what makes the run modal - the window behind it keeps drawing
	// (that is the thing being timed, and it is worth seeing) but takes no input
	// while it is up.
	fn bench_layout(&mut self) -> Option<(Rect, Vec<(f32, f32, String)>)> {
		self.bench_banner.as_ref()?;
		let lines = if self.bench_stalled {
			BENCH_STALLED_LINES
		} else {
			bench_banner_lines(self.bench_kept.as_ref())
		};
		let attrs = crate::text::ui_attrs();
		let pad = self.text.dip(BENCH_BANNER_PAD);
		let line_h = self.text.ui_line_h;
		let text_w = lines.iter().fold(0.0f32, |widest, line| {
			widest.max(self.text.measure_ui_text(line, &attrs))
		});
		let w = text_w + 2.0 * pad;
		let h = line_h * lines.len() as f32 + 2.0 * pad;
		let (win_w, win_h) = (self.surface_px.0 as f32, self.surface_px.1 as f32);
		let (x, y) = (
			((win_w - w) / 2.0).max(0.0).round(),
			((win_h - h) / 2.0).max(0.0).round(),
		);
		let placed = lines
			.iter()
			.enumerate()
			.map(|(i, line)| {
				let left = x + (w - self.text.measure_ui_text(line, &attrs)) / 2.0;
				(
					left.round(),
					y + pad + line_h * i as f32,
					(*line).to_string(),
				)
			})
			.collect();
		Some((Rect { x, y, w, h }, placed))
	}

	// When the loop next has to wake for the tip - to raise one whose pointer has
	// rested, or to re-read one that is already up (its clock ticks).
	fn tab_tip_wake(&self) -> Option<Instant> {
		match &self.tab_tip {
			Some(tip) => Some(tip.built + TAB_TIP_REFRESH),
			None => self.tab_hover.wake(),
		}
	}

	// What a tip says, as key/value pairs padded to one column (tabtitle::tip_lines).
	// The path is shown WHOLE here - the tab is where it gets shortened, and the tip
	// is the place to look when the short form was not enough. A value that carries
	// a space or a quote is quoted, so its edges are never in doubt.
	fn tab_tip_lines(&mut self, index: usize) -> Vec<String> {
		let Some(pm) = self.tabs.list.get_mut(index) else {
			return Vec::new();
		};
		let created = pm.created;
		let override_title = pm.title_override.clone();
		let focused_id = pm.focused;
		let (said, launched) = pm.panes.get(&focused_id).map_or_else(
			|| (String::new(), None),
			|pane| (pane.title.clone(), pane.launched().map(str::to_string)),
		);
		let (command, task, cwd) = pm.tab_facts();
		let settings = config::settings();
		let command_line = tab_command_line(command.as_deref());
		let quoted = crate::tabtitle::tip_value;
		let mut rows: Vec<(&str, String)> = Vec::new();
		if let Some(title) = override_title {
			rows.push(("Tab title", quoted(&title)));
		}
		if let Some(said) =
			crate::tabtitle::program_title(config::rights(), &said, launched.as_deref())
		{
			rows.push(("Program title", quoted(said)));
		}
		rows.push((
			"Shell name",
			quoted(&crate::shells::friendly(&command_line, &settings.shells)),
		));
		if !command_line.is_empty() {
			rows.push(("Shell command", quoted(&command_line)));
		}
		// Only what is running NOW. A tab already says so itself, but it says it in
		// the width it has left; the tip has the whole name.
		if let crate::term::Task::Running(program) = task {
			rows.push(("Running", quoted(&program)));
		}
		rows.push((
			"Current path",
			cwd.map_or_else(
				// not a value, so it takes no quotes - a directory called
				// "(not reported)" is not what this line is saying
				|| "(not reported)".to_string(),
				|dir| {
					quoted(
						&crate::tabtitle::path_forms(
							&dir.to_string_lossy(),
							None,
							crate::tabtitle::Style::native(),
						)
						.into_iter()
						.next()
						.unwrap_or_default(),
					)
				},
			),
		));
		// a clock reading, not a value either
		rows.push((
			"Open",
			crate::tabtitle::elapsed(created.elapsed().as_secs()),
		));
		crate::tabtitle::tip_lines(&rows)
	}

	// The tip's box, and where each of its lines sits inside it. Measured in the
	// TERMINAL font, which is the one thing in the chrome that is: the lines are a
	// key/value table padded with spaces, and spaces align nothing in a
	// proportional face. The box fits the longest line rather than guessing; it
	// hangs off its own TAB rather than off the pointer, so it does not jitter as
	// the pointer moves about inside one, and it is pushed back inside the window
	// rather than being allowed to run off the right edge.
	fn tab_tip_layout(&mut self) -> Option<(Rect, Vec<(f32, f32, String)>)> {
		let (tab, lines) = {
			let tip = self.tab_tip.as_ref()?;
			(tip.tab, tip.lines.clone())
		};
		if lines.is_empty() {
			return None;
		}
		let text_w = lines.iter().fold(0.0f32, |widest, line| {
			widest.max(self.text.measure_mono_text(line))
		});
		let pad = self.text.dip(TAB_TIP_PAD);
		let line_h = self.text.cell_h;
		let w = text_w + 2.0 * pad;
		let h = line_h * lines.len() as f32 + 2.0 * pad;
		let win_w = self.surface_px.0 as f32;
		// A tab paged off the strip while its tip was up takes the tip with it -
		// a tip hanging off nothing would sit at the bar's left end, pointing at
		// whichever tab happened to be there.
		let x = self.tab_box(tab)?.0.min((win_w - w).max(0.0)).max(0.0);
		let y = self.menubar_h() + self.tab_bar_h() + self.text.dip(TAB_TIP_GAP);
		let placed = lines
			.into_iter()
			.enumerate()
			.map(|(i, line)| (x + pad, y + pad + line_h * i as f32, line))
			.collect();
		Some((Rect { x, y, w, h }, placed))
	}

	// The active tab's title, in full - the window title has the whole title bar
	// and the OS elides it itself.
	fn active_tab_title(&mut self) -> String {
		self.tab_label_forms(self.tabs.active)
			.into_iter()
			.next()
			.unwrap_or_else(|| config::APP_NAME.to_string())
	}

	// What the window title says after the app name - see tabtitle::window_suffix
	// for the order the three sources come in.
	fn title_suffix(&mut self) -> Option<String> {
		let typed = self.tabs.cur().title_override.clone();
		let program = self.pane_title();
		let pm = self.tabs.cur();
		let launched = pm
			.panes
			.get(&pm.focused)
			.and_then(|pane| pane.launched())
			.map(str::to_string);
		crate::tabtitle::window_suffix(
			config::rights(),
			typed.as_deref(),
			program.as_deref(),
			launched.as_deref(),
			config::settings().title_shows_tab,
			|| self.active_tab_title(),
		)
	}

	// The title the focused pane's program asked for, if it asked for one.
	fn pane_title(&self) -> Option<String> {
		let pm = self.tabs.cur();
		let title = &pm.panes.get(&pm.focused)?.title;
		(!title.trim().is_empty()).then(|| title.clone())
	}

	// The window title (taskbar / alt-tab): a CLI --title override verbatim, else
	// the app name (with the dogfood build when this is one) and whatever the
	// active tab has to say. Called on tab/focus change and each rendered frame;
	// set_title only fires when the string actually changed (avoids WM flicker).
	fn update_title(&mut self) {
		let custom = self.win_title.clone();
		// A --title is the whole answer, so nothing else is worked out.
		let suffix = match custom {
			Some(_) => None,
			None => self.title_suffix(),
		};
		let title = crate::tabtitle::window_title(
			config::rights(),
			custom.as_deref(),
			&config::title_prefix(),
			suffix.as_deref(),
		);
		let title = crate::tabtitle::with_note(title, self.conserve.note(Instant::now()));
		if title != self.last_win_title {
			self.window.set_title(&title);
			self.last_win_title = title;
		}
	}

	// Effective window opacity: a CLI --background-opacity override for this
	// window, else the configured value.
	fn opacity(&self) -> f32 {
		self.win_opacity
			.unwrap_or_else(|| config::settings().opacity)
	}

	fn open_menu(&mut self, target: PaneId, mx: f32, my: f32) {
		let p = self.tabs.cur().panes.get(&target);
		let read_only = p.is_some_and(|p| p.read_only);
		let copy_select = p.is_some_and(|p| p.copy_select);
		let copy_output = p.is_some_and(|p| p.copy_output);
		self.menu_link = self.link_at_pointer().map(|(_, link)| link.url);
		let on = CtxState {
			link: self.menu_link.is_some(),
			read_only,
			copy_select,
			copy_output,
			menu_bar: self.menu_bar,
			next_wallpaper: self.can_rotate(),
		};
		let entries = context_menu_items(on, &config::settings().shells);
		self.bar_open = None;
		self.popup(target, entries, mx, my);
	}

	// Build and place a dropdown/context popup, clamped on-screen.
	fn popup(&mut self, target: PaneId, entries: Vec<Entry>, mx: f32, my: f32) {
		self.menu = Some(self.build_popup(target, entries, mx, my));
	}

	// Lay one popup out at (mx, my), clamped on-screen. Width is the widest
	// (proportional) label plus the checkmark gutter, the padding, and - only
	// where a row opens a submenu - the column its arrow sits in. Shared by the
	// menu bar, the right-click menu and the submenus, so all of them size and
	// clamp alike.
	fn build_popup(
		&mut self,
		target: PaneId,
		entries: Vec<Entry>,
		mx: f32,
		my: f32,
	) -> ContextMenu {
		debug_assert!(
			accel_clash(&entries).is_none(),
			"two rows of one menu claim the accelerator {:?}",
			accel_clash(&entries)
		);
		let attrs = crate::text::ui_attrs();
		let mut max_label_w: f32 = 0.0;
		let mut any_sub = false;
		for entry in &entries {
			if let Some(label) = entry_label(entry) {
				max_label_w = max_label_w.max(self.text.measure_ui_text(label, &attrs));
			}
			any_sub |= matches!(entry, Entry::Sub { .. });
		}
		let arrow_col = if any_sub {
			self.text.dip(config::MENU_SUB_ARROW)
		} else {
			0.0
		};
		let w = self.text.dip(config::MENU_GUTTER)
			+ max_label_w
			+ arrow_col
			+ self.text.dip(config::MENU_PAD_X) * 2.0;
		let item_h = self.text.ui_line_h;
		let (pad_y, sep_h) = menu_metrics(self.text.scale);
		let menu = ContextMenu {
			x: mx,
			y: my,
			w,
			item_h,
			pad_y,
			sep_h,
			target,
			entries,
			hover: None,
			sub: None,
		};
		let sw = self.surface_px.0 as f32;
		let sh = self.surface_px.1 as f32;
		let x = mx.min((sw - w).max(0.0));
		let y = my.min((sh - menu.height()).max(0.0));
		ContextMenu { x, y, ..menu }
	}

	// Open the submenu on row `row` of the open popup, or close whatever was
	// standing open if that row does not have one.
	//
	// It goes to the RIGHT of the parent, never overlapping it, with its first
	// row lined up on the parent row - which is what lets the pointer rule stay
	// as simple as it is: moving right off the row leaves the parent entirely,
	// so nothing else can claim the hover on the way in. It flips to the left
	// only when there is no room on the right.
	fn open_submenu(&mut self, row: usize) {
		let Some(menu) = self.menu.as_ref() else {
			return;
		};
		let Some(Entry::Sub { items, .. }) = menu.entries.get(row) else {
			if let Some(menu) = self.menu.as_mut() {
				menu.sub = None;
			}
			return;
		};
		let items = items.clone();
		let (px, pw, top, pad_y) = (menu.x, menu.w, menu.row_top(row), menu.pad_y);
		// measured against a provisional build, since the width is what decides
		// which side it goes on
		let mut popup = self.build_popup(menu.target, items, px + pw, top - pad_y);
		if px + pw + popup.w > self.surface_px.0 as f32 {
			popup = self.build_popup(
				popup.target,
				popup.entries,
				(px - popup.w).max(0.0),
				top - pad_y,
			);
		}
		if let Some(menu) = self.menu.as_mut() {
			menu.sub = Some(Box::new(popup));
		}
	}

	// Point the open menu at (x, y). The innermost popup under the pointer takes
	// the highlight, and moving onto (or off) a submenu row opens (or closes) its
	// popup. Returns whether anything moved.
	fn menu_hover(&mut self, x: f32, y: f32) -> bool {
		let Some(menu) = self.menu.as_mut() else {
			return false;
		};
		// A submenu takes the pointer first - it overlaps no parent row, so being
		// inside it is unambiguous, and the parent keeps its highlight on the row
		// the submenu belongs to.
		if let Some(sub) = menu.sub.as_mut() {
			if sub.hit(x, y) {
				let hovered = sub.item_at(x, y);
				let moved = hovered != sub.hover;
				sub.hover = hovered;
				return moved;
			}
		}
		let hovered = menu.item_at(x, y);
		if hovered == menu.hover {
			return false;
		}
		menu.hover = hovered;
		let row = hovered.filter(|&i| matches!(menu.entries[i], Entry::Sub { .. }));
		match row {
			Some(row) => self.open_submenu(row),
			None => {
				if let Some(menu) = self.menu.as_mut() {
					menu.sub = None;
				}
			}
		}
		true
	}

	// Act on a click at (x, y) with a menu open: an item fires and closes the
	// whole stack, a submenu row opens its popup and leaves everything standing,
	// and anything else dismisses.
	fn menu_click(&mut self, x: f32, y: f32, proxy: &EventLoopProxy<UserEvent>) {
		let Some(menu) = self.menu.as_ref() else {
			return;
		};
		let target = menu.target;
		let chain = menu.chain();
		// innermost first: a submenu is drawn over whatever it covers
		let found = chain
			.iter()
			.enumerate()
			.rev()
			.find_map(|(depth, popup)| popup.item_at(x, y).map(|row| (depth, row)));
		let Some((depth, row)) = found else {
			self.menu = None;
			self.bar_open = None;
			return;
		};
		let entry = chain[depth].entries[row].clone();
		match entry {
			// only the root popup carries submenus, so a deeper one cannot open
			Entry::Sub { .. } => {
				if depth == 0 {
					self.open_submenu(row);
				}
			}
			Entry::Item { action, .. } => {
				self.menu = None;
				self.bar_open = None;
				self.apply_menu(action, target, proxy);
			}
			Entry::Sep => {}
		}
	}

	// Fire row `row` of the innermost open popup, the way Enter and an
	// accelerator letter do. A submenu row opens and takes the highlight to its
	// first item instead of acting.
	fn menu_activate(&mut self, row: usize, proxy: &EventLoopProxy<UserEvent>) {
		let Some(menu) = self.menu.as_ref() else {
			return;
		};
		let target = menu.target;
		let chain = menu.chain();
		let depth = chain.len() - 1;
		let Some(entry) = chain[depth].entries.get(row).cloned() else {
			return;
		};
		match entry {
			Entry::Sub { .. } if depth == 0 => {
				self.open_submenu(row);
				if let Some(sub) = self.menu.as_mut().and_then(|menu| menu.sub.as_mut()) {
					sub.hover = sub.step(None, 1);
				}
			}
			Entry::Item { action, .. } => {
				self.menu = None;
				self.bar_open = None;
				self.apply_menu(action, target, proxy);
			}
			_ => {}
		}
	}

	// The popup the keyboard is on: the innermost one standing open.
	fn menu_inner(&mut self) -> Option<&mut ContextMenu> {
		self.menu.as_mut().map(ContextMenu::inner_mut)
	}

	// The highlighted row of the open popup when it is one that opens a submenu
	// and has not opened it yet - i.e. what Right arrow would enter.
	fn submenu_row(&self) -> Option<usize> {
		let menu = self.menu.as_ref()?;
		if menu.sub.is_some() {
			return None;
		}
		menu.hover
			.filter(|&row| matches!(menu.entries[row], Entry::Sub { .. }))
	}

	// Close the open submenu; returns false when there was none, so the caller
	// can fall through to whatever it does otherwise.
	fn close_submenu(&mut self) -> bool {
		let Some(menu) = self.menu.as_mut() else {
			return false;
		};
		if menu.sub.is_none() {
			return false;
		}
		menu.sub = None;
		true
	}

	// The dropdown entries for top-level menu-bar entry `idx` (File/Edit/...).
	fn bar_menu_items(&self, idx: usize) -> Vec<Entry> {
		let p = self.tabs.cur().panes.get(&self.tabs.cur().focused);
		let read_only = p.is_some_and(|p| p.read_only);
		let copy_select = p.is_some_and(|p| p.copy_select);
		let copy_output = p.is_some_and(|p| p.copy_output);
		match idx {
			0 => file_menu_items(),
			1 => edit_menu_items(copy_select, copy_output),
			2 => view_menu_items(ViewState {
				read_only,
				fullscreen: self.window.fullscreen().is_some(),
				window_frame: self.decorated,
				menu_bar: self.menu_bar,
				tab_strip: !config::settings().hide_single_tab,
				minimap: config::settings().minimap,
				bare: self.bare,
				remote: config::settings().remote_override,
				next_wallpaper: self.can_rotate(),
			}),
			3 => tabs_menu_items(&config::settings().shells),
			4 => panes_menu_items(&config::settings().shells),
			_ => help_menu_items(),
		}
	}

	// Open the dropdown for top-level menu `idx`, anchored under its title.
	fn open_bar_menu(&mut self, idx: usize) {
		let items = self.bar_menu_items(idx);
		let x = self.menubar_layout().get(idx).map_or(0.0, |&(x, _)| x);
		let target = self.tabs.cur().focused;
		let bar_h = self.menu_bar_h();
		self.popup(target, items, x, bar_h);
		self.bar_open = Some(idx);
	}

	// Per-title (x_left, width) layout of the menu bar, used for drawing and
	// hit-testing so they can't disagree. Titles use the proportional font.
	fn menubar_layout(&mut self) -> Vec<(f32, f32)> {
		let attrs = crate::text::ui_attrs();
		let mut x = 0.0;
		let mut out = Vec::with_capacity(MENU_BAR.len());
		for title in MENU_BAR {
			let w = self.text.measure_ui_text(title, &attrs) + self.text.dip(MENU_BAR_PAD) * 2.0;
			out.push((x, w));
			x += w;
		}
		out
	}

	fn menubar_hit(&mut self, mx: f32) -> Option<usize> {
		self.menubar_layout()
			.iter()
			.position(|&(x, w)| mx >= x && mx < x + w)
	}

	// The "Copy on [ ] select [ ] output" pair on the right of the menu bar. The
	// user has to be able to see when the focused pane is auto-copying, so the
	// cluster sheds parts rather than shrinking: the lead-in goes first, then the
	// two words, and only when even the boxes cannot clear the menu titles does
	// the whole thing go (None). Overlapping text says less about the copy state
	// than a clean absence does. It comes back on its own as the window widens.
	// label_x/label_w index-match COPYBOX_LABELS.
	fn copybox_layout(&mut self) -> Option<CopyBoxes> {
		let titles_right =
			self.menubar_layout().last().map_or(0.0, |&(x, w)| x + w) + self.text.dip(MENU_BAR_PAD);
		let attrs = crate::text::ui_attrs();
		let mut label_w = [0.0f32; 3];
		for (w, label) in label_w.iter_mut().zip(COPYBOX_LABELS) {
			*w = self.text.measure_ui_text(label, &attrs);
		}
		let box_sz = (self.text.ui_line_h * 0.6).round();
		let metrics = CopyMetrics {
			right: self.surface_px.0 as f32 - self.text.dip(MENU_BAR_PAD),
			label_w,
			box_sz,
			box_y: (self.menu_bar_h() - box_sz) / 2.0,
			box_gap: self.text.dip(COPYBOX_BOX_GAP),
			pair_gap: self.text.dip(COPYBOX_PAIR_GAP),
			lead_gap: self.text.dip(COPYBOX_LEAD_GAP),
		};
		copybox_fit(&metrics, titles_right)
	}

	// Which copy-mode checkbox (the square or its word) a menu-bar click hit.
	fn copybox_hit(&mut self, mx: f32) -> Option<CopyKind> {
		let cb = self.copybox_layout()?;
		for (i, kind) in [CopyKind::Select, CopyKind::Output].into_iter().enumerate() {
			let (left, right) = cb.hit_range(i);
			if mx >= left && mx <= right {
				return Some(kind);
			}
		}
		None
	}

	// Flip one of a pane's two auto-copy triggers. The two are independent and can
	// both be on; nothing else is touched (other panes/tabs/windows keep theirs -
	// only the focused pane of the active tab actually copies, gated at copy time).
	// A toggle from a context menu on an unfocused pane focuses it so the menu-bar
	// checkboxes reflect the pane just changed.
	fn toggle_copy(&mut self, target: PaneId, kind: CopyKind) {
		let Some(p) = self.tabs.find_pane_mut(target) else {
			return;
		};
		let now = !p.copy_enabled(kind);
		p.set_copy(kind, now);
		if self.tabs.cur().panes.contains_key(&target) {
			self.tabs.cur_mut().focused = target;
		}
	}

	// Request the About window. App opens it (window creation needs the event
	// loop); the old in-surface overlay path is no longer used.
	fn open_about(&mut self) {
		self.pending_about = true;
		self.menu = None;
		self.bar_open = None;
	}

	fn apply_menu(
		&mut self,
		action: MenuAction,
		target: PaneId,
		proxy: &EventLoopProxy<UserEvent>,
	) {
		let area = self.area();
		match action {
			// the URL was captured when the menu opened - the output under it may
			// have scrolled away since
			MenuAction::OpenLink => {
				if let Some(url) = self.menu_link.clone() {
					open_link(&url);
				}
			}
			MenuAction::CopyLink => {
				if let Some(url) = self.menu_link.clone() {
					self.clipboard.set_clipboard(url);
				}
			}
			MenuAction::Copy => {
				if let Some(text) = self
					.tabs
					.cur()
					.panes
					.get(&target)
					.and_then(super::pane::Pane::selection_text)
				{
					self.clipboard.set_clipboard(text);
				}
			}
			MenuAction::Paste => {
				if let Some(text) = self.clipboard.get_clipboard() {
					if let Some(p) = self.tabs.cur_mut().panes.get_mut(&target) {
						p.paste(&text);
					}
				}
			}
			MenuAction::PasteSelection => {
				if let Some(text) = self.clipboard.get_primary() {
					if let Some(p) = self.tabs.cur_mut().panes.get_mut(&target) {
						p.paste(&text);
					}
				}
			}
			MenuAction::ToggleCopySelect => self.toggle_copy(target, CopyKind::Select),
			MenuAction::ToggleCopyOutput => self.toggle_copy(target, CopyKind::Output),
			MenuAction::ToggleReadOnly => {
				if let Some(p) = self.tabs.cur_mut().panes.get_mut(&target) {
					p.read_only = !p.read_only;
				}
			}
			MenuAction::SplitVertical => {
				self.tabs
					.cur_mut()
					.split(&mut self.text, proxy, target, Dir::Vertical, area);
			}
			MenuAction::SplitHorizontal => {
				self.tabs
					.cur_mut()
					.split(&mut self.text, proxy, target, Dir::Horizontal, area);
			}
			MenuAction::SplitShell(dir, index) => {
				if let Some(cmd) = shell_argv(index) {
					self.tabs
						.cur_mut()
						.split_with(&mut self.text, proxy, target, dir, cmd, area);
				}
			}
			MenuAction::Close => {
				let scope = {
					let cur = self.tabs.cur();
					close_scope(
						cur.panes.contains_key(&target),
						cur.panes.len(),
						self.tabs.len(),
					)
				};
				match scope {
					CloseScope::Pane => {
						self.tabs.cur_mut().close(&mut self.text, target, area);
					}
					// last pane in this tab -> the tab; last pane of the last tab
					// -> the window
					CloseScope::Tab => self.close_tab(),
					CloseScope::Window => self.quit = true,
					CloseScope::Nothing => {}
				}
			}
			MenuAction::NewTab => self.new_tab(proxy),
			MenuAction::NewTabShell(index) => self.new_tab_with(proxy, shell_argv(index)),
			MenuAction::CloseTab => self.close_tab(),
			MenuAction::FontBigger => self.font_zoom(1),
			MenuAction::FontSmaller => self.font_zoom(-1),
			MenuAction::FontReset => self.font_zoom_reset(),
			MenuAction::ToggleFullscreen => self.toggle_fullscreen(),
			MenuAction::ToggleFrame => {
				self.decorated = !self.decorated;
				self.window.set_decorations(self.decorated);
			}
			MenuAction::ToggleMenuBar => {
				self.menu_bar = !self.menu_bar;
				self.relayout_all();
			}
			MenuAction::ToggleBare => self.toggle_bare(),
			MenuAction::ToggleRemote => self.toggle_remote(),
			MenuAction::ToggleMinimap => {
				let orig = (*config::settings()).clone();
				let mut new = orig.clone();
				new.minimap = !new.minimap;
				let _ = config::persist(&orig, &new);
				config::update(new);
				self.relayout_all();
			}
			MenuAction::ToggleSingleTab => {
				let orig = (*config::settings()).clone();
				let mut new = orig.clone();
				new.hide_single_tab = !new.hide_single_tab;
				// config open elsewhere -> persist skips; the session keeps the value
				let _ = config::persist(&orig, &new);
				config::update(new);
				self.relayout_all();
			}
			MenuAction::NextWallpaper => self.advance_wallpaper(),
			MenuAction::ReloadConfig => self.reload_config(),
			MenuAction::Settings => self.open_settings(),
			MenuAction::About => self.open_about(),
			MenuAction::Quit => self.quit = true,
		}
		self.update_title();
	}

	// relayout every tab (not just the active one) - needed when the tab bar
	// appears/disappears (1<->2 tabs) and the pane area changes.
	fn relayout_all(&mut self) {
		let area = self.area();
		for pm in &mut self.tabs.list {
			pm.relayout(&mut self.text, area);
		}
	}

	// Track the live window size as columns/rows so "remember last size" can
	// restore it next launch. Kept separate from the user's defined columns/rows
	// (unchecking the option reverts to those). The inverse of the launch sizing.
	fn save_window_size(&mut self, w: u32, h: u32) {
		// skip the creation/programmatic resizes that fire before the first frame,
		// so they don't clobber the remembered size with the launch size
		if !remember_resize(
			self.size_tracked,
			self.window.fullscreen().is_some(),
			self.window.is_maximized(),
		) {
			return;
		}
		let px_to_cells = |px: f32, cell: f32, chrome: f32| {
			(((px - 2.0 * self.text.margin - chrome) / cell).floor() as i64).max(1) as usize
		};
		let cols = px_to_cells(w as f32, self.text.cell_w, 0.0);
		let rows = px_to_cells(h as f32, self.text.cell_h, self.chrome_h());
		// debounce: an interactive drag fires many Resized events; writing
		// config.shcl on each would be dozens of file writes/sec. Persist in
		// flush_window_size once the size has held (or on exit).
		self.pending_size = Some((cols, rows));
		self.pending_size_at = Instant::now();
	}

	fn flush_window_size(&mut self, force: bool) {
		let Some((cols, rows)) = self.pending_size else {
			return;
		};
		if !force && self.pending_size_at.elapsed() < SIZE_SAVE_DEBOUNCE {
			return;
		}
		self.pending_size = None;
		let orig = (*config::settings()).clone();
		if cols == orig.remembered_columns && rows == orig.remembered_rows {
			return;
		}
		let mut new = orig.clone();
		new.remembered_columns = cols;
		new.remembered_rows = rows;
		// If the file's open elsewhere persist skips it (retried on the next resize
		// or at exit); the live size still updates in memory either way.
		let _ = config::persist(&orig, &new);
		config::update(new);
	}

	fn new_tab(&mut self, proxy: &EventLoopProxy<UserEvent>) {
		self.new_tab_with(proxy, None);
	}

	// `shell` is a shell picked by name from the Tabs menu; None inherits from
	// the pane that was active, as a plain new tab does. The directory is
	// inherited either way - picking a shell says nothing about where to start.
	fn new_tab_with(&mut self, proxy: &EventLoopProxy<UserEvent>, shell: Option<Vec<String>>) {
		self.commit_tab_edit();
		// area with the bar shown (we're about to have >1 tab); relayout_all fixes
		// the exact rects right after, this is just the new pane's provisional box
		let bar = self.menubar_h() + self.tab_bar_h();
		let area = Rect {
			x: 0.0,
			y: bar,
			w: self.surface_px.0 as f32,
			h: (self.surface_px.1 as f32 - bar).max(1.0),
		};
		// inherit shell + directory from the pane that was active when the tab
		// was opened; a default-shell pane carries None -> still the default
		let (cmd, cwd) = self
			.tabs
			.list
			.get(self.tabs.active)
			.map_or((None, None), PaneManager::inherit_spawn);
		let cmd = shell.or(cmd).or_else(config::default_shell_argv);
		if let Ok(pm) = PaneManager::new(&mut self.text, proxy, area, cmd, cwd) {
			self.tabs.list.push(pm);
			self.tabs.active = self.tabs.list.len() - 1;
			self.relayout_all(); // existing tab(s) shrink for the now-shown bar
			self.update_title();
			self.dirty = true;
		}
	}

	// New window = a fresh process (each window is its own process), started in
	// the focused pane's current directory so it picks up where you are. The
	// child sets up its own ctl socket/env at startup; a reaper thread waits on
	// it so a closed window can't linger as a zombie.
	fn new_window(&mut self) {
		let cwd = self
			.tabs
			.cur()
			.panes
			.get(&self.tabs.cur().focused)
			.and_then(|p| p.term.cwd());
		let exe = match std::env::current_exe() {
			Ok(p) => p,
			Err(e) => {
				eprintln!("new window: {e}");
				return;
			}
		};
		let config = config::config_override();
		let mut cmd = new_window_command(&exe, cwd.as_deref(), config.as_deref());
		match cmd.spawn() {
			Ok(mut child) => {
				std::thread::spawn(move || {
					let _ = child.wait();
				});
			}
			Err(e) => eprintln!("new window: {e}"),
		}
	}

	fn close_tab(&mut self) {
		self.close_tab_at(self.tabs.active);
	}

	// A pane whose shell has gone: close just that pane, then its tab, then the
	// window. Mirrors the Close-Pane menu cascade. The pane can be in any tab -
	// a background tab's shell exits the same way.
	fn close_dead_pane(&mut self, id: PaneId) {
		let area = self.area();
		let Some(tab_idx) = self
			.tabs
			.list
			.iter()
			.position(|pm| pm.panes.contains_key(&id))
		else {
			return;
		};
		match close_scope(true, self.tabs.list[tab_idx].panes.len(), self.tabs.len()) {
			CloseScope::Pane => {
				self.tabs.list[tab_idx].close(&mut self.text, id, area);
			}
			CloseScope::Tab => self.close_tab_at(tab_idx),
			CloseScope::Window => self.quit = true,
			CloseScope::Nothing => {}
		}
		self.forget_dead_menu();
		self.dirty = true;
	}

	// A popup acts on the pane it was opened for. A shell ending closes that pane,
	// and can take its tab with it, so the popup goes too rather than standing
	// open over a pane that is not there.
	fn forget_dead_menu(&mut self) {
		let Some(menu) = &self.menu else {
			return;
		};
		let target = menu.target;
		if !self
			.tabs
			.list
			.iter()
			.any(|pm| pm.panes.contains_key(&target))
		{
			self.menu = None;
			self.bar_open = None;
		}
	}

	// --keep-open: the shell is gone but the pane stays, saying how it ended and
	// waiting for a key. Answers whether the pane is being held, so the caller
	// knows not to close it. ChildExit carries the status and Exit follows with
	// nothing, so the second call finds it already held and leaves it alone.
	fn hold_dead_pane(&mut self, id: PaneId, status: &str) -> bool {
		let Some(p) = self
			.tabs
			.list
			.iter_mut()
			.find_map(|pm| pm.panes.get_mut(&id))
		else {
			return false;
		};
		if p.held {
			return true;
		}
		if !p.keep_open {
			return false;
		}
		p.held = true;
		p.read_only = true;
		p.term.feed(
			format!(
				"\r\n{}: exit {status} - press a key to close\r\n",
				config::APP_NAME
			)
			.as_bytes(),
		);
		self.dirty = true;
		true
	}

	// Close the tab at `idx` (not necessarily the active one - a background tab's
	// shell can exit). Keeps `active` pointing at the same tab where it can.
	fn close_tab_at(&mut self, idx: usize) {
		// A rename is keyed by position, so any change to the list ends it.
		self.cancel_tab_edit();
		if self.tabs.list.len() <= 1 {
			self.quit = true; // closing the only tab closes the window
			return;
		}
		let showed = idx == self.tabs.active;
		self.tabs.list.remove(idx);
		if self.tabs.active > idx {
			self.tabs.active -= 1; // a tab before the active one went away
		}
		if self.tabs.active >= self.tabs.list.len() {
			self.tabs.active = self.tabs.list.len() - 1;
		}
		if showed {
			self.freeze_catchup(); // closing the shown tab reveals a frozen one
		}
		self.relayout_all(); // if back to 1 tab, the bar hides and panes grow
		self.update_title();
		self.dirty = true;
	}

	// Nothing of this window is on screen: minimized, or occluded where the WM
	// says so. Both render entry points check it - a frame built here would bank
	// the whole buffered backlog into the output ease, and the reveal would then
	// play it back as if it had just arrived.
	fn hidden(&self) -> bool {
		self.revealed
			&& (self.occluded || (FREEZE_MINIMIZED && self.window.is_minimized().unwrap_or(false)))
	}

	// The freeze edge, owned in one place because both render entry points reach
	// it: on restore the WM's own redraw arrives before `about_to_wait` runs, so
	// whichever gets here first has to be the one that catches up - otherwise that
	// frame banks the whole backlog into the ease before anything cuts it.
	fn freeze_sync(&mut self) -> bool {
		let hidden = self.hidden();
		let frame = freeze_frame(self.was_hidden, hidden);
		if frame == Frame::CatchUp {
			self.freeze_catchup();
		}
		self.was_hidden = hidden;
		frame == Frame::Skip
	}

	// A frozen surface coming back on screen: hidden tabs never build, and a
	// minimized/occluded window builds nothing - so the reveal is one dirty
	// catch-up frame, hard-cut so the gap closes instantly instead of easing in
	// (that ease is the bounce class, and it also reads as output arriving now).
	// Every pane is cut, not just the ones flagged dirty: the flag is cleared by
	// whichever build got there first, so it answers "is a rebuild owed", not
	// "did the grid move while nobody was looking". A pane that really did sit
	// still is snapping a scroll already at rest.
	fn freeze_catchup(&mut self) {
		for pane in self.tabs.cur_mut().panes.values_mut() {
			pane.hard_cut();
		}
		self.dirty = true;
	}

	// The surface's new size, and the device's copies of it where there is one.
	fn resize_surface(&mut self, w: u32, h: u32) {
		if w == 0 || h == 0 {
			return;
		}
		self.surface_px = (w, h);
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.gfx.resize(w, h);
			gpu.scrim.resize(&gpu.gfx.device, w, h);
		}
	}

	// A sign of life: the idle clock starts over, and a window that let its
	// device go is owed it back.
	fn note_active(&mut self, why: &'static str) {
		if self.idle.active(self.gpu.is_none()) {
			idledbg(&format!("wake: {why}"));
		}
	}

	// Output, which counts only while the window can be seen (IdleClock::output).
	// The hidden flag is the one the last pass settled on.
	fn note_output(&mut self) {
		if self.idle.output(self.gpu.is_none(), self.was_hidden) {
			idledbg("wake: output");
		}
	}

	// The rating waits for the wallpaper, since that is part of what it times,
	// but no longer than its cap.
	fn bench_blocked(&self) -> bool {
		!self.wp_shown && self.bench_cap.is_some_and(|cap| Instant::now() < cap)
	}

	// The banner comes down once the run is over and it has been up long
	// enough to read.
	fn bench_banner_wake(&self) -> Option<Instant> {
		self.bench_banner
			.filter(|_| self.bench.is_none() && self.bench_at.is_none())
			.map(|up| up + BENCH_BANNER_MIN)
	}

	fn release_deadline(&self, cfg: &config::Settings, hidden: bool) -> Option<Instant> {
		release_deadline(
			cfg,
			&Idle {
				focused: self.focused,
				hidden,
				revealed: self.revealed,
				bench_busy: self.bench.is_some() || self.bench_at.is_some(),
				since: self.idle.since,
			},
		)
	}

	// Let the device and everything on it go. The window stays, the shells run
	// on and the grid keeps up; only drawing stops, and `rebuild_gpu` is the
	// way back. What the CPU held only for the device's sake goes too: the
	// rasterized glyphs, the shaped chrome.
	fn release_gpu(&mut self) {
		let Some(gpu) = self.gpu.take() else {
			return;
		};
		let start = Instant::now();
		self.text.detach_gpu();
		self.chrome = None;
		self.invalidate_prepared();
		self.rebirth = Some(gpu.release());
		self.idle.wake_owed = false;
		// no frame draws while released, so nothing else would update the title
		self.conserve = Conserve::Saving;
		self.update_title();
		trim_heap();
		idledbg(&format!("device released in {:?}", start.elapsed()));
	}

	// The device again, on the same window, and everything that lived on it
	// built afresh. The wallpaper is decoded again rather than having been kept,
	// as after a VT switch (recover_gpu). A failure leaves the window released
	// and the next sign of life tries again.
	fn rebuild_gpu(&mut self) {
		let Some(rebirth) = self.rebirth.as_ref() else {
			return;
		};
		let start = Instant::now();
		let gfx = match Gfx::rebuild(rebirth, &self.window) {
			Ok(gfx) => gfx,
			Err(e) => {
				eprintln!(
					"{}: could not bring the GPU device back ({e}); trying again on the next input",
					config::APP_NAME
				);
				self.idle.wake_owed = false;
				return;
			}
		};
		self.rebirth = None;
		let (w, h) = (gfx.config.width, gfx.config.height);
		self.surface_px = (w, h);
		self.text.attach_gpu(&gfx.device, &gfx.queue, gfx.format);
		let rects = RectRenderer::new(&gfx.device, gfx.format);
		let minimap = crate::minimap::MapRenderer::new(&gfx.device, gfx.format);
		let scrim = crate::scrim::Scrim::new(&gfx.device, gfx.format, w, h);
		self.gpu = Some(Gpu {
			gfx,
			rects,
			minimap,
			wallpaper_img: None,
			scrim,
		});
		self.idle.wake_owed = false;
		self.idle.since = Instant::now();
		self.vram_next = Instant::now() + VRAM_CHECK_IVL;
		// the window may have been resized while there was no surface to follow
		self.relayout_all();
		self.request_wallpaper(false);
		self.conserve = Conserve::Restoring;
		self.update_title();
		// the grid moved while nothing drew: one hard-cut catch-up frame
		self.freeze_catchup();
		idledbg(&format!("device rebuilt in {:?}", start.elapsed()));
	}

	// Every piece of chrome off at once, and back the way it was.
	fn toggle_bare(&mut self) {
		self.bare = !self.bare;
		let (saved, now) = bare_chrome(self.bare, self.bare_saved, (self.decorated, self.menu_bar));
		self.bare_saved = saved;
		(self.decorated, self.menu_bar) = now;
		self.window.set_decorations(self.decorated);
		self.bar_open = None;
		self.relayout_all();
	}

	// The Remote profile on or off by hand. Live only: nothing about it reaches
	// the file, so the next launch decides for itself.
	fn toggle_remote(&mut self) {
		let before = config::settings();
		let mut next = (*before).clone();
		next.remote_override = !next.remote_override;
		self.apply_new_settings(&before, next, false);
	}

	fn toggle_fullscreen(&self) {
		let fullscreen = match self.window.fullscreen() {
			Some(_) => None,
			None => Some(Fullscreen::Borderless(None)),
		};
		self.window.set_fullscreen(fullscreen);
	}

	// Request the Settings window (App opens it; window creation needs the loop).
	fn open_settings(&mut self) {
		self.pending_settings = true;
		self.menu = None;
		self.bar_open = None;
	}

	// Live-apply edited settings (from the dialog), persist, and rebuild whatever
	// the change touched (text metrics, background image, opacity, window size).
	// Returns false if the config file looked open elsewhere so the write was
	// skipped - the caller (dialog OK) then keeps the dialog open instead of
	// closing over an unsaved change. The values still apply live regardless.
	fn apply_settings_values(
		&mut self,
		orig: &config::Settings,
		edited: config::Settings,
		_system_font: bool,
	) -> bool {
		// The Shells tab edits the list now, so this is the one path allowed to
		// write it - and it is the dialog's copy that wins. The baseline is the
		// LIVE list rather than the dialog's own `orig`: a scan that arrived while
		// the dialog was open has already been folded into both of its copies
		// (Dialog::fold_shells), so the two agree, and taking the live one is what
		// keeps them honest if they ever do not. They do not when another window
		// changed the list after this one loaded; the dialog's list is then
		// written whole, and it already holds that change.
		//
		// An entry with no command names nothing to run, so it is dropped here
		// rather than written - that is the whole of the grid's "Command is
		// required" rule at the point the list leaves the dialog.
		let live = config::settings();
		let mut orig = orig.clone();
		let mut edited = edited;
		orig.shells.clone_from(&live.shells);
		edited.shells.retain(|e| !e.command.trim().is_empty());
		// Remote and a watch step are live state the dialog only copied when it
		// opened, so a change to either since then must survive the Apply.
		config::keep_session_on_apply(&live, &orig, &mut edited);
		config::keep_wallpaper_on_apply(&live, self.wp_locked, &mut orig, &mut edited);
		// use_system_font is a persisted setting that only reorders font_family at
		// resolve time, so nothing special to strip - persist the diff as usual.
		// The dialog opened on the file, so the file gets only what was edited,
		// while this window takes everything that differs from what it runs -
		// including what another window saved since this one loaded.
		let wrote = config::persist(&orig, &edited);
		self.apply_new_settings(&live, edited, false);
		wrote
	}

	// What Settings opens on: the file as it is now, not what this window loaded,
	// with this session's own choices folded in the way a reload folds them.
	fn settings_for_dialog(&self) -> config::Settings {
		settings_after_reload(
			&config::settings(),
			config::reload_from_disk(),
			&self.cli_style,
			self.wp_locked,
		)
	}

	// Re-read config.shcl from disk and live-apply it (the "internal command" for
	// picking up hand-edits without a file watcher). The file is the source here,
	// so nothing is persisted back.
	fn reload_config(&mut self) {
		let orig = config::settings().as_ref().clone();
		let edited = settings_after_reload(
			&orig,
			config::reload_from_disk(),
			&self.cli_style,
			self.wp_locked,
		);
		// Force the background image to re-read even when its path is unchanged:
		// the user may have swapped the file contents under the same name (#167).
		self.apply_new_settings(&orig, edited, true);
	}

	// Control-socket wallpaper change: live-only and window-scoped, like the
	// launch-time --background-image - nothing is persisted to config.shcl.
	fn set_wallpaper(&mut self, image: Option<std::path::PathBuf>) {
		let orig = config::settings().as_ref().clone();
		let mut edited = orig.clone();
		config::name_wallpaper(&mut edited, image);
		self.apply_new_settings(&orig, edited, true);
	}

	// Hand the wallpaper to a worker thread and carry on drawing. `scan` also
	// (re)reads the rotation folder and picks from it. Nothing here waits: the
	// folder, the image and its tags can all live on a share that answers slowly,
	// which is precisely why none of it runs on this thread.
	fn request_wallpaper(&mut self, scan: bool) {
		let settings = config::settings();
		let scan = scan
			|| needs_folder_read(
				self.wp_locked,
				self.wp_current.as_deref(),
				settings.rotation_folder(),
			);
		// a lock that names nothing is a bare flag asking for no picture
		let cleared = self.wp_locked && settings.wallpaper.is_none();
		// retires anything already in flight - a result arriving after a newer
		// request (a rotation tick overtaken by a settings change) is dropped, and
		// the worker sees the new stamp and stops at its next stage
		let seq = self
			.wp_seq
			.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
			.wrapping_add(1);
		self.wp_pacing.sent(seq);
		crate::wallpaper::spawn(
			&self.proxy,
			crate::wallpaper::Request {
				seq,
				newest: self.wp_seq.clone(),
				settings,
				scan,
				current: self.wp_current.clone(),
				cleared,
			},
		);
	}

	// Wallpaper rotation: unless a wallpaper came in on the command line (a
	// deliberate choice for this session, which leaves rotation out of it
	// entirely), scan the folder and pick one. The timer arms when the scan
	// answers - only then do we know whether there is anything to rotate through.
	fn init_wallpaper(&mut self, lock: bool) {
		self.wp_locked = lock;
		self.request_wallpaper(!lock);
	}

	fn can_rotate(&self) -> bool {
		rotation_live(
			self.wp_locked,
			self.wp_count,
			config::settings().rotation_folder().is_some(),
		)
	}

	// Rotate to the next image. The worker re-scans, so images added to or removed
	// from the folder since launch are picked up. Next wallpaper comes here too,
	// so the timer starts over from the pick it asked for.
	fn advance_wallpaper(&mut self) {
		// locked, switched off since the timer was armed, or one image (or none):
		// nothing to rotate to, so drop the timer
		let settings = config::settings();
		let live = self.can_rotate();
		self.wp_next = rotation_next(Instant::now(), live, settings.wallpaper_rotate_interval_s);
		if !live {
			return;
		}
		// A request still working (an image slower to prepare than the interval,
		// or a folder on a slow share) is left to finish. Its arrival re-arms the
		// timer, or brings the tick back if it was not a rotation itself.
		if !self.wp_pacing.tick() {
			self.wp_next = None;
			return;
		}
		self.request_wallpaper(true);
	}

	// A worker finished; uploading the pixels is all that was left for this thread.
	fn wallpaper_ready(&mut self, loaded: crate::wallpaper::Loaded) {
		if loaded.seq != self.wp_seq.load(std::sync::atomic::Ordering::Relaxed) {
			return; // superseded while it was working
		}
		if self.wp_pacing.arrived(loaded.scanned) {
			// a tick fired while this was working: due now, and the poll sends it
			// once there is a device to show it on
			self.wp_next = Some(Instant::now());
		}
		if loaded.scanned {
			// a scan is authoritative about rotation: no pick means the folder holds
			// nothing (or went away), so the timer goes with it
			self.wp_count = 0;
			self.wp_current = None;
			self.wp_next = None;
			if let Some(rot) = &loaded.rotation {
				self.wp_count = rot.count;
				self.wp_current = Some(rot.current.clone());
				// live-only, like a --wallpaper-file: the dialog shows what is on
				// screen, and nothing about the pick reaches config.shcl
				let mut settings = config::settings().as_ref().clone();
				settings.wallpaper_raw = rot.current.to_string_lossy().into_owned();
				settings.wallpaper = Some(rot.current.clone());
				config::update(settings);
				let ivl = config::settings().wallpaper_rotate_interval_s;
				self.wp_next = (ivl > 0.0 && rot.count > 1)
					.then(|| Instant::now() + Duration::from_secs_f32(ivl));
			}
		}
		// The derived text and cursor colors follow the picture (autotheme.rs), so
		// what this one is worth goes live with it. Session state: none of it
		// reaches the file, the same as a rotated pick.
		let summary = loaded.image.as_ref().map(|img| img.summary);
		if config::settings().wallpaper_summary != summary {
			let mut settings = config::settings().as_ref().clone();
			settings.wallpaper_summary = summary;
			config::update(settings);
			// the text is a different color now, so nothing retained is good
			self.invalidate_prepared();
			self.chrome = None;
		}
		// A window without a device drops the pixels: the rebuild asks for the
		// wallpaper again, and decoding it twice beats holding a copy of it.
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.wallpaper_img = loaded.image.map(|img| {
				let (w, h) = img.rgba.dimensions();
				ImageRenderer::new(
					&gpu.gfx.device,
					&gpu.gfx.queue,
					gpu.gfx.format,
					&img.rgba,
					w,
					h,
					img.opacity,
					img.fit,
					img.anchor,
				)
			});
		}
		// Answered either way: an empty result is the news that there is no
		// wallpaper to wait for, which settles the question just as well.
		self.wp_answered = true;
		self.conserve.wallpaper_answered(Instant::now());
		self.update_title();
		self.dirty = true;
	}

	// A wallpaper set from the command line while running: honor it for the rest
	// of the session and stop rotating, without touching the stored settings.
	fn lock_wallpaper(&mut self, image: Option<std::path::PathBuf>) {
		self.wp_locked = true;
		self.wp_next = None;
		// rotation is done for this session, so drop what it was showing - otherwise
		// an explicit clear would fall back to it instead of clearing
		self.wp_current = None;
		self.set_wallpaper(image);
	}

	// Rebuild the text context (cell metrics, chrome, pane buffers) for a new
	// scale factor or font, then relayout. Shared by settings-driven font
	// rebuilds and DPI scale-factor changes. The surface itself is reconfigured
	// separately (a Resized event follows a scale change).
	// Session font zoom (hotkeys / View menu): step the zoom offset and rebuild
	// the text context at the new effective size. Window-wide, never persisted.
	fn font_zoom(&mut self, dir: i32) {
		config::nudge_font_zoom(dir);
		let scale = config::display_scale(self.window.scale_factor());
		self.rebuild_text(scale);
		self.dirty = true;
	}

	fn font_zoom_reset(&mut self) {
		if config::font_zoom_px() == 0 {
			return; // already at the configured size
		}
		config::reset_font_zoom();
		let scale = config::display_scale(self.window.scale_factor());
		self.rebuild_text(scale);
		self.dirty = true;
	}

	// Force the next frame through a full prepare + scrim build. Call whenever
	// something outside the signature's reach makes the retained GPU state stale
	// (new atlases, recreated textures, lost VRAM).
	fn invalidate_prepared(&mut self) {
		self.text_sig = None;
		self.scrim_sig = None;
	}

	fn rebuild_text(&mut self, scale: f32) {
		self.text = TextCtx::new_cpu(scale);
		if let Some(gpu) = &self.gpu {
			self.text
				.attach_gpu(&gpu.gfx.device, &gpu.gfx.queue, gpu.gfx.format);
		}
		self.chrome = None; // cached chrome buffers are tied to the old FontSystem
		self.invalidate_prepared(); // fresh atlases hold nothing to reuse
		for pm in &mut self.tabs.list {
			pm.rebuild_buffers(&mut self.text);
		}
		self.relayout_all();
	}

	// Swap in `edited` and rebuild whatever changed vs `orig` (text metrics,
	// background image, window opacity). Shared by the dialog and config reload.
	// `force_bg` re-reads the image even if the path string didn't change.
	fn apply_new_settings(
		&mut self,
		orig: &config::Settings,
		edited: config::Settings,
		force_bg: bool,
	) {
		let resize = edited.columns != orig.columns || edited.rows != orig.rows;
		// copy_on_select changed -> apply to every existing pane too, so the
		// dialog toggle takes effect now, not only for panes spawned later
		if edited.copy_on_select != orig.copy_on_select {
			for pm in &mut self.tabs.list {
				for pane in pm.panes.values_mut() {
					pane.copy_select = edited.copy_on_select;
				}
			}
		}
		// What changed is judged on the LIVE values, before and after: a
		// performance profile sets the wallpaper and the halo on top of the
		// stored settings, so the dialog's own diff can be empty while the
		// picture changes completely.
		let before = config::settings();
		config::update(edited);
		let after = config::settings();
		let rebuild = crate::settings_ui::needs_text_rebuild(&before, &after);
		let bg = force_bg || crate::settings_ui::wallpaper_changed(&before, &after);
		let blur_changed = after.transparent_background_blur != before.transparent_background_blur;
		if crate::profile::current(&after) != crate::profile::current(&before) {
			self.rating.reset();
		}

		// Backdrop-blur hint toggled -> set/clear the compositor property live.
		if blur_changed {
			set_blur_behind(&self.window, config::settings().transparent_background_blur);
		}

		// Transparency is per-pixel (terminal background only) - never whole-window.
		// Nothing to do here; the bg fill picks up the new opacity on the next frame.
		// window dimensions changed in Settings -> resize to the new cell grid
		if resize {
			let settings = config::settings();
			let (w, h) = window_px(
				settings.columns,
				settings.rows,
				self.text.cell_w,
				self.text.cell_h,
				self.text.margin,
				self.chrome_h(),
			);
			let max_dim = self.gpu.as_ref().map_or(SAFE_MAX_DIM, |gpu| {
				gpu.gfx.device.limits().max_texture_dimension_2d
			});
			let (w, h) = fit_px(w, h, max_dim);
			let want = winit::dpi::PhysicalSize::new(w, h);
			// A size the window can honor straight away answers here and sends no
			// `Resized`, so this is the only chance to move everything the window
			// event moves - the scrim included, which was left at the old size.
			if let Some(applied) = self.window.request_inner_size(want) {
				self.resize_surface(applied.width, applied.height);
				self.invalidate_prepared();
			}
		}
		if rebuild {
			self.rebuild_text(config::display_scale(self.window.scale_factor()));
		} else if after.minimap != before.minimap || after.minimap_width != before.minimap_width {
			// the column takes real columns from the grid, so this is a layout change
			self.relayout_all();
		}
		if bg {
			// Turning the wallpaper or its rotation back on: the folder has to be
			// re-read, since the pick and its timer went with it on the way off.
			let resumed = after.rotation_folder().is_some() && before.rotation_folder().is_none();
			self.request_wallpaper(resumed);
		}
		self.dirty = true;
	}

	// Put a rung's settings live for the length of the benchmark. Nothing is
	// written: the run is a measurement and only its answer reaches the file.
	fn set_live_profile(&mut self, profile: crate::profile::Profile) {
		let before = config::settings();
		let next = with_measured_profile(&before, profile);
		self.apply_new_settings(&before, next, false);
	}

	// The benchmark settled on a rung. Write it down against the hardware it was
	// measured on, and give the window back.
	fn finish_bench(&mut self, pick: crate::profile::Profile) {
		self.bench = None;
		self.rating.reset();
		eprintln!(
			"{}: performance profile measured for this hardware: {}",
			config::APP_NAME,
			pick.label()
		);
		let orig = (*config::settings()).clone();
		let mut new = with_measured_profile(&orig, pick);
		let id = self.bench_id.take();
		let kept = keep_measured(pick, id.as_deref());
		if let Some(id) = id {
			new.rated_hardware = id;
		}
		if kept == config::Kept::Written {
			self.bench_kept = None;
		} else {
			// The banner stays up long enough to read why, from now, since this is
			// the one place a person learns the test will run again.
			note_rating_not_kept(&kept);
			self.bench_kept = Some(kept);
			self.bench_banner = Some(Instant::now());
			self.dirty = true;
		}
		self.apply_new_settings(&orig, new, false);
	}

	// The run could not tell the machine from the display (`Step::Stalled`), most
	// likely a monitor asleep. Nothing is written, so the next launch tests again,
	// and the session goes back to the profile it had. Saving Standard here once
	// left a machine without its wallpaper from then on.
	fn finish_bench_stalled(&mut self) {
		self.bench = None;
		self.bench_id = None;
		self.rating.reset();
		eprintln!(
			"{}: performance test gave no answer (the display was not drawing at its usual rate); it runs again at the next launch",
			config::APP_NAME
		);
		self.bench_stalled = true;
		self.bench_kept = None;
		self.bench_banner = Some(Instant::now());
		self.dirty = true;
		if let Some(from) = self.bench_from.take() {
			self.set_live_profile(from);
		}
	}

	// The display missed its budget over a whole window of eased frames: with
	// the profile on automatic, take one step down for the rest of the session.
	// Nothing is written, so the next launch starts from the rated profile.
	fn step_down_profile(&mut self) {
		let live = config::settings();
		let Some(next) = watch_step_down(&live) else {
			return;
		};
		eprintln!(
			"{}: the display is not keeping up; performance profile stepped down to {} until {} restarts",
			config::APP_NAME,
			crate::profile::current(&next).label(),
			config::APP_NAME
		);
		self.apply_new_settings(&live, next, false);
	}

	// GPU texture contents were lost (VT switch / suspend; see the Sentinel note
	// in gfx.rs). Re-upload everything that was uploaded once: fresh glyph
	// atlases + chrome via rebuild_text, and the wallpaper. rebuild_text also drops
	// the prepared/scrim signatures, so the next frame rebuilds the scrim source
	// instead of reusing a texture that no longer holds anything.
	// Everything on the device again, after a return to this console. The whole
	// device when nothing else shares it, since a switch can spoil any texture
	// and `recover_gpu` only knows the text and the wallpaper. An open dialog's
	// context cannot outlive the terminal's on X11, so that case stays partial.
	fn heal_gpu(&mut self, dialog_open: bool) {
		if self.gpu.is_none() {
			return; // a released window rebuilds from nothing anyway
		}
		if dialog_open {
			self.recover_gpu();
			return;
		}
		self.release_gpu();
		self.rebuild_gpu();
	}

	fn recover_gpu(&mut self) {
		if self.gpu.is_none() {
			return; // nothing uploaded to lose; the rebuild starts from nothing anyway
		}
		self.rebuild_text(config::display_scale(self.window.scale_factor()));
		// re-decoded rather than kept resident: a large wallpaper is tens of MB, and
		// a VT switch is rare enough not to trade that for a moment without one
		self.request_wallpaper(false);
		self.conserve = Conserve::Restoring;
		self.update_title();
		self.dirty = true;
	}

	// returns true while any pane is still animating (caller keeps frames coming).
	// `force_rebuild` = the frame changed content/scroll/bell (not a pure cursor
	// animation), so panes re-shape text; false lets them reuse the cached frame.
	// A frame, on the device the window has. No device means nothing drawn and
	// no animation to keep frames coming for.
	fn render(&mut self, force_rebuild: bool) -> bool {
		let Some(mut gpu) = self.gpu.take() else {
			return false;
		};
		let animating = self.render_with(&mut gpu, force_rebuild);
		self.gpu = Some(gpu);
		animating
	}

	fn render_with(&mut self, gpu: &mut Gpu, force_rebuild: bool) -> bool {
		// once a frame has been drawn, later resizes are user-driven and may update
		// the remembered window size (startup/programmatic ones happen before this)
		self.size_tracked = true;
		let area = self.area();
		if area.w < 1.0 || area.h < 1.0 {
			return false;
		}
		// keep the window title tracking the active tab's foreground program
		// (deduped inside update_title, so this is cheap when nothing changed)
		self.update_title();

		let now = Instant::now();
		let dt = (now - self.last_frame).as_secs_f32().min(0.1);
		self.last_frame = now;
		let cfg = config::settings(); // one snapshot per frame, not per use/pane

		// A full-screen program takes the minimap column and gives its width back to
		// the text, so the answer has to be settled before anything is laid out.
		if self.tabs.cur_mut().sync_minimap(&cfg) {
			self.relayout_all();
		}

		// Regaining the window, switching tab, or moving pane focus pokes the
		// focused pane: its cursor animation resumes immediately, from the top of
		// the cycle - no resume delay, that one is for input.
		let focus_sig = (self.tabs.active, self.tabs.cur().focused, self.focused);
		if self.focused && self.cursor_focus_sig != Some(focus_sig) {
			let id = self.tabs.cur().focused;
			if let Some(pane) = self.tabs.cur_mut().panes.get_mut(&id) {
				pane.poke_cursor();
			}
		}
		self.cursor_focus_sig = Some(focus_sig);

		// Visual-bell flash decays toward 0; while >0 the text is brightened (in
		// build) and we keep rendering so the fade is smooth.
		if self.bell_flash > 0.0 {
			self.bell_flash = (self.bell_flash * (-dt / BELL_TAU_S).exp()).max(0.0);
			if self.bell_flash < 0.01 {
				self.bell_flash = 0.0;
			}
		}
		let bell = self.bell_flash;

		// translucent background only when the surface supports it AND the user has
		// Transparency on - and it only ever affects the bg, never text/chrome.
		let bg_alpha = if gpu.gfx.transparent && cfg.transparent_background {
			self.opacity()
		} else {
			1.0
		};

		let mut under: Vec<RectInstance> = Vec::new();
		// what each pane's fill covers - the light-mode wallpaper draws the fill
		// itself and has to be clipped to it
		let mut pane_fulls: Vec<Rect> = Vec::new();
		// cursors are drawn separately (above the scrim, so its halo can't obscure them)
		let mut cursors: Vec<(Rect, RectInstance)> = Vec::new();
		let mut tops: HashMap<u64, f32> = HashMap::new();
		// retained-frame app-scroll slide geometry per pane (None = no active slide)
		let mut slides: HashMap<u64, Option<crate::pane::Slide>> = HashMap::new();
		let mut animating = bell > 0.0;
		if self.autoscroll_selection(dt) {
			animating = true;
		}
		// text-scrim color map needs each cell's bg (so a glyph's halo takes its
		// own cell color, not always the global) - collect them while building.
		// The outline shares the scrim's source and composite, so the pass runs
		// for either; only the blur is the halo's alone.
		let halo_on = cfg.text_scrim && cfg.text_scrim_radius > 0.0;
		let scrim_on = halo_on || cfg.text_outline > 0.0;
		// With both off nothing here draws, and its five full-screen textures have
		// no business being allocated. Turning either on grows them back.
		if gpu.scrim.set_enabled(&gpu.gfx.device, scrim_on) {
			self.invalidate_prepared();
		}
		let mut scrim_cells: Vec<RectInstance> = Vec::new();

		self.text.color_frame();
		let win_focused = self.focused;
		let active_pane = self.tabs.cur().focused;
		// pane fill color is loop-invariant
		let pane_bg = {
			let mut c = config::srgb_f32(cfg.bg);
			c[3] = bg_alpha;
			c
		};
		for (id, pane) in &mut self.tabs.cur_mut().panes {
			pane.scroll.advance(dt);
			pane.scrollbar_tick(dt, &cfg);
			if pane.bar_animating {
				animating = true;
			}
			let rect = pane.rect;
			// scope the expensive re-shape to panes that actually changed: fresh
			// PTY output (content_dirty), an active scroll ease, or a global
			// cause (bell flash, chrome/UI change) - idle siblings reuse their
			// cached frame instead of re-shaping at the busy pane's rate
			let force = force_rebuild || pane.content_dirty || pane.scroll.animating();
			crate::perf::timed(&crate::perf::BUILD_NS, || {
				pane.build(
					&mut self.text,
					dt,
					bell,
					force,
					win_focused && *id == active_pane,
				);
			});
			if pane.scroll.animating() || pane.cursor_animating {
				animating = true;
			}
			let draw = pane.draw();
			tops.insert(*id, draw.top);
			slides.insert(*id, draw.slide.clone());
			under.push(RectInstance {
				pos: [pane.full.x, pane.full.y],
				size: [pane.full.w, pane.full.h],
				color: crate::gfx::see_through(pane_bg),
				..Default::default()
			});
			pane_fulls.push(pane.full);
			if let Some(cursor_quad) = draw.cursor {
				cursors.push((rect, cursor_quad));
			}
		}

		// The builds above are where a hyperlink hover is resolved, so the pointer
		// shape can only be settled after them - a link found under a pointer that
		// has stopped moving gets no further pointer event to react to.
		self.sync_cursor_icon();

		let under_len = under.len() as u32;
		let mut instances = under;
		// per-pane bg quads (scissored to the pane so overscan rows don't bleed
		// into neighbors), copied once from each pane's retained frame
		let mut group_ranges: Vec<(Rect, u32, u32)> = Vec::new();
		for p in self.tabs.cur().panes.values() {
			let bg_quads = &p.draw().bg;
			let start = instances.len() as u32;
			instances.extend_from_slice(bg_quads);
			if scrim_on {
				scrim_cells.extend_from_slice(bg_quads);
			}
			group_ranges.push((p.rect, start, instances.len() as u32));
		}

		let ring_start = instances.len() as u32;
		// Scrollbars, drawn with the ring (after the text, so they overlay it) but
		// pushed first so the focus ring stays on top where they meet at the corner.
		for p in self.tabs.cur().panes.values() {
			if let Some(bar) = p.scrollbar(&self.text, &cfg) {
				let active = p.bar_drag.is_some() || p.bar_hover;
				instances.extend(scrollbar_insts(&bar, p.bar_fade(), active));
			}
			if let Some(g) = p.minimap(&self.text, &cfg) {
				instances.extend(minimap_insts(&g, p.map_drag.is_some()));
			}
		}
		// Focus ring only distinguishes panes when there's more than one; with a
		// single pane it's just an unwanted border line around the whole content
		// (the user wants background all the way to the edge), so skip it.
		if self.tabs.cur().panes.len() > 1 {
			if let Some(p) = self.tabs.cur().panes.get(&self.tabs.cur().focused) {
				instances.extend(focus_ring(p.full, self.text.scale));
			}
		}
		// drop-target tint while drag-reordering a pane
		if let Some(src) = self.dragging_pane {
			if let Some(target_id) = self.tabs.cur().pane_at(self.mouse.0, self.mouse.1) {
				if target_id != src {
					if let Some(p) = self.tabs.cur().panes.get(&target_id) {
						let mut color = config::srgb_f32(config::DROP_TARGET);
						color[3] = 0.30;
						instances.push(RectInstance {
							pos: [p.full.x, p.full.y],
							size: [p.full.w, p.full.h],
							color,
							..Default::default()
						});
					}
				}
			}
		}
		let ring_end = instances.len() as u32;

		// Column images: one texture per pane, uploaded only when the compose
		// behind them moved. Direct field access - `tabs.cur()` would borrow the
		// whole of self and the renderer needs it mutably.
		gpu.minimap.begin_frame();
		if cfg.minimap {
			let mm = &mut gpu.minimap;
			let (device, queue) = (&gpu.gfx.device, &gpu.gfx.queue);
			let res = (gpu.gfx.config.width as f32, gpu.gfx.config.height as f32);
			for (id, p) in &self.tabs.list[self.tabs.active].panes {
				if let Some(g) = p.minimap(&self.text, &cfg) {
					mm.prepare(device, queue, *id, g.preview, res, p.map_cache());
				}
			}
		}

		// cursor quads also feed the scrim's cursor-coverage texture (its own tex,
		// so cursor_scrim/cursor_outline gate it independently); the cursor still
		// draws crisp ABOVE the composite below. Collect them whenever the scrim is
		// on - the shader flags decide whether they reach the halo and/or outline.
		let scrim_cursor_quads: Vec<RectInstance> = if scrim_on {
			cursors.iter().map(|(_, q)| *q).collect()
		} else {
			Vec::new()
		};

		// Hyperlink underlines sit with the cursor, AFTER the scrim composite - they
		// are chrome about the text, not a cell background. Filed with the bg quads
		// they were painted over by the halo, which is densest right under the
		// glyphs, so a solid rule came out as a barcode tracing the letterforms.
		// They stay out of the scrim's coverage map either way (an underline should
		// cast no halo of its own), and stay under the cursor as before.
		let mut link_ranges: Vec<(Rect, u32, u32)> = Vec::new();
		for p in self.tabs.cur().panes.values() {
			let link_quads = &p.draw().links;
			if link_quads.is_empty() {
				continue;
			}
			let start = instances.len() as u32;
			instances.extend_from_slice(link_quads);
			link_ranges.push((p.rect, start, instances.len() as u32));
		}

		// cursor quads get their own per-pane ranges, drawn after the scrim composite
		let mut cursor_ranges: Vec<(Rect, u32, u32)> = Vec::new();
		for (rect, cursor_quad) in cursors {
			let start = instances.len() as u32;
			instances.push(cursor_quad);
			cursor_ranges.push((rect, start, instances.len() as u32));
		}

		let win_w = gpu.gfx.config.width as f32;
		let menu_h = self.menu_bar_h();
		let tab_h = self.tab_bar_h();

		// menu bar (File/Edit/...), drawn in the main pass at the very top; the
		// open menu's title is highlighted.
		let menubar_range = if self.menu_bar {
			let start = instances.len() as u32;
			instances.push(rect_inst(0.0, 0.0, win_w, menu_h, config::TAB_BAR_BG));
			let layout = self.menubar_layout();
			if let Some(idx) = self.bar_open {
				if let Some(&(x, w)) = layout.get(idx) {
					instances.push(rect_inst(x, 0.0, w, menu_h, config::menu_hover()));
				}
			}
			// Alt held (no dropdown open): underline each title's accelerator
			// letter, like the open-dropdown items do (press the letter to open).
			let marks = bar_title_underlines(self.mods.alt_key(), self.bar_open, &MENU_BAR);
			if !marks.is_empty() {
				let attrs = crate::text::ui_attrs();
				let rule = self.text.dip(CHROME_HAIRLINE);
				let underline_y = self.text.ui_baseline(0.0, menu_h) + rule;
				let title_pad = self.text.dip(MENU_BAR_PAD);
				for (i, c) in marks {
					let Some(&(x, _)) = layout.get(i) else {
						continue;
					};
					let mut buf = [0u8; 4];
					let letter_w = self.text.measure_ui_text(c.encode_utf8(&mut buf), &attrs);
					instances.push(rect_inst(
						x + title_pad,
						underline_y,
						letter_w,
						rule,
						config::menu_fg(),
					));
				}
			}
			// always-visible copy-mode checkboxes (right side): outlines always,
			// filled per the focused pane's two independent triggers, so the state
			// is never hidden. Dimmed when this window isn't focused - the flags
			// stay set, but nothing copies until it regains focus.
			let fp = self.tabs.cur().panes.get(&self.tabs.cur().focused);
			let checked = [
				fp.is_some_and(|p| p.copy_select),
				fp.is_some_and(|p| p.copy_output),
			];
			if let Some(cb) = self.copybox_layout() {
				let border = copy_dim(config::menu_border(), self.focused);
				let fill = copy_dim(config::menu_fg(), self.focused);
				let box_rule = self.text.dip(CHROME_HAIRLINE);
				let tick_inset = self.text.dip(COPYBOX_TICK_INSET);
				for (checkbox, on) in cb.boxes.iter().zip(checked) {
					instances.push(rect_inst(
						checkbox.x - box_rule,
						checkbox.y - box_rule,
						checkbox.w + 2.0 * box_rule,
						checkbox.h + 2.0 * box_rule,
						border,
					));
					instances.push(rect_inst(
						checkbox.x,
						checkbox.y,
						checkbox.w,
						checkbox.h,
						config::TAB_BAR_BG,
					));
					if on {
						instances.push(rect_inst(
							checkbox.x + tick_inset,
							checkbox.y + tick_inset,
							checkbox.w - 2.0 * tick_inset,
							checkbox.h - 2.0 * tick_inset,
							fill,
						));
					}
				}
			}
			Some((start, instances.len() as u32))
		} else {
			None
		};

		// tab bar (only with >1 tab), drawn just below the menu bar
		let tab_bar_y = self.menubar_h();
		let tabbar_range = if self.tab_bar_visible() {
			let start = instances.len() as u32;
			instances.push(rect_inst(0.0, tab_bar_y, win_w, tab_h, config::TAB_BAR_BG));
			let first = self.tab_layout.first;
			let strip = self.tab_layout.widths.clone();
			// per-tab loop invariants (each config accessor is an RwLock read)
			let box_border = config::menu_border();
			let x_rgb = close_x_rgb();
			let tab_gap = self.text.dip(TAB_GAP);
			let (btn_y, btn_h) = tab_button_v(tab_bar_y, tab_h, self.text.scale);
			let cb_rule = self.text.dip(CHROME_HAIRLINE);
			// Where the caret and the selection sit inside the label of a tab being
			// renamed, measured once before the loop borrows nothing (measuring
			// wants &mut self.text).
			let edit_marks = self.tab_edit.as_ref().map(|edit| {
				let (from, to) = edit.range();
				(edit.tab, edit.text.clone(), from, to, edit.caret)
			});
			let edit_marks = edit_marks.map(|(tab, text, from, to, caret)| {
				let attrs = crate::text::ui_attrs();
				let mut upto = |at: usize| self.text.measure_ui_text(&text[..at], &attrs);
				(tab, upto(from), upto(to), upto(caret))
			});
			let edit_pad = self.text.dip(TAB_EDIT_PAD);
			let caret_w = self.text.dip(CHROME_HAIRLINE).max(1.0);
			let mut x = 0.0;
			for (slot, tab_w) in strip.iter().copied().enumerate() {
				let i = first + slot;
				let color = if i == self.tabs.active {
					config::TAB_ACTIVE
				} else {
					config::TAB_INACTIVE
				};
				// the button sits inside the bar: a gap each side of the seam, and it
				// runs to the bar's bottom edge less one hairline
				instances.push(rect_inst(
					x + tab_gap,
					btn_y,
					tab_w - 2.0 * tab_gap,
					btn_h,
					color,
				));
				// A rename in progress draws a text box in place of the label: a
				// recessed well, an outline in the focus color, then the selection
				// and the caret. All of it goes down before the text pass, so the
				// label itself sits on top.
				if let Some((edit_tab, sel_from, sel_to, caret)) = edit_marks {
					if i == edit_tab {
						let field = tab_edit_box(
							x,
							tab_w,
							tab_bar_y,
							tab_h,
							self.text.ui_line_h,
							self.text.scale,
						);
						instances.push(rect_inst(
							field.x - cb_rule,
							field.y - cb_rule,
							field.w + 2.0 * cb_rule,
							field.h + 2.0 * cb_rule,
							config::settings().highlight,
						));
						instances.push(rect_inst(
							field.x,
							field.y,
							field.w,
							field.h,
							mix_rgb(color, [0x00, 0x00, 0x00], 0.45),
						));
						let text_x = field.x + edit_pad;
						let (top, h) = (field.y + cb_rule, field.h - 2.0 * cb_rule);
						let inside = |at: f32| (text_x + at).clamp(field.x, field.x + field.w);
						if sel_to > sel_from {
							let (from, to) = (inside(sel_from), inside(sel_to));
							instances.push(rect_inst(
								from,
								top,
								to - from,
								h,
								config::SELECTION_BG,
							));
						}
						instances.push(rect_inst(
							inside(caret).min(field.x + field.w - caret_w),
							top,
							caret_w,
							h,
							x_rgb,
						));
					}
				}
				// close-button box: a 1px outline (border rect + inner tab-bg fill).
				// The active tab's box fill leans faintly toward a pastel red - just
				// past noticeable, so the current tab reads at a glance without a
				// clashing accent.
				let cb = tab_close_box(x, tab_w, tab_bar_y, tab_h, self.text.scale);
				instances.push(rect_inst(
					cb.x - cb_rule,
					cb.y - cb_rule,
					cb.w + 2.0 * cb_rule,
					cb.h + 2.0 * cb_rule,
					box_border,
				));
				let box_fill = if self.tab_close_arm == Some(i) {
					// held down: light the button (press feedback; closes on release)
					mix_rgb(color, [0xff, 0xff, 0xff], 0.28)
				} else if i == self.tabs.active {
					mix_rgb(color, [0xd0, 0x80, 0x80], 0.12)
				} else {
					color
				};
				instances.push(rect_inst(cb.x, cb.y, cb.w, cb.h, box_fill));
				instances.push(close_x_inst(cb, x_rgb));
				x += tab_w;
			}
			Some((start, instances.len() as u32))
		} else {
			None
		};

		// context menu quads (drawn in a second pass, on top of everything). A
		// submenu is just another popup in the same pass, drawn after its parent.
		let menu_range = if let Some(root) = &self.menu {
			let start = instances.len() as u32;
			for menu in root.chain() {
				let popup_h = menu.height();
				let border = self.text.dip(CHROME_HAIRLINE);
				instances.push(rect_inst(
					menu.x - border,
					menu.y - border,
					menu.w + 2.0 * border,
					popup_h + 2.0 * border,
					config::menu_border(),
				));
				instances.push(rect_inst(
					menu.x,
					menu.y,
					menu.w,
					popup_h,
					config::menu_bg(),
				));
				if let Some(i) = menu.hover {
					instances.push(rect_inst(
						menu.x,
						menu.row_top(i),
						menu.w,
						menu.item_h,
						config::menu_hover(),
					));
				}
				// faint separator lines between logical groups
				for (i, entry) in menu.entries.iter().enumerate() {
					if matches!(entry, Entry::Sep) {
						let sep_y = menu.row_top(i) + menu.sep_h / 2.0;
						let pad_x = self.text.dip(config::MENU_PAD_X);
						instances.push(rect_inst(
							menu.x + pad_x,
							sep_y,
							menu.w - pad_x * 2.0,
							self.text.dip(CHROME_HAIRLINE),
							config::menu_sep(),
						));
					}
				}
				// the arrow marking a row that opens a submenu, drawn rather than set
				// in text: a font's own metrics decide where a glyph sits, and there
				// is no arrow every interface font carries (same reason as the tab
				// close mark)
				for (i, entry) in menu.entries.iter().enumerate() {
					if matches!(entry, Entry::Sub { .. }) {
						let h = (menu.item_h * 0.38).round().max(4.0);
						let w = (h * 0.62).round().max(3.0);
						let arrow = Rect {
							x: menu.x + menu.w - self.text.dip(config::MENU_PAD_X) - w,
							y: menu.row_top(i) + (menu.item_h - h) / 2.0,
							w,
							h,
						};
						instances.push(sub_arrow_inst(arrow, config::menu_fg()));
					}
				}
				// accelerator underline under each item's accelerator letter (press it
				// to pick); items without one draw no underline
				let acc_attrs = crate::text::ui_attrs();
				let line_h = self.text.ui_line_h;
				let acc_rule = self.text.dip(CHROME_HAIRLINE);
				let acc_x =
					menu.x + self.text.dip(config::MENU_PAD_X) + self.text.dip(config::MENU_GUTTER);
				for (i, entry) in menu.entries.iter().enumerate() {
					if let Some((label, pos)) = entry_accel(entry) {
						if let Some(c) = label[pos..].chars().next() {
							let prefix_w = self.text.measure_ui_text(&label[..pos], &acc_attrs);
							let mut buf = [0u8; 4];
							let letter_w = self
								.text
								.measure_ui_text(c.encode_utf8(&mut buf), &acc_attrs);
							let top = menu.row_top(i) + (menu.item_h - line_h) / 2.0;
							instances.push(rect_inst(
								acc_x + prefix_w,
								top + line_h - self.text.dip(MENU_ACCEL_DROP),
								letter_w,
								acc_rule,
								config::menu_fg(),
							));
						}
					}
				}
			}
			Some((start, instances.len() as u32))
		} else {
			None
		};

		// A tip's box, in the same overlay pass as the menus so it sits over
		// everything - including a pane's own text, which it sits on top of. The
		// tab strip's and a menu row's are the same two quads; only what they say
		// and where they sit differ.
		let tip_layout = self.tab_tip_layout();
		let menu_tip = self.menu_tip_layout();
		let bench_banner = self.bench_layout();
		let border = self.text.dip(CHROME_HAIRLINE);
		let tip_range = (tip_layout.is_some() || menu_tip.is_some() || bench_banner.is_some())
			.then(|| {
				let start = instances.len() as u32;
				// the banner dims the whole window first, which is the visible half of
				// "this window is busy"
				if bench_banner.is_some() {
					instances.push(RectInstance {
						pos: [0.0, 0.0],
						size: [gpu.gfx.config.width as f32, gpu.gfx.config.height as f32],
						color: [0.0, 0.0, 0.0, BENCH_SCRIM_ALPHA],
						..Default::default()
					});
				}
				// Flyover help has its own fill, a warmed lift off the menu color, so a
				// tip does not read as more of the chrome it hangs off. The banner is
				// a modal notice rather than a tip, so it keeps the menu's.
				let boxes = tip_layout
					.iter()
					.chain(menu_tip.iter())
					.map(|(rect, _)| (rect, config::tip_border(), config::tip_bg()))
					.chain(
						bench_banner
							.iter()
							.map(|(rect, _)| (rect, config::menu_border(), config::menu_bg())),
					);
				for (box_rect, edge, fill) in boxes {
					instances.push(rect_inst(
						box_rect.x - border,
						box_rect.y - border,
						box_rect.w + 2.0 * border,
						box_rect.h + 2.0 * border,
						edge,
					));
					instances.push(rect_inst(
						box_rect.x, box_rect.y, box_rect.w, box_rect.h, fill,
					));
				}
				(start, instances.len() as u32)
			});
		let overlay_range = match (menu_range, tip_range) {
			(Some((start, _)), Some((_, end))) | (Some((start, end)), None) => Some((start, end)),
			(None, other) => other,
		};

		let margin = self.text.margin;
		let menu_fg_rgb = config::menu_fg();
		let menu_fg = GColor::rgb(menu_fg_rgb[0], menu_fg_rgb[1], menu_fg_rgb[2]);
		// copy-mode labels dim with their checkboxes when the window is unfocused
		let copy_label_fg = {
			let c = copy_dim(menu_fg_rgb, self.focused);
			GColor::rgb(c[0], c[1], c[2])
		};
		// tab titles - measured first (the task probe and the fit are both &mut)
		// before self.text is borrowed for the buffers below. Each is fitted to
		// the space its own tab has, which is where a path gets shortened.
		self.rebuild_tab_layout();
		let (tab_widths, tab_titles) = if self.tab_bar_visible() {
			(
				self.tab_layout.widths.clone(),
				self.tab_layout.labels.clone(),
			)
		} else {
			(Vec::new(), Vec::new())
		};
		// keep the shaped chrome text current (see ChromeCache) - a color change
		// rebuilds it all, otherwise only changed tab titles re-shape
		if self
			.chrome
			.as_ref()
			.is_some_and(|cache| cache.menu_fg != menu_fg_rgb)
		{
			self.chrome = None;
		}
		if self.chrome.is_none() {
			let shape_ui = |text: &mut TextCtx, s: &str, w: f32, h: f32, color: GColor| {
				let mut buf = text.new_ui_buffer(w, h);
				let mut attrs = crate::text::ui_attrs();
				attrs.color_opt = Some(color);
				buf.set_text(&mut text.font_system, s, &attrs, Shaping::Advanced, None);
				buf.shape_until_scroll(&mut text.font_system, false);
				buf
			};
			// menu-bar titles (one per top-level menu) plus the trailing
			// "Copy on / select / output" labels for the always-visible checkboxes
			let menubar = MENU_BAR
				.iter()
				.chain(COPYBOX_LABELS.iter())
				.map(|title| {
					let w = self.text.dip(MENUBAR_TEXT_W);
					shape_ui(&mut self.text, title, w, menu_h, menu_fg)
				})
				.collect();
			self.chrome = Some(ChromeCache {
				menu_fg: menu_fg_rgb,
				menubar,
				tabs: Vec::new(),
			});
			self.chrome_rev = self.chrome_rev.wrapping_add(1);
		}
		{
			let mut reshaped = false;
			{
				let cache = self.chrome.as_mut().unwrap(); // ensured above
				if cache.tabs.len() > tab_titles.len() {
					reshaped = true;
				}
				cache.tabs.truncate(tab_titles.len());
			}
			let scale = self.text.scale;
			for (i, title) in tab_titles.into_iter().enumerate() {
				let title_w = tab_title_w(tab_widths[i], scale);
				// an unchanged title in an unchanged tab keeps its shaped buffer;
				// a width change re-wraps it
				if self
					.chrome
					.as_ref()
					.unwrap()
					.tabs
					.get(i)
					.is_some_and(|(cached, cached_w, _)| {
						cached == &title && (*cached_w - title_w).abs() < 0.01
					}) {
					continue;
				}
				reshaped = true;
				let mut buf = self.text.new_ui_buffer(title_w, tab_h);
				let mut attrs = crate::text::ui_attrs();
				attrs.color_opt = Some(menu_fg);
				buf.set_text(
					&mut self.text.font_system,
					&title,
					&attrs,
					Shaping::Advanced,
					None,
				);
				buf.shape_until_scroll(&mut self.text.font_system, false);
				let cache = self.chrome.as_mut().unwrap();
				if i < cache.tabs.len() {
					cache.tabs[i] = (title, title_w, buf);
				} else {
					cache.tabs.push((title, title_w, buf));
				}
			}
			if reshaped {
				self.chrome_rev = self.chrome_rev.wrapping_add(1);
			}
		}
		// compute before borrowing panes for `areas` (menubar_layout takes &mut self)
		let bar_layout = self.menubar_layout();
		let copyboxes = self.copybox_layout();

		// Fingerprint every input to the prepared text set. A pure cursor frame
		// reproduces it exactly, which is the signal that glyphon's retained
		// buffers are still correct and both prepares can be skipped. Anything
		// missed here shows up as an extra prepare, never as stale text - so err
		// toward including a value rather than reasoning that it can't change.
		let text_sig = {
			use std::hash::{Hash, Hasher};
			let mut h = std::collections::hash_map::DefaultHasher::new();
			self.chrome_rev.hash(&mut h);
			gpu.gfx.config.width.hash(&mut h);
			gpu.gfx.config.height.hash(&mut h);
			margin.to_bits().hash(&mut h);
			for w in &tab_widths {
				w.to_bits().hash(&mut h);
			}
			self.menu_bar.hash(&mut h);
			self.tab_bar_visible().hash(&mut h);
			self.tabs.active.hash(&mut h);
			self.tab_edit.as_ref().map(|e| e.tab).hash(&mut h); // moves the label into its box
			self.focused.hash(&mut h); // dims the copy-mode labels
			scrim_on.hash(&mut h);
			// one pointer covers every setting: a change swaps the whole snapshot
			(std::sync::Arc::as_ptr(&cfg) as usize).hash(&mut h);
			for (id, p) in &self.tabs.cur().panes {
				id.hash(&mut h);
				p.shape_rev.hash(&mut h); // bumped by every full re-shape
				tops[id].to_bits().hash(&mut h);
				for v in [p.rect.x, p.rect.y, p.rect.w, p.rect.h] {
					v.to_bits().hash(&mut h);
				}
				match &slides[id] {
					None => 0u8.hash(&mut h),
					Some(s) => {
						1u8.hash(&mut h);
						s.has_band.hash(&mut h);
						s.has_top_band.hash(&mut h);
						for v in [
							s.band_top,
							s.split_y,
							s.top_split_y,
							s.region_clip_t,
							s.region_clip_b,
						] {
							v.to_bits().hash(&mut h);
						}
					}
				}
			}
			h.finish()
		};
		let prep = crate::perf::mark();
		let text_same = self.text_sig == Some(text_sig);

		// All rect instances and the bg-image shader work in absolute
		// framebuffer pixels (matching the glyphon viewport), so the resolution
		// is the whole window - NOT the content `area`, which is shorter by the
		// menu/tab bars and would shift cell bg + cursor down relative to text.
		let (frame_w, frame_h) = (gpu.gfx.config.width as f32, gpu.gfx.config.height as f32);
		self.text
			.update_viewport(&gpu.gfx.queue, gpu.gfx.config.width, gpu.gfx.config.height);
		self.text.set_text_blend(
			&gpu.gfx.queue,
			crate::text::text_blend(cfg.fg, cfg.bg, cfg.text_dark_on_light),
		);
		gpu.rects.set_resolution(&gpu.gfx.queue, frame_w, frame_h);
		// How the picture is mixed with the background. Light mode needs a different
		// blend, not a different number (visibility.rs). Re-read per frame: the mode
		// can flip under a running window and nothing reloads a wallpaper for it.
		let wp_mix = gpu.wallpaper_img.as_ref().map(|img| {
			crate::visibility::wallpaper_mix(
				&cfg,
				img.opacity(),
				cfg.wallpaper_summary.map(|s| s.picture()),
			)
		});
		let wp_writes_fill = wp_mix.is_some_and(|m| m.perceptual);
		if let (Some(img), Some(mix)) = (&gpu.wallpaper_img, wp_mix) {
			img.set_look(&gpu.gfx.queue, frame_w, frame_h, mix, pane_bg);
		}
		gpu.rects
			.upload(&gpu.gfx.device, &gpu.gfx.queue, &instances);

		// Nothing that feeds the text changed, so glyphon's prepared buffers from
		// the last frame still describe this one exactly. Skipping the whole area
		// build + both prepares is the point of the signature: shaping and
		// glyph-cache lookups are over half the per-frame cost, and an idle cursor
		// pulse repeats them 30x a second for no visual difference.
		if !text_same {
			let chrome = self.chrome.as_ref().unwrap(); // ensured above
			let mut areas: Vec<TextArea> = Vec::new();
			for p in self.tabs.cur().panes.values() {
				// app-scroll slide: fill the revealed gap from the scrolled-off strip,
				// draw the current scroll region over it, then the static bands unshifted
				match &slides[&p.id] {
					Some(slide) => {
						if let Some(strip) = p.strip_text_area(slide, margin) {
							areas.push(strip);
						}
						areas.push(p.text_area_band(
							tops[&p.id],
							margin,
							slide.region_clip_t,
							slide.region_clip_b,
						));
						if slide.has_top_band {
							areas.push(p.text_area_band(
								slide.band_top,
								margin,
								f32::MIN,
								slide.top_split_y,
							));
						}
						if slide.has_band {
							areas.push(p.text_area_band(
								slide.band_top,
								margin,
								slide.split_y,
								f32::MAX,
							));
						}
					}
					None => areas.push(p.text_area(tops[&p.id], margin)),
				}
				areas.extend(p.glyph_areas(margin));
				areas.extend(p.emoji_area(margin));
			}
			if self.menu_bar {
				for (i, buf) in chrome.menubar.iter().enumerate() {
					let Some((left, left_bound, right_bound, top)) =
						menubar_text_slot(&self.text, menu_h, &bar_layout, copyboxes.as_ref(), i)
					else {
						continue;
					};
					// trailing buffers are the copy-mode labels - dim them off-focus
					let color = if i < bar_layout.len() {
						menu_fg
					} else {
						copy_label_fg
					};
					areas.push(TextArea {
						buffer: buf,
						left,
						top,
						scale: 1.0,
						bounds: TextBounds {
							left: left_bound as i32,
							top: 0,
							right: right_bound as i32,
							bottom: menu_h as i32,
						},
						default_color: color,
						custom_glyphs: &[],
					});
				}
			}
			let editing = self.tab_edit.as_ref().map(|e| e.tab);
			let mut x = 0.0;
			for (slot, (_, _, buf)) in chrome.tabs.iter().enumerate() {
				let tab_w = tab_widths.get(slot).copied().unwrap_or(0.0);
				let close_x = x + tab_w - self.text.dip(TAB_CLOSE_W);
				// A tab being renamed reads inside its own box, at the same left
				// inset as the label it replaced.
				let field = (editing == Some(self.tab_layout.first + slot)).then(|| {
					tab_edit_box(
						x,
						tab_w,
						tab_bar_y,
						tab_h,
						self.text.ui_line_h,
						self.text.scale,
					)
				});
				let (left, clip_l, clip_r) = match field {
					Some(f) => (
						f.x + self.text.dip(TAB_EDIT_PAD),
						f.x + self.text.dip(TAB_EDIT_PAD),
						f.x + f.w - self.text.dip(TAB_EDIT_PAD),
					),
					None => (x + self.text.dip(TAB_TITLE_PAD), x, close_x),
				};
				let (btn_y, btn_h) = tab_button_v(tab_bar_y, tab_h, self.text.scale);
				areas.push(TextArea {
					buffer: buf,
					left,
					// a title is a path, so center its whole ink box in the button
					top: self.text.ui_ink_top(btn_y, btn_h),
					scale: 1.0,
					bounds: TextBounds {
						left: clip_l as i32,
						top: tab_bar_y as i32,
						right: clip_r as i32, // leave room for the close "X"
						bottom: (tab_bar_y + tab_h) as i32,
					},
					default_color: menu_fg,
					custom_glyphs: &[],
				});
				// the close "X" itself is a shader-drawn rect instance (tab bar pass)
				x += tab_w;
			}

			if let Err(e) = self.text.prepare(&gpu.gfx.device, &gpu.gfx.queue, areas) {
				// Atlas full (after a long session of varied glyphs). The normal per-frame
				// trim is at the END of render, below this early return - so without
				// trimming here the atlas never recovers and ALL text goes black for good
				// (cursor/cell-bg quads use a separate renderer, so they still show). Trim
				// now to free space; the next frame re-prepares with room and recovers.
				eprintln!(
					"{}: text prepare failed; trimming atlas to recover: {e:?}",
					config::APP_NAME
				);
				self.text.trim_atlas();
				self.text_sig = None;
				self.scrim_sig = None;
				return animating;
			}
			// scrim source pass has its own prepared set: pane text only (no chrome),
			// with de-bolded buffers where a pane built one (text_scrim_regular_weight)
			if scrim_on {
				let mut scrim_areas: Vec<TextArea> = Vec::new();
				for p in self.tabs.cur().panes.values() {
					// scrim follows the current frame's slide, INCLUDING the scrolled-off
					// strip filling the reveal gap - without it the strip's text (e.g. the
					// row just below a static header) loses its readability halo mid-slide
					// and the halo "pops" when the slide settles, reading as a shadow that
					// jumps at the band boundary. The strip holds only region rows, so it
					// is always scrim-safe (no furniture to guard out of the scrim).
					match &slides[&p.id] {
						Some(slide) => {
							if let Some(strip) = p.strip_text_area(slide, margin) {
								scrim_areas.push(strip);
							}
							scrim_areas.push(p.scrim_text_area_band(
								tops[&p.id],
								margin,
								slide.region_clip_t,
								slide.region_clip_b,
							));
							if slide.has_top_band {
								scrim_areas.push(p.scrim_text_area_band(
									slide.band_top,
									margin,
									f32::MIN,
									slide.top_split_y,
								));
							}
							if slide.has_band {
								scrim_areas.push(p.scrim_text_area_band(
									slide.band_top,
									margin,
									slide.split_y,
									f32::MAX,
								));
							}
						}
						None => scrim_areas.push(p.scrim_text_area(tops[&p.id], margin)),
					}
					scrim_areas.extend(p.glyph_areas(margin));
					scrim_areas.extend(p.emoji_area(margin));
				}
				if let Err(e) =
					self.text
						.prepare_scrim(&gpu.gfx.device, &gpu.gfx.queue, scrim_areas)
				{
					eprintln!(
						"{}: scrim prepare failed; trimming atlas to recover: {e:?}",
						config::APP_NAME
					);
					self.text.trim_atlas();
					self.text_sig = None;
					self.scrim_sig = None;
					return animating;
				}
			}
		} // !text_same
		self.text_sig = Some(text_sig);

		// lay out the menu into the overlay renderer: one proportional buffer
		// per item label (at the gutter), plus a checkmark buffer for checked toggles.
		// Re-shaping every frame while a menu sits open (the cursor blink keeps
		// frames coming) is skippable: the overlay text only depends on the menu's
		// geometry/labels/color. Only skip alongside text_same - the end-of-frame
		// atlas trim (which runs when !text_same) drops glyphs the retained overlay
		// vertex buffers still reference, so a trim frame must re-prepare.
		if self.menu.is_some()
			|| tip_layout.is_some()
			|| menu_tip.is_some()
			|| bench_banner.is_some()
		{
			let overlay_sig = {
				use std::hash::{Hash, Hasher};
				let mut h = std::collections::hash_map::DefaultHasher::new();
				gpu.gfx.config.width.hash(&mut h);
				gpu.gfx.config.height.hash(&mut h);
				self.chrome_rev.hash(&mut h); // covers a menu color change
				for (_, placed) in tip_layout
					.iter()
					.chain(menu_tip.iter())
					.chain(bench_banner.iter())
				{
					for (left, top, line) in placed {
						left.to_bits().hash(&mut h);
						top.to_bits().hash(&mut h);
						line.hash(&mut h);
					}
				}
				for menu in self.menu.iter().flat_map(ContextMenu::chain) {
					menu.x.to_bits().hash(&mut h);
					menu.y.to_bits().hash(&mut h);
					menu.w.to_bits().hash(&mut h);
					menu.item_h.to_bits().hash(&mut h);
					for entry in &menu.entries {
						if let Some(label) = entry_label(entry) {
							label.hash(&mut h);
						}
						if let Entry::Item { check, .. } = entry {
							check.hash(&mut h);
						}
					}
				}
				h.finish()
			};
			if text_same && self.overlay_sig == Some(overlay_sig) {
				// prepared overlay from the last frame still matches
			} else {
				self.overlay_sig = Some(overlay_sig);
				// (left, top, buffer) collected first so the borrow of self.text ends
				let mut specs: Vec<(f32, f32, Buffer)> = Vec::new();
				let mut attrs = crate::text::ui_attrs();
				let fg = config::menu_fg();
				attrs.color_opt = Some(GColor::rgb(fg[0], fg[1], fg[2]));
				let tip_fg = config::tip_fg();
				let tip_col = Some(GColor::rgb(tip_fg[0], tip_fg[1], tip_fg[2]));
				// The tip alone shapes in the terminal font: its lines are a table
				// padded with spaces, which no proportional face can align.
				if let Some((box_rect, placed)) = &tip_layout {
					let mut tip_attrs = crate::text::mono_attrs();
					tip_attrs.color_opt = tip_col;
					let line_h = self.text.cell_h;
					for (left, top, line) in placed {
						let mut buf = self.text.new_buffer(box_rect.w, line_h);
						buf.set_text(
							&mut self.text.font_system,
							line,
							&tip_attrs,
							Shaping::Advanced,
							None,
						);
						buf.shape_until_scroll(&mut self.text.font_system, false);
						specs.push((*left, *top, buf));
					}
				}
				// a menu row's tip and the benchmark banner shape in the interface
				// font, like the rows and the chrome they sit among. The tip takes the
				// tip's text color with its box; the banner stays with the menu.
				let ui_lines = menu_tip
					.iter()
					.map(|(rect, placed)| (rect, placed, tip_col))
					.chain(
						bench_banner
							.iter()
							.map(|(rect, placed)| (rect, placed, attrs.color_opt)),
					);
				for (box_rect, placed, color) in ui_lines {
					let mut attrs = attrs.clone();
					attrs.color_opt = color;
					for (left, top, line) in placed {
						let mut buf = self.text.new_ui_buffer(box_rect.w, self.text.ui_line_h);
						buf.set_text(
							&mut self.text.font_system,
							line,
							&attrs,
							Shaping::Advanced,
							None,
						);
						buf.shape_until_scroll(&mut self.text.font_system, false);
						specs.push((*left, *top, buf));
					}
				}
				for menu in self.menu.iter().flat_map(ContextMenu::chain) {
					for (i, entry) in menu.entries.iter().enumerate() {
						let Some(label) = entry_label(entry) else {
							continue;
						};
						let top = menu.row_top(i) + (menu.item_h - self.text.ui_line_h) / 2.0;
						let mut buf = self.text.new_ui_buffer(menu.w, menu.item_h);
						buf.set_text(
							&mut self.text.font_system,
							label,
							&attrs,
							Shaping::Advanced,
							None,
						);
						buf.shape_until_scroll(&mut self.text.font_system, false);
						let pad_x = self.text.dip(config::MENU_PAD_X);
						let gutter = self.text.dip(config::MENU_GUTTER);
						specs.push((menu.x + pad_x + gutter, top, buf));
						if matches!(
							entry,
							Entry::Item {
								check: Some(true),
								..
							}
						) {
							let mut check_buf = self.text.new_ui_buffer(gutter, menu.item_h);
							check_buf.set_text(
								&mut self.text.font_system,
								"\u{2713}",
								&attrs,
								Shaping::Advanced,
								None,
							);
							check_buf.shape_until_scroll(&mut self.text.font_system, false);
							specs.push((menu.x + pad_x, top, check_buf));
						}
					}
				}
				let (sw, sh) = (gpu.gfx.config.width as i32, gpu.gfx.config.height as i32);
				let menu_color = GColor::rgb(fg[0], fg[1], fg[2]);
				let areas: Vec<TextArea> = specs
					.iter()
					.map(|(left, top, buf)| TextArea {
						buffer: buf,
						left: *left,
						top: *top,
						scale: 1.0,
						bounds: TextBounds {
							left: 0,
							top: 0,
							right: sw,
							bottom: sh,
						},
						default_color: menu_color,
						custom_glyphs: &[],
					})
					.collect();
				let _ = self
					.text
					.prepare_overlay(&gpu.gfx.device, &gpu.gfx.queue, areas);
			}
		}

		crate::perf::since(&crate::perf::PREP_NS, prep);
		let acquire = crate::perf::mark();
		let Some(frame) = gpu.gfx.begin_frame() else {
			return animating;
		};
		crate::perf::since(&crate::perf::ACQUIRE_NS, acquire);
		let encode = crate::perf::mark();
		let view = gpu.gfx.frame_view(&frame);
		let mut encoder = gpu
			.gfx
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("frame"),
			});

		// Text readability scrim: build the per-pixel color map, render the prepared
		// text to the scrim texture, blur it, then composite under the crisp text.
		// "Softness" 0..1 -> coverage boost: 0 = hard/solid (x10), 1 = soft/faint (x1)
		let scrim_intensity = 10.0 - cfg.text_scrim_softness.clamp(0.0, 1.0) * 9.0;
		// falloff curve index: 0 sigmoid, 1 half-normal, 2 linear, 3 log, 4 exp
		let scrim_ramp = match cfg.text_scrim_ramp.as_str() {
			"half_normal" => 1.0,
			"linear" => 2.0,
			"log" => 3.0,
			"exp" => 4.0,
			_ => 0.0, // "sigmoid"
		};
		// "Strength" 0..100% -> doublings of the finished halo alpha (0 = as built),
		// so the top of the slider is x32. In light mode the halo is a pale plate
		// on whatever the picture darkened, which reads harder than dark mode's
		// does at the same alpha, so it gives back a fraction of a doubling
		// (visibility.rs). The gain is 1 in dark mode and with no picture up.
		let shown_wallpaper = gpu
			.wallpaper_img
			.as_ref()
			.map_or(0.0, ImageRenderer::opacity);
		let scrim_strength = cfg.text_scrim_strength.clamp(0.0, 100.0) / SCRIM_PCT_PER_DOUBLING
			+ crate::visibility::halo_gain(&cfg, shown_wallpaper).log2();
		// build function index: 0 dilate, 1 sdf, 2 dt, 3 gaussian (legacy blur)
		let scrim_function = match cfg.text_scrim_function.as_str() {
			"dilate" => 0.0,
			"dt" => 2.0,
			"gaussian" => 3.0,
			_ => 1.0, // "sdf"
		};
		// distance paths measure the halo extent in px; keep it a touch wider than
		// the (sigma-based) gaussian look so switching functions doesn't shrink it.
		let scrim_ext = crate::scrim::clamp_ext(cfg.text_scrim_radius * 2.0);
		// The halo is built from the text alone - the cursor lives in its own
		// coverage texture and only joins at the blur (cursor_scrim) or the
		// composite (cursor_outline). So when the text is unchanged the color map,
		// the text-coverage pass and the blur can all be reused from last frame;
		// every scrim texture is stored, not transient. That is most of the idle
		// GPU cost. The blur still has to re-run if the cursor feeds it.
		let scrim_cached = scrim_on && text_same && self.scrim_sig == Some(text_sig);
		let blur_cached = scrim_cached && !cfg.cursor_scrim;
		if scrim_on {
			if !scrim_cached {
				gpu.scrim.render_bgcolor(
					&gpu.gfx.device,
					&gpu.gfx.queue,
					&mut encoder,
					&scrim_cells,
					config::srgb_f32(cfg.bg),
				);
			}
			if cfg.cursor_scrim || cfg.cursor_outline {
				gpu.scrim
					.upload_cursors(&gpu.gfx.device, &gpu.gfx.queue, &scrim_cursor_quads);
			}
			if !scrim_cached {
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("scrim text"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: gpu.scrim.text_view(),
						resolve_target: None,
						depth_slice: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: None,
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				let _ = self.text.render_scrim(&mut pass);
			}
			// cursor coverage in its own texture (kept apart from the text so the
			// halo and the outline can each include it independently). Skipped -
			// full-res clear included - when neither flag samples it.
			if cfg.cursor_scrim || cfg.cursor_outline {
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("scrim cursor"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: gpu.scrim.cursor_view(),
						resolve_target: None,
						depth_slice: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: None,
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				gpu.scrim.draw_cursors(&mut pass);
			}
			if halo_on && !blur_cached {
				gpu.scrim.blur(
					&gpu.gfx.queue,
					&mut encoder,
					cfg.text_scrim_radius,
					scrim_ext,
					scrim_ramp,
					if cfg.cursor_scrim { 1.0 } else { 0.0 },
					scrim_function,
				);
			}
			self.scrim_sig = Some(text_sig);
		} else {
			self.scrim_sig = None;
		}

		{
			let divider = config::srgb_f32(config::DIVIDER);
			// transparent base only when the background is actually see-through
			// (same gate as bg_alpha): pane-gap dividers then show the desktop.
			// Otherwise the clear must be opaque - the X11 window is always an
			// ARGB visual, so any alpha<1 pixel (the 1px divider slits, AA edges
			// of fractional pane rects) lets the compositor blend the desktop
			// through as bright speckles along the split lines.
			let clear = if gpu.gfx.transparent && cfg.transparent_background {
				wgpu::Color::TRANSPARENT
			} else {
				wgpu::Color {
					r: divider[0] as f64,
					g: divider[1] as f64,
					b: divider[2] as f64,
					a: 1.0,
				}
			};
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("main pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});

			let (sw, sh) = (gpu.gfx.config.width, gpu.gfx.config.height);
			// pane backgrounds (exactly pane-sized, no clip needed) - skipped where the
			// wallpaper writes them itself
			if !wp_writes_fill {
				gpu.rects.draw(&mut pass, 0..under_len);
			}
			// background image over the pane fill, under cells/text
			if let Some(img) = &gpu.wallpaper_img {
				if wp_writes_fill {
					// The mix replaces the fill rather than blending over it, so it has
					// to stop at the pane edge - otherwise the divider slits between
					// panes would take the pane's background color instead of their own.
					for full in &pane_fulls {
						let (x, y, w, h) = scissor(*full, sw, sh);
						if w == 0 || h == 0 {
							continue;
						}
						pass.set_scissor_rect(x, y, w, h);
						img.draw(&mut pass);
					}
					pass.set_scissor_rect(0, 0, sw, sh);
				} else {
					img.draw(&mut pass);
				}
			}
			// per-pane cell bg + cursor, clipped to the pane
			for (rect, start, end) in &group_ranges {
				let (x, y, w, h) = scissor(*rect, sw, sh);
				if w == 0 || h == 0 {
					continue;
				}
				pass.set_scissor_rect(x, y, w, h);
				gpu.rects.draw(&mut pass, *start..*end);
			}
			pass.set_scissor_rect(0, 0, sw, sh);
			// minimap previews over the pane fill and the wallpaper, under the
			// marker and thumb (which ride with the scrollbars, after the text)
			if cfg.minimap {
				for id in self.tabs.cur().panes.keys() {
					gpu.minimap.draw(&mut pass, *id);
				}
			}
			// menu/tab-bar quads before the text so their titles draw on top
			if let Some((start, end)) = menubar_range {
				gpu.rects.draw(&mut pass, start..end);
			}
			if let Some((start, end)) = tabbar_range {
				gpu.rects.draw(&mut pass, start..end);
			}
			// scrim goes under the crisp text, over the cell backgrounds. Clip it to
			// the content area so the halo only affects terminal text, never the
			// menu bar / tab titles above it.
			if scrim_on {
				// frame-invariant composite args: upload the uniform once, not per pane
				gpu.scrim.write_comp_uniform(
					&gpu.gfx.queue,
					scrim_intensity,
					cfg.text_outline,
					if cfg.cursor_outline { 1.0 } else { 0.0 },
					scrim_function,
					scrim_ramp,
					scrim_ext,
					scrim_strength,
					if halo_on { 1.0 } else { 0.0 },
				);
				// The scrim is a full-frame blur - each glyph's halo spreads ~scrim_ext
				// px every direction. Composite it PER-PANE, clipped per-side: an edge that
				// borders ANOTHER pane (internal divider) clips at the content edge (rect
				// inset by the margin) so the halo can't reach the inter-pane gutter - the
				// "garbage around split lines"; an edge at the WINDOW border clips at the rect
				// edge so the outer halo still fills the window margin. The gutter (margin +
				// gap + margin) is wider than the halo reach, so no pane's halo touches a
				// neighbor's content region.
				let area = self.area();
				for (rect, _, _) in &group_ranges {
					// external = sits on the content-area boundary (window edge); otherwise it
					// borders a gap/another pane -> pull the clip in by the margin.
					let l = if rect.x <= area.x + 0.5 {
						rect.x
					} else {
						rect.x + margin
					};
					let t = if rect.y <= area.y + 0.5 {
						rect.y
					} else {
						rect.y + margin
					};
					let r = if rect.x + rect.w >= area.x + area.w - 0.5 {
						rect.x + rect.w
					} else {
						rect.x + rect.w - margin
					};
					let b = if rect.y + rect.h >= area.y + area.h - 0.5 {
						rect.y + rect.h
					} else {
						rect.y + rect.h - margin
					};
					let clip = Rect {
						x: l,
						y: t,
						w: (r - l).max(0.0),
						h: (b - t).max(0.0),
					};
					let (cx, cy, cw, ch) = scissor(clip, sw, sh);
					if cw == 0 || ch == 0 {
						continue;
					}
					pass.set_scissor_rect(cx, cy, cw, ch);
					gpu.scrim.composite(&mut pass);
				}
				pass.set_scissor_rect(0, 0, sw, sh);
			}
			// link underlines above the scrim (halo can't eat them), under the cursor
			for (rect, start, end) in &link_ranges {
				let (x, y, w, h) = scissor(*rect, sw, sh);
				if w == 0 || h == 0 {
					continue;
				}
				pass.set_scissor_rect(x, y, w, h);
				gpu.rects.draw(&mut pass, *start..*end);
			}
			// cursor above the scrim (halo can't obscure it), still under the crisp text
			for (rect, start, end) in &cursor_ranges {
				let (x, y, w, h) = scissor(*rect, sw, sh);
				if w == 0 || h == 0 {
					continue;
				}
				pass.set_scissor_rect(x, y, w, h);
				gpu.rects.draw(&mut pass, *start..*end);
			}
			pass.set_scissor_rect(0, 0, sw, sh);
			if let Err(e) = self.text.render(&mut pass) {
				eprintln!("{}: text render failed: {e:?}", config::APP_NAME);
			}
			gpu.rects.draw(&mut pass, ring_start..ring_end);
		}

		// second pass: context menu / menu-bar dropdown on top (preserves main pass)
		if let Some((mstart, mend)) = overlay_range {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("overlay pass"),
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
			gpu.rects.draw(&mut pass, mstart..mend);
			let _ = self.text.render_overlay(&mut pass);
		}

		gpu.minimap.end_frame();
		crate::perf::since(&crate::perf::ENCODE_NS, encode);
		let submit = crate::perf::mark();
		gpu.gfx.queue.submit(Some(encoder.finish()));
		gpu.gfx.end_frame(frame);
		crate::perf::since(&crate::perf::SUBMIT_NS, submit);
		crate::perf::painted();
		// The window was created hidden; reveal it once a real frame is on screen at
		// the final size (no default-size/blank flash). reveal_want (async resize)
		// holds off until the surface reaches the grid size; the deadline is a hard
		// fallback so a WM that grants a different size can't leave it stuck hidden.
		if !self.revealed {
			let settled = self.reveal_want.is_none_or(|w| {
				gpu.gfx.config.width == w.width && gpu.gfx.config.height == w.height
			});
			if settled || Instant::now() >= self.reveal_deadline {
				self.revealed = true;
				self.window.set_visible(true);
				self.shell_scan_at = Some(Instant::now() + SHELL_SCAN_DELAY);
				self.shell_scan_cap = Some(Instant::now() + SHELL_SCAN_MAX_WAIT);
				if self.bench_id.is_some() {
					self.bench_at = Some(Instant::now() + BENCH_DELAY);
					self.bench_cap = Some(Instant::now() + BENCH_MAX_WAIT);
					self.bench_banner = Some(Instant::now());
				}
			}
		}
		// This frame carried whatever the wallpaper worker answered, so the scan's
		// clock starts from HERE rather than from the reveal - a wallpaper that took
		// two seconds to decode used to have the scan running underneath it. Only
		// ever pushed back, never re-armed: once the scan has gone (shell_scan_at
		// cleared, which the backstop guarantees) a late wallpaper must not start a
		// second one.
		if self.revealed && self.wp_answered && !self.wp_shown {
			self.wp_shown = true;
			self.shell_scan_at = push_back(
				self.shell_scan_at,
				self.shell_scan_cap,
				Instant::now() + SHELL_SCAN_DELAY,
			);
			self.bench_at = push_back(self.bench_at, self.bench_cap, Instant::now() + BENCH_DELAY);
		}
		if env_flag("SILK_DUMP") {
			gpu.gfx.dump_offscreen("/tmp/silk_offscreen.png");
		}
		// Trim only on a frame that prepared. The trim clears glyphon's in-use set,
		// and a later allocation evicts whatever isn't in it - so trimming after a
		// skipped prepare would let the atlas drop glyphs the retained buffers are
		// still pointing at.
		if !text_same {
			self.text.trim_atlas();
		}
		animating
	}
}

fn rect_inst(x: f32, y: f32, w: f32, h: f32, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [x, y],
		size: [w, h],
		color: config::srgb_f32(color),
		..Default::default()
	}
}

// The close-"X" mark: a shader-drawn quad (mode 1) whose two diagonal bars
// center exactly in `cb` at any size/DPI. Stroke scales with the box.
fn close_x_inst(cb: Rect, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [cb.x, cb.y],
		size: [cb.w, cb.h],
		color: config::srgb_f32(color),
		params: [1.0, (cb.w * 0.14).max(1.4)],
	}
}

// The submenu arrow: a shader-drawn quad (mode 3) holding a right-pointing
// triangle that centers exactly in `at` at any size and DPI.
fn sub_arrow_inst(at: Rect, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [at.x, at.y],
		size: [at.w, at.h],
		color: config::srgb_f32(color),
		params: [3.0, 0.0],
	}
}

// One scrollbar piece: a rounded quad (mode 2) with the pill radius its short
// side implies, at `alpha` (the pane's fade times the piece's own weight).
fn bar_inst(r: Rect, color: [u8; 3], alpha: f32) -> RectInstance {
	let mut c = config::srgb_f32(color);
	c[3] = alpha;
	RectInstance {
		pos: [r.x, r.y],
		size: [r.w, r.h],
		color: c,
		params: [2.0, r.w.min(r.h) * 0.5],
	}
}

// The minimap's viewport marker. The pane's own scrollbar sits at the far edge,
// over the preview, so the column draws no bar of its own.
fn minimap_insts(g: &crate::minimap::Geom, active: bool) -> Option<RectInstance> {
	let cfg = config::settings();
	let handle = g.handle?;
	let alpha = if active {
		config::SCROLLBAR_ACTIVE_A
	} else {
		config::SCROLLBAR_IDLE_A
	};
	// a wash, not a lid: it marks where you are without hiding what is under it
	Some(RectInstance {
		pos: [handle.x, handle.y],
		size: [g.preview.w, handle.h],
		color: {
			let mut c = config::srgb_f32(cfg.scrollbar_thumb);
			c[3] = alpha * 0.28;
			c
		},
		..Default::default()
	})
}

// The scrollbar's quads for one pane: a faint track with the handle on it. The
// thumb brightens while hovered or dragged, the usual affordance.
fn scrollbar_insts(bar: &crate::pane::Bar, fade: f32, active: bool) -> [RectInstance; 2] {
	let cfg = config::settings();
	let thumb_a = if active {
		config::SCROLLBAR_ACTIVE_A
	} else {
		config::SCROLLBAR_IDLE_A
	};
	[
		bar_inst(
			bar.track,
			cfg.scrollbar_trough,
			fade * config::SCROLLBAR_TROUGH_A,
		),
		bar_inst(bar.thumb, cfg.scrollbar_thumb, fade * thumb_a),
	]
}

// close-"X" stroke color: menu fg dimmed toward the tab bg (~0.6), so it reads
// as a quiet button mark rather than a title character
fn close_x_rgb() -> [u8; 3] {
	let fg = config::menu_fg();
	let dim = |v: u8| ((v as u16 * 3) / 5) as u8;
	[dim(fg[0]), dim(fg[1]), dim(fg[2])]
}

fn mix_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
	let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
	[mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

// Dim a chrome color toward the bar background when the window isn't focused;
// used on the copy-mode checkboxes + labels to signal auto-copy is inert until
// the window regains focus (the pane's flags stay set meanwhile). Focused = no
// change.
fn copy_dim(color: [u8; 3], focused: bool) -> [u8; 3] {
	if focused {
		return color;
	}
	let bg = config::TAB_BAR_BG;
	let mix = |a: u8, b: u8| (a as f32 * 0.4 + b as f32 * 0.6) as u8;
	[
		mix(color[0], bg[0]),
		mix(color[1], bg[1]),
		mix(color[2], bg[2]),
	]
}

// X11 session? (Per-pixel transparency needs the glutin GL path only on X11;
// Wayland's wgpu surface already does premultiplied alpha.)
fn is_x11(el: &ActiveEventLoop) -> bool {
	use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
	el.owned_display_handle()
		.display_handle()
		.is_ok_and(|handle| {
			matches!(
				handle.as_raw(),
				RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_)
			)
		})
}

// Stable X11 WM_CLASS (+ Wayland app_id) so the window is identifiable to the
// WM/taskbar and matchable in compositor rules - e.g. Compiz's blur "Blur
// Windows" = class=SilkTerm. winit's with_name(general, instance) yields
// WM_CLASS = "instance", "general", so res_class="SilkTerm", res_name="silkterm".
#[cfg(target_os = "linux")]
fn with_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
	use winit::platform::wayland::WindowAttributesExtWayland;
	use winit::platform::x11::WindowAttributesExtX11;
	let attrs = WindowAttributesExtX11::with_name(attrs, "SilkTerm", "silkterm");
	WindowAttributesExtWayland::with_name(attrs, "SilkTerm", "silkterm")
}
#[cfg(not(target_os = "linux"))]
fn with_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
	attrs
}

// Ask a KWin/picom-style compositor to blur the desktop behind the window's
// translucent regions (frosted glass) via _KDE_NET_WM_BLUR_BEHIND_REGION: a
// single 0 cardinal = blur the whole window, deleting the property turns it off.
// X11-only and compositor-dependent - Compiz/GNOME ignore the hint (there the
// user enables blur in the compositor), and the compositor, not us, owns the
// blur radius. Opens a throwaway connection; called only at startup / on toggle.
#[cfg(target_os = "linux")]
fn set_blur_behind(window: &Window, enable: bool) {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use x11rb::connection::Connection;
	use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
	use x11rb::wrapper::ConnectionExt as _;

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let xid = match handle.as_raw() {
		RawWindowHandle::Xlib(h) => h.window as u32,
		RawWindowHandle::Xcb(h) => h.window.get(),
		_ => return, // not X11 (Wayland/other): the hint is X11-only
	};
	let Ok((conn, _)) = x11rb::connect(None) else {
		return;
	};
	let Ok(cookie) = conn.intern_atom(false, b"_KDE_NET_WM_BLUR_BEHIND_REGION") else {
		return;
	};
	let Ok(reply) = cookie.reply() else {
		return;
	};
	let atom = reply.atom;
	if enable {
		let _ = conn.change_property32(PropMode::REPLACE, xid, atom, AtomEnum::CARDINAL, &[0u32]);
	} else {
		let _ = conn.delete_property(xid, atom);
	}
	let _ = conn.flush();
}
#[cfg(not(target_os = "linux"))]
fn set_blur_behind(_window: &Window, _enable: bool) {}

// wgpu's guaranteed floor for max_texture_dimension_2d. The window is born
// before the device exists, so a birth size is held to the floor, and the
// grid-derived resize after it to what the device actually reports.
const SAFE_MAX_DIM: u32 = 8192;

// The window a grid asks for: the cells, the margins either side, and the chrome
// above them. `chrome` counts the menu bar and the tab strip where they show, or
// the shell gets fewer rows than were asked for.
fn window_px(
	cols: usize,
	rows: usize,
	cell_w: f32,
	cell_h: f32,
	margin: f32,
	chrome: f32,
) -> (u32, u32) {
	(
		(cols as f32 * cell_w + 2.0 * margin).ceil() as u32,
		(rows as f32 * cell_h + 2.0 * margin + chrome).ceil() as u32,
	)
}

// A window may be no bigger than the largest texture the device will make: the
// GL path renders the scene into an offscreen texture at the window's size, and
// wgpu treats a refusal as fatal. So a count out of the config or the command
// line is held here, or it ends the launch in create_texture.
fn fit_px(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
	(w.clamp(1, max_dim), h.clamp(1, max_dim))
}

// Is this resize the size to launch at next time? Only once a frame has been
// drawn - before that it is the launch size arriving back - and never a
// fullscreen or maximized one, which is not a window to come back to.
fn remember_resize(size_tracked: bool, fullscreen: bool, maximized: bool) -> bool {
	size_tracked && !fullscreen && !maximized
}

// The window/taskbar icon, decoded from the bundled logo (downscaled so the
// _NET_WM_ICON payload stays small). The logo is wider than it is tall and every
// place an icon is shown reserves a square, so it is stretched to fill one
// rather than left floating in a band of nothing. None if it can't be decoded.
pub fn load_icon() -> Option<winit::window::Icon> {
	let img = image::load_from_memory(include_bytes!("../assets/logo.png")).ok()?;
	let img = img
		.resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
		.into_rgba8();
	let (w, h) = img.dimensions();
	winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

// A pane's shell, most specific first: its own --shell, the pane it splits, its
// tab's, the window's, then the default. The first pane of a tab has no split
// source, and a window with no tabs given has only the last two.
fn pane_shell(
	explicit: Option<&Vec<String>>,
	split_source: Option<&Vec<String>>,
	tab: Option<&Vec<String>>,
	window: Option<&Vec<String>>,
	default: impl FnOnce() -> Option<Vec<String>>,
) -> Option<Vec<String>> {
	explicit
		.or(split_source)
		.or(tab)
		.or(window)
		.cloned()
		.or_else(default)
}

// A new pane's direction: its own, else the one the pane it splits was given,
// which carries down the chain. None leaves it to `default_dir`.
fn pane_split_dir(
	explicit: Option<crate::cli::Dir4>,
	split_source: Option<crate::cli::Dir4>,
) -> Option<crate::cli::Dir4> {
	explicit.or(split_source)
}

// Build the initial tabs/panes from the parsed command line. Without
// hierarchical flags, one tab with one pane (running any window-level --shell).
fn build_layout(
	cli: &crate::cli::Cli,
	text: &mut TextCtx,
	proxy: &EventLoopProxy<UserEvent>,
	area: Rect,
) -> Vec<PaneManager> {
	use crate::cli::Size;
	// A bad --shell / default_shell (typo'd binary, PTY failure) should read
	// like the CLI parse errors, not a Rust panic + backtrace.
	// The lowest-precedence directory: None whenever a shell launched us, so the
	// directory it was in survives (see config::startup_dir).
	let start = config::startup_dir();
	// `--directory` is resolved once per place it was written, not once per pane,
	// so a path that isn't there is reported once however many panes inherit it.
	let win_dir = cli.win.style.directory.as_deref().and_then(config::cli_dir);
	let spawn = |text: &mut TextCtx, shell: Option<Vec<String>>, dir: Option<PathBuf>| {
		let dir = dir.or_else(|| start.clone());
		PaneManager::new(text, proxy, area, shell, dir).unwrap_or_else(|e| {
			eprintln!("{}: failed to start shell: {e}", config::APP_NAME);
			std::process::exit(2);
		})
	};
	// --keep-open cascades like the shell and the directory do
	let hold = |pm: &mut PaneManager, id: PaneId, keep: bool| {
		if let Some(p) = pm.panes.get_mut(&id) {
			p.keep_open = keep;
		}
	};
	if !cli.hierarchical {
		let shell = pane_shell(
			None,
			None,
			None,
			cli.win.style.shell.as_ref(),
			config::default_shell_argv,
		);
		let mut pm = spawn(text, shell, win_dir);
		let id = pm.focused;
		hold(&mut pm, id, cli.win.style.keep_open.unwrap_or(false));
		return vec![pm];
	}
	let mut out = Vec::new();
	for tab in &cli.tabs {
		// main pane's shell cascades pane -> tab -> window
		let main_shell = pane_shell(
			tab.panes[0].style.shell.as_ref(),
			None,
			tab.style.shell.as_ref(),
			cli.win.style.shell.as_ref(),
			config::default_shell_argv,
		);
		// directories cascade the same way the shells do
		let tab_dir = tab
			.style
			.directory
			.as_deref()
			.and_then(config::cli_dir)
			.or_else(|| win_dir.clone());
		let main_dir = tab.panes[0]
			.style
			.directory
			.as_deref()
			.and_then(config::cli_dir)
			.or_else(|| tab_dir.clone());
		let main_keep = tab.panes[0]
			.style
			.keep_open
			.or(tab.style.keep_open)
			.or(cli.win.style.keep_open)
			.unwrap_or(false);
		let mut pm = spawn(text, main_shell.clone(), main_dir.clone());
		let main_id = pm.focused;
		hold(&mut pm, main_id, main_keep);
		let mut handles: HashMap<String, PaneId> = HashMap::new();
		handles.insert("main".into(), main_id);
		handles.insert("0".into(), main_id);
		if let Some(handle) = &tab.panes[0].id {
			handles.insert(handle.clone(), main_id);
		}
		let mut shells: HashMap<PaneId, Option<Vec<String>>> = HashMap::new();
		shells.insert(main_id, main_shell);
		let mut dirs: HashMap<PaneId, Option<PathBuf>> = HashMap::new();
		dirs.insert(main_id, main_dir);
		let mut keeps: HashMap<PaneId, bool> = HashMap::new();
		keeps.insert(main_id, main_keep);
		// only a direction somebody gave, or inherited from one; the tab's first
		// pane has none
		let mut split_dirs: HashMap<PaneId, crate::cli::Dir4> = HashMap::new();
		let mut prev = main_id;

		for pane_spec in &tab.panes[1..] {
			let target = pane_spec
				.splits
				.as_deref()
				.and_then(|handle| handles.get(handle).copied())
				.unwrap_or(prev);
			let given_dir = pane_split_dir(pane_spec.dir, split_dirs.get(&target).copied());
			let dir4 = given_dir.unwrap_or_else(|| default_dir(&pm, target));
			let (dir, before) = match dir4 {
				crate::cli::Dir4::Down => (Dir::Horizontal, false),
				crate::cli::Dir4::Up => (Dir::Horizontal, true),
				crate::cli::Dir4::Right => (Dir::Vertical, false),
				crate::cli::Dir4::Left => (Dir::Vertical, true),
			};
			// new pane's shell: explicit -> the pane it splits -> tab -> window
			let shell = pane_shell(
				pane_spec.style.shell.as_ref(),
				shells.get(&target).and_then(Option::as_ref),
				tab.style.shell.as_ref(),
				cli.win.style.shell.as_ref(),
				config::default_shell_argv,
			);
			// and its directory: explicit -> the pane it splits -> tab -> window
			let pane_dir = pane_spec
				.style
				.directory
				.as_deref()
				.and_then(config::cli_dir)
				.or_else(|| dirs.get(&target).cloned().flatten())
				.or_else(|| tab_dir.clone());
			// and whether it is held open: explicit -> the pane it splits -> tab -> window
			let keep = pane_spec
				.style
				.keep_open
				.or_else(|| keeps.get(&target).copied())
				.or(tab.style.keep_open)
				.or(cli.win.style.keep_open)
				.unwrap_or(false);
			// no size evens out a run of same-direction splits, as a split from
			// the keyboard does
			let ratio = pane_spec.size.map(|size| match size {
				Size::Percent(pct) => pct / 100.0,
				Size::Cells(n) => {
					let rect = pm.panes.get(&target).map_or(area, |p| p.rect);
					let denom = match dir {
						Dir::Vertical => (rect.w / text.cell_w).max(1.0),
						Dir::Horizontal => (rect.h / text.cell_h).max(1.0),
					};
					n as f32 / denom
				}
			});
			if let Some(new_id) = pm.split_at(
				text,
				proxy,
				target,
				dir,
				before,
				ratio,
				shell.clone(),
				pane_dir.clone().or_else(|| start.clone()),
				area,
			) {
				if let Some(handle) = &pane_spec.id {
					handles.insert(handle.clone(), new_id);
				}
				shells.insert(new_id, shell);
				dirs.insert(new_id, pane_dir);
				keeps.insert(new_id, keep);
				if let Some(given) = given_dir {
					split_dirs.insert(new_id, given);
				}
				hold(&mut pm, new_id, keep);
				prev = new_id;
			}
		}
		// focus the tab's first pane, not the last split
		pm.focused = main_id;
		pm.title_override.clone_from(&tab.title);
		out.push(pm);
	}
	out
}

// A reload rereads the file, and the file never held what the command line gave
// at launch, so that goes back on first. The session's own state goes on after,
// which lets a wallpaper set through the socket since launch beat the one the
// launch named.
fn settings_after_reload(
	live: &config::Settings,
	mut from_disk: config::Settings,
	launch: &crate::cli::Style,
	wallpaper_locked: bool,
) -> config::Settings {
	crate::cli::fold_window_style(&mut from_disk, launch);
	config::keep_session(live, &mut from_disk, wallpaper_locked);
	from_disk
}

// What Ctrl+Shift+N starts. The settings file comes along, made absolute since
// the child runs somewhere else, and so does the pane's directory, flagged so
// the child keeps it even when it is home or a root (config::startup_dir).
// Passing `--config` alone leaves the file's own command_line in charge, as it
// is for any launch that names only a file.
fn new_window_command(
	exe: &std::path::Path,
	cwd: Option<&std::path::Path>,
	config: Option<&std::path::Path>,
) -> std::process::Command {
	let mut cmd = std::process::Command::new(exe);
	if let Some(file) = config {
		cmd.arg("--config")
			.arg(std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()));
	}
	if let Some(dir) = cwd {
		cmd.current_dir(dir).env(config::ENV_DIR_HANDED_DOWN, "1");
	}
	cmd
}

// Default split direction when none is given: split along the longer axis so the
// new pane goes where there's more room.
fn default_dir(pm: &PaneManager, target: PaneId) -> crate::cli::Dir4 {
	default_dir_for(pm.panes.get(&target).map(|p| p.rect))
}
fn default_dir_for(rect: Option<Rect>) -> crate::cli::Dir4 {
	match rect {
		Some(rect) if rect.h > rect.w => crate::cli::Dir4::Down,
		_ => crate::cli::Dir4::Right,
	}
}

// Open a URL in the user's default browser (fire-and-forget, per platform).
fn open_url(url: &str) {
	let mut cmd = if cfg!(target_os = "macos") {
		let mut command = std::process::Command::new("open");
		command.arg(url);
		command
	} else if cfg!(target_os = "windows") {
		let mut command = std::process::Command::new("cmd");
		command.args(["/C", "start", "", url]);
		command
	} else {
		let mut command = std::process::Command::new("xdg-open");
		command.arg(url);
		command
	};
	let _ = cmd.spawn();
}

// Decode the configured background image and upload it to a texture.

// Hand a clicked link to the desktop. A failure is the opener's (no xdg-open, a
// bad open_command) and is worth saying out loud once, not worth an alert.
fn open_link(url: &str) {
	let cfg = config::settings();
	if let Err(e) = crate::links::open(url, &cfg.hyperlink_open_command) {
		eprintln!("{}: could not open {url}: {e}", config::APP_NAME);
	}
}

// clamp a pane rect to an integer scissor box inside the surface
fn scissor(rect: Rect, sw: u32, sh: u32) -> (u32, u32, u32, u32) {
	let x = rect.x.max(0.0).min(sw as f32) as u32;
	let y = rect.y.max(0.0).min(sh as f32) as u32;
	let right = (rect.x + rect.w).max(0.0).min(sw as f32) as u32;
	let bottom = (rect.y + rect.h).max(0.0).min(sh as f32) as u32;
	(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

fn focus_ring(rect: Rect, scale: f32) -> [RectInstance; 4] {
	// the calm one: the ring marks which pane is live, alongside the dialog's own
	// sliders and revert arrows, rather than the single keyboard-focused control
	let color = config::srgb_f32(config::settings().highlight);
	let thickness = config::dip(config::FOCUS_RING_PX, scale);
	[
		RectInstance {
			pos: [rect.x, rect.y],
			size: [rect.w, thickness],
			color,
			..Default::default()
		},
		RectInstance {
			pos: [rect.x, rect.y + rect.h - thickness],
			size: [rect.w, thickness],
			color,
			..Default::default()
		},
		RectInstance {
			pos: [rect.x, rect.y],
			size: [thickness, rect.h],
			color,
			..Default::default()
		},
		RectInstance {
			pos: [rect.x + rect.w - thickness, rect.y],
			size: [thickness, rect.h],
			color,
			..Default::default()
		},
	]
}

impl ApplicationHandler<UserEvent> for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.state.is_some() {
			return;
		}
		let cli_win = &self.cli.win;
		let decorated = !cli_win.hide_frame.unwrap_or(false);
		let menu_bar = !cli_win.hide_menu.unwrap_or(false);
		let win_title = cli_win.title.clone();
		let win_opacity = cli_win.opacity;
		// When both pixel dims are given, the window must be BORN at that size, not
		// resized into it: some EGL presents (VirtualGL's, for one) latch the surface
		// size at creation and never see later resizes, leaving a stale-offset blit.
		// Held to the floor every device meets, since the real limit is not known
		// until one exists; the grid-derived resize below uses that limit.
		let initial_size: winit::dpi::Size = match (cli_win.pixel_width, cli_win.pixel_height) {
			(Some(w), Some(h)) => {
				let (w, h) = fit_px(w, h, SAFE_MAX_DIM);
				winit::dpi::PhysicalSize::new(w, h).into()
			}
			_ => winit::dpi::LogicalSize::new(1000.0, 640.0).into(),
		};
		// On Windows, requesting transparency forces a no-redirection-bitmap
		// (layered) window that some virtual-desktop managers - VirtuaWin - won't
		// track, so it sits still across workspace switches. Alpha only reaches the
		// screen there through the composited DX12 path (Gfx::new_composited), so
		// an always-transparent window buys nothing when Transparency is off. Ask
		// for it only when it's actually in use; X11/Wayland always request it so
		// the live toggle works (no such side effect there).
		let want_transparent =
			!cfg!(windows) || config::settings().transparent_background || win_opacity.is_some();
		let attrs = Window::default_attributes()
			.with_title(crate::tabtitle::window_title(
				config::rights(),
				win_title.as_deref(),
				&config::title_prefix(),
				None,
			))
			.with_window_icon(load_icon())
			.with_decorations(decorated)
			.with_transparent(want_transparent)
			.with_inner_size(initial_size);
		let attrs = with_app_id(attrs); // stable WM_CLASS/app_id
		// The composited swapchain (Gfx::new_composited) wants no redirection
		// surface under it: with one, the desktop manager composes that surface
		// too, and winit's blur-behind hack for it is not needed either.
		#[cfg(windows)]
		let attrs = {
			use winit::platform::windows::WindowAttributesExtWindows;
			attrs.with_no_redirection_bitmap(want_transparent)
		};
		// Born hidden, then resized to the grid-derived size and drawn once before
		// being shown (revealed after the first correct frame in render). Otherwise it
		// flashes the 1000x640 default with a blank client, then jumps to the real
		// size and paints - visible on X11/Wayland as well as Windows.
		let attrs = attrs.with_visible(false);

		// On X11 the wgpu surface can't do per-pixel alpha, so we ALWAYS take the
		// glutin GL path there (transparent-capable backend), regardless of the
		// current Transparency setting - that way the toggle works live without a
		// relaunch (the bg alpha is gated per-frame, not the backend). Off-X11 the
		// normal wgpu path is used (Wayland already supports premultiplied alpha).
		// If the GL context can't be created, fall back to the native wgpu surface.
		let want_gl = is_x11(event_loop);
		let (mut gfx, window) =
			match want_gl.then(|| Gfx::new_gl_transparent(event_loop, attrs.clone())) {
				Some(Ok(pair)) => pair,
				other => {
					if let Some(Err(e)) = other {
						eprintln!(
							"{}: GL backend unavailable ({e}); using native surface (no transparency)",
							config::APP_NAME
						);
					}
					let window = Arc::new(event_loop.create_window(attrs).unwrap_or_else(|e| {
						eprintln!("{}: could not create a window: {e}", config::APP_NAME);
						std::process::exit(2);
					}));
					#[cfg(windows)]
					let gfx = if want_transparent {
						Gfx::new_composited(window.clone())
					} else {
						Gfx::new(window.clone())
					};
					#[cfg(not(windows))]
					let gfx = Gfx::new(window.clone());
					let gfx = gfx.unwrap_or_else(|e| {
						eprintln!("{}: no usable GPU/renderer: {e}", config::APP_NAME);
						std::process::exit(2);
					});
					(gfx, window)
				}
			};
		// System theme mode: seed the OS dark/light bit before the first frame so a
		// system-mode theme resolves to the right palette immediately (no flash).
		config::reapply_for_os(!matches!(window.theme(), Some(winit::window::Theme::Light)));
		// New hardware starts the performance profile over, and answers with the id
		// to write down if the pick is worth measuring rather than assuming.
		remote_override_at_launch();
		let bench_id = rate_hardware(&gfx.adapter_info);
		// Window-level CLI style (--font-name/-size, colors, bg image/fit/opacity)
		// overrides the loaded settings before text + bg image are built. Applied
		// after the theme/OS palette settles so it isn't clobbered. Per-pane style
		// stays deferred (needs a per-pane renderer).
		cli_win.apply_style();
		if cli_win.fullscreen.unwrap_or(false) {
			window.set_fullscreen(Some(Fullscreen::Borderless(None)));
		}
		// Request compositor backdrop blur (KWin/picom) if the setting is on; no-op
		// off-X11 and on compositors that don't honor the hint.
		set_blur_behind(&window, config::settings().transparent_background_blur);

		// Transparency only ever affects the terminal background (per-pixel), never
		// the whole window - so there's no compositor whole-window-opacity fallback.
		let scale = config::display_scale(window.scale_factor());
		let mut text = TextCtx::new(&gfx.device, &gfx.queue, gfx.format, scale);
		let rects = RectRenderer::new(&gfx.device, gfx.format);
		let minimap = crate::minimap::MapRenderer::new(&gfx.device, gfx.format);
		let scrim =
			crate::scrim::Scrim::new(&gfx.device, gfx.format, gfx.config.width, gfx.config.height);

		// Resize to the configured initial grid now that cell metrics are known.
		// cell_w/cell_h/margin are physical px; floor() in content_dims gives the
		// exact column/row count at this size. If the request applies
		// synchronously winit returns the new size (no Resized event), so adopt
		// it here; otherwise a Resized event reconfigures the surface.
		let settings = config::settings();
		// CLI columns/rows override config; --pixel-width/height override either
		// dimension directly. Add the menu-bar height (when shown) so the content
		// still gets the requested row count (the tab bar only appears with >1 tab).
		// remember_size launches at the last actual size; CLI columns/rows still override
		let cols = cli_win.columns.unwrap_or(if settings.remember_size {
			settings.remembered_columns
		} else {
			settings.columns
		});
		let rows = cli_win.rows.unwrap_or(if settings.remember_size {
			settings.remembered_rows
		} else {
			settings.rows
		});
		let menu_bar_h = if menu_bar {
			text.ui_line_h + text.dip(MENU_BAR_VPAD)
		} else {
			0.0
		};
		let n_tabs = if self.cli.hierarchical {
			self.cli.tabs.len().max(1)
		} else {
			1
		};
		// The strip shows for more than one tab, and for a single one unless the
		// user opts out - State::tab_bar_visible's rule.
		let tab_bar_h = if n_tabs > 1 || !settings.hide_single_tab {
			text.ui_line_h + text.dip(TAB_BAR_VPAD)
		} else {
			0.0
		};
		let (grid_w, grid_h) = window_px(
			cols,
			rows,
			text.cell_w,
			text.cell_h,
			text.margin,
			menu_bar_h + tab_bar_h,
		);
		let (want_w, want_h) = fit_px(
			cli_win.pixel_width.unwrap_or(grid_w),
			cli_win.pixel_height.unwrap_or(grid_h),
			gfx.device.limits().max_texture_dimension_2d,
		);
		let want = winit::dpi::PhysicalSize::new(want_w, want_h);
		let mut scrim = scrim;
		// If the resize applies synchronously (Windows), the first frame is already at
		// the final size - reveal on it. Otherwise (async X11/Wayland) wait for the
		// surface to reach `want` before revealing, so the window never maps at the
		// default size first.
		let reveal_want = if let Some(applied) = window.request_inner_size(want) {
			gfx.resize(applied.width, applied.height);
			scrim.resize(&gfx.device, applied.width, applied.height);
			None
		} else {
			Some(want)
		};

		// initial content area, inset by the menu bar and the tab strip where each
		// shows, so panes start correctly sized.
		let top = menu_bar_h + tab_bar_h;
		let area = Rect {
			x: 0.0,
			y: top,
			w: gfx.config.width as f32,
			h: (gfx.config.height as f32 - top).max(1.0),
		};
		let list = build_layout(&self.cli, &mut text, &self.proxy, area);
		let frame_budget = crate::profile::FrameBudget::new(Instant::now(), refresh_hz(&window));
		let surface_px = (gfx.config.width, gfx.config.height);
		let gl = gfx.is_gl();
		let adapter_info = gfx.adapter_info.clone();

		self.state = Some(State {
			window,
			gpu: Some(Gpu {
				gfx,
				rects,
				minimap,
				// filled in when the worker answers; the window is not held up for it
				wallpaper_img: None,
				scrim,
			}),
			rebirth: None,
			text,
			proxy: self.proxy.clone(),
			tabs: Tabs { list, active: 0 },
			mods: ModifiersState::empty(),
			mouse: (0.0, 0.0),
			mouse_btn: None,
			mouse_cell: None,
			selecting: None,
			select_edge_held: 0.0,
			last_click: None,
			click_count: 0,
			cursor_focus_sig: None,
			resizing: None,
			dragging_pane: None,
			bar_dragging: None,
			map_dragging: None,
			link_arm: None,
			menu_link: None,
			cursor_icon: CursorIcon::Default,
			clipboard: Clipboard::new(),
			last_frame: Instant::now(),
			rating: crate::profile::Rating::new(),
			frame_budget,
			bench: None,
			bench_at: None,
			bench_cap: None,
			bench_banner: None,
			bench_id,
			bench_kept: None,
			bench_from: None,
			bench_stalled: false,
			dirty: true,
			bell_flash: 0.0,
			size_tracked: false,
			revealed: false,
			shell_scan_at: None,
			reveal_want,
			reveal_deadline: Instant::now() + Duration::from_millis(400),
			pending_size: None,
			pending_size_at: Instant::now(),
			menu: None,
			tab_close_arm: None,
			tab_edit: None,
			tab_dbl: None,
			tab_hover: crate::tip::Dwell::default(),
			menu_tip: crate::tip::Dwell::default(),
			menu_tip_up: None,
			tab_first: 0,
			tab_layout: TabLayout::default(),
			tab_followed: 0,
			tab_tip: None,
			decorated,
			menu_bar,
			bare: false,
			bare_saved: (false, false),
			bar_open: None,
			quit: false,
			win_opacity,
			win_title,
			last_win_title: String::new(),
			focused: true,
			pending_about: false,
			pending_settings: false,
			chrome: None,
			chrome_rev: 0,
			text_sig: None,
			overlay_sig: None,
			scrim_sig: None,
			occluded: false,
			was_hidden: false,
			next_frame: None,
			wp_count: 0,
			wp_current: None,
			wp_next: None,
			wp_locked: false,
			cli_style: self.cli.win.style.clone(),
			wp_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
			wp_pacing: crate::wallpaper::Pacing::default(),
			wp_answered: false,
			wp_shown: false,
			shell_scan_cap: None,
			vram_next: Instant::now() + VRAM_CHECK_IVL,
			vramloss_test: std::env::var_os("SILK_VRAMLOSS").is_some(),
			surface_px,
			gl,
			adapter_info,
			idle: IdleClock::new(),
			conserve: Conserve::Off,
			vt_heal: VtHeal::default(),
		});
		// A wallpaper given on the command line (--wallpaper-file, incl. an explicit
		// clear) owns this session: rotation is skipped entirely, whatever the config
		// says, and the stored rotation settings are left untouched.
		let cli_wallpaper = self.cli.win.style.wallpaper_img.is_some();
		if let Some(state) = self.state.as_mut() {
			state.init_wallpaper(cli_wallpaper);
		}
		// GL path only: the native path's swapchain reports loss itself
		if !self.vt_watch && self.state.as_ref().is_some_and(|s| s.gl) {
			self.vt_watch = spawn_vt_watch(self.proxy.clone());
		}
	}

	fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
		let _t = crate::perf::Span::new(&crate::perf::EVENT_NS);
		let Some(state) = self.state.as_mut() else {
			return;
		};
		match event {
			UserEvent::WallpaperReady(loaded) => state.wallpaper_ready(*loaded),
			UserEvent::ShellsReady(found) => {
				fold_shells(&found);
				if let Some(dialog) = self.dialog.as_mut() {
					dialog.fold_shells(&found);
				}
			}
			UserEvent::Wakeup(id) => {
				crate::perf::bump(&crate::perf::WAKEUPS);
				crate::perf::echoed(id);
				// output easing is triggered in Pane::build when the screen
				// actually scrolls, not on every content change. Only the pane
				// that produced output is marked; a background tab's flag just
				// waits until its tab is shown (the switch forces a rebuild).
				crate::perf::timed(&crate::perf::NOTE_NS, || {
					if let Some(p) = state.tabs.find_pane_mut(id) {
						p.term.wake_handled();
						p.content_dirty = true;
						p.note_output(); // copy-output: push the settle deadline out
						// scrollback depth, sampled per read cycle - the only
						// granularity that sees a `clear` truncate it
						p.note_history();
					}
				});
				state.note_output(); // a shell that prints is not idle, if seen
			}
			UserEvent::PtyWrite(id, bytes) => {
				// a reply the terminal owes the program (cursor position, device
				// attributes), not something the user sent - so read-only does not
				// withhold it, and this is the one direct write left in this file
				if let Some(p) = state.tabs.find_pane(id) {
					p.term.write(bytes);
				}
			}
			UserEvent::Title(id, title) => {
				if let Some(p) = state.tabs.find_pane_mut(id) {
					// The one place a program's own title arrives.
					let said = crate::tabtitle::plain(&title);
					// Any tab's label can carry it now, not just the one in front,
					// so a background pane's title has to reach the strip too. Only
					// a title that actually moved costs a frame: a shell that sets
					// the same one on every prompt is common.
					if p.title != said {
						p.title = said;
						state.dirty = true;
					}
				}
				if id == state.tabs.cur().focused {
					state.update_title();
				}
			}
			UserEvent::ClipboardStore(id, kind, text) => {
				let in_use = id == state.tabs.cur().focused;
				if input::copy_allowed(CopyFrom::Program, state.focused, in_use) {
					match kind {
						ClipboardType::Clipboard => state.clipboard.set_clipboard(text),
						// set_primary falls back to the real clipboard where there is
						// no primary, which a store never asked for
						ClipboardType::Selection if cfg!(target_os = "linux") => {
							state.clipboard.set_primary(text);
						}
						ClipboardType::Selection => {}
					}
				}
			}
			UserEvent::ChildExit(id, status) => {
				state.hold_dead_pane(id, &status);
			}
			UserEvent::Exit(id) => {
				// A shell exited: close just its pane, not the whole app - unless
				// --keep-open asked for the pane to stay and say how it ended.
				if !state.hold_dead_pane(id, "unknown") {
					state.close_dead_pane(id);
				}
				state.dirty = true;
			}
			UserEvent::Bell => {
				// Visual bell: brighten all text, then smoothly fade back (render).
				state.bell_flash = 1.0;
				state.dirty = true;
			}
			UserEvent::SetWallpaper(image) => state.lock_wallpaper(image),
			UserEvent::ReloadSettings => state.reload_config(),
			UserEvent::VtSwitched => {
				// Return to our console (the watcher signals only returns).
				// Rebuild unconditionally: focus may move to another window or
				// nowhere, and an unfocused window must heal too.
				vramdbg("vt return -> heal_gpu");
				state.heal_gpu(self.dialog.is_some() || self.notice.is_some());
				state.vt_heal.returned(Instant::now());
			}
		}
	}

	fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
		if self.notice.as_ref().is_some_and(|n| n.id() == id) {
			self.handle_notice_event(event);
			return;
		}
		if self.notice_holds(&event) {
			return;
		}
		// route events for a pop-out dialog window to its own handler
		if self.dialog.as_ref().is_some_and(|d| d.id() == id) {
			self.handle_dialog_event(event);
			return;
		}
		// Simulated modality: while a dialog is open the main window takes no
		// input; a click on it re-raises/focuses the dialog instead.
		if let Some(d) = &self.dialog {
			match &event {
				WindowEvent::KeyboardInput { .. }
				| WindowEvent::MouseWheel { .. }
				| WindowEvent::Ime(_) => return,
				WindowEvent::MouseInput {
					state: ElementState::Pressed,
					..
				} => {
					d.window.focus_window();
					return;
				}
				_ => {}
			}
		}
		let Some(state) = self.state.as_mut() else {
			return;
		};
		// The startup benchmark owns the window while its banner is up: anything
		// typed or clicked would change what is being timed. Closing still works.
		if state.bench_banner.is_some()
			&& matches!(
				event,
				WindowEvent::KeyboardInput { .. }
					| WindowEvent::MouseInput { .. }
					| WindowEvent::MouseWheel { .. }
					| WindowEvent::CursorMoved { .. }
					| WindowEvent::Ime(_)
			) {
			return;
		}
		// Anything a person does to the window is a sign of life (see
		// `release_deadline`); the hidden-to-shown edge is one too, in its arm.
		if matches!(
			event,
			WindowEvent::KeyboardInput { .. }
				| WindowEvent::MouseInput { .. }
				| WindowEvent::MouseWheel { .. }
				| WindowEvent::CursorMoved { .. }
				| WindowEvent::CursorEntered { .. }
				| WindowEvent::Touch(_)
				| WindowEvent::Ime(_)
				| WindowEvent::Focused(_)
		) {
			state.note_active("input or focus");
		}
		match event {
			WindowEvent::CloseRequested => event_loop.exit(),

			WindowEvent::Resized(size) => {
				state.resize_surface(size.width, size.height);
				state.relayout_all();
				state.save_window_size(size.width, size.height);
				state.invalidate_prepared(); // scrim textures were just recreated
				state.dirty = true;
			}

			// DPI/scale changed (monitor move or a live scaling change). Re-scale
			// cell metrics + chrome for the new factor; winit preserves the logical
			// size, so a Resized event follows to reconfigure the surface + scrim.
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				state.rebuild_text(config::display_scale(scale_factor));
				state.dirty = true;
			}

			WindowEvent::ModifiersChanged(mods) => {
				state.mods = mods.state();
				if env_flag("SILK_KEYDBG") {
					eprintln!("[mods] {:?}", mods.state());
				}
				// Alt toggles the menu-bar accelerator underlines, so redraw.
				state.dirty = true;
			}

			// Window focus gates copy-output: a background window never copies.
			WindowEvent::Focused(focused) => {
				if env_flag("SILK_KEYDBG") {
					eprintln!("[focus] {focused}");
				}
				state.focused = focused;
				if !focused {
					state.commit_tab_edit(); // a rename does not outlive the window's focus
				}
				// repaint: focus-dependent chrome (copybox dim) and the refocus
				// poke that resumes a long-idle-parked cursor both live in render
				state.dirty = true;
				// Regaining focus is the likely first moment back from a VT
				// switch/suspend - probe the GPU uploads now, not at the slow tick.
				if focused && state.gl {
					state.vram_next = Instant::now();
					vramdbg("focus regained -> immediate probe");
				}
			}

			// Becoming visible again (VT return, compositor remap) - probe now too;
			// a VT switch doesn't always hand focus straight back.
			WindowEvent::Occluded(occluded) => {
				state.occluded = occluded;
				if !occluded {
					// nothing was drawn while hidden, so catch up in one frame
					state.dirty = true;
					state.note_active("shown");
					if state.gl {
						state.vram_next = Instant::now();
						vramdbg("unoccluded -> immediate probe");
					}
				}
			}

			// OS switched dark/light: a "System" theme follows it live.
			WindowEvent::ThemeChanged(theme) => {
				let dark = !matches!(theme, winit::window::Theme::Light);
				if config::reapply_for_os(dark) {
					state.dirty = true;
				}
			}

			WindowEvent::CursorLeft { .. } => {
				// no pointer, no hover underline and no tab tip
				state.update_link_hover(None);
				state.note_tab_hover(f32::MIN, f32::MIN);
			}

			WindowEvent::CursorMoved { position, .. } => {
				state.mouse = (position.x as f32, position.y as f32);
				let (x, y) = state.mouse;
				// The tab bar is chrome and sits above every pane, so its tip is
				// tracked before anything that could claim the pointer - including a
				// mouse-tracking app, which never sees the bar at all.
				state.note_tab_hover(x, y);
				// A thumb drag in progress owns the pointer - before the mouse-report
				// path, so a tracking app can't swallow the drag half way down.
				if let Some(id) = state.bar_dragging {
					let cfg = config::settings();
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.bar_drag_to(y, &state.text, &cfg);
					}
					state.dirty = true;
					return;
				}
				if let Some(id) = state.map_dragging {
					let cfg = config::settings();
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.map_drag_to(y, &state.text, &cfg);
					}
					state.dirty = true;
					return;
				}
				// mouse-tracking app wants motion/drag reports; when it does, skip our
				// local hover/selection handling for this move. The report is
				// PTY-bound: nothing local changed, so no redraw - marking dirty here
				// forced a full re-shape of every pane per cell crossed.
				if state.report_mouse_motion() {
					// the app owns the pointer, so nothing of ours is hovering it
					state.update_link_hover(None);
					return;
				}
				state.update_bar_hover(x, y);
				state.update_link_hover(Some((x, y)));
				// hovering a different top-level title with a bar menu open
				// switches to it (standard menu-bar behavior)
				if state.bar_open.is_some() && y < state.menu_bar_h() {
					if let Some(i) = state.menubar_hit(x) {
						if state.bar_open != Some(i) {
							state.open_bar_menu(i);
							state.dirty = true;
						}
					}
				}
				if state.menu_hover(x, y) {
					state.dirty = true;
				}
				if let Some(path) = state.resizing.clone() {
					// drag a pane divider
					let area = state.area();
					state
						.tabs
						.cur_mut()
						.drag_divider(&mut state.text, &path, area, x, y);
					state.dirty = true;
				} else if let Some(id) = state.selecting {
					// extend an in-progress drag-selection. Clamped, not bounded: a
					// pointer dragged off the pane keeps selecting to the edge cell,
					// and the per-frame step below scrolls to reveal more.
					if let Some(p) = state.tabs.cur().panes.get(&id) {
						let (point, side) = p.point_clamped(x, y, &state.text);
						p.update_selection(point, side);
					}
					state.dirty = true;
				} else if state.dragging_pane.is_some() {
					// redraw the drop-target highlight as the cursor moves
					state.dirty = true;
				} else {
					// resize cursor over a divider, hand over a link
					state.sync_cursor_icon();
				}
			}

			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button,
				..
			} => {
				let (x, y) = state.mouse;
				// A rename ends wherever the next click goes, unless it goes back to
				// the tab being renamed (the tab-strip branch below handles that).
				if state.tab_edit.is_some() {
					let on_edited = state.tab_bar_visible()
						&& y >= state.menubar_h()
						&& y < state.menubar_h() + state.tab_bar_h()
						&& state.tab_at(x) == state.tab_edit.as_ref().map(|e| e.tab);
					if !on_edited {
						state.commit_tab_edit();
					}
				}
				// A popup tall enough to be clamped to the top of the window covers
				// the menu bar, and the click belongs to whatever is drawn on top -
				// otherwise its first item is unreachable. Same reason the tab-bar
				// branch below stands aside for an open menu.
				let on_popup = state.menu.as_ref().is_some_and(|m| m.hit_any(x, y));
				// click on the menu bar: toggle/open the top-level menu's dropdown
				if button == MouseButton::Left
					&& state.menu_bar
					&& !on_popup && y < state.menu_bar_h()
				{
					// the always-visible copy-mode checkboxes toggle the focused pane
					if let Some(kind) = state.copybox_hit(x) {
						let focused_id = state.tabs.cur().focused;
						state.toggle_copy(focused_id, kind);
						state.menu = None;
						state.bar_open = None;
						state.dirty = true;
						return;
					}
					match (state.menubar_hit(x), state.bar_open) {
						(Some(i), Some(open)) if i == open => {
							state.menu = None;
							state.bar_open = None;
						}
						(Some(i), _) => state.open_bar_menu(i),
						(None, _) => {
							state.menu = None;
							state.bar_open = None;
						}
					}
					state.dirty = true;
					return;
				}
				// click on the tab bar selects a tab
				let tab_bar_y = state.menubar_h();
				if button == MouseButton::Left
					&& input::tab_bar_takes_press(
						state.menu.is_some(),
						state.tab_bar_visible(),
						y,
						tab_bar_y,
						state.tab_bar_h(),
					) {
					if let Some(i) = state.tab_at(x) {
						// press in the close-button column only ARMS the close (the
						// button lights up); the close itself fires on release over
						// the same box, so a slipped press can be dragged off to
						// cancel - standard button feel. Elsewhere selects the tab.
						let bar_h = state.tab_bar_h();
						let on_close = state
							.tab_close_box_at(i, tab_bar_y, bar_h)
							.is_some_and(|cb| x >= cb.x);
						// second click on the same tab, soon enough: rename it
						let now = Instant::now();
						let again = state.tab_dbl.is_some_and(|(when, was)| {
							was == i && now.duration_since(when) < Duration::from_millis(400)
						});
						state.tab_dbl = Some((now, i));
						if on_close {
							state.tab_close_arm = Some(i);
						} else if again && !on_close {
							state.begin_tab_edit(i);
						} else {
							if state.tab_edit.as_ref().is_some_and(|e| e.tab != i) {
								state.commit_tab_edit();
							}
							if state.tabs.active != i {
								state.tabs.active = i;
								state.freeze_catchup();
							}
							state.update_title();
						}
						state.dirty = true;
					}
					return;
				}
				// A visible scrollbar takes the click before anything else - including a
				// mouse-tracking app, the same way the right-click menu does. `bar_hit`
				// answers None whenever the bar is faded out, so an invisible bar never
				// steals a click that belonged to the text under it.
				if button == MouseButton::Left && state.menu.is_none() {
					let cfg = config::settings();
					let hit = state.tabs.cur().pane_at(x, y).and_then(|id| {
						let p = state.tabs.cur().panes.get(&id)?;
						Some((id, p.bar_hit(x, y, &state.text, &cfg)?))
					});
					if let Some((id, hit)) = hit {
						state.focus_at(x, y);
						if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
							match hit {
								BarHit::Thumb => p.bar_grab(y, &state.text, &cfg),
								BarHit::TrackUp => p.bar_page(true, &state.text),
								BarHit::TrackDown => p.bar_page(false, &state.text),
							}
						}
						if hit == BarHit::Thumb {
							state.bar_dragging = Some(id);
						}
						state.dirty = true;
						return;
					}
					// The minimap column owns its presses the same way: drag the
					// marker, or press anywhere else in the column to go there.
					let hit = state.tabs.cur().pane_at(x, y).and_then(|id| {
						let p = state.tabs.cur().panes.get(&id)?;
						let g = p.minimap(&state.text, &cfg)?;
						Some((id, crate::minimap::hit(&g, x, y)?))
					});
					if let Some((id, hit)) = hit {
						state.focus_at(x, y);
						if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
							match hit {
								crate::minimap::Hit::Handle => p.map_grab(y, &state.text, &cfg),
								crate::minimap::Hit::Track => p.map_jump(y, &state.text, &cfg),
							}
						}
						if hit == crate::minimap::Hit::Handle {
							state.map_dragging = Some(id);
						}
						state.dirty = true;
						return;
					}
				}
				// mouse-tracking app owns the pointer: report the press, skip local
				// selection/paste/menu (Shift bypasses to the local action). An open
				// menu must get the click (operate/dismiss it), not the app underneath.
				if state.report_mouse_button(button, ElementState::Pressed) {
					state.dirty = true;
					return;
				}
				match button {
					MouseButton::Left => {
						if state.menu.is_some() {
							// click an item to act, a submenu row to open its popup,
							// anywhere else to dismiss
							state.menu_click(x, y, &self.proxy);
							state.dirty = true;
						} else if let Some((path, _)) =
							state
								.tabs
								.cur()
								.divider_at(x, y, state.area(), state.text.scale)
						{
							// grab a divider to resize instead of selecting
							state.resizing = Some(path);
						} else if state.mods.shift_key() {
							// Shift+drag a pane to reorder it
							if let Some(id) = state.tabs.cur().pane_at(x, y) {
								state.focus_at(x, y);
								state.dragging_pane = Some(id);
								state.window.set_cursor(CursorIcon::Grabbing);
								state.cursor_icon = CursorIcon::Grabbing;
							}
						} else if let Some((id, link)) = state
							.mods
							.control_key()
							.then(|| state.link_at_pointer())
							.flatten()
						{
							// Ctrl+click a link: arm here, open on the release over the
							// same link, so a slipped press can be dragged off to
							// cancel. Ctrl elsewhere still starts a block selection -
							// only a press ON a link is taken.
							state.focus_at(x, y);
							state.link_arm = Some((id, link.url));
						} else {
							state.focus_at(x, y);
							let now = Instant::now();
							state.click_count = input::click_count(
								state.last_click,
								state.click_count,
								now,
								(x, y),
								(state.text.cell_w, state.text.cell_h),
							);
							state.last_click = Some((now, x, y));
							let kind =
								input::click_select(state.click_count, state.mods.control_key());
							let pairs = if kind == ClickSelect::Word {
								config::selection_pairs()
							} else {
								Vec::new()
							};
							let started = state.tabs.cur().pane_at(x, y).and_then(|id| {
								let p = state.tabs.cur().panes.get(&id)?;
								let (point, side) = p.point_at(x, y, &state.text)?;
								if kind == ClickSelect::Line {
									// whole logical line, spanning wrapped continuation rows
									let (start, end) = p.line_span(point);
									p.begin_selection(start, Side::Left, SelectionType::Simple);
									p.update_selection(end, Side::Right);
								} else if kind == ClickSelect::Word {
									// a shape we can name (URL, path) wins; else the
									// contents of a matched pair; else a bracket to
									// its partner; else the word
									match p
										.shape_span(point)
										.or_else(|| p.pair_span(point, &pairs))
										.or_else(|| p.bracket_span(point))
									{
										Some((start, end)) => {
											p.begin_selection(
												start,
												Side::Left,
												SelectionType::Simple,
											);
											p.update_selection(end, Side::Right);
										}
										None => {
											p.begin_selection(point, side, SelectionType::Semantic);
										}
									}
								} else {
									let sel_type = if kind == ClickSelect::Block {
										SelectionType::Block
									} else {
										SelectionType::Simple
									};
									p.begin_selection(point, side, sel_type);
								}
								Some(id)
							});
							if started.is_some() {
								state.selecting = started;
								state.dirty = true;
							}
						}
					}
					MouseButton::Middle => {
						// paste the primary selection into the pane under the cursor
						if let Some(text) = state.clipboard.get_primary() {
							let id = state
								.tabs
								.cur()
								.pane_at(x, y)
								.unwrap_or(state.tabs.cur().focused);
							if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
								p.paste(&text);
							}
						}
					}
					MouseButton::Right => {
						if let Some(id) = state.tabs.cur().pane_at(x, y) {
							state.open_menu(id, x, y);
							state.dirty = true;
						}
					}
					_ => {}
				}
			}

			// a mouse-tracking app owns the pointer: report the release we opened.
			// Only for the SAME button as the reported press - releasing a different
			// one must not clear the held state (the app would see an unbalanced
			// press) nor steal that button's local release handling below.
			WindowEvent::MouseInput {
				state: ElementState::Released,
				button,
				..
			} if input::release_is_reported(state.mouse_btn, button) => {
				if state.report_mouse_button(button, ElementState::Released) {
					state.dirty = true;
				}
			}

			WindowEvent::MouseInput {
				state: ElementState::Released,
				button: MouseButton::Left,
				..
			} => {
				state.resizing = None;
				// armed link: open only if the release is still on the same one
				if let Some((armed_id, url)) = state.link_arm.take() {
					let same = state
						.link_at_pointer()
						.is_some_and(|(id, link)| id == armed_id && link.url == url);
					if same {
						open_link(&url);
					}
				}
				if let Some(id) = state.map_dragging.take() {
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.release_handle();
					}
					state.dirty = true;
					return;
				}
				// end a thumb drag; the hold keeps the bar up for a moment afterwards
				if let Some(id) = state.bar_dragging.take() {
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.release_handle();
					}
					state.dirty = true;
				}
				if let Some(i) = state.tab_close_arm.take() {
					let (x, y) = state.mouse;
					let tab_bar_y = state.menubar_h();
					let bar_h = state.tab_bar_h();
					let in_bar = y >= tab_bar_y && y < tab_bar_y + bar_h;
					let close_x = (in_bar && i < state.tabs.len())
						.then(|| state.tab_close_box_at(i, tab_bar_y, bar_h))
						.flatten()
						.map(|cb| cb.x);
					if input::close_on_release(i, x, in_bar, close_x, state.tab_at(x)) {
						state.close_tab_at(i);
					}
					state.dirty = true;
				}
				// drop a dragged pane onto the pane under the cursor (swap)
				if let Some(src) = state.dragging_pane.take() {
					let (x, y) = state.mouse;
					let area = state.area();
					if let Some(target_id) = state.tabs.cur().pane_at(x, y) {
						state
							.tabs
							.cur_mut()
							.swap_panes(&mut state.text, src, target_id, area);
					}
					state.window.set_cursor(CursorIcon::Default);
					state.cursor_icon = CursorIcon::Default;
					state.dirty = true;
				}
				// finish a drag-select: copy to primary, or clear if it was a click
				if let Some(id) = state.selecting.take() {
					let text = state.tabs.cur().panes.get(&id).and_then(|p| {
						let sel_text = p.selection_text();
						if sel_text.is_none() {
							p.clear_selection();
						}
						sel_text
					});
					match text {
						Some(sel_text) => {
							// copy-on-select: a finished selection also goes to the
							// desktop clipboard when the pane opted in
							let in_use = id == state.tabs.cur().focused;
							if input::copy_allowed(CopyFrom::Select, state.focused, in_use)
								&& state
									.tabs
									.cur()
									.panes
									.get(&id)
									.is_some_and(|p| p.copy_select)
							{
								state.clipboard.set_clipboard(sel_text.clone());
							}
							state.clipboard.set_primary(sel_text);
						}
						None => state.dirty = true,
					}
				}
			}

			WindowEvent::MouseWheel { delta, .. } => {
				let (x, y) = state.mouse;
				// The tab bar takes the wheel first, and turns its own page. With
				// more tabs than fit, this is the only way to reach one past the
				// edge with the mouse alone.
				let bar_y = state.menubar_h();
				if state.tab_bar_visible() && y >= bar_y && y < bar_y + state.tab_bar_h() {
					let up = match delta {
						MouseScrollDelta::LineDelta(_, dy) => dy > 0.0,
						MouseScrollDelta::PixelDelta(pos) => pos.y > 0.0,
					};
					state.scroll_tab_strip(if up { 1.0 } else { -1.0 });
					return;
				}
				let id = state
					.tabs
					.cur()
					.pane_at(x, y)
					.unwrap_or(state.tabs.cur().focused);
				let cell_h = state.text.cell_h;
				// A mouse-tracking app (muffer, tmux, vim with mouse on, ...) wants
				// the wheel as button 64/65 reports, not our scrollback. Report one
				// notch per line, then stop here.
				let shift = state.mods.shift_key();
				let (up, notches) = match delta {
					MouseScrollDelta::LineDelta(_, y) => (y > 0.0, (y.abs().round() as u32).max(1)),
					MouseScrollDelta::PixelDelta(pos) => (
						(pos.y as f32) > 0.0,
						((pos.y.abs() as f32 / cell_h).round() as u32).max(1),
					),
				};
				if let Some(p) = state.tabs.cur().panes.get(&id) {
					// No cell under the pointer means it is over the minimap
					// column, not the text - fall through and scroll the buffer.
					if let Some((col, row)) = (input::wheel_route(p.mode, shift)
						== WheelRoute::Report)
						.then(|| p.screen_cell_at(x, y, &state.text))
						.flatten()
					{
						let btn = if up {
							input::MouseBtn::WheelUp
						} else {
							input::MouseBtn::WheelDown
						};
						for _ in 0..notches.min(8) {
							if let Some(seq) =
								input::mouse_report(p.mode, btn, true, false, col, row, state.mods)
							{
								p.write_input(seq);
							}
						}
						state.dirty = true;
						return;
					}
				}
				// smooth scrollback uses WHEEL_LINES; full-screen apps get their
				// own (tunable) lines-per-notch via ALT_SCROLL_LINES
				let (lines, alt_lines) = match delta {
					MouseScrollDelta::LineDelta(_, y) => (
						y * config::settings().wheel_lines,
						y * config::settings().alt_scroll_lines,
					),
					MouseScrollDelta::PixelDelta(pos) => {
						let lines = pos.y as f32 / cell_h;
						(lines, lines)
					}
				};
				if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
					let mode = p.mode;
					if input::wheel_route(mode, shift) == WheelRoute::CursorKeys {
						// full-screen apps (less, nano, ...) have no scrollback of
						// their own; the wheel drives their cursor-key scrolling
						let n = alt_lines.abs().round() as i32;
						if n > 0 {
							let letter = if alt_lines > 0.0 { b'A' } else { b'B' };
							let seq =
								input::cursor_seq(letter, mode.contains(TermMode::APP_CURSOR));
							let mut bytes = Vec::with_capacity(seq.len() * n as usize);
							for _ in 0..n {
								bytes.extend_from_slice(&seq);
							}
							p.write_input(bytes);
						}
					} else {
						p.scroll.wheel(lines);
						// user-driven scroll, so the bar comes up; output-driven
						// scrolling deliberately doesn't (it never stops)
						p.poke_scrollbar();
					}
				}
				state.dirty = true;
			}

			WindowEvent::KeyboardInput {
				event: key,
				is_synthetic,
				..
			} => {
				let key_at = crate::perf::key_mark();
				if env_flag("SILK_KEYDBG") {
					eprintln!(
						"[key] {:?} {:?} synthetic={is_synthetic} focused={} mods=[{}{}{}]",
						key.logical_key,
						key.state,
						state.focused,
						if state.mods.control_key() { "C" } else { "" },
						if state.mods.alt_key() { "A" } else { "" },
						if state.mods.shift_key() { "S" } else { "" },
					);
				}
				// A replayed press is focus bookkeeping, and a release is the
				// shell's business, not ours - neither is typing.
				if !key_is_typed(key.state, is_synthetic) {
					return;
				}
				// A character handed to the window instead of typed at it arrives
				// with no key named; fill the key in so the rest of this reads it.
				let key = input::name_typed(key);
				// see IGNORE_KEYS_WHILE_UNFOCUSED. A copy types nothing, so it cannot be
				// the bare arrow that gate is for, and a flag lagging the WM must not
				// eat it while right-click Copy works.
				if IGNORE_KEYS_WHILE_UNFOCUSED && !state.focused {
					if state.menu.is_none() && is_copy_chord(state.mods, &key.logical_key) {
						state.copy_selection();
					}
					return;
				}
				// An open menu (context menu / menu-bar dropdown) captures the
				// navigation keys - they drive the menu, not the terminal pane.
				if state.menu.is_some() {
					// Every one of these drives the INNERMOST open popup - with a
					// submenu standing open, that is the submenu.
					match &key.logical_key {
						Key::Named(NamedKey::Escape) => {
							// back out one level at a time, the way a submenu is entered
							if !state.close_submenu() {
								state.menu = None;
								state.bar_open = None;
							}
						}
						Key::Named(NamedKey::ArrowDown) => {
							if let Some(menu) = state.menu_inner() {
								menu.hover = menu.step(menu.hover, 1);
							}
						}
						Key::Named(NamedKey::ArrowUp) => {
							if let Some(menu) = state.menu_inner() {
								menu.hover = menu.step(menu.hover, -1);
							}
						}
						// Right enters a submenu and Left leaves one; where there is
						// no submenu in the way they cycle between menu-bar dropdowns
						// (a no-op for a right-click context menu, which isn't
						// bar-anchored)
						Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
							let left = matches!(key.logical_key, Key::Named(NamedKey::ArrowLeft));
							let enter = (!left).then(|| state.submenu_row()).flatten();
							if let Some(row) = enter {
								state.menu_activate(row, &self.proxy);
							} else if !(left && state.close_submenu()) {
								if let Some(open_idx) = state.bar_open {
									let n = MENU_BAR.len();
									let next = if left {
										(open_idx + n - 1) % n
									} else {
										(open_idx + 1) % n
									};
									state.open_bar_menu(next);
								}
							}
						}
						Key::Named(NamedKey::Enter) => {
							if let Some(row) = state.menu_inner().and_then(|menu| menu.hover) {
								state.menu_activate(row, &self.proxy);
							}
						}
						// accelerator: a letter activates the item carrying it (the
						// underlined letter; unique per menu, some items have none)
						Key::Character(typed) => {
							let ch = typed.chars().next().map(|c| c.to_ascii_lowercase());
							let hit = ch.and_then(|ch| {
								state.menu.as_ref().and_then(|menu| {
									let chain = menu.chain();
									chain[chain.len() - 1].entries.iter().position(|entry| {
										entry_accel(entry).is_some_and(|(label, pos)| {
											label[pos..]
												.chars()
												.next()
												.map(|c| c.to_ascii_lowercase()) == Some(ch)
										})
									})
								})
							});
							if let Some(row) = hit {
								state.menu_activate(row, &self.proxy);
							}
						}
						_ => {}
					}
					state.dirty = true;
					return;
				}
				// A tab rename takes every key while it is up: the tab strip is the
				// only thing on screen accepting typing, so nothing reaches the shell.
				if state.tab_edit.is_some() {
					let ctrl = state.mods.control_key();
					let shift = state.mods.shift_key();
					match &key.logical_key {
						// Tab commits too - there is nowhere for it to move to.
						Key::Named(NamedKey::Enter | NamedKey::Tab) => state.commit_tab_edit(),
						Key::Named(NamedKey::Escape) => state.cancel_tab_edit(),
						Key::Named(NamedKey::Backspace) => {
							state.edit_tab(|edit| edit.erase(true));
						}
						Key::Named(NamedKey::Delete) => {
							state.edit_tab(|edit| edit.erase(false));
						}
						Key::Named(NamedKey::ArrowLeft) => {
							state.edit_tab(|edit| edit.move_caret(Caret::Left, shift));
						}
						Key::Named(NamedKey::ArrowRight) => {
							state.edit_tab(|edit| edit.move_caret(Caret::Right, shift));
						}
						Key::Named(NamedKey::Home) => {
							state.edit_tab(|edit| edit.move_caret(Caret::Home, shift));
						}
						Key::Named(NamedKey::End) => {
							state.edit_tab(|edit| edit.move_caret(Caret::End, shift));
						}
						Key::Named(NamedKey::Space) if !ctrl => {
							state.edit_tab(|edit| edit.insert(" "));
						}
						Key::Character(typed) if ctrl && typed.eq_ignore_ascii_case("a") => {
							state.edit_tab(|edit| {
								edit.anchor = 0;
								edit.caret = edit.text.len();
							});
						}
						Key::Character(typed) if ctrl && typed.eq_ignore_ascii_case("v") => {
							if let Some(text) = state.clipboard.get_clipboard() {
								// one line: a tab is one line high
								let flat = text.replace(['\n', '\r', '\t'], " ");
								state.edit_tab(move |edit| edit.insert(&flat));
							}
						}
						Key::Character(typed) if !ctrl && !state.mods.alt_key() => {
							let typed = typed.to_string();
							state.edit_tab(move |edit| edit.insert(&typed));
						}
						_ => {}
					}
					state.dirty = true;
					return;
				}
				match input::hotkey_for(&key.logical_key, state.mods, state.menu_bar) {
					Some(Hotkey::Settings) => {
						state.open_settings();
						return;
					}
					Some(Hotkey::Fullscreen) => {
						state.toggle_fullscreen();
						return;
					}
					// the Menu/Apps key opens the context menu on the focused pane
					Some(Hotkey::ContextMenu) => {
						let id = state.tabs.cur().focused;
						if let Some(p) = state.tabs.cur().panes.get(&id) {
							let (rect_x, rect_y) = (p.rect.x, p.rect.y);
							state.open_menu(id, rect_x + 12.0, rect_y + 12.0);
							state.dirty = true;
						}
						return;
					}
					// a letter no title starts with goes on to the shell
					Some(Hotkey::MenuTitle(ch)) => {
						if let Some(i) = bar_menu_for(ch) {
							state.open_bar_menu(i);
							state.dirty = true;
							return;
						}
					}
					Some(Hotkey::NewTab) => {
						state.new_tab(&self.proxy);
						return;
					}
					// the current tab, or the window if it is the last one
					Some(Hotkey::CloseTab) => {
						state.close_tab();
						return;
					}
					Some(Hotkey::NewWindow) => {
						state.new_window();
						return;
					}
					Some(Hotkey::Zoom(dir)) => {
						state.font_zoom(dir);
						return;
					}
					Some(Hotkey::ZoomReset) => {
						state.font_zoom_reset();
						return;
					}
					Some(hotkey @ (Hotkey::PrevTab | Hotkey::NextTab)) => {
						if hotkey == Hotkey::PrevTab {
							state.tabs.prev();
						} else {
							state.tabs.next();
						}
						state.freeze_catchup();
						state.update_title();
						state.dirty = true;
						return;
					}
					Some(Hotkey::MoveTab { forward }) => {
						state.tabs.move_active(forward); // same tab follows - nothing was frozen
						state.update_title();
						state.dirty = true;
						return;
					}
					Some(Hotkey::Copy) => {
						state.copy_selection();
						state.dirty = true;
						return;
					}
					Some(Hotkey::Paste) => {
						let focused = state.tabs.cur().focused;
						if let Some(text) = state.clipboard.get_clipboard() {
							if let Some(p) = state.tabs.cur_mut().panes.get_mut(&focused) {
								p.paste(&text);
							}
						}
						state.dirty = true;
						return;
					}
					None => {}
				}
				let focused = state.tabs.cur().focused;
				let app_cursor = state
					.tabs
					.cur()
					.panes
					.get(&focused)
					.is_some_and(|p| p.mode.contains(TermMode::APP_CURSOR));
				if let Some(bytes) = input::encode(&key, state.mods, app_cursor) {
					// a held pane has no shell left to type at, so any key that
					// would have gone to one closes it instead
					if state.tabs.cur().panes.get(&focused).is_some_and(|p| p.held) {
						state.close_dead_pane(focused);
						return;
					}
					// copy-output: Enter at the shell prompt may launch a command;
					// arm the capture so its output is copied once the pane settles.
					let is_enter = matches!(key.logical_key, Key::Named(NamedKey::Enter));
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&focused) {
						if !p.read_only {
							p.scroll.jump_bottom();
							p.write_input(bytes);
							crate::perf::typed(key_at, focused);
							p.note_typed();
							if is_enter && p.copy_output {
								p.arm_capture();
							}
						}
					}
					state.dirty = true;
				}
			}

			WindowEvent::RedrawRequested => {
				if state.freeze_sync() {
					return; // frozen - nothing on screen to paint
				}
				// The desktop wants pixels from a window that let its device go:
				// the rebuild is done where every other wake is (about_to_wait),
				// and the frame with it.
				if state.gpu.is_none() {
					state.note_active("redraw request");
					return;
				}
				let _ = state.render(true);
			}

			_ => {}
		}
	}

	// request_redraw isn't reliable under some compositors, so we drive frames
	// here: render when something changed or an animation is in flight, and
	// poll only while animating (otherwise sleep until the next event).
	fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
		// don't lose a resize done just before quitting
		if let Some(state) = self.state.as_mut() {
			state.flush_window_size(true);
		}
		crate::perf::report();
	}

	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		crate::perf::bump(&crate::perf::PASSES);
		let _t = crate::perf::Span::new(&crate::perf::PASS_NS);
		// One place to act on `quit`, so every path that sets it exits - menus,
		// hotkeys, and the tab-close box all reach here on the next pass.
		if self.state.as_ref().is_some_and(|state| state.quit) {
			event_loop.exit();
			return;
		}
		// cicd profiler: in profile mode run for SILK_PROFILE_SECS then exit, so
		// main can dump the flamegraph (the workload runs in the startup pane).
		#[cfg(feature = "profiling")]
		if std::env::var_os("SILK_PROFILE_OUT").is_some() {
			let now = std::time::Instant::now();
			let deadline = *self
				.profile_deadline
				.get_or_insert_with(|| now + std::time::Duration::from_secs(self.profile_secs));
			if now >= deadline {
				event_loop.exit();
				return;
			}
		}

		// Warm the dialogs' GPU context once the terminal is genuinely on screen,
		// so building it can't slow the path to the first frame. Idempotent.
		if WARM_DIALOG_GPU
			&& self
				.state
				.as_ref()
				.is_some_and(|state| state.revealed && state.gpu.is_some())
		{
			self.gpu_warm.start();
		}

		// Look for installed shells, once, a little after the window is genuinely
		// on screen. A PATH scan stats every directory the user has on it and the
		// Windows side reads the registry, so it runs on its own thread and comes
		// back as UserEvent::ShellsReady.
		if let Some(state) = self.state.as_mut() {
			if state.shell_scan_at.is_some_and(|at| Instant::now() >= at) {
				state.shell_scan_at = None;
				crate::shells::spawn(&self.proxy);
			}
		}

		// Raise a rested pointer's tab tip, and keep an open one's clock ticking.
		if let Some(state) = self.state.as_mut() {
			if state.update_tips() {
				state.dirty = true;
			}
		}

		// Open the About window if requested (window creation needs the event loop,
		// so State only signals and we act here).
		let open_about = self
			.state
			.as_mut()
			.is_some_and(|state| std::mem::take(&mut state.pending_about));
		// parent handle so the WM ties the dialog to the terminal window
		// (transient-for / owner)
		let parent = self.state.as_ref().and_then(|state| {
			use winit::raw_window_handle::HasWindowHandle;
			state
				.window
				.window_handle()
				.ok()
				.map(|handle| handle.as_raw())
		});
		// cloned (all wgpu handles, so refcount bumps) rather than borrowed: the
		// open arms below also take `&mut self` to store the dialog
		let warm = (open_about || self.state.as_ref().is_some_and(|s| s.pending_settings))
			.then(|| self.gpu_warm.get())
			.flatten();
		if open_about {
			if let Some(info) = self.state.as_ref().map(|state| state.adapter_info.clone()) {
				match crate::dialog::DialogWin::new_about(event_loop, &info, parent, warm.as_ref())
				{
					Ok(d) => {
						self.dialog = Some(d);
						self.center_dialog();
						self.reveal_dialog();
						self.dialog_dirty = true;
					}
					Err(e) => eprintln!("{}: About window failed: {e}", config::APP_NAME),
				}
			}
		}
		let settings_base = self.state.as_mut().and_then(|state| {
			std::mem::take(&mut state.pending_settings).then(|| state.settings_for_dialog())
		});
		if let Some(base) = settings_base {
			// a view older than the resume window is dead either way, so take it
			// unconditionally and discard it if it has expired
			let resume = self
				.settings_view
				.take()
				.filter(|(closed, _)| closed.elapsed() <= SETTINGS_RESUME)
				.map(|(_, view)| view);
			let sized = self.settings_size;
			match crate::dialog::DialogWin::new_settings(
				event_loop,
				parent,
				resume,
				sized,
				warm.as_ref(),
				base,
			) {
				Ok(d) => {
					self.dialog = Some(d);
					self.center_dialog();
					self.reveal_dialog();
					self.dialog_dirty = true;
				}
				Err(e) => eprintln!("{}: Settings window failed: {e}", config::APP_NAME),
			}
		}
		// a dialog with an animating field edit (view scroll / caret / blink)
		// keeps re-rendering at the cadence it reports (see dlg_wake below)
		if self
			.dialog
			.as_ref()
			.is_some_and(|d| d.anim_wake_ms().is_some())
		{
			self.dialog_dirty = true;
		}
		if self.dialog_dirty {
			if let Some(d) = &mut self.dialog {
				d.render();
			}
			self.dialog_dirty = false;
		}
		// A save that could not be written. Asked for from Settings, it was
		// taken there already (apply_dialog_settings); anything here is one of
		// the saves nobody asked for.
		if let Some(refusal) = config::take_refusal() {
			if notice_due(&mut self.told, &refusal.path, false) {
				self.notice_owed = Some(refusal);
			}
		}
		if self.notice_owed.is_some() {
			self.show_notice(event_loop);
		}
		if self.notice_dirty {
			if let Some(n) = &mut self.notice {
				n.render();
			}
			self.notice_dirty = false;
		}
		let dlg_wake = self
			.dialog
			.as_ref()
			.and_then(super::dialog::DialogWin::anim_wake_ms)
			.map(|ms| Instant::now() + Duration::from_millis(ms));

		// re-assert the dialog->terminal stacking a few times after focus (see the
		// field comment). Cleared when the dialog closes.
		if self.dialog.is_none() {
			self.raise_reassert = 0;
		} else if self.raise_reassert > 0 && Instant::now() >= self.raise_next {
			if let Some(d) = &self.dialog {
				d.raise_parent();
			}
			self.raise_reassert -= 1;
			self.raise_next = Instant::now() + RAISE_REASSERT_IVL;
		}
		let raise_wake = (self.raise_reassert > 0).then_some(self.raise_next);

		let Some(state) = self.state.as_mut() else {
			return;
		};
		let scroll_anim = state
			.tabs
			.cur()
			.panes
			.values()
			.any(|p| p.scroll.animating());
		let cursor_anim = state.tabs.cur().panes.values().any(|p| p.cursor_animating);
		let content = state.tabs.cur().panes.values().any(|p| p.content_dirty);
		// Parked-cursor resume: no frames flow while a cursor is parked at full,
		// so consume any due wake (render one catch-up frame - the pause state
		// sees the timeouts met and resumes the cycle) and keep the earliest
		// pending one to fold into the control flow below.
		let mut cursor_wake: Option<Instant> = None;
		let wake_now = Instant::now();
		for pane in state.tabs.cur_mut().panes.values_mut() {
			if let Some(wake) = pane.cursor_wake {
				if wake_now >= wake {
					pane.cursor_wake = None; // consumed - or an occluded window would spin on it
					state.dirty = true;
				} else {
					cursor_wake = Some(cursor_wake.map_or(wake, |w| w.min(wake)));
				}
			}
			// a minimap compose the throttle deferred: due now, or wake for it
			if let Some(wake) = pane.map_wake() {
				if wake_now >= wake {
					state.dirty = true;
				} else {
					cursor_wake = Some(cursor_wake.map_or(wake, |w| w.min(wake)));
				}
			}
		}
		// copy-output: catch the focused pane's command finishing (see method)
		state.poll_output_copy();
		// wallpaper rotation: swap to the next image when its interval elapses
		// (sets state.dirty so the change renders this cycle)
		if state.wp_next.is_some_and(|next| Instant::now() >= next) && state.gpu.is_some() {
			state.advance_wallpaper();
		}
		if state.vt_heal.due(Instant::now()) {
			vramdbg("vt return, second pass -> heal_gpu");
			state.heal_gpu(self.dialog.is_some() || self.notice.is_some());
		}
		// "resources restored" comes down on its own after a few seconds
		if state.conserve.wake().is_some_and(|at| Instant::now() >= at) {
			state.conserve = Conserve::Off;
			state.update_title();
		}
		// GL path: the VT watcher (spawn_vt_watch) is the real loss trigger; the
		// readback probes below stay as field evidence + a fallback for a missed
		// switch, since a real purge read back "intact" (driver restores readback
		// contents while sampled copies stay garbage).
		if let Some(gpu) = state.gpu.as_mut().filter(|gpu| gpu.gfx.is_gl()) {
			if state.vramloss_test && state.revealed {
				state.vramloss_test = false;
				gpu.gfx.vram_clobber();
				if let Some(wp) = &gpu.wallpaper_img {
					wp.vram_clobber(&gpu.gfx.queue);
				}
				vramdbg("SILK_VRAMLOSS: sentinels + wallpaper clobbered");
			}
			// Poll both probes; either detecting loss triggers the one rebuild.
			// Field logs showed EVERY witness - synthetic sentinels AND the
			// wallpaper's own uploaded block - reading back intact across a real
			// purge that blacked the window, so readback cannot be the primary
			// detector. Kept for the diagnostic trail and as a fallback.
			let mut lost = None;
			match gpu.gfx.vram_check_poll() {
				Some(VramProbe::Lost { uploaded, rendered }) => {
					lost = Some(format!(
						"sentinel uploaded={} rendered={}",
						if uploaded { "gone" } else { "ok" },
						if rendered { "gone" } else { "ok" }
					));
				}
				// not logged: every window every two seconds filled the log's
				// cap within a day, and it then missed the switches it was for
				Some(VramProbe::Intact) | None => {}
				Some(VramProbe::MapFailed) => {
					vramdbg("probe: sentinel readback map FAILED (inconclusive)");
				}
			}
			if let Some(wp) = gpu.wallpaper_img.as_mut() {
				match wp.vram_check_poll(&gpu.gfx.device) {
					Some(WpProbe::Lost) => lost = Some("wallpaper block gone".into()),
					Some(WpProbe::Intact) | None => {}
					Some(WpProbe::MapFailed) => {
						vramdbg("probe: wallpaper readback map FAILED (inconclusive)");
					}
				}
			}
			if Instant::now() >= state.vram_next {
				gpu.gfx.vram_check_start();
				if let Some(wp) = gpu.wallpaper_img.as_mut() {
					wp.vram_check_start(&gpu.gfx.device, &gpu.gfx.queue);
				}
				state.vram_next = Instant::now() + VRAM_CHECK_IVL;
			}
			if let Some(what) = lost {
				eprintln!(
					"{}: GPU texture contents lost (VT switch or resume?) - rebuilding",
					config::APP_NAME
				);
				vramdbg(&format!("probe: LOST ({what}) -> recover_gpu"));
				state.recover_gpu();
			}
		}
		let bell_anim = state.bell_flash > 0.0;
		// Until revealed (born hidden, shown at final size), keep rendering so the
		// reveal check runs each cycle; the deadline wake below guarantees it can't
		// stay hidden if no post-resize frame is otherwise triggered.
		if !state.revealed {
			state.dirty = true;
		}
		// The startup benchmark: due, so start it on the heaviest rung. It waits
		// for the wallpaper whatever the delay says, since the wallpaper is part
		// of what is being timed. A pinned frame rate would time itself rather
		// than the machine, so it cancels the run outright (the first pick stands).
		if state.bench_at.is_some_and(|at| Instant::now() >= at) && !state.bench_blocked() {
			state.bench_at = None;
			state.bench_cap = None;
			if max_fps().is_none() {
				state.bench_from = Some(crate::profile::current(&config::settings()));
				state.bench = Some(crate::profile::Bench::new());
				let rung = state
					.bench
					.as_ref()
					.map_or(crate::profile::Profile::Max, crate::profile::Bench::profile);
				state.set_live_profile(rung);
			} else {
				// nothing will be measured, so the banner has nothing to cover
				state.bench_banner = None;
				state.dirty = true;
			}
		}
		// A run renders flat out: the periods between frames ARE the measurement.
		if state.bench.is_some() {
			state.dirty = true;
		}
		if state
			.bench_banner_wake()
			.is_some_and(|wake| Instant::now() >= wake)
		{
			state.bench_banner = None;
			state.bench_kept = None;
			state.bench_stalled = false;
			state.dirty = true;
		}
		// Fully hidden window: don't build a frame nobody can see. PTY reading
		// never stops, so the grid keeps up and the reveal is one catch-up frame.
		let hidden = state.freeze_sync();
		// A window that let its device go takes it back the moment it is wanted
		// on screen, and not before: output into a hidden one waits for the
		// reveal. One left alone for long enough lets it go, unless a dialog is
		// up (on X11 the dialog's context cannot outlive the terminal's).
		let dialog_up = self.dialog.is_some() || self.notice.is_some();
		if state.gpu.is_none() {
			if state.idle.wake_owed && !hidden {
				state.rebuild_gpu();
			}
		} else if !dialog_up
			&& state
				.release_deadline(&config::settings(), hidden)
				.is_some_and(|due| Instant::now() >= due)
		{
			state.release_gpu();
			// the dialogs' warm context is a second device; it comes back
			// with the first (see the warm-up at the top of this pass)
			self.gpu_warm.release();
		}
		// A pass that draws nothing pauses the watch too, or the next ease's first
		// period would be the whole idle gap before it.
		let flow = if hidden || state.gpu.is_none() {
			state.rating.pause();
			ControlFlow::Wait
		} else if state.dirty || content || scroll_anim || cursor_anim || bell_anim {
			// UI/chrome changes and the bell force ALL panes to re-shape; fresh
			// output and scroll eases are scoped per pane inside render (a pure
			// cursor-animation frame lets every pane reuse its cached frame).
			let force = state.dirty || bell_anim;
			state.dirty = false;
			crate::perf::bump(&crate::perf::FRAMES);
			let animating = crate::perf::timed(&crate::perf::RENDER_NS, || state.render(force));
			// how the ease is paced tells whether the display keeps up
			let step = rating_step(
				state.bench.is_some(),
				scroll_anim,
				max_fps().is_some(),
				state.focused,
			);
			if state.bench.is_some() {
				// a run is timing the rungs itself; the step-down would be reading
				// the same frames and moving the profile out from under it
				let budget = state
					.frame_budget
					.at(Instant::now(), || refresh_hz(&state.window));
				match state.bench.as_mut().map(|b| b.note(Instant::now(), budget)) {
					Some(crate::profile::Step::Rung(next)) => state.set_live_profile(next),
					Some(crate::profile::Step::Done(pick)) => state.finish_bench(pick),
					Some(crate::profile::Step::Stalled) => state.finish_bench_stalled(),
					_ => {}
				}
			}
			match step {
				RatingStep::Note => {
					let budget = state
						.frame_budget
						.at(Instant::now(), || refresh_hz(&state.window));
					state.rating.note(Instant::now(), budget);
					if state.rating.verdict(budget) == Some(true) {
						state.step_down_profile();
					}
				}
				RatingStep::Pause => state.rating.pause(),
			}
			// a pane whose term was locked kept its content_dirty (rebuild was
			// skipped) - retry shortly instead of waiting for the next event,
			// or the last wakeup of a burst could leave a stale frame up
			let retry = state.tabs.cur().panes.values().any(|p| p.content_dirty);
			let pace = max_fps().map(|fps| Duration::from_secs_f64(1.0 / fps));
			if state.bench.is_some() {
				ControlFlow::Poll
			} else if animating && (scroll_anim || bell_anim) {
				// scroll (the flagship smooth feature) and the bell flash render
				// at full rate; fresh content needs no Poll - each PTY read
				// batch arrives as its own Wakeup
				match pace {
					Some(ivl) => pace_frame(&mut state.next_frame, ivl),
					None => ControlFlow::Poll,
				}
			} else if retry {
				ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(5))
			} else if animating {
				// a lone idle cursor blink is capped to ~30fps so it isn't
				// re-rendering every frame just to pulse - but a pinned rate
				// covers the cursor too, or a scene where the cursor is the only
				// thing moving samples off the grid the rest of the run is on
				match pace {
					Some(ivl) => pace_frame(&mut state.next_frame, ivl),
					None => ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33)),
				}
			} else {
				ControlFlow::Wait
			}
		} else {
			state.rating.pause();
			ControlFlow::Wait
		};
		// Debounced remember-size: persist once the size has held; while one is
		// pending, make sure the loop wakes up to flush it even when idle.
		state.flush_window_size(false);
		let flow = if let (ControlFlow::Wait, Some(_)) = (flow, state.pending_size) {
			ControlFlow::WaitUntil(state.pending_size_at + SIZE_SAVE_DEBOUNCE)
		} else {
			flow
		};
		// copy-output: while a capture is armed, make sure the loop wakes at its
		// settle deadline to run the capture check even when otherwise idle.
		let flow = match (flow, state.capture_wake()) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// keep frames coming while a dialog field edit animates
		let flow = match (flow, dlg_wake) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// keep the loop waking while dialog-raise retries are pending
		let flow = match (flow, raise_wake) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// Read after the frame, since the frame and the rating step after it can
		// set a wake or owe another frame: a cursor that parks, a minimap compose
		// that owes another, the window revealed, a rating ended. Read before,
		// each waited for some unrelated event.
		if let Some(wake) = pane_wake(state.tabs.cur().panes.values(), Instant::now()) {
			cursor_wake = Some(cursor_wake.map_or(wake, |w| w.min(wake)));
		}
		let flow = match (
			flow,
			(state.dirty && !hidden && state.gpu.is_some()).then(Instant::now),
		) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake a parked cursor at its scheduled resume time, even when idle
		let flow = match (flow, cursor_wake) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake to let the device go once the window has sat idle long enough
		let idle_wake = (state.gpu.is_some() && !dialog_up)
			.then(|| state.release_deadline(&config::settings(), hidden))
			.flatten();
		let flow = match (flow, idle_wake) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake to take "resources restored" out of the title
		let flow = match (flow, state.conserve.wake()) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake for the second heal after a return to this console
		let flow = match (flow, state.vt_heal.again) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake to rotate the wallpaper when its interval is up, even when idle
		// (not while the device is gone: the rebuild picks up where it left off)
		let flow = match (flow, state.wp_next.filter(|_| state.gpu.is_some())) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake when the background shell scan comes due, even on an idle window
		let flow = match (flow, state.shell_scan_at) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake to raise a tab tip whose pointer has rested, and to keep an open
		// one current - the pointer sitting still generates no events of its own,
		// so nothing else would bring the window back
		let flow = match (flow, state.tip_wake()) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake when the benchmark comes due, and again when its banner may come
		// down - an idle window generates nothing of its own to bring it back
		let bench_wake = match (state.bench_at, state.bench_cap) {
			(Some(_), Some(cap)) if state.bench_blocked() => Some(cap),
			(at, _) => at,
		};
		let flow = match (flow, bench_wake.or(state.bench_banner_wake())) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// wake at the reveal deadline so a hidden startup window is shown even if no
		// post-resize frame arrives
		let flow = match (flow, (!state.revealed).then_some(state.reveal_deadline)) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// slow-tick wake so the VRAM sentinel probe runs even while fully idle
		let flow = match (
			flow,
			(state.gl && state.gpu.is_some()).then_some(state.vram_next),
		) {
			(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
			(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
			(other_flow, _) => other_flow,
		};
		// Profiling keeps the loop hot so the workload is continuously exercised.
		#[cfg(feature = "profiling")]
		let flow = if std::env::var_os("SILK_PROFILE_OUT").is_some() {
			ControlFlow::Poll
		} else {
			flow
		};
		event_loop.set_control_flow(flow);
	}
}

impl State {
	// Ctrl+Shift+C, focused window or not
	fn copy_selection(&mut self) {
		if !input::copy_allowed(CopyFrom::Chord, self.focused, true) {
			return;
		}
		let focused = self.tabs.cur().focused;
		if let Some(text) = self
			.tabs
			.cur()
			.panes
			.get(&focused)
			.and_then(super::pane::Pane::selection_text)
		{
			self.clipboard.set_clipboard(text);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{
		Caret, CloseScope, Conserve, ContextMenu, CopyMetrics, Entry, Idle, IdleClock, MenuAction,
		PaneWakes, RESTORED_SHOWN, SCRIM_PCT_PER_DOUBLING, TAB_CLOSE_M, TabEdit, VT_SETTLE,
		ViewState, VtHeal, accel_at, accel_clash, close_scope, copybox_fit, copybox_place, fit_px,
		focus_ring, is_copy_chord, key_is_typed, menu_metrics, mia, msub, mta, needs_folder_read,
		new_window_command, notice_due, pace_frame, pane_wake, rating_step, release_deadline,
		remember_resize, rotation_live, rotation_next, settings_after_reload, tab_close_box,
		tab_command_line, tab_title_w, typed_title, view_menu_items, window_px,
	};
	use super::{
		CopyBoxes, CtxState, Dir, MENU_BAR, MENU_BAR_VPAD, Rect, ShellEntry, TextCtx, bar_menu_for,
		bar_title_underlines, context_menu_items, default_dir_for, edit_menu_items, entry_accel,
		entry_label, file_menu_items, help_menu_items, menubar_text_slot, pane_shell,
		pane_split_dir, panes_menu_items, push_back, split_shells, tabs_menu_items,
	};
	use crate::config;
	use std::time::{Duration, Instant};
	use winit::event::ElementState;

	// The strength scale went from 10% to 20% per doubling in August and design.md
	// kept the old numbers for weeks, with nothing to catch it. Both places that
	// quote the number are held against the code now.
	// Test ID: EqSDfqy
	#[test]
	fn the_docs_quote_the_scrim_strength_scale_the_code_uses() {
		let pct = SCRIM_PCT_PER_DOUBLING;
		let spelled = [
			"zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
		];
		let doublings = spelled[(100.0 / pct) as usize];
		let read = |rel: &str| {
			let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
			std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
		};
		let said =
			format!("each {pct:.0}% doubles its opacity, up to {doublings} doublings at 100%");
		// the doc's name starts with the time it was written, so find it by its tail
		let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../project/design_docs");
		let doc = std::fs::read_dir(&docs)
			.unwrap_or_else(|e| panic!("{}: {e}", docs.display()))
			.filter_map(Result::ok)
			.map(|e| e.path())
			.find(|p| p.to_string_lossy().ends_with("_scrim.md"))
			.expect("no scrim design doc");
		assert!(
			std::fs::read_to_string(&doc).unwrap().contains(&said),
			"{} does not say: {said}",
			doc.display()
		);
		let commented = format!("each {pct:.0}% is one doubling");
		assert!(
			read("src/settings_ui.shcl").contains(&commented),
			"settings_ui.shcl does not say: {commented}"
		);
	}

	// A save nobody asked for can be refused at every resize, so it is said once
	// a session for each file. An OK in Settings that could not save is said
	// every time, or the button would seem to do nothing.
	// Test ID: EqGnMOu
	#[test]
	fn a_refused_save_is_said_once_unless_it_was_asked_for() {
		let mut told = Vec::new();
		let file = std::path::Path::new("/c/config.shcl");
		let other = std::path::Path::new("/c/other.shcl");
		assert!(notice_due(&mut told, file, false));
		assert!(!notice_due(&mut told, file, false), "the next resize");
		assert!(notice_due(&mut told, other, false), "another file");
		assert!(notice_due(&mut told, file, true), "an OK in Settings");
		assert!(notice_due(&mut told, file, true), "and the next one");
	}

	// A menu outlives the pane it was opened for when that pane's shell ends, and
	// the tab can go with it. Close pane then found no such pane in the current tab
	// and fell through to closing the tab, or the window, with another tab's shells
	// still running.
	// Test ID: Eq4SnxI
	#[test]
	fn close_pane_on_a_pane_that_is_gone_closes_nothing() {
		assert_eq!(close_scope(false, 1, 1), CloseScope::Nothing);
		assert_eq!(close_scope(false, 2, 3), CloseScope::Nothing);
		// a live pane still cascades: the pane, then its tab, then the window
		assert_eq!(close_scope(true, 2, 1), CloseScope::Pane);
		assert_eq!(close_scope(true, 1, 2), CloseScope::Tab);
		assert_eq!(close_scope(true, 1, 1), CloseScope::Window);
	}

	// `window.rows: 1000` in the config, or --rows 1000, asked for a window taller
	// than the device's largest texture. The GL path's offscreen is made at the
	// window's size, and wgpu treats the refusal as fatal, so the launch died.
	// Test ID: Eq4SnxJ
	#[test]
	fn a_window_is_never_bigger_than_the_device_allows() {
		let (w, h) = window_px(1000, 1000, 8.0, 17.0, 4.0, 30.0);
		assert_eq!((w, h), (8008, 17038));
		assert_eq!(fit_px(w, h, 16384), (8008, 16384));
		assert_eq!(fit_px(0, 0, 16384), (1, 1));
	}

	// A requested row count is the shell's rows, so the tab strip counts against
	// the window's height while it shows. Left out of the sum, a window asked for
	// 24 rows gave its shell 22.
	// Test ID: Eq4SnxK
	#[test]
	fn the_tab_strip_counts_against_a_requested_row_count() {
		let shown = window_px(80, 24, 8.0, 17.0, 4.0, 20.0 + 24.0);
		let hidden = window_px(80, 24, 8.0, 17.0, 4.0, 20.0);
		assert_eq!(shown.0, hidden.0);
		assert_eq!(shown.1 - hidden.1, 24);
	}

	// The idle release: off by default, never while the window has focus on
	// screen, and a hidden window waits the shorter of the two times. Everything
	// that vetoes it is a None, since a deadline that then had to be checked
	// again elsewhere is how a veto gets forgotten.
	// Test ID: Eq8b2Gu
	#[test]
	fn the_idle_release_waits_on_the_window_and_only_an_unwatched_one() {
		let since = Instant::now();
		let idle = |focused, hidden| Idle {
			focused,
			hidden,
			revealed: true,
			bench_busy: false,
			since,
		};
		let mut cfg = config::Settings::default();
		assert!(
			release_deadline(&cfg, &idle(false, true)).is_none(),
			"off by default"
		);
		cfg.idle_release = true;
		cfg.idle_release_hidden_min = 30;
		cfg.idle_release_min = 240;
		assert!(
			release_deadline(&cfg, &idle(true, false)).is_none(),
			"focused and on screen"
		);
		assert_eq!(
			release_deadline(&cfg, &idle(false, false)),
			Some(since + Duration::from_hours(4))
		);
		assert_eq!(
			release_deadline(&cfg, &idle(false, true)),
			Some(since + Duration::from_mins(30))
		);
		// minimized with focus still nominally on it: out of sight is what counts
		assert_eq!(
			release_deadline(&cfg, &idle(true, true)),
			Some(since + Duration::from_mins(30))
		);
		let mut owed = idle(false, true);
		owed.bench_busy = true;
		assert!(
			release_deadline(&cfg, &owed).is_none(),
			"a rating in flight"
		);
		let mut unshown = idle(false, true);
		unshown.revealed = false;
		assert!(
			release_deadline(&cfg, &unshown).is_none(),
			"not on screen yet"
		);
	}

	// A program printing in a minimized window held its device for good, since
	// every output started the idle clock over. Output nobody can see leaves the
	// clock alone, but a released window is still owed its device for the reveal.
	// Test ID: EqBNCpU
	#[test]
	fn output_into_a_hidden_window_does_not_hold_its_device() {
		let long_ago = Instant::now()
			.checked_sub(Duration::from_secs(1))
			.expect("a second of uptime");
		let mut clock = IdleClock {
			since: long_ago,
			wake_owed: false,
		};
		assert!(!clock.output(false, true));
		assert_eq!(clock.since, long_ago, "hidden output restarted the clock");
		assert!(!clock.wake_owed);
		assert!(clock.output(true, true), "a released window is owed it");
		assert!(clock.wake_owed);
		assert_eq!(clock.since, long_ago);
		assert!(!clock.output(true, true), "owed once");

		let mut seen = IdleClock {
			since: long_ago,
			wake_owed: false,
		};
		assert!(!seen.output(false, false));
		assert!(seen.since > long_ago, "output on screen is a sign of life");
		assert!(seen.output(true, false));
		assert!(seen.wake_owed);
	}

	// Test ID: EqFWtPs
	#[test]
	fn the_title_note_follows_the_device_out_and_back() {
		let now = Instant::now();
		assert_eq!(Conserve::Off.note(now), None);
		assert_eq!(
			Conserve::Saving.note(now),
			Some("resource conservation mode")
		);
		let mut state = Conserve::Saving;
		state.wallpaper_answered(now);
		assert_eq!(
			state,
			Conserve::Saving,
			"a wallpaper while released changes nothing"
		);
		state = Conserve::Restoring;
		assert_eq!(state.note(now), Some("restoring resources ..."));
		assert_eq!(
			state.wake(),
			None,
			"restoring waits on the wallpaper, not a clock"
		);
		state.wallpaper_answered(now);
		assert_eq!(state.note(now), Some("resources restored"));
		assert_eq!(state.wake(), Some(now + RESTORED_SHOWN));
		assert_eq!(state.note(now + RESTORED_SHOWN), None);
	}

	// A return to this console is healed at once and once more when the X
	// server has settled, since a purge after the first rebuild spoiled it
	// (20260917: text gone and a gray background after a switch to VT 1).
	// Test ID: EqBP7iS
	#[test]
	fn a_return_to_this_console_is_healed_again_once_settled() {
		let back = Instant::now();
		let mut heal = VtHeal::default();
		assert!(!heal.due(back), "nothing owed before a switch");
		heal.returned(back);
		assert!(!heal.due(back), "the first heal is the event's own");
		assert!(!heal.due(back + VT_SETTLE / 2));
		assert!(heal.due(back + VT_SETTLE), "the second heal");
		assert!(!heal.due(back + VT_SETTLE * 2), "and only once");
		// a second switch before the first settled pushes the pass out
		heal.returned(back);
		heal.returned(back + VT_SETTLE / 2);
		assert!(!heal.due(back + VT_SETTLE));
		assert!(heal.due(back + VT_SETTLE + VT_SETTLE / 2));
	}

	// --fullscreen asks for fullscreen before the first frame, and the window
	// manager's resize arrives after it, so a one-off fullscreen launch stored the
	// whole screen as the size to open at next time.
	// Test ID: Eq4SnxL
	#[test]
	fn a_fullscreen_or_maximized_size_is_not_remembered() {
		assert!(remember_resize(true, false, false));
		assert!(!remember_resize(true, true, false));
		assert!(!remember_resize(true, false, true));
		// nothing before the first frame, as before
		assert!(!remember_resize(false, false, false));
	}

	// Only a focused window's own eased frame is evidence; every other pass
	// pauses the watch, so an idle gap is never read as a period.
	// Test ID: EpWnbLc
	#[test]
	fn only_a_focused_eased_unpinned_frame_is_counted() {
		use super::RatingStep;
		let mut notes = 0;
		for bits in 0..16u8 {
			let (bench, scroll, pinned, focused) =
				(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
			let step = rating_step(bench, scroll, pinned, focused);
			if (bench, scroll, pinned, focused) == (false, true, false, true) {
				assert_eq!(step, RatingStep::Note);
				notes += 1;
			} else {
				assert_eq!(
					step,
					RatingStep::Pause,
					"bench {bench} scroll {scroll} pinned {pinned} focused {focused}"
				);
			}
		}
		assert_eq!(notes, 1);
	}

	// A bench rung and the bench's answer are both measured, so a step the
	// display watch took goes, and the stored profile is the user's own values.
	// Test ID: EpWow4e
	#[test]
	fn a_measured_profile_replaces_a_session_step() {
		use crate::profile::Profile;
		let mut live = config::Settings {
			performance_profile: "max".to_string(),
			stepped_profile: Some(Profile::Low),
			..config::Settings::default()
		};
		crate::profile::apply(&mut live);
		assert_eq!(crate::profile::current(&live), Profile::Low);
		let next = super::with_measured_profile(&live, Profile::High);
		assert_eq!(next.stepped_profile, None);
		assert_eq!(next.performance_profile, "high");
		assert!(next.profile_shadow.is_none(), "the user's own values");
		assert_eq!(crate::profile::current(&next), Profile::High);
	}

	// The watch's step is session state. Stored in the profile or written out,
	// one stall became every later launch's profile and took the wallpaper.
	// Test ID: EpX5Wgq
	#[test]
	fn a_watch_step_never_reaches_the_stored_profile_or_the_file() {
		use crate::profile::Profile;
		let _guard = config::test_config_lock();
		let _ = config::settings();
		let dir = std::env::temp_dir().join(format!("silkterm_appstep_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		config::set_config_override(path.clone());
		let cases = [
			(true, "max", Some(Profile::High)),
			(true, "high", Some(Profile::Low)),
			(true, "low", None),
			(true, "standard", None),
			(true, "custom", None),
			(false, "max", None),
		];
		for (automatic, stored, want) in cases {
			std::fs::write(
				&path,
				format!("performance:\n\tautomatic: {automatic}\n\tprofile: \"{stored}\"\n"),
			)
			.unwrap();
			let mut live = config::reload_from_disk();
			crate::profile::apply(&mut live);
			let before = std::fs::read_to_string(&path).unwrap();
			let next = super::watch_step_down(&live);
			assert_eq!(
				std::fs::read_to_string(&path).unwrap(),
				before,
				"automatic {automatic}, stored {stored}: taking the step writes nothing"
			);
			assert_eq!(
				next.as_ref().and_then(|n| n.stepped_profile),
				want,
				"automatic {automatic}, stored {stored}"
			);
			let Some(next) = next else {
				continue;
			};
			assert_eq!(next.performance_profile, stored, "the stored profile stays");
			assert_eq!(Some(crate::profile::current(&next)), want);
			assert!(config::persist(&live, &next));
			assert_eq!(
				std::fs::read_to_string(&path).unwrap(),
				before,
				"stored {stored}: nor does saving the settings it is in"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	fn adapter(name: &str, device_type: wgpu::DeviceType) -> wgpu::AdapterInfo {
		wgpu::AdapterInfo {
			name: name.to_string(),
			vendor: 0,
			device: 0,
			device_type,
			device_pci_bus_id: String::new(),
			driver: String::new(),
			driver_info: String::new(),
			backend: wgpu::Backend::Gl,
			subgroup_min_size: 0,
			subgroup_max_size: 0,
			transient_saves_memory: false,
		}
	}

	// Which launches owe a rating. Pulled out of the launch path when every write
	// in it changed, so the decision itself provably did not.
	// Test ID: EpXN9p2
	#[test]
	fn rating_due_matches_the_launch_rules() {
		let hardware = "0123456789abcdef";
		let base = config::Settings {
			performance_automatic: true,
			performance_check_hardware: true,
			performance_check_next_run: false,
			rated_hardware: "fedcba9876543210".to_string(),
			remote_override: false,
			..config::Settings::default()
		};
		let matching = config::Settings {
			rated_hardware: hardware.to_string(),
			..base.clone()
		};
		let cases = [
			(
				"automatic off",
				config::Settings {
					performance_automatic: false,
					..base.clone()
				},
				false,
			),
			(
				"a remote screen",
				config::Settings {
					remote_override: true,
					..base.clone()
				},
				false,
			),
			(
				"asked for, even on the same hardware",
				config::Settings {
					performance_check_next_run: true,
					..matching.clone()
				},
				true,
			),
			("the same hardware", matching.clone(), false),
			("other hardware, checked", base.clone(), true),
			(
				"other hardware, not checked",
				config::Settings {
					performance_check_hardware: false,
					..base.clone()
				},
				false,
			),
			(
				"never rated, not checked",
				config::Settings {
					performance_check_hardware: false,
					rated_hardware: String::new(),
					..base.clone()
				},
				true,
			),
		];
		for (what, live, due) in cases {
			assert_eq!(super::rating_due(&live, hardware), due, "{what}");
		}
	}

	// A rating that did not reach the file was a test at every launch. Each file
	// here is one that used to lose it or read it back as nothing: a clean one, one
	// with a line the parse cannot place, and one with the key twice.
	// Test ID: EpXN9p3
	#[test]
	fn a_rating_survives_to_the_next_launch_whatever_else_the_file_holds() {
		use crate::profile::Profile;
		let _guard = config::test_config_lock();
		let saved = config::settings();
		let _store = config::test_store_lock();
		let dir = std::env::temp_dir().join(format!("silkterm_ratingkept_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		config::set_config_override(path.clone());
		let card = adapter("NVIDIA GeForce RTX 3060 Ti", wgpu::DeviceType::DiscreteGpu);
		let soft = adapter("llvmpipe (LLVM 19.1.7, 256 bits)", wgpu::DeviceType::Cpu);
		// what a launch does before it rates: read the file and put it live
		let install = || config::update(config::reload_from_disk());
		let read = || std::fs::read_to_string(&path).unwrap();
		let files = [
			(
				"clean",
				"performance:\n\t# rated_hardware: \"\"  ## Default\n",
			),
			(
				"an unreadable line",
				"window:\n\topacity: 1.0\n    margin: 4\n\nperformance:\n\t# rated_hardware: \"\"  ## Default\n",
			),
			(
				"the key twice",
				"performance:\n\trated_hardware: 0000000000000000\n\trated_hardware: 0000000000000000\n",
			),
		];
		for (name, file) in files {
			std::fs::write(&path, file).unwrap();
			install();
			let id = super::rate_hardware(&card)
				.unwrap_or_else(|| panic!("{name}: a card with no rating on file is measured"));
			assert_eq!(
				super::keep_measured(Profile::High, Some(&id)),
				config::Kept::Written,
				"{name}"
			);
			install();
			assert_eq!(
				super::rate_hardware(&card),
				None,
				"{name}: the next launch keeps the card's rating"
			);

			std::fs::write(&path, file).unwrap();
			install();
			match super::rate_hardware(&soft) {
				Some(id) if crate::profile::worth_measuring(&soft) => assert_eq!(
					super::keep_measured(Profile::Low, Some(&id)),
					config::Kept::Written,
					"{name}"
				),
				answer => assert_eq!(
					answer, None,
					"{name}: a software adapter is decided at launch"
				),
			}
			install();
			let stored = config::reload_from_disk();
			assert_eq!(stored.performance_profile, "low", "{name}");
			assert_eq!(
				stored.rated_hardware,
				crate::profile::hardware_id(&soft),
				"{name}"
			);
			let before = read();
			assert_eq!(
				super::rate_hardware(&soft),
				None,
				"{name}: the next launch keeps the software adapter's rating"
			);
			assert_eq!(read(), before, "{name}: and writes nothing");
			if name == "an unreadable line" {
				assert!(
					before.contains("\n    margin: 4\n"),
					"the unreadable line is still there:\n{before}"
				);
			}
		}
		config::update((*saved).clone());
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A failed save used to leave the banner saying the test was running and then
	// run it again next launch. It says why now, for the seconds it stays up.
	// Test ID: EpXN9p4
	#[test]
	fn the_banner_says_why_a_rating_was_not_kept() {
		const AGAIN: &str = "The test runs again at the next launch.";
		let running = ["Testing performance", "This takes a few seconds."];
		assert_eq!(super::bench_banner_lines(None), running);
		assert_eq!(
			super::bench_banner_lines(Some(&config::Kept::Written)),
			running
		);
		for (kept, why) in [
			(
				config::Kept::Busy,
				"The settings file is open in another program.",
			),
			(
				config::Kept::Unreadable,
				"The settings file has a line that cannot be read.",
			),
			(
				config::Kept::Unplaced,
				"The performance section of the settings file could not be updated.",
			),
			(
				config::Kept::Unwritable("could not write x: denied".to_string()),
				"The settings file cannot be written.",
			),
		] {
			assert_eq!(
				super::bench_banner_lines(Some(&kept)),
				["Could not save the result", why, AGAIN],
				"{kept:?}"
			);
		}
	}

	// Switching the wallpaper off and on again drops the rotation pick, and a
	// request that does not re-read the folder then answers with nothing at all:
	// the folder suppresses the built-in, and there is no pick to fall back on.
	// Test ID: Ep1Y6Tg
	#[test]
	fn a_rotation_folder_with_nothing_showing_is_read_again() {
		let pick = std::path::PathBuf::from("/w/a.jpg");
		let folder = std::path::PathBuf::from("/w");
		assert!(needs_folder_read(false, None, Some(&folder)));
		// something from the folder is already up, or there is no folder at all
		assert!(!needs_folder_read(false, Some(&pick), Some(&folder)));
		assert!(!needs_folder_read(false, None, None));
		// a command-line wallpaper owns the session, rotation stays out of it
		assert!(!needs_folder_read(true, None, Some(&folder)));
	}

	// Read-only means the pane takes nothing the user's hands sent. Typing and
	// paste were on that list; the mouse reports and the wheel's alt-screen
	// cursor keys were not, so one notch sent arrow keys to the job the pane said
	// it was protecting. Everything user-driven goes through `write_input` now,
	// and the only direct write left is the reply the terminal owes the program.
	// Test ID: EpHQ61Q
	#[test]
	fn a_read_only_pane_takes_nothing_the_user_sent() {
		let body = include_str!("app.rs")
			.split("\nmod tests {")
			.next()
			.expect("the file above its own tests");
		let direct: Vec<&str> = body
			.lines()
			.filter(|l| l.contains("term.write("))
			.map(str::trim)
			.collect();
		assert_eq!(
			direct.len(),
			1,
			"these bypass the read-only gate: {direct:?}"
		);
		assert!(
			body.matches("write_input(").count() >= 5,
			"the mouse and wheel writes go through the gate"
		);
	}

	// A tick that leaves the timer where it was fires again on the next pass, and
	// each of those starts another decode thread.
	// Test ID: EpHNMfQ
	#[test]
	fn a_rotation_tick_moves_the_timer_off_now() {
		let now = Instant::now();
		let next = rotation_next(now, true, 2.0).expect("a live rotation keeps its timer");
		assert!(next > now);
		// nothing to rotate to, or rotation switched off: no timer at all
		assert!(rotation_next(now, false, 2.0).is_none());
		assert!(rotation_next(now, true, 0.0).is_none());
	}

	// Clearing the box is how a renamed tab goes back to naming itself, so a
	// blank title must not be stored as one.
	// Test ID: EoTYmjQ
	#[test]
	fn a_cleared_tab_title_is_no_title_at_all() {
		assert_eq!(typed_title(String::new(), "bash"), None);
		assert_eq!(typed_title("   ".to_string(), "bash"), None);
		assert_eq!(typed_title("bash".to_string(), "bash"), None);
		assert_eq!(
			typed_title("notes".to_string(), "bash"),
			Some("notes".to_string())
		);
	}

	// A tab title is renamed by byte offset over text that need not be ASCII, so
	// every move and every erase has to fall on a character boundary or the
	// string operations panic.
	// Test ID: EoSiOoS
	#[test]
	fn renaming_a_tab_stays_on_character_boundaries() {
		let mut edit = TabEdit {
			tab: 0,
			text: "naïve".to_string(),
			caret: 0,
			anchor: 0,
		};
		// past the two-byte i-with-diaeresis and back
		for _ in 0..3 {
			edit.move_caret(Caret::Right, false);
		}
		assert_eq!(edit.caret, 4);
		edit.move_caret(Caret::Left, false);
		assert_eq!(edit.caret, 2);

		// a selection dragged with Shift, then typed over
		edit.move_caret(Caret::End, true);
		assert_eq!(edit.range(), (2, 6));
		edit.insert("p");
		assert_eq!(edit.text, "nap");
		assert_eq!(edit.caret, 3);

		// backspace over the whole thing leaves it empty rather than underflowing
		for _ in 0..5 {
			edit.erase(true);
		}
		assert_eq!(edit.text, "");
		assert_eq!(edit.caret, 0);

		// delete at the end has nothing to take
		edit.erase(false);
		assert_eq!(edit.text, "");
	}

	// The chrome shares a coordinate space with the terminal grid, so nothing
	// A narrow window used to draw "Copy on:" straight over "Panes" and "Help" -
	// both there, neither readable. The cluster sheds parts instead, and the
	// checkboxes are the last thing to go because they carry the state.
	// Test ID: Eo6mi5I
	#[test]
	fn the_copy_cluster_sheds_parts_before_it_reaches_the_menu_titles() {
		let metrics = |right: f32| CopyMetrics {
			right,
			label_w: [56.0, 34.0, 40.0],
			box_sz: 10.0,
			box_y: 4.0,
			box_gap: 6.0,
			pair_gap: 14.0,
			lead_gap: 10.0,
		};
		let titles_right = 300.0;
		let shown_at = |right: f32| copybox_fit(&metrics(right), titles_right).map(|cb| cb.shown);
		assert_eq!(shown_at(700.0), Some([true; 3]), "room for all of it");
		assert_eq!(
			shown_at(450.0),
			Some([false, true, true]),
			"the lead-in goes first"
		);
		assert_eq!(shown_at(400.0), Some([false; 3]), "then the two words");
		assert_eq!(shown_at(330.0), None, "and then the cluster itself");
	}

	// A word that is not drawn cannot be aimed at, so the box has to answer for
	// itself - otherwise the narrow arrangement has a dead checkbox.
	// Test ID: Eo6mi5J
	#[test]
	fn a_checkbox_with_no_word_is_still_clickable() {
		let metrics = CopyMetrics {
			right: 400.0,
			label_w: [56.0, 34.0, 40.0],
			box_sz: 10.0,
			box_y: 4.0,
			box_gap: 6.0,
			pair_gap: 14.0,
			lead_gap: 10.0,
		};
		let bare = copybox_place(&metrics, [false; 3]);
		for i in 0..2 {
			let (left, right) = bare.hit_range(i);
			assert_eq!(left, bare.boxes[i].x);
			assert_eq!(right, bare.boxes[i].x + bare.boxes[i].w);
		}
		let full = copybox_place(&metrics, [true; 3]);
		let (_, right) = full.hit_range(0);
		assert_eq!(
			right,
			full.label_x[1] + full.label_w[1],
			"the word counts too"
		);
	}

	// converts it at a boundary the way the Settings dialog does - every piece
	// scales at its own use site, and a piece that misses out is exactly the
	// defect this pass fixed (chrome thinning out as the display's DPI rises).
	// So: the same geometry at 2x must come out at twice the size, everywhere.
	// Test ID: EnLuU52
	#[test]
	fn the_chrome_doubles_when_the_display_does() {
		// the tab's close button, measured against a bar that has itself doubled
		let one = tab_close_box(0.0, 200.0, 0.0, 30.0, 1.0);
		let two = tab_close_box(0.0, 400.0, 0.0, 60.0, 2.0);
		assert_eq!(two.w, one.w * 2.0, "close button width");
		assert_eq!(two.h, one.h * 2.0, "close button height");
		assert_eq!(two.y, one.y * 2.0, "close button top margin");
		// its right margin, which is what a raw-px inset would have left frozen
		assert_eq!(400.0 - (two.x + two.w), (200.0 - (one.x + one.w)) * 2.0);
		assert_eq!(one.y, TAB_CLOSE_M, "1x must still be the DIP value itself");

		// the live pane's focus ring
		let rect = super::Rect {
			x: 0.0,
			y: 0.0,
			w: 100.0,
			h: 100.0,
		};
		let thin = focus_ring(rect, 1.0)[0].size[1];
		let thick = focus_ring(rect, 2.0)[0].size[1];
		assert_eq!(thin, config::FOCUS_RING_PX);
		assert_eq!(thick, thin * 2.0);
	}

	// Build a popup by hand, the way the geometry tests need it - no window, no
	// text context, just the numbers `row_top`/`item_at`/`step` are made of.
	// The width a title is FITTED to and the width the buffer is SHAPED at are
	// the same number by construction - shorten a path to a width the tab does not
	// then give it and the last component is clipped anyway, which is the whole
	// thing the shortening exists to avoid.
	// Test ID: EnbYSzw
	#[test]
	fn a_title_is_fitted_to_the_width_it_is_actually_given() {
		for scale in [1.0, 1.5, 2.0] {
			for tab_w in [60.0, 140.0, 300.0] {
				let title_w = tab_title_w(tab_w, scale);
				let close = tab_close_box(0.0, tab_w, 0.0, 30.0, scale);
				assert!(title_w > 0.0, "no room at all for a title");
				assert!(
					title_w <= tab_w,
					"a title wider than its own tab: {title_w} in {tab_w}"
				);
				// and it stops short of the close button rather than running under it
				assert!(
					title_w <= close.x || tab_w < config::dip(40.0, scale),
					"title {title_w} runs under the close box at {}",
					close.x
				);
			}
		}
	}

	// A tab may only name the shell it can SEE. A pane resolves its own command
	// at spawn, so an unresolved one means nothing was switched on and the engine
	// chose for itself - and a guess from the list is exactly what had a pane
	// running PowerShell labelled Command Prompt.
	// Test ID: EnbYSzx
	#[test]
	fn a_tab_names_only_the_shell_it_can_actually_see() {
		assert_eq!(tab_command_line(None), "");
		// An argument holding a space survives the round trip back into one line,
		// so the name lookup splits it the same way the launch did.
		let argv = vec!["C:/Program Files/x.exe".to_string(), "a b".to_string()];
		assert_eq!(
			tab_command_line(Some(&argv)),
			"\"C:/Program Files/x.exe\" \"a b\""
		);
	}

	fn test_menu(x: f32, w: f32, entries: Vec<Entry>) -> ContextMenu {
		ContextMenu {
			x,
			y: 0.0,
			w,
			item_h: 20.0,
			pad_y: 6.0,
			sep_h: 9.0,
			target: 0,
			entries,
			hover: None,
			sub: None,
		}
	}

	fn test_item(label: &str) -> Entry {
		Entry::Item {
			label: label.into(),
			action: MenuAction::Copy,
			check: None,
			accel: None,
		}
	}

	// A letter picks the first row carrying it, so a menu that spends one twice
	// does not read as ambiguous - it quietly makes the later row unreachable.
	// Every menu is checked for this where it is built.
	// Test ID: EnMAGUK
	#[test]
	fn one_menu_never_spends_an_accelerator_twice() {
		let rows = vec![
			mia('C', "Copy", MenuAction::Copy),
			msub(Some('S'), "New tab with shell", vec![]),
			mta('W', false, "Window frame", MenuAction::ToggleFrame),
		];
		assert_eq!(accel_clash(&rows), None);
		// 'w' again, which is what the View menu would have done had the submenu
		// row spelled its accelerator the way the Tabs menu's does
		let mut clashing = rows;
		clashing.insert(1, msub(Some('w'), "New tab with shell", vec![]));
		assert_eq!(accel_clash(&clashing), Some('w'));
	}

	// A submenu row is an ordinary row to the pointer and to the keyboard - only
	// what ACTIVATING it does is different. Treating it as a separator instead
	// (which is what the old two-arm matches did) leaves it unhoverable and
	// unreachable, i.e. an item nothing can ever pick.
	// Test ID: EnM97jE
	#[test]
	fn a_submenu_row_hit_tests_and_steps_like_an_item() {
		let menu = test_menu(
			0.0,
			200.0,
			vec![
				test_item("One"),
				msub(Some('w'), "With Shell", vec![test_item("Bash")]),
				Entry::Sep,
				test_item("Two"),
			],
		);
		let mid = |row: usize| menu.row_top(row) + menu.item_h / 2.0;
		assert_eq!(menu.item_at(10.0, mid(1)), Some(1), "the row is hoverable");
		let sep_mid = menu.row_top(2) + menu.sep_h / 2.0;
		assert_eq!(
			menu.item_at(10.0, sep_mid),
			None,
			"a separator still is not"
		);
		// down from the first row reaches it, and carries on past it
		assert_eq!(menu.step(Some(0), 1), Some(1));
		assert_eq!(menu.step(Some(1), 1), Some(3), "the separator is skipped");
		assert_eq!(menu.step(Some(3), -1), Some(1));
	}

	// The submenu is placed clear of its parent's right edge on purpose: that is
	// the whole of what keeps the pointer rule simple, since "inside the submenu"
	// and "on a parent row" can then never both be true. A submenu that overlaps
	// would close itself the moment the pointer entered it.
	// Test ID: EnM97jF
	#[test]
	fn a_submenu_stands_clear_of_the_rows_it_came_from() {
		let parent = test_menu(0.0, 200.0, vec![test_item("One"), test_item("Two")]);
		let sub = test_menu(200.0, 120.0, vec![test_item("Bash"), test_item("Zsh")]);
		for row in 0..2 {
			let y = sub.row_top(row) + sub.item_h / 2.0;
			let x = sub.x + sub.w / 2.0;
			assert!(sub.item_at(x, y).is_some(), "the submenu owns its own rows");
			assert!(
				parent.item_at(x, y).is_none(),
				"and the parent claims none of them"
			);
		}
	}

	// A click inside an open submenu is a click on the menu, so the chrome that
	// stands aside for a popup (the menu bar, the tab bar) has to stand aside for
	// it too - otherwise a submenu overlapping either band loses its clicks to it.
	// Test ID: EnM97jG
	#[test]
	fn a_click_in_the_submenu_still_counts_as_a_click_on_the_menu() {
		let mut parent = test_menu(0.0, 200.0, vec![msub(Some('w'), "With Shell", vec![])]);
		let sub = test_menu(200.0, 120.0, vec![test_item("Bash")]);
		let (x, y) = (sub.x + 10.0, sub.row_top(0) + 2.0);
		assert!(!parent.hit(x, y));
		assert!(!parent.hit_any(x, y), "nothing is open yet");
		parent.sub = Some(Box::new(sub));
		assert!(parent.hit_any(x, y));
		assert_eq!(parent.chain().len(), 2);
	}

	// A dropdown resolves its own padding and separator height from DIP once, at
	// the moment it is built, so the draw and the two hit tests read one set of
	// numbers. Whatever the display does to them, `item_at` and `row_top` have to
	// keep agreeing - a menu whose rows are drawn one place and clicked another
	// is the failure this guards.
	// Test ID: EnLuU53
	#[test]
	fn a_dropdown_scales_whole_and_its_rows_still_hit_test() {
		let menu_at = |scale: f32| {
			let (pad_y, sep_h) = menu_metrics(scale);
			ContextMenu {
				x: 0.0,
				y: 0.0,
				w: config::dip(200.0, scale),
				item_h: config::dip(20.0, scale),
				pad_y,
				sep_h,
				target: 0,
				entries: vec![
					Entry::Item {
						label: "One".into(),
						action: MenuAction::Copy,
						check: None,
						accel: None,
					},
					Entry::Sep,
					Entry::Item {
						label: "Two".into(),
						action: MenuAction::Paste,
						check: None,
						accel: None,
					},
				],
				hover: None,
				sub: None,
			}
		};
		// the padding and the separator row scale, which is what makes the whole
		// popup scale - height() and row_top() are built out of them
		assert_eq!(
			menu_metrics(1.0),
			(config::MENU_ITEM_PAD_Y, config::MENU_SEP_H)
		);
		assert_eq!(
			menu_metrics(2.0),
			(config::MENU_ITEM_PAD_Y * 2.0, config::MENU_SEP_H * 2.0)
		);
		let one = menu_at(1.0);
		let two = menu_at(2.0);
		assert_eq!(two.height(), one.height() * 2.0);
		assert_eq!(two.row_top(2), one.row_top(2) * 2.0);
		// every item is still picked at the row it is drawn on, at either scale
		for menu in [&one, &two] {
			for i in [0usize, 2] {
				let mid = menu.row_top(i) + menu.item_h / 2.0;
				assert_eq!(menu.item_at(menu.w / 2.0, mid), Some(i));
			}
			// the separator's own band belongs to no item
			assert_eq!(menu.item_at(menu.w / 2.0, menu.row_top(1) + 1.0), None);
			// and a click just past the last row is off the menu entirely
			assert!(!menu.hit(menu.w / 2.0, menu.height() + 1.0));
		}
	}

	// A WM hotkey grab (Ctrl+Alt+Arrow) brackets its chord with a focus change,
	// and winit replays every held key as a synthetic press on the way back in -
	// before it re-reads the modifiers. Taking that replay as typing is what put
	// a bare arrow into the shell, so only a real press may count.
	// Test ID: EmpzjSS
	#[test]
	fn a_replayed_key_is_not_typing() {
		assert!(key_is_typed(ElementState::Pressed, false));
		assert!(!key_is_typed(ElementState::Pressed, true));
		assert!(!key_is_typed(ElementState::Released, false));
		assert!(!key_is_typed(ElementState::Released, true));
	}

	// The copy chord skips the unfocused-key gate, so it must never match what
	// that gate is for: a key passed through a WM grab with the modifiers zeroed.
	// Test ID: EpyCuGe
	#[test]
	fn only_a_held_ctrl_shift_c_is_the_copy_chord() {
		use winit::keyboard::{Key, ModifiersState, NamedKey};
		let both = ModifiersState::CONTROL | ModifiersState::SHIFT;
		assert!(is_copy_chord(both, &Key::Character("c".into())));
		assert!(is_copy_chord(both, &Key::Character("C".into())));
		assert!(!is_copy_chord(
			ModifiersState::empty(),
			&Key::Character("c".into())
		));
		assert!(!is_copy_chord(
			ModifiersState::CONTROL,
			&Key::Character("c".into())
		));
		assert!(!is_copy_chord(both, &Key::Character("v".into())));
		assert!(!is_copy_chord(both, &Key::Named(NamedKey::ArrowUp)));
	}

	// The demo capture samples on a fixed clock, so a pinned rate that drifts is
	// worse than none: it would wander off the sampling grid instead of sitting
	// on it. Each frame's deadline therefore has to come from the LAST DEADLINE,
	// never from "now" - which is the whole of what this pins.
	// Test ID: EmptHsG
	#[test]
	fn a_pinned_frame_rate_does_not_drift() {
		let ivl = Duration::from_millis(20);
		let mut next = None;
		let start = Instant::now();
		pace_frame(&mut next, ivl);
		let first = next.unwrap();
		// a frame that takes most of its budget must not push the next one out
		for step in 1..=10u32 {
			pace_frame(&mut next, ivl);
			assert_eq!(next.unwrap(), first + ivl * step, "drifted at frame {step}");
		}
		assert!(next.unwrap() >= start + ivl * 11);

		// falling behind resyncs to now rather than firing a catch-up burst
		let behind = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
		let mut late = Some(behind);
		pace_frame(&mut late, ivl);
		assert!(
			late.unwrap() > Instant::now(),
			"a stale deadline must be dropped"
		);
	}

	struct Wakes {
		cursor: Option<Instant>,
		map: Option<Instant>,
	}

	impl PaneWakes for Wakes {
		fn cursor_wake_at(&self) -> Option<Instant> {
			self.cursor
		}

		fn map_wake_at(&self) -> Option<Instant> {
			self.map
		}
	}

	// A cursor that parks in a frame sets its resume time in that frame. The
	// loop read the cursor's wake only before drawing, so the resume waited for
	// some other event. Every kind of pane wake is read after the frame now.
	// Test ID: ErJF0fr
	#[test]
	fn a_wake_set_while_drawing_is_kept() {
		let now = Instant::now();
		let soon = now + Duration::from_millis(300);
		let later = now + Duration::from_secs(2);
		let parked = [Wakes {
			cursor: Some(soon),
			map: None,
		}];
		assert_eq!(
			pane_wake(&parked, now),
			Some(soon),
			"a parked cursor's wake"
		);
		let mixed = [
			Wakes {
				cursor: Some(later),
				map: None,
			},
			Wakes {
				cursor: None,
				map: Some(soon),
			},
			Wakes {
				cursor: Some(soon + Duration::from_millis(1)),
				map: Some(later),
			},
		];
		assert_eq!(
			pane_wake(&mixed, now),
			Some(soon),
			"the earliest, any pane or kind"
		);
		// due ones belong to the pass before the frame, or an idle window spins
		let due = [Wakes {
			cursor: Some(now),
			map: now.checked_sub(Duration::from_millis(1)),
		}];
		assert_eq!(pane_wake(&due, now), None);
	}

	// The style guide asks every View toggle to name the thing and be checked
	// while that thing is on, so a reader can take the whole column one way. A
	// "Hide ..." caption checked when the thing is GONE reads backwards next to
	// its neighbours, which is what this stops coming back.
	// Test ID: EolQiSW
	#[test]
	fn every_view_toggle_is_checked_while_its_subject_is_on() {
		let all_on = ViewState {
			read_only: true,
			fullscreen: true,
			window_frame: true,
			menu_bar: true,
			tab_strip: true,
			minimap: true,
			bare: true,
			remote: true,
			next_wallpaper: true,
		};
		for entry in view_menu_items(all_on) {
			if let Entry::Item {
				label,
				check: Some(on),
				..
			} = &entry
			{
				assert!(
					on,
					"{label} is a toggle that draws unchecked with everything on"
				);
				assert!(
					!label.starts_with("Hide "),
					"{label} names the absence of a thing rather than the thing"
				);
			}
		}
		// and each of these has a row at all, checked exactly while it is on
		let all_off = ViewState {
			read_only: false,
			fullscreen: false,
			window_frame: false,
			menu_bar: false,
			tab_strip: false,
			minimap: false,
			bare: false,
			remote: false,
			next_wallpaper: false,
		};
		let rows: [(&str, fn(&MenuAction) -> bool); 4] = [
			("Menu bar", |a| matches!(a, MenuAction::ToggleMenuBar)),
			("Window frame", |a| matches!(a, MenuAction::ToggleFrame)),
			("Fullscreen", |a| matches!(a, MenuAction::ToggleFullscreen)),
			("Read-only", |a| matches!(a, MenuAction::ToggleReadOnly)),
		];
		for (state, on) in [(all_on, true), (all_off, false)] {
			let items = view_menu_items(state);
			for (name, want) in rows {
				assert_eq!(
					check_of(&items, want),
					Some(Some(on)),
					"View's {name} row, everything {}",
					if on { "on" } else { "off" }
				);
			}
		}
	}

	// Test ID: EolQiSX
	#[test]
	fn the_view_menu_spends_no_accelerator_twice() {
		let off = ViewState {
			read_only: false,
			fullscreen: false,
			window_frame: false,
			menu_bar: false,
			tab_strip: false,
			minimap: false,
			bare: false,
			remote: false,
			next_wallpaper: true,
		};
		assert_eq!(accel_clash(&view_menu_items(off)), None);
	}

	// Test ID: EqpdApU
	#[test]
	fn next_wallpaper_shows_only_with_somewhere_to_go() {
		let state = |next_wallpaper| ViewState {
			read_only: false,
			fullscreen: false,
			window_frame: false,
			menu_bar: false,
			tab_strip: false,
			minimap: false,
			bare: false,
			remote: false,
			next_wallpaper,
		};
		let has_row = |items: Vec<Entry>| {
			items.iter().any(|entry| {
				matches!(
					entry,
					Entry::Item {
						action: MenuAction::NextWallpaper,
						..
					}
				)
			})
		};
		assert!(has_row(view_menu_items(state(true))));
		assert!(!has_row(view_menu_items(state(false))));

		assert!(rotation_live(false, 2, true));
		assert!(
			!rotation_live(true, 2, true),
			"a command-line wallpaper holds"
		);
		assert!(!rotation_live(false, 1, true), "one image has no next");
		assert!(
			!rotation_live(false, 5, false),
			"no folder, nothing to rotate"
		);
	}

	// Some(check) for the first top-level item doing what `want` picks.
	fn check_of(items: &[Entry], want: fn(&MenuAction) -> bool) -> Option<Option<bool>> {
		items.iter().find_map(|entry| match entry {
			Entry::Item { action, check, .. } if want(action) => Some(*check),
			_ => None,
		})
	}

	fn position_of(items: &[Entry], want: fn(&MenuAction) -> bool) -> Option<usize> {
		items
			.iter()
			.position(|entry| matches!(entry, Entry::Item { action, .. } if want(action)))
	}

	// Every action a menu can reach, submenus included.
	fn actions_in(items: &[Entry]) -> Vec<MenuAction> {
		let mut out = Vec::new();
		for entry in items {
			match entry {
				Entry::Item { action, .. } => out.push(*action),
				Entry::Sub { items, .. } => out.extend(actions_in(items)),
				Entry::Sep => {}
			}
		}
		out
	}

	fn shell(title: &str, active: bool) -> ShellEntry {
		ShellEntry {
			slug: title.to_lowercase(),
			title: title.into(),
			command: title.to_lowercase(),
			active,
			comment: String::new(),
			last_seen: String::new(),
		}
	}

	// A disabled entry sits between the two active ones, so an index that
	// counted only the offered rows would show.
	fn three_shells() -> Vec<ShellEntry> {
		vec![
			shell("PowerShell 7", true),
			shell("fish", false),
			shell("Nushell", true),
		]
	}

	// Every menu with as many rows as it can have: each toggle on, a link under
	// the pointer, a wallpaper to move on to, and shells to offer.
	fn every_menu(shells: &[ShellEntry]) -> Vec<(&'static str, Vec<Entry>)> {
		let view = ViewState {
			read_only: true,
			fullscreen: true,
			window_frame: true,
			menu_bar: true,
			tab_strip: true,
			minimap: true,
			bare: true,
			remote: true,
			next_wallpaper: true,
		};
		let ctx = CtxState {
			link: true,
			read_only: true,
			copy_select: true,
			copy_output: true,
			menu_bar: true,
			next_wallpaper: true,
		};
		vec![
			("File", file_menu_items()),
			("Edit", edit_menu_items(true, true)),
			("View", view_menu_items(view)),
			("Tabs", tabs_menu_items(shells)),
			("Panes", panes_menu_items(shells)),
			("Help", help_menu_items()),
			("right-click", context_menu_items(ctx, shells)),
		]
	}

	// The shortcut in "Label (Ctrl+Shift+C)", if the row shows one.
	fn shortcut_of(label: &str) -> Option<&str> {
		let open = label.rfind(" (")?;
		label[open + 2..].strip_suffix(')')
	}

	// Build time only checks a menu's letters in a debug build, and only when
	// that menu is opened. This holds every one of them to it.
	// Test ID: Er2UvPk
	#[test]
	fn no_menu_spends_an_accelerator_twice() {
		for shells in [three_shells(), Vec::new()] {
			for (name, items) in every_menu(&shells) {
				assert_eq!(accel_clash(&items), None, "{name}");
			}
		}
	}

	// Keys are spelled as words, so "Ctrl+=" or "Ctrl++" cannot come back.
	// Ctrl+, keeps its comma: the style guide spells it that way too.
	// Test ID: Er2UvPl
	#[test]
	fn every_shortcut_in_a_menu_is_spelled_one_way() {
		for (name, items) in every_menu(&three_shells()) {
			for label in items.iter().filter_map(entry_label) {
				let Some(keys) = shortcut_of(label) else {
					continue;
				};
				assert!(!keys.contains(' '), "{name}: {label}");
				for key in keys.split('+') {
					assert!(
						key == "," || (!key.is_empty() && key.chars().all(char::is_alphanumeric)),
						"{name}: {label} spells a key as {key:?}"
					);
				}
			}
		}
		let view = &every_menu(&[])[2].1;
		let label_of = |want| {
			position_of(view, want)
				.and_then(|i| entry_label(&view[i]))
				.unwrap_or_default()
		};
		assert_eq!(
			shortcut_of(label_of(|a| matches!(a, MenuAction::FontBigger))),
			Some("Ctrl+Plus")
		);
		assert_eq!(
			shortcut_of(label_of(|a| matches!(a, MenuAction::FontSmaller))),
			Some("Ctrl+Minus")
		);
	}

	// A row that opens a prompt or a dialog ends in the one ellipsis character,
	// never three dots.
	// Test ID: Er2UvPm
	#[test]
	fn a_row_that_asks_for_more_ends_in_a_real_ellipsis() {
		use crate::ui_spec::{Key, Kind};
		let spec = crate::ui_spec::ui()
			.specs
			.iter()
			.find(|spec| spec.key == Key::ThemeActions)
			.expect("the theme buttons row");
		let Kind::Buttons(labels) = spec.kind else {
			panic!("the theme actions are buttons");
		};
		for want in ["Save as\u{2026}", "Rename\u{2026}"] {
			assert!(labels.contains(&want), "{want} in {labels:?}");
		}
		for (name, items) in every_menu(&three_shells()) {
			for label in items.iter().filter_map(entry_label) {
				assert!(!label.contains("..."), "{name}: {label}");
			}
		}
	}

	// Sentence case: past the first letter, a capital is either the row's
	// accelerator ("Paste Selection") or part of the shortcut. Shell titles in a
	// flyout are names and keep theirs.
	// Test ID: Er2UvPn
	#[test]
	fn every_menu_row_is_in_sentence_case() {
		for (name, items) in every_menu(&three_shells()) {
			for entry in &items {
				let Some(label) = entry_label(entry) else {
					continue;
				};
				let accel = entry_accel(entry).map(|(_, pos)| pos);
				let words = label.rfind(" (").map_or(label, |open| &label[..open]);
				for (at, ch) in words.char_indices().skip(1) {
					assert!(
						!ch.is_uppercase() || accel == Some(at),
						"{name}: {label} capitalizes {ch}"
					);
				}
			}
		}
	}

	// Test ID: Er2UvPo
	#[test]
	fn the_right_click_menu_rules_off_the_tab_rows_from_the_pane_rows() {
		let ctx = CtxState {
			link: false,
			read_only: false,
			copy_select: false,
			copy_output: false,
			menu_bar: true,
			next_wallpaper: false,
		};
		for shells in [three_shells(), Vec::new()] {
			let items = context_menu_items(ctx, &shells);
			let last_tab = items
				.iter()
				.rposition(|entry| {
					actions_in(std::slice::from_ref(entry))
						.iter()
						.any(|a| matches!(a, MenuAction::NewTab | MenuAction::NewTabShell(_)))
				})
				.expect("a tab row");
			let first_pane = position_of(&items, |a| matches!(a, MenuAction::SplitVertical))
				.expect("a pane row");
			assert!(last_tab < first_pane);
			assert!(
				items[last_tab..first_pane]
					.iter()
					.any(|entry| matches!(entry, Entry::Sep)),
				"no rule between the tab rows and the pane rows"
			);
		}
	}

	// Test ID: Er2UvPp
	#[test]
	fn the_bar_reads_file_to_help_and_file_holds_no_tab_or_pane_action() {
		assert_eq!(MENU_BAR, ["File", "Edit", "View", "Tabs", "Panes", "Help"]);
		for action in actions_in(&file_menu_items()) {
			assert!(!matches!(
				action,
				MenuAction::NewTab
					| MenuAction::NewTabShell(_)
					| MenuAction::CloseTab
					| MenuAction::SplitVertical
					| MenuAction::SplitHorizontal
					| MenuAction::SplitShell(..)
					| MenuAction::Close
			));
		}
	}

	// Test ID: Er2UvPq
	#[test]
	fn the_right_click_menu_carries_the_pane_actions_and_their_checkmarks() {
		for on in [false, true] {
			let items = context_menu_items(
				CtxState {
					link: false,
					read_only: on,
					copy_select: false,
					copy_output: false,
					menu_bar: on,
					next_wallpaper: false,
				},
				&[],
			);
			let plain: [(&str, fn(&MenuAction) -> bool); 8] = [
				("Copy", |a| matches!(a, MenuAction::Copy)),
				("Paste", |a| matches!(a, MenuAction::Paste)),
				("Paste selection", |a| {
					matches!(a, MenuAction::PasteSelection)
				}),
				("New tab", |a| matches!(a, MenuAction::NewTab)),
				("Split vertical", |a| matches!(a, MenuAction::SplitVertical)),
				("Split horizontal", |a| {
					matches!(a, MenuAction::SplitHorizontal)
				}),
				("Reload config", |a| matches!(a, MenuAction::ReloadConfig)),
				("Settings", |a| matches!(a, MenuAction::Settings)),
			];
			for (name, want) in plain {
				assert_eq!(check_of(&items, want), Some(None), "{name}");
			}
			assert_eq!(
				check_of(&items, |a| matches!(a, MenuAction::ToggleReadOnly)),
				Some(Some(on))
			);
			assert_eq!(
				check_of(&items, |a| matches!(a, MenuAction::ToggleMenuBar)),
				Some(Some(on))
			);
			assert_eq!(accel_clash(&items), None);
		}
	}

	// Test ID: Er2UvPr
	#[test]
	fn a_new_tab_shell_row_lists_the_active_shells_or_is_not_there() {
		let ctx = CtxState {
			link: false,
			read_only: false,
			copy_select: false,
			copy_output: false,
			menu_bar: true,
			next_wallpaper: false,
		};
		let shells = three_shells();
		for (name, items) in [
			("Tabs", tabs_menu_items(&shells)),
			("right-click", context_menu_items(ctx, &shells)),
		] {
			let below = position_of(&items, |a| matches!(a, MenuAction::NewTab)).unwrap() + 1;
			let Entry::Sub { label, items, .. } = &items[below] else {
				panic!("{name}: no shell row under New tab");
			};
			assert_eq!(label, "New tab with shell");
			let offered: Vec<_> = items.iter().filter_map(entry_label).collect();
			assert_eq!(offered, ["PowerShell 7", "Nushell"], "{name}");
			let picks = actions_in(items);
			assert!(matches!(
				picks[..],
				[MenuAction::NewTabShell(0), MenuAction::NewTabShell(2)]
			));
		}
		for shells in [Vec::new(), vec![shell("fish", false)]] {
			for items in [tabs_menu_items(&shells), context_menu_items(ctx, &shells)] {
				assert!(!items.iter().any(|entry| matches!(entry, Entry::Sub { .. })));
			}
		}
	}

	// Test ID: Er2UvPs
	#[test]
	fn both_split_shell_rows_offer_the_same_shells_by_their_place_in_the_list() {
		let shells = three_shells();
		let rows = split_shells(&shells);
		assert_eq!(rows.len(), 2);
		for (row, (label, dir)) in rows.iter().zip([
			("Split vertical with shell", Dir::Vertical),
			("Split horizontal with shell", Dir::Horizontal),
		]) {
			let Entry::Sub {
				label: shown,
				items,
				..
			} = row
			else {
				panic!("{label} is not a submenu");
			};
			assert_eq!(shown, label);
			let offered: Vec<_> = items.iter().filter_map(entry_label).collect();
			assert_eq!(offered, ["PowerShell 7", "Nushell"], "{label}");
			let picks = actions_in(items);
			assert!(
				matches!(
					picks[..],
					[MenuAction::SplitShell(a, 0), MenuAction::SplitShell(b, 2)] if a == dir && b == dir
				),
				"{label}"
			);
		}
		// both rows sit under the two plain splits on the Panes menu
		let panes = panes_menu_items(&shells);
		assert!(matches!(
			panes[2..4],
			[Entry::Sub { .. }, Entry::Sub { .. }]
		));
		assert!(split_shells(&[shell("fish", false)]).is_empty());
		assert!(split_shells(&[]).is_empty());
	}

	// Test ID: Er2UvPt
	#[test]
	fn alt_and_a_title_letter_opens_that_menu() {
		for (i, ch) in "fevtph".chars().enumerate() {
			assert_eq!(bar_menu_for(ch), Some(i), "{ch}");
			assert_eq!(bar_menu_for(ch.to_ascii_uppercase()), Some(i), "{ch}");
		}
		assert_eq!(bar_menu_for('x'), None);
		let mut firsts: Vec<char> = MENU_BAR.iter().filter_map(|t| t.chars().next()).collect();
		firsts.sort_unstable();
		firsts.dedup();
		assert_eq!(firsts.len(), MENU_BAR.len(), "two titles share a letter");
	}

	// Test ID: Er2UvPu
	#[test]
	fn the_bar_titles_are_underlined_only_while_alt_is_held_and_nothing_is_open() {
		assert!(bar_title_underlines(false, None, &MENU_BAR).is_empty());
		assert!(bar_title_underlines(true, Some(2), &MENU_BAR).is_empty());
		assert!(bar_title_underlines(false, Some(2), &MENU_BAR).is_empty());
		let marks = bar_title_underlines(true, None, &MENU_BAR);
		assert_eq!(marks.len(), MENU_BAR.len());
		for (i, ch) in marks {
			assert!(MENU_BAR[i].starts_with(ch));
			// the letter drawn is the letter that opens it
			assert_eq!(bar_menu_for(ch), Some(i));
		}
	}

	// Test ID: Er2UvPv
	#[test]
	fn a_split_with_no_direction_goes_along_the_longer_side() {
		use crate::cli::Dir4;
		let rect = |w, h| {
			Some(Rect {
				x: 0.0,
				y: 0.0,
				w,
				h,
			})
		};
		assert_eq!(default_dir_for(rect(400.0, 900.0)), Dir4::Down);
		assert_eq!(default_dir_for(rect(900.0, 400.0)), Dir4::Right);
		assert_eq!(default_dir_for(rect(500.0, 500.0)), Dir4::Right);
		assert_eq!(default_dir_for(None), Dir4::Right);
	}

	// --new-pane=a --down --new-pane --splits=a stacks the second pane below too.
	// Test ID: ErCAz8d
	#[test]
	fn a_pane_splits_the_way_the_pane_it_splits_was_split() {
		use crate::cli::Dir4;
		assert_eq!(pane_split_dir(None, Some(Dir4::Down)), Some(Dir4::Down));
		assert_eq!(
			pane_split_dir(Some(Dir4::Left), Some(Dir4::Down)),
			Some(Dir4::Left)
		);
		assert_eq!(pane_split_dir(None, None), None, "left to the longer side");
	}

	// Test ID: Er2UvPw
	#[test]
	fn the_wallpaper_pushes_a_wait_back_but_never_starts_one_again() {
		let now = Instant::now();
		let armed = Some(now);
		let due = now + Duration::from_secs(3);
		let cap = now + Duration::from_secs(20);
		assert_eq!(push_back(armed, Some(cap), due), Some(due));
		assert_eq!(push_back(armed, None, due), Some(due));
		let late = now + Duration::from_secs(30);
		assert_eq!(
			push_back(armed, Some(cap), late),
			Some(cap),
			"past the backstop"
		);
		assert_eq!(
			push_back(None, Some(cap), due),
			None,
			"the scan already ran"
		);
	}

	// Test ID: Er2UvPx
	#[test]
	fn a_pane_takes_the_most_specific_shell_it_was_given() {
		let argv = |s: &str| vec![s.to_string()];
		let (pane, source, tab, window) =
			(argv("pane"), argv("source"), argv("tab"), argv("window"));
		let default = || Some(argv("default"));
		let pick = |p, s, t, w| pane_shell(p, s, t, w, default).unwrap()[0].clone();
		assert_eq!(
			pick(Some(&pane), Some(&source), Some(&tab), Some(&window)),
			"pane"
		);
		assert_eq!(
			pick(None, Some(&source), Some(&tab), Some(&window)),
			"source"
		);
		assert_eq!(pick(None, None, Some(&tab), Some(&window)), "tab");
		assert_eq!(pick(None, None, None, Some(&window)), "window");
		assert_eq!(pick(None, None, None, None), "default");
	}

	// The default shell is the first entry switched on, not the first entry.
	// Test ID: Er2VLed
	#[test]
	fn the_default_shell_is_the_first_active_entry() {
		let _store = config::test_store_lock();
		let saved = config::settings();
		let with = |shells: Vec<ShellEntry>| {
			config::update(config::Settings {
				shells,
				..(*saved).clone()
			});
			config::default_shell_argv()
		};
		let mut listed = vec![
			shell("fish", false),
			shell("Bash", true),
			shell("zsh", true),
		];
		listed[1].command = "bash -l".into();
		assert_eq!(
			with(listed),
			Some(vec!["bash".to_string(), "-l".to_string()])
		);
		assert_eq!(with(vec![shell("fish", false)]), None);
		config::update((*saved).clone());
	}

	// The titles and the copy labels beside them are one row of text, so they
	// sit on one baseline. The labels once centered their whole ink box instead
	// and rode half a descent higher.
	// Test ID: Er2VLee
	#[test]
	fn the_bar_titles_and_the_copy_labels_share_a_baseline() {
		let text = TextCtx::new_cpu(1.0);
		let menu_h = text.ui_line_h + text.dip(MENU_BAR_VPAD);
		let layout = [(0.0, 40.0), (40.0, 40.0)];
		let cb = copybox_place(
			&CopyMetrics {
				right: 900.0,
				label_w: [56.0, 34.0, 40.0],
				box_sz: 10.0,
				box_y: 4.0,
				box_gap: 6.0,
				pair_gap: 14.0,
				lead_gap: 10.0,
			},
			[true; 3],
		);
		let top = |i| menubar_text_slot(&text, menu_h, &layout, Some(&cb), i).map(|slot| slot.3);
		let title = top(0).expect("a title");
		for label in 0..3 {
			assert_eq!(top(layout.len() + label), Some(title), "copy label {label}");
		}
		// a label with no room is not drawn at all
		let narrow = CopyBoxes {
			shown: [false, true, true],
			..cb
		};
		assert_eq!(
			menubar_text_slot(&text, menu_h, &layout, Some(&narrow), layout.len()),
			None
		);
	}

	// Test ID: EkrYObg
	#[test]
	fn accel_prefers_exact_case_then_falls_back() {
		// 'S' must pick "Selection", not the 's' in "Paste"
		assert_eq!(accel_at("Paste Selection", 'S'), Some(6));
		// no capital 'O' -> case-insensitive fallback finds "only"
		assert_eq!(accel_at("Read-only", 'O'), Some(5));
		assert_eq!(accel_at("Quit", 'x'), None);
	}

	// Ctrl+Shift+N used to start the program with nothing but a working
	// directory, so a window opened with --config got the default file, and a
	// pane sitting in home looked like a desktop launch and took the setting.
	// Test ID: Eq4Yrbc
	#[test]
	fn a_new_window_keeps_the_settings_file_and_the_panes_directory() {
		let exe = std::path::Path::new("/opt/silkterm/silkterm");
		let home = config::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
		let args = |cmd: &std::process::Command| -> Vec<String> {
			cmd.get_args()
				.map(|a| a.to_string_lossy().into_owned())
				.collect()
		};
		let handed_down = |cmd: &std::process::Command| {
			cmd.get_envs()
				.any(|(name, value)| name == config::ENV_DIR_HANDED_DOWN && value.is_some())
		};

		// built from home so it is absolute on every platform; "/x/alt.shcl" is
		// not on Windows, where it came back as C:\x\alt.shcl
		let alt = home.join("alt.shcl");
		let cmd = new_window_command(exe, Some(&home), Some(&alt));
		assert_eq!(args(&cmd), ["--config", &*alt.to_string_lossy()]);
		assert_eq!(cmd.get_current_dir(), Some(home.as_path()));
		assert!(
			handed_down(&cmd),
			"home has to be kept, not read as a launcher's"
		);

		// a relative --config is made absolute, since the child starts elsewhere
		let cmd = new_window_command(exe, Some(&home), Some(std::path::Path::new("alt.shcl")));
		assert!(std::path::Path::new(&args(&cmd)[1]).is_absolute());

		// no --config at launch, none passed on, so the default file is used
		let cmd = new_window_command(exe, Some(&home), None);
		assert!(args(&cmd).is_empty());

		// no known directory: nothing is handed down, and the setting decides
		let cmd = new_window_command(exe, None, None);
		assert_eq!(cmd.get_current_dir(), None);
		assert!(!handed_down(&cmd));
	}

	// Reload config dropped the font and colors given on the command line, while
	// a value the command line did not name has to come from the file.
	// Test ID: Eq4Yrbd
	#[test]
	fn a_reload_keeps_the_launch_options_over_the_file() {
		let launch = crate::cli::Style {
			font_size: Some(21.0),
			bg_color: Some([0xff, 0, 0]),
			wallpaper_img: Some(Some("/launch.png".into())),
			..crate::cli::Style::default()
		};
		// the socket changed the wallpaper since launch
		let mut live = config::Settings::default();
		config::name_wallpaper(&mut live, Some("/socket.png".into()));
		let from_disk = config::Settings {
			font_size: 9.0,
			bg: [0, 0, 0],
			fg: [1, 2, 3],
			wallpaper_enabled: false,
			..config::Settings::default()
		};
		let reloaded = settings_after_reload(&live, from_disk, &launch, true);
		assert_eq!(reloaded.font_size, 21.0);
		assert_eq!(reloaded.bg, [0xff, 0, 0]);
		assert_eq!(
			reloaded.fg,
			[1, 2, 3],
			"not on the command line, so the file's"
		);
		assert_eq!(
			reloaded.wallpaper.as_deref(),
			Some(std::path::Path::new("/socket.png"))
		);
		assert!(
			reloaded.wallpaper_enabled,
			"a named wallpaper stays switched on"
		);
	}

	// Test ID: Er2UiYT
	#[test]
	fn changing_tab_wraps_at_both_ends() {
		use super::tab_step;
		assert_eq!(tab_step(0, 3, true), 1);
		assert_eq!(tab_step(2, 3, true), 0);
		assert_eq!(tab_step(0, 3, false), 2);
		assert_eq!(tab_step(1, 3, false), 0);
		assert_eq!(tab_step(0, 1, true), 0);
		assert_eq!(tab_step(0, 1, false), 0);
	}

	// Test ID: Er2UiYU
	#[test]
	fn a_moved_tab_trades_places_with_its_neighbour_and_stays_active() {
		use super::move_tab;
		let mut tabs = ['a', 'b', 'c'];
		assert_eq!(move_tab(&mut tabs, 1, true), 2);
		assert_eq!(tabs, ['a', 'c', 'b']);
		assert_eq!(move_tab(&mut tabs, 2, true), 0, "past the end");
		assert_eq!(tabs, ['b', 'c', 'a']);
		assert_eq!(move_tab(&mut tabs, 0, false), 2, "past the start");
		assert_eq!(tabs, ['a', 'c', 'b']);
		let mut one = ['a'];
		assert_eq!(move_tab(&mut one, 0, true), 0);
	}

	// Test ID: Er2UiYV
	#[test]
	fn only_a_return_to_this_console_is_signalled() {
		let mut watch = super::VtWatch::new("tty7".into());
		let seen: Vec<bool> = ["tty7", "tty1", "tty7", "tty7", "tty2", "tty3", "tty7"]
			.into_iter()
			.map(|vt| watch.step(vt.into()))
			.collect();
		assert_eq!(seen, [false, false, true, false, false, false, true]);
	}

	// A frame drawn while hidden banks the backlog into the ease, and the reveal
	// then plays it back as if it had just arrived.
	// Test ID: Er2UiYW
	#[test]
	fn a_hidden_window_draws_nothing_and_its_return_is_one_cut() {
		use super::{Frame, freeze_frame};
		assert_eq!(freeze_frame(false, true), Frame::Skip);
		assert_eq!(freeze_frame(true, true), Frame::Skip);
		assert_eq!(freeze_frame(true, false), Frame::CatchUp);
		assert_eq!(freeze_frame(false, false), Frame::Draw);
	}

	// Test ID: Er2UiYX
	#[test]
	fn leaving_a_bare_window_puts_back_only_what_was_on() {
		use super::bare_chrome;
		// going bare saves what was on and hides both
		assert_eq!(
			bare_chrome(true, (false, false), (true, true)),
			((true, true), (false, false))
		);
		assert_eq!(
			bare_chrome(false, (true, true), (false, false)).1,
			(true, true)
		);
		// the frame was already off, so it stays off
		assert_eq!(
			bare_chrome(false, (false, true), (false, false)).1,
			(false, true)
		);
		// a menu bar switched on while bare stays on
		assert_eq!(
			bare_chrome(false, (true, false), (false, true)).1,
			(true, true)
		);
	}
}
