// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use super::{EditState, PROMPT_ROW, Prompt, PromptFocus, PromptJob, SettingsDialog};
use crate::config;
use crate::ui_spec::Key;

/// The theme row's four buttons, in the order they are declared.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ThemeBtn {
	Save,
	SaveAs,
	Rename,
	Delete,
}

impl ThemeBtn {
	pub(super) fn of(part: u16) -> ThemeBtn {
		match part {
			0 => ThemeBtn::Save,
			1 => ThemeBtn::SaveAs,
			2 => ThemeBtn::Rename,
			_ => ThemeBtn::Delete,
		}
	}
}

impl SettingsDialog {
	// The saved theme the `theme` setting currently names, if it names one.
	fn user_theme_index(&self) -> Option<usize> {
		let name = self.edited.theme.trim();
		self.edited
			.user_themes
			.iter()
			.position(|t| t.name.eq_ignore_ascii_case(name))
	}

	/// Has the user moved a color away from what the current theme says? That IS
	/// the unsaved-changes test, and it needs no flag of its own: an edited color
	/// lives on as a `colors.*` line, so the answer survives a restart for free.
	pub(super) fn theme_dirty(&self) -> bool {
		// resolve the palette once - this runs per button, per frame
		let palette = self.theme_palette();
		(0..crate::theme::PALETTE_KEYS.len())
			.any(|i| self.get_col(Self::palette_key(i)) != palette.get(i))
	}

	// The dialog row key holding palette color `i` (same order as PALETTE_KEYS).
	fn palette_key(i: usize) -> Key {
		match i {
			0 => Key::ColBg,
			1 => Key::ColFg,
			2 => Key::ColCursor,
			3 => Key::ColHighlight,
			4 => Key::ColFocus,
			5 => Key::ColMenuBg,
			6 => Key::ColMenuFg,
			7 => Key::ColDialogBg,
			8 => Key::ColDialogFg,
			_ => Key::ColGutter,
		}
	}

	pub(super) fn theme_btn_enabled(&self, which: ThemeBtn) -> bool {
		match which {
			ThemeBtn::Save => self.theme_dirty(),
			ThemeBtn::SaveAs => true,
			// a built-in is not the user's to rename or throw away; saving over its
			// name first makes a copy that is
			ThemeBtn::Rename | ThemeBtn::Delete => self.user_theme_index().is_some(),
		}
	}

	/// Take on the colors of whatever theme and mode are now selected. Switching
	/// theme adopts the new scheme rather than keeping tweaks made to the old one
	/// on top of it - a picker that visibly changed nothing would be worse, and the
	/// tweaks were changes to the theme being left behind.
	///
	/// Reverting each key rather than just setting it is what keeps the file honest:
	/// the colors on screen are now the theme's own, so the per-color overrides have
	/// nothing left to say and Apply comments them out. Setting them alone would
	/// write ten active colors.* lines pinning this one palette, which then wins over
	/// every later theme change and freezes one variant under `theme_mode: system`.
	pub(super) fn adopt_theme(&mut self) {
		let pal = self.theme_palette();
		for i in 0..crate::theme::PALETTE_KEYS.len() {
			// default_col resolves through theme_palette, so this ends on pal.get(i)
			self.revert(Self::palette_key(i));
		}
		self.edited.ansi = pal.ansi;
	}

	/// Store the colors on screen under `name`, replacing a saved theme of that
	/// name or adding one. The variant the dialog is NOT showing is carried over
	/// from whatever `name` resolves to today, so a theme is always complete.
	pub(super) fn save_theme_as(&mut self, name: &str) {
		let name = name.trim().to_string();
		let dark_now = self.edited.theme_mode.is_dark(config::is_dark());
		let other = crate::theme::resolve_in(
			&self.edited.user_themes,
			&self.edited.theme,
			if dark_now {
				crate::theme::Mode::Light
			} else {
				crate::theme::Mode::Dark
			},
			config::is_dark(),
		);
		let mut shown = other; // start from a full palette, then overwrite with the edits
		for i in 0..crate::theme::PALETTE_KEYS.len() {
			shown.set(i, self.get_col(Self::palette_key(i)));
		}
		shown.ansi = self.edited.ansi;
		let (dark, light) = if dark_now {
			(shown, other)
		} else {
			(other, shown)
		};
		let existing = self
			.edited
			.user_themes
			.iter()
			.position(|t| t.name.eq_ignore_ascii_case(&name));
		let slug = match existing {
			Some(theme_index) => self.edited.user_themes[theme_index].slug.clone(),
			None => self.free_slug(&name),
		};
		let theme = crate::theme::UserTheme {
			slug,
			name: name.clone(),
			dark,
			light,
		};
		match existing {
			Some(k) => self.edited.user_themes[k] = theme,
			None => self.edited.user_themes.push(theme),
		}
		self.edited.theme = name;
		// the tweaks are the theme's own colors now, so the per-color overrides
		// have nothing left to say and are commented back out on Apply
		for i in 0..crate::theme::PALETTE_KEYS.len() {
			self.revert(Self::palette_key(i));
		}
	}

