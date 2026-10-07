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
							// gray the menu's Paste when the clipboard holds nothing
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

	/// Windows: an owned popup gets no automatic placement (it appears at the
	/// screen origin). macOS centers a new window on the screen at its first
	/// size, and growing it after keeps the bottom edge, so a tall one is pushed
	/// down from under the menu bar and off the bottom. Both center the dialog
	/// over the terminal and keep it on the work area. Linux WMs place
	/// transients themselves.
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
		// the terminal's monitor, not the dialog's: the dialog has not been
		// placed yet, so its own answer is for wherever the origin is
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

	/// Windows: the dialog is created hidden (see `dialog::make`), so after centering it
	/// draw one frame at the final position and then show it - no origin flash, no jump.
	/// Elsewhere the dialog is already mapped by `new_about` / `new_settings`.
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
	// through here - Cancel, OK, Esc and the window's own close button alike.
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

	/// Events for the notice window. It has one button, so everything that means
	/// OK or close closes it.
	pub(super) fn handle_notice_event(&mut self, event: WindowEvent) {
		let Some(n) = self.notice.as_mut() else {
			return;
		};
		let mut close = false;
		match event {
			WindowEvent::CloseRequested => close = true,
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
			WindowEvent::CursorMoved { position, .. } => {
				if n.set_cursor(position.x as f32, position.y as f32) {
					self.notice_dirty = true;
				}
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

	/// While a notice is up, the windows under it take no input, and a click on
	/// one brings the notice forward: the same rule a dialog holds the terminal to.
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

	/// Put an owed notice up, once nothing is saying one already. In front of
	/// Settings when that is open, since an OK there is the usual way to meet it.
	pub(super) fn show_notice(&mut self, event_loop: &ActiveEventLoop) {
		use winit::raw_window_handle::HasWindowHandle;
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
			.and_then(crate::dialog::DialogWin::settings_values)
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
					.map(crate::dialog::DialogWin::take_reverted)
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

#[cfg(test)]
mod tests {
	use super::notice_due;
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
}
