// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Text and cursor colors taken from the wallpaper (`colors.from_wallpaper`).
//!
//! Two halves, decided separately, because harmony and legibility are unrelated
//! problems and solving them with one number solves neither:
//!
//! - **Lightness** comes from how bright the background actually gets. A photo's
//!   brightness varies per cell, so the average says nothing useful: white text
//!   readable over a dark sky vanishes into a cloud. The text is placed the
//!   contrast floor away from the field's BRIGHT END.
//! - **Hue** comes from the image's dominant hue, rotated to its complement, at
//!   low chroma. The cursor takes a further third of the circle, the way every
//!   built-in theme's cursor already sits against its foreground.
//!
//! The work splits over two threads. `summarize` runs on the wallpaper worker,
//! where the prepared pixels are, and reduces an image to six numbers. `derive`
//! is pure and runs wherever the live settings are, so a theme change re-colors
//! the text with no second decode.
//!
//! What this cannot do: guarantee the floor. Measured over the shipped pack of
//! 104 images, a single foreground clears a 0.45 gap on all of them at the
//! shipped 10% visibility, on about two thirds at 35%, and on a fifth at 100%.
//! Past that no color exists - the field's own bright end is inside the floor of
//! white. The derived color takes the best position available and the text scrim
//! covers the rest, which is the job it already had.

use crate::config::{self, Settings};
use crate::palette::{from_oklab, to_oklab};

// Rec.709 luma, matching contrast.rs. Luma rather than Oklab L because it is
// affine under the alpha composite, which is what lets a percentile taken on
// the image alone survive being composited over a background color later.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

// The summary grid. One sample per terminal cell is the right footprint, since
// a glyph covers a cell and that average is what its legibility is decided by.
// Finer sampling chases single bright specks: measured over the shipped pack,
// 300x80 moves the bright-end percentile by at most 0.015 Oklab L against this,
// and a quarter of that grid moves it by 0.13.
const GRID_W: usize = 150;
const GRID_H: usize = 40;

// Where the bright and dark ends are read. Not the extremes: one specular
// highlight would drag the text to white on any image with a sun in it, and the
// scrim is what covers the last few cells.
const HI_PCT: f32 = 0.95;
const LO_PCT: f32 = 0.05;

// A tint, not a color. Body text over a photo wants a cast; a saturated
// foreground is tiring to read and is what makes a complement "vibrate" against
// its ground. Holds every theme to the same gentle tint, so a monochrome theme's
// vivid foreground does not come back as vivid yellow.
const MAX_CHROMA: f32 = 0.06;

// Below this mean chroma an image has no hue worth complementing - six of the
// shipped pack are here - and the theme's own hue is kept instead.
const MIN_CHROMA: f32 = 0.02;

// How far round the cursor sits from the text. A third of the circle, which is
// what SilkTerm's and Pastel's cursors already are.
const CURSOR_ROTATE: f32 = 120.0;

/// How many evenly spaced quantiles of each channel a summary keeps.
pub const SPREAD: usize = 16;

/// What one image is worth to the derivation. A handful of numbers, so the live
/// settings can hold it and re-derive on a theme change without decoding anything.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Summary {
	/// Per-cell linear luma at the bright and dark ends, alpha premultiplied.
	pub luma_hi: f32,
	pub luma_lo: f32,
	/// And the mean of the same grid. Only the visibility ramp reads it, which
	/// wants how bright the picture is overall rather than where its ends are.
	pub luma_mean: f32,
	/// Each linear channel of the same grid at SPREAD evenly spaced quantiles,
	/// darkest first, taken one channel at a time. The halo match averages over
	/// these, since an average over the picture is not the picture's average once
	/// the sRGB curve is applied, and both renderers work a channel at a time
	/// (`visibility::halo_match`).
	pub spread: [[f32; 3]; SPREAD],
	/// Mean alpha, so the share of a cell the image does not cover can be given
	/// back to the background color. 1.0 for every ordinary photo.
	pub alpha: f32,
	/// Chroma-weighted dominant hue, degrees in Oklab's a/b plane, and the mean
	/// per-cell chroma behind it.
	pub hue: f32,
	pub chroma: f32,
	/// The visibility this image is drawn at, its own tag folded in already. Baked
	/// in rather than read live because changing the slider reloads the wallpaper
	/// anyway, where changing the theme does not.
	pub opacity: f32,
}

