// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Background-image "contrast mask": flatten the image's contrast toward a local
//! mean so it stops competing with terminal text. Applied uniformly across the
//! whole image at load, in linear light (baked into the texture, like the blur).
//!
//! Three knobs (all 0..1):
//! - `size`: the flatten scale - the localMean blur radius. 1.0 = half the
//!   longest pixel dimension (localMean ~ the global average, so the whole image
//!   collapses toward one tone); small = a tight neighborhood, so only fine busy
//!   detail flattens. 0 = off.
//! - `strength`: how far each pixel is pulled toward that local mean. 0 = none,
//!   1 = fully flat.
//! - `auto`: blend the two manual knobs with values derived from the image's own
//!   busyness (a busy image gets flattened more). 1.0 = full auto override, 0.0 =
//!   manual only, 0.5 = average of the two.

use crate::config::LUMA;
use image::{ImageBuffer, Rgba};
use std::array::from_fn;

type Linear = ImageBuffer<Rgba<f32>, Vec<f32>>;

// Rec.709 luma weights (the buffer is linear-light).

// Busyness -> auto knob endpoints (feel-tunable). A smooth image needs little;
// a busy one wants fine detail knocked down: smaller scale, more strength.
const AUTO_SIZE_SMOOTH: f32 = 0.6;
const AUTO_SIZE_BUSY: f32 = 0.2;
const AUTO_STRENGTH_SMOOTH: f32 = 0.2;
const AUTO_STRENGTH_BUSY: f32 = 0.8;
// Gradient -> busyness saturation. Linear-luma gradients run small, so this maps
// a modest mean gradient into most of the 0..1 range. Tunable.
const BUSY_K: f32 = 8.0;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
	a + (b - a) * t
}

// Blend a manual value with an auto value by the auto amount.
fn blend(manual: f32, auto: f32, amount: f32) -> f32 {
	lerp(manual, auto, amount.clamp(0.0, 1.0))
}

// size fraction -> localMean radius in px. 1.0 = half the longest dimension.
fn size_to_radius(size: f32, max_dim: u32) -> u32 {
	(size.clamp(0.0, 1.0) * 0.5 * max_dim as f32).round() as u32
}

// Mean |luma gradient| over the image, mapped to a saturating 0..1 busyness.
// Flat image -> 0; lots of high-frequency detail -> toward 1. `scale` is the
// image's size against the one BUSY_K was tuned on: a pixel of an image held
// smaller spans more of the picture, so its steps are bigger.
fn busyness(px: &[[f32; 3]], w: usize, h: usize, scale: f32) -> f32 {
	if w < 2 || h < 2 {
		return 0.0;
	}
	let luma = |p: &[f32; 3]| p[0] * LUMA[0] + p[1] * LUMA[1] + p[2] * LUMA[2];
	let mut sum = 0.0f64;
	let mut n = 0u64;
	for y in 0..h {
		for x in 0..w {
			let l = luma(&px[y * w + x]);
			if x + 1 < w {
				sum += (luma(&px[y * w + x + 1]) - l).abs() as f64;
				n += 1;
			}
			if y + 1 < h {
				sum += (luma(&px[(y + 1) * w + x]) - l).abs() as f64;
				n += 1;
			}
		}
	}
	if n == 0 {
		return 0.0;
	}
	let g = (sum / n as f64) as f32 * scale;
	1.0 - (-BUSY_K * g).exp()
}

// Auto (size, strength) picked from image busyness.
fn auto_params(busy: f32) -> (f32, f32) {
	let b = busy.clamp(0.0, 1.0);
	(
		lerp(AUTO_SIZE_SMOOTH, AUTO_SIZE_BUSY, b),
		lerp(AUTO_STRENGTH_SMOOTH, AUTO_STRENGTH_BUSY, b),
	)
}

// Separable box mean (radius r) via per-row/col prefix sums, edge-clamped and
// count-normalized so borders average only the samples they actually have.
// O(pixels) regardless of radius. f64 accumulation avoids large-sum drift.
fn box_mean(src: &[[f32; 3]], w: usize, h: usize, r: usize) -> Vec<[f32; 3]> {
	if r == 0 {
		return src.to_vec();
	}
	let mut tmp = vec![[0.0f32; 3]; w * h];
	let mut pre = vec![[0.0f64; 3]; w.max(h) + 1];
	// horizontal
	for y in 0..h {
		for x in 0..w {
			let s = &src[y * w + x];
			pre[x + 1] = from_fn(|c| pre[x][c] + s[c] as f64);
		}
		for x in 0..w {
			let lo = x.saturating_sub(r);
			let hi = (x + r).min(w - 1);
			let cnt = (hi - lo + 1) as f64;
			tmp[y * w + x] = from_fn(|c| ((pre[hi + 1][c] - pre[lo][c]) / cnt) as f32);
		}
	}
	// vertical
	let mut out = vec![[0.0f32; 3]; w * h];
	for x in 0..w {
		for y in 0..h {
			let s = &tmp[y * w + x];
			pre[y + 1] = from_fn(|c| pre[y][c] + s[c] as f64);
		}
		for y in 0..h {
			let lo = y.saturating_sub(r);
			let hi = (y + r).min(h - 1);
			let cnt = (hi - lo + 1) as f64;
			out[y * w + x] = from_fn(|c| ((pre[hi + 1][c] - pre[lo][c]) / cnt) as f32);
		}
	}
	out
}

