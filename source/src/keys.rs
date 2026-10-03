// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The hotkeys: the name each one has in the config file, its default chords on
// each platform, and the bindings in force once the config has had its say.
// The key handler, the in-window menus and the macOS menu bar all read one
// `Bindings`, so a rebinding shows up in every one of them.

use std::fmt::Write as _;

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::input::Hotkey;
use crate::pane::Toward;

/// The key a chord is pressed on, as opposed to the ones held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyName {
	/// A key that types a character; a letter is kept lowercase.
	Char(char),
	/// Spelled as a word, since `+` joins the parts of a chord. `=` counts too,
	/// since "+" is Shift+"=" on most layouts.
	Plus,
	Minus,
	Named(NamedKey),
}

// The words a chord spells its named keys with. The first word for a key is
// the one a menu shows.
#[rustfmt::skip]
const NAMED_KEYS: &[(&str, NamedKey)] = &[
	("Left", NamedKey::ArrowLeft),
	("Right", NamedKey::ArrowRight),
	("Up", NamedKey::ArrowUp),
	("Down", NamedKey::ArrowDown),
	("PageUp", NamedKey::PageUp),
	("PageDown", NamedKey::PageDown),
	("PgUp", NamedKey::PageUp),
	("PgDn", NamedKey::PageDown),
	("Home", NamedKey::Home),
	("End", NamedKey::End),
	("Insert", NamedKey::Insert),
	("Delete", NamedKey::Delete),
	("Del", NamedKey::Delete),
	("Backspace", NamedKey::Backspace),
	("Tab", NamedKey::Tab),
	("Enter", NamedKey::Enter),
	("Escape", NamedKey::Escape),
	("Esc", NamedKey::Escape),
	("Space", NamedKey::Space),
	("Menu", NamedKey::ContextMenu),
	("F1", NamedKey::F1),
	("F2", NamedKey::F2),
	("F3", NamedKey::F3),
	("F4", NamedKey::F4),
	("F5", NamedKey::F5),
	("F6", NamedKey::F6),
	("F7", NamedKey::F7),
	("F8", NamedKey::F8),
	("F9", NamedKey::F9),
	("F10", NamedKey::F10),
	("F11", NamedKey::F11),
	("F12", NamedKey::F12),
];

impl KeyName {
	fn parse(word: &str) -> Option<KeyName> {
		if word.eq_ignore_ascii_case("plus") {
			return Some(KeyName::Plus);
		}
		if word.eq_ignore_ascii_case("minus") || word == "-" {
			return Some(KeyName::Minus);
		}
		let mut chars = word.chars();
		if let (Some(ch), None) = (chars.next(), chars.next()) {
			return (ch != '+').then(|| KeyName::Char(ch.to_ascii_lowercase()));
		}
		NAMED_KEYS
			.iter()
			.find(|(name, _)| name.eq_ignore_ascii_case(word))
			.map(|(_, named)| KeyName::Named(*named))
	}

	fn word(self) -> String {
		match self {
			KeyName::Char(ch) => ch.to_uppercase().collect(),
			KeyName::Plus => "Plus".into(),
			KeyName::Minus => "Minus".into(),
			KeyName::Named(named) => NAMED_KEYS
				.iter()
				.find(|(_, each)| *each == named)
				.map_or_else(|| format!("{named:?}"), |(name, _)| (*name).into()),
		}
	}

	// A function key or the Menu key does nothing in a shell by itself, so it
	// may be a hotkey alone. Any other key would stop typing.
	fn free_alone(self) -> bool {
		matches!(
			self,
			KeyName::Named(
				NamedKey::F1
					| NamedKey::F2 | NamedKey::F3
					| NamedKey::F4 | NamedKey::F5
					| NamedKey::F6 | NamedKey::F7
					| NamedKey::F8 | NamedKey::F9
					| NamedKey::F10 | NamedKey::F11
					| NamedKey::F12 | NamedKey::ContextMenu
			)
		)
	}
}

