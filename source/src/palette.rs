// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

use crate::config::Settings;

// The 16 ANSI colors come from the active theme (config::settings().ansi),
// resolved in config from the theme name + mode. See theme.rs. Callers pass
// their per-frame Settings snapshot: this runs ~2x per cell per rebuilt frame,
// and settings() is an RwLock read + Arc clone - too hot to take per color.

pub fn resolve(c: Color, colors: &Colors, s: &Settings) -> [u8; 3] {
	match c {
		Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
		Color::Indexed(i) => indexed(i, colors, s),
		Color::Named(n) => named(n, colors, s),
	}
}

fn indexed(i: u8, colors: &Colors, s: &Settings) -> [u8; 3] {
	if let Some(rgb) = colors[i as usize] {
		return [rgb.r, rgb.g, rgb.b];
	}
	default_indexed(i, s)
}

pub fn default_indexed(i: u8, s: &Settings) -> [u8; 3] {
	match i {
		0..=15 => s.ansi[i as usize],
		16..=231 => {
			// 6x6x6 cube
			let cube = i - 16;
			let r = cube / 36;
			let g = (cube % 36) / 6;
			let b = cube % 6;
			let step = |n: u8| if n == 0 { 0u8 } else { 55 + n * 40 };
			[step(r), step(g), step(b)]
		}
		_ => {
			// grayscale ramp 232..=255
			let gray = 8 + (i - 232) * 10;
			[gray, gray, gray]
		}
	}
}

fn named(n: NamedColor, colors: &Colors, s: &Settings) -> [u8; 3] {
	use NamedColor::*;
	if let Some(rgb) = colors[n] {
		return [rgb.r, rgb.g, rgb.b];
	}
	match n {
		Foreground | DimForeground | BrightForeground => s.fg,
		Background => s.bg,
		Cursor => s.cursor,
		Black | DimBlack => s.ansi[0],
		Red | DimRed => s.ansi[1],
		Green | DimGreen => s.ansi[2],
		Yellow | DimYellow => s.ansi[3],
		Blue | DimBlue => s.ansi[4],
		Magenta | DimMagenta => s.ansi[5],
		Cyan | DimCyan => s.ansi[6],
		White | DimWhite => s.ansi[7],
		BrightBlack => s.ansi[8],
		BrightRed => s.ansi[9],
		BrightGreen => s.ansi[10],
		BrightYellow => s.ansi[11],
		BrightBlue => s.ansi[12],
		BrightMagenta => s.ansi[13],
		BrightCyan => s.ansi[14],
		BrightWhite => s.ansi[15],
	}
}

// Minimum-contrast lift. A program that writes near-black text (ANSI black, a
// dark 256-cube entry) on a dark background, or a pale color on a light one, is
// unreadable through no fault of the theme. `readable` measures the gap in Oklab
// lightness and, when it is too small, moves the text away from its background -
// L only, so the hue and the saturation survive and colors stay told apart.
//
// Oklab rather than the WCAG ratio: that ratio's +0.05 term swamps the dark end,
// so two near-blacks score well while being invisible, which is exactly the case
// this is for.

pub fn readable(fg: [u8; 3], bg: [u8; 3], min_gap: f32) -> [u8; 3] {
	// Equal colors are deliberate concealment (the HIDDEN attribute, or a program
	// hiding text on purpose). Never second-guess that.
	if min_gap <= 0.0 || fg == bg {
		return fg;
	}
	let (l, a, b) = to_oklab(fg);
	let bg_l = to_oklab(bg).0;
	if (l - bg_l).abs() >= min_gap {
		return fg;
	}
	// Away from the background, on the side the text is already on - but a pale
	// color on a merely light background has no room going lighter, so take the
	// other side when the near one runs off the end. A tie goes by which end the
	// background sits at.
	let near = if l == bg_l { bg_l < 0.5 } else { l > bg_l };
	let (up, down) = (bg_l + min_gap, bg_l - min_gap);
	let target = if near {
		if up <= 1.0 || down < 0.0 { up } else { down }
	} else if down >= 0.0 || up > 1.0 {
		down
	} else {
		up
	};
	from_oklab(target.clamp(0.0, 1.0), a, b)
}

pub(crate) fn to_oklab(c: [u8; 3]) -> (f32, f32, f32) {
	let (r, g, b) = (
		crate::config::to_linear(c[0]),
		crate::config::to_linear(c[1]),
		crate::config::to_linear(c[2]),
	);
	let l = (0.412_221_5 * r + 0.536_332_54 * g + 0.051_445_995 * b).cbrt();
	let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
	let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
	(
		0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
		1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
		0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
	)
}

