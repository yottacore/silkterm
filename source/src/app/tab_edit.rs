// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// What a typed tab title is worth keeping, given what the tab would say on its
// own. Blank, or the same as the automatic name, means no override at all -
// both are the way back to a tab that names itself.
fn typed_title(typed: String, auto: &str) -> Option<String> {
	(!typed.trim().is_empty() && typed != auto).then_some(typed)
}

// A tab title being typed in place. `caret` and `anchor` are byte offsets into
// `text`; equal means no selection. Committing text that matches what the tab
// would have said on its own puts it back to naming the shell. It edits the way
// a Settings text box does, from the same keys and the same caret arithmetic.
struct TabEdit {
	tab: usize,
	text: String,
	caret: usize,
	anchor: usize,
	// a press in the box is held, so moving drags the selection
	dragging: bool,
	// the last press on the box and how many came in a row: two take a word,
	// three the whole name
	clicks: Option<(Instant, u32)>,
}

impl TabEdit {
	// All of it selected, so the first thing typed replaces it.
	fn new(tab: usize, text: String) -> TabEdit {
		TabEdit {
			tab,
			caret: text.len(),
			anchor: 0,
			text,
			dragging: false,
			clicks: None,
		}
	}

	fn range(&self) -> (usize, usize) {
		(self.caret.min(self.anchor), self.caret.max(self.anchor))
	}

	fn selected(&self) -> Option<&str> {
		let (from, to) = self.range();
		(from != to).then(|| &self.text[from..to])
	}

	fn select_all(&mut self) {
		self.anchor = 0;
		self.caret = self.text.len();
	}

	// Replace the selection (or insert at the caret) and leave the caret after it.
	fn insert(&mut self, typed: &str) {
		let (from, to) = self.range();
		self.text.replace_range(from..to, typed);
		self.caret = from + typed.len();
		self.anchor = self.caret;
	}

	// A tab is one line high, so a pasted line break or tab becomes a space and
	// any other control character is dropped.
	fn paste(&mut self, text: &str) {
		let flat: String = text
			.replace("\r\n", " ")
			.chars()
			.filter_map(|c| match c {
				'\n' | '\r' | '\t' => Some(' '),
				c if c.is_control() => None,
				c => Some(c),
			})
			.collect();
		self.insert(&flat);
	}

	// Backspace (back = true) or Delete, as far as `reach`. With a selection,
	// either just clears it.
	fn erase(&mut self, back: bool, reach: Reach) {
		let (mut from, mut to) = self.range();
		if from == to {
			if back {
				from = reach_left(&self.text, self.caret, reach);
			} else {
				to = reach_right(&self.text, self.caret, reach);
			}
		}
		self.text.replace_range(from..to, "");
		self.caret = from;
		self.anchor = from;
	}

	// Move the caret and either drag the selection with it or drop it. A plain
	// step out of a selection lands on that side of it.
	fn move_caret(&mut self, to: Caret, select: bool) {
		let (from, end) = self.range();
		let collapse = !select && from != end;
		self.caret = match to {
			Caret::Left(reach) if collapse && reach != Reach::End => from,
			Caret::Right(reach) if collapse && reach != Reach::End => end,
			Caret::Left(reach) => reach_left(&self.text, self.caret, reach),
			Caret::Right(reach) => reach_right(&self.text, self.caret, reach),
		};
		if !select {
			self.anchor = self.caret;
		}
	}

	// A press in the box at byte offset `at`. Shift carries the selection there;
	// otherwise one press places the caret, two take the word and three the
	// whole name.
	fn press(&mut self, at: usize, now: Instant, extend: bool) {
		let clicks = match self.clicks {
			Some((when, n)) if now.duration_since(when) < TAB_DBL_CLICK => n + 1,
			_ => 1,
		};
		self.clicks = Some((now, clicks));
		if extend {
			self.caret = at;
			self.dragging = true;
			return;
		}
		match clicks {
			1 => {
				self.caret = at;
				self.anchor = at;
				self.dragging = true;
			}
			2 => {
				(self.anchor, self.caret) = word_at(&self.text, at);
			}
			_ => self.select_all(),
		}
	}

