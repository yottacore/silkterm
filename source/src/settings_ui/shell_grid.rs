// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The shells grid spans the whole content width rather than starting at the
//! control column: there is no label beside it, and the command it holds is the
//! one value in the dialog that is routinely too long to read. Columns are laid
//! out from BOTH ends - the fixed ones from the right, the name from the left -
//! and the command takes whatever is left between them, so a wider panel widens
//! the column that needs it.

use super::{
	Dlg, Focus, Prompt, PromptFocus, PromptJob, SettingsDialog, border, lay, quad, shell_field_row,
};
use crate::config;
use crate::gfx::{QuadMode, RectInstance};
use crate::pane::Rect;
use crate::ui_spec::Kind;

/// One shell's own controls, in Tab order - which is also left to right across
/// its line. `Add` is the single stop past the last entry, so a grid of `n`
/// entries has `n * ShellPart::COUNT + 1` stops.
///
/// The grip is deliberately NOT one of them. Reordering is a mouse gesture now,
/// so a Tab through the grid walks the values and nothing else; there is no stop
/// that draws a control the keyboard cannot work.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ShellPart {
	Name,
	Command,
	Remove,
	Active,
}

impl ShellPart {
	pub(super) const COUNT: u16 = 4;
	const ALL: [ShellPart; 4] = [
		ShellPart::Name,
		ShellPart::Command,
		ShellPart::Remove,
		ShellPart::Active,
	];
	pub(super) fn of(stop: u16) -> ShellPart {
		ShellPart::ALL[(stop % ShellPart::COUNT) as usize]
	}
}

/// Left edge of every column on a shells line, plus the width of the one column
/// that is not fixed. The fixed columns are placed from BOTH ends and the command
/// takes whatever is left between them, so a wider panel widens the one value
/// that is routinely too long to read.
#[derive(Debug)]
pub(super) struct ShellCols {
	grip: f32,
	pub(super) name: f32,
	pub(super) command: f32,
	command_w: f32,
	remove: f32,
	pub(super) seen: f32,
	pub(super) active: f32,
}

/// A line being dragged by its grip. `at` is where it currently sits, because the
/// list is reordered as the pointer moves rather than on release - the line the
/// user is dragging is the line they can see moving, which is the whole reason to
/// use a grip instead of buttons.
#[derive(Debug)]
pub(super) struct ShellDrag {
	pub(super) at: usize,
	/// where inside the line the pointer took hold, so it does not jump on grab
	pub(super) grab_dy: f32,
}

/// Where a part index sits in the grid: an entry's control, or the Add button
/// past the end.
#[derive(Debug)]
pub(super) enum ShellStop {
	Entry(usize, ShellPart),
	Add,
}

pub(super) fn shell_stop(part: u16, entries: usize) -> ShellStop {
	let entry = (part / ShellPart::COUNT) as usize;
	if entry >= entries {
		ShellStop::Add
	} else {
		ShellStop::Entry(entry, ShellPart::of(part))
	}
}

fn shell_part_index(entry: usize, part: ShellPart) -> u16 {
	entry as u16 * ShellPart::COUNT
		+ ShellPart::ALL.iter().position(|p| *p == part).unwrap_or(0) as u16
}

impl SettingsDialog {
	/// One line of the grid - the same height as an ordinary settings row, so the
	/// fields in it match every other field in the dialog.
	pub(super) fn shell_line_h(&self) -> f32 {
		self.line_row_h()
	}

	/// Space or Enter on a grid stop: open a field, flip the switch, move the
	/// entry, or ask before dropping it.
	pub(super) fn shell_activate(&mut self, i: usize, part: u16) {
		match shell_stop(part, self.edited.shells.len()) {
			ShellStop::Add => self.shell_add(i),
			ShellStop::Entry(shell_index, ShellPart::Name) => {
				self.open_edit(shell_field_row(shell_index, false), true);
			}
			ShellStop::Entry(shell_index, ShellPart::Command) => {
				self.open_edit(shell_field_row(shell_index, true), true);
			}
			ShellStop::Entry(shell_index, ShellPart::Active) => {
				if let Some(entry) = self.edited.shells.get_mut(shell_index) {
					entry.active = !entry.active;
				}
			}
			ShellStop::Entry(shell_index, ShellPart::Remove) => {
				self.shell_confirm_remove(shell_index);
			}
		}
	}

	// Dropping an entry is the one grid action that cannot be undone by doing the
	// opposite, so it asks - the same box the theme delete uses.
	fn shell_confirm_remove(&mut self, shell_index: usize) {
		let Some(entry) = self.edited.shells.get(shell_index) else {
			return;
		};
		let name = if entry.title.trim().is_empty() {
			entry.command.clone()
		} else {
			entry.title.clone()
		};
		self.commit_edit();
		self.prompt = Some(Prompt {
			job: PromptJob::DropShell(shell_index),
			title: format!("Really remove \"{name}\" from the list?"),
			focus: PromptFocus::Ok,
			warn: None,
		});
	}

