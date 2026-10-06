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
//! The scrim's halo blends against the destination through the pipeline's blend
//! state and cannot read it, so it is matched against the picture's average
//! instead: light mode redraws each halo alpha at whatever moves that field as
//! far as dark mode's halo moves dark mode's.
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

// Does this settings copy resolve to the dark variant? `Settings` rather than
// the live store, so everything here stays a function of what it is handed.
fn dark(settings: &Settings) -> bool {
	settings.theme_mode.is_dark(config::os_dark())
}

// The theme's own dark background - what "as prominent as dark mode" is measured
// against. An overridden `bg` in light mode is still compared with the theme the
// user picked, since that is the dark mode they would see.
fn paired_dark_luma(settings: &Settings) -> f32 {
	config::luma(paired_dark_bg(settings))
}

fn paired_dark_bg(settings: &Settings) -> [u8; 3] {
	crate::theme::resolve_in(
		&settings.user_themes,
		&settings.theme,
		crate::theme::Mode::Dark,
		true,
	)
	.bg
}

// How far a linear-light mix of `from` toward `to` travels in sRGB-encoded luma.
// Signed, in the direction of the mix.
fn shift(from: f32, to: f32, alpha: f32) -> f32 {
	config::from_linear(from + (to - from) * alpha) - config::from_linear(from)
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

/// What the wallpaper pass does this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mix {
	/// The alpha of the linear blend, or the share of the picture in the power
	/// curve. Which one is decided by `perceptual`.
	pub amount: f32,
	/// False is the linear-light blend the program has always drawn, and is what
	/// dark mode gets. True mixes the background and the picture in a power curve,
	/// which needs the background color and so cannot be a hardware blend.
	pub perceptual: bool,
}

impl Mix {
	/// The field a glyph sits on, as a linear luma, for a picture of `picture`
	/// over a background of `bg`. The renderer's own blend in one number, so the
	/// derived text colors are placed against what will really be there.
	pub fn field(self, picture: f32, bg: f32) -> f32 {
		if !self.perceptual {
			return bg + (picture - bg) * self.amount;
		}
		let p = |x: f32| x.max(0.0).powf(1.0 / MIX_GAMMA);
		let mixed = p(bg) + (p(picture) - p(bg)) * self.amount;
		mixed.max(0.0).powf(MIX_GAMMA)
	}
}

/// How the wallpaper is drawn, for a slider reading `slider`. `picture` is how
/// bright it is - its overall level and its bright end - and None leaves the
/// ramp out, for a caller with no picture summarized yet.
pub fn wallpaper_mix(settings: &Settings, slider: f32, picture: Option<(f32, f32)>) -> Mix {
	let bg = config::luma(settings.bg);
	let even = |amount: f32| match picture {
		Some(p) => evened(amount, slider, p, bg, settings.wallpaper_even),
		None => amount,
	};
	if dark(settings) {
		return Mix {
			amount: even(slider),
			perceptual: false,
		};
	}
	Mix {
		amount: even(encoded_scale(slider, paired_dark_luma(settings)).clamp(0.0, 1.0)),
		perceptual: true,
	}
}

/// How many points the light-mode halo curve is solved at. They are spaced
/// evenly in how far dark mode's halo moves a picture, which is
/// `1 - (1 - alpha)^(1/2.4)` of its distance from black, rather than evenly in
/// alpha: that is where the curve bends, and in those terms it is close to a
/// straight line. The shader joins the points with straight lines.
pub const HALO_NODES: usize = 12;

// The asked alpha at node `k`.
fn halo_node(k: usize) -> f32 {
	1.0 - (1.0 - k as f32 / (HALO_NODES - 1) as f32).powf(MIX_GAMMA)
}

/// Light mode's halo, as the alpha to draw for each alpha asked. Solved so the
/// halo moves the picture under it toward the background as far, in sRGB levels
/// and on average over the picture, as dark mode's halo moves dark mode's
/// picture. The composite runs it on every halo alpha, the crisp outline
/// included (`matched_alpha` in scrim.rs).
///
/// One scalar gain used to stand in for this, matched at half the halo over an
/// average picture, with the outline left at full strength. sRGB's curve is
/// steep near black and flat near white, so dark mode's halo builds slowly and
/// light mode's fast, and the gap is widest over the dark parts of a picture,
/// which light mode shows far more of. A gain could not follow either.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HaloMatch {
	pub curve: [f32; HALO_NODES],
}