	// A config path segment for a new theme: the name reduced to something a path
	// can hold, then made unique. The slug never changes afterwards, so a rename
	// rewrites one line rather than moving a subtree.
	fn free_slug(&self, name: &str) -> String {
		let base: String = name
			.chars()
			.map(|c| {
				if c.is_ascii_alphanumeric() {
					c.to_ascii_lowercase()
				} else {
					'_'
				}
			})
			.collect();
		let base = base.trim_matches('_').to_string();
		let base = if base.is_empty() { "theme" } else { &base }.to_string();
		let taken = |s: &str| self.edited.user_themes.iter().any(|t| t.slug == s);
		if !taken(&base) {
			return base;
		}
		(2..=u32::from(u16::MAX))
			.map(|n| format!("{base}{n}"))
			.find(|s| !taken(s))
			.unwrap_or(base)
	}

	/// Why OK cannot accept the typed name, if it cannot.
	pub(super) fn name_problem(&self, which: ThemeBtn, name: &str) -> Option<String> {
		let name = name.trim();
		if name.is_empty() {
			return Some("Enter a name.".into());
		}
		// A theme is not in its own way. Rename opens on the theme's current name,
		// so OK with nothing changed has to go through, and so does a change of
		// case alone - which is the only way to make one.
		let mine = (which == ThemeBtn::Rename)
			.then(|| self.user_theme_index())
			.flatten();
		let clashes = self
			.edited
			.user_themes
			.iter()
			.enumerate()
			.any(|(k, t)| Some(k) != mine && t.name.eq_ignore_ascii_case(name));
		// Save as over a saved theme's name replaces it, which is a fair reading of
		// the button; a rename onto another theme's name would merge two into one.
		if which == ThemeBtn::Rename && clashes {
			return Some("That name is already taken.".into());
		}
		None
	}

	/// Press a theme button: Save acts at once, the other three ask first.
	pub(super) fn theme_action(&mut self, which: ThemeBtn) {
		if !self.theme_btn_enabled(which) {
			return;
		}
		self.commit_edit();
		match which {
			ThemeBtn::Save => {
				let name = self.edited.theme.clone();
				self.save_theme_as(&name);
			}
			ThemeBtn::SaveAs | ThemeBtn::Rename => {
				let (title, seed) = if which == ThemeBtn::SaveAs {
					("Enter a name for the new theme".to_string(), String::new())
				} else {
					(
						"Enter a new name for this theme".to_string(),
						self.edited.theme.clone(),
					)
				};
				// a rename opens on the existing name, selected, the way a rename
				// field does everywhere else
				let mut edit = EditState::new(PROMPT_ROW, seed);
				edit.sel = (!edit.buf.is_empty()).then_some(0);
				self.edit = Some(edit);
				self.prompt = Some(Prompt {
					job: PromptJob::Theme(which),
					title,
					focus: PromptFocus::Field,
					warn: None,
				});
			}
			ThemeBtn::Delete => {
				self.prompt = Some(Prompt {
					job: PromptJob::Theme(which),
					title: format!("Really delete theme \"{}\"?", self.edited.theme),
					focus: PromptFocus::Ok,
					warn: None,
				});
			}
		}
	}

	pub(super) fn rename_theme(&mut self, name: &str) {
		let name = name.trim().to_string();
		if let Some(theme_index) = self.user_theme_index() {
			self.edited.user_themes[theme_index].name.clone_from(&name);
			self.edited.theme = name;
		}
	}

	/// Drop the saved theme. A built-in of the same name comes back out from behind
	/// it; otherwise the selection falls to the first theme left.
	pub(super) fn delete_theme(&mut self) {
		let Some(theme_index) = self.user_theme_index() else {
			return;
		};
		let name = self.edited.user_themes[theme_index].name.clone();
		self.edited.user_themes.remove(theme_index);
		if !crate::theme::is_builtin(&name) {
			self.edited.theme = crate::theme::all_names(&self.edited.user_themes)
				.first()
				.cloned()
				.unwrap_or_else(|| name.clone());
		}
		self.adopt_theme();
	}
}