/// Flatten the image's contrast in place. `img` is linear-light RGBA f32; alpha
/// is left untouched. No-op when the effective strength or size comes out at zero.
/// `scale` is what the wallpaper is held at against its full size, so a smaller
/// copy of the same picture reads as busy as the whole one.
pub fn apply(img: &mut Linear, size: f32, strength: f32, auto: f32, scale: f32) {
	let (w, h) = (img.width() as usize, img.height() as usize);
	if w == 0 || h == 0 {
		return;
	}
	let mut rgb: Vec<[f32; 3]> = img.pixels().map(|p| [p[0], p[1], p[2]]).collect();

	let (eff_size, eff_strength) = if auto > 0.0 {
		let (a_size, a_strength) = auto_params(busyness(&rgb, w, h, scale));
		(blend(size, a_size, auto), blend(strength, a_strength, auto))
	} else {
		(size.clamp(0.0, 1.0), strength.clamp(0.0, 1.0))
	};
	if eff_strength <= 0.0 {
		return;
	}
	let r = size_to_radius(eff_size, w.max(h) as u32) as usize;
	if r == 0 {
		return; // localMean == pixel -> nothing to flatten
	}
	let mean = box_mean(&rgb, w, h, r);
	for (p, m) in rgb.iter_mut().zip(mean.iter()) {
		for (pc, mc) in p.iter_mut().zip(m.iter()) {
			*pc = lerp(*pc, *mc, eff_strength);
		}
	}
	for (dst, src) in img.pixels_mut().zip(rgb.iter()) {
		dst[0] = src[0];
		dst[1] = src[1];
		dst[2] = src[2];
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn variance(px: &[[f32; 3]]) -> f32 {
		let n = px.len() as f32;
		let mut mean = [0.0f32; 3];
		for p in px {
			for (mc, pc) in mean.iter_mut().zip(p) {
				*mc += pc;
			}
		}
		for mc in &mut mean {
			*mc /= n;
		}
		let mut v = 0.0;
		for p in px {
			for (pc, mc) in p.iter().zip(mean.iter()) {
				v += (pc - mc).powi(2);
			}
		}
		v / n
	}

	fn buf(rgb: &[[f32; 3]], w: u32, h: u32) -> Linear {
		let mut img = Linear::new(w, h);
		for (dst, src) in img.pixels_mut().zip(rgb.iter()) {
			*dst = Rgba([src[0], src[1], src[2], 1.0]);
		}
		img
	}

	fn checker8() -> Vec<[f32; 3]> {
		let mut c = vec![[0.0; 3]; 64];
		for y in 0..8 {
			for x in 0..8 {
				let v = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
				c[y * 8 + x] = [v, v, v];
			}
		}
		c
	}

	// Test ID: Ek1QlWa
	#[test]
	fn size_maps_to_half_the_longest_side() {
		assert_eq!(size_to_radius(1.0, 100), 50);
		assert_eq!(size_to_radius(0.5, 100), 25);
		assert_eq!(size_to_radius(0.0, 100), 0);
	}

	// Test ID: Ek1QlWb
	#[test]
	fn blend_is_manual_at_zero_auto_at_one() {
		assert_eq!(blend(0.2, 0.8, 0.0), 0.2);
		assert_eq!(blend(0.2, 0.8, 1.0), 0.8);
		assert!((blend(0.2, 0.8, 0.5) - 0.5).abs() < 1e-6);
	}

	// Test ID: Ek1QlWc
	#[test]
	fn flat_image_has_zero_busyness_checkerboard_high() {
		let flat = vec![[0.5, 0.5, 0.5]; 64];
		assert_eq!(busyness(&flat, 8, 8, 1.0), 0.0);
		assert!(busyness(&checker8(), 8, 8, 1.0) > 0.5);
	}

	// Test ID: Ek1QlWd
	#[test]
	fn auto_flattens_busy_more_than_smooth() {
		let (ss, sst) = auto_params(0.0);
		let (bs, bst) = auto_params(1.0);
		assert!(bs < ss); // busy -> smaller scale
		assert!(bst > sst); // busy -> more strength
	}

	// Test ID: Ek1QlWe
	#[test]
	fn box_mean_radius_zero_is_identity() {
		let src = vec![[0.1, 0.2, 0.3], [0.9, 0.8, 0.7]];
		assert_eq!(box_mean(&src, 2, 1, 0), src);
	}

	// Test ID: Ek1QlWf
	#[test]
	fn box_mean_large_radius_approaches_global_average() {
		let src = vec![[0.0; 3], [0.0; 3], [1.0; 3], [1.0; 3]];
		let m = box_mean(&src, 4, 1, 100);
		for p in &m {
			for &val in p {
				assert!((val - 0.5).abs() < 1e-5);
			}
		}
	}

	// Test ID: Ek1QlWg
	#[test]
	fn full_strength_large_size_collapses_variance() {
		let checker = checker8();
		let before = variance(&checker);
		let mut img = buf(&checker, 8, 8);
		apply(&mut img, 1.0, 1.0, 0.0, 1.0); // manual only, full flatten
		let after_px: Vec<[f32; 3]> = img.pixels().map(|p| [p[0], p[1], p[2]]).collect();
		assert!(variance(&after_px) < before * 0.05);
	}

	// Test ID: Ek1QlWh
	#[test]
	fn zero_strength_is_a_noop() {
		let src = vec![
			[0.1, 0.2, 0.3],
			[0.9, 0.8, 0.7],
			[0.4, 0.5, 0.6],
			[0.2, 0.1, 0.0],
		];
		let mut img = buf(&src, 2, 2);
		apply(&mut img, 1.0, 0.0, 0.0, 1.0);
		let out: Vec<[f32; 3]> = img.pixels().map(|p| [p[0], p[1], p[2]]).collect();
		assert_eq!(out, src);
	}
}