	/// A scan arrived while this dialog was open. Both copies move, so a user who
	/// has changed nothing still has nothing changed - the same reasoning that
	/// keeps the list out of an ordinary Apply diff. Anything they have already
	/// done to the list (a rename, a reorder, a removal) is what the merge folds
	/// INTO, so none of it is undone; a scan only ever appends and switches off.
	pub fn fold_shells(&mut self, found: &[crate::shells::Found]) {
		self.orig.shells = crate::shells::merge(&self.orig.shells, found);
		self.edited.shells = crate::shells::merge(&self.edited.shells, found);
	}

	/// The spec index of the grid, for the pseudo-row fields, which know which
	/// entry they belong to but not which row draws it.
	pub(super) fn shell_row(&self) -> Option<usize> {
		(0..self.specs.len()).find(|&i| matches!(self.specs[i].kind, Kind::ShellList))
	}

	/// Total width of everything except the command's own slack: what the panel
	/// must clear for the grid to be readable at all. Static, so `new` can size
	/// the window before Self exists.
	pub(super) fn shell_columns_w(font_scale: f32) -> f32 {
		let l = lay();
		l.shell_name_width
			+ l.shell_command_width
			+ l.shell_seen_width
			+ l.shell_active_width
			+ l.shell_col_gap * 5.0
			+ (l.shell_grip + l.shell_button) * font_scale
	}

	// The two icon columns follow the UI font the way the checkboxes do, so a
	// bigger desktop font gets a bigger grab handle rather than a fiddlier one.
	fn shell_grip_w(&self) -> f32 {
		lay().shell_grip * self.ui_scale()
	}

	fn shell_button_w(&self) -> f32 {
		lay().shell_button * self.ui_scale()
	}

	/// x of each column's left edge, plus the command column's width. Remove sits
	/// between the command and the read-only date rather than at the end of the
	/// line: it is the one control here that doing the opposite cannot undo, so
	/// it is deliberately kept off the right-hand edge the pointer travels down.
	pub(super) fn shell_cols(&self) -> ShellCols {
		let l = lay();
		let left = self.content_x() + l.pad;
		let right = self.content_x() + self.layout_w() - l.pad;
		let active = right - l.shell_active_width;
		let seen = active - l.shell_col_gap - l.shell_seen_width;
		let remove = seen - l.shell_col_gap - self.shell_button_w();
		let name = left + self.shell_grip_w() + l.shell_col_gap;
		let command = name + l.shell_name_width + l.shell_col_gap;
		let command_w = (remove - l.shell_col_gap - command).max(l.shell_command_width / 2.0);
		ShellCols {
			grip: left,
			name,
			command,
			command_w,
			remove,
			seen,
			active,
		}
	}

	/// Top of the grid's column titles, and of entry `shell_index`'s own line.
	pub(super) fn shell_head_y(&self, i: usize) -> f32 {
		self.row_y(i)
	}

	pub(super) fn shell_line_y(&self, i: usize, shell_index: usize) -> f32 {
		self.shell_head_y(i)
			+ self.line_h
			+ lay().shell_head_gap
			+ shell_index as f32 * self.shell_line_h()
	}

	// A boxed control centered in entry `shell_index`'s line, at `x` and `w` wide.
	fn shell_box(&self, i: usize, shell_index: usize, x: f32, w: f32) -> Rect {
		let line = self.shell_line_h();
		let h = self.field_h();
		Rect {
			x,
			y: self.shell_line_y(i, shell_index) + (line - h) / 2.0,
			w,
			h,
		}
	}

	pub(super) fn shell_name_box(&self, i: usize, shell_index: usize) -> Rect {
		self.shell_box(
			i,
			shell_index,
			self.shell_cols().name,
			lay().shell_name_width,
		)
	}

	pub(super) fn shell_cmd_box(&self, i: usize, shell_index: usize) -> Rect {
		let cols = self.shell_cols();
		self.shell_box(i, shell_index, cols.command, cols.command_w)
	}

	// The Active checkbox, centered under its own column title. Square like
	// every other checkbox, not field-tall like the boxes beside it.
	fn shell_active_box(&self, i: usize, shell_index: usize) -> Rect {
		let size = lay().swatch;
		let line = self.shell_line_h();
		Rect {
			x: self.shell_cols().active + (lay().shell_active_width - size) / 2.0,
			y: self.shell_line_y(i, shell_index) + (line - size) / 2.0,
			w: size,
			h: size,
		}
	}

	// The drag handle. As tall as the fields beside it rather than square,
	// because it is grabbed rather than aimed at - the taller box is the whole
	// difference between a reorder that feels direct and one that keeps missing.
	fn shell_grip_box(&self, i: usize, shell_index: usize) -> Rect {
		self.shell_box(i, shell_index, self.shell_cols().grip, self.shell_grip_w())
	}