#[cfg(test)]
mod tests {
	use super::super::SettingsDialog;
	use super::super::tests::{mk_dialog, on_theme};
	use crate::config;
	use crate::ui_spec::Key;

	fn theme_row(d: &SettingsDialog) -> usize {
		d.specs
			.iter()
			.position(|s| matches!(s.kind, super::super::Kind::Buttons(_)))
			.expect("the theme actions row")
	}

	// Nothing anywhere records "this theme has unsaved changes" - a color that
	// disagrees with the theme IS the record, and it lives in the config file, so
	// the answer is the same after a restart.
	// Test ID: Em3lZEu
	#[test]
	fn an_edited_color_is_what_makes_the_theme_dirty() {
		let mut d = on_theme("Matrix");
		let row = theme_row(&d);
		assert!(!d.theme_dirty());
		assert!(!d.theme_btn_enabled(super::super::ThemeBtn::Save));
		assert!(d.part_disabled(row, 0), "Save starts grayed");

		d.set_col(Key::ColFg, [1, 2, 3]);
		assert!(d.theme_dirty());
		assert!(!d.part_disabled(row, 0), "Save wakes up on an edit");
		// Save as is always available; the other two need a theme of the user's own
		assert!(!d.part_disabled(row, 1));
		assert!(d.part_disabled(row, 2), "Rename needs a saved theme");
		assert!(d.part_disabled(row, 3), "Delete needs a saved theme");
	}

	// Saving folds the edits into the theme itself, so the per-color overrides
	// have nothing left to say and are queued to be commented back out.
	// Test ID: Em3lZEv
	#[test]
	fn saving_folds_the_edits_into_the_theme() {
		let mut d = on_theme("Matrix");
		d.set_col(Key::ColFg, [1, 2, 3]);
		d.save_theme_as("Mine");

		assert_eq!(d.edited.theme, "Mine");
		let saved = crate::theme::find_user(&d.edited.user_themes, "Mine").expect("saved");
		assert_eq!(saved.dark.fg, [1, 2, 3]);
		// the mode the dialog was NOT showing still comes out complete
		assert_eq!(
			saved.light.fg,
			crate::theme::resolve("Matrix", crate::theme::Mode::Light, true).fg
		);
		// and the ANSI set came along, so the theme stands on its own
		assert_eq!(
			saved.dark.ansi,
			crate::theme::resolve("Matrix", crate::theme::Mode::Dark, true).ansi
		);

		assert!(!d.theme_dirty(), "the edit is the theme's own color now");
		assert!(
			d.reverted.contains(&"colors.foreground"),
			"the override is queued for removal"
		);
	}

	// Saving grays Save out, so the control the keyboard was on drops out of the
	// Tab ring. Focus has to carry on to the next button rather than snapping
	// back to the first control on the tab.
	// Test ID: Em430PA
	#[test]
	fn focus_stays_when_the_control_under_it_grays_out() {
		let mut d = on_theme("Matrix");
		let row = theme_row(&d);
		d.tab = d.specs[row].tab;
		d.set_col(Key::ColFg, [1, 2, 3]);
		d.focus = Some(super::super::Focus::Row(row, 0));
		d.theme_action(super::super::ThemeBtn::Save);
		assert!(d.part_disabled(row, 0), "Save grays out once it has saved");
		d.focus_move(true);
		assert_eq!(d.focus, Some(super::super::Focus::Row(row, 1)));
		// and backwards off the same gap goes to the row above, not the last button
		d.focus = Some(super::super::Focus::Row(row, 0));
		d.focus_move(false);
		assert!(matches!(d.focus, Some(super::super::Focus::Row(i, _)) if i < row));
	}

	// A theme may take a built-in's name and stand in for it; deleting it puts the
	// built-in back rather than leaving the name pointing at nothing.
	// Test ID: Em3lZEw
	#[test]
	fn a_saved_theme_shadows_a_builtin_and_delete_uncovers_it() {
		let mut d = on_theme("Matrix");
		let builtin_fg = d.get_col(Key::ColFg);
		d.set_col(Key::ColFg, [9, 9, 9]);
		d.save_theme_as("Matrix");
		assert_eq!(d.get_col(Key::ColFg), [9, 9, 9]);
		// it is the user's theme now, so it can be renamed or thrown away
		let row = theme_row(&d);
		assert!(!d.part_disabled(row, 2) && !d.part_disabled(row, 3));
		// the name is listed once, not twice
		let names = crate::theme::all_names(&d.edited.user_themes);
		assert_eq!(names.iter().filter(|n| *n == "Matrix").count(), 1);

		d.delete_theme();
		assert_eq!(d.edited.theme, "Matrix");
		assert_eq!(d.get_col(Key::ColFg), builtin_fg);
	}