	fn drag_to(&mut self, at: usize) {
		if self.dragging {
			self.caret = at;
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caret {
	Left(Reach),
	Right(Reach),
}

// What a key does to a tab being renamed.
#[derive(Debug, PartialEq, Eq)]
enum TabEditKey {
	Commit,
	Cancel,
	Menu,
	Edit(EditCmd),
	Type(String),
	Erase { back: bool, reach: Reach },
	Move { to: Caret, select: bool },
}

// A rename takes every key while it is up, and reads them as a Settings text
// box does (`input::edit_keys`): Ctrl for the shortcuts and the words, or on a
// Mac Command for the shortcuts and either end, and Option for the words.
fn tab_edit_key(key: &Key, keys: input::EditKeys, selected: bool) -> Option<TabEditKey> {
	let reach = Reach::new(keys.word, keys.line);
	let types = keys.types && !keys.alt;
	Some(match key {
		// Tab commits too - there is nowhere for it to move to.
		Key::Named(NamedKey::Enter | NamedKey::Tab) => TabEditKey::Commit,
		Key::Named(NamedKey::Escape) => TabEditKey::Cancel,
		Key::Named(NamedKey::ContextMenu) => TabEditKey::Menu,
		Key::Named(NamedKey::F10) if keys.shift => TabEditKey::Menu,
		Key::Named(NamedKey::Backspace) => TabEditKey::Erase { back: true, reach },
		Key::Named(NamedKey::Delete) if keys.shift && selected => TabEditKey::Edit(EditCmd::Cut),
		Key::Named(NamedKey::Delete) => TabEditKey::Erase {
			back: false,
			reach: Reach::new(keys.word, false),
		},
		Key::Named(NamedKey::Insert) if keys.shift => TabEditKey::Edit(EditCmd::Paste),
		Key::Named(NamedKey::Insert) if keys.shortcut => TabEditKey::Edit(EditCmd::Copy),
		Key::Named(NamedKey::ArrowLeft) => TabEditKey::Move {
			to: Caret::Left(reach),
			select: keys.shift,
		},
		Key::Named(NamedKey::ArrowRight) => TabEditKey::Move {
			to: Caret::Right(reach),
			select: keys.shift,
		},
		Key::Named(NamedKey::Home) => TabEditKey::Move {
			to: Caret::Left(Reach::End),
			select: keys.shift,
		},
		Key::Named(NamedKey::End) => TabEditKey::Move {
			to: Caret::Right(Reach::End),
			select: keys.shift,
		},
		Key::Named(NamedKey::Space) if types => TabEditKey::Type(" ".into()),
		Key::Character(typed) if keys.shortcut => {
			TabEditKey::Edit(match typed.to_ascii_lowercase().as_str() {
				"a" => EditCmd::SelectAll,
				"c" => EditCmd::Copy,
				"x" => EditCmd::Cut,
				"v" => EditCmd::Paste,
				_ => return None,
			})
		}
		Key::Character(typed) if types => TabEditKey::Type(typed.to_string()),
		_ => return None,
	})
}

// The rename's own right-click menu: a text box's rows, less any with nothing
// to act on.
fn tab_edit_menu_items(selected: bool, has_text: bool, can_paste: bool) -> Vec<Entry> {
	let mut entries = Vec::new();
	if selected {
		entries.extend([
			entry_item_accel('t', "Cut", MenuAction::Edit(EditCmd::Cut)),
			entry_item_accel('C', "Copy", MenuAction::Edit(EditCmd::Copy)),
		]);
	}
	if can_paste {
		entries.push(entry_item_accel(
			'P',
			"Paste",
			MenuAction::Edit(EditCmd::Paste),
		));
	}
	if selected {
		entries.push(entry_item_accel(
			'D',
			"Delete",
			MenuAction::Edit(EditCmd::Delete),
		));
	}
	if has_text {
		entries.push(entry_item_accel(
			'a',
			"Select all",
			MenuAction::Edit(EditCmd::SelectAll),
		));
	}
	entries
}

// A menu pick that goes to a tab being renamed rather than to the pane: the
// rename's own rows, and Copy, Paste and Paste Selection from any menu, as in
// any text box with the focus.
fn tab_edit_takes(action: MenuAction) -> bool {
	matches!(
		action,
		MenuAction::Edit(_) | MenuAction::Copy | MenuAction::Paste | MenuAction::PasteSelection
	)
}

impl State {
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
		let mut edit = TabEdit::new(tab, text);
		// begun by a double-click, so one more press is the third
		edit.clicks = Some((Instant::now(), 2));
		self.tab_edit = Some(edit);
		// the pointer has not moved, so the tip would come straight back
		self.tab_hover.point_at(None);
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

	// A key while a rename is up. Every key is the rename's, so nothing typed
	// reaches the shell.
	fn tab_edit_key(&mut self, key: &Key) {
		let keys = input::edit_keys(self.mods, cfg!(target_os = "macos"));
		let selected = self
			.tab_edit
			.as_ref()
			.is_some_and(|e| e.selected().is_some());
		match tab_edit_key(key, keys, selected) {
			Some(TabEditKey::Commit) => self.commit_tab_edit(),
			Some(TabEditKey::Cancel) => self.cancel_tab_edit(),
			Some(TabEditKey::Menu) => {
				if let Some(field) = self.tab_edit_field() {
					self.open_tab_edit_menu(field.x, field.y + field.h);
				}
			}
			Some(TabEditKey::Edit(cmd)) => self.tab_edit_cmd(cmd),
			Some(TabEditKey::Type(typed)) => self.edit_tab(|edit| edit.insert(&typed)),
			Some(TabEditKey::Erase { back, reach }) => {
				self.edit_tab(|edit| edit.erase(back, reach));
			}
			Some(TabEditKey::Move { to, select }) => {
				self.edit_tab(|edit| edit.move_caret(to, select));
			}
			None => {}
		}
	}

	// Cut, Copy, Paste, Delete or Select all on the name being typed.
	fn tab_edit_cmd(&mut self, cmd: EditCmd) {
		let Some(edit) = self.tab_edit.as_mut() else {
			return;
		};
		match cmd {
			EditCmd::Copy | EditCmd::Cut => {
				if let Some(text) = edit.selected() {
					self.clipboard.set_clipboard(text.to_string());
					if cmd == EditCmd::Cut {
						edit.erase(true, Reach::Char);
					}
				}
			}
			EditCmd::Paste => {
				if let Some(text) = self.clipboard.get_clipboard() {
					edit.paste(&text);
				}
			}
			EditCmd::Delete => {
				if edit.selected().is_some() {
					edit.erase(true, Reach::Char);
				}
			}
			EditCmd::SelectAll => edit.select_all(),
		}
		self.dirty = true;
	}

	// A middle-click on the name, or Paste Selection from a menu: the primary
	// selection where there is one, else the clipboard.
	fn tab_edit_paste_primary(&mut self) {
		if let Some(text) = self.clipboard.get_primary() {
			self.edit_tab(|edit| edit.paste(&text));
		}
	}

	// While a tab is being renamed, a menu's Copy and Paste act on the name.
	// Any other pick ends the rename first, as a click elsewhere does. True
	// when the rename took the pick.
	fn menu_reaches_tab_edit(&mut self, action: MenuAction) -> bool {
		if self.tab_edit.is_none() {
			return false;
		}
		if !tab_edit_takes(action) {
			self.commit_tab_edit();
			return false;
		}
		match action {
			MenuAction::Edit(cmd) => self.tab_edit_cmd(cmd),
			MenuAction::Copy => self.tab_edit_cmd(EditCmd::Copy),
			MenuAction::Paste => self.tab_edit_cmd(EditCmd::Paste),
			_ => self.tab_edit_paste_primary(),
		}
		true
	}

	fn open_tab_edit_menu(&mut self, x: f32, y: f32) {
		let Some(edit) = self.tab_edit.as_ref() else {
			return;
		};
		let (selected, has_text) = (edit.selected().is_some(), !edit.text.is_empty());
		let can_paste = self
			.clipboard
			.get_clipboard()
			.is_some_and(|t| !t.is_empty());
		let entries = tab_edit_menu_items(selected, has_text, can_paste);
		if entries.is_empty() {
			return;
		}
		let target = self.tabs.cur().focused;
		self.bar_open = None;
		self.popup(target, entries, x, y);
		self.dirty = true;
	}

	// The box a rename types into, where its tab is on the page.
	fn tab_edit_field(&mut self) -> Option<Rect> {
		let tab = self.tab_edit.as_ref()?.tab;
		let (x, w) = self.tab_box(tab)?;
		Some(tab_edit_box(
			x,
			w,
			self.menubar_h(),
			self.tab_bar_h(),
			self.text.ui_line_h,
			self.text.scale,
		))
	}

	// The byte offset in the name nearest a pointer at `x`.
	fn tab_edit_offset(&mut self, x: f32) -> Option<usize> {
		let field = self.tab_edit_field()?;
		let rel_x = x - (field.x + self.text.dip(TAB_EDIT_PAD));
		let text = self.tab_edit.as_ref()?.text.clone();
		let attrs = crate::text::ui_attrs();
		Some(caret_from_click(&text, rel_x, &mut |s: &str| {
			self.text.measure_ui_text(s, &attrs)
		}))
	}
}

#[cfg(test)]
mod tests {
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
		let mut edit = TabEdit::new(0, "naïve".to_string());
		edit.caret = 0;
		// past the two-byte i-with-diaeresis and back
		for _ in 0..3 {
			edit.move_caret(Caret::Right(Reach::Char), false);
		}
		assert_eq!(edit.caret, 4);
		edit.move_caret(Caret::Left(Reach::Char), false);
		assert_eq!(edit.caret, 2);

		// a selection dragged with Shift, then typed over
		edit.move_caret(Caret::Right(Reach::End), true);
		assert_eq!(edit.range(), (2, 6));
		edit.insert("p");
		assert_eq!(edit.text, "nap");
		assert_eq!(edit.caret, 3);

		// backspace over the whole thing leaves it empty rather than underflowing
		for _ in 0..5 {
			edit.erase(true, Reach::Char);
		}
		assert_eq!(edit.text, "");
		assert_eq!(edit.caret, 0);

		// delete at the end has nothing to take
		edit.erase(false, Reach::Char);
		assert_eq!(edit.text, "");
	}

	// A tab rename reads keys as a Settings text box does: Ctrl for the
	// shortcuts and the words elsewhere, and on a Mac Command for the shortcuts
	// and either end, Option for the words. Option plus a letter types on a Mac.
	// Test ID: ErbKd5c
	#[test]
	fn a_tab_rename_takes_the_text_box_keys() {
		use crate::input::edit_keys;
		use winit::keyboard::{Key, ModifiersState as M, NamedKey};
		let pc = |mods| edit_keys(mods, false);
		let mac = |mods| edit_keys(mods, true);
		let ch = |c: &str| Key::Character(c.into());
		let named = Key::Named;
		let edit = |cmd| Some(TabEditKey::Edit(cmd));
		for (letter, cmd) in [
			("a", EditCmd::SelectAll),
			("c", EditCmd::Copy),
			("x", EditCmd::Cut),
			("v", EditCmd::Paste),
		] {
			assert_eq!(tab_edit_key(&ch(letter), pc(M::CONTROL), false), edit(cmd));
			assert_eq!(tab_edit_key(&ch(letter), mac(M::SUPER), false), edit(cmd));
			assert_eq!(tab_edit_key(&ch(letter), mac(M::CONTROL), false), None);
		}
		assert_eq!(
			tab_edit_key(&ch("V"), pc(M::CONTROL | M::SHIFT), false),
			edit(EditCmd::Paste)
		);
		assert_eq!(tab_edit_key(&ch("k"), pc(M::CONTROL), false), None);
		assert_eq!(
			tab_edit_key(&named(NamedKey::Insert), pc(M::SHIFT), false),
			edit(EditCmd::Paste)
		);
		assert_eq!(
			tab_edit_key(&named(NamedKey::Insert), pc(M::CONTROL), false),
			edit(EditCmd::Copy)
		);
		assert_eq!(
			tab_edit_key(&named(NamedKey::Delete), pc(M::SHIFT), true),
			edit(EditCmd::Cut)
		);
		assert_eq!(
			tab_edit_key(&named(NamedKey::Delete), pc(M::SHIFT), false),
			Some(TabEditKey::Erase {
				back: false,
				reach: Reach::Char
			})
		);
		let moves = |mods, key| tab_edit_key(&named(key), mods, false);
		let to = |to, select| Some(TabEditKey::Move { to, select });
		assert_eq!(
			moves(pc(M::CONTROL), NamedKey::ArrowLeft),
			to(Caret::Left(Reach::Word), false)
		);
		assert_eq!(
			moves(pc(M::CONTROL | M::SHIFT), NamedKey::ArrowRight),
			to(Caret::Right(Reach::Word), true)
		);
		assert_eq!(
			moves(mac(M::ALT), NamedKey::ArrowLeft),
			to(Caret::Left(Reach::Word), false)
		);
		assert_eq!(
			moves(mac(M::SUPER), NamedKey::ArrowRight),
			to(Caret::Right(Reach::End), false)
		);
		assert_eq!(
			moves(pc(M::SHIFT), NamedKey::Home),
			to(Caret::Left(Reach::End), true)
		);
		let erase = |back, reach| Some(TabEditKey::Erase { back, reach });
		assert_eq!(
			moves(pc(M::CONTROL), NamedKey::Backspace),
			erase(true, Reach::Word)
		);
		assert_eq!(
			moves(mac(M::ALT), NamedKey::Backspace),
			erase(true, Reach::Word)
		);
		assert_eq!(
			moves(mac(M::SUPER), NamedKey::Backspace),
			erase(true, Reach::End)
		);
		assert_eq!(
			moves(mac(M::ALT), NamedKey::Delete),
			erase(false, Reach::Word)
		);
		assert_eq!(
			moves(pc(M::empty()), NamedKey::ContextMenu),
			Some(TabEditKey::Menu)
		);
		assert_eq!(moves(pc(M::SHIFT), NamedKey::F10), Some(TabEditKey::Menu));
		assert_eq!(moves(pc(M::empty()), NamedKey::F10), None);
		let typed = |s: &str| Some(TabEditKey::Type(s.into()));
		assert_eq!(tab_edit_key(&ch("q"), pc(M::empty()), false), typed("q"));
		assert_eq!(tab_edit_key(&ch("o"), pc(M::ALT), false), None);
		assert_eq!(
			tab_edit_key(&ch("\u{f8}"), mac(M::ALT), false),
			typed("\u{f8}")
		);
		assert_eq!(tab_edit_key(&ch("{"), mac(M::ALT), false), typed("{"));
		assert_eq!(tab_edit_key(&ch("t"), mac(M::SUPER), false), None);
		assert_eq!(moves(pc(M::SHIFT), NamedKey::Space), typed(" "));
		assert_eq!(
			moves(pc(M::empty()), NamedKey::Enter),
			Some(TabEditKey::Commit)
		);
		assert_eq!(
			moves(pc(M::empty()), NamedKey::Tab),
			Some(TabEditKey::Commit)
		);
		assert_eq!(
			moves(pc(M::empty()), NamedKey::Escape),
			Some(TabEditKey::Cancel)
		);
	}

	// The name edits as a Settings box does: one press places the caret, two
	// take a word, three the whole name, Shift extends and a held press drags.
	// A paste is one line.
	// Test ID: ErbKd9K
	#[test]
	fn a_tab_rename_edits_like_a_text_box() {
		let start = Instant::now();
		let at = |ms| start + Duration::from_millis(ms);
		let mut edit = TabEdit::new(0, "build server.log".to_string());
		assert_eq!(edit.selected(), Some("build server.log"), "opens selected");
		edit.press(3, at(0), false);
		assert_eq!((edit.caret, edit.selected()), (3, None));
		edit.drag_to(12);
		assert_eq!(edit.selected(), Some("ld server"));
		edit.dragging = false;
		edit.drag_to(1);
		assert_eq!(edit.caret, 12, "no drag once the button is up");
		edit.press(8, at(100), false);
		assert_eq!(
			edit.selected(),
			Some("server"),
			"a double-click takes the word"
		);
		edit.press(8, at(200), false);
		assert_eq!(
			edit.selected(),
			Some("build server.log"),
			"a third takes it all"
		);
		edit.press(5, at(1000), false);
		edit.press(12, at(2000), true);
		assert_eq!(edit.selected(), Some(" server"), "Shift extends");
		assert!(edit.dragging);

		// a plain step out of a selection lands on its edge
		edit.move_caret(Caret::Left(Reach::Char), false);
		assert_eq!((edit.caret, edit.selected()), (5, None));
		edit.move_caret(Caret::Right(Reach::Word), true);
		assert_eq!(edit.selected(), Some(" server"));
		edit.move_caret(Caret::Right(Reach::Word), false);
		assert_eq!((edit.caret, edit.selected()), (12, None));
		edit.erase(true, Reach::Word);
		assert_eq!(edit.text, "build .log");
		edit.erase(false, Reach::End);
		assert_eq!(edit.text, "build ");

		edit.paste("one\r\ntwo\tthree\u{1b}[0m\n");
		assert_eq!(edit.text, "build one two three[0m ");
		edit.select_all();
		edit.paste("x");
		assert_eq!((edit.text.as_str(), edit.caret), ("x", 1));
	}

	// Copy, Paste and Paste Selection from any menu go to a tab being renamed,
	// and so do the rename's own rows. Anything else ends the rename. Its own
	// menu leaves out rows with nothing to act on.
	// Test ID: ErbKdDM
	#[test]
	fn menus_reach_a_tab_rename() {
		for action in [
			MenuAction::Copy,
			MenuAction::Paste,
			MenuAction::PasteSelection,
			MenuAction::Edit(EditCmd::Cut),
			MenuAction::Edit(EditCmd::SelectAll),
		] {
			assert!(tab_edit_takes(action), "{action:?}");
		}
		for action in [
			MenuAction::NewTab,
			MenuAction::CloseTab,
			MenuAction::ToggleSingleTab,
			MenuAction::Settings,
			MenuAction::CopyLink,
		] {
			assert!(!tab_edit_takes(action), "{action:?}");
		}
		let labels = |entries: Vec<Entry>| -> Vec<String> {
			entries
				.iter()
				.filter_map(|entry| entry_label(entry).map(str::to_string))
				.collect()
		};
		let all = tab_edit_menu_items(true, true, true);
		assert_eq!(accel_clash(&all), None);
		assert_eq!(
			labels(all),
			["Cut", "Copy", "Paste", "Delete", "Select all"]
		);
		assert_eq!(
			labels(tab_edit_menu_items(false, true, true)),
			["Paste", "Select all"]
		);
		assert_eq!(
			labels(tab_edit_menu_items(false, true, false)),
			["Select all"]
		);
		assert!(tab_edit_menu_items(false, false, false).is_empty());
	}
}
