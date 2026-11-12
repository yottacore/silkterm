// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The About and Settings windows and the save notice, as the main window
//! drives them.

use super::{App, EnvFlag, RAISE_REASSERT_IVL, RAISE_REASSERTS, env_flag, key_is_typed, open_url};
use crate::config;
use crate::input;
use std::time::Instant;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

impl App {
	/// Events for the pop-out dialog window (its own surface/input).
	pub(super) fn handle_dialog_event(&mut self, event: WindowEvent) {
		use crate::dialog::DialogAction as DA;
		if env_flag(EnvFlag::DlgDbg) {
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
			WindowEvent::Focused(false) => {
				if self.dialog.as_mut().is_some_and(|d| d.set_focus(false)) {
					self.dialog_dirty = true;
				}
			}
			WindowEvent::Focused(true) => {
				if let Some(d) = &mut self.dialog {
					d.set_focus(true);
				}
				// keep the terminal right under the dialog when it's activated, so nothing gets
				// between the 2 (Compiz doesn't do this). The WM's own raise and animation can
				// restack it for a while, so retries are armed too. They aren't disarmed on
				// focus-out, since the restack never raises the dialog, and Compiz's animation
				// drops and restores focus for a moment.
				if let Some(d) = &self.dialog {
					d.raise_parent();
				}
				self.raise_reassert = RAISE_REASSERTS;
				self.raise_next = Instant::now() + RAISE_REASSERT_IVL;
			}
			// the window's own size, since a late creation-size event is stale
			WindowEvent::Resized(_) => {
				if let Some(d) = &mut self.dialog {
					let now = d.window.inner_size();
					d.resize(now.width, now.height);
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
				// a move that changes nothing drawn (no drag, no lit item, no tip
				// coming or going) gets no frame
				if let Some(d) = &mut self.dialog {
					if d.set_cursor(position.x as f32, position.y as f32) {
						self.dialog_dirty = true;
					}
				}
			}
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button,
				..
			} => {
				if let Some(d) = &mut self.dialog {
					match input::acting_button(button, d.mods(), cfg!(target_os = "macos")) {
						MouseButton::Left => {
							// clipboard for the field context-menu commands
							let clip = self.state.as_mut().map(|s| &mut s.clipboard);
							act = d.mouse_down(clip);
							self.dialog_dirty = true;
						}
						MouseButton::Right => {
							// gray the menu's Paste when the clipboard is empty
							let paste_ok = self.state.as_mut().is_some_and(|s| {
								s.clipboard.get_clipboard().is_some_and(|t| !t.is_empty())
							});
							d.mouse_right(paste_ok);
							self.dialog_dirty = true;
						}
						_ => {}
					}
				}
			}
			WindowEvent::MouseInput {
				state: ElementState::Released,
				button: MouseButton::Left,
				..
			} => {
				if let Some(d) = &mut self.dialog {
					act = d.mouse_up();
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
					if d.capture_key(&key_event.logical_key) {
						self.dialog_dirty = true;
						return;
					}
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
					d.set_mods(mods.state());
					d.set_keys(input::edit_keys(mods.state(), cfg!(target_os = "macos")));
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

	/// Windows gives an owned popup no placement, so it opens at the screen origin.
	/// macOS centers a new window at its first size and keeps the bottom edge as it
	/// grows, so a tall one ends up off the bottom. On both, the dialog is centered
	/// over the terminal and kept on the work area. Linux WMs place transients
	/// themselves.
	#[cfg(any(target_os = "windows", target_os = "macos"))]
	pub(super) fn center_dialog(&self) {
		let (Some(state), Some(dialog)) = (self.state.as_ref(), self.dialog.as_ref()) else {
			return;
		};
		let Ok(pos) = state.window.outer_position() else {
			return;
		};
		let win = state.window.outer_size();
		let dlg = dialog.window.outer_size();
		// the terminal's monitor, not the dialog's: the dialog hasn't been
		// placed yet, so its own is wherever the origin is
		let work = {
			use winit::raw_window_handle::HasWindowHandle;
			state
				.window
				.window_handle()
				.ok()
				.map(|h| h.as_raw())
				.and_then(crate::dialog::work_area_of)
		};
		let (x, y) = crate::dialog::dialog_origin(
			(pos.x, pos.y),
			(win.width, win.height),
			(dlg.width, dlg.height),
			work,
		);
		dialog
			.window
			.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
	}
	/// self kept for call-site parity with the version above
	#[cfg(not(any(target_os = "windows", target_os = "macos")))]
	#[allow(clippy::unused_self)]
	pub(super) fn center_dialog(&self) {}

	/// Windows: the dialog is created hidden (see `dialog::make`), so after centering,
	/// one frame is drawn at the final position before it's shown. That way it doesn't
	/// flash at the origin or jump. Elsewhere `new_about` and `new_settings` map it.
	#[cfg(target_os = "windows")]
	pub(super) fn reveal_dialog(&mut self) {
		if let Some(d) = self.dialog.as_mut() {
			d.render();
			d.window.set_visible(true);
		}
	}
	/// self kept for call-site parity with the Windows version above
	#[cfg(not(target_os = "windows"))]
	#[allow(clippy::unused_self)]
	pub(super) fn reveal_dialog(&self) {}

	// Drop the dialog window, remembering a Settings view on the way out so a
	// reopen within SETTINGS_RESUME picks up where it left off. Every close goes
	// through here: Cancel, OK, Esc, and the window's own close button.
	fn close_dialog(&mut self) {
		if let Some(view) = self
			.dialog
			.as_ref()
			.and_then(crate::dialog::DialogWin::settings_view)
		{
			self.settings_view = Some((Instant::now(), view));
			self.settings_size = self
				.dialog
				.as_ref()
				.and_then(crate::dialog::DialogWin::settings_size);
		}
		self.dialog = None;
	}

	/// Events for the notice window. A notice has one button, so everything
	/// that means OK or close closes it. The tmux question has 3, and a box.
	pub(super) fn handle_notice_event(&mut self, event: WindowEvent) {
		use crate::dialog::DialogAction as DA;
		let Some(n) = self.notice.as_mut() else {
			return;
		};
		let mut act: Option<DA> = None;
		match event {
			WindowEvent::CloseRequested => act = Some(DA::Close),
			WindowEvent::Resized(_) => {
				let now = n.window.inner_size();
				n.resize(now.width, now.height);
				self.notice_dirty = true;
			}
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				n.set_scale(scale_factor);
				self.notice_dirty = true;
			}
			WindowEvent::RedrawRequested => n.render(),
			WindowEvent::Focused(focused) => {
				if n.set_focus(focused) {
					self.notice_dirty = true;
				}
			}
			WindowEvent::CursorMoved { position, .. } => {
				if n.set_cursor(position.x as f32, position.y as f32) {
					self.notice_dirty = true;
				}
			}
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Left,
				..
			} => {
				act = n.mouse_down(None);
				self.notice_dirty = true;
			}
			WindowEvent::ModifiersChanged(mods) => n.set_mods(mods.state()),
			WindowEvent::KeyboardInput {
				event: key_event,
				is_synthetic,
				..
			} if key_is_typed(key_event.state, is_synthetic) => {
				act = match &input::name_typed(key_event).logical_key {
					Key::Named(NamedKey::Escape) => n.key_escape(),
					Key::Named(NamedKey::Enter) => n.key_enter(None),
					Key::Named(NamedKey::Space) => n.key_space(),
					Key::Named(NamedKey::Tab) => {
						n.key_tab();
						None
					}
					_ => None,
				};
				self.notice_dirty = true;
			}
			_ => {}
		}
		let Some(act) = act else {
			return;
		};
		self.notice = None;
		// a notice's OK, or the question answered or put away
		let Some(closing) = self.asking.take() else {
			return;
		};
		if let (DA::Tmux { end, remember }, Some(state)) = (act, self.state.as_mut()) {
			state.answer_close(closing, end, remember);
		}
	}

	/// While a notice is up, the windows under it take no input, and a click on
	/// one brings the notice forward: the same rule a dialog applies to the terminal.
	pub(super) fn notice_holds(&self, event: &WindowEvent) -> bool {
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

	/// Put a waiting notice up, once no other one is showing. In front of
	/// Settings when that is open, since an OK there is the usual way to meet it.
	pub(super) fn show_notice(&mut self, event_loop: &ActiveEventLoop) {
		#[cfg(target_os = "windows")]
		if NOTICE_UP.load(std::sync::atomic::Ordering::SeqCst) {
			return;
		}
		if self.notice.is_some() {
			return;
		}
		let (title, paras) = if let Some(loss) = self.loss_owed.take() {
			crate::dialog::conversion_notice(&loss)
		} else if let Some(refusal) = self.notice_owed.take() {
			crate::dialog::refusal_notice(&refusal)
		} else {
			return;
		};
		let parent = self
			.dialog
			.as_ref()
			.map(|d| &d.window)
			.or(self.state.as_ref().map(|s| &s.window))
			.and_then(|w| crate::dialog::Parent::of(w));
		#[cfg(target_os = "windows")]
		{
			let _ = event_loop;
			let owner = match parent.map(crate::dialog::Parent::handle) {
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
			// only the tmux question is answered this way, and it is never Settings
			DA::Tmux { .. } => {}
			DA::Apply => {
				self.apply_dialog_settings();
			}
			DA::ApplyAndClose => {
				// Only close on OK if the save worked or never can. If the file looked open
				// elsewhere, the change applied live but wasn't written, so the dialog stays up
				// to try again. The FYI went to stderr.
				if self.apply_dialog_settings() {
					self.close_dialog();
				}
			}
		}
	}

	// Pull the edited Settings from the dialog window and apply them live to the
	// main window. Returns true when the change was written, or when it can't be
	// until the file is fixed, which a notice says. False means the file looked open
	// elsewhere, so the change is live but not saved, and OK leaves the dialog open.
	fn apply_dialog_settings(&mut self) -> bool {
		let mut wrote = true;
		let mut refused = false;
		if let Some((orig, edited)) = self
			.dialog
			.as_ref()
			.and_then(crate::dialog::DialogWin::settings_values)
		{
			if let Some(state) = self.state.as_mut() {
				wrote = state.apply_settings_values(&orig, edited);
			}
			if let Some(refusal) = config::take_refusal() {
				refused = true;
				if notice_due(&mut self.told, &refusal.path, true) {
					self.notice_owed = Some(refusal);
				}
			}
			// Keys reverted to default get commented back out after persist wrote the diffs,
			// so the file returns to the template's default line. Skipped when the write was
			// deferred.
			if wrote {
				if let Some(reverted) = self
					.dialog
					.as_mut()
					.map(crate::dialog::DialogWin::take_reverted)
				{
					config::revert_keys(&reverted);
				}
			}
			// The applied values are the new baseline, so a later Apply diffs against the
			// live state. Without this, picking the open-time value again, such as Bg fit
			// back to Stretch, reads as no change. When another window saved while these
			// changes were open, the file has both, and the window and the dialog both take
			// it from there.
			if let Some(d) = self.dialog.as_mut() {
				let behind = wrote && d.take_behind();
				d.commit_baseline();
				if let Some(state) = self.state.as_mut().filter(|_| behind) {
					state.follow_peer();
					d.follow_file(state.settings_for_dialog());
				}
			}
			self.dialog_dirty = true;
		}
		wrote || refused
	}
}

/// Whether a refused save gets a notice. One the user asked for, an OK or Apply
/// in Settings, is answered every time, or the button would seem to do nothing.
/// The others (a resize, a menu switch, shells found at launch) are said once a
/// session for each file, or every resize would raise it again.
pub(super) fn notice_due(
	told: &mut Vec<std::path::PathBuf>,
	path: &std::path::Path,
	asked: bool,
) -> bool {
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

// What a launch says when no device could be made at all, as a headline and
// the detail under it. A Mac with no Metal device, such as a virtual machine
// without graphics, has nothing to fall back to: wgpu has no software adapter
// there (2026102209035801).
fn no_device_words(macos: bool, why: &str) -> (String, String) {
	if !macos {
		return (format!("no usable GPU/renderer: {why}"), String::new());
	}
	(
		format!(
			"{} needs a Metal graphics device, and none could be used.",
			config::APP_NAME
		),
		format!(
			"macOS has no software renderer to draw with instead, so a Mac without Metal, \
			such as a virtual machine with no graphics acceleration, can't run it.\n\n{why}"
		),
	)
}

/// The launch couldn't make a device on any adapter. A launch from Finder
/// shows no stderr, so macOS says it in an alert as well.
pub(super) fn no_device(e: &anyhow::Error) -> ! {
	let (headline, detail) = no_device_words(cfg!(target_os = "macos"), &format!("{e:#}"));
	eprintln!("{}: {headline}", config::APP_NAME);
	if !detail.is_empty() {
		eprintln!("{detail}");
	}
	#[cfg(target_os = "macos")]
	alert(&headline, &detail);
	std::process::exit(2);
}

// Modal, on the main thread, with nothing of the app's own drawn yet.
#[cfg(target_os = "macos")]
fn alert(headline: &str, detail: &str) {
	use objc2_app_kit::{NSAlert, NSAlertStyle};
	use objc2_foundation::NSString;
	let Some(mtm) = objc2::MainThreadMarker::new() else {
		return;
	};
	let alert = NSAlert::new(mtm);
	alert.setAlertStyle(NSAlertStyle::Critical);
	alert.setMessageText(&NSString::from_str(headline));
	alert.setInformativeText(&NSString::from_str(detail));
	alert.runModal();
}

#[cfg(test)]
mod tests {
	use super::notice_due;

	// A Mac with no Metal device ended the launch with a line on stderr only,
	// which a launch from Finder never shows.
	// Test ID: de9m1
	#[test]
	fn no_device_on_macos_says_why_and_that_nothing_can_stand_in() {
		let (headline, detail) = super::no_device_words(true, "No suitable graphics adapter found");
		assert!(headline.contains("Metal"), "{headline}");
		assert!(detail.contains("no software renderer"), "{detail}");
		assert!(
			detail.ends_with("No suitable graphics adapter found"),
			"{detail}"
		);
		let (headline, detail) = super::no_device_words(false, "boom");
		assert_eq!(
			headline, "no usable GPU/renderer: boom",
			"unchanged elsewhere"
		);
		assert!(detail.is_empty());
	}

	// A save nobody asked for can be refused at every resize, so it is said once
	// a session for each file. An OK in Settings that couldn't save is said
	// every time, or the button would seem to do nothing.
	// Test ID: dct5j
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
}