/// What Shift makes of a key on a US layout, for a chord that holds Shift on a
/// sign or a digit. A Mac names a Command+Shift press by that character, "{"
/// for Shift+[.
pub fn us_shifted(key: char) -> Option<&'static str> {
	Some(match key {
		'`' => "~",
		'1' => "!",
		'2' => "@",
		'3' => "#",
		'4' => "$",
		'5' => "%",
		'6' => "^",
		'7' => "&",
		'8' => "*",
		'9' => "(",
		'0' => ")",
		'=' => "+",
		'[' => "{",
		']' => "}",
		'\\' => "|",
		';' => ":",
		'\'' => "\"",
		',' => "<",
		'.' => ">",
		'/' => "?",
		_ => return None,
	})
}

/// One key combination: the keys held, and the key pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
	pub key: KeyName,
	pub ctrl: bool,
	/// Option on a Mac
	pub alt: bool,
	pub shift: bool,
	/// Command on a Mac, the Super or Windows key elsewhere
	pub command: bool,
}

impl Chord {
	/// Read a chord as the config file writes it: the keys held, then the key
	/// pressed, joined by "+", in any order and any case. "Ctrl+Shift+T".
	pub fn parse(text: &str) -> Result<Chord, String> {
		let parts: Vec<&str> = text.split('+').collect();
		let Some((last, held)) = parts.split_last() else {
			return Err(format!("\"{text}\" names no key"));
		};
		let mut chord = Chord {
			key: KeyName::Plus,
			ctrl: false,
			alt: false,
			shift: false,
			command: false,
		};
		for part in held {
			match part.to_ascii_lowercase().as_str() {
				"ctrl" | "control" => chord.ctrl = true,
				"alt" | "option" => chord.alt = true,
				"shift" => chord.shift = true,
				"command" | "cmd" | "super" | "win" => chord.command = true,
				"" => return Err(format!("\"{text}\": write the + key as Plus")),
				_ => {
					return Err(format!(
						"\"{text}\": {part} is not Ctrl, Alt, Shift or Command"
					));
				}
			}
		}
		if last.is_empty() {
			return Err(format!("\"{text}\": write the + key as Plus"));
		}
		chord.key =
			KeyName::parse(last).ok_or_else(|| format!("\"{text}\": {last} is not a key name"))?;
		if !(chord.ctrl || chord.alt || chord.command || chord.key.free_alone()) {
			return Err(format!(
				"\"{text}\" needs Ctrl, Alt or Command held, or the key would stop reaching the shell"
			));
		}
		Ok(chord)
	}

	/// How a menu shows the chord, and how the config file writes it back. A
	/// Mac puts the modifiers in Apple's order and names them its own way:
	/// `Control+Command+F`. Elsewhere it is `Ctrl+Alt+Shift+T`.
	pub fn spoken(self, mac: bool) -> String {
		let held = if mac {
			[
				(self.ctrl, "Control"),
				(self.alt, "Option"),
				(self.shift, "Shift"),
				(self.command, "Command"),
			]
		} else {
			[
				(self.ctrl, "Ctrl"),
				(self.alt, "Alt"),
				(self.shift, "Shift"),
				(self.command, "Super"),
			]
		};
		let mut out = String::new();
		for (on, name) in held {
			if on {
				out.push_str(name);
				out.push('+');
			}
		}
		out.push_str(&self.key.word());
		out
	}

