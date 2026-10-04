// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Flyover help: the parts every tip in the program shares.
//!
//! There are four places a tip comes up - a Settings row, a link in the About
//! box, a tab in the strip, and a menu item - drawn by two different renderers
//! in two different fonts. What they have in common is not the drawing: it is
//! how long the pointer has to rest before a tip appears, how the text is
//! broken to fit, and where the box goes relative to what it describes. Those
//! three live here, and each caller draws the result its own way.

use std::time::{Duration, Instant};

use crate::config;
use crate::pane::Rect;

// How long the pointer rests on something before its tip comes up. One value
// for every tip in the program - a menu that answered faster than the tab strip
// would read as a different kind of thing.
pub const DELAY: Duration = Duration::from_millis(600);

// The box itself, DIP (see config::dip). One set for every tip the two dialogs
// draw: a box that sat closer to its control in one window than in the other
// would read as a different kind of thing, the same argument as the delay.
pub const PAD_X: f32 = 8.0;
pub const PAD_Y: f32 = 4.0;
pub const DROP: f32 = 8.0; // offset below the control it describes
pub const EDGE: f32 = 4.0; // closest it may sit to a window edge
pub const BORDER: f32 = 1.0;
const WRAP_MARGIN: f32 = 8.0; // window width kept clear of a wrapped tip
const MIN_WRAP: f32 = 40.0; // a wrap budget never narrower than this

// Greedy word wrap, measured in whatever font the caller draws in. A single
// word wider than the budget still gets its own line rather than being split -
// breaking mid-word would be worse than a tip that overhangs by one long word.
pub fn wrap(text: &str, max_w: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
	let mut lines: Vec<String> = Vec::new();
	let mut line = String::new();
	for word in text.split_whitespace() {
		let candidate = if line.is_empty() {
			word.to_string()
		} else {
			format!("{line} {word}")
		};
		if !line.is_empty() && measure(&candidate) > max_w {
			lines.push(std::mem::take(&mut line));
			line = word.to_string();
		} else {
			line = candidate;
		}
	}
	if !line.is_empty() {
		lines.push(line);
	}
	if lines.is_empty() {
		lines.push(String::new());
	}
	lines
}

// Where a tip box goes: centered under what it describes, or above it when
// there is no room below, and never off an edge. Clamping into the bottom edge
// instead of flipping would sit a footer button's own tip on the buttons it is
// describing, which is the case that made the flip necessary.
pub fn place(anchor: Rect, size: (f32, f32), win: (f32, f32), gap: f32, edge: f32) -> (f32, f32) {
	let (box_w, box_h) = size;
	let (win_w, win_h) = win;
	let x = (anchor.x + anchor.w * 0.5 - box_w * 0.5).clamp(edge, (win_w - box_w - edge).max(edge));
	let below = anchor.y + anchor.h + gap;
	let y = if below + box_h + edge <= win_h {
		below
	} else {
		(anchor.y - gap - box_h).max(edge)
	};
	(x, y)
}

// Everything a caller needs to draw one tip, in physical pixels: the rule round
// the box, the box itself, and where its first line of text starts. Lines after
// the first step down by the caller's own line height.
#[derive(Debug)]
pub struct Placed {
	pub border: Rect,
	pub fill: Rect,
	pub text_x: f32,
	pub text_y: f32,
}