	fn shell_remove_box(&self, i: usize, shell_index: usize) -> Rect {
		let size = self.shell_button_w();
		let line = self.shell_line_h();
		Rect {
			x: self.shell_cols().remove,
			y: self.shell_line_y(i, shell_index) + (line - size) / 2.0,
			w: size,
			h: size,
		}
	}

	pub(super) fn shell_add_box(&self, i: usize) -> Rect {
		let n = self.edited.shells.len();
		Rect {
			x: self.shell_cols().grip,
			y: self.shell_line_y(i, n) + lay().shell_add_gap,
			w: self.row_btn_w,
			h: self.btn_h(),
		}
	}

	/// The rect the keyboard ring goes around, for any stop in the grid.
	pub(super) fn shell_stop_rect(&self, i: usize, part: u16) -> Rect {
		match shell_stop(part, self.edited.shells.len()) {
			ShellStop::Add => self.shell_add_box(i),
			ShellStop::Entry(shell_index, ShellPart::Name) => self.shell_name_box(i, shell_index),
			ShellStop::Entry(shell_index, ShellPart::Command) => self.shell_cmd_box(i, shell_index),
			ShellStop::Entry(shell_index, ShellPart::Active) => {
				self.shell_active_box(i, shell_index)
			}
			ShellStop::Entry(shell_index, ShellPart::Remove) => {
				self.shell_remove_box(i, shell_index)
			}
		}
	}

	/// Move one entry to another place in the list - the whole point of the grip.
	/// The list IS the order the Tabs menu offers, and its first switched-on line
	/// is the default shell, so a reorder is a real edit and not a view setting.
	pub(super) fn shell_move_to(&mut self, from: usize, to: usize) {
		let last = self.edited.shells.len().saturating_sub(1);
		let to = to.min(last);
		if from > last || from == to {
			return;
		}
		let entry = self.edited.shells.remove(from);
		self.edited.shells.insert(to, entry);
	}

	/// Where a pointer at `y` wants the dragged line to sit. Measured from the
	/// line's own TOP - the pointer keeps whatever offset inside the grip it took
	/// hold at - and rounded, so an entry changes place once it has travelled half
	/// a line rather than a whole one.
	///
	/// Both ends are clamped and neither needs a branch: a float-to-integer `as`
	/// saturates in Rust, so a line dragged off the top comes back 0 rather than
	/// wrapping, and `min` catches the other end.
	pub(super) fn shell_drop_at(&self, i: usize, y: f32, grab_dy: f32) -> usize {
		let last = self.edited.shells.len().saturating_sub(1);
		let line = self.shell_line_h().max(1.0);
		let offset = (y - grab_dy - self.shell_line_y(i, 0)) / line;
		(offset.round() as usize).min(last)
	}

	pub(super) fn shell_remove(&mut self, shell_index: usize) {
		if shell_index < self.edited.shells.len() {
			self.edited.shells.remove(shell_index);
		}
		self.commit_edit();
		self.focus = None;
	}

	// Add opens the new entry's Command field straight away: an entry with no
	// command names nothing to run, and is dropped rather than saved.
	fn shell_add(&mut self, i: usize) {
		self.commit_edit();
		let entry = crate::shells::adopted("", &self.edited.shells);
		self.edited.shells.push(entry);
		let shell_index = self.edited.shells.len() - 1;
		self.focus = Some(Focus::Row(
			i,
			shell_part_index(shell_index, ShellPart::Command),
		));
		self.open_edit(shell_field_row(shell_index, true), true);
	}

	/// A click somewhere in the grid. Returns whether it hit something.
	/// The move and remove buttons arm on press and fire on release, the same way
	/// the footer and the theme buttons do, so a press that drifts off cancels.
	pub(super) fn shell_mouse_down(
		&mut self,
		i: usize,
		x: f32,
		y: f32,
		measure: &mut impl FnMut(&str) -> f32,
	) -> bool {
		// The grip first, and outside the part walk: it is not a keyboard stop,
		// so there is no part index that names it.
		for shell_index in 0..self.edited.shells.len() {
			if self.shell_grip_box(i, shell_index).contains(x, y) {
				self.commit_edit();
				self.shell_drag = Some(ShellDrag {
					at: shell_index,
					grab_dy: y - self.shell_line_y(i, shell_index),
				});
				return true;
			}
		}
		for part in 0..self.parts_of(i) {
			if !self.shell_stop_rect(i, part).contains(x, y) {
				continue;
			}
			match shell_stop(part, self.edited.shells.len()) {
				ShellStop::Entry(shell_index, ShellPart::Name) => {
					let field = self.shell_name_box(i, shell_index);
					let row = shell_field_row(shell_index, false);
					self.field_click(row, Some((i, part)), field, x, measure);
				}
				ShellStop::Entry(shell_index, ShellPart::Command) => {
					let field = self.shell_cmd_box(i, shell_index);
					let row = shell_field_row(shell_index, true);
					self.field_click(row, Some((i, part)), field, x, measure);
				}
				ShellStop::Entry(shell_index, ShellPart::Active) => {
					self.focus = Some(Focus::Row(i, part));
					if let Some(entry) = self.edited.shells.get_mut(shell_index) {
						entry.active = !entry.active;
					}
				}
				// Add and Remove arm on press and fire on release, the same way
				// the footer and theme buttons do, so a press that drifts off
				// cancels - which matters most for the one that deletes a line.
				ShellStop::Add | ShellStop::Entry(_, ShellPart::Remove) => {
					self.focus = Some(Focus::Row(i, part));
					self.pressed_row = Some((i, part));
				}
			}
			return true;
		}
		false
	}

