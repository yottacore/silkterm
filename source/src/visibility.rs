// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! How much of the wallpaper, and how much of the text scrim's halo, actually
//! gets drawn. Both are authored amounts - a person moved a slider - and neither
//! survives being handed straight to the renderer.
//!
//! Two things get in the way, and both are the same shape: the number a person
//! picks is perceptual, and what the renderer wants is not.
//!
//! - **The mode.** A linear-light blend is what the program has always drawn,
//!   and sRGB's curve is steep at the bottom and flat at the top, so the same
//!   alpha covers a lot of visible ground over a near-black background and
//!   almost none over a light one. Dark mode is the reference and is never
//!   touched. Light mode mixes the background and the picture in a power curve
//!   instead, at the amount dark mode's blend would have delivered. Both halves
//!   are closed form - nothing is calibrated by eye and nothing is solved.
//! - **The picture.** At one setting a bright photo glares where a dark one is
//!   barely there, because the slider says how much of the picture to mix in
//!   rather than how far to move the background. `even_visibility` holds every
//!   picture to the same displacement, fading out toward 100% where the picture
//!   has to be drawn as it is.
//!
//! The scrim's halo is the one thing still calibrated rather than derived. That
//! composite blends against the destination through the pipeline's blend state
//! and cannot read it, so there is nothing to solve against: its alpha is scaled
//! down until it covers the same ground dark mode's does.
//!
//! The measure throughout is a transfer curve taken on Rec.709 luma. Luma
//! because it is affine under the alpha composite, so one number stands in for a
//! whole blend; a curve because linear light is not what the eye reads.

use crate::config::{self, Settings};

// Where the transfer curve's slope is read when the two modes are compared, as
// a linear luma. Measured over the shipped pack of 104: median 0.129, mean
// 0.141, quartiles 0.062 and 0.193. A picture far from this one is out by a few
// points, not by a factor.
const WALLPAPER_LUMA: f32 = 0.13;

// The transfer curve the mix happens in. A pure power rather than sRGB's own,
// because sRGB's `- 0.055` term does not cancel: over a black background the
// power curve makes the mix exactly the linear blend it replaces, and sRGB's
// would lift the black by eight levels.
const MIX_GAMMA: f32 = 2.4;

// The picture the visibility ramp measures against: the shipped pack's median
// overall brightness and median bright end, as linear luma. A picture matching
// these is drawn at exactly what the slider says.
const REF_MEAN: f32 = 0.124;
const REF_HI: f32 = 0.337;

// Where the two halos are compared. The gain cannot be right across the whole
// falloff - the curves meet at both ends whatever it is - so it is matched at
// half the halo, which is the widest the gap gets.
const HALO_REF: f32 = 0.5;

// The halo never drops below a quarter of what was asked for. Past that the
// plate stops doing the job it is there for, and legibility over a busy picture
// matters more than the plate being tidy.
const MIN_HALO_GAIN: f32 = 0.25;

// Does this settings copy resolve to the dark variant? `Settings` rather than
// the live store, so everything here stays a function of what it is handed.
fn dark(s: &Settings) -> bool {
	crate::theme::is_dark_mode(&s.theme_mode, config::os_dark())
}

// The theme's own dark background - what "as prominent as dark mode" is measured
// against. An overridden `bg` in light mode is still compared with the theme the
// user picked, since that is the dark mode they would see.
fn paired_dark_luma(s: &Settings) -> f32 {
	config::luma(crate::theme::resolve_in(&s.user_themes, &s.theme, "dark", true).bg)
}

// How far a linear-light mix of `from` toward `to` travels in sRGB-encoded luma.
// Signed, in the direction of the mix.
fn shift(from: f32, to: f32, alpha: f32) -> f32 {
	config::from_linear(from + (to - from) * alpha) - config::from_linear(from)
}

// The inverse: the alpha at which that mix covers `want` of sRGB distance. A mix
// that cannot get that far is pinned at 1.
fn alpha_for(from: f32, to: f32, want: f32) -> f32 {
	let span = to - from;
	if span.abs() < 1e-4 {
		return 1.0;
	}
	let target = (config::from_linear(from) + span.signum() * want).clamp(0.0, 1.0);
	((config::to_linear_f32(target) - from) / span).clamp(0.0, 1.0)
}

// The sRGB transfer curve's slope at a linear value. How much of a change in the
// picture the eye gets back, where the composite happens to sit.
fn slope(at: f32) -> f32 {
	if at <= 0.003_130_8 {
		12.92
	} else {
		(1.055 / 2.4) * at.powf(1.0 / 2.4 - 1.0)
	}
}

