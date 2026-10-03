// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// Caret arithmetic for the program's one-line text boxes: the Settings fields
// and a tab being renamed. Offsets are bytes, always on a character boundary.

pub fn prev_boundary(s: &str, i: usize) -> usize {
	let mut j = i.min(s.len());
	while j > 0 {
		j -= 1;
		if s.is_char_boundary(j) {
			return j;
		}
	}
	0
}
pub fn next_boundary(s: &str, i: usize) -> usize {
	let mut j = i;
	while j < s.len() {
		j += 1;
		if s.is_char_boundary(j) {
			return j;
		}
	}
	s.len()
}
// Word motion (Ctrl+Left/Right, Ctrl+Backspace/Delete, double-click): a word is
// a run of alphanumerics/underscore; everything else is a separator.
fn is_word_char(c: char) -> bool {
	c.is_alphanumeric() || c == '_'
}
pub fn word_left(s: &str, i: usize) -> usize {
	let mut j = i.min(s.len());
	// skip separators, then the word itself
	while j > 0 {
		let p = prev_boundary(s, j);
		if s[p..].chars().next().is_some_and(is_word_char) {
			break;
		}
		j = p;
	}
	while j > 0 {
		let p = prev_boundary(s, j);
		if !s[p..].chars().next().is_some_and(is_word_char) {
			break;
		}
		j = p;
	}
	j
}
pub fn word_right(s: &str, i: usize) -> usize {
	let mut j = i.min(s.len());
	while j < s.len() && !s[j..].chars().next().is_some_and(is_word_char) {
		j = next_boundary(s, j);
	}
	while j < s.len() && s[j..].chars().next().is_some_and(is_word_char) {
		j = next_boundary(s, j);
	}
	j
}
// Byte range of the word (or separator run) under byte index `i` (double-click).
pub fn word_at(s: &str, i: usize) -> (usize, usize) {
	if s.is_empty() {
		return (0, 0);
	}
	let i = if i >= s.len() {
		prev_boundary(s, s.len())
	} else {
		i
	};
	let wordy = s[i..].chars().next().is_some_and(is_word_char);
	let mut a = i;
	while a > 0 {
		let p = prev_boundary(s, a);
		if s[p..].chars().next().is_some_and(is_word_char) != wordy {
			break;
		}
		a = p;
	}
	let mut b = next_boundary(s, i);
	while b < s.len() && s[b..].chars().next().is_some_and(is_word_char) == wordy {
		b = next_boundary(s, b);
	}
	(a, b)
}
// Byte index of the caret nearest a click at `rel_x` px into the text (0 = the
// field's left text edge). Walks char boundaries, picking the one whose measured
// prefix width is closest to the click.
pub fn caret_from_click(text: &str, rel_x: f32, measure: &mut impl FnMut(&str) -> f32) -> usize {
	if rel_x <= 0.0 {
		return 0;
	}
	let (mut best_caret, mut best_dist) = (0usize, f32::MAX);
	let mut i = 0;
	loop {
		let dist = (measure(&text[..i]) - rel_x).abs();
		if dist < best_dist {
			best_dist = dist;
			best_caret = i;
		}
		if i >= text.len() {
			return best_caret;
		}
		i = next_boundary(text, i);
	}
}

/// How far an arrow or an erase key reaches from the caret: one character,
/// one word, or all the way to that end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
	Char,
	Word,
	End,
}

impl Reach {
	pub fn new(word: bool, line: bool) -> Reach {
		if line {
			Reach::End
		} else if word {
			Reach::Word
		} else {
			Reach::Char
		}
	}
}

pub fn reach_left(s: &str, i: usize, reach: Reach) -> usize {
	match reach {
		Reach::Char => prev_boundary(s, i),
		Reach::Word => word_left(s, i),
		Reach::End => 0,
	}
}

pub fn reach_right(s: &str, i: usize, reach: Reach) -> usize {
	match reach {
		Reach::Char => next_boundary(s, i),
		Reach::Word => word_right(s, i),
		Reach::End => s.len(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: EitjFLH
	#[test]
	fn caret_from_click_picks_nearest() {
		let mut m = |s: &str| s.chars().count() as f32; // 1 unit per ascii char
		assert_eq!(caret_from_click("hello", -5.0, &mut m), 0);
		assert_eq!(caret_from_click("hello", 0.0, &mut m), 0);
		assert_eq!(caret_from_click("hello", 2.4, &mut m), 2);
		assert_eq!(caret_from_click("hello", 100.0, &mut m), 5);
	}

	// Test ID: EkI1Txg
	#[test]
	fn word_motion_and_word_at() {
		let s = "foo bar_baz/qux.png";
		assert_eq!(word_left(s, 7), 4); // inside bar_baz -> its start
		assert_eq!(word_left(s, 4), 0); // at bar_baz -> foo start
		assert_eq!(word_right(s, 0), 3); // foo end
		assert_eq!(word_right(s, 3), 11); // past the space, bar_baz end
		assert_eq!(word_at(s, 5), (4, 11)); // bar_baz
		assert_eq!(word_at(s, 3), (3, 4)); // the separator run
		assert_eq!(word_at("", 0), (0, 0));
		assert_eq!(word_at(s, s.len()), (16, 19)); // clamps to last word (png)
	}

	// Test ID: ErbKd1H
	#[test]
	fn each_reach_stops_where_its_keys_say() {
		let s = "naïve foo";
		assert_eq!(reach_right(s, 2, Reach::Char), 4, "the two-byte i");
		assert_eq!(reach_left(s, 4, Reach::Char), 2);
		assert_eq!(reach_right(s, 0, Reach::Word), 6);
		assert_eq!(reach_left(s, s.len(), Reach::Word), 7);
		assert_eq!(reach_left(s, 5, Reach::End), 0);
		assert_eq!(reach_right(s, 0, Reach::End), s.len());
		assert_eq!(Reach::new(true, true), Reach::End);
		assert_eq!(Reach::new(true, false), Reach::Word);
		assert_eq!(Reach::new(false, false), Reach::Char);
	}
}