/// Reduce a prepared wallpaper to a `Summary`. `opacity` is what the image will
/// actually be drawn at.
pub fn summarize(img: &image::RgbaImage, opacity: f32) -> Summary {
	let (w, h) = (img.width() as usize, img.height() as usize);
	let mut sum = vec![[0.0f32; 4]; GRID_W * GRID_H];
	let mut count = vec![0u32; GRID_W * GRID_H];
	if w == 0 || h == 0 {
		return Summary {
			luma_hi: 0.0,
			luma_lo: 0.0,
			luma_mean: 0.0,
			spread: [[0.0; 3]; SPREAD],
			alpha: 1.0,
			hue: 0.0,
			chroma: 0.0,
			opacity,
		};
	}
	// One pass, box-averaging into the grid in linear light. An sRGB-space
	// average darkens, the same reason the blur works in linear.
	for (x, y, px) in img.enumerate_pixels() {
		let gx = (x as usize * GRID_W / w).min(GRID_W - 1);
		let gy = (y as usize * GRID_H / h).min(GRID_H - 1);
		let cell = gy * GRID_W + gx;
		let a = f32::from(px[3]) / 255.0;
		let bin = &mut sum[cell];
		for c in 0..3 {
			bin[c] += config::to_linear(px[c]) * a;
		}
		bin[3] += a;
		count[cell] += 1;
	}

	let mut lumas: Vec<f32> = Vec::with_capacity(GRID_W * GRID_H);
	let mut channels: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::with_capacity(GRID_W * GRID_H));
	let (mut a_sum, mut a_n) = (0.0f64, 0u32);
	// Chroma-weighted hue histogram, one bin a degree. A mean color cannot be
	// used here: opposite hues cancel, and 17 of the shipped pack average to a
	// near-gray whose hue is noise - one of them reads 174 degrees away from the
	// hue that is actually all over it.
	let mut hist = [0.0f32; 360];
	let mut chroma_sum = 0.0f64;
	for (bin, &n) in sum.iter().zip(count.iter()) {
		if n == 0 {
			continue;
		}
		let n = n as f32;
		let rgb = [bin[0] / n, bin[1] / n, bin[2] / n];
		lumas.push(rgb[0] * LUMA[0] + rgb[1] * LUMA[1] + rgb[2] * LUMA[2]);
		for (channel, value) in channels.iter_mut().zip(rgb) {
			channel.push(value);
		}
		a_sum += f64::from(bin[3] / n);
		a_n += 1;
		let (_, a, b) = to_oklab_linear(rgb);
		let chroma = a.hypot(b);
		chroma_sum += f64::from(chroma);
		let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
		hist[hue as usize % 360] += chroma;
	}
	if lumas.is_empty() {
		return Summary {
			luma_hi: 0.0,
			luma_lo: 0.0,
			luma_mean: 0.0,
			spread: [[0.0; 3]; SPREAD],
			alpha: 1.0,
			hue: 0.0,
			chroma: 0.0,
			opacity,
		};
	}
	lumas.sort_by(f32::total_cmp);
	for channel in &mut channels {
		channel.sort_by(f32::total_cmp);
	}
	let cells = a_n.max(1) as f64;
	let mean = if lumas.is_empty() {
		0.0
	} else {
		(lumas.iter().map(|&x| f64::from(x)).sum::<f64>() / lumas.len() as f64) as f32
	};
	Summary {
		luma_hi: pct(&lumas, HI_PCT),
		luma_lo: pct(&lumas, LO_PCT),
		luma_mean: mean,
		spread: std::array::from_fn(|i| {
			let at = (i as f32 + 0.5) / SPREAD as f32;
			[
				pct(&channels[0], at),
				pct(&channels[1], at),
				pct(&channels[2], at),
			]
		}),
		alpha: (a_sum / cells) as f32,
		hue: dominant_hue(&hist),
		chroma: (chroma_sum / cells) as f32,
		opacity: opacity.clamp(0.0, 1.0),
	}
}

fn pct(sorted: &[f32], p: f32) -> f32 {
	let i = ((sorted.len() - 1) as f32 * p).round() as usize;
	sorted[i.min(sorted.len() - 1)]
}

// The hue whose 30-degree neighborhood carries the most chroma. The window is
// what stops a gradient from splitting its own weight across a dozen bins.
fn dominant_hue(hist: &[f32; 360]) -> f32 {
	let mut best = (0.0f32, 0usize);
	for centre in 0..360 {
		let mut weight = 0.0;
		for off in -14i32..=15 {
			weight += hist[(centre as i32 + off).rem_euclid(360) as usize];
		}
		if weight > best.0 {
			best = (weight, centre);
		}
	}
	best.1 as f32
}

// Oklab straight from linear RGB. `palette::to_oklab` takes sRGB bytes, and the
// grid is already linear.
fn to_oklab_linear(rgb: [f32; 3]) -> (f32, f32, f32) {
	let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
	let l = (0.412_221_5 * r + 0.536_332_54 * g + 0.051_445_995 * b).cbrt();
	let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
	let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
	(
		0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
		1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
		0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
	)
}

