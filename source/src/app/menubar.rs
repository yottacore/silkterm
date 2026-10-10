// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The menu bar: its titles, the Alt underlines, and the copy-mode boxes at its
//! right end.

use super::{
	COPYBOX_BOX_GAP, COPYBOX_LABELS, COPYBOX_LEAD_GAP, COPYBOX_PAIR_GAP, MENU_BAR, MENU_BAR_PAD,
	State, save_live,
};
use crate::pane::{CopyKind, Rect};
use crate::term::PaneId;
use crate::text::TextCtx;

/// macOS has no in-window menu bar, since the system menu bar carries the same
/// menus and a second bar would break the Mac's own menu contract. `--hide-menu`
/// is accepted there and does nothing.
pub(super) fn menu_bar_at_launch(hide_menu: Option<bool>, mac: bool) -> bool {
	!mac && !hide_menu.unwrap_or(false)
}

/// The menu bar's right-side copy-mode cluster: "Copy on [ ] select [ ] output".
/// Drawing, label placement, and click hit-testing all read this one layout.
#[derive(Debug)]
pub(super) struct CopyBoxes {
	pub(super) boxes: [Rect; 2], // select, output checkbox squares
	label_x: [f32; 3],           // left edge per COPYBOX_LABELS entry
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

/// Where menu-bar buffer `i` is drawn: (left, clip left, clip right, top). The
/// titles come first, then the right-aligned copy-mode labels, and at a narrow
/// width some of those are not there at all.
///
/// Everything on this bar sits on ONE baseline. The copy labels used to center
/// their full ink box, which reads better on its own but left them half a
/// descent above the titles beside them.
pub(super) fn menubar_text_slot(
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

/// The top-level menu Alt plus `ch` opens: the one whose title starts with it.
pub(super) fn bar_menu_for(ch: char) -> Option<usize> {
	let ch = ch.to_ascii_uppercase();
	MENU_BAR.iter().position(|title| title.starts_with(ch))
}

/// Which bar titles get their accelerator underlined, and on which letter: all
/// of them while Alt is held, none while a dropdown is open, since the dropdown
/// underlines its own rows then.
pub(super) fn bar_title_underlines(
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

impl State {
	/// Per-title (`x_left`, width) layout of the menu bar, used for drawing and
	/// hit-testing so they can't disagree. Titles use the proportional font.
	pub(super) fn menubar_layout(&mut self) -> [(f32, f32); MENU_BAR.len()] {
		let attrs = crate::text::ui_attrs();
		let mut x = 0.0;
		MENU_BAR.map(|title| {
			let w = self.text.measure_ui_text(title, &attrs) + self.text.dip(MENU_BAR_PAD) * 2.0;
			let at = (x, w);
			x += w;
			at
		})
	}

	pub(super) fn menubar_hit(&mut self, mx: f32) -> Option<usize> {
		self.menubar_layout()
			.iter()
			.position(|&(x, w)| mx >= x && mx < x + w)
	}

	/// The "Copy on [ ] select [ ] output" pair on the right of the menu bar. The
	/// user has to be able to see when the focused pane is auto-copying, so the
	/// cluster sheds parts rather than shrinking: the lead-in goes first, then the
	/// two words, and only when even the boxes cannot clear the menu titles does
	/// the whole thing go (None). Overlapping text says less about the copy state
	/// than a clean absence does. It comes back on its own as the window widens.
	/// `label_x/label_w` index-match `COPYBOX_LABELS`.
	pub(super) fn copybox_layout(&mut self) -> Option<CopyBoxes> {
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

	/// Which copy-mode checkbox (the square or its word) a menu-bar click hit.
	pub(super) fn copybox_hit(&mut self, mx: f32) -> Option<CopyKind> {
		let cb = self.copybox_layout()?;
		for (i, kind) in [CopyKind::Select, CopyKind::Output].into_iter().enumerate() {
			let (left, right) = cb.hit_range(i);
			if mx >= left && mx <= right {
				return Some(kind);
			}
		}
		None
	}

	/// Flip one of a pane's two auto-copy triggers. The two are independent and can
	/// both be on; nothing else is touched (other panes/tabs/windows keep theirs -
	/// only the focused pane of the active tab actually copies, gated at copy time).
	/// A toggle from a context menu on an unfocused pane focuses it so the menu-bar
	/// checkboxes reflect the pane just changed.
	pub(super) fn toggle_copy(&mut self, target: PaneId, kind: CopyKind) {
		let Some(p) = self.tabs.find_pane_mut(target) else {
			return;
		};
		let now = !p.copy_enabled(kind);
		p.set_copy(kind, now);
		// the last choice is what the next launch and new tabs start with.
		// Other panes keep their own, which is why this skips apply_new_settings.
		if kind == CopyKind::Select {
			save_live(|live| {
				let changed = live.copy_on_select != now;
				let on = knobs::Value::Bool(now);
				crate::fields::set(live, crate::ui_spec::Key::CopyOnSelect, &on, None);
				changed
			});
		}
		if self.tabs.cur().panes.contains_key(&target) {
			self.tabs.cur_mut().focused = target;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::super::{MENU_BAR, MENU_BAR_VPAD};
	use super::{
		CopyBoxes, CopyMetrics, bar_menu_for, bar_title_underlines, copybox_fit, copybox_place,
		menubar_text_slot,
	};
	use crate::text::TextCtx;
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
}