// How much of the picture's own contrast a linear-light blend at `alpha` puts on
// screen, over a background of `bg`. This is what the visibility slider has
// always meant, whether or not anyone said so.
//
// Over pure black it works out at exactly `alpha^(1/2.4)`, which is why dark
// mode has never needed any of this: black leaves the blend a pure scale of the
// encoded picture, and a scale cannot touch contrast. A dark theme whose
// background is not black delivers less, and this says how much less.
fn encoded_scale(alpha: f32, bg: f32) -> f32 {
	alpha * slope(alpha * WALLPAPER_LUMA + (1.0 - alpha) * bg) / slope(WALLPAPER_LUMA)
}

// The mix curve, on a linear value.
fn curve(x: f32) -> f32 {
	x.max(0.0).powf(1.0 / MIX_GAMMA)
}

// How bright a picture reads, from its overall level and its bright end. The
// bright end is half the answer because glare comes from there, not from the
// average - a photo that is mostly night sky with a sun in it is not a dark
// picture to look at.
fn brightness(mean: f32, hi: f32) -> f32 {
	0.5 * (curve(mean) + curve(hi))
}

// How far this picture sits from the background, against how far the reference
// picture sits from it. 1 is an ordinary picture, above 1 is one that would
// glare, below 1 is one that would barely show.
fn standout(picture: (f32, f32), bg: f32) -> f32 {
	let reference = (brightness(REF_MEAN, REF_HI) - curve(bg)).abs();
	if reference < 1e-4 {
		return 1.0;
	}
	((brightness(picture.0, picture.1) - curve(bg)).abs() / reference).max(1e-3)
}

// The slider's amount, evened out for how bright this picture is. A picture
// further from the background than usual is drawn at less than the number says
// and one closer at more, so the setting means the same thing whatever is
// rotated in next.
//
// The correction fades out as the slider rises and is gone at 100%, because
// there the picture has to be drawn as it is - that is what 100% means, and it
// is the one reading a ramp must not disturb.
fn evened(amount: f32, slider: f32, picture: (f32, f32), bg: f32, strength: f32) -> f32 {
	let strength = strength.clamp(0.0, 1.0);
	if strength <= 0.0 {
		return amount;
	}
	let fade = strength * (1.0 - slider.clamp(0.0, 1.0));
	(amount * standout(picture, bg).powf(-fade)).clamp(0.0, 1.0)
}

// What the wallpaper pass does this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mix {
	// The alpha of the linear blend, or the share of the picture in the power
	// curve. Which one is decided by `perceptual`.
	pub amount: f32,
	// False is the linear-light blend the program has always drawn, and is what
	// dark mode gets. True mixes the background and the picture in a power curve,
	// which needs the background color and so cannot be a hardware blend.
	pub perceptual: bool,
}

impl Mix {
	// The field a glyph sits on, as a linear luma, for a picture of `picture`
	// over a background of `bg`. The renderer's own blend in one number, so the
	// derived text colors are placed against what will really be there.
	pub fn field(self, picture: f32, bg: f32) -> f32 {
		if !self.perceptual {
			return bg + (picture - bg) * self.amount;
		}
		let p = |x: f32| x.max(0.0).powf(1.0 / MIX_GAMMA);
		let mixed = p(bg) + (p(picture) - p(bg)) * self.amount;
		mixed.max(0.0).powf(MIX_GAMMA)
	}
}

// How the wallpaper is drawn, for a slider reading `slider`. `picture` is how
// bright it is - its overall level and its bright end - and None leaves the
// ramp out, for a caller with no picture summarized yet.
pub fn wallpaper_mix(s: &Settings, slider: f32, picture: Option<(f32, f32)>) -> Mix {
	let bg = config::luma(s.bg);
	let even = |amount: f32| match picture {
		Some(p) => evened(amount, slider, p, bg, s.wallpaper_even),
		None => amount,
	};
	if dark(s) {
		return Mix {
			amount: even(slider),
			perceptual: false,
		};
	}
	Mix {
		amount: even(encoded_scale(slider, paired_dark_luma(s)).clamp(0.0, 1.0)),
		perceptual: true,
	}
}

// How much of the asked-for halo alpha is actually drawn, for a wallpaper whose
// slider reads `slider`. 0 means no picture, which leaves the halo alone - it is
// then sitting on the background color it is made of, and invisible either way.
pub fn halo_gain(s: &Settings, slider: f32) -> f32 {
	if dark(s) || slider <= 0.0 {
		return 1.0;
	}
	gain_for(slider, config::luma(s.bg), paired_dark_luma(s))
}

