// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Color themes: each theme is a (dark, light) pair of `Palette`s. The active
//! theme name + mode (Dark / Light / System) resolve to one `Palette` - the
//! terminal bg/fg/cursor, the two attention colors and the 16 ANSI colors -
//! which `config` folds into `Settings` and `palette.rs` reads. The `colors.*`
//! keys still override on
//! top (a per-color tweak).
//!
//! A theme the user saves from the Settings dialog is a `UserTheme`: the same
//! (dark, light) pair, stored whole in `config.shcl` under `themes.<slug>` and
//! resolved ahead of the built-ins, so one may take a built-in's name and stand
//! in for it until it is deleted.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
	pub bg: [u8; 3],
	pub fg: [u8; 3],
	pub cursor: [u8; 3],

	// Two attention colors, deliberately separate. `highlight` marks several
	// things at once - the live pane's ring, slider handles, revert icons, the
	// default button - so it stays calm. `focus` marks the ONE element the
	// keyboard is on, so it is the more vivid of the pair and sits well away
	// from `highlight` in hue.
	pub highlight: [u8; 3],
	pub focus: [u8; 3],

	// Chrome: menu bar / dropdowns (menu_*) and pop-out dialogs (dialog_*). Every
	// built-in theme uses the SAME neutral defaults below (menu identical in both
	// modes, dialog lighter in Light mode) - a theme MAY override, and the
	// colors.menu_*/dialog_* keys tweak them per-user.
	pub menu_bg: [u8; 3],
	pub menu_fg: [u8; 3],
	pub dialog_bg: [u8; 3],
	pub dialog_fg: [u8; 3],
	/// Chrome areas that hold no interactive element - the strip the dialog's tabs
	/// sit on. Recessed against the panel in both modes.
	pub gutter: [u8; 3],
	pub ansi: [[u8; 3]; 16],
}

/// The ten palette colors a user can edit, spelled as `colors.*` spells them.
/// One order, used by the dialog's rows, by a saved theme's config block, and by
/// the index accessors below - so none of the three can drift from the others.
pub const PALETTE_KEYS: [&str; 10] = [
	"background",
	"foreground",
	"cursor",
	"highlight",
	"focus",
	"menu_background",
	"menu_foreground",
	"dialog_background",
	"dialog_foreground",
	"gutter",
];

impl Palette {
	pub fn get(&self, i: usize) -> [u8; 3] {
		match i {
			0 => self.bg,
			1 => self.fg,
			2 => self.cursor,
			3 => self.highlight,
			4 => self.focus,
			5 => self.menu_bg,
			6 => self.menu_fg,
			7 => self.dialog_bg,
			8 => self.dialog_fg,
			_ => self.gutter,
		}
	}
	pub fn set(&mut self, i: usize, color: [u8; 3]) {
		match i {
			0 => self.bg = color,
			1 => self.fg = color,
			2 => self.cursor = color,
			3 => self.highlight = color,
			4 => self.focus = color,
			5 => self.menu_bg = color,
			6 => self.menu_fg = color,
			7 => self.dialog_bg = color,
			8 => self.dialog_fg = color,
			_ => self.gutter = color,
		}
	}
}

/// A theme the user saved. It carries both variants in full rather than a base plus
/// the differences: saving, renaming and deleting are then all the same operation
/// on one config subtree, and a saved theme is self-contained enough to hand to
/// someone else. `slug` is its config path segment and never changes, so a rename
/// only rewrites `name` - and `name` is what the `theme` setting stores.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UserTheme {
	pub slug: String,
	pub name: String,
	pub dark: Palette,
	pub light: Palette,
}

// Shared chrome defaults (same for every theme). The menu keeps one neutral gray
// in both modes (unchanged look); the dialog panel is dark-gray / light-gray by mode.
pub const MENU_BG_DEF: [u8; 3] = [0x36, 0x36, 0x3b];
pub const MENU_FG_DEF: [u8; 3] = [0xf0, 0xf0, 0xf2];
const DLG_BG_DARK: [u8; 3] = [0x20, 0x20, 0x2a];
const DLG_FG_DARK: [u8; 3] = [0xe2, 0xe2, 0xea];
const DLG_BG_LIGHT: [u8; 3] = [0xe6, 0xe6, 0xe3];
const DLG_FG_LIGHT: [u8; 3] = [0x22, 0x24, 0x2c];
const GUTTER_DARK: [u8; 3] = [0x16, 0x16, 0x1e];
const GUTTER_LIGHT: [u8; 3] = [0xd3, 0xd3, 0xcf];