	// The colors on screen are no longer the theme the box names, so the box stops
	// naming it. The popup still highlights the theme the edits started from, and
	// picking it back adopts its colors and the box says its name again.
	// Test ID: EqSprxY
	#[test]
	fn an_edited_theme_reads_as_unsaved_in_the_box() {
		let mut d = on_theme("Matrix");
		let row = d
			.specs
			.iter()
			.position(|s| s.key == Key::Theme)
			.expect("the theme row");
		assert_eq!(d.dd_closed_label(row), "Matrix");

		let mode = d
			.specs
			.iter()
			.position(|s| s.key == Key::ThemeMode)
			.expect("the mode row");

		d.set_col(Key::ColFg, [1, 2, 3]);
		assert_eq!(d.dd_closed_label(row), super::super::UNSAVED_THEME);
		// only this one box: every other dropdown still says what it is on
		assert_eq!(d.dd_closed_label(mode), "Dark");
		// the list itself is untouched, and the highlight still finds Matrix
		let names = d.dd_options(row);
		assert!(!names.iter().any(|n| n == super::super::UNSAVED_THEME));
		assert_eq!(names[d.get_radio(Key::Theme)], "Matrix");

		d.set_radio(Key::Theme, d.get_radio(Key::Theme));
		assert_eq!(d.dd_closed_label(row), "Matrix", "re-picking discards");

		// saving under a new name makes the edits that theme's own
		d.set_col(Key::ColFg, [1, 2, 3]);
		d.save_theme_as("Mine");
		assert_eq!(d.dd_closed_label(row), "Mine");
	}

	// Picking a theme takes on its colors. Keeping the old theme's tweaks would
	// make the picker look broken on every color that had been edited.
	// Test ID: Em3lZEx
	#[test]
	fn picking_a_theme_adopts_its_colors() {
		let mut d = on_theme("SilkTerm");
		d.set_col(Key::ColFg, [1, 2, 3]);
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::Theme)
			.expect("the theme row");
		let names = d.dd_options(i);
		let k = names.iter().position(|n| n == "Matrix").expect("Matrix");
		d.set_radio(Key::Theme, k);

