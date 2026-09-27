// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use alacritty_terminal::term::TermMode;
use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

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
}
