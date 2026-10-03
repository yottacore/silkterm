// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::time::{Duration, Instant};

use alacritty_terminal::term::TermMode;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

use crate::keys::Bindings;
use crate::pane::Toward;

// A mouse event to report to the PTY. Wheel notches ride buttons 64/65; `None`
// is the "no button" code (3) used for bare motion and the X10 release.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseBtn {
	None,
	Left,
	Middle,
	Right,
	WheelUp,
	WheelDown,
}

impl MouseBtn {
	// xterm button code, before the motion/modifier bits are added
	fn code(self) -> u8 {
		match self {
			MouseBtn::Left => 0,
			MouseBtn::Middle => 1,
			MouseBtn::Right => 2,
			MouseBtn::None => 3,
			MouseBtn::WheelUp => 64,
			MouseBtn::WheelDown => 65,
		}
	}
	fn is_wheel(self) -> bool {
		matches!(self, MouseBtn::WheelUp | MouseBtn::WheelDown)
	}
}

// True when the app has any mouse tracking turned on (DECSET 1000/1002/1003).
pub fn wants_mouse(mode: TermMode) -> bool {
	mode.intersects(TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
}

// winit button -> the reportable subset (None for Back/Forward/etc.)
pub fn mouse_btn_of(button: MouseButton) -> Option<MouseBtn> {
	match button {
		MouseButton::Left => Some(MouseBtn::Left),
		MouseButton::Middle => Some(MouseBtn::Middle),
		MouseButton::Right => Some(MouseBtn::Right),
		_ => None,
	}
}

// Whether a press goes to a mouse-tracking app instead of being handled here.
// Right-click is reserved for our own context menu (muffer pastes on it), an
// open menu takes the click to operate or dismiss it, and Shift is the
// local-action override.
pub fn press_is_reported(btn: MouseBtn, menu_open: bool, shift: bool) -> bool {
	btn != MouseBtn::Right && !menu_open && !shift
}

// A release is reported only for the button whose press was. Any other one
// would clear the held state, and the app would see a release it never saw
// pressed.
pub fn release_is_reported(held: Option<MouseBtn>, button: MouseButton) -> bool {
	held.is_some() && held == mouse_btn_of(button)
}

// A left press in the tab-bar band goes to the tab bar - unless a dropdown is
// open. One opens flush under the menu bar, so its top item overlaps the band,
// and "Tabs|New tab" would select a tab instead of firing.
pub fn tab_bar_takes_press(menu_open: bool, bar_shown: bool, y: f32, top: f32, h: f32) -> bool {
	!menu_open && bar_shown && y >= top && y < top + h
}

// A tab close armed by a press fires only if the release is still over that
// tab's close box. `close_x` is the left edge of the armed tab's box, None when
// the tab is gone. Dragging off before releasing cancels, like any button.
pub fn close_on_release(
	armed: usize,
	x: f32,
	in_bar: bool,
	close_x: Option<f32>,
	tab_at: Option<usize>,
) -> bool {
	in_bar && close_x.is_some_and(|left| x >= left) && tab_at == Some(armed)
}

// How soon, and how near, a click has to follow the last one to count with it.
pub const MULTI_CLICK: Duration = Duration::from_millis(400);

// A press's place in a run of clicks on one spot: 1, 2 or 3, and a fourth
// starts over. One too late, or more than a cell away, starts a new run.
pub fn click_count(
	last: Option<(Instant, f32, f32)>,
	count: u32,
	now: Instant,
	at: (f32, f32),
	cell: (f32, f32),
) -> u32 {
	let near = last.is_some_and(|(when, x, y)| {
		now.duration_since(when) < MULTI_CLICK
			&& (at.0 - x).abs() <= cell.0
			&& (at.1 - y).abs() <= cell.1
	});
	if near { (count % 3) + 1 } else { 1 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickSelect {
	Run,   // plain selection from the press
	Block, // a rectangle, the shortcut key held
	Word,  // a shape, a pair's contents, a bracket to its partner, else the word
	Line,  // the whole logical line, wrapped rows included
}

pub fn click_select(count: u32, shortcut: bool) -> ClickSelect {
	match count {
		2 => ClickSelect::Word,
		3 => ClickSelect::Line,
		_ if shortcut => ClickSelect::Block,
		_ => ClickSelect::Run,
	}
}

/// Whether the key the program's own shortcuts use is held: Command on macOS,
/// Ctrl elsewhere. It is also what opens a link on a click and makes a block
/// selection on a drag. On a Mac a Ctrl chord goes to the shell.
pub fn shortcut_held(mods: ModifiersState, mac: bool) -> bool {
	if mac {
		mods.super_key()
	} else {
		mods.control_key()
	}
}

/// The button a press acts as. On a Mac, Ctrl+click is the right-click, as in
/// every Mac app there. Elsewhere a press is the button pressed.
pub fn acting_button(button: MouseButton, mods: ModifiersState, mac: bool) -> MouseButton {
	if mac && button == MouseButton::Left && mods.control_key() {
		MouseButton::Right
	} else {
		button
	}
}

/// What the held keys mean to a text box in a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditKeys {
	// the accelerator key for a dialog's buttons; a Mac has none, and Option
	// types a character there
	pub alt: bool,
	pub shift: bool,
	// copy, cut, paste and select all, and the dialog's tab switching
	pub shortcut: bool,
	// the arrows and the erase keys go by words
	pub word: bool,
	// the arrows go to either end and Backspace erases to the start
	pub line: bool,
	// a character key types
	pub types: bool,
}

/// A Mac holds Command for the shortcuts, Option to move by words and Command
/// to go to either end, as its own text fields do. Control types nothing there,
/// and Option plus a letter types whatever the layout puts there.
/// Elsewhere Ctrl does the first two.
pub fn edit_keys(mods: ModifiersState, mac: bool) -> EditKeys {
	if mac {
		EditKeys {
			alt: false,
			shift: mods.shift_key(),
			shortcut: mods.super_key(),
			word: mods.alt_key(),
			line: mods.super_key(),
			types: !mods.super_key() && !mods.control_key(),
		}
	} else {
		EditKeys {
			alt: mods.alt_key(),
			shift: mods.shift_key(),
			shortcut: mods.control_key(),
			word: mods.control_key(),
			line: false,
			types: !mods.control_key(),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelRoute {
	Report,     // button 64/65 reports to a mouse-tracking app
	CursorKeys, // arrow keys, for a full-screen app with no scrollback of its own
	Scrollback, // our own smooth scrollback
}

// Where a wheel turn goes. Alternate scroll (DECSET 1007) is on by default, so
// cursor keys also need the alt screen: on the primary screen they would walk
// shell history instead of scrolling. Shift keeps the wheel local.
pub fn wheel_route(mode: TermMode, shift: bool) -> WheelRoute {
	if !shift && wants_mouse(mode) {
		WheelRoute::Report
	} else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !wants_mouse(mode)
	{
		WheelRoute::CursorKeys
	} else {
		WheelRoute::Scrollback
	}
}

// The copy chord, read off the held modifiers. A grab's pass-through arrives
// with them zeroed, so it never matches.
pub fn is_copy_chord(keys: &Bindings, mods: ModifiersState, key: &Key) -> bool {
	keys.hotkey(key, mods) == Some(Hotkey::Copy)
}

// A key the terminal keeps rather than typing at the shell. Every one but
// `MenuTitle` can be bound in the config file (keys.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
	Settings,
	Fullscreen,
	ContextMenu,
	MenuTitle(char), // Alt+letter, the first letter of a menu-bar title
	NewTab,
	CloseTab,
	NewWindow,
	Zoom(i32),
	ZoomReset,
	PrevTab,
	NextTab,
	MoveTab { forward: bool },
	Copy,
	Paste,
	// Command+Q on macOS, for a press the menu bar does not take
	Quit,
	SplitRight,
	SplitDown,
	ClosePane,
	Focus(Toward),
}

/// Whether a press can go on to the shell. On macOS nothing typed with Command
/// held does, as in every other terminal there.
pub fn reaches_shell(mods: ModifiersState, mac: bool) -> bool {
	!(mac && mods.super_key())
}

// Which hotkey a press is, if any, by the bindings in force.
pub fn hotkey_for(key: &Key, mods: ModifiersState, menu_bar: bool) -> Option<Hotkey> {
	hotkey_in(
		&crate::config::settings().keys,
		key,
		mods,
		menu_bar,
		cfg!(target_os = "macos"),
	)
}

// `hotkey_for` on the default bindings, with the platform passed in, so either
// platform's chords can be checked from any box.
#[cfg(test)]
fn hotkey_on(key: &Key, mods: ModifiersState, menu_bar: bool, mac: bool) -> Option<Hotkey> {
	hotkey_in(&Bindings::defaults(mac), key, mods, menu_bar, mac)
}

// The bound chords come first, so a binding on Alt+letter wins over the menu
// title. Alt+letter opens a title only where there is an in-window bar, which a
// Mac does not have.
fn hotkey_in(
	keys: &Bindings,
	key: &Key,
	mods: ModifiersState,
	menu_bar: bool,
	mac: bool,
) -> Option<Hotkey> {
	if let Some(hotkey) = keys.hotkey(key, mods) {
		return Some(hotkey);
	}
	// This shadows the shell's Meta+<those letters> (Meta-f word-forward), the
	// usual menu-bar tradeoff.
	if !mac && menu_bar && opens_menu_title(mods) {
		if let Key::Character(typed) = key {
			if let Some(ch) = typed.chars().next() {
				return Some(Hotkey::MenuTitle(ch.to_ascii_uppercase()));
			}
		}
	}
	None
}

// Only Alt alone makes a letter a menu title. Ctrl rules out AltGr, which
// Windows reports as Ctrl+Alt; Shift or Super makes it some other chord.
pub fn opens_menu_title(mods: ModifiersState) -> bool {
	mods == ModifiersState::ALT
}

// Where a write to the desktop clipboard comes from.
#[derive(Clone, Copy)]
pub enum CopyFrom {
	Chord,   // Ctrl+Shift+C, Command+C on macOS
	Select,  // a finished drag-select, with copy on select on
	Program, // a program in a pane, through OSC 52
	Output,  // a finished command's output, with copy on output on
}

// Whether a copy goes through. The window-focus flag can lag the window
// manager, so the two a person drives never wait on it: a chord typed at this
// window, or a drag in it, is proof enough it is the one in use. The two that
// fire on their own take only the pane in use - a background pane printing a
// hostile file cannot swap what the next paste holds - and output copies only
// while its window has focus, so a command that finished while the user was
// elsewhere never copies late.
pub fn copy_allowed(from: CopyFrom, window_focused: bool, pane_in_use: bool) -> bool {
	match from {
		CopyFrom::Chord | CopyFrom::Select => true,
		CopyFrom::Program => pane_in_use,
		CopyFrom::Output => window_focused && pane_in_use,
	}
}

// Encode a mouse event as a report for the PTY, honouring the app's tracking
// mode (SGR 1006 vs the legacy X10 form) and modifier bits. `col`/`row` are
// 0-based cells within the viewport; `pressed` is press vs release (wheel is
// press-only); `motion` marks a drag/move report. Returns None when no tracking
// mode is set.
pub fn mouse_report(
	mode: TermMode,
	btn: MouseBtn,
	pressed: bool,
	motion: bool,
	col: usize,
	row: usize,
	mods: ModifiersState,
) -> Option<Vec<u8>> {
	if !wants_mouse(mode) {
		return None;
	}
	let mut button_code = btn.code();
	if motion {
		button_code += 32;
	}
	// modifier bits: shift 4, alt 8, ctrl 16
	button_code +=
		(mods.shift_key() as u8) * 4 + (mods.alt_key() as u8) * 8 + (mods.control_key() as u8) * 16;

	let (wire_col, wire_row) = (col + 1, row + 1); // 1-based on the wire

	if mode.contains(TermMode::SGR_MOUSE) {
		// ESC [ < Cb ; Cx ; Cy  (M press/motion/wheel | m release)
		let end = if pressed || btn.is_wheel() { 'M' } else { 'm' };
		return Some(format!("\x1b[<{button_code};{wire_col};{wire_row}{end}").into_bytes());
	}

	// Legacy X10 form: ESC [ M <Cb+32> <Cx+32> <Cy+32>, one byte each - so a
	// coordinate past 223 can't be encoded; clamp rather than corrupt. Release
	// reports button 3 (wheel never releases, so this only hits real buttons).
	let button_code = if pressed || btn.is_wheel() {
		button_code
	} else {
		(button_code & !0b11) | 3
	};
	let encode_coord = |coord: usize| (coord.min(223) as u8).wrapping_add(32);
	Some(vec![
		0x1b,
		b'[',
		b'M',
		button_code.wrapping_add(32),
		encode_coord(wire_col),
		encode_coord(wire_row),
	])
}

// Cursor-key sequence. In application-cursor-keys mode (DECCKM, set by full
// screen apps like `less`/vim) these use the SS3 (`ESC O`) form; otherwise CSI
// (`ESC [`). Sending the wrong form is why `less` arrow keys did nothing.
pub fn cursor_seq(letter: u8, app_cursor: bool) -> Vec<u8> {
	let prefix = if app_cursor { b'O' } else { b'[' };
	vec![0x1b, prefix, letter]
}

// Translate a key press into the bytes a PTY expects. Returns None for keys
// we don't forward (modifiers alone, unhandled named keys, etc.).
pub fn encode(ev: &KeyEvent, mods: ModifiersState, app_cursor: bool) -> Option<Vec<u8>> {
	encode_key(&ev.logical_key, ev.text.as_deref(), mods, app_cursor)
}

// Windows lets a program hand a window a character instead of a key press, and
// it arrives as a key the layout cannot name carrying only the text it stands
// for. The touch keyboard sends characters its layout has no key for that way,
// and so do text expanders and some accessibility tools. Nothing else produces
// an unnamed key with text on it, so the text is what was typed.
fn typed_key(key: &Key, text: Option<&str>) -> Option<Key> {
	match (key, text) {
		(Key::Unidentified(_), Some(text)) if !text.is_empty() => {
			Some(Key::Character(SmolStr::new(text)))
		}
		_ => None,
	}
}

// Name a key the platform could not, so every reader of the event sees the
// character rather than nothing. Both window event handlers call this before
// they look at a key.
pub fn name_typed(mut ev: KeyEvent) -> KeyEvent {
	if let Some(key) = typed_key(&ev.logical_key, ev.text.as_deref()) {
		ev.logical_key = key;
	}
	ev
}

// The tilde-form CSI number for a key, if it has one (`ESC [ <n> ~`).
fn tilde_num(named: NamedKey) -> Option<u8> {
	Some(match named {
		NamedKey::Insert => 2,
		NamedKey::Delete => 3,
		NamedKey::PageUp => 5,
		NamedKey::PageDown => 6,
		NamedKey::F5 => 15,
		NamedKey::F6 => 17, // 16 is skipped by xterm, as are 22 and 25
		NamedKey::F7 => 18,
		NamedKey::F8 => 19,
		NamedKey::F9 => 20,
		NamedKey::F10 => 21,
		NamedKey::F11 => 23,
		NamedKey::F12 => 24,
		_ => return None,
	})
}

// The final byte of the letter-form CSI for a key (`ESC [ 1 ; <m> <letter>`);
// same finals as the unmodified SS3/CSI forms for cursor keys and F1-F4.
fn letter_final(named: NamedKey) -> Option<u8> {
	Some(match named {
		NamedKey::ArrowUp => b'A',
		NamedKey::ArrowDown => b'B',
		NamedKey::ArrowRight => b'C',
		NamedKey::ArrowLeft => b'D',
		NamedKey::Home => b'H',
		NamedKey::End => b'F',
		NamedKey::F1 => b'P',
		NamedKey::F2 => b'Q',
		NamedKey::F3 => b'R',
		NamedKey::F4 => b'S',
		_ => return None,
	})
}

fn encode_key(
	key: &Key,
	text: Option<&str>,
	mods: ModifiersState,
	app_cursor: bool,
) -> Option<Vec<u8>> {
	let ctrl = mods.control_key();
	let alt = mods.alt_key();
	let shift = mods.shift_key();
	// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4)
	let mod_param = 1 + shift as u8 + ((alt as u8) << 1) + ((ctrl as u8) << 2);

	let with_alt = |bytes: Vec<u8>| -> Vec<u8> {
		if alt {
			let mut prefixed = vec![0x1b];
			prefixed.extend_from_slice(&bytes);
			prefixed
		} else {
			bytes
		}
	};

	match key {
		Key::Named(named) => {
			// Modified navigation/function keys use the xterm `;<m>` forms
			// (Ctrl+Arrow word-skip, Ctrl+Del, Shift+F<n>, ...). These replace
			// the ESC prefix for Alt too - apps expect CSI 1;3A, not ESC CSI A.
			if mod_param > 1 {
				if *named == NamedKey::Backspace && ctrl {
					// xterm/VTE convention; shells bind ^H to a word delete
					return Some(with_alt(vec![0x08]));
				}
				if let Some(letter) = letter_final(*named) {
					return Some(format!("\x1b[1;{mod_param}{}", letter as char).into_bytes());
				}
				if let Some(tilde_param) = tilde_num(*named) {
					return Some(format!("\x1b[{tilde_param};{mod_param}~").into_bytes());
				}
			}
			let bytes: Vec<u8> = match named {
				NamedKey::Enter => vec![b'\r'],
				NamedKey::Backspace => vec![0x7f],
				NamedKey::Tab => {
					if shift {
						return Some(b"\x1b[Z".to_vec());
					}
					vec![b'\t']
				}
				NamedKey::Escape => vec![0x1b],
				// winit reports the spacebar as a named key on every platform, so
				// this is where Ctrl+Space has to become NUL - set-mark in emacs,
				// readline and tmux all rely on it.
				NamedKey::Space if ctrl => vec![0],
				NamedKey::Space => vec![b' '],
				NamedKey::ArrowUp => cursor_seq(b'A', app_cursor),
				NamedKey::ArrowDown => cursor_seq(b'B', app_cursor),
				NamedKey::ArrowRight => cursor_seq(b'C', app_cursor),
				NamedKey::ArrowLeft => cursor_seq(b'D', app_cursor),
				NamedKey::Home => cursor_seq(b'H', app_cursor),
				NamedKey::End => cursor_seq(b'F', app_cursor),
				// F1-F4 are SS3; the rest of the named keys we forward are tilde-form
				NamedKey::F1 => b"\x1bOP".to_vec(),
				NamedKey::F2 => b"\x1bOQ".to_vec(),
				NamedKey::F3 => b"\x1bOR".to_vec(),
				NamedKey::F4 => b"\x1bOS".to_vec(),
				_ => match tilde_num(*named) {
					Some(tilde_param) => format!("\x1b[{tilde_param}~").into_bytes(),
					None => return None,
				},
			};
			Some(with_alt(bytes))
		}
		Key::Character(char_str) => {
			if ctrl {
				// map ctrl+<char> to its control code
				let c = char_str.chars().next()?;
				let lower = c.to_ascii_lowercase();
				let code = match lower {
					'a'..='z' => (lower as u8 - b'a') + 1,
					'@' => 0,
					'[' => 0x1b,
					'\\' => 0x1c,
					']' => 0x1d,
					'^' => 0x1e,
					'_' => 0x1f,
					_ => return None,
				};
				return Some(with_alt(vec![code]));
			}
			// printable text from the platform layout
			let text = text.map(|t| t.as_bytes().to_vec())?;
			Some(with_alt(text))
		}
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use winit::keyboard::NativeKey;

	use super::*;
	use crate::keys::{Chord, KeyName};

	const NONE: ModifiersState = ModifiersState::empty();

	fn enc(named: NamedKey, mods: ModifiersState, app_cursor: bool) -> Option<Vec<u8>> {
		encode_key(&Key::Named(named), None, mods, app_cursor)
	}

	// Test ID: EiokAEK
	#[test]
	fn arrows_follow_decckm() {
		assert_eq!(enc(NamedKey::ArrowUp, NONE, false).unwrap(), b"\x1b[A");
		assert_eq!(enc(NamedKey::ArrowUp, NONE, true).unwrap(), b"\x1bOA");
		assert_eq!(enc(NamedKey::End, NONE, false).unwrap(), b"\x1b[F");
	}

	// Test ID: EiokAEL
	#[test]
	fn modified_arrows_use_csi_mod_form() {
		// Ctrl+Right = word skip in readline/most TUIs
		let ctrl = ModifiersState::CONTROL;
		assert_eq!(
			enc(NamedKey::ArrowRight, ctrl, false).unwrap(),
			b"\x1b[1;5C"
		);
		// modified keys stay CSI even in app-cursor mode
		assert_eq!(enc(NamedKey::ArrowRight, ctrl, true).unwrap(), b"\x1b[1;5C");
		assert_eq!(
			enc(NamedKey::ArrowLeft, ModifiersState::SHIFT, false).unwrap(),
			b"\x1b[1;2D"
		);
		assert_eq!(
			enc(NamedKey::ArrowUp, ModifiersState::ALT, false).unwrap(),
			b"\x1b[1;3A"
		);
		assert_eq!(
			enc(NamedKey::ArrowDown, ctrl | ModifiersState::SHIFT, false).unwrap(),
			b"\x1b[1;6B"
		);
	}

	// Test ID: EiokAEM
	#[test]
	fn function_keys() {
		assert_eq!(enc(NamedKey::F1, NONE, false).unwrap(), b"\x1bOP");
		assert_eq!(enc(NamedKey::F5, NONE, false).unwrap(), b"\x1b[15~");
		assert_eq!(enc(NamedKey::F6, NONE, false).unwrap(), b"\x1b[17~");
		assert_eq!(enc(NamedKey::F10, NONE, false).unwrap(), b"\x1b[21~");
		assert_eq!(enc(NamedKey::F12, NONE, false).unwrap(), b"\x1b[24~");
		assert_eq!(
			enc(NamedKey::F1, ModifiersState::CONTROL, false).unwrap(),
			b"\x1b[1;5P"
		);
		assert_eq!(
			enc(NamedKey::F5, ModifiersState::SHIFT, false).unwrap(),
			b"\x1b[15;2~"
		);
	}

	// Test ID: EiokAEN
	#[test]
	fn editing_keys() {
		let ctrl = ModifiersState::CONTROL;
		assert_eq!(enc(NamedKey::Delete, NONE, false).unwrap(), b"\x1b[3~");
		assert_eq!(enc(NamedKey::Delete, ctrl, false).unwrap(), b"\x1b[3;5~");
		assert_eq!(enc(NamedKey::Backspace, NONE, false).unwrap(), [0x7f]);
		assert_eq!(enc(NamedKey::Backspace, ctrl, false).unwrap(), [0x08]);
		assert_eq!(enc(NamedKey::PageUp, NONE, false).unwrap(), b"\x1b[5~");
		assert_eq!(
			enc(NamedKey::Tab, ModifiersState::SHIFT, false).unwrap(),
			b"\x1b[Z"
		);
	}

	// Test ID: Eizuc4W
	#[test]
	fn mouse_sgr_and_x10() {
		let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
		// wheel down at col 5 / row 10 -> button 65, 1-based coords
		assert_eq!(
			mouse_report(sgr, MouseBtn::WheelDown, true, false, 5, 10, NONE).unwrap(),
			b"\x1b[<65;6;11M"
		);
		// left press vs release: same Cb, M vs m
		assert_eq!(
			mouse_report(sgr, MouseBtn::Left, true, false, 0, 0, NONE).unwrap(),
			b"\x1b[<0;1;1M"
		);
		assert_eq!(
			mouse_report(sgr, MouseBtn::Left, false, false, 0, 0, NONE).unwrap(),
			b"\x1b[<0;1;1m"
		);
		// ctrl adds 16; a bare motion uses button 3 + motion bit 32 = 35
		assert_eq!(
			mouse_report(
				sgr,
				MouseBtn::Left,
				true,
				false,
				0,
				0,
				ModifiersState::CONTROL
			)
			.unwrap(),
			b"\x1b[<16;1;1M"
		);
		assert_eq!(
			mouse_report(sgr, MouseBtn::None, true, true, 0, 0, NONE).unwrap(),
			b"\x1b[<35;1;1M"
		);
		// legacy X10 form: ESC [ M, then (Cb+32)(Cx+32)(Cy+32)
		let x10 = TermMode::MOUSE_REPORT_CLICK;
		assert_eq!(
			mouse_report(x10, MouseBtn::Left, true, false, 0, 0, NONE).unwrap(),
			[0x1b, b'[', b'M', 32, 33, 33]
		);
		assert_eq!(
			mouse_report(x10, MouseBtn::WheelUp, true, false, 0, 0, NONE).unwrap(),
			[0x1b, b'[', b'M', 96, 33, 33]
		);
	}

	// Test ID: Eizuc4X
	#[test]
	fn mouse_report_needs_tracking() {
		// no tracking mode set -> nothing to report
		assert!(mouse_report(TermMode::empty(), MouseBtn::Left, true, false, 0, 0, NONE).is_none());
		// SGR flag alone (no click/drag/motion) is not tracking
		assert!(
			mouse_report(TermMode::SGR_MOUSE, MouseBtn::Left, true, false, 0, 0, NONE).is_none()
		);
	}

	// winit calls the spacebar a named key, so Ctrl+Space never reached the
	// control-code path and sent a plain space. Emacs and readline set-mark, and
	// tmux begin-selection, all want NUL.
	// Test ID: EpHUQUa
	#[test]
	fn ctrl_space_is_nul() {
		let ctrl = ModifiersState::CONTROL;
		assert_eq!(enc(NamedKey::Space, ctrl, false).unwrap(), [0x00]);
		assert_eq!(enc(NamedKey::Space, NONE, false).unwrap(), b" ");
		// Ctrl+Alt+Space still takes the alt prefix
		assert_eq!(
			enc(NamedKey::Space, ctrl | ModifiersState::ALT, false).unwrap(),
			[0x1b, 0x00]
		);
	}

	// A character handed to the window instead of typed at it arrives as a key
	// the layout cannot name. It used to reach nothing at all.
	// Test ID: EpPhAdV
	#[test]
	fn an_unnamed_key_is_read_as_its_text() {
		let injected = Key::Unidentified(NativeKey::Windows(0xe7));
		assert_eq!(
			typed_key(&injected, Some("e")),
			Some(Key::Character("e".into()))
		);
		// what the shell would then get
		assert_eq!(
			encode_key(&Key::Character("e".into()), Some("e"), NONE, false).unwrap(),
			b"e".to_vec()
		);
		// a character no keyboard layout here has a key for
		assert_eq!(
			typed_key(&injected, Some("\u{e9}")),
			Some(Key::Character("\u{e9}".into()))
		);
	}

	// Test ID: EpPhAdW
	#[test]
	fn a_key_that_carries_no_text_stays_unnamed() {
		let injected = Key::Unidentified(NativeKey::Windows(0xe7));
		assert_eq!(typed_key(&injected, None), None);
		assert_eq!(typed_key(&injected, Some("")), None);
	}

	// Only an unnamed key is filled in from its text. A named key already says
	// what it is, and its text is a representation of that rather than typing -
	// Enter carries "\r", and reading it as a character would lose the key.
	// Test ID: EpPhAdX
	#[test]
	fn a_key_the_layout_named_is_left_alone() {
		assert_eq!(typed_key(&Key::Named(NamedKey::Enter), Some("\r")), None);
		assert_eq!(typed_key(&Key::Named(NamedKey::Tab), Some("\t")), None);
		assert_eq!(typed_key(&Key::Character("a".into()), Some("a")), None);
		assert_eq!(typed_key(&Key::Dead(Some('\u{301}')), Some("a")), None);
	}

	// Test ID: EiokAEO
	#[test]
	fn ctrl_chars_and_alt_prefix() {
		let ctrl = ModifiersState::CONTROL;
		let a = Key::Character("a".into());
		assert_eq!(encode_key(&a, Some("a"), ctrl, false).unwrap(), [0x01]);
		assert_eq!(
			encode_key(&a, Some("a"), NONE, false).unwrap(),
			b"a".to_vec()
		);
		assert_eq!(
			encode_key(&a, Some("a"), ModifiersState::ALT, false).unwrap(),
			b"\x1ba".to_vec()
		);
	}

	const CTRL: ModifiersState = ModifiersState::CONTROL;
	const CTRL_SHIFT: ModifiersState = ModifiersState::CONTROL.union(ModifiersState::SHIFT);

	// Linux and Windows; the Mac has its own tests below
	fn chord(typed: &str, mods: ModifiersState) -> Option<Hotkey> {
		hotkey_on(&Key::Character(typed.into()), mods, true, false)
	}

	fn named(key: NamedKey, mods: ModifiersState) -> Option<Hotkey> {
		hotkey_on(&Key::Named(key), mods, true, false)
	}

	// Test ID: Er2UiYC
	#[test]
	fn a_new_tab_takes_shift_so_plain_ctrl_t_reaches_the_shell() {
		assert_eq!(chord("T", CTRL_SHIFT), Some(Hotkey::NewTab));
		assert_eq!(chord("t", CTRL_SHIFT), Some(Hotkey::NewTab));
		assert_eq!(chord("t", CTRL), None);
	}

	// Test ID: Er2UiYD
	#[test]
	fn ctrl_page_keys_change_tab() {
		assert_eq!(named(NamedKey::PageUp, CTRL), Some(Hotkey::PrevTab));
		assert_eq!(named(NamedKey::PageDown, CTRL), Some(Hotkey::NextTab));
		assert_eq!(named(NamedKey::PageUp, NONE), None);
	}

	// Test ID: Er2UiYE
	#[test]
	fn ctrl_shift_page_keys_move_the_tab() {
		assert_eq!(
			named(NamedKey::PageUp, CTRL_SHIFT),
			Some(Hotkey::MoveTab { forward: false })
		);
		assert_eq!(
			named(NamedKey::PageDown, CTRL_SHIFT),
			Some(Hotkey::MoveTab { forward: true })
		);
	}

	// Test ID: Er2UiYF
	#[test]
	fn both_close_tab_chords_close_but_plain_ctrl_w_reaches_the_shell() {
		assert_eq!(chord("W", CTRL_SHIFT), Some(Hotkey::CloseTab));
		assert_eq!(named(NamedKey::F4, CTRL), Some(Hotkey::CloseTab));
		assert_eq!(chord("w", CTRL), None, "word erase");
		assert_eq!(named(NamedKey::F4, NONE), None);
	}

	// Test ID: Er2UiYG
	#[test]
	fn the_menu_key_settings_and_fullscreen_are_hotkeys() {
		assert_eq!(
			named(NamedKey::ContextMenu, NONE),
			Some(Hotkey::ContextMenu)
		);
		assert_eq!(chord(",", CTRL), Some(Hotkey::Settings));
		assert_eq!(named(NamedKey::F11, NONE), Some(Hotkey::Fullscreen));
		assert_eq!(chord("V", CTRL_SHIFT), Some(Hotkey::Paste));
		assert_eq!(chord("C", CTRL_SHIFT), Some(Hotkey::Copy));
		assert_eq!(chord("v", CTRL), None);
	}

	// Off since a Mac's own chords moved from Ctrl to Command (20261002), so
	// Ctrl+, no longer opens Settings there.
	// `command_comma_is_the_only_settings_chord_on_macos` covers it.
	// // Command+, is Settings on macOS and nowhere else. Ctrl+, stays on every
	// // platform, and Command with anything more is not the chord.
	// // Test ID: ErUnDJY
	// #[test]
	// fn command_comma_opens_settings_on_macos_only() {
	// 	const COMMAND: ModifiersState = ModifiersState::SUPER;
	// 	let on = |mods, mac| hotkey_on(&Key::Character(",".into()), mods, true, mac);
	// 	assert_eq!(on(COMMAND, true), Some(Hotkey::Settings));
	// 	assert_eq!(on(COMMAND, false), None, "Super+, off macOS");
	// 	assert_eq!(on(CTRL, true), Some(Hotkey::Settings));
	// 	assert_eq!(on(CTRL, false), Some(Hotkey::Settings));
	// 	for extra in [ModifiersState::SHIFT, ModifiersState::ALT] {
	// 		assert_ne!(on(COMMAND.union(extra), true), Some(Hotkey::Settings));
	// 	}
	// 	assert_eq!(on(NONE, true), None);
	// 	assert_eq!(
	// 		hotkey_on(&Key::Character(".".into()), COMMAND, true, true),
	// 		None
	// 	);
	// }

	// Off since a Mac's own chords moved from Ctrl to Command (20261002): the
	// Ctrl chords go to the shell there, where this held them unchanged.
	// `the_command_chords_are_the_only_program_chords_on_macos` covers it.
	// // On a Mac each action with an Apple standard shortcut answers to it, and
	// // nothing else held with Command does. Off a Mac, Super chords stay free. No
	// // Ctrl chord moves either way.
	// // Test ID: ErZrRVm
	// #[test]
	// fn the_command_chords_work_on_macos_and_leave_ctrl_alone() {
	// 	const COMMAND: ModifiersState = ModifiersState::SUPER;
	// 	let held = |chord: CommandChord| {
	// 		let mut mods = COMMAND;
	// 		for (on, flag) in [
	// 			(chord.shift, ModifiersState::SHIFT),
	// 			(chord.option, ModifiersState::ALT),
	// 			(chord.control, CTRL),
	// 		] {
	// 			if on {
	// 				mods |= flag;
	// 			}
	// 		}
	// 		mods
	// 	};
	// 	let on =
	// 		|typed: &str, mods, mac| hotkey_on(&Key::Character(typed.into()), mods, false, mac);
	// 	for (hotkey, chord) in COMMAND_CHORDS {
	// 		assert_eq!(
	// 			on(chord.key, held(*chord), true),
	// 			Some(*hotkey),
	// 			"{chord:?}"
	// 		);
	// 		assert_eq!(
	// 			on(chord.key, held(*chord), false),
	// 			None,
	// 			"{chord:?} off macOS"
	// 		);
	// 	}
	// 	let find = |hotkey| command_chord(hotkey).map(CommandChord::spoken);
	// 	assert_eq!(find(Hotkey::NewTab).as_deref(), Some("Command+T"));
	// 	assert_eq!(find(Hotkey::CloseTab).as_deref(), Some("Command+W"));
	// 	assert_eq!(find(Hotkey::NewWindow).as_deref(), Some("Command+N"));
	// 	assert_eq!(find(Hotkey::Copy).as_deref(), Some("Command+C"));
	// 	assert_eq!(find(Hotkey::Paste).as_deref(), Some("Command+V"));
	// 	assert_eq!(find(Hotkey::Settings).as_deref(), Some("Command+,"));
	// 	assert_eq!(find(Hotkey::Quit).as_deref(), Some("Command+Q"));
	// 	assert_eq!(find(Hotkey::Zoom(1)).as_deref(), Some("Command+Plus"));
	// 	assert_eq!(find(Hotkey::Zoom(-1)).as_deref(), Some("Command+Minus"));
	// 	assert_eq!(find(Hotkey::ZoomReset).as_deref(), Some("Command+0"));
	// 	assert_eq!(
	// 		find(Hotkey::Fullscreen).as_deref(),
	// 		Some("Control+Command+F")
	// 	);
	// 	// "+" is Shift+"=", so Command+= counts too
	// 	assert_eq!(on("=", COMMAND, true), Some(Hotkey::Zoom(1)));
	// 	assert_eq!(
	// 		on("+", COMMAND.union(ModifiersState::SHIFT), true),
	// 		Some(Hotkey::Zoom(1))
	// 	);
	// 	// a different modifier set is a different chord
	// 	assert_eq!(on("t", COMMAND.union(ModifiersState::SHIFT), true), None);
	// 	assert_eq!(on("c", COMMAND.union(ModifiersState::ALT), true), None);
	// 	assert_eq!(on("f", COMMAND, true), None, "no Find yet");
	// 	assert_eq!(on("k", COMMAND, true), None);
	// 	// every Ctrl chord means what it meant before
	// 	let mut keys: Vec<String> = ('a'..='z').map(String::from).collect();
	// 	keys.extend([",", "-", "=", "+", "0", ".", "["].map(String::from));
	// 	for mods in [
	// 		CTRL,
	// 		CTRL_SHIFT,
	// 		CTRL.union(ModifiersState::ALT),
	// 		ModifiersState::ALT,
	// 	] {
	// 		for typed in &keys {
	// 			assert_eq!(
	// 				on(typed, mods, true),
	// 				on(typed, mods, false),
	// 				"{mods:?} {typed}"
	// 			);
	// 		}
	// 	}
	// }

	const COMMAND: ModifiersState = ModifiersState::SUPER;

	fn held(chord: Chord) -> ModifiersState {
		let mut mods = NONE;
		for (on, flag) in [
			(chord.command, COMMAND),
			(chord.shift, ModifiersState::SHIFT),
			(chord.alt, ModifiersState::ALT),
			(chord.ctrl, CTRL),
		] {
			if on {
				mods |= flag;
			}
		}
		mods
	}

	// The key a chord is pressed on, as a press would name it.
	fn pressed(chord: Chord) -> Key {
		match chord.key {
			KeyName::Char(ch) => Key::Character(ch.to_string().into()),
			KeyName::Plus => Key::Character("+".into()),
			KeyName::Minus => Key::Character("-".into()),
			KeyName::Named(named) => Key::Named(named),
		}
	}

	// Every default chord on a platform with Command held, and its hotkey.
	fn command_defaults(mac: bool) -> Vec<(Hotkey, Chord)> {
		let keys = Bindings::defaults(mac);
		crate::keys::config_paths()
			.flat_map(|(hotkey, _)| {
				keys.chords(hotkey)
					.iter()
					.filter(|chord| chord.command)
					.map(move |chord| (hotkey, *chord))
					.collect::<Vec<_>>()
			})
			.collect()
	}

	// Command+, is Settings on a Mac, and Ctrl+, goes to the shell there.
	// Elsewhere Ctrl+, is Settings and Super+, is not.
	// Test ID: ErbGP9B
	#[test]
	fn command_comma_is_the_only_settings_chord_on_macos() {
		let on = |mods, mac| hotkey_on(&Key::Character(",".into()), mods, true, mac);
		assert_eq!(on(COMMAND, true), Some(Hotkey::Settings));
		assert_eq!(on(CTRL, true), None, "Ctrl+, on macOS");
		assert_eq!(on(CTRL, false), Some(Hotkey::Settings));
		assert_eq!(on(COMMAND, false), None, "Super+, off macOS");
		for extra in [ModifiersState::SHIFT, ModifiersState::ALT, CTRL] {
			assert_ne!(on(COMMAND.union(extra), true), Some(Hotkey::Settings));
		}
		assert_eq!(on(NONE, true), None);
	}

	// On a Mac the program answers to the Command chords and to nothing typed
	// with Ctrl, which all goes to the shell. Off a Mac, Super chords stay free
	// and the Ctrl chords are the program's.
	// Test ID: ErbGPD5
	#[test]
	fn the_command_chords_are_the_only_program_chords_on_macos() {
		let on = |typed: &str, mods, mac| hotkey_on(&Key::Character(typed.into()), mods, true, mac);
		let mac_chords = command_defaults(true);
		assert!(mac_chords.len() > 15, "{mac_chords:?}");
		for (hotkey, chord) in mac_chords {
			let press = pressed(chord);
			assert_eq!(
				hotkey_on(&press, held(chord), true, true),
				Some(hotkey),
				"{chord:?}"
			);
			assert_eq!(
				hotkey_on(&press, held(chord), true, false),
				None,
				"{chord:?} off macOS"
			);
		}
		assert!(command_defaults(false).is_empty());
		let mac_keys = Bindings::defaults(true);
		let find = |hotkey| mac_keys.shown(hotkey).map(|chord| chord.spoken(true));
		for (hotkey, spoken) in [
			(Hotkey::NewTab, "Command+T"),
			(Hotkey::CloseTab, "Command+W"),
			(Hotkey::NewWindow, "Command+N"),
			(Hotkey::Copy, "Command+C"),
			(Hotkey::Paste, "Command+V"),
			(Hotkey::Settings, "Command+,"),
			(Hotkey::Quit, "Command+Q"),
			(Hotkey::Zoom(1), "Command+Plus"),
			(Hotkey::Zoom(-1), "Command+Minus"),
			(Hotkey::ZoomReset, "Command+0"),
			(Hotkey::Fullscreen, "Control+Command+F"),
			(Hotkey::PrevTab, "Shift+Command+["),
			(Hotkey::NextTab, "Shift+Command+]"),
		] {
			assert_eq!(find(hotkey).as_deref(), Some(spoken));
		}
		// "+" is Shift+"=", so Command+= counts too
		assert_eq!(on("=", COMMAND, true), Some(Hotkey::Zoom(1)));
		assert_eq!(
			on("+", COMMAND.union(ModifiersState::SHIFT), true),
			Some(Hotkey::Zoom(1))
		);
		// a different modifier set is a different chord
		assert_eq!(on("t", COMMAND.union(ModifiersState::SHIFT), true), None);
		assert_eq!(on("c", COMMAND.union(ModifiersState::ALT), true), None);
		assert_eq!(on("f", COMMAND, true), None, "no Find yet");
		assert_eq!(on("k", COMMAND, true), None);
		// no Ctrl or Alt chord is the program's on a Mac, though many are elsewhere
		let mut keys: Vec<String> = ('a'..='z').map(String::from).collect();
		keys.extend([",", "-", "=", "+", "0", ".", "[", "]"].map(String::from));
		let mut elsewhere = 0;
		for mods in [
			CTRL,
			CTRL_SHIFT,
			CTRL.union(ModifiersState::ALT),
			ModifiersState::ALT,
		] {
			for typed in &keys {
				assert_eq!(on(typed, mods, true), None, "{mods:?} {typed}");
				elsewhere += usize::from(on(typed, mods, false).is_some());
			}
			for key in [NamedKey::PageUp, NamedKey::PageDown, NamedKey::F4] {
				let named = |mac| hotkey_on(&Key::Named(key), mods, true, mac);
				assert_eq!(named(true), None, "{mods:?} {key:?}");
				elsewhere += usize::from(named(false).is_some());
			}
		}
		assert!(elsewhere > 20, "{elsewhere}");
		// keys that are not chords keep what they do
		for mac in [false, true] {
			let named = |key| hotkey_on(&Key::Named(key), NONE, true, mac);
			assert_eq!(named(NamedKey::F11), Some(Hotkey::Fullscreen));
			assert_eq!(named(NamedKey::ContextMenu), Some(Hotkey::ContextMenu));
		}
	}

	// Command+Shift+[ and ] walk the tabs on a Mac, which reports the press by
	// the shifted character. The page keys walk and carry tabs with Command,
	// as they do with Ctrl elsewhere.
	// Test ID: ErbGPGY
	#[test]
	fn command_shift_brackets_walk_the_tabs_on_macos() {
		let shifted = COMMAND.union(ModifiersState::SHIFT);
		let on =
			|typed: &str, mods, mac| hotkey_on(&Key::Character(typed.into()), mods, false, mac);
		for typed in ["[", "{"] {
			assert_eq!(on(typed, shifted, true), Some(Hotkey::PrevTab), "{typed}");
			assert_eq!(on(typed, shifted, false), None, "{typed} off macOS");
		}
		for typed in ["]", "}"] {
			assert_eq!(on(typed, shifted, true), Some(Hotkey::NextTab), "{typed}");
		}
		assert_eq!(on("[", COMMAND, true), None, "no Shift");
		assert_eq!(on("{", shifted.union(ModifiersState::ALT), true), None);
		assert_eq!(on("{", shifted.union(CTRL), true), None);
		assert_eq!(crate::keys::us_shifted('['), Some("{"));
		assert_eq!(crate::keys::us_shifted('t'), None);
		let page = |key, mods| hotkey_on(&Key::Named(key), mods, false, true);
		assert_eq!(page(NamedKey::PageUp, COMMAND), Some(Hotkey::PrevTab));
		assert_eq!(page(NamedKey::PageDown, COMMAND), Some(Hotkey::NextTab));
		assert_eq!(
			page(NamedKey::PageUp, shifted),
			Some(Hotkey::MoveTab { forward: false })
		);
		assert_eq!(
			page(NamedKey::PageDown, shifted),
			Some(Hotkey::MoveTab { forward: true })
		);
		assert_eq!(page(NamedKey::PageUp, COMMAND.union(CTRL)), None);
		assert_eq!(page(NamedKey::Home, COMMAND), None);
	}

	// The key for the program's own shortcuts, and for a text box's, is Command
	// on a Mac and Ctrl elsewhere. A Mac moves by words with Option, and goes
	// to either end with Command.
	// Test ID: ErbGPK5
	#[test]
	fn the_shortcut_key_is_command_on_macos_and_ctrl_elsewhere() {
		assert!(shortcut_held(COMMAND, true));
		assert!(!shortcut_held(CTRL, true));
		assert!(shortcut_held(CTRL, false));
		assert!(!shortcut_held(COMMAND, false));
		let c = Key::Character("c".into());
		let (mac_keys, pc_keys) = (Bindings::defaults(true), Bindings::defaults(false));
		assert!(is_copy_chord(&mac_keys, COMMAND, &c));
		assert!(!is_copy_chord(&mac_keys, CTRL_SHIFT, &c));
		assert!(!is_copy_chord(
			&mac_keys,
			COMMAND.union(ModifiersState::SHIFT),
			&c
		));
		assert!(is_copy_chord(&pc_keys, CTRL_SHIFT, &c));
		assert!(!is_copy_chord(&pc_keys, COMMAND, &c));
		let option = ModifiersState::ALT;
		let mac = |mods| edit_keys(mods, true);
		assert!(mac(COMMAND).shortcut && mac(COMMAND).line && !mac(COMMAND).types);
		assert!(!mac(COMMAND).word);
		assert!(mac(option).word && !mac(option).shortcut && mac(option).types);
		assert!(!mac(CTRL).shortcut && !mac(CTRL).word && !mac(CTRL).types);
		assert!(mac(NONE).types);
		let pc = |mods| edit_keys(mods, false);
		assert!(pc(CTRL).shortcut && pc(CTRL).word && !pc(CTRL).types);
		assert!(!pc(CTRL).line);
		assert!(!pc(COMMAND).shortcut && pc(COMMAND).types);
		assert!(!pc(option).word && pc(option).alt);
		assert_eq!(
			pc(ModifiersState::SHIFT),
			EditKeys {
				shift: true,
				types: true,
				..EditKeys::default()
			}
		);
	}

	// Ctrl decides alone, whatever else is held. Linux and Windows keep
	// Ctrl+click for links and block selection.
	// Test ID: ErbblFg
	#[test]
	fn ctrl_click_is_the_right_click_on_macos_only() {
		let buttons = [
			MouseButton::Left,
			MouseButton::Right,
			MouseButton::Middle,
			MouseButton::Back,
			MouseButton::Forward,
			MouseButton::Other(5),
		];
		let keys = [
			ModifiersState::CONTROL,
			ModifiersState::SHIFT,
			ModifiersState::ALT,
			ModifiersState::SUPER,
		];
		for mac in [true, false] {
			for button in buttons {
				for combo in 0..16usize {
					let mods = keys
						.iter()
						.enumerate()
						.filter(|(bit, _)| combo & (1 << bit) != 0)
						.fold(ModifiersState::empty(), |acc, (_, key)| acc | *key);
					let expected = if mac && button == MouseButton::Left && mods.control_key() {
						MouseButton::Right
					} else {
						button
					};
					assert_eq!(
						acting_button(button, mods, mac),
						expected,
						"{button:?} {mods:?} mac={mac}"
					);
				}
			}
		}
	}

	// Option plus a letter types on a Mac, often a character the layout has
	// nowhere else, such as "{" on a German one. Only elsewhere is Alt a
	// dialog's accelerator key.
	// Test ID: ErbKdHM
	#[test]
	fn option_types_on_a_mac_and_alt_is_an_accelerator_elsewhere() {
		let option = ModifiersState::ALT;
		let mac = edit_keys(option, true);
		assert!(!mac.alt && mac.types && mac.word);
		let pc = edit_keys(option, false);
		assert!(pc.alt && pc.types && !pc.word);
	}

	// Nothing typed with Command held reaches the shell on a Mac. Elsewhere a
	// Super chord goes on as it always has.
	// Test ID: ErZrRpZ
	#[test]
	fn command_never_reaches_the_shell_on_macos() {
		const COMMAND: ModifiersState = ModifiersState::SUPER;
		assert!(!reaches_shell(COMMAND, true));
		assert!(!reaches_shell(COMMAND.union(CTRL), true));
		assert!(reaches_shell(COMMAND, false));
		for mods in [NONE, CTRL, CTRL_SHIFT, ModifiersState::ALT] {
			assert!(reaches_shell(mods, true), "{mods:?}");
		}
	}

	// Off since panes got hotkeys (20261003, 2026100220292607): Alt+Shift+Plus
	// and Minus split, Alt+Shift+W closes and Alt+arrows move, as in Windows
	// Terminal. Its Ctrl and Ctrl+Shift checks still hold and moved to
	// `alt_shift_chords_split_and_close_panes_and_alt_arrows_move`.
	// // Pane split, close and focus cycling are menu-only, so every other chord
	// // goes to the shell.
	// // Test ID: Er2UiYH
	// #[test]
	// fn no_chord_splits_closes_or_cycles_panes() {
	// 	for c in 'a'..='z' {
	// 		let hotkey = chord(&c.to_string(), CTRL_SHIFT);
	// 		match c {
	// 			't' | 'w' | 'n' | 'c' | 'v' => assert!(hotkey.is_some(), "{c}"),
	// 			_ => assert_eq!(hotkey, None, "Ctrl+Shift+{c}"),
	// 		}
	// 	}
	// 	for key in [
	// 		NamedKey::Tab,
	// 		NamedKey::ArrowLeft,
	// 		NamedKey::ArrowRight,
	// 		NamedKey::ArrowUp,
	// 		NamedKey::ArrowDown,
	// 	] {
	// 		assert_eq!(named(key, CTRL), None, "{key:?}");
	// 		assert_eq!(named(key, CTRL_SHIFT), None, "{key:?}");
	// 	}
	// }

	// The pane chords are Windows Terminal's: Alt+Shift+Plus splits right,
	// Alt+Shift+Minus splits down, Alt+Shift+W closes the pane and Alt+arrows
	// move between panes. Alt+letter still opens a menu, and no Ctrl or
	// Ctrl+Shift chord was taken for panes.
	// Test ID: EreU3sZ
	#[test]
	fn alt_shift_chords_split_and_close_panes_and_alt_arrows_move() {
		const ALT: ModifiersState = ModifiersState::ALT;
		const ALT_SHIFT: ModifiersState = ALT.union(ModifiersState::SHIFT);
		assert_eq!(chord("+", ALT_SHIFT), Some(Hotkey::SplitRight));
		assert_eq!(chord("_", ALT_SHIFT), Some(Hotkey::SplitDown));
		assert_eq!(chord("W", ALT_SHIFT), Some(Hotkey::ClosePane));
		for (key, toward) in [
			(NamedKey::ArrowLeft, Toward::Left),
			(NamedKey::ArrowRight, Toward::Right),
			(NamedKey::ArrowUp, Toward::Up),
			(NamedKey::ArrowDown, Toward::Down),
		] {
			assert_eq!(named(key, ALT), Some(Hotkey::Focus(toward)));
			assert_eq!(named(key, ALT_SHIFT), None, "{key:?}");
			assert_eq!(named(key, CTRL), None, "{key:?}");
			assert_eq!(named(key, CTRL_SHIFT), None, "{key:?}");
			assert_eq!(named(key, NONE), None, "{key:?}");
		}
		assert_eq!(named(NamedKey::Tab, CTRL), None);
		assert_eq!(named(NamedKey::Tab, CTRL_SHIFT), None);
		// the menu titles keep Alt plus their letter, and readline's Alt+b and
		// Alt+f are no more taken than before
		for (typed, title) in [("f", 'F'), ("p", 'P'), ("b", 'B')] {
			assert_eq!(chord(typed, ALT), Some(Hotkey::MenuTitle(title)));
		}
		assert_eq!(
			hotkey_on(&Key::Character("w".into()), ALT, false, false),
			None,
			"Alt+W with no menu bar"
		);
		for c in 'a'..='z' {
			let hotkey = chord(&c.to_string(), CTRL_SHIFT);
			match c {
				't' | 'w' | 'n' | 'c' | 'v' => assert!(hotkey.is_some(), "{c}"),
				_ => assert_eq!(hotkey, None, "Ctrl+Shift+{c}"),
			}
		}
	}

	// Only Alt alone makes a letter a menu title. Shift, Super or Ctrl held
	// with it is some other chord (AltGr arrives as Ctrl+Alt on Windows), so it
	// goes on to the shell.
	// Test ID: Erf6miU
	#[test]
	fn a_letter_opens_a_menu_title_only_with_alt_alone() {
		const ALT: ModifiersState = ModifiersState::ALT;
		let on = |mods| hotkey_on(&Key::Character("f".into()), mods, true, false);
		assert_eq!(on(ALT), Some(Hotkey::MenuTitle('F')));
		assert_eq!(
			hotkey_on(
				&Key::Character("F".into()),
				ALT.union(ModifiersState::SHIFT),
				true,
				false
			),
			None
		);
		for extra in [
			ModifiersState::SHIFT,
			COMMAND,
			CTRL,
			COMMAND.union(ModifiersState::SHIFT),
			COMMAND.union(CTRL),
		] {
			assert_eq!(on(ALT.union(extra)), None, "{extra:?}");
			assert!(!opens_menu_title(ALT.union(extra)), "{extra:?}");
		}
		assert!(opens_menu_title(ALT));
		assert!(!opens_menu_title(NONE));
	}

	// On a Mac the pane chords are iTerm2's: Command+D splits right,
	// Command+Shift+D splits down and Command+Option+arrows move. Command+W
	// closes the tab, so Option+Command+W closes the pane. Option+arrows and
	// the Alt+Shift chords go to the shell there.
	// Test ID: EreU3sa
	#[test]
	fn command_d_splits_and_command_option_arrows_move_on_macos() {
		const OPTION: ModifiersState = ModifiersState::ALT;
		let mac = |key: Key, mods| hotkey_on(&key, mods, false, true);
		let d = || Key::Character("d".into());
		assert_eq!(mac(d(), COMMAND), Some(Hotkey::SplitRight));
		assert_eq!(
			mac(
				Key::Character("D".into()),
				COMMAND.union(ModifiersState::SHIFT)
			),
			Some(Hotkey::SplitDown)
		);
		assert_eq!(mac(d(), CTRL), None);
		// Command holds back the character Option would type, so the press
		// arrives as "w"
		let w = || Key::Character("w".into());
		assert_eq!(mac(w(), COMMAND.union(OPTION)), Some(Hotkey::ClosePane));
		assert_eq!(mac(w(), COMMAND), Some(Hotkey::CloseTab));
		for (key, toward) in [
			(NamedKey::ArrowLeft, Toward::Left),
			(NamedKey::ArrowRight, Toward::Right),
			(NamedKey::ArrowUp, Toward::Up),
			(NamedKey::ArrowDown, Toward::Down),
		] {
			assert_eq!(
				mac(Key::Named(key), COMMAND.union(OPTION)),
				Some(Hotkey::Focus(toward))
			);
			assert_eq!(mac(Key::Named(key), OPTION), None, "{key:?}");
			assert_eq!(mac(Key::Named(key), COMMAND), None, "{key:?}");
		}
		let alt_shift = OPTION.union(ModifiersState::SHIFT);
		for typed in ["+", "_", "W"] {
			assert_eq!(
				mac(Key::Character(typed.into()), alt_shift),
				None,
				"{typed}"
			);
		}
	}

	// Test ID: Er2UiYI
	#[test]
	fn a_release_is_reported_only_for_the_button_held() {
		let left = Some(MouseBtn::Left);
		assert!(release_is_reported(left, MouseButton::Left));
		assert!(!release_is_reported(left, MouseButton::Middle));
		assert!(!release_is_reported(left, MouseButton::Right));
		assert!(!release_is_reported(None, MouseButton::Left));
		assert!(!release_is_reported(None, MouseButton::Back));
	}

	// A dropdown opens flush under the menu bar, over the tab-bar band.
	// Test ID: Er2UiYJ
	#[test]
	fn an_open_dropdown_keeps_a_press_in_the_tab_bar_band() {
		let (top, h) = (24.0, 30.0);
		assert!(tab_bar_takes_press(false, true, 30.0, top, h));
		assert!(!tab_bar_takes_press(true, true, 30.0, top, h));
		assert!(!tab_bar_takes_press(false, false, 30.0, top, h));
		assert!(!tab_bar_takes_press(false, true, top + h, top, h));
		assert!(!tab_bar_takes_press(false, true, top - 1.0, top, h));
	}

	// Test ID: Er2UiYK
	#[test]
	fn right_click_and_a_click_on_an_open_menu_stay_local() {
		assert!(press_is_reported(MouseBtn::Left, false, false));
		assert!(press_is_reported(MouseBtn::Middle, false, false));
		assert!(!press_is_reported(MouseBtn::Right, false, false));
		assert!(!press_is_reported(MouseBtn::Left, true, false));
		assert!(!press_is_reported(MouseBtn::Middle, true, false));
		assert!(!press_is_reported(MouseBtn::Left, false, true), "Shift");
	}

	// Alternate scroll is on by default, so the primary screen with it set is
	// the ordinary case, and it has to scroll the buffer.
	// Test ID: Er2UiYL
	#[test]
	fn the_wheel_scrolls_back_on_the_primary_screen() {
		let alt_scroll = TermMode::ALTERNATE_SCROLL;
		let alt_screen = TermMode::ALT_SCREEN | alt_scroll;
		assert_eq!(wheel_route(alt_scroll, false), WheelRoute::Scrollback);
		assert_eq!(
			wheel_route(TermMode::empty(), false),
			WheelRoute::Scrollback
		);
		assert_eq!(wheel_route(alt_screen, false), WheelRoute::CursorKeys);
		assert_eq!(
			wheel_route(TermMode::ALT_SCREEN, false),
			WheelRoute::Scrollback,
			"no alternate scroll"
		);
		for tracking in [
			TermMode::MOUSE_REPORT_CLICK,
			TermMode::MOUSE_DRAG,
			TermMode::MOUSE_MOTION,
		] {
			assert_eq!(
				wheel_route(alt_screen | tracking, false),
				WheelRoute::Report
			);
			assert_eq!(wheel_route(tracking, false), WheelRoute::Report);
		}
	}

	// Test ID: Er2UiYM
	#[test]
	fn shift_keeps_the_wheel_local_over_a_tracking_app() {
		let tracking = TermMode::MOUSE_REPORT_CLICK;
		assert_eq!(wheel_route(tracking, true), WheelRoute::Scrollback);
		assert_eq!(
			wheel_route(
				tracking | TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL,
				true
			),
			WheelRoute::Scrollback
		);
		// with no tracking app, Shift changes nothing
		let alt_screen = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
		assert_eq!(wheel_route(alt_screen, true), WheelRoute::CursorKeys);
	}

	// Test ID: Er2UiYN
	#[test]
	fn an_armed_tab_close_fires_only_on_its_own_box() {
		let close_x = Some(100.0);
		assert!(close_on_release(2, 110.0, true, close_x, Some(2)));
		// dragged off the box, but still on the tab
		assert!(!close_on_release(2, 90.0, true, close_x, Some(2)));
		// over the next tab's box
		assert!(!close_on_release(2, 130.0, true, close_x, Some(3)));
		// below the bar
		assert!(!close_on_release(2, 110.0, false, close_x, Some(2)));
		// the tab is gone
		assert!(!close_on_release(2, 110.0, true, None, None));
	}

	// Test ID: Er2UiYO
	#[test]
	fn clicks_count_to_three_on_one_spot_then_start_over() {
		let cell = (8.0, 16.0);
		let t0 = Instant::now();
		let soon = t0 + Duration::from_millis(150);
		let at = (40.0, 40.0);
		assert_eq!(click_count(None, 0, t0, at, cell), 1);
		let last = Some((t0, at.0, at.1));
		assert_eq!(click_count(last, 1, soon, at, cell), 2);
		assert_eq!(click_count(last, 2, soon, at, cell), 3);
		assert_eq!(click_count(last, 3, soon, at, cell), 1, "a fourth wraps");
		// too late, or more than a cell away, starts a new run
		assert_eq!(click_count(last, 1, t0 + MULTI_CLICK, at, cell), 1);
		assert_eq!(click_count(last, 1, soon, (at.0 + 9.0, at.1), cell), 1);
		assert_eq!(click_count(last, 1, soon, (at.0, at.1 - 17.0), cell), 1);
		assert_eq!(
			click_count(last, 1, soon, (at.0 + 8.0, at.1 + 16.0), cell),
			2
		);
	}

	// Test ID: Er2UiYP
	#[test]
	fn ctrl_selects_a_block_but_only_on_a_single_click() {
		assert_eq!(click_select(1, false), ClickSelect::Run);
		assert_eq!(click_select(1, true), ClickSelect::Block);
		assert_eq!(click_select(2, false), ClickSelect::Word);
		assert_eq!(click_select(2, true), ClickSelect::Word);
		assert_eq!(click_select(3, false), ClickSelect::Line);
		assert_eq!(click_select(3, true), ClickSelect::Line);
	}

	// Both regressed by waiting on the window-focus flag, which can lag the
	// window manager.
	// Test ID: Er2UiYQ
	#[test]
	fn a_copy_the_user_drives_does_not_wait_on_window_focus() {
		for from in [CopyFrom::Chord, CopyFrom::Select] {
			assert!(copy_allowed(from, false, true));
			assert!(copy_allowed(from, true, true));
		}
	}

	// Test ID: Er2UiYR
	#[test]
	fn a_program_sets_the_clipboard_only_from_the_pane_in_use() {
		assert!(copy_allowed(CopyFrom::Program, true, true));
		assert!(copy_allowed(CopyFrom::Program, false, true));
		assert!(!copy_allowed(CopyFrom::Program, true, false));
	}

	// A capture that is not allowed is disarmed, so output that finished while
	// the user was elsewhere never copies on the way back.
	// Test ID: Er2UiYS
	#[test]
	fn output_copies_only_from_the_pane_in_use_of_a_focused_window() {
		assert!(copy_allowed(CopyFrom::Output, true, true));
		assert!(!copy_allowed(CopyFrom::Output, false, true));
		assert!(!copy_allowed(CopyFrom::Output, true, false));
	}
}