	/// Which of the shells grid's editable fields is under (x, y): its pseudo row,
	/// its box, and the tab stop it belongs to.
	pub(super) fn shell_field_at(&self, i: usize, x: f32, y: f32) -> Option<(usize, Rect, u16)> {
		for part in 0..self.parts_of(i) {
			if !self.shell_stop_rect(i, part).contains(x, y) {
				continue;
			}
			return match shell_stop(part, self.edited.shells.len()) {
				ShellStop::Entry(shell_index, ShellPart::Name) => Some((
					shell_field_row(shell_index, false),
					self.shell_name_box(i, shell_index),
					part,
				)),
				ShellStop::Entry(shell_index, ShellPart::Command) => Some((
					shell_field_row(shell_index, true),
					self.shell_cmd_box(i, shell_index),
					part,
				)),
				_ => None,
			};
		}
		None
	}

	/// The grid's own quads: the two field boxes and the checkbox per entry, the
	/// five icon buttons, and the Add button. The arrows are shader-drawn (mode 3
	/// with a quarter-turn) for the same reason the tab close mark is - no
	/// interface font can be relied on to carry one, and a glyph's own metrics
	/// decide where it goes.
	pub(super) fn shell_rects(
		&self,
		colors: &Dlg,
		i: usize,
		out: &mut Vec<RectInstance>,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		let scale = self.ui_scale();
		let mut field = |out: &mut Vec<RectInstance>, r: Rect, row: usize, part: u16| {
			out.push(quad(r.x, r.y, r.w, r.h, colors.field_bg));
			let focused = matches!(&self.edit, Some(edit) if edit.row == row);
			if !self.ring_on(i, part) {
				border(
					out,
					r,
					1.0,
					if focused {
						colors.focus_out
					} else {
						colors.panel_border
					},
				);
			}
			if focused {
				self.caret_quad(colors, out, r, measure);
			}
		};
		for shell_index in 0..self.edited.shells.len() {
			let name = self.shell_name_box(i, shell_index);
			field(
				out,
				name,
				shell_field_row(shell_index, false),
				shell_part_index(shell_index, ShellPart::Name),
			);
			let cmd = self.shell_cmd_box(i, shell_index);
			field(
				out,
				cmd,
				shell_field_row(shell_index, true),
				shell_part_index(shell_index, ShellPart::Command),
			);
			// Active checkbox, drawn the way every other checkbox in the dialog is
			let box_r = self.shell_active_box(i, shell_index);
			out.push(quad(box_r.x, box_r.y, box_r.w, box_r.h, colors.field_bg));
			border(out, box_r, 1.0, colors.panel_border);
			if self
				.edited
				.shells
				.get(shell_index)
				.is_some_and(|e| e.active)
			{
				let inset = (box_r.w * 0.25).max(3.0);
				out.push(quad(
					box_r.x + inset,
					box_r.y + inset,
					box_r.w - inset * 2.0,
					box_r.h - inset * 2.0,
					colors.handle,
				));
			}
			// The grip: three stacked bars, the shape every reorderable list uses.
			// No box and no border around it - it is a texture to grab, not a
			// button to press, and drawing it as one would invite a click that
			// does nothing. Plain quads, so it costs no shader mode at all.
			let grip = self.shell_grip_box(i, shell_index);
			let held = self
				.shell_drag
				.as_ref()
				.is_some_and(|d| d.at == shell_index);
			let bar_h = (1.0 * scale).max(1.0);
			let bar_w = (grip.w * 0.56).max(4.0);
			let bar_x = grip.x + (grip.w - bar_w) / 2.0;
			let pitch = bar_h * 3.0;
			let stack = bar_h + pitch * 2.0;
			let top = grip.y + (grip.h - stack) / 2.0;
			for n in 0..3 {
				out.push(quad(
					bar_x,
					top + n as f32 * pitch,
					bar_w,
					bar_h,
					if held { colors.handle } else { colors.dim },
				));
			}
			// Remove, between the command and the date. Red, because it is the
			// one control in the whole dialog that destroys something.
			let r = self.shell_remove_box(i, shell_index);
			out.push(quad(r.x, r.y, r.w, r.h, colors.btn_bg));
			if !self.ring_on(i, shell_part_index(shell_index, ShellPart::Remove)) {
				border(out, r, 1.0, colors.panel_border);
			}
			out.push(RectInstance {
				pos: [r.x, r.y],
				size: [r.w, r.h],
				color: config::srgb_f32(colors.danger),
				params: [QuadMode::CloseMark.code(), (r.w * 0.12).max(1.2)],
			});
		}
		let add = self.shell_add_box(i);
		out.push(quad(add.x, add.y, add.w, add.h, colors.btn_bg));
		if !self.ring_on(i, self.parts_of(i).saturating_sub(1)) {
			border(out, add, 1.0, colors.panel_border);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::super::SettingsDialog;
	use super::super::lay;
	use super::super::tests::{mk_dialog, shell_entry};
	use crate::gfx::QuadMode;
	use crate::ui_spec::Kind;

	// The shells grid is one spec row carrying two editable fields per entry, and
	// the right-click handler walked spec rows - so those two were the only fields
	// in the dialog with no menu, while the Menu key worked on them.
	// Test ID: EpHbI5g
	#[test]
	fn the_shells_grid_fields_have_a_right_click_menu() {
		let mut m = |s: &str| s.chars().count() as f32;
		let mut d = mk_dialog(4000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, Kind::ShellList))
			.expect("the shells grid");
		d.tab = d.specs[i].tab;
		// its own entry: the live config decides what is in the list otherwise,
		// and another test can be holding a config override while this runs
		d.edited.shells = vec![crate::shells::ShellEntry {
			slug: "bash".into(),
			title: "Bash".into(),
			command: "/bin/bash".into(),
			active: true,
			comment: String::new(),
			last_seen: String::new(),
		}];

		for command in [false, true] {
			let field = if command {
				d.shell_cmd_box(i, 0)
			} else {
				d.shell_name_box(i, 0)
			};
			d.mouse_right_dip(field.x + 4.0, field.y + field.h / 2.0, true, &mut m);
			assert!(
				d.emenu.is_some(),
				"no menu on the {} field",
				if command { "command" } else { "name" }
			);
			assert_eq!(
				d.edit.as_ref().map(|e| e.row),
				Some(super::super::shell_field_row(0, command)),
				"the menu acts on the field that was clicked"
			);
			d.emenu = None;
		}

		// and a click on the grid away from either field opens nothing
		let row = d.shell_line_y(i, 0);
		d.mouse_right_dip(d.rect.x + 1.0, row, true, &mut m);
		assert!(d.emenu.is_none(), "a menu appeared off the fields");
	}

	// A dialog sitting on the Shell tab with `n` shells in it.
	fn mk_shell_dialog(n: usize) -> (SettingsDialog, usize) {
		let mut d = mk_dialog(4000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, super::super::Kind::ShellList))
			.expect("a shells grid");
		d.tab = d.specs[i].tab;
		d.edited.shells = (0..n)
			.map(|k| shell_entry(&format!("Shell {k}"), &format!("/bin/sh{k}")))
			.collect();
		d.orig.shells.clone_from(&d.edited.shells);
		(d, i)
	}