#[derive(Debug, Clone, Copy)]
pub struct Theme {
	pub dark: Palette,
	pub light: Palette,
}

// The project's original palette - now the default theme's dark variant.
#[rustfmt::skip]
const SILK_DARK: Palette = Palette {
	bg: [0x00, 0x00, 0x00],
	// The block cursor is a plate at pane::CURSOR_ALPHA under the glyph, and the
	// glyph keeps its own color, so the plate is a second background the text
	// has to clear the contrast floor on. A cursor at the fg's own brightness
	// mushes the two together. This is the fg's triadic partner dropped to the
	// brightness where the text on it clears the floor and it still shows
	// against the black bg (3.3:1). Every theme's cursor is held to that by a
	// test. The highlight stays warm: it marks the pane, not the caret, so it
	// wants its own identity rather than an echo of the cursor. Focus is its
	// azure complement - the one thing the keyboard is on.
	fg: [0x88, 0xee, 0xcc],
	cursor: [0x8a, 0x3f, 0xa4],
	highlight: [0xc8, 0xa0, 0x5a],
	focus: [0x40, 0x86, 0xff],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_DARK, dialog_fg: DLG_FG_DARK,
	gutter: GUTTER_DARK,
	// Hues sit where their names say, warmed toward the pair above; saturation is
	// the pastel end. Each color's BRIGHTNESS was carried over from the palette
	// this replaced, hue by hue, so contrast and legibility are unchanged - only
	// the family moved. The grays carry a faint warm cast for the same reason.
	ansi: [
		[0x1d, 0x1b, 0x18], [0xd0, 0x72, 0x64], [0x6c, 0xd0, 0x79], [0xd7, 0xc5, 0x7c],
		[0x7c, 0xa8, 0xe5], [0xbf, 0x7b, 0xd5], [0x53, 0xb9, 0xb5], [0xb7, 0xb1, 0xa5],
		[0x67, 0x61, 0x58], [0xe2, 0x90, 0x83], [0x8e, 0xe5, 0x99], [0xe5, 0xd6, 0x97],
		[0x9c, 0xc0, 0xf3], [0xd8, 0x9c, 0xec], [0x74, 0xd0, 0xcc], [0xeb, 0xe6, 0xdf],
	],
};

// A light theme's text is as dark as it is to make room for the cursor. The
// plate has to sit the contrast floor away from the text, so the paler the text
// the closer the plate crowds the background. At this depth, and at the light
// plate's stronger alpha, it stands about as far off the paper as a dark
// theme's plate stands off its background.
//
// Every light theme's ANSI colors sit 0.57 Oklab L under the paper on average,
// the normal row deeper than the bright one, where they used to average 0.36 to
// 0.47 and lean on the contrast floor. That is still short of a dark theme's
// 0.60 to 0.77, since much past it a yellow or a cyan has no color left to show.
// Chroma was kept or raised, so each reads as more saturated rather than as a
// muddier copy. The two monochrome themes moved their whole set down by one
// step instead, keeping the spread that is all they have to tell colors apart.
#[rustfmt::skip]
const SILK_LIGHT: Palette = Palette {
	bg: [0xf6, 0xf5, 0xf0],
	fg: [0x08, 0x0d, 0x23],
	cursor: [0x50, 0x75, 0xbc],
	highlight: [0x33, 0x66, 0xbb],
	focus: [0xb8, 0x6e, 0x00],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_LIGHT, dialog_fg: DLG_FG_LIGHT,
	gutter: GUTTER_LIGHT,
	ansi: [
		[0x32, 0x32, 0x3a], [0x7a, 0x06, 0x1a], [0x21, 0x4b, 0x04], [0x54, 0x3b, 0x04],
		[0x03, 0x3b, 0x85], [0x64, 0x06, 0x77], [0x05, 0x48, 0x52], [0x4a, 0x4d, 0x55],
		[0x5f, 0x63, 0x6d], [0x96, 0x09, 0x25], [0x2c, 0x5e, 0x06], [0x66, 0x4b, 0x06],
		[0x07, 0x4b, 0xa2], [0x78, 0x18, 0x8e], [0x07, 0x5a, 0x65], [0x20, 0x22, 0x28],
	],
};