	// Whether a press is this chord. `loose` lets a chord with no Shift take a
	// press with Shift held, on a key some layouts need Shift to reach: "+",
	// "-" or a digit, as on a French layout. Never a letter or another sign,
	// and never "_", which is Ctrl+_ undo in the shell rather than Ctrl+Minus.
	fn matches(self, key: &Key, mods: ModifiersState, loose: bool) -> bool {
		if mods.control_key() != self.ctrl
			|| mods.alt_key() != self.alt
			|| mods.super_key() != self.command
		{
			return false;
		}
		let shift_ok = mods.shift_key() == self.shift;
		let shift_loose = shift_ok || (loose && !self.shift);
		match (self.key, key) {
			(KeyName::Named(want), Key::Named(got)) => want == *got && shift_ok,
			(KeyName::Plus, Key::Character(typed)) => (typed == "+" || typed == "=") && shift_loose,
			(KeyName::Minus, Key::Character(typed)) => {
				(typed == "-" && shift_loose) || (typed == "_" && self.shift && shift_ok)
			}
			(KeyName::Char(want), Key::Character(typed)) => {
				let mut chars = typed.chars();
				let same = chars
					.next()
					.is_some_and(|ch| ch.to_lowercase().eq(want.to_lowercase()))
					&& chars.next().is_none();
				let shift_fits = shift_ok || (shift_loose && want.is_ascii_digit());
				(same && shift_fits)
					|| (self.shift && shift_ok && us_shifted(want) == Some(typed.as_str()))
			}
			_ => false,
		}
	}
}

/// Read a hotkey's value from the config file: one or more chords separated by
/// spaces, or "none" for no chord at all.
pub fn parse_value(text: &str) -> Result<Vec<Chord>, String> {
	let text = text.trim();
	if text.eq_ignore_ascii_case("none") {
		return Ok(Vec::new());
	}
	text.split_whitespace().map(Chord::parse).collect()
}

/// A hotkey's value as the config file writes it.
pub fn value_text(chords: &[Chord], mac: bool) -> String {
	if chords.is_empty() {
		return "none".into();
	}
	chords
		.iter()
		.map(|chord| chord.spoken(mac))
		.collect::<Vec<_>>()
		.join(" ")
}

// Every hotkey there is: its name under `keys:` in the config file, then its
// default chords on Linux and Windows, then on macOS. On a Mac the chords are
// Apple's standard ones where an action has one; the pane chords are iTerm2's.
// Elsewhere the pane chords are Windows Terminal's. Groups in the template
// follow the blank-line breaks in `TEMPLATE_GROUPS`.
#[rustfmt::skip]
const TABLE: &[(Hotkey, &str, &str, &str)] = &[
	(Hotkey::Copy,                      "copy",             "Ctrl+Shift+C",         "Command+C"),
	(Hotkey::Paste,                     "paste",            "Ctrl+Shift+V",         "Command+V"),
	(Hotkey::NewTab,                    "new_tab",          "Ctrl+Shift+T",         "Command+T"),
	(Hotkey::CloseTab,                  "close_tab",        "Ctrl+Shift+W Ctrl+F4", "Command+W"),
	(Hotkey::PrevTab,                   "previous_tab",     "Ctrl+PageUp",          "Shift+Command+[ Command+PageUp"),
	(Hotkey::NextTab,                   "next_tab",         "Ctrl+PageDown",        "Shift+Command+] Command+PageDown"),
	(Hotkey::MoveTab { forward: false }, "move_tab_back",   "Ctrl+Shift+PageUp",    "Shift+Command+PageUp"),
	(Hotkey::MoveTab { forward: true }, "move_tab_forward", "Ctrl+Shift+PageDown",  "Shift+Command+PageDown"),
	(Hotkey::SplitRight,                "split_right",      "Alt+Shift+Plus",       "Command+D"),
	(Hotkey::SplitDown,                 "split_down",       "Alt+Shift+Minus",      "Shift+Command+D"),
	(Hotkey::ClosePane,                 "close_pane",       "Alt+Shift+W",          "none"),
	(Hotkey::Focus(Toward::Left),       "focus_left",       "Alt+Left",             "Option+Command+Left"),
	(Hotkey::Focus(Toward::Right),      "focus_right",      "Alt+Right",            "Option+Command+Right"),
	(Hotkey::Focus(Toward::Up),         "focus_up",         "Alt+Up",               "Option+Command+Up"),
	(Hotkey::Focus(Toward::Down),       "focus_down",       "Alt+Down",             "Option+Command+Down"),
	(Hotkey::Zoom(1),                   "font_bigger",      "Ctrl+Plus",            "Command+Plus"),
	(Hotkey::Zoom(-1),                  "font_smaller",     "Ctrl+Minus",           "Command+Minus"),
	(Hotkey::ZoomReset,                 "font_reset",       "Ctrl+0",               "Command+0"),
	(Hotkey::Fullscreen,                "fullscreen",       "F11",                  "Control+Command+F F11"),
	(Hotkey::ContextMenu,               "context_menu",     "Menu",                 "Menu"),
	(Hotkey::NewWindow,                 "new_window",       "Ctrl+Shift+N",         "Command+N"),
	(Hotkey::Settings,                  "settings",         "Ctrl+,",               "Command+,"),
	(Hotkey::Quit,                      "quit",             "none",                 "Command+Q"),
];