	// Every control on every line is its own stop, and a part index names
	// exactly one of them - the encoding both the focus ring and the hit tests
	// read, so a mistake here would put the ring on one control and the click on
	// another.
	// Test ID: EnQUIKr
	#[test]
	fn a_part_index_names_one_control_on_one_line() {
		let (d, i) = mk_shell_dialog(3);
		assert_eq!(d.parts_of(i), 3 * super::super::ShellPart::COUNT + 1);
		for k in 0..3 {
			for part in super::super::ShellPart::ALL {
				let p = super::shell_part_index(k, part);
				match super::super::shell_stop(p, 3) {
					super::super::ShellStop::Entry(entry, named) => {
						assert_eq!((entry, named), (k, part));
					}
					super::super::ShellStop::Add => panic!("{k}/{part:?} read as the Add button"),
				}
			}
		}
		assert!(matches!(
			super::super::shell_stop(d.parts_of(i) - 1, 3),
			super::super::ShellStop::Add
		));
	}

	// Reordering left the keyboard when the arrows did, so the grip must not be a
	// stop - a ring sitting on a control that Space cannot work is worse than no
	// ring at all. Nothing in the grid is grayed any more either.
	// Test ID: EnRWor4
	#[test]
	fn the_grip_is_a_gesture_and_not_a_keyboard_stop() {
		let (d, i) = mk_shell_dialog(3);
		assert_eq!(d.parts_of(i), 3 * super::super::ShellPart::COUNT + 1);
		for part in 0..d.parts_of(i) {
			assert!(!d.part_disabled(i, part), "part {part} came up grayed");
		}
		// no stop draws where the grip does
		for k in 0..3 {
			let grip = d.shell_grip_box(i, k);
			let (cx, cy) = (grip.x + grip.w / 2.0, grip.y + grip.h / 2.0);
			for part in 0..d.parts_of(i) {
				assert!(
					!d.shell_stop_rect(i, part).contains(cx, cy),
					"part {part} hit-tests over line {k}'s grip"
				);
			}
		}
	}