impl HaloMatch {
	// What the shader draws for `asked`. It reads the same numbers the same
	// way, so this is what the tests hold.
	#[cfg(test)]
	pub fn alpha(&self, asked: f32) -> f32 {
		let reach = 1.0 - (1.0 - asked.clamp(0.0, 1.0)).powf(1.0 / MIX_GAMMA);
		let x = reach * (HALO_NODES - 1) as f32;
		let i = (x as usize).min(HALO_NODES - 2);
		let t = x - i as f32;
		self.curve[i] + (self.curve[i + 1] - self.curve[i]) * t
	}
}

// Rec.709, matching config::luma.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

// How far, on average and in sRGB levels, a halo of `alpha` moves these fields
// toward `bg`. A channel at a time, weighted as luma, since that is how both
// renderers blend and how far a colored pixel visibly moves.
fn moved(fields: &[[f32; 3]], bg: [f32; 3], alpha: f32) -> f32 {
	let one = |f: &[f32; 3]| {
		(0..3)
			.map(|c| LUMA[c] * shift(f[c], bg[c], alpha).abs())
			.sum::<f32>()
	};
	fields.iter().map(one).sum::<f32>() / fields.len().max(1) as f32
}

/// How the halo is redrawn for a wallpaper whose slider reads `slider`, whose
/// brightness is `picture` and whose linear channels run `spread` (the summary's
/// quantiles). None leaves the halo as asked: dark mode is the reference, and
/// with no picture up the halo sits on the background color it is made of. A
/// picture not summarized yet stands in as a gray at the shipped pack's median.
pub fn halo_match(
	settings: &Settings,
	slider: f32,
	picture: Option<(f32, f32)>,
	spread: &[[f32; 3]],
) -> Option<HaloMatch> {
	if dark(settings) || slider <= 0.0 {
		return None;
	}
	let picture = picture.unwrap_or((REF_MEAN, REF_HI));
	let gray = [[picture.0; 3]];
	let spread = if spread.is_empty() { &gray[..] } else { spread };
	let linear = |c: [u8; 3]| c.map(config::to_linear);
	let (dark_bg, light_bg) = (linear(paired_dark_bg(settings)), linear(settings.bg));
	// each mode's own blend, the visibility ramp included
	let dark_mix = Mix {
		amount: evened(
			slider,
			slider,
			picture,
			paired_dark_luma(settings),
			settings.wallpaper_even,
		),
		perceptual: false,
	};
	let light_mix = wallpaper_mix(settings, slider, Some(picture));
	let fields = |mix: Mix, bg: [f32; 3]| -> Vec<[f32; 3]> {
		spread
			.iter()
			.map(|p| std::array::from_fn(|c| mix.field(p[c], bg[c])))
			.collect()
	};
	let (dark, light) = (fields(dark_mix, dark_bg), fields(light_mix, light_bg));
	let reach = moved(&light, light_bg, 1.0);
	let mut curve = [0.0f32; HALO_NODES];
	for (k, node) in curve.iter_mut().enumerate().skip(1) {
		let want = moved(&dark, dark_bg, halo_node(k));
		// a light field that cannot move that far is simply covered
		if reach <= want {
			*node = 1.0;
			continue;
		}
		let (mut lo, mut hi) = (0.0f32, 1.0f32);
		for _ in 0..16 {
			let mid = 0.5 * (lo + hi);
			if moved(&light, light_bg, mid) < want {
				lo = mid;
			} else {
				hi = mid;
			}
		}
		*node = 0.5 * (lo + hi);
	}
	Some(HaloMatch { curve })
}

/// `halo_match` is several thousand powers, and its inputs change only with the
/// picture, the theme or a slider, so a window keeps the last answer.
#[derive(Default, Debug)]
pub struct HaloMemo {
	key: Vec<u32>,
	next: Vec<u32>,
	value: Option<HaloMatch>,
}

impl HaloMemo {
	pub fn get(
		&mut self,
		settings: &Settings,
		slider: f32,
		picture: Option<(f32, f32)>,
		spread: &[[f32; 3]],
	) -> Option<HaloMatch> {
		let key = &mut self.next;
		key.clear();
		let (bg, dark_bg) = (settings.bg, paired_dark_bg(settings));
		key.extend([
			u32::from(dark(settings)),
			slider.to_bits(),
			u32::from_le_bytes([bg[0], bg[1], bg[2], 0]),
			u32::from_le_bytes([dark_bg[0], dark_bg[1], dark_bg[2], 0]),
			settings.wallpaper_even.to_bits(),
		]);
		if let Some((mean, hi)) = picture {
			key.extend([1, mean.to_bits(), hi.to_bits()]);
		}
		key.extend(spread.iter().flatten().map(|l| l.to_bits()));
		if self.next != self.key {
			self.value = halo_match(settings, slider, picture, spread);
			std::mem::swap(&mut self.key, &mut self.next);
		}
		self.value
	}
}