// A gray's Oklab lightness from its linear luma. The three L coefficients sum to
// 1, so for a neutral the whole transform is one cube root.
fn gray_lightness(luma: f32) -> f32 {
	luma.max(0.0).cbrt()
}

// Oklab a/b from a hue in degrees and a chroma.
fn ab(hue: f32, chroma: f32) -> (f32, f32) {
	let rad = hue.to_radians();
	(chroma * rad.cos(), chroma * rad.sin())
}

// A color at this lightness and hue, with the chroma pulled in until it fits.
//
// Oklab will happily name a lightness that sRGB cannot reach at a given chroma,
// and `from_oklab` clamps the channels - which costs the lightness that was
// asked for, silently. That is fine for `palette::readable`, which is nudging
// text that already exists, and not fine here: the lightness IS the legibility,
// and a text color 0.02 short of its target takes the cursor plate with it.
// Reducing chroma at a constant lightness and hue is how CSS Color 4 maps into
// gamut, and it is the right way round for a tint - the cast is decoration.
// A gray is always reachable, so the search always has an answer.
fn at_lightness(lightness: f32, hue: f32, chroma: f32) -> [u8; 3] {
	// A byte's worth of lightness at the pale end is about this, so asking for
	// more precision than this only chases the quantization.
	const FITS: f32 = 0.006;
	let built = |c: f32| {
		let (a, b) = ab(hue, c);
		let out = from_oklab(lightness, a, b);
		(((to_oklab(out).0 - lightness).abs() <= FITS), out)
	};
	if let (true, out) = built(chroma) {
		return out;
	}
	let (mut lo, mut hi) = (0.0f32, chroma);
	for _ in 0..12 {
		let mid = 0.5 * (lo + hi);
		if built(mid).0 {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	built(lo).1
}

fn hue_chroma(c: [u8; 3]) -> (f32, f32) {
	let (_, a, b) = to_oklab(c);
	(b.atan2(a).to_degrees().rem_euclid(360.0), a.hypot(b))
}

// What the background behind a glyph really is, as a linear luma - the wallpaper
// pass's own blend, with the image's luma standing in for its color. Light mode
// mixes differently from dark, so `mix` carries which one (visibility.rs). Where
// the image does not cover its pixel the background shows through first, so the
// picture is filled with it before the two are mixed.
fn field_luma(sum: &Summary, image_luma: f32, bg: [u8; 3], mix: crate::visibility::Mix) -> f32 {
	let bg = config::luma(bg);
	let filled = image_luma + bg * (1.0 - sum.alpha.clamp(0.0, 1.0));
	mix.field(filled, bg)
}

// The cursor whose plate lands on `plate_target` over a field of `behind_luma`.
//
// It has to be searched rather than solved. The plate is blended in linear light,
// the floor is measured in Oklab lightness, and a tinted color's luma is not its
// lightness - which is the step that quietly cost the cursor a twentieth of the
// floor before. Plate lightness rises with the cursor's, so sixteen rounds of
// bisection place it inside a byte, and this runs once a picture rather than per
// frame. Where the field is already brighter than the target the search bottoms
// out at black, which is the best answer available. `alpha` is the plate's, which
// is stronger over a light theme (`pane::cursor_alpha`).
//
// The field is taken as a neutral at its summarized luma. The summary is a luma
// statistic by design, and `the_neutral_field_model_stays_inside_a_tolerance`
// bounds what the simplification costs on a strongly tinted picture.
fn cursor_for(plate_target: f32, behind_luma: f32, alpha: f32, hue: f32, chroma: f32) -> [u8; 3] {
	let plate_of = |cursor_l: f32| {
		let c = at_lightness(cursor_l, hue, chroma);
		let mix = |k: usize| config::to_linear(c[k]) * alpha + behind_luma * (1.0 - alpha);
		to_oklab_linear([mix(0), mix(1), mix(2)]).0
	};
	let (mut lo, mut hi) = (0.0f32, 1.0f32);
	for _ in 0..16 {
		let mid = 0.5 * (lo + hi);
		if plate_of(mid) < plate_target {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	at_lightness(0.5 * (lo + hi), hue, chroma)
}

impl Summary {
	/// How bright the picture is, for the visibility ramp: its overall level and
	/// its bright end, which is where glare comes from.
	pub fn picture(&self) -> (f32, f32) {
		(self.luma_mean, self.luma_hi)
	}
}

/// The derived pair. `None` for either means the theme's own color stands.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Derived {
	pub fg: [u8; 3],
	pub cursor: [u8; 3],
}

/// Text and cursor for this image under these settings. Pure, so the same
/// wallpaper gives the same answer on any box and a test needs no pixels.
pub fn derive(sum: &Summary, settings: &Settings) -> Derived {
	let floor = settings.text_min_contrast.clamp(0.0, 1.0);
	let (bg, fg, cursor) = (settings.bg, settings.fg, settings.cursor);
	let mix = crate::visibility::wallpaper_mix(settings, sum.opacity, Some(sum.picture()));
	let hi = gray_lightness(field_luma(sum, sum.luma_hi, bg, mix));
	let lo = gray_lightness(field_luma(sum, sum.luma_lo, bg, mix));

	// Which side the text sits on is the theme's, never the image's. A light
	// theme that flipped to light text because a photo was dark would stop being
	// the theme that was chosen.
	let (fg_l, fg_c) = (to_oklab(fg).0, hue_chroma(fg).1);
	let light_text = fg_l >= to_oklab(bg).0;

	// Away from the field's near end, and never dimmer than the theme asked for:
	// the wallpaper may demand more separation, not less. With the floor off this
	// leaves the theme's own lightness alone, which is the right reading of a
	// floor set to zero.
	let target = if light_text {
		(hi + floor).max(fg_l)
	} else {
		(lo - floor).min(fg_l)
	}
	.clamp(0.0, 1.0);

	// Hue from the image's complement, chroma held to a tint. An image with no
	// hue in it keeps the theme's.
	let chroma = fg_c.min(MAX_CHROMA);
	let hue = if sum.chroma >= MIN_CHROMA {
		(sum.hue + 180.0).rem_euclid(360.0)
	} else {
		hue_chroma(fg).0
	};
	let out_fg = at_lightness(target, hue, chroma);

	// The cursor is a plate with the glyph's own color on top, so
	// it is a second background the text has to clear the same floor on. Put the
	// plate exactly the floor away from the text - the furthest it can sit from
	// the field while still carrying a glyph - and find the cursor that draws it.
	// Solved against the field's bright end, so a darker cell only widens the gap.
	let plate = if light_text {
		target - floor
	} else {
		target + floor
	}
	.clamp(0.0, 1.0);
	let out_cursor = cursor_for(
		plate,
		field_luma(sum, sum.luma_hi, bg, mix),
		if light_text {
			crate::pane::CURSOR_ALPHA
		} else {
			crate::pane::CURSOR_ALPHA_LIGHT
		},
		(hue + CURSOR_ROTATE).rem_euclid(360.0),
		hue_chroma(cursor).1.min(MAX_CHROMA),
	);

	Derived {
		fg: out_fg,
		cursor: out_cursor,
	}
}

/// The user's own text and cursor, held while the derived ones are live. Two
/// colors, built the same way as `profile::Shadow` and for the same reason: the
/// file and the Settings dialog must only ever see what the user chose.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shadow {
	fg: [u8; 3],
	cursor: [u8; 3],
}

/// Put the user's own colors back. Safe on settings carrying no derived pair.
pub fn unapply(settings: &mut Settings) {
	if let Some(shadow) = settings.wallpaper_colors.take() {
		settings.fg = shadow.fg;
		settings.cursor = shadow.cursor;
	}
}

/// Overwrite fg and cursor with the wallpaper's, keeping the user's in the
/// shadow. Idempotent, so a live copy that already carries a derived pair is
/// unwound first and the new one is derived from the user's values rather than
/// from the last answer.
pub fn apply(settings: &mut Settings) {
	unapply(settings);
	if !settings.colors_from_wallpaper || !settings.wallpaper_enabled {
		return;
	}
	let Some(sum) = settings.wallpaper_summary else {
		return; // no picture yet, or none at all
	};
	let out = derive(&sum, settings);
	settings.wallpaper_colors = Some(Shadow {
		fg: settings.fg,
		cursor: settings.cursor,
	});
	settings.fg = out.fg;
	settings.cursor = out.cursor;
}

#[cfg(test)]
mod tests {
	use super::*;

	fn plain(rgb: [u8; 3], w: u32, h: u32) -> image::RgbaImage {
		image::RgbaImage::from_pixel(w, h, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
	}

	fn settings(bg: [u8; 3], fg: [u8; 3], cursor: [u8; 3]) -> Settings {
		Settings {
			bg,
			fg,
			cursor,
			colors_from_wallpaper: true,
			wallpaper_enabled: true,
			// A light background means light mode, which draws the picture at a
			// higher alpha than the slider reads (visibility.rs). Without this the
			// light cases here are placed against a field nobody will ever see.
			theme_mode: if lightness(bg) > 0.5 { "light" } else { "dark" }.to_string(),
			..Settings::default()
		}
	}

	const SILK_BG: [u8; 3] = [0, 0, 0];
	const SILK_FG: [u8; 3] = [0x88, 0xee, 0xcc];
	const SILK_CURSOR: [u8; 3] = [0x8a, 0x3f, 0xa4];

	fn lightness(c: [u8; 3]) -> f32 {
		to_oklab(c).0
	}

	// Test ID: EqRxesS
	#[test]
	fn a_flat_gray_reports_its_own_luma_at_both_ends() {
		let sum = summarize(&plain([128, 128, 128], 64, 64), 1.0);
		let want = config::luma([128, 128, 128]);
		assert!((sum.luma_hi - want).abs() < 1e-4, "{sum:?}");
		assert!((sum.luma_lo - want).abs() < 1e-4, "{sum:?}");
		assert!(sum.chroma < 1e-3, "a gray has no chroma: {sum:?}");
		assert_eq!(sum.alpha, 1.0);
	}

	// The bright end is what the text is placed against, and it has to find a
	// bright strip that most of the picture disagrees with - a sky over a dark
	// landscape is the ordinary case. A middle reading would call this image dark
	// and put the text where the strip swallows it.
	// Test ID: EqRxesT
	#[test]
	fn the_bright_end_finds_a_strip_most_of_the_picture_disagrees_with() {
		// four fifths near-black, one fifth white
		let img = image::RgbaImage::from_fn(300, 80, |x, _| {
			let c: u8 = if x < 240 { 4 } else { 255 };
			image::Rgba([c, c, c, 255])
		});
		let sum = summarize(&img, 1.0);
		assert!(
			sum.luma_hi > 0.8,
			"the strip should set the bright end: {sum:?}"
		);
		assert!(sum.luma_lo < 0.01, "and the rest the dark end: {sum:?}");
	}

	// A mean color cannot answer this: red and cyan in equal measure average to
	// gray, and the mean hue is then whichever way the rounding fell.
	// Test ID: EqRxesU
	#[test]
	fn the_dominant_hue_survives_a_second_colour_that_would_cancel_a_mean() {
		let red: [u8; 3] = [0xd0, 0x30, 0x30];
		let img = image::RgbaImage::from_fn(90, 60, |x, _| {
			// two thirds red, one third its near-opposite
			let c = if x < 60 { red } else { [0x30, 0xc0, 0xd0] };
			image::Rgba([c[0], c[1], c[2], 255])
		});
		let sum = summarize(&img, 1.0);
		let red_hue = hue_chroma(red).0;
		let off = (sum.hue - red_hue)
			.abs()
			.min(360.0 - (sum.hue - red_hue).abs());
		assert!(off < 25.0, "dominant hue {off} off red {red_hue}: {sum:?}");
		assert!(sum.chroma > MIN_CHROMA, "{sum:?}");
	}

	// And the weight is chroma, not a count of cells. A wide near-gray field with
	// the faintest tint covers more of the picture than a vivid patch does, and
	// the vivid patch is what anybody looking at it would call its color.
	// Test ID: EqRxesV
	#[test]
	fn the_hue_weight_is_chroma_rather_than_how_many_cells_carry_it() {
		let faint: [u8; 3] = [0x76, 0x78, 0x84]; // barely blue
		let vivid: [u8; 3] = [0xe6, 0x78, 0x14]; // plainly orange
		assert!(
			hue_chroma(faint).1 * 2.0 < hue_chroma(vivid).1,
			"the fixture needs the two chromas far apart"
		);
		let img = image::RgbaImage::from_fn(300, 80, |x, _| {
			let c = if x < 210 { faint } else { vivid };
			image::Rgba([c[0], c[1], c[2], 255])
		});
		let sum = summarize(&img, 1.0);
		let near = |want: f32| (sum.hue - want).abs().min(360.0 - (sum.hue - want).abs());
		assert!(
			near(hue_chroma(vivid).0) < 25.0,
			"hue {} should be the vivid patch's, not the faint field's {}",
			sum.hue,
			hue_chroma(faint).0
		);
	}

	// Test ID: EqRxesW
	#[test]
	fn a_dark_theme_keeps_light_text_and_a_light_theme_keeps_dark_text() {
		let sum = summarize(&plain([90, 110, 160], 32, 32), 0.35);
		let dark = derive(&sum, &settings(SILK_BG, SILK_FG, SILK_CURSOR));
		assert!(lightness(dark.fg) > 0.5, "{:?}", dark.fg);
		let light = derive(
			&sum,
			&settings([0xf6, 0xf5, 0xf0], [0x30, 0x32, 0x38], [0x33, 0x55, 0x99]),
		);
		assert!(lightness(light.fg) < 0.5, "{:?}", light.fg);
	}

	// The whole point of the lightness half: a brighter picture pushes the text
	// further away, and it is measured against the bright end. The theme here has
	// a mid-gray foreground on purpose - a shipped one sits high enough that the
	// "never dimmer than the theme" floor would swallow the range being measured.
	// Test ID: EqRxesX
	#[test]
	fn a_brighter_image_pushes_the_text_further_away() {
		let s = settings(SILK_BG, [0x70, 0x70, 0x70], SILK_CURSOR);
		let mut last = 0.0;
		for rgb in [[20u8, 20, 25], [90, 90, 100], [170, 170, 180]] {
			let out = derive(&summarize(&plain(rgb, 32, 32), 0.35), &s);
			let got = lightness(out.fg);
			assert!(
				got > last + 0.1,
				"{rgb:?} -> {:?}, L {got} after {last}",
				out.fg
			);
			last = got;
		}
	}

	// A dark image must not make the text dimmer than the theme's own. Without
	// the floor at the theme's foreground, a near-black wallpaper would answer
	// mid-gray text and read as the wallpaper spoiling the theme.
	// Test ID: EqRxesY
	#[test]
	fn a_dark_image_never_dims_the_text_below_the_themes_own() {
		let s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		let out = derive(&summarize(&plain([2, 2, 2], 32, 32), 0.1), &s);
		assert!(
			lightness(out.fg) >= lightness(SILK_FG) - 0.01,
			"{:?} is dimmer than {SILK_FG:?}",
			out.fg
		);
	}

	// The hue half. A blue picture has to answer warm text, whatever the theme's
	// own foreground was.
	// Test ID: EqRxesZ
	#[test]
	fn a_blue_image_gives_warm_text_and_a_warm_image_gives_cool_text() {
		let s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		let blue = derive(&summarize(&plain([40, 70, 180], 32, 32), 0.5), &s).fg;
		let warm = derive(&summarize(&plain([180, 120, 40], 32, 32), 0.5), &s).fg;
		assert!(blue[0] > blue[2], "blue image -> warm text, got {blue:?}");
		assert!(warm[2] > warm[0], "warm image -> cool text, got {warm:?}");
	}

	// Six of the shipped pack have no hue worth complementing. Inventing one from
	// that little chroma would swing the text about between two gray images.
	// Test ID: EqRxesa
	#[test]
	fn a_grey_image_keeps_the_themes_own_hue() {
		let s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		let out = derive(&summarize(&plain([100, 100, 100], 32, 32), 0.3), &s);
		let (want, _) = hue_chroma(SILK_FG);
		let (got, _) = hue_chroma(out.fg);
		let off = (got - want).abs().min(360.0 - (got - want).abs());
		assert!(off < 10.0, "hue {got} should have stayed near {want}");
	}

	// The tint cap. Matrix's foreground is chroma 0.25, and carrying that to a
	// complementary hue is what made a green theme answer pure yellow.
	// Test ID: EqRxesb
	#[test]
	fn a_vivid_theme_foreground_comes_back_as_a_tint() {
		let s = settings([0x00, 0x08, 0x02], [0x33, 0xff, 0x66], [0x0a, 0x7a, 0x2a]);
		assert!(
			hue_chroma(s.fg).1 > 0.2,
			"Matrix's fg is vivid to start with"
		);
		let out = derive(&summarize(&plain([40, 70, 180], 32, 32), 0.35), &s);
		assert!(
			hue_chroma(out.fg).1 <= MAX_CHROMA + 0.01,
			"{:?} is chroma {}",
			out.fg,
			hue_chroma(out.fg).1
		);
	}

	// Light mode draws the picture at a higher alpha than the slider reads, so the
	// field the text is placed against is darker than the number alone suggests.
	// Same colours, same picture, same slider - only the mode moves. A dark
	// picture turned well up, because below that the field stays bright enough
	// that the theme's own foreground wins either way.
	// Test ID: EqT4HIe
	#[test]
	fn light_mode_places_the_text_against_the_alpha_it_will_really_be_drawn_at() {
		let light_bg = [0xf6u8, 0xf5, 0xf0];
		let dark_text = [0x30u8, 0x32, 0x38];
		let sum = summarize(&plain([10, 10, 12], 32, 32), 0.50);
		let mut s = settings(light_bg, dark_text, SILK_CURSOR);
		assert_eq!(s.theme_mode, "light");
		let lit = derive(&sum, &s);
		s.theme_mode = "dark".to_string();
		let unlit = derive(&sum, &s);
		assert!(
			lightness(lit.fg) < lightness(unlit.fg) - 0.01,
			"light {:?} ({}) should sit below dark {:?} ({})",
			lit.fg,
			lightness(lit.fg),
			unlit.fg,
			lightness(unlit.fg)
		);
	}

	// What `derive` will place its colors against, so the test models the field the
	// code really uses rather than one of its own.
	fn mix(settings: &Settings, sum: &Summary) -> crate::visibility::Mix {
		crate::visibility::wallpaper_mix(settings, sum.opacity, Some(sum.picture()))
	}

	// The plate the block cursor draws, over a field taken as a neutral at
	// `behind` - the same model `cursor_for` searches against.
	fn plate_over(cursor: [u8; 3], behind: f32, alpha: f32) -> f32 {
		let mix = |k: usize| config::to_linear(cursor[k]) * alpha + behind * (1.0 - alpha);
		to_oklab_linear([mix(0), mix(1), mix(2)]).0
	}

	// The cursor rule every built-in theme is already held to: the plate is a
	// second background, so the text on it must clear the floor too.
	// Test ID: EqRxesc
	#[test]
	fn text_on_the_derived_cursor_plate_clears_the_floor() {
		let floor = Settings::default().text_min_contrast;
		for bg in [[0u8, 0, 0], [0x1d, 0x20, 0x28], [0xf6, 0xf5, 0xf0]] {
			let fg = if bg[0] > 0x80 {
				[0x30, 0x32, 0x38]
			} else {
				SILK_FG
			};
			let s = settings(bg, fg, SILK_CURSOR);
			for rgb in [[10, 10, 12], [90, 110, 160], [230, 220, 180]] {
				for op in [0.1f32, 0.35, 1.0] {
					let sum = summarize(&plain(rgb, 32, 32), op);
					let out = derive(&sum, &s);
					let alpha = if bg[0] > 0x80 {
						crate::pane::CURSOR_ALPHA_LIGHT
					} else {
						crate::pane::CURSOR_ALPHA
					};
					let plate = plate_over(
						out.cursor,
						field_luma(&sum, sum.luma_hi, bg, mix(&s, &sum)),
						alpha,
					);
					let gap = (lightness(out.fg) - plate).abs();
					// Short only where the field is already past the target and the
					// search bottoms out, which is the case the scrim covers.
					let cornered = out.cursor == [0, 0, 0] || out.cursor == [255, 255, 255];
					assert!(
						gap >= floor - 0.01 || cornered,
						"bg {bg:?} image {rgb:?} op {op}: gap {gap} under {floor}"
					);
				}
			}
		}
	}

	// `cursor_for` takes the field behind the plate as a neutral, because the
	// summary is a luma statistic. A strongly tinted field of the same luma has to
	// give nearly the same plate, or the simplification is buying the wrong thing.
	// Test ID: EqRxesd
	#[test]
	fn the_neutral_field_model_stays_inside_a_tolerance() {
		let alpha = crate::pane::CURSOR_ALPHA;
		let mut worst = 0.0f32;
		for behind in [0.02f32, 0.1, 0.3, 0.6] {
			// a vivid field at that luma, one hue at a time
			for hue in [20.0f32, 140.0, 260.0] {
				let (a, b) = ab(hue, 0.12);
				let tinted = from_oklab(gray_lightness(behind), a, b);
				let scale = behind / config::luma(tinted).max(1e-6);
				let field: Vec<f32> = (0..3)
					.map(|k| config::to_linear(tinted[k]) * scale)
					.collect();
				let cursor = cursor_for(gray_lightness(behind) + 0.2, behind, alpha, 0.0, 0.04);
				let neutral = plate_over(cursor, behind, alpha);
				let mix =
					|k: usize| config::to_linear(cursor[k]) * alpha + field[k] * (1.0 - alpha);
				let real = to_oklab_linear([mix(0), mix(1), mix(2)]).0;
				worst = worst.max((neutral - real).abs());
			}
		}
		assert!(worst < 0.04, "the neutral field costs {worst} Oklab L");
	}

	// The shipped config turns this on, and the wallpaper is on too, so a fresh
	// install re-colors its text without anyone visiting the Themes tab.
	// Test ID: EqSm2Rk
	#[test]
	fn the_shipped_defaults_take_the_text_color_from_the_wallpaper() {
		let mut s = Settings::default();
		assert!(s.colors_from_wallpaper, "shipped off");
		assert!(s.wallpaper_enabled, "no wallpaper to read");
		let plain_fg = s.fg;
		s.wallpaper_summary = Some(summarize(&plain([90, 110, 160], 32, 32), 0.35));
		apply(&mut s);
		assert_ne!(s.fg, plain_fg);
		assert!(s.wallpaper_colors.is_some());
	}

	// Test ID: EqRxese
	#[test]
	fn apply_is_off_unless_the_switch_and_the_wallpaper_are_both_on() {
		let sum = summarize(&plain([90, 110, 160], 32, 32), 0.35);
		let mut s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		s.wallpaper_summary = Some(sum);

		let mut off = s.clone();
		off.colors_from_wallpaper = false;
		apply(&mut off);
		assert_eq!(off.fg, SILK_FG);
		assert!(off.wallpaper_colors.is_none());

		let mut no_paper = s.clone();
		no_paper.wallpaper_enabled = false;
		apply(&mut no_paper);
		assert_eq!(no_paper.fg, SILK_FG);

		let mut no_image = s.clone();
		no_image.wallpaper_summary = None;
		apply(&mut no_image);
		assert_eq!(no_image.fg, SILK_FG);

		apply(&mut s);
		assert_ne!(s.fg, SILK_FG, "on, with an image: the text should move");
	}

	// The shadow is what keeps the file and the dialog seeing the user's colors.
	// Test ID: EqRxesf
	#[test]
	fn apply_then_unapply_is_the_identity_however_many_times_it_runs() {
		let mut s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		s.wallpaper_summary = Some(summarize(&plain([90, 110, 160], 32, 32), 0.35));
		apply(&mut s);
		let once = (s.fg, s.cursor);
		// a second apply must derive from the user's values again, not from its
		// own last answer
		apply(&mut s);
		assert_eq!((s.fg, s.cursor), once, "apply stacked on itself");
		unapply(&mut s);
		assert_eq!((s.fg, s.cursor), (SILK_FG, SILK_CURSOR));
		unapply(&mut s);
		assert_eq!((s.fg, s.cursor), (SILK_FG, SILK_CURSOR), "unapply twice");
	}

	// A summary is six numbers so that a theme change can re-derive with no
	// decode. That only works if the background is read live.
	// Test ID: EqRxesg
	#[test]
	fn the_same_image_answers_differently_under_a_different_background() {
		let sum = summarize(&plain([120, 120, 130], 32, 32), 0.3);
		let on_black = derive(&sum, &settings(SILK_BG, SILK_FG, SILK_CURSOR));
		let on_gray = derive(&sum, &settings([0x50, 0x50, 0x58], SILK_FG, SILK_CURSOR));
		assert!(
			lightness(on_gray.fg) > lightness(on_black.fg),
			"a lighter background leaves the text less room: {:?} vs {:?}",
			on_gray.fg,
			on_black.fg
		);
	}

	// With the floor off there is no gap to work with, so the lightness half has
	// nothing to say and the theme's own brightness stands.
	// Test ID: EqRxesh
	#[test]
	fn a_floor_of_zero_leaves_the_lightness_alone() {
		let mut s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		s.text_min_contrast = 0.0;
		let out = derive(&summarize(&plain([200, 200, 210], 32, 32), 1.0), &s);
		assert!(
			(lightness(out.fg) - lightness(SILK_FG)).abs() < 0.01,
			"{:?} vs {SILK_FG:?}",
			out.fg
		);
	}

	// An image too bright for any foreground is the case the scrim covers. What
	// must not happen is a panic or a color outside the cube.
	// Test ID: EqRxesi
	#[test]
	fn an_image_brighter_than_the_floor_allows_goes_to_the_limit() {
		let s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		let out = derive(&summarize(&plain([255, 255, 255], 8, 8), 1.0), &s);
		assert!(lightness(out.fg) > 0.95, "{:?}", out.fg);
	}

	// Test ID: EqRxesj
	#[test]
	fn an_empty_image_is_answered_rather_than_panicking() {
		let sum = summarize(&image::RgbaImage::new(0, 0), 0.5);
		assert_eq!(sum.alpha, 1.0);
		let out = derive(&sum, &settings(SILK_BG, SILK_FG, SILK_CURSOR));
		assert!(lightness(out.fg) >= lightness(SILK_FG) - 0.01);
	}

	// A transparent image lets the background through, so the field is the
	// background and the text should sit where the theme put it.
	// Test ID: EqRxesk
	#[test]
	fn a_fully_transparent_image_leaves_the_field_as_the_background() {
		let clear = image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 255, 255, 0]));
		let sum = summarize(&clear, 1.0);
		assert_eq!(sum.alpha, 0.0);
		assert!(sum.luma_hi < 1e-6, "premultiplied to nothing: {sum:?}");
		let s = settings(SILK_BG, SILK_FG, SILK_CURSOR);
		let out = derive(&sum, &s);
		assert!(
			(lightness(out.fg) - lightness(SILK_FG)).abs() < 0.02,
			"{:?}",
			out.fg
		);
	}
}