// Light mode's share of the halo alpha, given the two backgrounds. Both modes
// are measured at their own worst case: the halo is the background color, and
// the field under it is the picture as that mode draws it.
fn gain_for(slider: f32, light: f32, dark: f32) -> f32 {
	let shown = Mix {
		amount: encoded_scale(slider, dark).clamp(0.0, 1.0),
		perceptual: true,
	};
	let drawn = Mix {
		amount: slider,
		perceptual: false,
	};
	let field_dark = drawn.field(WALLPAPER_LUMA, dark);
	let field_light = shown.field(WALLPAPER_LUMA, light);
	let want = shift(field_dark, dark, HALO_REF).abs();
	(alpha_for(field_light, light, want) / HALO_REF).clamp(MIN_HALO_GAIN, 1.0)
}

#[cfg(test)]
mod tests {
	use super::{
		HALO_REF, MIN_HALO_GAIN, MIX_GAMMA, Mix, REF_HI, REF_MEAN, WALLPAPER_LUMA, encoded_scale,
		gain_for, halo_gain, shift, standout, wallpaper_mix,
	};
	use crate::config::{self, Settings};

	fn themed(name: &str, mode: &str) -> Settings {
		let pal = crate::theme::resolve(name, mode, true);
		Settings {
			theme: name.to_string(),
			theme_mode: mode.to_string(),
			bg: pal.bg,
			fg: pal.fg,
			cursor: pal.cursor,
			..Settings::default()
		}
	}

	fn bg_luma(name: &str, mode: &str) -> f32 {
		config::luma(crate::theme::resolve(name, mode, true).bg)
	}

	const SLIDERS: [f32; 7] = [0.0, 0.05, 0.1, 0.25, 0.5, 0.8, 1.0];
	// a picture's dark and bright ends, as linear luma
	const LO: f32 = 0.02;
	const HI: f32 = 0.45;

	// Dark mode never takes the other blend, whatever the picture. The visibility
	// ramp does reach it - that is the point of the ramp - so the exact identity
	// only holds with the ramp off, which is what the second half checks.
	// Test ID: EqTTXtg
	#[test]
	fn dark_mode_still_draws_the_blend_it_always_has() {
		for name in crate::theme::names() {
			// "system" resolves through the OS bit, which the tests leave dark
			for mode in ["dark", "system"] {
				let mut s = themed(name, mode);
				for v in SLIDERS {
					for picture in [None, Some(DIM), Some(ORDINARY), Some(BRIGHT)] {
						assert!(
							!wallpaper_mix(&s, v, picture).perceptual,
							"{name} {mode} {v}"
						);
					}
					assert_eq!(halo_gain(&s, v), 1.0, "{name} {mode} {v}");
				}
				s.wallpaper_even = 0.0;
				for v in SLIDERS {
					for picture in [None, Some(DIM), Some(BRIGHT)] {
						let mix = wallpaper_mix(&s, v, picture);
						assert_eq!(mix.amount, v, "{name} {mode} {v} {picture:?}");
					}
				}
			}
		}
	}

	// The whole reason the mix is a pure power curve rather than sRGB's own. Over
	// a black background the two are the same arithmetic, so a dark theme could
	// take either path and draw the same pixels.
	// Test ID: EqTOWF6
	#[test]
	fn over_black_the_mix_is_the_blend_it_replaces() {
		for v in SLIDERS {
			let blend = Mix {
				amount: v,
				perceptual: false,
			};
			let curve = Mix {
				amount: v.powf(1.0 / MIX_GAMMA),
				perceptual: true,
			};
			for p in [0.0f32, 0.01, 0.13, 0.5, 1.0] {
				let (a, b) = (blend.field(p, 0.0), curve.field(p, 0.0));
				assert!((a - b).abs() < 1e-5, "v {v}, picture {p}: {a} against {b}");
			}
		}
	}

	// What the slider means, in both modes: this much of the picture's own
	// contrast reaches the screen.
	fn contrast_on_screen(mix: Mix, bg: f32) -> f32 {
		config::from_linear(mix.field(HI, bg)) - config::from_linear(mix.field(LO, bg))
	}