// Matrix: monochrome green. Dark = bright green on near-black; light = dark green
// on a light gray. Being monochrome, the cursor is the same hue as the text at
// the brightness the text stays readable on (see SILK_DARK and SILK_LIGHT).
#[rustfmt::skip]
const MATRIX_DARK: Palette = Palette {
	bg: [0x00, 0x08, 0x02],
	fg: [0x33, 0xff, 0x66],
	cursor: [0x0a, 0x7a, 0x2a],
	highlight: [0x1f, 0xaa, 0x44],
	focus: [0xaa, 0xff, 0xcc],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_DARK, dialog_fg: DLG_FG_DARK,
	gutter: GUTTER_DARK,
	ansi: [
		[0x05, 0x18, 0x0a], [0x2a, 0xcc, 0x44], [0x33, 0xff, 0x66], [0x7a, 0xff, 0x8a],
		[0x1f, 0xaa, 0x3a], [0x44, 0xdd, 0x77], [0x55, 0xee, 0x88], [0x9a, 0xff, 0xaa],
		[0x1a, 0x55, 0x2a], [0x3a, 0xee, 0x55], [0x55, 0xff, 0x77], [0x99, 0xff, 0x99],
		[0x33, 0xcc, 0x55], [0x66, 0xff, 0x99], [0x77, 0xff, 0xaa], [0xcc, 0xff, 0xcc],
	],
};

#[rustfmt::skip]
const MATRIX_LIGHT: Palette = Palette {
	bg: [0xe9, 0xee, 0xe9],
	fg: [0x01, 0x1d, 0x02],
	cursor: [0x38, 0x89, 0x3f],
	highlight: [0x0a, 0x77, 0x2a],
	focus: [0x0a, 0x8f, 0x9a],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_LIGHT, dialog_fg: DLG_FG_LIGHT,
	gutter: GUTTER_LIGHT,
	ansi: [
		[0x14, 0x2a, 0x18], [0x04, 0x4b, 0x18], [0x02, 0x37, 0x10], [0x05, 0x53, 0x1d],
		[0x03, 0x42, 0x16], [0x05, 0x4d, 0x1d], [0x04, 0x48, 0x1a], [0x11, 0x37, 0x1d],
		[0x20, 0x3f, 0x26], [0x06, 0x5b, 0x1e], [0x04, 0x47, 0x16], [0x08, 0x63, 0x25],
		[0x05, 0x52, 0x1d], [0x07, 0x5c, 0x25], [0x06, 0x57, 0x21], [0x10, 0x30, 0x18],
	],
};

// Retro amber: monochrome amber/orange. Dark = amber on near-black; light = dark
// amber on a warm light gray. The cursors follow the same rule as Matrix's.
#[rustfmt::skip]
const AMBER_DARK: Palette = Palette {
	bg: [0x10, 0x0a, 0x00],
	fg: [0xff, 0xb0, 0x00],
	cursor: [0x7a, 0x3a, 0x00],
	highlight: [0xcc, 0x80, 0x00],
	focus: [0xff, 0x40, 0x20],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_DARK, dialog_fg: DLG_FG_DARK,
	gutter: GUTTER_DARK,
	ansi: [
		[0x2a, 0x1c, 0x06], [0xff, 0x8c, 0x1a], [0xff, 0xb0, 0x00], [0xff, 0xc8, 0x4a],
		[0xd0, 0x86, 0x10], [0xff, 0xa0, 0x33], [0xff, 0xc0, 0x55], [0xff, 0xd8, 0x9a],
		[0x6a, 0x46, 0x10], [0xff, 0x9a, 0x33], [0xff, 0xbe, 0x33], [0xff, 0xd4, 0x77],
		[0xe0, 0x96, 0x22], [0xff, 0xb0, 0x55], [0xff, 0xcc, 0x77], [0xff, 0xe8, 0xc0],
	],
};