// Where the template puts a blank line: before each of these names.
const TEMPLATE_GROUPS: &[&str] = &["new_tab", "split_right", "font_bigger", "new_window"];

/// The config path of a hotkey, `keys.split_right`, if it can be bound.
pub fn config_path(hotkey: Hotkey) -> Option<String> {
	TABLE
		.iter()
		.find(|(each, ..)| *each == hotkey)
		.map(|(_, name, ..)| format!("keys.{name}"))
}

/// Every hotkey that can be bound, with its config path, in table order.
pub fn config_paths() -> impl Iterator<Item = (Hotkey, String)> {
	TABLE
		.iter()
		.map(|(hotkey, name, ..)| (*hotkey, format!("keys.{name}")))
}

fn default_text(
	row: &(Hotkey, &'static str, &'static str, &'static str),
	mac: bool,
) -> &'static str {
	if mac { row.3 } else { row.2 }
}

/// The `keys:` block of the shipped template, every line at its default.
pub fn template_lines(mac: bool) -> String {
	let mut out = String::new();
	for row in TABLE {
		if TEMPLATE_GROUPS.contains(&row.1) {
			out.push('\n');
		}
		// writing to a String cannot fail
		let _ = writeln!(
			out,
			"\t# {}: \"{}\"  ## Default",
			row.1,
			default_text(row, mac)
		);
	}
	out
}

/// The hotkeys in force: the defaults, less any the config turned off or moved,
/// plus whatever it added. A chord answers to one hotkey only.
#[derive(Debug, Clone, PartialEq)]
pub struct Bindings {
	// in table order
	list: Vec<(Hotkey, Vec<Chord>)>,
}

impl Bindings {
	pub fn defaults(mac: bool) -> Self {
		Self::with(mac, &[]).0
	}

	/// The defaults with the config file's own values put in. A chord set for
	/// one hotkey is taken off any other that has it by default, and two set
	/// for the same chord leave it with the first. Also answers what to tell
	/// the user about either.
	pub fn with(mac: bool, set: &[(Hotkey, Vec<Chord>)]) -> (Self, Vec<String>) {
		// (hotkey, chords, whether the file set them)
		let mut list: Vec<(Hotkey, Vec<Chord>, bool)> = TABLE
			.iter()
			.map(
				|row| match set.iter().find(|(hotkey, _)| *hotkey == row.0) {
					Some((_, chords)) => (row.0, chords.clone(), true),
					// the table is checked by test, so a failed parse cannot happen
					None => (
						row.0,
						parse_value(default_text(row, mac)).unwrap_or_default(),
						false,
					),
				},
			)
			.collect();
		let mut notes = Vec::new();
		let path = |hotkey| config_path(hotkey).unwrap_or_default();
		// the chords the file set are claimed first, then the defaults, in order
		let mut taken: Vec<(Chord, Hotkey, bool)> = Vec::new();
		for pass_set in [true, false] {
			for (hotkey, chords, from_file) in list.iter_mut().filter(|b| b.2 == pass_set) {
				chords.retain(|chord| {
					let Some((_, owner, owner_set)) = taken.iter().find(|(c, ..)| c == chord)
					else {
						taken.push((*chord, *hotkey, *from_file));
						return true;
					};
					let spoken = chord.spoken(mac);
					notes.push(if pass_set && *owner_set {
						format!(
							"`{}` and `{}` both use {spoken} - only `{}` answers to it",
							path(*owner),
							path(*hotkey),
							path(*owner)
						)
					} else {
						format!(
							"{spoken} is set for `{}`, so `{}` no longer answers to it",
							path(*owner),
							path(*hotkey)
						)
					});
					false
				});
			}
		}
		let list = list
			.into_iter()
			.map(|(hotkey, chords, _)| (hotkey, chords))
			.collect();
		(Self { list }, notes)
	}