	// The grip's whole job. Dragging is live - the list reorders under the
	// pointer rather than on release - so each step of the gesture is asserted,
	// not just where it ended up.
	// Test ID: EnRWor5
	#[test]
	fn a_grip_drag_reorders_the_list() {
		let (mut d, i) = mk_shell_dialog(3);
		let mut measure = |s: &str| s.chars().count() as f32 * 7.0;
		let line = d.shell_line_h();
		let titles = |d: &super::super::SettingsDialog| -> Vec<String> {
			d.edited.shells.iter().map(|e| e.title.clone()).collect()
		};
		let grip = d.shell_grip_box(i, 0);
		let (x, y) = (grip.x + grip.w / 2.0, grip.y + grip.h / 2.0);
		assert!(
			d.shell_mouse_down(i, x, y, &mut measure),
			"the grip did not take the press"
		);
		// less than half a line is not yet a move
		d.mouse_move_dip(x, y + line * 0.4, &mut measure);
		assert_eq!(titles(&d), ["Shell 0", "Shell 1", "Shell 2"]);
		// past half, it swaps with the line below
		d.mouse_move_dip(x, y + line * 0.6, &mut measure);
		assert_eq!(titles(&d), ["Shell 1", "Shell 0", "Shell 2"]);
		// and keeps going, without letting go
		d.mouse_move_dip(x, y + line * 2.0, &mut measure);
		assert_eq!(titles(&d), ["Shell 1", "Shell 2", "Shell 0"]);
		// dragged past the end it stops at the end rather than vanishing
		d.mouse_move_dip(x, y + line * 40.0, &mut measure);
		assert_eq!(titles(&d), ["Shell 1", "Shell 2", "Shell 0"]);
		d.mouse_up_dip(x, y + line * 40.0);
		assert!(d.shell_drag.is_none(), "the drag outlived the release");
		// and a plain move afterwards moves nothing
		d.mouse_move_dip(x, y, &mut measure);
		assert_eq!(titles(&d), ["Shell 1", "Shell 2", "Shell 0"]);
	}

	// Dragged the other way, and off the top: the first line is as far as it
	// goes. The arithmetic is in f32 and ends on a usize, so this is the test
	// that says the saturating cast is being RELIED on rather than tolerated.
	// Test ID: EnRWor6
	#[test]
	fn a_line_dragged_off_the_top_ends_up_first() {
		let (mut d, i) = mk_shell_dialog(3);
		let mut measure = |s: &str| s.chars().count() as f32 * 7.0;
		let line = d.shell_line_h();
		let grip = d.shell_grip_box(i, 2);
		let (x, y) = (grip.x + grip.w / 2.0, grip.y + grip.h / 2.0);
		assert!(d.shell_mouse_down(i, x, y, &mut measure));
		d.mouse_move_dip(x, y - line * 99.0, &mut measure);
		let titles: Vec<&str> = d.edited.shells.iter().map(|e| e.title.as_str()).collect();
		assert_eq!(titles, ["Shell 2", "Shell 0", "Shell 1"]);
		d.mouse_up_dip(x, y);
	}

	// The command is REQUIRED: emptying the field cannot be what stores an entry
	// that names nothing to run. The stored value stands and the box shows it
	// again, which is the whole rule at the field.
	// Test ID: EnQUIKs
	#[test]
	fn a_blank_command_cannot_replace_a_stored_one() {
		let (mut d, _) = mk_shell_dialog(2);
		let row = super::super::shell_field_row(1, true);
		d.open_edit(row, true);
		d.select_all();
		d.backspace();
		assert_eq!(
			d.edited.shells[1].command, "/bin/sh1",
			"an emptied field leaves the stored command standing"
		);
		// a real value does apply, so the guard is not simply inert
		d.insert_str("/usr/bin/fish");
		assert_eq!(d.edited.shells[1].command, "/usr/bin/fish");
		assert_eq!(d.edited.shells[0].command, "/bin/sh0", "and only that one");
	}

	// A name edit applies to the entry it was opened for, not on the row index -
	// the two are different numbers here, which is the whole point of the
	// pseudo-row scheme.
	// Test ID: EnQUIKt
	#[test]
	fn a_field_edit_goes_to_its_own_entry() {
		let (mut d, _) = mk_shell_dialog(3);
		d.open_edit(super::super::shell_field_row(2, false), true);
		d.select_all();
		d.insert_str("Renamed");
		assert_eq!(d.edited.shells[2].title, "Renamed");
		assert_eq!(d.edited.shells[0].title, "Shell 0");
		assert_eq!(d.edited.shells[1].title, "Shell 1");
	}