#[rustfmt::skip]
const AMBER_LIGHT: Palette = Palette {
	bg: [0xf2, 0xee, 0xe6],
	fg: [0x28, 0x0d, 0x01],
	cursor: [0xad, 0x6a, 0x2e],
	highlight: [0x9a, 0x52, 0x00],
	focus: [0xc8, 0x10, 0x2e],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_LIGHT, dialog_fg: DLG_FG_LIGHT,
	gutter: GUTTER_LIGHT,
	ansi: [
		[0x33, 0x24, 0x10], [0x6d, 0x33, 0x04], [0x4b, 0x27, 0x02], [0x62, 0x3b, 0x04],
		[0x56, 0x2b, 0x03], [0x66, 0x35, 0x04], [0x52, 0x2d, 0x03], [0x31, 0x20, 0x0b],
		[0x3e, 0x29, 0x10], [0x7d, 0x3c, 0x06], [0x5a, 0x30, 0x03], [0x72, 0x46, 0x06],
		[0x62, 0x34, 0x04], [0x76, 0x3e, 0x05], [0x5f, 0x35, 0x04], [0x28, 0x1c, 0x0c],
	],
};

// Pastel: soft light pastels on a dark gray that carries a faint tint of the
// foreground's complement, so the ground reads as cool against the warm cream
// text instead of as flat charcoal. The light variant turns it round - the same
// hues deepened, on cream paper. The cursors follow the same rule as SilkTerm's.
#[rustfmt::skip]
const PASTEL_DARK: Palette = Palette {
	bg: [0x1d, 0x20, 0x28],
	fg: [0xec, 0xdf, 0xc4],
	cursor: [0x55, 0x66, 0xa8],
	highlight: [0xc9, 0x8f, 0xa4],
	focus: [0x59, 0xd9, 0xc0],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_DARK, dialog_fg: DLG_FG_DARK,
	gutter: GUTTER_DARK,
	// Every hue at the same low saturation, so no one color jumps out of the set.
	// The bright row is the same hue a step lighter rather than a step more vivid.
	ansi: [
		[0x2a, 0x2d, 0x36], [0xe8, 0xa0, 0xa8], [0xa8, 0xd8, 0xa0], [0xe6, 0xd2, 0x9a],
		[0xa0, 0xbc, 0xe8], [0xd0, 0xaa, 0xe4], [0x96, 0xd6, 0xd2], [0xd6, 0xd2, 0xc8],
		[0x5c, 0x60, 0x70], [0xf2, 0xb8, 0xbf], [0xc0, 0xe6, 0xb8], [0xf2, 0xe2, 0xb4],
		[0xb8, 0xd0, 0xf2], [0xe0, 0xc2, 0xf0], [0xb0, 0xe6, 0xe2], [0xf2, 0xee, 0xe4],
	],
};

#[rustfmt::skip]
const PASTEL_LIGHT: Palette = Palette {
	bg: [0xf2, 0xf0, 0xe9],
	fg: [0x11, 0x13, 0x1f],
	cursor: [0x78, 0x7e, 0xa4],
	highlight: [0xa8, 0x60, 0x7a],
	focus: [0x0f, 0x8f, 0x8a],
	menu_bg: MENU_BG_DEF, menu_fg: MENU_FG_DEF,
	dialog_bg: DLG_BG_LIGHT, dialog_fg: DLG_FG_LIGHT,
	gutter: GUTTER_LIGHT,
	ansi: [
		[0x3a, 0x3d, 0x48], [0x6c, 0x15, 0x2e], [0x06, 0x49, 0x21], [0x4d, 0x38, 0x03],
		[0x1b, 0x37, 0x78], [0x53, 0x1f, 0x6a], [0x04, 0x45, 0x43], [0x47, 0x49, 0x50],
		[0x5c, 0x5e, 0x69], [0x7e, 0x29, 0x3e], [0x1d, 0x59, 0x31], [0x60, 0x47, 0x05],
		[0x2c, 0x48, 0x89], [0x64, 0x31, 0x7b], [0x07, 0x57, 0x55], [0x2a, 0x2c, 0x34],
	],
};