	// Test ID: EqTOWF7
	#[test]
	fn light_mode_shows_the_contrast_dark_mode_shows() {
		for name in crate::theme::names() {
			let (dark, light) = (bg_luma(name, "dark"), bg_luma(name, "light"));
			for v in SLIDERS {
				let in_dark = contrast_on_screen(
					Mix {
						amount: v,
						perceptual: false,
					},
					dark,
				);
				let in_light =
					contrast_on_screen(wallpaper_mix(&themed(name, "light"), v, None), light);
				// the stand-in picture is one luma and a real one is a spread, so
				// this is close rather than exact
				assert!(
					(in_dark - in_light).abs() < 0.02,
					"{name} {v}: dark {in_dark}, light {in_light}"
				);
			}
		}
	}

	// Test ID: EqTOWF8
	#[test]
	fn the_shipped_default_asks_for_the_scale_black_would_have_given() {
		let s = themed("SilkTerm", "light");
		let mix = wallpaper_mix(&s, 0.10, None);
		assert!(mix.perceptual);
		// SilkTerm's dark background is black, so the closed form is exact
		assert!(
			(mix.amount - 0.10f32.powf(1.0 / MIX_GAMMA)).abs() < 1e-4,
			"{mix:?}"
		);
	}

	// A dark theme whose background is not black already shows less picture, so
	// its light mode shows less too. Self-consistent rather than uniform.
	// Test ID: EqTOWF9
	#[test]
	fn a_theme_with_a_lifted_dark_background_asks_for_less() {
		let silk = wallpaper_mix(&themed("SilkTerm", "light"), 0.10, None).amount;
		let pastel = wallpaper_mix(&themed("Pastel", "light"), 0.10, None).amount;
		assert!(bg_luma("Pastel", "dark") > bg_luma("SilkTerm", "dark"));
		assert!(pastel < silk - 0.05, "silk {silk}, pastel {pastel}");
	}

	// Test ID: EqTOWFA
	#[test]
	fn the_scale_runs_end_to_end_and_only_upward() {
		for name in crate::theme::names() {
			let dark = bg_luma(name, "dark");
			assert_eq!(encoded_scale(0.0, dark), 0.0, "{name}");
			assert!((encoded_scale(1.0, dark) - 1.0).abs() < 1e-5, "{name}");
			let mut last = 0.0;
			for i in 0..=100 {
				let g = encoded_scale(i as f32 / 100.0, dark);
				assert!(g >= last - 1e-6, "{name} at {i}: {g} after {last}");
				last = g;
			}
		}
	}

	// Test ID: EqT4HIf
	#[test]
	fn the_halo_is_quietened_wherever_a_picture_is_up() {
		let s = themed("SilkTerm", "light");
		for v in [0.05f32, 0.1, 0.35, 0.75, 1.0] {
			let g = halo_gain(&s, v);
			assert!((MIN_HALO_GAIN..1.0).contains(&g), "{v}: {g}");
		}
	}

	// Test ID: EqT4HIg
	#[test]
	fn the_halo_covers_the_same_ground_in_both_modes() {
		let name = "SilkTerm";
		let (dark, light) = (bg_luma(name, "dark"), bg_luma(name, "light"));
		for v in [0.05f32, 0.1, 0.25, 0.5] {
			let gain = gain_for(v, light, dark);
			let field_dark = Mix {
				amount: v,
				perceptual: false,
			}
			.field(WALLPAPER_LUMA, dark);
			let field_light =
				wallpaper_mix(&themed(name, "light"), v, None).field(WALLPAPER_LUMA, light);
			let want = shift(field_dark, dark, HALO_REF).abs();
			let got = shift(field_light, light, HALO_REF * gain).abs();
			assert!(
				(got - want).abs() < 0.01 || gain <= MIN_HALO_GAIN,
				"{v}: wanted {want}, got {got}"
			);
		}
	}

	// The visibility ramp. A picture further from the background than the pack's
	// median is drawn at less than the slider says, and one closer at more.
	const DIM: (f32, f32) = (0.01, 0.04);
	const BRIGHT: (f32, f32) = (0.45, 0.80);
	const ORDINARY: (f32, f32) = (REF_MEAN, REF_HI);

	// Test ID: EqTTXth
	#[test]
	fn an_ordinary_picture_is_drawn_at_what_the_slider_says() {
		for name in crate::theme::names() {
			for mode in ["dark", "light"] {
				let s = themed(name, mode);
				for v in SLIDERS {
					let plain = wallpaper_mix(&s, v, None).amount;
					let evened = wallpaper_mix(&s, v, Some(ORDINARY)).amount;
					assert!(
						(plain - evened).abs() < 0.02,
						"{name} {mode} {v}: {plain} {evened}"
					);
				}
			}
		}
	}