	// Add creates an entry with no command and puts the caret straight in it - the
	// one field that has to be filled before the entry means anything.
	// Test ID: EnQUIKu
	#[test]
	fn adding_a_shell_opens_the_field_it_needs() {
		let (mut d, i) = mk_shell_dialog(1);
		d.shell_add(i);
		assert_eq!(d.edited.shells.len(), 2);
		assert!(d.edited.shells[1].command.is_empty());
		assert_eq!(
			d.edit.as_ref().map(|e| e.row),
			Some(super::super::shell_field_row(1, true))
		);
		assert_eq!(
			d.focus,
			Some(super::super::Focus::Row(
				i,
				super::shell_part_index(1, super::super::ShellPart::Command)
			))
		);
	}

	// Removing is the one grid action doing the opposite cannot undo, so it asks
	// first - and asking must not remove anything by itself.
	// Test ID: EnQUIKv
	#[test]
	fn removing_a_shell_asks_before_it_happens() {
		let (mut d, _) = mk_shell_dialog(3);
		d.shell_confirm_remove(1);
		assert_eq!(
			d.edited.shells.len(),
			3,
			"the question alone changes nothing"
		);
		assert!(matches!(
			d.prompt.as_ref().map(|p| p.job),
			Some(super::super::PromptJob::DropShell(1))
		));
		d.prompt_accept();
		let titles: Vec<&str> = d.edited.shells.iter().map(|e| e.title.as_str()).collect();
		assert_eq!(titles, vec!["Shell 0", "Shell 2"]);
		assert!(d.prompt.is_none());
	}

	// The columns are laid out from both ends with the command taking the slack,
	// so the one thing that can go wrong is them meeting in the middle. The
	// order asserted here is the order asked for: remove sits between the
	// command and the date, where it is hard to press by accident.
	// Test ID: EnQUIKw
	#[test]
	fn the_grid_columns_stay_inside_the_panel_in_order() {
		let (d, i) = mk_shell_dialog(2);
		let left = d.rect.x + super::super::lay().pad;
		let right = d.rect.x + d.rect.w - super::super::lay().pad;
		let cols = d.shell_cols();
		for k in 0..2 {
			let grip = d.shell_grip_box(i, k);
			let name = d.shell_name_box(i, k);
			let cmd = d.shell_cmd_box(i, k);
			let remove = d.shell_remove_box(i, k);
			let active = d.shell_active_box(i, k);
			assert!(grip.x >= left - 0.01, "the grip starts inside the panel");
			assert!(grip.x + grip.w <= name.x + 0.01, "grip runs into the name");
			assert!(name.x + name.w <= cmd.x + 0.01, "name runs into command");
			assert!(cmd.w > 0.0, "the command column collapsed");
			assert!(cmd.x + cmd.w <= remove.x + 0.01, "command runs into remove");
			assert!(
				remove.x + remove.w <= cols.seen + 0.01,
				"remove runs into the date"
			);
			assert!(
				cols.seen + super::super::lay().shell_seen_width <= active.x + 0.01,
				"the date runs into active"
			);
			assert!(
				active.x + active.w <= right + 0.01,
				"the last column overruns the panel"
			);
			// and every line sits below the column titles
			assert!(d.shell_line_y(i, k) > d.shell_head_y(i));
		}
	}

	// The look of the two changed columns, asserted in the quads themselves,
	// because the geometry tests above pass just as happily on a grid that draws
	// the old arrows. Three things: the grip is three bars and not a button, the
	// remove mark is drawn in the danger colour and not the text colour, and no
	// arrow survives anywhere in the grid.
	// Test ID: EnRaGh6
	#[test]
	fn the_grip_reads_as_bars_and_the_remove_mark_reads_as_red() {
		let (mut d, i) = mk_shell_dialog(2);
		d.tab = d.specs[i].tab;
		let mut measure = |s: &str| s.chars().count() as f32 * 7.0;
		let (_, rows) = d.rects_dip(d.line_h, &mut measure);
		let danger = super::super::config::srgb_f32(super::super::dlg().danger);
		let dim = super::super::config::srgb_f32(super::super::dlg().dim);

		// the X: one per line, in the danger colour, inside the remove box
		let marks: Vec<_> = rows
			.iter()
			.filter(|r| r.mode() == QuadMode::CloseMark)
			.collect();
		assert_eq!(marks.len(), 2, "one remove mark per line");
		for (k, mark) in marks.iter().enumerate() {
			assert_eq!(
				mark.color, danger,
				"the remove mark is not the danger colour"
			);
			assert_ne!(
				mark.color,
				super::super::config::srgb_f32(super::super::dlg().text),
				"the remove mark reads as ordinary text"
			);
			let box_r = d.shell_remove_box(i, k);
			assert!((mark.pos[0] - box_r.x).abs() < 0.01, "mark off its own box");
		}

		// the grip: three bars of one width, stacked inside the grip column
		for k in 0..2 {
			let grip = d.shell_grip_box(i, k);
			let bars: Vec<_> = rows
				.iter()
				.filter(|r| {
					r.color == dim
						&& r.pos[0] > grip.x - 0.01
						&& r.pos[0] + r.size[0] < grip.x + grip.w + 0.01
						&& r.pos[1] >= grip.y - 0.01
						&& r.pos[1] <= grip.y + grip.h + 0.01
				})
				.collect();
			assert_eq!(bars.len(), 3, "line {k}'s grip is not three bars");
			assert!(
				bars.iter()
					.all(|b| (b.size[0] - bars[0].size[0]).abs() < 0.01),
				"the grip's bars are not one width"
			);
			let mut ys: Vec<f32> = bars.iter().map(|b| b.pos[1]).collect();
			ys.sort_by(f32::total_cmp);
			assert!(ys[0] < ys[1] && ys[1] < ys[2], "the bars are not stacked");
			assert!(
				(ys[1] - ys[0] - (ys[2] - ys[1])).abs() < 0.01,
				"the bars are not evenly spaced"
			);
		}

		// and nothing in the grid still draws a triangle
		assert!(
			!rows.iter().any(|r| r.mode() == QuadMode::Triangle),
			"an arrow is still being drawn in the shells grid"
		);
	}