#[cfg(test)]
mod tests {
	use super::{
		HALO_NODES, HaloMemo, MIX_GAMMA, Mix, REF_HI, REF_MEAN, encoded_scale, halo_match, moved,
		standout, wallpaper_mix,
	};
	use crate::config::{self, Settings};

	// the tests name a mode by its config word
	fn mode_of(word: &str) -> crate::theme::Mode {
		<crate::theme::Mode as crate::config::Choice>::parse(word).expect("a mode")
	}

	fn themed(name: &str, mode: &str) -> Settings {
		let pal = crate::theme::resolve(name, mode_of(mode), true);
		Settings {
			theme: name.to_string(),
			theme_mode: mode_of(mode),
			bg: pal.bg,
			fg: pal.fg,
			cursor: pal.cursor,
			..Settings::default()
		}
	}

	fn bg_luma(name: &str, mode: &str) -> f32 {
		config::luma(crate::theme::resolve(name, mode_of(mode), true).bg)
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
					for picture in [None, Some(DIM), Some(BRIGHT)] {
						assert_eq!(
							halo_match(&s, v, picture, &SPREAD),
							None,
							"{name} {mode} {v}"
						);
					}
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
	fn the_default_asks_for_the_scale_black_would_have_given() {
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

	// A picture's quantiles, darkest first, standing in for a summary's
	const SPREAD: [[f32; 3]; 6] = [
		[0.004; 3], [0.02; 3], [0.06; 3], [0.13; 3], [0.25; 3], [0.5; 3],
	];

	// Quantiles that go with each stand-in picture: grays, and a strongly colored
	// version of the same, which is where a luma-only model went wrong.
	fn spreads_of(picture: (f32, f32)) -> [Vec<[f32; 3]>; 2] {
		let (mean, hi) = picture;
		let steps = [
			mean * 0.05,
			mean * 0.3,
			mean * 0.7,
			mean,
			(mean + hi) * 0.5,
			hi,
		];
		[
			steps.iter().map(|&l| [l; 3]).collect(),
			steps
				.iter()
				.map(|&l| [(l * 2.5).min(1.0), l * 0.4, (l * 1.5).min(1.0)])
				.collect(),
		]
	}

	fn bg_linear(name: &str, mode: &str) -> [f32; 3] {
		crate::theme::resolve(name, mode_of(mode), true)
			.bg
			.map(config::to_linear)
	}

	// Each mode's fields for these channels, as the renderers draw them.
	fn fields(
		name: &str,
		mode: &str,
		v: f32,
		picture: (f32, f32),
		spread: &[[f32; 3]],
	) -> Vec<[f32; 3]> {
		let mix = wallpaper_mix(&themed(name, mode), v, Some(picture));
		let bg = bg_linear(name, mode);
		spread
			.iter()
			.map(|p| std::array::from_fn(|c| mix.field(p[c], bg[c])))
			.collect()
	}

	// Test ID: EqT4HIf
	#[test]
	fn the_halo_is_quietened_wherever_a_picture_is_up() {
		let s = themed("SilkTerm", "light");
		for v in [0.05f32, 0.1, 0.35, 0.75, 1.0] {
			let m = halo_match(&s, v, None, &SPREAD).expect("a light theme with a picture up");
			for a in [0.1f32, 0.5, 1.0] {
				let drawn = m.alpha(a);
				assert!(drawn > 0.0 && drawn < a, "{v} at {a}: {drawn}");
			}
		}
	}

	// was: matched at half the halo only, through one gain, over one average
	// picture. It now has to hold at every alpha and over the whole picture, since
	// the tail and the dark parts are where the old gain left light mode loud.
	// Test ID: EqT4HIg
	#[test]
	fn the_halo_covers_the_same_ground_in_both_modes() {
		for name in crate::theme::names() {
			let s = themed(name, "light");
			for picture in [DIM, ORDINARY, BRIGHT] {
				for (v, spread) in [0.05f32, 0.1, 0.25, 0.5]
					.into_iter()
					.flat_map(|v| spreads_of(picture).map(|sp| (v, sp)))
				{
					let m = halo_match(&s, v, Some(picture), &spread).expect("a picture is up");
					let dark = fields(name, "dark", v, picture, &spread);
					let light = fields(name, "light", v, picture, &spread);
					let (dark_bg, light_bg) = (bg_linear(name, "dark"), bg_linear(name, "light"));
					for i in 0..=32 {
						let a = i as f32 / 32.0;
						let want = moved(&dark, dark_bg, a);
						let drawn = m.alpha(a);
						let got = moved(&light, light_bg, drawn);
						// within a level, or full where light mode cannot reach that far
						assert!(
							(got - want).abs() < 1.0 / 255.0 || (drawn >= 1.0 && got < want),
							"{name} {picture:?} {v} at {a}: wanted {}, got {}",
							want * 255.0,
							got * 255.0
						);
					}
				}
			}
		}
	}

	// A picture that barely shows in dark mode gets a halo that barely shows in
	// light mode too, rather than the one an ordinary picture would get. Light
	// mode draws a dark picture much further from its paper than dark mode
	// draws it from black, so without this its halo was the loudest of all.
	// Test ID: Ers4sAg
	#[test]
	fn the_halo_follows_the_picture_it_sits_on() {
		let s = themed("SilkTerm", "light");
		for v in [0.1f32, 0.35] {
			let dim = halo_match(&s, v, Some(DIM), &[[DIM.0; 3], [DIM.1; 3]]).expect("up");
			let ordinary = halo_match(&s, v, Some(ORDINARY), &[]).expect("up");
			assert!(
				dim.alpha(1.0) < ordinary.alpha(1.0) * 0.8,
				"{v}: dim {dim:?}, ordinary {ordinary:?}"
			);
		}
		// and with no quantiles the picture's own mean stands in for them
		assert_eq!(
			halo_match(&s, 0.1, Some(ORDINARY), &[]),
			halo_match(&s, 0.1, Some(ORDINARY), &[[ORDINARY.0; 3]])
		);
	}

	// Test ID: Ers4sWJ
	#[test]
	fn the_matched_halo_only_ever_grows_with_what_was_asked() {
		for name in crate::theme::names() {
			for picture in [DIM, ORDINARY, BRIGHT] {
				let m =
					halo_match(&themed(name, "light"), 0.1, Some(picture), &SPREAD).expect("up");
				assert_eq!(m.alpha(0.0), 0.0, "{name} {picture:?}");
				assert_eq!(m.curve.len(), HALO_NODES);
				let mut last = 0.0;
				for i in 0..=100 {
					let drawn = m.alpha(i as f32 / 100.0);
					assert!(drawn >= last - 1e-6, "{name} {picture:?} at {i}");
					assert!(drawn <= 1.0, "{name} {picture:?} at {i}");
					last = drawn;
				}
			}
		}
	}

	// The memo answers what a fresh solve would, and solves again when anything
	// it is keyed on moves.
	// Test ID: Ers4ttU
	#[test]
	fn the_memo_never_hands_back_a_stale_halo() {
		let mut memo = HaloMemo::default();
		let light = themed("SilkTerm", "light");
		let first = memo.get(&light, 0.1, Some(ORDINARY), &SPREAD);
		assert_eq!(first, halo_match(&light, 0.1, Some(ORDINARY), &SPREAD));
		assert_eq!(memo.get(&light, 0.1, Some(ORDINARY), &SPREAD), first);
		let moved_slider = memo.get(&light, 0.35, Some(ORDINARY), &SPREAD);
		assert_ne!(moved_slider, first);
		assert_eq!(
			moved_slider,
			halo_match(&light, 0.35, Some(ORDINARY), &SPREAD)
		);
		let dimmer = [[0.002f32; 3], [0.01; 3], [0.03, 0.01, 0.02]];
		assert_eq!(
			memo.get(&light, 0.35, Some(ORDINARY), &dimmer),
			halo_match(&light, 0.35, Some(ORDINARY), &dimmer)
		);
		assert_eq!(
			memo.get(&themed("SilkTerm", "dark"), 0.35, Some(ORDINARY), &dimmer),
			None
		);
		let mut pastel = themed("Pastel", "light");
		pastel.wallpaper_even = 0.0;
		assert_eq!(
			memo.get(&pastel, 0.35, Some(ORDINARY), &dimmer),
			halo_match(&pastel, 0.35, Some(ORDINARY), &dimmer)
		);
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
	fn light_mode_tones_down_the_picture_that_stands_out_there_instead() {
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
				config::luma(crate::theme::resolve("SilkTerm", crate::theme::Mode::Dark, true).bg)
			) > 1.0
		);
		assert!(
			standout(
				BRIGHT,
				config::luma(crate::theme::resolve("SilkTerm", crate::theme::Mode::Light, true).bg)
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
		assert_eq!(
			halo_match(&themed("SilkTerm", "light"), 0.0, None, &SPREAD),
			None
		);
	}
}
