// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Modal settings dialog: sliders for numeric tunables, swatch + hex field for
//! colors, toggles, few-option radios, dropdown list boxes for longer enums, and
//! Cancel / Apply / OK. Edits a working copy of `Settings`; the app reads it back
//! on Apply/OK to live-apply + persist. Renders as flat quads (rects) + positioned
//! text; an open dropdown's popup draws in a second (`LoadOp::Load`) pass on top so
//! covered rows' text can't bleed through it (see `dropdown_overlay`).
//!
//! Sections are grouped into tabs (see `tab_titles()`/`tab_for_section`) so the
//! dialog stays well under screen height; if a tab still doesn't fit (huge UI
//! font / short screen) the rows region scrolls (wheel + draggable thumb) and
//! the window height is capped instead of clipping the buttons.
//!
//! Units: every measurement below is a DIP - a CSS pixel, i.e. 1/96 inch - and
//! the whole layout is solved in that space. The window's scale factor is
//! applied only at the boundary: pointer positions, measured text widths, the
//! UI line height and the height cap divide down on the way in; the window
//! size, the scissor viewport, quads and text positions multiply back out on
//! the way to the renderer. So the dialog keeps its proportions at any DPI
//! rather than shrinking as the scale factor grows, and there is exactly one
//! set of numbers to reason about. At scale 1 nothing changes.

use crate::config::{self, Choice, Settings};
use crate::fileassoc::Assoc;
use crate::gfx::{QuadMode, RectInstance};
use crate::input::Hotkey;
use crate::keys::Chord;
use crate::pane::Rect;
use crate::pick::{self, Picker};
use crate::profile::Profile;
use crate::textedit::{Reach, caret_from_click, reach_left, reach_right, word_at};
use crate::ui_spec::{self, Key, Kind, Layout, Spec, ui};
use prompt::{Prompt, PromptFocus, PromptJob};
use shell_grid::{ShellDrag, ShellPart, ShellStop, shell_stop};
use std::borrow::Cow;
use themes::ThemeBtn;
use winit::keyboard::ModifiersState;

mod picker;
mod prompt;
mod shell_grid;
mod themes;

// The declared geometry, all of it in DIP (see the units note above).
fn lay() -> &'static Layout {
	&ui().layout
}
pub fn tab_titles() -> &'static [&'static str] {
	&ui().tabs
}

// Dialog colors adapt to the active mode (dark-gray for dark, light-gray for
// light); see config::is_dark(). The menu/main-window chrome stays a fixed gray.
struct Dlg {
	panel_bg: [u8; 3],
	panel_border: [u8; 3],
	gutter: [u8; 3], // the strip the tabs stand on
	tab_bg: [u8; 3], // a tab that is not the current one
	tab_hl: [u8; 3], // the current tab: a lighter gray, deliberately not an accent
	track: [u8; 3],
	handle: [u8; 3],
	field_bg: [u8; 3],
	focus_out: [u8; 3],
	btn_bg: [u8; 3],
	btn_hl: [u8; 3],
	text: [u8; 3],
	dim: [u8; 3],
	// The dialog's one destructive control (the shells grid's remove). Chrome,
	// not theme-derived: "this deletes something" is a fixed meaning, and a
	// theme whose accent happened to be red would say it about everything.
	danger: [u8; 3],
}
#[rustfmt::skip]
const DARK_DLG: Dlg = Dlg {
	panel_bg: [0x20, 0x20, 0x2a], panel_border: [0x50, 0x50, 0x60],
	gutter: [0x16, 0x16, 0x1e],
	tab_bg: [0x28, 0x28, 0x32], tab_hl: [0x40, 0x40, 0x4c],
	track: [0x14, 0x14, 0x1c], handle: [0x7a, 0x9a, 0xd0],
	field_bg: [0x14, 0x14, 0x1c], focus_out: [0x7a, 0x9a, 0xd0],
	btn_bg: [0x34, 0x34, 0x40], btn_hl: [0x4a, 0x6a, 0x9a],
	text: [0xe2, 0xe2, 0xea], dim: [0x9a, 0x9a, 0xa6],
	danger: [0xe2, 0x6a, 0x6a],
};
#[rustfmt::skip]
const LIGHT_DLG: Dlg = Dlg {
	panel_bg: [0xe6, 0xe6, 0xe3], panel_border: [0xb2, 0xb2, 0xb6],
	gutter: [0xd3, 0xd3, 0xcf],
	tab_bg: [0xdd, 0xdd, 0xd9], tab_hl: [0xf4, 0xf4, 0xf1],
	track: [0xcf, 0xcf, 0xcc], handle: [0x4a, 0x6a, 0xa8],
	field_bg: [0xf8, 0xf8, 0xf6], focus_out: [0x3a, 0x6a, 0xc0],
	btn_bg: [0xd6, 0xd6, 0xd2], btn_hl: [0x9a, 0xb6, 0xe0],
	text: [0x22, 0x24, 0x2c], dim: [0x70, 0x70, 0x76],
	danger: [0xb8, 0x2c, 0x2c],
};
// sRGB-space blend of two colors (selection highlight = field bg toward accent)
fn mix3(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
	let mut out = [0u8; 3];
	for k in 0..3 {
		out[k] = (a[k] as f32 + (b[k] as f32 - a[k] as f32) * t).round() as u8;
	}
	out
}
// The user's own values: a live copy wears the performance profile, and the
// text and cursor it shows may be the wallpaper's rather than the user's -
// which would otherwise be what a saved theme stored.
fn users_own(mut settings: Settings) -> Settings {
	crate::profile::unapply(&mut settings);
	crate::autotheme::unapply(&mut settings);
	settings
}

// Builds of the dialog colors on this thread. Test builds only.
#[cfg(test)]
thread_local! {
	static DLG_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Hover tip lookups on this thread. Test builds only.
#[cfg(test)]
thread_local! {
	static HOVER_TIPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub fn hover_tips() -> usize {
	HOVER_TIPS.with(std::cell::Cell::get)
}

// The dialog color set for the active mode, with the panel background + text
// overridden by the configured dialog colors (theme default or a colors
// dialog_*/menu_* override). The remaining shades (border/track/handle/fields/
// buttons) stay from the mode preset so contrast holds.
fn dlg() -> Dlg {
	use config::auto::Setting;
	#[cfg(test)]
	DLG_BUILDS.with(|n| n.set(n.get() + 1));
	let base = if config::is_dark() {
		DARK_DLG
	} else {
		LIGHT_DLG
	};
	let settings = config::settings();
	// The two attention colors are themable and mean different things (see
	// theme.rs). `highlight` paints everything that calls attention at once -
	// slider handles, the scrollbar, revert arrows, the default button - and the
	// button fill is a dimmed version of it, mixed toward the panel so a pressed
	// button reads as pressed rather than as the focused one. `focus` paints only
	// the ring around whatever the keyboard is on.
	let color = |setting| config::auto::color(&settings, setting);
	Dlg {
		panel_bg: color(Setting::DialogBackground),
		text: color(Setting::DialogForeground),
		gutter: color(Setting::Gutter),
		handle: color(Setting::Highlight),
		btn_hl: mix3(
			color(Setting::DialogBackground),
			color(Setting::Highlight),
			0.62,
		),
		focus_out: color(Setting::Focus),
		..base
	}
}

/// Mode-adaptive dialog colors for the pop-out window (clear + About text).
pub fn dialog_bg() -> [u8; 3] {
	dlg().panel_bg
}
pub fn dialog_text() -> [u8; 3] {
	dlg().text
}
pub fn dialog_dim() -> [u8; 3] {
	dlg().dim
}
pub fn dialog_btn() -> [u8; 3] {
	dlg().btn_bg
}
pub fn dialog_btn_hl() -> [u8; 3] {
	dlg().btn_hl
}
pub fn dialog_border() -> [u8; 3] {
	dlg().panel_border
}

// The Scrolling tab's time constants are all shown as a friendly 1..100 but
// stored as milliseconds. Logarithmic: a time constant is felt by ratio, and a
// linear map wastes most of the travel on values that look identical (the old
// 300ms floor was why the slow end read as a no-op - barely below the 230ms
// default). Each range spans two decades around the value its segment was
// first tuned to, so a default anywhere in it still leaves room to
// move either way.
fn log_pos(v: f32, min: f32, max: f32) -> f32 {
	(v.clamp(min, max) / min).ln() / (max / min).ln()
}
fn log_val(pos: f32, min: f32, max: f32) -> f32 {
	min * (max / min).powf(pos.clamp(0.0, 1.0))
}
// Every feel slider falls: a HIGHER number means a SMALLER stored value,
// because each is stored as a time (per-line tau, doubling/halving period,
// ease duration) and a shorter time is a faster/harder/crisper feel. One
// direction across the whole tab - higher = faster.
fn falling_slider(v: f32, min: f32, max: f32) -> f32 {
	(100.0 - log_pos(v, min, max) * 99.0).round()
}
fn falling_value(slider: f32, min: f32, max: f32) -> f32 {
	log_val((100.0 - slider.clamp(1.0, 100.0)) / 99.0, min, max)
}
// 1 = one line/s, 100 = a hundred lines/s.
const TAU_MIN: f32 = 10.0;
const TAU_MAX: f32 = 1000.0;
// A fraction the config stores as 0..1 reads as a whole percent in the dialog:
// nobody thinks in 0.35. Only the display moves - the file keeps the decimal,
// so the two are the same transform in opposite directions and every default
// comparison happens on the same side of it.
fn to_percent(fraction: f32) -> f32 {
	fraction * 100.0
}
fn from_percent(percent: f32) -> f32 {
	percent / 100.0
}

fn tau_to_speed(tau: f32) -> f32 {
	falling_slider(tau, TAU_MIN, TAU_MAX)
}
fn speed_to_tau(speed: f32) -> f32 {
	falling_value(speed, TAU_MIN, TAU_MAX)
}
// Leave-from-rest duration: 8ms is instant, 800ms a long slow lift.
const EASE_IN_MIN: f32 = 8.0;
const EASE_IN_MAX: f32 = 800.0;
// Chase doubling period: 30ms is a near-instant ramp, 3s barely ramps at all.
const RAMP_UP_MIN: f32 = 30.0;
const RAMP_UP_MAX: f32 = 3000.0;
// Wind-down halving period, the same two-decade span as the ramp up.
const RAMP_DOWN_MIN: f32 = 45.0;
const RAMP_DOWN_MAX: f32 = 4500.0;
// Tail duration: 13ms is an abrupt stop, 1.3s a long float-in.
const EASE_OUT_MIN: f32 = 13.0;
const EASE_OUT_MAX: f32 = 1300.0;

// The keys the per-kind accessors below match on, as one pattern per list of
// kinds: `keys_of!(toggle | radio)`. Each accessor names its own kind's keys
// one by one and the rest through this, so a key added to `ui_spec` or left
// out of an accessor fails to compile instead of falling to a catch-all.
// `every_row_kind_matches_its_key_list` holds the lists against the rows.
// Built up in one pass rather than one macro per kind, which clippy reads as
// nested or-patterns.
macro_rules! keys_of {
	($($kind:ident)|+) => {
		keys_of!(@ [] $($kind)+)
	};
	(@ [$($acc:tt)*]) => {
		$($acc)*
	};
	(@ [$($acc:tt)*] slider $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::Opacity
			| Key::BgOpacity
			| Key::BgBlur
			| Key::BgContrastSize
			| Key::BgContrastStrength
			| Key::BgContrastAuto
			| Key::ScrimRadius
			| Key::ScrimSoftness
			| Key::ScrimStrength
			| Key::Outline
			| Key::MinContrast
			| Key::CursorBlinkRate
			| Key::CursorHeight
			| Key::CursorWidth
			| Key::CursorResume
			| Key::FontSize
			| Key::LineHeight
			| Key::Margin
			| Key::TabRegularWidth
			| Key::TabMaxWidth
			| Key::ScrollEaseIn
			| Key::ScrollRampUp
			| Key::SingleScreenTau
			| Key::ScrollRampDown
			| Key::ScrollEaseOut
			| Key::WheelLines
			| Key::ScrollbarThickness
			| Key::MinimapWidth
			| Key::Columns
			| Key::Rows
			| Key::IdleHiddenMin
			| Key::IdleMin
		] $($rest)*)
	};
	(@ [$($acc:tt)*] toggle $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::PerfAuto
			| Key::PerfCheckHardware
			| Key::PerfCheckNext
			| Key::Transparency
			| Key::BackdropBlur
			| Key::TextScrim
			| Key::CursorScrim
			| Key::CursorOutline
			| Key::CursorBlinking
			| Key::RememberSize
			| Key::RememberPerMonitor
			| Key::RememberMaximized
			| Key::NewTabNextToCurrent
			| Key::TabShowsTitle
			| Key::TabShowsShell
			| Key::TabShowsProgram
			| Key::TabShowsDirectory
			| Key::TitleShowsTab
			| Key::IdleRelease
			| Key::SoftwareRendering
			| Key::CopyOnSelect
			| Key::ShellIntegration
			| Key::BashPrompt
			| Key::Hyperlinks
			| Key::BgContrastMask
			| Key::BgEnabled
			| Key::BgRotate
			| Key::BgHonorXmp
			| Key::BgHonorXmpLook
			| Key::ColFromWallpaper
			| Key::SmoothScroll
			| Key::Scrollbar
			| Key::ScrollbarAutoHide
			| Key::Minimap
		] $($rest)*)
	};
	// radio buttons and dropdowns both
	(@ [$($acc:tt)*] radio $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::PerfProfile
			| Key::BgFit
			| Key::ScrimFunction
			| Key::ScrimRamp
			| Key::CursorAnimation
			| Key::Theme
			| Key::ThemeMode
		] $($rest)*)
	};
	(@ [$($acc:tt)*] color $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::ColBg
			| Key::ColFg
			| Key::ColCursor
			| Key::ColHighlight
			| Key::ColFocus
			| Key::ColGutter
			| Key::ColMenuBg
			| Key::ColMenuFg
			| Key::ColDialogBg
			| Key::ColDialogFg
			| Key::ColScrollbarThumb
			| Key::ColScrollbarTrough
		] $($rest)*)
	};
	(@ [$($acc:tt)*] text $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::BgImage
			| Key::FontFamily
			| Key::LinkOpenCommand
			| Key::StartupDirectory
		] $($rest)*)
	};
	// read and written through `Bindings`
	(@ [$($acc:tt)*] hotkey $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::HotkeyNewWindow
			| Key::HotkeySettings
			| Key::HotkeyQuit
			| Key::HotkeyCopy
			| Key::HotkeyPaste
			| Key::HotkeyFontBigger
			| Key::HotkeyFontSmaller
			| Key::HotkeyFontReset
			| Key::HotkeyFullscreen
			| Key::HotkeyContextMenu
			| Key::HotkeyNewTab
			| Key::HotkeyCloseTab
			| Key::HotkeyPrevTab
			| Key::HotkeyNextTab
			| Key::HotkeyMoveTabBack
			| Key::HotkeyMoveTabForward
			| Key::HotkeySplitRight
			| Key::HotkeySplitDown
			| Key::HotkeyClosePane
			| Key::HotkeyFocusLeft
			| Key::HotkeyFocusRight
			| Key::HotkeyFocusUp
			| Key::HotkeyFocusDown
		] $($rest)*)
	};
	// headings and two-setting rows (`None`), the theme buttons, and the shells
	// list, which is a list rather than one value
	(@ [$($acc:tt)*] valueless $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::None
			| Key::Shells
			| Key::ThemeActions
		] $($rest)*)
	};
	// the file-type rows, whose arrow undoes a registration
	(@ [$($acc:tt)*] assoc $($rest:ident)*) => {
		keys_of!(@ [$($acc)*
			| Key::OpenBatch
			| Key::OpenPowerShell
			| Key::OpenVbScript
			| Key::OpenFolder
		] $($rest)*)
	};
}

/// A slider's value in the dialog's own units, read from `settings`. One list for the
/// shown value and the default, so the two cannot disagree on a transform.
pub(crate) fn slider_of(settings: &Settings, key: Key) -> f32 {
	match key {
		Key::Opacity => to_percent(settings.opacity),
		Key::BgOpacity => to_percent(settings.wallpaper_opacity),
		Key::BgBlur => settings.wallpaper_blur,
		Key::BgContrastSize => to_percent(settings.wallpaper_contrast_mask_size),
		Key::BgContrastStrength => to_percent(settings.wallpaper_contrast_mask_strength),
		Key::BgContrastAuto => to_percent(settings.wallpaper_contrast_mask_auto),
		Key::ScrimRadius => settings.text_scrim_radius,
		Key::ScrimSoftness => to_percent(settings.text_scrim_softness),
		Key::ScrimStrength => settings.text_scrim_strength,
		Key::Outline => settings.text_outline,
		Key::MinContrast => to_percent(settings.text_min_contrast),
		Key::CursorBlinkRate => settings.cursor_blink_rate_s,
		Key::CursorHeight => settings.cursor_size_height,
		Key::CursorWidth => settings.cursor_size_width,
		Key::CursorResume => settings.cursor_animation_resume_s,
		Key::FontSize => config::auto::font_size(settings),
		Key::LineHeight => settings.line_height_scale,
		Key::Margin => settings.margin,
		Key::TabRegularWidth => settings.tab_regular_pct,
		Key::TabMaxWidth => settings.tab_max_pct,
		// shown as an intuitive 1..100 speed (higher = faster); stored as tau
		Key::ScrollEaseIn => falling_slider(settings.scroll_ease_in_ms, EASE_IN_MIN, EASE_IN_MAX),
		Key::ScrollRampUp => falling_slider(settings.scroll_ramp_up_ms, RAMP_UP_MIN, RAMP_UP_MAX),
		Key::SingleScreenTau => tau_to_speed(settings.scroll_single_screen_tau_ms),
		Key::ScrollRampDown => {
			falling_slider(settings.scroll_ramp_down_ms, RAMP_DOWN_MIN, RAMP_DOWN_MAX)
		}
		Key::ScrollEaseOut => {
			falling_slider(settings.scroll_ease_out_ms, EASE_OUT_MIN, EASE_OUT_MAX)
		}
		Key::WheelLines => settings.wheel_lines,
		Key::ScrollbarThickness => settings.scrollbar_thickness,
		Key::MinimapWidth => settings.minimap_width,
		Key::Columns => config::auto::grid(settings, None).0 as f32,
		Key::Rows => config::auto::grid(settings, None).1 as f32,
		Key::IdleHiddenMin => settings.idle_release_hidden_min as f32,
		Key::IdleMin => settings.idle_release_min as f32,
		keys_of!(toggle | radio | color | text | hotkey | valueless | assoc) => 0.0,
	}
}
// Why "Minutes when hidden" is grayed on a desktop that never says so.
const fn hidden_wait_tip(sees_hidden: bool) -> Option<&'static str> {
	if sees_hidden {
		None
	} else {
		Some("Wayland never says when a window is hidden, so Minutes otherwise is used.")
	}
}

// A switch's state in `settings`, for the shown value, the default and the revert.
fn toggle_of(settings: &Settings, key: Key) -> bool {
	match key {
		Key::PerfAuto => settings.performance_automatic,
		Key::PerfCheckHardware => settings.performance_check_hardware,
		Key::PerfCheckNext => settings.performance_check_next_run,
		Key::Transparency => settings.transparent_background,
		Key::BackdropBlur => settings.transparent_background_blur,
		Key::TextScrim => settings.text_scrim,
		Key::CursorScrim => settings.cursor_scrim,
		Key::CursorOutline => settings.cursor_outline,
		Key::CursorBlinking => settings.cursor_blink,
		// a group's switch is read off its members; mixed reads as off here
		Key::RememberSize => {
			config::auto::group_state(settings, config::auto::Group::WindowSize)
				== config::auto::State::On
		}
		Key::RememberPerMonitor => settings.remember_per_monitor,
		Key::RememberMaximized => settings.remember_maximized,
		Key::NewTabNextToCurrent => settings.new_tab_beside,
		Key::TabShowsTitle => settings.tab_shows_title,
		Key::TabShowsShell => settings.tab_shows_shell,
		Key::TabShowsProgram => settings.tab_shows_program,
		Key::TabShowsDirectory => settings.tab_shows_directory,
		Key::TitleShowsTab => settings.title_shows_tab,
		Key::IdleRelease => settings.idle_release,
		Key::SoftwareRendering => settings.software_rendering,
		Key::CopyOnSelect => settings.copy_on_select,
		Key::ShellIntegration => settings.shell_integration,
		Key::BashPrompt => settings.bash_prompt,
		Key::Hyperlinks => settings.hyperlinks,
		Key::BgContrastMask => settings.wallpaper_contrast_mask,
		Key::BgEnabled => settings.wallpaper_enabled,
		Key::BgRotate => settings.wallpaper_rotate_enabled,
		Key::BgHonorXmp => settings.wallpaper_honor_xmp,
		Key::BgHonorXmpLook => settings.wallpaper_honor_xmp_look,
		Key::ColFromWallpaper => settings.colors_from_wallpaper,
		Key::SmoothScroll => settings.scroll_smooth,
		Key::Scrollbar => settings.scrollbar,
		Key::ScrollbarAutoHide => settings.scrollbar_auto_hide,
		Key::Minimap => settings.minimap,
		keys_of!(slider | radio | color | text | hotkey | valueless | assoc) => false,
	}
}

// The option a radio or dropdown row has chosen, read from `settings`.
fn radio_of(settings: &Settings, key: Key) -> usize {
	match key {
		Key::PerfProfile => crate::profile::current(settings).index(),
		Key::BgFit => match settings.wallpaper_default_fit {
			config::Fit::Zoom => 1,
			config::Fit::Stretch => 0,
		},
		// each type's `ALL` is in the order settings_ui.shcl lists the options
		Key::ScrimFunction => settings.text_scrim_function.index(),
		Key::ScrimRamp => settings.text_scrim_ramp.index(),
		Key::CursorAnimation => settings.cursor_animation.index(),
		Key::Theme => crate::theme::all_names(&settings.user_themes)
			.iter()
			.position(|n| n.eq_ignore_ascii_case(settings.theme.trim()))
			.unwrap_or(0),
		Key::ThemeMode => settings.theme_mode.index(),
		keys_of!(slider | toggle | color | text | hotkey | valueless | assoc) => 0,
	}
}

// The rows a performance profile sets - one list, so the dialog's display rule
// and profile.rs's field list cannot drift apart without a test noticing.
const GOVERNED: &[Key] = &[
	Key::SmoothScroll,
	Key::ScrollEaseIn,
	Key::ScrollRampUp,
	Key::SingleScreenTau,
	Key::ScrollRampDown,
	Key::ScrollEaseOut,
	Key::CursorBlinking,
	Key::TextScrim,
	Key::ScrimRadius,
	Key::ScrimStrength,
	Key::ScrimSoftness,
	Key::ScrimFunction,
	Key::Outline,
	Key::BgEnabled,
	Key::BgBlur,
	Key::BgContrastMask,
];
const PROFILE_TIP: &str =
	"Set by the performance profile. Changing it switches the profile to Custom.";

// What the Theme dropdown says once a color has moved off the theme's own.
const UNSAVED_THEME: &str = "[unsaved]";
// The letter on the mark an automatic value carries, and the tips about it.
const AUTO_MARK: &str = "A";
const AUTO_TIP: &str = "Automatic. Change it to set your own value.";
const CLEAR_TIP: &str = "Back to automatic";

// A slider's handle, centered on the value, so it overhangs the track's ends.
const SLIDER_HANDLE_W: f32 = 10.0;
// The revert arrow's clear space either side, inside its column, DIP.
const REVERT_INSET: f32 = 4.0;
// What a hotkey row's box says while it waits for the new chord.
const CAPTURE_PROMPT: &str = "Press keys - Esc cancels, Backspace turns off";

// Clear space between the parts of a line that carries several controls, DIP.
const PAIR_GAP: f32 = 12.0;
// Between a label and the box it names, either side of it, DIP. Grows with the
// font through `font_gap`.
const PART_LABEL_GAP: f32 = 6.0;
// Between one toggle and the next label on a packed line, DIP. Well over
// PART_LABEL_GAP, so each label reads with its own box. Grows with the font too,
// or a big font's label gap creeps up on it.
const PACK_GAP: f32 = 24.0;
// The UI line height, DIP, the two gaps above were drawn at: the 11 pt default
// interface font. A taller line widens them in step and a shorter one leaves
// them be. The shells grid's name column and column gap go by it too.
const GAPS_DRAWN_AT: f32 = 20.0;

// A text gap, or a column sized for text nobody can measure ahead, at UI line
// height `line_h`, so a 24 pt label doesn't sit against its box
// (2026100817172887, 2026100818102267).
fn font_gap(gap: f32, line_h: f32) -> f32 {
	gap * (line_h / GAPS_DRAWN_AT).max(1.0)
}

// Side of a square box, checkbox or radio, at UI line height `line_h`. Both grow
// at one rate, the radio's, so a large font draws them one size. `floor` is the
// kind's own declared size, which is why the default font shows the checkbox a
// bit bigger (2026100817355493).
fn square_box(floor: f32, line_h: f32) -> f32 {
	(lay().radio_box * (line_h / lay().base_line_height)).max(floor)
}

// The warning mark after a label, in UI line heights, so it grows with the
// interface font. Ratios, because the label column is measured in physical
// pixels and the layout in DIP.
const WARN_GAP: f32 = 0.35;
const WARN_W: f32 = 0.95;
const WARN_H: f32 = 0.82;

// What a warning mark adds to its label's width, in the units `line_h` is in.
fn warning_room(line_h: f32) -> f32 {
	line_h * (WARN_GAP + WARN_W)
}

// `r` cut down to what falls inside `to`. Zero width when nothing does.
fn clip_rect(r: Rect, to: Rect) -> Rect {
	let x0 = r.x.max(to.x);
	let x1 = (r.x + r.w).min(to.x + to.w);
	let y0 = r.y.max(to.y);
	let y1 = (r.y + r.h).min(to.y + to.h);
	Rect {
		x: x0,
		y: y0,
		w: (x1 - x0).max(0.0),
		h: (y1 - y0).max(0.0),
	}
}

/// A plain quad in one color.
pub fn quad(x: f32, y: f32, w: f32, h: f32, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [x, y],
		size: [w, h],
		color: config::srgb_f32(color),
		..Default::default()
	}
}

// A frame `t` thick just outside `r`.
fn border(out: &mut Vec<RectInstance>, r: Rect, t: f32, color: [u8; 3]) {
	out.push(quad(r.x - t, r.y - t, r.w + 2.0 * t, t, color));
	out.push(quad(r.x - t, r.y + r.h, r.w + 2.0 * t, t, color));
	out.push(quad(r.x - t, r.y, t, r.h, color));
	out.push(quad(r.x + r.w, r.y, t, r.h, color));
}

// What holds keyboard focus: one control within a row, or a footer button (index
// into `buttons()`: 0 = Cancel, 1 = Apply, 2 = OK). `Row(i, part)` names a row and
// which of its focusable sub-controls (part 0 for a plain control; sliders and the
// combined cursor row expose two parts). Tab walks parts then buttons.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Focus {
	Row(usize, u16),
	Button(usize),
}

/// Where the user was looking when the dialog closed, so reopening it shortly
/// after opens on the same tab and scroll position instead of the top of
/// Appearance. Only the view - edits are discarded on close as before.
#[derive(Debug, Clone, Copy)]
pub struct View {
	tab: usize,
	scroll: f32,
}

// In-progress field edit: the row, its text, the caret (a byte index into
// `buf`, always on a char boundary), and an optional selection anchor. The
// selection spans anchor..caret in either direction; None = no selection.
struct EditState {
	row: usize,
	buf: String,
	cur: usize,
	sel: Option<usize>,
	// Horizontal view: px of text hidden left of the box. `view` is the smoothed
	// offset actually drawn, easing toward `view_to` (kept caret-in-view with a
	// lookahead margin by `animate`). Everything that maps px<->byte (clicks,
	// drags, the caret/selection quads, the drawn text x) offsets by `view`.
	view: f32,
	view_to: f32,
	// smoothed caret x in text-space px; None until first measured (then snaps)
	caret_vis: Option<f32>,
	blink_t: f32, // seconds since the last caret/text activity (drives the blink)
	// (cur, sel, buf.len()) at the last animate pass - a change resets the blink
	last_sig: (usize, Option<usize>, usize),
	// "File or folder" only: which of the two the field was opened on. Fixed for
	// the life of the field, or emptying the box on the way to typing an image
	// sends the rest of the typing to the folder.
	wallpaper_folder: bool,
}
impl EditState {
	// A field opened on `buf`, caret at the end. `row` is `usize::MAX` for the
	// prompt box's field, which belongs to no row.
	fn new(row: usize, buf: String) -> EditState {
		let cur = buf.len();
		EditState {
			row,
			buf,
			cur,
			sel: None,
			view: 0.0,
			view_to: 0.0,
			caret_vis: None,
			blink_t: 0.0,
			last_sig: (usize::MAX, None, usize::MAX),
			wallpaper_folder: false,
		}
	}
	// Smooth blink: solid just after activity, then a soft cosine pulse (never a
	// hard on/off pop).
	fn caret_alpha(&self) -> f32 {
		const HOLD: f32 = 0.55;
		const PERIOD: f32 = 1.1;
		if self.blink_t <= HOLD {
			return 1.0;
		}
		0.5 + 0.5 * ((self.blink_t - HOLD) / PERIOD * std::f32::consts::TAU).cos()
	}
	// normalized selection byte range, None when empty/absent
	fn sel_range(&self) -> Option<(usize, usize)> {
		let anchor = self.sel?;
		if anchor == self.cur {
			return None;
		}
		Some((anchor.min(self.cur), anchor.max(self.cur)))
	}
	// remove the selected span (caret ends at its start); true if anything went
	fn remove_selection(&mut self) -> bool {
		let Some((a, b)) = self.sel_range() else {
			self.sel = None;
			return false;
		};
		self.buf.replace_range(a..b, "");
		self.cur = a;
		self.sel = None;
		true
	}
}

// One arrow-key increment for a slider: ~1/100 of the range normally, ~1/10 with
// Shift (so ~100 / ~10 steps span it), rounded to a whole unit (>=1) for int fields.
fn slider_step(min: f32, max: f32, int: bool, shift: bool) -> f32 {
	let span = (max - min).abs();
	let raw = if shift { span / 10.0 } else { span / 100.0 };
	if int { raw.round().max(1.0) } else { raw }
}

// A slider's numbers, lifted out of its `Kind` so the track mapping, the arrow
// step and the typed clamp are one set of rules for every slider.
#[derive(Clone, Copy, Debug)]
struct SliderScale {
	min: f32,
	max: f32,
	int: bool,
	log: bool,
	typed_max: f32,
}

impl SliderScale {
	fn of(kind: &Kind) -> Option<Self> {
		match *kind {
			Kind::Slider {
				min,
				max,
				int,
				log,
				typed_max,
			} => Some(Self {
				min,
				max,
				int,
				log,
				typed_max,
			}),
			_ => None,
		}
	}

	// 0..1 along the track. A value typed past the end sits at the end.
	fn frac(self, value: f32) -> f32 {
		let frac = if self.log {
			log_pos(value, self.min, self.max)
		} else {
			(value - self.min) / (self.max - self.min)
		};
		// clamp lets NaN through
		if frac.is_nan() {
			0.0
		} else {
			frac.clamp(0.0, 1.0)
		}
	}

	fn at(self, frac: f32) -> f32 {
		let value = if self.log {
			log_val(frac, self.min, self.max)
		} else {
			self.min + frac.clamp(0.0, 1.0) * (self.max - self.min)
		};
		self.whole(value)
	}

	fn whole(self, value: f32) -> f32 {
		if self.int { value.round() } else { value }
	}

	// One arrow press. On a log scale a step is a hundredth of the track (a
	// tenth with Shift) as a ratio, so it is as big a change at 5 as at 500,
	// and a whole-number one always moves by at least 1. A press never takes a
	// value further past the end than it already is, so Up stops at the end,
	// and Down from a typed number steps down from that number.
	fn stepped(self, value: f32, dir: i32, shift: bool) -> f32 {
		let next = if self.log {
			let ratio = (self.max / self.min).powf(if shift { 0.1 } else { 0.01 });
			let next = self.whole(value * ratio.powi(dir));
			if self.int && next == value.round() {
				next + dir as f32
			} else {
				next
			}
		} else {
			self.whole(value + dir as f32 * slider_step(self.min, self.max, self.int, shift))
		};
		next.clamp(self.min, self.max.max(value).min(self.typed_max))
	}

	// What the number box takes. Below the slider still clamps; above it goes
	// as far as `typed_max`.
	fn typed(self, value: f32) -> f32 {
		self.whole(value.clamp(self.min, self.typed_max))
	}

	// The longest numbers the box can show: either end, and for a decimal the
	// top of each band before it drops a place, since 999.9 is longer than 3600.
	fn widest_texts(self) -> Vec<String> {
		let bands: &[f32] = if self.int {
			&[]
		} else {
			&[99.94, 999.94, -99.94, -999.94]
		};
		let reachable = |v: &&f32| (self.min..=self.typed_max).contains(*v);
		[self.min, self.typed_max]
			.iter()
			.chain(bands.iter().filter(reachable))
			.map(|&v| fmt_number(self.whole(v), self.int))
			.collect()
	}
}

// How a slider's box shows a value.
fn fmt_number(value: f32, int: bool) -> String {
	if int {
		format!("{}", value.round() as i64)
	} else {
		// fewer decimals as the whole part grows, so 3600 still fits the box
		let places = match value.abs() {
			v if v >= 999.95 => 0,
			v if v >= 99.95 => 1,
			_ => 2,
		};
		format!("{value:.places$}")
	}
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
	None,
	Apply,
	Ok,
	Cancel,
	// a field context-menu command; the clipboard glue lives in dialog.rs
	Edit(EditCmd),
}

/// Field context-menu commands (right-click / Menu key in an editable field).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditCmd {
	Cut,
	Copy,
	Paste,
	Delete,
	SelectAll,
}
const EDIT_MENU: [(&str, EditCmd); 5] = [
	("Cut", EditCmd::Cut),
	("Copy", EditCmd::Copy),
	("Paste", EditCmd::Paste),
	("Delete", EditCmd::Delete),
	("Select all", EditCmd::SelectAll),
];

// The Windows file-type rows, which act on the registry rather than a setting.
fn assoc_of(key: Key) -> Option<Assoc> {
	match key {
		Key::OpenBatch => Some(Assoc::Batch),
		Key::OpenPowerShell => Some(Assoc::PowerShell),
		Key::OpenVbScript => Some(Assoc::VbScript),
		Key::OpenFolder => Some(Assoc::Folder),
		_ => None,
	}
}
fn assoc_slot(assoc: Assoc) -> usize {
	Assoc::ALL.iter().position(|&a| a == assoc).unwrap_or(0)
}

// The prompt's text field, the shells grid's own fields and the color picker's
// six value boxes are the dialog's ONE open edit, so every bit of field behavior
// - selection, word ops, the clipboard, the caret ease, the right-click menu -
// works in all of them without a second copy. None belongs to a spec row, so
// each borrows an index no row can have: every `specs[edit.row]` comparison in
// this module simply never matches one, and the four places that INDEX specs by it
// each carry their own guard.
const PSEUDO_ROW: usize = usize::MAX / 2;
const PROMPT_ROW: usize = usize::MAX;
// Two per shell entry, counting down: name, then command.
const SHELL_ROW_BASE: usize = usize::MAX - 1;
// The picker box's six value boxes, counting up from the base. The grid counts
// down from the other end, so the two ranges cannot meet.
const PICK_ROW_BASE: usize = PSEUDO_ROW;

// Which value box a pseudo row stands for, if it is one of the picker's.
fn pick_field_of(row: usize) -> Option<pick::Field> {
	pick::Field::ALL
		.get(row.checked_sub(PICK_ROW_BASE)?)
		.copied()
}
fn pick_field_row(f: pick::Field) -> usize {
	PICK_ROW_BASE + pick::Field::ALL.iter().position(|&a| a == f).unwrap_or(0)
}

// Which grid field a pseudo row stands for: (entry index, is-the-command-field).
fn shell_field_of(row: usize) -> Option<(usize, bool)> {
	if row < PSEUDO_ROW || row == PROMPT_ROW || pick_field_of(row).is_some() {
		return None;
	}
	let field = SHELL_ROW_BASE - row;
	Some((field / 2, field % 2 == 1))
}
fn shell_field_row(entry: usize, command: bool) -> usize {
	SHELL_ROW_BASE - (entry * 2 + usize::from(command))
}

// Open field context menu: anchor point, keyboard-highlighted item, and whether
// the clipboard held text when it opened (grays Paste).
struct EMenu {
	x: f32,
	y: f32,
	hover: Option<usize>,
	paste_ok: bool,
}

#[derive(Debug)]
pub struct TextItem {
	pub text: String,
	pub x: f32,
	pub y: f32,
	pub color: [u8; 3],
	pub clip: Option<Rect>, // when set, clip drawing to this rect (e.g. a field)
	pub bold: bool,
	pub italic: bool, // an automatic value, the way a placeholder reads
	pub scale: f32,   // 1.0 normal; >1 for the prominent dialog title
}

impl TextItem {
	// Unclipped text at the normal size and weight.
	fn plain(text: String, x: f32, y: f32, color: [u8; 3]) -> Self {
		Self {
			text,
			x,
			y,
			color,
			clip: None,
			bold: false,
			italic: false,
			scale: 1.0,
		}
	}
}

// The row tops from the last walk down a tab, and what that walk started from.
// A frame asks for a row's top from every rect helper, so `row_y` walks again
// only when the tab, its first top, the line height or the shell count moves.
#[derive(Default)]
struct RowTops {
	walked: Option<(usize, u32, u32, usize)>,
	tops: Vec<f32>, // by spec index; a row the tab does not draw has `end`
	end: f32,
}

// Walks down a tab to find its row tops, on this thread. Test builds only.
#[cfg(test)]
thread_local! {
	static ROW_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub struct SettingsDialog {
	orig: Settings,
	edited: Settings,
	defaults: Settings,          // config defaults, for the revert-to-default buttons
	reverted: Vec<&'static str>, // config keys reverted this session -> comment out on Apply
	rect: Rect,
	// What the content wants, DIP. The window can be shorter or narrower than
	// this (a small screen, or the user dragging it in), and then the rows region
	// scrolls; wider, and the stretchy controls take up the slack.
	natural: (f32, f32),
	specs: &'static [Spec],
	tab: usize,                        // active tab
	tab_ws: Vec<f32>,                  // measured tab-button widths (UI font)
	label_ws: Vec<f32>,                // each row's measured label, by spec index
	scroll: f32,                       // rows-region scroll offset (0 when everything fits)
	hscroll: f32,                      // sideways offset, when the window is narrower than `natural`
	drag_thumb: Option<f32>,           // scrollbar-thumb drag: grab offset within the thumb
	drag_hthumb: Option<f32>,          // the same for the horizontal bar
	drag: Option<usize>,               // slider row being dragged
	shell_drag: Option<ShellDrag>,     // shells line being dragged by its grip
	pressed: Option<usize>,            // footer button held down (fires on release; drawn pressed)
	pressed_row: Option<(usize, u16)>, // a row's push-button held down (same press/release)
	prompt: Option<Prompt>,            // the name / confirm box a theme action puts over the panel
	pick: Option<Picker>,              // the color picker a chip opens, modal over the panel
	edit: Option<EditState>,           // row being typed (hex for Color, path for Text)
	// A hotkey row waiting for its new chord, and what was wrong with the last
	// press, if anything was.
	capture: Option<usize>,
	capture_refused: Option<String>,
	// Chords a press here took off another hotkey the file had set them for:
	// (that hotkey, the chord, the one that has it now). Kept to say so on the
	// row that lost it, since the bindings no longer see a clash.
	moved: Vec<(Hotkey, Chord, Hotkey)>,
	// Chords are spoken the Mac's way. Held so a test can ask for either.
	mac: bool,
	edit_drag: Option<usize>, // field row being drag-selected with the mouse
	select_all_on_up: bool, // a fresh single-click field entry: select all on release unless it became a drag
	// multi-click detection (double = select word, triple = select all)
	last_click: Option<(std::time::Instant, f32, f32)>,
	click_streak: u8,
	open: Option<usize>,  // row whose dropdown popup is open (None = all closed)
	pending: usize,       // highlighted option in the open popup (commits on Enter/click)
	emenu: Option<EMenu>, // open field context menu (right-click / Menu key)
	mouse: (f32, f32),    // last cursor pos (drag edge-autoscroll replays it)
	row_tops: std::cell::RefCell<RowTops>,
	// The monitor the window is on, for the automatic size's rule.
	monitor: Option<String>,
	// Whether the desktop says when a window is minimized or covered. Wayland
	// does not, so the hidden wait never applies there.
	sees_hidden: bool,
	// Where the Windows file-type rows write, and whether each of
	// `fileassoc::Assoc::ALL` is registered. A map stands in for the registry in
	// tests. The answer is read when the dialog opens and after each change, not
	// per frame, since it drives a revert arrow.
	assoc: Box<dyn crate::fileassoc::Store + Send>,
	assoc_on: [bool; 4],
	focus: Option<Focus>, // keyboard-focused control/button (None = mouse-only)
	alt: bool,            // Alt held: underline button accelerators (Cancel/Apply/OK)
	shift: bool,          // Shift held (Shift+Tab walks focus backwards)
	ctrl: bool,           // the shortcut key held: Ctrl, or Command on a Mac
	word: bool,           // the arrows and erase keys go by words
	line: bool,           // the arrows go to either end, Backspace to the start
	types: bool,          // a character key types
	// UI-font-driven geometry: rows/title/buttons grow with the desktop font so
	// a large or wide (e.g. bold serif) interface font never truncates. The
	// consts above are the floor (the classic look at small sizes).
	line_h: f32,
	label_w: f32,
	btn_w: f32,
	row_btn_w: f32, // push-buttons that sit on a row (see chrome_widths)
	value_w: f32,   // every slider's number box, so the sliders end in one column
	revert_w: f32,
	seen_w: f32,
	active_w: f32,
	pick_label_w: f32,
	pick_field_w: f32,
	// DIP -> physical pixel factor for the window this dialog lives in. Every
	// measurement in here is a DIP; this is applied only at the boundary.
	scale: f32,
}

// Three whole copies of the settings would bury the rest.
impl std::fmt::Debug for SettingsDialog {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SettingsDialog")
			.field("tab", &self.tab)
			.finish_non_exhaustive()
	}
}

impl SettingsDialog {
	// `shells` is the length of the stored list: the grid is one spec row and n
	// lines on screen, so it is the one kind whose height is not a constant.
	fn row_h_for(kind: &Kind, line_h: f32, shells: usize) -> f32 {
		let line = lay().row_height.max(line_h + lay().row_pad);
		match kind {
			Kind::Header(_) => lay().header_height.max(line_h + lay().header_pad),
			// column titles, a line per shell, then the Add button
			Kind::ShellList => {
				line_h
					+ lay().shell_head_gap
					+ shells as f32 * line
					+ lay().shell_add_gap
					+ lay().button_height.max(line_h + lay().row_pad)
			}
			_ => line,
		}
	}
	fn row_h(&self, kind: &Kind) -> f32 {
		Self::row_h_for(kind, self.line_h, self.edited.shells.len())
	}
	// The height of an ordinary row. Every boxed control centers in THIS, not in
	// the `row_height` floor - at a large UI font the two are far apart, and
	// centering in the floor leaves the controls riding high in their rows.
	fn line_row_h(&self) -> f32 {
		lay().row_height.max(self.line_h + lay().row_pad)
	}
	// How tall row `i` LOOKS. A row drawn beside another shares that row's line,
	// so it stands a line tall even though it advances the walk by nothing.
	fn row_screen_h(&self, i: usize) -> f32 {
		if self.specs[i].beside {
			self.line_row_h()
		} else {
			self.row_h(&self.specs[i].kind)
		}
	}
	// An editable field is taller than a checkbox: the text needs clear space
	// above and below it, not just left and right.
	fn field_h(&self) -> f32 {
		lay()
			.field_height
			.max(self.line_h + 2.0 * lay().field_pad_v)
	}
	fn btn_h(&self) -> f32 {
		lay().button_height.max(self.line_h + lay().row_pad)
	}

	// A heading that only repeats its own tab's title says nothing the tab strip
	// has not already said, so it takes no space and draws nothing. It stays in
	// the declarations because a heading is also what assigns the rows under it
	// to a tab - deleting it there would orphan them.
	fn header_is_tab_title(spec: &Spec) -> bool {
		matches!(spec.kind, Kind::Header(label) if tab_titles().get(spec.tab) == Some(&label))
	}

	// The rows one tab actually draws, in order.
	fn visible(specs: &[Spec], tab: usize) -> impl Iterator<Item = (usize, &Spec)> {
		specs
			.iter()
			.enumerate()
			.filter(move |(_, spec)| spec.tab == tab && !Self::header_is_tab_title(spec))
	}

	// The rows after `i` on the same tab, in order. Scanned forward from `i`
	// rather than filtered from the top: row_y calls this per row, and the walk
	// down a tab would otherwise be quadratic in the whole declaration.
	fn after(specs: &[Spec], i: usize, tab: usize) -> impl Iterator<Item = (usize, &Spec)> {
		specs
			.iter()
			.enumerate()
			.skip(i + 1)
			.filter(move |(_, spec)| spec.tab == tab && !Self::header_is_tab_title(spec))
	}
	// The next row DOWN from `i`, skipping anything that shares `i`'s own line.
	fn next_row(specs: &[Spec], i: usize, tab: usize) -> Option<&Spec> {
		Self::after(specs, i, tab)
			.find(|(_, spec)| !spec.beside)
			.map(|(_, spec)| spec)
	}
	// The row drawn beside `i`, when there is one. The second of the pair carries
	// the line's height, which is what keeps `row_y` returning the same y for both.
	fn paired_with(specs: &[Spec], i: usize, tab: usize) -> Option<usize> {
		Self::after(specs, i, tab)
			.next()
			.filter(|(_, next)| next.beside)
			.map(|(j, _)| j)
	}
	fn pairs_down(specs: &[Spec], i: usize, tab: usize) -> bool {
		Self::paired_with(specs, i, tab).is_some()
	}
	// The line row `i` is drawn on: its first row, how many rows share it, and
	// which of them `i` is. The parser keeps a line's rows next to each other.
	fn line_of(specs: &[Spec], i: usize) -> (usize, usize, usize) {
		let mut lead = i;
		while lead > 0 && specs[lead].beside {
			lead -= 1;
		}
		let parts = 1 + specs[lead + 1..].iter().take_while(|s| s.beside).count();
		(lead, parts, i - lead)
	}
	// A line of toggles packs at its natural width, out of the label and control
	// columns: each label right before its own box, a fixed gap to the next one
	// (2026100710173200). A line with anything else on it splits the column.
	fn packs(specs: &[Spec], i: usize) -> bool {
		let (lead, parts, _) = Self::line_of(specs, i);
		parts > 1
			&& specs[lead..lead + parts]
				.iter()
				.all(|s| matches!(s.kind, Kind::Toggle))
	}
	// A packed line in a group with other rows keeps its first box in the control
	// column, under theirs, and only the rest pack after it. One alone under its
	// heading has nothing to line up with (2026100812334385).
	fn lead_keeps_column(specs: &[Spec], i: usize) -> bool {
		let (lead, parts, _) = Self::line_of(specs, i);
		let heading = |s: &&Spec| matches!(s.kind, Kind::Header(_));
		specs[..lead]
			.iter()
			.rev()
			.take_while(|s| !heading(s))
			.chain(specs[lead + parts..].iter().take_while(|s| !heading(s)))
			.any(|s| !s.beside)
	}
	// Whether row `i`'s label counts toward the label column.
	fn in_label_column(specs: &[Spec], i: usize) -> bool {
		!Self::packs(specs, i) || (!specs[i].beside && Self::lead_keeps_column(specs, i))
	}
	// Where part `k` of the packed line led by `lead` puts its label and its box,
	// in from the line's left edge. A warning mark rides with its label.
	fn packed_at(
		specs: &[Spec],
		lead: usize,
		k: usize,
		label_ws: &[f32],
		line_h: f32,
	) -> (f32, f32) {
		let mut x = 0.0;
		for (j, spec) in specs.iter().enumerate().skip(lead).take(k + 1) {
			let label = if spec.label.is_empty() {
				0.0
			} else {
				let mark = if spec.warning.is_empty() {
					0.0
				} else {
					warning_room(line_h)
				};
				label_ws.get(j).copied().unwrap_or(0.0) + mark + font_gap(PART_LABEL_GAP, line_h)
			};
			if j == lead + k {
				return (x, x + label);
			}
			x += label + square_box(lay().swatch, line_h) + font_gap(PACK_GAP, line_h);
		}
		(x, x)
	}
	// How far a packed line runs, from its first label to the end of its last box.
	fn packed_w(specs: &[Spec], lead: usize, label_ws: &[f32], line_h: f32) -> f32 {
		let (_, parts, _) = Self::line_of(specs, lead);
		Self::packed_at(specs, lead, parts - 1, label_ws, line_h).1
			+ square_box(lay().swatch, line_h)
	}
	// The control column a line led by `lead` needs, from what each part has to
	// show: a dropdown its pair floor, a toggle its box and any label before it.
	// The middle parts lose a whole gap to their neighbors and the end ones half.
	fn line_need(specs: &[Spec], lead: usize, label_ws: &[f32], line_h: f32) -> f32 {
		let (_, parts, _) = Self::line_of(specs, lead);
		if parts < 2 || Self::packs(specs, lead) {
			return 0.0;
		}
		let font_scale = (line_h / lay().base_line_height).max(1.0);
		let n = parts as f32;
		(0..parts)
			.map(|k| {
				let spec = &specs[lead + k];
				let own = match spec.kind {
					Kind::Dropdown(_) => lay().dropdown_pair_width * font_scale,
					Kind::Toggle if k > 0 && !spec.label.is_empty() => {
						label_ws.get(lead + k).copied().unwrap_or(0.0)
							+ font_gap(PART_LABEL_GAP, line_h)
							+ square_box(lay().swatch, line_h)
					}
					Kind::Toggle => square_box(lay().swatch, line_h),
					_ => 0.0,
				};
				let gaps = [k > 0, k + 1 < parts].iter().filter(|&&g| g).count() as f32;
				n * (own + gaps * PAIR_GAP / 2.0)
			})
			.fold(0.0f32, f32::max)
	}
	// A row leads a sub-group when the row drawn under it is indented further.
	// Read off the indentation rather than declared a second time, so the two
	// cannot disagree.
	fn leads_subgroup(specs: &[Spec], i: usize, tab: usize) -> bool {
		// half a line leads nothing, and its own indent means nothing either
		!specs[i].beside
			&& Self::next_row(specs, i, tab).is_some_and(|next| next.indent > specs[i].indent)
	}

	// Clear space above a drawn row: a group heading is set off from the section
	// before it, a sub-group's leader from the sub-group before it. Neither
	// applies at the top of a section, where the separation is already there.
	fn gap_above(specs: &[Spec], i: usize, tab: usize, prev: Option<&Spec>) -> f32 {
		let Some(prev) = prev else { return 0.0 };
		if specs[i].beside {
			return 0.0; // mid-line: there is nothing above it
		}
		if matches!(specs[i].kind, Kind::Header(_)) {
			return lay().header_gap;
		}
		if matches!(prev.kind, Kind::Header(_)) || !Self::leads_subgroup(specs, i, tab) {
			return 0.0;
		}
		lay().subgroup_gap
	}

	// The tabs the dialog's height is taken from. The shell list's height moves
	// with the data, and the hotkeys are a long fixed list; both scroll instead.
	fn fixed_tabs(specs: &[Spec]) -> impl Iterator<Item = usize> + '_ {
		(0..tab_titles().len()).filter(|&t| {
			!Self::visible(specs, t)
				.any(|(_, spec)| matches!(spec.kind, Kind::ShellList | Kind::Hotkey(_)))
		})
	}

	// Natural height of one tab's rows (gaps included). Static so `new` can size
	// the window before Self exists; row_y must walk rows the same way.
	fn tab_content_h(specs: &[Spec], tab: usize, line_h: f32, shells: usize) -> f32 {
		let mut h = 0.0;
		let mut prev: Option<&Spec> = None;
		for (i, spec) in Self::visible(specs, tab) {
			h += Self::gap_above(specs, i, tab, prev);
			h += Self::row_advance(specs, i, tab, line_h, shells);
			prev = Some(spec);
		}
		h
	}
	// What a row adds to the walk down a tab. A row with another drawn beside it
	// adds nothing; the one beside it adds the line they share.
	fn row_advance(specs: &[Spec], i: usize, tab: usize, line_h: f32, shells: usize) -> f32 {
		if Self::pairs_down(specs, i, tab) {
			0.0
		} else {
			Self::row_h_for(&specs[i].kind, line_h, shells)
		}
	}

	/// `line_h` is the chrome (UI font) line height; `chrome` is the text it has
	/// to fit, measured in that font (see `chrome_widths`) so nothing truncates.
	/// `max_w`/`max_h` cap the window to what the screen can show; a tab that
	/// doesn't fit scrolls instead of clipping the buttons.
	/// `scale` is the window's DIP -> physical factor; every other argument arrives
	/// in physical pixels and is converted on the way in (see the module note on
	/// the DIP boundary).
	pub fn new(
		screen_w: f32,
		screen_h: f32,
		line_h: f32,
		chrome: Chrome,
		max_w: f32,
		max_h: f32,
		scale: f32,
	) -> Self {
		let scale = sane_scale(scale);
		let (screen_w, screen_h) = (screen_w / scale, screen_h / scale);
		let (line_h, max_w, max_h) = (line_h / scale, max_w / scale, max_h / scale);
		let chrome = chrome.in_dip(scale);
		// Natural size first, then what the screen leaves room for. Below the
		// natural size the rows region scrolls in that direction; above it the
		// stretchy controls spread out.
		let natural = Self::natural_dip(line_h, &chrome);
		let (w, natural_h) = natural;
		let (min_w, min_h) = Self::min_size_dip(line_h, chrome.btn_w);
		let w = w.min(max_w.max(min_w));
		let h = natural_h.min(max_h.max(min_h));
		let rect = Rect {
			x: ((screen_w - w) / 2.0).max(0.0),
			y: ((screen_h - h) / 2.0).max(0.0),
			w,
			h,
		};
		let specs: &'static [Spec] = &ui().specs;
		let settings = users_own((*config::settings()).clone());
		let mut dialog = Self {
			orig: settings.clone(),
			edited: settings,
			defaults: Settings::default(),
			reverted: Vec::new(),
			rect,
			natural,
			specs,
			tab: 0,
			tab_ws: chrome.tab_ws,
			label_ws: chrome.label_ws,
			scroll: 0.0,
			hscroll: 0.0,
			drag_thumb: None,
			drag_hthumb: None,
			drag: None,
			shell_drag: None,
			pressed: None,
			pressed_row: None,
			prompt: None,
			pick: None,
			edit: None,
			capture: None,
			capture_refused: None,
			moved: Vec::new(),
			mac: cfg!(target_os = "macos"),
			edit_drag: None,
			select_all_on_up: false,
			last_click: None,
			click_streak: 0,
			open: None,
			pending: 0,
			emenu: None,
			mouse: (0.0, 0.0),
			row_tops: std::cell::RefCell::default(),
			monitor: None,
			sees_hidden: true,
			assoc: crate::fileassoc::system(),
			assoc_on: [false; 4],
			focus: None,
			alt: false,
			shift: false,
			ctrl: false,
			word: false,
			line: false,
			types: true,
			line_h,
			label_w: chrome.label_w,
			btn_w: chrome.btn_w,
			row_btn_w: chrome.row_btn_w,
			value_w: chrome.value_w,
			revert_w: chrome.revert_w,
			seen_w: chrome.seen_w,
			active_w: chrome.active_w,
			pick_label_w: chrome.pick_label_w,
			pick_field_w: chrome.pick_field_w,
			scale,
		};
		dialog.assoc_refresh();
		dialog
	}

	// The size the content wants, in DIP, from the chrome already converted at
	// the boundary. `new` and `rescale` both solve it, and it is long enough that
	// a second copy would drift.
	fn natural_dip(line_h: f32, chrome: &Chrome) -> (f32, f32) {
		let Chrome {
			label_w,
			btn_w,
			row_btn_w,
			value_w,
			revert_w,
			..
		} = *chrome;
		let (tab_ws, label_ws) = (&chrome.tab_ws, &chrome.label_ws);
		let specs: &'static [Spec] = &ui().specs;
		let btn_h = lay().button_height.max(line_h + lay().row_pad);
		// A tab whose height moves with the data, the shell list's, scrolls
		// instead. Otherwise a long list of shells makes every tab tall.
		let tallest = Self::fixed_tabs(specs)
			.map(|t| Self::tab_content_h(specs, t, line_h, 0))
			.fold(0.0f32, f32::max);
		let natural_h = Self::gutter_h_for(line_h)
			+ 1.0 + lay().tabs_gap
			+ tallest + lay().buttons_gap
			+ btn_h + lay().pad;
		let tabs_w = lay().pad * 2.0
			+ tab_ws.iter().sum::<f32>()
			+ lay().tab_gap * tab_ws.len().saturating_sub(1) as f32;
		// widest radio row (scaled pitch at HiDPI / large fonts) must fit the panel,
		// or the last option overflows the right edge
		let font_scale = (line_h / lay().base_line_height).max(1.0);
		let max_radio_opts = specs
			.iter()
			.filter_map(|spec| match spec.kind {
				Kind::Radio(opts) => Some(opts.len()),
				_ => None,
			})
			.max()
			.unwrap_or(0) as f32;
		let radio_w =
			lay().pad + label_w + max_radio_opts * lay().radio_pitch * font_scale + lay().pad;
		// a dropdown's collapsed box (+ revert column) must fit too, and a shared
		// line needs every part's room in every one of its even parts
		let dd_ctl = (0..specs.len())
			.filter(|&i| !specs[i].beside)
			.map(|lead| Self::line_need(specs, lead, label_ws, line_h))
			.chain(
				specs
					.iter()
					.filter(|s| matches!(s.kind, Kind::Dropdown(_)) && !s.beside)
					.map(|_| lay().dropdown_width * font_scale),
			)
			.fold(0.0f32, f32::max);
		let dd_w = if dd_ctl > 0.0 {
			lay().pad + label_w + dd_ctl + 6.0 + revert_w + lay().pad
		} else {
			0.0
		};
		// a row of push-buttons starts at the control column and must fit too
		let max_row_btns = specs
			.iter()
			.filter_map(|spec| match spec.kind {
				Kind::Buttons(captions) => Some(captions.len()),
				_ => None,
			})
			.max()
			.unwrap_or(0) as f32;
		let btns_w = if max_row_btns > 0.0 {
			lay().pad
				+ label_w + max_row_btns * row_btn_w
				+ (max_row_btns - 1.0) * lay().button_gap
				+ lay().pad
		} else {
			0.0
		};
		// the shells grid spans the whole content width, so its columns are a floor
		// on the panel rather than on a column of it
		let grid_w = if specs.iter().any(|s| matches!(s.kind, Kind::ShellList)) {
			lay().pad + Self::shell_columns_w(line_h, chrome) + lay().pad
		} else {
			0.0
		};
		// a packed line of toggles ignores the columns, so it is a floor of its own
		let packed_w = (0..specs.len())
			.filter(|&lead| !specs[lead].beside && Self::packs(specs, lead))
			.map(|lead| {
				let run = Self::packed_w(specs, lead, label_ws, line_h);
				let line = if Self::lead_keeps_column(specs, lead) {
					label_w + run - Self::packed_at(specs, lead, 0, label_ws, line_h).1
				} else {
					f32::from(specs[lead].indent) * lay().indent + run
				};
				lay().pad + line + 6.0 + revert_w + lay().pad
			})
			.fold(0.0f32, f32::max);
		// a wider number box widens this floor, as a longer label does
		let w = (lay().width
			+ (label_w - lay().label_width)
			+ (btn_w - lay().button_width) * 3.0
			+ (value_w - lay().value_width))
			.max(tabs_w)
			.max(packed_w)
			.max(radio_w)
			.max(dd_w)
			.max(btns_w)
			.max(grid_w);
		(w, natural_h)
	}

	/// A scale-factor change: the window moved to a monitor at another scale, or
	/// the desktop's own scale moved under it. Everything below the boundary is
	/// DIP and stays as it is, so only the factor and the chrome measured in
	/// physical pixels at the old one are refreshed. Values, edits, focus and
	/// scroll are deliberately untouched - a rebuild would lose every unapplied
	/// edit the moment the window crossed a monitor edge.
	pub fn rescale(&mut self, line_h: f32, chrome: Chrome, max_w: f32, max_h: f32, scale: f32) {
		let scale = sane_scale(scale);
		let (line_h, max_w, max_h) = (line_h / scale, max_w / scale, max_h / scale);
		let chrome = chrome.in_dip(scale);
		self.line_h = line_h;
		self.scale = scale;
		self.natural = Self::natural_dip(line_h, &chrome);
		self.label_w = chrome.label_w;
		self.btn_w = chrome.btn_w;
		self.row_btn_w = chrome.row_btn_w;
		self.value_w = chrome.value_w;
		self.revert_w = chrome.revert_w;
		self.seen_w = chrome.seen_w;
		self.active_w = chrome.active_w;
		self.pick_label_w = chrome.pick_label_w;
		self.pick_field_w = chrome.pick_field_w;
		self.tab_ws = chrome.tab_ws;
		self.label_ws = chrome.label_ws;
		// The screen holds fewer DIP at a higher scale, so the window may no
		// longer fit what it was dragged to.
		let (min_w, min_h) = Self::min_size_dip(self.line_h, self.btn_w);
		self.rect.w = self.rect.w.min(max_w.max(min_w)).max(min_w);
		self.rect.h = self.rect.h.min(max_h.max(min_h)).max(min_h);
		self.scroll = self.scroll.clamp(0.0, self.max_scroll());
		self.hscroll = self.hscroll.clamp(0.0, self.max_hscroll());
	}

	// DIP <-> physical pixels. Coordinates and sizes cross the boundary in the
	// public methods only: everything below them is DIP.
	fn to_dip(&self, px: f32) -> f32 {
		px / self.scale
	}
	fn to_px(&self, dip: f32) -> f32 {
		dip * self.scale
	}
	fn rect_px(&self, r: Rect) -> Rect {
		Rect {
			x: self.to_px(r.x),
			y: self.to_px(r.y),
			w: self.to_px(r.w),
			h: self.to_px(r.h),
		}
	}
	// Scale a batch of quads out to physical pixels. `params.y` is a stroke width
	// or corner radius, so it is a measurement too and scales with the rest. A
	// triangle's is a count of quarter-turns, which a scale would turn it by.
	fn quads_px(&self, quads: &mut [RectInstance]) {
		for quad in quads {
			quad.pos = [self.to_px(quad.pos[0]), self.to_px(quad.pos[1])];
			quad.size = [self.to_px(quad.size[0]), self.to_px(quad.size[1])];
			match quad.mode() {
				QuadMode::Triangle => {}
				QuadMode::Solid
				| QuadMode::CloseMark
				| QuadMode::Rounded
				| QuadMode::PickSquare
				| QuadMode::HueStrip => quad.params[1] = self.to_px(quad.params[1]),
			}
		}
	}
	fn texts_px(&self, items: &mut [TextItem]) {
		for item in items {
			item.x = self.to_px(item.x);
			item.y = self.to_px(item.y);
			item.clip = item.clip.map(|r| self.rect_px(r));
		}
	}

	/// The pointer/measurement boundary. Pointer positions arrive in physical
	/// pixels and the caller's text measurement answers in them too, so both are
	/// divided down before any of the layout below sees them.
	pub fn mouse_down(&mut self, x: f32, y: f32, measure: &mut impl FnMut(&str) -> f32) -> Action {
		let s = self.scale;
		self.mouse_down_dip(x / s, y / s, &mut |t| measure(t) / s)
	}
	pub fn mouse_up(&mut self, x: f32, y: f32) -> Action {
		let s = self.scale;
		self.mouse_up_dip(x / s, y / s)
	}
	pub fn mouse_move(&mut self, x: f32, y: f32, measure: &mut impl FnMut(&str) -> f32) -> bool {
		let s = self.scale;
		self.mouse_move_dip(x / s, y / s, &mut |t| measure(t) / s)
	}
	pub fn mouse_right(
		&mut self,
		x: f32,
		y: f32,
		paste_ok: bool,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		let s = self.scale;
		self.mouse_right_dip(x / s, y / s, paste_ok, &mut |t| measure(t) / s);
	}
	pub fn menu_key(&mut self, paste_ok: bool, measure: &mut impl FnMut(&str) -> f32) {
		let s = self.scale;
		self.menu_key_dip(paste_ok, &mut |t| measure(t) / s);
	}
	pub fn animate(&mut self, dt: f32, measure: &mut impl FnMut(&str) -> f32) -> Option<u64> {
		let s = self.scale;
		self.animate_dip(dt, &mut |t| measure(t) / s)
	}
	pub fn hover_tip(&self, mx: f32, my: f32) -> Option<(Cow<'static, str>, Rect)> {
		#[cfg(test)]
		HOVER_TIPS.with(|n| n.set(n.get() + 1));
		let s = self.scale;
		self.hover_tip_dip(mx / s, my / s)
			.map(|(tip, anchor)| (tip, self.rect_px(anchor)))
	}

	/// The drawing boundary: the layout is solved in DIP, then everything handed
	/// to the renderer is multiplied out to physical pixels.
	pub fn rects(
		&self,
		line_h: f32,
		mut measure: impl FnMut(&str) -> f32,
	) -> (Vec<RectInstance>, Vec<RectInstance>) {
		let s = self.scale;
		let (mut fixed, mut rows) = self.rects_dip(line_h / s, |t| measure(t) / s);
		self.quads_px(&mut fixed);
		self.quads_px(&mut rows);
		(fixed, rows)
	}
	pub fn texts(&self, line_h: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<TextItem> {
		let s = self.scale;
		let mut items = self.texts_dip(line_h / s, |t| measure(t) / s);
		self.texts_px(&mut items);
		items
	}
	pub fn overlay(
		&self,
		measure: &mut impl FnMut(&str) -> f32,
	) -> (Vec<RectInstance>, Vec<TextItem>) {
		let s = self.scale;
		let (mut quads, mut items) = self.overlay_dip(&mut |t| measure(t) / s);
		self.quads_px(&mut quads);
		self.texts_px(&mut items);
		(quads, items)
	}

	// Tab-strip / rows-viewport / scrollbar geometry. The rows region sits between
	// the strip and the buttons; only it scrolls (chrome stays put).
	//
	// The tabs stand on a gutter strip that runs the panel's full width, and the
	// line closing that strip is what they stand ON - so a tab is shorter than a
	// footer button (it is chrome, not a control) and the strip's height is
	// simply the drop from the panel edge plus that tab.
	fn tab_h(&self) -> f32 {
		Self::tab_h_for(self.line_h)
	}
	fn tab_h_for(line_h: f32) -> f32 {
		lay().tab_height.max(line_h + lay().tab_pad_v)
	}
	fn gutter_h_for(line_h: f32) -> f32 {
		lay().tab_top + Self::tab_h_for(line_h)
	}
	fn tab_bar_y(&self) -> f32 {
		self.rect.y + lay().tab_top
	}
	// The strip, and the 1px rule closing it off from the rows below.
	fn gutter_rect(&self) -> Rect {
		Rect {
			x: self.rect.x,
			y: self.rect.y,
			w: self.rect.w,
			h: Self::gutter_h_for(self.line_h),
		}
	}
	// Where a tab sits, and where the whole strip does. The strip has an offset of
	// its own rather than riding the rows' sideways scroll: a tab panned off the
	// window could not be clicked at all, and the tabs are how you leave a tab
	// that will not fit. It moves only far enough to keep the current one in view.
	fn tabs_w(&self) -> f32 {
		lay().pad * 2.0
			+ self.tab_ws.iter().sum::<f32>()
			+ lay().tab_gap * self.tab_ws.len().saturating_sub(1) as f32
	}
	fn tab_scroll(&self) -> f32 {
		let over = (self.tabs_w() - self.rect.w).max(0.0);
		if over <= 0.0 || self.tab >= self.tab_ws.len() {
			return 0.0;
		}
		let right = lay().pad
			+ self.tab_ws[..=self.tab].iter().sum::<f32>()
			+ lay().tab_gap * self.tab as f32
			+ lay().pad;
		(right - self.rect.w).clamp(0.0, over)
	}
	fn tab_rect(&self, tab: usize) -> Rect {
		let x = self.rect.x - self.tab_scroll()
			+ lay().pad
			+ self.tab_ws[..tab].iter().sum::<f32>()
			+ lay().tab_gap * tab as f32;
		Rect {
			x,
			y: self.tab_bar_y(),
			w: self.tab_ws[tab],
			h: self.tab_h(),
		}
	}
	// The strip's own clip: the gutter, less the panel's border on either side.
	fn tab_strip(&self) -> Rect {
		let gut = self.gutter_rect();
		Rect {
			x: gut.x + 1.0,
			w: (gut.w - 2.0).max(0.0),
			..gut
		}
	}
	fn rows_y0(&self) -> f32 {
		let g = self.gutter_rect();
		g.y + g.h + 1.0 + lay().tabs_gap
	}
	/// The scroll viewport in physical pixels (the render pass scissors to it).
	pub fn viewport_px(&self) -> Rect {
		self.rect_px(self.viewport())
	}
	fn viewport(&self) -> Rect {
		let y0 = self.rows_y0();
		Rect {
			x: self.rect.x,
			y: y0,
			w: self.rect.w,
			h: (self.rect.y + self.rect.h
				- lay().pad - self.btn_h()
				- lay().buttons_gap
				- self.hbar_h()
				- y0)
				.max(0.0),
		}
	}
	// The strip the sideways bar takes off the bottom of the rows, when there is
	// one. It goes above the footer's clear space rather than in it: that space is
	// what keeps a stray click off Cancel and OK.
	fn hbar_h(&self) -> f32 {
		if self.max_hscroll() > 0.0 {
			lay().scrollbar_width + lay().scrollbar_inset * 2.0
		} else {
			0.0
		}
	}
	fn content_h(&self) -> f32 {
		Self::tab_content_h(self.specs, self.tab, self.line_h, self.edited.shells.len())
	}
	fn max_scroll(&self) -> f32 {
		(self.content_h() - self.viewport().h).max(0.0)
	}

	// The smallest useful window: the three footer buttons have to fit across it,
	// and a couple of rows have to be left above them to be worth scrolling.
	fn min_size_dip(line_h: f32, btn_w: f32) -> (f32, f32) {
		let l = lay();
		let row = l.row_height.max(line_h + l.row_pad);
		(
			l.pad * 2.0 + btn_w.max(l.button_width) * 3.0 + l.button_gap * 2.0,
			Self::gutter_h_for(line_h)
				+ 1.0 + l.tabs_gap
				+ row * 2.0 + l.buttons_gap
				+ l.button_height.max(line_h + l.row_pad)
				+ l.pad,
		)
	}
	// Rows are laid out at the natural width or the window's, whichever is
	// larger: a narrower window scrolls sideways rather than truncating, a wider
	// one hands the slack to the stretchy controls.
	fn layout_w(&self) -> f32 {
		self.rect.w.max(self.natural.0)
	}
	// Left edge of the laid-out content, which is the panel's own left edge until
	// the window is too narrow to hold it.
	fn content_x(&self) -> f32 {
		self.rect.x - self.hscroll
	}
	fn max_hscroll(&self) -> f32 {
		(self.natural.0 - self.rect.w).max(0.0)
	}
	// Right edge every stretchy control ends on: the revert column's left side.
	// Fixed, so a value field and a revert arrow line up down the whole tab
	// whatever the window's width.
	fn ctl_right_full(&self) -> f32 {
		self.content_x() + self.layout_w() - lay().pad - self.revert_w - 6.0
	}
	// The same, for one row: a row sharing its line stops at the end of its part.
	fn ctl_right(&self, i: usize) -> f32 {
		if Self::packs(self.specs, i) {
			return self.control_x(i) + self.check_sz();
		}
		let (_, parts, k) = Self::line_of(self.specs, i);
		self.part_span(k, parts).1
	}
	// Part `k` of a control column split `parts` ways: its left and right edge,
	// with a gap between neighbors and none at either end of the column.
	fn part_span(&self, k: usize, parts: usize) -> (f32, f32) {
		let left = self.content_x() + lay().pad + self.label_w;
		let full = self.ctl_right_full();
		let w = ((full - left) / parts as f32).max(0.0);
		let start = if k > 0 {
			left + k as f32 * w + PAIR_GAP / 2.0
		} else {
			left
		};
		let end = if k + 1 < parts {
			left + (k + 1) as f32 * w - PAIR_GAP / 2.0
		} else {
			full
		};
		(start, end)
	}
	fn hthumb(&self) -> Option<Rect> {
		let scroll_max = self.max_hscroll();
		if scroll_max <= 0.0 {
			return None;
		}
		let track = self.htrack();
		let thumb_w = (track.w * self.rect.w / self.layout_w()).max(lay().scrollbar_thumb_min);
		Some(Rect {
			x: track.x + (self.hscroll / scroll_max) * (track.w - thumb_w).max(0.0),
			w: thumb_w,
			..track
		})
	}
	fn htrack(&self) -> Rect {
		let vp = self.viewport();
		Rect {
			x: vp.x + lay().pad,
			y: vp.y + vp.h + lay().scrollbar_inset,
			w: (vp.w - lay().pad * 2.0).max(1.0),
			h: lay().scrollbar_width,
		}
	}
	pub fn wheel(&mut self, dx_px: f32, dy_px: f32) {
		if self.modal() {
			return; // nothing behind a modal box may move under it
		}
		self.dismiss_menu();
		let (dx, dy) = (self.to_dip(dx_px), self.to_dip(dy_px));
		self.scroll = (self.scroll - dy).clamp(0.0, self.max_scroll());
		self.hscroll = (self.hscroll - dx).clamp(0.0, self.max_hscroll());
	}

	/// The size the content wants, and the smallest the window may be, both in
	/// physical pixels - the window's own limits and the resize snap read them.
	pub fn natural_size(&self) -> (f32, f32) {
		(self.to_px(self.natural.0), self.to_px(self.natural.1))
	}
	pub fn min_size(&self) -> (f32, f32) {
		let (w, h) = Self::min_size_dip(self.line_h, self.btn_w);
		(self.to_px(w), self.to_px(h))
	}
	/// The window was resized. Everything is laid out from `rect`, so this is all
	/// of it - except that a smaller window can leave either scroll offset past
	/// its new limit, and a popup placed against the old edges is stale.
	pub fn set_size(&mut self, w_px: f32, h_px: f32) {
		self.rect.w = self.to_dip(w_px).max(1.0);
		self.rect.h = self.to_dip(h_px).max(1.0);
		self.scroll = self.scroll.clamp(0.0, self.max_scroll());
		self.hscroll = self.hscroll.clamp(0.0, self.max_hscroll());
		// every gesture in flight was aimed at rects that have just moved
		self.open = None;
		self.drag = None;
		self.drag_thumb = None;
		self.drag_hthumb = None;
		self.edit_drag = None;
		self.shell_drag = None;
		if let Some(picker) = self.pick.as_mut() {
			picker.drag = None;
		}
		self.dismiss_menu();
	}
	pub fn view(&self) -> View {
		View {
			tab: self.tab,
			scroll: self.scroll,
		}
	}
	/// Open on these values instead of the live copy. The app hands in the file
	/// as it is now, since another window may have saved since this one loaded.
	pub fn start_from(&mut self, settings: Settings) {
		let settings = users_own(settings);
		self.orig = settings.clone();
		self.edited = settings;
	}
	/// Whether the desktop says when a window is minimized or covered.
	pub fn set_sees_hidden(&mut self, sees: bool) {
		self.sees_hidden = sees;
	}
	/// The main window's monitor and the size it was last given, which the
	/// automatic size shows. Both copies take the size, as a shell scan is
	/// folded, so it does not read as an edit. True when anything moved.
	pub fn follow_window(&mut self, monitor: Option<&str>, live: &Settings) -> bool {
		let kept = |s: &Settings| {
			(
				s.remembered_columns,
				s.remembered_rows,
				s.remembered_font_zoom,
			)
		};
		let moved = self.monitor.as_deref() != monitor;
		let resized =
			kept(&self.edited) != kept(live) || self.edited.monitor_sizes != live.monitor_sizes;
		if moved {
			self.monitor = monitor.map(str::to_string);
		}
		if resized {
			for settings in [&mut self.orig, &mut self.edited] {
				settings.remembered_columns = live.remembered_columns;
				settings.remembered_rows = live.remembered_rows;
				settings.remembered_font_zoom = live.remembered_font_zoom;
				settings.monitor_sizes.clone_from(&live.monitor_sizes);
			}
		}
		moved || resized
	}
	fn place(&self) -> config::auto::Place<'_> {
		config::auto::Place {
			monitor: self.monitor.as_deref(),
		}
	}
	// The auto setting a row edits, if it edits one. File or folder edits the
	// one its box shows.
	fn auto_of(&self, key: Key) -> Option<config::auto::Setting> {
		match ui().settings_of(key) {
			[path] => config::auto::by_path(path),
			_ if key == Key::BgImage => Some(if self.wallpaper_box_is_folder() {
				config::auto::Setting::WallpaperFolder
			} else {
				config::auto::Setting::WallpaperImage
			}),
			_ => None,
		}
	}
	// What an auto setting's row shows: its value, and whether that is automatic.
	fn auto_shown(&self, setting: config::auto::Setting) -> (config::auto::Value, bool) {
		(
			config::auto::value(&self.edited, setting, self.place()),
			config::auto::automatic(&self.edited, setting),
		)
	}

	/// A restored view comes from a dialog that no longer exists, so nothing about
	/// its geometry can be assumed: the UI font, screen height or field set may all
	/// have changed since. Clamp rather than trust.
	pub fn restore(&mut self, view: View) {
		if view.tab >= tab_titles().len() {
			return;
		}
		self.tab = view.tab;
		self.scroll = view.scroll.clamp(0.0, self.max_scroll());
	}
	fn thumb(&self) -> Option<Rect> {
		let scroll_max = self.max_scroll();
		if scroll_max <= 0.0 {
			return None;
		}
		let vp = self.viewport();
		let thumb_h = (vp.h * vp.h / self.content_h()).max(lay().scrollbar_thumb_min);
		Some(Rect {
			x: self.rect.x + self.rect.w - lay().scrollbar_inset - lay().scrollbar_width,
			y: vp.y + (self.scroll / scroll_max) * (vp.h - thumb_h),
			w: lay().scrollbar_width,
			h: thumb_h,
		})
	}

	// Alt-key accelerators: while Alt is held the buttons underline their first
	// letter (Cancel/Apply/OK), and Alt+that-letter triggers the button. Shift
	// (Shift+Tab) and Ctrl (Ctrl+Tab) steer keyboard focus / tab switching.
	#[cfg(test)]
	pub fn set_mods(&mut self, alt: bool, shift: bool, ctrl: bool) {
		self.set_keys(crate::input::EditKeys {
			alt,
			shift,
			shortcut: ctrl,
			word: ctrl,
			line: false,
			types: !ctrl,
		});
	}
	/// The held keys as the platform reads them (`input::edit_keys`).
	pub fn set_keys(&mut self, keys: crate::input::EditKeys) {
		self.alt = keys.alt;
		self.shift = keys.shift;
		self.ctrl = keys.shortcut;
		self.word = keys.word;
		self.line = keys.line;
		self.types = keys.types;
	}
	pub fn alt(&self) -> bool {
		self.alt
	}
	pub fn ctrl(&self) -> bool {
		self.ctrl
	}
	pub fn shift(&self) -> bool {
		self.shift
	}
	/// Takes &self so the prompt can swallow it: the footer accelerators would
	/// otherwise apply and close the dialog out from under an open theme box.
	pub fn alt_key(&self, c: char) -> Action {
		if self.modal() {
			return Action::None;
		}
		match c.to_ascii_lowercase() {
			'c' => Action::Cancel,
			'a' => Action::Apply,
			'o' => Action::Ok,
			_ => Action::None,
		}
	}

	// dropdown popup (open list; commits on Enter / click)

	// A dropdown's option list. Every one but the theme picker is fixed in the
	// declarations; that one is whatever themes exist right now, so the list has
	// to be built per call rather than borrowed from the document.
	fn dd_options(&self, i: usize) -> Vec<String> {
		match self.specs[i].kind {
			_ if self.specs[i].key == Key::Theme => {
				crate::theme::all_names(&self.edited.user_themes)
			}
			Kind::Dropdown(opts) => opts.iter().map(|o| (*o).to_string()).collect(),
			_ => Vec::new(),
		}
	}
	// What the collapsed box says. Same as the highlighted option, except that a
	// theme carrying edits is no longer that theme, so it says so instead of
	// naming a palette the colors below have moved away from. Display only - the
	// dirty state is still derived from the colors themselves (nothing is stored),
	// and the popup still highlights the theme the edits started from.
	fn dd_closed_label(&self, i: usize) -> String {
		if self.specs[i].key == Key::Theme && self.theme_dirty() {
			return UNSAVED_THEME.to_string();
		}
		let sel = self.get_radio(self.specs[i].key);
		self.dd_options(i).get(sel).cloned().unwrap_or_default()
	}

	// Open row `i`'s popup with the current value highlighted.
	fn dd_open(&mut self, i: usize) {
		self.commit_edit();
		self.open = Some(i);
		self.pending = self.get_radio(self.specs[i].key);
		self.focus = Some(Focus::Row(i, 0));
		self.scroll_focus_into_view();
	}
	// Apply the highlighted option and close (Enter / Space / click on an option).
	fn dd_commit(&mut self) {
		if let Some(i) = self.open.take() {
			self.set_radio(self.specs[i].key, self.pending);
		}
	}

	// keyboard focus + control activation

	// Rows on the active tab with at least one focusable (enabled, non-header)
	// sub-control, in visual order. (Used by the focus tests.)
	#[cfg(test)]
	fn focusables(&self) -> Vec<usize> {
		(0..self.specs.len())
			.filter(|&i| {
				self.specs[i].tab == self.tab
					&& (0..self.parts_of(i)).any(|p| !self.part_disabled(i, p))
			})
			.collect()
	}
	fn first_focus(&self) -> Option<Focus> {
		self.focus_ring().first().copied()
	}
	// The full Tab order for the active tab: each enabled sub-control (a slider's
	// track then its field, a Dual row's two checkboxes, else the single control),
	// then the three footer buttons (Cancel / Apply / OK), always reachable.
	fn focus_ring(&self) -> Vec<Focus> {
		let mut ring = Vec::new();
		for i in 0..self.specs.len() {
			if self.specs[i].tab != self.tab || Self::header_is_tab_title(&self.specs[i]) {
				continue;
			}
			for part in 0..self.parts_of(i) {
				if !self.part_disabled(i, part) {
					ring.push(Focus::Row(i, part));
				}
			}
		}
		ring.extend((0..3).map(Focus::Button));
		ring
	}
	// Sort key matching the ring above: rows in spec order, footer buttons last.
	fn focus_order(f: Focus) -> (usize, u16) {
		match f {
			Focus::Row(i, p) => (i, p),
			Focus::Button(b) => (usize::MAX, b as u16),
		}
	}
	// Tab / Shift+Tab (and Down / Up off a non-slider row): move focus to the
	// next/prev item in the ring, wrapping, and scroll a focused row into view.
	fn focus_move(&mut self, forward: bool) {
		self.commit_edit();
		self.open = None; // Tab/arrow away closes any open popup
		let ring = self.focus_ring();
		if ring.is_empty() {
			self.focus = None;
			return;
		}
		let cur = self.focus.and_then(|f| ring.iter().position(|&r| r == f));
		let n = ring.len();
		let next = match cur {
			Some(p) if forward => (p + 1) % n,
			Some(p) => (p + n - 1) % n,
			// Nothing focused, or the focused control grayed out from under us
			// (pressing Save turns Save off). Resume from where it sat rather
			// than snapping back to the top of the tab.
			None => match self.focus.map(Self::focus_order) {
				Some(k) if forward => ring
					.iter()
					.position(|&r| Self::focus_order(r) > k)
					.unwrap_or(0),
				Some(k) => ring
					.iter()
					.rposition(|&r| Self::focus_order(r) < k)
					.unwrap_or(n - 1),
				None if forward => 0,
				None => n - 1,
			},
		};
		self.focus = Some(ring[next]);
		self.scroll_focus_into_view();
		self.open_focused_field();
	}
	// A text field the keyboard walks onto opens with its value selected, the way
	// any other dialog does it: typing replaces, arrows keep. The mouse paths open
	// their own field, so this is only for focus arriving by key.
	//
	// A slider's number box opens too, and its arrows then split the way a numeric
	// field's do anywhere else: Up / Down keep stepping the value (key_vertical
	// runs whether the field is open or not), Left / Right move the caret.
	fn open_focused_field(&mut self) {
		let Some(Focus::Row(i, part)) = self.focus else {
			return;
		};
		if self.disabled(self.part_key(i, part)) {
			return;
		}
		match self.specs[i].kind {
			// part 0 is the chip, which opens the picker rather than a field
			Kind::Color if part == 0 => {}
			Kind::Text | Kind::Color | Kind::Slider { .. } => self.open_edit(i, true),
			Kind::ShellList => match shell_stop(part, self.edited.shells.len()) {
				ShellStop::Entry(shell_index, ShellPart::Name) => {
					self.open_edit(shell_field_row(shell_index, false), true);
				}
				ShellStop::Entry(shell_index, ShellPart::Command) => {
					self.open_edit(shell_field_row(shell_index, true), true);
				}
				_ => {}
			},
			_ => {}
		}
	}
	// Scroll the rows region so a focused control row is fully visible (buttons
	// are fixed chrome - always visible).
	fn scroll_focus_into_view(&mut self) {
		let Some(Focus::Row(i, part)) = self.focus else {
			return;
		};
		let vp = self.viewport();
		// The shells grid is one row and many lines, and a long one is taller
		// than the viewport - so it is the focused CONTROL that has to come into
		// view there, not the row, which may not fit at all.
		let (top, bottom) = if matches!(self.specs[i].kind, Kind::ShellList) {
			let r = self.shell_stop_rect(i, part);
			(r.y - 4.0, r.y + r.h + 4.0)
		} else {
			let top = self.row_y(i);
			(top, top + self.row_screen_h(i))
		};
		if top < vp.y {
			self.scroll -= vp.y - top; // row above viewport -> scroll it down into view
		} else if bottom > vp.y + vp.h {
			self.scroll += bottom - (vp.y + vp.h); // row below -> scroll up
		}
		self.scroll = self.scroll.clamp(0.0, self.max_scroll());
		// and sideways, for a window too narrow to hold the whole row
		if self.max_hscroll() > 0.0 {
			let ctl = self.focus_ctl_rect(i, part);
			if ctl.x < vp.x {
				self.hscroll -= vp.x - ctl.x;
			} else if ctl.x + ctl.w > vp.x + vp.w {
				self.hscroll += ctl.x + ctl.w - (vp.x + vp.w);
			}
			self.hscroll = self.hscroll.clamp(0.0, self.max_hscroll());
		}
	}
	// Ctrl+Tab / Ctrl+Shift+Tab: cycle the active tab, focusing its first control.
	fn tab_switch(&mut self, forward: bool) {
		self.commit_edit();
		self.capture_end();
		self.open = None;
		let n = self.tab_ws.len();
		if n == 0 {
			return;
		}
		self.tab = if forward {
			(self.tab + 1) % n
		} else {
			(self.tab + n - 1) % n
		};
		self.scroll = 0.0;
		self.hscroll = 0.0;
		self.drag = None;
		self.focus = self.first_focus();
		self.open_focused_field();
	}
	/// The Tab key: Ctrl switches tabs, otherwise walk control focus (Shift = back).
	pub fn key_tab(&mut self) {
		self.dismiss_menu();
		if self.pick.is_some() {
			self.pick_focus_move(!self.shift);
			return;
		}
		if self.prompt.is_some() {
			self.prompt_focus_move(!self.shift);
			return;
		}
		if self.ctrl {
			self.tab_switch(!self.shift);
		} else {
			self.focus_move(!self.shift);
		}
	}
	/// Ctrl+PageUp / Ctrl+PageDown cycle the active tab (PageDown = next).
	pub fn key_page(&mut self, forward: bool) {
		if self.ctrl {
			self.switch_tab(forward);
		}
	}
	/// The next or previous tab, unless a box is up over the dialog.
	pub fn switch_tab(&mut self, forward: bool) {
		if !self.modal() {
			self.tab_switch(forward);
		}
	}
	/// Up / Down arrows: navigate an open popup, else Alt+Down opens a focused
	/// dropdown, else step a focused numeric slider (spinbox feel), else walk control
	/// focus (a peer of Tab).
	pub fn key_vertical(&mut self, forward: bool) {
		if self.pick.is_some() && self.emenu.is_none() {
			self.pick_arrow(if forward { 1 } else { -1 }, true);
			return;
		}
		if self.prompt.is_some() && self.emenu.is_none() {
			self.prompt_focus_move(forward);
			return;
		}
		if self.emenu.is_some() {
			// walk the field context-menu items (wraps)
			let n = EDIT_MENU.len() as i32;
			if let Some(menu) = &mut self.emenu {
				let step = if forward { 1 } else { -1 };
				let cur = menu
					.hover
					.map_or(if forward { -1 } else { 0 }, |h| h as i32);
				menu.hover = Some((cur + step).rem_euclid(n) as usize);
			}
			return;
		}
		if let Some(i) = self.open {
			let n = self.dd_options(i).len();
			if n > 0 {
				let step = if forward { 1 } else { -1 };
				self.pending = (self.pending as i32 + step).rem_euclid(n as i32) as usize;
			}
			return;
		}
		if forward && self.alt {
			if let Some(Focus::Row(i, _)) = self.focus {
				if matches!(self.specs[i].kind, Kind::Dropdown(_))
					&& !self.disabled(self.specs[i].key)
				{
					self.dd_open(i);
					return;
				}
			}
		}
		// Up/Down step a focused numeric field (spinbox feel; Shift = 10x). Tab still
		// walks between controls. Works whether the field is just focused or open.
		// forward = Down (decrease); !forward = Up (increase).
		if let Some(Focus::Row(i, _)) = self.focus {
			if matches!(self.specs[i].kind, Kind::Slider { .. })
				&& !self.disabled(self.specs[i].key)
			{
				self.step_slider(i, if forward { -1 } else { 1 }, self.shift);
				return;
			}
		}
		self.focus_move(forward);
	}
	// Adjust a focused/open slider by one arrow step (dir = +1/-1, Shift = 10x). When
	// the field is open for editing, its buffer is refreshed to the new value and
	// fully selected, so continued stepping and a following commit see the number.
	fn step_slider(&mut self, i: usize, dir: i32, shift: bool) {
		let Some(scale) = SliderScale::of(&self.specs[i].kind) else {
			return;
		};
		let key = self.specs[i].key;
		if self.disabled(key) {
			return;
		}
		self.set_f32(key, scale.stepped(self.get_f32(key), dir, shift));
		if self.edit.as_ref().is_some_and(|e| e.row == i) {
			let buf = self.fmt_val(key, scale.int);
			if let Some(edit) = &mut self.edit {
				edit.cur = buf.len();
				edit.sel = (!buf.is_empty()).then_some(0);
				edit.buf = buf;
				edit.view_to = 0.0;
			}
		}
	}
	/// Left / Right: caret motion while a field is being edited, otherwise adjust
	/// the focused slider (by one step) or move a focused radio's selection.
	pub fn key_horizontal(&mut self, dir: i32) {
		self.dismiss_menu();
		// in the picker, a value box owns Left/Right (caret) and everything else
		// takes them as adjustment or a focus move
		if let Some(picker) = self.pick.as_ref() {
			if matches!(picker.focus, pick::Focus::Field(_)) && self.edit.is_some() {
				if dir < 0 {
					self.cursor_left();
				} else {
					self.cursor_right();
				}
			} else {
				self.pick_arrow(dir, false);
			}
			return;
		}
		// in the prompt box, the field owns Left/Right while it has focus and the
		// two buttons share them otherwise
		if let Some(prompt) = self.prompt.as_ref() {
			if prompt.focus != PromptFocus::Field {
				self.prompt_focus_move(dir > 0);
				return;
			}
		}
		if self.edit.is_some() {
			if dir < 0 {
				self.cursor_left();
			} else {
				self.cursor_right();
			}
			return;
		}
		if self.open.is_some() {
			return; // an open popup owns arrow keys (Up/Down navigate it)
		}
		let Some(Focus::Row(i, _)) = self.focus else {
			return;
		};
		let key = self.specs[i].key;
		if self.disabled(key) {
			return;
		}
		match self.specs[i].kind {
			Kind::Slider { .. } => self.step_slider(i, dir, self.shift),
			// closed dropdown: Left/Right nudge the value without opening (combobox feel)
			Kind::Radio(options) => {
				let sel = self.get_radio(key) as i32;
				let new_sel = (sel + dir).clamp(0, options.len() as i32 - 1);
				self.set_radio(key, new_sel as usize);
			}
			Kind::Dropdown(_) => {
				let n = self.dd_options(i).len() as i32;
				if n > 0 {
					let sel = self.get_radio(key) as i32;
					self.set_radio(key, (sel + dir).clamp(0, n - 1) as usize);
				}
			}
			_ => {}
		}
	}
	/// Space: type into an active edit, activate a focused button, else activate the
	/// focused control - flip a toggle or open a text/color field for editing.
	pub fn key_space(&mut self) -> Action {
		if self.pick.is_some() {
			self.pick_activate();
			return Action::None;
		}
		if let Some(prompt) = self.prompt.as_ref() {
			match prompt.focus {
				PromptFocus::Cancel => self.prompt_close(),
				PromptFocus::Ok => self.prompt_accept(),
				PromptFocus::Field => self.char_input(' '),
			}
			return Action::None;
		}
		if self.open.is_some() {
			self.dd_commit(); // Space picks the highlighted option
			return Action::None;
		}
		if self.edit.is_some() {
			self.char_input(' ');
			return Action::None;
		}
		let (i, part) = match self.focus {
			Some(Focus::Button(b)) => return self.buttons()[b].0,
			Some(Focus::Row(i, part)) => (i, part),
			None => return Action::None,
		};
		let key = self.part_key(i, part);
		if self.disabled(key) {
			return Action::None;
		}
		match self.specs[i].kind {
			// flip the focused checkbox (for Dual, key is that part's key)
			Kind::Toggle | Kind::Dual { .. } => self.set_toggle(key, !self.get_toggle(key)),
			// the chip opens the picker; every other field opens pre-filled with
			// the current value, fully selected (standard field-entry: typing
			// replaces, arrows keep it)
			Kind::Color if part == 0 => self.pick_open(i),
			Kind::Text | Kind::Color | Kind::Slider { .. } => self.open_edit(i, true),
			Kind::Dropdown(_) => self.dd_open(i),
			Kind::Buttons(_) => self.row_button(i, part),
			Kind::ShellList => self.shell_activate(i, part),
			Kind::Hotkey(_) => self.capture_start(i),
			_ => {}
		}
		Action::None
	}

	// Current value of row i's editable field, as text.
	fn edit_buf(&self, i: usize) -> String {
		if i == PROMPT_ROW {
			return self.edit.as_ref().map_or(String::new(), |e| e.buf.clone());
		}
		if let Some(f) = pick_field_of(i) {
			return self
				.pick
				.as_ref()
				.map_or_else(String::new, |p| f.text(p.hsv));
		}
		if let Some((shell_index, command)) = shell_field_of(i) {
			return self
				.edited
				.shells
				.get(shell_index)
				.map_or_else(String::new, |entry| {
					if command {
						entry.command.clone()
					} else {
						entry.title.clone()
					}
				});
		}
		match self.specs[i].kind {
			Kind::Text => self.get_text(self.specs[i].key),
			Kind::Color => {
				let c = self.get_col(self.specs[i].key);
				format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
			}
			Kind::Slider { int, .. } => self.fmt_val(self.specs[i].key, int),
			_ => String::new(),
		}
	}
	// Open row i's field for editing; select_all puts the whole value under the
	// selection so the next keystroke replaces it.
	fn open_edit(&mut self, i: usize, select_all: bool) {
		let mut edit = EditState::new(i, self.edit_buf(i));
		edit.sel = (select_all && edit.cur > 0).then_some(0);
		edit.wallpaper_folder = self.wallpaper_box_is_folder();
		self.edit = Some(edit);
	}

	/// Panel size (used to size a dedicated dialog window when the panel is laid
	/// out at the origin - `new(0.0, 0.0, ...)`).
	/// Window size in physical pixels.
	pub fn size(&self) -> (f32, f32) {
		(self.to_px(self.rect.w), self.to_px(self.rect.h))
	}

	pub fn edited(&self) -> &Settings {
		&self.edited
	}
	pub fn orig(&self) -> &Settings {
		&self.orig
	}

	/// After an Apply, make the applied values the new baseline so a later Apply
	/// compares against the live state, not the stale open-time snapshot (otherwise
	/// re-selecting the original value reads as "no change" and isn't applied).
	pub fn commit_baseline(&mut self) {
		self.orig = self.edited.clone();
	}

	// Top of row `i` on the active tab (scrolled). Walks visible rows the same
	// way tab_content_h does so heights and header gaps stay in sync.
	fn row_y(&self, i: usize) -> f32 {
		let first = self.rows_y0() - self.scroll;
		let shells = self.edited.shells.len();
		let walk = (self.tab, first.to_bits(), self.line_h.to_bits(), shells);
		let mut cache = self.row_tops.borrow_mut();
		if cache.walked != Some(walk) {
			self.walk_rows(&mut cache, first, shells);
			cache.walked = Some(walk);
		}
		cache.tops.get(i).copied().unwrap_or(cache.end)
	}
	fn walk_rows(&self, cache: &mut RowTops, first: f32, shells: usize) {
		#[cfg(test)]
		ROW_WALKS.with(|n| n.set(n.get() + 1));
		let mut drawn = vec![false; self.specs.len()];
		cache.tops.clear();
		cache.tops.resize(self.specs.len(), 0.0);
		let mut y = first;
		let mut prev: Option<&Spec> = None;
		for (j, spec) in Self::visible(self.specs, self.tab) {
			y += Self::gap_above(self.specs, j, self.tab, prev);
			cache.tops[j] = y;
			drawn[j] = true;
			y += Self::row_advance(self.specs, j, self.tab, self.line_h, shells);
			prev = Some(spec);
		}
		cache.end = y;
		for (top, drawn) in cache.tops.iter_mut().zip(drawn) {
			if !drawn {
				*top = y;
			}
		}
	}

	// hotkey rows

	fn hotkey_of(&self, i: usize) -> Option<Hotkey> {
		match self.specs[i].kind {
			Kind::Hotkey(hotkey) => Some(hotkey),
			_ => None,
		}
	}
	// The hotkey a row's setting is, found through the declarations so the two
	// cannot disagree.
	fn hotkey_for_key(&self, key: Key) -> Option<Hotkey> {
		let at = self.specs.iter().position(|spec| spec.key == key)?;
		self.hotkey_of(at)
	}
	// The name a hotkey goes by on its own row, for a note on another one.
	fn hotkey_label(&self, hotkey: Hotkey) -> &'static str {
		self.specs
			.iter()
			.find(|spec| matches!(spec.kind, Kind::Hotkey(each) if each == hotkey))
			.map_or("", |spec| spec.label)
	}
	// What a hotkey row's box shows: its chords, whether it has none, and what
	// is worth saying about a chord it lost or took. A lost chord is said the way
	// the launch says it about the file.
	fn hotkey_text(&self, i: usize) -> (String, bool, String) {
		let Some(hotkey) = self.hotkey_of(i) else {
			return (String::new(), false, String::new());
		};
		if self.capture == Some(i) {
			let why = self.capture_refused.as_deref().unwrap_or(CAPTURE_PROMPT);
			return (String::new(), false, why.into());
		}
		let keys = &self.edited.keys;
		let chords = keys.chords(hotkey);
		let shown = if chords.is_empty() {
			"Off".to_string()
		} else {
			chords
				.iter()
				.map(|chord| chord.spoken(self.mac))
				.collect::<Vec<_>>()
				.join(" or ")
		};
		let mut notes = Vec::new();
		for taken in keys.taken() {
			let spoken = taken.chord.spoken(self.mac);
			if taken.from == hotkey {
				notes.push(format!(
					"{spoken} is set for {}",
					self.hotkey_label(taken.by)
				));
			} else if taken.by == hotkey && taken.both_set {
				notes.push(format!(
					"{spoken} is also set for {}",
					self.hotkey_label(taken.from)
				));
			} else if taken.by == hotkey {
				notes.push(format!(
					"took {spoken} from {}",
					self.hotkey_label(taken.from)
				));
			}
		}
		for (from, chord, by) in &self.moved {
			if *from == hotkey && keys.chords(*by).contains(chord) {
				notes.push(format!(
					"{} is set for {}",
					chord.spoken(self.mac),
					self.hotkey_label(*by)
				));
			}
		}
		(shown, chords.is_empty(), notes.join("; "))
	}
	// Wait for the next chord on a hotkey row. Every key goes to it until one is
	// taken, so nothing starts this but a deliberate press or click on the box.
	fn capture_start(&mut self, i: usize) {
		self.commit_edit();
		self.open = None;
		self.focus = Some(Focus::Row(i, 0));
		self.capture = Some(i);
		self.capture_refused = None;
	}
	fn capture_end(&mut self) {
		self.capture = None;
		self.capture_refused = None;
	}
	/// A key press while a hotkey row waits. Escape on its own leaves the row as
	/// it was, and Backspace or Delete on its own turns the hotkey off. Answers
	/// whether the press was taken, so the caller sends it nowhere else.
	pub fn capture_key(&mut self, key: &winit::keyboard::Key, mods: ModifiersState) -> bool {
		use winit::keyboard::{Key as Pressed, NamedKey};
		let Some(i) = self.capture else {
			return false;
		};
		let Some(hotkey) = self.hotkey_of(i) else {
			self.capture_end();
			return false;
		};
		let bare = !(mods.control_key() || mods.alt_key() || mods.shift_key() || mods.super_key());
		match key {
			Pressed::Named(NamedKey::Escape) if bare => self.capture_end(),
			Pressed::Named(NamedKey::Backspace | NamedKey::Delete) if bare => {
				self.set_hotkey(hotkey, Vec::new());
				self.capture_end();
			}
			_ => match crate::keys::press(key, mods, self.mac) {
				crate::keys::Press::Held => {}
				crate::keys::Press::Refused(why) => self.capture_refused = Some(why),
				crate::keys::Press::Chord(chord) => {
					self.set_hotkey(hotkey, vec![chord]);
					self.capture_end();
				}
			},
		}
		true
	}
	// Give a hotkey new chords of its own. A chord another hotkey's own value
	// has is taken off that one, so the latest press wins; one another has only
	// by default leaves it the way the file would make it.
	fn set_hotkey(&mut self, hotkey: Hotkey, chords: Vec<Chord>) {
		self.moved.retain(|(from, ..)| *from != hotkey);
		let mut keys = self.edited.keys.clone();
		for &chord in &chords {
			for (other, _) in crate::keys::config_paths() {
				let Some(own) = keys.own(other).filter(|_| other != hotkey) else {
					continue;
				};
				if !own.contains(&chord) {
					continue;
				}
				let rest = own.iter().copied().filter(|each| *each != chord).collect();
				keys = keys.with_own(other, Some(rest));
				self.moved.push((other, chord, hotkey));
			}
		}
		self.edited.keys = keys.with_own(hotkey, Some(chords));
	}

	// The warning mark: just after the label's text, centered in the row.
	fn warning_box(&self, i: usize, measure: &mut impl FnMut(&str) -> f32) -> Rect {
		let h = self.line_h * WARN_H;
		Rect {
			x: self.label_x(i) + measure(self.specs[i].label) + self.line_h * WARN_GAP,
			y: self.centered_in_row(i, h),
			w: self.line_h * WARN_W,
			h,
		}
	}
	// Left edge of a row's label: its own sub-group depth in from the panel pad.
	fn label_x(&self, i: usize) -> f32 {
		if Self::packs(self.specs, i) {
			return self.packed_x(i).0;
		}
		if self.specs[i].beside {
			// part of a shared line has no label column: its label starts its part
			let (_, parts, k) = Self::line_of(self.specs, i);
			return self.part_span(k, parts).0;
		}
		self.content_x() + lay().pad + f32::from(self.specs[i].indent) * lay().indent
	}
	// Where row `i`'s controls start. A packed line puts each box right after its
	// label, bar a first box that keeps the column. Any other shared line splits
	// the control column evenly, and a part with a label of its own puts the
	// control after it.
	fn control_x(&self, i: usize) -> f32 {
		if Self::packs(self.specs, i) {
			return self.packed_x(i).1;
		}
		let (_, parts, k) = Self::line_of(self.specs, i);
		let start = self.part_span(k, parts).0;
		if self.specs[i].beside && !self.specs[i].label.is_empty() {
			start
				+ self.label_ws.get(i).copied().unwrap_or(0.0)
				+ font_gap(PART_LABEL_GAP, self.line_h)
		} else {
			start
		}
	}
	// Label and box of row `i` on a packed line. The line starts where its first
	// row's label would, indent and all. Where the first box keeps the column,
	// the rest move along with it.
	fn packed_x(&self, i: usize) -> (f32, f32) {
		let (lead, _, k) = Self::line_of(self.specs, i);
		let start =
			self.content_x() + lay().pad + f32::from(self.specs[lead].indent) * lay().indent;
		let (label, ctl) = Self::packed_at(self.specs, lead, k, &self.label_ws, self.line_h);
		if !Self::lead_keeps_column(self.specs, lead) {
			return (start + label, start + ctl);
		}
		let column = self.content_x() + lay().pad + self.label_w;
		if k == 0 {
			return (start, column);
		}
		let first_box = Self::packed_at(self.specs, lead, 0, &self.label_ws, self.line_h).1;
		let shift = column - (start + first_box);
		(start + label + shift, start + ctl + shift)
	}
	// Top of a control `h` tall, centered in row `i`'s line.
	fn centered_in_row(&self, i: usize, h: f32) -> f32 {
		self.row_y(i) + (self.line_row_h() - h) / 2.0
	}
	// A heading's faint rule, near the bottom of its tall row, leaving a clear
	// gap below the heading text above it.
	fn header_rule_y(&self, i: usize) -> f32 {
		self.row_y(i) + self.row_h(&Kind::Header("")) - 8.0
	}
	fn track(&self, i: usize) -> Rect {
		let x = self.control_x(i);
		Rect {
			x,
			y: self.centered_in_row(i, 6.0),
			w: (self.ctl_right(i) - self.value_w - 14.0 - x).max(lay().slider_width / 4.0),
			h: 6.0,
		}
	}
	// The color chip stands as tall as the hex field beside it, so the pair reads
	// as one control rather than two of different sizes.
	fn swatch(&self, i: usize) -> Rect {
		let h = self.field_h();
		Rect {
			x: self.control_x(i),
			y: self.centered_in_row(i, h),
			w: h,
			h,
		}
	}
	fn hexbox(&self, i: usize) -> Rect {
		let h = self.field_h();
		let x = self.control_x(i) + h + 8.0;
		Rect {
			x,
			y: self.centered_in_row(i, h),
			w: (self.ctl_right(i) - x).max(lay().hex_width),
			h,
		}
	}
	// editable numeric field to the right of a slider (shows/edits the value)
	fn valbox(&self, i: usize) -> Rect {
		let h = self.field_h();
		Rect {
			x: self.ctl_right(i) - self.value_w,
			y: self.centered_in_row(i, h),
			w: self.value_w,
			h,
		}
	}
	// wide editable field (background-image path), control_x -> the revert column
	fn textbox(&self, i: usize) -> Rect {
		let x = self.control_x(i);
		let h = self.field_h();
		Rect {
			x,
			y: self.centered_in_row(i, h),
			w: (self.ctl_right(i) - x).max(lay().value_width),
			h,
		}
	}
	// The field an auto setting's row shows its value in, if row `i` edits one.
	fn auto_field(&self, i: usize) -> Option<(config::auto::Setting, Rect)> {
		let setting = self.auto_of(self.specs[i].key)?;
		match self.specs[i].kind {
			Kind::Slider { .. } => Some((setting, self.valbox(i))),
			Kind::Text => Some((setting, self.textbox(i))),
			// the hex box, beside the chip that opens the picker
			Kind::Color => Some((setting, self.hexbox(i))),
			_ => None,
		}
	}
	// The automatic mark, or once set by hand the icon that clears it, at the
	// right end of an auto setting's field. Not drawn while the field is open,
	// where the caret needs the room.
	fn auto_slot(&self, i: usize) -> Option<(config::auto::Setting, Rect)> {
		if matches!(&self.edit, Some(edit) if edit.row == i) {
			return None;
		}
		let (setting, field) = self.auto_field(i)?;
		let side = (self.line_h * 0.6).round().max(8.0);
		Some((
			setting,
			Rect {
				x: field.x + field.w - lay().field_pad / 2.0 - side,
				y: field.y + ((field.h - side) / 2.0).round(),
				w: side,
				h: side,
			},
		))
	}
	// The part of row `i`'s field its text may use: all of it, less the slot.
	fn text_room(&self, i: usize, field: Rect) -> Rect {
		match self.auto_slot(i) {
			Some((_, slot)) => Rect {
				w: (slot.x - 2.0 - field.x).max(0.0),
				..field
			},
			None => field,
		}
	}
	// The state of the group row `i` is the switch for, if it is one.
	fn group_state(&self, i: usize) -> Option<config::auto::State> {
		self.specs[i]
			.group
			.map(|group| config::auto::group_state(&self.edited, group))
	}
	// Whether row `i` shows an automatic value, read the the way a placeholder is.
	fn shows_automatic(&self, i: usize) -> bool {
		self.auto_slot(i)
			.is_some_and(|(setting, _)| config::auto::automatic(&self.edited, setting))
	}
	// The letter on the automatic mark.
	fn auto_mark_text(
		&self,
		colors: &Dlg,
		i: usize,
		line_h: f32,
		vp: Rect,
		out: &mut Vec<TextItem>,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		const MARK_SCALE: f32 = 0.7;
		let Some((_, r)) = self.auto_slot(i).filter(|_| self.shows_automatic(i)) else {
			return;
		};
		let w = measure(AUTO_MARK) * MARK_SCALE;
		out.push(TextItem {
			bold: true,
			scale: MARK_SCALE,
			clip: Some(vp),
			..TextItem::plain(
				AUTO_MARK.to_string(),
				r.x + (r.w - w) / 2.0,
				r.y + (r.h - line_h * MARK_SCALE) / 2.0,
				colors.panel_bg,
			)
		});
	}
	fn auto_slot_quads(&self, colors: &Dlg, i: usize, out: &mut Vec<RectInstance>) {
		let Some((setting, r)) = self.auto_slot(i) else {
			return;
		};
		let (color, params) = if config::auto::automatic(&self.edited, setting) {
			(colors.dim, [QuadMode::Rounded.code(), (r.w * 0.3).round()])
		} else {
			(
				colors.text,
				[QuadMode::CloseMark.code(), (r.w * 0.14).max(1.2)],
			)
		};
		out.push(RectInstance {
			pos: [r.x, r.y],
			size: [r.w, r.h],
			color: config::srgb_f32(color),
			params,
		});
	}
	// right-edge revert-to-default icon for row `i`
	fn revert_box(&self, i: usize) -> Rect {
		let h = self.check_sz();
		Rect {
			x: self.content_x() + self.layout_w() - lay().pad - self.revert_w,
			y: self.centered_in_row(i, h),
			w: self.revert_w,
			h,
		}
	}
	fn check_sz(&self) -> f32 {
		square_box(lay().swatch, self.line_h)
	}
	fn checkbox(&self, i: usize) -> Rect {
		let size = self.check_sz();
		Rect {
			x: self.control_x(i),
			y: self.centered_in_row(i, size),
			w: size,
			h: size,
		}
	}
	fn dual_pitch(&self) -> f32 {
		lay().dual_pitch * self.ui_scale()
	}
	// checkbox `part` (0/1) on a Dual row; its label sits just to the right
	fn dual_box(&self, i: usize, part: u16) -> Rect {
		let size = self.check_sz();
		Rect {
			x: self.control_x(i) + part as f32 * self.dual_pitch(),
			y: self.centered_in_row(i, size),
			w: size,
			h: size,
		}
	}
	// Radio geometry scales with the UI font (HiDPI or a large desktop font), so
	// multi-option labels don't collide the way fixed 96px pitch does at 2x.
	fn ui_scale(&self) -> f32 {
		(self.line_h / lay().base_line_height).max(1.0)
	}
	fn radio_pitch(&self) -> f32 {
		lay().radio_pitch * self.ui_scale()
	}
	fn radio_box_sz(&self) -> f32 {
		square_box(lay().radio_box, self.line_h)
	}
	// Where the label after a radio or pair box starts.
	fn label_after(&self, bx: Rect) -> f32 {
		bx.x + bx.w + font_gap(PART_LABEL_GAP, self.line_h)
	}
	// indicator box for radio option `choice` in row `i`
	fn radio_box(&self, i: usize, choice: usize) -> Rect {
		let size = self.radio_box_sz();
		Rect {
			x: self.control_x(i) + choice as f32 * self.radio_pitch(),
			y: self.centered_in_row(i, size),
			w: size,
			h: size,
		}
	}
	// Collapsed dropdown box (the always-visible control): shows the current option
	// + a down-arrow; clicking it opens the popup list.
	fn dd_box(&self, i: usize) -> Rect {
		let h = self.field_h();
		let x = self.control_x(i);
		let shares_a_line = self.specs[i].beside || Self::pairs_down(self.specs, i, self.tab);
		let floor = if shares_a_line {
			lay().dropdown_pair_width
		} else {
			lay().dropdown_width
		};
		Rect {
			x,
			y: self.centered_in_row(i, h),
			w: (self.ctl_right(i) - x).max(floor * self.ui_scale()),
			h,
		}
	}
	// One option row inside the open popup.
	fn dd_item_h(&self) -> f32 {
		(self.line_h + lay().dropdown_item_pad).max(lay().dropdown_item_min)
	}
	// The open popup box. Opens downward from the collapsed box, or upward when that
	// would spill past the viewport bottom (so a dropdown low in a scrolled tab still
	// shows all its options).
	fn dd_popup(&self, i: usize, n: usize) -> Rect {
		let boxr = self.dd_box(i);
		let h = n as f32 * self.dd_item_h();
		let vp = self.viewport();
		let down_y = boxr.y + boxr.h;
		let y = if down_y + h <= vp.y + vp.h || boxr.y - h < vp.y {
			down_y
		} else {
			boxr.y - h
		};
		Rect {
			x: boxr.x,
			y,
			w: boxr.w,
			h,
		}
	}
	fn dd_item_rect(&self, i: usize, n: usize, choice: usize) -> Rect {
		let popup = self.dd_popup(i, n);
		Rect {
			x: popup.x,
			y: popup.y + choice as f32 * self.dd_item_h(),
			w: popup.w,
			h: self.dd_item_h(),
		}
	}
	// Number of focusable sub-controls in row `i` (0 for a header). Sliders and
	// the Dual (cursor) row expose two; every other control is a single part.
	fn parts_of(&self, i: usize) -> u16 {
		match self.specs[i].kind {
			Kind::Header(_) => 0,
			// the chip (which opens the picker) and the hex field beside it
			Kind::Slider { .. } | Kind::Dual { .. } | Kind::Color => 2,
			Kind::Buttons(captions) => captions.len() as u16,
			// every entry's own controls, then the one Add stop past the end
			Kind::ShellList => self.edited.shells.len() as u16 * ShellPart::COUNT + 1,
			_ => 1,
		}
	}
	// The config Key that governs `part` of row `i` (Dual parts differ; every
	// other kind uses the row's single key for both the value and its graying).
	fn part_key(&self, i: usize, part: u16) -> Key {
		match self.specs[i].kind {
			Kind::Dual { keys, .. } => keys[part as usize],
			_ => self.specs[i].key,
		}
	}
	// A push-button decides for itself (there is nothing to gate on); everything
	// else asks the setting behind it.
	fn part_disabled(&self, i: usize, part: u16) -> bool {
		match self.specs[i].kind {
			Kind::Buttons(_) => {
				assoc_of(self.specs[i].key).is_none() && !self.theme_btn_enabled(ThemeBtn::of(part))
			}
			// Nothing in the grid is ever grayed: every stop is a value the user
			// can always edit, and reordering left the keyboard with the arrows.
			Kind::ShellList => false,
			_ => self.disabled(self.part_key(i, part)),
		}
	}
	// Flyover text for a control the environment disables rather than another
	// setting - explains why it is inert. The hidden wait, where the desktop
	// never says a window is hidden.
	fn disabled_tip(&self, key: Key) -> Option<&'static str> {
		match key {
			Key::IdleHiddenMin => hidden_wait_tip(self.sees_hidden),
			_ => None,
		}
	}
	// What row `i` says about `key`, one of its settings. Why a control is
	// grayed wins over what it does - that is the more urgent question when it
	// is - and a value the profile set says so before the usual text.
	// The tip ends with the value in use and the shipped one, where they differ
	// (the automatic settings design). An auto setting's state line says that
	// already, so it gets none.
	fn row_tip(&self, i: usize, key: Key) -> Option<Cow<'static, str>> {
		let text = self.row_tip_text(i, key);
		let Some((now, default)) = self
			.value_pair(i, key)
			.filter(|(now, default)| now != default)
		else {
			return text;
		};
		let values = format!("Current value: {now}\nDefault value: {default}");
		Some(Cow::Owned(match text {
			Some(text) => format!("{text}\n\n{values}"),
			None => values,
		}))
	}
	// What row `i` shows for `key` and what it shows by default, written the
	// way the row writes them. None for an auto setting, a group's switch, and
	// anything that holds no value.
	fn value_pair(&self, i: usize, key: Key) -> Option<(String, String)> {
		if self.auto_of(key).is_some() || self.specs[i].group.is_some() {
			return None;
		}
		let (now, default) = (self.shown(key), &self.defaults);
		let on_off = |on: bool| if on { "On" } else { "Off" }.to_string();
		Some(match self.specs[i].kind {
			Kind::Slider { int, .. } => (
				fmt_number(self.get_f32(key), int),
				fmt_number(self.default_f32(key), int),
			),
			Kind::Toggle | Kind::Dual { .. } => {
				(on_off(toggle_of(now, key)), on_off(toggle_of(default, key)))
			}
			Kind::Radio(_) | Kind::Dropdown(_) if key == Key::Theme => {
				(self.edited.theme.clone(), default.theme.clone())
			}
			Kind::Radio(options) | Kind::Dropdown(options) => {
				let option = |settings| {
					options
						.get(radio_of(settings, key))
						.map_or_else(String::new, |o| (*o).to_string())
				};
				(option(now), option(default))
			}
			Kind::Color => (
				config::format_hex(self.get_col(key)),
				config::format_hex(self.default_col(key)),
			),
			Kind::Text => {
				let text = |settings: &Settings| match key {
					Key::StartupDirectory if !settings.startup_directory.is_empty() => {
						settings.startup_directory.clone()
					}
					_ => "(none)".to_string(),
				};
				(text(now), text(default))
			}
			Kind::Hotkey(_) => {
				let hotkey = self.hotkey_for_key(key)?;
				let spoken = |keys: &crate::keys::Bindings| {
					let chords = keys.chords(hotkey);
					if chords.is_empty() {
						return "Off".to_string();
					}
					chords
						.iter()
						.map(|chord| chord.spoken(self.mac))
						.collect::<Vec<_>>()
						.join(" or ")
				};
				(spoken(&self.edited.keys), spoken(&default.keys))
			}
			Kind::Header(_) | Kind::Buttons(_) | Kind::ShellList => return None,
		})
	}
	fn row_tip_text(&self, i: usize, key: Key) -> Option<Cow<'static, str>> {
		if let Some(why) = self.disabled_tip(key).filter(|_| self.disabled(key)) {
			return Some(Cow::Borrowed(why));
		}
		if self.profile_shows(key) {
			return Some(Cow::Borrowed(PROFILE_TIP));
		}
		let help = self.specs[i].help;
		// an auto setting adds a line on whether it is automatic
		let Some(setting) = self.auto_of(key) else {
			return Some(Cow::Borrowed(help)).filter(|help| !help.is_empty());
		};
		let state = if config::auto::automatic(&self.edited, setting) {
			Cow::Borrowed(AUTO_TIP)
		} else {
			let rule = config::auto::rule(&self.edited, setting, self.place());
			// written the way the row's own box writes a number
			let rule = match self.specs[i].kind {
				Kind::Slider { int, .. } => fmt_number(config::auto::number(&rule), int),
				_ => rule.to_string(),
			};
			Cow::Owned(format!("Set by hand. Automatic would be: {rule}."))
		};
		Some(if help.is_empty() {
			state
		} else {
			Cow::Owned(format!("{help}\n\n{state}"))
		})
	}
	// The flyover to show while the cursor rests on something that has one:
	// (text, anchor rect to hang the tip box under).
	fn hover_tip_dip(&self, mx: f32, my: f32) -> Option<(Cow<'static, str>, Rect)> {
		if self.modal() {
			return None; // the box covers the panel; nothing behind it answers
		}
		for (action, r, _) in self.buttons() {
			if r.contains(mx, my) {
				let help = &ui().help;
				return Some((
					Cow::Borrowed(match action {
						Action::Cancel => help.cancel,
						Action::Apply => help.apply,
						_ => help.ok,
					}),
					r,
				));
			}
		}
		let vp = self.viewport();
		if !vp.contains(mx, my) {
			return None;
		}
		for i in 0..self.specs.len() {
			if self.specs[i].tab != self.tab || matches!(self.specs[i].kind, Kind::Header(_)) {
				continue;
			}
			let arrow = self.revert_box(i);
			if self.has_revert(i) && !self.specs[i].revert_help.is_empty() && arrow.contains(mx, my)
			{
				return Some((Cow::Borrowed(self.specs[i].revert_help), arrow));
			}
			// the icon that clears a value set by hand says what it does
			if let Some((setting, slot)) = self.auto_slot(i) {
				if slot.contains(mx, my) && !config::auto::automatic(&self.edited, setting) {
					return Some((Cow::Borrowed(CLEAR_TIP), slot));
				}
			}
			// hover target: the row's label + control span. The shells grid is
			// the exception - its "row" is the whole grid, and a tip that popped
			// up over every line of it would be in the way of the work; it hangs
			// off the column titles instead, which is where the question is.
			// a row of buttons hangs its tip under its buttons, not a checkbox
			let ctl = match self.specs[i].kind {
				Kind::Buttons(captions) => {
					let first = self.row_btn_rect(i, 0);
					let last = self.row_btn_rect(i, captions.len().saturating_sub(1) as u16);
					Rect {
						w: last.x + last.w - first.x,
						..first
					}
				}
				Kind::Hotkey(_) => self.textbox(i),
				_ => self.checkbox(i),
			};
			// Everything on the row answers with its one tip: the label, a
			// warning mark, each option's label and every control, up to the
			// revert column (2026100812334387). Only the arrow has its own.
			let hit = if matches!(self.specs[i].kind, Kind::ShellList) {
				Rect {
					x: self.content_x() + lay().pad,
					y: self.shell_head_y(i),
					w: self.layout_w() - lay().pad * 2.0,
					h: self.line_h,
				}
			} else {
				// part of a shared line starts at its own label
				let x = if self.specs[i].beside {
					self.label_x(i)
				} else {
					self.content_x() + lay().pad
				};
				Rect {
					x,
					y: self.row_y(i),
					w: (self.ctl_right(i) - x).max(ctl.x + ctl.w - x),
					h: self.row_screen_h(i),
				}
			};
			if !hit.contains(mx, my) {
				continue;
			}
			// a pair is two settings under one label, and either half can be
			// grayed by the desktop alone, so each box answers for its own
			let (key, anchor) = match self.specs[i].kind {
				Kind::ShellList => (self.specs[i].key, hit),
				Kind::Dual { keys, .. } if mx >= self.dual_box(i, 1).x => {
					(keys[1], self.dual_box(i, 1))
				}
				Kind::Dual { keys, .. } => (keys[0], ctl),
				_ => (self.specs[i].key, ctl),
			};
			if let Some(tip) = self.row_tip(i, key) {
				return Some((tip, anchor));
			}
		}
		None
	}
	// Tight box around one focused sub-control (the keyboard-focus ring hugs this,
	// a couple px out, instead of spanning the whole row).
	fn focus_ctl_rect(&self, i: usize, part: u16) -> Rect {
		match self.specs[i].kind {
			Kind::Slider { .. } => {
				if part == 0 {
					// the handle overhangs the track by half its width at either end
					let t = self.track(i);
					Rect {
						x: t.x - SLIDER_HANDLE_W / 2.0,
						y: t.y - 7.0,
						w: t.w + SLIDER_HANDLE_W,
						h: t.h + 14.0,
					}
				} else {
					self.valbox(i)
				}
			}
			Kind::Dual { .. } => {
				let bx = self.dual_box(i, part);
				let (y, h) = self.with_label_line(i, bx.y, bx.h);
				Rect {
					x: bx.x,
					y,
					w: self.dual_pitch() - 12.0,
					h,
				}
			}
			Kind::Toggle => self.checkbox(i),
			Kind::Text | Kind::Hotkey(_) => self.textbox(i),
			Kind::Color if part == 0 => self.swatch(i),
			Kind::Color => self.hexbox(i),
			Kind::Radio(opts) => {
				let first = self.radio_box(i, 0);
				let (y, h) = self.with_label_line(i, first.y - 2.0, first.h + 4.0);
				Rect {
					x: first.x,
					y,
					w: opts.len() as f32 * self.radio_pitch() - 12.0,
					h,
				}
			}
			Kind::Dropdown(_) => self.dd_box(i),
			Kind::Buttons(_) => self.row_btn_rect(i, part),
			Kind::ShellList => self.shell_stop_rect(i, part),
			Kind::Header(_) => self.track(i), // unreachable (headers aren't focusable)
		}
	}
	// A ring around a box and the labels after it, `y`/`h` from the box. Those
	// labels are a line of text, and at a large font the line is the taller of
	// the two (2026100817355494).
	fn with_label_line(&self, i: usize, y: f32, h: f32) -> (f32, f32) {
		if h >= self.line_h {
			return (y, h);
		}
		(self.centered_in_row(i, self.line_h), self.line_h)
	}
	// Does the keyboard ring sit exactly on a control's own outline? For a boxed
	// control it does, and then the box must not draw its border as well - a
	// field ringed twice reads as two outlines for one control. A color row is
	// the near miss: its ring spans the chip AND the hex field, so it stays a
	// couple of pixels out and only the hex field's own border stands down.
	fn ring_is_the_box(&self, i: usize, part: u16) -> bool {
		match self.specs[i].kind {
			Kind::Text | Kind::Dropdown(_) | Kind::Buttons(_) | Kind::Color | Kind::Hotkey(_) => {
				true
			}
			Kind::Slider { .. } => part == 1,
			// the two fields and the Add button are boxes; the checkbox and the
			// icon buttons are not, so they keep the ring a couple of pixels out
			Kind::ShellList => matches!(
				shell_stop(part, self.edited.shells.len()),
				ShellStop::Add | ShellStop::Entry(_, ShellPart::Name | ShellPart::Command)
			),
			_ => false,
		}
	}
	fn ring_on(&self, i: usize, part: u16) -> bool {
		self.focus == Some(Focus::Row(i, part))
	}
	// Is this row at its config default? (drives the revert icon). A Dual row is
	// "default" only when both its keys are.
	// A row showing a profile's value has nothing to revert - what is on screen is
	// not the user's value - so it is skipped rather than answering for the row.
	// Skipping it matters on a shared line, where one half can be governed and the
	// other not.
	fn row_is_default(&self, i: usize) -> bool {
		self.row_keys(i)
			.iter()
			.filter(|&&k| !self.profile_shows(k))
			.all(|&k| self.is_default(k))
	}
	// A row of push-buttons has no value, and the shells grid is a list rather
	// than a setting - neither has a default to go back to. A row drawn beside
	// another has none of its own either: the one revert arrow at the end of the
	// line stands for both halves. The file-type rows are the exception: their
	// arrow puts back what Register replaced.
	fn has_revert(&self, i: usize) -> bool {
		!self.specs[i].beside
			// a group's switch holds nothing; each member has its own
			&& self.specs[i].group.is_none()
			&& match self.specs[i].kind {
				Kind::Header(_) | Kind::ShellList => false,
				Kind::Buttons(_) => assoc_of(self.specs[i].key).is_some(),
				_ => true,
			}
	}
	// Every setting one revert arrow answers for: the row's own, both halves of a
	// Dual, and every row drawn beside it on its line.
	fn row_keys(&self, i: usize) -> Vec<Key> {
		let own = |i: usize| match self.specs[i].kind {
			Kind::Dual { keys, .. } => keys.to_vec(),
			_ => vec![self.specs[i].key],
		};
		let mut keys = own(i);
		if !self.specs[i].beside {
			let (_, parts, _) = Self::line_of(self.specs, i);
			for j in i + 1..i + parts {
				keys.extend(own(j));
			}
		}
		keys
	}
	// Revert a whole row to defaults - every key the row's own arrow covers, less
	// the ones a profile is showing for.
	fn row_revert(&mut self, i: usize) {
		if let Some(assoc) = assoc_of(self.specs[i].key) {
			self.assoc_set(assoc, false);
			return;
		}
		for k in self.row_keys(i) {
			if !self.profile_shows(k) && !self.is_default(k) {
				self.revert(k);
			}
		}
	}
	// Cancel, Apply, OK rects (right-aligned)
	fn buttons(&self) -> [(Action, Rect, &'static str); 3] {
		let y = self.rect.y + self.rect.h - lay().pad - self.btn_h();
		let x_ok = self.rect.x + self.rect.w - lay().pad - self.btn_w;
		let x_apply = x_ok - lay().button_gap - self.btn_w;
		let x_cancel = x_apply - lay().button_gap - self.btn_w;
		let mk = |x| Rect {
			x,
			y,
			w: self.btn_w,
			h: self.btn_h(),
		};
		[
			(Action::Cancel, mk(x_cancel), "Cancel"),
			(Action::Apply, mk(x_apply), "Apply"),
			(Action::Ok, mk(x_ok), "OK"),
		]
	}

	// file types

	// A push-button on a row: the theme row's, or a file type's Register.
	fn row_button(&mut self, i: usize, part: u16) {
		match assoc_of(self.specs[i].key) {
			Some(assoc) => self.assoc_set(assoc, true),
			None => self.theme_action(ThemeBtn::of(part)),
		}
	}

	fn assoc_refresh(&mut self) {
		for assoc in Assoc::ALL {
			self.assoc_on[assoc_slot(assoc)] = crate::fileassoc::registered(assoc, &*self.assoc);
		}
	}

	// Register or put back, at once: the registry is the state, so there is
	// nothing for Apply to write and nothing for Cancel to undo.
	fn assoc_set(&mut self, assoc: Assoc, on: bool) {
		let done = if on {
			std::env::current_exe()
				.map_err(anyhow::Error::from)
				.and_then(|exe| {
					let exe = crate::fileassoc::exe_to_register(&exe, &|p| p.exists());
					crate::fileassoc::register(assoc, &exe, &mut *self.assoc)
				})
		} else {
			crate::fileassoc::unregister(assoc, &mut *self.assoc)
		};
		crate::fileassoc::changed();
		self.assoc_refresh();
		let say = match done {
			Err(why) => Some((
				"Windows did not take the change".to_string(),
				format!("{why:#}"),
			)),
			Ok(()) if on => {
				let picked = crate::fileassoc::overridden(assoc, &*self.assoc);
				(!picked.is_empty()).then(|| {
					(
						format!(
							"Windows still opens {} files with the app picked for them",
							picked.join(" and ")
						),
						"To change that, choose Open with on one, then SilkTerm, then Always"
							.to_string(),
					)
				})
			}
			Ok(()) => None,
		};
		if let Some((title, detail)) = say {
			self.commit_edit();
			self.prompt = Some(Prompt {
				job: PromptJob::Notice,
				title,
				focus: PromptFocus::Ok,
				warn: Some(detail),
			});
		}
	}

	// One of a Buttons row's push-buttons. They start at the control column, so
	// they line up under whatever the row above them holds.
	fn row_btn_rect(&self, i: usize, part: u16) -> Rect {
		let h = self.btn_h();
		Rect {
			x: self.control_x(i) + f32::from(part) * (self.row_btn_w + lay().button_gap),
			y: self.row_y(i) + (self.row_screen_h(i) - h) / 2.0,
			w: self.row_btn_w,
			h,
		}
	}

	// What a row displays. `edited` is the user's own values; while a profile
	// is chosen the governed rows show the profile's instead, and only the
	// display - Apply still writes `edited`, so Custom finds everything intact.
	// A profile sets the governed fields from its own values alone and leaves
	// every other field as it was, so a governed row reads the profile's values
	// and any other row reads `edited`, with nothing copied. That holds only
	// while no row's value is read from both kinds of field;
	// `a_shown_value_is_the_profile_laid_over_the_settings` checks every key.
	fn shown(&self, key: Key) -> &Settings {
		match crate::profile::current(&self.edited) {
			Profile::Custom => &self.edited,
			_ if !GOVERNED.contains(&key) => &self.edited,
			profile => crate::profile::values_of(profile),
		}
	}
	fn get_f32(&self, key: Key) -> f32 {
		match self.auto_of(key) {
			Some(setting) => config::auto::number(&self.auto_shown(setting).0),
			None => slider_of(self.shown(key), key),
		}
	}
	fn set_f32(&mut self, key: Key, value: f32) {
		self.leave_profile(key);
		let settings = &mut self.edited;
		match key {
			Key::Opacity => settings.opacity = from_percent(value),
			Key::BgOpacity => settings.wallpaper_opacity = from_percent(value),
			Key::BgBlur => settings.wallpaper_blur = value,
			Key::BgContrastSize => settings.wallpaper_contrast_mask_size = from_percent(value),
			Key::BgContrastStrength => {
				settings.wallpaper_contrast_mask_strength = from_percent(value);
			}
			Key::BgContrastAuto => settings.wallpaper_contrast_mask_auto = from_percent(value),
			Key::ScrimRadius => settings.text_scrim_radius = value,
			Key::ScrimSoftness => settings.text_scrim_softness = from_percent(value),
			Key::ScrimStrength => settings.text_scrim_strength = value,
			Key::Outline => settings.text_outline = value,
			Key::MinContrast => settings.text_min_contrast = from_percent(value),
			Key::CursorBlinkRate => settings.cursor_blink_rate_s = value,
			Key::CursorHeight => settings.cursor_size_height = value,
			Key::CursorWidth => settings.cursor_size_width = value,
			Key::CursorResume => settings.cursor_animation_resume_s = value,
			Key::FontSize => settings.font_size = config::auto::Auto::by_hand(value),
			Key::LineHeight => settings.line_height_scale = value,
			Key::Margin => settings.margin = value,
			Key::TabRegularWidth => settings.tab_regular_pct = value,
			Key::TabMaxWidth => settings.tab_max_pct = value,
			Key::ScrollEaseIn => {
				settings.scroll_ease_in_ms = falling_value(value, EASE_IN_MIN, EASE_IN_MAX);
			}
			Key::ScrollRampUp => {
				settings.scroll_ramp_up_ms = falling_value(value, RAMP_UP_MIN, RAMP_UP_MAX);
			}
			Key::SingleScreenTau => settings.scroll_single_screen_tau_ms = speed_to_tau(value),
			Key::ScrollRampDown => {
				settings.scroll_ramp_down_ms = falling_value(value, RAMP_DOWN_MIN, RAMP_DOWN_MAX);
			}
			Key::ScrollEaseOut => {
				settings.scroll_ease_out_ms = falling_value(value, EASE_OUT_MIN, EASE_OUT_MAX);
			}
			Key::WheelLines => settings.wheel_lines = value,
			Key::ScrollbarThickness => settings.scrollbar_thickness = value,
			Key::MinimapWidth => settings.minimap_width = value,
			Key::Columns => {
				settings.columns = config::auto::Auto::by_hand(value.round().max(1.0) as usize);
			}
			Key::Rows => {
				settings.rows = config::auto::Auto::by_hand(value.round().max(1.0) as usize);
			}
			Key::IdleHiddenMin => {
				settings.idle_release_hidden_min = value.round().max(1.0) as usize;
			}
			Key::IdleMin => settings.idle_release_min = value.round().max(1.0) as usize,
			keys_of!(toggle | radio | color | text | hotkey | valueless | assoc) => {}
		}
	}
	// "File or folder" is where the picture comes from. A named image wins at
	// run time, so it shows whenever there is one. Otherwise the box follows the
	// Rotate switch: the folder with it on, the image with it off.
	fn wallpaper_box_is_folder(&self) -> bool {
		match &self.edit {
			Some(edit)
				if self
					.specs
					.get(edit.row)
					.is_some_and(|s| s.key == Key::BgImage) =>
			{
				edit.wallpaper_folder
			}
			_ => {
				config::auto::automatic(&self.edited, config::auto::Setting::WallpaperImage)
					&& self.edited.wallpaper_rotate_enabled
			}
		}
	}

	// Current value of a Text field (background image path / font family).
	fn get_text(&self, key: Key) -> String {
		match key {
			// the configured text, or what automatic finds
			Key::BgImage if self.wallpaper_box_is_folder() => {
				config::auto::text(&self.edited, config::auto::Setting::WallpaperFolder)
			}
			Key::BgImage => config::auto::text(&self.edited, config::auto::Setting::WallpaperImage),
			Key::FontFamily => config::auto::font_family(&self.edited),
			Key::LinkOpenCommand => {
				config::auto::text(&self.edited, config::auto::Setting::OpenCommand)
			}
			Key::StartupDirectory => self.edited.startup_directory.clone(),
			keys_of!(slider | toggle | radio | color | hotkey | valueless | assoc) => String::new(),
		}
	}
	fn set_text(&mut self, key: Key, text: &str) {
		let trimmed = text.trim();
		match key {
			// An emptied box goes back to the usual place, and so does the usual
			// place typed out, since that is what the file reads it as.
			Key::BgImage if self.wallpaper_box_is_folder() => {
				let usual = trimmed.is_empty() || trimmed == config::WALLPAPER_DIR_TOKEN;
				config::auto::set(
					&mut self.edited,
					config::auto::Setting::WallpaperFolder,
					(!usual).then(|| config::auto::Value::Text(trimmed.to_string())),
				);
				self.rewallpaper();
			}
			Key::BgImage => {
				config::auto::set(
					&mut self.edited,
					config::auto::Setting::WallpaperImage,
					Some(config::auto::Value::Text(trimmed.to_string())),
				);
				self.rewallpaper();
			}
			// an emptied box goes back to automatic
			Key::FontFamily => {
				self.edited.font_family = if trimmed.is_empty() {
					config::auto::Auto::automatic()
				} else {
					config::auto::Auto::by_hand(trimmed.to_string())
				};
			}
			Key::LinkOpenCommand => config::auto::set(
				&mut self.edited,
				config::auto::Setting::OpenCommand,
				Some(config::auto::Value::Text(trimmed.to_string())),
			),
			Key::StartupDirectory => self.edited.startup_directory = trimmed.to_string(),
			keys_of!(slider | toggle | radio | color | hotkey | valueless | assoc) => {}
		}
	}
	fn get_toggle(&self, key: Key) -> bool {
		toggle_of(self.shown(key), key)
	}
	fn set_toggle(&mut self, key: Key, on: bool) {
		self.leave_profile(key);
		match key {
			Key::PerfAuto => {
				// the step belongs to the automatic choice, so it goes when that does
				// and does not carry over into one freshly switched on
				if self.edited.performance_automatic != on {
					self.edited.stepped_profile = None;
				}
				self.edited.performance_automatic = on;
			}
			Key::PerfCheckHardware => self.edited.performance_check_hardware = on,
			Key::PerfCheckNext => self.edited.performance_check_next_run = on,
			Key::Transparency => self.edited.transparent_background = on,
			Key::BackdropBlur => self.edited.transparent_background_blur = on,
			Key::TextScrim => self.edited.text_scrim = on,
			Key::CursorScrim => self.edited.cursor_scrim = on,
			Key::CursorOutline => self.edited.cursor_outline = on,
			Key::CursorBlinking => self.edited.cursor_blink = on,
			Key::RememberSize => {
				let at = config::auto::Place {
					monitor: self.monitor.as_deref(),
				};
				config::auto::set_group(&mut self.edited, config::auto::Group::WindowSize, on, at);
				// automatic is no line, and the file only learns that through
				// the revert list
				if on {
					self.queue_revert(Key::Columns);
					self.queue_revert(Key::Rows);
				}
			}
			Key::RememberPerMonitor => self.edited.remember_per_monitor = on,
			Key::RememberMaximized => self.edited.remember_maximized = on,
			Key::NewTabNextToCurrent => self.edited.new_tab_beside = on,
			Key::TabShowsTitle => self.edited.tab_shows_title = on,
			Key::TabShowsShell => self.edited.tab_shows_shell = on,
			Key::TabShowsProgram => self.edited.tab_shows_program = on,
			Key::TabShowsDirectory => self.edited.tab_shows_directory = on,
			Key::TitleShowsTab => self.edited.title_shows_tab = on,
			Key::IdleRelease => self.edited.idle_release = on,
			Key::SoftwareRendering => self.edited.software_rendering = on,
			Key::CopyOnSelect => self.edited.copy_on_select = on,
			Key::ShellIntegration => self.edited.shell_integration = on,
			Key::BashPrompt => self.edited.bash_prompt = on,
			Key::Hyperlinks => self.edited.hyperlinks = on,
			Key::BgContrastMask => self.edited.wallpaper_contrast_mask = on,
			Key::BgEnabled => self.edited.wallpaper_enabled = on,
			Key::BgRotate => self.edited.wallpaper_rotate_enabled = on,
			Key::BgHonorXmp => self.edited.wallpaper_honor_xmp = on,
			Key::BgHonorXmpLook => self.edited.wallpaper_honor_xmp_look = on,
			Key::ColFromWallpaper => self.edited.colors_from_wallpaper = on,
			Key::SmoothScroll => self.edited.scroll_smooth = on,
			Key::Scrollbar => self.edited.scrollbar = on,
			Key::ScrollbarAutoHide => self.edited.scrollbar_auto_hide = on,
			Key::Minimap => self.edited.minimap = on,
			keys_of!(slider | radio | color | text | hotkey | valueless | assoc) => {}
		}
	}
	fn get_radio(&self, key: Key) -> usize {
		radio_of(self.shown(key), key)
	}
	fn set_radio(&mut self, key: Key, idx: usize) {
		self.leave_profile(key);
		match key {
			// Remote is never stored: picking it raises the session override and
			// leaves the stored profile for the next launch to come back to. Any
			// pick by hand also lifts a step the display watch took.
			//
			// The dropdown stays live while the profile is chosen automatically,
			// and naming one is how that choice is taken back - otherwise the pick
			// would be overwritten at the next launch with no sign of it. Remote is
			// the exception, since it lasts only for this session and says nothing
			// about what the machine should settle on.
			Key::PerfProfile => {
				self.edited.stepped_profile = None;
				match Profile::from_index(idx) {
					Profile::Remote => self.edited.remote_override = true,
					profile => {
						self.edited.remote_override = false;
						self.edited.performance_profile = profile;
						self.edited.performance_automatic = false;
					}
				}
			}
			Key::BgFit => {
				self.edited.wallpaper_default_fit = if idx == 1 {
					config::Fit::Zoom
				} else {
					config::Fit::Stretch
				};
			}
			Key::ScrimFunction => {
				if let Some(function) = Choice::from_index(idx) {
					self.edited.text_scrim_function = function;
				}
			}
			Key::ScrimRamp => {
				if let Some(ramp) = Choice::from_index(idx) {
					self.edited.text_scrim_ramp = ramp;
				}
			}
			Key::CursorAnimation => {
				if let Some(animation) = Choice::from_index(idx) {
					self.edited.cursor_animation = animation;
				}
			}
			// picking a theme or a mode re-reads the whole palette, so the color
			// rows below follow the selection instead of describing the last one
			Key::Theme => {
				let names = crate::theme::all_names(&self.edited.user_themes);
				if let Some(name) = names.get(idx) {
					self.edited.theme.clone_from(name);
					self.adopt_theme();
				}
			}
			Key::ThemeMode => {
				if let Some(mode) = Choice::from_index(idx) {
					self.edited.theme_mode = mode;
				}
				self.adopt_theme();
			}
			keys_of!(slider | toggle | color | text | hotkey | valueless | assoc) => {}
		}
	}
	// A control grayed out by a gate in settings_ui.shcl, or by the machine.
	fn disabled(&self, key: Key) -> bool {
		!ui().needs_of(key).iter().all(|need| self.gate_ok(need))
			// nothing for the platform to do with it (the tip says so)
			|| self.disabled_tip(key).is_some()
	}
	// A row the chosen performance profile sets. It shows the profile's value
	// rather than the user's own, and still takes input - see `leave_profile`.
	fn profile_shows(&self, key: Key) -> bool {
		crate::profile::current(&self.edited) != Profile::Custom && GOVERNED.contains(&key)
	}
	// Changing a row a profile governs is the user taking the settings back.
	// The values on screen become their own and the profile drops to Custom, so
	// the edit is a change to what was visible rather than to values the profile
	// had been hiding. Cheap to call on every drag step: it does nothing once
	// Custom is in force.
	fn leave_profile(&mut self, key: Key) {
		if GOVERNED.contains(&key) {
			crate::profile::adopt(&mut self.edited);
		}
	}
	// Is one declared prerequisite satisfied? A slider counts while it sits above
	// zero, everything else while it is switched on.
	fn gate_ok(&self, need: &ui_spec::Need) -> bool {
		let on = if need.numeric {
			self.get_f32(need.key) > 0.0
		} else {
			self.get_toggle(need.key)
		};
		on != need.invert
	}
	// Every color but the two the wallpaper can set is an auto setting, so the
	// key answers through the table.
	fn get_col(&self, key: Key) -> [u8; 3] {
		if let Some(setting) = self.auto_of(key) {
			return config::auto::color(&self.edited, setting);
		}
		match key {
			Key::ColFg => self.edited.fg,
			Key::ColCursor => self.edited.cursor,
			_ => [0, 0, 0],
		}
	}
	// A color chosen is set by hand, even the one automatic gives.
	fn set_col(&mut self, key: Key, color: [u8; 3]) {
		if let Some(setting) = self.auto_of(key) {
			config::auto::set(
				&mut self.edited,
				setting,
				Some(config::auto::Value::Color(color)),
			);
			return;
		}
		match key {
			Key::ColFg => self.edited.fg = color,
			Key::ColCursor => self.edited.cursor = color,
			_ => {}
		}
	}

	// The active theme's palette - the effective default for the colors.* keys
	// (commented-out colors fall back to the theme, not to SilkTerm-dark).
	fn theme_palette(&self) -> crate::theme::Palette {
		config::theme_palette(&self.edited)
	}
	fn default_col(&self, key: Key) -> [u8; 3] {
		let palette = self.theme_palette();
		match key {
			Key::ColBg => palette.bg,
			Key::ColFg => palette.fg,
			Key::ColCursor => palette.cursor,
			Key::ColHighlight => palette.highlight,
			Key::ColFocus => palette.focus,
			Key::ColGutter => palette.gutter,
			Key::ColMenuBg => palette.menu_bg,
			Key::ColMenuFg => palette.menu_fg,
			Key::ColDialogBg => palette.dialog_bg,
			Key::ColDialogFg => palette.dialog_fg,
			// chrome, not a palette color - the same neutral under every theme
			Key::ColScrollbarThumb => config::SCROLLBAR_THUMB_DEF,
			Key::ColScrollbarTrough => config::SCROLLBAR_TROUGH_DEF,
			keys_of!(slider | toggle | radio | text | hotkey | valueless | assoc) => [0, 0, 0],
		}
	}

	// Put row `i`'s auto setting back to automatic, which at Apply comments its
	// line out.
	fn back_to_automatic(&mut self, i: usize) {
		let key = self.specs[i].key;
		if let Some(setting) = self.auto_of(key) {
			config::auto::set(&mut self.edited, setting, None);
			if key == Key::BgImage {
				self.rewallpaper();
			}
			self.queue_revert(key);
		}
	}
	// The picture and the rotation folder the image and folder settings come
	// to, found the way the loader finds them, so a typed relative name applies
	// at once. A named image hides a folder found by convention.
	fn rewallpaper(&mut self) {
		use config::auto::Setting;
		let image = (!config::auto::automatic(&self.edited, Setting::WallpaperImage))
			.then(|| config::auto::text(&self.edited, Setting::WallpaperImage));
		let pinned = image.is_some();
		self.edited.wallpaper = config::resolve_wallpaper(image);
		(
			self.edited.wallpaper_folder,
			self.edited.wallpaper_folder_auto,
		) = config::rotation_folder_for(
			&config::auto::text(&self.edited, Setting::WallpaperFolder),
			pinned,
		);
	}
	// Is this setting at its config default? Drives the revert icon's state.
	fn is_default(&self, key: Key) -> bool {
		// An auto setting's default is automatic, whatever value that gives.
		// File or folder is two of them, below.
		if let Some(setting) = self.auto_of(key).filter(|_| key != Key::BgImage) {
			return config::auto::automatic(&self.edited, setting);
		}
		let edited = &self.edited;
		let defaults = &self.defaults;
		// set in the file is not default, even to the shipped chord: a chord set
		// there takes priority over one a hotkey has by default
		if let Some(hotkey) = self.hotkey_for_key(key) {
			return edited.keys.own(hotkey).is_none();
		}
		match key {
			keys_of!(toggle) => toggle_of(edited, key) == toggle_of(defaults, key),
			keys_of!(slider) => self.get_f32(key) == self.default_f32(key),
			keys_of!(color) => self.get_col(key) == self.default_col(key),
			Key::BgFit => edited.wallpaper_default_fit == defaults.wallpaper_default_fit,
			Key::ScrimRamp => edited.text_scrim_ramp == defaults.text_scrim_ramp,
			Key::ScrimFunction => edited.text_scrim_function == defaults.text_scrim_function,
			Key::CursorAnimation => edited.cursor_animation == defaults.cursor_animation,
			Key::PerfProfile => {
				edited.performance_profile == defaults.performance_profile
					&& !edited.remote_override
					&& edited.stepped_profile.is_none()
			}
			Key::StartupDirectory => edited.startup_directory == defaults.startup_directory,
			Key::Theme => edited.theme == defaults.theme,
			Key::ThemeMode => edited.theme_mode == defaults.theme_mode,
			// buttons, headings and the shells list hold nothing to revert, and a
			// hotkey with a row was answered above
			// its arrow puts back both
			Key::BgImage => {
				edited.wallpaper_raw.is_automatic() && edited.wallpaper_folder_raw.is_automatic()
			}
			// the other auto settings were answered above
			Key::FontFamily => edited.font_family.is_automatic(),
			Key::LinkOpenCommand => edited.hyperlink_open_command.is_automatic(),
			keys_of!(valueless | hotkey) => true,
			// nothing to put back until Register has saved something
			keys_of!(assoc) => assoc_of(key).is_none_or(|assoc| !self.assoc_on[assoc_slot(assoc)]),
		}
	}
	// Default for a slider key, in get_f32's own units (speed for SingleScreenTau).
	fn default_f32(&self, key: Key) -> f32 {
		slider_of(&self.defaults, key)
	}
	// Revert a setting to its default and remember its config key(s), so Apply
	// can comment them out in config.shcl (config::revert_keys).
	fn revert(&mut self, key: Key) {
		// an auto setting's default is automatic; File or folder is two, below
		if let Some(setting) = self.auto_of(key).filter(|_| key != Key::BgImage) {
			config::auto::set(&mut self.edited, setting, None);
			self.queue_revert(key);
			return;
		}
		if let Some(hotkey) = self.hotkey_for_key(key) {
			self.edited.keys = self.edited.keys.with_own(hotkey, None);
			self.moved.retain(|(from, ..)| *from != hotkey);
			self.queue_revert(key);
			return;
		}
		match key {
			keys_of!(toggle) => self.set_toggle(key, toggle_of(&self.defaults, key)),
			Key::BgFit => self.edited.wallpaper_default_fit = self.defaults.wallpaper_default_fit,
			Key::ScrimRamp => self.edited.text_scrim_ramp = self.defaults.text_scrim_ramp,
			Key::ScrimFunction => {
				self.edited.text_scrim_function = self.defaults.text_scrim_function;
			}
			Key::CursorAnimation => self.edited.cursor_animation = self.defaults.cursor_animation,
			Key::PerfProfile => {
				self.edited.performance_profile = self.defaults.performance_profile;
				self.edited.remote_override = false;
				self.edited.stepped_profile = None;
			}
			Key::Theme => {
				self.edited.theme = self.defaults.theme.clone();
				self.adopt_theme();
			}
			Key::ThemeMode => {
				self.edited.theme_mode = self.defaults.theme_mode;
				self.adopt_theme();
			}
			Key::StartupDirectory => {
				self.edited.startup_directory = self.defaults.startup_directory.clone();
			}
			keys_of!(color) => {
				let color = self.default_col(key);
				self.set_col(key, color);
			}
			keys_of!(slider) => {
				let value = self.default_f32(key);
				self.set_f32(key, value);
			}
			// File or folder puts back both of its settings
			Key::BgImage => {
				use config::auto::Setting;
				config::auto::set(&mut self.edited, Setting::WallpaperImage, None);
				config::auto::set(&mut self.edited, Setting::WallpaperFolder, None);
				self.rewallpaper();
			}
			// The other auto settings and hotkeys went back above, and
			// `row_revert` undoes a registration.
			Key::FontFamily => self.edited.font_family = config::auto::Auto::automatic(),
			Key::LinkOpenCommand => {
				self.edited.hyperlink_open_command = config::auto::Auto::automatic();
			}
			keys_of!(valueless | assoc | hotkey) => {}
		}
		self.queue_revert(key);
	}
	// The row's line goes back to the template's at Apply.
	fn queue_revert(&mut self, key: Key) {
		for cfg_key in ui().settings_of(key) {
			if !self.reverted.contains(cfg_key) {
				self.reverted.push(cfg_key);
			}
		}
	}
	/// Config keys reverted since the last Apply (cleared by taking them). A row set
	/// away from its default again after the revert keeps the new value, so only
	/// the rows still at their default go back to the template's line.
	pub fn take_reverted(&mut self) -> Vec<&'static str> {
		let mut reverted = std::mem::take(&mut self.reverted);
		let keys: Vec<Key> = self
			.specs
			.iter()
			.flat_map(|spec| match spec.kind {
				Kind::Dual { keys, .. } => keys.to_vec(),
				_ => vec![spec.key],
			})
			.collect();
		reverted.retain(|cfg_key| {
			keys.iter()
				.find(|&&key| ui().settings_of(key).contains(cfg_key))
				.is_none_or(|&key| self.is_default(key))
		});
		reverted
	}

	fn fmt_val(&self, key: Key, int: bool) -> String {
		fmt_number(self.get_f32(key), int)
	}

	// `measure` gives a string's rendered width in the UI font (for placing the
	// caret at the clicked position inside a text field).
	fn mouse_down_dip(&mut self, x: f32, y: f32, measure: &mut impl FnMut(&str) -> f32) -> Action {
		// double/triple-click detection (word / whole-value selection in fields)
		let now = std::time::Instant::now();
		self.click_streak = match self.last_click {
			Some((t, lx, ly))
				if now.duration_since(t).as_millis() < 400
					&& (x - lx).abs() < 6.0
					&& (y - ly).abs() < 6.0 =>
			{
				self.click_streak.saturating_add(1)
			}
			_ => 1,
		};
		self.last_click = Some((now, x, y));
		// a click anywhere stops a hotkey row waiting; one on its box starts it again
		self.capture_end();
		// the picker is modal over the panel: it takes the click either way
		if self.pick.is_some() {
			if self.emenu.is_some() {
				let hit = (0..EDIT_MENU.len()).find(|&k| self.em_item_rect(k).contains(x, y));
				let cmd = hit.filter(|&k| self.em_enabled(k)).map(|k| EDIT_MENU[k].1);
				self.emenu = None;
				return cmd.map_or(Action::None, Action::Edit);
			}
			self.pick_mouse_down(x, y, measure);
			return Action::None;
		}
		// the prompt box is modal over the panel: it takes the click either way
		if self.prompt.is_some() {
			if self.emenu.is_some() {
				let hit = (0..EDIT_MENU.len()).find(|&k| self.em_item_rect(k).contains(x, y));
				let cmd = hit.filter(|&k| self.em_enabled(k)).map(|k| EDIT_MENU[k].1);
				self.emenu = None;
				return cmd.map_or(Action::None, Action::Edit);
			}
			self.prompt_mouse_down(x, y, measure);
			return Action::None;
		}
		// an open field context menu captures the click: an enabled item fires its
		// command (clipboard glue in dialog.rs), anywhere else just dismisses
		if self.emenu.is_some() {
			let hit = (0..EDIT_MENU.len()).find(|&k| self.em_item_rect(k).contains(x, y));
			let cmd = hit.filter(|&k| self.em_enabled(k)).map(|k| EDIT_MENU[k].1);
			self.emenu = None;
			return cmd.map_or(Action::None, Action::Edit);
		}
		// an open dropdown captures the click: on an option -> pick it, anywhere
		// else -> just close (a click-away dismiss, consumed either way)
		if let Some(oi) = self.open.take() {
			let n = self.dd_options(oi).len();
			for choice in 0..n {
				if self.dd_item_rect(oi, n, choice).contains(x, y) {
					self.set_radio(self.specs[oi].key, choice);
					break;
				}
			}
			return Action::None;
		}
		// footer buttons arm on press (drawn pressed) and fire on release, so a
		// press-drag-off cancels - and the user gets click feedback
		for (btn_idx, (_, r, _)) in self.buttons().into_iter().enumerate() {
			if r.contains(x, y) {
				self.pressed = Some(btn_idx);
				return Action::None;
			}
		}
		// click outside the panel cancels
		if !self.rect.contains(x, y) {
			return Action::Cancel;
		}
		// a click inside the field being edited keeps the edit (caret/selection
		// handling below); anywhere else commits it
		let keep_edit = self
			.edit
			.as_ref()
			.is_some_and(|e| self.field_rect(e.row).is_some_and(|r| r.contains(x, y)));
		if !keep_edit {
			self.commit_edit();
		}
		// tab bar
		for tab in 0..self.tab_ws.len() {
			if self.tab_rect(tab).contains(x, y) {
				if tab != self.tab {
					self.tab = tab;
					self.scroll = 0.0;
					self.hscroll = 0.0;
					self.drag = None;
					self.focus = None; // mouse mode; Tab re-establishes focus
				}
				return Action::None;
			}
		}
		// scrollbar: drag the thumb, or jump-and-drag from the track
		if let Some(thumb) = self.thumb() {
			if thumb.contains(x, y) {
				self.drag_thumb = Some(y - thumb.y);
				return Action::None;
			}
			let vp = self.viewport();
			if x >= thumb.x && x <= thumb.x + thumb.w && y >= vp.y && y <= vp.y + vp.h {
				let frac = ((y - vp.y - thumb.h / 2.0) / (vp.h - thumb.h).max(1.0)).clamp(0.0, 1.0);
				self.scroll = frac * self.max_scroll();
				self.drag_thumb = Some(thumb.h / 2.0);
				return Action::None;
			}
		}
		// the sideways bar, the same two gestures. Its grab band is the whole strip
		// it sits in, so an 8 DIP bar is not an 8 DIP target.
		if let Some(thumb) = self.hthumb() {
			let track = self.htrack();
			let band = Rect {
				y: track.y - lay().scrollbar_inset,
				h: self.hbar_h(),
				..track
			};
			if band.contains(x, y) {
				let grab = if x >= thumb.x && x <= thumb.x + thumb.w {
					x - thumb.x
				} else {
					let frac = ((x - track.x - thumb.w / 2.0) / (track.w - thumb.w).max(1.0))
						.clamp(0.0, 1.0);
					self.hscroll = frac * self.max_hscroll();
					thumb.w / 2.0
				};
				self.drag_hthumb = Some(grab);
				return Action::None;
			}
		}
		// rows: only within the (possibly scrolled) viewport, only the active tab
		let vp = self.viewport();
		if y < vp.y || y > vp.y + vp.h {
			return Action::None;
		}
		for i in 0..self.specs.len() {
			if self.specs[i].tab != self.tab || Self::header_is_tab_title(&self.specs[i]) {
				continue;
			}
			// revert-to-default icon (any control row; inert when already default)
			if self.has_revert(i) && self.revert_box(i).contains(x, y) {
				if !self.row_is_default(i) {
					self.row_revert(i);
				}
				return Action::None;
			}
			// A grayed control takes no click. This used to sit inside each arm, and
			// the color, text and radio arms were the three that never got it - so
			// a control the dialog draws as inert still changed its setting. The
			// three whose parts gray separately keep their own per-part check. A
			// pair row's key is only its FIRST part, so gating the whole row on
			// that key took the click away from a live second part.
			if !matches!(
				self.specs[i].kind,
				Kind::Buttons(_) | Kind::ShellList | Kind::Dual { .. }
			) && self.disabled(self.specs[i].key)
			{
				continue;
			}
			// the icon in a field set by hand puts it back to automatic
			if let Some((setting, slot)) = self.auto_slot(i) {
				if slot.contains(x, y) && !config::auto::automatic(&self.edited, setting) {
					// the field's own stop, which a slider and a color have second
					self.focus = Some(Focus::Row(
						i,
						u16::from(matches!(
							self.specs[i].kind,
							Kind::Slider { .. } | Kind::Color
						)),
					));
					self.back_to_automatic(i);
					return Action::None;
				}
			}
			match self.specs[i].kind {
				Kind::Slider { .. } => {
					// click the numeric field -> edit the value, caret at the click
					let val_box = self.valbox(i);
					if val_box.contains(x, y) {
						self.field_click(i, Some((i, 1)), val_box, x, measure);
						return Action::None;
					}
					let track = self.track(i);
					let hit = x >= track.x - 8.0
						&& x <= track.x + track.w + 8.0
						&& (y - (track.y + track.h / 2.0)).abs() <= 12.0;
					if hit {
						self.focus = Some(Focus::Row(i, 0));
						self.drag = Some(i);
						self.drag_to(x);
						return Action::None;
					}
				}
				Kind::Color => {
					// the chip opens the picker; a hex-box click places the caret
					if self.swatch(i).contains(x, y) {
						self.focus = Some(Focus::Row(i, 0));
						self.pick_open(i);
						return Action::None;
					}
					let hex_box = self.hexbox(i);
					if hex_box.contains(x, y) {
						self.field_click(i, Some((i, 1)), hex_box, x, measure);
						return Action::None;
					}
				}
				Kind::Text => {
					let text_box = self.textbox(i);
					if text_box.contains(x, y) {
						self.field_click(i, Some((i, 0)), text_box, x, measure);
						return Action::None;
					}
				}
				Kind::Hotkey(_) => {
					if self.textbox(i).contains(x, y) {
						self.capture_start(i);
						return Action::None;
					}
				}
				Kind::Toggle => {
					if self.checkbox(i).contains(x, y) {
						let key = self.specs[i].key;
						self.focus = Some(Focus::Row(i, 0));
						self.set_toggle(key, !self.get_toggle(key));
						return Action::None;
					}
				}
				Kind::Dual { keys, .. } => {
					// hit either checkbox (or its label span, out to the next pitch)
					for part in 0u16..2 {
						let bx = self.dual_box(i, part);
						if x >= bx.x
							&& x <= bx.x + self.dual_pitch() - 8.0
							&& (y - (bx.y + bx.h / 2.0)).abs() <= bx.h / 2.0 + 4.0
						{
							if self.disabled(keys[part as usize]) {
								continue; // grayed checkbox ignores clicks
							}
							let key = keys[part as usize];
							self.focus = Some(Focus::Row(i, part));
							self.set_toggle(key, !self.get_toggle(key));
							return Action::None;
						}
					}
				}
				Kind::Radio(options) => {
					for choice in 0..options.len() {
						let radio_rect = self.radio_box(i, choice);
						// click the box or its label
						if x >= radio_rect.x
							&& x <= radio_rect.x + self.radio_pitch() - 8.0
							&& (y - (radio_rect.y + radio_rect.h / 2.0)).abs()
								<= radio_rect.h / 2.0 + 4.0
						{
							self.focus = Some(Focus::Row(i, 0));
							self.set_radio(self.specs[i].key, choice);
							return Action::None;
						}
					}
				}
				Kind::Dropdown(_) => {
					if self.dd_box(i).contains(x, y) {
						self.dd_open(i);
						return Action::None;
					}
				}
				// same press-arm / release-fire as the footer buttons, so a
				// press-drag-off cancels and the press is visible
				Kind::Buttons(captions) => {
					for part in 0..captions.len() as u16 {
						if self.row_btn_rect(i, part).contains(x, y) {
							if self.part_disabled(i, part) {
								continue;
							}
							self.focus = Some(Focus::Row(i, part));
							self.pressed_row = Some((i, part));
							return Action::None;
						}
					}
				}
				Kind::ShellList => {
					if self.shell_mouse_down(i, x, y, measure) {
						return Action::None;
					}
				}
				Kind::Header(_) => {}
			}
		}
		Action::None
	}

	// The editable text box of row i, by kind (None for non-field rows).
	fn field_rect(&self, i: usize) -> Option<Rect> {
		if i == PROMPT_ROW {
			return self.prompt_field_rect();
		}
		if let Some(f) = pick_field_of(i) {
			return self.pick.as_ref().map(|_| self.pick_geom().field(f));
		}
		if let Some((shell_index, command)) = shell_field_of(i) {
			let grid = self.shell_row()?;
			return Some(if command {
				self.shell_cmd_box(grid, shell_index)
			} else {
				self.shell_name_box(grid, shell_index)
			});
		}
		match self.specs[i].kind {
			Kind::Slider { .. } => Some(self.valbox(i)),
			Kind::Color => Some(self.hexbox(i)),
			Kind::Text => Some(self.textbox(i)),
			_ => None,
		}
	}
	// Click into an editable field: caret at the click; Shift extends the
	// selection; double-click selects the word, triple selects all; a plain
	// click starts a drag-selection.
	// `row` is the edit's own index - a spec row, or one of the pseudo rows the
	// grid's fields use - and `focus` is the spec row and part the ring goes on.
	// For every field but the grid's they are the same row; for those two they
	// cannot be, since no spec row draws them.
	fn field_click(
		&mut self,
		row: usize,
		// None for the prompt box, which is not a row and must never reach the
		// focus ring: the ring indexes `specs` with whatever it is given.
		focus: Option<(usize, u16)>,
		field: Rect,
		x: f32,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		let i = row;
		let same_row = self.edit.as_ref().is_some_and(|e| e.row == i);
		if !same_row {
			self.open_edit(i, false);
		}
		self.select_all_on_up = false;
		let (shift, streak) = (self.shift, self.click_streak);
		if let Some((r, part)) = focus {
			self.focus = Some(Focus::Row(r, part));
		}
		let Some(edit) = &mut self.edit else { return };
		let cur = caret_from_click(
			&edit.buf,
			x - (field.x + lay().field_pad) + edit.view,
			measure,
		);
		if shift && same_row {
			if edit.sel.is_none() {
				edit.sel = Some(edit.cur);
			}
			edit.cur = cur;
			return;
		}
		match streak {
			2 => {
				let (a, b) = word_at(&edit.buf, cur);
				edit.sel = (a != b).then_some(a);
				edit.cur = b;
			}
			n if n >= 3 => {
				edit.sel = (!edit.buf.is_empty()).then_some(0);
				edit.cur = edit.buf.len();
			}
			_ => {
				edit.cur = cur;
				edit.sel = None;
				self.edit_drag = Some(i);
				// fresh entry (not repositioning a caret in the field already open):
				// select all on release, unless the click turns into a drag-select
				self.select_all_on_up = !same_row;
			}
		}
	}

	// --- field context menu (right-click / Menu key inside an editable field) ---
	fn em_item_h(&self) -> f32 {
		self.dd_item_h()
	}
	fn em_rect(&self) -> Rect {
		let Some(menu) = &self.emenu else {
			return Rect {
				x: 0.0,
				y: 0.0,
				w: 0.0,
				h: 0.0,
			};
		};
		let w = lay().edit_menu_width * self.ui_scale();
		let h = EDIT_MENU.len() as f32 * self.em_item_h();
		// clamp into the panel; flip upward when it would spill past the bottom
		let x = menu
			.x
			.min(self.rect.x + self.rect.w - w - 2.0)
			.max(self.rect.x);
		let y = if menu.y + h > self.rect.y + self.rect.h - 2.0 {
			(menu.y - h).max(self.rect.y)
		} else {
			menu.y
		};
		Rect { x, y, w, h }
	}
	fn em_item_rect(&self, item: usize) -> Rect {
		let r = self.em_rect();
		Rect {
			x: r.x,
			y: r.y + item as f32 * self.em_item_h(),
			w: r.w,
			h: self.em_item_h(),
		}
	}
	fn em_enabled(&self, item: usize) -> bool {
		let edit = self.edit.as_ref();
		match EDIT_MENU[item].1 {
			EditCmd::Cut | EditCmd::Copy | EditCmd::Delete => {
				edit.is_some_and(|e| e.sel_range().is_some())
			}
			EditCmd::Paste => self.emenu.as_ref().is_some_and(|m| m.paste_ok),
			EditCmd::SelectAll => edit.is_some_and(|e| !e.buf.is_empty()),
		}
	}
	fn dismiss_menu(&mut self) {
		self.emenu = None;
	}
	// Right-click in an editable field: open (or keep) the edit, place the caret
	// at the click unless it falls inside the selection (standard), pop the menu.
	fn mouse_right_dip(
		&mut self,
		x: f32,
		y: f32,
		paste_ok: bool,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		self.mouse = (x, y);
		self.emenu = None;
		self.open = None;
		// A modal box takes every click while it is up. Its own fields still get
		// the menu; anywhere else does nothing, or the click would open an edit on
		// the row behind the box and OK would then save under that row's value.
		if self.pick.is_some() {
			let g = self.pick_geom();
			if let Some(f) = pick::Field::ALL
				.into_iter()
				.find(|&f| g.field(f).contains(x, y))
			{
				self.pick_focus_to(pick::Focus::Field(f));
				self.pop_field_menu(g.field(f), x, y, paste_ok, measure);
			}
			return;
		}
		if self.prompt.is_some() {
			if let Some(field) = self.prompt_field_rect().filter(|f| f.contains(x, y)) {
				self.pop_field_menu(field, x, y, paste_ok, measure);
			}
			return;
		}
		let vp = self.viewport();
		if y < vp.y || y > vp.y + vp.h {
			return;
		}
		for i in 0..self.specs.len() {
			if self.specs[i].tab != self.tab || Self::header_is_tab_title(&self.specs[i]) {
				continue;
			}
			// The shells grid is one spec row holding two editable fields per
			// entry, so the field under the pointer has to be found the way the
			// press handler finds it. Without this the two fields were the only
			// ones in the dialog with no right-click menu, while the Menu key
			// worked on them.
			if matches!(self.specs[i].kind, Kind::ShellList) {
				if let Some((row, field, part)) = self.shell_field_at(i, x, y) {
					if self.edit.as_ref().is_none_or(|e| e.row != row) {
						self.commit_edit();
						self.open_edit(row, false);
					}
					self.focus = Some(Focus::Row(i, part));
					self.pop_field_menu(field, x, y, paste_ok, measure);
				}
				return;
			}
			let Some(field) = self.field_rect(i) else {
				continue;
			};
			if !field.contains(x, y) {
				continue;
			}
			if self.disabled(self.specs[i].key) {
				return;
			}
			let same_row = self.edit.as_ref().is_some_and(|e| e.row == i);
			if !same_row {
				self.commit_edit();
				self.open_edit(i, false);
			}
			let part = u16::from(matches!(self.specs[i].kind, Kind::Slider { .. }));
			self.focus = Some(Focus::Row(i, part));
			self.pop_field_menu(field, x, y, paste_ok, measure);
			return;
		}
	}

	// Caret placement plus the menu itself, shared by the panel rows and the theme
	// prompt's own field. A click inside an existing selection leaves it alone, so
	// the menu can act on it (standard).
	fn pop_field_menu(
		&mut self,
		field: Rect,
		x: f32,
		y: f32,
		paste_ok: bool,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		if let Some(edit) = &mut self.edit {
			let rel_x = x - (field.x + lay().field_pad) + edit.view;
			let cur = caret_from_click(&edit.buf, rel_x, measure);
			let inside = edit.sel_range().is_some_and(|(a, b)| cur >= a && cur <= b);
			if !inside {
				edit.cur = cur;
				edit.sel = None;
			}
		}
		self.emenu = Some(EMenu {
			x,
			y,
			hover: None,
			paste_ok,
		});
	}
	// Keyboard Menu key: pop the context menu at the caret of the active edit.
	fn menu_key_dip(&mut self, paste_ok: bool, measure: &mut impl FnMut(&str) -> f32) {
		let Some(edit) = &self.edit else { return };
		let Some(field) = self.field_rect(edit.row) else {
			return;
		};
		let cx = (field.x + lay().field_pad + measure(&edit.buf[..edit.cur]) - edit.view)
			.clamp(field.x, field.x + field.w);
		self.emenu = Some(EMenu {
			x: cx,
			y: field.y + field.h,
			hover: Some(0),
			paste_ok,
		});
	}

	// Per-frame upkeep of the active field edit, with real frame time: eases the
	// horizontal view (caret kept visible with a lookahead margin so several
	// characters show ahead of travel; lay().caret_pad keeps the caret clear of the
	// right edge at end-of-text), eases the caret x, advances the blink, and
	// replays a drag past the box edges (edge autoscroll). Returns the wake the
	// caller should schedule: fast while something moves, blink-rate while an
	// idle edit pulses, None when there's nothing to animate.
	fn animate_dip(&mut self, dt: f32, measure: &mut impl FnMut(&str) -> f32) -> Option<u64> {
		// Edge autoscroll: a drag held past the box keeps selecting while the view
		// crawls, so the pointer is replayed each frame. Only past the box, though.
		// Replaying it inside fed the view ease back into the caret: a click near
		// the right edge of a value wider than its box scrolled the view, the
		// replay then read a later character under the same pointer, that scrolled
		// the view further, and the selection ran off to the end of the text.
		if let Some(row) = self.edit_drag {
			let (mx, my) = self.mouse;
			let past_edge = self
				.field_rect(row)
				.is_some_and(|f| mx < f.x || mx > f.x + f.w);
			if past_edge {
				let _ = self.mouse_move_dip(mx, my, measure);
			}
		}
		let row = self.edit.as_ref().map(|e| e.row)?;
		let field = self.field_rect(row)?;
		let inner_w = (field.w - 2.0 * lay().field_pad).max(1.0);
		let ahead = (lay().view_ahead * self.ui_scale()).min(inner_w / 3.0);
		let (caret_x, text_w, sig) = {
			#[allow(clippy::unwrap_used, reason = "Some: row extracted above")]
			let edit = self.edit.as_ref().unwrap();
			(
				measure(&edit.buf[..edit.cur]),
				measure(&edit.buf),
				(edit.cur, edit.sel, edit.buf.len()),
			)
		};
		let dragging = self.edit_drag.is_some();
		#[allow(clippy::unwrap_used, reason = "Some: row extracted above")]
		let edit = self.edit.as_mut().unwrap();
		if sig == edit.last_sig {
			edit.blink_t += dt;
		} else {
			edit.last_sig = sig;
			edit.blink_t = 0.0; // activity holds the caret solid
		}
		// target view: keep the caret in sight with the margin; the clamp snaps
		// the margin away at the true ends so 0 / end-of-text sit flush
		let max_view = (text_w + lay().caret_pad - inner_w).max(0.0);
		let mut to = edit.view_to;
		if caret_x < to + ahead {
			to = caret_x - ahead;
		}
		if caret_x > to + inner_w - ahead {
			to = caret_x - (inner_w - ahead);
		}
		edit.view_to = to.clamp(0.0, max_view);
		// exponential ease toward the targets (same idiom as the pane scroll)
		edit.view += (edit.view_to - edit.view) * (1.0 - (-dt / 0.05).exp());
		let cv = edit.caret_vis.get_or_insert(caret_x);
		*cv += (caret_x - *cv) * (1.0 - (-dt / 0.04).exp());
		let moving = (edit.view_to - edit.view).abs() > 0.25 || (caret_x - *cv).abs() > 0.25;
		if !moving {
			edit.view = edit.view_to;
			*cv = caret_x;
		}
		Some(if moving || dragging { 8 } else { 33 })
	}

	// True when the move changed something drawn. Nothing is drawn from the
	// pointer itself, so a move that changes none of this needs no frame.
	fn mouse_move_dip(&mut self, x: f32, y: f32, measure: &mut impl FnMut(&str) -> f32) -> bool {
		self.mouse = (x, y);
		if let Some(was) = self
			.pick
			.as_ref()
			.filter(|p| p.drag.is_some())
			.map(|p| p.hsv)
		{
			self.pick_drag_to(x, y);
			return self.pick.as_ref().is_some_and(|p| p.hsv != was);
		}
		// a line being dragged by its grip, reordered as it travels
		if let Some(drag) = &self.shell_drag {
			let (at, grab_dy) = (drag.at, drag.grab_dy);
			let Some(i) = self.shell_row() else {
				return false;
			};
			let want = self.shell_drop_at(i, y, grab_dy);
			if want == at {
				return false;
			}
			self.shell_move_to(at, want);
			if let Some(drag) = &mut self.shell_drag {
				drag.at = want;
			}
			return true;
		}
		// open field context menu: track the hovered item
		if self.emenu.is_some() {
			let hover = (0..EDIT_MENU.len()).find(|&k| self.em_item_rect(k).contains(x, y));
			let Some(menu) = &mut self.emenu else {
				return false;
			};
			let was = menu.hover;
			menu.hover = hover.or(menu.hover);
			return menu.hover != was;
		}
		// drag-selection inside an editable field (a drag past the box edges keeps
		// selecting: `animate` replays this pos while the view crawls)
		if let Some(row) = self.edit_drag {
			let Some(field) = self.field_rect(row) else {
				return false;
			};
			let moved = if let Some(edit) = &mut self.edit {
				let rel_x = x - (field.x + lay().field_pad) + edit.view;
				let cur = caret_from_click(&edit.buf, rel_x, measure);
				if cur == edit.cur {
					false
				} else {
					if edit.sel.is_none() {
						edit.sel = Some(edit.cur);
					}
					edit.cur = cur;
					true
				}
			} else {
				false
			};
			// a click that turned into a drag keeps the dragged range, not select-all
			if moved {
				self.select_all_on_up = false;
			}
			return moved;
		}
		if let Some(oi) = self.open {
			let was = self.pending;
			let n = self.dd_options(oi).len();
			for choice in 0..n {
				if self.dd_item_rect(oi, n, choice).contains(x, y) {
					self.pending = choice;
					break;
				}
			}
			return self.pending != was;
		}
		if let Some(grab) = self.drag_thumb {
			let was = self.scroll;
			let vp = self.viewport();
			let thumb_h = self.thumb().map_or(lay().scrollbar_thumb_min, |t| t.h);
			let frac = ((y - grab - vp.y) / (vp.h - thumb_h).max(1.0)).clamp(0.0, 1.0);
			self.scroll = frac * self.max_scroll();
			return self.scroll.to_bits() != was.to_bits();
		}
		if let Some(grab) = self.drag_hthumb {
			let was = self.hscroll;
			let track = self.htrack();
			let thumb_w = self.hthumb().map_or(lay().scrollbar_thumb_min, |t| t.w);
			let frac = ((x - grab - track.x) / (track.w - thumb_w).max(1.0)).clamp(0.0, 1.0);
			self.hscroll = frac * self.max_hscroll();
			return self.hscroll.to_bits() != was.to_bits();
		}
		let Some(i) = self.drag else {
			return false;
		};
		let key = self.specs[i].key;
		let was = self.get_f32(key);
		self.drag_to(x);
		self.get_f32(key).to_bits() != was.to_bits()
	}
	// Release: end any slider/thumb drag, and fire an armed button's action only if
	// the cursor is still over it (a press that drifted off cancels).
	fn mouse_up_dip(&mut self, x: f32, y: f32) -> Action {
		// A release that ends a grip drag is not a click on anything: the list was
		// reordered as the pointer moved, and there is nothing left to fire.
		if self.shell_drag.take().is_some() {
			return Action::None;
		}
		if let Some(picker) = self.pick.as_mut() {
			picker.drag = None;
		}
		self.drag = None;
		self.drag_thumb = None;
		self.drag_hthumb = None;
		self.edit_drag = None;
		// an empty drag-selection collapses back to a plain caret
		if let Some(edit) = &mut self.edit {
			if edit.sel == Some(edit.cur) {
				edit.sel = None;
			}
		}
		// a fresh single-click field entry that never became a drag selects all, so
		// the next keystroke replaces the value (standard field entry)
		if std::mem::take(&mut self.select_all_on_up) {
			if let Some(edit) = &mut self.edit {
				if edit.sel.is_none() && !edit.buf.is_empty() {
					edit.sel = Some(0);
					edit.cur = edit.buf.len();
				}
			}
		}
		if let Some(btn_idx) = self.pressed.take() {
			let (action, r, _) = self.buttons()[btn_idx];
			if r.contains(x, y) {
				return action;
			}
		}
		if let Some((i, part)) = self.pressed_row.take() {
			if matches!(self.specs[i].kind, Kind::ShellList) {
				if self.shell_stop_rect(i, part).contains(x, y) {
					self.shell_activate(i, part);
				}
			} else if self.row_btn_rect(i, part).contains(x, y) {
				self.row_button(i, part);
			}
		}
		Action::None
	}

	fn drag_to(&mut self, x: f32) {
		let Some(i) = self.drag else { return };
		let Some(scale) = SliderScale::of(&self.specs[i].kind) else {
			return;
		};
		let track = self.track(i);
		let frac = ((x - track.x) / track.w).clamp(0.0, 1.0);
		let key = self.specs[i].key;
		// a press on a handle parked at the end keeps the number typed past it
		if frac >= 1.0 && self.get_f32(key) > scale.max {
			return;
		}
		self.set_f32(key, scale.at(frac));
	}

	pub fn char_input(&mut self, c: char) {
		self.dismiss_menu();
		if !self.types {
			return; // Ctrl+letter is a shortcut (copy/paste/...), never types
		}
		// typing into a keyboard-focused (but not-yet-open) field opens it with
		// the value selected, so the keystroke replaces it (standard field entry).
		// The delete confirmation has no field of its own, so without the prompt
		// test this would open the row sitting behind the box and edit that.
		if self.edit.is_none() {
			// the picker's own boxes are open whenever they hold focus, so there
			// is nothing here for a keystroke to fall through to
			if self.pick.is_some() {
				return;
			}
			let (Some(Focus::Row(i, _)), None) = (self.focus, self.prompt.as_ref()) else {
				return;
			};
			match self.specs[i].kind {
				Kind::Text | Kind::Color | Kind::Slider { .. } => self.open_edit(i, true),
				_ => return,
			}
		}
		if self.insert_char(c) {
			self.reparse_edit();
		}
	}
	// One char through the field's own validation (replacing any selection).
	// Returns whether the buffer changed; caller reparses.
	fn insert_char(&mut self, c: char) -> bool {
		let Some(edit) = &mut self.edit else {
			return false;
		};
		let sel_len = edit.sel_range().map_or(0, |(a, b)| b - a);
		// where the char would go once any selection is gone
		let landing = edit.sel_range().map_or(edit.cur, |(a, _)| a);
		// a picker value box: whole percents, or the Color row's own hex rule
		if let Some(field) = pick_field_of(edit.row) {
			if !field.accepts(c, edit.buf.len() - sel_len, landing == 0) {
				return false;
			}
			edit.remove_selection();
			edit.buf.insert(edit.cur, c);
			edit.cur += c.len_utf8();
			return true;
		}
		// a theme name, a shell's title or its command line: any ordinary
		// character, within a sane length (a command may be a long path)
		if edit.row >= PSEUDO_ROW {
			let cap = if shell_field_of(edit.row).is_some_and(|(_, cmd)| cmd) {
				512
			} else {
				64
			};
			if c.is_control() || edit.buf.len() - sel_len >= cap {
				return false;
			}
			edit.remove_selection();
			edit.buf.insert(edit.cur, c);
			edit.cur += c.len_utf8();
			return true;
		}
		let ok = match self.specs[edit.row].kind {
			Kind::Color => {
				(c == '#' || c.is_ascii_hexdigit())
					&& edit.buf.len() - sel_len < 7
					// '#' only makes sense up front
					&& (c != '#' || landing == 0)
			}
			Kind::Text => !c.is_control() && edit.buf.len() - sel_len < 256,
			// numeric slider field: digits always; one '.' only for float sliders
			Kind::Slider { int, .. } => {
				let kept = match edit.sel_range() {
					Some((a, b)) => format!("{}{}", &edit.buf[..a], &edit.buf[b..]),
					None => edit.buf.clone(),
				};
				let dot_ok = !int && c == '.' && !kept.contains('.');
				(c.is_ascii_digit() || dot_ok) && kept.len() < 8
			}
			_ => false,
		};
		if !ok {
			return false;
		}
		edit.remove_selection();
		edit.buf.insert(edit.cur, c);
		edit.cur += c.len_utf8();
		true
	}
	/// Paste: run the text through the same per-field validation, one char at a
	/// time (invalid chars are dropped, length caps hold).
	pub fn insert_str(&mut self, text: &str) {
		let mut changed = false;
		for c in text.chars() {
			changed |= self.insert_char(c);
		}
		if changed {
			self.reparse_edit();
		}
	}
	pub fn select_all(&mut self) {
		// Ctrl+A on a focused-but-closed field opens it first - but not through an
		// open prompt, whose delete variant leaves no edit for this to find
		if self.edit.is_none() && self.prompt.is_none() && self.pick.is_none() {
			if let Some(Focus::Row(i, _)) = self.focus {
				if matches!(
					self.specs[i].kind,
					Kind::Text | Kind::Color | Kind::Slider { .. }
				) {
					self.open_edit(i, true);
				}
			}
			return;
		}
		if let Some(edit) = &mut self.edit {
			edit.sel = (!edit.buf.is_empty()).then_some(0);
			edit.cur = edit.buf.len();
		}
	}
	pub fn selected_text(&self) -> Option<String> {
		let edit = self.edit.as_ref()?;
		let (a, b) = edit.sel_range()?;
		Some(edit.buf[a..b].to_string())
	}
	pub fn delete_selection(&mut self) {
		if let Some(edit) = &mut self.edit {
			if edit.remove_selection() {
				self.reparse_edit();
			}
		}
	}
	pub fn backspace(&mut self) {
		self.dismiss_menu();
		let reach = self.reach();
		if let Some(edit) = &mut self.edit {
			if edit.remove_selection() {
				self.reparse_edit();
				return;
			}
			if edit.cur > 0 {
				let prev = reach_left(&edit.buf, edit.cur, reach);
				edit.buf.replace_range(prev..edit.cur, "");
				edit.cur = prev;
				self.reparse_edit();
			}
		}
	}
	pub fn delete_forward(&mut self) {
		self.dismiss_menu();
		// Command+Delete is nothing on a Mac, so only the word key reaches far
		let reach = Reach::new(self.word, false);
		if let Some(edit) = &mut self.edit {
			if edit.remove_selection() {
				self.reparse_edit();
				return;
			}
			if edit.cur < edit.buf.len() {
				let next = reach_right(&edit.buf, edit.cur, reach);
				edit.buf.replace_range(edit.cur..next, "");
				self.reparse_edit();
			}
		}
	}
	// Caret movement within the focused field (Left/Right/Home/End). Shift
	// extends the selection; Ctrl jumps by words (Option on a Mac, where Command
	// goes to either end); a plain move collapses any selection to its edge
	// (standard).
	fn move_caret(&mut self, to: usize) {
		let shift = self.shift;
		if let Some(edit) = &mut self.edit {
			if shift {
				if edit.sel.is_none() {
					edit.sel = Some(edit.cur);
				}
			} else {
				edit.sel = None;
			}
			edit.cur = to;
			// an emptied extension drops the anchor so a lone Shift press is inert
			if edit.sel == Some(edit.cur) {
				edit.sel = None;
			}
		}
	}
	fn reach(&self) -> Reach {
		Reach::new(self.word, self.line)
	}
	pub fn cursor_left(&mut self) {
		let Some(edit) = &self.edit else { return };
		let reach = self.reach();
		// plain Left with a selection collapses to its start
		if !self.shift && reach != Reach::End {
			if let Some((a, _)) = edit.sel_range() {
				self.move_caret(a);
				return;
			}
		}
		self.move_caret(reach_left(&edit.buf, edit.cur, reach));
	}
	pub fn cursor_right(&mut self) {
		let Some(edit) = &self.edit else { return };
		let reach = self.reach();
		if !self.shift && reach != Reach::End {
			if let Some((_, b)) = edit.sel_range() {
				self.move_caret(b);
				return;
			}
		}
		self.move_caret(reach_right(&edit.buf, edit.cur, reach));
	}
	pub fn cursor_home(&mut self) {
		if self.edit.is_some() {
			self.move_caret(0);
		}
	}
	pub fn cursor_end(&mut self) {
		let Some(edit) = &self.edit else { return };
		let end = edit.buf.len();
		self.move_caret(end);
	}
	// live-apply the in-progress edit (hex color, or background-image path)
	fn reparse_edit(&mut self) {
		let Some((i, buf)) = self.edit.as_ref().map(|edit| (edit.row, edit.buf.clone())) else {
			return;
		};
		if i == PROMPT_ROW {
			// nothing to apply yet - but the name just changed, so whatever OK
			// last objected to may no longer be true
			if let Some(prompt) = self.prompt.as_mut() {
				prompt.warn = None;
			}
			return;
		}
		if let Some(field) = pick_field_of(i) {
			let Some(hsv) = self.pick.as_ref().and_then(|p| field.apply(p.hsv, &buf)) else {
				return;
			};
			self.pick_set(hsv);
			return;
		}
		if let Some((shell_index, command)) = shell_field_of(i) {
			let Some(entry) = self.edited.shells.get_mut(shell_index) else {
				return;
			};
			if command {
				// The command is REQUIRED, and this is where that is enforced:
				// a blank buffer simply isn't written, so committing an emptied
				// field leaves the stored command standing and the box shows it
				// again. (An entry that never got one is dropped on the way out
				// of the dialog - see `app::apply_settings_values`.)
				if !buf.trim().is_empty() {
					entry.command = buf.trim().to_string();
				}
			} else {
				entry.title = buf.trim().to_string();
			}
			return;
		}
		match self.specs[i].kind {
			// an emptied box puts an auto setting back to automatic
			Kind::Slider { .. } | Kind::Color
				if buf.trim().is_empty() && self.auto_of(self.specs[i].key).is_some() =>
			{
				self.back_to_automatic(i);
			}
			Kind::Color => {
				if let Some(color) = config::parse_hex(&buf) {
					self.set_col(self.specs[i].key, color);
				}
			}
			Kind::Text => self.set_text(self.specs[i].key, &buf),
			// a valid partial number applies live, clamped to what the box takes
			Kind::Slider { .. } => {
				if let (Some(scale), Ok(value)) = (
					SliderScale::of(&self.specs[i].kind),
					buf.trim().parse::<f32>(),
				) {
					self.set_f32(self.specs[i].key, scale.typed(value));
				}
			}
			_ => {}
		}
	}
	fn commit_edit(&mut self) {
		self.edit = None;
		self.emenu = None;
	}

	/// Esc cancels the dialog. A menu, a popup or the prompt box eats it first.
	///
	/// An open field does not. Closing the field was all it used to do, and since
	/// a typed value applies as it is typed there was nothing to take back - so
	/// the press bought a lost caret and a second Esc. Now that walking onto a
	/// field opens it, that would have been every field on the way past.
	pub fn key_escape(&mut self) -> Action {
		if self.emenu.take().is_some() || self.open.take().is_some() {
			Action::None
		} else if self.pick.is_some() {
			self.pick_cancel();
			Action::None
		} else if self.prompt.is_some() {
			self.prompt_close();
			Action::None
		} else {
			Action::Cancel
		}
	}
	pub fn key_enter(&mut self) -> Action {
		if self.emenu.is_some() {
			// fire the highlighted (enabled) menu item
			let cmd = self
				.emenu
				.as_ref()
				.and_then(|m| m.hover)
				.filter(|&k| self.em_enabled(k))
				.map(|k| EDIT_MENU[k].1);
			self.emenu = None;
			return cmd.map_or(Action::None, Action::Edit);
		}
		// Enter in the picker is its OK, unless Cancel is the focused button
		if let Some(picker) = self.pick.as_ref() {
			if picker.focus == pick::Focus::Cancel {
				self.pick_cancel();
			} else {
				self.pick_accept();
			}
			return Action::None;
		}
		// Enter in the prompt box is its OK, unless Cancel is the focused button
		if let Some(prompt) = self.prompt.as_ref() {
			if prompt.focus == PromptFocus::Cancel {
				self.prompt_close();
			} else {
				self.prompt_accept();
			}
			return Action::None;
		}
		if self.open.is_some() {
			self.dd_commit();
			Action::None
		} else if self.edit.is_some() {
			// Enter in a field is the dialog's OK, the way it is in any other
			// dialog. It used to close the field and stop there, so OK took two
			// presses. Values apply as they are typed, so closing the field first
			// only drops the caret.
			self.commit_edit();
			Action::Ok
		} else if let Some(Focus::Button(b)) = self.focus {
			self.buttons()[b].0 // a focused footer button
		} else if let Some(Focus::Row(i, part)) = self.focus {
			// a focused push-button is what Enter presses, not the dialog's OK -
			// and every stop in the shells grid is one of those or a field
			match self.specs[i].kind {
				Kind::Buttons(_) => {
					self.row_button(i, part);
					Action::None
				}
				Kind::ShellList => {
					self.shell_activate(i, part);
					Action::None
				}
				Kind::Color if part == 0 => {
					self.pick_open(i);
					Action::None
				}
				// Enter sets the hotkey, the way it presses a focused button
				Kind::Hotkey(_) => {
					self.capture_start(i);
					Action::None
				}
				_ => Action::Ok,
			}
		} else {
			Action::Ok
		}
	}

	// caret line (and selection highlight) inside a focused field, at the
	// measured prefix widths
	fn caret_quad(
		&self,
		colors: &Dlg,
		out: &mut Vec<RectInstance>,
		field: Rect,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		let Some(edit) = &self.edit else { return };
		let left = field.x + lay().field_pad - edit.view;
		let (lo, hi) = (field.x + 1.0, field.x + field.w - 1.0);
		// the caret's own x is the eased position (smooth caret travel); other
		// selection edges are exact
		let caret_x = edit
			.caret_vis
			.unwrap_or_else(|| measure(&edit.buf[..edit.cur]));
		if let Some((a, b)) = edit.sel_range() {
			let edge = |i: usize, measure: &mut dyn FnMut(&str) -> f32| {
				if i == edit.cur {
					caret_x
				} else {
					measure(&edit.buf[..i])
				}
			};
			let x1 = (left + edge(a, measure)).clamp(lo, hi);
			let x2 = (left + edge(b, measure)).clamp(lo, hi);
			if x2 > x1 {
				// the text draws after the rects, so it stays legible on top
				out.push(RectInstance {
					pos: [x1, field.y + 2.0],
					size: [x2 - x1, field.h - 4.0],
					color: config::srgb_f32(mix3(colors.field_bg, colors.focus_out, 0.45)),
					..Default::default()
				});
			}
		}
		let x = (left + caret_x).clamp(lo, hi - 1.5);
		// smooth blink: fade the bar toward the field bg instead of a hard on/off
		let color = mix3(colors.field_bg, colors.focus_out, edit.caret_alpha());
		out.push(RectInstance {
			pos: [x, field.y + 2.0],
			size: [1.5, field.h - 4.0],
			color: config::srgb_f32(color),
			..Default::default()
		});
	}

	// (fixed chrome, scrolled rows): the rows vec is drawn scissored to
	// `viewport()` so scrolled-out controls can't paint over the chrome.
	// `measure` gives the rendered width of a string in the UI font (for the caret).
	fn rects_dip(
		&self,
		line_h: f32,
		mut measure: impl FnMut(&str) -> f32,
	) -> (Vec<RectInstance>, Vec<RectInstance>) {
		let colors = dlg();
		let mut fixed = Vec::new();
		let mut out = Vec::new();
		// panel
		fixed.push(quad(
			self.rect.x,
			self.rect.y,
			self.rect.w,
			self.rect.h,
			colors.panel_bg,
		));
		border(&mut fixed, self.rect, 1.0, colors.panel_border);
		// The tab strip: a recessed gutter closed off by a rule, with the tabs
		// standing on that rule. The current one is a lighter gray rather than an
		// accent - it says "you are here", which is not the same job as the
		// highlight color's "look at this".
		let gut = self.gutter_rect();
		fixed.push(quad(gut.x, gut.y, gut.w, gut.h, colors.gutter));
		fixed.push(quad(gut.x, gut.y + gut.h, gut.w, 1.0, colors.panel_border));
		let strip = self.tab_strip();
		for tab in 0..self.tab_ws.len() {
			let r = clip_rect(self.tab_rect(tab), strip);
			if r.w <= 0.0 {
				continue; // scrolled right out of the strip
			}
			let active = tab == self.tab;
			fixed.push(quad(
				r.x,
				r.y,
				r.w,
				r.h,
				if active { colors.tab_hl } else { colors.tab_bg },
			));
		}
		// scrollbar (only when the active tab overflows the viewport)
		if let Some(thumb) = self.thumb() {
			let vp = self.viewport();
			fixed.push(quad(thumb.x, vp.y, thumb.w, vp.h, colors.track));
			fixed.push(quad(thumb.x, thumb.y, thumb.w, thumb.h, colors.handle));
		}
		// and the sideways one, in the clear space above the footer buttons
		if let Some(thumb) = self.hthumb() {
			let track = self.htrack();
			fixed.push(quad(track.x, track.y, track.w, track.h, colors.track));
			fixed.push(quad(thumb.x, thumb.y, thumb.w, thumb.h, colors.handle));
		}

		for i in 0..self.specs.len() {
			if self.specs[i].tab != self.tab || Self::header_is_tab_title(&self.specs[i]) {
				continue;
			}
			if !self.specs[i].warning.is_empty() {
				self.warning_quads(&colors, i, &mut out, &mut measure);
			}
			match self.specs[i].kind {
				Kind::Slider { .. } => {
					let off = self.disabled(self.specs[i].key);
					let track = self.track(i);
					out.push(quad(track.x, track.y, track.w, track.h, colors.track));
					let value = self.get_f32(self.specs[i].key);
					let frac = SliderScale::of(&self.specs[i].kind).map_or(0.0, |s| s.frac(value));
					let handle_x = track.x + frac * track.w - SLIDER_HANDLE_W / 2.0;
					out.push(quad(
						handle_x,
						track.y - 6.0,
						SLIDER_HANDLE_W,
						track.h + 12.0,
						if off {
							colors.panel_border
						} else {
							colors.handle
						},
					));
					// editable numeric field
					let val_box = self.valbox(i);
					out.push(quad(
						val_box.x,
						val_box.y,
						val_box.w,
						val_box.h,
						colors.field_bg,
					));
					let focused = matches!(&self.edit, Some(edit) if edit.row == i);
					if !self.ring_on(i, 1) {
						border(
							&mut out,
							val_box,
							1.0,
							if focused && !off {
								colors.focus_out
							} else {
								colors.panel_border
							},
						);
					}
					if focused && !off {
						self.caret_quad(&colors, &mut out, val_box, &mut measure);
					}
					self.auto_slot_quads(&colors, i, &mut out);
				}
				Kind::Color => {
					let swatch = self.swatch(i);
					out.push(quad(
						swatch.x,
						swatch.y,
						swatch.w,
						swatch.h,
						self.get_col(self.specs[i].key),
					));
					if !self.ring_on(i, 0) {
						border(&mut out, swatch, 1.0, colors.panel_border);
					}
					let hex_box = self.hexbox(i);
					out.push(quad(
						hex_box.x,
						hex_box.y,
						hex_box.w,
						hex_box.h,
						colors.field_bg,
					));
					let focused = matches!(&self.edit, Some(edit) if edit.row == i);
					if !self.ring_on(i, 1) {
						border(
							&mut out,
							hex_box,
							1.0,
							if focused {
								colors.focus_out
							} else {
								colors.panel_border
							},
						);
					}
					if focused {
						self.caret_quad(&colors, &mut out, hex_box, &mut measure);
					}
					self.auto_slot_quads(&colors, i, &mut out);
				}
				Kind::Text => {
					let text_box = self.textbox(i);
					out.push(quad(
						text_box.x,
						text_box.y,
						text_box.w,
						text_box.h,
						colors.field_bg,
					));
					let focused = matches!(&self.edit, Some(edit) if edit.row == i);
					if !self.ring_on(i, 0) {
						border(
							&mut out,
							text_box,
							1.0,
							if focused {
								colors.focus_out
							} else {
								colors.panel_border
							},
						);
					}
					if focused {
						self.caret_quad(&colors, &mut out, text_box, &mut measure);
					}
					self.auto_slot_quads(&colors, i, &mut out);
				}
				Kind::Hotkey(_) => {
					let key_box = self.textbox(i);
					out.push(quad(
						key_box.x,
						key_box.y,
						key_box.w,
						key_box.h,
						colors.field_bg,
					));
					if !self.ring_on(i, 0) {
						border(
							&mut out,
							key_box,
							1.0,
							if self.capture == Some(i) {
								colors.focus_out
							} else {
								colors.panel_border
							},
						);
					}
				}
				Kind::Toggle => {
					let off = self.disabled(self.specs[i].key);
					let check_box = self.checkbox(i);
					out.push(quad(
						check_box.x,
						check_box.y,
						check_box.w,
						check_box.h,
						colors.field_bg,
					));
					border(&mut out, check_box, 1.0, colors.panel_border);
					// filled inner square when on (the checkmark glyph is drawn in texts)
					if self.get_toggle(self.specs[i].key) {
						out.push(quad(
							check_box.x + 4.0,
							check_box.y + 4.0,
							check_box.w - 8.0,
							check_box.h - 8.0,
							if off {
								colors.panel_border
							} else {
								colors.handle
							},
						));
					} else if self.group_state(i) == Some(config::auto::State::Mixed) {
						// a group's switch with some members set by hand: a dash
						let bar = (check_box.h * 0.16).round().max(2.0);
						out.push(quad(
							check_box.x + 4.0,
							check_box.y + ((check_box.h - bar) / 2.0).round(),
							check_box.w - 8.0,
							bar,
							colors.handle,
						));
					}
				}
				Kind::Dual { keys, .. } => {
					for part in 0u16..2 {
						let off = self.disabled(keys[part as usize]);
						let bx = self.dual_box(i, part);
						out.push(quad(bx.x, bx.y, bx.w, bx.h, colors.field_bg));
						border(&mut out, bx, 1.0, colors.panel_border);
						if self.get_toggle(keys[part as usize]) {
							out.push(quad(
								bx.x + 4.0,
								bx.y + 4.0,
								bx.w - 8.0,
								bx.h - 8.0,
								if off {
									colors.panel_border
								} else {
									colors.handle
								},
							));
						}
					}
				}
				Kind::Radio(options) => {
					let sel = self.get_radio(self.specs[i].key);
					for choice in 0..options.len() {
						let radio_rect = self.radio_box(i, choice);
						out.push(quad(
							radio_rect.x,
							radio_rect.y,
							radio_rect.w,
							radio_rect.h,
							colors.field_bg,
						));
						border(&mut out, radio_rect, 1.0, colors.panel_border);
						if choice == sel {
							out.push(quad(
								radio_rect.x + 4.0,
								radio_rect.y + 4.0,
								radio_rect.w - 8.0,
								radio_rect.h - 8.0,
								colors.handle,
							));
						}
					}
				}
				Kind::Dropdown(_) => {
					// collapsed box only; the open popup is drawn in the overlay pass
					let off = self.disabled(self.specs[i].key);
					let box_r = self.dd_box(i);
					out.push(quad(box_r.x, box_r.y, box_r.w, box_r.h, colors.field_bg));
					if !self.ring_on(i, 0) {
						border(
							&mut out,
							box_r,
							1.0,
							if self.open == Some(i) && !off {
								colors.focus_out
							} else {
								colors.panel_border
							},
						);
					}
				}
				// A pressed button fills with the highlight, the same click feedback
				// the footer gives. Its outline is left to the focus ring below,
				// which sits exactly on this box rather than outside it.
				Kind::Buttons(captions) => {
					for part in 0..captions.len() as u16 {
						let r = self.row_btn_rect(i, part);
						let fill = if self.pressed_row == Some((i, part)) {
							colors.btn_hl
						} else {
							colors.btn_bg
						};
						out.push(quad(r.x, r.y, r.w, r.h, fill));
						if !self.ring_on(i, part) {
							border(&mut out, r, 1.0, colors.panel_border);
						}
					}
				}
				Kind::ShellList => {
					self.shell_rects(&colors, i, &mut out, &mut measure);
				}
				Kind::Header(_) => {
					let y = self.header_rule_y(i);
					let x = self.content_x() + lay().pad;
					out.push(quad(
						x,
						y,
						self.layout_w() - lay().pad * 2.0,
						1.0,
						colors.panel_border,
					));
				}
			}
		}
		// keyboard-focus ring around the active control row (scrolls + clips with
		// the rows; a focused button is ringed below, in the fixed chrome).
		if let Some(Focus::Row(fr, fp)) = self.focus {
			if self.specs[fr].tab == self.tab && !matches!(self.specs[fr].kind, Kind::Header(_)) {
				let r = self.focus_ctl_rect(fr, fp);
				let inset = if self.ring_is_the_box(fr, fp) {
					0.0
				} else {
					2.0
				};
				let ring = Rect {
					x: r.x - inset,
					y: r.y - inset,
					w: r.w + inset * 2.0,
					h: r.h + inset * 2.0,
				};
				border(&mut out, ring, 1.0, colors.focus_out);
			}
		}
		for (btn_idx, (_, r, label)) in self.buttons().into_iter().enumerate() {
			// pressed button fills with the highlight for click feedback
			let fill = if self.pressed == Some(btn_idx) {
				colors.btn_hl
			} else {
				colors.btn_bg
			};
			fixed.push(quad(r.x, r.y, r.w, r.h, fill));
			let ring = self.focus == Some(Focus::Button(btn_idx));
			// Only the default button (OK) is outlined in the highlight color;
			// the others take the same quiet gray the tabs use, so "this is the
			// one Enter fires" stays a single, readable signal.
			let outline = if ring {
				colors.focus_out
			} else if btn_idx == 2 {
				colors.btn_hl
			} else {
				colors.panel_border
			};
			border(&mut fixed, r, if ring { 2.0 } else { 1.0 }, outline);
			// Alt held: underline the accelerator (the label's first letter). The
			// label is drawn centered on the button; the cap glyph is ~0.55*line_h
			// wide, and its baseline sits near the text bottom.
			if self.alt && !label.is_empty() {
				let tx = r.x + (r.w - measure(label)).max(0.0) / 2.0;
				let ty = r.y + (r.h - line_h) / 2.0 + line_h * 0.82;
				fixed.push(quad(tx, ty, line_h * 0.5, 1.5, colors.text));
			}
		}
		(fixed, out)
	}

	// A triangle with an exclamation mark cut out of it, in the label's color. All
	// quads, for the same reason as the grid's icons: no interface font can be
	// relied on to carry the sign. Not red, since red is only for removal.
	fn warning_quads(
		&self,
		colors: &Dlg,
		i: usize,
		out: &mut Vec<RectInstance>,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		let r = self.warning_box(i, measure);
		let color = if self.disabled(self.specs[i].key) {
			colors.dim
		} else {
			colors.text
		};
		out.push(RectInstance {
			pos: [r.x, r.y],
			size: [r.w, r.h],
			color: config::srgb_f32(color),
			params: [QuadMode::Triangle.code(), 3.0],
		});
		let stroke = (r.w * 0.13).max(1.5);
		let x = r.x + (r.w - stroke) / 2.0;
		let cut = config::srgb_f32(colors.panel_bg);
		for (top, h) in [(0.36, 0.32), (0.76, 0.0)] {
			out.push(RectInstance {
				pos: [x, r.y + r.h * top],
				size: [stroke, (r.h * h).max(stroke)],
				color: cut,
				..Default::default()
			});
		}
	}

	// `line_h` is the rendered text line height (the app's cell_h); rows, hex
	// fields, and buttons center their text vertically against it so alignment
	// holds for any font/size rather than a baked-in guess.
	fn texts_dip(&self, line_h: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<TextItem> {
		let colors = dlg();
		let mut out = Vec::new();
		let mk = |text: String, x: f32, y: f32| TextItem::plain(text, x, y, colors.text);
		let row_text_y = |y: f32, h: f32| y + (h - line_h) / 2.0;
		// an automatic value: readable, and set apart from one set by hand
		let placeholder = mix3(colors.text, colors.dim, 0.5);
		// tab titles - the current one reads at full strength, the rest step back
		let strip = self.tab_strip();
		for (tab, title) in tab_titles().iter().enumerate() {
			let r = self.tab_rect(tab);
			out.push(TextItem {
				color: if tab == self.tab {
					colors.text
				} else {
					colors.dim
				},
				clip: Some(strip),
				..mk(
					(*title).into(),
					r.x + lay().tab_pad / 2.0,
					row_text_y(r.y, r.h),
				)
			});
		}
		// row text clips to the scroll viewport so it can't ride over the chrome
		let vp = self.viewport();
		for i in 0..self.specs.len() {
			if self.specs[i].tab != self.tab || Self::header_is_tab_title(&self.specs[i]) {
				continue;
			}
			let ty = row_text_y(self.row_y(i), self.line_row_h());
			if let Kind::Header(section) = self.specs[i].kind {
				// heading near the top of the row; the rule sits lower (gap between)
				let hy = self.row_y(i) + 5.0;
				out.push(TextItem {
					bold: true,
					clip: Some(vp),
					..mk(section.into(), self.content_x() + lay().pad, hy)
				});
				continue;
			}
			let off = self.disabled(self.specs[i].key);
			let label_color = if off { colors.dim } else { colors.text };
			// a half-line whose control says what it is carries no label at all
			if !self.specs[i].label.is_empty() {
				out.push(TextItem {
					color: label_color,
					clip: Some(vp),
					..mk(self.specs[i].label.into(), self.label_x(i), ty)
				});
			}
			// revert-to-default icon: bright + clickable when off-default, dim when at it
			if self.has_revert(i) {
				let revert_rect = self.revert_box(i);
				out.push(TextItem {
					color: if self.row_is_default(i) {
						colors.dim
					} else {
						colors.handle
					},
					clip: Some(vp),
					..mk(ui().icons.revert.into(), revert_rect.x + REVERT_INSET, ty)
				});
			}
			// horizontal view offset of row i's field while it's being edited (the
			// text slides left as the view scrolls; the box clip crops the rest)
			let view = |i: usize| -> f32 {
				self.edit
					.as_ref()
					.filter(|e| e.row == i)
					.map_or(0.0, |e| e.view)
			};
			match self.specs[i].kind {
				Kind::Slider { int, .. } => {
					let val_box = self.valbox(i);
					let txt = match &self.edit {
						Some(edit) if edit.row == i => edit.buf.clone(),
						_ => self.fmt_val(self.specs[i].key, int),
					};
					let automatic = self.shows_automatic(i);
					out.push(TextItem {
						color: if automatic { placeholder } else { label_color },
						italic: automatic,
						clip: Some(clip_rect(self.text_room(i, val_box), vp)),
						..mk(
							txt,
							val_box.x + lay().field_pad - view(i),
							row_text_y(val_box.y, val_box.h),
						)
					});
					self.auto_mark_text(&colors, i, line_h, vp, &mut out, &mut measure);
				}
				Kind::Color => {
					let hex_box = self.hexbox(i);
					let txt = match &self.edit {
						Some(edit) if edit.row == i => edit.buf.clone(),
						_ => config::format_hex(self.get_col(self.specs[i].key)),
					};
					let automatic = self.shows_automatic(i);
					out.push(TextItem {
						color: if automatic { placeholder } else { colors.text },
						italic: automatic,
						clip: Some(clip_rect(self.text_room(i, hex_box), vp)),
						..mk(
							txt,
							hex_box.x + lay().field_pad - view(i),
							row_text_y(hex_box.y, hex_box.h),
						)
					});
					self.auto_mark_text(&colors, i, line_h, vp, &mut out, &mut measure);
				}
				Kind::Text => {
					let text_box = self.textbox(i);
					let val = match &self.edit {
						Some(edit) if edit.row == i => edit.buf.clone(),
						_ => self.get_text(self.specs[i].key),
					};
					let automatic = self.shows_automatic(i);
					let (txt, color) = if val.is_empty() || self.disabled(self.specs[i].key) {
						(
							if val.is_empty() {
								"(none)".to_string()
							} else {
								val
							},
							colors.dim,
						)
					} else if automatic {
						(val, placeholder)
					} else {
						(val, colors.text)
					};
					out.push(TextItem {
						color,
						italic: automatic,
						clip: Some(clip_rect(self.text_room(i, text_box), vp)),
						..mk(
							txt,
							text_box.x + lay().field_pad - view(i),
							row_text_y(text_box.y, text_box.h),
						)
					});
					self.auto_mark_text(&colors, i, line_h, vp, &mut out, &mut measure);
				}
				Kind::Hotkey(_) => {
					let key_box = self.textbox(i);
					let (shown, off, note) = self.hotkey_text(i);
					let ty = row_text_y(key_box.y, key_box.h);
					let mut tx = key_box.x + lay().field_pad;
					if !shown.is_empty() {
						let width = measure(&shown);
						out.push(TextItem {
							color: if off { colors.dim } else { colors.text },
							clip: Some(clip_rect(key_box, vp)),
							..mk(shown, tx, ty)
						});
						tx += width + line_h;
					}
					if !note.is_empty() {
						out.push(TextItem {
							color: colors.dim,
							clip: Some(clip_rect(key_box, vp)),
							..mk(note, tx, ty)
						});
					}
				}
				Kind::Dual { keys, labels } => {
					for part in 0u16..2 {
						let off = self.disabled(keys[part as usize]);
						let color = if off { colors.dim } else { colors.text };
						let bx = self.dual_box(i, part);
						out.push(TextItem {
							color,
							clip: Some(vp),
							..mk(labels[part as usize].into(), self.label_after(bx), ty)
						});
					}
				}
				Kind::Radio(options) => {
					let off = self.disabled(self.specs[i].key);
					let color = if off { colors.dim } else { colors.text };
					for (choice, opt) in options.iter().enumerate() {
						let radio_rect = self.radio_box(i, choice);
						out.push(TextItem {
							color,
							clip: Some(vp),
							..mk((*opt).into(), self.label_after(radio_rect), ty)
						});
					}
				}
				Kind::Dropdown(_) => {
					let off = self.disabled(self.specs[i].key);
					let color = if off { colors.dim } else { colors.text };
					let box_r = self.dd_box(i);
					let label = self.dd_closed_label(i);
					out.push(TextItem {
						color,
						clip: Some(clip_rect(box_r, vp)),
						..mk(label, box_r.x + 8.0, row_text_y(box_r.y, box_r.h))
					});
					out.push(TextItem {
						color,
						clip: Some(vp),
						..mk(
							ui().icons.dropdown_arrow.into(),
							box_r.x + box_r.w - 18.0,
							row_text_y(box_r.y, box_r.h),
						)
					});
				}
				Kind::Buttons(captions) => {
					for (part, caption) in captions.iter().enumerate() {
						let r = self.row_btn_rect(i, part as u16);
						let color = if self.part_disabled(i, part as u16) {
							colors.dim
						} else {
							colors.text
						};
						let lx = r.x + (r.w - measure(caption)).max(0.0) / 2.0;
						out.push(TextItem {
							color,
							clip: Some(vp),
							..mk((*caption).into(), lx, row_text_y(r.y, r.h))
						});
					}
				}
				Kind::ShellList => {
					let cols = self.shell_cols();
					// Column titles, once, above the whole grid. The grip and the
					// remove button get none: neither is a value, and a title over
					// either would read as a column of data that is not there.
					let head_y = self.shell_head_y(i);
					for (title, tx) in [
						("Name", cols.name),
						("Command", cols.command),
						(shell_grid::SEEN_TITLE, cols.seen),
						(shell_grid::ACTIVE_TITLE, cols.active),
					] {
						out.push(TextItem {
							color: colors.dim,
							clip: Some(vp),
							..mk((*title).to_string(), tx, head_y)
						});
					}
					for shell_index in 0..self.edited.shells.len() {
						let name_box = self.shell_name_box(i, shell_index);
						let cmd_box = self.shell_cmd_box(i, shell_index);
						let name_row = shell_field_row(shell_index, false);
						let cmd_row = shell_field_row(shell_index, true);
						let entry = &self.edited.shells[shell_index];
						// an inactive shell is still listed, but reads as parked
						let color = if entry.active {
							colors.text
						} else {
							colors.dim
						};
						let text = |row: usize, stored: &str| -> String {
							match &self.edit {
								Some(edit) if edit.row == row => edit.buf.clone(),
								_ => stored.to_string(),
							}
						};
						out.push(TextItem {
							color,
							clip: Some(clip_rect(name_box, vp)),
							..mk(
								text(name_row, &entry.title),
								name_box.x + lay().field_pad - view(name_row),
								row_text_y(name_box.y, name_box.h),
							)
						});
						let cmd = text(cmd_row, &entry.command);
						let (cmd, cmd_color) = if cmd.is_empty() {
							("(required)".to_string(), colors.dim)
						} else {
							(cmd, color)
						};
						out.push(TextItem {
							color: cmd_color,
							clip: Some(clip_rect(cmd_box, vp)),
							..mk(
								cmd,
								cmd_box.x + lay().field_pad - view(cmd_row),
								row_text_y(cmd_box.y, cmd_box.h),
							)
						});
						// Last seen is the program's own note, never edited here
						let seen_text = if entry.last_seen.is_empty() {
							shell_grid::NEVER_SEEN.to_string()
						} else {
							entry.last_seen.clone()
						};
						out.push(TextItem {
							color: colors.dim,
							clip: Some(vp),
							..mk(
								seen_text,
								cols.seen,
								row_text_y(self.shell_line_y(i, shell_index), self.shell_line_h()),
							)
						});
					}
					let add = self.shell_add_box(i);
					let caption = "Add";
					let lx = add.x + (add.w - measure(caption)).max(0.0) / 2.0;
					out.push(TextItem {
						clip: Some(vp),
						..mk(caption.to_string(), lx, row_text_y(add.y, add.h))
					});
				}
				Kind::Toggle | Kind::Header(_) => {}
			}
		}
		for (_, r, label) in self.buttons() {
			// center the caption within the button
			let lx = r.x + (r.w - measure(label)).max(0.0) / 2.0;
			out.push(mk(label.into(), lx, row_text_y(r.y, r.h)));
		}
		out
	}

	// The open dropdown's popup, as (rects, text), for a second (LoadOp::Load) pass
	// drawn on top of the dialog so the covered rows' text can't bleed through the
	// opaque box (same reason the context menu uses its own pass). Empty when closed.
	fn dropdown_overlay(&self, colors: &Dlg) -> (Vec<RectInstance>, Vec<TextItem>) {
		let mut rects = Vec::new();
		let mut texts = Vec::new();
		let Some(i) = self.open else {
			return (rects, texts);
		};
		let options = self.dd_options(i);
		let n = options.len();
		if n == 0 {
			return (rects, texts);
		}
		let popup = self.dd_popup(i, n);
		rects.push(quad(popup.x, popup.y, popup.w, popup.h, colors.field_bg));
		border(&mut rects, popup, 1.0, colors.panel_border);
		let sel = self.get_radio(self.specs[i].key);
		let mk = |text: String, x: f32, y: f32| TextItem::plain(text, x, y, colors.text);
		for (choice, opt) in options.iter().enumerate() {
			let r = self.dd_item_rect(i, n, choice);
			if choice == self.pending {
				rects.push(quad(r.x + 1.0, r.y, r.w - 2.0, r.h, colors.btn_hl));
			}
			let ty = r.y + (r.h - self.line_h) / 2.0;
			if choice == sel {
				texts.push(mk(ui().icons.dropdown_check.into(), r.x + r.w - 18.0, ty));
			}
			texts.push(mk(opt.clone(), r.x + 10.0, ty));
		}
		(rects, texts)
	}

	/// True when anything needs the second (on-top) render pass.
	pub fn overlay_open(&self) -> bool {
		self.open.is_some() || self.emenu.is_some() || self.modal()
	}
	// A box that owns every click and key while it is up.
	fn modal(&self) -> bool {
		self.prompt.is_some() || self.pick.is_some()
	}
	// Everything for the second pass: the open dropdown popup and/or the field
	// context menu (only one is ever open at a time in practice).
	fn overlay_dip(
		&self,
		measure: &mut impl FnMut(&str) -> f32,
	) -> (Vec<RectInstance>, Vec<TextItem>) {
		let colors = dlg();
		let (mut rects, mut texts) = self.dropdown_overlay(&colors);
		// the prompt box sits over everything, including an open popup
		let (prompt_rects, prompt_texts) = self.prompt_overlay(&colors, measure);
		rects.extend(prompt_rects);
		texts.extend(prompt_texts);
		let (pick_rects, pick_texts) = self.pick_overlay(&colors, measure);
		rects.extend(pick_rects);
		texts.extend(pick_texts);
		if self.emenu.is_none() {
			return (rects, texts);
		}
		let menu = self.em_rect();
		let t = 1.0;
		rects.push(quad(
			menu.x - t,
			menu.y - t,
			menu.w + 2.0 * t,
			menu.h + 2.0 * t,
			colors.panel_border,
		));
		rects.push(quad(menu.x, menu.y, menu.w, menu.h, colors.field_bg));
		let hover = self.emenu.as_ref().and_then(|m| m.hover);
		for (item, (label, _)) in EDIT_MENU.iter().enumerate() {
			let r = self.em_item_rect(item);
			let enabled = self.em_enabled(item);
			if enabled && hover == Some(item) {
				rects.push(quad(r.x + 1.0, r.y, r.w - 2.0, r.h, colors.btn_hl));
			}
			texts.push(TextItem {
				text: (*label).into(),
				x: r.x + 10.0,
				y: r.y + (r.h - self.line_h) / 2.0,
				color: if enabled { colors.text } else { colors.dim },
				clip: None,
				bold: false,
				italic: false,
				scale: 1.0,
			});
		}
		(rects, texts)
	}
}

// A measured width plus the clear space that goes around it. The measurement is
// physical and the clear space is DIP, so the constant converts before they meet.
fn measured_plus(measured_px: f32, clear_dip: f32, scale: f32) -> f32 {
	measured_px + config::dip(clear_dip, scale)
}

/// A scale factor the boundary can divide by. A monitor that reports nothing
/// useful must not take the layout to zero or NaN.
pub fn sane_scale(scale: f32) -> f32 {
	if scale.is_finite() && scale > 0.0 {
		scale
	} else {
		1.0
	}
}

/// The chrome's text, measured in the UI font. `chrome_widths` fills it in
/// physical pixels; `new` and `rescale` take it whole and turn it into DIP once,
/// through `in_dip`, which is also where each declared floor applies. A column
/// that holds text belongs here, or it stays one size while its text grows
/// (2026100818102267).
#[derive(Debug, Clone, Default)]
pub struct Chrome {
	pub label_w: f32,
	pub btn_w: f32,
	pub row_btn_w: f32, // push-buttons that sit on a row
	pub value_w: f32,   // every slider's number box
	pub tab_ws: Vec<f32>,
	pub label_ws: Vec<f32>,
	pub revert_w: f32,     // the revert arrow's column
	pub seen_w: f32,       // the shells grid's "Last seen": its title, a date or "never"
	pub active_w: f32,     // the shells grid's "Active" title
	pub pick_label_w: f32, // the color picker's label column
	pub pick_field_w: f32, // the color picker's value boxes, "#rrggbb" the widest
}

impl Chrome {
	fn in_dip(self, scale: f32) -> Chrome {
		let l = lay();
		let dip = |w: f32| w / scale;
		// Whole DIP for the text columns, rounded down: a part pixel spills into
		// the clear space after it rather than moving a column at the default
		// font, where the date measures 78.3 against the 78 it was drawn for.
		let whole = |w: f32| (w / scale).floor();
		Chrome {
			label_w: dip(self.label_w).max(l.label_width),
			btn_w: dip(self.btn_w).max(l.button_width),
			row_btn_w: dip(self.row_btn_w).max(l.button_width),
			value_w: dip(self.value_w).max(l.value_width),
			tab_ws: self.tab_ws.into_iter().map(dip).collect(),
			label_ws: self.label_ws.into_iter().map(dip).collect(),
			revert_w: whole(self.revert_w).max(l.revert_width),
			seen_w: whole(self.seen_w).max(l.shell_seen_width),
			active_w: whole(self.active_w).max(l.shell_active_width),
			pick_label_w: whole(self.pick_label_w).max(l.pick_label_width),
			pick_field_w: whole(self.pick_field_w).max(l.pick_field_width),
		}
	}
}

/// Widest field label, button caption, slider number, per-tab title and text
/// column widths at the current UI font, so the dialog sizes to the real text (a
/// wide serif or a big desktop size never truncates).
///
/// This measures against the text context, so it works in PHYSICAL pixels - which
/// is why every layout constant it reads converts through `config::dip` at its use
/// site, the way the main window's chrome does. Adding a raw DIP number to a
/// physical measurement here is a live bug: `SettingsDialog::new` divides the whole
/// sum by the scale factor, so the constant arrives shrunk by that factor. That is
/// what put a tab's title `tab_pad/2` from its left edge inside a box only
/// `tab_pad/scale` wider than the title - flush right at 2x, overflowing past it
/// above that.
pub fn chrome_widths(text: &mut crate::text::TextCtx, scale: f32) -> Chrome {
	let attrs = crate::text::ui_attrs();
	let dip = |v: f32| config::dip(v, scale);
	// an indented label starts further right, so the column has to clear the
	// deepest one plus its own indent - not merely the longest string - and a
	// warning mark after one
	let line_h = text.ui_line_h;
	let specs = &ui().specs;
	let label_w = specs
		.iter()
		.enumerate()
		// a packed line's labels sit beside their own boxes, not in the column,
		// bar a first one that keeps it
		.filter(|&(i, _)| SettingsDialog::in_label_column(specs, i))
		.map(|(_, spec)| {
			let mark = if spec.warning.is_empty() {
				0.0
			} else {
				warning_room(line_h)
			};
			text.measure_ui_text(spec.label, &attrs)
				+ f32::from(spec.indent) * dip(lay().indent)
				+ mark
		})
		.fold(0.0f32, f32::max);
	let label_w = measured_plus(label_w, lay().label_gap, scale);
	let btn_w: f32 = ["Cancel", "Apply", "OK"]
		.iter()
		.map(|caption| text.measure_ui_text(caption, &attrs))
		.fold(0.0f32, f32::max);
	let btn_w = measured_plus(btn_w, lay().button_pad, scale);
	// the buttons that sit on a row are measured apart from the footer's, so a
	// long caption there widens its own row instead of every button in the dialog
	let row_btn_w = ui()
		.specs
		.iter()
		.filter_map(|spec| match spec.kind {
			Kind::Buttons(captions) => Some(captions),
			_ => None,
		})
		.flatten()
		.map(|caption| text.measure_ui_text(caption, &attrs))
		.fold(0.0f32, f32::max);
	let row_btn_w = measured_plus(row_btn_w, lay().button_pad, scale);
	// one box width for every slider, the widest any of them can show
	let value_w = specs
		.iter()
		.filter_map(|spec| SliderScale::of(&spec.kind))
		.flat_map(SliderScale::widest_texts)
		.map(|number| text.measure_ui_text(&number, &attrs))
		.fold(0.0f32, f32::max);
	let value_w = measured_plus(value_w, lay().field_pad * 2.0, scale);
	let tab_ws = tab_titles()
		.iter()
		.map(|title| measured_plus(text.measure_ui_text(title, &attrs), lay().tab_pad, scale))
		.collect();
	// a label on a shared line sits in its own part, before its box, so the part
	// has to know how long it is
	let label_ws = ui()
		.specs
		.iter()
		.map(|spec| text.measure_ui_text(spec.label, &attrs))
		.collect();
	let revert_w = measured_plus(
		widest_text(text, &attrs, &[ui().icons.revert]),
		REVERT_INSET * 2.0,
		scale,
	);
	// dates and hex values come from the data, so each is measured at its
	// font's widest digit
	let digit = widest_char(text, &attrs, "0123456789");
	let date = [4, 2, 2].map(|n| digit.repeat(n)).join("-");
	let seen_w = widest_text(
		text,
		&attrs,
		&[shell_grid::SEEN_TITLE, &date, shell_grid::NEVER_SEEN],
	);
	let active_w = widest_text(text, &attrs, &[shell_grid::ACTIVE_TITLE]);
	let pick_labels: Vec<&str> = crate::pick::Field::ALL.iter().map(|f| f.label()).collect();
	let pick_label_w = measured_plus(
		widest_text(text, &attrs, &pick_labels),
		font_gap(PART_LABEL_GAP, line_h / scale),
		scale,
	);
	let hex = format!(
		"#{}",
		widest_char(text, &attrs, "0123456789abcdef").repeat(6)
	);
	let pick_field_w = measured_plus(
		widest_text(text, &attrs, &[&hex, &digit.repeat(3)]),
		lay().field_pad * 2.0,
		scale,
	);
	Chrome {
		label_w,
		btn_w,
		row_btn_w,
		value_w,
		tab_ws,
		label_ws,
		revert_w,
		seen_w,
		active_w,
		pick_label_w,
		pick_field_w,
	}
}

fn widest_text(text: &mut crate::text::TextCtx, attrs: &glyphon::Attrs, texts: &[&str]) -> f32 {
	texts
		.iter()
		.map(|s| text.measure_ui_text(s, attrs))
		.fold(0.0f32, f32::max)
}

// The character of `set` that draws widest in the UI font, as a string.
fn widest_char(text: &mut crate::text::TextCtx, attrs: &glyphon::Attrs, set: &str) -> String {
	let mut best = (String::new(), -1.0f32);
	for c in set.chars() {
		let s = c.to_string();
		let w = text.measure_ui_text(&s, attrs);
		if w > best.1 {
			best = (s, w);
		}
	}
	best.0
}

/// Returns true if `old` and `new` differ in any field that needs a text-context
/// rebuild (cell metrics change) rather than just a re-render.
pub fn needs_text_rebuild(old: &Settings, new: &Settings) -> bool {
	config::auto::font_size(old) != config::auto::font_size(new)
		|| old.line_height_scale != new.line_height_scale
		|| config::auto::font_family(old) != config::auto::font_family(new)
		|| old.margin != new.margin
}

/// Returns true if a background-image-affecting setting changed.
pub fn wallpaper_changed(old: &Settings, new: &Settings) -> bool {
	old.wallpaper_enabled != new.wallpaper_enabled
		|| old.wallpaper_rotate_enabled != new.wallpaper_rotate_enabled
		|| old.wallpaper_opacity != new.wallpaper_opacity
		|| old.wallpaper_default_fit != new.wallpaper_default_fit
		|| old.wallpaper_honor_xmp != new.wallpaper_honor_xmp
		|| old.wallpaper_honor_xmp_look != new.wallpaper_honor_xmp_look
		|| old.wallpaper != new.wallpaper
		|| old.wallpaper_blur != new.wallpaper_blur
		|| old.wallpaper_contrast_mask != new.wallpaper_contrast_mask
		|| old.wallpaper_contrast_mask_size != new.wallpaper_contrast_mask_size
		|| old.wallpaper_contrast_mask_strength != new.wallpaper_contrast_mask_strength
		|| old.wallpaper_contrast_mask_auto != new.wallpaper_contrast_mask_auto
		// a profile change can move how small a blurred picture is held
		|| crate::wallpaper::per_sigma(old) != crate::wallpaper::per_sigma(new)
		// and whether it goes up as BC1 or BC7
		|| crate::wallpaper::packing(old) != crate::wallpaper::packing(new)
}

#[cfg(test)]
mod tests {
	use super::{
		Chrome, EASE_IN_MAX, EASE_IN_MIN, EASE_OUT_MAX, EASE_OUT_MIN, Key, Kind, RAMP_DOWN_MAX,
		RAMP_DOWN_MIN, RAMP_UP_MAX, RAMP_UP_MIN, SettingsDialog, TAU_MAX, TAU_MIN, falling_slider,
		lay, speed_to_tau, tab_titles, tau_to_speed,
	};
	use crate::config;
	use crate::gfx::QuadMode;

	// A text auto setting as a box left with `text` in it: empty is automatic.
	pub(super) fn hand(text: &str) -> config::auto::Auto<String> {
		if text.trim().is_empty() {
			config::auto::Auto::automatic()
		} else {
			config::auto::Auto::by_hand(text.to_string())
		}
	}

	// A tip as text a test can compare and keep.
	pub(super) fn tip_text(tip: std::borrow::Cow<'static, str>) -> &'static str {
		match tip {
			std::borrow::Cow::Borrowed(text) => text,
			std::borrow::Cow::Owned(text) => Box::leak(text.into_boxed_str()),
		}
	}

	// A stand-in for the UI font: every character the same width.
	pub(super) fn chars7(s: &str) -> f32 {
		s.chars().count() as f32 * 7.0
	}

	// Each row's label measured in that font, `scale` times over.
	fn labels7(scale: f32) -> Vec<f32> {
		super::ui()
			.specs
			.iter()
			.map(|s| chars7(s.label) * scale)
			.collect()
	}

	pub(super) fn mk_dialog(max_h: f32) -> SettingsDialog {
		mk_dialog_at(max_h, 1.0)
	}
	// Everything a real dialog is handed arrives in physical pixels, so a scale
	// of 2 means twice the line height, label width, tab widths and height cap.
	fn mk_dialog_at(max_h: f32, scale: f32) -> SettingsDialog {
		let mut d = SettingsDialog::new(
			0.0,
			0.0,
			18.0 * scale,
			Chrome {
				label_w: 170.0 * scale,
				btn_w: 80.0 * scale,
				row_btn_w: 90.0 * scale,
				value_w: 0.0,
				tab_ws: vec![90.0 * scale; tab_titles().len()],
				label_ws: labels7(scale),
				..Chrome::default()
			},
			f32::MAX,
			max_h * scale,
			scale,
		);
		// the rows a profile governs answer with its values while one is chosen;
		// the tests below drive the rows themselves, so they start from Custom
		d.orig.performance_profile = crate::profile::Profile::Custom;
		d.edited.performance_profile = crate::profile::Profile::Custom;
		d
	}

	// A scale change is a boundary change and nothing else. The layout is solved
	// in DIP, so the same window comes out the same apparent size on a monitor at
	// another scale - and the values and unapplied edits have to still be there,
	// which is the whole reason the dialog is not rebuilt for one.
	// Test ID: EqMD0kq
	#[test]
	fn a_scale_change_moves_the_boundary_and_leaves_the_rest() {
		let mut d = mk_dialog_at(900.0, 1.0);
		d.set_size(700.0, 600.0);
		d.edited.margin = 42.0;
		d.tab = 2;
		let (was_w, was_line_h, was_label_w, was_natural) =
			(d.rect.w, d.line_h, d.label_w, d.natural);
		// what a real caller hands over: the same chrome, measured at 2x
		d.rescale(
			18.0 * 2.0,
			Chrome {
				label_w: 170.0 * 2.0,
				btn_w: 80.0 * 2.0,
				row_btn_w: 90.0 * 2.0,
				value_w: 0.0,
				tab_ws: vec![90.0 * 2.0; tab_titles().len()],
				label_ws: labels7(2.0),
				..Chrome::default()
			},
			f32::MAX,
			900.0 * 2.0,
			2.0,
		);
		// twice the pixels for the same DIP, so nothing below the boundary moved
		assert!((d.line_h - was_line_h).abs() < 0.01, "line height moved");
		assert!((d.label_w - was_label_w).abs() < 0.01, "label column moved");
		assert!(
			(d.natural.0 - was_natural.0).abs() < 0.01,
			"natural width moved"
		);
		assert!(
			(d.natural.1 - was_natural.1).abs() < 0.01,
			"natural height moved"
		);
		// and the window is the same size on screen
		assert!(
			(d.to_px(d.rect.w) - was_w * 2.0).abs() < 0.01,
			"the box is a different size on screen"
		);
		// the user's own state is untouched
		assert!((d.edited.margin - 42.0).abs() < f32::EPSILON, "edit lost");
		assert_eq!(d.tab, 2, "tab lost");
	}

	// A maximized or tiled window keeps its physical size through a scale change,
	// so the window manager sends no resize and nothing else tells the dialog it
	// is now claiming twice the pixels the window has. Handing it the size the
	// window really measures is what puts the two back in step.
	// Test ID: EqQh9oP
	#[test]
	fn a_window_that_keeps_its_pixels_through_a_scale_change_still_fits_them() {
		let mut d = mk_dialog_at(900.0, 1.0);
		d.set_size(700.0, 600.0);
		d.rescale(
			18.0 * 2.0,
			Chrome {
				label_w: 170.0 * 2.0,
				btn_w: 80.0 * 2.0,
				row_btn_w: 90.0 * 2.0,
				value_w: 0.0,
				tab_ws: vec![90.0 * 2.0; tab_titles().len()],
				label_ws: labels7(2.0),
				..Chrome::default()
			},
			f32::MAX,
			900.0 * 2.0,
			2.0,
		);
		// rescale alone keeps the DIP, which is twice the pixels the window has
		assert!(
			d.to_px(d.rect.w) > 700.0 + 1.0,
			"the box would fit the window with no resize, so there is nothing to fix"
		);
		d.set_size(700.0, 600.0);
		assert!(
			(d.to_px(d.rect.w) - 700.0).abs() < 1.0,
			"box is {} px wide in a 700 px window",
			d.to_px(d.rect.w)
		);
		assert!(
			(d.to_px(d.rect.h) - 600.0).abs() < 1.0,
			"box is {} px tall in a 600 px window",
			d.to_px(d.rect.h)
		);
	}

	// A tab's title is drawn `tab_pad / 2` inside its own box, and the box is only
	// as wide as the title plus `tab_pad` - so the pad on both sides of that sum has
	// to be the SAME pad. Mixing a physical measurement with a raw DIP constant
	// halves the box's share of it at 2x (flush right) and takes two thirds of it
	// at 3x (the title runs past the box).
	// Test ID: EncVe5o
	#[test]
	fn a_tab_title_keeps_its_clear_space_at_every_scale() {
		let pad = super::lay().tab_pad;
		for scale in [1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
			let title_dip = 61.0;
			// what chrome_widths hands over, back in the dialog's own units
			let box_dip = super::measured_plus(title_dip * scale, pad, scale) / scale;
			assert!(
				(box_dip - (title_dip + pad)).abs() <= 1.0 / scale,
				"at {scale}x the box is {box_dip}, wanted {}",
				title_dip + pad
			);
			assert!(
				box_dip >= title_dip + pad / 2.0,
				"at {scale}x the title overflows its box ({box_dip} < {})",
				title_dip + pad / 2.0
			);
		}
	}

	// Test ID: EipgKd6
	#[test]
	fn tabs_partition_all_specs() {
		let d = mk_dialog(2000.0);
		// every spec sits on a valid tab and no tab is empty
		assert!(d.specs.iter().all(|s| s.tab < tab_titles().len()));
		for t in 0..tab_titles().len() {
			assert!(d.specs.iter().any(|s| s.tab == t), "tab {t} has no rows");
		}
	}

	// Test ID: Eit1amm
	#[test]
	fn revert_restores_default_and_records_key() {
		let mut d = mk_dialog(2000.0);
		let def = d.defaults.opacity; // edited may start off-default (loaded config)
		d.edited.opacity = def + 0.5;
		assert!(!d.is_default(super::Key::Opacity));
		d.revert(super::Key::Opacity);
		assert!(d.is_default(super::Key::Opacity));
		assert_eq!(d.edited.opacity, def);
		let rev = d.take_reverted();
		assert!(rev.contains(&"transparency.opacity"));
		assert!(d.take_reverted().is_empty(), "taking clears the list");
		// was: reverting font size must not clear the system-size follow. The
		// follow switch went with 2026100907341818, and the size's default is
		// automatic, which a revert puts back and the file learns as no line.
		d.edited.font_size = config::auto::Auto::by_hand(99.0);
		d.revert(super::Key::FontSize);
		assert!(d.edited.font_size.is_automatic());
		assert!(d.take_reverted().contains(&"font.size"));
	}

	// Test ID: EipgKd7
	#[test]
	fn height_cap_enables_scroll() {
		// generous cap: natural size, nothing to scroll
		let d = mk_dialog(2000.0);
		assert!(d.size().1 < 2000.0);
		assert_eq!(d.max_scroll(), 0.0);
		assert!(d.thumb().is_none());
		// tight cap: window clamps, the (tallest) appearance tab overflows
		let mut d = mk_dialog(400.0);
		d.tab = 1; // Background
		assert!(d.size().1 <= 400.0);
		assert!(d.max_scroll() > 0.0);
		assert!(d.thumb().is_some());
		// wheel scrolls rows up and clamps at both ends
		let y_first = d.row_y(1);
		d.wheel(0.0, -120.0);
		assert!(d.scroll > 0.0 && d.scroll <= d.max_scroll());
		assert!(d.row_y(1) < y_first);
		d.wheel(0.0, 1e9);
		assert_eq!(d.scroll, 0.0);
		d.wheel(0.0, -1e9);
		assert_eq!(d.scroll, d.max_scroll());
	}

	// The Shell tab grows a line per shell, so it scrolls instead of making
	// every other tab as tall as its list.
	// Test ID: ErPQry8
	#[test]
	fn the_dialog_is_as_tall_as_its_tallest_fixed_tab() {
		let mut d = mk_dialog(4000.0);
		let fixed: Vec<usize> = SettingsDialog::fixed_tabs(d.specs).collect();
		let shell_tab = (0..tab_titles().len())
			.find(|t| !fixed.contains(t))
			.expect("the Shell tab is not a fixed one");
		d.edited.shells = (0..60)
			.map(|i| shell_entry(&format!("Shell {i}"), "sh"))
			.collect();
		let mut tallest = 0.0f32;
		for &t in &fixed {
			d.tab = t;
			assert_eq!(d.max_scroll(), 0.0, "tab {t} fits");
			tallest = tallest.max(d.content_h());
		}
		assert!((d.viewport().h - tallest).abs() < 0.01, "no room left over");
		d.tab = shell_tab;
		assert!(d.max_scroll() > 0.0, "a long shell list scrolls");
		// the hotkeys are a long list too, and would make every tab that tall
		let keys_tab = tab_titles().iter().position(|t| *t == "Keys").unwrap();
		assert!(!fixed.contains(&keys_tab), "the Keys tab is left out");
		d.tab = keys_tab;
		assert!(d.max_scroll() > 0.0, "the hotkey list scrolls");
		// and a row walked onto by keyboard comes into view
		d.focus = None;
		for _ in 0..20 {
			d.focus_move(true);
		}
		let Some(super::Focus::Row(i, _)) = d.focus else {
			panic!("focus left the rows")
		};
		let vp = d.viewport();
		assert!(d.row_y(i) >= vp.y && d.row_y(i) + d.row_screen_h(i) <= vp.y + vp.h);
	}

	// Too narrow, and the rows keep their natural width and slide sideways under
	// the window instead of being cut down to fit it. The bar that does it lives
	// in the clear space above the footer, so showing it costs the rows nothing.
	// Test ID: EpOQNMS
	#[test]
	fn a_narrow_window_scrolls_sideways_rather_than_truncating() {
		let mut d = mk_dialog(2000.0);
		let wide = d.size().0;
		assert_eq!(d.max_hscroll(), 0.0);
		assert!(d.hthumb().is_none());
		let rows_before = d.viewport().h;

		d.set_size(wide - 200.0, d.size().1);
		assert!((d.max_hscroll() - 200.0).abs() < 0.01);
		assert!(d.hthumb().is_some());
		let bar = d.htrack();
		let vp = d.viewport();
		assert_eq!(
			vp.h,
			rows_before - d.hbar_h(),
			"the bar takes its strip off the rows"
		);
		assert!(bar.y >= vp.y + vp.h, "the bar sits below the rows");
		assert!(
			bar.y + bar.h + super::lay().buttons_gap <= d.buttons()[0].1.y + 0.01,
			"and the footer keeps its whole clear space below it"
		);

		// the revert column starts off the right-hand edge and scrolling reaches it
		let row = SettingsDialog::visible(d.specs, d.tab)
			.find(|(i, _)| d.has_revert(*i))
			.map(|(i, _)| i)
			.expect("a tab with something to revert");
		assert!(d.revert_box(row).x + d.revert_box(row).w > d.rect.x + d.rect.w);
		d.wheel(-1e9, 0.0);
		assert_eq!(d.hscroll, d.max_hscroll());
		assert!(d.revert_box(row).x + d.revert_box(row).w <= d.rect.x + d.rect.w + 0.01);
		// and back
		d.wheel(1e9, 0.0);
		assert_eq!(d.hscroll, 0.0);
	}

	// Clear space between a control's right edge and the panel's.
	fn right_gap(d: &SettingsDialog, r: super::Rect) -> f32 {
		d.rect.x + d.rect.w - (r.x + r.w)
	}

	// Widening hands the extra room to the control in the middle of the row. The
	// value field and the revert arrow keep their distance from the right edge,
	// so both stay in one column whatever the window is doing.
	// Test ID: EpOQNMT
	#[test]
	fn widening_stretches_the_control_and_not_its_value_field() {
		let mut d = mk_dialog(2000.0);
		let row = SettingsDialog::visible(d.specs, 0)
			.find(|(_, s)| matches!(s.kind, Kind::Slider { .. }))
			.map(|(i, _)| i)
			.expect("a slider on the first tab");
		let narrow = (d.track(row).w, d.valbox(row), d.revert_box(row));
		let (val_gap, rev_gap) = (right_gap(&d, narrow.1), right_gap(&d, narrow.2));

		d.set_size(d.size().0 + 300.0, d.size().1);
		assert!(
			d.track(row).w > narrow.0 + 299.0,
			"the slider took the whole 300"
		);
		assert!((right_gap(&d, d.valbox(row)) - val_gap).abs() < 0.01);
		assert!((right_gap(&d, d.revert_box(row)) - rev_gap).abs() < 0.01);
		assert!(
			d.valbox(row).w == narrow.1.w,
			"the value field is fixed width"
		);
	}

	// Commented out 20261006: a line may hold more than two rows now
	// (2026100614510984), and this read every row followed by a `beside` one as
	// a line's first, which the tab text line's middle rows are not. Replaced by
	// `a_shared_line_puts_its_parts_side_by_side` (Ery4fxK), which checks the same
	// things for every part of every line.
	// // A row declared `beside` shares the line above it: same y, and the two split
	// // the control column without touching. The pair costs one line, not two.
	// // Test ID: EpOQNMU
	// #[test]
	// fn a_paired_row_shares_the_line_above_it() {
	// 	let d = mk_dialog(4000.0);
	// 	let mut pairs = 0;
	// 	for tab in 0..tab_titles().len() {
	// 		let rows: Vec<usize> = SettingsDialog::visible(d.specs, tab)
	// 			.map(|(i, _)| i)
	// 			.collect();
	// 		for w in rows.windows(2) {
	// 			let (lead, follow) = (w[0], w[1]);
	// 			if !d.specs[follow].beside {
	// 				continue;
	// 			}
	// 			pairs += 1;
	// 			let mut d = mk_dialog(4000.0);
	// 			d.tab = tab;
	// 			assert_eq!(d.row_y(lead), d.row_y(follow), "one line, not two");
	// 			assert!(
	// 				d.ctl_right(lead) <= d.control_x(follow),
	// 				"the two halves do not overlap"
	// 			);
	// 			assert!(
	// 				d.control_x(follow) < d.ctl_right(follow),
	// 				"the second half has room to draw in"
	// 			);
	// 			// one revert arrow answers for both settings
	// 			assert!(d.has_revert(lead) && !d.has_revert(follow));
	// 			assert!(d.row_keys(lead).contains(&d.specs[follow].key));
	// 		}
	// 	}
	// 	assert!(pairs >= 2, "expected paired rows, saw {pairs}");
	// }

	// Every row declared `beside` shares its line's y, and the line's parts sit
	// in order without touching, each with room to draw in. One revert arrow,
	// on the first row, answers for every setting on the line.
	// Test ID: Ery4fxK
	#[test]
	fn a_shared_line_puts_its_parts_side_by_side() {
		let mut d = mk_dialog(4000.0);
		let (w, h) = d.natural;
		d.set_size(w, h);
		let mut lines = 0;
		let mut widest = 0;
		for lead in 0..d.specs.len() {
			let (_, parts, _) = SettingsDialog::line_of(d.specs, lead);
			if d.specs[lead].beside || parts < 2 {
				continue;
			}
			lines += 1;
			widest = widest.max(parts);
			d.tab = d.specs[lead].tab;
			assert!(d.has_revert(lead), "the line's first row has the arrow");
			for j in lead + 1..lead + parts {
				assert_eq!(d.row_y(lead), d.row_y(j), "one line, not {parts}");
				assert!(
					!d.has_revert(j),
					"{:?} has an arrow of its own",
					d.specs[j].key
				);
				assert!(d.row_keys(lead).contains(&d.specs[j].key));
				// the part before ends before this one's label starts
				assert!(
					d.ctl_right(j - 1) <= d.label_x(j),
					"{:?} runs into {:?}",
					d.specs[j - 1].key,
					d.specs[j].key
				);
				assert!(
					d.control_x(j) < d.ctl_right(j),
					"{:?} has room to draw in",
					d.specs[j].key
				);
			}
		}
		assert!(lines >= 3, "expected shared lines, saw {lines}");
		assert!(widest >= 4, "the tab text line has four parts");
	}

	// Where the tab text line is, on whatever tab it is on.
	fn tab_text_line(d: &mut SettingsDialog) -> [usize; 4] {
		let at = |key: Key| d.specs.iter().position(|s| s.key == key).unwrap();
		let line = [
			at(Key::TabShowsTitle),
			at(Key::TabShowsShell),
			at(Key::TabShowsProgram),
			at(Key::TabShowsDirectory),
		];
		d.tab = d.specs[line[0]].tab;
		line
	}

	// The four tab text toggles are one line with one revert arrow, and the
	// arrow puts all four back - whichever of them moved.
	// Test ID: Ery4g1k
	#[test]
	fn one_revert_puts_back_all_four_tab_text_toggles() {
		let mut d = mk_dialog(4000.0);
		let line = tab_text_line(&mut d);
		let lead = line[0];
		assert_eq!(SettingsDialog::line_of(d.specs, lead), (lead, 4, 0));
		for i in line {
			assert_eq!(d.row_y(i), d.row_y(lead));
			assert_eq!(d.has_revert(i), i == lead);
		}
		let keys = line.map(|i| d.specs[i].key);
		assert_eq!(d.row_keys(lead), keys.to_vec());

		// each one alone lights the arrow
		for key in keys {
			for k in keys {
				let at = super::toggle_of(&d.defaults, k);
				d.set_toggle(k, at);
			}
			assert!(d.row_is_default(lead));
			let at = super::toggle_of(&d.defaults, key);
			d.set_toggle(key, !at);
			assert!(!d.row_is_default(lead), "{key:?} off its default");
		}

		// all four off their defaults, then one click on the arrow
		for k in keys {
			let at = super::toggle_of(&d.defaults, k);
			d.set_toggle(k, !at);
		}
		let arrow = d.revert_box(lead);
		d.mouse_down_dip(
			arrow.x + arrow.w / 2.0,
			arrow.y + arrow.h / 2.0,
			&mut chars7,
		);
		for k in keys {
			assert!(d.is_default(k), "{k:?} was not put back");
		}
		assert!(d.row_is_default(lead));
		// and all four lines go back to the template's at Apply
		let reverted = d.take_reverted();
		for path in [
			"window.tab_shows_title",
			"window.tab_shows_shell",
			"window.tab_shows_program",
			"window.tab_shows_directory",
		] {
			assert!(
				reverted.contains(&path),
				"{path} stays in force: {reverted:?}"
			);
		}
	}

	// A click on one of the four boxes, or Space with focus on it, flips that
	// setting and leaves the other three alone. Tab and the arrows reach each.
	// Test ID: Ery4g6B
	#[test]
	fn each_tab_text_toggle_flips_only_its_own_setting() {
		let mut d = mk_dialog(4000.0);
		let line = tab_text_line(&mut d);
		let keys = line.map(|i| d.specs[i].key);
		let values = |d: &SettingsDialog| keys.map(|k| d.get_toggle(k));
		for (n, &i) in line.iter().enumerate() {
			let before = values(&d);
			let bx = d.checkbox(i);
			d.mouse_down_dip(bx.x + bx.w / 2.0, bx.y + bx.h / 2.0, &mut chars7);
			let mut want = before;
			want[n] = !want[n];
			assert_eq!(values(&d), want, "a click on {:?}", keys[n]);
			assert_eq!(d.focus, Some(super::Focus::Row(i, 0)));
		}

		// the keyboard: Down walks onto each box in turn, Space flips only it
		d.focus = Some(super::Focus::Row(line[0], 0));
		for (n, &i) in line.iter().enumerate() {
			assert_eq!(
				d.focus,
				Some(super::Focus::Row(i, 0)),
				"Down reaches {:?}",
				keys[n]
			);
			let before = values(&d);
			d.key_space();
			let mut want = before;
			want[n] = !want[n];
			assert_eq!(values(&d), want, "Space on {:?}", keys[n]);
			d.key_vertical(true);
		}
		// and Shift+Tab walks back over the same four
		d.focus = Some(super::Focus::Row(line[3], 0));
		d.shift = true;
		for &i in line.iter().rev().skip(1) {
			d.key_tab();
			assert_eq!(d.focus, Some(super::Focus::Row(i, 0)));
		}
	}

	// Each toggle keeps its own flyover, over its box and over its label.
	// Test ID: Ery4gAt
	#[test]
	fn each_tab_text_toggle_has_its_own_tip_over_box_and_label() {
		let mut d = mk_dialog(4000.0);
		let line = tab_text_line(&mut d);
		for i in line {
			// at its default, so the tip is the help alone with no value lines,
			// whatever this box's own config holds (G6)
			d.revert(d.specs[i].key);
			let help = d.specs[i].help;
			assert!(!help.is_empty(), "{:?} has a tip", d.specs[i].key);
			let bx = d.checkbox(i);
			let mid = bx.y + bx.h / 2.0;
			for x in [bx.x + bx.w / 2.0, d.label_x(i) + 2.0] {
				let tip = d.hover_tip_dip(x, mid).map(|(tip, _)| tip_text(tip));
				assert_eq!(tip, Some(help), "{:?} at x {x}", d.specs[i].key);
			}
		}
	}

	// One setting, one tip, wherever on its row the pointer rests: its label, a
	// warning mark, each option's label and every part of its control. A pair
	// is two settings under one label, so each half answers for its own. The
	// revert arrow is the one part that may say something else.
	// Test ID: Es9VuY0
	#[test]
	fn every_part_of_a_row_shows_the_rows_tip() {
		let mut d = mk_dialog(4000.0);
		let mut probed = 0;
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			let vp = d.viewport();
			for i in 0..d.specs.len() {
				let spec = &d.specs[i];
				if spec.tab != tab || matches!(spec.kind, Kind::Header(_) | Kind::ShellList) {
					continue;
				}
				let bx = d.checkbox(i);
				let mid = bx.y + bx.h / 2.0;
				let mut spots = vec![(d.label_x(i) + 2.0, 0u16)];
				if !spec.warning.is_empty() {
					let mark = d.warning_box(i, &mut chars7);
					spots.push((mark.x + mark.w / 2.0, 0));
				}
				for part in 0..d.parts_of(i) {
					let r = d.focus_ctl_rect(i, part);
					spots.extend([
						(r.x + 2.0, part),
						(r.x + r.w / 2.0, part),
						(r.x + r.w - 2.0, part),
					]);
				}
				if let Kind::Radio(options) = spec.kind {
					for (choice, option) in options.iter().enumerate() {
						let r = d.radio_box(i, choice);
						spots.push((r.x + r.w / 2.0, 0));
						spots.push((r.x + r.w + 4.0 + chars7(option) / 2.0, 0));
					}
				}
				for (x, part) in spots {
					if !vp.contains(x, mid) {
						continue;
					}
					let want = d.row_tip(i, d.part_key(i, part)).map(tip_text);
					assert_eq!(
						d.hover_tip_dip(x, mid).map(|(tip, _)| tip_text(tip)),
						want,
						"{:?} part {part} at x {x}",
						spec.key
					);
					probed += 1;
				}
			}
		}
		assert!(probed > 400, "only {probed} spots probed");
		// the row the item named: all of "Fit  ( ) Stretch  ( ) Zoom"
		let i = d.specs.iter().position(|s| s.key == Key::BgFit).unwrap();
		d.tab = d.specs[i].tab;
		// at its default, so the tip has no value lines (G6)
		d.revert(Key::BgFit);
		let Kind::Radio(options) = d.specs[i].kind else {
			panic!("Fit is a radio row")
		};
		let mid = d.checkbox(i).y + d.checkbox(i).h / 2.0;
		let mut spots = vec![d.label_x(i) + 2.0];
		for (choice, option) in options.iter().enumerate() {
			let r = d.radio_box(i, choice);
			spots.extend([r.x + r.w / 2.0, r.x + r.w + 4.0 + chars7(option) / 2.0]);
		}
		for x in spots {
			let tip = d.hover_tip_dip(x, mid).map(|(tip, _)| tip_text(tip));
			assert_eq!(tip, Some(d.specs[i].help), "Fit at x {x}");
		}
	}

	// At its natural width the panel fits the whole line: each label ends before
	// its own box, each box inside its part, and the last before the arrow. Also
	// with labels long enough that the line, not the tab strip, sets the width.
	// Test ID: Ery4gFO
	#[test]
	fn the_tab_text_line_fits_the_panel() {
		for per_char in [7.0f32, 16.0] {
			for line_h in [18.0f32, 38.0] {
				let k = line_h / 18.0;
				let labels = super::ui()
					.specs
					.iter()
					.map(|s| s.label.chars().count() as f32 * per_char * k)
					.collect();
				let mut d = SettingsDialog::new(
					0.0,
					0.0,
					line_h,
					Chrome {
						label_w: 170.0 * k,
						btn_w: 80.0 * k,
						row_btn_w: 90.0 * k,
						value_w: 0.0,
						tab_ws: vec![90.0 * k; tab_titles().len()],
						label_ws: labels,
						..Chrome::default()
					},
					f32::MAX,
					4000.0,
					1.0,
				);
				let (w, h) = d.natural;
				d.set_size(w, h);
				let line = tab_text_line(&mut d);
				for &i in &line[1..] {
					let label_end =
						d.label_x(i) + d.specs[i].label.chars().count() as f32 * per_char * k;
					let bx = d.checkbox(i);
					assert!(
						label_end < bx.x,
						"{:?}: label runs into its box ({per_char}, {line_h})",
						d.specs[i].key
					);
					assert!(
						bx.x + bx.w <= d.ctl_right(i),
						"{:?}: box past its part ({per_char}, {line_h})",
						d.specs[i].key
					);
				}
				let last = d.checkbox(line[3]);
				assert!(last.x + last.w < d.revert_box(line[0]).x);
				assert!(d.revert_box(line[0]).x + d.revert_box(line[0]).w <= d.rect.x + d.rect.w);
			}
		}
	}

	// Commented out 20261008: a packed line in a group with other rows keeps its
	// first box in the control column (2026100812334385), so that box no longer
	// sits PART_LABEL_GAP after its label. Replaced by
	// `a_line_of_toggles_packs_each_label_against_its_box_off_the_column` (Es9Zhyr).
	// // A line of toggles packs at its natural width (2026100710173200): every
	// // label, the first one's included, sits PART_LABEL_GAP before its own box
	// // instead of across the label column, and the next label starts PACK_GAP
	// // after it. A click, the focus ring and the tip all follow the box where it
	// // is drawn. A line of anything else still splits the control column.
	// // Test ID: Es2i5CM
	// #[test]
	// fn a_line_of_toggles_packs_each_label_against_its_box() {
	// 	for scale in [1.0f32, 2.0] {
	// 		let mut d = mk_dialog_at(4000.0, scale);
	// 		let (w, h) = d.natural;
	// 		d.set_size(w * scale, h * scale);
	// 		let (mut packed, mut split) = (0, 0);
	// 		for lead in 0..d.specs.len() {
	// 			let (_, parts, _) = SettingsDialog::line_of(d.specs, lead);
	// 			if d.specs[lead].beside || parts < 2 {
	// 				continue;
	// 			}
	// 			d.tab = d.specs[lead].tab;
	// 			if !d.specs[lead..lead + parts]
	// 				.iter()
	// 				.all(|s| matches!(s.kind, Kind::Toggle))
	// 			{
	// 				split += 1;
	// 				assert!(!SettingsDialog::packs(d.specs, lead));
	// 				let column = d.rect.x + super::lay().pad + d.label_w;
	// 				assert!((d.control_x(lead) - column).abs() < 0.01, "{scale}");
	// 				continue;
	// 			}
	// 			packed += 1;
	// 			for i in lead..lead + parts {
	// 				let key = d.specs[i].key;
	// 				let bx = d.checkbox(i);
	// 				let label_end = d.label_x(i) + chars7(d.specs[i].label);
	// 				assert!(
	// 					(bx.x - label_end - super::PART_LABEL_GAP).abs() < 0.01,
	// 					"{key:?}: label to box is {} ({scale})",
	// 					bx.x - label_end
	// 				);
	// 				if i > lead {
	// 					let prev = d.checkbox(i - 1);
	// 					assert!(
	// 						(d.label_x(i) - (prev.x + prev.w) - super::PACK_GAP).abs() < 0.01,
	// 						"{key:?}: gap to the toggle before is {} ({scale})",
	// 						d.label_x(i) - (prev.x + prev.w)
	// 					);
	// 				}
	// 				assert_eq!(d.focus_ctl_rect(i, 0), bx, "{key:?}: focus ring");
	// 				let mid = bx.y + bx.h / 2.0;
	// 				for x in [bx.x + bx.w / 2.0, d.label_x(i) + 2.0] {
	// 					let tip = d.hover_tip_dip(x, mid).map(|(tip, _)| tip_text(tip));
	// 					assert_eq!(tip, Some(d.specs[i].help), "{key:?} tip at {x}");
	// 				}
	// 				let was = d.get_toggle(key);
	// 				d.mouse_down_dip(bx.x + bx.w / 2.0, mid, &mut chars7);
	// 				assert_eq!(d.get_toggle(key), !was, "{key:?}: click on its box");
	// 				assert_eq!(d.focus, Some(super::Focus::Row(i, 0)));
	// 				d.set_toggle(key, was);
	// 			}
	// 			let last = d.checkbox(lead + parts - 1);
	// 			assert!(
	// 				last.x + last.w < d.revert_box(lead).x,
	// 				"runs into the arrow"
	// 			);
	// 		}
	// 		assert!(packed >= 2, "the tab text and re-test lines, saw {packed}");
	// 		assert!(split >= 1, "the scrim dropdown pair, saw {split}");
	// 	}
	// }

	// A line of toggles packs at its natural width (2026100710173200): each label
	// sits PART_LABEL_GAP before its own box and the next label starts PACK_GAP
	// after it. A line alone under its heading starts the first label at the
	// label edge, Tab text's. One in a group with other rows keeps its first box
	// in the control column, under theirs, as the hardware check line does
	// (2026100812334385). A click, the focus ring and the tip all follow the box
	// where it is drawn. A line of anything else still splits the control column.
	// Test ID: Es9Zhyr
	#[test]
	fn a_line_of_toggles_packs_each_label_against_its_box_off_the_column() {
		for scale in [1.0f32, 2.0] {
			let mut d = mk_dialog_at(4000.0, scale);
			let (w, h) = d.natural;
			d.set_size(w * scale, h * scale);
			let (mut packed, mut split, mut kept) = (0, 0, Vec::new());
			let column = d.rect.x + super::lay().pad + d.label_w;
			for lead in 0..d.specs.len() {
				let (_, parts, _) = SettingsDialog::line_of(d.specs, lead);
				if d.specs[lead].beside || parts < 2 {
					continue;
				}
				d.tab = d.specs[lead].tab;
				if !d.specs[lead..lead + parts]
					.iter()
					.all(|s| matches!(s.kind, Kind::Toggle))
				{
					split += 1;
					assert!(!SettingsDialog::packs(d.specs, lead));
					assert!((d.control_x(lead) - column).abs() < 0.01, "{scale}");
					continue;
				}
				packed += 1;
				let keeps = SettingsDialog::lead_keeps_column(d.specs, lead);
				if keeps {
					kept.push(d.specs[lead].key);
				}
				let indent = f32::from(d.specs[lead].indent) * super::lay().indent;
				assert!((d.label_x(lead) - (d.rect.x + super::lay().pad + indent)).abs() < 0.01);
				for i in lead..lead + parts {
					let key = d.specs[i].key;
					let bx = d.checkbox(i);
					let label_end = d.label_x(i) + chars7(d.specs[i].label);
					if keeps && i == lead {
						assert!(
							(bx.x - column).abs() < 0.01,
							"{key:?}: box off the column ({scale})"
						);
					} else {
						assert!(
							(bx.x - label_end - super::PART_LABEL_GAP).abs() < 0.01,
							"{key:?}: label to box is {} ({scale})",
							bx.x - label_end
						);
					}
					if i > lead {
						let prev = d.checkbox(i - 1);
						assert!(
							(d.label_x(i) - (prev.x + prev.w) - super::PACK_GAP).abs() < 0.01,
							"{key:?}: gap to the toggle before is {} ({scale})",
							d.label_x(i) - (prev.x + prev.w)
						);
					}
					assert_eq!(d.focus_ctl_rect(i, 0), bx, "{key:?}: focus ring");
					let mid = bx.y + bx.h / 2.0;
					// at its default, so the tip has no value lines (G6)
					d.revert(key);
					for x in [bx.x + bx.w / 2.0, d.label_x(i) + 2.0, label_end - 2.0] {
						let tip = d.hover_tip_dip(x, mid).map(|(tip, _)| tip_text(tip));
						assert_eq!(tip, Some(d.specs[i].help), "{key:?} tip at {x}");
					}
					let was = d.get_toggle(key);
					d.mouse_down_dip(bx.x + bx.w / 2.0, mid, &mut chars7);
					assert_eq!(d.get_toggle(key), !was, "{key:?}: click on its box");
					assert_eq!(d.focus, Some(super::Focus::Row(i, 0)));
					d.set_toggle(key, was);
				}
				let last = d.checkbox(lead + parts - 1);
				assert!(
					last.x + last.w < d.revert_box(lead).x,
					"runs into the arrow"
				);
			}
			assert!(packed >= 2, "the tab text and re-test lines, saw {packed}");
			assert!(split >= 1, "the scrim dropdown pair, saw {split}");
			// by name, so a reshuffle of the spec cannot quietly flip either one
			assert_eq!(kept, [Key::PerfCheckHardware], "{scale}");
			let row = |key: Key| d.specs.iter().position(|s| s.key == key).expect("row");
			d.tab = d.specs[row(Key::PerfAuto)].tab;
			assert_eq!(
				d.checkbox(row(Key::PerfCheckHardware)).x,
				d.checkbox(row(Key::PerfAuto)).x,
				"under Choose automatically"
			);
			// and only a label in the column is measured for it
			let in_column = |key: Key| SettingsDialog::in_label_column(d.specs, row(key));
			assert!(in_column(Key::PerfCheckHardware) && in_column(Key::PerfAuto));
			assert!(!in_column(Key::PerfCheckNext) && !in_column(Key::TabShowsTitle));
		}
	}

	// At a 24 pt interface font the Tab text labels sat 6 DIP from boxes under
	// text twice the default's size (2026100817172887). Each gap between a label
	// and its own box, and before the next toggle, keeps its share of the line,
	// and never drops below what it is at the default font. A radio option's and
	// a pair's label after its box go the same way.
	// Test ID: Es9uxSu
	#[test]
	fn a_large_interface_font_keeps_each_label_clear_of_its_box() {
		use super::{GAPS_DRAWN_AT, PACK_GAP, PART_LABEL_GAP};
		// UI line heights, DIP: the test default, the 11 pt default, 24 pt, 32 pt
		for line in [18.0f32, GAPS_DRAWN_AT, 43.0, 58.0] {
			let grow = (line / GAPS_DRAWN_AT).max(1.0);
			let (label_gap, pack_gap) = (PART_LABEL_GAP * grow, PACK_GAP * grow);
			for scale in [1.0f32, 2.0] {
				let k = line / 18.0 * scale;
				let mut d = SettingsDialog::new(
					0.0,
					0.0,
					line * scale,
					Chrome {
						label_w: 170.0 * k,
						btn_w: 80.0 * k,
						row_btn_w: 90.0 * k,
						value_w: 0.0,
						tab_ws: vec![90.0 * k; tab_titles().len()],
						label_ws: labels7(k),
						..Chrome::default()
					},
					f32::MAX,
					4000.0 * scale,
					scale,
				);
				let (w, h) = d.natural;
				d.set_size(w * scale, h * scale);
				let at = format!("line {line}, {scale}x");
				let near = |got: f32, want: f32, what: &str| {
					assert!(
						(got - want).abs() < 0.01,
						"{what}: {got}, want {want} at {at}"
					);
				};
				let mut packed = 0;
				for i in 0..d.specs.len() {
					if !SettingsDialog::packs(d.specs, i) {
						continue;
					}
					d.tab = d.specs[i].tab;
					let (lead, _, _) = SettingsDialog::line_of(d.specs, i);
					let bx = d.checkbox(i);
					if !(i == lead && SettingsDialog::lead_keeps_column(d.specs, i)) {
						let mark = if d.specs[i].warning.is_empty() {
							0.0
						} else {
							super::warning_room(d.line_h)
						};
						let label_end = d.label_x(i) + d.label_ws[i] + mark;
						near(bx.x - label_end, label_gap, d.specs[i].label);
						packed += 1;
					}
					if i > lead {
						let prev = d.checkbox(i - 1);
						near(d.label_x(i) - (prev.x + prev.w), pack_gap, d.specs[i].label);
					}
				}
				assert!(
					packed >= 4,
					"the Tab text line and Re-test, saw {packed} at {at}"
				);
				// a label after its box, where it is drawn
				let mut after = 0;
				for i in 0..d.specs.len() {
					let boxes: Vec<(&str, super::Rect)> = match d.specs[i].kind {
						Kind::Radio(options) => options
							.iter()
							.enumerate()
							.map(|(c, o)| (*o, d.radio_box(i, c)))
							.collect(),
						Kind::Dual { labels, .. } => {
							vec![(labels[0], d.dual_box(i, 0)), (labels[1], d.dual_box(i, 1))]
						}
						_ => continue,
					};
					d.tab = d.specs[i].tab;
					let texts = d.texts_dip(d.line_h, chars7);
					for (label, bx) in boxes {
						let item = texts
							.iter()
							.find(|t| t.text == label && t.x > bx.x && t.x < bx.x + 200.0 * k)
							.unwrap_or_else(|| panic!("{label} is not drawn at {at}"));
						near(item.x - (bx.x + bx.w), label_gap, label);
						after += 1;
					}
				}
				assert!(
					after >= 4,
					"Fit's two options and the font pair, saw {after} at {at}"
				);
			}
		}
	}

	// Every square box grows at the radio's rate from its own floor. At a large
	// font a checkbox, a pair's box, a packed toggle and a radio box are one size,
	// and at the default font each keeps the size it always had.
	// Test ID: EsA3ceS
	#[test]
	fn a_checkbox_and_a_radio_box_grow_to_one_size() {
		let lay = lay();
		// UI line heights, DIP: the test default, 11 pt at 2x and 1x, 24 pt, 32 pt
		for line in [18.0f32, 19.5, 20.0, 43.0, 58.0] {
			let radio = lay.radio_box * (line / lay.base_line_height).max(1.0);
			let check = if line < 40.0 { lay.swatch } else { radio };
			for scale in [1.0f32, 2.0] {
				let k = line / 18.0 * scale;
				let mut d = SettingsDialog::new(
					0.0,
					0.0,
					line * scale,
					Chrome {
						label_w: 170.0 * k,
						btn_w: 80.0 * k,
						row_btn_w: 90.0 * k,
						value_w: 0.0,
						tab_ws: vec![90.0 * k; tab_titles().len()],
						label_ws: labels7(k),
						..Chrome::default()
					},
					f32::MAX,
					4000.0 * scale,
					scale,
				);
				let (w, h) = d.natural;
				d.set_size(w * scale, h * scale);
				let at = format!("line {line}, {scale}x");
				let mut seen = [0; 4];
				for i in 0..d.specs.len() {
					d.tab = d.specs[i].tab;
					let (boxes, want, kind) = match d.specs[i].kind {
						Kind::Toggle if SettingsDialog::packs(d.specs, i) => {
							(vec![d.checkbox(i)], check, 0)
						}
						Kind::Toggle => (vec![d.checkbox(i)], check, 1),
						Kind::Dual { .. } => (vec![d.dual_box(i, 0), d.dual_box(i, 1)], check, 2),
						Kind::Radio(opts) => (
							(0..opts.len()).map(|c| d.radio_box(i, c)).collect(),
							radio,
							3,
						),
						_ => continue,
					};
					for bx in boxes {
						assert!(
							(bx.w - want).abs() < 0.001 && (bx.h - want).abs() < 0.001,
							"{}: {}x{}, want {want} at {at}",
							d.specs[i].label,
							bx.w,
							bx.h
						);
					}
					seen[kind] += 1;
				}
				assert!(
					seen.iter().all(|&n| n > 0),
					"packed, plain, pair, radio: {seen:?} at {at}"
				);
			}
		}
	}

	// A ring around boxes and the labels after them, a radio group or half a
	// pair, takes in the labels' whole line in the real UI font, at the default
	// size and well past it.
	// Test ID: EsA3cyd
	#[test]
	fn a_focus_ring_takes_in_the_line_its_labels_are_on() {
		let attrs = crate::text::ui_attrs();
		// (font size, display scale)
		for (big, scale) in [(1.0, 1.0), (1.0, 2.0), (2.0, 1.0), (2.0, 2.0), (2.5, 1.0)] {
			let mut text = crate::text::TextCtx::new_cpu(big);
			let chrome = super::chrome_widths(&mut text, scale);
			let mut d = SettingsDialog::new(
				0.0,
				0.0,
				text.ui_line_h,
				chrome,
				f32::MAX,
				4000.0 * scale,
				scale,
			);
			let (w, h) = d.natural;
			d.set_size(w * scale, h * scale);
			let at = format!("{big}x the font, {scale}x");
			let mut rings = 0;
			for i in 0..d.specs.len() {
				let parts: Vec<(u16, Vec<(&str, super::Rect)>)> = match d.specs[i].kind {
					Kind::Dual { labels, .. } => vec![
						(0, vec![(labels[0], d.dual_box(i, 0))]),
						(1, vec![(labels[1], d.dual_box(i, 1))]),
					],
					Kind::Radio(opts) => vec![(
						0,
						opts.iter()
							.enumerate()
							.map(|(c, o)| (*o, d.radio_box(i, c)))
							.collect(),
					)],
					_ => continue,
				};
				d.tab = d.specs[i].tab;
				let texts = d.texts_dip(d.line_h, chars7);
				for (part, labels) in parts {
					assert!(!d.ring_is_the_box(i, part));
					let r = d.focus_ctl_rect(i, part);
					let ring = super::Rect {
						x: r.x - 2.0,
						y: r.y - 2.0,
						w: r.w + 4.0,
						h: r.h + 4.0,
					};
					for (label, bx) in labels {
						let item = texts
							.iter()
							.find(|t| t.text == label && t.x > bx.x && t.x < ring.x + ring.w)
							.unwrap_or_else(|| panic!("{label} is not drawn at {at}"));
						let end = item.x + text.measure_ui_text(label, &attrs) / scale;
						assert!(
							item.y >= ring.y - 0.01 && item.y + d.line_h <= ring.y + ring.h + 0.01,
							"{label}: line {}..{}, ring {}..{} at {at}",
							item.y,
							item.y + d.line_h,
							ring.y,
							ring.y + ring.h
						);
						assert!(
							end <= ring.x + ring.w,
							"{label} ends at {end}, ring at {} at {at}",
							ring.x + ring.w
						);
					}
					rings += 1;
				}
			}
			// the font pair went with 2026100907341818
			assert!(rings >= 3, "Fit and Visibility, saw {rings} at {at}");
		}
	}

	// A pair shares one revert arrow, so a profile showing the FIRST half must not
	// silence the arrow for the second - which is not governed and can still be
	// off its default with no other way back.
	// Test ID: EqRTxpI
	#[test]
	fn a_governed_half_does_not_silence_its_partner_s_revert() {
		let mut d = mk_dialog(4000.0);
		let lead = d
			.specs
			.iter()
			.position(|s| s.key == Key::ScrimFunction)
			.expect("the scrim function row");
		d.tab = d.specs[lead].tab;
		let follow = SettingsDialog::paired_with(d.specs, lead, d.tab).expect("a row beside it");
		d.set_radio(Key::PerfProfile, super::Profile::Max.index());
		assert!(
			d.profile_shows(Key::ScrimFunction),
			"the first half is governed"
		);
		assert!(
			!d.profile_shows(d.specs[follow].key),
			"the second half is not"
		);

		// take the ungoverned half off its default
		let ramp = d.specs[follow].key;
		let was = d.get_radio(ramp);
		d.set_radio(ramp, usize::from(was == 0));
		assert!(!d.is_default(ramp));

		assert!(d.has_revert(lead) && !d.has_revert(follow));
		assert!(
			!d.row_is_default(lead),
			"the line's one arrow has something to undo"
		);
		d.row_revert(lead);
		assert!(d.is_default(ramp), "and it undid it");
	}

	// The tab strip is chrome. It travels only far enough to keep the current tab
	// in view, and never with the rows' sideways scroll - a tab panned off the
	// window edge could not be clicked, which is how you leave a tab that will
	// not fit.
	// Test ID: EpOZLYG
	#[test]
	fn the_tab_strip_keeps_the_current_tab_in_the_window() {
		let mut d = mk_dialog(4000.0);
		d.set_size(d.size().0 - 300.0, d.size().1);
		assert!(d.max_hscroll() > 0.0);
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			for hscroll in [0.0, d.max_hscroll() / 2.0, d.max_hscroll()] {
				d.hscroll = hscroll;
				let r = d.tab_rect(tab);
				assert!(
					r.x >= d.rect.x - 0.01 && r.x + r.w <= d.rect.x + d.rect.w + 0.01,
					"tab {tab} at hscroll {hscroll} runs from {} to {}",
					r.x,
					r.x + r.w
				);
			}
		}
	}

	// A pair is worth having only if it is shorter than the two rows it replaces.
	// Test ID: EpOQNMV
	#[test]
	fn pairing_rows_makes_the_tab_shorter() {
		let d = mk_dialog(4000.0);
		let shells = d.edited.shells.len();
		for tab in 0..tab_titles().len() {
			let paired = SettingsDialog::visible(d.specs, tab).any(|(_, s)| s.beside);
			if !paired {
				continue;
			}
			let unpaired: f32 = SettingsDialog::visible(d.specs, tab)
				.map(|(_, s)| SettingsDialog::row_h_for(&s.kind, d.line_h, shells))
				.sum();
			let actual: f32 = SettingsDialog::visible(d.specs, tab)
				.map(|(i, _)| SettingsDialog::row_advance(d.specs, i, tab, d.line_h, shells))
				.sum();
			assert!(actual < unpaired, "tab {tab} saved nothing by pairing");
		}
	}

	// The strip is chrome: shorter than a footer button, dropped clear of the
	// panel edge, and closed off by a rule the rows start below.
	// Test ID: Em3Pif3
	#[test]
	fn the_tabs_stand_on_the_line_that_closes_their_strip() {
		let d = mk_dialog(2000.0);
		let gut = d.gutter_rect();
		let tab = d.tab_rect(0);
		assert!(d.tab_h() < d.btn_h(), "a tab is shorter than a button");
		assert!(tab.y > gut.y, "the tabs are clear of the panel edge");
		assert!(
			(tab.y + tab.h - (gut.y + gut.h)).abs() < 0.01,
			"and stand on the strip's closing line"
		);
		assert!(d.rows_y0() > gut.y + gut.h, "rows begin below that line");
	}

	// Commented out 20261007: a packed line of toggles leaves the control column
	// (2026100710173200), so its first row's box no longer starts there. Replaced
	// by `a_sub_group_indents_labels_and_nothing_else_off_a_packed_line` (Es2i58B),
	// which skips packed lines the way this skipped `beside` rows.
	// // The whole point of a sub-group: the label steps right, the control does
	// // not. A control that moved with its label would break the one column every
	// // row shares, which is what makes a settings list scannable.
	// // Test ID: Em3akaG
	// #[test]
	// fn a_sub_group_indents_labels_and_nothing_else() {
	// 	let mut d = mk_dialog(4000.0);
	// 	let mut seen_indented = false;
	// 	for tab in 0..tab_titles().len() {
	// 		d.tab = tab;
	// 		let rows: Vec<usize> = SettingsDialog::visible(d.specs, tab)
	// 			.map(|(i, _)| i)
	// 			.collect();
	// 		for &i in &rows {
	// 			// a row drawn beside another has no label column of its own
	// 			if d.specs[i].beside {
	// 				continue;
	// 			}
	// 			let indent = f32::from(d.specs[i].indent);
	// 			seen_indented |= indent > 0.0;
	// 			assert!(
	// 				(d.label_x(i) - (d.rect.x + super::lay().pad + indent * super::lay().indent))
	// 					.abs() < 0.01
	// 			);
	// 			assert!(
	// 				d.label_x(i) >= d.rect.x + super::lay().pad,
	// 				"a label never steps left of the panel pad"
	// 			);
	// 			// every control on the tab starts in the same column
	// 			assert!((d.control_x(i) - (d.rect.x + super::lay().pad + d.label_w)).abs() < 0.01);
	// 			assert!(
	// 				d.label_x(i) + super::lay().indent <= d.control_x(i),
	// 				"the label column still clears the deepest indent"
	// 			);
	// 		}
	// 		// a member is never deeper than one step below its leader
	// 		let rows: Vec<usize> = rows.into_iter().filter(|&i| !d.specs[i].beside).collect();
	// 		for pair in rows.windows(2) {
	// 			let (prev, next) = (d.specs[pair[0]].indent, d.specs[pair[1]].indent);
	// 			assert!(next <= prev + 1, "sub-group depth jumps more than one step");
	// 		}
	// 	}
	// 	assert!(seen_indented, "no sub-groups declared at all");
	// }

	// The whole point of a sub-group: the label steps right, the control does
	// not. A control that moved with its label would break the one column every
	// row shares, which is what makes a settings list scannable.
	// A packed line of toggles is the one exception (2026100710173200), and has
	// its own test.
	// Test ID: Es2i58B
	#[test]
	fn a_sub_group_indents_labels_and_nothing_else_off_a_packed_line() {
		let mut d = mk_dialog(4000.0);
		let mut seen_indented = false;
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			let rows: Vec<usize> = SettingsDialog::visible(d.specs, tab)
				.map(|(i, _)| i)
				.collect();
			for &i in &rows {
				// a row drawn beside another has no label column of its own, and
				// a packed line uses neither column
				if d.specs[i].beside || SettingsDialog::packs(d.specs, i) {
					continue;
				}
				let indent = f32::from(d.specs[i].indent);
				seen_indented |= indent > 0.0;
				assert!(
					(d.label_x(i) - (d.rect.x + super::lay().pad + indent * super::lay().indent))
						.abs() < 0.01
				);
				assert!(
					d.label_x(i) >= d.rect.x + super::lay().pad,
					"a label never steps left of the panel pad"
				);
				// every control on the tab starts in the same column
				assert!((d.control_x(i) - (d.rect.x + super::lay().pad + d.label_w)).abs() < 0.01);
				assert!(
					d.label_x(i) + super::lay().indent <= d.control_x(i),
					"the label column still clears the deepest indent"
				);
			}
			// a member is never deeper than one step below its leader
			let rows: Vec<usize> = rows.into_iter().filter(|&i| !d.specs[i].beside).collect();
			for pair in rows.windows(2) {
				let (prev, next) = (d.specs[pair[0]].indent, d.specs[pair[1]].indent);
				assert!(next <= prev + 1, "sub-group depth jumps more than one step");
			}
		}
		assert!(seen_indented, "no sub-groups declared at all");
	}

	// A sub-group's leader is set off from whatever sat above it, the way a
	// heading is - but its own members are not, or the run would not read as one.
	// Test ID: Em3akaH
	#[test]
	fn a_sub_group_leader_gets_the_gap_and_its_members_do_not() {
		let d = mk_dialog(4000.0);
		let mut leaders = 0;
		for tab in 0..tab_titles().len() {
			let rows: Vec<usize> = SettingsDialog::visible(d.specs, tab)
				.map(|(i, _)| i)
				.collect();
			for (n, &i) in rows.iter().enumerate() {
				let prev = n.checked_sub(1).map(|k| &d.specs[rows[k]]);
				let gap = SettingsDialog::gap_above(d.specs, i, tab, prev);
				let leads = SettingsDialog::leads_subgroup(d.specs, i, tab);
				let after_header = prev.is_some_and(|p| matches!(p.kind, super::Kind::Header(_)));
				if matches!(d.specs[i].kind, super::Kind::Header(_)) {
					continue; // headings carry their own gap, tested elsewhere
				}
				if leads && prev.is_some() && !after_header {
					leaders += 1;
					assert_eq!(gap, super::lay().subgroup_gap, "{}", d.specs[i].label);
				} else {
					assert_eq!(gap, 0.0, "{}", d.specs[i].label);
				}
			}
		}
		assert!(leaders >= 3, "expected several sub-groups, saw {leaders}");
	}

	// Change whatever a row edits, whichever kind it is, to something it is not.
	fn nudge(d: &mut SettingsDialog, i: usize, key: Key) {
		match d.specs[i].kind {
			super::Kind::Slider { min, max, int, .. } => {
				let far = if (d.get_f32(key) - min).abs() < (max - d.get_f32(key)).abs() {
					max
				} else {
					min
				};
				d.set_f32(key, if int { far.round() } else { far });
			}
			super::Kind::Color => {
				let c = d.get_col(key);
				d.set_col(key, [c[0] ^ 0x7f, c[1] ^ 0x7f, c[2] ^ 0x7f]);
			}
			super::Kind::Text => d.set_text(key, "silkterm-roundtrip"),
			super::Kind::Toggle | super::Kind::Dual { .. } => {
				let was = d.get_toggle(key);
				d.set_toggle(key, !was);
			}
			super::Kind::Radio(_) | super::Kind::Dropdown(_) => {
				let n = d.dd_options(i).len().max(match d.specs[i].kind {
					super::Kind::Radio(opts) => opts.len(),
					_ => 0,
				});
				if n > 1 {
					let next = (d.get_radio(key) + 1) % n;
					d.set_radio(key, next);
				}
			}
			// a chord no default uses, so nothing else moves
			super::Kind::Hotkey(hotkey) => {
				let chord = crate::keys::Chord::parse("Ctrl+Alt+Shift+F9").unwrap();
				d.set_hotkey(hotkey, vec![chord]);
			}
			// no single value to nudge: a push-button row, a heading, or the
			// shells grid (a list, exercised by its own tests)
			super::Kind::Buttons(_) | super::Kind::Header(_) | super::Kind::ShellList => {}
		}
	}
	// What the row shows, whichever kind it is.
	fn row_value(d: &SettingsDialog, i: usize, key: Key) -> String {
		match d.specs[i].kind {
			super::Kind::Slider { .. } => format!("{}", d.get_f32(key)),
			super::Kind::Color => format!("{:?}", d.get_col(key)),
			super::Kind::Text => d.get_text(key),
			super::Kind::Toggle | super::Kind::Dual { .. } => format!("{}", d.get_toggle(key)),
			super::Kind::Radio(_) | super::Kind::Dropdown(_) => format!("{}", d.get_radio(key)),
			// what it answers to, and what the file sets for it
			super::Kind::Hotkey(hotkey) => format!(
				"{:?} {:?}",
				d.edited.keys.chords(hotkey),
				d.edited.keys.own(hotkey)
			),
			super::Kind::Buttons(_) | super::Kind::Header(_) | super::Kind::ShellList => {
				String::new()
			}
		}
	}

	// A row whose setting the writer never writes is a dead end: the change
	// applies for the session and is gone at relaunch, with nothing anywhere to
	// say so. Both scrollbar colors did exactly that from the day the bar
	// shipped, so the check is generic - every row, saved and read back.
	// Test ID: Em3akaI
	#[test]
	fn every_row_survives_a_save_and_a_relaunch() {
		let _guard = config::test_config_lock();
		let _ = config::settings(); // memoize before the override goes in
		let dir = crate::testdir::run_dir().join(format!("silkterm_rows_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let _ = std::fs::write(&path, "");
		config::set_config_override(path.clone());
		config::reload_from_disk(); // lets backfill lay the template down once
		// the governed rows show a profile's values until it is Custom, so the
		// file the rows are read back from says so
		let pristine = std::fs::read_to_string(&path)
			.unwrap()
			.replace("# profile: \"max\"  ## Default", "profile: \"custom\"");
		assert!(pristine.contains("profile: \"custom\""));

		let mut d = mk_dialog(4000.0);
		let mut checked = 0;
		for i in 0..d.specs.len() {
			let keys: Vec<Key> = match d.specs[i].kind {
				// a heading and a row of push-buttons store nothing; the shells
				// grid stores a list rather than a setting
				super::Kind::Header(_) | super::Kind::Buttons(_) | super::Kind::ShellList => {
					vec![]
				}
				super::Kind::Dual { keys, .. } => keys.to_vec(),
				_ => vec![d.specs[i].key],
			};
			for key in keys {
				let _ = std::fs::write(&path, &pristine);
				let base = config::reload_from_disk();
				d.orig = base.clone();
				d.edited = base.clone();
				let before = row_value(&d, i, key);
				nudge(&mut d, i, key);
				let want = row_value(&d, i, key);
				assert_ne!(before, want, "{} did not budge", d.specs[i].label);
				assert!(
					config::persist(&base, &d.edited),
					"{} was not written at all",
					d.specs[i].label
				);
				let mut back = mk_dialog(4000.0);
				back.edited = config::reload_from_disk();
				assert_eq!(
					row_value(&back, i, key),
					want,
					"{} is lost on relaunch",
					d.specs[i].label
				);
				checked += 1;
			}
		}
		assert!(checked > 40, "only {checked} rows checked");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A revert queues the row's line to go back to the template's default at
	// Apply. A change made to the same row after that has to win, where it used to
	// be written and then commented straight back out, and so gone at relaunch.
	// Test ID: Epz4rDk
	#[test]
	fn a_row_changed_after_its_revert_keeps_the_change() {
		let _guard = config::test_config_lock();
		let _ = config::settings(); // memoize before the override goes in
		let dir = crate::testdir::run_dir().join(format!("silkterm_revert_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let _ = std::fs::write(&path, "");
		config::set_config_override(path.clone());
		config::reload_from_disk();
		let pristine = std::fs::read_to_string(&path)
			.unwrap()
			.replace("# profile: \"max\"  ## Default", "profile: \"custom\"");
		assert!(pristine.contains("profile: \"custom\""));

		let mut d = mk_dialog(4000.0);
		let mut checked = 0;
		for i in 0..d.specs.len() {
			let keys: Vec<Key> = match d.specs[i].kind {
				super::Kind::Header(_) | super::Kind::Buttons(_) | super::Kind::ShellList => {
					vec![]
				}
				super::Kind::Dual { keys, .. } => keys.to_vec(),
				_ => vec![d.specs[i].key],
			};
			for key in keys {
				let _ = std::fs::write(&path, &pristine);
				let base = config::reload_from_disk();
				d.orig = base.clone();
				d.edited = base.clone();
				d.reverted.clear();
				d.revert(key);
				let reverted = row_value(&d, i, key);
				nudge(&mut d, i, key);
				let want = row_value(&d, i, key);
				assert_ne!(reverted, want, "{} did not budge", d.specs[i].label);
				// the order apply_dialog_settings runs them in
				assert!(config::persist(&base, &d.edited), "{}", d.specs[i].label);
				config::revert_keys(&d.take_reverted());
				let mut back = mk_dialog(4000.0);
				back.edited = config::reload_from_disk();
				assert_eq!(
					row_value(&back, i, key),
					want,
					"{} went back to its default on relaunch",
					d.specs[i].label
				);
				checked += 1;
			}
		}
		assert!(checked > 40, "only {checked} rows checked");

		// A row reverted and left at its default still goes back to the default.
		let _ = std::fs::write(&path, &pristine);
		let base = config::reload_from_disk();
		let mut moved = base.clone();
		moved.margin = d.defaults.margin + 3.0;
		assert!(config::persist(&base, &moved));
		let base = config::reload_from_disk();
		assert_eq!(base.margin, d.defaults.margin + 3.0);
		d.orig = base.clone();
		d.edited = base.clone();
		d.reverted.clear();
		d.revert(Key::Margin);
		assert!(config::persist(&base, &d.edited));
		config::revert_keys(&d.take_reverted());
		assert_eq!(config::reload_from_disk().margin, d.defaults.margin);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Every revert arrow, clicked on a row moved off its default, puts every
	// setting it answers for back. "Program's own title" was missing from one arm
	// of `revert` and fell to the slider arm, which did nothing for a toggle.
	// Test ID: Erg35nf
	#[test]
	fn every_revert_arrow_puts_its_row_back_to_the_default() {
		let mut d = mk_dialog(4000.0);
		d.edited = d.defaults.clone();
		d.edited.performance_profile = crate::profile::Profile::Custom;
		d.adopt_theme();
		let base = d.edited.clone();
		let spec_of = |d: &SettingsDialog, key: Key| {
			d.specs
				.iter()
				.position(|s| match s.kind {
					super::Kind::Dual { keys, .. } => keys.contains(&key),
					_ => s.key == key,
				})
				.unwrap()
		};
		let mut checked = 0;
		for i in 0..d.specs.len() {
			// the file-type rows' arrow undoes a registration, tested on its own
			if !d.has_revert(i) || matches!(d.specs[i].kind, super::Kind::Buttons(_)) {
				continue;
			}
			d.tab = d.specs[i].tab;
			for key in d.row_keys(i) {
				let at = spec_of(&d, key);
				d.edited = base.clone();
				d.moved.clear();
				d.reverted.clear();
				let want = row_value(&d, at, key);
				nudge(&mut d, at, key);
				// one step from Custom can be the default profile
				if key == Key::PerfProfile && d.is_default(key) {
					nudge(&mut d, at, key);
				}
				assert_ne!(row_value(&d, at, key), want, "{key:?} did not budge");
				assert!(!d.is_default(key), "{key:?} moved but reads as default");
				d.row_revert(i);
				assert!(d.is_default(key), "{key:?} is not default after its revert");
				// the base is Custom on purpose, so the rows show their own values
				if key != Key::PerfProfile {
					assert_eq!(row_value(&d, at, key), want, "{key:?} did not go back");
				}
				checked += 1;
			}
		}
		assert!(checked > 80, "only {checked} settings checked");
	}

	// The key lists the accessors match on have to agree with the rows. A switch
	// filed under the sliders would compile and read as off forever.
	// Test ID: Erg4Cz0
	#[test]
	fn every_row_kind_matches_its_key_list() {
		let listed = |key: Key| match key {
			keys_of!(slider) => "slider",
			keys_of!(toggle) => "toggle",
			keys_of!(radio) => "radio",
			keys_of!(color) => "color",
			keys_of!(text) => "text",
			keys_of!(hotkey) => "hotkey",
			keys_of!(valueless | assoc) => "other",
		};
		let d = mk_dialog(4000.0);
		let mut checked = 0;
		for spec in d.specs {
			let (keys, want) = match spec.kind {
				super::Kind::Slider { .. } => (vec![spec.key], "slider"),
				super::Kind::Toggle => (vec![spec.key], "toggle"),
				super::Kind::Dual { keys, .. } => (keys.to_vec(), "toggle"),
				super::Kind::Radio(_) | super::Kind::Dropdown(_) => (vec![spec.key], "radio"),
				super::Kind::Color => (vec![spec.key], "color"),
				super::Kind::Text => (vec![spec.key], "text"),
				super::Kind::Hotkey(_) => (vec![spec.key], "hotkey"),
				super::Kind::Buttons(_) | super::Kind::ShellList | super::Kind::Header(_) => {
					(vec![spec.key], "other")
				}
			};
			for key in keys {
				assert_eq!(listed(key), want, "{:?} on row {:?}", key, spec.label);
				checked += 1;
			}
		}
		assert!(checked > 100, "only {checked} keys checked");
	}

	// The rule for a tip is not a quota, it is whether the tip says anything the
	// label does not. A dialog of rendering settings carries one on most of its
	// rows because a name cannot say what a falloff curve does to the picture; a
	// tip that only reworded its label would be the thing to delete.
	// Test ID: EpHedwm
	#[test]
	fn no_flyover_merely_restates_its_label() {
		let d = mk_dialog(4000.0);
		let mut with_help = 0;
		for spec in d.specs {
			if spec.help.is_empty() {
				continue;
			}
			with_help += 1;
			let label = spec.label.trim().trim_end_matches(['%', 's']).trim();
			let help = spec.help.trim();
			assert!(
				help.ends_with('.'),
				"{:?}: a tip is prose and ends in a period",
				spec.label
			);
			// a row with no label of its own (the shells grid, a button strip) has
			// nothing to restate
			if label.is_empty() {
				continue;
			}
			let bare = |t: &str| {
				t.trim()
					.trim_end_matches('.')
					.to_ascii_lowercase()
					.replace(['%', '"'], "")
					.split_whitespace()
					.collect::<Vec<_>>()
					.join(" ")
			};
			assert!(
				bare(help) != bare(label),
				"{:?}: the tip is the label again",
				spec.label
			);
		}
		assert!(with_help > 10, "only {with_help} rows carry a tip");
	}

	// With the look switch on, an image's own tags beat Visibility and Blur, and
	// the bundled pack is all tagged, so a slider that seems to do nothing has to
	// say why. Each slider names the switch by its label as it reads now.
	// Test ID: Es4N609
	#[test]
	fn the_wallpaper_look_sliders_say_a_tag_wins() {
		let d = mk_dialog(4000.0);
		let spec = |key: Key| d.specs.iter().find(|s| s.key == key).unwrap();
		let switch = spec(Key::BgHonorXmpLook);
		assert!(
			switch.help.contains("Opacity and Blur tags win"),
			"{}",
			switch.help
		);
		assert!(
			switch.help.contains("only apply to images without"),
			"{}",
			switch.help
		);
		for (key, tag) in [(Key::BgOpacity, "Opacity tag"), (Key::BgBlur, "Blur tag")] {
			let help = spec(key).help;
			assert!(help.contains(switch.label.trim()), "{key:?}: {help}");
			assert!(help.contains(tag), "{key:?}: {help}");
			assert!(
				help.contains("only applies to images without"),
				"{key:?}: {help}"
			);
		}
	}

	// A control the dialog draws as inert must not act on a click. The check used
	// to sit inside each arm of the press handler, and the color, text and radio
	// arms never got it - so a grayed field still changed its setting, and the
	// font Family field switched off "use the system font" as a side effect.
	// Rows that only count while a switch above them is on stay live with every
	// switch off (2026100907341818). What still grays is a switch over the rows
	// it replaces, not yet an automatic setting, and the machine.
	// Test ID: EsDxpmJ
	#[test]
	fn rows_that_only_count_while_a_switch_is_on_stay_live() {
		let mut d = mk_dialog(4000.0);
		d.edited.colors_from_wallpaper = false;
		d.edited.text_outline = 0.0;
		let toggles: Vec<Key> = d
			.specs
			.iter()
			.flat_map(|spec| match spec.kind {
				Kind::Dual { keys, .. } => keys.to_vec(),
				Kind::Toggle if spec.group.is_none() => vec![spec.key],
				_ => Vec::new(),
			})
			.collect();
		for key in toggles {
			d.set_toggle(key, false);
		}
		for i in 0..d.specs.len() {
			if matches!(d.specs[i].kind, Kind::Header(_)) {
				continue;
			}
			for part in 0..d.parts_of(i) {
				assert!(
					!d.part_disabled(i, part) || matches!(d.specs[i].kind, Kind::Buttons(_)),
					"{:?} part {part} grays with its switch off",
					d.specs[i].key
				);
			}
		}
		for key in Key::ALL {
			assert!(
				super::ui().needs_of(*key).iter().all(|need| need.invert),
				"{key:?} grays while a switch is off"
			);
		}
	}

	// An auto setting's field shows its rule's value, set apart and with a
	// mark, until a value is typed. Then the mark is an icon that puts it back,
	// and so is emptying the box.
	// Test ID: EsDxpmK
	#[test]
	fn an_auto_field_shows_its_rule_until_set_and_its_icon_puts_it_back() {
		let mut m = |s: &str| s.chars().count() as f32 * 7.0;
		let mut d = mk_dialog(4000.0);
		let fam = d
			.specs
			.iter()
			.position(|s| s.key == Key::FontFamily)
			.unwrap();
		d.tab = d.specs[fam].tab;
		d.edited.font_family = config::auto::Auto::automatic();
		let rule = config::auto::font_family(&d.edited);
		assert!(d.shows_automatic(fam));
		let texts = d.texts_dip(d.line_h, chars7);
		let shown = texts
			.iter()
			.find(|t| t.text == rule)
			.expect("the rule's value");
		assert!(shown.italic, "an automatic value reads as one");
		assert!(texts.iter().any(|t| t.text == super::AUTO_MARK));
		let (_, slot) = d.auto_slot(fam).unwrap();
		let at_slot = |d: &SettingsDialog, mode: QuadMode| {
			d.rects_dip(d.line_h, chars7)
				.1
				.iter()
				.any(|q| q.mode() == mode && q.pos == [slot.x, slot.y])
		};
		assert!(at_slot(&d, QuadMode::Rounded));
		// a click on the mark opens the field, which keeps its value until typed in
		d.mouse_down_dip(slot.x + 1.0, slot.y + 1.0, &mut m);
		assert!(
			d.edit.is_some()
				&& config::auto::automatic(&d.edited, config::auto::Setting::FontFamily)
		);
		d.select_all();
		for c in "Iosevka".chars() {
			d.char_input(c);
		}
		assert_eq!(config::auto::font_family(&d.edited), "Iosevka");
		d.commit_edit();
		let texts = d.texts_dip(d.line_h, chars7);
		assert!(!texts.iter().find(|t| t.text == "Iosevka").unwrap().italic);
		assert!(at_slot(&d, QuadMode::CloseMark));
		let _ = d.take_reverted();
		d.mouse_down_dip(slot.x + slot.w / 2.0, slot.y + slot.h / 2.0, &mut m);
		assert!(d.edited.font_family.is_automatic(), "the icon puts it back");
		assert!(d.take_reverted().contains(&"font.family"));
		assert!(d.row_is_default(fam));

		// the number box: emptied, it goes back too
		let size = d.specs.iter().position(|s| s.key == Key::FontSize).unwrap();
		d.edited.font_size = config::auto::Auto::by_hand(20.0);
		d.focus = Some(super::Focus::Row(size, 1));
		d.open_edit(size, true);
		d.backspace();
		assert!(d.edited.font_size.is_automatic());
		assert!(d.take_reverted().contains(&"font.size"));
		d.commit_edit();
		// and a drag sets it by hand again
		d.set_f32(Key::FontSize, 12.0);
		assert_eq!(config::auto::font_size(&d.edited), 12.0);
		assert!(!d.row_is_default(size));
	}

	// "Remember last size" holds nothing: it reads Columns and Rows, shows a
	// dash when they differ, and a click goes to automatic from mixed.
	// Test ID: EsDxpmL
	#[test]
	fn the_size_switch_is_read_off_columns_and_rows() {
		let mut m = |s: &str| s.chars().count() as f32 * 7.0;
		let mut d = mk_dialog(4000.0);
		let sw = d
			.specs
			.iter()
			.position(|s| s.key == Key::RememberSize)
			.unwrap();
		d.tab = d.specs[sw].tab;
		assert!(
			!d.has_revert(sw),
			"a group's switch has no revert of its own"
		);
		d.edited.columns = config::auto::Auto::automatic();
		d.edited.rows = config::auto::Auto::automatic();
		d.edited.remembered_columns = 150;
		d.edited.remembered_rows = 45;
		d.edited.remember_per_monitor = false;
		assert!(d.get_toggle(Key::RememberSize));
		assert_eq!(d.get_f32(Key::Columns), 150.0);
		d.set_f32(Key::Columns, 99.0);
		assert_eq!(d.group_state(sw), Some(config::auto::State::Mixed));
		assert!(!d.get_toggle(Key::RememberSize));
		let bx = d.checkbox(sw);
		let dash = d.rects_dip(d.line_h, chars7).1.iter().any(|q| {
			q.pos[0] > bx.x && q.pos[1] > bx.y && q.size[1] < bx.h / 3.0 && q.size[0] > bx.w / 3.0
		});
		assert!(dash, "a mixed switch draws a dash");
		let _ = d.take_reverted();
		d.mouse_down_dip(bx.x + bx.w / 2.0, bx.y + bx.h / 2.0, &mut m);
		assert!(d.edited.columns.is_automatic() && d.edited.rows.is_automatic());
		let reverted = d.take_reverted();
		assert!(reverted.contains(&"window.columns") && reverted.contains(&"window.rows"));
		d.mouse_down_dip(bx.x + bx.w / 2.0, bx.y + bx.h / 2.0, &mut m);
		assert!(!d.get_toggle(Key::RememberSize));
		assert_eq!(
			config::auto::grid(&d.edited, None),
			(150, 45),
			"off keeps what showed"
		);
		assert!(!d.disabled(Key::RememberPerMonitor) && !d.disabled(Key::Columns));
	}

	// An auto setting's tip says whether it is automatic, after its own text
	// and a blank line, and the icon that puts it back says that.
	// Test ID: EsDxpmM
	#[test]
	fn an_auto_rows_tip_says_whether_it_is_automatic() {
		let mut d = mk_dialog(4000.0);
		let fam = d
			.specs
			.iter()
			.position(|s| s.key == Key::FontFamily)
			.unwrap();
		d.tab = d.specs[fam].tab;
		let tb = d.textbox(fam);
		let tip = |d: &SettingsDialog, x: f32, y: f32| {
			d.hover_tip_dip(x, y).map(|(tip, _)| tip_text(tip))
		};
		d.edited.font_family = config::auto::Auto::automatic();
		let help = d.specs[fam].help;
		assert!(!help.is_empty());
		assert_eq!(
			tip(&d, tb.x + 2.0, tb.y + 2.0),
			Some(format!("{help}\n\n{}", super::AUTO_TIP).as_str())
		);
		let rule = config::auto::font_family(&d.edited);
		d.edited.font_family = config::auto::Auto::by_hand("Iosevka".into());
		assert_eq!(
			tip(&d, tb.x + 2.0, tb.y + 2.0),
			Some(format!("{help}\n\nSet by hand. Automatic would be: {rule}.").as_str())
		);
		let (_, slot) = d.auto_slot(fam).unwrap();
		assert_eq!(tip(&d, slot.x + 1.0, slot.y + 1.0), Some(super::CLEAR_TIP));
		// a row with no text of its own says only that
		let size = d.specs.iter().position(|s| s.key == Key::FontSize).unwrap();
		d.edited.font_size = config::auto::Auto::automatic();
		let vb = d.valbox(size);
		assert_eq!(tip(&d, vb.x + 2.0, vb.y + 2.0), Some(super::AUTO_TIP));
		// a whole-number box's rule is written whole, as the box shows it
		d.edited.font_size = config::auto::Auto::by_hand(30.0);
		let want = format!(
			"Set by hand. Automatic would be: {}.",
			config::default_font_size().round()
		);
		assert_eq!(tip(&d, vb.x + 2.0, vb.y + 2.0), Some(want.as_str()));
	}

	// A theme color is automatic until chosen: its hex box shows the theme's
	// value set apart, with the mark. A typed value or a pick sets it by hand,
	// the x and an emptied box put it back, Cancel in the picker leaves it as it
	// was, and picking a theme leaves it automatic under the new one.
	// Test ID: EsEAixa
	#[test]
	fn a_theme_color_is_automatic_until_chosen_and_its_x_puts_it_back() {
		use config::auto::Auto;
		let mut m = |s: &str| s.chars().count() as f32 * 7.0;
		let mut d = mk_dialog(4000.0);
		let i = d.specs.iter().position(|s| s.key == Key::ColBg).unwrap();
		d.tab = d.specs[i].tab;
		d.edited.bg = Auto::automatic();
		assert!(d.shows_automatic(i));
		let hex = config::format_hex(d.edited.theme_palette.bg);
		let texts = d.texts_dip(d.line_h, chars7);
		assert!(
			texts
				.iter()
				.find(|t| t.text == hex)
				.expect("the theme's")
				.italic,
			"an automatic color reads as one"
		);
		assert!(texts.iter().any(|t| t.text == super::AUTO_MARK));
		let (_, slot) = d.auto_slot(i).unwrap();
		let hex_box = d.hexbox(i);
		assert!(
			slot.x > hex_box.x && slot.x + slot.w <= hex_box.x + hex_box.w,
			"the mark sits in the hex box, beside the chip"
		);

		d.focus = Some(super::Focus::Row(i, 1));
		d.open_edit(i, true);
		for c in "#102030".chars() {
			d.char_input(c);
		}
		assert_eq!(d.edited.bg, Auto::by_hand([0x10, 0x20, 0x30]));
		d.commit_edit();
		let texts = d.texts_dip(d.line_h, chars7);
		assert!(!texts.iter().find(|t| t.text == "#102030").unwrap().italic);
		let close = d
			.rects_dip(d.line_h, chars7)
			.1
			.iter()
			.any(|q| q.mode() == QuadMode::CloseMark && q.pos == [slot.x, slot.y]);
		assert!(close, "set by hand, the mark is the x");
		let _ = d.take_reverted();
		d.mouse_down_dip(slot.x + slot.w / 2.0, slot.y + slot.h / 2.0, &mut m);
		assert!(d.edited.bg.is_automatic(), "the x puts it back");
		assert_eq!(d.focus, Some(super::Focus::Row(i, 1)), "on the hex box");
		assert!(d.take_reverted().contains(&"colors.background"));

		// an emptied box
		d.set_col(Key::ColBg, [9, 9, 9]);
		d.open_edit(i, true);
		d.backspace();
		assert!(d.edited.bg.is_automatic(), "an emptied box puts it back");
		d.commit_edit();

		// the picker writes through, and Cancel leaves it automatic again
		d.pick_open(i);
		d.pick_set(crate::pick::Hsv {
			h: 120.0,
			s: 1.0,
			v: 1.0,
		});
		assert!(!d.edited.bg.is_automatic());
		d.pick_cancel();
		assert!(d.edited.bg.is_automatic(), "Cancel puts back automatic");

		// a theme pick: automatic, under the theme picked
		d.set_col(Key::ColBg, [9, 9, 9]);
		let names = crate::theme::all_names(&d.edited.user_themes);
		let matrix = names.iter().position(|n| n == "Matrix").unwrap();
		d.set_radio(Key::Theme, matrix);
		assert!(d.edited.bg.is_automatic());
		assert_eq!(d.edited.theme_palette, config::theme_palette(&d.edited));
		assert_eq!(d.get_col(Key::ColBg), d.edited.theme_palette.bg);
		assert_eq!(d.edited.theme, "Matrix");
	}

	// The open command shows the desktop's opener until one is typed. File or
	// folder shows the one setting its box edits: its x puts back only that one,
	// and the arrow puts back both.
	// Test ID: EsEAj1i
	#[test]
	fn the_open_command_and_file_or_folder_are_automatic_until_typed() {
		use config::auto::{Auto, Setting};
		let mut m = |s: &str| s.chars().count() as f32 * 7.0;
		let mut d = mk_dialog(4000.0);
		let row = |d: &SettingsDialog, key| d.specs.iter().position(|s| s.key == key).unwrap();
		let i = row(&d, Key::LinkOpenCommand);
		d.edited.hyperlink_open_command = Auto::automatic();
		assert_eq!(
			d.get_text(Key::LinkOpenCommand),
			crate::links::desktop_opener()
		);
		assert!(d.shows_automatic(i));
		d.set_text(Key::LinkOpenCommand, "firefox --new-tab");
		assert!(!d.shows_automatic(i) && !d.is_default(Key::LinkOpenCommand));
		d.set_text(Key::LinkOpenCommand, " ");
		assert!(d.shows_automatic(i) && d.is_default(Key::LinkOpenCommand));

		let w = row(&d, Key::BgImage);
		d.tab = d.specs[w].tab;
		d.edited.wallpaper_rotate_enabled = true;
		d.edited.wallpaper_raw = Auto::automatic();
		d.edited.wallpaper_folder_raw = hand("/pics");
		assert!(d.wallpaper_box_is_folder() && !d.shows_automatic(w));
		let (_, slot) = d.auto_slot(w).unwrap();
		let mid = (slot.x + slot.w / 2.0, slot.y + slot.h / 2.0);
		d.mouse_down_dip(mid.0, mid.1, &mut m);
		assert!(d.edited.wallpaper_folder_raw.is_automatic());
		assert!(d.shows_automatic(w));
		assert_eq!(d.get_text(Key::BgImage), config::WALLPAPER_DIR_TOKEN);
		assert!(d.is_default(Key::BgImage));

		// an image set by hand shows either way, and its x leaves a folder set
		// by hand alone
		d.edited.wallpaper_folder_raw = hand("/pics");
		d.edited.wallpaper_raw = hand("/a.png");
		d.rewallpaper();
		assert!(!d.wallpaper_box_is_folder());
		assert_eq!(d.get_text(Key::BgImage), "/a.png");
		d.mouse_down_dip(mid.0, mid.1, &mut m);
		assert!(d.edited.wallpaper_raw.is_automatic());
		assert_eq!(d.edited.wallpaper_folder_raw, hand("/pics"));
		assert_eq!(d.edited.wallpaper, config::resolve_wallpaper(None));
		assert_eq!(
			d.get_text(Key::BgImage),
			"/pics",
			"the box is the folder again"
		);
		assert!(!d.is_default(Key::BgImage));
		d.revert(Key::BgImage);
		assert!(
			config::auto::automatic(&d.edited, Setting::WallpaperImage)
				&& config::auto::automatic(&d.edited, Setting::WallpaperFolder)
		);
		assert!(d.is_default(Key::BgImage));
	}

	// A tip ends with the value in use and the shipped one where the two
	// differ, after a blank line, written the way the row writes them. At the
	// default it has neither, and an auto setting keeps its state line instead.
	// Test ID: EsEAj63
	#[test]
	fn a_tip_ends_with_the_current_and_default_values_where_they_differ() {
		let mut d = mk_dialog(4000.0);
		d.edited.colors_from_wallpaper = false;
		let row = |d: &SettingsDialog, key| d.specs.iter().position(|s| s.key == key).unwrap();
		let tip = |d: &SettingsDialog, i: usize, key| d.row_tip(i, key).map(tip_text);
		let lines =
			|now: &str, default: &str| format!("Current value: {now}\nDefault value: {default}");
		let check = |d: &mut SettingsDialog,
		             key: Key,
		             change: &dyn Fn(&mut SettingsDialog),
		             want: String| {
			let i = row(d, key);
			d.revert(key);
			let help = d.specs[i].help;
			assert_eq!(
				tip(d, i, key),
				(!help.is_empty()).then_some(help),
				"{key:?} at its default"
			);
			change(d);
			let want = if help.is_empty() {
				want
			} else {
				format!("{help}\n\n{want}")
			};
			assert_eq!(tip(d, i, key), Some(want.as_str()), "{key:?}");
		};
		let margin = d.default_f32(Key::Margin);
		let Kind::Slider { int, .. } = d.specs[row(&d, Key::Margin)].kind else {
			panic!("Margin is a slider")
		};
		check(
			&mut d,
			Key::Margin,
			&|d| d.set_f32(Key::Margin, margin + 4.0),
			lines(
				&super::fmt_number(margin + 4.0, int),
				&super::fmt_number(margin, int),
			),
		);
		let minimap = d.defaults.minimap;
		let on_off = |on: bool| if on { "On" } else { "Off" };
		check(
			&mut d,
			Key::Minimap,
			&|d| d.set_toggle(Key::Minimap, !minimap),
			lines(on_off(!minimap), on_off(minimap)),
		);
		check(
			&mut d,
			Key::BgFit,
			&|d| d.set_radio(Key::BgFit, 1),
			lines("Zoom", "Stretch"),
		);
		let fg = config::format_hex(d.default_col(Key::ColFg));
		check(
			&mut d,
			Key::ColFg,
			&|d| d.set_col(Key::ColFg, [1, 2, 3]),
			lines("#010203", &fg),
		);
		let h = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, Kind::Hotkey(_)))
			.unwrap();
		let key = d.specs[h].key;
		let hotkey = d.hotkey_of(h).unwrap();
		let shipped = d.defaults.keys.chords(hotkey);
		let spoken = shipped
			.iter()
			.map(|chord| chord.spoken(d.mac))
			.collect::<Vec<_>>()
			.join(" or ");
		check(
			&mut d,
			key,
			&|d| d.edited.keys = d.edited.keys.with_own(hotkey, Some(Vec::new())),
			lines("Off", &spoken),
		);

		// an auto setting says it in its state line, and a group's switch holds
		// nothing to compare
		let size = row(&d, Key::FontSize);
		d.edited.font_size = config::auto::Auto::by_hand(30.0);
		assert!(
			!tip(&d, size, Key::FontSize)
				.unwrap()
				.contains("Current value")
		);
		d.edited.columns = config::auto::Auto::by_hand(99);
		let sw = row(&d, Key::RememberSize);
		assert!(tip(&d, sw, Key::RememberSize).is_none_or(|t| !t.contains("Current value")));
	}

	// Test ID: EpHT2u8
	#[test]
	fn a_grayed_control_takes_no_click() {
		let mut m = |s: &str| s.chars().count() as f32;
		let mut d = mk_dialog(4000.0);
		// Gray what still grays: the text colors under Text colors from
		// wallpaper, and the hidden wait on a desktop that never says. Rows that
		// only count while a switch is on stay live (2026100907341818), so the
		// wallpaper and the scrim no longer gray anything.
		d.edited.colors_from_wallpaper = true;
		d.set_sees_hidden(false);
		// what every row shows, which is as good a snapshot as the values
		let snapshot = |d: &SettingsDialog| -> Vec<(String, usize, bool)> {
			(0..d.specs.len())
				.map(|i| {
					(
						d.edit_buf(i),
						d.get_radio(d.specs[i].key),
						d.get_toggle(d.specs[i].key),
					)
				})
				.collect()
		};
		let before = snapshot(&d);

		let mut clicked = 0;
		for i in 0..d.specs.len() {
			if matches!(d.specs[i].kind, Kind::Header(_)) || !d.disabled(d.specs[i].key) {
				continue;
			}
			d.tab = d.specs[i].tab;
			let r = d.focus_ctl_rect(i, 0);
			d.mouse_down_dip(r.x + r.w / 2.0, r.y + r.h / 2.0, &mut m);
			clicked += 1;
			assert!(
				d.edit.is_none(),
				"{:?} opened an edit while grayed",
				d.specs[i].key
			);
			assert!(
				snapshot(&d) == before,
				"{:?} changed a setting while grayed",
				d.specs[i].key
			);
		}
		assert!(clicked >= 3, "only {clicked} grayed rows were reachable");
	}

	// While a profile is chosen, every row it governs shows the profile's value
	// and the user's own value waits underneath. Custom is the one profile that
	// governs nothing.
	// Test ID: EqRTxpJ
	#[test]
	fn a_profile_shows_its_values_in_its_rows() {
		use super::{GOVERNED, PROFILE_TIP};
		let mut d = mk_dialog(4000.0);
		d.edited.scroll_ease_in_ms = 300.0;
		d.edited.wallpaper_enabled = false;
		d.edited.margin = 7.0;
		let own = d.get_f32(Key::ScrollEaseIn);
		assert!(!d.disabled(Key::ScrollEaseIn));

		d.set_radio(Key::PerfProfile, super::Profile::Max.index());
		assert_eq!(
			d.get_f32(Key::ScrollEaseIn),
			d.default_f32(Key::ScrollEaseIn)
		);
		assert!(
			d.get_toggle(Key::BgEnabled),
			"Max shows the shipped default"
		);
		assert_eq!(
			d.get_f32(Key::Margin),
			7.0,
			"an ungoverned row is untouched"
		);
		// Governed rows used to be grayed. They take input now, and the edit is
		// what switches the profile off:
		//     assert!(d.disabled(*key), "{key:?} should be locked");
		for key in GOVERNED {
			assert!(!d.disabled(*key), "{key:?} should still take input");
		}
		assert!(!d.disabled(Key::Margin));
		let outline = d.specs.iter().position(|s| s.key == Key::Outline).unwrap();
		d.edited.text_outline = 3.0;
		assert!(
			d.row_is_default(outline),
			"a row showing a profile's value offers no revert"
		);
		// a member of a locked switch is grayed by the shown value, not the stored one
		assert!(!d.disabled(Key::BgImage), "the wallpaper is on under Max");
		// and the flyover says why, in place of the row's own help
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::ScrollEaseIn)
			.unwrap();
		d.tab = d.specs[i].tab;
		let ctl = d.checkbox(i);
		let tip = d
			.hover_tip_dip(ctl.x + 1.0, ctl.y + 1.0)
			.map(|(tip, _)| tip_text(tip));
		assert_eq!(tip, Some(PROFILE_TIP));

		d.set_radio(Key::PerfProfile, super::Profile::Custom.index());
		assert_eq!(
			d.get_f32(Key::ScrollEaseIn),
			own,
			"Custom puts the value back"
		);
		assert!(!d.get_toggle(Key::BgEnabled));
		assert!(!d.disabled(Key::ScrollEaseIn));
		// was: the wallpaper's rows grayed with it off. They stay live under it
		// now (2026100907341818).
		assert!(!d.disabled(Key::BgImage), "the wallpaper is off again");
		// The dropdown used to follow the automatic switch. It stays live now, so
		// a profile can be named while the machine is still choosing one:
		//     d.set_toggle(Key::PerfAuto, true);
		//     assert!(d.disabled(Key::PerfProfile));
		d.set_toggle(Key::PerfAuto, true);
		assert!(!d.disabled(Key::PerfProfile));
		d.set_toggle(Key::PerfAuto, false);
		assert!(!d.disabled(Key::PerfProfile));
	}

	// Nothing is drawn from the pointer itself, so a move is owed a frame only
	// when it changed something that is: the item lit in an open dropdown, a
	// dragged slider's value. dialog.rs draws nothing for any other move.
	// Test ID: Erlkwhi
	#[test]
	fn a_pointer_move_says_whether_it_changed_anything() {
		use super::{Key, Kind};
		let mut d = mk_dialog(2000.0);
		d.set_size(700.0, 600.0);
		let mut m = chars7;
		for k in 0..60 {
			let (x, y) = (d.rect.x + 11.0 * k as f32, d.rect.y + 9.0 * k as f32);
			assert!(
				!d.mouse_move_dip(x, y, &mut m),
				"a move to {x},{y} with nothing held changed something"
			);
		}
		// an open dropdown lights the item under the pointer
		let i = (0..d.specs.len())
			.find(|&j| matches!(d.specs[j].kind, Kind::Dropdown(_)) && d.dd_options(j).len() >= 2)
			.unwrap();
		d.tab = d.specs[i].tab;
		d.open = Some(i);
		d.pending = 0;
		let n = d.dd_options(i).len();
		let item = d.dd_item_rect(i, n, 1);
		let (x, y) = (item.x + item.w / 2.0, item.y + item.h / 2.0);
		assert!(
			d.mouse_move_dip(x, y, &mut m),
			"lighting another item drew nothing"
		);
		assert_eq!(d.pending, 1);
		assert!(
			!d.mouse_move_dip(x + 1.0, y, &mut m),
			"a move inside the lit item changed something"
		);
		d.open = None;
		// a dragged slider moves with the pointer, and only when its value does
		d.edited.font_size = config::auto::Auto::by_hand(6.0);
		let i = d.specs.iter().position(|s| s.key == Key::FontSize).unwrap();
		d.tab = d.specs[i].tab;
		d.drag = Some(i);
		let track = d.track(i);
		let x = track.x + track.w / 2.0;
		assert!(
			d.mouse_move_dip(x, track.y, &mut m),
			"dragging the slider drew nothing"
		);
		assert!(d.get_f32(Key::FontSize) > 6.0);
		assert!(
			!d.mouse_move_dip(x, track.y + 1.0, &mut m),
			"a drag that left the value alone changed something"
		);
	}

	// What dialog.rs asks of the dialog for one frame.
	fn one_frame(d: &SettingsDialog, mx: f32, my: f32) {
		let _ = d.hover_tip(mx, my);
		let _ = d.rects(18.0, chars7);
		let _ = d.texts(18.0, chars7);
		let _ = d.overlay(&mut chars7);
		let _ = d.hover_tip(mx, my);
	}

	// A frame reads every row's value, and a performance profile is chosen by
	// default. Laying the profile over a copy of the whole settings for each read
	// made a frame copy them dozens of times.
	// Test ID: ErleXuV
	#[test]
	fn a_dialog_frame_copies_the_settings_at_most_once() {
		for profile in super::Profile::ALL {
			let mut d = mk_dialog(4000.0);
			d.edited.performance_profile = profile;
			d.edited.remote_override = profile == super::Profile::Remote;
			for tab in 0..tab_titles().len() {
				d.tab = tab;
				let mid = d.rect.y + d.rect.h / 2.0;
				let before = crate::config::settings_clones();
				one_frame(&d, d.rect.x + d.rect.w / 2.0, mid);
				let copies = crate::config::settings_clones() - before;
				assert!(
					copies <= 1,
					"{profile:?}, tab {tab}: one frame copied the settings {copies} times"
				);
			}
		}
	}

	// Every rect helper asks for its row's top. Walking the tab from the top for
	// each one made a frame quadratic in the declarations.
	// Test ID: ErleYlF
	#[test]
	fn a_dialog_frame_walks_the_tab_at_most_once() {
		let mut d = mk_dialog(300.0);
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			d.scroll = 0.0;
			// a frame with nothing moved, then one scrolled
			for scroll in [0.0, 1.0] {
				d.scroll += scroll;
				let before = super::ROW_WALKS.with(std::cell::Cell::get);
				one_frame(&d, d.rect.x + d.rect.w / 2.0, d.rect.y + d.rect.h / 2.0);
				let walks = super::ROW_WALKS.with(std::cell::Cell::get) - before;
				assert!(
					walks <= 1,
					"tab {tab}: one frame walked the tab {walks} times"
				);
			}
		}
	}

	// A row's top is kept from one walk to the next, so anything the walk reads
	// that moves has to start a new one. Each change is made after a walk and
	// read through what was kept, then against a walk from nothing.
	// Test ID: Erlg3Ip
	#[test]
	fn a_kept_row_top_follows_what_it_was_walked_from() {
		let shell_tab = mk_dialog(300.0)
			.specs
			.iter()
			.find(|spec| matches!(spec.kind, super::Kind::ShellList))
			.unwrap()
			.tab;
		let changes: [(&str, fn(&mut SettingsDialog)); 5] = [
			("tab", |d| d.tab = (d.tab + 1) % tab_titles().len()),
			("scroll", |d| d.scroll += 13.5),
			("line height", |d| d.line_h += 3.0),
			("shell count", |d| {
				d.edited.shells.push(shell_entry("Extra", "/bin/extra"));
			}),
			("window top", |d| d.rect.y += 7.0),
		];
		for tab in 0..tab_titles().len() {
			for (what, change) in changes {
				let mut d = mk_dialog(300.0);
				d.tab = if what == "shell count" {
					shell_tab
				} else {
					tab
				};
				let _ = d.row_y(0);
				change(&mut d);
				let kept: Vec<f32> = (0..d.specs.len()).map(|i| d.row_y(i)).collect();
				*d.row_tops.borrow_mut() = super::RowTops::default();
				let fresh: Vec<f32> = (0..d.specs.len()).map(|i| d.row_y(i)).collect();
				assert_eq!(
					kept, fresh,
					"tab {tab}: the {what} moved and a row top did not"
				);
			}
		}
	}

	// The colors come from the live settings, behind a lock. Each of a frame's
	// three drawing calls builds them once and hands them down, rather than
	// every quad asking again.
	// Test ID: Erlfs8O
	#[test]
	fn a_dialog_frame_builds_its_colors_once_per_drawing_call() {
		let mut d = mk_dialog(300.0);
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			let before = super::DLG_BUILDS.with(std::cell::Cell::get);
			one_frame(&d, d.rect.x + d.rect.w / 2.0, d.rect.y + d.rect.h / 2.0);
			let builds = super::DLG_BUILDS.with(std::cell::Cell::get) - before;
			assert!(
				builds <= 3,
				"tab {tab}: one frame built the colors {builds} times"
			);
		}
	}

	// A governed row reads the profile's values and every other row reads the
	// user's. That answers what laying the profile over a copy did only while no
	// row's value comes from both kinds of field, so every reader is asked for
	// every row under every profile, on the defaults and on settings moved off
	// them.
	// Test ID: ErleYKw
	#[test]
	fn a_shown_value_is_the_profile_laid_over_the_settings() {
		use super::Profile;
		// every value a row holds, moved once: the far end of a slider, the other
		// state of a checkbox, the next option
		fn moved(d: &mut SettingsDialog) {
			for i in 0..d.specs.len() {
				let key = d.specs[i].key;
				if let super::Kind::Dual { keys, .. } = d.specs[i].kind {
					for part in keys {
						let was = d.get_toggle(part);
						d.set_toggle(part, !was);
					}
				} else if !matches!(key, Key::PerfProfile | Key::PerfAuto) {
					nudge(d, i, key);
				}
			}
		}
		let base = mk_dialog(4000.0);
		let mut once = mk_dialog(4000.0);
		moved(&mut once);
		let mut twice = mk_dialog(4000.0);
		twice.edited = once.edited.clone();
		moved(&mut twice);
		for start in [&base.edited, &once.edited, &twice.edited] {
			for profile in Profile::ALL {
				for stepped in [None, Some(Profile::Low), Some(Profile::Standard)] {
					let mut d = mk_dialog(4000.0);
					d.edited = start.clone();
					d.edited.performance_profile = profile;
					d.edited.remote_override = profile == Profile::Remote;
					d.edited.performance_automatic = stepped.is_some();
					d.edited.stepped_profile = stepped;
					let mut laid = d.edited.clone();
					crate::profile::apply(&mut laid);
					for &key in Key::ALL {
						let case = format!("{key:?} under {profile:?}, stepped {stepped:?}");
						assert_eq!(
							d.get_f32(key).to_bits(),
							super::slider_of(&laid, key).to_bits(),
							"{case}"
						);
						assert_eq!(d.get_toggle(key), super::toggle_of(&laid, key), "{case}");
						assert_eq!(d.get_radio(key), super::radio_of(&laid, key), "{case}");
					}
				}
			}
		}
	}

	// A row's shown value, whichever kind it is, as something comparable.
	fn shown_of(d: &SettingsDialog, i: usize, key: Key) -> String {
		match d.specs[i].kind {
			super::Kind::Slider { .. } => format!("{}", d.get_f32(key)),
			super::Kind::Toggle | super::Kind::Dual { .. } => format!("{}", d.get_toggle(key)),
			_ => format!("{}", d.get_radio(key)),
		}
	}

	// Changing a row a profile governs is how the profile is taken back. The
	// values on screen become the user's own, the profile drops to Custom and
	// the machine stops choosing - and every other governed row keeps what it
	// was showing, so only the row that was touched moves.
	// Test ID: EqRTxpK
	#[test]
	fn changing_a_governed_row_takes_the_profile_to_custom() {
		use super::GOVERNED;
		for &key in GOVERNED {
			let mut d = mk_dialog(4000.0);
			d.set_radio(Key::PerfProfile, super::Profile::Low.index());
			d.edited.performance_automatic = true; // the pick above switched it off
			let i = d.specs.iter().position(|s| s.key == key).unwrap();
			let others: Vec<(usize, Key, String)> = GOVERNED
				.iter()
				.filter(|&&k| k != key)
				.map(|&k| {
					let j = d.specs.iter().position(|s| s.key == k).unwrap();
					(j, k, shown_of(&d, j, k))
				})
				.collect();
			let before = shown_of(&d, i, key);

			nudge(&mut d, i, key);

			assert_eq!(
				crate::profile::current(&d.edited),
				super::Profile::Custom,
				"{key:?} should take the profile to Custom"
			);
			assert!(
				!d.edited.performance_automatic,
				"{key:?} should switch the automatic choice off"
			);
			assert_ne!(
				shown_of(&d, i, key),
				before,
				"{key:?} should hold what was typed into it"
			);
			for (j, k, was) in others {
				assert_eq!(shown_of(&d, j, k), was, "{k:?} should keep what it showed");
			}
		}
	}

	// Remote lasts the session only and normally leaves the stored profile alone.
	// A change to a setting it governs is the one thing that has to move it: the
	// override would otherwise go on covering the new value, and so would the
	// stored profile underneath it.
	// Test ID: EqRTxpL
	#[test]
	fn changing_a_governed_row_under_remote_drops_the_override() {
		let mut d = mk_dialog(900.0);
		d.edited.performance_profile = crate::profile::Profile::High;
		d.set_radio(Key::PerfProfile, super::Profile::Remote.index());
		assert!(d.edited.remote_override);

		d.set_f32(Key::Outline, 3.0);
		assert!(!d.edited.remote_override, "the override has to go");
		assert_eq!(
			d.edited.performance_profile,
			crate::profile::Profile::Custom
		);
		assert_eq!(d.get_f32(Key::Outline), 3.0);
	}

	// Naming a profile is how the automatic choice is taken back, or the pick
	// would be overwritten at the next launch with nothing to show for it.
	// Remote is the exception: it lasts this session only and says nothing about
	// what the machine should settle on.
	// Test ID: EqMD0kr
	#[test]
	fn naming_a_profile_switches_off_the_automatic_choice_except_remote() {
		let mut d = mk_dialog(900.0);
		d.set_toggle(Key::PerfAuto, true);
		d.set_radio(Key::PerfProfile, super::Profile::High.index());
		assert!(
			!d.get_toggle(Key::PerfAuto),
			"a named profile leaves the machine still choosing"
		);
		assert_eq!(d.edited.performance_profile, crate::profile::Profile::High);

		d.set_toggle(Key::PerfAuto, true);
		d.set_radio(Key::PerfProfile, super::Profile::Remote.index());
		assert!(
			d.get_toggle(Key::PerfAuto),
			"a temporary remote pick should leave the switch alone"
		);
		assert!(d.edited.remote_override);
	}

	// A 0..1 fraction reads as a whole percent and is stored as the decimal. The
	// two directions have to be exact inverses: a revert that came a hair off
	// its own default would leave the arrow lit with nothing to undo.
	// Test ID: Em3akaJ
	#[test]
	fn a_fraction_reads_as_a_whole_percent_and_stores_as_a_decimal() {
		let mut d = mk_dialog(4000.0);
		for key in [
			Key::Opacity,
			Key::BgOpacity,
			Key::ScrimSoftness,
			Key::MinContrast,
			Key::BgContrastSize,
			Key::BgContrastStrength,
			Key::BgContrastAuto,
		] {
			let spec = d.specs.iter().find(|s| s.key == key).unwrap();
			let super::Kind::Slider { min, max, int, .. } = spec.kind else {
				panic!("{} is not a slider", spec.label)
			};
			assert!(
				min == 0.0 && int && max <= 100.0 && max.fract() == 0.0,
				"{} must run from 0 to a whole percent, in whole steps",
				spec.label
			);
			d.set_f32(key, 35.0);
			assert_eq!(d.get_f32(key), 35.0, "{}", spec.label);
			d.revert(key);
			assert!(d.is_default(key), "{}", spec.label);
			assert_eq!(d.get_f32(key), d.default_f32(key), "{}", spec.label);
		}
		// and the decimal really is what reaches the settings the app runs on
		d.set_f32(Key::Opacity, 35.0);
		assert_eq!(d.edited.opacity, 0.35);
		d.set_f32(Key::BgContrastSize, 100.0);
		assert_eq!(d.edited.wallpaper_contrast_mask_size, 1.0);
	}

	// A heading that only repeats its tab's title is gone from the layout
	// entirely - not merely hidden, or it would leave a gap where it used to be.
	// Test ID: Em3Pif4
	#[test]
	fn a_heading_that_repeats_its_tab_takes_no_room() {
		let mut d = mk_dialog(2000.0);
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			let redundant = d
				.specs
				.iter()
				.enumerate()
				.find(|(_, spec)| spec.tab == tab && SettingsDialog::header_is_tab_title(spec));
			if redundant.is_none() {
				continue;
			}
			// the first row that IS drawn starts hard against the top of the
			// viewport, so the heading left no gap behind it
			let first = (0..d.specs.len())
				.find(|&j| {
					d.specs[j].tab == tab && !SettingsDialog::header_is_tab_title(&d.specs[j])
				})
				.expect("a tab with rows");
			assert!((d.row_y(first) - (d.rows_y0() - d.scroll)).abs() < 0.01);
			// and the tab's height accounts for the rows it draws and nothing
			// more: the surplus over them is header gaps, which are far smaller
			// than the heading row that was dropped
			let shells = d.edited.shells.len();
			let drawn: f32 = d
				.specs
				.iter()
				.filter(|s| s.tab == tab && !SettingsDialog::header_is_tab_title(s))
				// Changed 20261006: a shared line is drawn once, so a row with
				// others beside it counts once (2026100614510984). This summed
				// every row whole, which held only while no tab with a hidden
				// heading had a shared line:
				//     .map(|s| SettingsDialog::row_h_for(&s.kind, d.line_h, shells))
				.map(|s| {
					let i = d.specs.iter().position(|o| std::ptr::eq(o, s)).unwrap();
					SettingsDialog::row_advance(d.specs, i, tab, d.line_h, shells)
				})
				.sum();
			// the surplus over the rows is the declared gaps and NOTHING else -
			// stated exactly rather than bounded by a heading's height, so
			// retuning header_gap cannot quietly turn this into a near miss
			let mut gaps = 0.0;
			let mut prev = None;
			for (i, s) in SettingsDialog::visible(d.specs, tab) {
				gaps += SettingsDialog::gap_above(d.specs, i, tab, prev);
				prev = Some(s);
			}
			let counted = SettingsDialog::tab_content_h(d.specs, tab, d.line_h, shells);
			assert!(
				(counted - drawn - gaps).abs() < 0.01,
				"tab {tab}: {counted} != {drawn} rows + {gaps} gaps"
			);
		}
	}

	pub(super) fn shell_entry(title: &str, command: &str) -> crate::shells::ShellEntry {
		crate::shells::ShellEntry {
			slug: title.to_lowercase().replace(' ', "_"),
			title: title.into(),
			command: command.into(),
			active: true,
			comment: String::new(),
			last_seen: String::new(),
		}
	}

	// A dialog on the Shell tab, writing to `store`, with row `key` in view.
	fn mk_assoc_dialog(
		key: Key,
		store: Box<dyn crate::fileassoc::Store + Send>,
	) -> (SettingsDialog, usize) {
		let mut d = mk_dialog(4000.0);
		d.assoc = store;
		d.assoc_refresh();
		let i = d
			.specs
			.iter()
			.position(|s| s.key == key)
			.expect("a file-type row");
		d.tab = d.specs[i].tab;
		// the Shell tab is not what sizes the dialog, so its last rows may need
		// scrolling to
		d.focus = Some(super::Focus::Row(i, 0));
		d.scroll_focus_into_view();
		d.focus = None;
		(d, i)
	}

	fn click_on(d: &mut SettingsDialog, r: crate::pane::Rect) {
		let mut measure = |s: &str| s.len() as f32;
		click(d, r.x + 2.0, r.y + 2.0, &mut measure);
	}

	// Register acts at once and lights the arrow; the arrow puts back what was
	// there and goes dim again.
	// Test ID: ErNGry1
	#[test]
	fn a_file_type_registers_at_once_and_its_arrow_puts_it_back() {
		let store = Box::new(crate::fileassoc::Memory::default());
		let (mut d, i) = mk_assoc_dialog(Key::OpenBatch, store);
		assert!(
			d.has_revert(i) && d.row_is_default(i),
			"nothing to put back yet"
		);
		assert!(!d.part_disabled(i, 0));
		let command = r"HKCU\Software\Classes\batfile\shell\open\command";
		let r = d.row_btn_rect(i, 0);
		click_on(&mut d, r);
		assert!(
			d.assoc
				.get(command, "")
				.is_some_and(|v| v.text.contains("--open"))
		);
		assert!(!d.row_is_default(i), "the arrow lights");
		assert!(d.prompt.is_none(), "nothing to say");
		let r = d.revert_box(i);
		click_on(&mut d, r);
		assert!(d.assoc.get(command, "").is_none());
		assert!(d.row_is_default(i));
	}

	// The button and the arrow each say what they do.
	// Test ID: ErNGry2
	#[test]
	fn a_file_types_button_and_arrow_have_their_own_tips() {
		let store = Box::new(crate::fileassoc::Memory::default());
		for key in [
			Key::OpenBatch,
			Key::OpenPowerShell,
			Key::OpenVbScript,
			Key::OpenFolder,
		] {
			let (d, i) = mk_assoc_dialog(key, store.clone());
			let tip = |r: crate::pane::Rect| {
				d.hover_tip(r.x + 2.0, r.y + 2.0)
					.map(|(tip, _)| tip_text(tip))
			};
			assert!(!d.specs[i].help.is_empty() && !d.specs[i].revert_help.is_empty());
			assert_eq!(tip(d.row_btn_rect(i, 0)), Some(d.specs[i].help), "{key:?}");
			assert_eq!(
				tip(d.revert_box(i)),
				Some(d.specs[i].revert_help),
				"{key:?}"
			);
		}
	}

	// A type the user picked an app for under Open with keeps that app, and
	// the dialog says so rather than looking as if it worked.
	// Test ID: ErNGry3
	#[test]
	fn a_type_windows_keeps_elsewhere_is_reported() {
		use crate::fileassoc::Store;
		let mut store = crate::fileassoc::Memory::default();
		let choice =
			r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.ps1\UserChoice";
		let picked = crate::fileassoc::Value {
			kind: crate::fileassoc::REG_SZ,
			text: "AppXsomething".into(),
		};
		store.set(choice, "ProgId", &picked).unwrap();
		let (mut d, i) = mk_assoc_dialog(Key::OpenPowerShell, Box::new(store));
		let r = d.row_btn_rect(i, 0);
		click_on(&mut d, r);
		let prompt = d.prompt.as_ref().expect("a notice");
		assert_eq!(prompt.job, super::PromptJob::Notice);
		assert!(prompt.title.contains(".ps1"), "{}", prompt.title);
		assert_eq!(prompt.parts(), [super::PromptFocus::Ok]);
		assert!(!d.row_is_default(i), "it is registered all the same");
		d.key_enter();
		assert!(d.prompt.is_none());
	}

	// Test ID: ErNGry4
	#[test]
	fn a_registry_that_refuses_says_why() {
		struct Refuses;
		impl crate::fileassoc::Store for Refuses {
			fn get(&self, _: &str, _: &str) -> Option<crate::fileassoc::Value> {
				None
			}
			fn set(
				&mut self,
				key: &str,
				_: &str,
				_: &crate::fileassoc::Value,
			) -> anyhow::Result<()> {
				Err(anyhow::anyhow!("Access is denied.").context(format!("cannot write {key}")))
			}
			fn delete(&mut self, _: &str, _: &str) -> anyhow::Result<()> {
				Ok(())
			}
			fn exists(&self, _: &str) -> bool {
				false
			}
			fn prune(&mut self, _: &str) -> anyhow::Result<()> {
				Ok(())
			}
		}
		let (mut d, i) = mk_assoc_dialog(Key::OpenFolder, Box::new(Refuses));
		let r = d.row_btn_rect(i, 0);
		click_on(&mut d, r);
		let prompt = d.prompt.as_ref().expect("a notice");
		assert!(
			prompt
				.warn
				.as_deref()
				.is_some_and(|w| w.contains("Access is denied"))
		);
		assert!(d.row_is_default(i), "nothing was registered");
	}

	// Test ID: EnQUIKp
	#[test]
	fn the_shells_grid_has_a_tab_to_itself() {
		let ui = super::ui_spec::ui();
		let grid = ui
			.specs
			.iter()
			.find(|s| matches!(s.kind, super::Kind::ShellList))
			.expect("a shells grid");
		assert_eq!(tab_titles()[grid.tab], "Shell");
		// The tab is the grid, its headings, the startup directory the grid's own
		// default shell starts in, the two switches for what SilkTerm sets up in a
		// shell it starts, and on Windows the script types it opens - nothing else
		// belongs beside them.
		for spec in ui.specs.iter().filter(|s| s.tab == grid.tab) {
			assert!(
				matches!(spec.kind, super::Kind::ShellList | super::Kind::Header(_))
					|| spec.key == Key::StartupDirectory
					|| spec.key == Key::ShellIntegration
					|| spec.key == Key::BashPrompt
					|| super::assoc_of(spec.key).is_some(),
				"{} does not belong on the Shell tab",
				spec.label
			);
		}
		// and the directory reads BELOW the list it applies to
		let dir = ui
			.specs
			.iter()
			.position(|s| s.key == Key::StartupDirectory)
			.expect("a startup directory row");
		let at = ui
			.specs
			.iter()
			.position(|s| matches!(s.kind, super::Kind::ShellList))
			.expect("a shells grid");
		assert!(dir > at, "the startup directory sits below the shells");
	}

	// Asked for on the Cursor tab, and asked for LAST - so both halves are
	// pinned, or a row appended later quietly takes its place.
	// Test ID: EnQUIKq
	#[test]
	fn copy_on_select_is_the_last_thing_on_the_cursor_tab() {
		let d = mk_dialog(4000.0);
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::CopyOnSelect)
			.expect("a copy-on-select row");
		let tab = d.specs[i].tab;
		assert_eq!(tab_titles()[tab], "Cursor");
		let last = SettingsDialog::visible(d.specs, tab)
			.map(|(j, _)| j)
			.last()
			.expect("the tab draws rows");
		assert_eq!(last, i);
	}

	// Test ID: Elzhjoe
	#[test]
	fn a_restored_view_never_outruns_the_new_dialog() {
		// scrolled to the bottom of the last tab, as if the user had just closed it
		let mut d = mk_dialog(400.0);
		// the tallest tab, not merely the last: which tab overflows a short
		// window is a property of the content, and a new tab can change it. The
		// Shell tab is left out, since its height is the shell count's.
		d.tab = SettingsDialog::fixed_tabs(d.specs)
			.max_by(|a, b| {
				let h = |t: usize| {
					SettingsDialog::tab_content_h(d.specs, t, d.line_h, d.edited.shells.len())
				};
				h(*a).total_cmp(&h(*b))
			})
			.expect("at least one tab");
		d.wheel(0.0, -1e9);
		let view = d.view();
		assert!(view.scroll > 0.0);
		let mut same = mk_dialog(400.0);
		same.restore(view);
		assert_eq!(same.tab, view.tab);
		assert_eq!(same.scroll, view.scroll);
		// a roomier window has nothing to scroll, so the offset must not survive
		let mut roomy = mk_dialog(2000.0);
		roomy.restore(view);
		assert_eq!(roomy.tab, view.tab);
		assert_eq!(roomy.scroll, 0.0);
		// a tab that no longer exists leaves the fresh dialog alone
		let mut d = mk_dialog(400.0);
		d.restore(super::View {
			tab: tab_titles().len(),
			scroll: 50.0,
		});
		assert_eq!(d.tab, 0);
		assert_eq!(d.scroll, 0.0);
	}

	// Test ID: EitmBBQ
	#[test]
	fn keyboard_focus_walks_controls_then_buttons() {
		use super::Focus;
		let mut d = mk_dialog(2000.0);
		let master = d
			.specs
			.iter()
			.position(|s| s.key == super::Key::SmoothScroll)
			.unwrap();
		d.tab = d.specs[master].tab;
		let f = d.focusables();
		let at = f.iter().position(|&i| i == master).unwrap();
		d.set_mods(false, false, false);
		d.key_tab(); // from nothing -> the first control on the tab
		assert_eq!(d.focus, Some(Focus::Row(f[0], 0)));
		// the smooth-scroll master toggle is a single stop; each slider under it
		// is two (track, then numeric field)
		d.focus = Some(Focus::Row(f[at], 0));
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Row(f[at + 1], 0)));
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Row(f[at + 1], 1)));
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Row(f[at + 2], 0)));
		// after the LAST control the ring visits the three footer buttons
		let last = *f.last().unwrap();
		d.focus = Some(super::Focus::Row(last, d.parts_of(last) - 1));
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Button(0)));
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Button(1)));
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Button(2)));
		d.key_tab(); // wraps back to the first control
		assert_eq!(d.focus, Some(Focus::Row(f[0], 0)));
		d.set_mods(false, true, false); // Shift+Tab walks back (wraps to last button)
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Button(2)));
	}

	// Test ID: Ejak3Qu
	#[test]
	fn dual_cursor_row_two_stops_toggle_and_revert() {
		use super::{Focus, Kind};
		let mut d = mk_dialog(2000.0);
		let i = d
			.specs
			.iter()
			.position(
				|s| matches!(s.kind, Kind::Dual { keys, .. } if keys[0] == super::Key::CursorScrim),
			)
			.unwrap();
		d.tab = d.specs[i].tab;
		// enabled prerequisites: scrim on, an outline present
		d.edited.text_scrim = true;
		d.edited.text_outline = 2.0;
		assert_eq!(d.parts_of(i), 2);
		assert!(!d.part_disabled(i, 0) && !d.part_disabled(i, 1));
		// Space on each part flips its own key
		let (s0, o0) = (d.edited.cursor_scrim, d.edited.cursor_outline);
		d.focus = Some(Focus::Row(i, 0));
		d.key_space();
		assert_eq!(d.edited.cursor_scrim, !s0);
		assert_eq!(d.edited.cursor_outline, o0, "part 0 leaves outline alone");
		d.focus = Some(Focus::Row(i, 1));
		d.key_space();
		assert_eq!(d.edited.cursor_outline, !o0);
		// was: no outline grayed the Outline checkbox. It only counts while there
		// is an outline, so it stays live (2026100907341818).
		d.edited.text_outline = 0.0;
		assert!(!d.part_disabled(i, 1) && !d.part_disabled(i, 0));
		// reverting the row restores both keys
		d.edited.text_outline = 2.0;
		d.edited.cursor_scrim = !d.defaults.cursor_scrim;
		d.edited.cursor_outline = !d.defaults.cursor_outline;
		assert!(!d.row_is_default(i));
		d.row_revert(i);
		assert_eq!(d.edited.cursor_scrim, d.defaults.cursor_scrim);
		assert_eq!(d.edited.cursor_outline, d.defaults.cursor_outline);
		assert!(d.row_is_default(i));
		assert!(d.take_reverted().contains(&"cursor.scrim"));
	}

	// // Software rendering is grayed, with its reason, only where the platform
	// // has no software renderer. Elsewhere it is an ordinary switch on the
	// // Window tab that a click flips.
	// // Test ID: ErnMaGS
	// #[test]
	// fn software_rendering_is_grayed_only_without_a_software_renderer() {
	// 	use super::Key;
	// 	assert_eq!(super::software_tip(true), None);
	// 	assert!(super::software_tip(false).is_some_and(|tip| tip.contains("macOS")));
	// 	let mut d = mk_dialog(2000.0);
	// 	let i = d
	// 		.specs
	// 		.iter()
	// 		.position(|s| matches!(s.key, Key::SoftwareRendering))
	// 		.unwrap();
	// 	assert_eq!(
	// 		d.specs[i].tab,
	// 		d.specs
	// 			.iter()
	// 			.find(|s| matches!(s.key, Key::IdleRelease))
	// 			.unwrap()
	// 			.tab
	// 	);
	// 	assert!(!d.defaults.software_rendering, "default off");
	// 	d.tab = d.specs[i].tab;
	// 	assert_eq!(
	// 		d.disabled(Key::SoftwareRendering),
	// 		!crate::gfx::SOFTWARE_POSSIBLE
	// 	);
	// 	if crate::gfx::SOFTWARE_POSSIBLE {
	// 		let bx = d.checkbox(i);
	// 		let mut measure = |s: &str| s.len() as f32;
	// 		d.mouse_down(bx.x + 2.0, bx.y + 2.0, &mut measure);
	// 		assert!(d.edited.software_rendering);
	// 	}
	// }

	// Software rendering is an ordinary switch on the Window tab that a click
	// flips, never grayed. The macOS build has no row for it at all.
	// Test ID: ErycRwI
	#[test]
	fn software_rendering_is_a_plain_switch_beside_the_idle_rows() {
		use super::Key;
		let mut d = mk_dialog(2000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.key, Key::SoftwareRendering))
			.unwrap();
		assert_eq!(
			d.specs[i].tab,
			d.specs
				.iter()
				.find(|s| matches!(s.key, Key::IdleRelease))
				.unwrap()
				.tab
		);
		assert!(d.specs[i].not_macos);
		assert!(!d.defaults.software_rendering, "default off");
		d.tab = d.specs[i].tab;
		assert!(!d.disabled(Key::SoftwareRendering));
		let bx = d.checkbox(i);
		let mut measure = |s: &str| s.len() as f32;
		d.mouse_down(bx.x + 2.0, bx.y + 2.0, &mut measure);
		assert!(d.edited.software_rendering);
	}

	// Commented out by 2026100907341818: the Use system font switches are gone.
	// Family and Size are automatic settings whose automatic value is the
	// desktop's font, so nothing is grayed for a desktop that names none.
	// `the_font_rows_show_the_desktops_font_until_set_by_hand` replaces it.
	//
	// // The "use system font" face toggle is inert wherever the OS reports no
	// // monospace family - always on Windows, and on a desktop with none set. That
	// // is a property of the environment, not of the platform, so the test asks the
	// // same question the code does.
	// // Test ID: ElEvh0T
	// #[test]
	// fn system_font_toggle_inert_without_an_os_family() {
	// 	use super::Key;
	// 	let mut d = mk_dialog(2000.0);
	// 	let i = d
	// 		.specs
	// 		.iter()
	// 		.position(|s| matches!(s.key, Key::SystemFont))
	// 		.unwrap();
	// 	d.tab = d.specs[i].tab;
	// 	let bx = d.checkbox(i);
	// 	if crate::sysfont::monospace().family.is_none() {
	// 		assert!(d.disabled(Key::SystemFont));
	// 		// Grayed is what says "inert"; the box still shows what is stored, both
	// 		// ways. It used to show off whatever was stored, which put an unchecked
	// 		// box beside an at-default revert arrow - the two disagreeing about a
	// 		// default that is on.
	// 		d.edited.use_system_font = true;
	// 		assert!(d.get_toggle(Key::SystemFont));
	// 		d.edited.use_system_font = false;
	// 		assert!(!d.get_toggle(Key::SystemFont));
	// 		d.edited.use_system_font = d.defaults.use_system_font;
	// 		assert_eq!(
	// 			d.get_toggle(Key::SystemFont),
	// 			d.defaults.use_system_font,
	// 			"the box must show the default it reports"
	// 		);
	// 		assert!(d.is_default(Key::SystemFont));
	// 		// clicking the grayed checkbox must not flip the setting
	// 		let mut measure = |s: &str| s.len() as f32;
	// 		d.mouse_down(bx.x + 2.0, bx.y + 2.0, &mut measure);
	// 		assert!(d.edited.use_system_font);
	// 		// the flyover explains WHY it is grayed, in place of the row's own
	// 		// help text, and only over the row
	// 		assert_eq!(
	// 			d.hover_tip(bx.x + 2.0, bx.y + 2.0).map(|(tip, _)| tip_text(tip)),
	// 			Some("The desktop reports no monospace font to follow.")
	// 		);
	// 		assert!(d.hover_tip(bx.x + 2.0, bx.y - 200.0).is_none());
	// 		// the family field stays editable, since it is what actually resolves
	// 		assert!(!d.disabled(Key::FontFamily));
	// 	} else {
	// 		assert!(!d.disabled(Key::SystemFont));
	// 		// live, so the row explains what it does rather than why it cannot
	// 		assert_ne!(
	// 			d.hover_tip(bx.x + 2.0, bx.y + 2.0).map(|(tip, _)| tip_text(tip)),
	// 			Some("The desktop reports no monospace font to follow.")
	// 		);
	// 		// following the OS grays the field it overrides
	// 		d.edited.use_system_font = true;
	// 		assert!(d.disabled(Key::FontFamily));
	// 		d.edited.use_system_font = false;
	// 		assert!(!d.disabled(Key::FontFamily));
	// 	}
	// }

	// Test ID: Em2dFyi
	#[test]
	fn a_large_ui_font_scales_radio_layout_and_widens_panel() {
		use super::Kind;
		// base vs a desktop UI font twice the size (a bigger font, same DPI)
		let base = mk_dialog(4000.0);
		let big = SettingsDialog::new(
			0.0,
			0.0,
			38.0,
			Chrome {
				label_w: 340.0,
				btn_w: 160.0,
				row_btn_w: 180.0,
				value_w: 0.0,
				tab_ws: vec![180.0; tab_titles().len()],
				label_ws: labels7(2.0),
				..Chrome::default()
			},
			f32::MAX,
			4000.0,
			1.0,
		);
		// radio pitch tracks the font so multi-option labels don't collide
		assert!(big.radio_pitch() > base.radio_pitch() * 1.5);
		// the widest radio's last option stays inside the panel
		let (ri, opts) = big
			.specs
			.iter()
			.enumerate()
			.filter_map(|(i, s)| match s.kind {
				Kind::Radio(o) => Some((i, o.len())),
				_ => None,
			})
			.max_by_key(|(_, n)| *n)
			.unwrap();
		let last = big.radio_box(ri, opts - 1);
		assert!(
			last.x + last.w <= big.rect.x + big.rect.w,
			"last radio option overflows the panel at 2x"
		);
	}

	// The layout is DIP, so a doubled scale factor may only multiply it: same
	// dialog, twice the pixels, and a pointer still hits the same control.
	// Test ID: Em2dFyj
	#[test]
	fn the_scale_factor_only_multiplies_the_layout() {
		let mut base = mk_dialog(4000.0);
		let mut hidpi = mk_dialog_at(4000.0, 2.0);
		base.tab = 1; // Background, where the Transparency checkbox is
		hidpi.tab = 1;
		let ((bw, bh), (hw, hh)) = (base.size(), hidpi.size());
		assert!(
			(hw - bw * 2.0).abs() < 0.01 && (hh - bh * 2.0).abs() < 0.01,
			"window {bw}x{bh} at 1x vs {hw}x{hh} at 2x"
		);
		let (bv, hv) = (base.viewport_px(), hidpi.viewport_px());
		assert!((hv.y - bv.y * 2.0).abs() < 0.01 && (hv.h - bv.h * 2.0).abs() < 0.01);
		// a click at the checkbox's physical center still toggles its setting
		let i = base
			.specs
			.iter()
			.position(|s| s.key == Key::Transparency)
			.unwrap();
		let target = base.checkbox(i);
		let mut d = hidpi;
		let before = d.edited.transparent_background;
		d.mouse_down(
			(target.x + target.w / 2.0) * 2.0,
			(target.y + target.h / 2.0) * 2.0,
			&mut |s: &str| s.len() as f32 * 12.0,
		);
		assert_ne!(d.edited.transparent_background, before);
	}

	// Test ID: EjYm8dk
	#[test]
	fn dropdown_open_navigate_commit() {
		use super::{Action, Focus, Key, Kind};
		let mut d = mk_dialog(2000.0);
		d.tab = 0;
		d.edited.text_scrim = true; // not grayed out
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::ScrimFunction)
			.unwrap();
		assert!(matches!(d.specs[i].kind, Kind::Dropdown(_)));
		d.edited.text_scrim_function = crate::scrim::Function::Sdf; // option index 0
		d.focus = Some(Focus::Row(i, 0));
		// Space opens with the current value highlighted
		d.key_space();
		assert_eq!(d.open, Some(i));
		assert_eq!(d.pending, 0);
		// Down moves the highlight but does not commit yet
		d.key_vertical(true);
		assert_eq!(d.pending, 1);
		assert_eq!(
			d.edited.text_scrim_function,
			crate::scrim::Function::Sdf,
			"not committed until Enter"
		);
		// Enter commits + closes
		assert!(matches!(d.key_enter(), Action::None));
		assert_eq!(d.open, None);
		assert_eq!(d.edited.text_scrim_function, crate::scrim::Function::Dt); // index 1
		// reopen, move, Esc -> closes and discards the highlight
		d.key_space();
		d.key_vertical(true);
		assert_eq!(d.key_escape(), Action::None);
		assert_eq!(d.open, None);
		assert_eq!(d.edited.text_scrim_function, crate::scrim::Function::Dt);
	}

	// Each option a choice row lists stands for one value, both ways round: the
	// row shows it for that value, and picking it stores that value.
	// Test ID: ErstamD
	#[test]
	fn each_choice_row_shows_and_sets_the_option_it_names() {
		use super::Key;
		use crate::config::Settings;
		use crate::pane::CursorAnimation as C;
		use crate::scrim::{Function as F, Ramp as R};
		use crate::theme::Mode as M;
		fn row<T: Copy + PartialEq + std::fmt::Debug>(
			d: &mut super::SettingsDialog,
			key: Key,
			field: fn(&mut Settings) -> &mut T,
			cases: &[(T, &str)],
		) {
			let i = d.specs.iter().position(|s| s.key == key).unwrap();
			let options = d.dd_options(i);
			assert_eq!(options.len(), cases.len(), "{key:?}");
			for (k, &(value, name)) in cases.iter().enumerate() {
				*field(&mut d.edited) = value;
				assert_eq!(options[d.get_radio(key)], name, "{key:?} {value:?}");
				*field(&mut d.edited) = cases[(k + 1) % cases.len()].0;
				let at = options.iter().position(|o| o == name).unwrap();
				d.set_radio(key, at);
				assert_eq!(*field(&mut d.edited), value, "{key:?} {name}");
			}
		}
		let mut d = mk_dialog(2000.0);
		// governed rows show the profile's values unless the profile is Custom
		d.edited.performance_profile = crate::profile::Profile::Custom;
		d.edited.performance_automatic = false;
		row(
			&mut d,
			Key::ScrimFunction,
			|s| &mut s.text_scrim_function,
			&[
				(F::Sdf, "Distance field"),
				(F::Dt, "Distance transform"),
				(F::Dilate, "Dilate + feather"),
				(F::Gaussian, "Gaussian [ugly]"),
			],
		);
		row(
			&mut d,
			Key::ScrimRamp,
			|s| &mut s.text_scrim_ramp,
			&[
				(R::Exp, "Exponential"),
				(R::HalfNormal, "Half-normal"),
				(R::Log, "Logarithmic"),
				(R::Sigmoid, "Sigmoid"),
				(R::Linear, "Linear"),
			],
		);
		row(
			&mut d,
			Key::CursorAnimation,
			|s| &mut s.cursor_animation,
			&[
				(C::Phase, "Phase"),
				(C::PulseVertical, "Pulse vertical"),
				(C::PulseHorizontal, "Pulse horizontal"),
				(C::PulseBoth, "Pulse both"),
			],
		);
		row(
			&mut d,
			Key::ThemeMode,
			|s| &mut s.theme_mode,
			&[
				(M::Dark, "Dark"),
				(M::Light, "Light"),
				(M::System, "System"),
			],
		);
	}

	// Test ID: EjYm8dl
	#[test]
	fn dropdown_mouse_open_and_pick() {
		use super::Key;
		let mut d = mk_dialog(2000.0);
		d.edited.text_scrim = true;
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::ScrimRamp)
			.unwrap();
		d.tab = d.specs[i].tab;
		let n = d.dd_options(i).len();
		let mut m = |_: &str| 8.0;
		// click the collapsed box opens the popup
		let box_r = d.dd_box(i);
		d.mouse_down(box_r.x + 4.0, box_r.y + 4.0, &mut m);
		assert_eq!(d.open, Some(i));
		// click option 2 ("Logarithmic") selects it and closes
		let r = d.dd_item_rect(i, n, 2);
		d.mouse_down(r.x + 4.0, r.y + r.h / 2.0, &mut m);
		assert_eq!(d.open, None);
		assert_eq!(d.edited.text_scrim_ramp, crate::scrim::Ramp::Log);
	}

	// Test ID: ElpQBin
	#[test]
	fn the_scrolling_feel_sliders_read_where_their_defaults_claim() {
		// Every one of these is documented in the config template as coming out at a
		// particular number, and each stored default was picked to match. A
		// range or a default edited without its comment would drift silently.
		let d = config::Settings::default();
		assert_eq!(tau_to_speed(d.scroll_single_screen_tau_ms), 75.0);
		for (got, want, what) in [
			(
				falling_slider(d.scroll_ease_in_ms, EASE_IN_MIN, EASE_IN_MAX),
				50.0,
				"ease-in",
			),
			(
				falling_slider(d.scroll_ramp_up_ms, RAMP_UP_MIN, RAMP_UP_MAX),
				75.0,
				"ramp-up",
			),
			(
				falling_slider(d.scroll_ramp_down_ms, RAMP_DOWN_MIN, RAMP_DOWN_MAX),
				75.0,
				"ramp-down",
			),
			(
				falling_slider(d.scroll_ease_out_ms, EASE_OUT_MIN, EASE_OUT_MAX),
				40.0,
				"ease-out",
			),
		] {
			assert_eq!(got, want, "{what} default should read where it claims");
		}
	}

	// Test ID: ElpQBio
	#[test]
	fn the_scrolling_feel_sliders_round_trip_and_run_the_right_way() {
		// A slider that reads back as something else is the "setting does
		// nothing" bug in its quietest form. Also pins the DIRECTION of every
		// feel slider: higher = faster, whichever way each is stored
		// underneath.
		let mut d = mk_dialog(4000.0);
		for (key, label) in [
			(Key::ScrollEaseIn, "Ease-in"),
			(Key::ScrollRampUp, "Ramp-up"),
			(Key::SingleScreenTau, "Single-screen speed"),
			(Key::ScrollRampDown, "Ramp-down"),
			(Key::ScrollEaseOut, "Ease-out"),
		] {
			for want in [1.0, 25.0, 50.0, 75.0, 100.0] {
				d.set_f32(key, want);
				let got = d.get_f32(key);
				assert!(
					(got - want).abs() < 1.5,
					"{label} set to {want} read back as {got}"
				);
			}
		}
		// higher = crisper on both ends of the ease (stored as a shorter duration)
		d.set_f32(Key::ScrollEaseIn, 80.0);
		let crisp_in = d.edited.scroll_ease_in_ms;
		d.set_f32(Key::ScrollEaseIn, 20.0);
		assert!(crisp_in < d.edited.scroll_ease_in_ms);
		d.set_f32(Key::ScrollEaseOut, 80.0);
		let crisp_out = d.edited.scroll_ease_out_ms;
		d.set_f32(Key::ScrollEaseOut, 20.0);
		assert!(
			crisp_out < d.edited.scroll_ease_out_ms,
			"a higher Ease-out must be a SHORTER tail, matching its Ease-in partner"
		);
		// higher = harder on both ramps (stored as a shorter period)
		d.set_f32(Key::ScrollRampUp, 80.0);
		let hard_up = d.edited.scroll_ramp_up_ms;
		d.set_f32(Key::ScrollRampUp, 20.0);
		assert!(hard_up < d.edited.scroll_ramp_up_ms);
		d.set_f32(Key::ScrollRampDown, 80.0);
		let hard_down = d.edited.scroll_ramp_down_ms;
		d.set_f32(Key::ScrollRampDown, 20.0);
		assert!(hard_down < d.edited.scroll_ramp_down_ms);
	}

	// Test ID: EiuyGZk
	#[test]
	fn buttons_fire_on_release_over_button() {
		use super::Action;
		let mut d = mk_dialog(2000.0);
		let (action, r, _) = d.buttons()[1]; // Apply
		assert_eq!(action, Action::Apply);
		let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
		let mut m = |_: &str| 10.0;
		// press arms the button (feedback) without firing
		assert_eq!(d.mouse_down(cx, cy, &mut m), Action::None);
		assert_eq!(d.pressed, Some(1));
		// release over the same button fires its action and disarms
		assert_eq!(d.mouse_up(cx, cy), Action::Apply);
		assert_eq!(d.pressed, None);
		// press then release away from the button cancels (no action)
		d.mouse_down(cx, cy, &mut m);
		assert_eq!(d.mouse_up(cx, r.y - 100.0), Action::None);
		assert_eq!(d.pressed, None);
	}

	// Test ID: EitmBBR
	#[test]
	fn space_or_enter_activates_focused_button() {
		use super::{Action, Focus};
		let mut d = mk_dialog(2000.0);
		d.focus = Some(Focus::Button(0)); // Cancel
		assert_eq!(d.key_space(), Action::Cancel);
		d.focus = Some(Focus::Button(2)); // OK
		assert_eq!(d.key_enter(), Action::Ok);
	}

	// Test ID: EitjFLE
	#[test]
	fn keyboard_skips_headers_and_disabled() {
		let mut d = mk_dialog(2000.0);
		d.tab = 1; // Background
		// with transparency + scrim off, the opacity/blur/scrim rows are disabled
		d.edited.transparent_background = false;
		d.edited.text_scrim = false;
		for &i in &d.focusables() {
			assert!(!matches!(d.specs[i].kind, super::Kind::Header(_)));
			assert!(!d.disabled(d.specs[i].key), "disabled row in tab order");
		}
	}

	// Test ID: EitjFLF
	#[test]
	fn space_toggles_focused_boolean() {
		let mut d = mk_dialog(2000.0);
		d.tab = 1;
		d.key_tab(); // first focusable = Wallpaper (a toggle)
		let before = d.get_toggle(super::Key::BgEnabled);
		d.key_space();
		assert_eq!(d.get_toggle(super::Key::BgEnabled), !before);
	}

	// Test ID: EitjFLG
	#[test]
	fn arrows_adjust_slider_and_radio() {
		use super::Key;
		let mut d = mk_dialog(2000.0);
		// slider: focus the scroll-speed slider, nudge it both ways
		d.tab = 5;
		d.key_tab();
		let base = d.get_f32(Key::SingleScreenTau);
		d.key_horizontal(-1);
		let lower = d.get_f32(Key::SingleScreenTau);
		assert!(lower <= base);
		d.key_horizontal(1);
		d.key_horizontal(1);
		assert!(d.get_f32(Key::SingleScreenTau) >= lower);
		// radio: focus the (always-enabled) bg-fit radio and move its selection
		let i = d.specs.iter().position(|s| s.key == Key::BgFit).unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(super::Focus::Row(i, 0));
		let before = d.get_radio(Key::BgFit);
		d.key_horizontal(1);
		assert!(d.get_radio(Key::BgFit) > before || before == 1);
		d.key_horizontal(-1);
		assert_eq!(d.get_radio(Key::BgFit), 0);
	}

	// Test ID: EkZgQnY
	#[test]
	fn slider_step_matches_spec() {
		use super::slider_step;
		// float: ~1/100 normally, ~1/10 with Shift
		assert!((slider_step(0.0, 1.0, false, false) - 0.01).abs() < 1e-6);
		assert!((slider_step(0.0, 1.0, false, true) - 0.1).abs() < 1e-6);
		// int: rounded to a whole unit, never below 1
		assert_eq!(slider_step(6.0, 40.0, true, false), 1.0); // 34/100 -> 0 -> 1
		assert_eq!(slider_step(20.0, 400.0, true, false), 4.0); // 380/100 -> 4
		assert_eq!(slider_step(20.0, 400.0, true, true), 38.0); // 380/10 -> 38
	}

	// The idle waits run from a minute to a day, and a straight track put the
	// first hour in its first 4%. A log track gives each doubling the same
	// travel, and an arrow press is the same ratio anywhere along it.
	// Test ID: Es9f0qI
	#[test]
	fn a_log_slider_gives_each_doubling_the_same_travel() {
		use super::{Key, SliderScale};
		let d = mk_dialog(4000.0);
		let scale_of = |key: Key| {
			let spec = d.specs.iter().find(|s| s.key == key).unwrap();
			SliderScale::of(&spec.kind).unwrap()
		};
		for key in [Key::IdleHiddenMin, Key::IdleMin] {
			let scale = scale_of(key);
			assert!(scale.log && scale.int, "{key:?}");
			assert_eq!(
				(scale.min, scale.max),
				(1.0, 1440.0),
				"{key:?} kept its ends"
			);
			assert_eq!(scale.frac(1.0), 0.0);
			assert_eq!(scale.frac(1440.0), 1.0);
			let hour = scale.frac(60.0);
			assert!(hour > 0.5 && hour < 0.6, "an hour sits at {hour}");
			let low = scale.frac(20.0) - scale.frac(10.0);
			let high = scale.frac(200.0) - scale.frac(100.0);
			assert!((low - high).abs() < 1e-4, "{low} against {high}");
			for value in [1.0, 2.0, 5.0, 30.0, 240.0, 1440.0] {
				assert_eq!(scale.at(scale.frac(value)), value, "{key:?}");
			}

			// every press moves, even at 1 where the ratio rounds to nothing,
			// and Up walks the whole track in about a hundred
			let mut value = 1.0;
			let mut presses = 0;
			while value < scale.max {
				let next = scale.stepped(value, 1, false);
				assert!(next > value, "{key:?} stuck at {value}");
				value = next;
				presses += 1;
				assert!(presses < 200, "{key:?} took too many presses");
			}
			assert_eq!(value, 1440.0);
			assert!(presses > 60, "{key:?} steps too coarse: {presses}");
			let shifted = scale.stepped(240.0, 1, true);
			assert!(
				shifted > 400.0 && shifted < 600.0,
				"Shift+Up from 240 gave {shifted}"
			);
			assert_eq!(scale.stepped(1.0, -1, false), 1.0);
			assert_eq!(
				scale.stepped(1440.0, 1, false),
				1440.0,
				"Up stops at the end"
			);

			// past the end, Up keeps the typed number and Down steps down from it
			assert_eq!(scale.stepped(5000.0, 1, false), 5000.0);
			let down = scale.stepped(5000.0, -1, false);
			assert!(down < 5000.0 && down > 4000.0, "Down from 5000 gave {down}");
			assert_eq!(scale.frac(5000.0), 1.0, "the handle sits at the end");
		}

		// a straight slider steps as it did, under the same past-the-end rule
		let columns = scale_of(Key::Columns);
		assert!(!columns.log);
		assert_eq!(columns.stepped(100.0, 1, false), 104.0);
		assert_eq!(columns.stepped(400.0, 1, false), 400.0);
		assert_eq!(columns.stepped(600.0, -1, false), 596.0);
		assert_eq!(columns.frac(600.0), 1.0);
	}

	// A number typed past the slider's end is kept, up to the row's cap. The
	// handle parks at the end, the box shows the number, and a press on the
	// parked handle does not throw it away. Below the slider still clamps.
	// Test ID: Es9f0uU
	#[test]
	fn a_typed_wait_can_go_past_the_slider_up_to_a_week() {
		use super::{Focus, Key, SliderScale};
		let mut d = mk_dialog(4000.0);
		d.edited.idle_release = true;
		let i = d.specs.iter().position(|s| s.key == Key::IdleMin).unwrap();
		let key = Key::IdleMin;
		let scale = SliderScale::of(&d.specs[i].kind).unwrap();
		d.tab = d.specs[i].tab;
		let type_in = |d: &mut SettingsDialog, text: &str| {
			d.focus = Some(Focus::Row(i, 0));
			d.key_space();
			d.select_all();
			d.insert_str(text);
			d.edit = None;
		};

		type_in(&mut d, "5000");
		assert_eq!(d.get_f32(key), 5000.0);
		assert_eq!(d.edited.idle_release_min, 5000);
		assert_eq!(d.fmt_val(key, true), "5000");
		assert_eq!(scale.frac(d.get_f32(key)), 1.0);

		// a press on the parked handle keeps it; one along the track moves it
		let track = d.track(i);
		let mut m = |s: &str| s.chars().count() as f32;
		let y = track.y + track.h / 2.0;
		d.mouse_down(track.x + track.w, y, &mut m);
		d.mouse_up(track.x + track.w, y);
		assert_eq!(d.get_f32(key), 5000.0, "a press at the end lost the number");
		d.last_click = None;
		d.mouse_down(track.x + track.w / 2.0, y, &mut m);
		d.mouse_up(track.x + track.w / 2.0, y);
		assert_eq!(d.get_f32(key), scale.at(0.5));
		assert!(d.get_f32(key) < 60.0, "halfway is {}", d.get_f32(key));

		type_in(&mut d, "20000");
		assert_eq!(d.get_f32(key), 10080.0, "past a week holds at a week");
		type_in(&mut d, "0");
		assert_eq!(d.get_f32(key), 1.0, "below the slider still clamps");

		// Down from a typed number steps from it rather than jumping to the end
		type_in(&mut d, "5000");
		d.edit = None;
		d.focus = Some(Focus::Row(i, 0));
		d.key_horizontal(-1);
		let down = d.get_f32(key);
		assert!(down > 1440.0 && down < 5000.0, "Down from 5000 gave {down}");

		// a row with a % keeps the old clamp
		let pct = d.specs.iter().position(|s| s.key == Key::Opacity).unwrap();
		d.tab = d.specs[pct].tab;
		d.focus = Some(Focus::Row(pct, 0));
		d.key_space();
		d.select_all();
		d.insert_str("250");
		assert_eq!(d.get_f32(Key::Opacity), 100.0);
	}

	// A number typed past a slider has to show whole in its box, so a decimal
	// drops places as its whole part grows. 3600 seconds showed "3600.00", cut off.
	// Test ID: Es9hNXm
	#[test]
	fn a_big_decimal_drops_places_to_fit_its_box() {
		use super::Key;
		let mut d = mk_dialog(4000.0);
		for (value, shown) in [
			(60.0, "60.00"),
			(0.25, "0.25"),
			(99.94, "99.94"),
			(150.5, "150.5"),
			(999.94, "999.9"),
			(999.96, "1000"),
			(3600.0, "3600"),
		] {
			d.set_f32(Key::CursorResume, value);
			assert_eq!(d.fmt_val(Key::CursorResume, false), shown, "{value}");
		}
	}

	// The number box was a fixed 56 DIP, so a big desktop font cut off "10080"
	// and "999.9". Measured in the real UI font at 1x and 2x, and at 2.5 times
	// its size.
	// Test ID: Es9oaEN
	#[test]
	fn every_slider_number_fits_its_box_at_a_large_interface_font() {
		use super::SliderScale;
		let texts: Vec<String> = super::ui()
			.specs
			.iter()
			.filter_map(|spec| SliderScale::of(&spec.kind))
			.flat_map(SliderScale::widest_texts)
			.collect();
		for want in ["10080", "999.9", "20.00", "4.00"] {
			assert!(texts.iter().any(|t| t == want), "{want} is never measured");
		}
		let attrs = crate::text::ui_attrs();
		// (font size, display scale)
		for (big, scale) in [(1.0, 1.0), (2.0, 2.0), (2.5, 1.0)] {
			let mut text = crate::text::TextCtx::new_cpu(big);
			let chrome = super::chrome_widths(&mut text, scale);
			let d = SettingsDialog::new(0.0, 0.0, text.ui_line_h, chrome, f32::MAX, 4000.0, scale);
			for (i, spec) in d.specs.iter().enumerate() {
				let Some(slider) = SliderScale::of(&spec.kind) else {
					continue;
				};
				let room = d.valbox(i).w - 2.0 * super::lay().field_pad;
				for number in slider.widest_texts() {
					let w = text.measure_ui_text(&number, &attrs) / scale;
					assert!(
						w <= room + 0.01,
						"{} at {big}x the font, {scale}x: {number} is {w} wide, room for {room}",
						spec.label
					);
				}
			}
		}
	}

	// The revert arrow's column was a fixed 22 DIP, so at a 24 pt font the arrow
	// ran out of it and under the scrollbar. The color picker's value boxes and
	// labels went by a guess at the font's width, which cut "#rrggbb" short at
	// every size. Measured in the real UI font at 1x and 2x, and at 2.2 times its
	// size.
	// Test ID: EsDinME
	#[test]
	fn the_revert_arrow_and_the_pickers_text_fit_at_a_large_interface_font() {
		let row = super::ui()
			.specs
			.iter()
			.position(|s| matches!(s.kind, Kind::Color))
			.expect("a color row");
		for (big, scale) in [(1.0, 1.0), (1.0, 2.0), (2.2, 1.0), (2.2, 2.0)] {
			let mut text = crate::text::TextCtx::new_cpu(big);
			// after the context, which is what picks the family
			let attrs = crate::text::ui_attrs();
			let chrome = super::chrome_widths(&mut text, scale);
			let mut d = SettingsDialog::new(
				0.0,
				0.0,
				text.ui_line_h,
				chrome,
				f32::MAX,
				4000.0 * scale,
				scale,
			);
			let (w, h) = d.natural;
			d.set_size(w * scale, h * scale);
			d.tab = d.specs[row].tab;
			let at = format!("{big}x the font, {scale}x");
			let mut tw = |s: &str| text.measure_ui_text(s, &attrs) / scale;

			// the arrow ends inside its column, which ends where the content does
			let arrow = d.revert_box(row);
			let glyph_end = arrow.x + super::REVERT_INSET + tw(super::ui().icons.revert);
			assert!(
				glyph_end <= arrow.x + arrow.w + 0.5,
				"{at}: the arrow ends at {glyph_end}, its column at {}",
				arrow.x + arrow.w
			);
			assert!(arrow.x + arrow.w <= d.content_x() + d.layout_w() - lay().pad + 0.01);

			d.pick_open(row);
			let g = d.pick_geom();
			let gap = super::font_gap(super::PART_LABEL_GAP, d.line_h);
			for field in crate::pick::Field::ALL {
				let end = g.labels_x + tw(field.label());
				assert!(
					end + gap <= g.fields[0].x + 1.0,
					"{at}: {:?} runs to {end}, its box starts at {}",
					field.label(),
					g.fields[0].x
				);
			}
			let room = g.fields[5].w - 2.0 * lay().field_pad;
			for value in ["#20202a", "#dddddd", "#888888", "#bbbbbb", "100"] {
				let w = tw(value);
				assert!(
					w <= room + 1.0,
					"{at}: {value} is {w} wide, room for {room}"
				);
			}
		}
	}

	// A box wider than the floor comes out of the panel, so the sliders keep their
	// length, and the floor holds when every number fits. Wide footer buttons make
	// the panel's own floor the widest thing, as a wide font's labels can.
	// Test ID: Es9oaYr
	#[test]
	fn a_wider_number_box_widens_the_panel_and_leaves_the_sliders() {
		let row = super::ui()
			.specs
			.iter()
			.position(|spec| matches!(spec.kind, Kind::Slider { .. }))
			.expect("a slider row");
		let floor = super::lay().value_width;
		for scale in [1.0, 2.0] {
			let at = |value_w: f32| {
				let mut d = SettingsDialog::new(
					0.0,
					0.0,
					18.0 * scale,
					Chrome {
						label_w: 170.0 * scale,
						btn_w: 300.0 * scale,
						row_btn_w: 90.0 * scale,
						value_w: value_w * scale,
						tab_ws: vec![40.0 * scale; tab_titles().len()],
						label_ws: labels7(scale),
						..Chrome::default()
					},
					f32::MAX,
					4000.0 * scale,
					scale,
				);
				let (w, h) = d.size();
				d.set_size(w, h);
				d
			};
			let (narrow, wide) = (at(floor - 20.0), at(floor + 30.0));
			assert!((narrow.valbox(row).w - floor).abs() < 0.01, "at {scale}x");
			assert!(
				(wide.valbox(row).w - (floor + 30.0)).abs() < 0.01,
				"at {scale}x"
			);
			assert!(
				(wide.natural.0 - narrow.natural.0 - 30.0).abs() < 0.01,
				"at {scale}x the panel grew {} for a box 30 wider",
				wide.natural.0 - narrow.natural.0
			);
			assert!(
				(wide.track(row).w - narrow.track(row).w).abs() < 0.01,
				"at {scale}x the slider went from {} to {}",
				narrow.track(row).w,
				wide.track(row).w
			);
		}
	}

	// Every number a box takes past its slider has to come back from the file
	// as typed, or the change shows and is gone at the next launch (G33). No row
	// with a % goes past its slider, and every log row starts above 0.
	// Test ID: Es9f0yE
	#[test]
	fn a_number_typed_past_a_slider_survives_a_save_and_a_relaunch() {
		use super::SliderScale;
		let _guard = config::test_config_lock();
		let _ = config::settings(); // memoize before the override goes in
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_typedmax_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let _ = std::fs::write(&path, "");
		config::set_config_override(path.clone());
		config::reload_from_disk();
		let pristine = std::fs::read_to_string(&path)
			.unwrap()
			.replace("# profile: \"max\"  ## Default", "profile: \"custom\"");
		assert!(pristine.contains("profile: \"custom\""));

		let mut d = mk_dialog(4000.0);
		let mut checked = Vec::new();
		for i in 0..d.specs.len() {
			let Some(scale) = SliderScale::of(&d.specs[i].kind) else {
				continue;
			};
			let label = d.specs[i].label;
			assert!(!scale.log || scale.min > 0.0, "{label}");
			if label.contains('%') {
				assert_eq!(scale.typed_max, scale.max, "{label} is a percent");
				continue;
			}
			if scale.typed_max <= scale.max {
				continue;
			}
			let key = d.specs[i].key;
			let _ = std::fs::write(&path, &pristine);
			let base = config::reload_from_disk();
			d.orig = base.clone();
			d.edited = base.clone();
			d.set_f32(key, scale.typed(f32::MAX));
			assert_eq!(d.get_f32(key), scale.typed_max, "{label}");
			assert!(config::persist(&base, &d.edited), "{label} was not written");
			let mut back = mk_dialog(4000.0);
			back.edited = config::reload_from_disk();
			assert_eq!(
				back.get_f32(key),
				scale.typed_max,
				"{label} came back from the file clamped"
			);
			checked.push(label);
		}
		assert_eq!(checked.len(), 11, "{checked:?}");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EkZgQnZ
	#[test]
	fn up_down_step_focused_slider() {
		use super::Key;
		let mut d = mk_dialog(2000.0);
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::SingleScreenTau)
			.unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(super::Focus::Row(i, 0));
		d.set_f32(Key::SingleScreenTau, 50.0);
		d.key_vertical(false); // Up -> increase by 1 (int step)
		assert_eq!(d.get_f32(Key::SingleScreenTau), 51.0);
		d.key_vertical(true); // Down -> decrease
		d.key_vertical(true);
		assert_eq!(d.get_f32(Key::SingleScreenTau), 49.0);
		d.set_mods(false, true, false); // Shift held
		d.key_vertical(false); // Shift+Up -> ~1/10 of the range (10)
		assert_eq!(d.get_f32(Key::SingleScreenTau), 59.0);
	}

	// Test ID: EkZgQna
	#[test]
	fn up_down_step_slider_during_edit() {
		use super::Key;
		let mut d = mk_dialog(2000.0);
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::SingleScreenTau)
			.unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(super::Focus::Row(i, 0));
		d.set_f32(Key::SingleScreenTau, 30.0);
		d.key_space(); // open the field, fully selected
		assert!(d.edit.is_some());
		d.key_vertical(false); // Up steps the value and refreshes the buffer
		assert_eq!(d.get_f32(Key::SingleScreenTau), 31.0);
		assert_eq!(d.edit.as_ref().unwrap().buf, "31");
		assert_eq!(d.selected_text().as_deref(), Some("31")); // stays fully selected
	}

	// Test ID: EkZgQnb
	#[test]
	fn fresh_click_selects_all_but_drag_keeps_range() {
		use super::Key;
		let i0 = mk_dialog(4000.0)
			.specs
			.iter()
			.position(|s| s.key == Key::BgImage)
			.unwrap();
		let mut m = |s: &str| s.chars().count() as f32; // 1px per char
		// fresh single click into a text field: select all on release
		let mut d = mk_dialog(4000.0);
		d.tab = d.specs[i0].tab;
		d.edited.wallpaper_raw = hand("foo bar.png");
		let field = d.textbox(i0);
		let at = |k: usize| field.x + lay().field_pad + k as f32;
		let y = field.y + field.h / 2.0;
		d.mouse_down(at(2), y, &mut m);
		assert!(d.edit.is_some(), "click opens the field");
		assert!(d.selected_text().is_none(), "not selected until release");
		d.mouse_up(at(2), y);
		assert_eq!(
			d.selected_text().as_deref(),
			Some("foo bar.png"),
			"a no-drag click selects all"
		);
		// a click that drags selects the dragged range instead
		let mut d = mk_dialog(4000.0);
		d.tab = d.specs[i0].tab;
		d.edited.wallpaper_raw = hand("foo bar.png");
		d.mouse_down(at(2), y, &mut m);
		d.mouse_move(at(6), y, &mut m);
		d.mouse_up(at(6), y);
		assert_eq!(
			d.selected_text().as_deref(),
			Some("o ba"),
			"a drag keeps its range"
		);
	}

	// All four tab chords, in both directions, and the plain keys they must not
	// steal: Tab alone walks controls, and PageUp/PageDown alone do nothing here.
	// Test ID: Em3Pif5
	#[test]
	fn ctrl_tab_and_ctrl_page_walk_the_tabs_both_ways() {
		let mut d = mk_dialog(2000.0);
		let last = tab_titles().len() - 1;
		d.set_mods(false, false, true); // Ctrl held
		d.key_tab();
		assert_eq!(d.tab, 1);
		assert!(d.focus.is_some(), "a tab switch lands focus on a control");
		d.set_mods(false, true, true); // Ctrl+Shift
		d.key_tab();
		assert_eq!(d.tab, 0);
		d.key_tab();
		assert_eq!(d.tab, last, "and wraps round the far end");

		d.set_mods(false, false, true);
		d.key_page(true);
		assert_eq!(d.tab, 0, "PageDown is forward, wrapping");
		d.key_page(false);
		assert_eq!(d.tab, last);

		// without Ctrl these are not tab keys at all
		d.tab = 1;
		d.set_mods(false, false, false);
		d.key_page(true);
		d.key_page(false);
		assert_eq!(d.tab, 1);
		d.key_tab();
		assert_eq!(d.tab, 1, "plain Tab walks controls, not tabs");
	}

	// Test ID: Eiustaa
	#[test]
	fn slider_numeric_field_edits_and_clamps() {
		use super::{Focus, Key};
		let mut d = mk_dialog(2000.0);
		// Font size: an int slider on the Font tab, range 6..40
		let i = d.specs.iter().position(|s| s.key == Key::FontSize).unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(Focus::Row(i, 0));
		// Space opens the field pre-filled with the current value
		d.key_space();
		assert!(d.edit.is_some());
		// clear it and type an exact number
		while d.edit.as_ref().is_some_and(|e| !e.buf.is_empty()) {
			d.backspace();
		}
		d.char_input('2');
		d.char_input('4');
		assert_eq!(config::auto::font_size(&d.edited), 24.0);
		// was: over-range types clamp to the slider max (40). Size takes a typed
		// number past its slider now, up to 128 (2026100812334386).
		while d.edit.as_ref().is_some_and(|e| !e.buf.is_empty()) {
			d.backspace();
		}
		d.char_input('9');
		d.char_input('9');
		// assert_eq!(d.edited.font_size, 40.0);
		assert_eq!(config::auto::font_size(&d.edited), 99.0);
		d.char_input('9');
		assert_eq!(config::auto::font_size(&d.edited), 128.0);
		// Enter commits and is the dialog's OK; the field closes on the clamped value
		assert_eq!(d.key_enter(), super::Action::Ok);
		assert!(d.edit.is_none());
	}

	// Test ID: Eiustab
	#[test]
	fn slider_field_typing_starts_fresh_and_rejects_letters() {
		use super::{Focus, Key};
		let mut d = mk_dialog(2000.0);
		// Line height: a slider that really is a decimal, so the dot is legal
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::LineHeight)
			.unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(Focus::Row(i, 0));
		// typing a digit into the focused (unopened) slider starts a fresh number
		d.char_input('1');
		d.char_input('.');
		d.char_input('2');
		assert_eq!(d.edited.line_height_scale, 1.2);
		// a second '.' and any letter are ignored (buffer stays "1.2")
		d.char_input('.');
		d.char_input('x');
		assert_eq!(d.edit.as_ref().unwrap().buf, "1.2");
		// a fraction shown as a whole percent takes no dot at all - it is an
		// integer field, and 0.5 typed into one would read as half a percent
		d.commit_edit();
		let j = d.specs.iter().position(|s| s.key == Key::Opacity).unwrap();
		d.tab = d.specs[j].tab;
		d.edited.transparent_background = true; // opacity enabled
		d.focus = Some(Focus::Row(j, 0));
		d.char_input('5');
		d.char_input('.');
		d.char_input('0');
		assert_eq!(d.edit.as_ref().unwrap().buf, "50");
		assert_eq!(d.edited.opacity, 0.5);
	}

	// "File or folder" says where the picture comes from. With Rotate folder on
	// that is the folder, shipped as the usual place; with it off, the image. A
	// named image wins at run time, so it shows whichever way the switch is set.
	// Test ID: Er1vmpQ
	#[test]
	fn the_wallpaper_box_follows_the_rotate_switch() {
		use super::Key;
		let token = crate::config::WALLPAPER_DIR_TOKEN;
		// "/pics" is rooted but not absolute on Windows, which puts it on a drive
		let pics = if cfg!(windows) { "C:/pics" } else { "/pics" };
		let mut d = mk_dialog(4000.0);
		// automatic, both: the folder's is the usual place
		d.edited.wallpaper_raw = hand("");
		d.edited.wallpaper = None;
		d.edited.wallpaper_folder_raw = hand("");
		d.edited.wallpaper_rotate_enabled = true;
		assert_eq!(d.get_text(Key::BgImage), token, "pre-filled");
		assert!(d.is_default(Key::BgImage));

		d.set_text(Key::BgImage, pics);
		assert_eq!(d.edited.wallpaper_folder_raw, hand(pics));
		assert_eq!(
			d.edited.wallpaper_folder,
			Some(std::path::PathBuf::from(pics))
		);
		assert!(!d.edited.wallpaper_folder_auto);
		assert!(
			d.edited.wallpaper_raw.is_automatic(),
			"the image is untouched"
		);
		assert!(!d.is_default(Key::BgImage));
		d.set_text(Key::BgImage, " ");
		assert!(
			d.edited.wallpaper_folder_raw.is_automatic(),
			"emptied is the usual place"
		);
		// and so is the usual place typed out, which is how the file reads it
		d.set_text(Key::BgImage, token);
		assert!(d.edited.wallpaper_folder_raw.is_automatic());

		// was: off, the box was empty. It is the image, which is automatic and
		// shows what automatic finds (2026100910295903).
		d.edited.wallpaper_rotate_enabled = false;
		assert_eq!(
			d.get_text(Key::BgImage),
			config::auto::rule(
				&d.edited,
				config::auto::Setting::WallpaperImage,
				config::auto::Place::default()
			)
			.to_string(),
			"off, it is the image"
		);
		d.set_text(Key::BgImage, "/a.png");
		assert_eq!(d.edited.wallpaper_raw, hand("/a.png"));
		assert!(
			d.edited.wallpaper_folder_raw.is_automatic(),
			"the folder is untouched"
		);
		d.edited.wallpaper_rotate_enabled = true;
		assert_eq!(
			d.get_text(Key::BgImage),
			"/a.png",
			"a named image shows either way"
		);

		d.edited.wallpaper_folder_raw = hand(pics);
		d.revert(Key::BgImage);
		assert!(d.edited.wallpaper_raw.is_automatic());
		assert!(d.edited.wallpaper_folder_raw.is_automatic());
		assert_eq!(d.get_text(Key::BgImage), token);
		assert!(d.is_default(Key::BgImage));

		// emptying a named image on the way to typing another one keeps typing
		// the image, though an empty box with Rotate on would be the folder
		let i = d.specs.iter().position(|s| s.key == Key::BgImage).unwrap();
		d.tab = d.specs[i].tab;
		d.edited.wallpaper_raw = hand("/a.png");
		d.focus = Some(super::Focus::Row(i, 0));
		d.set_mods(false, false, false);
		d.key_space();
		d.select_all();
		d.delete_selection();
		d.insert_str("/b.png");
		assert_eq!(d.edited.wallpaper_raw, hand("/b.png"));
		assert!(d.edited.wallpaper_folder_raw.is_automatic());
	}

	// open the Background image text field for editing, focused, with a value
	fn mk_text_edit(value: &str) -> (SettingsDialog, usize) {
		use super::{Focus, Key};
		let mut d = mk_dialog(4000.0);
		let i = d.specs.iter().position(|s| s.key == Key::BgImage).unwrap();
		d.tab = d.specs[i].tab;
		d.edited.wallpaper_raw = hand(value);
		d.focus = Some(Focus::Row(i, 0));
		d.set_mods(false, false, false);
		d.key_space(); // opens with the value fully selected
		(d, i)
	}

	// The same, with a value comfortably wider than the field it sits in - at the
	// 1px-per-char measure these tests use. Derived from the field rather than
	// hardcoded, or a wider panel (a new tab, a longer label) quietly makes the
	// scrolling case untestable instead of failing.
	fn mk_long_text_edit(fill: char) -> (SettingsDialog, usize, usize) {
		let (probe, i) = mk_text_edit("");
		let n = probe.textbox(i).w as usize + 100;
		let (d, i) = mk_text_edit(&fill.to_string().repeat(n));
		(d, i, n)
	}

	// Test ID: EkI1Txh
	#[test]
	fn open_selects_all_and_typing_replaces() {
		let (mut d, _) = mk_text_edit("old.png");
		assert_eq!(d.selected_text().as_deref(), Some("old.png"));
		d.char_input('n');
		assert_eq!(d.edit.as_ref().unwrap().buf, "n");
		assert_eq!(d.edited.wallpaper_raw, hand("n")); // live reparse
		// plain arrows collapse; shift+arrows extend a fresh selection
		d.char_input('e');
		d.char_input('w');
		d.set_mods(false, true, false);
		d.cursor_left();
		d.cursor_left();
		assert_eq!(d.selected_text().as_deref(), Some("ew"));
		// backspace removes the selection only
		d.set_mods(false, false, false);
		d.backspace();
		assert_eq!(d.edit.as_ref().unwrap().buf, "n");
	}

	// Test ID: EkI1Txi
	#[test]
	fn ctrl_word_nav_and_word_delete() {
		let (mut d, _) = mk_text_edit("foo bar.png");
		d.cursor_end(); // also collapses the open-time selection
		d.set_mods(false, false, true); // Ctrl
		d.cursor_left(); // to "png" start
		assert_eq!(d.edit.as_ref().unwrap().cur, 8);
		d.backspace(); // Ctrl+Backspace eats "bar." ... no - the word left of caret
		assert_eq!(d.edit.as_ref().unwrap().buf, "foo png");
		// Ctrl never types (shortcut chars must not reach the buffer)
		d.char_input('c');
		assert_eq!(d.edit.as_ref().unwrap().buf, "foo png");
		// Ctrl+Shift+Right extends by a word
		d.set_mods(false, true, true);
		d.cursor_right();
		assert_eq!(d.selected_text().as_deref(), Some("png"));
	}

	// On a Mac, Option held is no button accelerator, so Option plus a letter
	// types what the layout gives it, here "{" as on a German layout.
	// Test ID: ErbKdLR
	#[test]
	fn option_types_into_a_mac_text_box() {
		use winit::keyboard::ModifiersState as M;
		let (mut d, _) = mk_text_edit("a");
		d.cursor_end();
		d.set_keys(crate::input::edit_keys(M::ALT, true));
		assert!(!d.alt(), "the dialog would take it as Alt+letter");
		d.char_input('{');
		assert_eq!(d.edit.as_ref().unwrap().buf, "a{");
	}

	// A text box on a Mac moves by words with Option, goes to either end with
	// Command, and types nothing with Command or Control held. Command+Shift+[
	// and ] reach the tabs through the menu bar.
	// Test ID: ErbGPQa
	#[test]
	fn a_mac_text_box_takes_the_mac_keys() {
		use crate::input::edit_keys;
		use winit::keyboard::ModifiersState as M;
		let (mut d, _) = mk_text_edit("foo bar.png");
		d.cursor_end();
		let mac = |mods| edit_keys(mods, true);
		d.set_keys(mac(M::ALT));
		d.cursor_left();
		assert_eq!(d.edit.as_ref().unwrap().cur, 8, "Option+Left, a word");
		d.set_keys(mac(M::SUPER));
		d.cursor_left();
		assert_eq!(d.edit.as_ref().unwrap().cur, 0, "Command+Left, the start");
		d.set_keys(mac(M::SUPER | M::SHIFT));
		d.cursor_right();
		assert_eq!(d.selected_text().as_deref(), Some("foo bar.png"));
		d.set_keys(mac(M::empty()));
		d.cursor_right();
		assert_eq!(d.edit.as_ref().unwrap().cur, 11);
		d.cursor_left();
		d.cursor_left();
		for held in [M::SUPER, M::CONTROL] {
			d.set_keys(mac(held));
			d.char_input('c');
			assert_eq!(d.edit.as_ref().unwrap().buf, "foo bar.png", "{held:?}");
		}
		d.set_keys(mac(M::SUPER));
		d.backspace();
		assert_eq!(d.edit.as_ref().unwrap().buf, "ng", "Command+Backspace");
		assert_eq!(d.edit.as_ref().unwrap().cur, 0);
		d.set_keys(mac(M::CONTROL));
		d.cursor_right();
		assert_eq!(d.edit.as_ref().unwrap().cur, 1, "Control+Right, one step");

		let mut d = mk_dialog(2000.0);
		d.switch_tab(true);
		assert_eq!(d.tab, 1);
		d.switch_tab(false);
		d.switch_tab(false);
		assert_eq!(d.tab, tab_titles().len() - 1);
		// Ctrl+Tab walks focus there, since Command+Tab is the app switcher
		d.tab = 1;
		d.set_keys(mac(M::CONTROL));
		d.key_tab();
		d.key_page(true);
		assert_eq!(d.tab, 1);
	}

	// Test ID: EkI1Txj
	#[test]
	fn select_all_cut_paste_roundtrip() {
		let (mut d, _) = mk_text_edit("keep me");
		d.cursor_end();
		d.select_all();
		assert_eq!(d.selected_text().as_deref(), Some("keep me"));
		d.delete_selection(); // the "cut" half (clipboard handled a level up)
		assert_eq!(d.edit.as_ref().unwrap().buf, "");
		assert_eq!(d.edited.wallpaper_raw, hand(""));
		d.insert_str("pasted.png");
		assert_eq!(d.edited.wallpaper_raw, hand("pasted.png"));
		// pasting over a selection replaces it
		d.select_all();
		d.insert_str("x");
		assert_eq!(d.edit.as_ref().unwrap().buf, "x");
	}

	// Test ID: EkI1Txk
	#[test]
	fn paste_respects_field_validation() {
		use super::{Focus, Key, Kind};
		// color field: hex chars pass, junk drops, '#' only up front
		let mut d = mk_dialog(4000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, Kind::Color))
			.unwrap();
		d.tab = d.specs[i].tab;
		// part 1 is the hex box; part 0 is the chip - was Row(i, 0)
		d.focus = Some(Focus::Row(i, 1));
		d.key_space();
		d.select_all();
		d.insert_str("#a0b1c2");
		assert_eq!(d.edit.as_ref().unwrap().buf, "#a0b1c2");
		d.select_all();
		d.insert_str("zz#12 34-56");
		assert_eq!(d.edit.as_ref().unwrap().buf, "#123456");
		// slider field: digits/dot only, single dot
		let mut d = mk_dialog(4000.0);
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::LineHeight)
			.unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(Focus::Row(i, 0));
		d.key_space();
		d.select_all();
		d.insert_str("1.2.5x");
		assert_eq!(d.edit.as_ref().unwrap().buf, "1.25");
	}

	// Test ID: EkI1Txl
	#[test]
	fn mouse_click_drag_and_multiclick_select() {
		let (mut d, i) = mk_text_edit("foo bar.png");
		let field = d.textbox(i);
		let mut m = |s: &str| s.chars().count() as f32; // 1px per char
		let at = |k: usize| field.x + lay().field_pad + k as f32;
		let y = field.y + field.h / 2.0;
		// single click: caret there, no selection
		d.mouse_down(at(2), y, &mut m);
		assert_eq!(d.edit.as_ref().unwrap().cur, 2);
		assert!(d.selected_text().is_none());
		// drag to char 6 selects "o ba"
		d.mouse_move(at(6), y, &mut m);
		d.mouse_up(at(6), y);
		assert_eq!(d.selected_text().as_deref(), Some("o ba"));
		// double-click on "bar" selects the word (streak reset: the 1-unit-per-
		// char test metric puts every click inside the multi-click radius)
		d.last_click = None;
		d.mouse_down(at(5), y, &mut m);
		d.mouse_up(at(5), y);
		d.mouse_down(at(5), y, &mut m);
		assert_eq!(d.selected_text().as_deref(), Some("bar"));
		d.mouse_up(at(5), y);
		// third click in place: the whole value
		d.mouse_down(at(5), y, &mut m);
		assert_eq!(d.selected_text().as_deref(), Some("foo bar.png"));
		d.mouse_up(at(5), y);
		// shift+click extends from a plain caret
		d.last_click = None;
		d.mouse_down(at(0), y, &mut m);
		d.mouse_up(at(0), y);
		d.last_click = None;
		d.set_mods(false, true, false);
		d.mouse_down(at(3), y, &mut m);
		assert_eq!(d.selected_text().as_deref(), Some("foo"));
	}

	// Test ID: EiMPv8K
	#[test]
	fn scroll_speed_inverts_tau() {
		// endpoints: slowest tau = slowest speed, fastest tau = fastest speed
		assert_eq!(tau_to_speed(TAU_MAX), 1.0);
		assert_eq!(tau_to_speed(TAU_MIN), 100.0);
		// higher speed -> lower tau (faster)
		assert!(speed_to_tau(100.0) < speed_to_tau(1.0));
		// round-trips within slider rounding (log scale: error is proportional)
		for tau in [10.0f32, 75.0, 150.0, 300.0, 1000.0] {
			let rt = speed_to_tau(tau_to_speed(tau));
			assert!((rt - tau).abs() <= tau * 0.03, "tau {tau} -> {rt}");
		}
	}

	// settle the field-edit animation (view/caret eases converge)
	fn settle(d: &mut SettingsDialog, m: &mut impl FnMut(&str) -> f32) {
		for _ in 0..200 {
			d.animate(0.016, m);
		}
	}

	// Test ID: EkIvixE
	#[test]
	fn long_value_scrolls_to_keep_caret_visible() {
		use super::lay;
		let (mut d, i, n) = mk_long_text_edit('x');
		let mut m = |s: &str| s.chars().count() as f32; // 1px per char
		d.cursor_end(); // collapse the open-time selection, caret at the last char
		settle(&mut d, &mut m);
		let field = d.textbox(i);
		let inner = field.w - 2.0 * lay().field_pad;
		let e = d.edit.as_ref().unwrap();
		// scrolled right, caret in view, with the end padding visible after it
		assert!(e.view_to > 0.0);
		assert!((n as f32 - e.view) <= inner - lay().caret_pad + 0.5);
		assert_eq!(e.view, e.view_to, "ease settles exactly on the target");
		// moving left keeps the lookahead margin of context before the caret
		for _ in 0..200 {
			d.cursor_left();
		}
		settle(&mut d, &mut m);
		let e = d.edit.as_ref().unwrap();
		assert!(
			(n - 200) as f32 - e.view_to >= 27.0,
			"margin ahead of leftward travel"
		);
		// Home scrolls all the way back
		d.cursor_home();
		settle(&mut d, &mut m);
		assert_eq!(d.edit.as_ref().unwrap().view_to, 0.0);
	}

	// Test ID: EkIvixF
	#[test]
	fn short_value_never_scrolls() {
		let (mut d, _) = mk_text_edit("short.png");
		let mut m = |s: &str| s.chars().count() as f32;
		d.cursor_end();
		settle(&mut d, &mut m);
		assert_eq!(d.edit.as_ref().unwrap().view_to, 0.0);
	}

	// Test ID: EkIvixG
	#[test]
	fn click_and_drag_map_through_the_view() {
		use super::lay;
		let (mut d, i, n) = mk_long_text_edit('y');
		let mut m = |s: &str| s.chars().count() as f32;
		d.cursor_end();
		settle(&mut d, &mut m);
		let view = d.edit.as_ref().unwrap().view;
		assert!(view > 0.0);
		let field = d.textbox(i);
		let y = field.y + field.h / 2.0;
		// a click 10px into the box hits the char 10px past the scrolled-off part
		d.last_click = None;
		d.mouse_down(field.x + lay().field_pad + 10.0, y, &mut m);
		let cur = d.edit.as_ref().unwrap().cur;
		assert!(
			(cur as f32 - (view + 10.0)).abs() <= 0.5,
			"cur {cur} vs view {view}"
		);
		d.mouse_up(field.x + lay().field_pad + 10.0, y);
		// from the far left, dragging past the right edge keeps selecting while
		// the view crawls (edge autoscroll)
		d.cursor_home();
		settle(&mut d, &mut m);
		d.last_click = None;
		d.mouse_down(field.x + lay().field_pad, y, &mut m);
		d.mouse_move(field.x + field.w + 40.0, y, &mut m);
		let cur0 = d.edit.as_ref().unwrap().cur;
		assert!(cur0 < n, "the first drag event lands short of the end");
		settle(&mut d, &mut m);
		d.mouse_up(field.x + field.w + 40.0, y);
		let e = d.edit.as_ref().unwrap();
		assert!(e.cur > cur0, "edge autoscroll extends the selection");
		assert!(e.view > 0.0, "view followed the drag");
		assert!(d.selected_text().is_some());
	}

	// Test ID: EkIvixH
	#[test]
	fn context_menu_open_fire_and_gating() {
		use super::{Action, EditCmd, lay};
		let (mut d, i) = mk_text_edit("hello world");
		let mut m = |s: &str| s.chars().count() as f32;
		let field = d.textbox(i);
		let y = field.y + field.h / 2.0;
		// right-click inside the (select-all) selection keeps it; menu opens
		d.mouse_right(field.x + lay().field_pad + 3.0, y, true, &mut m);
		assert!(d.emenu.is_some());
		assert_eq!(d.selected_text().as_deref(), Some("hello world"));
		// Copy is enabled; clicking it returns the command for the clipboard glue
		assert!(d.em_enabled(1));
		let r = d.em_item_rect(1);
		d.last_click = None;
		let act = d.mouse_down(r.x + 2.0, r.y + 2.0, &mut m);
		assert_eq!(act, Action::Edit(EditCmd::Copy));
		assert!(d.emenu.is_none());
		// no selection + empty clipboard: only Select all stays enabled
		d.cursor_end();
		d.mouse_right(field.x + lay().field_pad + 3.0, y, false, &mut m);
		assert!(
			d.selected_text().is_none(),
			"right-click outside sel places caret"
		);
		assert!(!d.em_enabled(0) && !d.em_enabled(1) && !d.em_enabled(2) && !d.em_enabled(3));
		assert!(d.em_enabled(4));
		// keyboard: walk to Select all, Enter fires it
		for _ in 0..5 {
			d.key_vertical(true);
		}
		assert_eq!(d.key_enter(), Action::Edit(EditCmd::SelectAll));
		assert!(d.emenu.is_none());
		// Esc closes the menu but keeps the edit alive
		d.mouse_right(field.x + lay().field_pad + 3.0, y, true, &mut m);
		assert!(d.emenu.is_some());
		assert_eq!(d.key_escape(), Action::None);
		assert!(d.emenu.is_none() && d.edit.is_some());
		// typing dismisses a stale menu
		d.mouse_right(field.x + lay().field_pad + 3.0, y, true, &mut m);
		d.char_input('a');
		assert!(d.emenu.is_none());
	}

	// Test ID: EkIvixI
	#[test]
	fn blink_stays_solid_on_activity() {
		let (mut d, _) = mk_text_edit("abc");
		let mut m = |s: &str| s.chars().count() as f32;
		settle(&mut d, &mut m); // ~3.2s idle: blink well past the hold
		assert!(d.edit.as_ref().unwrap().blink_t > 1.0);
		d.char_input('z');
		d.animate(0.016, &mut m);
		let e = d.edit.as_ref().unwrap();
		assert!(e.blink_t < 0.1);
		assert_eq!(e.caret_alpha(), 1.0);
	}

	// Put the dialog on a known theme with no color overrides on top.
	pub(super) fn on_theme(name: &str) -> SettingsDialog {
		let mut d = mk_dialog(4000.0);
		d.edited = config::Settings::default();
		d.edited.theme = name.to_string();
		d.adopt_theme();
		d.orig = d.edited.clone();
		d.reverted.clear(); // adopting queues them; start each test from nothing pending
		d
	}

	// Commented out by 2026100907341818: the Use system font pair row is gone,
	// and with it the one pair whose halves the desktop grayed apart.
	//
	// // A pair row's key is only its FIRST part, so gating the whole row on that
	// // key took the click away from a live second part. Windows always reports a
	// // size and no monospace family, so there it was every user; here the report
	// // has to be said out loud, or a desktop that does name a font never reaches
	// // the case.
	// // Test ID: Eq4Gng9
	// #[test]
	// fn a_live_half_of_a_pair_row_takes_a_click_while_the_other_half_is_grayed() {
	// 	use super::Key;
	// 	let mut m = |s: &str| s.chars().count() as f32;
	// 	let mut d = mk_dialog(4000.0);
	// 	let i = d
	// 		.specs
	// 		.iter()
	// 		.position(|s| matches!(s.key, Key::SystemFont))
	// 		.unwrap();
	// 	d.tab = d.specs[i].tab;
	// 	// the desktop names a size to follow but no family: Face grays, Size does not
	// 	d.os_font = crate::sysfont::Monospace {
	// 		family: None,
	// 		size_pt: Some(9.0),
	// 	};
	// 	assert!(d.disabled(Key::SystemFont), "Face should be grayed");
	// 	assert!(!d.disabled(Key::SystemFontSize), "Size should be live");
	//
	// 	let was = d.get_toggle(Key::SystemFontSize);
	// 	let size_box = d.dual_box(i, 1);
	// 	d.mouse_down_dip(
	// 		size_box.x + size_box.w / 2.0,
	// 		size_box.y + size_box.h / 2.0,
	// 		&mut m,
	// 	);
	// 	assert_eq!(
	// 		d.get_toggle(Key::SystemFontSize),
	// 		!was,
	// 		"Size took no click"
	// 	);
	//
	// 	// and the grayed half still takes none
	// 	let face = d.get_toggle(Key::SystemFont);
	// 	let face_box = d.dual_box(i, 0);
	// 	d.mouse_down_dip(
	// 		face_box.x + face_box.w / 2.0,
	// 		face_box.y + face_box.h / 2.0,
	// 		&mut m,
	// 	);
	// 	assert_eq!(d.get_toggle(Key::SystemFont), face, "a grayed half acted");
	// }

	// Settings opens on the file, so a change another window saved after this
	// one loaded is what the dialog shows, and a save from here writes only what
	// was edited.
	// Test ID: EqdzLhw
	#[test]
	fn settings_opens_on_the_file_as_it_is_now() {
		let _guard = config::test_config_lock();
		let _ = config::settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_reopen_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let _ = std::fs::write(&path, "");
		config::set_config_override(path.clone());
		let loaded = config::reload_from_disk();
		let mut other = loaded.clone();
		other.margin = loaded.margin + 5.0;
		assert!(config::persist(&loaded, &other));

		let mut d = mk_dialog(4000.0);
		d.start_from(config::reload_from_disk());
		assert_eq!(d.orig.margin, other.margin, "the other window's save");
		assert_eq!(d.edited.margin, other.margin);
		let columns = config::auto::grid(&loaded, None).0 + 7;
		d.edited.columns = config::auto::Auto::by_hand(columns);
		assert!(config::persist(&d.orig, &d.edited));
		let back = config::reload_from_disk();
		assert_eq!(back.margin, other.margin);
		assert_eq!(config::auto::grid(&back, None).0, columns);

		let _ = std::fs::remove_dir_all(&dir);
	}

	// The outline is drawn by the scrim pass but is not the halo: it takes input
	// with the scrim off, and the cursor may join it either way.
	// Test ID: Ep17Tr0
	#[test]
	fn the_outline_stands_without_the_scrim() {
		let mut d = mk_dialog(4000.0);
		d.edited.text_scrim = false;
		d.edited.text_outline = 2.0;
		assert!(!d.disabled(Key::Outline));
		// was: the halo's own rows grayed with the scrim off. They only count
		// while it is on, so they stay live (2026100907341818).
		assert!(!d.disabled(Key::ScrimRadius));
		assert!(!d.disabled(Key::ScrimSoftness));
		d.edited.text_scrim = true;
		assert!(!d.disabled(Key::ScrimSoftness));
		d.edited.text_scrim = false;
		let i = d
			.specs
			.iter()
			.position(
				|s| matches!(s.kind, super::Kind::Dual { keys, .. } if keys[0] == Key::CursorScrim),
			)
			.unwrap();
		assert!(!d.part_disabled(i, 0) && !d.part_disabled(i, 1));
		// and it sits on its own, unindented, after the scrim's members
		let outline = d.specs.iter().position(|s| s.key == Key::Outline).unwrap();
		let contrast = d
			.specs
			.iter()
			.position(|s| s.key == Key::MinContrast)
			.unwrap();
		assert_eq!(d.specs[outline].indent, 0);
		assert_eq!(d.specs[contrast].indent, 1);
		assert_eq!(
			contrast + 1,
			outline,
			"contrast closes the scrim group, the outline follows"
		);
	}

	// Remote is a profile the file never holds: picking it raises the session
	// override over the stored profile, and any other pick lowers it again.
	// Test ID: Ep17Tr1
	#[test]
	fn picking_remote_never_reaches_the_stored_profile() {
		let mut d = mk_dialog(4000.0);
		d.edited.performance_profile = crate::profile::Profile::High;
		d.set_radio(Key::PerfProfile, super::Profile::Remote.index());
		assert!(d.edited.remote_override);
		assert_eq!(d.edited.performance_profile, crate::profile::Profile::High);
		assert_eq!(
			d.get_radio(Key::PerfProfile),
			super::Profile::Remote.index()
		);
		// governing used to mean graying, so this read `d.disabled`:
		//     assert!(d.disabled(Key::SmoothScroll), "Remote governs like Standard");
		assert!(
			d.profile_shows(Key::SmoothScroll),
			"Remote governs like Standard"
		);
		d.set_radio(Key::PerfProfile, super::Profile::Low.index());
		assert!(!d.edited.remote_override);
		assert_eq!(d.edited.performance_profile, crate::profile::Profile::Low);
		// the revert arrow drops the override too
		d.set_radio(Key::PerfProfile, super::Profile::Remote.index());
		let row = d
			.specs
			.iter()
			.position(|s| s.key == Key::PerfProfile)
			.unwrap();
		assert!(!d.row_is_default(row));
		d.row_revert(row);
		assert!(!d.edited.remote_override);
		assert!(d.row_is_default(row));
	}

	// A step the display watch took shows in the dropdown and reads as a change,
	// so the arrow offers the way back. A hand pick lifts it, and picking the
	// profile the file already holds writes only the automatic switch.
	// Test ID: EpWow4j
	#[test]
	fn a_session_step_shows_and_a_hand_pick_lifts_it() {
		let _guard = config::test_config_lock();
		let _ = config::settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_uistep_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(
			&path,
			"performance:\n\tautomatic: true\n\tprofile: \"max\"\n",
		)
		.unwrap();
		config::set_config_override(path.clone());
		let mut stored = config::reload_from_disk();
		stored.stepped_profile = Some(super::Profile::Low);
		let before = std::fs::read_to_string(&path).unwrap();

		let mut d = mk_dialog(4000.0);
		d.orig = stored.clone();
		d.edited = stored;
		let row = d
			.specs
			.iter()
			.position(|s| s.key == Key::PerfProfile)
			.unwrap();
		assert_eq!(d.get_radio(Key::PerfProfile), super::Profile::Low.index());
		assert!(!d.row_is_default(row), "the arrow offers the way back");

		d.set_radio(Key::PerfProfile, super::Profile::Max.index());
		assert!(d.edited.stepped_profile.is_none());
		assert_eq!(d.get_radio(Key::PerfProfile), super::Profile::Max.index());
		assert!(d.row_is_default(row));
		assert!(config::persist(&d.orig, &d.edited));
		// The file already held Max, so its profile line is left alone. Until
		// naming one switched the automatic choice off, nothing was written:
		//     assert_eq!(read_to_string(&path).unwrap(), before, "...");
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			before.replace("automatic: true", "automatic: false"),
			"only the automatic switch should have moved"
		);

		d.edited.stepped_profile = Some(super::Profile::Low);
		d.row_revert(row);
		assert!(d.edited.stepped_profile.is_none(), "a revert lifts it");

		// the pick above already switched it off, so put it back to have
		// something for the toggle to change
		d.edited.performance_automatic = true;
		d.edited.stepped_profile = Some(super::Profile::Low);
		d.set_toggle(Key::PerfAuto, false);
		assert!(
			d.edited.stepped_profile.is_none(),
			"and so does switching automatic off"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The handle overhangs the track at either end, so a ring drawn around the
	// track alone crossed it there.
	// Test ID: Ep17Tr2
	#[test]
	fn the_slider_focus_ring_clears_the_handle_at_both_ends() {
		let d = mk_dialog(4000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, super::Kind::Slider { .. }))
			.unwrap();
		let track = d.track(i);
		// the tight box; the drawn ring sits 2 px outside it
		let tight = d.focus_ctl_rect(i, 0);
		let half = super::SLIDER_HANDLE_W / 2.0;
		assert!(tight.x <= track.x - half, "left end");
		assert!(tight.x + tight.w >= track.x + track.w + half, "right end");
		assert!(
			tight.x + tight.w + 2.0 < d.valbox(i).x,
			"and clear of the value field"
		);
	}

	// A click into a field selects the whole value on release, so the next
	// keystroke replaces it. The awkward case is a value wider than its box: the
	// view scrolls under the caret, and the frames between press and release used
	// to drag the selection away with it.
	// press, hold for a few frames, release - with the pointer put there first,
	// the way the window delivers it
	fn click(d: &mut SettingsDialog, x: f32, y: f32, m: &mut impl FnMut(&str) -> f32) {
		d.last_click = None;
		d.mouse_move(x, y, m);
		d.mouse_down(x, y, m);
		for _ in 0..8 {
			d.animate(0.016, m);
		}
		d.mouse_up(x, y);
	}

	// Test ID: EqAiJBo
	#[test]
	fn a_click_into_a_field_selects_all_on_release() {
		use super::lay;
		let (mut d, i) = mk_text_edit("old.png");
		let mut m = |s: &str| s.chars().count() as f32;
		// short value: anywhere in the box
		d.edit = None;
		let f = d.textbox(i);
		click(&mut d, f.x + f.w / 2.0, f.y + f.h / 2.0, &mut m);
		assert_eq!(d.selected_text().as_deref(), Some("old.png"));

		// long value, clicked near the right edge - where the view has to scroll
		let (mut d, i, _) = mk_long_text_edit('y');
		let want = d.edit.as_ref().unwrap().buf.clone();
		d.edit = None;
		// The x that puts an automatic setting back sits at the box's right end
		// since File or folder became one (2026100910295903), so the click
		// lands just short of it.
		let f = d.text_room(i, d.textbox(i));
		click(
			&mut d,
			f.x + f.w - lay().field_pad - 2.0,
			f.y + f.h / 2.0,
			&mut m,
		);
		assert_eq!(d.selected_text().as_deref(), Some(want.as_str()));
	}

	// Walking onto a text field opens it with the value selected, so typing
	// replaces it. A slider is deliberately not in that set.
	// Test ID: EqAiJBp
	#[test]
	fn keyboard_focus_opens_a_text_field_with_the_value_selected() {
		use super::{Focus, Key};
		let mut d = mk_dialog(2000.0);
		let i = d.specs.iter().position(|s| s.key == Key::BgImage).unwrap();
		d.tab = d.specs[i].tab;
		d.edited.wallpaper_raw = hand("old.png");
		d.focus = None;
		for _ in 0..200 {
			d.key_tab();
			if d.focus == Some(Focus::Row(i, 0)) {
				break;
			}
		}
		assert_eq!(d.focus, Some(Focus::Row(i, 0)), "never reached the row");
		assert_eq!(d.selected_text().as_deref(), Some("old.png"));
		d.char_input('n');
		assert_eq!(d.edited.wallpaper_raw, hand("n"));
		// and walking off closes it again
		d.key_tab();
		assert!(d.edit.as_ref().is_none_or(|e| e.row != i));

		// a slider's number box opens the same way, and Up/Down still step the
		// value through it
		let mut d = mk_dialog(2000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, super::Kind::Slider { .. }) && !d.disabled(s.key))
			.unwrap();
		let key = d.specs[i].key;
		let super::Kind::Slider { min, .. } = d.specs[i].kind else {
			unreachable!()
		};
		d.set_f32(key, min); // so there is room to step up
		d.tab = d.specs[i].tab;
		d.focus = None;
		for _ in 0..200 {
			d.key_tab();
			if d.focus == Some(Focus::Row(i, 0)) {
				break;
			}
		}
		assert_eq!(d.focus, Some(Focus::Row(i, 0)), "never reached the slider");
		let shown = d.selected_text().expect("number box opens selected");
		let before = d.get_f32(key);
		d.key_vertical(false); // Up
		assert!(d.get_f32(key) > before, "Up did not step the value");
		assert_ne!(
			d.selected_text().as_deref(),
			Some(shown.as_str()),
			"the box still shows the old number"
		);

		// a hex field the same: it shows its own color, selected, rather than
		// clearing itself the way a plain text box would. Part 1, since part 0 is
		// the chip that opens the picker and opens no field.
		// was: Focus::Row(i, 0), when a Color row had one part
		// ColBg rather than ColFg: the live config the test process reads can
		// have `colors.from_wallpaper` on, which grays ColFg out of the ring
		let mut d = mk_dialog(2000.0);
		let i = d.specs.iter().position(|s| s.key == Key::ColBg).unwrap();
		let want = {
			let c = d.get_col(Key::ColBg);
			format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
		};
		d.tab = d.specs[i].tab;
		d.focus = None;
		for _ in 0..200 {
			d.key_tab();
			if d.focus == Some(Focus::Row(i, 1)) {
				break;
			}
		}
		assert_eq!(
			d.focus,
			Some(Focus::Row(i, 1)),
			"never reached the hex field"
		);
		assert_eq!(d.selected_text().as_deref(), Some(want.as_str()));
		// and the chip on the way past opened no field of its own
		assert!(
			d.pick.is_none(),
			"walking onto the chip must not open the picker"
		);
	}

	// Esc from inside a field is the dialog's Cancel, not "shut the field".
	// Test ID: EqAiJBq
	#[test]
	fn escape_from_inside_a_field_cancels_the_dialog() {
		use super::Action;
		let (mut d, _) = mk_text_edit("old.png");
		assert!(d.edit.is_some());
		assert_eq!(d.key_escape(), Action::Cancel);
	}

	// Enter in a field is OK, not "close the field and wait for another Enter".
	// Test ID: EqAiJBr
	#[test]
	fn enter_in_a_field_is_the_dialogs_ok() {
		use super::{Action, Key};
		let (mut d, _) = mk_text_edit("old.png");
		d.char_input('n');
		assert_eq!(d.key_enter(), Action::Ok);
		assert!(d.edit.is_none());
		assert_eq!(d.edited.wallpaper_raw, hand("n"));

		// same from a hex field, and from a shells-grid field
		let mut d = mk_dialog(2000.0);
		let i = d.specs.iter().position(|s| s.key == Key::ColFg).unwrap();
		d.tab = d.specs[i].tab;
		d.open_edit(i, true);
		assert_eq!(d.key_enter(), Action::Ok);

		let mut d = mk_dialog(2000.0);
		let i = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, super::Kind::ShellList))
			.unwrap();
		d.tab = d.specs[i].tab;
		d.open_edit(super::shell_field_row(0, false), true);
		assert_eq!(d.key_enter(), Action::Ok);
	}

	// While the wallpaper is picking the text colors, the two rows for them gray
	// out and still read the USER's own values rather than the derived pair. The
	// live copy wears the derived one, and a dialog that took it as the baseline
	// would write it to the file and store it in the next saved theme.
	// Test ID: EqRxesm
	#[test]
	fn the_wallpaper_switch_grays_its_two_rows_and_leaves_their_values_the_users() {
		let mine = ([0x12u8, 0x34, 0x56], [0x65u8, 0x43, 0x21]);
		let _store = config::test_store_lock();
		let saved = config::settings();

		let mut live = (*saved).clone();
		crate::profile::unapply(&mut live);
		crate::autotheme::unapply(&mut live);
		// Custom, so nothing about the wallpaper is governed out from under this
		live.performance_profile = crate::profile::Profile::Custom;
		live.performance_automatic = false;
		live.wallpaper_enabled = true;
		live.colors_from_wallpaper = true;
		live.fg = mine.0;
		live.cursor = mine.1;
		live.wallpaper_summary = Some(crate::autotheme::Summary {
			luma_hi: 0.3,
			luma_lo: 0.02,
			luma_mean: 0.13,
			spread: [[0.13; 3]; crate::autotheme::SPREAD],
			alpha: 1.0,
			hue: 250.0,
			chroma: 0.08,
			opacity: 0.35,
		});
		config::update(live);
		assert_ne!(
			config::settings().fg,
			mine.0,
			"the derived text color should be live"
		);

		let d = mk_dialog(4000.0);
		assert_eq!(
			d.edited.fg, mine.0,
			"the row shows the user's own text color"
		);
		assert_eq!(d.edited.cursor, mine.1, "and their own cursor");
		assert!(d.disabled(Key::ColFg), "Foreground should gray out");
		assert!(d.disabled(Key::ColCursor), "Cursor should gray out");
		assert!(
			!d.disabled(Key::ColBg),
			"the background is not one of the two"
		);
		assert!(
			!d.disabled(Key::ColFromWallpaper),
			"the switch itself stays live"
		);

		config::update((*saved).clone());
	}

	// Switching it off in the dialog ungrays them in the same pass, since the
	// gates read the edited copy rather than the live one.
	// Test ID: EqRxesn
	#[test]
	fn switching_it_off_ungrays_the_two_rows_at_once() {
		let mut d = mk_dialog(4000.0);
		d.edited.wallpaper_enabled = true;
		d.set_toggle(Key::ColFromWallpaper, true);
		assert!(d.disabled(Key::ColFg));
		d.set_toggle(Key::ColFromWallpaper, false);
		assert!(!d.disabled(Key::ColFg));
		assert!(!d.disabled(Key::ColCursor));
	}

	// A dialog at a given UI line height, the other chrome measured to match.
	fn mk_dialog_line(line_h: f32) -> SettingsDialog {
		let k = line_h / 19.0;
		let mut d = SettingsDialog::new(
			0.0,
			0.0,
			line_h,
			Chrome {
				label_w: 170.0 * k,
				btn_w: 80.0 * k,
				row_btn_w: 90.0 * k,
				value_w: 0.0,
				tab_ws: vec![90.0 * k; tab_titles().len()],
				label_ws: labels7(k),
				..Chrome::default()
			},
			f32::MAX,
			4000.0,
			1.0,
		);
		d.orig.performance_profile = crate::profile::Profile::Custom;
		d.edited.performance_profile = crate::profile::Profile::Custom;
		d
	}

	// The colors of the quads that are exactly one of the four edges `border`
	// draws round `r`, `t` thick.
	fn edge_colors(quads: &[crate::gfx::RectInstance], r: super::Rect, t: f32) -> Vec<[f32; 4]> {
		let edges = [
			[r.x - t, r.y - t, r.w + 2.0 * t, t],
			[r.x - t, r.y + r.h, r.w + 2.0 * t, t],
			[r.x - t, r.y, t, r.h],
			[r.x + r.w, r.y, t, r.h],
		];
		quads
			.iter()
			.filter(|q| {
				edges.iter().any(|e| {
					(q.pos[0] - e[0]).abs() < 0.01
						&& (q.pos[1] - e[1]).abs() < 0.01
						&& (q.size[0] - e[2]).abs() < 0.01
						&& (q.size[1] - e[3]).abs() < 0.01
				})
			})
			.map(|q| q.color)
			.collect()
	}

	// A second Apply has to diff against what the first one applied, not against
	// the dialog as it opened, or putting a value back reads as no change.
	// Test ID: Er2X6Ej
	#[test]
	fn a_second_apply_diffs_against_the_first() {
		let mut d = mk_dialog(4000.0);
		d.orig.wallpaper_default_fit = config::Fit::Stretch;
		d.edited.wallpaper_default_fit = config::Fit::Stretch;
		d.set_radio(Key::BgFit, 1);
		assert_eq!(d.edited().wallpaper_default_fit, config::Fit::Zoom);
		d.commit_baseline();
		d.set_radio(Key::BgFit, 0);
		assert_ne!(
			d.orig().wallpaper_default_fit,
			d.edited().wallpaper_default_fit,
			"going back to Stretch reads as no change, so nothing is written"
		);
		// and the app does move the baseline on every Apply
		// a Windows checkout has CRLF line ends
		let app = include_str!("app/dialogs.rs").replace("\r\n", "\n");
		let at = app
			.find("fn apply_dialog_settings")
			.expect("apply_dialog_settings");
		let body = &app[at..at + app[at..].find("\n\t}\n").expect("its end")];
		assert!(
			body.contains(".commit_baseline()"),
			"Apply no longer resets the baseline"
		);
	}

	// Labels center on the row they are in, measured from the real line height,
	// so they line up with their controls at any interface font.
	// Test ID: Er2X6Ek
	#[test]
	fn a_label_centers_on_its_row_at_any_line_height() {
		for line_h in [19.0, 38.0] {
			let mut d = mk_dialog_line(line_h);
			let mut checked = 0;
			for tab in 0..tab_titles().len() {
				d.tab = tab;
				let texts = d.texts_dip(d.line_h, |s| s.chars().count() as f32 * 7.0);
				for (i, spec) in SettingsDialog::visible(d.specs, tab) {
					if spec.beside || spec.label.is_empty() || matches!(spec.kind, Kind::Header(_))
					{
						continue;
					}
					let want = d.centered_in_row(i, 0.0);
					let x = d.label_x(i);
					assert!(
						texts.iter().any(|t| t.text == spec.label
							&& (t.x - x).abs() < 0.01
							&& (t.y + line_h / 2.0 - want).abs() < 0.5),
						"{} is off its row's center at line height {line_h}",
						spec.label
					);
					checked += 1;
				}
			}
			assert!(checked > 20, "only {checked} labels found");
		}
	}

	// The performance-related sections share one tab, Performance first, and the
	// old Performance tab is gone.
	// Test ID: Er2X6El
	#[test]
	fn the_silk_tab_has_performance_readability_and_scrolling() {
		let specs = &super::ui().specs;
		let at = |key: Key| specs.iter().position(|s| s.key == key).unwrap();
		let silk = specs[at(Key::PerfProfile)].tab;
		assert_eq!(tab_titles()[silk], "Silk");
		let heading_over = |i: usize| {
			specs[..i]
				.iter()
				.rev()
				.find_map(|s| match s.kind {
					Kind::Header(label) => Some(label),
					_ => None,
				})
				.unwrap()
		};
		let mut last = 0;
		for (key, heading) in [
			(Key::PerfAuto, "Performance"),
			(Key::Outline, "Text readability"),
			(Key::SmoothScroll, "Scrolling"),
		] {
			let i = at(key);
			assert_eq!(specs[i].tab, silk, "{} left the Silk tab", key.name());
			assert_eq!(heading_over(i), heading, "{} changed section", key.name());
			assert!(i > last, "{} is out of order", key.name());
			last = i;
		}
		assert!(!tab_titles().contains(&"Performance"));
	}

	// A heading's text sits at the top of its row and the rule near the bottom,
	// and the two stay apart whatever the interface font.
	// Test ID: Er2X6Em
	#[test]
	fn a_heading_never_runs_into_its_rule() {
		for line_h in [14.0, 19.0, 26.0, 38.0, 60.0] {
			let mut d = mk_dialog_line(line_h);
			let mut checked = 0;
			for tab in 0..tab_titles().len() {
				d.tab = tab;
				let texts = d.texts_dip(d.line_h, |s| s.chars().count() as f32 * 7.0);
				for (i, spec) in SettingsDialog::visible(d.specs, tab) {
					let Kind::Header(label) = spec.kind else {
						continue;
					};
					let text = texts
						.iter()
						.find(|t| t.bold && t.text == label)
						.expect("the heading's text");
					let rule = d.header_rule_y(i);
					assert!(
						text.y + line_h < rule,
						"{label} runs into its rule at line height {line_h}"
					);
					assert!(
						rule + 1.0 <= d.row_y(i) + d.row_h(&spec.kind) + 0.01,
						"{label}'s rule falls out of its row"
					);
					checked += 1;
				}
			}
			assert!(checked > 5, "only {checked} headings found");
		}
	}

	// The footer buttons explain themselves, each with its own line.
	// Test ID: Er2X6En
	#[test]
	fn every_footer_button_has_its_own_tip() {
		let d = mk_dialog(4000.0);
		let help = &super::ui().help;
		for (action, r, label) in d.buttons() {
			let want = match action {
				super::Action::Cancel => help.cancel,
				super::Action::Apply => help.apply,
				_ => help.ok,
			};
			assert!(!want.is_empty(), "{label} has no tip declared");
			let (tip, anchor) = d
				.hover_tip_dip(r.x + r.w / 2.0, r.y + r.h / 2.0)
				.unwrap_or_else(|| panic!("{label} shows no tip"));
			assert_eq!(tip, want, "{label} shows the wrong tip");
			assert!(
				(anchor.x - r.x).abs() < 0.01,
				"{label}'s tip hangs elsewhere"
			);
		}
		let (c, a, o) = (help.cancel, help.apply, help.ok);
		assert!(c != a && a != o && c != o, "two buttons share a tip");
	}

	// A sub-group needs no heading above it: an unindented control followed by
	// indented rows leads one, and gets the sub-group gap after a plain row.
	// Test ID: Er2X6Eo
	#[test]
	fn a_sub_group_stands_without_a_heading() {
		let row = |key: Key, indent: u8| super::Spec {
			label: "",
			key,
			kind: Kind::Toggle,
			tab: 0,
			help: "",
			indent,
			beside: false,
			revert_help: "",
			windows: false,
			not_macos: false,
			warning: "",
			windows_warning: "",
			group: None,
		};
		let specs = [
			row(Key::PerfCheckHardware, 0),
			row(Key::Transparency, 0),
			row(Key::Opacity, 1),
			row(Key::BackdropBlur, 1),
		];
		assert!(!SettingsDialog::leads_subgroup(&specs, 0, 0));
		assert!(SettingsDialog::leads_subgroup(&specs, 1, 0));
		assert_eq!(SettingsDialog::gap_above(&specs, 0, 0, None), 0.0);
		assert_eq!(
			SettingsDialog::gap_above(&specs, 1, 0, Some(&specs[0])),
			lay().subgroup_gap
		);
		assert!(lay().subgroup_gap > 0.0);
		assert_eq!(
			SettingsDialog::gap_above(&specs, 2, 0, Some(&specs[1])),
			0.0
		);
		assert_eq!(
			SettingsDialog::gap_above(&specs, 3, 0, Some(&specs[2])),
			0.0
		);
	}

	// Which tab a row is on and which sub-group it belongs to is the design. The
	// completeness test only asks that each row exists, so this pins the rest,
	// by key so a relabel does not move it.
	// Test ID: Er2X6Ep
	#[test]
	fn each_tab_has_its_designed_sub_groups() {
		let specs = &super::ui().specs;
		let at = |key: Key| {
			specs
				.iter()
				.position(|s| s.key == key)
				.unwrap_or_else(|| panic!("no {} row", key.name()))
		};
		// the rows under a leader, down to the first one back at its depth; half
		// a line's indent means nothing, so those are stepped over
		let members = |lead: usize| -> Vec<Key> {
			let top = &specs[lead];
			specs[lead + 1..]
				.iter()
				.filter(|s| !s.beside)
				.take_while(|s| s.tab == top.tab && s.indent > top.indent)
				.map(|s| {
					assert_eq!(s.indent, top.indent + 1, "{} is nested too deep", s.label);
					s.key
				})
				.collect()
		};
		let groups: &[(&str, Key, &[Key])] = &[
			(
				"Silk",
				Key::TextScrim,
				&[
					Key::ScrimStrength,
					Key::ScrimRadius,
					Key::ScrimSoftness,
					Key::ScrimFunction,
					Key::MinContrast,
				],
			),
			(
				"Silk",
				Key::SmoothScroll,
				&[
					Key::ScrollEaseIn,
					Key::ScrollRampUp,
					Key::SingleScreenTau,
					Key::ScrollRampDown,
					Key::ScrollEaseOut,
				],
			),
			(
				"Background",
				Key::Transparency,
				&[Key::Opacity, Key::BackdropBlur],
			),
			(
				"Background",
				Key::BgEnabled,
				&[
					Key::BgImage,
					Key::BgFit,
					Key::BgHonorXmp,
					Key::BgRotate,
					Key::BgOpacity,
					Key::BgBlur,
					Key::BgHonorXmpLook,
				],
			),
			(
				"Background",
				Key::BgContrastMask,
				&[
					Key::BgContrastSize,
					Key::BgContrastStrength,
					Key::BgContrastAuto,
				],
			),
			(
				"Cursor",
				Key::CursorBlinking,
				&[
					Key::CursorBlinkRate,
					Key::CursorAnimation,
					Key::CursorResume,
				],
			),
			(
				"Movement",
				Key::Scrollbar,
				&[Key::ScrollbarThickness, Key::ScrollbarAutoHide],
			),
			("Movement", Key::Minimap, &[Key::MinimapWidth]),
			(
				"Themes",
				Key::ColBg,
				&[Key::ColFromWallpaper, Key::ColFg, Key::ColCursor],
			),
			(
				"Themes",
				Key::ColDialogBg,
				&[
					Key::ColDialogFg,
					Key::ColMenuBg,
					Key::ColMenuFg,
					Key::ColGutter,
					Key::ColHighlight,
					Key::ColFocus,
				],
			),
			(
				"Window",
				Key::RememberSize,
				&[Key::RememberPerMonitor, Key::Columns, Key::Rows],
			),
		];
		for &(tab, lead, want) in groups {
			let i = at(lead);
			assert_eq!(
				tab_titles()[specs[i].tab],
				tab,
				"{} left its tab",
				lead.name()
			);
			let got = members(i);
			let names = |keys: &[Key]| keys.iter().map(|k| k.name()).collect::<Vec<_>>();
			assert_eq!(
				names(&got),
				names(want),
				"{}'s sub-group changed",
				lead.name()
			);
		}
		// the whole Cursor tab, in order; the scrim and outline pair has no key
		// of its own, so it answers by its first part
		let cursor = specs[at(Key::CursorBlinking)].tab;
		assert_eq!(tab_titles()[cursor], "Cursor");
		let rows: Vec<&str> = specs
			.iter()
			.filter(|s| s.tab == cursor && !matches!(s.kind, Kind::Header(_)))
			.map(|s| match s.kind {
				Kind::Dual { keys, .. } => keys[0].name(),
				_ => s.key.name(),
			})
			.collect();
		assert_eq!(
			rows,
			[
				"CursorHeight",
				"CursorWidth",
				"CursorBlinking",
				"CursorBlinkRate",
				"CursorAnimation",
				"CursorResume",
				"CursorScrim",
				"CopyOnSelect",
			]
		);
		// the margin stands on its own after the size group
		let margin = &specs[at(Key::Margin)];
		assert_eq!(tab_titles()[margin.tab], "Window");
		assert_eq!(margin.indent, 0);
	}

	// Every slider says what it counts in, and one that counts pixels steps in
	// whole ones rather than showing 10.00 beside whole percentages.
	// Test ID: Er2X6Er
	#[test]
	fn every_slider_names_its_unit_and_pixels_step_whole() {
		// counts and sizes whose label already says what they are
		let unitless = [
			Key::FontSize,
			Key::LineHeight,
			Key::WheelLines,
			Key::Columns,
			Key::Rows,
			Key::IdleHiddenMin,
			Key::IdleMin,
		];
		let specs = &super::ui().specs;
		for s in specs {
			let Kind::Slider { int, .. } = s.kind else {
				continue;
			};
			if s.label.ends_with(" px") {
				assert!(int, "{} steps in fractions of a pixel", s.label);
			}
			assert!(
				[" %", " px", " ms", " s"]
					.iter()
					.any(|unit| s.label.ends_with(unit))
					|| unitless.contains(&s.key),
				"{} names no unit",
				s.label
			);
		}
		let label = |key: Key| specs.iter().find(|s| s.key == key).unwrap().label;
		for (key, unit) in [
			(Key::BgBlur, " px"),
			(Key::ScrollbarThickness, " px"),
			(Key::ScrimRadius, " px"),
			(Key::Outline, " px"),
			(Key::CursorBlinkRate, " s"),
			(Key::CursorResume, " s"),
		] {
			assert!(
				label(key).ends_with(unit),
				"{} should end in{unit}",
				label(key)
			);
		}
	}

	// A field is its own height, taller than a checkbox so the text has room
	// above and below it; the color chip matches the field beside it; and a box
	// centers in the row it is in, not in the row floor.
	// Test ID: Er2X6Es
	#[test]
	fn a_field_has_room_for_its_text_and_centers_in_its_row() {
		for line_h in [18.0, 38.0] {
			let mut d = mk_dialog_line(line_h);
			assert!(d.field_h() >= d.line_h + 2.0 * lay().field_pad_v);
			assert!(d.field_h() > lay().swatch);
			let c = d.specs.iter().position(|s| s.key == Key::ColBg).unwrap();
			d.tab = d.specs[c].tab;
			assert!((d.swatch(c).h - d.hexbox(c).h).abs() < 0.01);
			let mut checked = 0;
			for tab in 0..tab_titles().len() {
				d.tab = tab;
				for (i, spec) in SettingsDialog::visible(d.specs, tab) {
					let r = match spec.kind {
						Kind::Slider { .. } => d.valbox(i),
						Kind::Color => d.hexbox(i),
						Kind::Text => d.textbox(i),
						_ => continue,
					};
					let mid = d.row_y(i) + d.row_screen_h(i) / 2.0;
					assert!(
						(r.y + r.h / 2.0 - mid).abs() < 0.5,
						"{} rides off its row's center at line height {line_h}",
						spec.label
					);
					checked += 1;
				}
			}
			assert!(checked > 20, "only {checked} fields found");
		}
	}

	// A focused field draws one outline: the ring on the field's own edge, and
	// the field's border standing down.
	// Test ID: Er2X6Eu
	#[test]
	fn a_focused_field_draws_one_outline() {
		let mut d = mk_dialog(4000.0);
		let i = d.specs.iter().position(|s| s.key == Key::BgImage).unwrap();
		d.tab = d.specs[i].tab;
		d.focus = Some(super::Focus::Row(i, 0));
		let (_, rows) = d.rects_dip(d.line_h, |s: &str| s.chars().count() as f32 * 7.0);
		let ring = super::config::srgb_f32(super::dlg().focus_out);
		let edges = edge_colors(&rows, d.textbox(i), 1.0);
		assert_eq!(edges.len(), 4, "{} quads on the field's edge", edges.len());
		assert!(edges.iter().all(|&c| c == ring), "the edge is not the ring");
	}

	// Only OK, the button Enter fires, is outlined in the highlight; the others
	// take the quiet gray.
	// Test ID: Er2X6Ev
	#[test]
	fn only_the_default_button_is_outlined_in_the_highlight() {
		let mut d = mk_dialog(4000.0);
		d.focus = None;
		let (fixed, _) = d.rects_dip(d.line_h, |s: &str| s.chars().count() as f32 * 7.0);
		let hl = super::config::srgb_f32(super::dlg().btn_hl);
		let gray = super::config::srgb_f32(super::dlg().panel_border);
		assert_ne!(hl, gray);
		for (action, r, label) in d.buttons() {
			let want = if action == super::Action::Ok {
				hl
			} else {
				gray
			};
			let edges = edge_colors(&fixed, r, 1.0);
			assert_eq!(edges.len(), 4, "{label} has no outline");
			assert!(edges.iter().all(|&c| c == want), "{label}'s outline");
		}
	}

	// Test ID: Er2X6Ew
	#[test]
	fn a_footer_caption_is_centered_in_its_button() {
		let d = mk_dialog(4000.0);
		let measure = |s: &str| s.chars().count() as f32 * 7.0;
		let texts = d.texts_dip(d.line_h, measure);
		for (_, r, label) in d.buttons() {
			let t = texts
				.iter()
				.find(|t| t.text == label && r.contains(t.x, t.y + d.line_h / 2.0))
				.unwrap_or_else(|| panic!("{label} has no caption"));
			assert!(
				(t.x + measure(label) / 2.0 - (r.x + r.w / 2.0)).abs() < 0.5,
				"{label} is off center"
			);
		}
	}

	// A second section on a tab is set off from the one above it.
	// Test ID: Er2X6Ex
	#[test]
	fn a_heading_after_a_section_gets_clear_space_above_it() {
		assert!(lay().header_gap > 0.0);
		let mut d = mk_dialog(4000.0);
		let mut checked = 0;
		for tab in 0..tab_titles().len() {
			d.tab = tab;
			let mut prev: Option<usize> = None;
			for (i, spec) in SettingsDialog::visible(d.specs, tab) {
				if let (Kind::Header(label), Some(p)) = (&spec.kind, prev) {
					let gap = d.row_y(i) - (d.row_y(p) + d.row_screen_h(p));
					assert!(
						(gap - lay().header_gap).abs() < 0.01,
						"{label} is {gap} below the section above it"
					);
					checked += 1;
				}
				prev = Some(i);
			}
		}
		assert!(checked > 3, "only {checked} headings follow a section");
	}

	// Alt+C, Alt+A and Alt+O fire the footer buttons, and holding Alt underlines
	// the letter that does it.
	// Test ID: Er2X6Ey
	#[test]
	fn alt_fires_the_footer_buttons_and_underlines_them() {
		use super::Action;
		let mut d = mk_dialog(4000.0);
		assert_eq!(d.alt_key('c'), Action::Cancel);
		assert_eq!(d.alt_key('A'), Action::Apply);
		assert_eq!(d.alt_key('o'), Action::Ok);
		assert_eq!(d.alt_key('x'), Action::None);
		let text = super::config::srgb_f32(super::dlg().text);
		let underlines = |d: &SettingsDialog| {
			let (fixed, _) = d.rects_dip(d.line_h, |s: &str| s.chars().count() as f32 * 7.0);
			d.buttons()
				.iter()
				.filter(|(_, r, _)| {
					fixed.iter().any(|q| {
						q.color == text
							&& (q.size[1] - 1.5).abs() < 0.01
							&& r.contains(q.pos[0], q.pos[1])
					})
				})
				.count()
		};
		d.set_mods(true, false, false);
		assert_eq!(
			underlines(&d),
			3,
			"Alt held, and not every button underlined"
		);
		d.set_mods(false, false, false);
		assert_eq!(underlines(&d), 0, "underlined with Alt up");
	}

	// Space typed into an open field is a space, and the dialog takes no action.
	// Test ID: Er2X6Ez
	#[test]
	fn space_types_into_an_open_field() {
		let (mut d, _) = mk_text_edit("DejaVu");
		d.cursor_end();
		assert_eq!(d.key_space(), super::Action::None);
		d.char_input('S');
		assert_eq!(d.edit.as_ref().unwrap().buf, "DejaVu S");
	}

	// Transparency and its two rows are the last group on the Background tab,
	// after everything that works whatever the desktop.
	// Test ID: EreHnnq
	#[test]
	fn transparency_is_the_last_group_on_the_background_tab() {
		let specs = &super::ui().specs;
		let background = tab_titles()
			.iter()
			.position(|t| *t == "Background")
			.expect("a Background tab");
		let keys: Vec<Key> = specs
			.iter()
			.filter(|s| s.tab == background && !matches!(s.kind, Kind::Header(_)))
			.map(|s| s.key)
			.collect();
		assert_eq!(
			keys[keys.len() - 3..],
			[Key::Transparency, Key::Opacity, Key::BackdropBlur],
			"{keys:?}"
		);
	}

	// Commented out 20261006: "Free resources when idle" has a warning mark too
	// now (2026100418225506), so Transparency is no longer the only row with
	// one, nor Background the only tab that draws one. Replaced by
	// `two_rows_warn_and_each_mark_answers_for_its_own` (EryD9nl), which checks
	// the same things for both marks.
	// // The Transparency row carries a warning mark after its label, with its own
	// // flyover, and it is the only row that does.
	// // Test ID: EreHnrx
	// #[test]
	// fn the_transparency_row_warns_that_it_needs_the_compositor() {
	// 	let mut d = mk_dialog(4000.0);
	// 	let warned: Vec<Key> = d
	// 		.specs
	// 		.iter()
	// 		.filter(|s| !s.warning.is_empty())
	// 		.map(|s| s.key)
	// 		.collect();
	// 	assert_eq!(warned, [Key::Transparency]);
	// 	let i = d
	// 		.specs
	// 		.iter()
	// 		.position(|s| s.key == Key::Transparency)
	// 		.unwrap();
	// 	assert!(d.specs[i].warning.contains("compositor"));
	// 	d.tab = d.specs[i].tab;
	// 	let mark = d.warning_box(i, &mut chars7);
	// 	// after the label's text, and clear of the checkbox
	// 	assert!(mark.x > d.label_x(i) + chars7(d.specs[i].label));
	// 	assert!(
	// 		mark.x + mark.w <= d.checkbox(i).x,
	// 		"the mark runs into the checkbox"
	// 	);
	// 	// drawn: one triangle, where the mark is
	// 	let (_, rows) = d.rects_dip(d.line_h, &mut chars7);
	// 	let triangles: Vec<_> = rows
	// 		.iter()
	// 		.filter(|r| r.mode() == QuadMode::Triangle)
	// 		.collect();
	// 	assert_eq!(triangles.len(), 1, "one mark on the tab");
	// 	assert!((triangles[0].pos[0] - mark.x).abs() < 0.01);
	// 	assert!(
	// 		(triangles[0].params[1] - 3.0).abs() < f32::EPSILON,
	// 		"points up"
	// 	);
	// 	// its own tip over it, the row's own over the label
	// 	let (cx, cy) = (mark.x + mark.w / 2.0, mark.y + mark.h / 2.0);
	// 	assert_eq!(
	// 		d.hover_tip_dip(cx, cy, &mut chars7).map(|(tip, _)| tip_text(tip)),
	// 		Some(d.specs[i].warning)
	// 	);
	// 	let label = d.label_x(i) + 2.0;
	// 	assert_eq!(
	// 		d.hover_tip_dip(label, cy, &mut chars7).map(|(tip, _)| tip_text(tip)),
	// 		Some(d.specs[i].help)
	// 	);
	// 	// no other tab draws one
	// 	let background = d.tab;
	// 	for tab in (0..tab_titles().len()).filter(|t| *t != background) {
	// 		d.tab = tab;
	// 		let (_, rows) = d.rects_dip(d.line_h, &mut chars7);
	// 		assert!(
	// 			!rows.iter().any(|r| r.mode() == QuadMode::Triangle),
	// 			"a triangle on tab {tab}"
	// 		);
	// 	}
	// }

	// Commented out 20261008: a row now has one tip, shown over its label, its
	// mark and its control alike (2026100812334388), so a mark no longer
	// answers with text of its own. Replaced by
	// `two_rows_warn_and_each_mark_shows_the_rows_tip` (Es9VucS), which checks
	// the same things with the row's tip over the mark.
	// // Two rows carry a warning mark: Transparency on the Background tab, and
	// // "Free resources when idle" on the Window tab. Each draws one triangle after
	// // its label, clear of its checkbox, and answers with its own flyover, while
	// // the label keeps the row's usual one. No other tab draws a mark.
	// // Test ID: EryD9nl
	// #[test]
	// fn two_rows_warn_and_each_mark_answers_for_its_own() {
	// 	// the label column the way `chrome_widths` measures it, marks included
	// 	let line_h = 18.0;
	// 	let label_w = super::ui()
	// 		.specs
	// 		.iter()
	// 		.map(|s| {
	// 			let mark = if s.warning.is_empty() {
	// 				0.0
	// 			} else {
	// 				super::warning_room(line_h)
	// 			};
	// 			chars7(s.label) + f32::from(s.indent) * lay().indent + mark
	// 		})
	// 		.fold(0.0f32, f32::max)
	// 		+ lay().label_gap;
	// 	let mut d = SettingsDialog::new(
	// 		0.0,
	// 		0.0,
	// 		line_h,
	// 		label_w,
	// 		80.0,
	// 		90.0,
	// 		0.0,
	// 		vec![90.0; tab_titles().len()],
	// 		labels7(1.0),
	// 		f32::MAX,
	// 		4000.0,
	// 		1.0,
	// 	);
	// 	let warned: Vec<Key> = d
	// 		.specs
	// 		.iter()
	// 		.filter(|s| !s.warning.is_empty())
	// 		.map(|s| s.key)
	// 		.collect();
	// 	assert_eq!(warned, [Key::Transparency, Key::IdleRelease]);
	// 	let mut marked = Vec::new();
	// 	for key in warned {
	// 		let i = d.specs.iter().position(|s| s.key == key).unwrap();
	// 		d.tab = d.specs[i].tab;
	// 		marked.push(d.tab);
	// 		let mark = d.warning_box(i, &mut chars7);
	// 		assert!(mark.x > d.label_x(i) + chars7(d.specs[i].label));
	// 		assert!(
	// 			mark.x + mark.w <= d.checkbox(i).x,
	// 			"{}'s mark runs into the checkbox",
	// 			key.name()
	// 		);
	// 		let (_, rows) = d.rects_dip(d.line_h, &mut chars7);
	// 		let triangles: Vec<_> = rows
	// 			.iter()
	// 			.filter(|r| r.mode() == QuadMode::Triangle)
	// 			.collect();
	// 		assert_eq!(triangles.len(), 1, "one mark on {}'s tab", key.name());
	// 		assert!((triangles[0].pos[0] - mark.x).abs() < 0.01);
	// 		let (cx, cy) = (mark.x + mark.w / 2.0, mark.y + mark.h / 2.0);
	// 		assert_eq!(
	// 			d.hover_tip_dip(cx, cy).map(|(tip, _)| tip_text(tip)),
	// 			Some(d.specs[i].warning)
	// 		);
	// 		let label = d.label_x(i) + 2.0;
	// 		assert_eq!(
	// 			d.hover_tip_dip(label, cy).map(|(tip, _)| tip_text(tip)),
	// 			Some(d.specs[i].help)
	// 		);
	// 	}
	// 	assert_eq!(tab_titles()[marked[0]], "Background");
	// 	assert_eq!(tab_titles()[marked[1]], "Window");
	// 	let at = |key: Key| d.specs.iter().find(|s| s.key == key).unwrap().warning;
	// 	assert!(at(Key::Transparency).contains("compositor"));
	// 	assert!(at(Key::IdleRelease).contains("driver"));
	// 	// the memory half is Windows' alone
	// 	assert_eq!(
	// 		at(Key::Transparency).contains("memory"),
	// 		cfg!(windows),
	// 		"{}",
	// 		at(Key::Transparency)
	// 	);
	// 	for tab in (0..tab_titles().len()).filter(|t| !marked.contains(t)) {
	// 		d.tab = tab;
	// 		let (_, rows) = d.rects_dip(d.line_h, &mut chars7);
	// 		assert!(
	// 			!rows.iter().any(|r| r.mode() == QuadMode::Triangle),
	// 			"a triangle on tab {tab}"
	// 		);
	// 	}
	// }

	// Two rows have a warning mark: Transparency on the Background tab, and
	// "Free resources when idle" on the Window tab. Each draws one triangle after
	// its label, clear of its checkbox. The mark's text ends the row's one tip,
	// which shows over the label, the mark and the checkbox. No other tab draws
	// a mark.
	// Test ID: Es9VucS
	#[test]
	fn two_rows_warn_and_each_mark_shows_the_rows_tip() {
		// the label column the way `chrome_widths` measures it, marks included
		let line_h = 18.0;
		let label_w = super::ui()
			.specs
			.iter()
			.map(|s| {
				let mark = if s.warning.is_empty() {
					0.0
				} else {
					super::warning_room(line_h)
				};
				chars7(s.label) + f32::from(s.indent) * lay().indent + mark
			})
			.fold(0.0f32, f32::max)
			+ lay().label_gap;
		let mut d = SettingsDialog::new(
			0.0,
			0.0,
			line_h,
			Chrome {
				label_w,
				btn_w: 80.0,
				row_btn_w: 90.0,
				value_w: 0.0,
				tab_ws: vec![90.0; tab_titles().len()],
				label_ws: labels7(1.0),
				..Chrome::default()
			},
			f32::MAX,
			4000.0,
			1.0,
		);
		let warned: Vec<Key> = d
			.specs
			.iter()
			.filter(|s| !s.warning.is_empty())
			.map(|s| s.key)
			.collect();
		assert_eq!(warned, [Key::Transparency, Key::IdleRelease]);
		let mut marked = Vec::new();
		for key in warned {
			let i = d.specs.iter().position(|s| s.key == key).unwrap();
			d.tab = d.specs[i].tab;
			marked.push(d.tab);
			let mark = d.warning_box(i, &mut chars7);
			assert!(mark.x > d.label_x(i) + chars7(d.specs[i].label));
			assert!(
				mark.x + mark.w <= d.checkbox(i).x,
				"{}'s mark runs into the checkbox",
				key.name()
			);
			let (_, rows) = d.rects_dip(d.line_h, &mut chars7);
			let triangles: Vec<_> = rows
				.iter()
				.filter(|r| r.mode() == QuadMode::Triangle)
				.collect();
			assert_eq!(triangles.len(), 1, "one mark on {}'s tab", key.name());
			assert!((triangles[0].pos[0] - mark.x).abs() < 0.01);
			let help = d.specs[i].help;
			assert!(
				help.ends_with(d.specs[i].warning) && help.len() > d.specs[i].warning.len(),
				"{}: {help}",
				key.name()
			);
			let bx = d.checkbox(i);
			let cy = mark.y + mark.h / 2.0;
			// at its default, so the tip has no value lines (G6)
			d.revert(key);
			for x in [d.label_x(i) + 2.0, mark.x + mark.w / 2.0, bx.x + bx.w / 2.0] {
				assert_eq!(
					d.hover_tip_dip(x, cy).map(|(tip, _)| tip_text(tip)),
					Some(help),
					"{} at x {x}",
					key.name()
				);
			}
		}
		assert_eq!(tab_titles()[marked[0]], "Background");
		assert_eq!(tab_titles()[marked[1]], "Window");
		let at = |key: Key| d.specs.iter().find(|s| s.key == key).unwrap().warning;
		assert!(at(Key::Transparency).contains("compositor"));
		assert!(at(Key::IdleRelease).contains("driver"));
		// the memory half is Windows' alone
		assert_eq!(
			at(Key::Transparency).contains("memory"),
			cfg!(windows),
			"{}",
			at(Key::Transparency)
		);
		for tab in (0..tab_titles().len()).filter(|t| !marked.contains(t)) {
			d.tab = tab;
			let (_, rows) = d.rects_dip(d.line_h, &mut chars7);
			assert!(
				!rows.iter().any(|r| r.mode() == QuadMode::Triangle),
				"a triangle on tab {tab}"
			);
		}
	}

	// "Resource use" is the last group on the Window tab: the idle switch with
	// its two waits under it, then software rendering at the switch's depth.
	// Test ID: EryD9rp
	#[test]
	fn the_resource_use_group_ends_the_window_tab() {
		let specs = &super::ui().specs;
		let head = specs
			.iter()
			.position(|s| matches!(s.kind, Kind::Header(label) if label == "Resource use"))
			.expect("a Resource use heading");
		assert_eq!(tab_titles()[specs[head].tab], "Window");
		let rest: Vec<(Key, u8)> = specs[head + 1..]
			.iter()
			.take_while(|s| s.tab == specs[head].tab)
			.map(|s| (s.key, s.indent))
			.collect();
		assert_eq!(
			rest,
			[
				(Key::IdleRelease, 0),
				(Key::IdleHiddenMin, 1),
				(Key::IdleMin, 1),
				(Key::SoftwareRendering, 0),
			],
			"nothing else in the group, and nothing after it on the tab"
		);
	}

	// A desktop that never says a window is hidden grays "Minutes when hidden"
	// and says why, since the other wait is the one that runs there. The switch
	// and the other wait stay as they were.
	// Test ID: EryD9vW
	#[test]
	fn the_hidden_wait_is_grayed_where_the_desktop_never_says() {
		assert_eq!(super::hidden_wait_tip(true), None);
		assert!(super::hidden_wait_tip(false).is_some_and(|tip| tip.contains("Wayland")));
		let mut d = mk_dialog(4000.0);
		d.edited.idle_release = true;
		let i = d
			.specs
			.iter()
			.position(|s| s.key == Key::IdleHiddenMin)
			.unwrap();
		d.tab = d.specs[i].tab;
		assert!(!d.disabled(Key::IdleHiddenMin));
		d.set_sees_hidden(false);
		assert!(d.disabled(Key::IdleHiddenMin));
		assert!(!d.disabled(Key::IdleMin));
		assert!(!d.disabled(Key::IdleRelease));
		let track = d.track(i);
		let y = track.y + track.h / 2.0;
		assert_eq!(
			d.hover_tip_dip(d.label_x(i) + 2.0, y)
				.map(|(tip, _)| tip_text(tip)),
			super::hidden_wait_tip(false)
		);
	}

	// A triangle's params.y is a count of quarter-turns, not a length, so the
	// boundary must not scale it. At 2x the mark turned to point the other way.
	// Test ID: EreHnvO
	#[test]
	fn a_triangle_keeps_its_direction_at_any_scale() {
		for scale in [1.0, 1.25, 1.5, 2.0] {
			let mut d = mk_dialog_at(4000.0, scale);
			let i = d
				.specs
				.iter()
				.position(|s| s.key == Key::Transparency)
				.unwrap();
			d.tab = d.specs[i].tab;
			let (_, rows) = d.rects(18.0 * scale, |s| chars7(s) * scale);
			let turns: Vec<f32> = rows
				.iter()
				.filter(|r| r.mode() == QuadMode::Triangle)
				.map(|r| r.params[1])
				.collect();
			assert_eq!(turns, [3.0], "at {scale}x");
		}
	}

	// The Keys tab, with every row on the defaults for the platform asked for,
	// whatever the box's own config binds.
	fn keys_dialog(mac: bool) -> (SettingsDialog, usize) {
		let mut d = mk_dialog(4000.0);
		d.mac = mac;
		d.orig.keys = crate::keys::Bindings::defaults(mac);
		d.edited.keys = crate::keys::Bindings::defaults(mac);
		let tab = tab_titles()
			.iter()
			.position(|title| *title == "Keys")
			.expect("a Keys tab");
		d.tab = tab;
		(d, tab)
	}
	fn row_of(d: &SettingsDialog, key: Key) -> usize {
		d.specs
			.iter()
			.position(|spec| spec.key == key)
			.unwrap_or_else(|| panic!("no row for {}", key.name()))
	}
	fn shown_on(d: &SettingsDialog, i: usize) -> String {
		let (chords, _, note) = d.hotkey_text(i);
		format!("{chords} | {note}")
	}

	// A hotkey row waits for the next chord only once asked, takes every key
	// while it waits, and ends on a chord, on Escape or on Backspace.
	// Test ID: Erejamb
	#[test]
	fn a_hotkey_row_takes_the_next_chord_pressed() {
		use crate::input::Hotkey;
		use winit::keyboard::{Key as Pressed, ModifiersState, NamedKey};
		let (mut d, _) = keys_dialog(false);
		let i = row_of(&d, Key::HotkeySplitRight);
		let ctrl_alt = ModifiersState::CONTROL.union(ModifiersState::ALT);
		let typed = |text: &str| Pressed::Character(text.into());
		// walking onto it by keyboard does not start it, or Tab could never leave
		d.focus = Some(super::Focus::Row(i - 1, 0));
		d.key_tab();
		assert_eq!(d.focus, Some(super::Focus::Row(i, 0)));
		assert!(d.capture.is_none());
		assert!(!d.capture_key(&typed("r"), ctrl_alt), "nothing is waiting");
		assert_eq!(shown_on(&d, i), "Alt+Shift+Plus | ");

		d.key_space();
		assert_eq!(d.capture, Some(i));
		assert!(shown_on(&d, i).contains(super::CAPTURE_PROMPT));
		assert!(d.capture_key(&Pressed::Named(NamedKey::Control), ModifiersState::CONTROL));
		assert_eq!(d.capture, Some(i), "a modifier alone is not the chord");
		assert!(d.capture_key(&typed("r"), ModifiersState::empty()));
		assert_eq!(d.capture, Some(i), "a bare letter is refused");
		assert!(shown_on(&d, i).contains("R needs Ctrl, Alt or Super held"));
		assert!(d.capture_key(&Pressed::Named(NamedKey::Tab), ModifiersState::CONTROL));
		assert_eq!(
			d.capture, None,
			"Ctrl+Tab is a chord here, not a tab switch"
		);
		assert_eq!(shown_on(&d, i), "Ctrl+Tab | ");
		assert_eq!(
			d.tab,
			tab_titles().iter().position(|t| *t == "Keys").unwrap()
		);

		// Enter starts it as well, and Escape leaves the row as it was
		assert_eq!(d.key_enter(), super::Action::None);
		assert!(d.capture_key(&Pressed::Named(NamedKey::Escape), ModifiersState::empty()));
		assert_eq!(d.capture, None);
		assert_eq!(shown_on(&d, i), "Ctrl+Tab | ");

		d.key_space();
		assert!(d.capture_key(&typed("r"), ctrl_alt));
		assert_eq!(shown_on(&d, i), "Ctrl+Alt+R | ");
		assert!(!d.is_default(Key::HotkeySplitRight));
		// Backspace turns it off, which is set too, and "Off" says so
		d.key_space();
		assert!(d.capture_key(
			&Pressed::Named(NamedKey::Backspace),
			ModifiersState::empty()
		));
		assert!(d.edited.keys.chords(Hotkey::SplitRight).is_empty());
		assert_eq!(d.edited.keys.own(Hotkey::SplitRight), Some(&[][..]));
		let (shown, off, _) = d.hotkey_text(i);
		assert_eq!((shown.as_str(), off), ("Off", true));
		// the revert arrow puts the shipped chord back and queues the line
		d.row_revert(i);
		assert!(d.is_default(Key::HotkeySplitRight));
		assert_eq!(shown_on(&d, i), "Alt+Shift+Plus | ");
		assert_eq!(d.take_reverted(), ["keys.split_right"]);

		// a click on the box starts it, and a click anywhere else stops it
		let mut m = chars7;
		let r = d.textbox(i);
		d.mouse_down_dip(r.x + 4.0, r.y + r.h / 2.0, &mut m);
		assert_eq!(d.capture, Some(i));
		let label = d.label_x(i);
		d.mouse_down_dip(label + 2.0, r.y + r.h / 2.0, &mut m);
		assert_eq!(d.capture, None);
		assert_eq!(shown_on(&d, i), "Alt+Shift+Plus | ");
	}

	// A chord pressed for one hotkey that another has is said on both rows, the
	// way the launch says it about the file. One the other hotkey had only by
	// default stays the file's business; one it had been set to moves.
	// Test ID: Erejaq5
	#[test]
	fn a_chord_another_hotkey_had_is_said_on_both_rows() {
		use crate::input::Hotkey;
		use winit::keyboard::{Key as Pressed, ModifiersState};
		let (mut d, _) = keys_dialog(false);
		let close_tab = row_of(&d, Key::HotkeyCloseTab);
		let close_pane = row_of(&d, Key::HotkeyClosePane);
		let ctrl_shift = ModifiersState::CONTROL.union(ModifiersState::SHIFT);
		d.capture_start(close_pane);
		assert!(d.capture_key(&Pressed::Character("W".into()), ctrl_shift));
		assert_eq!(
			shown_on(&d, close_tab),
			"Ctrl+F4 | Ctrl+Shift+W is set for Close pane"
		);
		assert_eq!(
			shown_on(&d, close_pane),
			"Ctrl+Shift+W | took Ctrl+Shift+W from Close tab"
		);
		assert!(
			d.is_default(Key::HotkeyCloseTab),
			"close_tab itself was not set"
		);

		// set for one, then pressed for another: the latest press has it
		let right = row_of(&d, Key::HotkeySplitRight);
		let down = row_of(&d, Key::HotkeySplitDown);
		let alt_d = Pressed::Character("d".into());
		d.capture_start(right);
		assert!(d.capture_key(&alt_d, ModifiersState::ALT));
		d.capture_start(down);
		assert!(d.capture_key(&alt_d, ModifiersState::ALT));
		assert_eq!(shown_on(&d, down), "Alt+D | ");
		assert_eq!(
			shown_on(&d, right),
			"Off | Alt+D is set for Split horizontal"
		);
		assert_eq!(d.edited.keys.own(Hotkey::SplitRight), Some(&[][..]));
		assert!(d.edited.keys.taken().iter().all(|t| !t.both_set));
		// once the chord moves on again there is nothing left to say
		d.capture_start(down);
		assert!(d.capture_key(&Pressed::Character("e".into()), ModifiersState::ALT));
		assert_eq!(shown_on(&d, right), "Off | ");

		// two set the same way in the file are both named
		d.edited.keys = crate::keys::Bindings::with(
			false,
			&[
				(
					Hotkey::SplitDown,
					vec![crate::keys::Chord::parse("Alt+D").unwrap()],
				),
				(
					Hotkey::SplitRight,
					vec![crate::keys::Chord::parse("Alt+D").unwrap()],
				),
			],
		)
		.0;
		d.moved.clear();
		assert_eq!(
			shown_on(&d, right),
			"Alt+D | Alt+D is also set for Split horizontal"
		);
		assert_eq!(shown_on(&d, down), "Off | Alt+D is set for Split vertical");
	}

	// On a Mac the rows show Command chords in Apple's order, and Command held
	// at a press is the chord's Command.
	// Test ID: Erejatf
	#[test]
	fn the_keys_tab_shows_the_mac_chords_on_a_mac() {
		use winit::keyboard::{Key as Pressed, ModifiersState};
		let (mut d, _) = keys_dialog(true);
		let right = row_of(&d, Key::HotkeySplitRight);
		assert_eq!(shown_on(&d, right), "Command+D | ");
		let prev = row_of(&d, Key::HotkeyPrevTab);
		assert_eq!(shown_on(&d, prev), "Shift+Command+[ or Command+PageUp | ");
		// Close pane had no Mac chord until 20261003
		// assert_eq!(shown_on(&d, row_of(&d, Key::HotkeyClosePane)), "Off | ");
		assert_eq!(
			shown_on(&d, row_of(&d, Key::HotkeyClosePane)),
			"Option+Command+W | "
		);
		d.capture_start(right);
		assert!(d.capture_key(&Pressed::Character("r".into()), ModifiersState::empty()));
		assert!(shown_on(&d, right).contains("R needs Control, Option or Command held"));
		let option_command = ModifiersState::SUPER.union(ModifiersState::ALT);
		assert!(d.capture_key(&Pressed::Character("r".into()), option_command));
		assert_eq!(shown_on(&d, right), "Option+Command+R | ");
	}

	// The tab draws what each row says: its chords, then the note in the dim
	// color, both inside the box.
	// Test ID: ErejaxH
	#[test]
	fn a_hotkey_row_draws_its_chords_and_its_note_in_the_box() {
		use winit::keyboard::{Key as Pressed, ModifiersState};
		let (mut d, _) = keys_dialog(false);
		let close_tab = row_of(&d, Key::HotkeyCloseTab);
		let close_pane = row_of(&d, Key::HotkeyClosePane);
		d.capture_start(close_pane);
		let ctrl_shift = ModifiersState::CONTROL.union(ModifiersState::SHIFT);
		assert!(d.capture_key(&Pressed::Character("W".into()), ctrl_shift));
		let texts = d.texts_dip(d.line_h, chars7);
		let r = d.textbox(close_tab);
		let in_box: Vec<&super::TextItem> = texts
			.iter()
			.filter(|t| t.x >= r.x && t.x < r.x + r.w && t.y >= r.y - 1.0 && t.y < r.y + r.h)
			.collect();
		assert_eq!(in_box.len(), 2, "chords and note");
		assert_eq!(in_box[0].text, "Ctrl+F4");
		assert_eq!(in_box[0].color, super::dlg().text);
		assert_eq!(in_box[1].text, "Ctrl+Shift+W is set for Close pane");
		assert_eq!(in_box[1].color, super::dlg().dim);
		assert!(in_box[1].x > in_box[0].x + chars7("Ctrl+F4"));
		assert!(in_box.iter().all(|t| t.clip.is_some()));
	}

	// A hotkey set in Settings goes into the `keys:` block the way a hand edit
	// puts it there, and loads as the same thing. The revert puts the template's
	// own line back.
	// Test ID: Erejb15
	#[test]
	fn a_hotkey_set_in_settings_saves_as_a_hand_edit_would() {
		use winit::keyboard::{Key as Pressed, ModifiersState};
		let _guard = config::test_config_lock();
		let _ = config::settings(); // memoize before the override goes in
		let mac = cfg!(target_os = "macos");
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_keystab_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let _ = std::fs::write(&path, "");
		config::set_config_override(path.clone());
		config::reload_from_disk(); // lays the template down
		let pristine = std::fs::read_to_string(&path).unwrap();
		let shipped = format!(
			"# close_pane: \"{}\"  ## Default",
			crate::keys::value_text(
				crate::keys::Bindings::defaults(mac).chords(crate::input::Hotkey::ClosePane),
				mac
			)
		);
		assert!(pristine.contains(&shipped), "{pristine}");

		let (mut d, _) = keys_dialog(mac);
		d.orig = config::reload_from_disk();
		d.edited = d.orig.clone();
		let i = row_of(&d, Key::HotkeyClosePane);
		d.capture_start(i);
		// a chord other than the default, which on a Mac is Option+Command+W
		let (key, mods) = if mac {
			("W", ModifiersState::SUPER.union(ModifiersState::SHIFT))
		} else {
			("W", ModifiersState::CONTROL.union(ModifiersState::SHIFT))
		};
		assert!(d.capture_key(&Pressed::Character(key.into()), mods));
		let value =
			crate::keys::value_text(d.edited.keys.chords(crate::input::Hotkey::ClosePane), mac);
		assert!(config::persist(&d.orig, &d.edited));
		config::revert_keys(&d.take_reverted());
		let saved = std::fs::read_to_string(&path).unwrap();
		// only the hotkey that was set is in the file; the one that lost a
		// chord to it is not
		let active = |text: &str| -> Vec<String> {
			text.lines()
				.map(str::trim)
				.filter(|line| {
					crate::keys::config_paths()
						.any(|(_, p)| line.starts_with(&format!("{}:", &p["keys.".len()..])))
				})
				.map(String::from)
				.collect()
		};
		let lines = active(&saved);
		assert_eq!(lines.len(), 1, "{saved}");
		let written = lines[0]
			.trim_start_matches("close_pane:")
			.trim()
			.trim_matches('"');
		assert_eq!(written, value);

		// the same as uncommenting the line and typing the chord in
		let by_hand = pristine.replace(&shipped, &format!("close_pane: \"{value}\""));
		let _ = std::fs::write(&path, &by_hand);
		let hand = config::reload_from_disk();
		let _ = std::fs::write(&path, &saved);
		let after = config::reload_from_disk();
		assert!(after.keys == hand.keys, "loads as the hand edit does");
		assert!(after.keys == d.edited.keys, "loads as it was set");

		// the revert arrow takes it out of the file again
		d.orig = after.clone();
		d.edited = after;
		d.row_revert(i);
		assert!(config::persist(&d.orig, &d.edited));
		config::revert_keys(&d.take_reverted());
		let reverted = std::fs::read_to_string(&path).unwrap();
		assert_eq!(reverted, pristine, "the file is back as it shipped");
		assert!(config::reload_from_disk().keys == crate::keys::Bindings::defaults(mac));

		// signs the file format has its own use for still read back as written
		let base = config::reload_from_disk();
		for sign in ["#", "\"", "\\", ";", ":", "'", "[", ","] {
			let chord = crate::keys::Chord::parse(&format!("Ctrl+Alt+{sign}")).expect(sign);
			let mut d = mk_dialog(4000.0);
			d.orig = base.clone();
			d.edited = base.clone();
			d.set_hotkey(crate::input::Hotkey::Quit, vec![chord]);
			assert!(config::persist(&d.orig, &d.edited), "{sign}");
			let back = config::reload_from_disk();
			assert_eq!(
				back.keys.own(crate::input::Hotkey::Quit),
				Some(&[chord][..]),
				"{sign}"
			);
			let _ = std::fs::write(&path, &pristine);
			config::reload_from_disk();
		}
		let _ = std::fs::remove_dir_all(&dir);
	}
}