	// The draw path has no other cover, and it is where a new control kind fails
	// silently: a grid that renders nothing at all still passes every geometry
	// test above. So this walks what would actually be handed to the renderer.
	// Test ID: EnQWzbk
	#[test]
	fn the_grid_draws_a_line_for_every_shell() {
		let (d, i) = mk_shell_dialog(3);
		let mut measure = |s: &str| s.chars().count() as f32 * 7.0;
		let texts = d.texts_dip(d.line_h, &mut measure);
		let said: Vec<&str> = texts.iter().map(|t| t.text.as_str()).collect();
		for want in [
			"Name",
			"Command",
			"Last seen",
			"Active",
			"Add",
			"Shell 0",
			"Shell 1",
			"Shell 2",
			"/bin/sh0",
			"/bin/sh2",
		] {
			assert!(
				said.contains(&want),
				"the grid never drew {want:?}: {said:?}"
			);
		}
		// a shell no scan has vouched for says so rather than leaving a blank
		assert_eq!(
			said.iter().filter(|t| **t == "never").count(),
			3,
			"one 'last seen' per line"
		);
		// and the quads scale with the list rather than being drawn once
		let count = |n: usize| {
			let (mut d, _) = mk_shell_dialog(n);
			d.tab = d.specs[i].tab;
			let mut m = |s: &str| s.chars().count() as f32 * 7.0;
			let (fixed, rows) = d.rects_dip(d.line_h, &mut m);
			let _ = fixed;
			rows.len()
		};
		let (one, three) = (count(1), count(3));
		assert!(
			three > one && three - one == 2 * (one - count(0)),
			"each line costs the same quads: 0->{} 1->{one} 3->{three}",
			count(0)
		);
	}

	// An entry being edited shows the BUFFER, not the stored value - the field
	// would otherwise look inert while it is typed into.
	// Test ID: EnQWzbl
	#[test]
	fn a_grid_field_being_edited_shows_what_is_typed() {
		let (mut d, _) = mk_shell_dialog(2);
		d.open_edit(super::super::shell_field_row(1, false), true);
		d.select_all();
		d.insert_str("Half typed");
		let mut measure = |s: &str| s.chars().count() as f32 * 7.0;
		let texts = d.texts_dip(d.line_h, &mut measure);
		let said: Vec<&str> = texts.iter().map(|t| t.text.as_str()).collect();
		assert!(said.contains(&"Half typed"));
		assert!(said.contains(&"Shell 0"), "and the other line is untouched");
	}

	// A scan that arrives while the dialog is open moves BOTH copies, so it does not
	// read as an edit the user made - and it folds into what they have already
	// done rather than replacing it.
	// Test ID: EnQUIKx
	#[test]
	fn a_scan_that_arrives_mid_edit_is_not_mistaken_for_an_edit() {
		let (mut d, _) = mk_shell_dialog(1);
		d.edited.shells[0].title = "Renamed by hand".into();
		// the command is the one already stored, so nothing is added
		let found = vec![crate::shells::Found::new("Fish", "/bin/sh0".into(), "")];
		d.fold_shells(&found);
		assert_eq!(
			d.edited.shells[0].title, "Renamed by hand",
			"a scan never rewrites what the user typed"
		);
		assert_eq!(d.orig.shells.len(), d.edited.shells.len());
	}

	// The Active box on a shell's line is a checkbox like any other: square, and
	// centered on its line with the fields beside it.
	// Test ID: Er2X6Et
	#[test]
	fn a_shells_active_box_is_square_and_centered_on_its_line() {
		let (d, i) = mk_shell_dialog(2);
		for k in 0..2 {
			let active = d.shell_active_box(i, k);
			let name = d.shell_name_box(i, k);
			assert!((active.w - lay().swatch).abs() < 0.01);
			assert!((active.h - lay().swatch).abs() < 0.01);
			assert!(
				(active.y + active.h / 2.0 - (name.y + name.h / 2.0)).abs() < 0.5,
				"line {k}'s box is off the line's center"
			);
		}
	}
}