	// Test ID: EqTTXti
	#[test]
	fn a_glaring_picture_is_held_back_and_a_faint_one_lifted() {
		let s = themed("SilkTerm", "dark");
		for v in [0.05f32, 0.1, 0.35, 0.6] {
			let plain = wallpaper_mix(&s, v, None).amount;
			assert!(
				wallpaper_mix(&s, v, Some(BRIGHT)).amount < plain,
				"bright at {v}"
			);
			assert!(wallpaper_mix(&s, v, Some(DIM)).amount > plain, "dim at {v}");
		}
	}

	// The same rule read from the other side, which is what the backlog asked
	// for: over a light background it is the DARK picture that stands out.
	// Test ID: EqTTXtj
	#[test]
	fn light_mode_holds_back_the_picture_that_stands_out_there_instead() {
		let s = themed("SilkTerm", "light");
		for v in [0.05f32, 0.1, 0.35, 0.6] {
			let plain = wallpaper_mix(&s, v, None).amount;
			assert!(wallpaper_mix(&s, v, Some(DIM)).amount < plain, "dim at {v}");
			assert!(
				wallpaper_mix(&s, v, Some(BRIGHT)).amount > plain,
				"bright at {v}"
			);
		}
		// and the two modes disagree about which picture that is
		assert!(
			standout(
				BRIGHT,
				config::luma(crate::theme::resolve("SilkTerm", "dark", true).bg)
			) > 1.0
		);
		assert!(
			standout(
				BRIGHT,
				config::luma(crate::theme::resolve("SilkTerm", "light", true).bg)
			) < 1.0
		);
	}

	// Glare comes from a picture's bright end, not from its average. A night sky
	// with a sun in it has the same overall level as a flat dark picture and is
	// nothing like it to look at, so the ramp has to tell them apart.
	// Test ID: EqTTXtk
	#[test]
	fn a_dark_picture_with_a_bright_area_is_not_a_dark_picture() {
		let s = themed("SilkTerm", "dark");
		let flat = (0.05f32, 0.08f32);
		let with_a_sun = (0.05f32, 0.60f32);
		for v in [0.05f32, 0.1, 0.35] {
			let a = wallpaper_mix(&s, v, Some(flat)).amount;
			let b = wallpaper_mix(&s, v, Some(with_a_sun)).amount;
			assert!(b < a - 0.01, "{v}: flat {a}, with a bright area {b}");
		}
	}

	// Test ID: EqTTXtl
	#[test]
	fn a_full_slider_always_draws_the_picture_as_it_is() {
		for name in crate::theme::names() {
			for mode in ["dark", "light"] {
				for picture in [DIM, ORDINARY, BRIGHT] {
					let mut s = themed(name, mode);
					for strength in [0.0f32, 0.5, 1.0] {
						s.wallpaper_even = strength;
						let mix = wallpaper_mix(&s, 1.0, Some(picture));
						assert!(
							(mix.amount - 1.0).abs() < 1e-5,
							"{name} {mode} {strength}: {mix:?}"
						);
					}
				}
			}
		}
	}

	// Test ID: EqTTXtm
	#[test]
	fn the_strength_setting_turns_the_ramp_off() {
		let mut s = themed("SilkTerm", "dark");
		s.wallpaper_even = 0.0;
		for v in SLIDERS {
			for picture in [DIM, ORDINARY, BRIGHT] {
				assert_eq!(
					wallpaper_mix(&s, v, Some(picture)).amount,
					v,
					"{v} {picture:?}"
				);
			}
		}
	}

	// Test ID: EqTTXtn
	#[test]
	fn the_ramp_never_takes_the_slider_backwards() {
		for mode in ["dark", "light"] {
			let s = themed("SilkTerm", mode);
			for picture in [DIM, ORDINARY, BRIGHT] {
				let mut last = 0.0;
				for i in 0..=100 {
					let a = wallpaper_mix(&s, i as f32 / 100.0, Some(picture)).amount;
					assert!(
						a >= last - 1e-5,
						"{mode} {picture:?} at {i}: {a} after {last}"
					);
					last = a;
				}
			}
		}
	}

	// Test ID: EqT4HIh
	#[test]
	fn no_picture_leaves_the_halo_alone() {
		assert_eq!(halo_gain(&themed("SilkTerm", "light"), 0.0), 1.0);
	}
}