pub(crate) fn from_oklab(lightness: f32, a: f32, b: f32) -> [u8; 3] {
	let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
	let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
	let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
	// Out-of-gamut ends here whenever a saturated color is pushed toward an end;
	// from_linear_u8 clamps, which costs a little saturation and no hue.
	[
		crate::config::from_linear_u8(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
		crate::config::from_linear_u8(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
		crate::config::from_linear_u8(-0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s),
	]
}

// The distinct (fg, bg) pairs on one screen are few - a handful even in a busy
// TUI - and `readable` is six cube roots. Memo it for the length of a build; a
// screen with more pairs than the cap just recomputes past that point.
#[derive(Debug, Default)]
pub struct Readable {
	seen: Vec<([u8; 3], [u8; 3], [u8; 3])>,
	gap: f32, // what `seen` was computed against; a change to it invalidates them
}

const READABLE_MEMO_CAP: usize = 64;

impl Readable {
	pub fn get(&mut self, fg: [u8; 3], bg: [u8; 3], min_gap: f32) -> [u8; 3] {
		if min_gap <= 0.0 {
			return fg;
		}
		if self.gap != min_gap {
			self.seen.clear();
			self.gap = min_gap;
		}
		if let Some((.., out)) = self.seen.iter().find(|(f, b, _)| *f == fg && *b == bg) {
			return *out;
		}
		let out = readable(fg, bg, min_gap);
		if self.seen.len() < READABLE_MEMO_CAP {
			self.seen.push((fg, bg, out));
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn gap(a: [u8; 3], b: [u8; 3]) -> f32 {
		(to_oklab(a).0 - to_oklab(b).0).abs()
	}

	// Test ID: EoTQguW
	#[test]
	fn oklab_round_trips() {
		for c in [[0, 0, 0], [255, 255, 255], [124, 168, 229], [200, 30, 90]] {
			let (l, a, b) = to_oklab(c);
			let back = from_oklab(l, a, b);
			for (x, y) in back.iter().zip(c.iter()) {
				assert!((*x as i16 - *y as i16).abs() <= 1, "{c:?} -> {back:?}");
			}
		}
	}

	// Test ID: EoTQguX
	#[test]
	fn dark_text_on_a_dark_background_is_lifted() {
		let bg = [0, 0, 0];
		for fg in [[0, 0, 0x80], [8, 8, 8], [0x1d, 0x1b, 0x18]] {
			let out = readable(fg, bg, 0.30);
			assert!(
				gap(out, bg) >= 0.29,
				"{fg:?} -> {out:?}, gap {}",
				gap(out, bg)
			);
			assert!(out != fg);
		}
	}

	// Test ID: EoTQguY
	#[test]
	fn pale_text_on_a_light_background_is_darkened() {
		let bg = [0xf6, 0xf5, 0xf0];
		let fg = [0xe5, 0xd6, 0x97];
		let out = readable(fg, bg, 0.30);
		assert!(to_oklab(out).0 < to_oklab(fg).0);
		assert!(gap(out, bg) >= 0.29);
	}

	// Test ID: EoTQguZ
	#[test]
	fn a_readable_pair_is_left_exactly_alone() {
		let bg = [0, 0, 0];
		for fg in [[0x88, 0xee, 0xcc], [0x7c, 0xa8, 0xe5], [0x67, 0x61, 0x58]] {
			assert_eq!(readable(fg, bg, 0.30), fg);
		}
	}

	// The HIDDEN attribute sets fg to bg, and a program can do the same by hand.
	// Either way it is on purpose.
	// Test ID: EoTQgua
	#[test]
	fn text_hidden_in_the_background_color_stays_hidden() {
		assert_eq!(
			readable([0x20, 0x20, 0x20], [0x20, 0x20, 0x20], 0.5),
			[0x20, 0x20, 0x20]
		);
	}

	// Test ID: EoTQgub
	#[test]
	fn zero_is_off() {
		let fg = [0, 0, 0x80];
		assert_eq!(readable(fg, [0, 0, 0], 0.0), fg);
	}

	// Same lightness, different hue: nothing to compare, so it still has to move.
	// Test ID: EoTQguc
	#[test]
	fn equal_lightness_moves_away_from_the_background() {
		let bg = [0x30, 0x30, 0x30];
		let (l, ..) = to_oklab(bg);
		let fg = from_oklab(l, 0.08, -0.05);
		let out = readable(fg, bg, 0.30);
		assert!(gap(out, bg) >= 0.29);
		assert!(to_oklab(out).0 > l, "a dark background lifts");
	}

	// Hue survives the move - a lifted red must not come back some other color.
	// Test ID: EoTQgud
	#[test]
	fn the_hue_survives_a_lift() {
		let fg = [0x40, 0x00, 0x00];
		let out = readable(fg, [0, 0, 0], 0.40);
		assert!(out[0] > out[1] && out[0] > out[2], "{out:?}");
	}

	// A pale color on a light-but-not-white background: lighter runs out of room
	// well before the gap is met, so it has to go the other way instead.
	// Test ID: EoTQgue
	#[test]
	fn a_direction_with_no_room_flips_to_the_other_one() {
		let bg = [0xb7, 0xb1, 0xa5];
		let fg = [0xf0, 0xf0, 0xc8];
		assert!(
			to_oklab(fg).0 > to_oklab(bg).0,
			"fg starts on the light side"
		);
		let out = readable(fg, bg, 0.45);
		assert!(to_oklab(out).0 < to_oklab(bg).0, "{out:?} should be darker");
		assert!(gap(out, bg) >= 0.44);
	}

	// Test ID: EoTQguf
	#[test]
	fn the_memo_answers_the_same_as_the_plain_call() {
		let mut memo = Readable::default();
		let pairs = [
			([0, 0, 0x80], [0, 0, 0]),
			([0x88, 0xee, 0xcc], [0, 0, 0]),
			([0xe5, 0xd6, 0x97], [0xf6, 0xf5, 0xf0]),
		];
		for (fg, bg) in pairs {
			assert_eq!(memo.get(fg, bg, 0.30), readable(fg, bg, 0.30));
			assert_eq!(memo.get(fg, bg, 0.30), readable(fg, bg, 0.30)); // now cached
		}
		// A changed threshold must not be answered from the old entries.
		for (fg, bg) in pairs {
			assert_eq!(memo.get(fg, bg, 0.50), readable(fg, bg, 0.50));
		}
	}
}