// Lay a tip out. `text_w` is the widest line and `line_h` the line height, both
// already measured in the font the caller draws in, so both arrive physical.
// Every number the box brings itself is a DIP converted once here, which is what
// makes a tip at twice the scale the 1x tip doubled.
pub fn lay_out(
	anchor: Rect,
	lines: usize,
	text_w: f32,
	line_h: f32,
	win: (f32, f32),
	scale: f32,
) -> Placed {
	let pad_x = config::dip(PAD_X, scale);
	let pad_y = config::dip(PAD_Y, scale);
	let border = config::dip(BORDER, scale);
	let box_w = text_w + pad_x * 2.0;
	let box_h = line_h * lines.max(1) as f32 + pad_y * 2.0;
	let (x, y) = place(
		anchor,
		(box_w, box_h),
		win,
		config::dip(DROP, scale),
		config::dip(EDGE, scale),
	);
	Placed {
		border: Rect {
			x: x - border,
			y: y - border,
			w: box_w + border * 2.0,
			h: box_h + border * 2.0,
		},
		fill: Rect {
			x,
			y,
			w: box_w,
			h: box_h,
		},
		text_x: x + pad_x,
		text_y: y + pad_y,
	}
}

// How wide a tip's text may run before it wraps: the window, less a margin and
// the box's own padding. A tip wraps rather than being clamped to the window
// edge, so neither a longer sentence nor a larger interface font runs off it.
pub fn wrap_budget(win_w: f32, scale: f32) -> f32 {
	(win_w - config::dip(WRAP_MARGIN, scale) - config::dip(PAD_X, scale) * 2.0)
		.max(config::dip(MIN_WRAP, scale))
}

// Where a tip goes when it must not cover what it describes: clear of the
// anchor's right edge, flipped to its left when there is no room there, and top
// aligned with it. A menu tip needs this - a box centered under the row would
// sit on the rows below it, which are exactly what the reader is choosing
// between.
pub fn beside(anchor: Rect, size: (f32, f32), win: (f32, f32), gap: f32, edge: f32) -> (f32, f32) {
	let (box_w, box_h) = size;
	let (win_w, win_h) = win;
	let right = anchor.x + anchor.w + gap;
	let x = if right + box_w + edge <= win_w {
		right
	} else {
		(anchor.x - gap - box_w).max(edge)
	};
	let y = anchor.y.clamp(edge, (win_h - box_h - edge).max(edge));
	(x, y)
}

// What the pointer is resting on, and since when. `T` names the thing in
// whatever terms the caller thinks in - a tab index, a menu row - so the timing
// rule is written once and the identity stays the caller's business.
#[derive(Debug)]
pub struct Dwell<T> {
	over: Option<(T, Instant)>,
}

// By hand rather than derived: the derive would want `T: Default` too, and what
// a caller points at - a rect, a tab index - has no default worth naming.
impl<T> Default for Dwell<T> {
	fn default() -> Self {
		Self { over: None }
	}
}

impl<T: Copy + PartialEq> Dwell<T> {
	// Point at something, or at nothing. The clock runs on while the target is
	// unchanged, and restarts when it is not. True means the caller has to
	// redraw: a tip that was up is now pointing somewhere else, or at nothing.
	pub fn point_at(&mut self, target: Option<T>) -> bool {
		match (target, &self.over) {
			(Some(want), Some((have, _))) if *have == want => false,
			(Some(want), _) => {
				let was_ripe = self.ripe().is_some();
				self.over = Some((want, Instant::now()));
				was_ripe
			}
			(None, None) => false,
			(None, Some(_)) => {
				let was_ripe = self.ripe().is_some();
				self.over = None;
				was_ripe
			}
		}
	}

	// What the pointer has rested on long enough to deserve a tip, if anything.
	pub fn ripe(&self) -> Option<T> {
		let (target, since) = self.over.as_ref()?;
		(Instant::now().duration_since(*since) >= DELAY).then_some(*target)
	}

	// `ripe`, for a tip that should not stay up forever. After `limit` it goes
	// down, and stays down until the pointer leaves and comes back, since only
	// a new target restarts the clock. A zero limit never takes it down.
	pub fn ripe_for(&self, limit: Duration) -> Option<T> {
		self.ripe_at(Instant::now(), limit)
	}

	fn ripe_at(&self, now: Instant, limit: Duration) -> Option<T> {
		let (target, since) = self.over.as_ref()?;
		let up = now.saturating_duration_since(*since).checked_sub(DELAY)?;
		(limit.is_zero() || up < limit).then_some(*target)
	}