#[rustfmt::skip]
pub const THEMES: &[(&str, Theme)] = &[
	("SilkTerm", Theme { dark: SILK_DARK, light: SILK_LIGHT }),
	("Matrix", Theme { dark: MATRIX_DARK, light: MATRIX_LIGHT }),
	("Retro Amber", Theme { dark: AMBER_DARK, light: AMBER_LIGHT }),
	("Pastel", Theme { dark: PASTEL_DARK, light: PASTEL_LIGHT }),
];

pub fn names() -> impl Iterator<Item = &'static str> {
	THEMES.iter().map(|(n, _)| *n)
}

pub fn is_builtin(name: &str) -> bool {
	names().any(|n| n.eq_ignore_ascii_case(name.trim()))
}

/// Every selectable theme name, saved ones first so a saved theme that took a
/// built-in's name appears once, as itself.
pub fn all_names(user: &[UserTheme]) -> Vec<String> {
	let mut out: Vec<String> = user.iter().map(|t| t.name.clone()).collect();
	for name in names() {
		if !out.iter().any(|n| n.eq_ignore_ascii_case(name)) {
			out.push(name.to_string());
		}
	}
	out
}

pub fn find_user<'a>(user: &'a [UserTheme], name: &str) -> Option<&'a UserTheme> {
	user.iter()
		.find(|t| t.name.eq_ignore_ascii_case(name.trim()))
}

/// Which variant of the theme to use, `theme_mode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
	Dark,
	Light,
	System,
}

impl crate::config::Choice for Mode {
	const ALL: &'static [Self] = &[Self::Dark, Self::Light, Self::System];

	fn key(self) -> &'static str {
		match self {
			Self::Dark => "dark",
			Self::Light => "light",
			Self::System => "system",
		}
	}
}

impl Mode {
	/// Does this mode resolve to the dark variant? System follows the OS.
	pub fn is_dark(self, system_dark: bool) -> bool {
		match self {
			Self::Dark => true,
			Self::Light => false,
			Self::System => system_dark,
		}
	}
}

/// Resolve the active palette from a theme name + mode. A saved theme wins over a
/// built-in of the same name; an unknown name falls back to the first built-in.
pub fn resolve_in(user: &[UserTheme], name: &str, mode: Mode, system_dark: bool) -> Palette {
	let dark = mode.is_dark(system_dark);
	if let Some(t) = find_user(user, name) {
		return if dark { t.dark } else { t.light };
	}
	let theme = THEMES
		.iter()
		.find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
		.map_or(&THEMES[0].1, |(_, t)| t);
	if dark { theme.dark } else { theme.light }
}