	/// The hotkey a press is, if any. A press with exactly the chord's keys held
	/// wins over one that only fits loosely.
	pub fn hotkey(&self, key: &Key, mods: ModifiersState) -> Option<Hotkey> {
		let find = |loose| {
			self.list
				.iter()
				.find(|(_, chords)| chords.iter().any(|c| c.matches(key, mods, loose)))
				.map(|(hotkey, _)| *hotkey)
		};
		find(false).or_else(|| find(true))
	}

	/// Every chord a hotkey answers to, the one a menu shows first.
	pub fn chords(&self, hotkey: Hotkey) -> &[Chord] {
		self.list
			.iter()
			.find(|(each, _)| *each == hotkey)
			.map_or(&[], |(_, chords)| chords.as_slice())
	}

	/// The chord a menu row shows for a hotkey.
	pub fn shown(&self, hotkey: Hotkey) -> Option<Chord> {
		self.chords(hotkey).first().copied()
	}
}

/// What is wrong with the `keys:` values in a config, each with the line it is
/// on. `value` answers a path's raw text and its line numbers.
pub fn complaints(
	mac: bool,
	value: impl Fn(&str) -> Option<(String, Vec<usize>)>,
	cite: impl Fn(&[usize]) -> String,
) -> Vec<String> {
	let mut out = Vec::new();
	let mut set = Vec::new();
	for (hotkey, path) in config_paths() {
		let Some((text, lines)) = value(&path) else {
			continue;
		};
		if text.trim().is_empty() {
			continue;
		}
		match parse_value(&text) {
			Ok(chords) => set.push((hotkey, chords)),
			Err(why) => out.push(format!("`{path}`{} is not used - {why}", cite(&lines))),
		}
	}
	out.extend(Bindings::with(mac, &set).1);
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	const NONE: ModifiersState = ModifiersState::empty();
	const ALT_SHIFT: ModifiersState = ModifiersState::ALT.union(ModifiersState::SHIFT);

	fn chord(text: &str) -> Chord {
		Chord::parse(text).expect(text)
	}

	fn typed(text: &str) -> Key {
		Key::Character(text.into())
	}

	// Every default in the table reads back, on both platforms, and no chord is
	// given to two hotkeys, since only one of them could ever answer to it.
	// Test ID: EreU3sR
	#[test]
	fn every_default_chord_reads_and_none_is_shared() {
		for mac in [false, true] {
			for row in TABLE {
				let text = default_text(row, mac);
				let chords = parse_value(text).unwrap_or_else(|e| panic!("{}: {e}", row.1));
				assert_eq!(
					value_text(&chords, mac),
					text,
					"{} reads back as written",
					row.1
				);
			}
			let (bindings, notes) = Bindings::with(mac, &[]);
			assert!(notes.is_empty(), "{notes:?}");
			for (hotkey, _) in config_paths() {
				if !(mac && hotkey == Hotkey::ClosePane || !mac && hotkey == Hotkey::Quit) {
					assert!(bindings.shown(hotkey).is_some(), "{hotkey:?} mac={mac}");
				}
			}
		}
	}

	// The spellings the template comment promises: any order, any case, the
	// Mac's names for the keys held, and the named keys as words.
	// Test ID: EreU3sS
	#[test]
	fn a_chord_reads_in_any_order_and_case() {
		let t = chord("Ctrl+Shift+T");
		assert_eq!(chord("shift+ctrl+t"), t);
		assert_eq!(chord("CONTROL+Shift+T"), t);
		assert_eq!(t.spoken(false), "Ctrl+Shift+T");
		assert_eq!(t.spoken(true), "Control+Shift+T");
		assert_eq!(chord("Option+Command+Left"), chord("Alt+Cmd+left"));
		assert_eq!(chord("Alt+Super+Left").spoken(true), "Option+Command+Left");
		assert_eq!(chord("Alt+Shift+Plus").key, KeyName::Plus);
		assert_eq!(chord("Alt+Shift+-").key, KeyName::Minus);
		assert_eq!(chord("Ctrl+PgDn").spoken(false), "Ctrl+PageDown");
		assert_eq!(chord("F11").spoken(false), "F11");
		assert_eq!(chord("Menu").key, KeyName::Named(NamedKey::ContextMenu));
		assert_eq!(chord("Ctrl+,").spoken(false), "Ctrl+,");
		assert_eq!(parse_value("NONE"), Ok(Vec::new()));
		assert_eq!(
			parse_value("  Ctrl+Shift+W   Ctrl+F4 ").map(|c| value_text(&c, false)),
			Ok("Ctrl+Shift+W Ctrl+F4".into())
		);
	}

	// A bad value names what is wrong with it, and a key that would stop
	// typing at the shell is refused.
	// Test ID: EreU3sT
	#[test]
	fn a_bad_chord_says_what_is_wrong() {
		for (text, says) in [
			("Ctrl++", "Plus"),
			("Ctrl+", "Plus"),
			("Ctlr+T", "is not Ctrl, Alt, Shift or Command"),
			("Ctrl+Shift+Bogus", "is not a key name"),
			("T", "needs Ctrl, Alt or Command"),
			("Shift+T", "needs Ctrl, Alt or Command"),
			("Left", "needs Ctrl, Alt or Command"),
			("Shift+Enter", "needs Ctrl, Alt or Command"),
		] {
			let got = Chord::parse(text).expect_err(text);
			assert!(got.contains(says), "{text}: {got}");
		}
		assert!(parse_value("Ctrl+Shift+W Ctrl+Bogus").is_err());
		assert!(Chord::parse("Shift+F10").is_ok());
	}

	// Alt+Shift+Plus is Alt+Shift+= on a US layout, which names the key "+";
	// Alt+Shift+Minus names it "_". Plain Alt+= is not the chord.
	// Test ID: EreU3sU
	#[test]
	fn plus_and_minus_take_what_shift_makes_of_them() {
		let b = Bindings::defaults(false);
		assert_eq!(b.hotkey(&typed("+"), ALT_SHIFT), Some(Hotkey::SplitRight));
		assert_eq!(b.hotkey(&typed("="), ALT_SHIFT), Some(Hotkey::SplitRight));
		assert_eq!(b.hotkey(&typed("_"), ALT_SHIFT), Some(Hotkey::SplitDown));
		assert_eq!(b.hotkey(&typed("-"), ALT_SHIFT), Some(Hotkey::SplitDown));
		assert_eq!(b.hotkey(&typed("="), ModifiersState::ALT), None);
		assert_eq!(b.hotkey(&typed("-"), ModifiersState::ALT), None);
		// Ctrl+Plus needs no Shift, but takes one, since "+" is often shifted
		let ctrl = ModifiersState::CONTROL;
		let ctrl_shift = ctrl.union(ModifiersState::SHIFT);
		assert_eq!(b.hotkey(&typed("="), ctrl), Some(Hotkey::Zoom(1)));
		assert_eq!(b.hotkey(&typed("+"), ctrl_shift), Some(Hotkey::Zoom(1)));
		assert_eq!(b.hotkey(&typed("-"), ctrl), Some(Hotkey::Zoom(-1)));
		// Ctrl+_ is undo in the shell, never Ctrl+Minus
		assert_eq!(b.hotkey(&typed("_"), ctrl_shift), None);
		// a digit that needs Shift on some layouts, as 0 does on French ones
		assert_eq!(b.hotkey(&typed("0"), ctrl_shift), Some(Hotkey::ZoomReset));
		// a letter never ignores Shift
		assert_eq!(b.hotkey(&typed("t"), ctrl), None);
	}

	// A chord set in the file wins over a default that has it, and the default
	// stops showing it on its menu row. Two set the same way leave it with the
	// first, and both are reported.
	// Test ID: EreU3sV
	#[test]
	fn a_chord_set_in_the_file_takes_it_from_a_default() {
		let ctrl_shift = ModifiersState::CONTROL.union(ModifiersState::SHIFT);
		let (b, notes) = Bindings::with(false, &[(Hotkey::ClosePane, vec![chord("Ctrl+Shift+W")])]);
		assert_eq!(b.hotkey(&typed("W"), ctrl_shift), Some(Hotkey::ClosePane));
		assert_eq!(b.shown(Hotkey::CloseTab), Some(chord("Ctrl+F4")));
		assert_eq!(
			b.hotkey(&typed("W"), ALT_SHIFT),
			None,
			"the old one is gone"
		);
		assert_eq!(
			notes,
			[
				"Ctrl+Shift+W is set for `keys.close_pane`, so `keys.close_tab` no longer answers to it"
			]
		);
		let (b, notes) = Bindings::with(
			false,
			&[
				(Hotkey::SplitDown, vec![chord("Alt+D")]),
				(Hotkey::SplitRight, vec![chord("Alt+D")]),
			],
		);
		assert_eq!(
			b.hotkey(&typed("d"), ModifiersState::ALT),
			Some(Hotkey::SplitRight)
		);
		assert_eq!(b.shown(Hotkey::SplitDown), None);
		assert_eq!(notes.len(), 1, "{notes:?}");
		assert!(notes[0].contains("both use Alt+D"), "{notes:?}");
		// turning one off frees its chord and touches nothing else
		let (b, notes) = Bindings::with(false, &[(Hotkey::SplitRight, Vec::new())]);
		assert!(notes.is_empty());
		assert_eq!(b.hotkey(&typed("+"), ALT_SHIFT), None);
		assert_eq!(b.shown(Hotkey::SplitDown), Some(chord("Alt+Shift+Minus")));
	}

	// A bad value is reported with its line and leaves the default in place,
	// rather than being dropped in silence.
	// Test ID: EreU3sW
	#[test]
	fn a_bad_value_is_reported_with_its_line() {
		let file = |path: &str| match path {
			"keys.split_right" => Some(("Alt+Shift+Plsu".to_string(), vec![7])),
			"keys.new_tab" => Some(("Ctrl+T".to_string(), vec![9])),
			"keys.paste" => Some((String::new(), vec![3])),
			_ => None,
		};
		let cite = |lines: &[usize]| format!(" line {}", lines[0]);
		let said = complaints(false, file, cite);
		assert_eq!(said.len(), 1, "{said:?}");
		assert!(
			said[0].starts_with("`keys.split_right` line 7 is not used"),
			"{said:?}"
		);
		assert!(said[0].contains("Plsu is not a key name"), "{said:?}");
	}

	// Test ID: EreU3sX
	#[test]
	fn a_named_key_ignores_nothing_held() {
		let b = Bindings::defaults(false);
		let left = Key::Named(NamedKey::ArrowLeft);
		assert_eq!(
			b.hotkey(&left, ModifiersState::ALT),
			Some(Hotkey::Focus(Toward::Left))
		);
		assert_eq!(b.hotkey(&left, ALT_SHIFT), None);
		assert_eq!(b.hotkey(&left, NONE), None);
		assert_eq!(b.hotkey(&left, ModifiersState::CONTROL), None);
		assert_eq!(
			b.hotkey(&Key::Named(NamedKey::F11), ModifiersState::SHIFT),
			None
		);
	}
}