	// When the loop next has to wake to raise a tip. None while nothing is being
	// pointed at, and while one is already up.
	pub fn wake(&self) -> Option<Instant> {
		let (_, since) = self.over.as_ref()?;
		let due = *since + DELAY;
		(due > Instant::now()).then_some(due)
	}
}

#[cfg(test)]
mod tests {
	use super::{DELAY, Dwell, beside, lay_out, place, wrap, wrap_budget};
	use crate::pane::Rect;
	use std::time::Duration;

	fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
		Rect { x, y, w, h }
	}

	// Six pixels a character is enough to make the arithmetic obvious.
	fn measure(s: &str) -> f32 {
		s.chars().count() as f32 * 6.0
	}

	// A tip has to fit whatever it hangs off, whatever the font does to the text
	// - one clamped to a window edge simply runs off it.
	// Test ID: EoThLQe
	#[test]
	fn a_tip_wraps_on_words_and_never_splits_one() {
		let lines = wrap(
			"Apply changes now, without closing Settings.",
			120.0,
			measure,
		);
		assert!(lines.len() > 1);
		assert!(lines.iter().all(|line| measure(line) <= 120.0));
		assert_eq!(
			lines.join(" "),
			"Apply changes now, without closing Settings.",
			"wrapping lost or added text"
		);
		// a word too long for the budget still gets a line of its own
		assert_eq!(
			wrap("a supercalifragilistic word", 48.0, measure),
			vec!["a", "supercalifragilistic", "word"]
		);
		// text that already fits stays on one line, and empty text is one empty
		// line rather than none
		assert_eq!(wrap("short", 240.0, measure), vec!["short"]);
		assert_eq!(wrap("", 100.0, measure), vec![""]);
	}

	// A tip that cannot fit below what it describes flips above it, rather than
	// clamping into the bottom edge and covering it.
	// Test ID: EoThLQf
	#[test]
	fn a_tip_with_no_room_below_goes_above() {
		let anchor = rect(100.0, 40.0, 60.0, 20.0);
		let (x, y) = place(anchor, (80.0, 30.0), (400.0, 300.0), 8.0, 4.0);
		assert_eq!((x, y), (90.0, 68.0));
		// the same anchor near the bottom of a short window
		let low = rect(100.0, 250.0, 60.0, 20.0);
		let (_, up) = place(low, (80.0, 30.0), (400.0, 300.0), 8.0, 4.0);
		assert_eq!(up, 212.0);
	}

	// Test ID: EoThLQg
	#[test]
	fn a_tip_stays_inside_both_side_edges() {
		let win = (400.0, 300.0);
		let (left, _) = place(rect(0.0, 10.0, 10.0, 10.0), (80.0, 30.0), win, 8.0, 4.0);
		assert_eq!(left, 4.0);
		let (right, _) = place(rect(390.0, 10.0, 10.0, 10.0), (80.0, 30.0), win, 8.0, 4.0);
		assert_eq!(right, 316.0);
	}

	// Every measurement the box brings itself is a DIP converted once, so the same
	// tip on a 2x display is the 1x one doubled. A number left in raw pixels shows
	// up here as a box that grew by less than its text did.
	// Test ID: Eq4Llx3
	#[test]
	fn a_tip_at_twice_the_scale_is_the_1x_tip_doubled() {
		let one = lay_out(
			rect(100.0, 60.0, 80.0, 24.0),
			2,
			150.0,
			18.0,
			(600.0, 400.0),
			1.0,
		);
		let two = lay_out(
			rect(200.0, 120.0, 160.0, 48.0),
			2,
			300.0,
			36.0,
			(1200.0, 800.0),
			2.0,
		);
		assert_eq!(
			(two.fill.x, two.fill.y),
			(one.fill.x * 2.0, one.fill.y * 2.0)
		);
		assert_eq!(
			(two.fill.w, two.fill.h),
			(one.fill.w * 2.0, one.fill.h * 2.0)
		);
		assert_eq!(
			(two.border.w, two.border.h),
			(one.border.w * 2.0, one.border.h * 2.0)
		);
		assert_eq!(
			two.fill.x - two.border.x,
			(one.fill.x - one.border.x) * 2.0,
			"the rule stayed 1 pixel"
		);
		assert_eq!(
			two.text_x - two.fill.x,
			(one.text_x - one.fill.x) * 2.0,
			"the padding stayed its 1x size"
		);
		assert_eq!(two.text_y - two.fill.y, (one.text_y - one.fill.y) * 2.0);
		assert_eq!(wrap_budget(1200.0, 2.0), wrap_budget(600.0, 1.0) * 2.0);
	}

	// A menu tip stands clear of the menu, and swaps to the other side rather
	// than covering the rows it is describing.
	// Test ID: EoThLQh
	#[test]
	fn a_menu_tip_never_lies_over_its_own_menu() {
		let row = rect(20.0, 60.0, 180.0, 24.0);
		let win = (400.0, 300.0);
		let (x, y) = beside(row, (150.0, 40.0), win, 6.0, 4.0);
		assert_eq!((x, y), (206.0, 60.0));
		// the same row on a menu that has been opened against the right edge
		let far = rect(230.0, 60.0, 160.0, 24.0);
		let (left, _) = beside(far, (150.0, 40.0), win, 6.0, 4.0);
		assert_eq!(left, 74.0);
	}

	// A tip left up too long goes away, and moving about on the same target
	// does not bring it back - only leaving and coming back does.
	// Test ID: EqdzLhx
	#[test]
	fn a_tip_goes_down_after_its_limit_until_the_pointer_comes_back() {
		let limit = Duration::from_secs(30);
		let mut dwell: Dwell<usize> = Dwell::default();
		dwell.point_at(Some(1));
		let (_, since) = dwell.over.unwrap();
		let at = |secs: u64| since + DELAY + Duration::from_secs(secs);
		assert_eq!(dwell.ripe_at(since, limit), None, "not up before the delay");
		assert_eq!(dwell.ripe_at(at(1), limit), Some(1));
		assert_eq!(dwell.ripe_at(at(29), limit), Some(1));
		assert_eq!(dwell.ripe_at(at(31), limit), None);
		assert_eq!(dwell.ripe_at(at(600), limit), None, "stays down");
		assert_eq!(
			dwell.ripe_at(at(600), Duration::ZERO),
			Some(1),
			"zero never hides"
		);
		dwell.point_at(Some(1));
		assert_eq!(dwell.over.unwrap().1, since, "same target keeps its clock");
		dwell.point_at(None);
		dwell.point_at(Some(1));
		let (_, back) = dwell.over.unwrap();
		assert!(back >= since);
		assert_eq!(
			dwell.ripe_at(back + DELAY + Duration::from_secs(1), limit),
			Some(1)
		);
	}

	// The clock runs on while the pointer stays put, and restarts when it moves
	// to something else - otherwise dragging across a strip would flash a tip
	// over every tab on the way.
	// Test ID: EoThLQi
	#[test]
	fn moving_to_something_else_restarts_the_clock() {
		let mut dwell: Dwell<usize> = Dwell::default();
		assert!(!dwell.point_at(Some(1)));
		let first = dwell.wake().expect("a wake-up while nothing is up yet");
		assert!(!dwell.point_at(Some(1)));
		assert_eq!(dwell.wake(), Some(first), "same target: the clock runs on");
		assert!(!dwell.point_at(Some(2)));
		assert!(dwell.wake().expect("a fresh wake-up") > first);
		assert_eq!(dwell.ripe(), None);
		// pointing at nothing is how a tip is put away
		assert!(!dwell.point_at(None));
		assert_eq!(dwell.wake(), None);
	}
}