		assert_eq!(d.edited.theme, "Matrix");
		assert_eq!(
			d.get_col(Key::ColFg),
			crate::theme::resolve("Matrix", crate::theme::Mode::Dark, true).fg
		);
		assert!(!d.theme_dirty(), "a fresh theme starts unmodified");
	}

	// Picking a theme must not leave the old palette behind as colors.* overrides.
	// Those would be written as active lines and then outrank every later theme
	// change, which also freezes one variant when the mode follows the desktop.
	// Test ID: Em430PB
	#[test]
	fn adopting_a_theme_clears_the_color_overrides() {
		let mut d = on_theme("SilkTerm");
		d.set_col(Key::ColFg, [1, 2, 3]);
		d.set_col(Key::ColBg, [4, 5, 6]);
		d.reverted.clear();

		d.edited.theme = "Matrix".to_string();
		d.adopt_theme();

		let pending = d.take_reverted();
		for i in 0..crate::theme::PALETTE_KEYS.len() {
			for cfg_key in super::super::ui().settings_of(SettingsDialog::palette_key(i)) {
				assert!(
					pending.contains(cfg_key),
					"{cfg_key} must be commented out on Apply"
				);
			}
		}
		assert!(!d.theme_dirty(), "the adopted palette is not an edit");
	}

	// Renaming moves the name and the selection together; the slug behind it does
	// not move, so the config subtree stays where it is.
	// Test ID: Eq4Gng8
	#[test]
	fn a_theme_can_be_renamed_to_its_own_name_or_a_different_case() {
		let mut d = on_theme("Matrix");
		d.save_theme_as("Mine");
		// Rename opens on the theme's own name, so OK with nothing changed used
		// to answer "that name is taken" and keep the box up.
		assert!(
			d.name_problem(super::super::ThemeBtn::Rename, "Mine")
				.is_none()
		);
		assert!(
			d.name_problem(super::super::ThemeBtn::Rename, "MINE")
				.is_none()
		);

		let slug = d.edited.user_themes[0].slug.clone();
		d.rename_theme("MINE");
		assert_eq!(d.edited.theme, "MINE");
		assert_eq!(d.edited.user_themes[0].slug, slug, "still the same theme");

		// another saved theme's name is still refused
		d.save_theme_as("Other");
		assert!(
			d.name_problem(super::super::ThemeBtn::Rename, "mine")
				.is_some()
		);
	}

	// Test ID: Em3lZEy
	#[test]
	fn a_rename_moves_the_name_and_the_selection() {
		let mut d = on_theme("Matrix");
		d.set_col(Key::ColFg, [4, 5, 6]);
		d.save_theme_as("Mine");
		let slug = d.edited.user_themes[0].slug.clone();

		d.rename_theme("Ours");
		assert_eq!(d.edited.theme, "Ours");
		assert_eq!(d.edited.user_themes[0].slug, slug);
		assert_eq!(d.get_col(Key::ColFg), [4, 5, 6]);

		// two themes cannot share a name, or one would swallow the other
		d.save_theme_as("Theirs");
		assert_eq!(d.edited.user_themes.len(), 2);
		assert!(
			d.name_problem(super::super::ThemeBtn::Rename, "ours")
				.is_some()
		);
		assert!(
			d.name_problem(super::super::ThemeBtn::Rename, "  ")
				.is_some()
		);
		assert!(
			d.name_problem(super::super::ThemeBtn::Rename, "Third")
				.is_none()
		);
		// Save as over an existing name replaces it, which is a fair reading
		assert!(
			d.name_problem(super::super::ThemeBtn::SaveAs, "ours")
				.is_none()
		);
	}

	// A saved theme has to come back after a restart, or saving it meant nothing.
	// Test ID: Em3lZF1
	#[test]
	fn a_saved_theme_survives_a_relaunch() {
		let _guard = config::test_config_lock();
		let _ = config::settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_theme_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let _ = std::fs::write(&path, "");
		config::set_config_override(path.clone());
		let base = config::reload_from_disk();

		let mut d = mk_dialog(4000.0);
		d.orig = base.clone();
		d.edited = base.clone();
		d.edited.theme = "Matrix".into();
		d.adopt_theme();
		d.set_col(Key::ColFg, [0x12, 0x34, 0x56]);
		d.save_theme_as("Saved One");
		assert!(config::persist(&base, &d.edited));

		let back = config::reload_from_disk();
		let saved = crate::theme::find_user(&back.user_themes, "Saved One").expect("on disk");
		assert_eq!(
			// which variant an edit goes in follows the mode, so name it: a mode
			// arriving out of somebody else's config file is what this last went
			// wrong as, and the colors alone do not say that
			saved.dark.fg,
			[0x12, 0x34, 0x56],
			"mode {:?}",
			d.edited.theme_mode
		);
		assert_eq!(
			saved.dark.ansi,
			crate::theme::resolve("Matrix", crate::theme::Mode::Dark, true).ansi
		);
		assert_eq!(back.theme, "Saved One");
		assert_eq!(
			back.fg,
			[0x12, 0x34, 0x56],
			"and it is what the terminal uses"
		);

		// deleting it takes the whole subtree with it
		let mut d2 = mk_dialog(4000.0);
		d2.orig = back.clone();
		d2.edited = back.clone();
		d2.delete_theme();
		assert!(config::persist(&back, &d2.edited));
		let after = config::reload_from_disk();
		assert!(after.user_themes.is_empty(), "gone from the file");
		assert!(
			!std::fs::read_to_string(&path)
				.unwrap()
				.contains("Saved One"),
			"and nothing of it is left behind"
		);

		let _ = std::fs::remove_dir_all(&dir);
	}

	// Rename opens on the theme's own name, all of it selected, so typing
	// replaces it.
	// Test ID: Er2X6Eq
	#[test]
	fn rename_opens_on_the_name_selected() {
		let mut d = on_theme("Matrix");
		d.save_theme_as("Mine");
		d.theme_action(super::super::ThemeBtn::Rename);
		let edit = d.edit.as_ref().expect("the name field");
		assert_eq!(edit.buf, "Mine");
		assert_eq!(edit.sel, Some(0), "the name is not selected");
		assert_eq!(edit.cur, edit.buf.len(), "the caret is not at the end");
		d.char_input('X');
		d.prompt_accept();
		assert!(d.prompt.is_none());
		assert_eq!(d.edited.theme, "X");
		assert_eq!(d.edited.user_themes.len(), 1);
		assert_eq!(d.edited.user_themes[0].name, "X");
	}
}