/// Built-ins only - for paths that have no user themes to hand (and the tests).
pub fn resolve(name: &str, mode: Mode, system_dark: bool) -> Palette {
	resolve_in(&[], name, mode, system_dark)
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: EiMPv8L
	#[test]
	fn resolve_picks_theme_and_mode() {
		use crate::config::Choice;
		// unknown name falls back to the first theme (SilkTerm)
		assert_eq!(resolve("nope", Mode::Dark, true).bg, THEMES[0].1.dark.bg);
		// mode selects the variant; "system" honors system_dark
		assert_eq!(
			resolve("Matrix", Mode::Light, true).bg,
			find("Matrix").light.bg
		);
		assert_eq!(
			resolve("Matrix", Mode::System, true).bg,
			find("Matrix").dark.bg
		);
		assert_eq!(
			resolve("Matrix", Mode::System, false).bg,
			find("Matrix").light.bg
		);
		// case/space tolerant
		let dark = Mode::parse("DARK").unwrap();
		assert_eq!(resolve(" matrix ", dark, true).fg, find("Matrix").dark.fg);
	}

	// Test ID: EiuvVez
	#[test]
	fn chrome_defaults_shared_across_themes() {
		// every built-in theme uses the same neutral menu colors (both modes)
		for (_, t) in THEMES {
			assert_eq!(t.dark.menu_bg, MENU_BG_DEF);
			assert_eq!(t.light.menu_bg, MENU_BG_DEF);
			assert_eq!(t.dark.menu_fg, MENU_FG_DEF);
			// the dialog panel is darker in dark mode than in light mode
			assert!(t.dark.dialog_bg[0] < t.light.dialog_bg[0]);
			// the gutter is recessed against the panel it sits on, both ways round
			assert!(t.dark.gutter[0] < t.dark.dialog_bg[0]);
			assert!(t.light.gutter[0] < t.light.dialog_bg[0]);
		}
	}

	// The pair only works if the two read as different signals. A theme that let
	// them converge would draw the focused control and everything merely
	// highlighted in the same color, which is the whole point of splitting them.
	// Test ID: Em3Pif6
	#[test]
	fn the_two_attention_colours_stay_apart() {
		for (name, t) in THEMES {
			for pal in [t.dark, t.light] {
				let apart: i32 = (0..3)
					.map(|k| (i32::from(pal.highlight[k]) - i32::from(pal.focus[k])).abs())
					.sum();
				assert!(apart >= 120, "{name}: highlight and focus are too close");
			}
		}
	}

	// Body text has to clear the minimum-contrast floor on its own - if a theme's
	// own foreground needed lifting, the floor would be repainting the thing it is
	// measured against. ANSI black is the other end of the same check: it is
	// invisible on a dark ground by definition, which is the case the floor is for.
	// Test ID: EoTQgug
	#[test]
	fn a_theme_fg_clears_the_floor_and_ansi_black_does_not() {
		let floor = crate::config::Settings::default().text_min_contrast;
		for (name, t) in THEMES {
			for pal in [t.dark, t.light] {
				assert_eq!(
					crate::palette::readable(pal.fg, pal.bg, floor),
					pal.fg,
					"{name}: the theme's own fg would be repainted"
				);
			}
			assert_ne!(
				crate::palette::readable(t.dark.ansi[0], t.dark.bg, floor),
				t.dark.ansi[0],
				"{name}: ansi black on a dark ground should be lifted"
			);
		}
	}

	// A flyover tip lifts its box off the menu background, which moves it toward
	// the menu's own text. The gap is still wide at the shipped colors, but a
	// theme that overrode the menu pair could land somewhere the lift makes
	// unreadable, and nothing repaints chrome at run time.
	// Test ID: EqQPotH
	#[test]
	fn tip_text_clears_the_floor_on_its_own_box() {
		let floor = crate::config::Settings::default().text_min_contrast;
		for (name, t) in THEMES {
			for (mode, pal) in [("dark", t.dark), ("light", t.light)] {
				let bg = crate::config::tip_bg_of(pal.menu_bg);
				let fg = crate::config::tip_fg_of(pal.menu_fg);
				assert_eq!(
					crate::palette::readable(fg, bg, floor),
					fg,
					"{name} {mode}: a tip's text would be repainted on its own box"
				);
			}
		}
	}

	// The plate a theme's cursor draws over its background, at the alpha the
	// renderer picks for that palette.
	fn cursor_plate(pal: &Palette, floor: f32) -> [u8; 3] {
		let alpha = crate::pane::cursor_alpha(pal.fg, pal.bg, pal.cursor, floor);
		crate::pane::cursor_plate(pal.cursor, pal.bg, alpha)
	}

	// The block cursor is a plate under the glyph, and the glyph keeps its own
	// color. So the plate is a second background the text has to clear the floor
	// on, and a cursor at the fg's own brightness fails it.
	// Test ID: Eq9PYAL
	#[test]
	fn text_on_the_cursor_plate_clears_the_floor() {
		let floor = crate::config::Settings::default().text_min_contrast;
		for (name, t) in THEMES {
			for (mode, pal) in [("dark", t.dark), ("light", t.light)] {
				let plate = cursor_plate(&pal, floor);
				assert_eq!(
					crate::palette::readable(pal.fg, plate, floor),
					pal.fg,
					"{name} {mode}: text on the cursor would be repainted"
				);
			}
		}
	}

	// A light theme's plate used to sit 0.12 to 0.20 Oklab L off its background,
	// against 0.20 to 0.42 in dark mode, because pale text left it no room. The
	// light text is darker now and the plate stronger, and this holds the room.
	// Test ID: ErJIAXF
	#[test]
	fn a_light_cursor_plate_stands_off_the_background_like_a_dark_one() {
		let floor = crate::config::Settings::default().text_min_contrast;
		let lightness = |c: [u8; 3]| crate::palette::to_oklab(c).0;
		for (name, t) in THEMES {
			let pal = t.light;
			let gap = lightness(pal.bg) - lightness(cursor_plate(&pal, floor));
			assert!(
				gap >= 0.24,
				"{name} light: the cursor plate is only {gap:.3} off the background"
			);
		}
	}

	// Light-mode colored text used to sit close enough to the paper that the
	// contrast floor held most of it up, and darkening a color alone turns it
	// into a dark gray. So each colored slot has to stand well off the paper and
	// keep enough chroma, for its lightness, to still read as its color.
	// Test ID: Ers4tYC
	#[test]
	fn light_text_is_dark_and_still_a_color() {
		let lab = crate::palette::to_oklab;
		for (name, t) in THEMES {
			let pal = t.light;
			let paper = lab(pal.bg).0;
			let gap = paper - lab(pal.fg).0;
			assert!(
				gap >= 0.74,
				"{name}: the text sits only {gap:.3} off the paper"
			);
			let mut sum = 0.0;
			for i in (1..=6).chain(9..=14) {
				let (l, a, b) = lab(pal.ansi[i]);
				assert!(
					paper - l >= 0.50,
					"{name} ansi {i} sits only {:.3} off the paper",
					paper - l
				);
				assert!(a.hypot(b) / l >= 0.15, "{name} ansi {i} has gone gray");
				sum += paper - l;
			}
			assert!(
				sum / 12.0 >= 0.55,
				"{name}: the colors average {:.3} off the paper",
				sum / 12.0
			);
		}
	}

	fn luma(c: [u8; 3]) -> f32 {
		0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])
	}

	// The three the project started with. Matrix and Retro Amber are monochrome,
	// and a color that wandered off the hue would break that in either mode.
	// Test ID: Er2UJeJ
	#[test]
	fn the_first_three_built_ins_keep_their_names_and_hues() {
		let first: Vec<&str> = names().take(3).collect();
		assert_eq!(first, ["SilkTerm", "Matrix", "Retro Amber"]);
		for pal in [find("Matrix").dark, find("Matrix").light] {
			for c in std::iter::once(pal.fg).chain(pal.ansi) {
				assert!(c[1] > c[0] && c[1] > c[2], "Matrix {c:02x?} is not green");
			}
		}
		for pal in [find("Retro Amber").dark, find("Retro Amber").light] {
			for c in std::iter::once(pal.fg).chain(pal.ansi) {
				assert!(
					c[0] > c[1] && c[1] > c[2],
					"Retro Amber {c:02x?} is not amber"
				);
			}
		}
	}

	// Dark mode is light on dark for the terminal and the dialogs alike, light
	// mode the reverse, and the dialog panel never matches the terminal behind it.
	// Test ID: Er2UJeK
	#[test]
	fn dark_mode_is_light_on_dark_and_light_mode_the_reverse() {
		for (name, t) in THEMES {
			let (d, l) = (t.dark, t.light);
			assert!(luma(d.bg) < luma(d.fg), "{name} dark text");
			assert!(luma(l.bg) > luma(l.fg), "{name} light text");
			assert!(luma(d.dialog_bg) < luma(d.dialog_fg), "{name} dark dialog");
			assert!(luma(l.dialog_bg) > luma(l.dialog_fg), "{name} light dialog");
			assert_ne!(d.dialog_bg, d.bg, "{name} dark dialog shade");
			assert_ne!(l.dialog_bg, l.bg, "{name} light dialog shade");
		}
	}

	fn find(name: &str) -> &'static Theme {
		THEMES
			.iter()
			.find(|(n, _)| *n == name)
			.map(|(_, t)| t)
			.unwrap()
	}
}
