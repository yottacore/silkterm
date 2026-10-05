// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

// Display name (window title, default tab title). The Cargo package / binary
// name lives in Cargo.toml; see README "Renaming the project".
pub const APP_NAME: &str = "SilkTerm";

// Where Help -> Support SilkTerm sends the browser. Points at DONATE.md (the
// canonical list of sponsor options and addresses) rather than
// a single link baked into the binary. HEAD resolves to the repo default branch.
pub const DONATE_URL: &str = "https://github.com/yottacore/silkterm/blob/HEAD/DONATE.md";

// The addresses worth handing straight to someone who has already decided.
// DONATE.md carries the rest; --donate prints all three.
pub const SPONSOR_URL: &str = "https://github.com/sponsors/jim-collier";
pub const KOFI_URL: &str = "https://ko-fi.com/jimcollier";

// Which exact build this is. The version can't say - every dogfood build of a
// release carries the same one - so build.rs bakes in whole minutes since 2000 in
// Crockford base 32 (source/src/buildnum.rs). Five characters, sorts in build
// order, and decodes back to the minute it was built.
pub const BUILD_ID: &str = env!("SILK_BUILD");

// A dogfood copy is installed as `slktrmdf_<stamp>_<tag>`, and the pool holds
// several at once, so the window title has to say which one is running. Anything
// else (a release install, a cargo build) answers None.
pub fn dogfood_detail() -> Option<&'static str> {
	static DETAIL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
	DETAIL
		.get_or_init(|| {
			let exe = std::env::current_exe().ok()?;
			let stem = exe.file_stem()?.to_str()?;
			Some(stem.strip_prefix("slktrmdf_")?.to_string())
		})
		.as_deref()
}

// What every window title starts with: the app name, plus the dogfood build when
// this is one.
pub fn title_prefix() -> String {
	match dogfood_detail() {
		Some(detail) => format!("{APP_NAME} [dogfood {detail}]"),
		None => APP_NAME.to_string(),
	}
}

// A terminal running with administrator or root rights says so in its window
// title. Windows already spells this "Administrator: " on the title bar of its
// own consoles, so that word is kept there, and a console sending a title while
// elevated writes it in front of that too - which is the half to take back off.
// SilkTerm never changes its own credentials, so both are answered once.
pub fn rights() -> crate::tabtitle::Rights {
	crate::tabtitle::Rights {
		say: privilege_label(),
		decorated: cfg!(windows),
	}
}

fn privilege_label() -> Option<&'static str> {
	static LABEL: OnceLock<Option<&'static str>> = OnceLock::new();
	*LABEL.get_or_init(privilege_word)
}

#[cfg(unix)]
fn privilege_word() -> Option<&'static str> {
	// SAFETY: geteuid takes no arguments, reads the calling process and cannot fail.
	(unsafe { libc::geteuid() } == 0).then_some("Root")
}

// Elevation is a property of the token, not of the account: an administrator
// running unelevated has the group but not the rights, and answers false here.
#[cfg(windows)]
fn privilege_word() -> Option<&'static str> {
	use windows_sys::Win32::Foundation::CloseHandle;
	use windows_sys::Win32::Security::{
		GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
	};
	use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
	// SAFETY: the token handle is only used between a successful open and its
	// close, and GetTokenInformation is given the size of the buffer it fills.
	unsafe {
		let mut token = std::ptr::null_mut();
		if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
			return None;
		}
		let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
		let mut wrote = 0u32;
		let read = GetTokenInformation(
			token,
			TokenElevation,
			(&raw mut elevation).cast(),
			size_of::<TOKEN_ELEVATION>() as u32,
			&raw mut wrote,
		);
		CloseHandle(token);
		(read != 0 && elevation.TokenIsElevated != 0).then_some("Administrator")
	}
}

#[cfg(not(any(unix, windows)))]
fn privilege_word() -> Option<&'static str> {
	None
}

// Which of the cross builds this binary is - otherwise indistinguishable at a
// glance. Shared by the About dialog and `--about` so the two can't drift.
pub fn build_target() -> String {
	let profile = if cfg!(debug_assertions) {
		"debug"
	} else {
		"release"
	};
	format!(
		"{} / {} ({profile})",
		std::env::consts::ARCH,
		std::env::consts::OS
	)
}

// How long this run has been going, for the About box. Marked from main before
// anything else, so the number is the session's rather than the first reader's.
static LAUNCHED: OnceLock<std::time::Instant> = OnceLock::new();

pub fn mark_launch() {
	let _ = LAUNCHED.set(std::time::Instant::now());
}

// Zero until the mark is set, which only something that skips main can see.
// A second mark does not restart the clock.
pub fn uptime() -> std::time::Duration {
	LAUNCHED
		.get()
		.map_or(std::time::Duration::ZERO, std::time::Instant::elapsed)
}

// The display scale factor to lay out at, given what the window reports.
// SILK_SCALE overrides it, which is the only way to see a high-DPI layout on a
// 1x display: chrome written in raw pixels is INVISIBLE at 1x and only thins out
// as the factor rises, so the defect it guards against cannot be looked at
// without one. Read once (var_os takes the env lock and scans environ), same
// pattern as SILK_MAX_FPS. Off X11 there is no winit knob for this at all.
pub fn display_scale(reported: f64) -> f32 {
	use std::sync::OnceLock;
	static OVERRIDE: OnceLock<Option<f32>> = OnceLock::new();
	let over = OVERRIDE.get_or_init(|| {
		std::env::var("SILK_SCALE")
			.ok()
			.and_then(|raw| raw.trim().parse::<f32>().ok())
			.filter(|s| *s > 0.0 && s.is_finite())
	});
	over.unwrap_or(reported as f32)
}

// Chrome measurements are written in DIP (a CSS pixel, 1/96 inch) and converted
// to physical pixels where they are used. The main window's chrome shares a
// coordinate space with the terminal grid, so there is no single boundary to
// divide at the way settings_ui.rs has - each measurement scales at its own use
// site, through here or `TextCtx::dip`.
//
// Rounded to whole pixels so a rule, a ring or a hairline gap stays crisp, and a
// measurement the author asked to be visible never rounds away to nothing (a 1
// DIP gap under a scale factor below 1 would otherwise vanish).
pub fn dip(v: f32, scale: f32) -> f32 {
	let px = v * scale;
	if v > 0.0 {
		px.round().max(1.0)
	} else {
		px.round()
	}
}

// internal, not user-tunable (yet); DIP, see `dip`
pub const PANE_GAP_PX: f32 = 1.0;
pub const DIVIDER_GRAB_PX: f32 = 5.0; // mouse tolerance for grabbing a pane divider
pub const FOCUS_RING_PX: f32 = 2.0;
pub const SETTLE_EPS: f32 = 0.002; // a settle threshold, not a measurement - never scaled
// Ceiling on text.dark_on_light. 1.0 is the sRGB blend the font was drawn for;
// the headroom above it is for taste, and past this the counters of small
// letters fill in.
pub const MAX_DARK_ON_LIGHT: f32 = 2.0;

pub const DIVIDER: [u8; 3] = [0x2c, 0x2c, 0x36];

// text-selection highlight
pub const SELECTION_BG: [u8; 3] = [0x33, 0x44, 0x66];

// drag-and-drop pane reorder: drop-target tint
pub const DROP_TARGET: [u8; 3] = [0x55, 0x80, 0xc8];

// Scrollbar. Neutral mid-gray in every theme rather than a palette color: desktop
// scrollbars read as chrome, not as part of the terminal's own color scheme. There
// is no portable way to ask the OS for its actual value (GTK only names a theme),
// so this is the shade those themes converge on. colors.scrollbar_* overrides.
pub const SCROLLBAR_THUMB_DEF: [u8; 3] = [0x8a, 0x8a, 0x92];
pub const SCROLLBAR_TROUGH_DEF: [u8; 3] = [0x2e, 0x2e, 0x36];
// Opacity the bar settles at, and what it rises to while hovered or dragged.
pub const SCROLLBAR_IDLE_A: f32 = 0.55;
pub const SCROLLBAR_ACTIVE_A: f32 = 0.95;
// The trough is a faint backing strip, well under the thumb.
pub const SCROLLBAR_TROUGH_A: f32 = 0.34;

// tab bar
pub const TAB_BAR_BG: [u8; 3] = [0x2c, 0x2c, 0x31];
pub const TAB_ACTIVE: [u8; 3] = [0x47, 0x47, 0x4f];
pub const TAB_INACTIVE: [u8; 3] = [0x36, 0x36, 0x3b];

// Used only when the system monospace size can't be read (see default_font_size).
const FALLBACK_FONT_SIZE: f32 = 17.0;

// Cross-platform monospace fallback stack (first installed wins): the
// font_family default, and the resolver's last resort on every platform when
// neither the configured family nor the OS monospace resolves. Windows always
// goes through it (no OS monospace setting exists there), so every entry must
// carry a real bold face - the bare Family::Monospace db query this replaces
// could pick a family without one, silently ejecting bold runs to an
// arbitrary (often proportional) fallback.
pub const DEFAULT_FONT_STACK: &str = "Monaspace Argon, Fira Code, JetBrains Mono, Cascadia Mono, Consolas, Ubuntu Mono, SF Mono, Menlo, Courier New";

// Stacks that shipped as the font_family default in an earlier version. Backfill
// only ever adds a missing key, so a config written back then still carries its
// stack forever; migration rewrites one to the current default. Matched whole
// and exactly, so anything the user has actually edited is left alone. Append
// the outgoing value here whenever DEFAULT_FONT_STACK changes.
const SUPERSEDED_FONT_STACKS: &[&str] = &[
	"JetBrains Mono, Fira Code, Cascadia Code, DejaVu Sans Mono, Menlo, Consolas, Liberation Mono, monospace",
];

// right-click context menu
pub const MENU_LINK: [u8; 3] = [0x6c, 0x9c, 0xff]; // clickable URL

// Menu bar / dropdown colors: bg + text come from the active theme (overridable
// via colors.menu_background/menu_foreground); hover, border, and the group
// separator are derived shades of the bg, so a custom menu color stays coherent
// in either a dark or a light direction.
pub fn menu_bg() -> [u8; 3] {
	settings().menu_bg
}
pub fn menu_fg() -> [u8; 3] {
	settings().menu_fg
}
pub fn menu_hover() -> [u8; 3] {
	shade(menu_bg(), 22)
}
pub fn menu_border() -> [u8; 3] {
	shade(menu_bg(), 34)
}
pub fn menu_sep() -> [u8; 3] {
	shade(menu_bg(), 20)
}
// Flyover help in the main window: the tab strip's tip and a menu row's. Both
// hang off chrome that is painted in the menu color - the shipped menu bg is the
// inactive tab's own bytes - so a tip filled with it reads as the thing it is
// explaining. It lifts by the same step the strip puts between an inactive and
// an active tab, and warms, since every tab color leans faintly blue. The
// dialogs' tips already stand off their panel this way, with dialog_btn().
// Each derivation is a pure function of the menu color it comes from, so a test
// can ask what a given menu color yields without the live user config deciding
// the answer (see the config lock the settings tests take).
const TIP_LIFT: i16 = 34;
const TIP_WARMTH: i16 = 8;
const TIP_TEXT_WARMTH: i16 = 5;
pub fn tip_bg() -> [u8; 3] {
	tip_bg_of(menu_bg())
}
pub fn tip_border() -> [u8; 3] {
	tip_border_of(menu_bg())
}
pub fn tip_fg() -> [u8; 3] {
	tip_fg_of(menu_fg())
}
pub(crate) fn tip_bg_of(menu_bg: [u8; 3]) -> [u8; 3] {
	warm(shade(menu_bg, TIP_LIFT), TIP_WARMTH)
}
pub(crate) fn tip_border_of(menu_bg: [u8; 3]) -> [u8; 3] {
	shade(tip_bg_of(menu_bg), 34)
}
pub(crate) fn tip_fg_of(menu_fg: [u8; 3]) -> [u8; 3] {
	warm(menu_fg, TIP_TEXT_WARMTH)
}
// Tilt a color toward the warm end: red up, blue down, green where it was. The
// lightness barely moves, so a contrast check reads about the same either side.
fn warm(color: [u8; 3], magnitude: i16) -> [u8; 3] {
	let step = |channel: u8, delta: i16| (channel as i16 + delta).clamp(0, 255) as u8;
	[
		step(color[0], magnitude),
		color[1],
		step(color[2], -magnitude),
	]
}
// Nudge a color toward more contrast: lighten a dark base, darken a light one.
fn shade(color: [u8; 3], magnitude: i16) -> [u8; 3] {
	let luminance = (color[0] as i16 * 30 + color[1] as i16 * 59 + color[2] as i16 * 11) / 100;
	let delta = if luminance < 128 {
		magnitude
	} else {
		-magnitude
	};
	let adjust = |channel: u8| (channel as i16 + delta).clamp(0, 255) as u8;
	[adjust(color[0]), adjust(color[1]), adjust(color[2])]
}
// Dropdown/context-menu geometry, DIP (see `dip`). The pop-out dialogs lay out
// in DIP throughout and use these raw; the main window's menus convert at each
// use site.
pub const MENU_PAD_X: f32 = 12.0;
pub const MENU_ITEM_PAD_Y: f32 = 6.0;
pub const MENU_SEP_H: f32 = 9.0; // height of a separator row (line + spacing)
pub const MENU_GUTTER: f32 = 20.0; // left checkmark gutter; item text starts after it
pub const MENU_SUB_ARROW: f32 = 14.0; // right column a submenu row draws its arrow in

// How a background image fills the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
	Zoom,    // cover: fill, preserve aspect, crop overflow
	Stretch, // fill exactly, ignore aspect
}

// The window size and font zoom kept for one monitor, named by
// `monitor::MonitorId::key`. The zoom is px on the font size, as
// `font_zoom_px` has it.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorSize {
	pub key: String,
	pub columns: usize,
	pub rows: usize,
	pub font_zoom: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeptWindow {
	pub columns: usize,
	pub rows: usize,
	pub font_zoom: i32,
}

// What to open at, or to take on arriving at another monitor, while
// remember_size is on: the monitor's own when it has one, else the last the
// window was given anywhere.
pub fn remembered_window(s: &Settings, monitor: Option<&str>) -> KeptWindow {
	let last = KeptWindow {
		columns: s.remembered_columns,
		rows: s.remembered_rows,
		font_zoom: s.remembered_font_zoom,
	};
	monitor
		.filter(|_| s.remember_per_monitor)
		.and_then(|key| s.monitor_sizes.iter().find(|m| m.key == key))
		.map_or(last, |m| KeptWindow {
			columns: m.columns,
			rows: m.rows,
			font_zoom: m.font_zoom,
		})
}

// Note a size or a font zoom the user gave the window. It is the last one
// anywhere, and the monitor's own when they are kept per monitor. A monitor
// seen for the first time starts from what it would have opened at.
pub fn remember_window(
	s: &mut Settings,
	monitor: Option<&str>,
	grid: Option<(usize, usize)>,
	font_zoom: Option<i32>,
) {
	if let Some((columns, rows)) = grid {
		s.remembered_columns = columns;
		s.remembered_rows = rows;
	}
	if let Some(zoom) = font_zoom {
		s.remembered_font_zoom = zoom;
	}
	let Some(key) = monitor.filter(|_| s.remember_size && s.remember_per_monitor) else {
		return;
	};
	if grid.is_none() && font_zoom.is_none() {
		return;
	}
	if !s.monitor_sizes.iter().any(|m| m.key == key) {
		s.monitor_sizes.push(MonitorSize {
			key: key.to_string(),
			columns: s.remembered_columns,
			rows: s.remembered_rows,
			font_zoom: s.remembered_font_zoom,
		});
	}
	let Some(entry) = s.monitor_sizes.iter_mut().find(|m| m.key == key) else {
		return;
	};
	if let Some((columns, rows)) = grid {
		entry.columns = columns;
		entry.rows = rows;
	}
	if let Some(zoom) = font_zoom {
		entry.font_zoom = zoom;
	}
}

// The window sizes in the file now, put into the live settings. Every
// window is its own process, so another may have kept a size since this one
// loaded. Nothing is written.
pub fn refresh_window_memory() {
	let Some(path) = config_path() else {
		return;
	};
	let Some(text) = read_settings_text(&path) else {
		return;
	};
	let mut live = (*settings()).clone();
	window_memory_from(&loaded_text(&text), &path, &mut live);
	update(live);
}

fn window_memory_from(text: &str, path: &std::path::Path, s: &mut Settings) {
	let r = Reader {
		doc: shcl::Document::parse(text),
		path,
		said: std::cell::RefCell::new(Vec::new()),
	};
	let d = Settings::default();
	s.remembered_columns = numi(
		r.u("window.remembered_columns"),
		d.remembered_columns,
		limits::GRID,
	);
	s.remembered_rows = numi(
		r.u("window.remembered_rows"),
		d.remembered_rows,
		limits::GRID,
	);
	s.remembered_font_zoom = zoom(r.i("window.remembered_font_zoom"), d.remembered_font_zoom);
	s.monitor_sizes = read_monitor_sizes(&r);
}

// Resolved, validated settings used throughout the app. PartialEq is for the
// template test, which loads the shipped config twice and compares the whole
// result; anything less would miss whichever field a bad `## Default` moved.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
	pub use_system_font: bool, // true = OS monospace FAMILY, overriding font_family
	pub use_system_font_size: bool, // true = OS monospace SIZE, overriding font_size
	pub font_family: Option<String>, // comma-separated fallback stack (first installed wins)
	pub font_size: f32,
	pub line_height_scale: f32,
	pub scrollback: usize,
	pub scroll_smooth: bool, // master switch: false = every scroll (wheel, output, app slide) happens instantly
	// The five knobs below are the named segments of the output-scroll speed
	// curve, in the order one burst traverses them: leave rest, accelerate,
	// top out, wind down, stop. Each hands its end point to the next and has
	// no other influence on it (scroll.rs holds the model).
	pub scroll_ease_in_ms: f32, // how long the lift from rest to the ramp handoff takes
	pub scroll_ramp_up_ms: f32, // catch-up speed doubles this often while output stays ahead
	pub scroll_single_screen_tau_ms: f32, // burst speed ceiling while the burst is still wholly on screen
	pub scroll_ramp_down_ms: f32,         // catch-up speed halves this often winding down to the stop
	pub scroll_ease_out_ms: f32,          // how long the last STOP_BAND of a line takes to stop
	pub wheel_lines: f32,
	pub alt_scroll_lines: f32,
	pub output_ease_lines: f32,
	pub smooth_scroll_apps: bool, // ease the line-jumps of full-screen / repaint apps (less/vim/nano; ConPTY TUIs that scroll above a fixed input line)
	pub scrollbar: bool,          // draw a scrollbar over each pane's right edge
	pub scrollbar_thickness: f32, // scrollbar width in logical px
	pub scrollbar_auto_hide: bool, // fade the scrollbar out while idle at the bottom
	pub minimap: bool,            // miniature of the whole buffer in its own column
	pub minimap_width: f32,       // the column's width in logical px
	pub minimap_tui_whitelist: String, // programs that keep the column on their own screen
	pub margin: f32,              // logical px between content and pane edge
	pub opacity: f32,             // background opacity 0..1 (1 = fully opaque)
	pub transparent_background: bool, // per-pixel bg transparency (text stays opaque): GL surface on X11, composited DX12 on Windows
	pub transparent_background_blur: bool, // X11: ask a KWin/picom compositor to blur the desktop behind the window
	pub wallpaper_enabled: bool,           // master switch: false = no wallpaper at all
	pub wallpaper: Option<PathBuf>,        // resolved path, or None
	pub wallpaper_raw: String, // the value as configured ("" = auto-detect); what the dialog shows
	pub wallpaper_fallback_builtin: bool, // no image/folder configured: show the built-in one
	pub wallpaper_rotate_enabled: bool, // master switch for folder rotation
	pub wallpaper_folder: Option<PathBuf>, // rotate the wallpaper through this folder's images (overrides wallpaper)
	pub wallpaper_folder_raw: String, // the folder as configured; WALLPAPER_DIR_TOKEN is the usual place
	pub wallpaper_folder_auto: bool,  // the folder above was found by convention, not configured
	pub wallpaper_rotate_random: bool, // rotate randomly instead of in filename order
	pub wallpaper_rotate_interval_s: f32, // seconds between rotations (0 = pick one at startup only)
	pub wallpaper_opacity: f32,       // image visibility 0..1
	pub wallpaper_even: f32, // hold every picture to the same visibility whatever its own brightness, 0..1
	pub wallpaper_default_fit: Fit, // used unless the image's own tags say otherwise
	pub wallpaper_honor_xmp: bool, // let a wallpaper's own Fit/Anchor tags win
	pub wallpaper_honor_xmp_look: bool, // and its Opacity/Blur tags, which replace the two settings below
	pub wallpaper_blur: f32,            // Gaussian blur sigma applied to the image (0 = none)
	pub wallpaper_contrast_mask: bool,  // flatten the image's contrast so it stops competing with text
	pub wallpaper_contrast_mask_size: f32, // flatten scale 0..1 (1 = half the longest pixel dim)
	pub wallpaper_contrast_mask_strength: f32, // how far toward the local mean 0..1
	pub wallpaper_contrast_mask_auto: f32, // blend manual knobs with image-derived auto 0..1 (1 = full auto)
	pub text_scrim: bool, // bg-colored blurry halo behind glyphs (readability over busy/transparent bg)
	pub text_scrim_radius: f32, // scrim blur sigma in px
	pub text_scrim_softness: f32, // 0 = hard/solid scrim, 1 = soft/faint (maps to the intensity boost)
	pub text_scrim_strength: f32, // 0..100% -> 0..5 doublings of the halo alpha (0 = as built)
	pub text_outline: f32, // antialiased outline around glyphs, px (0 = none; scrim color rules)
	pub text_dark_on_light: f32, // how much of the sRGB-blend correction dark-on-light text gets, 0..2 (0 = off, 1 = the blend)
	pub text_scrim_ramp: String, // halo falloff curve: "sigmoid" | "half_normal" | "linear" | "log" | "exp"
	pub text_scrim_function: String, // halo build: "dilate" | "sdf" | "dt" | "gaussian" (legacy blur)
	pub text_scrim_regular_weight: bool, // blur bold text at regular weight (uniform halo; crisp text keeps its weight)
	pub text_min_contrast: f32, // smallest Oklab lightness gap text may have from its own background, 0..1 (0 = leave every color alone)
	pub color_emoji: bool, // paint COLRv1 color glyphs (emoji) instead of falling back to a monochrome face
	pub embolden_inverse: bool, // render reverse-video (dark-on-light) text bold so it reads as strongly as normal text (the scrim only boosts light-on-dark)
	pub cursor_scrim: bool,     // cursor joins the text scrim halo (default off)
	pub cursor_outline: bool,   // cursor joins the text outline (default on)
	pub cursor_size_height: f32, // cursor height, 1..100% of the cell (from the bottom)
	pub cursor_size_width: f32, // cursor width, 1..100% of the cell (from the left)
	pub cursor_animation: String, // "none" | "phase" | "pulse_vertical" | "pulse_horizontal" | "pulse_both"
	pub cursor_animation_resume_s: f32, // idle seconds after typing before the animation resumes (output does not wait this out)
	pub cursor_animation_idle_stop_s: f32, // idle seconds until the animation stops (parked at full); 0 = never
	pub cursor_blink_rate_ms: f32,         // one animation cycle (ms)
	pub columns: usize,                    // initial window grid size (used when !remember_size)
	pub rows: usize,
	pub remember_size: bool, // launch at the last window size instead of columns/rows
	pub remember_per_monitor: bool, // ...and keep one for each monitor (monitor_sizes)
	pub remember_maximized: bool, // launch maximized if the last window closed that way
	pub hide_single_tab: bool, // hide the tab bar while only one tab is open
	pub tab_shows_title: bool, // let a program's own title name the tab (tabtitle::Parts)
	pub tab_shows_shell: bool, // parts a tab's own text is made of
	pub tab_shows_program: bool,
	pub tab_shows_directory: bool,
	pub title_shows_tab: bool, // let the window title fall back to what the tab says
	pub idle_release: bool,    // let the GPU device go after a long idle (app.rs, release_gpu)
	pub idle_release_minimized_min: usize, // ...after this long minimized
	pub idle_release_hidden_min: usize, // ...or this long covered
	pub idle_release_min: usize, // ...or this long merely unfocused and quiet
	pub software_rendering: bool, // draw on the CPU even with a graphics card (gfx::wanted)
	pub tab_regular_pct: f32,  // a tab's ordinary width, as a % of the window's width
	pub tab_max_pct: f32,      // widest a tab may be, as a % of the window's width
	pub tab_tip_max_s: f32,    // longest a tab's tip stays up; 0 = until the pointer leaves
	pub remembered_columns: usize, // last actual window size (not shown in the dialog)
	pub remembered_rows: usize,
	pub remembered_maximized: bool, // was the window last left maximized
	pub remembered_font_zoom: i32,  // last font zoom, px on the font size
	pub monitor_sizes: Vec<MonitorSize>, // window.monitors, in file order; file only
	pub word_separators: String,    // delimiters for double-click word selection
	pub selection_pairs: String,    // matched pairs a double-click selects inside of
	pub command_line: String,       // default CLI layout/options when launched with no args
	pub startup_directory: String,  // where a shell starts when nothing else said (see startup_dir)
	pub copy_on_select: bool,       // panes start with copy-on-select enabled
	pub shell_integration: bool,    // put the directory-reporting block in PowerShell profiles
	pub bash_prompt: bool,          // give bash panes the x9ps1-git prompt (see integration.rs)
	pub hyperlinks: bool,           // underline URLs in output on hover; Ctrl+click opens them
	pub hyperlink_open_command: String, // opener for a clicked link (empty = the desktop's own)
	pub bg: [u8; 3],
	pub fg: [u8; 3],
	pub cursor: [u8; 3],
	// Take `fg` and `cursor` from the wallpaper instead (autotheme.rs). While it
	// is on those two hold the derived colors and the user's own sit in
	// `wallpaper_colors`, the same arrangement `profile_shadow` uses.
	pub colors_from_wallpaper: bool,
	// Two attention colors (see theme.rs): `highlight` marks several things at
	// once, `focus` marks only what the keyboard is on.
	pub highlight: [u8; 3],
	pub focus: [u8; 3],
	// chrome colors (menu bar / dropdowns, and pop-out dialogs), from the theme
	// palette; colors.menu_*/colors.dialog_* keys override
	pub menu_bg: [u8; 3],
	pub menu_fg: [u8; 3],
	pub dialog_bg: [u8; 3],
	pub dialog_fg: [u8; 3],
	pub gutter: [u8; 3], // chrome areas holding no control (the dialog's tab strip)
	// scrollbar, neutral in every theme (see SCROLLBAR_THUMB_DEF); the
	// colors.scrollbar_* keys override
	pub scrollbar_thumb: [u8; 3],
	pub scrollbar_trough: [u8; 3],
	pub ansi: [[u8; 3]; 16], // 16-color ANSI palette, resolved from the active theme
	pub theme: String,       // active theme name (see theme.rs)
	pub theme_mode: String,  // "dark" | "light" | "system"
	// The performance profile (profile.rs): what the look may cost. While one
	// is live the fields it governs hold ITS values and the user's own sit in
	// `profile_shadow`, which is how Custom puts them back.
	pub performance_automatic: bool, // pick the profile for this machine, and step it down when the display cannot keep up
	pub performance_profile: String, // "custom" | "max" | "high" | "low" | "standard"
	pub performance_check_hardware: bool, // re-rate when the machine underneath changes
	pub performance_check_next_run: bool, // re-rate once at the next launch, then clear
	pub rated_hardware: String,      // hardware id the profile was last picked for ("" = never)
	pub profile_shadow: Option<Box<crate::profile::Shadow>>,
	// The Remote profile in force, over whatever `performance_profile` says. Never
	// written: it is set for a remote screen (or by hand from the View menu) and
	// lasts the session.
	pub remote_override: bool,
	// Where the display watch stepped the profile down to, for this session only.
	// Never written: one stall used to become every later launch's profile, with
	// no way back while automatic was on. Cleared by a hand pick or a measured one.
	pub stepped_profile: Option<crate::profile::Profile>,
	// What the wallpaper on screen is worth to the derivation, and the user's own
	// text and cursor while the derived pair is live. Neither is ever written:
	// the summary comes from whatever picture arrived, and a rotation replaces it.
	pub wallpaper_summary: Option<crate::autotheme::Summary>,
	pub wallpaper_colors: Option<crate::autotheme::Shadow>,
	// Themes saved from the Settings dialog, whole, in file order. They resolve
	// ahead of the built-ins, so one may carry a built-in's name.
	pub user_themes: Vec<crate::theme::UserTheme>,
	// The shells the Tabs menu offers, in file order. Written by the background
	// scan (shells.rs) and by hand; see `write_shells` for what a scan may touch.
	pub shells: Vec<crate::shells::ShellEntry>,
	// The hotkeys in force: the defaults with the file's `keys.*` values put in.
	// The key handler and every menu read these, so a rebinding shows up in all.
	pub keys: crate::keys::Bindings,
	#[cfg(test)]
	pub clone_probe: CloneProbe,
}

// Counts whole copies of `Settings` made on this thread, so a test can hold a
// hot path to a number. Test builds only.
#[cfg(test)]
thread_local! {
	static SETTINGS_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
#[derive(Debug, Default)]
pub struct CloneProbe;
#[cfg(test)]
impl Clone for CloneProbe {
	fn clone(&self) -> Self {
		SETTINGS_CLONES.with(|n| n.set(n.get() + 1));
		CloneProbe
	}
}
#[cfg(test)]
impl PartialEq for CloneProbe {
	fn eq(&self, _: &Self) -> bool {
		true
	}
}
#[cfg(test)]
pub fn settings_clones() -> usize {
	SETTINGS_CLONES.with(std::cell::Cell::get)
}

impl Settings {
	// The rotation folder, or None when either master switch is off. Both callers
	// (arming rotation, and the built-in fallback's "nothing is configured" test)
	// go through this so they cannot disagree - and it is derived rather than
	// folded into `wallpaper_folder` at load, because the Settings dialog edits a
	// Settings struct directly and never re-runs `resolve`.
	pub fn rotation_folder(&self) -> Option<&PathBuf> {
		(self.wallpaper_enabled && self.wallpaper_rotate_enabled)
			.then_some(self.wallpaper_folder.as_ref())
			.flatten()
	}

	// The app-slide gate, derived for the same reason: the smooth-scroll master
	// covers every scroll animation, so every smooth_scroll_apps consumer reads
	// this instead of the raw flag and cannot miss the master.
	pub fn smooth_apps(&self) -> bool {
		self.scroll_smooth && self.smooth_scroll_apps
	}
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			use_system_font: true,
			use_system_font_size: true,
			font_family: Some(DEFAULT_FONT_STACK.to_string()),
			font_size: FALLBACK_FONT_SIZE,
			line_height_scale: 1.22,
			scrollback: 10_000,
			scroll_smooth: true,
			scroll_ease_in_ms: 82.0, // ~ "Ease-in" 50 (motion builds over the first ~tenth of a second)
			scroll_ramp_up_ms: 96.0, // ~ "Ramp-up" 75 (catch-up speed doubles ~10x a second)
			scroll_single_screen_tau_ms: 32.0, // ~ "Single-screen speed" 75 (on-screen burst ceiling: ~31 lines/s)
			scroll_ramp_down_ms: 144.0, // ~ "Ramp-down" 75 (catch-up winds down by halving ~7x a second)
			scroll_ease_out_ms: 212.0,  // ~ "Ease-out" 40 (the tail finishes in ~a fifth of a second)
			wheel_lines: 3.0,
			alt_scroll_lines: 3.0,
			output_ease_lines: 1.0,
			smooth_scroll_apps: true,
			scrollbar: true,
			scrollbar_thickness: 16.0,
			scrollbar_auto_hide: true,
			minimap: true,
			minimap_width: 100.0,
			minimap_tui_whitelist: "less tmux screen".to_string(),
			margin: 8.0,
			opacity: 0.95,
			transparent_background: false,
			transparent_background_blur: false,
			wallpaper: None,
			wallpaper_enabled: true,
			wallpaper_raw: String::new(),
			wallpaper_fallback_builtin: true,
			wallpaper_rotate_enabled: true,
			wallpaper_folder: None,
			wallpaper_folder_raw: WALLPAPER_DIR_TOKEN.to_string(),
			wallpaper_folder_auto: false,
			wallpaper_rotate_random: true,
			wallpaper_rotate_interval_s: 0.0,
			wallpaper_opacity: 0.10, // image visibility relative to bg color
			wallpaper_even: 1.0,
			wallpaper_default_fit: Fit::Stretch,
			wallpaper_honor_xmp: true,
			wallpaper_honor_xmp_look: true,
			wallpaper_blur: 10.0,
			wallpaper_contrast_mask: true,
			wallpaper_contrast_mask_size: 0.5,
			wallpaper_contrast_mask_strength: 0.5,
			wallpaper_contrast_mask_auto: 0.5,
			text_scrim: true,
			text_scrim_radius: 8.0,
			text_scrim_softness: 0.5,
			text_scrim_strength: 20.0,
			text_outline: 1.0,
			text_dark_on_light: 1.0,
			text_scrim_ramp: "exp".to_string(),
			text_scrim_function: "sdf".to_string(),
			text_scrim_regular_weight: true,
			text_min_contrast: 0.45,
			color_emoji: true,
			embolden_inverse: true,
			cursor_scrim: false,
			cursor_outline: true,
			cursor_size_height: 100.0, // full height
			cursor_size_width: 100.0,  // full width - a block
			cursor_animation: "pulse_vertical".to_string(),
			cursor_animation_resume_s: 1.0,
			cursor_animation_idle_stop_s: 60.0,
			cursor_blink_rate_ms: 500.0,
			columns: 160,
			rows: 48,
			remember_size: true,
			remember_per_monitor: true,
			remember_maximized: false,
			hide_single_tab: false,
			tab_shows_shell: true,
			tab_shows_program: true,
			tab_shows_title: true,
			tab_shows_directory: true,
			title_shows_tab: true,
			idle_release: true,
			idle_release_minimized_min: 1,
			idle_release_hidden_min: 30,
			idle_release_min: 240,
			software_rendering: false,
			tab_regular_pct: 10.0,
			tab_max_pct: 100.0,
			tab_tip_max_s: 30.0,
			remembered_columns: 160,
			remembered_rows: 48,
			remembered_maximized: false,
			remembered_font_zoom: 0,
			monitor_sizes: Vec::new(),
			// alacritty's default delimiters minus ':', so a Windows drive path
			// (C:\...) stays whole on a double-click - and namespaced idents
			// (std::vec) and URLs (http://) with it. /.-_~ are already word chars.
			word_separators: alacritty_terminal::term::SEMANTIC_ESCAPE_CHARS
				.chars()
				.filter(|&c| c != ':')
				.collect(),
			selection_pairs: DEFAULT_SELECTION_PAIRS.to_owned(),
			command_line: String::new(),
			startup_directory: HOME_TOKEN.to_string(),
			copy_on_select: true,
			shell_integration: true,
			bash_prompt: false,
			hyperlinks: true,
			hyperlink_open_command: String::new(),
			bg: [0x00, 0x00, 0x00],
			fg: [0x88, 0xee, 0xcc],
			cursor: [0x8a, 0x3f, 0xa4],
			colors_from_wallpaper: true,
			highlight: [0xc8, 0xa0, 0x5a],
			focus: [0x40, 0x86, 0xff],
			menu_bg: crate::theme::MENU_BG_DEF,
			menu_fg: crate::theme::MENU_FG_DEF,
			dialog_bg: [0x20, 0x20, 0x2a],
			dialog_fg: [0xe2, 0xe2, 0xea],
			gutter: [0x16, 0x16, 0x1e],
			scrollbar_thumb: SCROLLBAR_THUMB_DEF,
			scrollbar_trough: SCROLLBAR_TROUGH_DEF,
			ansi: crate::theme::resolve("SilkTerm", "dark", true).ansi,
			theme: "SilkTerm".to_string(),
			theme_mode: "dark".to_string(),
			performance_automatic: true,
			performance_profile: "max".to_string(),
			performance_check_hardware: true,
			performance_check_next_run: false,
			rated_hardware: String::new(),
			profile_shadow: None,
			remote_override: false,
			stepped_profile: None,
			wallpaper_summary: None,
			wallpaper_colors: None,
			user_themes: Vec::new(),
			shells: Vec::new(),
			keys: crate::keys::Bindings::defaults(cfg!(target_os = "macos")),
			#[cfg(test)]
			clone_probe: CloneProbe,
		}
	}
}

fn store() -> &'static RwLock<Arc<Settings>> {
	static S: OnceLock<RwLock<Arc<Settings>>> = OnceLock::new();
	S.get_or_init(|| {
		let mut settings = load();
		crate::profile::apply(&mut settings);
		crate::autotheme::apply(&mut settings);
		RwLock::new(Arc::new(settings))
	})
}

// Live OS dark/light bit (winit `Window::theme()`), used only when theme_mode = "system".
static OS_DARK: AtomicBool = AtomicBool::new(true);

// The effective dark/light for the active mode (chrome + dialogs follow this).
pub fn is_dark() -> bool {
	match settings().theme_mode.as_str() {
		"light" => false,
		"system" => OS_DARK.load(Ordering::Relaxed),
		_ => true,
	}
}

// The OS bit on its own, for the callers that answer from a settings copy rather
// than from the live store.
pub fn os_dark() -> bool {
	OS_DARK.load(Ordering::Relaxed)
}

// On an OS dark/light change (System mode only): recompute the theme palette and
// swap it in (no file write). Returns true if anything changed (caller redraws).
pub fn reapply_for_os(dark: bool) -> bool {
	let prev = OS_DARK.swap(dark, Ordering::Relaxed);
	let current = settings();
	if prev == dark || current.theme_mode != "system" {
		return false;
	}
	let palette = |dark| {
		crate::theme::resolve_in(
			&current.user_themes,
			&current.theme,
			&current.theme_mode,
			dark,
		)
	};
	let (was, pal) = (palette(prev), palette(dark));
	// A color that is not the theme's own is an override, from the file, the
	// command line or the dialog, so it stays put.
	let follow = |live: &mut [u8; 3], was: [u8; 3], now: [u8; 3]| {
		if *live == was {
			*live = now;
		}
	};
	let mut new = (*current).clone();
	follow(&mut new.bg, was.bg, pal.bg);
	follow(&mut new.fg, was.fg, pal.fg);
	follow(&mut new.cursor, was.cursor, pal.cursor);
	follow(&mut new.highlight, was.highlight, pal.highlight);
	follow(&mut new.focus, was.focus, pal.focus);
	follow(&mut new.menu_bg, was.menu_bg, pal.menu_bg);
	follow(&mut new.menu_fg, was.menu_fg, pal.menu_fg);
	follow(&mut new.dialog_bg, was.dialog_bg, pal.dialog_bg);
	follow(&mut new.dialog_fg, was.dialog_fg, pal.dialog_fg);
	follow(&mut new.gutter, was.gutter, pal.gutter);
	new.ansi = pal.ansi;
	update(new);
	true
}

// Current settings snapshot. Cheap to call (an Arc clone); the settings dialog
// can swap the whole thing at runtime via `update`. Callers in hot paths should
// snapshot once per frame rather than per cell.
pub fn settings() -> Arc<Settings> {
	crate::locks::read(store()).clone()
}

// Default double-click inclusion pairs, in precedence order (highest first):
// backticks, double quotes, single quotes, then {} () [] <>.
pub const DEFAULT_SELECTION_PAIRS: &str = "`` \"\" '' {} () [] <>";

// argv for the default shell - the first ACTIVE entry in the stored list, which
// is what "the one at the top" means. None hands the choice to the system: an
// empty list, or one whose every entry is switched off.
//
// The order is the user's (the Settings dialog's Shells tab moves entries; a
// scan only ever appends), and an initial population is led by the shell the
// user actually logs in with - see `shells::detect`.
pub fn default_shell_argv() -> Option<Vec<String>> {
	let shell = settings()
		.shells
		.iter()
		.find(|entry| entry.active)
		.map(|entry| entry.command.clone())?;
	command_argv(&shell)
}

// Where the FIRST pane of a freshly launched window starts, or None to leave it
// where SilkTerm itself was started. Four things can decide a shell's directory
// and this is the last of them: `--directory` on the command line (cli_dir, the
// caller's first choice), a new tab, pane or window inheriting from the pane it
// came from (handled by the caller, which passes an inherited path instead of
// asking here), an inherited directory that somebody picked on purpose, and only
// what is left over reads the setting.
//
// So the setting is what a launch from the desktop, a menu or a shortcut gets -
// which is the case where the inherited directory is an accident of whoever
// started us rather than anything the user chose.
pub fn startup_dir() -> Option<std::path::PathBuf> {
	if inherited_dir_is_a_choice() {
		return None;
	}
	resolve_dir(&settings().startup_directory, "shell.startup_directory")
}

// Is the directory we were started in a choice or an accident? A shell that
// launched us was sitting somewhere on purpose, and so is a file manager's
// "Open in terminal", which hands us the folder being looked at without giving
// us a terminal. What is left - a desktop icon, a Start-menu entry, a shortcut -
// starts in the home directory, at a filesystem root, beside the executable, or
// in the Windows system folder, and none of those say anything about where the
// user wants to be.
fn inherited_dir_is_a_choice() -> bool {
	if DIR_HANDED_DOWN.load(Ordering::Relaxed) || launched_from_shell() {
		return true;
	}
	let exe_dir = std::env::current_exe()
		.ok()
		.and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
	dir_is_a_choice(
		std::env::current_dir().ok().as_deref(),
		home_dir().as_deref(),
		exe_dir.as_deref(),
		&system_dirs(),
	)
}

// A packaged app's Start-menu entry can't name a working directory, so every
// launch from one starts in System32, or SysWOW64 for a 32-bit build.
fn system_dirs() -> Vec<std::path::PathBuf> {
	if !cfg!(windows) {
		return Vec::new();
	}
	let Some(root) = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")) else {
		return Vec::new();
	};
	let root = std::path::PathBuf::from(root);
	vec![root.join("System32"), root.join("SysWOW64")]
}

// The decision on its own, so both platforms' answers are testable from either
// box. Compared through `canonicalize` where it works, since $HOME is routinely
// spelled differently from what getcwd hands back.
fn dir_is_a_choice(
	cwd: Option<&std::path::Path>,
	home: Option<&std::path::Path>,
	exe_dir: Option<&std::path::Path>,
	system: &[std::path::PathBuf],
) -> bool {
	let Some(cwd) = cwd else {
		return false;
	};
	if cwd.parent().is_none() {
		return false; // a filesystem root is where a launcher leaves us, not a choice
	}
	let real = |dir: &std::path::Path| dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
	let cwd = real(cwd);
	![home, exe_dir]
		.into_iter()
		.flatten()
		.chain(system.iter().map(std::path::PathBuf::as_path))
		.any(|dir| real(dir) == cwd)
}

// A window opened from another one's pane is started in that pane's directory,
// which can be home or a root as easily as anywhere. Nothing in the directory
// itself says so, and a launcher leaves us in the same places, so the parent
// says it out loud. Read and dropped once at the top of main, so no shell of
// ours passes it on to a SilkTerm it starts.
pub const ENV_DIR_HANDED_DOWN: &str = "SILKTERM_DIR_HANDED_DOWN";
static DIR_HANDED_DOWN: AtomicBool = AtomicBool::new(false);

pub fn take_handed_down_dir() {
	if std::env::var_os(ENV_DIR_HANDED_DOWN).is_some() {
		// SAFETY: called from main before any thread exists, like
		// term::sanitize_shell_env.
		unsafe { std::env::remove_var(ENV_DIR_HANDED_DOWN) };
		DIR_HANDED_DOWN.store(true, Ordering::Relaxed);
	}
}

// The file `--config` named, if one did. A new window is given the same one.
pub fn config_override() -> Option<PathBuf> {
	CONFIG_OVERRIDE.get().cloned()
}

// Where a shell named on the command line starts (`--directory`). Sits ABOVE
// every one of startup_dir's three cases: asking for a directory on the command
// line is the most deliberate statement of the lot, so it beats an inherited
// one, the shell we were launched from, and the setting alike.
pub fn cli_dir(raw: &str) -> Option<std::path::PathBuf> {
	resolve_dir(raw, "--directory")
}

// Expand and check one directory, naming what asked for it when it isn't there.
// `label` is how the user spelled it, so the line points at the setting or the
// flag rather than at a path they may never have typed.
fn resolve_dir(raw: &str, label: &str) -> Option<std::path::PathBuf> {
	let wanted = raw.trim();
	if wanted.is_empty() {
		return None;
	}
	// Absolute before the check, so a drive-less spelling on Windows resolves
	let dir = std::path::PathBuf::from(expand_vars(wanted));
	let dir = std::path::absolute(&dir).unwrap_or(dir);
	if dir.is_dir() {
		return Some(dir);
	}
	// Worth one line: a directory that has been renamed or unmounted would
	// otherwise look like the setting being ignored.
	eprintln!("{APP_NAME}: {label}: no such directory: {wanted}");
	None
}

// Substitute environment variables written in any of the three spellings a
// person is likely to reach for - `$NAME` and `${NAME}` from bash, `%NAME%`
// from cmd, `$env:NAME` and `${env:NAME}` from PowerShell - plus a leading `~`.
// All of them on every platform on purpose: this is text SilkTerm reads, not
// something a shell ever sees, so which shell the person likes should not
// decide whether their config works. An unset name expands to nothing, the way
// a shell does it.
//
// What a shell DOES see never comes through here - `command_argv` expands the
// program name and leaves the arguments alone.
pub fn expand_vars(text: &str) -> String {
	let text = match text.strip_prefix('~') {
		// With no home to put there, leave the `~` standing rather than turning
		// `~/pics` into an absolute `/pics` that means something else entirely.
		Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
			let home = home_string();
			if home.is_empty() {
				text.to_string()
			} else {
				format!("{home}{rest}")
			}
		}
		_ => text.to_string(),
	};
	let mut out = String::with_capacity(text.len());
	let mut rest = text.as_str();
	while let Some(at) = rest.find(['$', '%']) {
		out.push_str(&rest[..at]);
		let tail = &rest[at..];
		let (name, after) = if let Some(braced) = tail.strip_prefix("${") {
			match braced.split_once('}') {
				Some((name, after)) => (name, after),
				None => break,
			}
		} else if let Some(percent) = tail.strip_prefix('%') {
			match percent.split_once('%') {
				// `%%` is an empty name, not a variable - leave it alone.
				Some((name, after)) if !name.is_empty() => (name, after),
				_ => {
					out.push_str(&tail[..1]);
					rest = &tail[1..];
					continue;
				}
			}
		} else {
			// PowerShell writes `$env:NAME`, where the colon belongs to the
			// spelling and not to the name. Stripped before the scan below,
			// which stops at the colon and would expand `$env` instead.
			let bare = strip_env_prefix(&tail[1..]);
			let end = bare
				.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
				.unwrap_or(bare.len());
			if end == 0 {
				out.push_str(&tail[..1]);
				rest = &tail[1..];
				continue;
			}
			(&bare[..end], &bare[end..])
		};
		out.push_str(&lookup(strip_env_prefix(name)).unwrap_or_default());
		rest = after;
	}
	out.push_str(rest);
	out
}

// `env:` off the front of a name, in whatever case it was written.
fn strip_env_prefix(name: &str) -> &str {
	// get_ rather than a slice: a name whose fourth byte falls inside a character
	// used to abort here
	match name.get(..4) {
		Some(head) if head.eq_ignore_ascii_case("env:") => &name[4..],
		_ => name,
	}
}

// One variable's value, answering the few names that mean the same thing under
// a different spelling on the other platform. Native Windows sets no HOME and
// unix sets no USERPROFILE, so a config written on either box would otherwise
// go quiet on the other. Only names with an honest one-to-one counterpart are
// listed - guessing at the rest would be worse than an empty expansion the
// user can see.
fn lookup(name: &str) -> Option<String> {
	const ALIASES: &[&[&str]] = &[
		&["HOME", "USERPROFILE"],
		&["USER", "USERNAME"],
		&["TMPDIR", "TEMP", "TMP"],
	];
	let read = |n: &str| {
		std::env::var_os(n)
			.filter(|v| !v.is_empty())
			.map(|v| v.to_string_lossy().into_owned())
	};
	if let Some(value) = read(name) {
		return Some(value);
	}
	let group = ALIASES
		.iter()
		.find(|group| group.iter().any(|alt| alt.eq_ignore_ascii_case(name)))?;
	group.iter().find_map(|alt| read(alt)).or_else(|| {
		// Home is the one we can still answer with nothing in the environment.
		group[0]
			.eq_ignore_ascii_case("HOME")
			.then(home_string)
			.filter(|home| !home.is_empty())
	})
}

// Split a command from the config into argv, expanding the program name and
// nothing after it. Splitting first is what keeps
// `%ProgramFiles%\PowerShell\7\pwsh.exe` one argument once the space in
// "Program Files" turns up.
//
// The arguments go through exactly as written, because the program being
// started is what reads them and it has its own rules: `cmd /k prompt $P$G`
// sets a cmd prompt, `bash -c 'echo $FOO'` wants bash's own `$FOO`, and
// substituting either here hands the program a word nobody typed. The program
// name is different only because nothing else would ever expand it.
pub fn command_argv(command: &str) -> Option<Vec<String>> {
	let mut argv = crate::cli::shell_split(command).ok()?;
	if let Some(program) = argv.first_mut() {
		*program = expand_vars(program);
	}
	Some(argv)
}

// The home directory as text, empty when the environment names none. Same
// answer `home_dir` gives the config-location logic, so a `~` here and a `~` in
// a path there can never disagree.
fn home_string() -> String {
	home_dir().map_or_else(String::new, |dir| dir.to_string_lossy().into_owned())
}

// Did a shell start us? If so its directory is a deliberate choice and outranks
// the setting; if not, the directory we inherited is whatever the launcher felt
// like and says nothing about where the user wants to be.
//
// The test is the same on both platforms - "is standard input a terminal" - and
// it is what separates the two cases without asking about parent processes,
// which costs a process-table walk on Windows and would sit on the path to the
// first frame. Measured on Windows: launched from a console the std handles are
// the console's (FILE_TYPE_CHAR), launched through ShellExecute the way Explorer
// and the Start menu do it they are NULL. A release build is a GUI-subsystem
// binary and owns no console either way, so GetConsoleWindow cannot answer this
// and AttachConsole answers it wrong (it succeeds via the grandparent).
#[cfg(unix)]
fn launched_from_shell() -> bool {
	// SAFETY: isatty only inspects the descriptor; 0 is always a valid argument.
	unsafe { libc::isatty(0) == 1 }
}

#[cfg(windows)]
fn launched_from_shell() -> bool {
	use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
	use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_CHAR, GetFileType};
	use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE};
	// SAFETY: both calls only read the process's own standard-handle table.
	unsafe {
		let handle = GetStdHandle(STD_INPUT_HANDLE);
		!handle.is_null() && handle != INVALID_HANDLE_VALUE && GetFileType(handle) == FILE_TYPE_CHAR
	}
}

#[cfg(not(any(unix, windows)))]
fn launched_from_shell() -> bool {
	true
}

// Parse `selection_pairs` into (open, close) char pairs, in precedence order.
pub fn selection_pairs() -> Vec<(char, char)> {
	parse_pairs(&settings().selection_pairs)
}

fn parse_pairs(text: &str) -> Vec<(char, char)> {
	text.split_whitespace()
		.filter_map(|pair| {
			let mut chars = pair.chars();
			Some((chars.next()?, chars.next()?))
		})
		.collect()
}

// Replace the live settings (used by the settings dialog's Apply/OK). The
// performance profile goes on here, so what `settings()` answers is what is
// drawn, and `persist` takes it back off before anything reaches the file.
pub fn update(mut new: Settings) {
	crate::profile::apply(&mut new);
	// After the profile: one that turns the wallpaper off leaves nothing to
	// derive from, and the derivation reads `wallpaper_enabled`.
	crate::autotheme::apply(&mut new);
	*crate::locks::write(store()) = Arc::new(new);
}

// Re-read config.shcl from disk (e.g. after the user edited it by hand). Returns
// the freshly parsed settings; the caller applies them. Does not mutate the live
// store - pair with `update` plus whatever rebuild the change needs.
pub fn reload_from_disk() -> Settings {
	load()
}

// The live state a reload has to carry across: what is never in the file and
// lasts the session. A reload re-reads the file, and the file never held these,
// so taking the fresh copy as-is would lift a remote screen's profile or the
// display watch's step on a menu command.
pub fn keep_session(live: &Settings, reloaded: &mut Settings, wallpaper_locked: bool) {
	reloaded.remote_override = live.remote_override;
	reloaded.stepped_profile = live.stepped_profile;
	// The picture on screen did not change, so what it is worth to a derived
	// text color did not either.
	reloaded.wallpaper_summary = live.wallpaper_summary;
	if wallpaper_locked {
		take_wallpaper(live, reloaded);
		reloaded.wallpaper_enabled |= reloaded.wallpaper.is_some();
	}
}

// A wallpaper named for the session, at launch (`--wallpaper-file`) or while
// running (`--wallpaper`). Naming one is a deliberate choice for the run, so a
// file with the wallpaper switched off does not swallow it. Both go through
// `update` afterwards, so a performance profile that turns the wallpaper off
// still wins for either one.
pub fn name_wallpaper(s: &mut Settings, image: Option<PathBuf>) {
	s.wallpaper_raw = image
		.as_ref()
		.map(|path| path.to_string_lossy().into_owned())
		.unwrap_or_default();
	s.wallpaper_enabled |= image.is_some();
	s.wallpaper = image;
}

// A wallpaper given on the command line lasts the session (`wp_locked` in
// app.rs). An Apply keeps it unless the dialog picked another, and both copies
// take it, so the save writes nothing about it.
pub fn keep_wallpaper_on_apply(
	live: &Settings,
	wallpaper_locked: bool,
	opened: &mut Settings,
	edited: &mut Settings,
) {
	if wallpaper_locked
		&& edited.wallpaper_raw == opened.wallpaper_raw
		&& edited.wallpaper == opened.wallpaper
	{
		take_wallpaper(live, opened);
		take_wallpaper(live, edited);
	}
}

fn take_wallpaper(live: &Settings, s: &mut Settings) {
	s.wallpaper_raw.clone_from(&live.wallpaper_raw);
	s.wallpaper.clone_from(&live.wallpaper);
}

// The same for an Apply from the Settings dialog, whose copy is as old as the
// dialog: a remote switch or a watch step taken since it opened stays, unless
// the dialog made the choice itself. A pick or the automatic switch can leave
// the session field exactly as the dialog opened with it, so the choice is read
// from what those write too.
pub fn keep_session_on_apply(live: &Settings, opened: &Settings, edited: &mut Settings) {
	let picked = edited.performance_profile != opened.performance_profile
		|| edited.remote_override != opened.remote_override;
	if !picked {
		edited.remote_override = live.remote_override;
	}
	if !picked
		&& edited.performance_automatic == opened.performance_automatic
		&& edited.stepped_profile == opened.stepped_profile
	{
		edited.stepped_profile = live.stepped_profile;
	}
	// The dialog's copy dates from when it opened, and a rotation since then has
	// changed the picture. The summary is never something the dialog edits.
	edited.wallpaper_summary = live.wallpaper_summary;
}

// Read the config as an editable document. The parser is forgiving (a bad line
// becomes a diagnostic, not a failed load), so unlike the old strict TOML path
// this cannot bail on a file the loader reads fine and silently save nothing.
fn read_doc(path: &std::path::Path) -> Result<shcl::Document, Unread> {
	let text = read_settings(path).map_err(|e| match e.kind() {
		std::io::ErrorKind::NotFound => Unread::Missing,
		_ => Unread::Failed(e),
	})?;
	// a launch that found the file busy left it as 2.x wrote it
	Ok(parse_kept(&from_shcl2_text(&text).unwrap_or(text)))
}

// Why a save found no document to edit.
#[derive(Debug)]
enum Unread {
	Missing,
	Failed(std::io::Error),
}

// The settings file as this process last read or wrote it.
static LAST_SEEN: std::sync::Mutex<Option<(PathBuf, String)>> = std::sync::Mutex::new(None);

fn note_seen(path: &std::path::Path, text: &str) {
	*crate::locks::lock(&LAST_SEEN) = Some((path.to_path_buf(), text.to_string()));
}

// What a save edits when the file was deleted while the program ran: a new
// file from the template, as a first launch writes, with every setting the
// file held when this process last saw it. The save's own change goes on top,
// so the session's settings are still there at the next launch. The shell list
// is diffed against the file (`shells_to_save`), so a file never seen here
// gets the list the window loaded, or every entry would read as removed.
fn regrown_doc(
	path: &std::path::Path,
	loaded_shells: &[crate::shells::ShellEntry],
) -> shcl::Document {
	let seen = crate::locks::lock(&LAST_SEEN)
		.as_ref()
		.filter(|(at, _)| at == path)
		.map(|(_, text)| text.clone());
	let Some(text) = seen else {
		let mut doc = parse_kept(default_config());
		write_shells(&mut doc, &[], loaded_shells);
		return doc;
	};
	parse_kept(&rebuilt_config_text(&loaded_text(&text), &[]).0)
}

// A parse whose save writes back every line no edit touched, as it was typed.
// Only Strict can refuse a file, so the Err arm never runs.
fn parse_kept(text: &str) -> shcl::Document {
	shcl::Document::parse_keep_lines(text, shcl::Strictness::Standard)
		.unwrap_or_else(|e| e.document)
}

// What a save writes. shcl falls back to the canonical form where it cannot keep
// the lines.
fn saved_text(doc: &shcl::Document) -> String {
	doc.to_text_keep_lines().0
}

// shcl's save_file_keep_lines gate. A line the parse dropped is written back as
// it was while the lines are kept, so only a save that falls back to the
// canonical form would delete it, and only that one is refused.
fn save_refused(doc: &shcl::Document) -> bool {
	doc.lost_count() > 0 && !doc.to_text_keep_lines().1
}

// One classified line of a config text, with its full nested path resolved from
// the indentation context (an `enabled:` two levels deep under `wallpaper:` /
// `rotate:` is "wallpaper.rotate.enabled"). Comment lines that spell a setting
// (`# enabled: true`) get a path too - the machinery treats them as that
// setting's disabled default. Lines inside a raw fence are passed through as
// `Fence`; blank lines as `Blank`; anything else as `Other`.
enum WalkLine {
	Setting {
		index: usize,
		path: String,
		active: bool,
		header: bool,
	},
	Fence,
	Blank,
	Other(usize),
}

// Walk a config text line by line, resolving each setting line's full path from
// the enclosing block headers: active ones for an active line, active or
// commented for a commented one. Indentation is compared by
// leading-whitespace length, matching how the template is written (tabs); a
// line's own key may itself be dotted, which simply extends the path.
fn walk_settings(text: &str) -> Vec<WalkLine> {
	let mut out = Vec::new();
	// (indent, name) of each enclosing block header, commented ones included
	let mut stack: Vec<(usize, String)> = Vec::new();
	// The same, active headers only. shcl gives a comment line no depth, and a
	// save writes a comment at the depth of the setting below it, so an active
	// path taken from a comment changed at the first save. Commented lines still
	// take commented headers, which is how a commented-out block names its
	// children.
	let mut open: Vec<(usize, String)> = Vec::new();
	let mut fence: Option<(char, usize)> = None;
	for (index, line) in text.lines().enumerate() {
		if let Some((ch, len)) = fence {
			out.push(WalkLine::Fence);
			let t = line.trim();
			if t.chars().all(|c| c == ch) && t.len() >= len {
				fence = None; // closing fence
			}
			continue;
		}
		if let Some(open) = fence_run(line) {
			out.push(WalkLine::Fence);
			fence = Some(open);
			continue;
		}
		if line.trim().is_empty() {
			out.push(WalkLine::Blank);
			continue;
		}
		let Some(key) = line_setting_key(line) else {
			out.push(WalkLine::Other(index));
			continue;
		};
		let trimmed = line.trim_start();
		let active = !trimmed.starts_with('#');
		let indent = line.len() - trimmed.len();
		while stack.last().is_some_and(|(col, _)| indent <= *col) {
			stack.pop();
		}
		if active {
			while open.last().is_some_and(|(col, _)| indent <= *col) {
				open.pop();
			}
		}
		let parents = if active { &open } else { &stack };
		let path = parents
			.iter()
			.map(|(_, name)| name.as_str())
			.chain(std::iter::once(key))
			.collect::<Vec<_>>()
			.join(".");
		let header = line_setting_value(line).is_some_and(|v| {
			let v = v.trim();
			v.is_empty() || v.starts_with('#')
		});
		if header {
			stack.push((indent, key.to_string()));
			if active {
				open.push((indent, key.to_string()));
			}
		}
		out.push(WalkLine::Setting {
			index,
			path,
			active,
			header,
		});
	}
	out
}

// A raw-block fence opener: a non-comment line whose content (after an optional
// `key:`) starts a run of 3+ backticks or tildes. Returns the char and length.
fn fence_run(line: &str) -> Option<(char, usize)> {
	if line.trim_start().starts_with('#') {
		return None;
	}
	let after = line
		.split_once(':')
		.map_or(line.trim(), |(_, rest)| rest.trim());
	let ch = after.chars().next()?;
	if ch != '`' && ch != '~' {
		return None;
	}
	let len = after.chars().take_while(|c| *c == ch).count();
	(len >= 3).then_some((ch, len))
}

// Serialize a document back to disk.
//
// The canonical text keeps comments, blank-line grouping, indentation and line
// order, and never rewrites a scalar - so it IS the disk text (shcl 1.2 made
// that true; before it a comment run under a block of commented-out defaults
// came back at the header's depth). The write goes through a temp file and a
// rename, so a crash mid-save cannot leave a truncated config, and it is
// refused outright when the parse dropped lines the save would delete - the
// user's own text is worth more than one changed setting.
// Answers whether it wrote. A refusal has to reach the caller: the dialog closes
// on a save, and three failures used to present as a clean one - shcl refusing a
// lossy round trip, an unreadable file, an unwritable one.
// Every write of the settings file goes through here, launch-time rewrites too,
// and so does a PowerShell profile write.
// It writes beside the file and renames over it: `fs::write` truncates first, so
// a crash or a full disk during one leaves nothing where the config was. A linked
// settings file is written through its link rather than replaced by a copy, the
// file keeps its mode, and the temp file is created exclusively, so a link left
// at its name is never written through. On Windows the publish is ReplaceFile,
// which keeps the file's ACLs. A path that is not UTF-8 is refused rather than
// converted lossily, which could name a different file. A write that moves the
// file to a newer SHCL format, or replaces one that is not UTF-8, keeps the old
// one first (`keep_old_format`).
pub(crate) fn write_config_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
	write_config_keeping(path, text).map(|_| ())
}

// The same write, answering where the old file was kept when it converted one.
// Settings the conversion could not keep are said here, so a save that converts
// a file a busy launch left alone says it as the launch would. A clean
// conversion says nothing.
fn write_config_keeping(path: &std::path::Path, text: &str) -> Result<Option<PathBuf>, String> {
	let kept = publish_keeping(path, text, shcl::write_file_atomic)?;
	note_seen(path, text);
	for line in restated_launch_messages(path, text) {
		eprintln!("{line}");
	}
	if let Some(loss) = kept.as_deref().and_then(|copy| lost_converting(path, copy)) {
		eprintln!("{}", loss.terminal_line());
		// a footer put back as shipped lost nothing anybody wrote
		let owed = !matches!(loss.how, Converted::Dropped(_)) || !loss.lines.is_empty();
		if owed {
			*crate::locks::lock(&LOST_IN_CONVERSION) = Some(loss);
		}
	}
	Ok(kept)
}

// The copy is the file exactly as it was before the conversion.
fn lost_converting(path: &std::path::Path, copy: &std::path::Path) -> Option<ConversionLoss> {
	let old = std::fs::read(copy).ok()?;
	conversion_losses(&old, path, Some(copy.to_path_buf()))
}

#[cfg(test)]
fn write_config_atomic_with(
	path: &std::path::Path,
	text: &str,
	publish: fn(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
	publish_keeping(path, text, publish).map(|_| ())
}

// Windows' replace can fail after the old file is gone (1176, 1177), and shcl
// then deletes its temp copy too, so the text is written at the empty name
// rather than lost. The publish is a parameter so a test can fail it that way.
fn publish_keeping(
	path: &std::path::Path,
	text: &str,
	publish: fn(&str, &str) -> Result<(), String>,
) -> Result<Option<PathBuf>, String> {
	if !may_write(path) {
		return Ok(None);
	}
	let Some(file) = path.to_str() else {
		return Err(format!("{} is not a UTF-8 path", path.display()));
	};
	// Resolved before the write: once the replace has taken a linked file, the
	// link dangles and no longer resolves.
	let before = std::fs::canonicalize(path).ok().and_then(|real| {
		let perms = std::fs::metadata(&real).ok()?.permissions();
		Some((real, perms))
	});
	let kept = keep_old_format(path, text)?;
	publish(file, text).or_else(|e| restore_config(before, text, e))?;
	let Some((backup, old)) = kept else {
		return Ok(None);
	};
	// A file kept for its lines that are not UTF-8 is said by `write_config_keeping`.
	if let Some(new) = shcl::format_version(text).filter(|new| *new > old) {
		eprintln!(
			"{APP_NAME}: {} converted to SHCL format {new}; the old file is kept at {}",
			path.display(),
			backup.display()
		);
	}
	Ok(Some(backup))
}

// A file with no Format line is read as shcl 2.x, the last format without one.
const UNSTAMPED_FORMAT: u32 = 2;

fn format_of(text: &str) -> u32 {
	shcl::format_version(text).unwrap_or(UNSTAMPED_FORMAT)
}

// The name a file in format `n` is kept under when it is converted, beside it
// and still ending in .shcl: `config.shcl` ->
// `config_backup_20261003-142233_format-v2.shcl`. A second one made in the same
// second gets `_2` before the `.shcl`, and so on.
fn backup_name(stem: &str, stamp: &str, n: u32, attempt: u32) -> String {
	let again = if attempt > 1 {
		format!("_{attempt}")
	} else {
		String::new()
	};
	format!("{stem}_backup_{stamp}_format-v{n}{again}.shcl")
}

// Before a write moves the file to a newer format, the file as it is now is
// copied beside it under a `backup_name` stamped with the local time. That
// covers every conversion, and a save that converts a file a launch left
// alone. A current file with lines that are not UTF-8 is written again without
// them (`upgrade`), and is kept the same way, so no write ever loses its bytes.
// Every older version is kept: a backup already there is never replaced. The
// write is refused when the copy cannot be made. Answers where the copy is,
// and the format the file had.
fn keep_old_format(path: &std::path::Path, text: &str) -> Result<Option<(PathBuf, u32)>, String> {
	let (stamp, _) = local_stamp();
	keep_old_format_at(path, text, &stamp)
}

fn keep_old_format_at(
	path: &std::path::Path,
	text: &str,
	stamp: &str,
) -> Result<Option<(PathBuf, u32)>, String> {
	let Some(new) = shcl::format_version(text) else {
		return Ok(None);
	};
	let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
		return Ok(None);
	};
	let body = match std::fs::read(path) {
		Ok(body) => body,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(e) => return Err(format!("could not read {} to keep it: {e}", path.display())),
	};
	let old = format_of(&String::from_utf8_lossy(&body));
	let decodes = std::str::from_utf8(&body).is_ok();
	if (new <= old && decodes) || body.iter().all(u8::is_ascii_whitespace) {
		return Ok(None);
	}
	let stem = name.strip_suffix(".shcl").unwrap_or(&name);
	#[cfg(unix)]
	let perms = std::fs::metadata(path).ok().map(|meta| meta.permissions());
	#[cfg(not(unix))]
	let perms = None;
	write_new_file(
		|attempt| path.with_file_name(backup_name(stem, stamp, old, attempt)),
		&body,
		perms.as_ref(),
	)
	.map(|copy| Some((copy, old)))
	.map_err(|e| format!("could not keep the old file beside {}: {e}", path.display()))
}

// Writes `body` at the first of `names(1)`, `names(2)` and on that nothing has,
// and never leaves a part of it there. It is written under a name of its own
// and then linked, which fails rather than replace anything. A name already
// holding the same bytes is the same copy, from another window converting at
// the same moment, so two at once leave one whole copy. Where the filesystem
// has no hard links, each name is written directly. Answers where it went.
fn write_new_file(
	names: impl Fn(u32) -> PathBuf,
	body: &[u8],
	perms: Option<&std::fs::Permissions>,
) -> std::io::Result<PathBuf> {
	use std::io::Write;
	let first = names(1);
	let name = first
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let mut opts = std::fs::OpenOptions::new();
	opts.write(true).create_new(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
	let fill = |file: &mut std::fs::File, at: &std::path::Path| {
		// Private before it holds anything, as the config is. Elsewhere the mode is
		// only a read-only flag, which would stop the temp name being removed.
		if let Some(perms) = perms {
			std::fs::set_permissions(at, perms.clone())?;
		}
		file.write_all(body)?;
		file.sync_all()
	};
	let same = |at: &std::path::Path| std::fs::read(at).is_ok_and(|seen| seen == body);
	let mut n = 0u32;
	let (temp, mut file) = loop {
		n += 1;
		let temp = first.with_file_name(format!(".{name}.{}-{n}.tmp", std::process::id()));
		match opts.open(&temp) {
			Ok(file) => break (temp, file),
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < BACKUPS_MAX => {}
			Err(e) => return Err(e),
		}
	};
	let filled = fill(&mut file, &temp);
	drop(file);
	let linked = filled.map(|()| {
		for attempt in 1..=BACKUPS_MAX {
			let dest = names(attempt);
			match std::fs::hard_link(&temp, &dest) {
				Ok(()) => return Some(Ok(dest)),
				Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
					if same(&dest) {
						return Some(Ok(dest));
					}
				}
				// vfat answers EPERM, not "unsupported", so any other failure tries a
				// plain write
				Err(_) => return None,
			}
		}
		Some(Err(no_free_name()))
	});
	let _ = std::fs::remove_file(&temp);
	if let Some(done) = linked? {
		return done;
	}
	for attempt in 1..=BACKUPS_MAX {
		let dest = names(attempt);
		let mut file = match opts.open(&dest) {
			Ok(file) => file,
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
				if same(&dest) {
					return Ok(dest);
				}
				continue;
			}
			Err(e) => return Err(e),
		};
		let filled = fill(&mut file, &dest);
		drop(file);
		if let Err(e) = filled {
			let _ = std::fs::remove_file(&dest);
			return Err(e);
		}
		return Ok(dest);
	}
	Err(no_free_name())
}

fn no_free_name() -> std::io::Error {
	std::io::Error::new(
		std::io::ErrorKind::AlreadyExists,
		format!("{BACKUPS_MAX} copies already made this second"),
	)
}

// The local time to the second, `YYYYmmDD-HHMMSS`, and the hundredths past it.
// Local, as the test folders and the pipeline's log names are.
#[cfg(unix)]
pub(crate) fn local_stamp() -> (String, u32) {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default();
	let seconds = libc::time_t::try_from(now.as_secs()).unwrap_or(libc::time_t::MAX);
	// SAFETY: tm is plain data, so all zeros is a valid value, and localtime_r
	// only writes the tm it is handed.
	let fields = unsafe {
		let mut fields: libc::tm = std::mem::zeroed();
		libc::localtime_r(&raw const seconds, &raw mut fields);
		fields
	};
	(
		format!(
			"{:04}{:02}{:02}-{:02}{:02}{:02}",
			fields.tm_year + 1900,
			fields.tm_mon + 1,
			fields.tm_mday,
			fields.tm_hour,
			fields.tm_min,
			fields.tm_sec,
		),
		now.subsec_millis() / 10,
	)
}

#[cfg(windows)]
pub(crate) fn local_stamp() -> (String, u32) {
	use windows_sys::Win32::Foundation::SYSTEMTIME;
	use windows_sys::Win32::System::SystemInformation::GetLocalTime;
	// SAFETY: SYSTEMTIME is plain data, so all zeros is a valid value, and
	// GetLocalTime only fills the struct it is handed.
	let now = unsafe {
		let mut now: SYSTEMTIME = std::mem::zeroed();
		GetLocalTime(&raw mut now);
		now
	};
	(
		format!(
			"{:04}{:02}{:02}-{:02}{:02}{:02}",
			now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond,
		),
		u32::from(now.wMilliseconds / 10),
	)
}

// Writes the text where the file was, only while nothing is at that name. The
// ordinary failure leaves the old file there and returns at once, and a name
// taken meanwhile, a link included, is never written through. A name only
// waiting for its delete to finish counts as empty.
fn restore_config(
	before: Option<(PathBuf, std::fs::Permissions)>,
	text: &str,
	err: String,
) -> Result<(), String> {
	use std::io::Write;
	let Some((real, perms)) = before else {
		return Err(err);
	};
	// elsewhere the mode is only a read-only flag, which adds no privacy
	#[cfg(not(unix))]
	let _ = perms;
	let mut last = String::new();
	for _ in 0..RESTORE_ATTEMPTS {
		if name_taken(&real) {
			return Err(err);
		}
		std::thread::sleep(RESTORE_PAUSE);
		let mut opts = std::fs::OpenOptions::new();
		opts.write(true).create_new(true);
		#[cfg(unix)]
		std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
		let mut file = match opts.open(&real) {
			Ok(file) => file,
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(err),
			Err(e) => {
				last = e.to_string();
				continue;
			}
		};
		// cloned because a later attempt sets it again
		#[cfg(unix)]
		let written = file
			.set_permissions(perms.clone())
			.and_then(|()| file.write_all(text.as_bytes()));
		#[cfg(not(unix))]
		let written = file.write_all(text.as_bytes());
		match written.and_then(|()| file.sync_all()) {
			Ok(()) => {
				eprintln!(
					"{APP_NAME}: {err}; the file was gone after that, so {} was written directly",
					real.display()
				);
				return Ok(());
			}
			Err(e) => {
				drop(file);
				// the file this attempt created, not one that was there before
				let _ = std::fs::remove_file(&real);
				last = e.to_string();
			}
		}
	}
	Err(format!(
		"{err}; the file it replaced is gone, and writing {} directly failed: {last}",
		real.display()
	))
}

// Something is at the name, unless it is only waiting for its delete to finish.
// Not exists(): it follows a link, and a dangling one answers false.
fn name_taken(real: &std::path::Path) -> bool {
	std::fs::symlink_metadata(real).is_ok() && !delete_pending(real)
}

#[cfg(not(windows))]
fn delete_pending(_: &std::path::Path) -> bool {
	false
}

// A name whose delete waits on another program's handle is still listed and
// still answers symlink_metadata, but nothing can open it. Only the NT status
// tells that apart from a file this process may not open. Takes the \\?\ form
// canonicalize gives; anything else answers false, as does any status but
// the pending one.
#[cfg(windows)]
fn delete_pending(real: &std::path::Path) -> bool {
	use std::os::windows::ffi::OsStrExt;
	use windows_sys::Wdk::Foundation::{NtClose, OBJECT_ATTRIBUTES};
	use windows_sys::Wdk::Storage::FileSystem::{
		FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
		NtOpenFile,
	};
	use windows_sys::Win32::Foundation::{HANDLE, STATUS_DELETE_PENDING, UNICODE_STRING};
	use windows_sys::Win32::Storage::FileSystem::{
		FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
	};
	use windows_sys::Win32::System::IO::{IO_STATUS_BLOCK, IO_STATUS_BLOCK_0};

	// Win32_System_Kernel, not enabled for one constant
	const OBJ_CASE_INSENSITIVE: u32 = 0x40;
	const WIN32_PREFIX: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
	const NT_PREFIX: [u16; 4] = [b'\\' as u16, b'?' as u16, b'?' as u16, b'\\' as u16];

	let mut units: Vec<u16> = real.as_os_str().encode_wide().collect();
	let Some(prefix) = units.get_mut(..4) else {
		return false;
	};
	if *prefix != WIN32_PREFIX {
		return false;
	}
	prefix.copy_from_slice(&NT_PREFIX);
	// Length counts bytes and needs no terminator
	let Ok(bytes) = u16::try_from(units.len() * 2) else {
		return false;
	};
	let Ok(attributes_size) = u32::try_from(size_of::<OBJECT_ATTRIBUTES>()) else {
		return false;
	};
	let name = UNICODE_STRING {
		Length: bytes,
		MaximumLength: bytes,
		Buffer: units.as_mut_ptr(),
	};
	let attributes = OBJECT_ATTRIBUTES {
		Length: attributes_size,
		RootDirectory: std::ptr::null_mut(),
		ObjectName: &raw const name,
		Attributes: OBJ_CASE_INSENSITIVE,
		SecurityDescriptor: std::ptr::null(),
		SecurityQualityOfService: std::ptr::null(),
	};
	let mut io_status = IO_STATUS_BLOCK {
		Anonymous: IO_STATUS_BLOCK_0 { Status: 0 },
		Information: 0,
	};
	let mut handle: HANDLE = std::ptr::null_mut();
	// Attributes only and full sharing never conflict with another program's
	// handle. The reparse flag judges a link at the name, never its target.
	// SAFETY: plain FFI on locals that outlive the call. The name's buffer holds
	// exactly `Length` bytes, and `attributes` points at `name`.
	let status = unsafe {
		NtOpenFile(
			&raw mut handle,
			SYNCHRONIZE | FILE_READ_ATTRIBUTES,
			&raw const attributes,
			&raw mut io_status,
			FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
			FILE_OPEN_REPARSE_POINT | FILE_OPEN_FOR_BACKUP_INTENT | FILE_SYNCHRONOUS_IO_NONALERT,
		)
	};
	if status >= 0 {
		// SAFETY: a successful open returned this handle, and nothing else holds it
		unsafe { NtClose(handle) };
	}
	status == STATUS_DELETE_PENDING
}

// A save refused because the file has a line that cannot be read, and a write
// would drop it. The save happens wherever it was called from, and only the
// window can put a message in front of anybody, so it waits here to be asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
	pub path: std::path::PathBuf,
	pub lines: Vec<usize>,
	pub lost: usize,
}

static REFUSED: std::sync::Mutex<Option<Refusal>> = std::sync::Mutex::new(None);

pub fn take_refusal() -> Option<Refusal> {
	crate::locks::lock(&REFUSED).take()
}

fn leave_refusal(refusal: Refusal) {
	*crate::locks::lock(&REFUSED) = Some(refusal);
}

fn unreadable_lines(doc: &shcl::Document) -> Vec<usize> {
	let mut lines: Vec<usize> = doc
		.diagnostics()
		.iter()
		.filter(|d| matches!(d.severity, shcl::Severity::Error))
		.map(|d| d.line)
		.filter(|line| *line > 0)
		.collect();
	lines.sort_unstable();
	lines.dedup();
	lines
}

// The same gate and text as shcl's save_file_keep_lines, through the writer above, which
// can put the file back when a replace took it.
#[must_use]
fn write_doc(path: &std::path::Path, doc: &shcl::Document) -> bool {
	let lost = if save_refused(doc) {
		doc.lost_count()
	} else {
		0
	};
	if lost > 0 {
		leave_refusal(Refusal {
			path: path.to_path_buf(),
			lines: unreadable_lines(doc),
			lost,
		});
	}
	let written = if lost > 0 {
		Err(shcl::SaveError::Refused {
			path: path.display().to_string(),
			lost,
		}
		.to_string())
	} else {
		write_config_atomic(path, &saved_text(doc))
	};
	if let Err(e) = written {
		eprintln!("{APP_NAME}: could not save config {}: {e}", path.display());
		return false;
	}
	true
}

// A setter answers whether the write applied, and every path here is one of
// ours, so a refusal is a bug worth hearing about rather than a silent no-op.
trait Put {
	fn put_string(&mut self, path: &str, v: &str);
	fn put_bool(&mut self, path: &str, v: bool);
	fn put_float(&mut self, path: &str, v: f64);
	fn put_int(&mut self, path: &str, v: i64);
	fn put_datetime(&mut self, path: &str, v: &shcl::ShclDateTime);
	fn put_string_array(&mut self, path: &str, v: &[&str]);
}
impl Put for shcl::Document {
	fn put_string(&mut self, path: &str, v: &str) {
		let applied = self.set_string(path, v);
		unwritable(self, path, applied);
	}
	fn put_bool(&mut self, path: &str, v: bool) {
		let applied = self.set_bool(path, v);
		unwritable(self, path, applied);
	}
	fn put_float(&mut self, path: &str, v: f64) {
		let applied = self.set_float(path, v);
		unwritable(self, path, applied);
	}
	fn put_int(&mut self, path: &str, v: i64) {
		let applied = self.set_int(path, v);
		unwritable(self, path, applied);
	}
	fn put_datetime(&mut self, path: &str, v: &shcl::ShclDateTime) {
		let applied = self.set_datetime(path, v);
		unwritable(self, path, applied);
	}
	fn put_string_array(&mut self, path: &str, v: &[&str]) {
		let applied = self.set_string_array(path, v);
		unwritable(self, path, applied);
	}
}
fn unwritable(doc: &shcl::Document, path: &str, applied: bool) {
	if !applied {
		eprintln!(
			"{APP_NAME}: config: could not write {path} ({:?})",
			doc.write_reason(path)
		);
	}
}

// Settings the user cleared back to "not set". These write nothing - there is no
// value to write - so without naming them here the old line stays in the file
// and the setting comes back next launch. One long today; anything optional
// added to the dialog belongs on it.
fn cleared_keys(orig: &Settings, s: &Settings) -> Vec<&'static str> {
	let mut out = Vec::new();
	if s.font_family.is_none() && orig.font_family.is_some() {
		out.push("font.family");
	}
	out
}

// NaN is never equal to itself, so a plain `!=` reads a NaN on both sides as a
// change and writes it over the value in the file. Every float setting is
// compared through this. A NaN only ever arrived from the command line, which
// refuses one now, but the comparison is where the damage was done.
fn same_f32(a: f32, b: f32) -> bool {
	a == b || (a.is_nan() && b.is_nan())
}

// Write the values that differ from `orig` back into the config in place. The
// user's comments and blank-line grouping survive (see `to_text`); untouched
// settings keep whatever they were (commented / following the system). Returns
// false (writing nothing) if the file looks open in another program, so the
// caller can hold off - e.g. the Settings dialog stays open instead of
// clobbering an in-flight edit.
#[must_use]
pub fn persist(orig: &Settings, s: &Settings) -> bool {
	let Some(path) = config_path() else {
		return true;
	};
	if config_open_elsewhere(&path) {
		note_config_busy(&path);
		return false;
	}
	let mut doc = match read_doc(&path) {
		Ok(doc) => doc,
		Err(Unread::Missing) => {
			if let Some(dir) = path.parent().filter(|_| may_write(&path)) {
				let _ = std::fs::create_dir_all(dir);
			}
			regrown_doc(&path, &orig.shells)
		}
		Err(Unread::Failed(e)) => {
			eprintln!("{APP_NAME}: could not read config {}: {e}", path.display());
			return false;
		}
	};
	// Both sides diff as the user's own values. A live copy carries a profile's
	// values over them, and those must never reach the file.
	let mut own = (orig.clone(), s.clone());
	crate::profile::unapply(&mut own.0);
	crate::profile::unapply(&mut own.1);
	crate::autotheme::unapply(&mut own.0);
	crate::autotheme::unapply(&mut own.1);
	let (orig, s) = (&own.0, &own.1);
	// round f32 -> a clean decimal so persisted floats aren't 0.2000000029...
	let r = |v: f32| (v as f64 * 1000.0).round() / 1000.0;

	if s.theme != orig.theme {
		doc.put_string("theme", s.theme.as_str());
	}
	if s.theme_mode != orig.theme_mode {
		doc.put_string("theme_mode", s.theme_mode.as_str());
	}
	if s.performance_automatic != orig.performance_automatic {
		doc.put_bool("performance.automatic", s.performance_automatic);
	}
	if s.performance_profile != orig.performance_profile {
		doc.put_string("performance.profile", s.performance_profile.as_str());
	}
	if s.performance_check_hardware != orig.performance_check_hardware {
		doc.put_bool("performance.check_hardware", s.performance_check_hardware);
	}
	if s.performance_check_next_run != orig.performance_check_next_run {
		doc.put_bool("performance.check_next_run", s.performance_check_next_run);
	}
	if s.rated_hardware != orig.rated_hardware {
		doc.put_string("performance.rated_hardware", s.rated_hardware.as_str());
	}
	write_user_themes(&mut doc, &orig.user_themes, &s.user_themes);
	write_shells(&mut doc, &orig.shells, &s.shells);

	if s.use_system_font != orig.use_system_font {
		doc.put_bool("font.use_system_family", s.use_system_font);
	}
	if s.use_system_font_size != orig.use_system_font_size {
		doc.put_bool("font.use_system_size", s.use_system_font_size);
	}
	if s.font_family != orig.font_family {
		if let Some(f) = &s.font_family {
			doc.put_string("font.family", f);
		}
	}
	if !same_f32(s.font_size, orig.font_size) {
		doc.put_float("font.size", r(s.font_size));
	}
	if !same_f32(s.line_height_scale, orig.line_height_scale) {
		doc.put_float("font.line_height_scale", r(s.line_height_scale));
	}
	if s.scrollback != orig.scrollback {
		doc.put_int("scroll.scrollback", s.scrollback as i64);
	}
	if s.scroll_smooth != orig.scroll_smooth {
		doc.put_bool("scroll.smooth", s.scroll_smooth);
	}
	if !same_f32(s.scroll_ease_in_ms, orig.scroll_ease_in_ms) {
		doc.put_float("scroll.ease_in_ms", r(s.scroll_ease_in_ms));
	}
	if !same_f32(s.scroll_ramp_up_ms, orig.scroll_ramp_up_ms) {
		doc.put_float("scroll.ramp_up_ms", r(s.scroll_ramp_up_ms));
	}
	if !same_f32(
		s.scroll_single_screen_tau_ms,
		orig.scroll_single_screen_tau_ms,
	) {
		doc.put_float(
			"scroll.single_screen_tau_ms",
			r(s.scroll_single_screen_tau_ms),
		);
	}
	if !same_f32(s.scroll_ramp_down_ms, orig.scroll_ramp_down_ms) {
		doc.put_float("scroll.ramp_down_ms", r(s.scroll_ramp_down_ms));
	}
	if !same_f32(s.scroll_ease_out_ms, orig.scroll_ease_out_ms) {
		doc.put_float("scroll.ease_out_ms", r(s.scroll_ease_out_ms));
	}
	if !same_f32(s.wheel_lines, orig.wheel_lines) {
		doc.put_float("scroll.wheel_lines", r(s.wheel_lines));
	}
	if !same_f32(s.alt_scroll_lines, orig.alt_scroll_lines) {
		doc.put_float("scroll.alt_scroll_lines", r(s.alt_scroll_lines));
	}
	if !same_f32(s.output_ease_lines, orig.output_ease_lines) {
		doc.put_float("scroll.output_ease_lines", r(s.output_ease_lines));
	}
	if s.scrollbar != orig.scrollbar {
		doc.put_bool("scroll.scrollbar.enabled", s.scrollbar);
	}
	if !same_f32(s.scrollbar_thickness, orig.scrollbar_thickness) {
		doc.put_float("scroll.scrollbar.thickness", r(s.scrollbar_thickness));
	}
	if s.scrollbar_auto_hide != orig.scrollbar_auto_hide {
		doc.put_bool("scroll.scrollbar.auto_hide", s.scrollbar_auto_hide);
	}
	if s.minimap != orig.minimap {
		doc.put_bool("scroll.minimap.enabled", s.minimap);
	}
	if !same_f32(s.minimap_width, orig.minimap_width) {
		doc.put_float("scroll.minimap.width", r(s.minimap_width));
	}
	if s.minimap_tui_whitelist != orig.minimap_tui_whitelist {
		doc.put_string(
			"scroll.minimap.tui_process_whitelist",
			&s.minimap_tui_whitelist,
		);
	}
	if !same_f32(s.margin, orig.margin) {
		doc.put_float("window.margin", r(s.margin));
	}
	if !same_f32(s.opacity, orig.opacity) {
		doc.put_float("transparency.opacity", r(s.opacity));
	}
	if s.transparent_background != orig.transparent_background {
		doc.put_bool("transparency.enabled", s.transparent_background);
	}
	if s.transparent_background_blur != orig.transparent_background_blur {
		doc.put_bool("transparency.blur_behind", s.transparent_background_blur);
	}
	if !same_f32(s.wallpaper_opacity, orig.wallpaper_opacity) {
		doc.put_float("wallpaper.opacity", r(s.wallpaper_opacity));
	}
	if !same_f32(s.wallpaper_even, orig.wallpaper_even) {
		doc.put_float("wallpaper.even_visibility", r(s.wallpaper_even));
	}
	if s.wallpaper_enabled != orig.wallpaper_enabled {
		doc.put_bool("wallpaper.enabled", s.wallpaper_enabled);
	}
	if s.wallpaper_rotate_enabled != orig.wallpaper_rotate_enabled {
		doc.put_bool("wallpaper.rotate.enabled", s.wallpaper_rotate_enabled);
	}
	if s.wallpaper_default_fit != orig.wallpaper_default_fit {
		doc.put_string(
			"wallpaper.default_fit",
			match s.wallpaper_default_fit {
				Fit::Zoom => "zoom",
				Fit::Stretch => "stretch",
			},
		);
	}
	if s.wallpaper_honor_xmp != orig.wallpaper_honor_xmp {
		doc.put_bool("wallpaper.honor_xmp", s.wallpaper_honor_xmp);
	}
	if s.wallpaper_honor_xmp_look != orig.wallpaper_honor_xmp_look {
		doc.put_bool("wallpaper.honor_xmp_look", s.wallpaper_honor_xmp_look);
	}
	if !same_f32(s.wallpaper_blur, orig.wallpaper_blur) {
		doc.put_float("wallpaper.blur", r(s.wallpaper_blur));
	}
	if s.wallpaper_contrast_mask != orig.wallpaper_contrast_mask {
		doc.put_bool("wallpaper.contrast_mask.enabled", s.wallpaper_contrast_mask);
	}
	if !same_f32(
		s.wallpaper_contrast_mask_size,
		orig.wallpaper_contrast_mask_size,
	) {
		doc.put_float(
			"wallpaper.contrast_mask.size",
			r(s.wallpaper_contrast_mask_size),
		);
	}
	if !same_f32(
		s.wallpaper_contrast_mask_strength,
		orig.wallpaper_contrast_mask_strength,
	) {
		doc.put_float(
			"wallpaper.contrast_mask.strength",
			r(s.wallpaper_contrast_mask_strength),
		);
	}
	if !same_f32(
		s.wallpaper_contrast_mask_auto,
		orig.wallpaper_contrast_mask_auto,
	) {
		doc.put_float(
			"wallpaper.contrast_mask.auto",
			r(s.wallpaper_contrast_mask_auto),
		);
	}
	if s.text_scrim != orig.text_scrim {
		doc.put_bool("text.scrim.enabled", s.text_scrim);
	}
	if !same_f32(s.text_scrim_radius, orig.text_scrim_radius) {
		doc.put_float("text.scrim.radius", r(s.text_scrim_radius));
	}
	if !same_f32(s.text_scrim_softness, orig.text_scrim_softness) {
		doc.put_float("text.scrim.softness", r(s.text_scrim_softness));
	}
	if !same_f32(s.text_scrim_strength, orig.text_scrim_strength) {
		doc.put_float("text.scrim.strength", r(s.text_scrim_strength));
	}
	if !same_f32(s.text_outline, orig.text_outline) {
		doc.put_float("text.outline", r(s.text_outline));
	}
	if !same_f32(s.text_dark_on_light, orig.text_dark_on_light) {
		doc.put_float("text.dark_on_light", r(s.text_dark_on_light));
	}
	if s.text_scrim_ramp != orig.text_scrim_ramp {
		doc.put_string("text.scrim.ramp", &s.text_scrim_ramp);
	}
	if s.text_scrim_function != orig.text_scrim_function {
		doc.put_string("text.scrim.function", &s.text_scrim_function);
	}
	if s.text_scrim_regular_weight != orig.text_scrim_regular_weight {
		doc.put_bool("text.scrim.regular_weight", s.text_scrim_regular_weight);
	}
	if !same_f32(s.text_min_contrast, orig.text_min_contrast) {
		doc.put_float("text.min_contrast", r(s.text_min_contrast));
	}
	if s.color_emoji != orig.color_emoji {
		doc.put_bool("text.color_emoji", s.color_emoji);
	}
	if s.embolden_inverse != orig.embolden_inverse {
		doc.put_bool("text.embolden_inverse", s.embolden_inverse);
	}
	if s.cursor_scrim != orig.cursor_scrim {
		doc.put_bool("cursor.scrim", s.cursor_scrim);
	}
	if s.cursor_outline != orig.cursor_outline {
		doc.put_bool("cursor.outline", s.cursor_outline);
	}
	if !same_f32(s.cursor_size_height, orig.cursor_size_height) {
		doc.put_float("cursor.size.height", r(s.cursor_size_height));
	}
	if !same_f32(s.cursor_size_width, orig.cursor_size_width) {
		doc.put_float("cursor.size.width", r(s.cursor_size_width));
	}
	if s.cursor_animation != orig.cursor_animation {
		doc.put_string("cursor.animation", &s.cursor_animation);
	}
	if !same_f32(s.cursor_animation_resume_s, orig.cursor_animation_resume_s) {
		doc.put_float("cursor.animation_resume_s", r(s.cursor_animation_resume_s));
	}
	if !same_f32(s.cursor_blink_rate_ms, orig.cursor_blink_rate_ms) {
		doc.put_float("cursor.blink_rate_ms", r(s.cursor_blink_rate_ms));
	}
	if s.columns != orig.columns {
		doc.put_int("window.columns", s.columns as i64);
	}
	if s.rows != orig.rows {
		doc.put_int("window.rows", s.rows as i64);
	}
	if s.remember_size != orig.remember_size {
		doc.put_bool("window.remember_size", s.remember_size);
	}
	if s.remember_per_monitor != orig.remember_per_monitor {
		doc.put_bool("window.remember_per_monitor", s.remember_per_monitor);
	}
	if s.remember_maximized != orig.remember_maximized {
		doc.put_bool("window.remember_maximized", s.remember_maximized);
	}
	if s.hide_single_tab != orig.hide_single_tab {
		doc.put_bool("window.hide_single_tab", s.hide_single_tab);
	}
	if s.tab_shows_shell != orig.tab_shows_shell {
		doc.put_bool("window.tab_shows_shell", s.tab_shows_shell);
	}
	if s.tab_shows_program != orig.tab_shows_program {
		doc.put_bool("window.tab_shows_program", s.tab_shows_program);
	}
	if s.tab_shows_title != orig.tab_shows_title {
		doc.put_bool("window.tab_shows_title", s.tab_shows_title);
	}
	if s.tab_shows_directory != orig.tab_shows_directory {
		doc.put_bool("window.tab_shows_directory", s.tab_shows_directory);
	}
	if s.title_shows_tab != orig.title_shows_tab {
		doc.put_bool("window.title_shows_tab", s.title_shows_tab);
	}
	if s.idle_release != orig.idle_release {
		doc.put_bool("window.idle_release", s.idle_release);
	}
	if s.idle_release_minimized_min != orig.idle_release_minimized_min {
		doc.put_int(
			"window.idle_release_minimized_min",
			s.idle_release_minimized_min as i64,
		);
	}
	if s.idle_release_hidden_min != orig.idle_release_hidden_min {
		doc.put_int(
			"window.idle_release_hidden_min",
			s.idle_release_hidden_min as i64,
		);
	}
	if s.idle_release_min != orig.idle_release_min {
		doc.put_int("window.idle_release_min", s.idle_release_min as i64);
	}
	if s.software_rendering != orig.software_rendering {
		doc.put_bool("window.software_rendering", s.software_rendering);
	}
	if !same_f32(s.tab_regular_pct, orig.tab_regular_pct) {
		doc.put_float("window.tab_regular_width_pct", r(s.tab_regular_pct));
	}
	if !same_f32(s.tab_max_pct, orig.tab_max_pct) {
		doc.put_float("window.tab_max_width_pct", r(s.tab_max_pct));
	}
	if s.remembered_columns != orig.remembered_columns {
		doc.put_int("window.remembered_columns", s.remembered_columns as i64);
	}
	if s.remembered_rows != orig.remembered_rows {
		doc.put_int("window.remembered_rows", s.remembered_rows as i64);
	}
	if s.remembered_maximized != orig.remembered_maximized {
		doc.put_bool("window.remembered_maximized", s.remembered_maximized);
	}
	if s.remembered_font_zoom != orig.remembered_font_zoom {
		doc.put_int(
			"window.remembered_font_zoom",
			i64::from(s.remembered_font_zoom),
		);
	}
	write_monitor_sizes(&mut doc, &orig.monitor_sizes, &s.monitor_sizes);
	if s.word_separators != orig.word_separators {
		doc.put_string("selection.word_separators", &s.word_separators);
	}
	if s.selection_pairs != orig.selection_pairs {
		doc.put_string("selection.pairs", &s.selection_pairs);
	}
	if s.command_line != orig.command_line {
		doc.put_string("shell.command_line", &s.command_line);
	}
	if s.startup_directory != orig.startup_directory {
		doc.put_string("shell.startup_directory", &s.startup_directory);
	}
	if s.shell_integration != orig.shell_integration {
		doc.put_bool("shell.integration", s.shell_integration);
	}
	if s.bash_prompt != orig.bash_prompt {
		doc.put_bool("shell.bash_prompt", s.bash_prompt);
	}
	if s.copy_on_select != orig.copy_on_select {
		doc.put_bool("shell.copy_on_select", s.copy_on_select);
	}
	if s.hyperlinks != orig.hyperlinks {
		doc.put_bool("hyperlinks.enabled", s.hyperlinks);
	}
	if s.hyperlink_open_command != orig.hyperlink_open_command {
		doc.put_string("hyperlinks.open_command", &s.hyperlink_open_command);
	}
	if s.keys != orig.keys {
		write_keys(&mut doc, &orig.keys, &s.keys);
	}
	if s.wallpaper_folder_raw != orig.wallpaper_folder_raw {
		let folder = s.wallpaper_folder_raw.trim();
		doc.put_string(
			"wallpaper.rotate.folder",
			if folder.is_empty() {
				WALLPAPER_DIR_TOKEN
			} else {
				folder
			},
		);
	}
	if s.wallpaper != orig.wallpaper || s.wallpaper_raw != orig.wallpaper_raw {
		// the file keeps whatever form the user wrote (bare/relative/absolute)
		if s.wallpaper_raw.trim().is_empty() {
			doc.remove("wallpaper.image");
		} else {
			doc.put_string("wallpaper.image", s.wallpaper_raw.trim());
		}
	}
	if s.wallpaper_fallback_builtin != orig.wallpaper_fallback_builtin {
		doc.put_bool("wallpaper.fallback_builtin", s.wallpaper_fallback_builtin);
	}

	if s.colors_from_wallpaper != orig.colors_from_wallpaper {
		doc.put_bool("colors.from_wallpaper", s.colors_from_wallpaper);
	}
	let mut set_color = |key: &str, color: [u8; 3], orig_color: [u8; 3]| {
		if color != orig_color {
			doc.put_string(&format!("colors.{key}"), &format_hex(color));
		}
	};
	set_color("background", s.bg, orig.bg);
	set_color("foreground", s.fg, orig.fg);
	set_color("cursor", s.cursor, orig.cursor);
	set_color("highlight", s.highlight, orig.highlight);
	set_color("focus", s.focus, orig.focus);
	set_color("gutter", s.gutter, orig.gutter);
	set_color("menu_background", s.menu_bg, orig.menu_bg);
	set_color("menu_foreground", s.menu_fg, orig.menu_fg);
	set_color("dialog_background", s.dialog_bg, orig.dialog_bg);
	set_color("dialog_foreground", s.dialog_fg, orig.dialog_fg);
	// the two scrollbar colors have had dialog rows since the bar shipped but
	// were never written back, so an edit lasted only as long as the session
	set_color("scrollbar_thumb", s.scrollbar_thumb, orig.scrollbar_thumb);
	set_color(
		"scrollbar_trough",
		s.scrollbar_trough,
		orig.scrollbar_trough,
	);

	let cleared = cleared_keys(orig, s);
	let wrote = write_doc(&path, &doc);
	if wrote && !cleared.is_empty() {
		// commented out rather than reverted: the box was cleared, and "not set"
		// is not the same as the value the template ships
		disable_keys(&cleared);
	}
	wrote
}

pub fn format_hex(c: [u8; 3]) -> String {
	format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

// The surface is an sRGB format, so the GPU re-encodes linear->sRGB on write.
// Feed it linear values derived from our sRGB byte colors.
pub fn srgb_f32(c: [u8; 3]) -> [f32; 4] {
	[to_linear(c[0]), to_linear(c[1]), to_linear(c[2]), 1.0]
}

// LUT: this runs per background cell per rebuilt frame (thousands of powf
// calls otherwise - see pane.rs build).
pub fn to_linear(b: u8) -> f32 {
	static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
	LUT.get_or_init(|| std::array::from_fn(|i| linear_of(i as u8)))[b as usize]
}

fn linear_of(b: u8) -> f32 {
	to_linear_f32(f32::from(b) / 255.0)
}

// sRGB -> linear on a 0..1 value rather than a byte.
pub fn to_linear_f32(c: f32) -> f32 {
	let c = c.clamp(0.0, 1.0);
	if c <= 0.04045 {
		c / 12.92
	} else {
		((c + 0.055) / 1.055).powf(2.4)
	}
}

// Inverse of to_linear_f32. The one Rust-side copy - the WGSL lin2srgb in
// gfx.rs/scrim.rs is necessarily separate.
pub fn from_linear(c: f32) -> f32 {
	let c = c.clamp(0.0, 1.0);
	if c <= 0.003_130_8 {
		c * 12.92
	} else {
		1.055 * c.powf(1.0 / 2.4) - 0.055
	}
}

// Encode a linear value back to an sRGB byte.
pub fn from_linear_u8(c: f32) -> u8 {
	(from_linear(c) * 255.0 + 0.5) as u8
}

// Rec.709 luma of an sRGB color, in linear light. Matches contrast.rs and the
// per-pixel weights in autotheme.rs.
pub fn luma(c: [u8; 3]) -> f32 {
	0.2126 * to_linear(c[0]) + 0.7152 * to_linear(c[1]) + 0.0722 * to_linear(c[2])
}

// config file loading

#[derive(Default)]
struct RawConfig {
	use_system_font: Option<bool>,
	use_system_font_size: Option<bool>,
	font_family: Option<String>,
	font_size: Option<f32>,
	line_height_scale: Option<f32>,
	scrollback: Option<usize>,
	scroll_smooth: Option<bool>,
	scroll_ease_in_ms: Option<f32>,
	scroll_ramp_up_ms: Option<f32>,
	scroll_single_screen_tau_ms: Option<f32>,
	scroll_ramp_down_ms: Option<f32>,
	scroll_ease_out_ms: Option<f32>,
	wheel_lines: Option<f32>,
	alt_scroll_lines: Option<f32>,
	output_ease_lines: Option<f32>,
	smooth_scroll_apps: Option<bool>,
	scrollbar: Option<bool>,
	scrollbar_thickness: Option<f32>,
	scrollbar_auto_hide: Option<bool>,
	minimap: Option<bool>,
	minimap_width: Option<f32>,
	minimap_tui_whitelist: Option<String>,
	margin: Option<f32>,
	opacity: Option<f32>,
	transparent_background: Option<bool>,
	transparent_background_blur: Option<bool>,
	wallpaper_enabled: Option<bool>,
	wallpaper: Option<String>,
	wallpaper_fallback_builtin: Option<bool>,
	wallpaper_rotate_enabled: Option<bool>,
	wallpaper_folder: Option<String>,
	wallpaper_rotate_random: Option<bool>,
	wallpaper_rotate_interval_s: Option<f32>,
	wallpaper_opacity: Option<f32>,
	wallpaper_even: Option<f32>,
	wallpaper_default_fit: Option<String>,
	wallpaper_honor_xmp: Option<bool>,
	wallpaper_honor_xmp_look: Option<bool>,
	wallpaper_blur: Option<f32>,
	wallpaper_contrast_mask: Option<bool>,
	wallpaper_contrast_mask_size: Option<f32>,
	wallpaper_contrast_mask_strength: Option<f32>,
	wallpaper_contrast_mask_auto: Option<f32>,
	theme: Option<String>,
	theme_mode: Option<String>,
	text_scrim: Option<bool>,
	text_scrim_radius: Option<f32>,
	text_scrim_softness: Option<f32>,
	text_scrim_strength: Option<f32>,
	text_outline: Option<f32>,
	text_dark_on_light: Option<f32>,
	text_scrim_ramp: Option<String>,
	text_scrim_function: Option<String>,
	text_scrim_regular_weight: Option<bool>,
	text_min_contrast: Option<f32>,
	color_emoji: Option<bool>,
	embolden_inverse: Option<bool>,
	cursor_scrim: Option<bool>,
	cursor_outline: Option<bool>,
	cursor_size_height: Option<f32>,
	cursor_size_width: Option<f32>,
	cursor_animation: Option<String>,
	cursor_animation_resume_s: Option<f32>,
	cursor_animation_idle_stop_s: Option<f32>,
	cursor_blink_rate_ms: Option<f32>,
	columns: Option<usize>,
	rows: Option<usize>,
	remember_size: Option<bool>,
	remember_per_monitor: Option<bool>,
	remember_maximized: Option<bool>,
	hide_single_tab: Option<bool>,
	tab_shows_shell: Option<bool>,
	tab_shows_program: Option<bool>,
	tab_shows_title: Option<bool>,
	tab_shows_directory: Option<bool>,
	title_shows_tab: Option<bool>,
	idle_release: Option<bool>,
	idle_release_minimized_min: Option<usize>,
	idle_release_hidden_min: Option<usize>,
	idle_release_min: Option<usize>,
	software_rendering: Option<bool>,
	tab_regular_pct: Option<f32>,
	tab_max_pct: Option<f32>,
	tab_tip_max_s: Option<f32>,
	remembered_columns: Option<usize>,
	remembered_rows: Option<usize>,
	remembered_maximized: Option<bool>,
	remembered_font_zoom: Option<i64>,
	monitor_sizes: Vec<MonitorSize>,
	word_separators: Option<String>,
	selection_pairs: Option<String>,
	command_line: Option<String>,
	startup_directory: Option<String>,
	copy_on_select: Option<bool>,
	shell_integration: Option<bool>,
	bash_prompt: Option<bool>,
	hyperlinks: Option<bool>,
	hyperlink_open_command: Option<String>,
	performance_automatic: Option<bool>,
	performance_profile: Option<String>,
	performance_check_hardware: Option<bool>,
	performance_check_next_run: Option<bool>,
	rated_hardware: Option<String>,
	colors: RawColors,
	user_themes: Vec<crate::theme::UserTheme>,
	shells: Vec<crate::shells::ShellEntry>,
	keys: Vec<(crate::input::Hotkey, Vec<crate::keys::Chord>)>,
}

#[derive(Default)]
struct RawColors {
	from_wallpaper: Option<bool>,
	background: Option<String>,
	foreground: Option<String>,
	cursor: Option<String>,
	highlight: Option<String>,
	focus: Option<String>,
	menu_background: Option<String>,
	menu_foreground: Option<String>,
	dialog_background: Option<String>,
	dialog_foreground: Option<String>,
	gutter: Option<String>,
	scrollbar_thumb: Option<String>,
	scrollbar_trough: Option<String>,
}

fn load() -> Settings {
	// A launch's own writes come before it says anything about the file.
	*crate::locks::lock(&LAUNCH_SAID) = None;
	adopt_legacy_config();
	let Some(path) = config_path() else {
		return Settings::default();
	};
	if !path.exists() && may_write(&path) {
		if let Some(dir) = path.parent() {
			let _ = std::fs::create_dir_all(dir);
		}
		if let Err(e) = write_config_atomic(&path, default_config()) {
			eprintln!(
				"{APP_NAME}: could not create config {}: {e}",
				path.display()
			);
		}
	}
	// A pre-nesting config converts wholesale first (backed up to .bak, active
	// values carried over). Then migrate an existing config in place
	// (rename/remove changed keys) and backfill any keys it's missing, so an
	// updated config stays current without clobbering the user's existing
	// values. These are the only launch-time writes, and each runs only when
	// the program's own option set changed. The in-place writes defer (with an
	// FYI) if the file looks open in another program. A heading an earlier
	// conversion left holding the wallpaper image is put right before that, or
	// the file reads as pre-nesting and converts again. Ahead of all of it, a
	// file shcl 2.x wrote is respelled for 3.0, since every step parses it, and
	// the 2.x file is kept beside it as `config_backup_<time>_format-v2.shcl`.
	// A current file with lines that are not UTF-8 is written again without
	// them here too, kept the same way, so the launch that finds it says so once
	// and every later save works as usual.
	convert_shcl2_config(&path);
	repair_wallpaper_heading(&path);
	convert_legacy_config(&path);
	adopt_default_shell(&path);
	migrate_config(&path);
	backfill_config(&path);
	refresh_shcl_banner(&path);
	let raw = match read_settings(&path) {
		// The writes above defer when the file looks open elsewhere, so parse the
		// migrated text rather than what is on disk: a renamed key must never be
		// read under its old spelling, which matters most where a rename hands an
		// old name to a new setting (colors.focus).
		Ok(text) => {
			let (raw, said) = read_config_text(&loaded_text(&text), &path);
			for line in &said {
				eprintln!("{line}");
			}
			remember_launch_messages(&path, said);
			raw
		}
		Err(_) => RawConfig::default(),
	};
	resolve(raw)
}

// The values in a config text, and everything a launch prints about it.
fn read_config_text(text: &str, path: &std::path::Path) -> (RawConfig, Vec<String>) {
	let mut said: Vec<String> = config_complaints(text)
		.into_iter()
		.map(|line| format!("{APP_NAME}: {}: {line}", path.display()))
		.collect();
	let (raw, read) = read_raw(text, path);
	said.extend(read);
	(raw, said)
}

// What the last launch printed about the settings file. A write after it can
// move the lines it named: the performance rating adds lines near the top once
// the window is up. Such a write says again whatever now reads differently, so
// the last word names the line the file has.
static LAUNCH_SAID: std::sync::Mutex<Option<(std::path::PathBuf, Vec<String>)>> =
	std::sync::Mutex::new(None);

fn remember_launch_messages(path: &std::path::Path, said: Vec<String>) {
	*crate::locks::lock(&LAUNCH_SAID) = Some((path.to_path_buf(), said));
}

// The messages a write of `text` to `path` brings that were not said already.
// They are remembered as said.
fn restated_launch_messages(path: &std::path::Path, text: &str) -> Vec<String> {
	let mut held = crate::locks::lock(&LAUNCH_SAID);
	let Some((at, said)) = held.as_mut() else {
		return Vec::new();
	};
	if at != path {
		return Vec::new();
	}
	let (_, now) = read_config_text(&loaded_text(text), path);
	let new: Vec<String> = now
		.iter()
		.filter(|line| !said.contains(line))
		.cloned()
		.collect();
	*said = now;
	new
}

// What is wrong with a config file that nothing else says out loud. All three
// were silent, and the first of them permanently stops the program saving.
//
// Subtrees the user fills in themselves (their shells, their themes) are not
// checked for unknown keys - only the settings the program ships.
fn config_complaints(text: &str) -> Vec<String> {
	let doc = shcl::Document::parse(text);
	let mut out = Vec::new();

	// A hotkey that does not read keeps its default, and one set over another's
	// default takes it from that one. Both are easy to miss from the keyboard.
	out.extend(crate::keys::complaints(
		cfg!(target_os = "macos"),
		|path| {
			doc.get_string(path)
				.ok()
				.map(|text| (text, doc.lines(path)))
		},
		line_list,
	));

	let lines = unreadable_lines(&doc);
	if !lines.is_empty() || doc.lost_count() > 0 {
		// shcl writes such a line back as it was, so a save goes through, but it
		// still sets nothing. A save it cannot keep the lines for is refused, and
		// the window says so then.
		out.push(format!(
			"{} line(s) could not be read{} - they are kept but set nothing",
			lines.len().max(doc.lost_count()),
			line_list(&lines)
		));
	}

	// `background: #112233` sets nothing, since `#` starts a comment, and the
	// theme's color is used instead. The walk takes such a line for a heading.
	let source: Vec<&str> = text.lines().collect();
	for w in walk_settings(text) {
		if let WalkLine::Setting {
			index,
			path,
			active: true,
			header: true,
		} = w && let Some(color) = unquoted_color(source[index])
		{
			out.push(format!(
				"`{path}` is empty{} - `#` starts a comment, so write the color in quotes: \"{color}\"",
				line_list(&[index + 1])
			));
		}
	}

	// A key written twice resolves to neither spelling, so the setting is there
	// in the file, plainly set, and doing nothing.
	let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
	let mut active: Vec<(String, usize)> = Vec::new();
	for w in walk_settings(text) {
		// a block header carries no value of its own
		if let WalkLine::Setting {
			index,
			path,
			active: true,
			header: false,
			..
		} = w
		{
			active.push((path, index + 1));
		}
	}
	for (path, _) in &active {
		*seen.entry(path.as_str()).or_insert(0) += 1;
	}
	let mut twice: Vec<&str> = seen
		.iter()
		.filter(|(_, n)| **n > 1)
		.map(|(p, _)| *p)
		.collect();
	twice.sort_unstable();
	for path in twice {
		let at: Vec<usize> = active
			.iter()
			.filter(|(p, _)| p == path)
			.map(|(_, line)| *line)
			.collect();
		out.push(format!(
			"`{path}` is set more than once{} - neither value is used",
			line_list(&at)
		));
	}

	// A key nothing reads is almost always a typo, and it looks exactly like a
	// setting that is not working.
	let known: std::collections::HashSet<String> = setting_lines(default_config())
		.into_iter()
		.map(|(path, _)| path)
		.collect();
	let mut unread: Vec<(String, usize)> = active
		.iter()
		.filter(|(path, _)| {
			!known.contains(path)
				&& !path.starts_with("shells.")
				&& !path.starts_with("themes.")
				&& !is_monitor_size_path(path)
		})
		.cloned()
		.collect();
	unread.sort_unstable();
	unread.dedup_by(|a, b| a.0 == b.0);
	for (path, line) in unread {
		out.push(format!(
			"nothing reads `{path}`{} - check the spelling",
			line_list(&[line])
		));
	}
	out
}

// Typed reads off a parsed document, warning about (and then ignoring) any single
// setting whose value won't coerce. A key that is absent, empty, or unreadable
// comes back None, so `resolve` falls through to that setting's default.
struct Reader<'a> {
	doc: shcl::Document,
	path: &'a std::path::Path,
	said: std::cell::RefCell<Vec<String>>,
}

// The color in `key: #rrggbb` with nothing but a comment after it, which is
// what an unquoted color reads as. Three, six or eight hex digits.
fn unquoted_color(line: &str) -> Option<&str> {
	let (_, value) = line.split_once(':')?;
	let value = value.trim_start();
	let digits = value.strip_prefix('#')?;
	let len = digits
		.find(|c: char| !c.is_ascii_hexdigit())
		.unwrap_or(digits.len());
	let ends = digits[len..].chars().next().is_none_or(char::is_whitespace);
	(matches!(len, 3 | 6 | 8) && ends).then(|| &value[..=len])
}

// " line 4" / " lines 2, 4" for a diagnostic, empty when there is nothing to
// cite (a writer-built node reports 0, and a wildcard slot that missed does too).
fn line_list(lines: &[usize]) -> String {
	let cited: Vec<String> = lines
		.iter()
		.filter(|n| **n > 0)
		.map(std::string::ToString::to_string)
		.collect();
	match cited.len() {
		0 => String::new(),
		1 => format!(" line {}", cited[0]),
		_ => format!(" lines {}", cited.join(", ")),
	}
}

impl Reader<'_> {
	// Complain once about a value that is present but the wrong type. Anything
	// else (absent, empty) is silent - a commented-out setting is the norm here.
	fn note<T>(&self, key: &str, got: Result<T, shcl::Status>) -> Option<T> {
		match got {
			Ok(v) => Some(v),
			Err(shcl::Status::BadType) => {
				self.said.borrow_mut().push(format!(
					"{APP_NAME}: {}{}: ignoring invalid value for `{key}`",
					self.path.display(),
					line_list(&self.doc.lines(key))
				));
				None
			}
			Err(shcl::Status::Multiple) => {
				// Set more than once. shcl refuses to pick a winner, so the
				// default is what actually takes effect - which used to happen
				// in silence, and reads as the setting being ignored outright.
				// Cite every line: the point is that there IS more than one.
				self.said.borrow_mut().push(format!(
					"{APP_NAME}: {}{}: `{key}` is set more than once, so its default is used",
					self.path.display(),
					line_list(&self.doc.lines(key))
				));
				None
			}
			Err(_) => None,
		}
	}
	fn b(&self, key: &str) -> Option<bool> {
		self.note(key, self.doc.get_bool(key))
	}
	fn f(&self, key: &str) -> Option<f32> {
		self.note(key, self.doc.get_float(key)).map(|v| v as f32)
	}
	fn i(&self, key: &str) -> Option<i64> {
		self.note(key, self.doc.get_int(key))
	}
	fn u(&self, key: &str) -> Option<usize> {
		self.note(key, self.doc.get_int(key))
			.map(|v| v.max(0) as usize)
	}
	fn s(&self, key: &str) -> Option<String> {
		self.note(key, self.doc.get_string(key))
	}
}

// Parse the config and pull out every key we know. The parser is forgiving by
// design - a malformed line becomes a diagnostic and is skipped rather than
// sinking the whole document - so this needs no retry loop of its own, and no
// leading-zero rewriting (`.25` is a valid float here). Also answers what the
// read has to say about the file, for the caller to print.
fn read_raw(text: &str, path: &std::path::Path) -> (RawConfig, Vec<String>) {
	let doc = shcl::Document::parse(text);
	let said: Vec<String> = doc
		.diagnostics()
		.iter()
		.filter(|d| matches!(d.severity, shcl::Severity::Error))
		.map(|d| {
			format!(
				"{APP_NAME}: {} line {}: {} [{}]",
				path.display(),
				d.line,
				d.message,
				d.code
			)
		})
		.collect();
	let r = Reader {
		doc,
		path,
		said: std::cell::RefCell::new(said),
	};
	let raw = RawConfig {
		use_system_font: r.b("font.use_system_family"),
		use_system_font_size: r.b("font.use_system_size"),
		font_family: r.s("font.family"),
		font_size: r.f("font.size"),
		line_height_scale: r.f("font.line_height_scale"),
		scrollback: r.u("scroll.scrollback"),
		scroll_smooth: r.b("scroll.smooth"),
		scroll_ease_in_ms: r.f("scroll.ease_in_ms"),
		scroll_ramp_up_ms: r.f("scroll.ramp_up_ms"),
		scroll_single_screen_tau_ms: r.f("scroll.single_screen_tau_ms"),
		scroll_ramp_down_ms: r.f("scroll.ramp_down_ms"),
		scroll_ease_out_ms: r.f("scroll.ease_out_ms"),
		wheel_lines: r.f("scroll.wheel_lines"),
		alt_scroll_lines: r.f("scroll.alt_scroll_lines"),
		output_ease_lines: r.f("scroll.output_ease_lines"),
		smooth_scroll_apps: r.b("scroll.smooth_apps"),
		scrollbar: r.b("scroll.scrollbar.enabled"),
		scrollbar_thickness: r.f("scroll.scrollbar.thickness"),
		scrollbar_auto_hide: r.b("scroll.scrollbar.auto_hide"),
		minimap: r.b("scroll.minimap.enabled"),
		minimap_width: r.f("scroll.minimap.width"),
		minimap_tui_whitelist: r.s("scroll.minimap.tui_process_whitelist"),
		margin: r.f("window.margin"),
		opacity: r.f("transparency.opacity"),
		transparent_background: r.b("transparency.enabled"),
		transparent_background_blur: r.b("transparency.blur_behind"),
		wallpaper_enabled: r.b("wallpaper.enabled"),
		wallpaper: r.s("wallpaper.image"),
		wallpaper_fallback_builtin: r.b("wallpaper.fallback_builtin"),
		wallpaper_rotate_enabled: r.b("wallpaper.rotate.enabled"),
		wallpaper_folder: r.s("wallpaper.rotate.folder"),
		wallpaper_rotate_random: r.b("wallpaper.rotate.random"),
		wallpaper_rotate_interval_s: r.f("wallpaper.rotate.interval_s"),
		wallpaper_opacity: r.f("wallpaper.opacity"),
		wallpaper_even: r.f("wallpaper.even_visibility"),
		wallpaper_default_fit: r.s("wallpaper.default_fit"),
		wallpaper_honor_xmp: r.b("wallpaper.honor_xmp"),
		wallpaper_honor_xmp_look: r.b("wallpaper.honor_xmp_look"),
		wallpaper_blur: r.f("wallpaper.blur"),
		wallpaper_contrast_mask: r.b("wallpaper.contrast_mask.enabled"),
		wallpaper_contrast_mask_size: r.f("wallpaper.contrast_mask.size"),
		wallpaper_contrast_mask_strength: r.f("wallpaper.contrast_mask.strength"),
		wallpaper_contrast_mask_auto: r.f("wallpaper.contrast_mask.auto"),
		theme: r.s("theme"),
		theme_mode: r.s("theme_mode"),
		performance_automatic: r.b("performance.automatic"),
		performance_profile: r.s("performance.profile"),
		performance_check_hardware: r.b("performance.check_hardware"),
		performance_check_next_run: r.b("performance.check_next_run"),
		rated_hardware: r.s("performance.rated_hardware"),
		text_scrim: r.b("text.scrim.enabled"),
		text_scrim_radius: r.f("text.scrim.radius"),
		text_scrim_softness: r.f("text.scrim.softness"),
		text_scrim_strength: r.f("text.scrim.strength"),
		text_outline: r.f("text.outline"),
		text_dark_on_light: r.f("text.dark_on_light"),
		text_scrim_ramp: r.s("text.scrim.ramp"),
		text_scrim_function: r.s("text.scrim.function"),
		text_scrim_regular_weight: r.b("text.scrim.regular_weight"),
		text_min_contrast: r.f("text.min_contrast"),
		color_emoji: r.b("text.color_emoji"),
		embolden_inverse: r.b("text.embolden_inverse"),
		cursor_scrim: r.b("cursor.scrim"),
		cursor_outline: r.b("cursor.outline"),
		cursor_size_height: r.f("cursor.size.height"),
		cursor_size_width: r.f("cursor.size.width"),
		cursor_animation: r.s("cursor.animation"),
		cursor_animation_resume_s: r.f("cursor.animation_resume_s"),
		cursor_animation_idle_stop_s: r.f("cursor.animation_idle_stop_s"),
		cursor_blink_rate_ms: r.f("cursor.blink_rate_ms"),
		columns: r.u("window.columns"),
		rows: r.u("window.rows"),
		remember_size: r.b("window.remember_size"),
		remember_per_monitor: r.b("window.remember_per_monitor"),
		remember_maximized: r.b("window.remember_maximized"),
		hide_single_tab: r.b("window.hide_single_tab"),
		tab_shows_shell: r.b("window.tab_shows_shell"),
		tab_shows_program: r.b("window.tab_shows_program"),
		tab_shows_title: r.b("window.tab_shows_title"),
		tab_shows_directory: r.b("window.tab_shows_directory"),
		title_shows_tab: r.b("window.title_shows_tab"),
		idle_release: r.b("window.idle_release"),
		idle_release_minimized_min: r.u("window.idle_release_minimized_min"),
		idle_release_hidden_min: r.u("window.idle_release_hidden_min"),
		idle_release_min: r.u("window.idle_release_min"),
		software_rendering: r.b("window.software_rendering"),
		tab_regular_pct: r.f("window.tab_regular_width_pct"),
		tab_max_pct: r.f("window.tab_max_width_pct"),
		tab_tip_max_s: r.f("window.tab_tip_max_s"),
		remembered_columns: r.u("window.remembered_columns"),
		remembered_rows: r.u("window.remembered_rows"),
		remembered_maximized: r.b("window.remembered_maximized"),
		remembered_font_zoom: r.i("window.remembered_font_zoom"),
		monitor_sizes: read_monitor_sizes(&r),
		word_separators: r.s("selection.word_separators"),
		selection_pairs: r.s("selection.pairs"),
		command_line: r.s("shell.command_line"),
		startup_directory: r.s("shell.startup_directory"),
		copy_on_select: r.b("shell.copy_on_select"),
		shell_integration: r.b("shell.integration"),
		bash_prompt: r.b("shell.bash_prompt"),
		hyperlinks: r.b("hyperlinks.enabled"),
		hyperlink_open_command: r.s("hyperlinks.open_command"),
		colors: RawColors {
			background: r.s("colors.background"),
			from_wallpaper: r.b("colors.from_wallpaper"),
			foreground: r.s("colors.foreground"),
			cursor: r.s("colors.cursor"),
			highlight: r.s("colors.highlight"),
			focus: r.s("colors.focus"),
			menu_background: r.s("colors.menu_background"),
			menu_foreground: r.s("colors.menu_foreground"),
			dialog_background: r.s("colors.dialog_background"),
			dialog_foreground: r.s("colors.dialog_foreground"),
			gutter: r.s("colors.gutter"),
			scrollbar_thumb: r.s("colors.scrollbar_thumb"),
			scrollbar_trough: r.s("colors.scrollbar_trough"),
		},
		user_themes: read_user_themes(&r.doc),
		shells: read_shells(&r.doc),
		keys: read_keys(&r),
	};
	(raw, r.said.into_inner())
}

// Saved themes, in file order. A slug with no readable colors at all is skipped;
// anything else missing falls back to the first built-in, so a hand-edited or
// half-written block still yields a usable theme rather than none.
fn read_user_themes(doc: &shcl::Document) -> Vec<crate::theme::UserTheme> {
	let base = crate::theme::THEMES[0].1;
	let mut out = Vec::new();
	for slug in doc.children("themes") {
		let read = |mode: &str, fallback: crate::theme::Palette| {
			let mut pal = fallback;
			let mut any = false;
			for (i, key) in crate::theme::PALETTE_KEYS.iter().enumerate() {
				if let Some(c) = doc
					.get_string(&format!("themes.{slug}.{mode}.{key}"))
					.ok()
					.as_deref()
					.and_then(parse_hex)
				{
					pal.set(i, c);
					any = true;
				}
			}
			if let Ok(list) = doc.get_string_array(&format!("themes.{slug}.{mode}.ansi")) {
				for (slot, text) in pal.ansi.iter_mut().zip(list.iter()) {
					if let Some(c) = parse_hex(text) {
						*slot = c;
						any = true;
					}
				}
			}
			(pal, any)
		};
		let (dark, got_dark) = read("dark", base.dark);
		let (light, got_light) = read("light", base.light);
		if !got_dark && !got_light {
			continue;
		}
		let name = doc
			.get_string(&format!("themes.{slug}.name"))
			.ok()
			.filter(|n| !n.trim().is_empty())
			.unwrap_or_else(|| slug.clone());
		out.push(crate::theme::UserTheme {
			slug,
			name,
			dark,
			light,
		});
	}
	out
}

// Bring the file's `themes.*` subtrees in line with the dialog's list. A theme
// that changed at all is dropped and rewritten whole rather than edited field by
// field: saving, renaming and deleting are then one operation with one shape, and
// a stale color cannot survive under a name that no longer sets it.
fn write_user_themes(
	doc: &mut shcl::Document,
	orig: &[crate::theme::UserTheme],
	now: &[crate::theme::UserTheme],
) {
	for old in orig {
		if !now.iter().any(|t| t.slug == old.slug) {
			doc.remove(&format!("themes.{}", old.slug));
		}
	}
	for theme in now {
		if orig.iter().any(|t| t == theme) {
			continue;
		}
		let at = format!("themes.{}", theme.slug);
		doc.remove(&at);
		doc.put_string(&format!("{at}.name"), &theme.name);
		for (mode, pal) in [("dark", &theme.dark), ("light", &theme.light)] {
			for (i, key) in crate::theme::PALETTE_KEYS.iter().enumerate() {
				doc.put_string(&format!("{at}.{mode}.{key}"), &format_hex(pal.get(i)));
			}
			let ansi: Vec<String> = pal.ansi.iter().map(|c| format_hex(*c)).collect();
			let ansi: Vec<&str> = ansi.iter().map(String::as_str).collect();
			doc.put_string_array(&format!("{at}.{mode}.ansi"), &ansi);
		}
	}
}

const MONITOR_SIZE_FIELDS: &[&str] = &["columns", "rows", "font_zoom"];

// `window.monitors.<monitor>.<field>`, for a field an entry is read for.
fn is_monitor_size_path(path: &str) -> bool {
	path.strip_prefix("window.monitors.")
		.and_then(|rest| rest.split_once('.'))
		.is_some_and(|(key, field)| !key.is_empty() && MONITOR_SIZE_FIELDS.contains(&field))
}

// The sizes kept per monitor. An entry with nothing readable sets nothing,
// and a number missing from one takes the default.
fn read_monitor_sizes(r: &Reader) -> Vec<MonitorSize> {
	let mut out = Vec::new();
	for key in r.doc.children("window.monitors") {
		let at = format!("window.monitors.{key}");
		let columns = r.u(&format!("{at}.columns"));
		let rows = r.u(&format!("{at}.rows"));
		let font_zoom = r.i(&format!("{at}.font_zoom"));
		if columns.is_none() && rows.is_none() && font_zoom.is_none() {
			continue;
		}
		let d = Settings::default();
		out.push(MonitorSize {
			columns: numi(columns, d.remembered_columns, limits::GRID),
			rows: numi(rows, d.remembered_rows, limits::GRID),
			font_zoom: zoom(font_zoom, d.remembered_font_zoom),
			key,
		});
	}
	out
}

// Only a number this window changed is written. Every window is its own
// process, so an entry another window saved since this one loaded stays as
// that window left it.
fn write_monitor_sizes(doc: &mut shcl::Document, orig: &[MonitorSize], now: &[MonitorSize]) {
	for entry in now {
		let before = orig.iter().find(|m| m.key == entry.key);
		let at = format!("window.monitors.{}", entry.key);
		if before.is_none_or(|b| b.columns != entry.columns) {
			doc.put_int(&format!("{at}.columns"), entry.columns as i64);
		}
		if before.is_none_or(|b| b.rows != entry.rows) {
			doc.put_int(&format!("{at}.rows"), entry.rows as i64);
		}
		if before.is_none_or(|b| b.font_zoom != entry.font_zoom) {
			doc.put_int(&format!("{at}.font_zoom"), i64::from(entry.font_zoom));
		}
	}
}

// Stored shells, in file order - which IS the menu's order. An entry with no
// command is skipped: a title alone names nothing to run. Everything else falls
// back rather than dropping the entry, so a hand-written or half-written block
// still yields a usable shell.
fn read_shells(doc: &shcl::Document) -> Vec<crate::shells::ShellEntry> {
	let mut out = Vec::new();
	for slug in doc.children("shells") {
		let text = |key: &str| {
			doc.get_string(&format!("shells.{slug}.{key}"))
				.ok()
				.map(|v| v.trim().to_string())
				.filter(|v| !v.is_empty())
		};
		let Some(command) = text("command") else {
			continue;
		};
		out.push(crate::shells::ShellEntry {
			title: text("title").unwrap_or_else(|| slug.clone()),
			command,
			active: doc
				.get_bool(&format!("shells.{slug}.active"))
				.unwrap_or(true),
			comment: text("comment").unwrap_or_default(),
			// A date the file spells any other way is simply not a date we can
			// show, so it reads as "never seen" rather than failing the entry.
			last_seen: doc
				.get_datetime(&format!("shells.{slug}.last_seen"))
				.ok()
				.and_then(|when| when.date)
				.map_or_else(String::new, |(y, m, d)| format!("{y:04}-{m:02}-{d:02}")),
			slug,
		});
	}
	out
}

// Bring the file's `shells.*` subtrees in line with the list in memory. Same
// shape as the themes above - an entry that changed at all is dropped and
// rewritten whole, so a field that no longer has a value cannot survive under a
// key that stopped setting it - and, as there, an entry that did not change is
// not touched, which is what keeps a scan that found nothing new from rewriting
// the file at all.
fn write_shells(
	doc: &mut shcl::Document,
	orig: &[crate::shells::ShellEntry],
	now: &[crate::shells::ShellEntry],
) {
	let disk = read_shells(doc);
	let want = shells_to_save(&disk, orig, now);
	// File ORDER is the list's order - it decides the menu and, at the top, the
	// default shell - and a reorder changes no entry, so the per-entry path below
	// would write nothing at all and lose it. A moved entry therefore rewrites
	// the whole subtree in the new order, which is the only way to say it.
	let order = |list: &[crate::shells::ShellEntry]| -> Vec<String> {
		list.iter().map(|e| e.slug.clone()).collect()
	};
	if order(&disk) != order(&want) {
		for old in &disk {
			doc.remove(&format!("shells.{}", old.slug));
		}
		for entry in &want {
			write_shell(doc, entry);
		}
		return;
	}
	for entry in &want {
		if disk.iter().any(|e| e == entry) {
			continue;
		}
		write_shell(doc, entry);
	}
}

// The list a save leaves in the file. `orig` is what this window loaded and
// `now` its list since, but another window may have saved in between, and the
// window has no file watcher. Diffing `now` against a stale `orig` put another
// window's new find above the whole list, and so made it the default shell:
// only the entries `orig` named were taken out before the rewrite.
//
// So the file is the third side. Another window's new entry stays, at the end,
// and an entry it removed stays gone. An entry this window did not change keeps
// the file's copy. The order is this window's only when it moved something;
// otherwise it is the file's, with this window's new entries after it.
fn shells_to_save(
	disk: &[crate::shells::ShellEntry],
	orig: &[crate::shells::ShellEntry],
	now: &[crate::shells::ShellEntry],
) -> Vec<crate::shells::ShellEntry> {
	use crate::shells::ShellEntry;
	let find = |list: &'_ [ShellEntry], slug: &str| -> Option<ShellEntry> {
		list.iter().find(|e| e.slug == slug).cloned()
	};
	let resolve = |entry: &ShellEntry| match (find(orig, &entry.slug), find(disk, &entry.slug)) {
		(Some(_), None) => None,
		(Some(loaded), Some(stored)) if loaded == *entry => Some(stored),
		_ => Some(entry.clone()),
	};
	let is_new = |entry: &ShellEntry| find(orig, &entry.slug).is_none();
	let kept = |list: &[ShellEntry], other: &[ShellEntry]| -> Vec<String> {
		list.iter()
			.filter(|e| other.iter().any(|o| o.slug == e.slug))
			.map(|e| e.slug.clone())
			.collect()
	};
	let added = now.iter().filter(|e| is_new(e)).count();
	let moved =
		kept(now, orig) != kept(orig, now) || now[now.len() - added..].iter().any(|e| !is_new(e));
	let mut out: Vec<ShellEntry> = Vec::new();
	if moved {
		out.extend(now.iter().filter_map(resolve));
	} else {
		for stored in disk {
			match (find(orig, &stored.slug), find(now, &stored.slug)) {
				(Some(_), None) => {}
				(_, Some(entry)) => out.extend(resolve(&entry)),
				(None, None) => out.push(stored.clone()),
			}
		}
	}
	for entry in now.iter().filter(|e| is_new(e)).chain(disk) {
		if !out.iter().any(|e| e.slug == entry.slug) && find(orig, &entry.slug).is_none() {
			out.push(entry.clone());
		}
	}
	out
}

// One entry's fields, set where they already are. Dropping the subtree first
// would be simpler, but it takes the entry's comments with it and puts it back
// at the end of the block - and the scan stamps `last_seen` daily, so the first
// launch of each day rewrote every entry it saw. The file's order names the
// default shell, so a relocated entry can change which shell a new tab gets.
// The two optional fields are removed rather than left behind when they empty.
fn write_shell(doc: &mut shcl::Document, entry: &crate::shells::ShellEntry) {
	let at = format!("shells.{}", entry.slug);
	doc.put_string(&format!("{at}.title"), &entry.title);
	doc.put_string(&format!("{at}.command"), &entry.command);
	doc.put_bool(&format!("{at}.active"), entry.active);
	if entry.comment.is_empty() {
		let _ = doc.remove(&format!("{at}.comment"));
	} else {
		doc.put_string(&format!("{at}.comment"), &entry.comment);
	}
	match parse_iso_date(&entry.last_seen) {
		Some(when) => doc.put_datetime(&format!("{at}.last_seen"), &when),
		None => {
			doc.remove(&format!("{at}.last_seen"));
		}
	}
}

// "YYYY-MM-DD" as the document's own date type. Anything else is None, so a
// hand-typed value that is not a date is dropped rather than written back.
fn parse_iso_date(text: &str) -> Option<shcl::ShclDateTime> {
	let when = shcl::parse_datetime(text.trim())?;
	when.date.is_some().then_some(when)
}

// Every numeric setting's range, and the two readers that enforce it. A floor on
// its own was the 20260707 `output_ease_lines` defect; fixing that one in place
// left the rest of the table with the same hole. A window in the low thousands
// of columns asks for a texture past the GPU's limit and aborts at launch, and
// an unbounded scrollback grows until the process is killed.
//
// Ceilings are generous - well past anything anyone would set on purpose, and
// well short of what breaks. Both readers fall back to the default rather than
// to an edge when the value is not a number at all: shcl reads `1e400` as
// infinity and reports it good, and infinity survives a clamp.
#[rustfmt::skip]
pub(crate) mod limits {
	pub const FONT_SIZE:          (f32, f32) = (4.0, 400.0);
	pub const LINE_HEIGHT:        (f32, f32) = (0.5, 10.0);
	pub const EASE_MS:            (f32, f32) = (1.0, 60_000.0);
	pub const BLINK_MS:           (f32, f32) = (50.0, 60_000.0);
	pub const WHEEL_LINES:        (f32, f32) = (0.0, 1_000.0);
	pub const MARGIN:             (f32, f32) = (0.0, 1_000.0);
	pub const ROTATE_S:           (f32, f32) = (0.0, 604_800.0);
	pub const GRID:               (usize, usize) = (1, 1_000);
	pub const FONT_ZOOM:          (i32, i32) = (-400, 400); // px; the size it gives is held to 4..128 again
	pub const IDLE_MIN:           (usize, usize) = (1, 10_080); // a week
	pub const SCROLLBACK:         (usize, usize) = (0, 1_000_000);
}

// A number from the file, held to its range.
fn numf(raw: Option<f32>, default: f32, (lo, hi): (f32, f32)) -> f32 {
	match raw {
		Some(v) if v.is_finite() => v.clamp(lo, hi),
		_ => default,
	}
}

fn numi(raw: Option<usize>, default: usize, (lo, hi): (usize, usize)) -> usize {
	raw.map_or(default, |v| v.clamp(lo, hi))
}

fn zoom(raw: Option<i64>, default: i32) -> i32 {
	let (lo, hi) = limits::FONT_ZOOM;
	raw.map_or(default, |v| v.clamp(i64::from(lo), i64::from(hi)) as i32)
}

fn resolve(raw: RawConfig) -> Settings {
	let d = Settings::default();
	let theme_name = raw.theme.unwrap_or_else(|| d.theme.clone());
	let theme_mode = raw.theme_mode.unwrap_or_else(|| d.theme_mode.clone());
	// system-mode OS dark/light detection is wired later; default to dark for now
	let pal = crate::theme::resolve_in(
		&raw.user_themes,
		&theme_name,
		&theme_mode,
		OS_DARK.load(Ordering::Relaxed),
	);
	let color = |raw: Option<String>, fallback: [u8; 3]| {
		raw.as_deref().and_then(parse_hex).unwrap_or(fallback)
	};
	// Default enabled, but a config that predates the key and set an explicit
	// font_family keeps that font (infer off) instead of being overridden.
	let use_system_font = raw.use_system_font.unwrap_or(raw.font_family.is_none());
	// A pinned wallpaper is a deliberate choice, so it suppresses the auto-detected
	// rotation folder; without one, a stocked wallpapers/ dir rotates by itself.
	let pinned_wallpaper = raw
		.wallpaper
		.as_deref()
		.is_some_and(|value| !value.trim().is_empty());
	let wallpaper_folder_raw = raw
		.wallpaper_folder
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.unwrap_or(WALLPAPER_DIR_TOKEN)
		.to_string();
	let (folder, folder_auto) = rotation_folder_for(&wallpaper_folder_raw, pinned_wallpaper);
	let wallpaper_enabled = raw.wallpaper_enabled.unwrap_or(d.wallpaper_enabled);
	let wallpaper_rotate_enabled = raw
		.wallpaper_rotate_enabled
		.unwrap_or(d.wallpaper_rotate_enabled);
	// With rotation live, don't also hunt for a conventional wallpaper file: that
	// is a run of stats on paths which may be a slow mount, and the first rotation
	// pick replaces whatever it found anyway.
	let rotating = wallpaper_enabled && wallpaper_rotate_enabled && folder.is_some();
	let wallpaper = (pinned_wallpaper || !rotating)
		.then(|| resolve_wallpaper(raw.wallpaper.clone()))
		.flatten();
	Settings {
		// only the convention folder is "auto"; whether it holds anything is the
		// scan's business, and the scan runs off this thread
		wallpaper_folder_auto: folder_auto,
		use_system_font,
		// absent = follow the face toggle, so configs predating the split (and an
		// explicit font_size, which used to imply off) keep their exact behavior
		use_system_font_size: raw
			.use_system_font_size
			.unwrap_or(use_system_font && raw.font_size.is_none()),
		font_family: raw.font_family.filter(|s| !s.trim().is_empty()),
		font_size: numf(raw.font_size, default_font_size(), limits::FONT_SIZE),
		line_height_scale: numf(
			raw.line_height_scale,
			d.line_height_scale,
			limits::LINE_HEIGHT,
		),
		scrollback: numi(raw.scrollback, d.scrollback, limits::SCROLLBACK),
		scroll_smooth: raw.scroll_smooth.unwrap_or(d.scroll_smooth),
		scroll_ease_in_ms: numf(raw.scroll_ease_in_ms, d.scroll_ease_in_ms, limits::EASE_MS),
		scroll_ramp_up_ms: numf(raw.scroll_ramp_up_ms, d.scroll_ramp_up_ms, limits::EASE_MS),
		scroll_single_screen_tau_ms: numf(
			raw.scroll_single_screen_tau_ms,
			d.scroll_single_screen_tau_ms,
			limits::EASE_MS,
		),
		scroll_ramp_down_ms: numf(
			raw.scroll_ramp_down_ms,
			d.scroll_ramp_down_ms,
			limits::EASE_MS,
		),
		scroll_ease_out_ms: numf(
			raw.scroll_ease_out_ms,
			d.scroll_ease_out_ms,
			limits::EASE_MS,
		),
		wheel_lines: numf(raw.wheel_lines, d.wheel_lines, limits::WHEEL_LINES),
		alt_scroll_lines: numf(
			raw.alt_scroll_lines,
			d.alt_scroll_lines,
			limits::WHEEL_LINES,
		),
		// MUST clamp: scroll's backlog clamp uses this as its lower bound, and
		// f32::clamp panics (aborts, in release) when min > max - an over-range
		// value here killed the terminal on the first scrolling output.
		output_ease_lines: raw
			.output_ease_lines
			.unwrap_or(d.output_ease_lines)
			.clamp(0.0, crate::scroll::MAX_BACKLOG),
		smooth_scroll_apps: raw.smooth_scroll_apps.unwrap_or(d.smooth_scroll_apps),
		scrollbar: raw.scrollbar.unwrap_or(d.scrollbar),
		// floor keeps it grabbable; ceiling keeps it from swallowing a narrow pane
		scrollbar_thickness: raw
			.scrollbar_thickness
			.unwrap_or(d.scrollbar_thickness)
			.clamp(4.0, 64.0),
		scrollbar_auto_hide: raw.scrollbar_auto_hide.unwrap_or(d.scrollbar_auto_hide),
		minimap: raw.minimap.unwrap_or(d.minimap),
		// the column has to be wide enough to read and narrow enough to spare
		minimap_width: raw
			.minimap_width
			.unwrap_or(d.minimap_width)
			.clamp(24.0, 400.0),
		minimap_tui_whitelist: raw.minimap_tui_whitelist.unwrap_or(d.minimap_tui_whitelist),
		margin: numf(raw.margin, d.margin, limits::MARGIN),
		opacity: raw.opacity.unwrap_or(d.opacity).clamp(0.0, 1.0),
		transparent_background: raw
			.transparent_background
			.unwrap_or(d.transparent_background),
		transparent_background_blur: raw
			.transparent_background_blur
			.unwrap_or(d.transparent_background_blur),
		wallpaper_enabled,
		wallpaper_raw: raw.wallpaper.clone().unwrap_or_default(),
		wallpaper,
		wallpaper_fallback_builtin: raw
			.wallpaper_fallback_builtin
			.unwrap_or(d.wallpaper_fallback_builtin),
		wallpaper_rotate_enabled,
		wallpaper_folder: folder,
		wallpaper_folder_raw,
		wallpaper_rotate_random: raw
			.wallpaper_rotate_random
			.unwrap_or(d.wallpaper_rotate_random),
		wallpaper_rotate_interval_s: numf(
			raw.wallpaper_rotate_interval_s,
			d.wallpaper_rotate_interval_s,
			limits::ROTATE_S,
		),
		wallpaper_opacity: raw
			.wallpaper_opacity
			.unwrap_or(d.wallpaper_opacity)
			.clamp(0.0, 1.0),
		wallpaper_even: raw
			.wallpaper_even
			.unwrap_or(d.wallpaper_even)
			.clamp(0.0, 1.0),
		wallpaper_blur: raw
			.wallpaper_blur
			.unwrap_or(d.wallpaper_blur)
			.clamp(0.0, 100.0),
		wallpaper_contrast_mask: raw
			.wallpaper_contrast_mask
			.unwrap_or(d.wallpaper_contrast_mask),
		wallpaper_contrast_mask_size: raw
			.wallpaper_contrast_mask_size
			.unwrap_or(d.wallpaper_contrast_mask_size)
			.clamp(0.0, 1.0),
		wallpaper_contrast_mask_strength: raw
			.wallpaper_contrast_mask_strength
			.unwrap_or(d.wallpaper_contrast_mask_strength)
			.clamp(0.0, 1.0),
		wallpaper_contrast_mask_auto: raw
			.wallpaper_contrast_mask_auto
			.unwrap_or(d.wallpaper_contrast_mask_auto)
			.clamp(0.0, 1.0),
		text_scrim: raw.text_scrim.unwrap_or(d.text_scrim),
		text_scrim_radius: raw
			.text_scrim_radius
			.unwrap_or(d.text_scrim_radius)
			.clamp(0.0, 50.0),
		text_scrim_softness: raw
			.text_scrim_softness
			.unwrap_or(d.text_scrim_softness)
			.clamp(0.0, 1.0),
		text_scrim_strength: raw
			.text_scrim_strength
			.unwrap_or(d.text_scrim_strength)
			.clamp(0.0, 100.0),
		text_outline: raw.text_outline.unwrap_or(d.text_outline).clamp(0.0, 8.0),
		text_dark_on_light: raw
			.text_dark_on_light
			.unwrap_or(d.text_dark_on_light)
			.clamp(0.0, MAX_DARK_ON_LIGHT),
		// the older spellings still parse: "s" was renamed to "sigmoid" (which is
		// what a smoothstep is), and the falloff's "gaussian" to "half_normal" so
		// it stops reading like the gaussian BLUR the function list also offers.
		text_scrim_ramp: match raw.text_scrim_ramp.as_deref() {
			Some("linear") => "linear".to_string(),
			Some("half_normal" | "gaussian") => "half_normal".to_string(),
			Some("sigmoid" | "s") => "sigmoid".to_string(),
			Some("log") => "log".to_string(),
			Some("exp") => "exp".to_string(),
			_ => d.text_scrim_ramp.clone(), // missing/unknown -> default (exponential)
		},
		text_scrim_function: match raw.text_scrim_function.as_deref() {
			Some("dilate") => "dilate".to_string(),
			Some("sdf") => "sdf".to_string(),
			Some("dt") => "dt".to_string(),
			Some("gaussian") => "gaussian".to_string(),
			_ => d.text_scrim_function.clone(), // missing/unknown -> default (SDF)
		},
		text_scrim_regular_weight: raw
			.text_scrim_regular_weight
			.unwrap_or(d.text_scrim_regular_weight),
		text_min_contrast: raw
			.text_min_contrast
			.unwrap_or(d.text_min_contrast)
			.clamp(0.0, 0.6),
		color_emoji: raw.color_emoji.unwrap_or(d.color_emoji),
		embolden_inverse: raw.embolden_inverse.unwrap_or(d.embolden_inverse),
		cursor_scrim: raw.cursor_scrim.unwrap_or(d.cursor_scrim),
		cursor_outline: raw.cursor_outline.unwrap_or(d.cursor_outline),
		cursor_size_height: raw
			.cursor_size_height
			.unwrap_or(d.cursor_size_height)
			.clamp(1.0, 100.0),
		cursor_size_width: raw
			.cursor_size_width
			.unwrap_or(d.cursor_size_width)
			.clamp(1.0, 100.0),
		cursor_animation: raw.cursor_animation.unwrap_or(d.cursor_animation),
		cursor_animation_resume_s: raw
			.cursor_animation_resume_s
			.unwrap_or(d.cursor_animation_resume_s)
			.clamp(0.05, 3600.0),
		cursor_animation_idle_stop_s: raw
			.cursor_animation_idle_stop_s
			.unwrap_or(d.cursor_animation_idle_stop_s)
			.clamp(0.0, 86400.0),
		cursor_blink_rate_ms: numf(
			raw.cursor_blink_rate_ms,
			d.cursor_blink_rate_ms,
			limits::BLINK_MS,
		),
		wallpaper_default_fit: match raw.wallpaper_default_fit.as_deref() {
			Some("zoom") => Fit::Zoom,
			_ => Fit::Stretch,
		},
		wallpaper_honor_xmp: raw.wallpaper_honor_xmp.unwrap_or(d.wallpaper_honor_xmp),
		wallpaper_honor_xmp_look: raw
			.wallpaper_honor_xmp_look
			.unwrap_or(d.wallpaper_honor_xmp_look),
		columns: numi(raw.columns, d.columns, limits::GRID),
		rows: numi(raw.rows, d.rows, limits::GRID),
		remember_size: raw.remember_size.unwrap_or(d.remember_size),
		remember_per_monitor: raw.remember_per_monitor.unwrap_or(d.remember_per_monitor),
		remember_maximized: raw.remember_maximized.unwrap_or(d.remember_maximized),
		hide_single_tab: raw.hide_single_tab.unwrap_or(d.hide_single_tab),
		tab_shows_shell: raw.tab_shows_shell.unwrap_or(d.tab_shows_shell),
		tab_shows_program: raw.tab_shows_program.unwrap_or(d.tab_shows_program),
		tab_shows_title: raw.tab_shows_title.unwrap_or(d.tab_shows_title),
		tab_shows_directory: raw.tab_shows_directory.unwrap_or(d.tab_shows_directory),
		title_shows_tab: raw.title_shows_tab.unwrap_or(d.title_shows_tab),
		idle_release: raw.idle_release.unwrap_or(d.idle_release),
		idle_release_minimized_min: numi(
			raw.idle_release_minimized_min,
			d.idle_release_minimized_min,
			limits::IDLE_MIN,
		),
		idle_release_hidden_min: numi(
			raw.idle_release_hidden_min,
			d.idle_release_hidden_min,
			limits::IDLE_MIN,
		),
		idle_release_min: numi(raw.idle_release_min, d.idle_release_min, limits::IDLE_MIN),
		software_rendering: raw.software_rendering.unwrap_or(d.software_rendering),
		tab_regular_pct: raw
			.tab_regular_pct
			.unwrap_or(d.tab_regular_pct)
			.clamp(2.0, 100.0),
		// The pair is read as a range wherever it is used (tabtitle::bounds), so a
		// maximum dragged below the regular width is stored as it was set rather
		// than quietly rewritten under the user.
		tab_max_pct: raw.tab_max_pct.unwrap_or(d.tab_max_pct).clamp(2.0, 100.0),
		tab_tip_max_s: raw.tab_tip_max_s.unwrap_or(d.tab_tip_max_s).max(0.0),
		remembered_columns: numi(raw.remembered_columns, d.remembered_columns, limits::GRID),
		remembered_rows: numi(raw.remembered_rows, d.remembered_rows, limits::GRID),
		remembered_maximized: raw.remembered_maximized.unwrap_or(d.remembered_maximized),
		remembered_font_zoom: zoom(raw.remembered_font_zoom, d.remembered_font_zoom),
		monitor_sizes: raw.monitor_sizes,
		word_separators: raw.word_separators.unwrap_or(d.word_separators),
		selection_pairs: raw.selection_pairs.unwrap_or(d.selection_pairs),
		command_line: raw.command_line.unwrap_or(d.command_line),
		startup_directory: raw.startup_directory.unwrap_or(d.startup_directory),
		copy_on_select: raw.copy_on_select.unwrap_or(d.copy_on_select),
		shell_integration: raw.shell_integration.unwrap_or(d.shell_integration),
		bash_prompt: raw.bash_prompt.unwrap_or(d.bash_prompt),
		hyperlinks: raw.hyperlinks.unwrap_or(d.hyperlinks),
		hyperlink_open_command: raw
			.hyperlink_open_command
			.unwrap_or(d.hyperlink_open_command),
		bg: color(raw.colors.background, pal.bg),
		colors_from_wallpaper: raw.colors.from_wallpaper.unwrap_or(d.colors_from_wallpaper),
		// Session only: the summary arrives with a picture and the shadow holds
		// the user's own colors while a derived pair is live.
		wallpaper_summary: None,
		wallpaper_colors: None,
		fg: color(raw.colors.foreground, pal.fg),
		cursor: color(raw.colors.cursor, pal.cursor),
		highlight: color(raw.colors.highlight, pal.highlight),
		focus: color(raw.colors.focus, pal.focus),
		menu_bg: color(raw.colors.menu_background, pal.menu_bg),
		menu_fg: color(raw.colors.menu_foreground, pal.menu_fg),
		dialog_bg: color(raw.colors.dialog_background, pal.dialog_bg),
		dialog_fg: color(raw.colors.dialog_foreground, pal.dialog_fg),
		gutter: color(raw.colors.gutter, pal.gutter),
		scrollbar_thumb: color(raw.colors.scrollbar_thumb, SCROLLBAR_THUMB_DEF),
		scrollbar_trough: color(raw.colors.scrollbar_trough, SCROLLBAR_TROUGH_DEF),
		ansi: pal.ansi,
		theme: theme_name,
		theme_mode,
		performance_automatic: raw.performance_automatic.unwrap_or(d.performance_automatic),
		performance_profile: crate::profile::Profile::parse(
			raw.performance_profile
				.as_deref()
				.unwrap_or(&d.performance_profile),
		)
		.key()
		.to_string(),
		performance_check_hardware: raw
			.performance_check_hardware
			.unwrap_or(d.performance_check_hardware),
		performance_check_next_run: raw
			.performance_check_next_run
			.unwrap_or(d.performance_check_next_run),
		rated_hardware: raw.rated_hardware.unwrap_or_default(),
		profile_shadow: None,
		remote_override: false,
		stepped_profile: None,
		user_themes: raw.user_themes,
		shells: raw.shells,
		keys: crate::keys::Bindings::with(cfg!(target_os = "macos"), &raw.keys).0,
		#[cfg(test)]
		clone_probe: CloneProbe,
	}
}

// The hotkeys the file sets. A value that does not read is left out, so its
// default stays: the reader reports one that is not text, and
// `config_complaints` one that names no key.
fn read_keys(r: &Reader) -> Vec<(crate::input::Hotkey, Vec<crate::keys::Chord>)> {
	crate::keys::config_paths()
		.filter_map(|(hotkey, path)| {
			let text = r.s(&path)?;
			if text.trim().is_empty() {
				return None;
			}
			crate::keys::parse_value(&text)
				.ok()
				.map(|chords| (hotkey, chords))
		})
		.collect()
}

// Put a hotkey's changed value in the file, by the platform's own names. Only
// the value set for that hotkey is written, never what it was left with after
// another took a chord from it, so the file says what a hand edit would. One
// put back to its default writes nothing here: the Settings revert puts the
// template's line back (`revert_keys`).
fn write_keys(doc: &mut shcl::Document, orig: &crate::keys::Bindings, now: &crate::keys::Bindings) {
	for (hotkey, path) in crate::keys::config_paths() {
		let own = now.own(hotkey);
		if own == orig.own(hotkey) {
			continue;
		}
		if let Some(chords) = own {
			doc.put_string(
				&path,
				&crate::keys::value_text(chords, cfg!(target_os = "macos")),
			);
		}
	}
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
	let s = s.trim().trim_start_matches('#');
	// six BYTES is not six digits: a value carrying a multi-byte character is the
	// right length and splits mid-character, which used to abort at launch
	if s.len() != 6 || !s.is_ascii() {
		return None;
	}
	Some([
		u8::from_str_radix(&s[0..2], 16).ok()?,
		u8::from_str_radix(&s[2..4], 16).ok()?,
		u8::from_str_radix(&s[4..6], 16).ok()?,
	])
}

// Default font size (logical px) when the user hasn't set one: follow the OS's
// monospace size if we can detect it, else FALLBACK_FONT_SIZE.
pub fn default_font_size() -> f32 {
	crate::sysfont::monospace()
		.size_pt
		.map(crate::sysfont::px_from_pt)
		.filter(|px| *px >= 4.0)
		.unwrap_or(FALLBACK_FONT_SIZE)
}

// Whether "use system font" actually has an OS monospace setting to follow.
// Face and size follow the OS independently (the Settings dual checkboxes), and
// each is inert unless the OS really reports that half: Windows has a system
// font SIZE (the message-box font) but no monospace FAMILY, and a Linux desktop
// with no readable font setting reports neither. Keying on what was detected
// rather than on the platform keeps one rule everywhere - a toggle with nothing
// to follow resolves from font_family / font_size as if off, and grays out.
pub fn system_font_face_active(s: &Settings) -> bool {
	s.use_system_font && crate::sysfont::monospace().family.is_some()
}
pub fn system_font_size_active(s: &Settings) -> bool {
	s.use_system_font_size && crate::sysfont::monospace().size_pt.is_some()
}

// Font zoom (Ctrl+-/+/= hotkeys), in logical px added to the effective size.
// Kept with the window size while remember_size is on (remembered_window).
// Process-wide is per-window since each window is its own process. Per-pane
// scoping is deferred - it needs per-pane text metrics the single-TextCtx
// architecture doesn't have.
static FONT_ZOOM_PX: AtomicI32 = AtomicI32::new(0);
pub fn font_zoom_px() -> i32 {
	FONT_ZOOM_PX.load(Ordering::Relaxed)
}
// Step the zoom, clamped so the effective size stays renderable - stepping
// past the floor must not bank offset the other direction has to pay back.
pub fn nudge_font_zoom(dir: i32) {
	let current = settings();
	let base = if system_font_size_active(&current) {
		default_font_size()
	} else {
		current.font_size
	};
	FONT_ZOOM_PX.store(zoom_within(font_zoom_px() + dir, base), Ordering::Relaxed);
}
// Put the zoom at `px`, held the same way a step is.
pub fn set_font_zoom(px: i32) {
	let current = settings();
	let base = if system_font_size_active(&current) {
		default_font_size()
	} else {
		current.font_size
	};
	FONT_ZOOM_PX.store(zoom_within(px, base), Ordering::Relaxed);
}
fn zoom_within(px: i32, base: f32) -> i32 {
	px.clamp((4.0 - base).ceil() as i32, (128.0 - base).floor() as i32)
}
// Drop the session zoom, back to the configured (or system) size.
pub fn reset_font_zoom() {
	FONT_ZOOM_PX.store(0, Ordering::Relaxed);
}

// The size the text is actually rendered at: the OS monospace size while
// `use_system_font_size` is on (and the OS has one), else the configured
// `font_size`; plus any session zoom, clamped to a renderable range.
pub fn effective_font_size() -> f32 {
	let current = settings();
	let base = if system_font_size_active(&current) {
		default_font_size()
	} else {
		current.font_size
	};
	(base + font_zoom_px() as f32).clamp(4.0, 128.0)
}

// Resolve the background image: an explicit path (absolute, or a filename
// relative to the config dir), else auto-detect backgrounds/background.{png,jpg,jpeg}
// under the config dir. The value is text a person edits by hand, so it goes
// through the same expander the startup directory does - `~` and the three
// spellings of an environment variable.
pub fn resolve_wallpaper(explicit: Option<String>) -> Option<PathBuf> {
	let dir = config_dir()?;
	if let Some(given) = explicit.filter(|value| !value.trim().is_empty()) {
		let path = PathBuf::from(expand_vars(given.trim()));
		// Handed back unchecked: the loader opens it on its own thread and says so
		// if it can't, which keeps a wallpaper the user explicitly named from
		// costing a stat here - it may be the very mount that answers slowly.
		return Some(if path.is_absolute() {
			path
		} else {
			dir.join(path)
		});
	}
	// Current convention first (wallpaper/wallpaper.*), then the older spellings
	// so existing setups keep working - across every directory a pack may sit in,
	// which on Windows means Local before the config dir it used to share.
	wallpaper_search_dirs()
		.into_iter()
		.flat_map(|dir| {
			[
				("wallpaper", "wallpaper"),
				("wallpapers", "wallpaper"),
				("backgrounds", "background"),
			]
			.into_iter()
			.flat_map(move |(sub, stem)| {
				let sub_dir = dir.join(sub);
				["png", "jpg", "jpeg"]
					.into_iter()
					.map(move |ext| sub_dir.join(format!("{stem}.{ext}")))
			})
		})
		.find(|path| path.exists())
}

// The wallpaper-rotation folder: a relative value resolves against the config
// dir (like the single wallpaper). Not checked for existence here - the scan
// runs off the startup thread and reports an unreadable folder itself, so a typo
// still just leaves rotation off.
pub fn resolve_wallpaper_folder(explicit: Option<String>) -> Option<PathBuf> {
	let given = explicit.filter(|value| {
		let value = value.trim();
		!value.is_empty() && value != WALLPAPER_DIR_TOKEN
	})?;
	let path = PathBuf::from(expand_vars(given.trim()));
	if path.is_absolute() {
		return Some(path);
	}
	Some(config_dir()?.join(&path))
}

// Enough kept resets that nobody hits the ceiling in practice, low enough that a
// script looping on --reset-config stops piling up files instead of forever.
const BACKUPS_MAX: u32 = 99;

// Chromium's retry for the same antivirus and indexer locks. A restore runs
// only when a replace took the file, so the wait costs nothing otherwise.
const RESTORE_ATTEMPTS: u32 = 5;
const RESTORE_PAUSE: std::time::Duration = std::time::Duration::from_millis(100);

// Move a config aside to the first free `.bak` name - so doing it twice never
// overwrites the copy from the first time. Returns where it went.
fn backup_aside(path: &std::path::Path) -> Option<PathBuf> {
	if !may_write(path) {
		return None;
	}
	let name = path.file_name()?.to_string_lossy().into_owned();
	let backup = (1u32..=BACKUPS_MAX)
		.map(|n| match n {
			1 => path.with_file_name(format!("{name}.bak")),
			_ => path.with_file_name(format!("{name}.bak{n}")),
		})
		.find(|candidate| !candidate.exists());
	let Some(backup) = backup else {
		eprintln!(
			"{APP_NAME}: {BACKUPS_MAX} config backups already in {}; clear some out first",
			path.display()
		);
		return None;
	};
	match std::fs::rename(path, &backup) {
		Ok(()) => Some(backup),
		Err(e) => {
			eprintln!("{APP_NAME}: could not move {} aside: {e}", path.display());
			None
		}
	}
}

// Copy a config to the first free `.bak` name, for a rewrite that leaves the
// file where it is. `create_new` skips a name held by anything, a dangling link
// included, so nothing is written through a link left there. The body is the
// text the caller read, so the backup is exactly what was rewritten.
fn backup_copy(path: &std::path::Path, body: &[u8]) -> Option<PathBuf> {
	use std::io::Write;
	if !may_write(path) {
		return None;
	}
	let name = path.file_name()?.to_string_lossy().into_owned();
	#[cfg(unix)]
	let perms = std::fs::metadata(path).ok()?.permissions();
	for n in 1u32..=BACKUPS_MAX {
		let backup = match n {
			1 => path.with_file_name(format!("{name}.bak")),
			_ => path.with_file_name(format!("{name}.bak{n}")),
		};
		let mut file = match std::fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&backup)
		{
			Ok(file) => file,
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
			Err(e) => {
				eprintln!("{APP_NAME}: could not back up {}: {e}", path.display());
				return None;
			}
		};
		// Private before it holds anything. Elsewhere the mode is only a read-only
		// flag: it adds no privacy (a new file takes its folder's ACLs), and it
		// would stop a failed conversion removing the backup it just made.
		#[cfg(unix)]
		let written = std::fs::set_permissions(&backup, perms).and_then(|()| file.write_all(body));
		#[cfg(not(unix))]
		let written = file.write_all(body);
		if let Err(e) = written.and_then(|()| file.sync_all()) {
			eprintln!("{APP_NAME}: could not back up {}: {e}", path.display());
			let _ = std::fs::remove_file(&backup);
			return None;
		}
		return Some(backup);
	}
	eprintln!(
		"{APP_NAME}: {BACKUPS_MAX} config backups already in {}; clear some out first",
		path.display()
	);
	None
}

// Move the config aside so the next load writes a fresh one from the template.
// The old file is kept, not deleted. Returns where it went, or None if there
// was nothing to move.
pub fn reset_config() -> Option<PathBuf> {
	let path = config_path()?;
	if !path.exists() {
		return None;
	}
	backup_aside(&path)
}

// Where the wallpaper shuffle keeps its recently-shown list. Beside the config,
// so a --config override gets its own history instead of sharing one.
pub fn wallpaper_history_path() -> Option<PathBuf> {
	let name = ".wallpaper-history";
	let beside_config = config_dir().map(|dir| dir.join(name));
	if let Some(old) = beside_config.filter(|p| p.exists()) {
		return Some(old);
	}
	Some(data_dir()?.join(name))
}

// Image files we're willing to load as a wallpaper. One list, so the folder
// auto-detect below and the rotation scan can't disagree about what counts.
// Must track the `image` crate's enabled features (png + jpeg) - it is built
// with default-features off to keep the binary small, so listing anything else
// here just picks a file that then fails to decode.
pub fn is_image_file(path: &std::path::Path) -> bool {
	path.extension()
		.and_then(|ext| ext.to_str())
		.is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

// The rotation folder a configured value comes to, and whether it was found by
// convention. Empty and the shipped default both mean the usual place, which is
// looked up rather than expanded: it holds the older spellings, and on Windows
// a pack left beside the config, and it follows `--config` and XDG_CONFIG_HOME.
// A named image outranks a folder found that way but not one configured. The
// loader and the Settings dialog both come here, so the two cannot disagree.
pub fn rotation_folder_for(raw: &str, pinned_wallpaper: bool) -> (Option<PathBuf>, bool) {
	match resolve_wallpaper_folder(Some(raw.to_string())) {
		Some(folder) => (Some(folder), false),
		None => (
			(!pinned_wallpaper).then(default_wallpaper_folder).flatten(),
			true,
		),
	}
}

// The rotation folder to use when none is configured: the conventional
// wallpaper/ dir (or the legacy spellings) under the config dir. Only its
// existence is tested - reading it to see whether it holds an image is the
// scan's job, off the startup thread, and an empty one still means no rotation
// and no diagnostic, since the user never asked for one.
fn default_wallpaper_folder() -> Option<PathBuf> {
	wallpaper_search_dirs()
		.into_iter()
		.flat_map(|dir| {
			["wallpaper", "wallpapers", "backgrounds"]
				.into_iter()
				.map(move |sub| dir.join(sub))
		})
		.find(|sub| sub.is_dir())
}

// Every setting line of `text` as (full path, verbatim line), commented ones
// included - the walker resolves nesting, so a `# height: 100` two levels down
// comes back as "cursor.size.height".
fn setting_lines(text: &str) -> Vec<(String, String)> {
	let lines: Vec<&str> = text.lines().collect();
	walk_settings(text)
		.into_iter()
		.filter_map(|w| match w {
			WalkLine::Setting { index, path, .. } => Some((path, lines[index].to_string())),
			_ => None,
		})
		.collect()
}

// Like `setting_lines`, but each setting carries the contiguous comment lines
// directly above it (its block), plus `new_group` = whether a blank line
// precedes it in the template. Backfill uses this to keep a template group's
// settings together (no internal blank) while separating groups by a blank line.
fn setting_groups(text: &str) -> Vec<(String, Vec<String>, bool)> {
	let lines: Vec<&str> = text.lines().collect();
	let mut pending: Vec<String> = Vec::new();
	let mut group_break = true; // the first setting begins a group
	let mut out = Vec::new();
	for w in walk_settings(text) {
		match w {
			WalkLine::Setting { index, path, .. } => {
				let mut block = std::mem::take(&mut pending);
				block.push(lines[index].to_string());
				out.push((path, block, group_break));
				group_break = false;
			}
			WalkLine::Blank => {
				pending.clear();
				group_break = true;
			}
			WalkLine::Other(index) if lines[index].trim_start().starts_with('#') => {
				pending.push(lines[index].to_string());
			}
			_ => pending.clear(),
		}
	}
	out
}

// The key of a settings line, active or commented-out. Dots are part of the key
// ("colors.foreground"), so a dotted setting stays one self-contained line; the
// walker supplies the enclosing-block context for truly nested ones.
fn line_setting_key(line: &str) -> Option<&str> {
	let trimmed = line.trim_start();
	let trimmed = trimmed.strip_prefix('#').map_or(trimmed, str::trim_start);
	let end = trimmed
		.find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'))?;
	let key = &trimmed[..end];
	if key.is_empty() {
		return None;
	}
	trimmed[end..].trim_start().starts_with(':').then_some(key)
}

// Paths that were renamed across versions (old -> new). A rename rewrites the
// key on its line, preserving the comment/active state; if the new path is
// already present the old one is just dropped. Renames must stay within the
// same parent block - the machinery rewrites the line in place, it does not
// move lines between blocks. (The whole pre-nesting flat namespace is handled
// separately by `convert_legacy_config`, not here.)
const CONFIG_RENAMES: &[(&str, &str)] = &[
	("scroll.inview_tau_ms", "scroll.single_screen_tau_ms"),
	// The one attention color became two. The old key's value IS the calmer of
	// the pair, so it carries to `highlight` and the freed-up `colors.focus`
	// starts from its own default. That reuse is the reason `load` migrates the
	// text it parses as well as the file: a config open in an editor defers the
	// write, and the old spelling must not read as its successor even once.
	("colors.focus", "colors.highlight"),
	// the narrowest a tab could be became the width it sits at by default; the
	// old value carries, since both are read the same way
	("window.tab_min_width_pct", "window.tab_regular_width_pct"),
	(
		"scroll.minimap.keep_for",
		"scroll.minimap.tui_process_whitelist",
	),
];
// Paths that no longer exist and should be removed from an existing config.
// scroll.tau_ms ("Initial scroll speed") has no successor: the speed curve now
// leaves rest through Ease-in and the one knob that fed four mechanisms is
// gone. scroll.ease_in was a unitless fraction; its replacement is a duration
// (scroll.ease_in_ms), so the old value cannot be carried by a rename.
// text.dark_on_light_gamma is the same: it named a coverage exponent, and its
// replacement (text.dark_on_light) is how much of a correction to apply, so
// the old number means nothing under the new one.
// `shell.default` is here because the list itself now names the default (its
// top active entry). Adoption runs BEFORE this drops the line - see
// `adopt_default_shell` - so the value is moved into the list, not discarded.
const CONFIG_REMOVED: &[&str] = &[
	"scroll.tau_ms",
	"scroll.ease_in",
	"shell.default",
	"text.dark_on_light_gamma",
];

// Defaults that changed, as (path, the value that used to be the default). An
// existing config carries the template's commented lines verbatim, so after a
// default changes those lines quietly describe the old behavior. A commented
// line matching the outgoing default is refreshed to the current template line.
// An ACTIVE line is never touched: that value is the user's own choice, and it
// keeps working exactly as they set it. NOTE: the stored value is the raw
// post-colon text, trailing `## Default` marker included.
const SUPERSEDED_DEFAULTS: &[(&str, &str)] = &[
	// the falloff's "gaussian" is spelled "half_normal" now (both still parse),
	// and the shipped curve moved on again to the exponential
	("text.scrim.ramp", "\"gaussian\"  ## Default"),
	("text.scrim.ramp", "\"half_normal\"  ## Default"),
	// the halo used to ship exactly as built, before the scale halved. 20 is the
	// current value and so is not listed, though it did ship once before.
	("text.scrim.strength", "0  ## Default"),
	("text.scrim.strength", "30  ## Default"),
	("text.scrim.strength", "15  ## Default"),
	// the exponential falloff got twice as steep, so the halo reaches further to
	// finish in about the same place
	("text.scrim.radius", "5.0  ## Default"),
	// the outline shipped at two pixels before the halo carried more of the work
	("text.outline", "2.0  ## Default"),
	// these two never tracked the theme they document - they carried a gray and a
	// steel blue from before themes existed, so every config in the wild has them
	("colors.foreground", "\"#d2d2da\"  ## Default"),
	("colors.cursor", "\"#7a9ad0\"  ## Default"),
	// the cursor briefly shipped as the cool third of the same triad, then as the
	// warm one - both at the text's own brightness, which is why neither could be
	// read through
	("colors.cursor", "\"#cc88ee\"  ## Default"),
	("colors.cursor", "\"#eecc88\"  ## Default"),
	// the first readable one sat a shade under the contrast floor on the plate
	("colors.cursor", "\"#9649af\"  ## Default"),
	// the pane ring was a cold blue, picked for the palette before this one (the
	// key was `colors.focus` then, so a config carrying it arrives here renamed)
	("colors.highlight", "\"#5580c8\"  ## Default"),
	// tabs used to divide the bar evenly between two bounds, so the cap had to
	// be low enough that a couple of them did not swallow the window
	("window.tab_max_width_pct", "26.0  ## Default"),
	("window.tab_regular_width_pct", "8.0  ## Default"),
	// `~` was the default for part of a day, between two runs at the variable.
	// The other platform's spelling is not listed: it is that platform's current
	// default rather than an outgoing one, and listing it would rewrite the line
	// every time a config crossed between machines.
	("shell.startup_directory", "\"~\"  ## Default"),
	// the minimap shipped off until the column stepped aside for full-screen
	// programs on its own
	("scroll.minimap.enabled", "false  ## Default"),
	// wallpaper text colors shipped off while the measurements were being made
	("colors.from_wallpaper", "false  ## Default"),
	// copy on select shipped off
	("shell.copy_on_select", "false  ## Default"),
	// so did letting an idle window's GPU device go
	("window.idle_release", "false  ## Default"),
	// Seven lines named an example rather than the default they were marked
	// with, so uncommenting one changed what loaded. The values below are the
	// examples they used to carry.
	("transparency.enabled", "true  ## Default"),
	("transparency.blur_behind", "true  ## Default"),
	("wallpaper.image", "\"wallpaper.png\"  ## Default"),
	("wallpaper.rotate.folder", "\"wallpaper/\"  ## Default"),
	// then empty, which meant the same place without naming it
	("wallpaper.rotate.folder", "\"\"  ## Default"),
	// then with single backslashes, which shcl 3.0 reads as escapes, so the line
	// set nothing once uncommented
	#[cfg(windows)]
	(
		"wallpaper.rotate.folder",
		"\"%LOCALAPPDATA%\\silkterm\\wallpaper\"  ## Default",
	),
	(
		"selection.word_separators",
		"\",|\\\"' ()[]{}<>\"  ## Default",
	),
	(
		"hyperlinks.open_command",
		"\"firefox --new-tab\"  ## Default",
	),
	(
		"shell.command_line",
		"\"--new-pane --right --size 35%\"  ## Default",
	),
	// on by default until it was clear that it replaces a PS1 set in .bashrc,
	// which Debian's own files do
	("shell.bash_prompt", "true  ## Default"),
	// shipped on for its first day
	("window.remember_maximized", "true  ## Default"),
	// Close pane had no chord on a Mac for its first day
	#[cfg(target_os = "macos")]
	("keys.close_pane", "\"none\"  ## Default"),
];

// The whole pre-nesting flat namespace, old key -> new nested path. Primary
// (most recent flat) names first; still-older aliases after, so when a config
// somehow carries both spellings the newer one wins. `colors.*` map to
// themselves - the path didn't change, but active overrides still carry over.
#[rustfmt::skip]
const LEGACY_KEYS: &[(&str, &str)] = &[
	("use_system_font", "font.use_system_family"),
	("use_system_font_size", "font.use_system_size"),
	("font_family", "font.family"),
	("font_size", "font.size"),
	("line_height_scale", "font.line_height_scale"),
	("margin", "window.margin"),
	("columns", "window.columns"),
	("rows", "window.rows"),
	("remember_size", "window.remember_size"),
	("remembered_columns", "window.remembered_columns"),
	("remembered_rows", "window.remembered_rows"),
	("hide_single_tab", "window.hide_single_tab"),
	("transparent_background", "transparency.enabled"),
	("opacity", "transparency.opacity"),
	("transparent_background_blur", "transparency.blur_behind"),
	("wallpaper_enabled", "wallpaper.enabled"),
	("wallpaper", "wallpaper.image"),
	("wallpaper_fallback_builtin", "wallpaper.fallback_builtin"),
	("wallpaper_rotate_enabled", "wallpaper.rotate.enabled"),
	("wallpaper_folder", "wallpaper.rotate.folder"),
	("wallpaper_rotate_interval_s", "wallpaper.rotate.interval_s"),
	("wallpaper_rotate_random", "wallpaper.rotate.random"),
	("wallpaper_opacity", "wallpaper.opacity"),
	("wallpaper_default_fit", "wallpaper.default_fit"),
	("wallpaper_honor_xmp", "wallpaper.honor_xmp"),
	("wallpaper_honor_xmp_look", "wallpaper.honor_xmp_look"),
	("wallpaper_blur", "wallpaper.blur"),
	("wallpaper_contrast_mask", "wallpaper.contrast_mask.enabled"),
	("wallpaper_contrast_mask_size", "wallpaper.contrast_mask.size"),
	("wallpaper_contrast_mask_strength", "wallpaper.contrast_mask.strength"),
	("wallpaper_contrast_mask_auto", "wallpaper.contrast_mask.auto"),
	("text_scrim", "text.scrim.enabled"),
	("text_scrim_radius", "text.scrim.radius"),
	("text_scrim_softness", "text.scrim.softness"),
	("text_scrim_function", "text.scrim.function"),
	("text_scrim_ramp", "text.scrim.ramp"),
	("text_scrim_regular_weight", "text.scrim.regular_weight"),
	("text_outline", "text.outline"),
	("text_dark_on_light", "text.dark_on_light"),
	("color_emoji", "text.color_emoji"),
	("embolden_inverse", "text.embolden_inverse"),
	("cursor_scrim", "cursor.scrim"),
	("cursor_outline", "cursor.outline"),
	("cursor_size_height", "cursor.size.height"),
	("cursor_size_width", "cursor.size.width"),
	("cursor_animation", "cursor.animation"),
	("cursor_animation_resume_s", "cursor.animation_resume_s"),
	("cursor_animation_idle_stop_s", "cursor.animation_idle_stop_s"),
	("cursor_blink_rate_ms", "cursor.blink_rate_ms"),
	("word_separators", "selection.word_separators"),
	("selection_pairs", "selection.pairs"),
	("default_shell", "shell.default"),
	("command_line", "shell.command_line"),
	("copy_on_select", "shell.copy_on_select"),
	("scrollback", "scroll.scrollback"),
	("scroll_tau_ms", "scroll.tau_ms"),
	("wheel_lines", "scroll.wheel_lines"),
	("alt_scroll_lines", "scroll.alt_scroll_lines"),
	("output_ease_lines", "scroll.output_ease_lines"),
	("smooth_scroll_apps", "scroll.smooth_apps"),
	// still-older spellings (pre-rename vintages), lowest precedence
	("cursor_size_vertical", "cursor.size.height"),
	("cursor_size_horizontal", "cursor.size.width"),
	("text_glow", "text.scrim.enabled"),
	("text_glow_radius", "text.scrim.radius"),
	("text_glow_softness", "text.scrim.softness"),
	("text_glow_ramp", "text.scrim.ramp"),
	("text_glow_regular_weight", "text.scrim.regular_weight"),
	("text_glow_border", "text.outline"),
	("cursor_glow", "cursor.scrim"),
	("background_image", "wallpaper.image"),
	("background_folder", "wallpaper.rotate.folder"),
	("background_default", "wallpaper.fallback_builtin"),
	("wallpaper_default", "wallpaper.fallback_builtin"),
	("background_fit", "wallpaper.default_fit"),
	("wallpaper_fit", "wallpaper.default_fit"),
	("background_blur", "wallpaper.blur"),
	("background_opacity", "wallpaper.opacity"),
	("background_rotate_random", "wallpaper.rotate.random"),
	("background_rotate_interval_s", "wallpaper.rotate.interval_s"),
	("background_contrast_mask", "wallpaper.contrast_mask.enabled"),
	("background_contrast_mask_size", "wallpaper.contrast_mask.size"),
	("background_contrast_mask_strength", "wallpaper.contrast_mask.strength"),
	("background_contrast_mask_auto", "wallpaper.contrast_mask.auto"),
];

// A carried value keeps its exact spelling but not an old trailing comment -
// the new template's comments describe the setting already. `#` inside a
// quoted value (every color) survives.
fn strip_trailing_comment(value: &str) -> &str {
	let mut quote: Option<char> = None;
	let mut escaped = false;
	for (at, c) in value.char_indices() {
		if escaped {
			escaped = false;
			continue;
		}
		match c {
			'\\' => escaped = true,
			'"' | '\'' => match quote {
				Some(q) if q == c => quote = None,
				None => quote = Some(c),
				_ => {}
			},
			'#' if quote.is_none() => return value[..at].trim_end(),
			_ => {}
		}
	}
	value
}

// Rewrite the template line for `path` as an active assignment of `value`,
// keeping the template's own indentation and key spelling.
fn activate_line(lines: &mut [String], path: &str, value: &str) -> bool {
	let text = lines.join("\n");
	for w in walk_settings(&text) {
		if let WalkLine::Setting { index, path: p, .. } = w {
			if p == path {
				let line = &lines[index];
				let trimmed = line.trim_start();
				let indent = &line[..line.len() - trimmed.len()];
				let Some(key) = line_setting_key(line) else {
					return false;
				};
				lines[index] = format!("{indent}{key}: {value}");
				return true;
			}
		}
	}
	false
}

// A column-0 `wallpaper:` line holding a value. No shipped setting reads one.
fn valued_wallpaper_line(line: &str) -> bool {
	line.starts_with("wallpaper")
		&& line_setting_key(line) == Some("wallpaper")
		&& line_setting_value(line).is_some_and(|v| !strip_trailing_comment(v).trim().is_empty())
}

// An earlier build converted a flat `wallpaper: <image>` onto the template's
// `wallpaper:` heading. shcl still reads the block under it, so only the image
// was lost, but the line kept the file reading as flat and every launch
// converted it again. The value moves to `image:`, or is dropped when the file
// names an image already. No other line changes, but line endings come back LF.
fn wallpaper_heading_repaired(text: &str) -> Option<String> {
	// every launch comes through here, and almost no file has such a line
	if !text.lines().any(valued_wallpaper_line) {
		return None;
	}
	let lines: Vec<&str> = text.lines().collect();
	// the walk leaves out lines inside a fenced value, which are text
	let valued: Vec<usize> = walk_settings(text)
		.into_iter()
		.filter_map(|w| match w {
			WalkLine::Setting {
				index,
				active: true,
				..
			} if valued_wallpaper_line(lines[index]) => Some(index),
			_ => None,
		})
		.collect();
	// two of them cannot be told apart
	let [at] = valued[..] else {
		return None;
	};
	// What follows must be the template's block. A flat file's next setting sits
	// at column 0, or is an old flat name indented by hand, and the conversion
	// reads that correctly. Comments give no depth.
	let child = lines[at + 1..].iter().find(|line| {
		let trimmed = line.trim_start();
		!trimmed.is_empty() && !trimmed.starts_with('#')
	})?;
	let indent = &child[..child.len() - child.trim_start().len()];
	if indent.is_empty() {
		return None;
	}
	let child_path = format!("wallpaper.{}", line_setting_key(child)?);
	let in_template = walk_settings(default_config())
		.iter()
		.any(|w| matches!(w, WalkLine::Setting { path, .. } if *path == child_path));
	if !in_template {
		return None;
	}
	let value = line_setting_value(lines[at])?;
	let named = shcl::Document::parse(text).count("wallpaper.image") > 0;
	let mut out = String::with_capacity(text.len() + indent.len() + 8);
	for (index, line) in lines.iter().enumerate() {
		if index != at {
			out.push_str(line);
			out.push('\n');
			continue;
		}
		out.push_str("wallpaper:\n");
		if !named {
			out.push_str(indent);
			out.push_str("image: ");
			out.push_str(value);
			out.push('\n');
		}
	}
	Some(out)
}

// In place and with no backup: an install this damaged converted the file at
// every launch, and has used up most of the backup names doing it.
fn repair_wallpaper_heading(path: &std::path::Path) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	let Some(out) = wallpaper_heading_repaired(&text) else {
		return;
	};
	if config_open_elsewhere(path) {
		note_config_busy(path);
		return;
	}
	if let Err(e) = write_config_atomic(path, &out) {
		eprintln!(
			"{APP_NAME}: could not repair config {}: {e}",
			path.display()
		);
		return;
	}
	// the image line is added only when the file named no image already
	if out.lines().count() > text.lines().count() {
		eprintln!(
			"{APP_NAME}: moved the wallpaper image in {} from the `wallpaper:` line to `image:`",
			path.display()
		);
	} else {
		eprintln!(
			"{APP_NAME}: cleared the `wallpaper:` line in {}; the file already names an image under `image:`",
			path.display()
		);
	}
}

// One-time conversion of a pre-nesting config: the flat `wallpaper_*`-style
// namespace became nested blocks, and rewriting that in place would shred the
// old file's comments and grouping. Instead the old file is copied to a `.bak`
// and a fresh template is written over it, with every ACTIVE old value carried
// over to its new path - settings survive, and the file's documentation is
// current instead of half-old. Unknown `themes.*` subtrees (user data for a
// future feature) are carried verbatim in dotted form. The rewrite keeps a
// linked file linked and a private one private.
fn convert_legacy_config(path: &std::path::Path) {
	convert_legacy_config_with(path, write_config_atomic);
}

// The writer is a parameter so a test can fail it the ways the real one can.
fn convert_legacy_config_with(
	path: &std::path::Path,
	write: fn(&std::path::Path, &str) -> Result<(), String>,
) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	let Some(joined) = converted_config_text(&text) else {
		return;
	};
	if config_open_elsewhere(path) {
		note_config_busy(path);
		return;
	}
	let Some(backup) = backup_copy(path, text.as_bytes()) else {
		return;
	};
	if let Err(e) = write(path, &joined) {
		// A file that stays unwritable would gain a backup at every launch, so one
		// that reads back whole needs none. Anything else keeps it: ReplaceFile can
		// fail after the file it replaces is gone and writing it again fails,
		// leaving the backup the only copy.
		if std::fs::read(path).is_ok_and(|now| now == text.as_bytes()) {
			let _ = std::fs::remove_file(&backup);
			eprintln!(
				"{APP_NAME}: could not convert config {}: {e}",
				path.display()
			);
		} else {
			eprintln!(
				"{APP_NAME}: could not convert config {}: {e}; the old file is kept at {}",
				path.display(),
				backup.display()
			);
		}
		return;
	}
	eprintln!(
		"{APP_NAME}: config converted to the new nested layout; the old file is kept at {}",
		backup.display()
	);
}

// The conversion as text: None for a file that is not pre-nesting.
fn converted_config_text(text: &str) -> Option<String> {
	let walked = walk_settings(text);
	// A line shcl cannot read sets nothing, whatever it is called. The walk puts
	// a line that steps back to a depth nothing uses at the top level, where an
	// old flat name such as `margin` looked like a whole old file.
	let unread = unreadable_lines(&shcl::Document::parse(text));
	let read = |index: usize| !unread.contains(&(index + 1));
	// Only an ACTIVE flat key marks a file as legacy: any real old config has
	// several (the template shipped with them), while a comment that merely
	// spells an old name - e.g. one a relayouted save left at column 0 - must
	// never nuke a current-format file.
	let legacy = walked.iter().any(|w| {
		matches!(w, WalkLine::Setting { index, path: p, header: false, active: true }
			if read(*index) && !p.contains('.') && LEGACY_KEYS.iter().any(|(old, _)| old == p))
	});
	if !legacy {
		return None;
	}
	Some(rebuilt_config_text(text, &[]).0)
}

// The template with every active setting of `text` that still reads carried
// over, and how many lines holding a setting it left behind: each one shcl
// cannot read, and each setting it reads that has nowhere to go. Lines listed
// in `garbled` (0-based) did not decode and are never carried.
fn rebuilt_config_text(text: &str, garbled: &[usize]) -> (String, usize) {
	let lines: Vec<&str> = text.lines().collect();
	let walked = walk_settings(text);
	// shcl reads the garbled lines as comments, so a block under one reads as
	// lines with no parent
	let clean: String = text
		.split('\n')
		.enumerate()
		.map(|(index, line)| {
			if garbled.contains(&index) {
				format!("#{line}")
			} else {
				line.to_string()
			}
		})
		.collect::<Vec<_>>()
		.join("\n");
	let parsed = shcl::Document::parse(&clean);
	let unread = unreadable_lines(&parsed);
	let read = |index: usize| !unread.contains(&(index + 1)) && !garbled.contains(&index);

	// active values: current-format paths carry as themselves (a mixed file
	// loses nothing), old spellings map through the table, best (lowest table
	// index) spelling winning per new path
	// Settings only, never a block heading: a flat `wallpaper:` held the image,
	// and matched as a current path it was written onto the `wallpaper:` heading.
	let known_new: std::collections::HashSet<String> = walk_settings(default_config())
		.into_iter()
		.filter_map(|w| match w {
			WalkLine::Setting {
				path,
				header: false,
				..
			} => Some(path),
			_ => None,
		})
		.collect();
	let mut carry: std::collections::HashMap<String, (usize, String)> =
		std::collections::HashMap::new();
	let mut extras: Vec<String> = Vec::new();
	let mut had_font_family = false;
	let mut had_use_system = false;
	let mut left: std::collections::BTreeSet<usize> = unread.iter().map(|line| line - 1).collect();
	left.extend(garbled.iter().copied().filter(|index| {
		lines.get(*index).is_some_and(|line| {
			let line = line.trim();
			!line.is_empty() && !line.starts_with('#')
		})
	}));
	for w in &walked {
		let WalkLine::Setting {
			index,
			path: p,
			active,
			header,
		} = w
		else {
			continue;
		};
		if !active || *header {
			continue;
		}
		// A value that opens a raw block would carry without its body. No
		// setting takes one.
		if !read(*index) || fence_run(lines[*index]).is_some() {
			left.insert(*index);
			continue;
		}
		let Some(value) = line_setting_value(lines[*index]) else {
			continue;
		};
		let value = strip_trailing_comment(value).trim();
		if value.is_empty() {
			continue;
		}
		if p == "use_system_font" {
			had_use_system = true;
		}
		if p == "font_family" {
			had_font_family = true;
		}
		let target = if known_new.contains(p) {
			Some((0, p.clone()))
		} else {
			LEGACY_KEYS
				.iter()
				.position(|(old, _)| old == p)
				.map(|rank| (rank + 1, LEGACY_KEYS[rank].1.to_string()))
		};
		if let Some((rank, new)) = target {
			// a mapped path that has since been retired stays retired - the
			// old value is still in the .bak, but never resurrects here
			if !CONFIG_REMOVED.contains(&new.as_str()) {
				match carry.entry(new) {
					std::collections::hash_map::Entry::Vacant(slot) => {
						slot.insert((rank, value.to_string()));
					}
					// two spellings of one setting keep one
					std::collections::hash_map::Entry::Occupied(mut slot) => {
						left.insert(*index);
						if rank < slot.get().0 {
							slot.insert((rank, value.to_string()));
						}
					}
				}
			}
		} else if p.starts_with("themes.") || is_monitor_size_path(p) {
			extras.push(format!("{p}: {value}"));
		} else if !p.starts_with("shells.") {
			// dropped from the new file, still in the copy
			left.insert(*index);
		}
		// a shell list carries whole, below
	}
	// Old semantics: no use_system_font line + an explicit font_family meant
	// "use that font" - keep meaning that, not the new template's default.
	if had_font_family && !had_use_system {
		carry
			.entry("font.use_system_family".to_string())
			.or_insert((usize::MAX, "false".to_string()));
	}

	let mut out: Vec<String> = default_config().lines().map(str::to_string).collect();
	for (new_path, (_, value)) in &carry {
		if !activate_line(&mut out, new_path, value) {
			// no template line (shouldn't happen) - keep it as a dotted line
			extras.push(format!("{new_path}: {value}"));
		}
	}
	if !extras.is_empty() {
		out.push(String::new());
		extras.sort();
		out.extend(extras);
	}
	let mut joined = out.join("\n");
	joined.push('\n');
	// The shell list is the user's, and its order names the default shell, so it
	// carries whole and in order rather than as sorted dotted lines.
	let shells = read_shells(&parsed);
	if !shells.is_empty() {
		let mut doc = shcl::Document::parse(&joined);
		write_shells(&mut doc, &[], &shells);
		joined = doc.to_canonical();
	}
	(joined, left.len())
}

// Migrate an existing config in place across program updates: rename keys whose
// name changed, drop keys that no longer exist. Preserves the user's values,
// comments, and layout (line-based, like backfill). New keys are added by
// backfill_config; this only renames/removes, so run it first.
fn migrate_config(path: &std::path::Path) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	if let Some(out) = migrated_text(&text, true) {
		if config_open_elsewhere(path) {
			note_config_busy(path);
			return;
		}
		if let Err(e) = write_config_atomic(path, &out) {
			eprintln!(
				"{APP_NAME}: could not migrate config {}: {e}",
				path.display()
			);
		}
	}
}

// Best-effort check that some OTHER process has the config file open right now
// (e.g. the user is editing it). Linux only, via /proc/<pid>/fd; elsewhere we
// assume it's free. It only catches editors that hold the descriptor open, so a
// false "not busy" is possible - fine, because the writes we gate on it only add
// program-driven options and never touch the user's own values or comments.
#[cfg(target_os = "linux")]
fn config_open_elsewhere(path: &std::path::Path) -> bool {
	let Ok(target) = path.canonicalize() else {
		return false;
	};
	let me = std::process::id();
	let Ok(procs) = std::fs::read_dir("/proc") else {
		return false;
	};
	for proc in procs.flatten() {
		let Some(pid) = proc
			.file_name()
			.to_str()
			.and_then(|s| s.parse::<u32>().ok())
		else {
			continue;
		};
		if pid == me {
			continue;
		}
		let Ok(fds) = std::fs::read_dir(proc.path().join("fd")) else {
			continue; // not ours to read / gone - skip
		};
		for fd in fds.flatten() {
			if std::fs::read_link(fd.path()).is_ok_and(|link| link == target) {
				return true;
			}
		}
	}
	false
}

#[cfg(not(target_os = "linux"))]
fn config_open_elsewhere(_path: &std::path::Path) -> bool {
	false
}

fn note_config_busy(path: &std::path::Path) {
	eprintln!(
		"{APP_NAME}: {} looks open in another program; leaving it as-is for now.",
		path.display()
	);
}

// Bring a font_family line still carrying a superseded default stack up to the
// current one. The value is matched as it reads, in either quote, because a save
// swaps one quote for the other. An edited stack, or one with a note on the
// line, is left exactly as the user wrote it.
fn refresh_font_stack(line: &str) -> Option<String> {
	let (head, value) = line.split_once(':')?;
	let value = value.trim();
	if strip_trailing_comment(value) != value {
		return None;
	}
	let doc = shcl::Document::parse(&format!("v: {value}\n"));
	if doc.lost_count() > 0 {
		return None;
	}
	doc.get_string("v")
		.is_ok_and(|read| SUPERSEDED_FONT_STACKS.contains(&read.as_str()))
		.then(|| format!("{head}: \"{DEFAULT_FONT_STACK}\""))
}

// Everything after a settings line's first colon, trimmed - a trailing `##`
// comment included, deliberately, so the exact-match test below refuses any line
// the user has written a note on.
fn line_setting_value(line: &str) -> Option<&str> {
	Some(line.split_once(':')?.1.trim())
}

// Bring a commented line still echoing a superseded default up to the template's
// current line for that path. Only a bare, exactly-matching value migrates, so an
// edited value - or one trailing a note - stays as the user wrote it.
fn refresh_superseded_default(line: &str, path: &str) -> Option<String> {
	if !line.trim_start().starts_with('#') {
		return None; // active: the user's own value, leave it alone
	}
	// a path can carry several superseded values (a default retuned more than
	// once), so every entry for it is a candidate - not just the first
	let value = line_setting_value(line)?;
	if !SUPERSEDED_DEFAULTS
		.iter()
		.any(|(name, old)| *name == path && *old == value)
	{
		return None;
	}
	setting_lines(default_config())
		.into_iter()
		.find(|(name, _)| name == path)
		.map(|(_, template)| template)
		.filter(|template| template != line)
}

// The rename/remove/refresh transform, as a pure fn (testable). Returns
// Some(new text) only if something changed.
fn migrate_config_text(text: &str) -> Option<String> {
	migrated_text(text, false)
}

// `keep_default_shell` is for the write to disk. An active `shell.default`
// leaves with its adoption into the shell list (`adopt_default_shell`), which
// removes it once that save goes through. Where the save was refused it has to
// stay, or the choice is lost. A read drops it either way.
fn migrated_text(text: &str, keep_default_shell: bool) -> Option<String> {
	let lines: Vec<&str> = text.lines().collect();
	// full path per line index, for the lines that are settings
	let mut path_of: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
	let mut keep: std::collections::HashSet<usize> = std::collections::HashSet::new();
	let mut active_old_name = false;
	for w in walk_settings(text) {
		if let WalkLine::Setting {
			index,
			path,
			active,
			header,
		} = w
		{
			if keep_default_shell && active && !header && path == "shell.default" {
				keep.insert(index);
			}
			active_old_name |= active && CONFIG_RENAMES.iter().any(|(old, _)| *old == path);
			path_of.insert(index, path);
		}
	}
	// Rename targets already present (active or commented): don't create a dup.
	// Only an active old name moves a value, so only then is the answer taken
	// from the saved form; the template's commented `# focus:` pays nothing.
	let raw_paths = || path_of.values().cloned().collect();
	let have: std::collections::HashSet<String> = if active_old_name {
		saved_paths(text).unwrap_or_else(raw_paths)
	} else {
		raw_paths()
	};

	let mut changed = false;
	let mut out: Vec<String> = Vec::new();
	for (index, line) in lines.iter().enumerate() {
		let Some(path) = path_of.get(&index) else {
			out.push((*line).to_string());
			continue;
		};
		if CONFIG_REMOVED.contains(&path.as_str()) && !keep.contains(&index) {
			changed = true;
			continue; // drop
		}
		// A rename fires only while the new spelling is absent. Where both are
		// present the old line is left ALONE, never dropped: a rename can free
		// its old name for a NEW setting (colors.focus did exactly that), and
		// dropping there would delete that setting's own line on every launch.
		let renamed = CONFIG_RENAMES
			.iter()
			.find(|(old, _)| old == path)
			.filter(|(_, new)| !have.contains(*new));
		let mut kept = match renamed {
			Some((_, new)) => {
				changed = true;
				// the line spells the leaf (nested) or the full path
				// (dotted); rewrite whichever token is actually there
				let old_leaf = path.rsplit('.').next().unwrap_or(path);
				let new_leaf = new.rsplit('.').next().unwrap_or(new);
				let key = line_setting_key(line).unwrap_or(old_leaf);
				let target = if key == *path { new } else { new_leaf };
				line.replacen(key, target, 1)
			}
			None => (*line).to_string(),
		};
		// the refreshes below key on where the line ENDS UP, not where it came
		// from - a just-renamed line belongs to its new path now
		let path: &str = renamed.map_or(path.as_str(), |(_, new)| new);
		if let Some(refreshed) = refresh_superseded_default(&kept, path) {
			kept = refreshed;
			changed = true;
		}
		if path == "font.family" && !kept.trim_start().starts_with('#') {
			if let Some(refreshed) = refresh_font_stack(&kept) {
				kept = refreshed;
				changed = true;
			}
		}
		out.push(kept);
	}
	changed.then(|| {
		let mut joined = out.join("\n");
		joined.push('\n');
		joined
	})
}

// What a launch parses when its rewrites were put off because the file looked
// open elsewhere: the renames still apply, in memory.
fn loaded_text(text: &str) -> std::borrow::Cow<'_, str> {
	rewritten_by(text, &[from_shcl2_text, migrate_config_text])
}

// What the next launch parses: the text its rewrites leave, in the launch's own
// order. The rating check compares through this, because a step can read how a
// line is written as well as what it says. Two did: the font list refresh looked
// at the quote character, which the conversion before it copies, and the renames
// took a commented heading for a parent. A save changes both, so a rating write
// moved another setting a launch later. Neither reads layout now, and this is
// what catches the next step that does. Backfill is left out on purpose: it only
// adds lines the program owns, and it runs whether or not a rating was written.
// A step added to `load` belongs here too.
type LaunchStep = fn(&str) -> Option<String>;

const LAUNCH_STEPS: [LaunchStep; 5] = [
	from_shcl2_text,
	wallpaper_heading_repaired,
	converted_config_text,
	adopted_shell_text,
	migrate_config_text,
];

#[cfg(test)]
fn next_launch_text(text: &str) -> std::borrow::Cow<'_, str> {
	rewritten_by(text, &LAUNCH_STEPS)
}

fn rewritten_by<'a>(text: &'a str, steps: &[LaunchStep]) -> std::borrow::Cow<'a, str> {
	let mut out: Option<String> = None;
	for step in steps {
		if let Some(next) = step(out.as_deref().unwrap_or(text)) {
			out = Some(next);
		}
	}
	out.map_or(std::borrow::Cow::Borrowed(text), std::borrow::Cow::Owned)
}

// Every setting path, active and commented, where every save agrees on it. A
// save that keeps the lines leaves a comment where it is, but one that falls
// back to the canonical form writes it at the depth of the setting below it, so
// a commented new name can move into the old name's block, and a rename that a
// save turns on or off moved a value at the next launch. shcl picks between the
// two by what the save changes, not by the file: a window size save can fall
// back where a font size save keeps the lines. So the answer is the canonical
// form's, which is also the canonical form of whatever either save writes.
// Where the file has a lost line a fallback is refused and only the lines are
// kept, so the file as it is has the answer.
fn saved_paths(text: &str) -> Option<std::collections::HashSet<String>> {
	let doc = parse_kept(text);
	if doc.lost_count() > 0 {
		return None;
	}
	Some(
		walk_settings(&doc.to_canonical())
			.into_iter()
			.filter_map(|w| match w {
				WalkLine::Setting { path, .. } => Some(path),
				_ => None,
			})
			.collect(),
	)
}

// One-time: `shell.default` used to name the default shell on its own. The list
// names it now - its first active entry - so the two could disagree, and the
// stored value is the user's own statement of which one they meant. Move the
// entry it names to the top (creating one if the list has nothing running that
// program) and drop the key; `CONFIG_REMOVED` clears any commented remnant.
//
// It runs BEFORE that removal, and only while the key is actively set: a config
// that never had one, or has already been through this, is not touched at all.
fn adopt_default_shell(path: &std::path::Path) {
	let Ok(doc) = read_doc(path) else { return };
	let Some(adopted) = adopted_shell_doc(&doc) else {
		return;
	};
	if config_open_elsewhere(path) {
		note_config_busy(path);
		return;
	}
	let _ = write_doc(path, &adopted);
}

// None for a file with no active `shell.default`.
fn adopted_shell_doc(doc: &shcl::Document) -> Option<shcl::Document> {
	let wanted = doc
		.get_string("shell.default")
		.ok()
		.map(|v| v.trim().to_string())
		.filter(|v| !v.is_empty())?;
	let stored = read_shells(doc);
	let moved = adopt_default_into(&stored, &wanted);
	let mut doc = doc.clone();
	write_shells(&mut doc, &stored, &moved);
	doc.remove("shell.default");
	Some(doc)
}

// The adoption as text, for `next_launch_text`. Where the save is refused the
// file is left alone.
fn adopted_shell_text(text: &str) -> Option<String> {
	adopted_shell_doc(&parse_kept(text))
		.filter(|doc| !save_refused(doc))
		.map(|doc| saved_text(&doc))
}

// The list with `wanted` at the front. An entry already running that shell moves;
// anything else is added as a new entry, since a command the list does not carry
// is still a shell the user chose to launch.
//
// "Already running that shell" is an identity question, not a string one: the
// retired `shell.default` was routinely a bare name where the scan had already
// stored the full path to the same file, and a string compare therefore promoted
// a DUPLICATE of the user's default shell to the top of their list.
fn adopt_default_into(
	stored: &[crate::shells::ShellEntry],
	wanted: &str,
) -> Vec<crate::shells::ShellEntry> {
	adopt_default_into_with(stored, wanted, &crate::shells::same_command)
}

fn adopt_default_into_with(
	stored: &[crate::shells::ShellEntry],
	wanted: &str,
	same: &dyn Fn(&str, &str) -> bool,
) -> Vec<crate::shells::ShellEntry> {
	let mut out = stored.to_vec();
	let at = out
		.iter()
		.position(|e| e.command.trim() == wanted)
		.or_else(|| out.iter().position(|e| same(&e.command, wanted)));
	match at {
		Some(at) => {
			let entry = out.remove(at);
			out.insert(0, entry);
		}
		None => out.insert(0, crate::shells::adopted(wanted, stored)),
	}
	out
}

// Revert config keys to their defaults: drop the active assignment from
// config.shcl (dotted keys are paths), then backfill so the
// key comes back as the template's commented default line. Used by the Settings
// dialog's revert-to-default buttons.
pub fn revert_keys(keys: &[&str]) {
	if keys.is_empty() {
		return;
	}
	let Some(path) = config_path() else { return };
	if config_open_elsewhere(&path) {
		note_config_busy(&path);
		return;
	}
	let Ok(text) = std::fs::read_to_string(&path) else {
		return;
	};
	let Some(out) = reverted_text(&text, keys) else {
		return;
	};
	if let Err(e) = write_config_atomic(&path, &out) {
		eprintln!(
			"{APP_NAME}: could not update config {}: {e}",
			path.display()
		);
		return;
	}
	backfill_config(&path);
}

// Comment the named settings out, so each is as good as absent. Used for a
// setting the user cleared: there is no value to write, and leaving the old line
// alone brought it back next launch.
pub fn disable_keys(keys: &[&str]) {
	if keys.is_empty() {
		return;
	}
	let Some(path) = config_path() else { return };
	if config_open_elsewhere(&path) {
		note_config_busy(&path);
		return;
	}
	let Ok(text) = std::fs::read_to_string(&path) else {
		return;
	};
	let Some(out) = disabled_text(&text, keys) else {
		return;
	};
	if let Err(e) = write_config_atomic(&path, &out) {
		eprintln!(
			"{APP_NAME}: could not update config {}: {e}",
			path.display()
		);
	}
}

// What becomes of one setting's line.
enum LineEdit {
	Keep,
	Put(String),
	Drop,
}

// Rewrite the named settings' own lines, leaving everything above them alone.
//
// A line edit rather than a document one on purpose: removing the node takes its
// leading comments with it, so reverting a scrim setting used to destroy seven
// lines of documentation, and a note written above a value went the same way.
// `line_for` is given the key, the file's own indentation, the current line and
// every other commented line for that key, and answers what becomes of it.
// Answers None when nothing needs writing.
//
// A setting's own line is its active one. A save adds a set value as a new
// line, sometimes above the commented default it replaces, and taking the
// last line for the path found that comment instead, so a revert left the
// value in force.
fn edit_setting_lines(
	text: &str,
	keys: &[&str],
	line_for: impl Fn(&str, &str, &str, &[&str]) -> LineEdit,
) -> Option<String> {
	let mut lines: Vec<Option<String>> = text.lines().map(|l| Some(l.to_string())).collect();
	let mut active: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
	let mut commented: std::collections::HashMap<String, Vec<usize>> =
		std::collections::HashMap::new();
	for w in walk_settings(text) {
		let WalkLine::Setting {
			index,
			path,
			active: is_active,
			..
		} = w
		else {
			continue;
		};
		if is_active {
			active.insert(path, index);
		} else {
			commented.entry(path).or_default().push(index);
		}
	}
	let mut changed = false;
	for key in keys {
		let others = commented.get(*key).map_or(&[][..], Vec::as_slice);
		let Some(i) = active.get(*key).copied().or_else(|| others.last().copied()) else {
			continue;
		};
		let Some(line) = lines[i].clone() else {
			continue;
		};
		let indent: String = line
			.chars()
			.take_while(|c| *c == '\t' || *c == ' ')
			.collect();
		let rest: Vec<&str> = others
			.iter()
			.filter(|&&k| k != i)
			.filter_map(|&k| lines[k].as_deref())
			.collect();
		match line_for(key, &indent, &line, &rest) {
			LineEdit::Keep => {}
			LineEdit::Put(replacement) => {
				if line != replacement {
					lines[i] = Some(replacement);
					changed = true;
				}
			}
			LineEdit::Drop => {
				lines[i] = None;
				changed = true;
			}
		}
	}
	if !changed {
		return None;
	}
	let mut out = lines.into_iter().flatten().collect::<Vec<_>>().join("\n");
	if text.ends_with('\n') {
		out.push('\n');
	}
	Some(out)
}

// One setting's line commented out, so the setting is as good as absent.
fn commented(indent: &str, line: &str) -> LineEdit {
	let body = line.trim_start();
	if body.starts_with('#') {
		LineEdit::Keep
	} else {
		LineEdit::Put(format!("{indent}# {body}"))
	}
}

// The file with each named setting put back the way the template ships it. A
// value saved beside the template's own commented line just goes, rather than
// leaving a second copy of that line, which would pile up one more each time.
fn reverted_text(text: &str, keys: &[&str]) -> Option<String> {
	let template: std::collections::HashMap<String, String> =
		setting_lines(default_config()).into_iter().collect();
	edit_setting_lines(text, keys, |key, indent, line, others| {
		match template.get(key) {
			Some(shipped)
				if !line.trim_start().starts_with('#')
					&& others.iter().any(|other| other.trim() == shipped.trim()) =>
			{
				LineEdit::Drop
			}
			Some(shipped) => LineEdit::Put(format!("{indent}{}", shipped.trim_start())),
			// nothing ships it, so commenting it out is the whole revert
			None => commented(indent, line),
		}
	})
}

// The file with each named setting commented out. This is what a cleared box
// means - "not set" - which is not the same as the template's default, and the
// template ships some of these with a value.
fn disabled_text(text: &str, keys: &[&str]) -> Option<String> {
	edit_setting_lines(text, keys, |_, indent, line, _| commented(indent, line))
}

// Insert any settings the shipped template defines that `path` lacks,
// using the template's own (commented or active) line so follow-system keys stay
// absent and behavior is unchanged. Existing values, comments, and formatting are
// preserved (nothing already in the file is rewritten). Every setting is one
// self-contained line - nested keys are written in dotted form - so there is no
// table header to insert under.
//
// A template group is one comment block plus the settings it introduces, and the
// block belongs to the group's FIRST setting. A group the file has never seen is
// appended whole, comments and all - but a group the file already has PART of
// already carries those comments, and re-appending them would duplicate the whole
// paragraph at the end of the file. Stragglers from a part-present group are put
// back beside the siblings that explain them instead.
fn backfill_config(path: &std::path::Path) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	let out = match backfilled_text(&text) {
		Ok(Some(out)) => out,
		Ok(None) => return,
		Err(setting) => {
			eprintln!(
				"{APP_NAME}: {}: adding the missing settings would stop {setting} being read, so none were added",
				path.display()
			);
			return;
		}
	};
	if config_open_elsewhere(path) {
		note_config_busy(path);
		return;
	}
	if let Err(e) = write_config_atomic(path, &out) {
		eprintln!(
			"{APP_NAME}: could not update config {}: {e}",
			path.display()
		);
	}
}

// The backfill as text: None where the file lacks nothing. Err names a setting
// whose value would stop loading, in which case nothing may be written.
fn backfilled_text(text: &str) -> Result<Option<String>, String> {
	let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
	// where each line came from, None for one added here
	let mut origin: Vec<Option<usize>> = (0..lines.len()).map(Some).collect();

	let mut groups: Vec<Vec<(String, Vec<String>)>> = Vec::new();
	for (p, block, new_group) in setting_groups(default_config()) {
		if new_group || groups.is_empty() {
			groups.push(Vec::new());
		}
		if let Some(group) = groups.last_mut() {
			group.push((p, block));
		}
	}
	// template order of every path, for sibling anchoring across group bounds
	let order: Vec<String> = groups.iter().flatten().map(|(p, _)| p.clone()).collect();

	// A heading counts as there when a dotted line such as
	// `window.rows: 34` already speaks for its block.
	let has = |at: &std::collections::HashMap<String, usize>, p: &str| {
		at.contains_key(p)
			|| at
				.keys()
				.any(|k| k.strip_prefix(p).is_some_and(|r| r.starts_with('.')))
	};

	// A group refused because of where it would go can fit once the groups after
	// it are in, so the groups are tried again until a round adds nothing. That
	// is what the next launch would do, and it would write the file again.
	let mut changed = false;
	loop {
		let mut round = false;
		for group in &groups {
			// fresh view after any earlier insertion
			let at = paths_at(&lines);
			let present = group.iter().filter(|(p, _)| has(&at, p)).count();
			if present == group.len() {
				continue;
			}
			if present == 0 {
				// wholly-new group: comments and all, in template position
				let saved = (lines.clone(), origin.clone());
				let block: Vec<String> =
					group.iter().flat_map(|(_, b)| b.iter().cloned()).collect();
				match anchor_for(&group[0].0, &order, &at, &lines, true) {
					Anchor::Before(index) => {
						// separate from the next group's comment block below, and from
						// whatever ends above
						add_line(&mut lines, &mut origin, index, String::new());
						let mut index = index;
						if index > 0 && !lines[index - 1].trim().is_empty() {
							add_line(&mut lines, &mut origin, index, String::new());
							index += 1;
						}
						for (offset, line) in block.into_iter().enumerate() {
							add_line(&mut lines, &mut origin, index + offset, line);
						}
					}
					Anchor::After(index) => {
						let mut added = vec![String::new()];
						added.extend(block);
						for (offset, line) in added.into_iter().enumerate() {
							add_line(&mut lines, &mut origin, index + 1 + offset, line);
						}
					}
					Anchor::Append => {
						let end = lines.len();
						add_line(&mut lines, &mut origin, end, String::new());
						for line in block {
							let end = lines.len();
							add_line(&mut lines, &mut origin, end, line);
						}
					}
				}
				let paths: Vec<&String> = group.iter().map(|(p, _)| p).collect();
				let mut now = (lines, origin);
				round |= settle(text, &mut now, saved, &paths);
				(lines, origin) = now;
				continue;
			}
			// part-present group: the comments are already in the file next to the
			// siblings, so re-appending them would duplicate the paragraph - put
			// each straggler back beside its siblings, line only, template order
			for (p, block) in group {
				let at = paths_at(&lines);
				if has(&at, p) {
					continue;
				}
				let Some(line) = block.last() else { continue };
				let saved = (lines.clone(), origin.clone());
				match anchor_for(p, &order, &at, &lines, false) {
					Anchor::Before(index) => add_line(&mut lines, &mut origin, index, line.clone()),
					Anchor::After(index) => {
						add_line(&mut lines, &mut origin, index + 1, line.clone());
					}
					Anchor::Append => {
						let end = lines.len();
						add_line(&mut lines, &mut origin, end, line.clone());
					}
				}
				let mut now = (lines, origin);
				round |= settle(text, &mut now, saved, &[p]);
				(lines, origin) = now;
			}
		}
		changed |= round;
		if !round {
			break;
		}
	}
	if !changed {
		return Ok(None);
	}
	unbury(text, &mut lines, &origin)?;

	let mut out = lines.join("\n");
	out.push('\n');
	Ok((out != text).then_some(out))
}

// A backfilled file's lines, and where each came from: None for one added.
type Backfill = (Vec<String>, Vec<Option<usize>>);

fn add_line(lines: &mut Vec<String>, origin: &mut Vec<Option<usize>>, at: usize, line: String) {
	lines.insert(at, line);
	origin.insert(at, None);
}

// An added line is kept only where it reads as the setting it was added for,
// once `unbury` has moved out any line it buried. Under a line that has a
// value, as in `shell: bash`, beside a heading indented unlike the template's,
// or above a line that `unbury` moves out, it would read as something else,
// and every launch would find the setting still missing and add it again.
fn settle(text: &str, now: &mut Backfill, saved: Backfill, paths: &[&String]) -> bool {
	let mut tidied = now.0.clone();
	let placed = unbury(text, &mut tidied, &now.1).is_ok() && {
		let at = paths_at(&tidied);
		paths.iter().all(|p| at.contains_key(*p))
	};
	if placed {
		now.0 = tidied;
	} else {
		*now = saved;
	}
	placed
}

// A line indented deeper than its block needs still reads as that block's, until
// a line is added beside it at the block's own depth. An active line above then
// takes it for a child and its value stops loading, and a line below it at the
// shallower depth cannot be read at all, which makes every later save refuse.
// Nothing says so either way. A short hand-written file is where it happens:
// `rows:` two tabs in under `window:`, and the template's lines arriving around
// it. Such lines are moved out to the depth their block gives, one at a time
// until the file reads as it did, which a save would do anyway. Err names what
// still reads differently after that.
fn unbury(text: &str, lines: &mut [String], origin: &[Option<usize>]) -> Result<(), String> {
	let before = shcl::Document::parse(text);
	let source: Vec<&str> = text.lines().collect();
	let walked = walk_settings(text);
	// each active setting with the line it should be: its block's indent plus a tab
	let mut settings: Vec<(usize, &str, String)> = Vec::new();
	for w in &walked {
		let WalkLine::Setting {
			index,
			path,
			active: true,
			header: false,
		} = w
		else {
			continue;
		};
		let key = line_setting_key(source[*index]).unwrap_or(path);
		let block = path
			.strip_suffix(key)
			.map_or("", |rest| rest.trim_end_matches('.'));
		let indent = if block.is_empty() {
			String::new()
		} else {
			let header = walked.iter().rev().find_map(|w| match w {
				WalkLine::Setting {
					index: at,
					path: p,
					active: true,
					header: true,
				} if at < index && p == block => Some(*at),
				_ => None,
			});
			let Some(header) = header else { continue };
			let line = source[header];
			format!("{}\t", &line[..line.len() - line.trim_start().len()])
		};
		settings.push((
			*index,
			path.as_str(),
			format!("{indent}{}", source[*index].trim_start()),
		));
	}
	let reads = |doc: &shcl::Document, path: &str| {
		let read = doc.read_string(path);
		(read.value, read.status)
	};
	// What shcl reads is the measure, and it can differ from the walk on a file
	// that is part junk. A setting the walk cannot point at is not moved.
	let loaded: Vec<String> = before
		.paths()
		.into_iter()
		.filter(|path| before.get_string(path).is_ok())
		.collect();
	// The lines shcl cannot read stay exactly those. A line added at a depth a
	// dropped one steps back to gives it a block, and it starts being read.
	let unread: Vec<Option<usize>> = unreadable_lines(&before).into_iter().map(Some).collect();
	let unread_now = |doc: &shcl::Document| -> Vec<Option<usize>> {
		unreadable_lines(doc)
			.into_iter()
			.map(|line| origin.get(line - 1).copied().flatten().map(|from| from + 1))
			.collect()
	};
	// each pass moves one line, and a line is never moved twice
	for _ in 0..=settings.len() {
		let mut joined = lines.join("\n");
		joined.push('\n');
		let after = shcl::Document::parse(&joined);
		let changed = loaded
			.iter()
			.find(|path| reads(&before, path) != reads(&after, path));
		if changed.is_none()
			&& after.lost_count() == before.lost_count()
			&& unread_now(&after) == unread
		{
			return Ok(());
		}
		let now_at = |index: usize| origin.iter().position(|from| *from == Some(index));
		// the setting that changed, or with only a line gone unreadable, the first
		// one still deeper than it should be. A changed one already in place can
		// still be under a line that is not, which skips it with that line.
		let misplaced =
			|index: usize, proper: &String| now_at(index).is_some_and(|now| lines[now] != *proper);
		let pick = changed
			.and_then(|changed| {
				settings
					.iter()
					.find(|(index, path, proper)| path == changed && misplaced(*index, proper))
			})
			.or_else(|| {
				settings
					.iter()
					.find(|(index, _, proper)| misplaced(*index, proper))
			});
		let what = || changed.map_or("a line".to_string(), |path| format!("`{path}`"));
		let Some((index, _, proper)) = pick else {
			return Err(what());
		};
		let Some(now) = now_at(*index) else {
			return Err(what());
		};
		if lines[now] == *proper {
			return Err(what());
		}
		lines[now].clone_from(proper);
	}
	Err("a setting".to_string())
}

// What became of a rating's lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
	// the file holds every value asked for, including when it already did
	Written,
	// another process holds the file (Linux); nothing written
	Busy,
	// the file has a line the parse drops, and the values could not go in beside
	// it; nothing written
	Unreadable,
	// the file reads clean, but the values could not go in so that they read back
	// and every other setting reads as it did; nothing written
	Unplaced,
	// no settings path, or the read or the rename failed: the reason
	Unwritable(String),
}

// The only lines a rating writes. The keys are fixed here, so no caller can
// hand the writer a path.
#[derive(Clone, Copy, Debug, Default)]
pub struct RatingLines<'a> {
	pub profile: Option<&'a str>,        // Profile::key() of a measured rung
	pub rated_hardware: Option<&'a str>, // profile::hardware_id()
	pub check_next_run: Option<bool>,
}

#[derive(Clone, Copy)]
enum RatingValue<'a> {
	Word(&'a str),
	Flag(bool),
}

// Write a finished rating into the settings file line by line, the way migrate
// and backfill write at launch. `persist` refuses a file the parse dropped a
// line from, which is right for the user's own values, but a rating that never
// sticks is a test at every launch with nothing on screen to say why. These are
// lines the program owns, so every other byte stays as it was, except on a file
// that reads clean and still has nowhere to put them, which gets what the
// dialog's save would write.
#[must_use]
pub fn keep_rating(lines: &RatingLines) -> Kept {
	let Some(path) = config_path() else {
		return Kept::Unwritable("no settings file location".to_string());
	};
	let body = match std::fs::read(&path) {
		Ok(body) => body,
		Err(e) => return Kept::Unwritable(format!("could not read {}: {e}", path.display())),
	};
	// A file a busy launch left with lines that are not UTF-8 is written again
	// without them, as the launch would have.
	let text = settings_text(&body);
	let out = match with_rating_lines(&text, lines) {
		Ok(out) => out,
		Err(kept) => return kept,
	};
	if out.as_bytes() == body {
		return Kept::Written;
	}
	if config_open_elsewhere(&path) {
		note_config_busy(&path);
		return Kept::Busy;
	}
	match write_config_atomic(&path, &out) {
		Ok(()) => Kept::Written,
		// the reason already names the file
		Err(e) => Kept::Unwritable(format!("could not write {e}")),
	}
}

// How every setting reads, in file order, leaving out the keys being written
// and anything under them. A new line can move which line a parse drops without
// changing how many it drops, so the count alone would let another setting load
// differently. A rating key this write does not touch counts as another setting,
// since a new `profile:` line can pull a deeper `check_next_run:` under itself.
// The block that holds a written key is left out while it has no value of its
// own, because a write into a file with no such block creates it. The quoted
// flag is left out, since a save adds or drops quotes and no read sees the flag
// (`a_save_that_requotes_a_value_changes_no_read`). What a quote character or an
// indent does to the launch's migration is compared by the caller, on the
// migrated text.
fn settings_besides(
	doc: &shcl::Document,
	written: &[String],
) -> Vec<(String, Vec<String>, shcl::Status)> {
	doc.paths()
		.into_iter()
		.filter(|path| {
			!written.iter().any(|key| {
				let under = path
					.strip_prefix(key.as_str())
					.is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
				let holds = key
					.strip_prefix(path.as_str())
					.is_some_and(|rest| rest.starts_with('.'));
				under || (holds && doc.read_string(path).status == shcl::Status::Empty)
			})
		})
		.map(|path| {
			let read = doc.read_string(&path);
			let instances = doc.instances(&path);
			(path, instances, read.status)
		})
		.collect()
}

// The file's text with the rating's values in it, or why not. Pure, so every
// placement rule is testable without a file. Line by line first, so every other
// byte stays as it was. Where that fails on a file that reads clean, the text is
// what the dialog's save would write, since that save kept a rating in any such
// file. Either result is parsed back before it is offered.
fn with_rating_lines(text: &str, lines: &RatingLines) -> Result<String, Kept> {
	with_rating_lines_through(text, lines, &LAUNCH_STEPS)
}

// The launch's rewrites are a parameter so a test can hand in one that reads
// how a line is written. None of the real ones does any more, and the check
// still has to catch the next one that does.
fn with_rating_lines_through(
	text: &str,
	lines: &RatingLines,
	steps: &[LaunchStep],
) -> Result<String, Kept> {
	let wanted = [
		("profile", lines.profile.map(RatingValue::Word)),
		(
			"rated_hardware",
			lines.rated_hardware.map(RatingValue::Word),
		),
		(
			"check_next_run",
			lines.check_next_run.map(RatingValue::Flag),
		),
	];
	let before = parse_kept(text);
	let migrated = migrated_parse(text, steps);
	let loaded = migrated.as_ref().unwrap_or(&before);
	// A file that reads clean is never said to have a line that cannot be read.
	let refused = if before.lost_count() == 0 {
		Kept::Unplaced
	} else {
		Kept::Unreadable
	};
	let mut spelled = Vec::new();
	for (leaf, value) in wanted {
		let Some(value) = value else { continue };
		let Some(spelling) = rating_spelling(leaf, value) else {
			return Err(refused);
		};
		spelled.push((leaf, spelling));
	}
	if let Some(out) = placed_rating_lines(text, &spelled)
		&& reads_as_asked(&before, loaded, &out, &wanted, steps)
	{
		return Ok(out);
	}
	if let Some(out) = saved_rating(&before, &wanted)
		&& reads_as_asked(&before, loaded, &out, &wanted, steps)
	{
		return Ok(out);
	}
	Err(refused)
}

// The lines with each value put in place, joined the way backfill joins them.
fn placed_rating_lines(text: &str, spelled: &[(&str, String)]) -> Option<String> {
	let mut out: Vec<String> = text.lines().map(str::to_string).collect();
	for (leaf, spelling) in spelled {
		place_rating_line(&mut out, leaf, spelling)?;
	}
	let mut joined = out.join("\n");
	joined.push('\n');
	Some(joined)
}

// What the dialog's save leaves: shcl's own setters over the parse, then the
// text its save writes. Only for a file that lost nothing, since that save falls
// back to the canonical text there, which deletes the line it lost.
fn saved_rating(before: &shcl::Document, wanted: &[(&str, Option<RatingValue>)]) -> Option<String> {
	let mut doc = before.clone();
	for (leaf, value) in wanted {
		let path = format!("performance.{leaf}");
		let applied = match value {
			None => true,
			Some(RatingValue::Word(word)) => doc.set_string(&path, word),
			Some(RatingValue::Flag(flag)) => doc.set_bool(&path, *flag),
		};
		if !applied {
			return None;
		}
	}
	(!save_refused(&doc)).then(|| saved_text(&doc))
}

// The parse a launch makes of this text, or None where the launch's rewrites
// leave the text alone and the caller's own parse of it serves.
fn migrated_parse(text: &str, steps: &[LaunchStep]) -> Option<shcl::Document> {
	match rewritten_by(text, steps) {
		std::borrow::Cow::Owned(text) => Some(shcl::Document::parse(&text)),
		std::borrow::Cow::Borrowed(_) => None,
	}
}

// Whether a result is fit to write: it loses no more than the input already
// had, every setting it does not write loads as it did, and each key reads as
// exactly what was asked. Lost lines are counted on the bytes (`before` and
// `out`). Settings are compared as a launch parses them (`loaded` and the
// migrated `out`), because the launch's renames and refreshes read how a line
// is written - the quotes around a font stack, the indent of a commented
// header - and a save changes both.
fn reads_as_asked(
	before: &shcl::Document,
	loaded: &shcl::Document,
	out: &str,
	wanted: &[(&str, Option<RatingValue>)],
	steps: &[LaunchStep],
) -> bool {
	let raw_after = shcl::Document::parse(out);
	let migrated = migrated_parse(out, steps);
	let after = migrated.as_ref().unwrap_or(&raw_after);
	let written: Vec<String> = wanted
		.iter()
		.filter(|(_, value)| value.is_some())
		.map(|(leaf, _)| format!("performance.{leaf}"))
		.collect();
	raw_after.lost_count() <= before.lost_count()
		&& settings_besides(after, &written) == settings_besides(loaded, &written)
		// in the file as written and as the next launch leaves it: that launch
		// writes a file from before the nested layout afresh, rating and all
		&& [&raw_after, after].into_iter().all(|doc| {
			wanted.iter().all(|(leaf, value)| {
				let path = format!("performance.{leaf}");
				match value {
					None => true,
					Some(RatingValue::Word(word)) => {
						doc.get_string(&path).is_ok_and(|got| got == *word)
					}
					Some(RatingValue::Flag(flag)) => doc.get_bool(&path) == Ok(*flag),
				}
			})
		})
}

// The value as a canonical save spells it, taken from shcl rather than written
// by hand, so a rating line never differs from what the dialog's save would
// leave (G68, G69). A word is program-made; the guard only stops a later caller
// from putting a quote or a line break into the file.
fn rating_spelling(leaf: &str, value: RatingValue) -> Option<String> {
	let path = format!("performance.{leaf}");
	let mut doc = shcl::Document::new();
	let applied = match value {
		RatingValue::Word(word) => {
			let plain = (1..=32).contains(&word.len())
				&& word
					.bytes()
					.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
			plain && doc.set_string(&path, word)
		}
		RatingValue::Flag(flag) => doc.set_bool(&path, flag),
	};
	if !applied {
		return None;
	}
	doc.to_canonical().lines().find_map(|line| {
		let key = line_setting_key(line)?;
		if key.rsplit('.').next() != Some(leaf) {
			return None;
		}
		line_setting_value(line).map(str::to_string)
	})
}

// One value into the lines: over the key's active line, else after its
// commented default inside the `performance:` block, else first in that block.
// None when there is no block to put it in.
fn place_rating_line(lines: &mut Vec<String>, leaf: &str, spelled: &str) -> Option<()> {
	let path = format!("performance.{leaf}");
	let walk = walk_settings(&lines.join("\n"));
	let depth = |line: &str| line.len() - line.trim_start().len();
	// (index, path, header) of every active setting line
	let active: Vec<(usize, &str, bool)> = walk
		.iter()
		.filter_map(|w| match w {
			WalkLine::Setting {
				index,
				path,
				active: true,
				header,
			} => Some((*index, path.as_str(), *header)),
			_ => None,
		})
		.collect();
	let has_child = |at: usize| {
		active
			.iter()
			.find(|(index, ..)| *index > at)
			.is_some_and(|(index, ..)| depth(&lines[*index]) > depth(&lines[at]))
	};
	// A value cleared by hand leaves `rated_hardware:`, which the walk takes for
	// a header. With nothing under it, it is still the key's own line.
	let own: Vec<usize> = active
		.iter()
		.filter(|(index, p, header)| *p == path && (!header || !has_child(*index)))
		.map(|(index, ..)| *index)
		.collect();
	if let Some((&first, later)) = own.split_first() {
		lines[first] = with_setting_value(&lines[first], spelled);
		// Only the program writes this key, and two of it read as the default,
		// which is itself a test at every launch.
		for &index in later.iter().rev() {
			lines.remove(index);
		}
		return Some(());
	}
	// A block written twice reads as one, so the first takes the line.
	let header = active
		.iter()
		.find(|(_, p, header)| *header && *p == "performance")
		.map(|(index, ..)| *index)?;
	// The block runs to the next active line at or left of its header; comments
	// do not end it. shcl sets the block's depth from its first active line and
	// ignores comments, so that line's indent is the one a new line must match.
	let end = active
		.iter()
		.map(|(index, ..)| *index)
		.find(|&index| index > header && depth(&lines[index]) <= depth(&lines[header]))
		.unwrap_or(lines.len());
	let first_child = active
		.iter()
		.map(|(index, ..)| *index)
		.find(|&index| index > header && index < end);
	let commented = walk.iter().find_map(|w| match w {
		WalkLine::Setting {
			index,
			path: p,
			active: false,
			..
		} if *p == path && index > &header && *index < end => Some(*index),
		_ => None,
	});
	let indent = |line: &str| line[..depth(line)].to_string();
	let lead = match (first_child, commented) {
		(Some(child), _) => indent(&lines[child]),
		(None, Some(comment)) if depth(&lines[comment]) > depth(&lines[header]) => {
			indent(&lines[comment])
		}
		_ => format!("{}\t", indent(&lines[header])),
	};
	let at = commented.map_or(header + 1, |index| index + 1);
	lines.insert(at, format!("{lead}{leaf}: {spelled}"));
	Some(())
}

// An active setting line with a new value: the indent, key and colon kept, and
// any comment the old value carried kept after it, two spaces out as canonical
// writes one.
fn with_setting_value(line: &str, spelled: &str) -> String {
	let Some((head, rest)) = line.split_once(':') else {
		return line.to_string();
	};
	match trailing_comment(rest) {
		Some(comment) => format!("{head}: {spelled}  {comment}"),
		None => format!("{head}: {spelled}"),
	}
}

// Where a comment starts after a value: a '#' after whitespace, outside quotes.
fn trailing_comment(rest: &str) -> Option<&str> {
	let mut quoted = false;
	let mut escaped = false;
	let mut after_space = true;
	for (at, ch) in rest.char_indices() {
		if escaped {
			escaped = false;
		} else if ch == '\\' && quoted {
			escaped = true;
		} else if ch == '"' {
			quoted = !quoted;
		} else if ch == '#' && !quoted && after_space {
			return Some(&rest[at..]);
		}
		after_space = ch.is_whitespace();
	}
	None
}

// shcl's own footer. Kept last: backfill appends a wholly-new section at the
// end of the file, which would otherwise leave the footer stranded in the
// middle. Its Format line is also what marks a file as past shcl 2.x.
const SHCL_BANNER: &str = shcl::GEN_BANNER;

// The first 3.0 footer, whose Syntax link named a tag that was never cut.
const SHCL_BANNER_OLD_V3: &str = "\
##
## This config file format is SHCL.
## \"Simple Hierarchical Config Language\"
##    Format   3
##    Home     https://github.com/yottacore/shcl
##    Syntax   https://github.com/yottacore/shcl/blob/v3.0.0/project/spec.md
##    Legal    SHCL is Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]. License: MIT. No warranty.
##
";

// The footer from before it carried a Format line.
const SHCL_BANNER_OLD_MAIN: &str = "\
##
## This config file format is SHCL.
## \"Simple Hierarchical Config Language\"
##    Home     https://github.com/yottacore/shcl
##    Syntax   https://github.com/yottacore/shcl/blob/main/project/spec.md
##    Legal    SHCL is Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]. License: MIT. No warranty.
##
";

// The same footer from before shcl moved to the yottacore org.
const SHCL_BANNER_OLD_HOME: &str = "\
##
## This config file format is SHCL.
## \"Simple Hierarchical Config Language\"
##    Home     https://github.com/jim-collier/shcl
##    Syntax   https://github.com/jim-collier/shcl/blob/main/project/spec.md
##    Legal    SHCL is Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]. License: MIT. No warranty.
##
";

// The single-'#' spelling it shipped with before the '##' convention won.
const SHCL_BANNER_OLD: &str = "\
#
# This config file format is SHCL.
# \"Simple Hierarchical Config Language\"
#    Home     https://github.com/jim-collier/shcl
#    Syntax   https://github.com/jim-collier/shcl/blob/main/project/spec.md
#    Legal    SHCL is Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]. License: MIT. No warranty.
#
";

// Enough of it to recognize one somebody has rewritten.
const SHCL_BANNER_MARK: &str = "This config file format is SHCL.";

// Put the footer back on a config that predates it, and move it under anything
// backfill appended below it. A footer that has been edited is left alone -
// re-imposing our own wording over someone's would be the rude half of this.
fn with_shcl_banner(text: &str) -> Option<String> {
	let mut lines: Vec<&str> = text.lines().collect();
	for spelling in [
		SHCL_BANNER,
		SHCL_BANNER_OLD_V3,
		SHCL_BANNER_OLD_MAIN,
		SHCL_BANNER_OLD_HOME,
		SHCL_BANNER_OLD,
	] {
		let run: Vec<&str> = spelling.trim_end_matches('\n').lines().collect();
		while let Some(at) = run_at(&lines, &run) {
			lines.drain(at..at + run.len());
		}
	}
	if lines.iter().any(|l| l.contains(SHCL_BANNER_MARK)) {
		return None;
	}
	while lines.last().is_some_and(|l| l.trim().is_empty()) {
		lines.pop();
	}
	if !lines.is_empty() {
		lines.push("");
	}
	lines.extend(SHCL_BANNER.trim_end_matches('\n').lines());
	let mut out = lines.join("\n");
	out.push('\n');
	(out != text).then_some(out)
}

// First index where `run` appears in `lines`.
fn run_at(lines: &[&str], run: &[&str]) -> Option<usize> {
	if run.is_empty() || lines.len() < run.len() {
		return None;
	}
	(0..=lines.len() - run.len()).find(|&i| lines[i..i + run.len()] == *run)
}

// How a file in an older SHCL format reaches the current one, and how many
// settings it leaves behind. shcl's migration respells the file in place,
// comments and layout kept, wherever it can. A file shcl cannot read (not
// UTF-8) or cannot migrate is written new from the template instead, carrying
// every setting that still reads. A current file that is not UTF-8 is written
// again without the lines that do not decode. The writer keeps the old file
// every time. This is the one place that chooses, so a later shcl that
// converts whole files can take it over here.
#[derive(Debug, PartialEq)]
enum Upgrade {
	Current,
	InPlace {
		text: String,
		lost: usize,
	},
	Rewritten {
		text: String,
		lost: usize,
	},
	// `lines` are the ones that did not decode, numbered from 1
	Dropped {
		text: String,
		lost: usize,
		lines: Vec<usize>,
		rewrite: Rewrite,
	},
}

// What a current file with lines that are not UTF-8 is written again as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rewrite {
	// the file as it was, those lines left out
	Kept,
	// the template with every setting that still reads, since leaving the lines
	// out would change what another line means
	Template,
}

fn upgrade(body: &[u8]) -> Upgrade {
	let Ok(text) = std::str::from_utf8(body) else {
		return unreadable(body);
	};
	// shcl 2.x wrote no Format line, and 3.0 reads a few of its spellings
	// differently, most of all a backslash outside double quotes, which a
	// Windows path is full of. `migrate_unstamped` cannot say when it could not
	// finish, so the stamped `migrate` is asked: it adds the Format line only
	// to a file it migrated whole, and not, for one, to a raw block that never
	// closes.
	let stamped = shcl::migrate(text, true);
	if stamped.current {
		return Upgrade::Current;
	}
	if shcl::format_version(&stamped.text).is_none_or(|n| n < shcl::FORMAT_MAJOR) {
		let respelled = shcl::migrate_unstamped(text, true).text;
		let (text, lost) = rebuilt_config_text(&respelled, &[]);
		return Upgrade::Rewritten { text, lost };
	}
	// The footer's Format line is what says it was done, so the footer goes on
	// in place of migrate's own stamp wherever it is ours.
	let migrated = shcl::migrate_unstamped(text, true);
	// None here is a footer somebody rewrote, which gets migrate's stamp
	let out = with_shcl_banner(&migrated.text)
		.filter(|out| out.contains(shcl::FORMAT_LINE))
		.unwrap_or(stamped.text);
	if out == text {
		return Upgrade::Current;
	}
	Upgrade::InPlace {
		text: out,
		lost: migrated.lost,
	}
}

// Not UTF-8. A line that does not decode is left out, and counted when it held
// a setting. An older file is written new, as one shcl cannot migrate is.
fn unreadable(body: &[u8]) -> Upgrade {
	let lossy = String::from_utf8_lossy(body);
	let garbled = garbled_indexes(body);
	if format_of(&lossy) >= shcl::FORMAT_MAJOR {
		return dropped(&lossy, &garbled);
	}
	// migrate keeps every line where it was, so the indexes still match
	let respelled = shcl::migrate_unstamped(&lossy, true).text;
	let (text, lost) = rebuilt_config_text(&respelled, &garbled);
	Upgrade::Rewritten { text, lost }
}

// A current file keeps its own layout with the garbled lines left out, as long
// as every other line reads as it did beside them: a block heading that does
// not decode would hand its lines to the block above. Otherwise, and when no
// setting is left at all, it gets the template with what still reads, which at
// worst is the defaults. A line of shcl's own footer goes back as shipped.
fn dropped(lossy: &str, garbled: &[usize]) -> Upgrade {
	let all: Vec<&str> = lossy.split('\n').collect();
	let footer = garbled_footer(&all, garbled);
	let left_out: Vec<usize> = garbled
		.iter()
		.copied()
		.filter(|index| !footer.iter().any(|(at, _)| at == index))
		.collect();
	let lines: Vec<usize> = left_out.iter().map(|index| index + 1).collect();
	let kept = all
		.iter()
		.enumerate()
		.filter_map(
			|(index, line)| match footer.iter().find(|(at, _)| *at == index) {
				Some((_, shipped)) => Some(shipped.as_str()),
				None => (!left_out.contains(&index)).then_some(*line),
			},
		)
		.collect::<Vec<_>>()
		.join("\n");
	let before: Vec<LineReading> = line_readings(lossy)
		.into_iter()
		.enumerate()
		.filter(|(index, _)| !left_out.contains(index))
		.map(|(_, reading)| reading)
		.collect();
	let after = line_readings(&kept);
	let sets_something = after.iter().any(|reading| reading.active && reading.read);
	if left_out.is_empty() || (before == after && sets_something) {
		let lost = left_out
			.iter()
			.filter(|index| {
				let line = all[**index].trim();
				!line.is_empty() && !line.starts_with('#')
			})
			.count();
		return Upgrade::Dropped {
			text: kept,
			lost,
			lines,
			rewrite: Rewrite::Kept,
		};
	}
	let (text, lost) = rebuilt_config_text(lossy, garbled);
	Upgrade::Dropped {
		text,
		lost,
		lines,
		rewrite: Rewrite::Template,
	}
}

// The lines of shcl's footer in `all` that did not decode, each with its text
// as shipped. An editor saving in Latin-1 turns the footer's copyright sign
// into one, and left out, the footer would read as edited and never be
// refreshed again. A run counts as the footer while the lines that decode
// outnumber the ones that do not, and each that decodes is as shipped.
fn garbled_footer(all: &[&str], garbled: &[usize]) -> Vec<(usize, String)> {
	let banner: Vec<&str> = SHCL_BANNER.trim_end_matches('\n').lines().collect();
	let Some(last) = all.len().checked_sub(banner.len()) else {
		return Vec::new();
	};
	for start in (0..=last).rev() {
		let bad: Vec<usize> = (start..start + banner.len())
			.filter(|index| garbled.contains(index))
			.collect();
		if bad.is_empty() || bad.len() * 2 >= banner.len() {
			continue;
		}
		let fits = banner.iter().enumerate().all(|(at, shipped)| {
			let index = start + at;
			bad.contains(&index) || all[index].trim_end_matches('\r') == *shipped
		});
		if fits {
			return bad
				.into_iter()
				.map(|index| {
					let ending = if all[index].ends_with('\r') { "\r" } else { "" };
					(index, format!("{}{ending}", banner[index - start]))
				})
				.collect();
		}
	}
	Vec::new()
}

// How one line reads: the setting it names, whether that is active, whether it
// is part of a raw block, and whether shcl reads it.
#[derive(Debug, PartialEq)]
struct LineReading {
	path: Option<String>,
	active: bool,
	fence: bool,
	read: bool,
}

fn line_readings(text: &str) -> Vec<LineReading> {
	let unread = unreadable_lines(&shcl::Document::parse(text));
	walk_settings(text)
		.into_iter()
		.enumerate()
		.map(|(index, walked)| {
			let read = !unread.contains(&(index + 1));
			match walked {
				WalkLine::Setting {
					path,
					active,
					header,
					..
				} => LineReading {
					path: Some(path),
					active: active && !header,
					fence: false,
					read,
				},
				WalkLine::Fence => LineReading {
					path: None,
					active: false,
					fence: true,
					read,
				},
				WalkLine::Blank | WalkLine::Other(_) => LineReading {
					path: None,
					active: false,
					fence: false,
					read,
				},
			}
		})
		.collect()
}

// The text a file converts to, or None for a current one.
fn upgraded_text(body: &[u8]) -> Option<String> {
	match upgrade(body) {
		Upgrade::Current => None,
		Upgrade::InPlace { text, .. }
		| Upgrade::Rewritten { text, .. }
		| Upgrade::Dropped { text, .. } => Some(text),
	}
}

fn from_shcl2_text(text: &str) -> Option<String> {
	upgraded_text(text.as_bytes())
}

// The indexes of the lines in `body` that are not UTF-8. A newline byte is
// never part of a longer character, so each line decodes or not on its own.
fn garbled_indexes(body: &[u8]) -> Vec<usize> {
	body.split(|b| *b == b'\n')
		.enumerate()
		.filter(|(_, line)| std::str::from_utf8(line).is_err())
		.map(|(index, _)| index)
		.collect()
}

// The settings file's text as a reader takes it. A file shcl cannot read reads
// as what the write that converts it puts there, so a launch that found it busy,
// and a save after that launch, read what the file is about to hold.
fn settings_text(body: &[u8]) -> String {
	match std::str::from_utf8(body) {
		Ok(text) => text.to_string(),
		// `upgrade` writes again every file that is not UTF-8, so the lossy
		// read is never reached
		Err(_) => upgraded_text(body).unwrap_or_else(|| String::from_utf8_lossy(body).into_owned()),
	}
}

fn read_settings(path: &std::path::Path) -> std::io::Result<String> {
	let text = std::fs::read(path).map(|body| settings_text(&body))?;
	note_seen(path, &text);
	Ok(text)
}

fn read_settings_text(path: &std::path::Path) -> Option<String> {
	read_settings(path).ok()
}

fn convert_shcl2_config(path: &std::path::Path) {
	let Ok(body) = std::fs::read(path) else {
		return;
	};
	let Some(out) = upgraded_text(&body) else {
		return;
	};
	if config_open_elsewhere(path) {
		note_config_busy(path);
		return;
	}
	if let Err(e) = write_config_atomic(path, &out) {
		eprintln!(
			"{APP_NAME}: could not update config {}: {e}",
			path.display()
		);
	}
}

// What the conversion of `body`, the file as it was, could not carry over. In
// place, shcl counts the lines 2.x gave a bracketed list to: the new format has
// no way to write one, so they stay as written and set nothing. A line it
// cannot read at all is said at every launch by `config_complaints`. A new
// file counts every setting it left behind. A current file written again for
// lines that are not UTF-8 is always said, since that write made a copy.
fn conversion_losses(
	body: &[u8],
	path: &std::path::Path,
	backup: Option<PathBuf>,
) -> Option<ConversionLoss> {
	let (lost, how, lines) = match upgrade(body) {
		Upgrade::Current => return None,
		Upgrade::InPlace { lost, .. } => (lost, Converted::InPlace, Vec::new()),
		Upgrade::Rewritten { lost, .. } => (lost, Converted::Rewritten, Vec::new()),
		Upgrade::Dropped {
			lost,
			lines,
			rewrite,
			..
		} => (lost, Converted::Dropped(rewrite), lines),
	};
	(lost > 0 || matches!(how, Converted::Dropped(_))).then(|| ConversionLoss {
		path: path.to_path_buf(),
		backup,
		lost,
		how,
		lines,
	})
}

// Settings a conversion could not keep, at launch or in a later write. The
// terminal hears at once; the window says it in a notice once it is on screen,
// as for a refused save.
#[derive(Clone, Debug, PartialEq)]
pub struct ConversionLoss {
	pub path: PathBuf,
	pub backup: Option<PathBuf>,
	pub lost: usize,
	pub how: Converted,
	// the lines left out for not being UTF-8, numbered from 1 (`Dropped` only)
	pub lines: Vec<usize>,
}

// Whether the file was converted where it stood, written new, or written again
// without lines that are not UTF-8 (`upgrade`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Converted {
	InPlace,
	Rewritten,
	Dropped(Rewrite),
}

impl ConversionLoss {
	fn terminal_line(&self) -> String {
		let kept = self
			.backup
			.as_ref()
			.map(|copy| format!(" The old file is at {}.", copy.display()))
			.unwrap_or_default();
		match self.how {
			Converted::InPlace => format!(
				"{APP_NAME}: {}: {} line(s) set a list in brackets, which the new format cannot hold; they are kept as written but set nothing.{kept}",
				self.path.display(),
				self.lost
			),
			Converted::Rewritten => format!(
				"{APP_NAME}: {}: could not be converted in place, so a new file was written; {} setting(s) could not be carried over.{kept}",
				self.path.display(),
				self.lost
			),
			Converted::Dropped(Rewrite::Kept) if self.lines.is_empty() => format!(
				"{APP_NAME}: {}: its SHCL footer was not UTF-8 text, so the file was written again with the footer as shipped.{kept}",
				self.path.display()
			),
			Converted::Dropped(Rewrite::Kept) => format!(
				"{APP_NAME}: {}: not UTF-8 text at{}, so the file was written again without {}.{kept}",
				self.path.display(),
				line_list(&self.lines),
				if self.lines.len() == 1 { "it" } else { "them" }
			),
			Converted::Dropped(Rewrite::Template) => format!(
				"{APP_NAME}: {}: not UTF-8 text at{}, so a new file was written from the defaults; {} setting(s) could not be carried over.{kept}",
				self.path.display(),
				line_list(&self.lines),
				self.lost
			),
		}
	}
}

static LOST_IN_CONVERSION: std::sync::Mutex<Option<ConversionLoss>> = std::sync::Mutex::new(None);

pub fn take_conversion_loss() -> Option<ConversionLoss> {
	crate::locks::lock(&LOST_IN_CONVERSION).take()
}

fn refresh_shcl_banner(path: &std::path::Path) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	// The footer's Format line says a file was converted. One still in 2.x
	// spellings, because its conversion was put off, waits for the next launch.
	if from_shcl2_text(&text).is_some() {
		return;
	}
	let Some(out) = with_shcl_banner(&text) else {
		return;
	};
	if config_open_elsewhere(path) {
		note_config_busy(path);
		return;
	}
	if let Err(e) = write_config_atomic(path, &out) {
		eprintln!(
			"{APP_NAME}: could not update config {}: {e}",
			path.display()
		);
	}
}

// Path -> line index for the current file lines.
fn paths_at(lines: &[String]) -> std::collections::HashMap<String, usize> {
	let text = lines.join("\n");
	walk_settings(&text)
		.into_iter()
		.filter_map(|w| match w {
			WalkLine::Setting { index, path, .. } => Some((path, index)),
			_ => None,
		})
		.collect()
}

enum Anchor {
	Before(usize), // insert at this line index
	After(usize),  // insert just after this line index
	Append,        // end of file
}

// Where a missing template path belongs in the file: before the next present
// template sibling (same parent), after the last present earlier sibling's
// subtree, after the parent block header, or appended at the end. Whole-group
// inserts back up over the next sibling's comment block so the new group sits
// above it rather than splitting the comments from their setting.
fn anchor_for(
	p: &str,
	order: &[String],
	at: &std::collections::HashMap<String, usize>,
	lines: &[String],
	whole_group: bool,
) -> Anchor {
	let parent = p.rsplit_once('.').map_or("", |(head, _)| head);
	let my_pos = order.iter().position(|o| o == p);
	let siblings: Vec<&String> = order
		.iter()
		.filter(|o| o.as_str() != p && o.rsplit_once('.').map_or("", |(head, _)| head) == parent)
		.collect();
	if let Some(my_pos) = my_pos {
		// next sibling in template order that the file has
		let next = siblings.iter().find(|s| {
			order
				.iter()
				.position(|o| o == **s)
				.is_some_and(|pos| pos > my_pos)
				&& at.contains_key(**s)
		});
		if let Some(next) = next {
			let mut index = at[*next];
			if whole_group {
				// sit above the sibling's own comment block, not inside it. A
				// commented-out setting is not part of that block, and neither is a
				// comment at another depth: both belong to what comes before, often
				// a group added a moment ago.
				let indent_of = |line: &str| line.len() - line.trim_start().len();
				let depth = indent_of(&lines[index]);
				while index > 0 && {
					let above = &lines[index - 1];
					above.trim_start().starts_with('#')
						&& line_setting_key(above).is_none()
						&& indent_of(above) == depth
				} {
					index -= 1;
				}
			}
			return Anchor::Before(index);
		}
		// last earlier sibling present: insert after its whole subtree
		let prev = siblings.iter().rev().find(|s| {
			order
				.iter()
				.position(|o| o == **s)
				.is_some_and(|pos| pos < my_pos)
				&& at.contains_key(**s)
		});
		if let Some(prev) = prev {
			let prefix = format!("{prev}.");
			let end = at
				.iter()
				.filter(|(path, _)| *path == *prev || path.starts_with(&prefix))
				.map(|(_, index)| *index)
				.max()
				.unwrap_or(at[*prev]);
			return Anchor::After(end);
		}
	}
	if !parent.is_empty() {
		if let Some(index) = at.get(parent) {
			return Anchor::After(*index);
		}
	}
	Anchor::Append
}

// Set by `--config PATH` before any settings are read; overrides the default
// location for this process.
static CONFIG_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();
// Serializes the tests that install a config-path override. The override is
// process-global, so two of them running at once would each read the other's
// file - and they live in different modules, so the guard has to live here.
// The refusal `write_doc` leaves for the window is process-global too, so a
// test whose save can be refused takes this as well.
#[cfg(test)]
pub fn test_config_lock() -> std::sync::MutexGuard<'static, ()> {
	static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
	crate::locks::lock(&LOCK)
}

// Serializes the tests that put settings into the live store, which is
// process-global as well. A test that installs a rated profile would otherwise
// change the scroll feel under a scroll test running beside it.
#[cfg(test)]
pub fn test_store_lock() -> std::sync::MutexGuard<'static, ()> {
	static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
	crate::locks::lock(&LOCK)
}

pub fn set_config_override(path: PathBuf) {
	#[cfg(test)]
	{
		*crate::locks::lock(test_override()) = Some(path.clone());
	}
	let _ = CONFIG_OVERRIDE.set(path);
}

// `--config` is decided once, before any setting is read, which is exactly what
// a OnceLock says - but a test installs SEVERAL throwaway configs in one
// process, and the second `set` there is silently ignored. That failed quietly
// and in the worst possible way: every later test wrote its own file and read
// the FIRST one's, so one test's leftovers steered another's assertions
// (measured: the theme round-trip read theme_mode "light" out of the generic
// row test's config and stored its edit in the wrong variant of the palette).
// So a test build gets a settable door beside the OnceLock. It is still
// process-global, hence `test_config_lock` around any test that uses it.
#[cfg(test)]
fn test_override() -> &'static std::sync::Mutex<Option<PathBuf>> {
	static OVERRIDE: OnceLock<std::sync::Mutex<Option<PathBuf>>> = OnceLock::new();
	OVERRIDE.get_or_init(|| std::sync::Mutex::new(None))
}

// Stands in for the box's own settings file, so a test can show what a run
// with no override would do to it without going near the real one.
#[cfg(test)]
fn test_native_config() -> &'static std::sync::Mutex<Option<PathBuf>> {
	static NATIVE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
	&NATIVE
}

// The directory name under whichever base a platform hands us. One spelling, so
// the config dir, the data dir and the legacy probe cannot drift apart.
const APP_DIR: &str = "silkterm";

// Which platform's conventions to lay the paths out by. Named rather than read
// off `cfg!` at each site so the WHOLE scheme can be tested for every platform
// from any one of them - the boxes here are Linux and Windows, and a macOS path
// nobody present can run is exactly the kind that rots unnoticed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layout {
	Windows,
	MacOs,
	Xdg,
}

fn host_layout() -> Layout {
	if cfg!(windows) {
		Layout::Windows
	} else if cfg!(target_os = "macos") {
		Layout::MacOs
	} else {
		Layout::Xdg
	}
}

// An environment variable as a path, treating empty as unset - an exported but
// empty HOME is not a home directory.
fn env_path(name: &str) -> Option<PathBuf> {
	std::env::var_os(name)
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
}

pub fn home_dir() -> Option<PathBuf> {
	env_path("HOME").or_else(|| env_path("USERPROFILE"))
}

// Where settings live, by platform convention.
//
// An explicit XDG_CONFIG_HOME is honoured on EVERY platform - somebody who sets
// it means it - and only then does the platform get its say: Roaming on Windows
// (settings are meant to follow the user between machines), Application Support
// on macOS, ~/.config elsewhere. Each falls back to ~/.config if the native base
// is missing from the environment, which is better than having no config at all.
fn config_base_for(
	layout: Layout,
	xdg: Option<&std::path::Path>,
	home: Option<&std::path::Path>,
	appdata: Option<&std::path::Path>,
) -> Option<PathBuf> {
	if let Some(dir) = xdg {
		return Some(dir.to_path_buf());
	}
	let dotconfig = || home.map(|h| h.join(".config"));
	match layout {
		Layout::Windows => appdata.map(std::path::Path::to_path_buf).or_else(dotconfig),
		Layout::MacOs => home
			.map(|h| h.join("Library").join("Application Support"))
			.or_else(dotconfig),
		Layout::Xdg => dotconfig(),
	}
}

fn config_path() -> Option<PathBuf> {
	// Every override a test sets goes through this door as well (G52), so the
	// OnceLock adds nothing there, and leaving it out lets a test go back to none.
	#[cfg(test)]
	let chosen = crate::locks::lock(test_override()).clone();
	#[cfg(not(test))]
	let chosen = CONFIG_OVERRIDE.get().cloned();
	chosen.or_else(native_config_path)
}

// The settings file with no `--config`: the box's own.
fn native_config_path() -> Option<PathBuf> {
	#[cfg(test)]
	if let Some(path) = crate::locks::lock(test_native_config()).clone() {
		return Some(path);
	}
	env_config_path()
}

fn env_config_path() -> Option<PathBuf> {
	let xdg = env_path("XDG_CONFIG_HOME");
	let home = home_dir();
	let appdata = env_path("APPDATA");
	let base = config_base_for(
		host_layout(),
		xdg.as_deref(),
		home.as_deref(),
		appdata.as_deref(),
	)?;
	Some(base.join(APP_DIR).join("config.shcl"))
}

pub fn config_dir() -> Option<PathBuf> {
	Some(config_path()?.parent()?.to_path_buf())
}

// A test run reads the box's own config on purpose (G6), but writes nothing
// there or beside it: a test's launch once refreshed a line in a real file. A
// test that needs a write points the config at a file of its own.
#[cfg(test)]
pub(crate) fn may_write(path: &std::path::Path) -> bool {
	let seam = crate::locks::lock(test_native_config()).clone();
	let mut dirs: Vec<PathBuf> = [seam, env_config_path()]
		.into_iter()
		.flatten()
		.filter_map(|file| file.parent().map(std::path::Path::to_path_buf))
		.collect();
	dirs.extend(data_dir_for(
		host_layout(),
		env_path("XDG_CONFIG_HOME").is_some(),
		env_path("LOCALAPPDATA").as_deref(),
		None,
	));
	let real: Vec<PathBuf> = dirs.iter().flat_map(std::fs::canonicalize).collect();
	dirs.extend(real);
	// a link into the box's folder, or a name not made yet
	let mut names = vec![path.to_path_buf()];
	names.extend(std::fs::canonicalize(path));
	if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
		names.extend(std::fs::canonicalize(parent).map(|dir| dir.join(name)));
	}
	!names
		.iter()
		.any(|name| dirs.iter().any(|dir| name.starts_with(dir)))
}

#[cfg(not(test))]
pub(crate) fn may_write(_: &std::path::Path) -> bool {
	true
}

// Where bulk, machine-local data goes. On Windows that is Local rather than
// Roaming: a wallpaper pack is 60 MiB and has no business following the user
// onto every machine they sign into. Everywhere else it IS the config dir, which
// is what those platforms' conventions already mean.
//
// Two cases deliberately keep everything together instead: a `--config` override
// (so an alternate config still gets its own wallpaper and history rather than
// sharing the default ones - that isolation is the point of the flag), and an
// explicit XDG_CONFIG_HOME (somebody who asks for one tree means one tree).
pub fn data_dir() -> Option<PathBuf> {
	data_dir_for(
		host_layout(),
		CONFIG_OVERRIDE.get().is_some() || env_path("XDG_CONFIG_HOME").is_some(),
		env_path("LOCALAPPDATA").as_deref(),
		config_dir(),
	)
}

// The decision above with its inputs passed in, so every platform's answer can
// be checked from any box (G15).
fn data_dir_for(
	layout: Layout,
	one_tree: bool,
	local_appdata: Option<&std::path::Path>,
	config_dir: Option<PathBuf>,
) -> Option<PathBuf> {
	match layout {
		Layout::Windows if !one_tree => local_appdata.map(|dir| dir.join(APP_DIR)).or(config_dir),
		_ => config_dir,
	}
}

// Every directory a wallpaper folder may sit in, best first. More than one entry
// only where the data dir split off from the config dir - a pack already beside
// the config still has to be found, because nobody should have to move 60 MiB to
// keep what already worked.
fn wallpaper_search_dirs() -> Vec<PathBuf> {
	let mut dirs = Vec::new();
	if let Some(dir) = data_dir() {
		dirs.push(dir);
	}
	if let Some(dir) = config_dir() {
		if !dirs.contains(&dir) {
			dirs.push(dir);
		}
	}
	dirs
}

// A config left at ~/.config by a build that predates the per-platform layout.
// None where that IS the native location, so there is nothing to adopt on Linux.
fn legacy_config_path() -> Option<PathBuf> {
	if CONFIG_OVERRIDE.get().is_some() || host_layout() == Layout::Xdg {
		return None;
	}
	let legacy = home_dir()?
		.join(".config")
		.join(APP_DIR)
		.join("config.shcl");
	// An XDG_CONFIG_HOME pointing at the old spot makes the two the same file.
	if config_path().is_some_and(|native| native == legacy) {
		return None;
	}
	Some(legacy)
}

// Bring a pre-layout config across, ONCE, and only when the native location
// holds nothing. If both exist the native one is the live file and the old one
// is left exactly where it is - guessing which of two real configs somebody
// wants is not a call this can make, so it says what it found and moves on.
// Called before the default template would be written, or a fresh template
// would win the race against the file being adopted.
fn adopt_legacy_config() {
	let (Some(native), Some(legacy)) = (config_path(), legacy_config_path()) else {
		return;
	};
	if !legacy.exists() || !may_write(&native) {
		return;
	}
	if native.exists() {
		eprintln!(
			"{APP_NAME}: using {}; the older {} is being ignored (delete it, or point --config at it)",
			native.display(),
			legacy.display()
		);
		return;
	}
	if let Some(dir) = native.parent() {
		if let Err(e) = std::fs::create_dir_all(dir) {
			eprintln!("{APP_NAME}: could not create {}: {e}", dir.display());
			return;
		}
	}
	// rename first: same volume is the ordinary case and it is atomic. A cross
	// volume move fails EXDEV, so fall back to copy-then-remove, and keep the
	// original if the remove fails rather than risk having neither.
	let moved = std::fs::rename(&legacy, &native).is_ok()
		|| (std::fs::copy(&legacy, &native).is_ok() && std::fs::remove_file(&legacy).is_ok());
	if moved {
		eprintln!(
			"{APP_NAME}: moved your config from {} to {}",
			legacy.display(),
			native.display()
		);
	} else {
		eprintln!(
			"{APP_NAME}: could not move {} to {}",
			legacy.display(),
			native.display()
		);
	}
}

// The shipped template, with `{HOME}` filled in from the same constant
// `Settings::default()` uses, so the commented default line and the real
// default cannot drift apart.
//
// Everything that compares a config against the template (backfill, the
// superseded-default refresh, the group walk) reads it through here, so the
// text is assembled once and every one of them sees the same bytes.
static DEFAULT_CONFIG_TEXT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
	DEFAULT_CONFIG_TEMPLATE
		.replace("{HOME}", HOME_TOKEN)
		.replace("{WPDIR}", &wallpaper_dir_escaped())
		.replace(
			"{KEYS}",
			&crate::keys::template_lines(cfg!(target_os = "macos")),
		)
});

fn default_config() -> &'static str {
	DEFAULT_CONFIG_TEXT.as_str()
}

// How a shipped default names the home directory: the variable somebody on this
// platform would type. A config written here still works if it is carried
// elsewhere, since both names are read on both platforms.
#[cfg(windows)]
pub const HOME_TOKEN: &str = "%USERPROFILE%";
#[cfg(not(windows))]
pub const HOME_TOKEN: &str = "$HOME";

// The wallpaper folder's shipped default: the usual place on this platform, in
// the same spelling. It is looked up rather than expanded (`rotation_folder_for`),
// so it is right even where XDG_CONFIG_HOME is unset. The template writes it in
// double quotes, so a backslash goes in doubled (`wallpaper_dir_escaped`).
#[cfg(windows)]
pub const WALLPAPER_DIR_TOKEN: &str = r"%LOCALAPPDATA%\silkterm\wallpaper";
#[cfg(target_os = "macos")]
pub const WALLPAPER_DIR_TOKEN: &str = "$HOME/Library/Application Support/silkterm/wallpaper";
#[cfg(not(any(windows, target_os = "macos")))]
pub const WALLPAPER_DIR_TOKEN: &str = "$XDG_CONFIG_HOME/silkterm/wallpaper";

fn wallpaper_dir_escaped() -> String {
	WALLPAPER_DIR_TOKEN.replace('\\', r"\\")
}

const DEFAULT_CONFIG_TEMPLATE: &str = r##"# SilkTerm configuration file.
#
## Delete this file to reset everything. A line starting with '# ' is a
## setting at its default. Remove the '# ' to change it. Paths can use ~,
## $NAME and %NAME%.

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Performance
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

performance:

	# automatic: true  ## Default

	## Visual effects level. "max", "high", "low", "standard" (no effects),
	## or "custom" (use the values in this file).
	# profile: "max"  ## Default

	# check_hardware: true  ## Default
	# check_next_run: false  ## Default
	# rated_hardware: ""  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Background and transparency
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

transparency:

	# enabled: false  ## Default
	opacity: 0.95

	# blur_behind: false  ## Default

wallpaper:

	# enabled: true  ## Default
	## A single image, instead of the folder below. Empty uses the folder.
	# image: ""  ## Default
	# fallback_builtin: true  ## Default

	## Use a folder of images instead of the single image above. Interval 0
	## picks one at launch and keeps it.
	rotate:
		# enabled: true  ## Default
		## The default is the usual place on this system. A wallpapers or
		## backgrounds folder there is found too.
		# folder: "{WPDIR}"  ## Default
		# interval_s: 0.0  ## Default
		# random: true  ## Default

	## How much of the picture shows through the background color. Light mode
	## mixes it differently to reach the same visible result, so the number means
	## the same thing in either mode - at 10% a picture is plainly there over a
	## dark background and all but gone over a light one.
	# opacity: 0.10  ## Default

	## Hold every picture to the visibility above, whatever its own brightness,
	## so a bright photo does not glare where a dark one is barely there. A
	## picture further from the background color than usual is drawn at less than
	## the number says, and one closer at more. 0 turns it off. At 100%
	## visibility the picture is always drawn as it is.
	# even_visibility: 1.0  ## Default

	## "stretch" fills the window even if that distorts the image. "zoom" keeps
	## the proportions and crops the edges.
	# default_fit: "stretch"  ## Default

	## If an image has its own fit, opacity or blur tags, use those instead.
	# honor_xmp: true  ## Default
	# blur: 10.0  ## Default
	# honor_xmp_look: true  ## Default

	contrast_mask:
		# enabled: true  ## Default
		# size: 0.5  ## Default
		# strength: 0.5  ## Default
		# auto: 0.5  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Font
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

font:

	use_system_family: true
	# use_system_size: true  ## Default

	family: "Monaspace Argon, Fira Code, JetBrains Mono, Cascadia Mono, Consolas, Ubuntu Mono, SF Mono, Menlo, Courier New"

	# size: 17.0  ## Default
	line_height_scale: 1.22

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Text
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

text:

	## A blurred patch of background color behind each letter, so text stays
	## readable over a wallpaper.
	scrim:
		# enabled: true  ## Default
		## Light mode quiets the patch by about a doubling and a half: a pale
		## patch on a darkened background shows more than a dark one does on a
		## lightened background.
		# strength: 20  ## Default
		# radius: 8.0  ## Default
		# softness: 0.5  ## Default
		## The patch's shape. "sdf" is rounded and soft. "dt" is the same but
		## solid, with hard edges. "dilate" is square. "gaussian" is the old
		## method and looks worse.
		# function: "sdf"  ## Default
		## How the patch fades out toward its edge. "exp" stays strong then
		## drops off fast. "log" drops off fast then fades slowly. "sigmoid" is
		## smooth at both ends. "half_normal" is a bell curve. "linear" is a
		## straight line.
		# ramp: "exp"  ## Default
		# regular_weight: true  ## Default

	# outline: 1.0  ## Default

	## How much of the correction text darker than its background gets.
	## Partly covered pixels are blended in linear light, which costs dark text
	## on a light background most of the ink at the edge of every stroke, so a
	## light theme reads thin and pale. At 1.0 a letter carries the ink it was
	## drawn with, the way almost every other program paints text; 0 turns the
	## correction off. Above 1.0 it keeps going, for a display or a font where
	## even that reads light - expect small letters to start closing up.
	## Light themes only - light-on-dark text is left alone.
	## Range: 0 to 2
	# dark_on_light: 1.0  ## Default

	## Brighten or darken text that is too close to its background color to
	## read. 0 is off.
	# min_contrast: 0.45  ## Default

	# color_emoji: true  ## Default
	# embolden_inverse: true  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Cursor
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

cursor:

	## Percent of the cell: 100/100 block, 100/25 bar, 15/100 underline.
	size:
		# height: 100  ## Default
		# width: 100  ## Default

	## "none", "phase" (fade), "pulse_vertical", "pulse_horizontal",
	## "pulse_both". All of them stop after some time with no typing.
	# animation: "pulse_vertical"  ## Default

	# animation_resume_s: 1  ## Default
	# animation_idle_stop_s: 60  ## Default
	# blink_rate_ms: 500  ## Default
	# scrim: false  ## Default
	# outline: true  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Selection
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

selection:

	# word_separators: ",│`|\"' ()[]{}<>\t"  ## Default
	# pairs: "`` \"\" '' {} () [] <>"  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Scrolling
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

scroll:

	scrollback: 10000
	# smooth: true  ## Default

	## Timing for scrolling in new output, from the start of a burst to the
	## end. Milliseconds.
	ease_in_ms: 82.0
	ramp_up_ms: 96.0
	single_screen_tau_ms: 32.0
	ramp_down_ms: 144.0
	ease_out_ms: 212.0

	wheel_lines: 3.0
	alt_scroll_lines: 3.0
	output_ease_lines: 1.0

	## Also animate scrolling in programs like less and vim, which redraw the
	## screen instead of scrolling it.
	# smooth_apps: true  ## Default

	scrollbar:
		# enabled: true  ## Default
		# thickness: 16.0  ## Default
		# auto_hide: true  ## Default

	## The minimap takes space away from the text. The scrollbar does not.
	minimap:
		# enabled: true  ## Default
		# width: 100.0  ## Default
		# tui_process_whitelist: "less tmux screen"  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Theme and colors
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

theme: SilkTerm
theme_mode: dark

## Overrides for the current theme. The two scrollbar colors are not part
## of any theme.
colors:
	## Take the text and cursor colors from the wallpaper instead: the text is
	## placed as far as it can get from the picture's brightest areas, in a hue
	## complementary to the picture's own. The two rows for them gray out in
	## Settings while this is on, and nothing about the derived colors is saved.
	# from_wallpaper: true  ## Default
	# background: "#000000"  ## Default
	# foreground: "#88eecc"  ## Default
	# cursor: "#8a3fa4"  ## Default
	# highlight: "#c8a05a"  ## Default
	# focus: "#4086ff"  ## Default
	# menu_background: "#36363b"  ## Default
	# menu_foreground: "#f0f0f2"  ## Default
	# dialog_background: "#20202a"  ## Default
	# dialog_foreground: "#e2e2ea"  ## Default
	# gutter: "#16161e"  ## Default
	# scrollbar_thumb: "#8a8a92"  ## Default
	# scrollbar_trough: "#2e2e36"  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Window
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

window:

	margin: 8.0

	columns: 160
	rows: 48

	# remember_size: true  ## Default
	remembered_columns: 160
	remembered_rows: 48
	remembered_font_zoom: 0

	## While remember_size is on, also keep a size and font zoom for each
	## monitor. A window opens at its monitor's, and takes those of the one it
	## is moved to once it stops there. They go under monitors:, one block per
	## monitor, named for its resolution, its scale and, where the system
	## reports it, its physical size in millimeters.
	# remember_per_monitor: true  ## Default

	# remember_maximized: false  ## Default
	remembered_maximized: false

	# hide_single_tab: false  ## Default

	## What a tab says, and whether the window title falls back to it. The
	## first line lets a title the running program asked for name the tab,
	## outranked only by a name typed on the tab itself. The three after it
	## are what the tab works out on its own; turning them all off leaves a
	## tab naming its shell, since a tab with no text cannot be told from the
	## one beside it. The tab's flyover always names the lot, whatever these
	## say.
	# tab_shows_title: true  ## Default
	# tab_shows_shell: true  ## Default
	# tab_shows_program: true  ## Default
	# tab_shows_directory: true  ## Default
	# title_shows_tab: true  ## Default

	## Let the graphics card's memory go after the window has sat unused, and
	## take it back the moment the window is used again. The waits are in
	## minutes: the first for a window that is minimized, the second for one
	## that is covered, the third for one that is only unfocused with nothing
	## printing. On Windows with transparency on, a window still on screen is
	## never let go, since it would turn black.
	# idle_release: true  ## Default
	# idle_release_minimized_min: 1  ## Default
	# idle_release_hidden_min: 30  ## Default
	# idle_release_min: 240  ## Default

	## Draw on the processor rather than the graphics card. Slower, but uses
	## none of the card's memory. Where the card cannot give a window what it
	## needs, the window falls back to this on its own. Not on macOS.
	# software_rendering: false  ## Default

	## Tab width as a percent of the window width.
	# tab_regular_width_pct: 10.0  ## Default
	# tab_max_width_pct: 100.0  ## Default

	## Seconds a tab's flyover stays up before it goes away. It comes back
	## once the pointer has left the tab and come back. 0 keeps it up.
	# tab_tip_max_s: 30  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Hyperlinks
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

hyperlinks:

	## Recognized: http, https, ftp, ftps, sftp, ssh, file and mailto links.
	# enabled: true  ## Default

	## Empty uses the desktop's own opener. Example: "firefox --new-tab"
	# open_command: ""  ## Default

## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Keys
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

keys:

	## Each hotkey is one or more key combinations, separated by spaces. A
	## combination is the keys held, then the key pressed, joined by "+",
	## such as "Ctrl+Shift+T". The keys held are Ctrl, Alt, Shift and
	## Command. On a Mac, Alt is Option; elsewhere, Command is the Windows
	## or Super key. These keys are written as words: Plus, Minus, Space,
	## Tab, Enter, Escape, Backspace, Insert, Delete, Home, End, PageUp,
	## PageDown, Left, Right, Up, Down, Menu, and F1 to F12. "none" turns a
	## hotkey off. A combination set here is taken from any hotkey that has
	## it by default.
{KEYS}
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
## Shell
## ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

## A list of installed shells is added here after the first launch. The
## first enabled one is the default.

shell:

	## Applied at every launch. Same syntax as the real command line, which
	## overrides these. Example: "--new-pane --right --size 35%"
	# command_line: ""  ## Default

	## Used when launched from a menu or shortcut, not from a shell. A new tab
	## or split starts in the same directory as the pane it came from.
	# startup_directory: "{HOME}"  ## Default

	## Adds a few lines to each PowerShell profile, so a new tab or split can
	## start in the pane's current directory.
	# integration: true  ## Default

	## Give bash panes the x9ps1-git prompt, which shows git status. It
	## replaces a prompt set in .bashrc.
	# bash_prompt: false  ## Default

	# copy_on_select: true  ## Default

##
## This config file format is SHCL.
## "Simple Hierarchical Config Language"
##    Format   3
##    Home     https://github.com/yottacore/shcl
##    Syntax   https://github.com/yottacore/shcl/blob/v3.0.0-beta1/project/spec.md
##    Legal    SHCL is Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]. License: MIT. No warranty.
##
"##;

#[cfg(test)]
mod tests {
	use super::*;

	// The About box reports how long the session has been up, and the clock it
	// reads runs from the mark main sets rather than from whatever first asked.
	// A second mark must not restart it, or a later caller would reset the
	// session's own clock.
	// Test ID: EqRZeqO
	#[test]
	fn a_session_uptime_runs_from_the_launch_mark() {
		mark_launch();
		let first = uptime();
		std::thread::sleep(std::time::Duration::from_millis(20));
		let later = uptime();
		assert!(
			later >= first + std::time::Duration::from_millis(10),
			"{later:?}"
		);
		mark_launch();
		assert!(uptime() >= later, "a second mark restarted the clock");
	}

	// The shipped menu background is the inactive tab's own bytes, so a tip
	// filled with it drew as a tab that grew downward. It has to sit off every
	// tab color - lighter, and warm where the whole strip leans blue - without
	// turning into a different surface. The lightness floor and ceiling are the
	// strip's own inactive-to-active step either side, so "slightly lighter"
	// means what the strip already means by it.
	// Test ID: EqQPotE
	#[test]
	fn a_tip_sits_off_every_tab_color() {
		let l = |c: [u8; 3]| crate::palette::to_oklab(c).0;
		let warmth = |c: [u8; 3]| crate::palette::to_oklab(c).2;
		let tip = tip_bg_of(crate::theme::MENU_BG_DEF);
		let step = l(TAB_ACTIVE) - l(TAB_INACTIVE);
		for (name, tab) in [
			("bar", TAB_BAR_BG),
			("inactive", TAB_INACTIVE),
			("active", TAB_ACTIVE),
		] {
			assert!(
				l(tip) - l(tab) >= step * 0.75,
				"{name}: the tip is not clearly lighter ({:.4} against a {step:.4} tab step)",
				l(tip) - l(tab)
			);
			assert!(
				warmth(tip) - warmth(tab) >= 0.012,
				"{name}: the tip is not warmer than the tab"
			);
		}
		// The ceiling is against the lightest tab, since that is the one a tip can
		// stop looking like chrome by outrunning.
		assert!(
			l(tip) - l(TAB_ACTIVE) <= step * 2.5,
			"the tip has stopped being a shade of the chrome ({:.4})",
			l(tip) - l(TAB_ACTIVE)
		);
	}

	// The three tip colors are the point of the item, so none of them may come
	// back as the menu color it is derived from.
	// Test ID: EqQPotF
	#[test]
	fn a_tip_is_not_painted_in_the_menu_colors() {
		let (bg, fg) = (crate::theme::MENU_BG_DEF, crate::theme::MENU_FG_DEF);
		assert_ne!(tip_bg_of(bg), bg);
		assert_ne!(tip_border_of(bg), bg);
		assert_ne!(tip_border_of(bg), tip_bg_of(bg));
		assert_ne!(tip_fg_of(fg), fg);
	}

	// A custom menu color carries the tip with it, which is the whole reason
	// these are shades rather than two more colors on the Themes tab. shade()
	// picks its direction from luminance, so a light menu color has to send the
	// tip the other way rather than off the top end.
	// Test ID: EqQPotG
	#[test]
	fn a_tip_follows_a_custom_menu_color_either_way() {
		let l = |c: [u8; 3]| crate::palette::to_oklab(c).0;
		let dark = [0x18, 0x1a, 0x20];
		let light = [0xe4, 0xe4, 0xe8];
		assert!(
			l(tip_bg_of(dark)) > l(dark),
			"a dark menu wants a lighter tip"
		);
		assert!(
			l(tip_bg_of(light)) < l(light),
			"a light menu wants a darker tip"
		);
	}

	// A Windows console writes the terminal's rights into the titles it sends
	// while elevated, and that copy is taken back off. Nothing on unix writes
	// one, so taking anything off there could only lose somebody's own text.
	// The one seam between the rights this process holds and the title that says
	// so. Without it the whole thing can be cut with nothing to notice. It only
	// bites when the suite is run holding those rights, which on unix needs no
	// privilege at all: `unshare -Ur cargo test`.
	// Test ID: EpPPU7c
	#[test]
	fn the_title_reports_the_rights_this_process_holds() {
		assert_eq!(rights().say, privilege_word());
		assert_eq!(rights().decorated, cfg!(windows));
	}

	// This used to abort before the window existed, which left the file that
	// caused it unfixable from the terminal it killed. Both halves cut a string
	// at a fixed byte offset: a hex color, and the `env:` in front of a variable
	// name.
	// Test ID: EpHOAM4
	#[test]
	fn a_config_value_cannot_abort_the_launch_on_a_byte_slice() {
		// six bytes, three characters
		assert_eq!(parse_hex("\u{20ac}abc"), None);
		assert_eq!(parse_hex("#\u{20ac}abc"), None);
		// still reads the ordinary ones
		assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
		assert_eq!(parse_hex("00ff00"), Some([0, 255, 0]));
		// a name whose fourth byte falls inside a character
		assert_eq!(strip_env_prefix("ab\u{20ac}cd"), "ab\u{20ac}cd");
		assert_eq!(strip_env_prefix("env:X"), "X");
		assert_eq!(strip_env_prefix("ENV:X"), "X", "case");
	}

	// The case that keeps coming up is a file manager's "Open in terminal": no
	// tty, but a directory that was very much chosen. Only the three directories
	// a launcher leaves us in by default may fall through to the setting.
	// Test ID: Eo6i2no
	#[test]
	fn an_inherited_directory_is_a_choice_unless_a_launcher_picked_it() {
		let home = PathBuf::from("/home/u");
		let exe_dir = PathBuf::from("/opt/silkterm");
		let choice = |cwd: &str| {
			dir_is_a_choice(Some(&PathBuf::from(cwd)), Some(&home), Some(&exe_dir), &[])
		};
		assert!(choice("/home/u/src/thing"), "a file manager's folder");
		assert!(!choice("/home/u"), "where a desktop icon starts");
		assert!(!choice("/opt/silkterm"), "double-clicked the executable");
		assert!(!choice("/"), "a launcher with no directory of its own");
		assert!(
			!dir_is_a_choice(None, Some(&home), Some(&exe_dir), &[]),
			"no directory at all is not a statement either"
		);
	}

	// An MSIX package's Start-menu entry has no working directory, so Windows
	// starts it in System32. That is where the launcher left us, not a folder
	// anybody picked, so the first shell goes to the setting instead.
	// Test ID: ErC7NYR
	#[test]
	fn a_start_in_the_windows_system_folder_is_not_a_choice() {
		let home = PathBuf::from("C:/Users/u");
		let exe_dir = PathBuf::from("C:/Program Files/SilkTerm");
		let system = [
			PathBuf::from("C:/Windows/System32"),
			PathBuf::from("C:/Windows/SysWOW64"),
		];
		let choice = |cwd: &str| {
			dir_is_a_choice(
				Some(&PathBuf::from(cwd)),
				Some(&home),
				Some(&exe_dir),
				&system,
			)
		};
		assert!(
			!choice("C:/Windows/System32"),
			"a packaged Start-menu launch"
		);
		assert!(
			!choice("C:/Windows/SysWOW64"),
			"the same for a 32-bit build"
		);
		assert!(
			choice("C:/Windows"),
			"the folder above is somewhere a person went"
		);
		assert!(choice("C:/Windows/System32/drivers"), "and so is one below");
		assert!(
			choice("C:/Users/u/src"),
			"an ordinary folder is still a choice"
		);
		assert!(
			system_dirs().is_empty() || cfg!(windows),
			"the system folder only counts on Windows"
		);
	}

	// Each platform keeps settings somewhere of its own, and the reason the
	// decision is a pure function of (layout, environment) is exactly this test:
	// it pins the macOS answer from a box that has no macOS, and the Windows one
	// from Linux. Reading cfg! at each use site instead would leave two thirds of
	// this unrunnable wherever it happens to be run.
	// Test ID: EnPU6SG
	#[test]
	fn each_platform_keeps_its_config_where_that_platform_keeps_settings() {
		let home = PathBuf::from("/home/u");
		let roaming = PathBuf::from("C:/Users/u/AppData/Roaming");
		assert_eq!(
			config_base_for(Layout::Windows, None, Some(&home), Some(&roaming)),
			Some(roaming.clone()),
			"Windows settings roam, so they belong in Roaming"
		);
		assert_eq!(
			config_base_for(Layout::MacOs, None, Some(&home), None),
			Some(home.join("Library").join("Application Support"))
		);
		assert_eq!(
			config_base_for(Layout::Xdg, None, Some(&home), None),
			Some(home.join(".config"))
		);
		// and no platform reaches for another's spelling
		assert_ne!(
			config_base_for(Layout::Windows, None, Some(&home), Some(&roaming)),
			Some(home.join(".config"))
		);
	}

	// Test ID: EnPU6SH
	#[test]
	fn an_explicit_xdg_config_home_wins_on_every_platform() {
		// Setting it is a deliberate act, so it outranks the platform default
		// everywhere - including the two platforms that have a native answer.
		let xdg = PathBuf::from("/elsewhere/cfg");
		let home = PathBuf::from("/home/u");
		let roaming = PathBuf::from("C:/Users/u/AppData/Roaming");
		for layout in [Layout::Windows, Layout::MacOs, Layout::Xdg] {
			assert_eq!(
				config_base_for(layout, Some(&xdg), Some(&home), Some(&roaming)),
				Some(xdg.clone()),
				"{layout:?} ignored an explicit XDG_CONFIG_HOME"
			);
		}
	}

	// Test ID: EnPU6SI
	#[test]
	fn a_missing_native_base_falls_back_rather_than_leaving_no_config() {
		// A stripped environment (a service, a bare shell) can carry no APPDATA.
		// Falling back to ~/.config is better than having nowhere to put settings.
		let home = PathBuf::from("/home/u");
		assert_eq!(
			config_base_for(Layout::Windows, None, Some(&home), None),
			Some(home.join(".config"))
		);
		// With nothing at all to go on there is genuinely no answer.
		assert_eq!(config_base_for(Layout::Windows, None, None, None), None);
		assert_eq!(config_base_for(Layout::Xdg, None, None, None), None);
	}

	// The whole point of the DIP pass: a chrome measurement is the same physical
	// size on any display, so it doubles when the scale factor does. The floor is
	// the other half - a hairline the author asked to be visible must never round
	// down to nothing, which is what a 1 DIP rule does on a display below 1x.
	// Test ID: EnLuU54
	#[test]
	fn a_chrome_measurement_scales_and_a_hairline_survives() {
		assert_eq!(dip(10.0, 1.0), 10.0);
		assert_eq!(dip(10.0, 2.0), 20.0);
		assert_eq!(dip(FOCUS_RING_PX, 2.0), FOCUS_RING_PX * 2.0);
		// fractional factors round to whole pixels so a rule stays crisp
		assert_eq!(dip(10.0, 1.5), 15.0);
		assert_eq!(dip(9.0, 1.25), 11.0);
		// a hairline holds at every factor, including down-scaled displays
		for scale in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0] {
			assert!(
				dip(PANE_GAP_PX, scale) >= 1.0,
				"the pane gap vanished at {scale}x"
			);
		}
		// zero stays zero - the floor is for measurements meant to be seen
		assert_eq!(dip(0.0, 2.0), 0.0);
	}

	fn shell_entry(slug: &str, command: &str) -> crate::shells::ShellEntry {
		crate::shells::ShellEntry {
			slug: slug.into(),
			title: slug.into(),
			command: command.into(),
			active: true,
			comment: String::new(),
			last_seen: String::new(),
		}
	}

	fn doc_with_shells(list: &[crate::shells::ShellEntry]) -> shcl::Document {
		let mut doc = shcl::Document::parse("");
		write_shells(&mut doc, &[], list);
		doc
	}

	// The scan stamps the date daily, so this fires on its own on the first launch
	// of each day. It used to drop the entry's subtree and put it back at the end
	// of the block, which loses its comments and moves it - and the first entry in
	// the file is the default shell.
	// Test ID: EpHPcsy
	#[test]
	fn a_daily_stamp_leaves_a_shell_where_it_is() {
		let text = "shells:\n\n\t## the one I use\n\tbash:\n\t\ttitle: \"bash\"\n\t\tcommand: \"/bin/bash\"\n\t\tactive: true\n\n\tzsh:\n\t\ttitle: \"zsh\"\n\t\tcommand: \"/bin/zsh\"\n\t\tactive: true\n";
		let mut doc = shcl::Document::parse(text);
		let stored = read_shells(&doc);
		assert_eq!(stored.len(), 2);
		let mut now = stored.clone();
		now[0].last_seen = "2026-09-08".to_string();

		write_shells(&mut doc, &stored, &now);

		assert_eq!(
			doc.children("shells"),
			vec!["bash", "zsh"],
			"the default shell is the first entry, so it may not move"
		);
		let out = doc.to_canonical();
		assert!(
			out.contains("## the one I use"),
			"its comment survives: {out:?}"
		);
		assert!(out.contains("2026-09-08"), "the stamp went in: {out:?}");
	}

	// File order IS the list's order - it decides what the menu offers first and
	// therefore which shell is the default - and a reorder changes no entry, so
	// the per-entry write path would have written nothing at all and lost it.
	// Test ID: EnQUIKm
	#[test]
	fn a_reorder_is_written_even_though_no_entry_changed() {
		let list = vec![
			shell_entry("bash", "/bin/bash"),
			shell_entry("fish", "/usr/bin/fish"),
			shell_entry("zsh", "/bin/zsh"),
		];
		let mut doc = doc_with_shells(&list);
		assert_eq!(doc.children("shells"), vec!["bash", "fish", "zsh"]);
		let moved = vec![list[2].clone(), list[0].clone(), list[1].clone()];
		write_shells(&mut doc, &list, &moved);
		assert_eq!(doc.children("shells"), vec!["zsh", "bash", "fish"]);
		// and every entry survived the rewrite whole
		let back = read_shells(&doc);
		assert_eq!(
			back.iter().map(|e| e.command.as_str()).collect::<Vec<_>>(),
			vec!["/bin/zsh", "/bin/bash", "/usr/bin/fish"]
		);
	}

	// A first launch writes the scan's list into a brand-new file, and the file's
	// order is the menu's and names the default shell.
	// Test ID: Er1q5Y8
	#[test]
	fn a_fresh_file_keeps_the_order_the_scan_found() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_freshshells_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, default_config()).unwrap();
		set_config_override(path.clone());

		let orig = load();
		assert!(orig.shells.is_empty(), "a fresh file has no list yet");
		let found = crate::shells::detect_every_known();
		let mut new = orig.clone();
		new.shells = crate::shells::merge(&orig.shells, &found);
		assert!(persist(&orig, &new));
		let titles = |list: &[crate::shells::ShellEntry]| -> Vec<String> {
			list.iter().map(|e| e.title.clone()).collect()
		};
		assert_eq!(
			titles(&load().shells),
			found.iter().map(|f| f.title.clone()).collect::<Vec<_>>()
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A test run with no override reads the box's own file and must leave it,
	// and its folder, exactly as they were: no launch refresh, conversion, backup
	// or rewrite, no save, no rating, no reset. A file in a folder of its own
	// stands in for the box's, so the real one is never at risk here.
	// Test ID: Erl5E2U
	#[test]
	fn a_test_run_never_writes_the_boxs_own_config() {
		struct PutBack(Option<PathBuf>);
		impl Drop for PutBack {
			fn drop(&mut self) {
				*crate::locks::lock(test_native_config()) = None;
				*crate::locks::lock(test_override()) = self.0.take();
			}
		}
		let _guard = super::test_config_lock();
		let _ = settings();
		let _put_back = PutBack(crate::locks::lock(test_override()).take());
		let stale = default_config().replace(
			"\t# idle_release: true  ## Default",
			"\t# idle_release: false  ## Default",
		);
		assert_ne!(stale, default_config(), "the template moved");
		let (first, rest) = default_config().split_once('\n').unwrap();
		let mut garbled = format!("{first}\n").into_bytes();
		garbled.extend(b"# caf\xe9\n");
		garbled.extend(rest.as_bytes());
		let cases: [(&str, Option<Vec<u8>>); 4] = [
			("stale commented default", Some(stale.into_bytes())),
			("shcl 2.x", Some(b"font:\n\tsize: 12\n".to_vec())),
			("not UTF-8", Some(garbled)),
			("no file", None),
		];
		for (n, (what, body)) in cases.into_iter().enumerate() {
			let home = crate::testdir::run_dir()
				.join(format!("silkterm_ownconfig_{}_{n}", std::process::id()));
			let _ = std::fs::remove_dir_all(&home);
			std::fs::create_dir_all(&home).unwrap();
			let dir = home.join(APP_DIR);
			let path = dir.join("config.shcl");
			if let Some(body) = &body {
				std::fs::create_dir_all(&dir).unwrap();
				std::fs::write(&path, body).unwrap();
			}
			*crate::locks::lock(test_native_config()) = Some(path.clone());
			assert_eq!(config_path().as_ref(), Some(&path), "{what}");

			let loaded = load();
			if let Some(body) = &body {
				assert!(
					std::fs::read(&path).unwrap() == *body,
					"{what}: the launch wrote it"
				);
			}
			let mut moved = loaded.clone();
			moved.font_size += 1.0;
			let _ = persist(&loaded, &moved);
			let _ = keep_rating(&RatingLines {
				profile: Some("low"),
				rated_hardware: Some("0123456789abcdef"),
				check_next_run: Some(false),
			});
			disable_keys(&["font.size"]);
			let _ = reset_config();

			let seen: Vec<String> = std::fs::read_dir(&dir)
				.map(|list| {
					list.flatten()
						.map(|entry| entry.file_name().to_string_lossy().into_owned())
						.collect()
				})
				.unwrap_or_default();
			match &body {
				Some(body) => {
					assert_eq!(seen, ["config.shcl"], "{what}");
					assert!(
						std::fs::read(&path).unwrap() == *body,
						"{what}: file changed"
					);
				}
				None => assert!(!dir.exists(), "{what}: {seen:?}"),
			}
			assert!(!may_write(&dir.join(".wallpaper-history")), "{what}");
			*crate::locks::lock(test_native_config()) = None;
			let _ = std::fs::remove_dir_all(&home);
		}
		// asked only, never written
		if let Some(real) = env_config_path() {
			assert!(!may_write(&real));
			assert!(!may_write(&real.with_file_name(".wallpaper-history")));
		}
	}

	// Two windows, and the second one loaded before the first one's scan saved a
	// new find. Its own scan finds the same program and saves. It used to take out
	// only the entries it had loaded and rewrite them, so the find it never
	// loaded stayed above them all and a REPL became the default shell.
	// Test ID: Er1q5Y9
	#[test]
	fn a_stale_window_cannot_put_another_windows_find_on_top() {
		let loaded = vec![
			shell_entry("bash", "/bin/bash"),
			shell_entry("nushell", "/bin/nu"),
			shell_entry("zsh", "/bin/zsh"),
		];
		let mut on_disk = loaded.clone();
		on_disk.push(shell_entry("node_js_2", "/nvm/bin/node"));
		let mut doc = parse_kept(&saved_text(&doc_with_shells(&on_disk)));

		write_shells(&mut doc, &loaded, &on_disk);

		assert_eq!(
			doc.children("shells"),
			vec!["bash", "nushell", "zsh", "node_js_2"]
		);
	}

	// The same window with an older list, doing something else to it. Every save
	// keeps what the other window did, unless this window changed that entry.
	// Test ID: Er1q5YA
	#[test]
	fn a_stale_window_keeps_what_another_window_saved() {
		let loaded = vec![
			shell_entry("bash", "/bin/bash"),
			shell_entry("nushell", "/bin/nu"),
			shell_entry("python_3", "/bin/python3"),
			shell_entry("zsh", "/bin/zsh"),
		];
		let slugs = |doc: &shcl::Document| doc.children("shells");

		// another window switched Zsh off and added Fish; this one switches
		// Python off
		let mut theirs = loaded.clone();
		theirs[3].active = false;
		theirs.push(shell_entry("fish", "/bin/fish"));
		let mut doc = doc_with_shells(&theirs);
		let mut mine = loaded.clone();
		mine[2].active = false;
		write_shells(&mut doc, &loaded, &mine);
		assert_eq!(
			slugs(&doc),
			vec!["bash", "nushell", "python_3", "zsh", "fish"]
		);
		let back = read_shells(&doc);
		assert!(!back[2].active, "this window's change went in");
		assert!(!back[3].active, "the other window's change stayed");

		// another window removed Nushell; this one stamps the date on everything
		let mut theirs = loaded.clone();
		theirs.remove(1);
		let mut doc = doc_with_shells(&theirs);
		let mut mine = loaded.clone();
		for entry in &mut mine {
			entry.last_seen = "2026-09-27".into();
		}
		write_shells(&mut doc, &loaded, &mine);
		assert_eq!(
			slugs(&doc),
			vec!["bash", "python_3", "zsh"],
			"it stays removed"
		);

		// another window moved Zsh to the top; this one only switches Python off
		let theirs = vec![
			loaded[3].clone(),
			loaded[0].clone(),
			loaded[1].clone(),
			loaded[2].clone(),
		];
		let mut doc = doc_with_shells(&theirs);
		let mut mine = loaded.clone();
		mine[2].active = false;
		write_shells(&mut doc, &loaded, &mine);
		assert_eq!(slugs(&doc), vec!["zsh", "bash", "nushell", "python_3"]);

		// but a move made in this window is written, with the other window's
		// new entry after it
		let mut theirs = loaded.clone();
		theirs.push(shell_entry("fish", "/bin/fish"));
		let mut doc = doc_with_shells(&theirs);
		let mine = vec![
			loaded[1].clone(),
			loaded[0].clone(),
			loaded[2].clone(),
			loaded[3].clone(),
		];
		write_shells(&mut doc, &loaded, &mine);
		assert_eq!(
			slugs(&doc),
			vec!["nushell", "bash", "python_3", "zsh", "fish"]
		);
	}

	// Test ID: EnQUIKn
	#[test]
	fn the_date_a_shell_was_last_seen_round_trips() {
		let mut entry = shell_entry("bash", "/bin/bash");
		entry.last_seen = "2026-08-19".into();
		let doc = doc_with_shells(std::slice::from_ref(&entry));
		assert_eq!(read_shells(&doc)[0].last_seen, "2026-08-19");
		// a value that is not a date is not written, and reads as never seen
		let mut junk = shell_entry("fish", "/usr/bin/fish");
		junk.last_seen = "whenever".into();
		let doc = doc_with_shells(std::slice::from_ref(&junk));
		assert!(read_shells(&doc)[0].last_seen.is_empty());
	}

	// The list names the default now (its top active entry), so an old
	// `shell.default` has to become the top of the list rather than be dropped.
	// Test ID: EnQUIKo
	#[test]
	fn an_old_default_shell_becomes_the_top_of_the_list() {
		let list = vec![
			shell_entry("bash", "/bin/bash"),
			shell_entry("fish", "/usr/bin/fish"),
		];
		// one the list already carries simply moves
		let moved = adopt_default_into(&list, "/usr/bin/fish");
		assert_eq!(
			moved.iter().map(|e| e.slug.as_str()).collect::<Vec<_>>(),
			vec!["fish", "bash"]
		);
		// one it does not know is still the shell they chose, so it is added there
		let added = adopt_default_into(&list, "/opt/ion --login");
		assert_eq!(added.len(), 3);
		assert_eq!(added[0].command, "/opt/ion --login");
		assert!(added[0].active);
		assert_eq!(added[1].slug, "bash", "and nothing else moved");
	}

	// The migration drops `shell.default` only once its adoption into the list is
	// saved. A line shcl cannot read is written back as it was, so the save goes
	// through beside one. Where the save cannot keep the lines it would delete the
	// line instead, so it is refused and the choice has to wait.
	// Test ID: EpyvpeC
	#[test]
	fn an_old_default_shell_waits_for_a_save_that_can_happen() {
		let _guard = super::test_config_lock();
		let dir = crate::testdir::run_dir()
			.join(format!("silkterm_default_shell_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let mut doc = shcl::Document::parse(default_config());
		write_shells(
			&mut doc,
			&[],
			&[
				shell_entry("bash", "/bin/bash"),
				shell_entry("zsh", "/bin/zsh"),
			],
		);
		doc.put_string("shell.default", "/bin/zsh");
		let clean = doc.to_canonical();
		let order = |text: &str| -> Vec<String> {
			read_shells(&shcl::Document::parse(text))
				.into_iter()
				.map(|e| e.slug)
				.collect()
		};
		let default_of = |text: &str| shcl::Document::parse(text).get_string("shell.default").ok();

		std::fs::write(&path, format!("{clean}mm:\n\t\tnn: 1\n\too: 2\n")).unwrap();
		adopt_default_shell(&path);
		migrate_config(&path);
		let beside = std::fs::read_to_string(&path).unwrap();
		assert_eq!(default_of(&beside), None, "{beside}");
		assert_eq!(order(&beside), ["zsh", "bash"]);
		assert!(beside.contains("mm:\n\t\tnn: 1\n\too: 2\n"), "{beside}");

		// a second shell block sends the save back to the canonical form
		std::fs::write(&path, format!("{clean}shell:\n\t\tnn: 1\n\too: 2\n")).unwrap();
		adopt_default_shell(&path);
		migrate_config(&path);
		let refused = std::fs::read_to_string(&path).unwrap();
		assert_eq!(
			default_of(&refused).as_deref(),
			Some("/bin/zsh"),
			"{refused}"
		);
		assert_eq!(order(&refused), ["bash", "zsh"]);

		std::fs::write(&path, &clean).unwrap();
		adopt_default_shell(&path);
		migrate_config(&path);
		let saved = std::fs::read_to_string(&path).unwrap();
		assert_eq!(default_of(&saved), None, "{saved}");
		assert_eq!(order(&saved), ["zsh", "bash"]);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// ':' must NOT be a word separator, else a double-click on C:\... drops the
	// drive prefix (the alacritty default splits on ':'). Regression guard.
	// Test ID: EkVfEUC
	#[test]
	fn default_word_separators_keep_drive_colon() {
		let d = Settings::default();
		assert!(
			!d.word_separators.contains(':'),
			"':' should stay a word char so drive paths select whole"
		);
		// still a real separator set (space + comma remain delimiters)
		assert!(d.word_separators.contains(' '));
		assert!(d.word_separators.contains(','));
	}

	// Word separators are read from the file, not only defaulted.
	// Test ID: Er2UFeR
	#[test]
	fn word_separators_come_from_the_file() {
		let s = resolve(
			read_raw(
				"selection:\n\tword_separators: \" ,;\"\n",
				std::path::Path::new("test.shcl"),
			)
			.0,
		);
		assert_eq!(s.word_separators, " ,;");
	}

	// Pairs are two characters each, in precedence order; a lone character is
	// not a pair and is dropped.
	// Test ID: Er2UFeS
	#[test]
	fn selection_pairs_parse_in_order_and_come_from_the_file() {
		assert_eq!(
			parse_pairs(DEFAULT_SELECTION_PAIRS),
			[
				('`', '`'),
				('"', '"'),
				('\'', '\''),
				('{', '}'),
				('(', ')'),
				('[', ']'),
				('<', '>'),
			]
		);
		assert_eq!(parse_pairs("() ab x"), [('(', ')'), ('a', 'b')]);
		let s = resolve(
			read_raw(
				"selection:\n\tpairs: \"()\"\n",
				std::path::Path::new("test.shcl"),
			)
			.0,
		);
		assert_eq!(s.selection_pairs, "()");
	}

	// A bare-decimal float (`.1`, missing leading zero) must not stop persist from
	// saving. Regressed once under TOML, where persist strict-parsed the raw file,
	// bailed on `.1`, and silently dropped every dialog change (relaunch reverted).
	// `.1` is simply a valid float now, and the writer never rewrites a scalar, so
	// the value is also left exactly as the user typed it.
	// A profile's values ride on top of the live settings, and a write that
	// starts from a live copy (the minimap toggle, the remembered size) must
	// not carry them into the file - or Custom would have nothing to put back.
	// Test ID: EorkTk0
	#[test]
	fn the_file_keeps_the_users_values_under_a_profile() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfgprof_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(
			&path,
			"scroll:\n\tease_in_ms: 300.0\nperformance:\n\tprofile: \"low\"\n",
		)
		.unwrap();
		set_config_override(path.clone());

		let stored = load();
		assert_eq!(stored.performance_profile, "low");
		assert_eq!(
			stored.scroll_ease_in_ms, 300.0,
			"the file is read as written"
		);
		let mut live = stored.clone();
		crate::profile::apply(&mut live);
		assert!(!live.text_scrim, "Low drops the halo");
		assert_ne!(live.scroll_ease_in_ms, 300.0);

		let mut new = live.clone();
		new.minimap = !new.minimap;
		assert!(persist(&live, &new));
		let back = load();
		assert_eq!(back.minimap, new.minimap, "the change itself is written");
		assert_eq!(back.scroll_ease_in_ms, 300.0, "the profile's value was not");
		assert!(back.wallpaper_enabled, "nor its wallpaper switch");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A text color the wallpaper picked is session state, the same as a display
	// step. Written down, every rotation would rewrite the file, and the colors
	// would outlive the picture they came from.
	// Test ID: EqRxesl
	#[test]
	fn a_text_colour_taken_from_the_wallpaper_never_reaches_the_file() {
		let mine = ([0x12u8, 0x34, 0x56], [0x65u8, 0x43, 0x21]);
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_cfgwp_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(&path, "colors:\n\tfrom_wallpaper: true\n").unwrap();
		set_config_override(path.clone());

		let mut stored = load();
		assert!(stored.colors_from_wallpaper, "the file is read as written");
		stored.performance_profile = "custom".to_string();
		stored.performance_automatic = false;
		stored.wallpaper_enabled = true;
		stored.fg = mine.0;
		stored.cursor = mine.1;
		stored.wallpaper_summary = Some(crate::autotheme::Summary {
			luma_hi: 0.3,
			luma_lo: 0.02,
			luma_mean: 0.13,
			alpha: 1.0,
			hue: 250.0,
			chroma: 0.08,
			opacity: 0.35,
		});
		let mut live = stored.clone();
		crate::autotheme::apply(&mut live);
		assert_ne!(live.fg, mine.0, "the derived text colour is what is drawn");

		// A save that changes something else must not take the derived pair with it.
		// The two sides have to differ for the diff to see it at all: the file's
		// own colours on one, and the live copy wearing the derived pair on the
		// other, which is what a save outside the dialog hands over.
		let mut new = live.clone();
		new.minimap = !new.minimap;
		assert!(persist(&stored, &new));
		let text = std::fs::read_to_string(&path).unwrap();
		assert!(
			!text.contains("\n\tforeground:") && !text.contains("\n\tcursor:"),
			"the derived colours were written:\n{text}"
		);

		// the user's own colour still saves while the switch is on, since the row
		// is only grayed - the value under it is still theirs to change by hand
		let mut edited = live.clone();
		crate::autotheme::unapply(&mut edited);
		edited.fg = [0x0au8, 0x0b, 0x0c];
		let mut base = live.clone();
		crate::autotheme::unapply(&mut base);
		assert!(persist(&base, &edited));
		assert_eq!(load().fg, [0x0a, 0x0b, 0x0c]);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A step the display watch took lasts the session. Written down, one stall
	// became every later launch's profile, with no way back while automatic was
	// on - that took a desktop to Standard terminal and its wallpaper with it.
	// Test ID: EpWow4f
	#[test]
	fn a_session_step_down_never_reaches_the_file() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfgstep_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(
			&path,
			"performance:\n\tautomatic: true\n\tprofile: \"max\"\n",
		)
		.unwrap();
		set_config_override(path.clone());

		let mut live = load();
		crate::profile::apply(&mut live);
		let before = std::fs::read_to_string(&path).unwrap();
		let mut stepped = live.clone();
		stepped.stepped_profile = Some(crate::profile::Profile::Low);
		crate::profile::apply(&mut stepped);
		assert!(!stepped.text_scrim, "the step is in force live");
		assert!(persist(&live, &stepped));
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			before,
			"nothing about the step is written"
		);

		let mut changed = stepped.clone();
		changed.minimap = !changed.minimap;
		assert!(persist(&stepped, &changed));
		let after = std::fs::read_to_string(&path).unwrap();
		let new_lines: Vec<&str> = after.lines().filter(|l| !before.contains(l)).collect();
		assert!(!new_lines.is_empty(), "the unrelated change is written");
		assert!(
			new_lines.iter().all(|l| !l.contains("profile")),
			"and nothing about the profile: {new_lines:?}"
		);
		let back = load();
		assert_eq!(back.minimap, changed.minimap);
		assert_eq!(back.performance_profile, "max");
		assert!(back.stepped_profile.is_none());
		assert!(back.wallpaper_enabled && back.text_scrim);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// "Reload config" re-reads the file, which never holds the session's own
	// state, so a reload must not lift a remote profile or a watch step.
	// Test ID: EpWow4g
	#[test]
	fn a_reload_keeps_what_the_file_never_held() {
		let live = Settings {
			remote_override: true,
			stepped_profile: Some(crate::profile::Profile::High),
			..Settings::default()
		};
		let mut reloaded = Settings::default();
		keep_session(&live, &mut reloaded, false);
		assert!(reloaded.remote_override);
		assert_eq!(
			reloaded.stepped_profile,
			Some(crate::profile::Profile::High)
		);
	}

	// An Apply from a dialog opened earlier keeps a remote switch or a watch step
	// taken since, and the dialog's own pick, revert or automatic switch still
	// lifts them.
	// Test ID: EpX7AvY
	#[test]
	fn an_apply_keeps_the_session_state_the_dialog_did_not_touch() {
		use crate::profile::Profile;
		let apply = |live: &Settings, opened: &Settings, edited: &Settings| {
			let mut out = edited.clone();
			keep_session_on_apply(live, opened, &mut out);
			(out.stepped_profile, out.remote_override)
		};
		let base = Settings {
			performance_automatic: true,
			performance_profile: "max".to_string(),
			..Settings::default()
		};
		let stepped = Settings {
			stepped_profile: Some(Profile::Low),
			..base.clone()
		};
		let remote = Settings {
			remote_override: true,
			..base.clone()
		};
		let other_row = Settings {
			minimap: !base.minimap,
			..base.clone()
		};
		let picked = Settings {
			performance_profile: "high".to_string(),
			..base.clone()
		};
		let manual = Settings {
			performance_automatic: false,
			..base.clone()
		};

		// taken after the dialog opened: an Apply of an unrelated row keeps it
		assert_eq!(apply(&stepped, &base, &base), (Some(Profile::Low), false));
		assert_eq!(
			apply(&stepped, &base, &other_row),
			(Some(Profile::Low), false)
		);
		assert_eq!(apply(&remote, &base, &base), (None, true));
		assert_eq!(apply(&remote, &base, &other_row), (None, true));
		// switched off since: it stays off
		assert_eq!(apply(&base, &remote, &remote), (None, false));
		assert_eq!(apply(&base, &stepped, &stepped), (None, false));
		// the dialog lifted what it opened with (a pick, a revert, the switch)
		assert_eq!(apply(&stepped, &stepped, &base), (None, false));
		assert_eq!(apply(&remote, &remote, &base), (None, false));
		// a pick or the automatic switch over a step the dialog never saw
		assert_eq!(apply(&stepped, &base, &picked), (None, false));
		assert_eq!(apply(&stepped, &base, &manual), (None, false));
		// a pick lowers a Remote the dialog never saw; the switch does not
		assert_eq!(apply(&remote, &base, &picked), (None, false));
		assert_eq!(apply(&remote, &base, &manual), (None, true));
		// Remote picked in the dialog
		assert_eq!(apply(&base, &base, &remote), (None, true));
		// and over a step taken since the dialog opened, which the pick lifts
		assert_eq!(apply(&stepped, &base, &remote), (None, true));
	}

	// `silkterm --wallpaper PATH` set the image and left the switch off, so it
	// said ok and showed nothing. Naming one turns the switch on, the way
	// `--wallpaper-file` does at launch, and a profile that turns the wallpaper
	// off still wins for both.
	// Test ID: Eq4Yrbf
	#[test]
	fn naming_a_wallpaper_turns_it_on_unless_the_profile_says_off() {
		let off = Settings {
			wallpaper_enabled: false,
			performance_profile: "custom".into(),
			..Settings::default()
		};
		let mut named = off.clone();
		name_wallpaper(&mut named, Some("/x.png".into()));
		assert!(named.wallpaper_enabled);
		assert_eq!(named.wallpaper_raw, "/x.png");
		// a clear names nothing and leaves the switch alone
		let mut cleared = off.clone();
		name_wallpaper(&mut cleared, None);
		assert!(!cleared.wallpaper_enabled && cleared.wallpaper_raw.is_empty());
		let mut remote = named.clone();
		remote.remote_override = true;
		crate::profile::apply(&mut remote);
		assert!(!remote.wallpaper_enabled, "the Remote profile keeps it off");
	}

	// A wallpaper given on the command line lasts the session: a reload keeps it
	// over the file, and an Apply keeps it unless the dialog picked another.
	// Test ID: Epytxce
	#[test]
	fn a_command_line_wallpaper_outlasts_a_reload_and_an_apply() {
		let with = |raw: &str| Settings {
			wallpaper_raw: raw.to_string(),
			wallpaper: (!raw.is_empty()).then(|| std::path::PathBuf::from(raw)),
			..Settings::default()
		};
		let live = with("/cli.png");
		let mut reloaded = with("/file.png");
		keep_session(&live, &mut reloaded, true);
		assert_eq!(reloaded.wallpaper, live.wallpaper);
		let mut reloaded = with("/file.png");
		keep_session(&live, &mut reloaded, false);
		assert_eq!(
			reloaded.wallpaper_raw, "/file.png",
			"no lock, the file wins"
		);
		// a file with the wallpaper off does not hide one the session named
		let mut named = Settings::default();
		name_wallpaper(&mut named, Some("/cli.png".into()));
		let mut reloaded = Settings {
			wallpaper_enabled: false,
			..with("/file.png")
		};
		keep_session(&named, &mut reloaded, true);
		assert!(reloaded.wallpaper_enabled);
		// an explicit clear on the command line is kept too
		let mut reloaded = with("/file.png");
		keep_session(&with(""), &mut reloaded, true);
		assert!(reloaded.wallpaper.is_none() && reloaded.wallpaper_raw.is_empty());

		// a dialog opened before the lock, applied without touching the wallpaper
		let (mut opened, mut edited) = (with("/old.png"), with("/old.png"));
		keep_wallpaper_on_apply(&live, true, &mut opened, &mut edited);
		assert_eq!(edited.wallpaper, live.wallpaper);
		assert_eq!(
			(&opened.wallpaper_raw, &opened.wallpaper),
			(&edited.wallpaper_raw, &edited.wallpaper),
			"both sides match, so the save writes nothing about it"
		);
		// the dialog picked one itself
		let (mut opened, mut edited) = (with("/old.png"), with("/picked.png"));
		keep_wallpaper_on_apply(&live, true, &mut opened, &mut edited);
		assert_eq!(edited.wallpaper_raw, "/picked.png");
		assert_eq!(opened.wallpaper_raw, "/old.png");
		// no lock
		let (mut opened, mut edited) = (with("/old.png"), with("/old.png"));
		keep_wallpaper_on_apply(&live, false, &mut opened, &mut edited);
		assert_eq!(edited.wallpaper_raw, "/old.png");
	}

	// Every launch-time rewrite used to truncate the file before writing it, so a
	// crash or a full disk during one left nothing where the config was.
	// Test ID: EpHaZLU
	#[test]
	fn a_launch_time_rewrite_never_truncates_the_config() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfgatomic_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font.size: 12.0\n").expect("write");

		write_config_atomic(&path, "font.size: 13.0\n").expect("rewrite");
		assert_eq!(std::fs::read_to_string(&path).unwrap(), "font.size: 13.0\n");
		assert!(
			!dir.join("config.shcl.new").exists(),
			"no half-written file left beside it"
		);

		// nothing in the module writes the config any other way
		let body = include_str!("config.rs")
			.split("\nmod tests {")
			.next()
			.expect("the file above its own tests");
		let raw: Vec<&str> = body
			.lines()
			.filter(|l| l.contains("fs::write(") && l.contains("path"))
			.map(str::trim)
			.collect();
		assert!(
			raw.is_empty(),
			"these truncate the config before writing it: {raw:?}"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// On Windows nothing said a file with a line it cannot read could no longer
	// be saved. Only the window can say so, so a refused write leaves word of it.
	// Test ID: EqGnMOv
	#[test]
	fn a_refused_save_leaves_word_for_the_window() {
		let _guard = super::test_config_lock();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_refused_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let doc = shcl::Document::parse("window:\n\t\tmargin: 4\n\tstray: 1\n");
		assert_eq!(doc.lost_count(), 1);
		// whatever another test refused before this one
		let _ = take_refusal();
		assert!(!write_doc(&path, &doc));
		assert_eq!(
			take_refusal(),
			Some(Refusal {
				path: path.clone(),
				lines: vec![3],
				lost: 1,
			})
		);
		assert_eq!(take_refusal(), None, "taken once");
		assert!(write_doc(
			&path,
			&shcl::Document::parse("font:\n\tsize: 12.0\n")
		));
		assert_eq!(take_refusal(), None, "a save that went through");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A failed save used to present to the dialog as a clean one, so it closed as
	// if it had written. shcl refusing a lossy round trip is the case that makes
	// this permanent.
	// Test ID: EpHOR0K
	#[test]
	fn a_save_that_failed_is_not_reported_as_a_save() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfgfail_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let doc = shcl::Document::parse("font.size: 12.0\n");
		// a directory that is not there is the cheapest unwritable path
		let missing = dir.join("no-such-dir").join("config.shcl");
		assert!(
			!write_doc(&missing, &doc),
			"an unwritable path is not a save"
		);
		assert!(
			write_doc(&dir.join("config.shcl"), &doc),
			"and a real one is"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Stands in for shcl's publish, which on Windows can take the old file off its
	// name and then fail (1176, 1177).
	type RestorePublish = fn(&str, &str) -> Result<(), String>;

	fn restore_test_dir(what: &str) -> std::path::PathBuf {
		let dir = crate::testdir::run_dir().join(format!("silkterm_{what}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		dir
	}

	fn names_in_dir(dir: &std::path::Path) -> Vec<String> {
		let mut names: Vec<String> = std::fs::read_dir(dir)
			.unwrap()
			.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}

	// shcl deletes its temp copy after a replace that took the old file fails, so
	// the settings are written at the empty name instead of lost, keeping the
	// file's mode and leaving nothing beside it.
	// Test ID: Epa8S0e
	#[test]
	fn a_write_that_took_the_file_writes_it_again() {
		let took: RestorePublish = |file, _| {
			std::fs::remove_file(std::fs::canonicalize(file).unwrap()).unwrap();
			Err(format!(
				"{file}: The replacement file could not be renamed. (os error 1176)"
			))
		};
		let dir = restore_test_dir("restore");
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font:\n\tsize: 12\n").unwrap();
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
		}
		let new = "font:\n\tsize: 13\n";

		let result = write_config_atomic_with(&path, new, took);

		let text = std::fs::read_to_string(&path).ok();
		let names = names_in_dir(&dir);
		#[cfg(unix)]
		let mode = std::fs::metadata(&path)
			.ok()
			.map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777);
		let _ = std::fs::remove_dir_all(&dir);
		assert_eq!(result, Ok(()), "the write is reported as done");
		assert_eq!(text.as_deref(), Some(new), "the new text is at the name");
		assert_eq!(names, ["config.shcl"], "no temp file and no backup");
		#[cfg(unix)]
		assert_eq!(mode, Some(0o640), "the file keeps its mode");
	}

	// The real file is found before the write, since a link to a file the replace
	// took no longer resolves. The link stays and the real file keeps its mode.
	// Test ID: Epa8S0f
	#[cfg(unix)]
	#[test]
	fn a_restore_keeps_a_linked_private_settings_file() {
		use std::os::unix::fs::PermissionsExt;
		let took: RestorePublish = |file, _| {
			std::fs::remove_file(std::fs::canonicalize(file).unwrap()).unwrap();
			Err("replaced file gone".to_string())
		};
		let dir = restore_test_dir("restorelink");
		let real = dir.join("real.shcl");
		std::fs::write(&real, "font:\n\tsize: 12\n").unwrap();
		std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
		let link = dir.join("config.shcl");
		std::os::unix::fs::symlink(&real, &link).unwrap();
		let new = "font:\n\tsize: 13\n";

		let result = write_config_atomic_with(&link, new, took);

		let still_link = std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink());
		let target = std::fs::read_link(&link).ok();
		let text = std::fs::read_to_string(&real).ok();
		let mode = std::fs::metadata(&real)
			.ok()
			.map(|m| m.permissions().mode() & 0o777);
		let names = names_in_dir(&dir);
		let _ = std::fs::remove_dir_all(&dir);
		assert_eq!(result, Ok(()), "the write is reported as done");
		assert!(still_link, "still a link");
		assert_eq!(target, Some(real), "to the same file");
		assert_eq!(
			text.as_deref(),
			Some(new),
			"the real file holds the new text"
		);
		assert_eq!(mode, Some(0o600), "and stays private");
		assert_eq!(names, ["config.shcl", "real.shcl"], "nothing beside them");
	}

	// The ordinary failure (read-only, held open) leaves the old file in place, so
	// nothing is written over it and nothing waits.
	// Test ID: Epa8S0g
	#[test]
	fn a_failed_write_that_left_the_file_changes_nothing() {
		let refuse: RestorePublish = |_, _| Err("refused".to_string());
		let dir = restore_test_dir("restorekeep");
		let path = dir.join("config.shcl");
		let old = "font:\n\tsize: 12\n";
		std::fs::write(&path, old).unwrap();

		let started = std::time::Instant::now();
		let result = write_config_atomic_with(&path, "font:\n\tsize: 13\n", refuse);
		let elapsed = started.elapsed();

		let bytes = std::fs::read(&path).ok();
		let _ = std::fs::remove_dir_all(&dir);
		assert_eq!(
			result,
			Err("refused".to_string()),
			"the publish's own error"
		);
		assert_eq!(bytes.as_deref(), Some(old.as_bytes()), "the file as it was");
		assert!(
			elapsed < std::time::Duration::from_millis(50),
			"no pause when the file is still there: {elapsed:?}"
		);
	}

	// Something can take the empty name before the restore does. A link left
	// there is never written through, even one whose target does not exist.
	// Test ID: Epa8S0h
	#[cfg(unix)]
	#[test]
	fn a_restore_never_writes_through_a_name_taken_meanwhile() {
		let planted: RestorePublish = |file, _| {
			std::fs::remove_file(file).unwrap();
			let victim = std::path::Path::new(file).with_file_name("victim");
			std::os::unix::fs::symlink(victim, file).unwrap();
			Err("replaced file gone".to_string())
		};
		let dir = restore_test_dir("restoreplant");
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font:\n\tsize: 12\n").unwrap();

		let result = write_config_atomic_with(&path, "font:\n\tsize: 13\n", planted);

		let victim = std::fs::symlink_metadata(dir.join("victim")).is_ok();
		let link = std::fs::read_link(&path).ok();
		let _ = std::fs::remove_dir_all(&dir);
		assert_eq!(
			result,
			Err("replaced file gone".to_string()),
			"the publish's own error"
		);
		assert!(!victim, "nothing written through the link");
		assert_eq!(link, Some(dir.join("victim")), "the link left as it was");
	}

	// A restore that cannot write stops within its bound and says the file is gone,
	// rather than retrying forever or reporting the replace's error alone.
	// Test ID: Epa8S0i
	#[cfg(unix)]
	#[test]
	fn a_restore_that_cannot_write_gives_up_and_says_so() {
		use std::os::unix::fs::PermissionsExt;
		let locked: RestorePublish = |file, _| {
			let path = std::path::Path::new(file);
			std::fs::remove_file(path).unwrap();
			let folder = path.parent().unwrap();
			std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o500)).unwrap();
			Err("replaced file gone".to_string())
		};
		let dir = restore_test_dir("restorefail");
		let unlock = || {
			let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
		};
		// a run with the rights to write there anyway has nothing to test
		std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
		let writable = std::fs::File::create(dir.join("probe")).is_ok();
		unlock();
		if writable {
			let _ = std::fs::remove_dir_all(&dir);
			return;
		}
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font:\n\tsize: 12\n").unwrap();

		let started = std::time::Instant::now();
		let result = write_config_atomic_with(&path, "font:\n\tsize: 13\n", locked);
		let elapsed = started.elapsed();

		unlock();
		let left = std::fs::symlink_metadata(&path).is_ok();
		let _ = std::fs::remove_dir_all(&dir);
		let err = result.expect_err("a restore that could not write is not a save");
		assert!(
			err.contains("replaced file gone") && err.contains("is gone"),
			"names the replace's error and that the file is gone: {err}"
		);
		assert!(
			elapsed >= std::time::Duration::from_millis(400)
				&& elapsed < std::time::Duration::from_secs(2),
			"five tries, a tenth of a second apart: {elapsed:?}"
		);
		assert!(!left, "nothing at the name");
	}

	// Leaves the name waiting for its delete while a reader holds the file, and
	// lets the reader go after `hold`. A plain delete frees the name at once
	// where the volume deletes POSIX-style, so this closes a delete-on-close
	// handle instead.
	#[cfg(windows)]
	fn delete_while_held(real: &std::path::Path, hold: std::time::Duration) {
		use std::os::windows::fs::OpenOptionsExt;
		use windows_sys::Win32::Storage::FileSystem::{
			DELETE, FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
		};
		let share_all = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
		let holder = std::fs::OpenOptions::new()
			.read(true)
			.share_mode(share_all)
			.open(real)
			.unwrap();
		let deleter = std::fs::OpenOptions::new()
			.access_mode(DELETE)
			.share_mode(share_all)
			.custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
			.open(real)
			.unwrap();
		drop(deleter);
		let listed = std::fs::symlink_metadata(real).is_ok();
		let refused = match std::fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(real)
		{
			Ok(file) => {
				drop(file);
				let _ = std::fs::remove_file(real);
				false
			}
			Err(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
		};
		// a pass for the wrong reason is worse than a failure
		assert!(
			listed && refused,
			"the name was freed at once, so this cannot see a pending delete"
		);
		std::thread::spawn(move || {
			std::thread::sleep(hold);
			drop(holder);
		});
	}

	// so the cleanup never meets a name still being deleted
	#[cfg(windows)]
	fn wait_until_gone(path: &std::path::Path) {
		let started = std::time::Instant::now();
		while std::fs::symlink_metadata(path).is_ok()
			&& started.elapsed() < std::time::Duration::from_secs(3)
		{
			std::thread::sleep(std::time::Duration::from_millis(10));
		}
	}

	// Error 5 alone cannot tell a pending delete from a file this process may not
	// open, so every look-alike is checked beside the real one.
	// Test ID: EpaOYRs
	#[cfg(windows)]
	#[test]
	fn only_a_delete_in_progress_reads_as_pending() {
		use std::os::windows::fs::OpenOptionsExt;
		let dir = std::fs::canonicalize(restore_test_dir("pendcheck")).unwrap();

		let missing = delete_pending(&dir.join("missing.shcl"));

		let plain_path = dir.join("plain.shcl");
		std::fs::write(&plain_path, "a").unwrap();
		let plain = delete_pending(&plain_path);

		let readonly_path = dir.join("readonly.shcl");
		std::fs::write(&readonly_path, "a").unwrap();
		let mut perms = std::fs::metadata(&readonly_path).unwrap().permissions();
		perms.set_readonly(true);
		std::fs::set_permissions(&readonly_path, perms.clone()).unwrap();
		let readonly = delete_pending(&readonly_path);
		// only the read-only flag on Windows, cleared so the cleanup can remove it
		#[allow(clippy::permissions_set_readonly_false)]
		perms.set_readonly(false);
		let _ = std::fs::set_permissions(&readonly_path, perms);

		let held_path = dir.join("held.shcl");
		std::fs::write(&held_path, "a").unwrap();
		let held_file = std::fs::OpenOptions::new()
			.read(true)
			.share_mode(0)
			.open(&held_path)
			.unwrap();
		let held = delete_pending(&held_path);
		drop(held_file);

		let folder_path = dir.join("folder");
		std::fs::create_dir(&folder_path).unwrap();
		let folder = delete_pending(&folder_path);

		let pending_path = dir.join("config.shcl");
		std::fs::write(&pending_path, "a").unwrap();
		delete_while_held(&pending_path, std::time::Duration::from_millis(300));
		let pending = delete_pending(&pending_path);
		wait_until_gone(&pending_path);
		let freed = delete_pending(&pending_path);
		let gone = std::fs::symlink_metadata(&pending_path).is_err();

		let _ = std::fs::remove_dir_all(&dir);
		assert_eq!(
			[
				("missing", missing),
				("plain", plain),
				("read-only", readonly),
				("held without sharing", held),
				("folder", folder),
				("pending", pending),
				("freed", freed),
			],
			[
				("missing", false),
				("plain", false),
				("read-only", false),
				("held without sharing", false),
				("folder", false),
				("pending", true),
				("freed", false),
			]
		);
		assert!(gone, "the name is gone once the reader lets go");
	}

	// A replace that fails can leave the old name waiting on a scanner's handle.
	// The restore waits that out within its bound and writes the text there.
	// Test ID: EpaOYRt
	#[cfg(windows)]
	#[test]
	fn a_restore_waits_for_a_name_still_being_deleted() {
		let pending: RestorePublish = |file, _| {
			delete_while_held(
				&std::fs::canonicalize(file).unwrap(),
				std::time::Duration::from_millis(250),
			);
			Err(format!(
				"{file}: The replacement file could not be renamed. (os error 1176)"
			))
		};
		let dir = restore_test_dir("pendwait");
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font:\n\tsize: 12\n").unwrap();
		let new = "font:\n\tsize: 13\n";

		let started = std::time::Instant::now();
		let result = write_config_atomic_with(&path, new, pending);
		let elapsed = started.elapsed();

		let text = std::fs::read_to_string(&path).ok();
		let names = names_in_dir(&dir);
		let _ = std::fs::remove_dir_all(&dir);
		assert_eq!(result, Ok(()), "the write is reported as done");
		assert_eq!(text.as_deref(), Some(new), "the new text is at the name");
		assert_eq!(names, ["config.shcl"], "nothing beside it");
		assert!(
			elapsed < std::time::Duration::from_secs(2),
			"within the restore's bound: {elapsed:?}"
		);
	}

	// A name still pending when the tries run out gets the give-up error, and
	// nothing is written at it or beside it.
	// Test ID: EpaOYRu
	#[cfg(windows)]
	#[test]
	fn a_restore_gives_up_on_a_delete_that_stays_pending() {
		let pending: RestorePublish = |file, _| {
			delete_while_held(
				&std::fs::canonicalize(file).unwrap(),
				std::time::Duration::from_millis(1500),
			);
			Err(format!(
				"{file}: The replacement file could not be renamed. (os error 1176)"
			))
		};
		let dir = restore_test_dir("pendstay");
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font:\n\tsize: 12\n").unwrap();

		let started = std::time::Instant::now();
		let result = write_config_atomic_with(&path, "font:\n\tsize: 13\n", pending);
		let elapsed = started.elapsed();

		wait_until_gone(&path);
		let names = names_in_dir(&dir);
		let _ = std::fs::remove_dir_all(&dir);
		let err = result.expect_err("a restore that could not write is not a save");
		assert!(
			err.contains("could not be renamed. (os error 1176)") && err.contains("is gone"),
			"names the replace's error and that the file is gone: {err}"
		);
		assert!(
			elapsed >= std::time::Duration::from_millis(400)
				&& elapsed < std::time::Duration::from_millis(1400),
			"five tries, a tenth of a second apart: {elapsed:?}"
		);
		assert!(names.is_empty(), "nothing written: {names:?}");
	}

	// shcl's own save has no seam and no restore, so every settings write goes
	// through write_config_atomic instead.
	// Test ID: Epa8S0j
	#[test]
	fn every_settings_write_goes_through_the_restore() {
		let body = include_str!("config.rs")
			.split("\nmod tests {")
			.next()
			.expect("the file above its own tests");
		for call in [".save_file(", "save_file_lossy("] {
			assert!(!body.contains(call), "{call} skips the restore");
		}
		let publish = "shcl::write_file_atomic";
		assert_eq!(body.matches(publish).count(), 1, "{publish} is named once");
		// "\n}" alone: a Windows checkout can end the line with "\r\n"
		let fn_body = |name: &str| {
			let start = body.find(&format!("fn {name}(")).expect("the writer");
			&body[start..start + body[start..].find("\n}").expect("its end")]
		};
		assert!(
			fn_body("write_config_atomic").contains("write_config_keeping("),
			"write_config_atomic is write_config_keeping"
		);
		assert!(
			fn_body("write_config_keeping").contains(publish),
			"{publish} is called from write_config_keeping"
		);
	}

	// A hotkey set in the file is what loads. A bad one keeps its default and
	// is reported at launch with its line, rather than dropped in silence, and
	// so is a chord taken from another hotkey's default.
	// Test ID: EreU3sb
	#[test]
	fn a_hotkey_set_in_the_file_loads_and_a_bad_one_is_reported() {
		use crate::input::Hotkey;
		use crate::keys::Chord;
		use crate::pane::Toward;
		let path = std::path::Path::new("test.shcl");
		let text = "keys:\n\tsplit_right: \"Ctrl+Alt+R\"\n\tsplit_down: \"Ctrl+Alt+Bogus\"\n\tfocus_left: \"none\"\n\tclose_pane: \"Ctrl+Shift+W\"\n\tfocus_up: Alt+K\n\tfont_reset: 5\n";
		let (raw, said) = read_config_text(text, path);
		let s = resolve(raw);
		let d = Settings::default();
		let chord = |text| Chord::parse(text).expect(text);
		assert_eq!(s.keys.chords(Hotkey::SplitRight), [chord("Ctrl+Alt+R")]);
		assert_eq!(
			s.keys.chords(Hotkey::SplitDown),
			d.keys.chords(Hotkey::SplitDown)
		);
		assert!(s.keys.chords(Hotkey::Focus(Toward::Left)).is_empty());
		// quotes are optional
		assert_eq!(s.keys.chords(Hotkey::Focus(Toward::Up)), [chord("Alt+K")]);
		assert_eq!(
			s.keys.chords(Hotkey::ZoomReset),
			d.keys.chords(Hotkey::ZoomReset)
		);
		assert_eq!(
			s.keys.chords(Hotkey::Focus(Toward::Right)),
			d.keys.chords(Hotkey::Focus(Toward::Right))
		);
		let about_keys: Vec<&String> = said.iter().filter(|line| line.contains("keys.")).collect();
		assert!(
			about_keys
				.iter()
				.any(|line| line.contains("`keys.split_down` line 3 is not used")
					&& line.contains("Bogus is not a key name")),
			"{said:?}"
		);
		assert!(
			about_keys
				.iter()
				.any(|line| line.contains("`keys.font_reset` line 7 is not used")),
			"{said:?}"
		);
		if !cfg!(target_os = "macos") {
			assert!(
				about_keys
					.iter()
					.any(|line| line.contains("so `keys.close_tab` no longer answers to it")),
				"{said:?}"
			);
			assert_eq!(about_keys.len(), 3, "{said:?}");
		}
	}

	// A changed hotkey goes back into the file under its own name, by the
	// platform's spelling, and the next load reads the same bindings. The
	// Settings dialog's Keys tab is to save through this.
	// Test ID: EreU3sc
	#[test]
	fn a_changed_hotkey_is_written_back_by_its_name() {
		use crate::input::Hotkey;
		use crate::keys::{Bindings, Chord, value_text};
		let _guard = super::test_config_lock();
		let _ = settings();
		let mac = cfg!(target_os = "macos");
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfgkeys_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, "keys:\n\tclose_pane: \"Ctrl+Shift+W\"\n").unwrap();
		set_config_override(path.clone());
		let chord = |text| Chord::parse(text).expect(text);
		let orig = load();
		assert_eq!(
			orig.keys.shown(Hotkey::ClosePane),
			Some(chord("Ctrl+Shift+W"))
		);
		let mut edited = orig.clone();
		let right = vec![chord("Ctrl+Alt+R"), chord("Ctrl+Alt+Right")];
		edited.keys = Bindings::with(
			mac,
			&[
				(Hotkey::ClosePane, vec![chord("Ctrl+Shift+W")]),
				(Hotkey::SplitRight, right.clone()),
			],
		)
		.0;
		assert!(persist(&orig, &edited));
		let saved = std::fs::read_to_string(&path).unwrap();
		let want = format!("split_right: \"{}\"", value_text(&right, mac));
		assert!(saved.contains(&want), "{want} in {saved}");
		assert!(saved.contains("close_pane: \"Ctrl+Shift+W\""), "{saved}");
		assert!(load().keys == edited.keys);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A save can add a value as a new line above the template's commented one.
	// The revert has to find the value, not the comment, and leave the file as
	// it shipped rather than with a second copy of the commented line.
	// Test ID: Erekiyk
	#[test]
	fn a_revert_takes_out_a_value_saved_beside_its_default() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_revertline_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, "").unwrap();
		set_config_override(path.clone());
		reload_from_disk(); // lays the template down
		let pristine = std::fs::read_to_string(&path).unwrap();
		let flips: [(&str, fn(&mut Settings)); 3] = [
			("performance.automatic", |s| {
				s.performance_automatic = !s.performance_automatic;
			}),
			("scroll.smooth", |s| s.scroll_smooth = !s.scroll_smooth),
			("keys.close_pane", |s| {
				s.keys = s
					.keys
					.with_own(crate::input::Hotkey::ClosePane, Some(Vec::new()));
			}),
		];
		for (key, flip) in flips {
			let base = reload_from_disk();
			let mut changed = base.clone();
			flip(&mut changed);
			assert!(persist(&base, &changed));
			assert!(reload_from_disk() == changed, "{key} was saved");
			revert_keys(&[key]);
			assert!(reload_from_disk() == base, "{key} went back to its default");
			assert_eq!(std::fs::read_to_string(&path).unwrap(), pristine, "{key}");
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A file from before hotkeys gets the whole keys block at its next launch,
	// every line commented at its default, and loads the same bindings.
	// Test ID: EreU3sd
	#[test]
	fn an_older_file_gains_the_keys_block_commented() {
		let text = "window:\n\tmargin: 6\n";
		let after = backfilled_text(text)
			.expect("nothing refused")
			.expect("something added");
		let path = std::path::Path::new("test.shcl");
		assert!(after.contains("\nkeys:\n"), "{after}");
		for (_, path) in crate::keys::config_paths() {
			let name = path.trim_start_matches("keys.");
			assert!(after.contains(&format!("\t# {name}: \"")), "{name}");
		}
		assert!(after.contains("\"none\" turns a\n"), "{after}");
		assert!(resolve(read_raw(&after, path).0).keys == Settings::default().keys);
		assert_eq!(backfilled_text(&after), Ok(None), "settles in one launch");
	}

	// Test ID: Eq4SnxO
	#[test]
	fn persist_writes_nothing_for_a_nan_on_both_sides() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_cfgnan_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font.use_system_size: false\nfont.size: 17\n").unwrap();
		set_config_override(path.clone());

		let orig = load();
		assert_eq!(orig.font_size, 17.0);
		// What --font-size nan folded into the live settings, standing on both
		// sides of the diff: it is the run's own value, not a change to save.
		let mut live = orig.clone();
		live.font_size = f32::NAN;
		let same = live.clone();
		assert!(persist(&live, &same));
		let saved = std::fs::read_to_string(&path).unwrap();
		assert!(
			saved.contains("size: 17"),
			"NaN written over the user's size: {saved:?}"
		);
		assert!(!saved.to_lowercase().contains("nan"), "{saved:?}");
		assert_eq!(load().font_size, 17.0);

		// and a real change still reaches the file
		let mut edited = live.clone();
		edited.font_size = 22.0;
		assert!(persist(&live, &edited));
		assert_eq!(load().font_size, 22.0);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EjkEYAK
	#[test]
	fn persist_survives_bare_decimal_float() {
		// Memoize settings() BEFORE installing the override: a test on another
		// thread initializing settings() after the override would load() - an
		// in-place migrate/backfill REWRITE of our temp file - racing our own
		// read below (parallel-suite flake: truncated read -> defaults).
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfgsave_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, "wallpaper.opacity: .1\ntext.scrim.ramp: \"s\"\n").unwrap();
		set_config_override(path.clone());

		let orig = load();
		assert_eq!(orig.text_scrim_ramp, "sigmoid"); // the file's older spelling
		let mut edited = orig.clone();
		edited.text_scrim_ramp = "log".to_string();
		assert!(
			persist(&orig, &edited),
			"persist should write to our temp file"
		);

		assert_eq!(
			load().text_scrim_ramp,
			"log",
			"dialog change lost after relaunch"
		);
		// and the value is still spelled the way the user wrote it
		let saved = std::fs::read_to_string(&path).unwrap();
		assert!(
			saved.contains("opacity: .1"),
			"scalar should be left verbatim: {saved:?}"
		);
		assert_eq!(load().wallpaper_opacity, 0.1);

		// clearing the Family box has to take the line out: writing nothing left
		// it there and the font came back next launch
		let before = load();
		let mut named = before.clone();
		named.font_family = Some("Iosevka".to_string());
		assert!(persist(&before, &named));
		assert_eq!(load().font_family.as_deref(), Some("Iosevka"));

		let before = load();
		let mut cleared = before.clone();
		cleared.font_family = None;
		assert!(persist(&before, &cleared));
		assert_eq!(load().font_family, None, "cleared, and it stays cleared");
	}

	// The /proc-based busy check: a child process holding the file open is seen as
	// busy; once it exits the file reads as free again. Linux only (the check is a
	// no-op elsewhere).
	// Test ID: EjwSEES
	#[cfg(target_os = "linux")]
	#[test]
	fn config_open_elsewhere_sees_a_holder() {
		let path =
			crate::testdir::run_dir().join(format!("silkterm_busy_{}.shcl", std::process::id()));
		std::fs::write(&path, "margin: 8.0\n").unwrap();
		assert!(!config_open_elsewhere(&path), "nobody holds it yet");

		// A child with the file as its stdin holds the descriptor open until it exits.
		let hold = std::fs::File::open(&path).unwrap();
		let mut child = std::process::Command::new("sleep")
			.arg("30")
			.stdin(std::process::Stdio::from(hold))
			.spawn()
			.unwrap();

		// give the child a moment to exist in /proc, then confirm we see it
		let mut seen = false;
		for _ in 0..50 {
			if config_open_elsewhere(&path) {
				seen = true;
				break;
			}
			std::thread::sleep(std::time::Duration::from_millis(20));
		}
		let _ = child.kill();
		let _ = child.wait();
		let _ = std::fs::remove_file(&path);
		assert!(seen, "a process holding the file open should read as busy");
	}

	// Where a rating's lines go, and that nothing else in the file moves.
	// Test ID: EpXN9p5
	#[test]
	fn rating_lines_replace_insert_and_collapse() {
		const ID: &str = "0123456789abcdef";
		let id_only = RatingLines {
			rated_hardware: Some(ID),
			..RatingLines::default()
		};
		let cases = [
			(
				"an active line is replaced in place, its indent kept",
				"theme_mode: dark\nperformance:\n    automatic: true\n    rated_hardware: 0000000000000000\n    profile: high\n\nwindow:\n\tmargin: 4\n",
				"theme_mode: dark\nperformance:\n    automatic: true\n    rated_hardware: 0123456789abcdef\n    profile: high\n\nwindow:\n\tmargin: 4\n",
			),
			(
				"a trailing note is kept",
				"performance:\n\trated_hardware: 0000000000000000  ## note\n",
				"performance:\n\trated_hardware: 0123456789abcdef  ## note\n",
			),
			(
				"only the commented default: directly after it",
				"performance:\n\n\t# automatic: true  ## Default\n\t# rated_hardware: \"\"  ## Default\n\t# profile: \"max\"  ## Default\n\n## next\n",
				"performance:\n\n\t# automatic: true  ## Default\n\t# rated_hardware: \"\"  ## Default\n\trated_hardware: 0123456789abcdef\n\t# profile: \"max\"  ## Default\n\n## next\n",
			),
			(
				"only the header: first in the block, at its children's depth",
				"performance:\n\tautomatic: true\n",
				"performance:\n\trated_hardware: 0123456789abcdef\n\tautomatic: true\n",
			),
			(
				"a header with no child: one tab deeper",
				"performance:\nwindow:\n\tmargin: 4\n",
				"performance:\n\trated_hardware: 0123456789abcdef\nwindow:\n\tmargin: 4\n",
			),
			(
				"two active lines: one is left, holding the new value",
				"performance:\n\trated_hardware: 0000000000000000\n\tautomatic: true\n\trated_hardware: 1111111111111111\n",
				"performance:\n\trated_hardware: 0123456789abcdef\n\tautomatic: true\n",
			),
		];
		for (what, text, want) in cases {
			assert_eq!(
				with_rating_lines(text, &id_only).as_deref(),
				Ok(want),
				"{what}"
			);
		}

		let both = RatingLines {
			profile: Some("high"),
			check_next_run: Some(false),
			..RatingLines::default()
		};
		assert_eq!(
			with_rating_lines(
				"performance:\n\t# profile: \"max\"  ## Default\n\t# check_next_run: false  ## Default\n\tcheck_next_run: true\n",
				&both
			)
			.as_deref(),
			Ok(
				"performance:\n\t# profile: \"max\"  ## Default\n\tprofile: high\n\t# check_next_run: false  ## Default\n\tcheck_next_run: false\n"
			),
			"a word and a flag together"
		);

		// A block written twice reads as one, so the first takes the line.
		assert_eq!(
			with_rating_lines(
				"performance:\n\tautomatic: true\nperformance:\n\tprofile: high\n",
				&id_only
			)
			.as_deref(),
			Ok(
				"performance:\n\trated_hardware: 0123456789abcdef\n\tautomatic: true\nperformance:\n\tprofile: high\n"
			),
			"two performance headers"
		);
		// With no block to put a line in, a file that reads clean gets what the
		// dialog's save would write.
		let bare = "window:\n\tmargin: 4\n";
		let mut saved = shcl::Document::parse(bare);
		assert!(saved.set_string("performance.rated_hardware", ID));
		assert_eq!(
			with_rating_lines(bare, &id_only).as_deref(),
			Ok(saved.to_canonical().as_str()),
			"no performance header"
		);
		// A word no rating writes is refused, and the reason is true of the file:
		// it reads clean, or it has a line the parse drops.
		let clean = "performance:\n\trated_hardware: 0000000000000000\n";
		let lossy = "performance:\n\t\trated_hardware: 0000000000000000\n\tautomatic: true\n";
		assert_eq!(shcl::Document::parse(lossy).lost_count(), 1);
		for word in [
			"a\"b",
			"a b",
			"Max",
			"",
			"a\nb",
			"0123456789abcdef0123456789abcdef0",
		] {
			let lines = RatingLines {
				rated_hardware: Some(word),
				..RatingLines::default()
			};
			assert_eq!(
				with_rating_lines(clean, &lines),
				Err(Kept::Unplaced),
				"{word:?} is not a program-made word"
			);
			assert_eq!(
				with_rating_lines(lossy, &lines),
				Err(Kept::Unreadable),
				"{word:?}, beside a line that cannot be read"
			);
		}
	}

	// The template is a save fixed point (G69), and a rating written into it must
	// not be the thing that makes the next save rewrite it. An all-digit id is the
	// spelling most likely to come out differently.
	// Test ID: EpXN9p6
	#[test]
	fn a_rating_leaves_a_canonical_file_canonical() {
		for id in ["0123456789abcdef", "1234567890123456"] {
			let lines = RatingLines {
				profile: Some("max"),
				rated_hardware: Some(id),
				check_next_run: Some(false),
			};
			let out = with_rating_lines(default_config(), &lines)
				.unwrap_or_else(|kept| panic!("id {id}: {kept:?}"));
			assert_ne!(out, default_config(), "id {id}: the values are in it");
			assert_eq!(shcl::Document::parse(&out).to_canonical(), out, "id {id}");
		}
	}

	// A line placed in a block that already drops one can change which line the
	// parse drops. The count does not grow, and another setting loads differently.
	// Test ID: EpXeZQW
	#[test]
	fn a_rating_changes_no_other_setting() {
		let text = "performance:\n\t\t# rated_hardware: \"\"  ## Default\n\t\t\tautomatic: false\n\t\tcheck_hardware: false\n";
		let before = shcl::Document::parse(text);
		assert_eq!(before.lost_count(), 1);
		assert_eq!(before.get_bool("performance.automatic"), Ok(false));
		assert_eq!(
			before.get_bool("performance.check_hardware"),
			Err(shcl::Status::NotFound)
		);
		let lines = RatingLines {
			rated_hardware: Some("0123456789abcdef"),
			..RatingLines::default()
		};
		if let Ok(out) = with_rating_lines(text, &lines) {
			let after = shcl::Document::parse(&out);
			assert_eq!(
				after.get_bool("performance.automatic"),
				Ok(false),
				"automatic loads as before:\n{out}"
			);
			assert_eq!(
				after.get_bool("performance.check_hardware"),
				Err(shcl::Status::NotFound),
				"check_hardware loads as before:\n{out}"
			);
		}

		// A rating key this write leaves alone is another setting too: a new
		// `profile:` line above a deeper `check_next_run:` takes it in as a child.
		let text = "performance:\n\t# check_hardware: \"\"  ## Default\n\t\tcheck_next_run: true\n";
		assert_eq!(
			shcl::Document::parse(text).get_bool("performance.check_next_run"),
			Ok(true)
		);
		let lines = RatingLines {
			profile: Some("high"),
			..RatingLines::default()
		};
		if let Ok(out) = with_rating_lines(text, &lines) {
			assert_eq!(
				shcl::Document::parse(&out).get_bool("performance.check_next_run"),
				Ok(true),
				"check_next_run loads as before:\n{out}"
			);
		}

		// Every later line for the key is deleted, and what sat under one moves up to
		// the line above. Here that is "Re-test next run", which this write does not
		// touch. Placement alone loses no line and reads the profile back, so the
		// comparison is the only thing that refuses it.
		let text =
			"performance:\n\tprofile: max\nperformance.profile: low\n\tcheck_next_run: true\n";
		assert!(
			migrate_config_text(text).is_none(),
			"a launch parses this text as it is"
		);
		let before = shcl::Document::parse(text);
		assert_eq!(
			before.get_bool("performance.check_next_run"),
			Err(shcl::Status::NotFound)
		);
		let placed = placed_rating_lines(text, &[("profile", "high".to_string())])
			.expect("a performance block to place into");
		let placed = shcl::Document::parse(&placed);
		assert!(
			placed.lost_count() <= before.lost_count()
				&& placed.get_string("performance.profile").as_deref() == Ok("high")
				&& placed.get_bool("performance.check_next_run") == Ok(true),
			"placement no longer moves check_next_run, so the case proves nothing"
		);
		let lines = RatingLines {
			profile: Some("high"),
			..RatingLines::default()
		};
		if let Ok(out) = with_rating_lines(text, &lines) {
			assert_eq!(
				shcl::Document::parse(&out).get_bool("performance.check_next_run"),
				Err(shcl::Status::NotFound),
				"check_next_run loads as before:\n{out}"
			);
		}
	}

	// The template in shapes that read clean but that a rating's line did not go
	// into: a line typed in a spaces editor, a value cleared by hand, a comment
	// indented deeper than the block.
	fn clean_rating_shapes() -> Vec<(&'static str, String)> {
		let template = default_config();
		let default_line = "\t# rated_hardware: \"\"  ## Default\n";
		let shapes = [
			(
				"a line typed with spaces first in the block",
				template.replacen(
					"\nperformance:\n",
					"\nperformance:\n    automatic: true\n",
					1,
				),
			),
			(
				"a line typed with spaces after the commented defaults",
				template.replacen(
					default_line,
					&format!("{default_line}    check_hardware: true\n"),
					1,
				),
			),
			(
				"rated_hardware cleared by hand",
				template.replacen(default_line, "\trated_hardware:\n", 1),
			),
			(
				"the commented default deeper than the block",
				template.replacen(
					default_line,
					"\t\t# rated_hardware: \"\"  ## Default\n\tautomatic: true\n",
					1,
				),
			),
		];
		for (what, text) in &shapes {
			assert_ne!(
				text, template,
				"{what}: the template no longer has that line"
			);
			assert_eq!(
				shcl::Document::parse(text).lost_count(),
				0,
				"{what}: reads clean"
			);
		}
		shapes.into()
	}

	// Each of these kept a rating through the dialog's save, and the line writer
	// answered that it had a line that could not be read, so the test ran at
	// every launch.
	// Test ID: EpXiS3s
	#[test]
	fn a_rating_reaches_a_clean_file_wherever_its_lines_sit() {
		const ID: &str = "0123456789abcdef";
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratingclean_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some(ID),
			check_next_run: None,
		};
		for (what, text) in clean_rating_shapes() {
			std::fs::write(&path, &text).unwrap();
			let _ = reload_from_disk();
			assert_eq!(keep_rating(&lines), Kept::Written, "{what}");
			let reloaded = reload_from_disk();
			assert_eq!(reloaded.rated_hardware, ID, "{what}");
			assert_eq!(reloaded.performance_profile, "high", "{what}");
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// On those files the lines go in one by one: every line already there stays,
	// byte for byte and in order, apart from the cleared value that gets one.
	// Test ID: EpXiS3t
	#[test]
	fn a_rating_in_a_clean_file_moves_no_other_line() {
		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some("0123456789abcdef"),
			check_next_run: None,
		};
		for (what, text) in clean_rating_shapes() {
			let out =
				with_rating_lines(&text, &lines).unwrap_or_else(|kept| panic!("{what}: {kept:?}"));
			let mut rest = out.lines();
			for line in text.lines().filter(|line| line.trim() != "rated_hardware:") {
				assert!(
					rest.any(|kept| kept == line),
					"{what}: {line:?} is not where it was"
				);
			}
		}
	}

	// A value cleared by hand leaves `rated_hardware:` with nothing under it, and
	// that line takes the rating. Beside a line the parse drops there is no save's
	// text to fall back on, so a second line for the key leaves the rating unread
	// and the test runs at every launch.
	// Test ID: EpYWC5Y
	#[test]
	fn a_value_cleared_by_hand_takes_the_rating_on_its_own_line() {
		let lines = RatingLines {
			rated_hardware: Some("0123456789abcdef"),
			..RatingLines::default()
		};
		let lost = "window:\n\t\tmargin: 4\n\tstray: 1\n";
		for (what, block, want) in [
			(
				"first in the block",
				"performance:\n\trated_hardware:\n\tautomatic: true\n",
				"performance:\n\trated_hardware: 0123456789abcdef\n\tautomatic: true\n",
			),
			(
				"last in the block",
				"performance:\n\tautomatic: true\n\trated_hardware:\n",
				"performance:\n\tautomatic: true\n\trated_hardware: 0123456789abcdef\n",
			),
		] {
			let text = format!("{block}{lost}");
			assert_eq!(
				shcl::Document::parse(&text).lost_count(),
				1,
				"{what}: the line stepping back to no level is the one the parse drops"
			);
			assert_eq!(
				with_rating_lines(&text, &lines),
				Ok(format!("{want}{lost}")),
				"{what}"
			);
		}
	}

	// Deleting a later line for the key moves what sat under it up into the block,
	// and at an indent the block does not use the parse drops it. It was under a
	// written key before and is not read at all after, so no setting reads
	// differently, and only the lost-line count stops a file the next Settings
	// save refuses whole.
	// Test ID: EpYhM4m
	#[test]
	fn a_rating_loses_no_line_the_file_did_not_already_lose() {
		const ID: &str = "0123456789abcdef";
		let lines = RatingLines {
			rated_hardware: Some(ID),
			..RatingLines::default()
		};
		let spelled = [(
			"rated_hardware",
			rating_spelling("rated_hardware", RatingValue::Word(ID)).expect("a plain id"),
		)];
		let written = ["performance.rated_hardware".to_string()];
		let block = "performance:\n\t\trated_hardware: 0000000000000000\nperformance.rated_hardware: 1111111111111111\n\tcheck_hardware: true\n";
		let lost = "window:\n\t\tmargin: 4\n\tstray: 1\n";
		for (what, text, had) in [
			("a file that reads clean", block.to_string(), 0),
			(
				"a file that already lost a line",
				format!("{block}{lost}"),
				1,
			),
		] {
			let before = shcl::Document::parse(&text);
			assert_eq!(before.lost_count(), had, "{what}");
			let placed =
				placed_rating_lines(&text, &spelled).expect("a performance block to place into");
			assert!(
				migrate_config_text(&text).is_none() && migrate_config_text(&placed).is_none(),
				"{what}: a launch parses both texts as they are"
			);
			let placed = shcl::Document::parse(&placed);
			assert!(
				placed.lost_count() > had
					&& placed.get_string("performance.rated_hardware").as_deref() == Ok(ID),
				"{what}: placement no longer drops check_hardware, so the case proves nothing"
			);
			assert_eq!(
				settings_besides(&placed, &written),
				settings_besides(&before, &written),
				"{what}: the comparison refuses this text as well, so the case proves nothing"
			);
			if let Ok(out) = with_rating_lines(&text, &lines) {
				assert!(
					shcl::Document::parse(&out).lost_count() <= had,
					"{what}: a line was lost, and the next Settings save refuses the file:\n{out}"
				);
			}
		}
	}

	// The dialog's save was how a rating was written before, so the files it kept
	// one in are the floor: the rating writer keeps one in each of them too.
	// Test ID: EpXiS3u
	#[test]
	fn a_rating_is_kept_wherever_the_dialogs_save_kept_it() {
		const ID: &str = "0123456789abcdef";
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratingparity_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some(ID),
			check_next_run: None,
		};
		let holds = || {
			let s = reload_from_disk();
			s.rated_hardware == ID && s.performance_profile == "high"
		};
		let mut files = vec![("the template", default_config().to_string())];
		files.extend(clean_rating_shapes());
		for (what, text) in files {
			std::fs::write(&path, &text).unwrap();
			let orig = reload_from_disk();
			let mut new = orig.clone();
			new.rated_hardware = ID.to_string();
			new.performance_profile = "high".to_string();
			let saved = persist(&orig, &new) && holds();

			std::fs::write(&path, &text).unwrap();
			let _ = reload_from_disk();
			let kept = keep_rating(&lines);
			let written = kept == Kept::Written && holds();
			assert!(
				!saved || written,
				"{what}: the save keeps a rating here, and the rating writer answers {kept:?}"
			);
			assert!(
				saved,
				"{what}: the save keeps no rating here, so the case proves nothing"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// "A line that cannot be read" is said only of a file that has one. A clean
	// file the writer still cannot place gets a reason that is true of it.
	// Test ID: EpXiS3v
	#[test]
	fn a_clean_file_is_never_called_unreadable() {
		let id_only = RatingLines {
			rated_hardware: Some("0123456789abcdef"),
			..RatingLines::default()
		};
		let mut cases: Vec<(&str, String, RatingLines)> = vec![
			(
				"no performance block",
				"window:\n\tmargin: 4\n".to_string(),
				id_only,
			),
			(
				"a dotted performance setting only",
				"performance.automatic: true\n".to_string(),
				id_only,
			),
			(
				"two performance blocks",
				"performance:\n\tautomatic: true\nperformance:\n\tprofile: high\n".to_string(),
				id_only,
			),
			(
				"a word the program would not write",
				"performance:\n\trated_hardware: 0000000000000000\n".to_string(),
				RatingLines {
					rated_hardware: Some("Max"),
					..RatingLines::default()
				},
			),
		];
		cases.extend(
			clean_rating_shapes()
				.into_iter()
				.map(|(what, text)| (what, text, id_only)),
		);
		for (what, text, lines) in cases {
			assert_eq!(
				shcl::Document::parse(&text).lost_count(),
				0,
				"{what}: reads clean"
			);
			assert_ne!(
				with_rating_lines(&text, &lines),
				Err(Kept::Unreadable),
				"{what}"
			);
		}
	}

	// A canonical save quotes a bare value holding a space or a colon, and takes
	// the quotes off one that reads as a number, bool or date. The rating check
	// leaves the quoted flag out because of this. The quote character itself is
	// read by the launch's migration, and the check compares migrated text for that.
	// Test ID: EpXyGI4
	#[test]
	fn a_save_that_requotes_a_value_changes_no_read() {
		const PATH: &str = "blk.k";
		let values = [
			"Cascadia Mono",
			r"C:\Users\x",
			"a: b",
			r#""unclosed"#,
			"it's fine",
			"12:30",
			"2026-09-10T10:00:00",
			"2026-09-10 10:00",
			r"1\,000",
			r#""5""#,
			"'5'",
			r#""true""#,
			r#""yes""#,
			r#""on""#,
			r#""off""#,
			r#""FALSE""#,
			r#""0x10""#,
			r#""1e5""#,
			r#""08""#,
			r#""-0""#,
			r#""+5""#,
			r#""0.5""#,
			r#"".25""#,
			r#""1""#,
			r#""2026-09-10""#,
			r#""1,000""#,
			r#""1,000.5""#,
			r"'1\,000'",
			r#""a, b""#,
			"a, b",
			r#""a,b", c"#,
			r"x\",
			r#""@null""#,
		];
		let reads = |doc: &shcl::Document| {
			(
				doc.get_string(PATH),
				doc.instances(PATH),
				doc.count(PATH),
				doc.read_string(PATH).status,
				doc.get_int(PATH),
				doc.get_float(PATH).map(f64::to_bits),
				doc.get_bool(PATH),
				doc.get_datetime(PATH),
				doc.get_string_array(PATH),
				doc.lost_count(),
			)
		};
		let (mut added, mut dropped) = (false, false);
		for value in values {
			let before = shcl::Document::parse(&format!("blk:\n\tk: {value}\n"));
			let after = shcl::Document::parse(&before.to_canonical());
			assert_eq!(reads(&after), reads(&before), "{value}");
			let (was, is) = (
				before.read_string(PATH).quoted,
				after.read_string(PATH).quoted,
			);
			added |= !was && is;
			dropped |= was && !is;
		}
		assert!(
			added,
			"a save quotes none of these now, so the rating check's handling of quoting needs another look"
		);
		assert!(
			dropped,
			"a save unquotes none of these now, so the rating check's handling of quoting needs another look"
		);
	}

	// With no performance block the rating gets the save's text, which requoted
	// values nobody changed where it fell back to the canonical form, and no load
	// reads the difference. Each file is
	// written again just before the rating, since a load adds the block and the
	// line writer would then never reach the save's text.
	// Test ID: EpXyGI5
	#[test]
	fn a_rating_is_kept_where_a_save_only_requotes() {
		const ID: &str = "0123456789abcdef";
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir()
			.join(format!("silkterm_ratingrequote_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some(ID),
			check_next_run: None,
		};
		let files: [(&str, &str, fn(&Settings) -> String); 4] = [
			(
				"a bare font name with a space",
				"performance.automatic: true\nfont:\n\tfamily: Cascadia Mono\n",
				|s| format!("{:?}", s.font_family),
			),
			(
				"a bare Windows folder",
				"performance.automatic: true\nshell:\n\tstartup_directory: C:\\Users\\x\n",
				|s| s.startup_directory.clone(),
			),
			(
				"a bare font name, nothing else in the file",
				"font:\n\tfamily: Cascadia Mono\n",
				|s| format!("{:?}", s.font_family),
			),
			(
				"a quoted number",
				"performance.automatic: true\nwindow:\n\tmargin: \"5\"\n",
				|s| s.margin.to_string(),
			),
		];
		for (what, text, other) in files {
			let mut saved = parse_kept(text);
			assert!(saved.set_string("performance.profile", "high"), "{what}");
			assert!(saved.set_string("performance.rated_hardware", ID), "{what}");
			assert_eq!(
				with_rating_lines(text, &lines).as_deref(),
				Ok(saved_text(&saved).as_str()),
				"{what}"
			);

			std::fs::write(&path, text).unwrap();
			let loaded = other(&reload_from_disk());
			assert_ne!(
				loaded,
				other(&Settings::default()),
				"{what}: the value is the default, so the case proves nothing"
			);
			std::fs::write(&path, text).unwrap();
			assert_eq!(keep_rating(&lines), Kept::Written, "{what}");
			let reloaded = reload_from_disk();
			assert_eq!(reloaded.rated_hardware, ID, "{what}");
			assert_eq!(reloaded.performance_profile, "high", "{what}");
			assert_eq!(
				other(&reloaded),
				loaded,
				"{what}: the other value loads as before"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A launch parses migrated text, and the migration reads how a line is
	// written: only a double-quoted old font list is refreshed, and a commented
	// heading deeper than its block becomes the parent of the setting under it.
	// The save's text moves both without changing a value. Each file is written
	// again before the rating, as a launch that found it busy leaves it.
	// Test ID: EpYDF0i
	#[test]
	fn a_rating_changes_nothing_the_next_launch_migrates() {
		let stack = format!(
			"performance.automatic: true\nfont:\n\tuse_system_family: false\n\tfamily: '{}'\n",
			SUPERSEDED_FONT_STACKS[0]
		);
		rating_leaves_other_loads_alone(
			"ratingmigrate",
			vec![
				(
					"an old default font list in single quotes",
					stack.clone(),
					|s| format!("{:?}", s.font_family),
				),
				(
					"the same beside a bare Windows folder",
					format!("{stack}shell:\n\tstartup_directory: C:\\Users\\x\n"),
					|s| format!("{:?}", s.font_family),
				),
				(
					"a renamed color under a commented heading",
					"performance.automatic: true\ncolors:\n\t# x:\n\t\tfocus: \"#112233\"\n"
						.to_string(),
					|s| format!("{:?} {:?}", s.focus, s.highlight),
				),
				(
					"a renamed tab width under a commented heading",
					"performance.automatic: true\nwindow:\n\t# x:\n\t\ttab_min_width_pct: 12\n"
						.to_string(),
					|s| s.tab_regular_pct.to_string(),
				),
			],
		);
	}

	// The same one launch step earlier. A file from before the nested layout is
	// converted first, and the conversion copies a font list with its quotes, which
	// the refresh after it reads. A save spells them the other way, so the rating
	// check has to look through the conversion as well.
	// Test ID: EqHPuMK
	#[test]
	fn a_rating_changes_nothing_the_next_launch_converts() {
		let flat = format!("font_family: '{}'\n", SUPERSEDED_FONT_STACKS[0]);
		let font = |s: &Settings| format!("{:?}", s.font_family);
		rating_leaves_other_loads_alone(
			"ratingconvert",
			vec![
				("an old font list in a flat file", flat.clone(), font),
				(
					"the same with the system font switched off",
					format!("use_system_font: false\n{flat}"),
					font,
				),
				(
					"the same with Windows line ends",
					format!("use_system_font: false\r\n{}", flat.replace('\n', "\r\n")),
					font,
				),
				(
					"the same beside a bare Windows folder",
					format!(
						"use_system_font: false\n{flat}wallpaper_folder: C:\\Users\\x\\Pictures\n"
					),
					font,
				),
			],
		);
	}

	// A short file gets most of the template at its first launch, one group at a
	// time. A group placed above the next section used to step back over the
	// commented-out settings of the group added just before it, so `font:` went
	// in under `contrast_mask:` and the later groups split each other. Their
	// settings read as missing at the next launch and were added again.
	// Test ID: EquUFK4
	#[test]
	fn backfill_puts_each_group_in_its_own_section() {
		let text = "wallpaper:\n\timage: x\nwindow:\n\tmargin: 6\nperformance:\n\tautomatic: false\n\tprofile: custom\n";
		let text = next_launch_text(text).into_owned();
		let out = backfilled_text(&text).unwrap().unwrap();
		let at = paths_at(&out.lines().map(str::to_string).collect::<Vec<_>>());
		for path in [
			"font",
			"wallpaper.contrast_mask.strength",
			"wallpaper.contrast_mask.auto",
			"text.color_emoji",
			"text.embolden_inverse",
		] {
			assert!(
				at.contains_key(path),
				"{path} is not where it belongs\n{out}"
			);
		}
		assert_eq!(backfilled_text(&out), Ok(None), "\n{out}");

		// a file written with dotted keys only, like the scroll harness's
		let dotted = "performance.automatic: false\nperformance.profile: custom\nscroll.smooth_apps: true\nscroll.minimap.enabled: false\ntransparency.enabled: false\ntext.scrim.enabled: false\nwallpaper.fallback_builtin: false\nwindow.columns: 100\nwindow.rows: 34\ncursor.animation: none\n";
		let mut text = dotted.to_string();
		for launch in 1..=3 {
			let next = next_launch_text(&text).into_owned();
			let next = backfilled_text(&next).unwrap().unwrap_or(next);
			if launch > 1 {
				assert_eq!(next, text, "launch {launch} changed the file");
			}
			text = next;
		}
	}

	// The check compares what the next launch loads, through that launch's own
	// rewrites. Here one of them reads the quote character, as the font list
	// refresh once did: a value in single quotes is replaced. The only text that
	// keeps a rating in this file is a save's, which spells the value in double
	// quotes, so the step stops firing and the value loads differently. That write
	// has to be refused, and with no such step it goes through.
	// A setting typed two tabs in under its block loads, until the launch adds the
	// template's own active lines above it. It then read as part of one of those
	// and was ignored from the next launch on, with nothing said.
	// `stray` steps back to a depth nothing under `window:` uses, so shcl drops
	// it. The window settings backfill would add at one tab give that depth a
	// block, and the stray started being read as a window setting.
	// Test ID: ErCHjkM
	#[test]
	fn backfill_leaves_a_dropped_line_dropped() {
		let text = "window:\n\t\tmargin: 4\n\trows: 40\n";
		let before = shcl::Document::parse(text);
		assert_eq!(before.lost_count(), 1);
		assert!(before.get_string("window.rows").is_err());
		let out = backfilled_text(text)
			.expect("nothing that loaded changes")
			.expect("other groups are still missing");
		let after = shcl::Document::parse(&out);
		assert_eq!(after.lost_count(), 1, "{out}");
		assert!(after.get_string("window.rows").is_err(), "{out}");
		assert_eq!(
			after.get_string("window.margin"),
			before.get_string("window.margin")
		);
		assert!(out.contains("\nfont:\n"), "another block is still added");
	}

	// Test ID: EpZCS12
	#[test]
	fn backfill_keeps_a_setting_that_is_indented_too_deep() {
		let files = [
			("window.rows", "window:\n\t# x:\n\t\trows: 31\n"),
			("window.rows", "window:\n\t\trows: 31\n"),
			(
				"window.tab_regular_width_pct",
				"window:\n\t# x:\n\t\ttab_regular_width_pct: 12\n",
			),
			(
				"wallpaper.rotate.interval_s",
				"wallpaper:\n\trotate:\n\t\t\t\tinterval_s: 40\n",
			),
			// two of them: a line added between at the block's depth could not be
			// read, and no save went through after that
			(
				"window.columns",
				"window:\n\t# x:\n\t\trows: 31\n\t\tcolumns: 97\n",
			),
		];
		for (path, text) in files {
			let wanted = shcl::Document::parse(text).get_string(path);
			assert!(wanted.is_ok(), "{path} loads to begin with");
			let out = backfilled_text(text)
				.unwrap_or_else(|lost| panic!("{path}: gave up over {lost}"))
				.expect("a short file lacks settings");
			assert_eq!(
				shcl::Document::parse(&out).get_string(path),
				wanted,
				"{path} still loads:\n{out}"
			);
			assert_eq!(
				shcl::Document::parse(&out).lost_count(),
				0,
				"{path}: every line still reads:\n{out}"
			);
			assert_eq!(
				backfilled_text(&out),
				Ok(None),
				"{path}: settles in one pass"
			);
		}
	}

	// Test ID: EpZCS13
	#[test]
	fn a_rating_is_refused_where_a_launch_step_reads_the_layout() {
		fn reads_quotes(text: &str) -> Option<String> {
			text.contains("'Old Mono'")
				.then(|| text.replace("'Old Mono'", "\"New Mono\""))
		}
		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some("0123456789abcdef"),
			check_next_run: None,
		};
		// the save keeps the quotes of a file it can keep line by line
		let single = "font:\n\tfamily: 'Old Mono'\n";
		let out = with_rating_lines_through(single, &lines, &[reads_quotes]).expect("kept");
		assert!(out.contains("'Old Mono'"), "{out}");
		// `font` twice is folded into one block, so the save falls back to the
		// canonical form
		let text = "font:\n\tfamily: 'Old Mono'\nwindow:\n\tcolumns: 90\nfont:\n\tsize: 13\n";
		let plain = with_rating_lines_through(text, &lines, &[]).expect("no step, so it is kept");
		assert!(
			plain.contains("\"Old Mono\""),
			"the save respells the value:\n{plain}"
		);
		assert_eq!(
			with_rating_lines_through(text, &lines, &[reads_quotes]),
			Err(Kept::Unplaced),
			"the next launch would load another font"
		);
		// a file with somewhere to put the lines is written line by line, the value
		// keeps its quotes, and the step fires on both sides
		let blocked = format!("{text}performance:\n\tautomatic: true\n");
		let out = with_rating_lines_through(&blocked, &lines, &[reads_quotes]).expect("kept");
		assert!(out.contains("'Old Mono'"), "{out}");
	}

	// Each file is loaded, put back as it was (the state a launch leaves when it
	// finds the file open elsewhere), rated, and loaded again. Either the rating
	// went in and `other` loads as before, or nothing was written and it goes in a
	// launch later.
	fn rating_leaves_other_loads_alone(
		tag: &str,
		files: Vec<(&str, String, fn(&Settings) -> String)>,
	) {
		const ID: &str = "0123456789abcdef";
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_{tag}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some(ID),
			check_next_run: None,
		};
		for (what, text, other) in files {
			std::fs::write(&path, &text).unwrap();
			let loaded = other(&reload_from_disk());
			std::fs::write(&path, &text).unwrap();
			let kept = keep_rating(&lines);
			let disk = std::fs::read_to_string(&path).unwrap();
			if kept == Kept::Written {
				assert_eq!(
					other(&reload_from_disk()),
					loaded,
					"{what}: loads as before"
				);
			} else {
				assert_eq!(kept, Kept::Unplaced, "{what}");
				assert_eq!(disk, text, "{what}: nothing written");
				// a launch that is not busy migrates the file, and the rating goes in then
				let _ = reload_from_disk();
				assert_eq!(keep_rating(&lines), Kept::Written, "{what}: next launch");
				let reloaded = reload_from_disk();
				assert_eq!(reloaded.rated_hardware, ID, "{what}");
				assert_eq!(
					other(&reloaded),
					loaded,
					"{what}: loads as before after the next launch"
				);
			}

			// Each case ended by showing that a plain save of the same rating did
			// move `other`. The launch steps read no quotes or indents since
			// 2026-09-18, so a save moves none of them and that guard could only
			// fail. `a_rating_is_refused_where_a_launch_step_reads_the_layout` holds
			// the check to account instead, with a step of its own.
			//   assert_ne!(other(&reload_from_disk()), loaded, "... proves nothing");
			let mut saved = shcl::Document::parse(&text);
			assert!(saved.set_string("performance.profile", "high"), "{what}");
			assert!(saved.set_string("performance.rated_hardware", ID), "{what}");
			std::fs::write(&path, saved.to_canonical()).unwrap();
			assert_eq!(
				other(&reload_from_disk()),
				loaded,
				"{what}: a save of the same rating loads it as before too"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A line the parse cannot place made every save refuse, and the rating is a
	// save, so the test ran at every launch. The rating goes in beside it now, and
	// so does the dialog's save, with the line kept as it was. The line sits in a
	// theme, where the reload's backfill adds nothing that could give it a level.
	// Test ID: EpXN9p7
	#[test]
	fn a_rating_is_kept_beside_an_unreadable_line() {
		const ID: &str = "0123456789abcdef";
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratinglost_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		let text = "themes:\n\tone:\n\t\t\tname: One\n\t\tstray: 1\n\nperformance:\n\t# profile: \"max\"  ## Default\n\t# rated_hardware: \"\"  ## Default\n";
		assert_eq!(
			shcl::Document::parse(text).lost_count(),
			1,
			"the line stepping back to no level is the one the parse drops"
		);
		std::fs::write(&path, text).unwrap();
		set_config_override(path.clone());

		let lines = RatingLines {
			profile: Some("high"),
			rated_hardware: Some(ID),
			check_next_run: None,
		};
		assert_eq!(keep_rating(&lines), Kept::Written);
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			"themes:\n\tone:\n\t\t\tname: One\n\t\tstray: 1\n\nperformance:\n\t# profile: \"max\"  ## Default\n\tprofile: high\n\t# rated_hardware: \"\"  ## Default\n\trated_hardware: 0123456789abcdef\n",
			"the unreadable line and its neighbours are as they were"
		);
		let reloaded = reload_from_disk();
		assert_eq!(reloaded.rated_hardware, ID);
		assert_eq!(reloaded.performance_profile, "high");

		let mut next = reloaded.clone();
		next.rated_hardware = "fedcba9876543210".to_string();
		assert!(persist(&reloaded, &next), "the dialog's save goes through");
		let after = std::fs::read_to_string(&path).unwrap();
		assert!(
			after.starts_with("themes:\n\tone:\n\t\t\tname: One\n\t\tstray: 1\n"),
			"the unreadable line is as it was\n{after}"
		);
		assert_eq!(reload_from_disk().rated_hardware, "fedcba9876543210");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Every launch-time write defers to a holder, and so does a rating, which then
	// has to say so rather than report a write.
	// Test ID: EpXN9p8
	#[cfg(target_os = "linux")]
	#[test]
	fn a_held_settings_file_keeps_no_rating() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratingheld_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		let text = "performance:\n\trated_hardware: 0000000000000000\n";
		std::fs::write(&path, text).unwrap();
		set_config_override(path.clone());
		let lines = RatingLines {
			rated_hardware: Some("0123456789abcdef"),
			..RatingLines::default()
		};

		let hold = std::fs::File::open(&path).unwrap();
		let mut child = std::process::Command::new("sleep")
			.arg("30")
			.stdin(std::process::Stdio::from(hold))
			.spawn()
			.unwrap();
		let seen = (0..50).any(|_| {
			std::thread::sleep(std::time::Duration::from_millis(20));
			config_open_elsewhere(&path)
		});
		let held = keep_rating(&lines);
		let bytes = std::fs::read_to_string(&path).unwrap();
		let _ = child.kill();
		let _ = child.wait();
		assert!(seen, "the holder never showed up");
		assert_eq!(held, Kept::Busy);
		assert_eq!(bytes, text, "nothing is written while it is held");

		assert_eq!(keep_rating(&lines), Kept::Written);
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			"performance:\n\trated_hardware: 0123456789abcdef\n"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A rename publishes a new file, so a rating written that way replaced a linked
	// settings file with a plain copy and reset a private one's mode. The dialog's
	// save never did either.
	// Test ID: EpXatHM
	#[cfg(unix)]
	#[test]
	fn a_rating_keeps_a_linked_private_settings_file() {
		use std::os::unix::fs::PermissionsExt;
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratinglink_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let real = dir.join("real.shcl");
		std::fs::write(&real, "performance:\n\trated_hardware: 0000000000000000\n").unwrap();
		std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
		let link = dir.join("config.shcl");
		std::os::unix::fs::symlink(&real, &link).unwrap();
		set_config_override(link.clone());

		let lines = RatingLines {
			rated_hardware: Some("0123456789abcdef"),
			..RatingLines::default()
		};
		assert_eq!(keep_rating(&lines), Kept::Written);
		let meta = std::fs::symlink_metadata(&link).unwrap();
		assert!(
			meta.file_type().is_symlink(),
			"the settings file is still a link"
		);
		assert_eq!(std::fs::read_link(&link).unwrap(), real);
		assert_eq!(
			std::fs::read_to_string(&real).unwrap(),
			"performance:\n\trated_hardware: 0123456789abcdef\n",
			"the linked file holds the rating"
		);
		let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
		assert_eq!(mode, 0o600, "the file stays private");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Adding missing settings at launch had the same faults the rating write had,
	// and so did the template write and the renames beside it.
	// Test ID: EpyTXE0
	#[cfg(unix)]
	#[test]
	fn a_launch_rewrite_keeps_a_linked_private_settings_file() {
		use std::os::unix::fs::PermissionsExt;
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_launchlink_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let real = dir.join("real.shcl");
		std::fs::write(&real, "font:\n\tsize: 12\n").unwrap();
		std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
		let link = dir.join("config.shcl");
		std::os::unix::fs::symlink(&real, &link).unwrap();

		backfill_config(&link);
		let meta = std::fs::symlink_metadata(&link).unwrap();
		assert!(
			meta.file_type().is_symlink(),
			"the settings file is still a link"
		);
		let text = std::fs::read_to_string(&real).unwrap();
		assert!(
			text.contains("\tsize: 12") && text.lines().count() > 2,
			"the linked file got the missing settings: {text}"
		);
		let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
		assert_eq!(mode, 0o600, "the file stays private");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EpyTXE1
	#[cfg(unix)]
	#[test]
	fn a_launch_rewrite_writes_through_no_link_left_at_a_temp_name() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_launchplant_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(&path, "font:\n\tsize: 12\n").unwrap();
		let victim = dir.join("victim.txt");
		std::fs::write(&victim, "untouched\n").unwrap();
		// the name launch rewrites used to write, and the first one shcl's writer tries
		for name in [
			"config.shcl.new".to_string(),
			format!(".config.shcl.tmp{}.0", std::process::id()),
		] {
			std::os::unix::fs::symlink(&victim, dir.join(name)).unwrap();
		}

		backfill_config(&path);
		assert_eq!(
			std::fs::read_to_string(&victim).unwrap(),
			"untouched\n",
			"a link at a temp name is not written through"
		);
		let meta = std::fs::symlink_metadata(&path).unwrap();
		assert!(
			meta.file_type().is_file(),
			"the settings file is a plain file"
		);
		let text = std::fs::read_to_string(&path).unwrap();
		assert!(
			text.contains("\tsize: 12") && text.lines().count() > 2,
			"the missing settings were added: {text}"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The temp file's name is predictable, so something already sitting there must
	// never be written through: a link to another file would get the settings text.
	// Test ID: EpXatHN
	#[cfg(unix)]
	#[test]
	fn a_rating_writes_through_no_link_left_at_a_temp_name() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratingplant_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(&path, "performance:\n\trated_hardware: 0000000000000000\n").unwrap();
		let victim = dir.join("victim.txt");
		std::fs::write(&victim, "untouched\n").unwrap();
		// the name the rating used to write, and the first one shcl's writer tries
		for name in [
			"config.shcl.new".to_string(),
			format!(".config.shcl.tmp{}.0", std::process::id()),
		] {
			std::os::unix::fs::symlink(&victim, dir.join(name)).unwrap();
		}
		set_config_override(path.clone());

		let lines = RatingLines {
			rated_hardware: Some("0123456789abcdef"),
			..RatingLines::default()
		};
		assert_eq!(keep_rating(&lines), Kept::Written);
		assert_eq!(
			std::fs::read_to_string(&victim).unwrap(),
			"untouched\n",
			"a link at a temp name is not written through"
		);
		let meta = std::fs::symlink_metadata(&path).unwrap();
		assert!(
			meta.file_type().is_file(),
			"the settings file is a plain file"
		);
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			"performance:\n\trated_hardware: 0123456789abcdef\n"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// One unusable line must cost only its own setting, never the whole file. This
	// used to need a hand-rolled retry loop (blank the offending line, reparse);
	// the parser is forgiving now, so the guarantee has to be re-proven here.
	// Test ID: ElcTgW8
	#[test]
	fn a_bad_line_drops_only_its_own_setting() {
		let p = std::path::Path::new("test.shcl");
		let s = resolve(
			read_raw(
				"scroll.scrollback: 4242\nwindow.margin: not-a-number\ncolors.focus: \"#abcdef\"\n",
				p,
			)
			.0,
		);
		assert_eq!(s.scrollback, 4242, "settings before the bad line survive");
		assert_eq!(s.focus, [0xab, 0xcd, 0xef], "and settings after it");
		assert_eq!(
			s.margin,
			Settings::default().margin,
			"the unusable one falls back to its default"
		);
	}

	// Clearing the Family box wrote nothing at all, so the old line survived and
	// the font came back next launch.
	// Test ID: EpHR81A
	#[test]
	fn clearing_the_font_family_takes_the_old_line_out() {
		let set = Settings {
			font_family: Some("Iosevka".to_string()),
			..Default::default()
		};
		let mut none = set.clone();
		none.font_family = None;
		assert_eq!(cleared_keys(&set, &none), vec!["font.family"]);
		// setting one, or leaving it alone, is an ordinary write
		assert!(cleared_keys(&none, &set).is_empty());
		assert!(cleared_keys(&set, &set).is_empty());
	}

	// Reverting used to remove the node, and shcl takes a node's leading comments
	// with it - so a scrim setting destroyed seven lines of documentation, and a
	// note written above a value went the same way.
	// Test ID: EpHPcsz
	#[test]
	fn reverting_a_setting_keeps_the_comments_above_it() {
		let text = "text:\n\n\t## A blurred patch of background color behind each letter, so text stays\n\t## readable over a wallpaper.\n\tscrim:\n\t\t## mine: I like it stronger\n\t\tstrength: 40\n";
		let out = reverted_text(text, &["text.scrim.strength"]).expect("something to write");
		assert!(
			out.contains("## A blurred patch of background color"),
			"the template's comments survive: {out:?}"
		);
		assert!(
			out.contains("## mine: I like it stronger"),
			"and so does a note written by hand: {out:?}"
		);
		assert!(
			!out.contains("strength: 40"),
			"the value itself is gone: {out:?}"
		);
		// back to how the template ships it, at the file's own indentation
		assert!(
			out.contains("\t\t# strength: 20  ## Default"),
			"the template line went back: {out:?}"
		);
		// and a second revert of the same key has nothing left to do
		assert!(reverted_text(&out, &["text.scrim.strength"]).is_none());
	}

	// Every numeric setting, at both extremes, in one place. Floors were there
	// already; ceilings were not, and a value in the low thousands aborted the
	// launch on a texture limit while a large scrollback grew until the process
	// was killed. `1e400` is here because shcl reads it as infinity and reports
	// it good, and infinity survives a clamp.
	// Test ID: EpHOzrc
	#[test]
	fn every_numeric_setting_has_a_floor_and_a_ceiling() {
		let p = std::path::Path::new("test.shcl");
		#[rustfmt::skip]
		let keys: &[(&str, f32, f32)] = &[
			("font.size",                        limits::FONT_SIZE.0,   limits::FONT_SIZE.1),
			("font.line_height_scale",           limits::LINE_HEIGHT.0, limits::LINE_HEIGHT.1),
			("scroll.wheel_lines",               limits::WHEEL_LINES.0, limits::WHEEL_LINES.1),
			("scroll.alt_scroll_lines",          limits::WHEEL_LINES.0, limits::WHEEL_LINES.1),
			("scroll.ease_in_ms",                limits::EASE_MS.0,     limits::EASE_MS.1),
			("scroll.ramp_up_ms",                limits::EASE_MS.0,     limits::EASE_MS.1),
			("scroll.single_screen_tau_ms",      limits::EASE_MS.0,     limits::EASE_MS.1),
			("scroll.ramp_down_ms",              limits::EASE_MS.0,     limits::EASE_MS.1),
			("scroll.ease_out_ms",               limits::EASE_MS.0,     limits::EASE_MS.1),
			("window.margin",                    limits::MARGIN.0,      limits::MARGIN.1),
			("cursor.blink_rate_ms",             limits::BLINK_MS.0,    limits::BLINK_MS.1),
			("wallpaper.rotate.interval_s",      limits::ROTATE_S.0,    limits::ROTATE_S.1),
		];
		let read = |key: &str, value: &str| resolve(read_raw(&format!("{key}: {value}\n"), p).0);
		let of = |s: &Settings, key: &str| -> f32 {
			match key {
				"font.size" => s.font_size,
				"font.line_height_scale" => s.line_height_scale,
				"scroll.wheel_lines" => s.wheel_lines,
				"scroll.alt_scroll_lines" => s.alt_scroll_lines,
				"scroll.ease_in_ms" => s.scroll_ease_in_ms,
				"scroll.ramp_up_ms" => s.scroll_ramp_up_ms,
				"scroll.single_screen_tau_ms" => s.scroll_single_screen_tau_ms,
				"scroll.ramp_down_ms" => s.scroll_ramp_down_ms,
				"scroll.ease_out_ms" => s.scroll_ease_out_ms,
				"window.margin" => s.margin,
				"cursor.blink_rate_ms" => s.cursor_blink_rate_ms,
				"wallpaper.rotate.interval_s" => s.wallpaper_rotate_interval_s,
				other => panic!("{other} is not in the reader"),
			}
		};
		for &(key, lo, hi) in keys {
			for value in ["1e30", "1e400", "-1e30", "-1e400", "0"] {
				let got = of(&read(key, value), key);
				assert!(got.is_finite(), "{key} at {value} resolved to {got}");
				assert!(got >= lo && got <= hi, "{key} at {value} resolved to {got}");
			}
		}

		// the integers, each against its own limit
		let huge = "99999999";
		for (key, (lo, hi)) in [
			("window.columns", limits::GRID),
			("window.rows", limits::GRID),
			("window.remembered_columns", limits::GRID),
			("window.remembered_rows", limits::GRID),
			("window.idle_release_minimized_min", limits::IDLE_MIN),
			("window.idle_release_hidden_min", limits::IDLE_MIN),
			("window.idle_release_min", limits::IDLE_MIN),
		] {
			let s = read(key, huge);
			let got = match key {
				"window.columns" => s.columns,
				"window.rows" => s.rows,
				"window.remembered_columns" => s.remembered_columns,
				"window.remembered_rows" => s.remembered_rows,
				"window.idle_release_minimized_min" => s.idle_release_minimized_min,
				"window.idle_release_hidden_min" => s.idle_release_hidden_min,
				"window.idle_release_min" => s.idle_release_min,
				other => panic!("{other} is not in the reader"),
			};
			assert!((lo..=hi).contains(&got), "{key} resolved to {got}");
		}
		assert!(
			read("scroll.scrollback", huge).scrollback <= limits::SCROLLBACK.1,
			"an unbounded scrollback grows until the process is killed"
		);
	}

	// All three of these were silent, and the file looks perfectly fine while the
	// setting does nothing. The first one also stops every future save.
	// An unquoted color was read as empty and the theme's color used, with no word
	// of it anywhere.
	// Test ID: ErCYi4K
	#[test]
	fn an_unquoted_color_is_named() {
		let said = config_complaints("colors:\n\tbackground: #112233\n\tcursor: #abc  # mine\n");
		assert_eq!(said.len(), 2, "{said:?}");
		assert!(said[0].contains("colors.background"), "{said:?}");
		assert!(said[0].contains("line 2"), "{said:?}");
		assert!(said[0].contains("\"#112233\""), "{said:?}");
		assert!(said[1].contains("\"#abc\""), "{said:?}");
		assert!(config_complaints("colors:\n\tbackground: \"#112233\"\n").is_empty());
		// a heading with a comment after it is still a heading
		assert!(
			config_complaints("colors:  # the palette\n\tbackground: \"#112233\"\n").is_empty()
		);
		assert_eq!(unquoted_color("x: #aabbccdd"), Some("#aabbccdd"));
		assert_eq!(unquoted_color("x: #abcd"), None);
		assert_eq!(unquoted_color("x: #1234567g"), None);
		assert_eq!(unquoted_color("x:"), None);
	}

	// Test ID: EpHb2Tw
	#[test]
	fn a_config_says_what_is_wrong_with_it() {
		// a key set twice
		let twice =
			config_complaints("font:\n\tfamily: \"One\"\n\tsize: 13.0\n\tfamily: \"Two\"\n");
		assert_eq!(twice.len(), 1, "{twice:?}");
		assert!(twice[0].contains("font.family"), "{twice:?}");
		assert!(twice[0].contains("lines 2, 4"), "{twice:?}");

		// a key nothing reads
		let typo = config_complaints("font:\n\tfamly: \"One\"\n");
		assert_eq!(typo.len(), 1, "{typo:?}");
		assert!(typo[0].contains("font.famly"), "{typo:?}");

		// a monitor's size is read, and a misspelled field in one is not
		let kept =
			"window:\n\tmonitors:\n\t\t1920x1080_100pct:\n\t\t\tcolumns: 90\n\t\t\trows: 30\n";
		assert!(
			config_complaints(kept).is_empty(),
			"{:?}",
			config_complaints(kept)
		);
		let typo = config_complaints(&kept.replace("rows:", "rowz:"));
		assert_eq!(typo.len(), 1, "{typo:?}");
		assert!(
			typo[0].contains("window.monitors.1920x1080_100pct.rowz"),
			"{typo:?}"
		);

		// a line the parser had to drop, which a save writes back as it was
		let lost = config_complaints("font:\n\t\tsize: 13.0\n\tfamily: \"One\"\n");
		assert!(
			lost.iter()
				.any(|m| m.contains("line 3") && m.contains("set nothing")),
			"{lost:?}"
		);

		// the shipped template says nothing, and neither does a config full of
		// the user's own shells and themes
		assert!(config_complaints(default_config()).is_empty());
		let mine = "shells:\n\tmine:\n\t\ttitle: Mine\n\t\tcommand: /bin/sh\nthemes:\n\tone:\n\t\tname: One\n";
		assert!(
			config_complaints(mine).is_empty(),
			"{:?}",
			config_complaints(mine)
		);
	}

	// Saved themes are written and read under `themes.`, so a file holding one has
	// nothing in it to complain about. A real typo beside it still gets a line.
	// Test ID: Epz2LOS
	#[test]
	fn a_saved_theme_is_not_taken_for_a_typo() {
		let pal = crate::theme::resolve_in(&[], "SilkTerm", "dark", true);
		let theme = crate::theme::UserTheme {
			slug: "mine".to_string(),
			name: "Mine".to_string(),
			dark: pal,
			light: pal,
		};
		let mut doc = shcl::Document::parse(default_config());
		write_user_themes(&mut doc, &[], &[theme]);
		let text = doc.to_canonical();
		assert_eq!(read_user_themes(&shcl::Document::parse(&text)).len(), 1);
		assert!(
			config_complaints(&text).is_empty(),
			"{:?}",
			config_complaints(&text)
		);
		let typo = format!("{text}\ntheme_mdoe: dark\n");
		assert!(
			config_complaints(&typo)
				.iter()
				.any(|m| m.contains("theme_mdoe")),
			"{:?}",
			config_complaints(&typo)
		);
	}

	// A color that is not the theme's own is an override, wherever it came from,
	// and the system switching between dark and light must not take it away.
	// Test ID: Epz2LOT
	#[test]
	fn a_color_override_survives_the_system_switching_modes() {
		let _store = test_store_lock();
		let before = settings();
		let was_dark = OS_DARK.load(Ordering::Relaxed);
		OS_DARK.store(true, Ordering::Relaxed);
		let mut s = resolve(
			read_raw(
				"theme_mode: system\ncolors.background: \"#123456\"\n",
				std::path::Path::new("test.shcl"),
			)
			.0,
		);
		// one from the command line, which the file never holds
		s.fg = [1, 2, 3];
		update(s);
		for dark in [false, true, false] {
			assert!(reapply_for_os(dark));
			let live = settings();
			let pal = crate::theme::resolve_in(&live.user_themes, &live.theme, "system", dark);
			assert_eq!(live.bg, [0x12, 0x34, 0x56], "dark {dark}");
			assert_eq!(live.fg, [1, 2, 3], "dark {dark}");
			assert_eq!(live.dialog_bg, pal.dialog_bg, "dark {dark}");
			assert_eq!(live.ansi, pal.ansi, "dark {dark}");
		}
		OS_DARK.store(was_dark, Ordering::Relaxed);
		update((*before).clone());
	}

	// A key written twice cannot resolve to one value, so the default takes
	// effect. That is the right outcome, but it has to be SAID - the setting is
	// there in the file, plainly set, and doing nothing.
	// Test ID: Em2JGe0
	#[test]
	fn a_setting_written_twice_falls_back_and_is_reported() {
		let p = std::path::Path::new("test.shcl");
		let s = resolve(
			read_raw(
				"font:\n\tfamily: \"One\"\n\tsize: 13.0\n\tfamily: \"Two\"\n",
				p,
			)
			.0,
		);
		// as good as absent: neither spelling wins
		let absent = resolve(read_raw("font:\n\tsize: 13.0\n", p).0);
		assert_eq!(
			s.font_family, absent.font_family,
			"a repeated key falls back as if it were not there"
		);
		assert_eq!(s.font_size, 13.0, "its siblings are unaffected");
		// the message is what makes the fallback discoverable; both lines cited
		assert_eq!(line_list(&[2, 4]), " lines 2, 4");
		assert_eq!(line_list(&[7]), " line 7");
		assert_eq!(line_list(&[0]), "", "an uncitable node adds nothing");
	}

	// Test ID: ElcTgW9
	#[test]
	fn default_config_is_valid_shcl() {
		let doc = shcl::Document::parse(default_config());
		let errors: Vec<_> = doc
			.diagnostics()
			.iter()
			.filter(|d| matches!(d.severity, shcl::Severity::Error))
			.collect();
		assert!(
			errors.is_empty(),
			"the shipped template has errors: {errors:?}"
		);
	}

	// The shipped template must already be what a save would produce, or the very
	// first save would reflow the file we just wrote. Nearly every setting here is
	// a commented default with no active sibling - the shape that shcl used to
	// re-pad to the block header's depth - so this is now the guard on the writer
	// itself, and a bump that reintroduced the reflow would fail here.
	// Test ID: ElcTgWA
	#[test]
	fn default_config_survives_a_save_unchanged() {
		let doc = shcl::Document::parse(default_config());
		assert_eq!(
			doc.to_canonical(),
			default_config(),
			"a save would rewrite the shipped template"
		);
	}

	// The remembered size is live from the first write, never a commented
	// default, since the window rewrites it on every resize.
	// Test ID: Er2UFeN
	#[test]
	fn the_template_carries_the_remembered_size_as_live_lines() {
		for want in [
			"window.remembered_columns",
			"window.remembered_rows",
			"window.remembered_font_zoom",
			"window.remembered_maximized",
		] {
			let active = walk_settings(default_config())
				.into_iter()
				.find_map(|w| match w {
					WalkLine::Setting { path, active, .. } if path == want => Some(active),
					_ => None,
				});
			assert_eq!(active, Some(true), "{want} is not a live line");
		}
		let s = resolve(read_raw(default_config(), std::path::Path::new("test.shcl")).0);
		let d = Settings::default();
		assert_eq!(
			(
				s.remembered_columns,
				s.remembered_rows,
				s.remembered_font_zoom,
				s.remembered_maximized
			),
			(
				d.remembered_columns,
				d.remembered_rows,
				d.remembered_font_zoom,
				d.remembered_maximized
			)
		);
	}

	// A size or zoom set by hand is the last one anywhere, and with
	// remember_size and remember_per_monitor on, that monitor's own too.
	// Opening on a monitor with none of its own takes the last anywhere.
	// Test ID: EreYcuQ
	#[test]
	fn a_size_set_by_hand_is_kept_for_its_monitor_and_found_again_there() {
		let mut s = Settings::default();
		let (a, b) = (Some("2560x1440_125pct_597x336mm"), Some("1920x1080_100pct"));
		let kept = |columns, rows, font_zoom| KeptWindow {
			columns,
			rows,
			font_zoom,
		};
		remember_window(&mut s, a, Some((200, 60)), Some(3));
		remember_window(&mut s, b, Some((100, 30)), Some(0));
		assert_eq!(remembered_window(&s, a), kept(200, 60, 3));
		assert_eq!(remembered_window(&s, b), kept(100, 30, 0));
		let last = kept(100, 30, 0);
		assert_eq!(
			remembered_window(&s, Some("3840x2160_150pct")),
			last,
			"no entry"
		);
		assert_eq!(remembered_window(&s, None), last, "monitor unknown");
		remember_window(&mut s, a, Some((210, 61)), Some(3));
		assert_eq!(
			s.monitor_sizes.len(),
			2,
			"an entry is updated, not added again"
		);
		assert_eq!(remembered_window(&s, a), kept(210, 61, 3));

		// a zoom alone, as in a maximized window, leaves the size as it was;
		// a monitor seen first that way starts from the last size anywhere
		remember_window(&mut s, a, None, Some(-2));
		assert_eq!(remembered_window(&s, a), kept(210, 61, -2));
		remember_window(&mut s, Some("800x600_100pct"), None, Some(1));
		assert_eq!(
			remembered_window(&s, Some("800x600_100pct")),
			kept(210, 61, 1)
		);
		// a font size from the command line is not a zoom to keep
		remember_window(&mut s, a, Some((150, 40)), None);
		assert_eq!(remembered_window(&s, a), kept(150, 40, -2));

		// switched off, a monitor's entry is neither used nor written
		s.remember_per_monitor = false;
		assert_eq!(remembered_window(&s, b), kept(150, 40, 1));
		remember_window(&mut s, b, Some((90, 25)), Some(4));
		s.remember_per_monitor = true;
		assert_eq!(remembered_window(&s, b), kept(100, 30, 0));
		// and nothing is kept per monitor while remember_size is off
		s.remember_size = false;
		remember_window(&mut s, Some("640x480_100pct"), Some((80, 24)), Some(0));
		assert_eq!(s.monitor_sizes.len(), 3);
		assert_eq!(
			(
				s.remembered_columns,
				s.remembered_rows,
				s.remembered_font_zoom
			),
			(80, 24, 0)
		);
	}

	// The per-monitor sizes read back from the file as written, and a write
	// touches only the numbers this window changed, so another window's entry,
	// or its change to a shared one, stays.
	// Test ID: EreYcuR
	#[test]
	fn monitor_sizes_round_trip_and_leave_another_windows_alone() {
		let p = std::path::Path::new("test.shcl");
		let entry = |key: &str, columns, rows, font_zoom| MonitorSize {
			key: key.into(),
			columns,
			rows,
			font_zoom,
		};
		// another window kept B and changed A's rows since this one loaded
		let mut doc = shcl::Document::parse(
			"window:\n\tmonitors:\n\t\ta_100pct:\n\t\t\tcolumns: 100\n\t\t\trows: 41\n\t\t\tfont_zoom: 0\n\t\tb_100pct:\n\t\t\tcolumns: 70\n\t\t\trows: 20\n\t\t\tfont_zoom: -1\n",
		);
		let loaded = vec![entry("a_100pct", 100, 40, 0)];
		let mine = vec![entry("a_100pct", 120, 40, 2), entry("c_150pct", 90, 30, 0)];
		write_monitor_sizes(&mut doc, &loaded, &mine);
		let back = resolve(read_raw(&doc.to_canonical(), p).0).monitor_sizes;
		assert_eq!(
			back,
			vec![
				entry("a_100pct", 120, 41, 2),
				entry("b_100pct", 70, 20, -1),
				entry("c_150pct", 90, 30, 0)
			]
		);
		// a written file is a fixed point: nothing changed, nothing written
		let text = doc.to_canonical();
		write_monitor_sizes(&mut doc, &back, &back);
		assert_eq!(doc.to_canonical(), text);

		// Names are folded to lower case, which is how the keys are written.
		// A number out of range is held to its range, a missing one takes the
		// default, and an entry with nothing readable is no entry.
		let odd = "window:\n\tmonitors:\n\t\tX:\n\t\t\tcolumns: 0\n\t\t\tfont_zoom: 9999\n\t\ty:\n\t\t\tnote: 1\n";
		let back = resolve(read_raw(odd, p).0).monitor_sizes;
		let d = Settings::default();
		assert_eq!(
			back,
			vec![entry("x", 1, d.remembered_rows, limits::FONT_ZOOM.1)]
		);
	}

	// What another window kept is read back the way a launch reads it.
	// Test ID: EreYcuT
	#[test]
	fn a_window_reads_back_the_sizes_another_window_kept() {
		let p = std::path::Path::new("test.shcl");
		let text = default_config()
			.replace("remembered_columns: 160", "remembered_columns: 132")
			.replace("remembered_font_zoom: 0", "remembered_font_zoom: -3")
			.replace(
				"\t# remember_maximized:",
				"\tmonitors:\n\t\t1920x1080_100pct:\n\t\t\tcolumns: 90\n\t\t\trows: 30\n\t\t\tfont_zoom: 2\n\n\t# remember_maximized:",
			);
		let launch = resolve(read_raw(&text, p).0);
		let mut live = Settings::default();
		window_memory_from(&text, p, &mut live);
		assert_eq!(
			(live.remembered_columns, live.remembered_font_zoom),
			(132, -3)
		);
		assert_eq!(live.monitor_sizes.len(), 1);
		assert_eq!(
			(
				live.remembered_columns,
				live.remembered_rows,
				live.remembered_font_zoom,
				&live.monitor_sizes
			),
			(
				launch.remembered_columns,
				launch.remembered_rows,
				launch.remembered_font_zoom,
				&launch.monitor_sizes
			)
		);
	}

	// A file from before remember_per_monitor gets it as its own paragraph,
	// comment and all, under the remembered size and above remember_maximized.
	// Test ID: EreYcuS
	#[test]
	fn an_older_file_gets_the_per_monitor_switch_beside_the_size_it_follows() {
		let old = default_config().replace(
			&default_config()[default_config()
				.find("\t## While remember_size is on")
				.unwrap()
				..default_config().find("\t# remember_maximized").unwrap()],
			"",
		);
		assert!(!old.contains("remember_per_monitor"));
		let new = backfilled_text(&old).unwrap().unwrap();
		let at = |text: &str| new.find(text).unwrap();
		assert!(at("remembered_rows: 48") < at("\t## While remember_size is on"));
		assert!(at("# remember_per_monitor: true  ## Default") < at("# remember_maximized"));
		assert_eq!(new.matches("remember_per_monitor").count(), 1, "{new}");
		assert_eq!(
			new.matches("## While remember_size is on").count(),
			1,
			"{new}"
		);
	}

	// A new file follows the Settings dialog's tabs: Background, Text, Cursor,
	// Movement, Themes, Window, Shell. The Silk tab borrows rows from the others,
	// so its performance block leads and the rest keep their home tab's place.
	// The hotkeys have no tab yet (2026100307252506) and sit before Shell.
	// Test ID: Er2UFeO
	#[test]
	fn the_template_blocks_follow_the_dialog() {
		let mut roots: Vec<&str> = Vec::new();
		for line in default_config().lines() {
			if line.starts_with(|c: char| c.is_whitespace() || c == '#') {
				continue;
			}
			let Some((key, _)) = line.split_once(':') else {
				continue;
			};
			if roots.last() != Some(&key) {
				roots.push(key);
			}
		}
		assert_eq!(
			roots,
			[
				"performance",
				"transparency",
				"wallpaper",
				"font",
				"text",
				"cursor",
				"selection",
				"scroll",
				"theme",
				"theme_mode",
				"colors",
				"window",
				"hyperlinks",
				"keys",
				"shell",
			]
		);
	}

	// The template's own header says a line starting with '# ' is a setting at
	// its default, so removing the '# ' must change nothing. Seven lines named
	// an example instead, and uncommenting any of them quietly changed what
	// loaded. This reads the template, so a line added later is checked too.
	// Test ID: Eq4CjrE
	#[test]
	fn every_commented_default_line_loads_as_the_default() {
		// resolve() hunts for a wallpaper folder under the config and data dirs,
		// so a test that points those elsewhere mid-loop would move the answer
		// between the two loads being compared.
		let _guard = super::test_config_lock();
		let path = std::path::Path::new("test.shcl");
		let base = resolve(read_raw(default_config(), path).0);
		let lines: Vec<&str> = default_config().lines().collect();
		let mut checked = 0;
		for w in walk_settings(default_config()) {
			let WalkLine::Setting {
				index,
				path: key,
				active,
				header,
			} = w
			else {
				continue;
			};
			if active || header || !lines[index].contains("## Default") {
				continue;
			}
			// font.size is the one line that cannot say it. A config older than
			// the family/size split named a size to mean "not the system one",
			// so resolve() still reads the key's presence that way.
			if key == "font.size" {
				continue;
			}
			let bare = {
				let indent = lines[index].len() - lines[index].trim_start().len();
				let rest = lines[index]
					.trim_start()
					.trim_start_matches('#')
					.trim_start();
				format!("{}{rest}", &lines[index][..indent])
			};
			let mut edited = lines.clone();
			edited[index] = &bare;
			let text = edited.join("\n") + "\n";
			let mut loaded = resolve(read_raw(&text, path).0);
			// A hotkey uncommented is set in the file, which Settings shows on its
			// Keys tab, but it answers to the same chords.
			for (hotkey, _) in crate::keys::config_paths() {
				assert_eq!(
					loaded.keys.chords(hotkey),
					base.keys.chords(hotkey),
					"uncommenting `{}` changes what {hotkey:?} answers to",
					lines[index].trim()
				);
			}
			loaded.keys = base.keys.clone();
			assert!(
				loaded == base,
				"uncommenting `{}` changes what loads",
				lines[index].trim()
			);
			checked += 1;
		}
		// the walk finding nothing would pass the loop in silence
		assert!(
			checked > 50,
			"only {checked} commented defaults were checked"
		);
	}

	// The template is where the footer actually reaches a new file; the const is
	// what puts it back on an old one. Nothing else keeps the two in step.
	// Test ID: Ep6mucC
	#[test]
	fn the_template_ends_with_the_banner() {
		assert!(default_config().ends_with(SHCL_BANNER));
		assert!(
			with_shcl_banner(default_config()).is_none(),
			"the shipped template should need no footer work"
		);
	}

	// Test ID: Ep6mucD
	#[test]
	fn the_banner_is_retrofitted_and_kept_last() {
		let plain = "font:\n\tsize: 13.0\n";
		let added = with_shcl_banner(plain).expect("a config without one gets it");
		assert!(added.starts_with(plain) && added.ends_with(SHCL_BANNER));
		assert_eq!(added, plain.to_string() + "\n" + SHCL_BANNER);
		assert!(with_shcl_banner(&added).is_none(), "not idempotent");

		// the single-'#' spelling is replaced, not duplicated
		let old = format!("font:\n\tsize: 13.0\n\n{SHCL_BANNER_OLD}");
		let fixed = with_shcl_banner(&old).expect("the old spelling is refreshed");
		assert_eq!(fixed, added);
		assert_eq!(fixed.matches(SHCL_BANNER_MARK).count(), 1);

		// and the one from before it had a Format line
		let main = format!("font:\n\tsize: 13.0\n\n{SHCL_BANNER_OLD_MAIN}");
		assert_eq!(with_shcl_banner(&main).as_deref(), Some(added.as_str()));

		// so is the one with the old shcl home
		let moved_home = format!("font:\n\tsize: 13.0\n\n{SHCL_BANNER_OLD_HOME}");
		assert_eq!(
			with_shcl_banner(&moved_home).as_deref(),
			Some(added.as_str())
		);

		// and the first 3.0 one, whose link went nowhere
		let first_v3 = format!("font:\n\tsize: 13.0\n\n{SHCL_BANNER_OLD_V3}");
		assert_eq!(with_shcl_banner(&first_v3).as_deref(), Some(added.as_str()));

		// backfill appends under it; it goes back to the bottom
		let stranded = format!("{added}\nwindow:\n\tmargin: 8.0\n");
		let moved = with_shcl_banner(&stranded).expect("a stranded footer moves");
		assert!(moved.ends_with(SHCL_BANNER));
		assert!(moved.contains("\tmargin: 8.0\n"));
		assert_eq!(moved.matches(SHCL_BANNER_MARK).count(), 1);
	}

	// Someone who rewrote the footer gets to keep their wording.
	// Test ID: Ep6mucE
	#[test]
	fn an_edited_banner_is_left_alone() {
		let mine = "font:\n\tsize: 13.0\n\n## This config file format is SHCL. Go read the spec.\n";
		assert!(with_shcl_banner(mine).is_none());
	}

	// These are the lines shcl 2.0.0 wrote for a UNC path, a drive path and a
	// tab. Read as they stand, 3.0 doubles the backslashes in the first.
	// Test ID: EqpHg6S
	#[test]
	fn a_file_shcl2_wrote_reads_the_same() {
		let body =
			"shell:\n\tunc: \\\\\\\\server\\\\share\n\tdir: \"C:\\\\Users\\\\new\"\n\ttab: a\\tb\n";
		let wanted = [
			("shell.unc", "\\\\server\\share"),
			("shell.dir", "C:\\Users\\new"),
			("shell.tab", "a\tb"),
		];
		for text in [
			body.to_string(),
			format!("{body}\n{SHCL_BANNER_OLD_MAIN}"),
			format!("{body}\n{SHCL_BANNER_OLD}"),
		] {
			let out = from_shcl2_text(&text).expect("a 2.x file is rewritten");
			let doc = shcl::Document::parse(&out);
			for (path, value) in wanted {
				assert_eq!(doc.get_string(path).as_deref(), Ok(value), "{out}");
			}
			assert!(out.ends_with(SHCL_BANNER), "{out}");
			assert_eq!(out.matches(shcl::FORMAT_LINE_HEAD).count(), 1, "{out}");
			assert!(from_shcl2_text(&out).is_none(), "twice:\n{out}");
			assert!(with_shcl_banner(&out).is_none(), "{out}");
			assert_eq!(next_launch_text(&text), next_launch_text(&out));
		}
		// a footer somebody rewrote still gets marked, just not by ours
		let mine = format!("{body}\n## This config file format is SHCL. Mine.\n");
		let out = from_shcl2_text(&mine).expect("rewritten");
		assert!(out.contains(shcl::FORMAT_LINE) && !out.contains(SHCL_BANNER));
		assert!(from_shcl2_text(&out).is_none());
		assert_eq!(
			shcl::Document::parse(&out)
				.get_string("shell.unc")
				.as_deref(),
			Ok("\\\\server\\share")
		);
		// and the shipped template has nothing to do
		assert!(from_shcl2_text(default_config()).is_none());
	}

	// A 2.x file as a launch finds it: a UNC path the conversion respells.
	const SHCL2_FILE: &str =
		"shell:\n\tunc: \\\\\\\\server\\\\share\n\tdir: \"C:\\\\Users\\\\new\"\n";

	fn format_test_dir(what: &str) -> PathBuf {
		let dir = crate::testdir::run_dir().join(format!("silkterm_{what}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		dir
	}

	fn dir_names(dir: &std::path::Path) -> Vec<String> {
		let mut names: Vec<String> = std::fs::read_dir(dir)
			.unwrap()
			.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}

	// The copies beside a config, by name: everything but the file itself and a
	// temp name still being written.
	fn backups_in(dir: &std::path::Path, stem: &str) -> Vec<String> {
		dir_names(dir)
			.into_iter()
			.filter(|name| name.starts_with(&format!("{stem}_backup_")))
			.collect()
	}

	// `<stem>_backup_YYYYmmDD-HHMMSS_format-v<n>.shcl`, as `backup_name` makes it.
	fn is_backup_name(name: &str, stem: &str, n: u32) -> bool {
		let Some(rest) = name.strip_prefix(&format!("{stem}_backup_")) else {
			return false;
		};
		let Some(stamp) = rest.strip_suffix(&format!("_format-v{n}.shcl")) else {
			return false;
		};
		stamp.len() == 15
			&& stamp.chars().enumerate().all(|(at, c)| {
				if at == 8 {
					c == '-'
				} else {
					c.is_ascii_digit()
				}
			})
	}

	// The launch that converts a 2.x file keeps it, byte for byte, as
	// `config_backup_<time>_format-v2.shcl`, and reads the settings it had. The
	// next launch has nothing to convert and leaves the copy alone.
	// Test ID: EreLZJY
	#[test]
	fn a_launch_keeps_the_2x_file_beside_the_converted_one() {
		let _guard = test_config_lock();
		let dir = format_test_dir("fmtcopy_launch");
		let path = dir.join("config.shcl");
		std::fs::write(&path, SHCL2_FILE).unwrap();
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
		}
		set_config_override(path.clone());

		let _ = load();
		let kept = backups_in(&dir, "config");
		assert_eq!(kept.len(), 1, "{kept:?}");
		assert!(is_backup_name(&kept[0], "config", 2), "{kept:?}");
		let copy = dir.join(&kept[0]);
		assert_eq!(std::fs::read_to_string(&copy).unwrap(), SHCL2_FILE);
		let now = std::fs::read_to_string(&path).unwrap();
		assert_eq!(
			shcl::format_version(&now),
			Some(shcl::FORMAT_MAJOR),
			"{now}"
		);
		assert_eq!(
			shcl::Document::parse(&now)
				.get_string("shell.unc")
				.as_deref(),
			Ok("\\\\server\\share")
		);
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
			assert_eq!(mode & 0o777, 0o600, "the copy is as private as the file");
		}

		let _ = load();
		assert_eq!(std::fs::read_to_string(&path).unwrap(), now, "settled");
		assert_eq!(std::fs::read_to_string(&copy).unwrap(), SHCL2_FILE);
		assert_eq!(dir_names(&dir), ["config.shcl", kept[0].as_str()]);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Every older version is kept. A second conversion makes a second copy and
	// leaves the first as it was, and a current file gets no copy at all.
	// Test ID: EreLZMm
	#[test]
	fn a_second_conversion_keeps_a_second_copy() {
		let dir = format_test_dir("fmtcopy_kept");
		let path = dir.join("config.shcl");
		std::fs::write(&path, SHCL2_FILE).unwrap();
		convert_shcl2_config(&path);
		let first = backups_in(&dir, "config");
		assert_eq!(first.len(), 1, "{first:?}");

		let other = "font:\n\tfamily: \"C:\\\\Fonts\\\\mine.ttf\"\n";
		std::fs::write(&path, other).unwrap();
		convert_shcl2_config(&path);
		let both = backups_in(&dir, "config");
		assert_eq!(both.len(), 2, "{both:?}");
		assert!(both.contains(&first[0]), "{both:?}");
		assert_eq!(
			std::fs::read_to_string(dir.join(&first[0])).unwrap(),
			SHCL2_FILE,
			"the first copy is untouched"
		);
		let second = both.iter().find(|name| **name != first[0]).unwrap();
		assert_eq!(std::fs::read_to_string(dir.join(second)).unwrap(), other);
		assert_eq!(
			shcl::format_version(&std::fs::read_to_string(&path).unwrap()),
			Some(shcl::FORMAT_MAJOR)
		);

		// a save of a current file, and a write over a blank one, keep nothing
		write_config_atomic(&path, default_config()).unwrap();
		std::fs::write(&path, "\n").unwrap();
		write_config_atomic(&path, default_config()).unwrap();
		assert_eq!(backups_in(&dir, "config"), both);

		// a file named by --config keeps its own name, still ending in .shcl
		for (name, stem) in [("mine.shcl", "mine"), ("mine.conf", "mine.conf")] {
			let mine = dir.join(name);
			std::fs::write(&mine, SHCL2_FILE).unwrap();
			convert_shcl2_config(&mine);
			let kept = backups_in(&dir, stem);
			assert_eq!(kept.len(), 1, "{kept:?}");
			assert!(is_backup_name(&kept[0], stem, 2), "{kept:?}");
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Two conversions in the same second never share a name: the second takes
	// `_2`, and a name already holding something else is passed over untouched.
	// The same bytes again are the same copy.
	// Test ID: Erf0QeH
	#[test]
	fn copies_made_in_one_second_never_replace_each_other() {
		let dir = format_test_dir("fmtcopy_second");
		let path = dir.join("config.shcl");
		let stamp = "20261003-142233";
		let keep = |body: &str| {
			std::fs::write(&path, body).unwrap();
			keep_old_format_at(&path, default_config(), stamp)
				.unwrap()
				.unwrap()
				.0
		};
		let first = keep(SHCL2_FILE);
		assert_eq!(
			first,
			dir.join("config_backup_20261003-142233_format-v2.shcl")
		);
		std::fs::write(
			dir.join("config_backup_20261003-142233_format-v2_2.shcl"),
			"planted",
		)
		.unwrap();
		let other = "font:\n\tsize: 9\n";
		let second = keep(other);
		assert_eq!(
			second,
			dir.join("config_backup_20261003-142233_format-v2_3.shcl")
		);
		assert_eq!(std::fs::read_to_string(&first).unwrap(), SHCL2_FILE);
		assert_eq!(
			std::fs::read_to_string(dir.join("config_backup_20261003-142233_format-v2_2.shcl"))
				.unwrap(),
			"planted"
		);
		assert_eq!(std::fs::read_to_string(&second).unwrap(), other);
		assert_eq!(keep(other), second, "the same file again is the same copy");
		assert_eq!(backups_in(&dir, "config").len(), 3);
		assert_eq!(dir_names(&dir).len(), 4, "no temp file is left");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Several windows can launch at once, and each converts the file it read.
	// Every copy that comes of it is whole, they are one copy unless the clock
	// ticked between them, and no temp file is left.
	// Test ID: EreLZQQ
	#[test]
	fn launches_converting_at_once_leave_one_whole_copy() {
		let dir = format_test_dir("fmtcopy_race");
		let path = dir.join("config.shcl");
		// big enough that a copy written in place would be seen half done
		let body = format!(
			"{SHCL2_FILE}{}",
			"## filler line for size\n".repeat(200_000)
		);
		for _round in 0..5 {
			for name in backups_in(&dir, "config") {
				std::fs::remove_file(dir.join(name)).unwrap();
			}
			std::fs::write(&path, &body).unwrap();
			let start = std::sync::Barrier::new(5);
			let torn = std::sync::atomic::AtomicBool::new(false);
			let done = std::sync::atomic::AtomicBool::new(false);
			std::thread::scope(|scope| {
				scope.spawn(|| {
					start.wait();
					// the names are not known ahead, and listing the folder every pass
					// would be too slow to catch a part copy, so each found is read a
					// while before looking again
					while !done.load(std::sync::atomic::Ordering::Relaxed) {
						for name in backups_in(&dir, "config") {
							let copy = dir.join(name);
							for _ in 0..200 {
								if let Ok(seen) = std::fs::read(&copy)
									&& seen != body.as_bytes()
								{
									torn.store(true, std::sync::atomic::Ordering::Relaxed);
								}
							}
						}
					}
				});
				let launches: Vec<_> = (0..4)
					.map(|_| {
						scope.spawn(|| {
							start.wait();
							convert_shcl2_config(&path);
						})
					})
					.collect();
				for launch in launches {
					launch.join().unwrap();
				}
				done.store(true, std::sync::atomic::Ordering::Relaxed);
			});
			assert!(
				!torn.load(std::sync::atomic::Ordering::Relaxed),
				"a part copy was seen"
			);
			let kept = backups_in(&dir, "config");
			assert!((1..=2).contains(&kept.len()), "{kept:?}");
			for name in &kept {
				assert!(is_backup_name(name, "config", 2), "{kept:?}");
				assert_eq!(std::fs::read(dir.join(name)).unwrap(), body.as_bytes());
			}
			assert_eq!(
				dir_names(&dir).len(),
				kept.len() + 1,
				"no temp file is left"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// No file moves to a newer format without its copy: a file that cannot be
	// read to keep is not written over.
	// Test ID: EreLZTl
	#[cfg(unix)]
	#[test]
	fn a_write_that_cannot_keep_the_old_file_is_refused() {
		use std::os::unix::fs::PermissionsExt;
		let dir = format_test_dir("fmtcopy_refused");
		let path = dir.join("config.shcl");
		std::fs::write(&path, SHCL2_FILE).unwrap();
		std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o200)).unwrap();
		let readable = std::fs::read(&path).is_ok();
		let result = write_config_atomic(&path, default_config());
		std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
		if !readable {
			// root reads anything, so this only means something for a user
			assert!(result.is_err(), "written with no copy kept");
			assert_eq!(std::fs::read_to_string(&path).unwrap(), SHCL2_FILE);
			assert_eq!(dir_names(&dir), ["config.shcl"]);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The footer's Format line marks a file converted, so it must not go onto one
	// whose conversion was put off. Its backslashes would then read the 3.0 way
	// and nothing would convert it again.
	// Test ID: EreLZX8
	#[test]
	fn the_footer_never_stamps_a_2x_file() {
		let dir = format_test_dir("fmtcopy_footer");
		let path = dir.join("config.shcl");
		std::fs::write(&path, SHCL2_FILE).unwrap();
		refresh_shcl_banner(&path);
		assert_eq!(std::fs::read_to_string(&path).unwrap(), SHCL2_FILE);
		assert_eq!(dir_names(&dir), ["config.shcl"]);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A line 2.x gave a bracketed list binds nothing after the conversion, and
	// the launch says so, naming the copy that still has it.
	// Test ID: EreLZaX
	#[test]
	fn a_setting_the_conversion_cannot_keep_is_reported() {
		let path = std::path::Path::new("/cfg/config.shcl");
		let copy =
			std::path::Path::new("/cfg").join("config_backup_20261003-142233_format-v2.shcl");
		let text = "font:\n\tfamily:[One, Two]\n\tsize: 13\n";
		let loss = conversion_losses(text.as_bytes(), path, Some(copy.clone())).unwrap();
		assert_eq!(loss.lost, 1);
		let said = loss.terminal_line();
		assert!(said.contains("1 line(s)"), "{said}");
		assert!(said.contains(&copy.display().to_string()), "{said}");
		assert_eq!(
			conversion_losses(SHCL2_FILE.as_bytes(), path, Some(copy)),
			None
		);
		// and every later launch still names the line
		let out = from_shcl2_text(text).unwrap();
		let complaints = config_complaints(&out);
		assert!(
			complaints.iter().any(|c| c.contains("line 2")),
			"{complaints:?}"
		);
	}

	// The launch that loses a setting leaves word for the window, with the count
	// and the copy it really made. A launch that loses nothing leaves none.
	// Test ID: Erf0Qhu
	#[test]
	fn a_launch_that_loses_a_setting_leaves_a_notice_for_the_window() {
		let _guard = test_config_lock();
		let _ = take_conversion_loss();
		let dir = format_test_dir("fmtcopy_notice");
		let path = dir.join("config.shcl");
		let text = "font:\n\tfamily:[One, Two]\n\tsize: 13\n";
		std::fs::write(&path, text).unwrap();
		set_config_override(path.clone());

		let _ = load();
		let loss = take_conversion_loss().expect("a notice is owed");
		assert_eq!(loss.path, path);
		assert_eq!(loss.lost, 1);
		let copy = loss.backup.expect("a copy was kept");
		assert_eq!(copy.parent(), Some(dir.as_path()));
		assert_eq!(std::fs::read_to_string(&copy).unwrap(), text);
		assert_eq!(take_conversion_loss(), None, "taken once");

		std::fs::write(&path, SHCL2_FILE).unwrap();
		let _ = load();
		assert_eq!(take_conversion_loss(), None, "nothing was lost");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A Settings save that converts a file a busy launch left alone says what it
	// lost as the launch does, once, with the count and the copy. A save that
	// converts without losing anything still keeps a copy and says nothing.
	// Test ID: ErfTRqP
	#[test]
	fn a_save_that_loses_a_setting_converting_leaves_a_notice_for_the_window() {
		let _guard = test_config_lock();
		let _ = take_conversion_loss();
		let dir = format_test_dir("fmtcopy_save");
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let orig = Settings::default();
		let mut edited = orig.clone();
		edited.font_size += 1.0;

		let text = "font:\n\tfamily:[One, Two]\n";
		std::fs::write(&path, text).unwrap();
		assert!(persist(&orig, &edited));
		let now = std::fs::read_to_string(&path).unwrap();
		assert_eq!(
			shcl::format_version(&now),
			Some(shcl::FORMAT_MAJOR),
			"{now}"
		);
		let loss = take_conversion_loss().expect("a notice is owed");
		assert_eq!(loss.path, path);
		assert_eq!(loss.lost, 1);
		let copy = loss.backup.expect("a copy was kept");
		assert_eq!(copy.parent(), Some(dir.as_path()));
		assert_eq!(std::fs::read_to_string(&copy).unwrap(), text);
		assert_eq!(take_conversion_loss(), None, "taken once");

		std::fs::write(&path, SHCL2_FILE).unwrap();
		assert!(persist(&orig, &edited));
		assert_eq!(backups_in(&dir, "config").len(), 2, "a copy is still kept");
		assert_eq!(take_conversion_loss(), None, "nothing was lost");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A file shcl can migrate is converted where it stands. One it cannot, here
	// a raw block that never closes, is written new from the template with the
	// settings that still read, and every line it could not carry is counted.
	// Test ID: ErgDo2H
	#[test]
	fn a_file_shcl_cannot_migrate_is_written_new() {
		assert!(
			matches!(
				upgrade(SHCL2_FILE.as_bytes()),
				Upgrade::InPlace { lost: 0, .. }
			),
			"a file shcl can migrate stays where it is"
		);
		let text = "font:\n\tsize: 19\n\tfamily:[One, Two]\nwindow:\n\tcolumns: 103\nnotes: ```\nline one\nwindow.rows: 40\n";
		let Upgrade::Rewritten { text: out, lost } = upgrade(text.as_bytes()) else {
			panic!("not written new");
		};
		assert_eq!(lost, 2, "the list and the raw block");
		let doc = shcl::Document::parse(&out);
		assert_eq!(doc.get_float("font.size"), Ok(19.0), "{out}");
		assert_eq!(doc.get_int("window.columns"), Ok(103), "{out}");
		assert_eq!(shcl::format_version(&out), Some(shcl::FORMAT_MAJOR));
		assert!(out.ends_with(SHCL_BANNER), "{out}");
		assert!(!out.contains("```"), "{out}");
		assert!(config_complaints(&out).is_empty(), "{out}");
		assert_eq!(from_shcl2_text(&out), None, "settled");
		assert_eq!(next_launch_text(text), next_launch_text(&out));
	}

	// A file that is not UTF-8 cannot be read by shcl at all. In an older format
	// it is kept byte for byte and written new, with the settings that still
	// read, and the launch says what it could not carry, as for any conversion.
	// The next launch has nothing to do.
	// Test ID: ErgDoNP
	#[test]
	fn a_file_shcl_cannot_read_is_written_new() {
		let _guard = test_config_lock();
		let _ = take_conversion_loss();
		let dir = format_test_dir("fmtfresh_launch");
		let path = dir.join("config.shcl");
		let body: &[u8] = b"font:\n\tsize: 19\n# caf\xe9\nwindow:\n\tcolumns: 103\nwallpaper:\n\timage: /pics/caf\xe9.jpg\n";
		std::fs::write(&path, body).unwrap();
		set_config_override(path.clone());

		let s = load();
		assert!((s.font_size - 19.0).abs() < f32::EPSILON, "{}", s.font_size);
		assert_eq!(s.columns, 103);
		let kept = backups_in(&dir, "config");
		assert_eq!(kept.len(), 1, "{kept:?}");
		assert!(is_backup_name(&kept[0], "config", 2), "{kept:?}");
		let copy = dir.join(&kept[0]);
		assert_eq!(std::fs::read(&copy).unwrap(), body);
		let now = std::fs::read_to_string(&path).expect("UTF-8 now");
		assert_eq!(
			shcl::format_version(&now),
			Some(shcl::FORMAT_MAJOR),
			"{now}"
		);
		assert!(!now.contains('\u{fffd}'), "{now}");
		let loss = take_conversion_loss().expect("a notice is owed");
		assert_eq!((loss.lost, loss.how), (1, Converted::Rewritten));
		assert_eq!(loss.backup, Some(copy));
		assert!(
			loss.terminal_line().contains("1 setting(s)"),
			"{}",
			loss.terminal_line()
		);

		let _ = load();
		assert_eq!(std::fs::read_to_string(&path).unwrap(), now, "settled");
		assert_eq!(backups_in(&dir, "config").len(), 1);
		assert_eq!(take_conversion_loss(), None);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Off since a current file that is not UTF-8 is written again without those
	// lines (2026100315581313), where it was left alone.
	// `a_current_file_that_is_not_utf8_drops_only_those_lines` covers `upgrade`.
	// // A current file that is not UTF-8 is no upgrade, so it is left as it was.
	// // Test ID: ErgDoiP
	// #[test]
	// fn a_current_file_shcl_cannot_read_is_left_alone() {
	// 	let body = [
	// 		b"font:\n\tsize: 19\n# caf\xe9\n".as_slice(),
	// 		shcl::FORMAT_LINE.as_bytes(),
	// 		b"\n",
	// 	]
	// 	.concat();
	// 	assert!(std::str::from_utf8(&body).is_err());
	// 	assert_eq!(upgrade(&body), Upgrade::Current);
	// 	assert_eq!(
	// 		conversion_losses(&body, std::path::Path::new("/c"), None),
	// 		None
	// 	);
	// }

	// A Settings save that finds an old file shcl cannot read writes it new and
	// keeps the old one, and says what it could not carry as a launch would.
	// Test ID: ErgDp3N
	#[test]
	fn a_save_on_a_file_shcl_cannot_read_writes_it_new() {
		let _guard = test_config_lock();
		let _ = take_conversion_loss();
		let dir = format_test_dir("fmtfresh_save");
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let orig = Settings::default();
		let mut edited = orig.clone();
		edited.font_size += 1.0;

		let body: &[u8] = b"window:\n\tcolumns: 103\n\tti\xe9tle: x\n";
		std::fs::write(&path, body).unwrap();
		assert!(persist(&orig, &edited));
		let now = std::fs::read_to_string(&path).expect("UTF-8 now");
		let doc = shcl::Document::parse(&now);
		assert_eq!(doc.get_int("window.columns"), Ok(103), "{now}");
		assert_eq!(
			doc.get_float("font.size").ok(),
			Some(f64::from(edited.font_size)),
			"{now}"
		);
		let loss = take_conversion_loss().expect("a notice is owed");
		assert_eq!((loss.lost, loss.how), (1, Converted::Rewritten));
		let copy = loss.backup.expect("a copy was kept");
		assert_eq!(std::fs::read(&copy).unwrap(), body);
		let _ = std::fs::remove_dir_all(&dir);
	}

	fn not_utf8_current_file() -> Vec<u8> {
		[
			b"font:\n\tsize: 19\n# caf\xe9\nwindow:\n\tcolumns: 103\n".as_slice(),
			shcl::FORMAT_LINE.as_bytes(),
			b"\n",
		]
		.concat()
	}

	// Off since a current file that is not UTF-8 is written again at launch
	// without those lines, the old bytes kept beside it (2026100315581313), where
	// it was left as it was and every save refused.
	// `a_launch_writes_a_current_file_that_is_not_utf8_again` covers it now.
	// // A current file that is not UTF-8 used to load as all defaults with no
	// // word. It is still left as it was, but every line that decodes is read.
	// // Test ID: ErgK1sy
	// #[test]
	// fn a_current_file_that_is_not_utf8_loads_what_reads() {
	// 	let _guard = test_config_lock();
	// 	let dir = format_test_dir("notutf8_launch");
	// 	let path = dir.join("config.shcl");
	// 	let body = not_utf8_current_file();
	// 	std::fs::write(&path, &body).unwrap();
	// 	set_config_override(path.clone());
	//
	// 	let s = load();
	// 	assert!((s.font_size - 19.0).abs() < f32::EPSILON, "{}", s.font_size);
	// 	assert_eq!(s.columns, 103);
	// 	assert_eq!(std::fs::read(&path).unwrap(), body, "left as it was");
	// 	assert_eq!(dir_names(&dir), vec!["config.shcl".to_string()]);
	// 	let said = LAUNCH_SAID.lock().unwrap().clone().expect("said").1;
	// 	assert!(said.contains(&not_utf8_line(&path, &[3])), "{said:?}");
	// 	assert_eq!(
	// 		take_launch_refusal(),
	// 		Some(Refusal {
	// 			path: path.clone(),
	// 			lines: vec![3],
	// 			lost: 1,
	// 			why: Unreadable::NotUtf8,
	// 		})
	// 	);
	// 	assert_eq!(take_launch_refusal(), None, "taken once");
	//
	// 	let fixed = String::from_utf8_lossy(&body).replace('\u{fffd}', "e");
	// 	std::fs::write(&path, &fixed).unwrap();
	// 	let _ = load();
	// 	assert_eq!(take_launch_refusal(), None, "a fixed file says nothing");
	// 	let _ = std::fs::remove_dir_all(&dir);
	// }

	// Off since a save on a current file that is not UTF-8 writes it again
	// without those lines and keeps the old bytes (2026100315581313), rather than
	// refusing. `a_save_on_a_current_file_that_is_not_utf8_writes_it_again` covers
	// the save and the rating write now.
	// // A save on that file used to say it saved and write nothing. Writing it
	// // would drop the line that does not decode, so it is refused, and the
	// // window hears of it as for any refused save.
	// // Test ID: ErgK2CS
	// #[test]
	// fn a_save_on_a_current_file_that_is_not_utf8_is_refused() {
	// 	let _guard = test_config_lock();
	// 	let dir = format_test_dir("notutf8_save");
	// 	let path = dir.join("config.shcl");
	// 	let body = not_utf8_current_file();
	// 	std::fs::write(&path, &body).unwrap();
	// 	set_config_override(path.clone());
	// 	let orig = Settings::default();
	// 	let mut edited = orig.clone();
	// 	edited.font_size += 1.0;
	//
	// 	let _ = take_refusal();
	// 	assert!(!persist(&orig, &edited), "nothing was written");
	// 	assert_eq!(
	// 		take_refusal(),
	// 		Some(Refusal {
	// 			path: path.clone(),
	// 			lines: vec![3],
	// 			lost: 1,
	// 			why: Unreadable::NotUtf8,
	// 		})
	// 	);
	// 	assert_eq!(std::fs::read(&path).unwrap(), body);
	//
	// 	// the rating's own writer says it could not keep the result
	// 	let kept = keep_rating(&RatingLines {
	// 		profile: Some("high"),
	// 		..RatingLines::default()
	// 	});
	// 	assert_eq!(kept, Kept::Unreadable);
	// 	assert_eq!(std::fs::read(&path).unwrap(), body);
	// 	let _ = std::fs::remove_dir_all(&dir);
	// }

	fn current_file(lines: &[u8]) -> Vec<u8> {
		[lines, shcl::FORMAT_LINE.as_bytes(), b"\n"].concat()
	}

	// A current file with lines that are not UTF-8 is written again without
	// them, keeping its own layout, as long as every other line reads as it did.
	// A heading that does not decode would hand its block to the one above, so
	// that file gets the template with what reads, and so does one where nothing
	// reads, which is the defaults.
	// Test ID: ErgZK0Q
	#[test]
	fn a_current_file_that_is_not_utf8_drops_only_those_lines() {
		let body = not_utf8_current_file();
		let Upgrade::Dropped {
			text,
			lost,
			lines,
			rewrite,
		} = upgrade(&body)
		else {
			panic!("{:?}", upgrade(&body));
		};
		assert_eq!(
			text,
			format!(
				"font:\n\tsize: 19\nwindow:\n\tcolumns: 103\n{}\n",
				shcl::FORMAT_LINE
			)
		);
		assert_eq!((lost, lines, rewrite), (0, vec![3], Rewrite::Kept));
		assert_eq!(upgraded_text(text.as_bytes()), None, "settled");
		let loss = conversion_losses(&body, std::path::Path::new("/c"), None).expect("said");
		assert_eq!(
			(loss.lost, loss.how, loss.lines),
			(0, Converted::Dropped(Rewrite::Kept), vec![3])
		);

		// a setting that does not decode is left out and counted
		let body =
			current_file(b"font:\n\tsize: 19\n\tfamily: Caf\xe9 Sans\r\nwindow:\n\tcolumns: 103\n");
		let Upgrade::Dropped {
			text,
			lost,
			rewrite,
			..
		} = upgrade(&body)
		else {
			panic!();
		};
		assert_eq!((lost, rewrite), (1, Rewrite::Kept));
		assert!(!text.contains("family"), "{text}");

		// `columns: 7` would read as font.columns without its heading
		let body =
			current_file(b"font:\n\tsize: 19\ncaf\xe9:\n\tcolumns: 7\nwindow:\n\tcolumns: 103\n");
		let Upgrade::Dropped {
			text,
			lines,
			rewrite,
			..
		} = upgrade(&body)
		else {
			panic!();
		};
		assert_eq!((lines, rewrite), (vec![3], Rewrite::Template));
		let doc = shcl::Document::parse(&text);
		assert_eq!(doc.get_float("font.size"), Ok(19.0), "{text}");
		assert_eq!(doc.get_int("window.columns"), Ok(103), "{text}");
		assert!(!text.contains("columns: 7"), "{text}");
		assert!(unreadable_lines(&doc).is_empty(), "{text}");
		assert_eq!(upgraded_text(text.as_bytes()), None, "settled");

		// nothing left that sets anything
		let body = current_file(b"## notes\nfont:\n\tsize: 1\xe9\n");
		let Upgrade::Dropped { text, rewrite, .. } = upgrade(&body) else {
			panic!();
		};
		assert_eq!(rewrite, Rewrite::Template);
		assert_eq!(text, default_config());
	}

	// An editor saving in Latin-1 garbles the copyright sign in shcl's footer.
	// The footer is the program's text, so that line goes back as shipped
	// rather than out, and is not one of the lines said to be left out. A file
	// whose only such line is there is written again all the same, since it is
	// still not UTF-8, and says so on the terminal only.
	// Test ID: ErgZKCc
	#[test]
	fn a_footer_line_that_is_not_utf8_goes_back_as_shipped() {
		let latin1 = |text: &str| -> Vec<u8> {
			text.chars()
				.map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
				.collect()
		};
		assert!(default_config().contains('\u{a9}'), "the footer has one");
		let body = latin1(default_config());
		let Upgrade::Dropped {
			text,
			lost,
			lines,
			rewrite,
		} = upgrade(&body)
		else {
			panic!("{:?}", upgrade(&body));
		};
		assert_eq!(
			(lost, lines.as_slice(), rewrite),
			(0, &[][..], Rewrite::Kept)
		);
		let unmangled: String = String::from_utf8_lossy(&body)
			.lines()
			.zip(default_config().lines())
			.map(|(read, shipped)| {
				if read.contains('\u{fffd}') {
					shipped
				} else {
					read
				}
			})
			.collect::<Vec<_>>()
			.join("\n");
		assert_eq!(text, format!("{unmangled}\n"));
		assert!(text.ends_with(SHCL_BANNER), "{text}");
		let loss = conversion_losses(&body, std::path::Path::new("/c"), None).expect("said");
		assert!(loss.lines.is_empty());
		assert_eq!(
			loss.terminal_line(),
			format!(
				"{APP_NAME}: /c: its SHCL footer was not UTF-8 text, so the file was written again with the footer as shipped."
			)
		);

		// beside a setting that is left out, in a file with CRLF endings
		let crlf = default_config()
			.replacen("theme_mode:", "theme_mode: caf\u{e9}\ntheme_mode:", 1)
			.replace('\n', "\r\n");
		let body = latin1(&crlf);
		let Upgrade::Dropped { text, lines, .. } = upgrade(&body) else {
			panic!();
		};
		assert_eq!(lines.len(), 1, "{lines:?}");
		assert!(!text.contains("caf"), "{text}");
		assert!(text.ends_with(&SHCL_BANNER.replace('\n', "\r\n")), "{text}");
	}

	// The launch that finds such a file writes it again and keeps the old bytes
	// under the format's backup name. It says which line went, on the terminal
	// and in the notice, once: the next launch has nothing to do, and a save
	// after it writes as usual.
	// Test ID: ErgZK4X
	#[test]
	fn a_launch_writes_a_current_file_that_is_not_utf8_again() {
		let _guard = test_config_lock();
		let _ = take_conversion_loss();
		let _ = take_refusal();
		let dir = format_test_dir("notutf8_rewrite");
		let path = dir.join("config.shcl");
		let body = not_utf8_current_file();
		std::fs::write(&path, &body).unwrap();
		set_config_override(path.clone());

		let s = load();
		assert!((s.font_size - 19.0).abs() < f32::EPSILON, "{}", s.font_size);
		assert_eq!(s.columns, 103);
		let kept = backups_in(&dir, "config");
		assert_eq!(kept.len(), 1, "{kept:?}");
		assert!(
			is_backup_name(&kept[0], "config", shcl::FORMAT_MAJOR),
			"{kept:?}"
		);
		let copy = dir.join(&kept[0]);
		assert_eq!(std::fs::read(&copy).unwrap(), body);
		let now = std::fs::read_to_string(&path).expect("UTF-8 now");
		let doc = shcl::Document::parse(&now);
		assert_eq!(doc.get_float("font.size"), Ok(19.0), "{now}");
		assert_eq!(doc.get_int("window.columns"), Ok(103), "{now}");
		assert!(!now.contains("caf"), "{now}");
		let loss = take_conversion_loss().expect("a notice is owed");
		assert_eq!(
			(loss.how, loss.lines.clone()),
			(Converted::Dropped(Rewrite::Kept), vec![3])
		);
		assert_eq!(loss.backup, Some(copy.clone()));
		assert_eq!(
			loss.terminal_line(),
			format!(
				"{APP_NAME}: {}: not UTF-8 text at line 3, so the file was written again without it. The old file is at {}.",
				path.display(),
				copy.display()
			)
		);

		let _ = load();
		assert_eq!(std::fs::read_to_string(&path).unwrap(), now, "settled");
		assert_eq!(backups_in(&dir, "config").len(), 1);
		assert_eq!(take_conversion_loss(), None, "said once");

		let orig = Settings::default();
		let mut edited = orig.clone();
		edited.font_size = 21.0;
		assert!(persist(&orig, &edited));
		let saved = std::fs::read_to_string(&path).unwrap();
		assert_eq!(
			shcl::Document::parse(&saved).get_float("font.size"),
			Ok(21.0),
			"{saved}"
		);
		assert_eq!(take_refusal(), None);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A launch that found the file busy left it as it was, so the first write
	// after it does what the launch would have: a Settings save, or the rating.
	// Each keeps the old bytes and says what went, and neither is refused.
	// Test ID: ErgZK8b
	#[test]
	fn a_save_on_a_current_file_that_is_not_utf8_writes_it_again() {
		let _guard = test_config_lock();
		let _ = take_conversion_loss();
		let _ = take_refusal();
		let dir = format_test_dir("notutf8_saved");
		let path = dir.join("config.shcl");
		let body = not_utf8_current_file();
		std::fs::write(&path, &body).unwrap();
		set_config_override(path.clone());
		let orig = Settings::default();
		let mut edited = orig.clone();
		edited.font_size += 1.0;

		assert!(persist(&orig, &edited));
		assert_eq!(take_refusal(), None);
		let now = std::fs::read_to_string(&path).expect("UTF-8 now");
		let doc = shcl::Document::parse(&now);
		assert_eq!(
			doc.get_float("font.size").ok(),
			Some(f64::from(edited.font_size)),
			"{now}"
		);
		assert_eq!(doc.get_int("window.columns"), Ok(103), "{now}");
		let loss = take_conversion_loss().expect("a notice is owed");
		assert_eq!(loss.how, Converted::Dropped(Rewrite::Kept));
		let copy = loss.backup.expect("a copy was kept");
		assert_eq!(std::fs::read(&copy).unwrap(), body);

		std::fs::write(&path, &body).unwrap();
		let kept = keep_rating(&RatingLines {
			profile: Some("high"),
			..RatingLines::default()
		});
		assert_eq!(kept, Kept::Written);
		let now = std::fs::read_to_string(&path).expect("UTF-8 now");
		assert_eq!(
			shcl::Document::parse(&now).get_string("performance.profile"),
			Ok("high".to_string()),
			"{now}"
		);
		let loss = take_conversion_loss().expect("a notice is owed");
		let copy = loss.backup.expect("a copy was kept");
		assert_eq!(std::fs::read(&copy).unwrap(), body);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A file with some of everything a save writes, loaded as a launch would.
	fn settled_config(path: &std::path::Path) -> Settings {
		std::fs::write(path, "").unwrap();
		set_config_override(path.to_path_buf());
		let base = reload_from_disk();
		let mut own = base.clone();
		own.font_size = 15.0;
		own.theme_mode = "light".to_string();
		own.scrollback = 5000;
		own.remembered_columns = 101;
		own.shells = vec![
			shell_entry("ash", "/bin/ash"),
			shell_entry("bsh", "/bin/bsh"),
			shell_entry("csh", "/bin/csh"),
		];
		own.monitor_sizes.push(MonitorSize {
			key: "1920x1080_100pct_527x296mm".to_string(),
			columns: 90,
			rows: 30,
			font_zoom: 1,
		});
		own.keys = own
			.keys
			.with_own(crate::input::Hotkey::ClosePane, Some(Vec::new()));
		let pal = crate::theme::resolve_in(&[], "SilkTerm", "dark", true);
		own.user_themes.push(crate::theme::UserTheme {
			slug: "mine".to_string(),
			name: "Mine".to_string(),
			dark: pal,
			light: pal,
		});
		assert!(persist(&base, &own));
		let loaded = reload_from_disk();
		assert!(loaded.keys == own.keys && loaded.user_themes.len() == 1);
		assert_eq!(loaded.monitor_sizes, own.monitor_sizes);
		assert_eq!(
			(loaded.font_size, loaded.theme_mode.as_str()),
			(15.0, "light")
		);
		loaded
	}

	// A save on a file deleted while running wrote nothing and answered that it
	// saved. Both a Settings OK and a window size save write it new from the
	// template, with what the file held and the save's own change, and nothing
	// that only lasts the session.
	// Test ID: ErkRECv
	#[test]
	fn a_save_on_a_deleted_file_writes_it_new() {
		let _guard = test_config_lock();
		let dir = format_test_dir("deleted_save");
		let path = dir.join("config.shcl");
		let loaded = settled_config(&path);
		// a rotated pick is in the live copy and never in the file
		let mut live = loaded.clone();
		live.wallpaper_raw = "/pics/rotated.png".to_string();
		live.wallpaper = Some(PathBuf::from("/pics/rotated.png"));

		std::fs::remove_file(&path).unwrap();
		let mut edited = live.clone();
		edited.scroll_smooth = !edited.scroll_smooth;
		assert!(persist(&live, &edited), "the Settings save");
		let text = std::fs::read_to_string(&path).expect("the save wrote the file");
		assert_eq!(
			text.lines().next(),
			default_config().lines().next(),
			"{text}"
		);
		let mut want = loaded.clone();
		want.scroll_smooth = edited.scroll_smooth;
		assert!(reload_from_disk() == want, "{text}");
		assert!(!text.contains("rotated"), "{text}");

		// the window size save, with the whole folder gone this time
		std::fs::remove_dir_all(&dir).unwrap();
		let mut sized = want.clone();
		remember_window(&mut sized, None, Some((120, 40)), None);
		assert!(persist(&want, &sized), "the window size save");
		assert!(path.exists(), "the save made the folder and the file");
		assert!(reload_from_disk() == sized);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The shell list save is three-way against the file (G117), and a deleted
	// file must not read as another window having removed every entry.
	// Test ID: ErkREXH
	#[test]
	fn a_save_on_a_deleted_file_keeps_the_shell_list() {
		let _guard = test_config_lock();
		let dir = format_test_dir("deleted_shells");
		let path = dir.join("config.shcl");
		let loaded = settled_config(&path);
		let slugs =
			|s: &Settings| -> Vec<String> { s.shells.iter().map(|e| e.slug.clone()).collect() };
		assert_eq!(slugs(&loaded), ["ash", "bsh", "csh"]);

		std::fs::remove_file(&path).unwrap();
		let mut sized = loaded.clone();
		sized.remembered_columns += 1;
		assert!(persist(&loaded, &sized));
		assert_eq!(slugs(&reload_from_disk()), ["ash", "bsh", "csh"]);

		// a move and a new find, the same as on a file that is there
		std::fs::remove_file(&path).unwrap();
		let mut moved = sized.clone();
		moved.shells.rotate_right(1);
		moved.shells.push(shell_entry("dsh", "/bin/dsh"));
		assert!(persist(&sized, &moved));
		assert_eq!(slugs(&reload_from_disk()), ["csh", "ash", "bsh", "dsh"]);

		// a file this process never read keeps the list the window loaded
		let unseen = dir.join("unseen.shcl");
		set_config_override(unseen.clone());
		let mut edited = moved.clone();
		edited.font_size += 1.0;
		assert!(persist(&moved, &edited));
		let doc = shcl::Document::parse(&std::fs::read_to_string(&unseen).unwrap());
		let kept: Vec<String> = read_shells(&doc).into_iter().map(|e| e.slug).collect();
		assert_eq!(kept, ["csh", "ash", "bsh", "dsh"]);
		assert_eq!(
			doc.get_float("font.size").ok(),
			Some(f64::from(edited.font_size))
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// shcl 2.0.0 wrote `# enabled: true` above `# rotate:` and indented under
	// `opacity`, so the lines read wrong once uncommented. The save is shcl's.
	// Test ID: EqpHg6T
	#[test]
	fn a_save_keeps_a_commented_block_in_order() {
		let text = "wallpaper:\n\topacity: 0.2\n\t# rotate:\n\t\t# enabled: true\n\tblur: 3\n";
		let mut doc = shcl::Document::parse(text);
		assert!(doc.set_int("wallpaper.blur", 4));
		assert_eq!(doc.to_canonical(), text.replace("blur: 3", "blur: 4"));
	}

	// Quotes, a space indent and a dotted line nobody changed stay as typed.
	// Test ID: EqwBj7o
	#[test]
	fn a_save_writes_only_the_lines_it_changed() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_keeplines_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		let body = "font:\n\tfamily: 'Cascadia Mono'\n\tsize: \"12\"\nwindow:\n\tcolumns: 90\ncolors:\n    background: \"#112233\"\nscroll.speed: 3\n";
		// stamped as a launch leaves it, or the read converts it first
		let text = &from_shcl2_text(body).unwrap();
		assert!(text.starts_with(body));
		std::fs::write(&path, text).unwrap();
		let mut doc = read_doc(&path).unwrap();
		assert!(doc.set_int("window.columns", 100));
		assert!(write_doc(&path, &doc));
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			text.replace("columns: 90", "columns: 100")
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// #136 convention: explanatory comments use '## '; commented-out (disabled)
	// settings use a single '# '.
	// Test ID: Eibw0CG
	#[test]
	fn default_config_comment_style() {
		// The file header is verbatim prose in single-'#' form and is exempt.
		let text = default_config();
		let body = text.find("\n##").map_or(text, |i| &text[i..]);
		for line in body.lines() {
			let t = line.trim_start();
			if !t.starts_with('#') {
				continue; // active setting / blank
			}
			if line_setting_key(line).is_some() {
				assert!(
					!t.starts_with("##"),
					"disabled setting must use a single '# ': {line:?}"
				);
			} else {
				assert!(
					t.starts_with("##"),
					"explanatory comment must use '## ': {line:?}"
				);
			}
		}
	}

	// #142: the default values.
	// Test ID: EibwbOi
	#[test]
	fn changed_defaults() {
		let d = Settings::default();
		assert!(d.text_scrim, "text_scrim should default on");
		// both were tuned up when the exponential falloff got twice as steep: a
		// halo that drops away sooner needs to start further out and heavier
		assert_eq!(d.text_scrim_radius, 8.0);
		assert_eq!(d.text_scrim_softness, 0.5);
		// 20% on the 20%-per-doubling scale, so exactly one doubling
		assert_eq!(d.text_scrim_strength, 20.0);
		assert_eq!(d.text_outline, 1.0);
		assert_eq!(d.text_scrim_ramp, "exp");
		assert_eq!(d.text_scrim_function, "sdf");
		assert!(d.text_scrim_regular_weight);
		assert!(!d.cursor_scrim, "cursor scrim halo defaults off");
		assert!(d.cursor_outline, "cursor outline defaults on");
		assert_eq!(d.wallpaper_blur, 10.0);
		assert_eq!(d.wallpaper_opacity, 0.10);
		// a block cursor: full height AND full width
		assert_eq!(d.cursor_size_height, 100.0);
		assert_eq!(d.cursor_size_width, 100.0);
		// rotation, when a folder turns up, varies instead of pinning image one
		assert!(d.wallpaper_rotate_random, "rotation defaults to shuffled");
		assert_eq!(d.cursor_animation_resume_s, 1.0);
		assert!(d.minimap, "the minimap defaults on");
		assert_eq!(d.cursor_animation, "pulse_vertical");
		// fills the window, ignoring aspect; a file that names no fit gets it too
		assert_eq!(d.wallpaper_default_fit, Fit::Stretch);
		let p = std::path::Path::new("test.shcl");
		assert_eq!(
			resolve(read_raw("", p).0).wallpaper_default_fit,
			Fit::Stretch
		);
		assert_eq!((d.columns, d.rows), (160, 48));
		assert_eq!(d.margin, 8.0);
		assert_eq!(d.bg, [0, 0, 0], "an all-black background");
	}

	// Scrim function + the five falloff curves resolve; unknown values fall to the
	// defaults (sdf / exponential). The falloff's two renamed curves keep parsing
	// under their old spellings, so a config written before the rename still reads
	// as the same curve rather than silently falling back to the default.
	// Test ID: EjYS5vM
	#[test]
	fn scrim_function_and_ramp_resolve() {
		let p = std::path::Path::new("test.shcl");
		for f in ["dilate", "sdf", "dt", "gaussian"] {
			let s = resolve(read_raw(&format!("text.scrim.function: \"{f}\"\n"), p).0);
			assert_eq!(s.text_scrim_function, f);
		}
		for r in ["sigmoid", "half_normal", "linear", "log", "exp"] {
			let s = resolve(read_raw(&format!("text.scrim.ramp: \"{r}\"\n"), p).0);
			assert_eq!(s.text_scrim_ramp, r);
		}
		for (old, new) in [("s", "sigmoid"), ("gaussian", "half_normal")] {
			let s = resolve(read_raw(&format!("text.scrim.ramp: \"{old}\"\n"), p).0);
			assert_eq!(s.text_scrim_ramp, new, "{old} should still parse");
		}
		let s = resolve(read_raw("text.scrim.function: \"bogus\"\n", p).0);
		assert_eq!(s.text_scrim_function, "sdf", "unknown -> default");
		let s = resolve(read_raw("text.scrim.ramp: \"bogus\"\n", p).0);
		assert_eq!(s.text_scrim_ramp, "exp", "unknown -> default");
	}

	// Each zoom step is a pixel on the configured size. Stepping past the floor
	// banks nothing, so the first step back up leaves it at once. The zoom is
	// process-wide, hence the store lock.
	// Test ID: Er2UFeP
	#[test]
	fn font_zoom_steps_a_pixel_and_stops_at_the_floor() {
		let _store = test_store_lock();
		let before = settings();
		let mut s = (*before).clone();
		s.font_size = 12.0;
		s.use_system_font_size = false;
		update(s);
		reset_font_zoom();
		assert_eq!(effective_font_size(), 12.0);
		nudge_font_zoom(1);
		nudge_font_zoom(1);
		assert_eq!(effective_font_size(), 14.0);
		for _ in 0..20 {
			nudge_font_zoom(-1);
		}
		assert_eq!(effective_font_size(), 4.0, "held at the floor");
		nudge_font_zoom(1);
		assert_eq!(effective_font_size(), 5.0, "no offset banked below it");
		reset_font_zoom();
		assert_eq!(effective_font_size(), 12.0);
		update((*before).clone());
	}

	// The face/size split's inference for configs predating use_system_font_size:
	// absent = follow the face toggle, except an explicit font_size (which the old
	// single toggle silently ignored) reads as intent and turns the size follow off.
	// Test ID: EkoQjqK
	#[test]
	fn system_font_size_split_inference() {
		let p = std::path::Path::new("test.shcl");
		let s = resolve(read_raw("", p).0);
		assert!(s.use_system_font && s.use_system_font_size, "defaults on");
		let s = resolve(read_raw("font.use_system_family: false\n", p).0);
		assert!(!s.use_system_font_size, "size follows the face toggle");
		let s = resolve(read_raw("font.size: 20.0\n", p).0);
		assert!(s.use_system_font, "explicit size keeps the system face");
		assert!(
			!s.use_system_font_size,
			"explicit size wins over the OS size"
		);
		let s = resolve(read_raw("font.size: 20.0\nfont.use_system_size: true\n", p).0);
		assert!(s.use_system_font_size, "explicit key beats the inference");
	}

	// The setting ships as a literal token, so the expander is what makes it name
	// a directory at all. Both platforms' spellings everywhere: a config file gets
	// carried between machines, and a `$HOME` left standing as a literal folder
	// name on Windows would be a very quiet way to fail.
	// Test ID: EnRWor2
	#[test]
	fn a_home_token_expands_however_it_is_spelled() {
		let home = super::home_string();
		assert!(!home.is_empty(), "this box names no home directory");
		for spelling in [
			"~",
			"$HOME",
			"${HOME}",
			"%USERPROFILE%",
			"%userprofile%",
			"$env:HOME",
			"$env:USERPROFILE",
			"${env:HOME}",
			"$ENV:home",
		] {
			assert_eq!(expand_vars(spelling), home, "{spelling} did not expand");
		}
		assert_eq!(expand_vars("~/work"), format!("{home}/work"));
		// a name that is not a variable is left exactly as it stands
		assert_eq!(expand_vars("/srv/~backup"), "/srv/~backup", "~ mid-path");
		assert_eq!(expand_vars("100%"), "100%", "a lone percent");
		assert_eq!(expand_vars("50%% off"), "50%% off", "an empty name");
		assert_eq!(expand_vars("cost $ 5"), "cost $ 5", "a lone dollar");
		// and an unset one expands to nothing, the way a shell does it
		assert_eq!(expand_vars("$SILKTERM_NO_SUCH_VAR/x"), "/x");
	}

	// The shipped default is the home variable spelled the way somebody on this
	// platform would type it, and the template's commented line has to say the
	// same thing or the first save rewrites the file we just wrote (G69, G72).
	// Test ID: Eq3hq92
	#[test]
	fn the_shipped_startup_directory_is_this_platforms_home_variable() {
		let home = super::home_string();
		assert!(!home.is_empty(), "this box names no home directory");
		let want = if cfg!(windows) {
			"%USERPROFILE%"
		} else {
			"$HOME"
		};
		assert_eq!(HOME_TOKEN, want);
		assert_eq!(expand_vars(HOME_TOKEN), home, "left standing as a literal");
		assert_eq!(Settings::default().startup_directory, HOME_TOKEN);
		let line = format!("startup_directory: \"{HOME_TOKEN}\"  ## Default");
		assert!(
			default_config().contains(&line),
			"template says something else"
		);
	}

	// The other pairs that mean one thing under two spellings. Only the one the
	// running platform sets is checked against a value; the point is that the
	// other spelling answers too rather than expanding to nothing.
	// Test ID: EoSvoUy
	#[test]
	fn the_other_platforms_spelling_of_a_name_still_answers() {
		for (ours, theirs) in [("USER", "USERNAME"), ("TMPDIR", "TEMP")] {
			let Some(value) = std::env::var_os(ours)
				.or_else(|| std::env::var_os(theirs))
				.filter(|v| !v.is_empty())
			else {
				continue; // neither is set here; nothing to compare against
			};
			let value = value.to_string_lossy().into_owned();
			assert_eq!(expand_vars(&format!("${ours}")), value);
			assert_eq!(expand_vars(&format!("%{theirs}%")), value);
		}
	}

	// The program name is expanded because nothing else ever would. Everything
	// after it is left exactly as written, because the program itself is what
	// reads those words and they mean something to it: a cmd prompt string, a
	// bash `-c` script, a percent that is just a percent. Splitting happens
	// first, which keeps a variable holding a path with a space in it whole.
	// Test ID: Eq3cPFw
	#[test]
	fn a_config_command_expands_the_program_and_nothing_after_it() {
		let home = super::home_string();
		assert_eq!(
			command_argv("$HOME/bin/sh --norc").unwrap(),
			[format!("{home}/bin/sh"), "--norc".to_string()]
		);
		assert_eq!(
			command_argv(r#""%USERPROFILE%\my app\sh" -l"#).unwrap(),
			[format!(r"{home}\my app\sh"), "-l".to_string()]
		);
		assert_eq!(
			command_argv("~/bin/sh ~/rc").unwrap(),
			[format!("{home}/bin/sh"), "~/rc".to_string()],
			"a ~ in an argument is the program's to resolve"
		);
		// the arguments that used to arrive mangled
		assert_eq!(
			command_argv("cmd /k prompt $P$G").unwrap(),
			["cmd", "/k", "prompt", "$P$G"]
		);
		assert_eq!(
			command_argv("bash -c 'echo $FOO'").unwrap(),
			["bash", "-c", "echo $FOO"]
		);
		assert_eq!(
			command_argv("cmd /k echo %PATH%").unwrap(),
			["cmd", "/k", "echo", "%PATH%"]
		);
		assert_eq!(
			command_argv(r#"pwsh -NoExit -Command "$Host.UI.RawUI.WindowTitle = 'x'""#).unwrap(),
			[
				"pwsh",
				"-NoExit",
				"-Command",
				"$Host.UI.RawUI.WindowTitle = 'x'"
			]
		);
	}

	// A directory named on the command line is checked before anything spawns, so
	// a path that is not there reads as one line naming the flag rather than as a
	// shell that failed to start - and it expands the same tokens the setting does.
	// Test ID: EnWOVqj
	#[test]
	fn a_named_directory_is_expanded_and_checked() {
		let home = super::home_string();
		assert!(!home.is_empty(), "this box names no home directory");
		assert_eq!(cli_dir("~"), Some(PathBuf::from(&home)));
		assert_eq!(cli_dir(" $HOME "), Some(PathBuf::from(&home)), "trimmed");
		assert_eq!(cli_dir("   "), None, "nothing asked for");
		assert_eq!(
			cli_dir("$SILKTERM_NO_SUCH_VAR/nowhere"),
			None,
			"no such dir"
		);
	}

	// The bug this fixes shipped in everyone's config: `shell.default` was
	// routinely a bare name where the scan had already stored the full path to
	// the same file, so a STRING compare promoted a second copy of the user's
	// default shell to the top of their own list - where the top is what "default
	// shell" now means, so the duplicate became the default.
	// Test ID: EnRWor3
	#[test]
	fn a_default_shell_already_in_the_list_moves_instead_of_doubling() {
		let entry = |slug: &str, command: &str| crate::shells::ShellEntry {
			slug: slug.to_string(),
			title: slug.to_string(),
			command: command.to_string(),
			active: true,
			comment: String::new(),
			last_seen: String::new(),
		};
		let stored = vec![
			entry("cmd", "cmd.exe"),
			entry("pwsh", "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\""),
		];
		// the stub says what the real resolver says on a box with pwsh installed
		let same = |a: &str, b: &str| a.contains("pwsh") && b.contains("pwsh");
		let out = adopt_default_into_with(&stored, "pwsh", &same);
		assert_eq!(out.len(), 2, "the list grew a duplicate");
		assert_eq!(
			out[0].slug, "pwsh",
			"the default shell did not move to the top"
		);
		assert_eq!(
			out[0].command, stored[1].command,
			"the stored command was replaced by the bare name"
		);
		// a shell the list really does not carry is still added
		let same_none = |_: &str, _: &str| false;
		let out = adopt_default_into_with(&stored, "fish", &same_none);
		assert_eq!(out.len(), 3);
		assert_eq!(out[0].command, "fish");
	}

	// Test ID: EkrTZxY
	#[test]
	fn copy_on_select_key_parses_and_defaults_on() {
		let p = std::path::Path::new("test.shcl");
		assert!(resolve(read_raw("", p).0).copy_on_select, "default on");
		assert!(!resolve(read_raw("shell.copy_on_select: false\n", p).0).copy_on_select);
	}

	// Test ID: Erg8fz0
	#[test]
	fn idle_release_ships_on() {
		let p = std::path::Path::new("test.shcl");
		assert!(Settings::default().idle_release);
		assert!(resolve(read_raw("", p).0).idle_release, "default on");
		let template = setting_lines(default_config())
			.into_iter()
			.find_map(|(name, line)| (name == "window.idle_release").then_some(line));
		assert_eq!(
			template.as_deref(),
			Some("\t# idle_release: true  ## Default")
		);
		assert!(!resolve(read_raw("window.idle_release: false\n", p).0).idle_release);
	}

	// Test ID: ErnMaKh
	#[test]
	fn software_rendering_ships_off() {
		let p = std::path::Path::new("test.shcl");
		assert!(!Settings::default().software_rendering);
		assert!(
			!resolve(read_raw("", p).0).software_rendering,
			"default off"
		);
		let template = setting_lines(default_config())
			.into_iter()
			.find_map(|(name, line)| (name == "window.software_rendering").then_some(line));
		assert_eq!(
			template.as_deref(),
			Some("\t# software_rendering: false  ## Default")
		);
		assert!(resolve(read_raw("window.software_rendering: true\n", p).0).software_rendering);
	}

	// The minimized wait came after the other two, so a file written before
	// it has the idle paragraph without it. It goes in beside them, first of
	// the waits, and the paragraph is not written twice.
	// Test ID: ErmrbFB
	#[test]
	fn an_existing_config_learns_the_minimized_wait() {
		let p = std::path::Path::new("test.shcl");
		assert_eq!(resolve(read_raw("", p).0).idle_release_minimized_min, 1);
		let set = "window.idle_release_minimized_min: 5\n";
		assert_eq!(resolve(read_raw(set, p).0).idle_release_minimized_min, 5);

		let path = crate::testdir::run_dir().join("silkterm_minimized_wait_test.shcl");
		let before = "window:\n\
			\t## Let the graphics card's memory go after the window has sat unused, and\n\
			\t# idle_release: true  ## Default\n\
			\t# idle_release_hidden_min: 30  ## Default\n\
			\t# idle_release_min: 240  ## Default\n";
		std::fs::write(&path, before).unwrap();
		backfill_config(&path);
		let out = std::fs::read_to_string(&path).unwrap();
		let _ = std::fs::remove_file(&path);
		let at = |line: &str| {
			out.find(line)
				.unwrap_or_else(|| panic!("no {line:?} in:\n{out}"))
		};
		let minimized = at("\n\t# idle_release_minimized_min: 1  ## Default\n");
		assert!(
			at("\t# idle_release: true") < minimized
				&& minimized < at("\t# idle_release_hidden_min: 30"),
			"out of order:\n{out}"
		);
		assert_eq!(
			out.matches("Let the graphics card's memory go").count(),
			1,
			"{out}"
		);
	}

	// Test ID: Em1S9yq
	#[test]
	fn hyperlink_keys_parse_in_their_block() {
		let p = std::path::Path::new("test.shcl");
		let d = resolve(read_raw("", p).0);
		assert!(d.hyperlinks, "on by default");
		assert!(
			d.hyperlink_open_command.is_empty(),
			"opener is the desktop's"
		);
		let s = resolve(
			read_raw(
				"hyperlinks:\n\tenabled: false\n\topen_command: \"firefox --new-tab\"\n",
				p,
			)
			.0,
		);
		assert!(!s.hyperlinks);
		assert_eq!(s.hyperlink_open_command, "firefox --new-tab");
	}

	// An over-range output_ease_lines must clamp: scroll's backlog clamp uses it
	// as a lower bound and panics (aborts, in release) when it exceeds the cap.
	// Test ID: EjNOyIT
	#[test]
	fn output_ease_lines_clamps_to_backlog_cap() {
		let raw = read_raw(
			"scroll.output_ease_lines: 20.0\n",
			std::path::Path::new("test.shcl"),
		)
		.0;
		let s = resolve(raw);
		assert!(s.output_ease_lines < 20.0, "over-range value must clamp");
		assert!(s.output_ease_lines <= crate::scroll::MAX_BACKLOG);
		let raw = read_raw(
			"scroll.output_ease_lines: -3.0\n",
			std::path::Path::new("test.shcl"),
		)
		.0;
		assert!(resolve(raw).output_ease_lines >= 0.0);
	}

	// One syntax-broken line must not sink the valid settings around it.
	// Test ID: Eij9gBE
	#[test]
	fn parse_lenient_drops_only_the_bad_line() {
		let text = "transparency.opacity: 0.7\ncursor_blink: enable\nwindow.margin: 12.0\n";
		let raw = read_raw(text, std::path::Path::new("test.shcl")).0;
		assert_eq!(raw.opacity, Some(0.7)); // before the bad line
		assert_eq!(raw.margin, Some(12.0)); // after the bad line
	}

	// Test ID: EiuvVey
	#[test]
	fn chrome_colors_default_and_override() {
		// theme provides the chrome; the default matches the shared menu colors
		let d = Settings::default();
		assert_eq!(d.menu_bg, crate::theme::MENU_BG_DEF);
		assert_eq!(d.menu_fg, crate::theme::MENU_FG_DEF);
		// a colors override wins; unspecified chrome stays at the theme default
		let raw = read_raw(
			"colors.menu_background: \"#123456\"\ncolors.dialog_foreground: \"#abcdef\"\n",
			std::path::Path::new("test.shcl"),
		)
		.0;
		let s = resolve(raw);
		assert_eq!(s.menu_bg, [0x12, 0x34, 0x56]);
		assert_eq!(s.dialog_fg, [0xab, 0xcd, 0xef]);
		assert_eq!(s.menu_fg, crate::theme::MENU_FG_DEF);
	}

	// A pre-nesting config converts wholesale: every ACTIVE value ends at its
	// new nested path (oldest alias spellings included), obsolete keys drop,
	// themes.* user data survives, and the original file is kept as a .bak.
	// When both an old alias and its newer flat spelling are present, the newer
	// one wins.
	// Test ID: ElmIYG0
	#[test]
	fn legacy_config_converts_with_values_carried() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_convert_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let legacy = "## my own note\n\
			scrollback: 5000\n\
			cursor_size_vertical: 40\n\
			cursor_shape: \"block\"\n\
			background_fit: \"zoom\"\n\
			wallpaper_default: false\n\
			wallpaper_opacity: 0.4\n\
			background_opacity: 0.9\n\
			opacity: 0.8\n\
			scroll_tau_ms: 120.0\n\
			wheel_lines: 4  ## an old trailing note\n\
			# margin: 8.0\n\
			font_family: \"Iosevka\"\n\
			themes.mine.dark.background: \"#010203\"\n\
			colors.focus: \"#abcdef\"\n\
			theme: Matrix\n";
		std::fs::write(&path, legacy).unwrap();
		convert_legacy_config(&path);
		let out = std::fs::read_to_string(&path).unwrap();

		assert!(out.contains("\tscrollback: 5000"), "value carried:\n{out}");
		assert!(
			out.contains("\t\theight: 40"),
			"oldest alias lands at the nested path:\n{out}"
		);
		assert!(!out.contains("cursor_shape"), "obsolete key dropped");
		assert!(
			out.contains("\tdefault_fit: \"zoom\""),
			"background_* alias carried:\n{out}"
		);
		assert!(
			out.contains("\tfallback_builtin: false"),
			"the builtin switch carried under its new name:\n{out}"
		);
		assert!(
			out.contains("\topacity: 0.4") && !out.contains("\topacity: 0.9"),
			"the newer flat spelling wins over its alias:\n{out}"
		);
		assert!(
			out.contains("\topacity: 0.8"),
			"old bare opacity is the transparency one:\n{out}"
		);
		assert!(
			!out.contains("tau_ms: 120.0"),
			"a since-retired setting must not resurrect through conversion:\n{out}"
		);
		assert!(
			out.contains("\twheel_lines: 4\n"),
			"value carried without its stale trailing note:\n{out}"
		);
		assert!(
			out.contains("\tmargin: 8.0"),
			"a commented old line just leaves the fresh default in place:\n{out}"
		);
		assert!(
			out.contains("\tfamily: \"Iosevka\"") && out.contains("\tuse_system_family: false"),
			"an explicit font pins the system toggle off, as it always meant:\n{out}"
		);
		assert!(
			out.contains("themes.mine.dark.background: \"#010203\""),
			"future-feature user data carried:\n{out}"
		);
		assert!(
			out.contains("\tfocus: \"#abcdef\""),
			"color override carried"
		);
		assert!(out.contains("theme: Matrix"), "theme choice carried");
		assert!(
			!out.contains("## my own note"),
			"old comments live in the .bak, not the fresh template"
		);
		let bak = std::fs::read_to_string(dir.join("config.shcl.bak")).unwrap();
		assert_eq!(bak, legacy, "the original file survives untouched as .bak");

		// the converted file is current-format: converting again is a no-op
		convert_legacy_config(&path);
		assert_eq!(out, std::fs::read_to_string(&path).unwrap());
		let _ = std::fs::remove_file(&path);
		let _ = std::fs::remove_file(dir.join("config.shcl.bak"));
	}

	// The glow settings were renamed to scrim, and the glow border to the text
	// outline. A flat file from before that keeps every value.
	// Test ID: Er2UFeQ
	#[test]
	fn the_old_glow_names_convert_to_scrim_and_outline() {
		let out = converted_config_text(
			"text_glow: false\ntext_glow_radius: 7\ntext_glow_softness: 0.3\ncursor_glow: true\ntext_glow_border: 2.5\ntext_glow_ramp: \"s\"\n",
		)
		.expect("a flat file converts");
		let s = resolve(read_raw(&out, std::path::Path::new("test.shcl")).0);
		assert!(!s.text_scrim, "{out}");
		assert_eq!(s.text_scrim_radius, 7.0, "{out}");
		assert_eq!(s.text_scrim_softness, 0.3, "{out}");
		assert!(s.cursor_scrim, "{out}");
		assert_eq!(s.text_outline, 2.5, "{out}");
		assert_eq!(s.text_scrim_ramp, "sigmoid", "{out}");
	}

	// shcl drops a line that steps back to a depth nothing uses, so it sets
	// nothing. Called `margin`, it used to send the whole file to `.bak` as an
	// old flat one.
	// Test ID: ErCHjg8
	#[test]
	fn a_line_that_sets_nothing_never_converts_the_file() {
		let text = "\t\twindow:\n\t\t\tcolumns: 100\n\tmargin: 4\n";
		let doc = shcl::Document::parse(text);
		assert_eq!(doc.lost_count(), 1);
		assert!(doc.get_string("margin").is_err());
		assert_eq!(converted_config_text(text), None);
		// an old name that does read still converts
		assert!(converted_config_text("margin: 4\nwindow:\n\tcolumns: 100\n").is_some());
	}

	// The shipped template itself must never read as legacy.
	// Test ID: ElmIYG1
	#[test]
	fn a_new_format_config_never_converts() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_noconvert_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, default_config()).unwrap();
		convert_legacy_config(&path);
		assert_eq!(std::fs::read_to_string(&path).unwrap(), default_config());
		assert!(
			!dir.join("config.shcl.bak").exists(),
			"no backup for a current-format file"
		);
		let _ = std::fs::remove_file(&path);
	}

	// A current file with one flat key at the margin still converts, and its shell
	// list is the user's own: every entry carries, in order, with all its fields.
	// Test ID: EpyvpeD
	#[test]
	fn a_converted_file_keeps_its_shell_list() {
		let list = vec![
			crate::shells::ShellEntry {
				comment: "the one in use".into(),
				last_seen: "2026-09-01".into(),
				..shell_entry("zsh", "/bin/zsh")
			},
			crate::shells::ShellEntry {
				active: false,
				title: "Old bash".into(),
				..shell_entry("bash", "/bin/bash")
			},
			shell_entry("fish", "/usr/bin/fish"),
		];
		let mut doc = shcl::Document::parse(default_config());
		write_shells(&mut doc, &[], &list);
		let text = format!("{}rows: 40\n", doc.to_canonical());
		let out = converted_config_text(&text).expect("a flat key at the margin converts");
		assert!(
			read_shells(&shcl::Document::parse(&out)) == list,
			"converted:\n{out}"
		);
		assert!(
			!out.lines().any(|l| l == "rows: 40"),
			"the flat key is gone from the margin:\n{out}"
		);
		assert_eq!(converted_config_text(&out), None, "converted twice:\n{out}");

		let dir = crate::testdir::run_dir()
			.join(format!("silkterm_convert_shells_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		std::fs::write(&path, &text).unwrap();
		convert_legacy_config(&path);
		let disk = std::fs::read_to_string(&path).unwrap();
		assert!(
			read_shells(&shcl::Document::parse(&disk)) == list,
			"on disk:\n{disk}"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A flat `wallpaper:` held the image. It reaches `wallpaper.image`, never the
	// block heading, whatever order the carried values are placed in, and a save
	// from Settings then loads every carried value the same.
	// Test ID: EpZcBUO
	#[test]
	fn a_flat_wallpaper_converts_to_the_image_and_survives_a_save() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_flatwp_{}", std::process::id()));
		let flat = "wallpaper: /home/x/Pictures/a.png\nbackground_opacity: 0.5\nwallpaper_fit: zoom\nwallpaper_blur: 3\nbackground_contrast_mask_auto: 0.8\nfont_size: 13\n";
		// the carried values are placed in hash order, so one round proves little
		for round in 0..16 {
			let text = if round % 2 == 1 {
				flat.replace('\n', "\r\n")
			} else {
				flat.to_string()
			};
			let _ = std::fs::remove_dir_all(&dir);
			std::fs::create_dir_all(&dir).unwrap();
			let path = dir.join("config.shcl");
			std::fs::write(&path, &text).unwrap();
			set_config_override(path.clone());
			let check = |s: &Settings, when: &str| {
				assert_eq!(
					s.wallpaper_raw, "/home/x/Pictures/a.png",
					"{when}, round {round}: the image"
				);
				assert_eq!(s.wallpaper_opacity, 0.5, "{when}, round {round}: opacity");
				assert!(
					matches!(s.wallpaper_default_fit, Fit::Zoom),
					"{when}, round {round}: fit"
				);
				assert_eq!(s.wallpaper_blur, 3.0, "{when}, round {round}: blur");
				assert_eq!(
					s.wallpaper_contrast_mask_auto, 0.8,
					"{when}, round {round}: contrast mask"
				);
			};
			let launched = load();
			check(&launched, "first launch");
			let mut edited = launched.clone();
			edited.font_size += 1.0;
			assert!(persist(&launched, &edited));
			check(&load(), "after a save");
			let baks = std::fs::read_dir(&dir)
				.unwrap()
				.flatten()
				.filter(|e| e.file_name().to_string_lossy().contains(".bak"))
				.count();
			assert_eq!(baks, 1, "round {round}: converted once");
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	fn active_headings(text: &str) -> Vec<(usize, String)> {
		walk_settings(text)
			.into_iter()
			.filter_map(|w| match w {
				WalkLine::Setting {
					index,
					path,
					header: true,
					active: true,
				} => Some((index, path)),
				_ => None,
			})
			.collect()
	}

	// Every block name, not only `wallpaper`: an old flat line named like a block
	// is either carried to a setting or dropped, and the block keeps its heading.
	// Test ID: EpZcBUP
	#[test]
	fn a_flat_key_named_like_a_block_never_lands_on_its_heading() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_flathead_{}", std::process::id()));
		let heads = active_headings(default_config());
		assert!(heads.iter().any(|(_, p)| p == "wallpaper"));
		for (_, head) in heads.iter().filter(|(_, p)| !p.contains('.')) {
			let _ = std::fs::remove_dir_all(&dir);
			std::fs::create_dir_all(&dir).unwrap();
			let path = dir.join("config.shcl");
			std::fs::write(&path, format!("{head}: x\nfont_size: 13\n")).unwrap();
			convert_legacy_config(&path);
			let out = std::fs::read_to_string(&path).unwrap();
			assert!(
				out.contains("\tsize: 13"),
				"`{head}: x` was not converted:\n{out}"
			);
			assert_eq!(
				active_headings(&out),
				heads,
				"`{head}: x` changed a block heading:\n{out}"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	fn misplaced_image(text: &str) -> String {
		let out = text.replacen("\nwallpaper:\n", "\nwallpaper: /home/x/Pictures/a.png\n", 1);
		assert_ne!(out, text, "the template has no plain wallpaper heading");
		out + "wallpaper.default_fit: zoom\n"
	}

	// A file an earlier conversion left with the image on the `wallpaper:` heading
	// gets it back under `image:`, in place. No backup is taken, so a folder that
	// already holds every backup name is repaired too, and the next launch neither
	// converts the file nor changes it again.
	// Test ID: EpZefUm
	#[test]
	fn an_image_left_on_the_wallpaper_heading_moves_to_image() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_wphead_{}", std::process::id()));
		let damaged = misplaced_image(default_config());
		let saved = {
			let mut doc = shcl::Document::parse(&damaged);
			assert!(doc.set_float("font.size", 15.5));
			doc.to_canonical()
		};
		let (a, b) = ("/home/x/Pictures/a.png", "/home/x/Pictures/b.png");
		// (what, file, image, every backup name taken, missing settings added first)
		let cases = [
			("as converted", damaged.clone(), a, false, false),
			("after a save", saved, a, false, false),
			("with CRLF", damaged.replace('\n', "\r\n"), a, false, false),
			(
				"an image named since",
				format!("{damaged}wallpaper.image: {b}\n"),
				b,
				false,
				false,
			),
			("every backup name taken", damaged.clone(), a, true, false),
			("as that launch left it", damaged.clone(), a, false, true),
		];
		for (what, text, image, full, backfilled) in cases {
			let _ = std::fs::remove_dir_all(&dir);
			std::fs::create_dir_all(&dir).unwrap();
			let path = dir.join("config.shcl");
			std::fs::write(&path, &text).unwrap();
			if full {
				for n in 1..=BACKUPS_MAX {
					let name = if n == 1 {
						"config.shcl.bak".to_string()
					} else {
						format!("config.shcl.bak{n}")
					};
					std::fs::write(dir.join(name), "old\n").unwrap();
				}
			}
			if backfilled {
				// the launch that converted it added the settings it then misread,
				// under the heading, as backfill did before it checked
				let heading = format!("wallpaper: {a}\n");
				let added = "\n\t# honor_xmp: true  ## Default\n\t# enabled: true  ## Default\n";
				let text = text.replacen(&heading, &format!("{heading}{added}"), 1);
				assert!(
					text.contains(added),
					"{what}: nothing was added, so the case proves nothing"
				);
				std::fs::write(&path, &text).unwrap();
			}
			set_config_override(path.clone());
			let s = load();
			assert_eq!(s.wallpaper_raw, image, "{what}: the image");
			assert!(
				matches!(s.wallpaper_default_fit, Fit::Zoom),
				"{what}: the rest of the block"
			);
			let once = std::fs::read_to_string(&path).unwrap();
			let doc = shcl::Document::parse(&once);
			assert!(
				doc.get_string("wallpaper").is_err() && doc.count("wallpaper") == 1,
				"{what}: a heading again:\n{once}"
			);
			assert_eq!(doc.count("wallpaper.image"), 1, "{what}: one image line");
			let baks = std::fs::read_dir(&dir)
				.unwrap()
				.flatten()
				.filter(|e| e.file_name().to_string_lossy().contains(".bak"))
				.count();
			assert_eq!(
				baks,
				if full { BACKUPS_MAX as usize } else { 0 },
				"{what}: nothing converted"
			);
			assert_eq!(load().wallpaper_raw, image, "{what}: next launch");
			assert_eq!(
				std::fs::read_to_string(&path).unwrap(),
				once,
				"{what}: settled"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The repair rewrites a settings file at launch, so every shape that is not
	// the one an earlier conversion wrote is left alone.
	// Test ID: EpZefUn
	#[test]
	fn only_a_heading_holding_a_value_is_repaired() {
		let damaged = misplaced_image(default_config());
		let want = default_config().replacen(
			"\nwallpaper:\n",
			"\nwallpaper:\n\timage: /home/x/Pictures/a.png\n",
			1,
		) + "wallpaper.default_fit: zoom\n";
		assert_eq!(
			wallpaper_heading_repaired(&damaged).as_deref(),
			Some(want.as_str()),
			"one line cleared, one added, nothing else"
		);
		assert_eq!(wallpaper_heading_repaired(&want), None, "settled");
		assert_eq!(
			wallpaper_heading_repaired(&damaged.replace('\n', "\r\n")).as_deref(),
			Some(want.as_str()),
			"CRLF"
		);
		let fence = "notes: ```\nwallpaper: /p/b.png\n\trotate:\n```\n";
		assert_eq!(
			wallpaper_heading_repaired(&format!("{damaged}{fence}")),
			Some(format!("{want}{fence}")),
			"a fenced value's text is kept"
		);
		for (text, out) in [
			(
				"wallpaper: /p/a.png\n\trotate:\n\t\tenabled: true\n",
				"wallpaper:\n\timage: /p/a.png\n\trotate:\n\t\tenabled: true\n",
			),
			(
				"wallpaper: /p/a.png  # mine\n\n\t# enabled: true\n\t\tcontrast_mask:\n",
				"wallpaper:\n\t\timage: /p/a.png  # mine\n\n\t# enabled: true\n\t\tcontrast_mask:\n",
			),
		] {
			assert_eq!(
				wallpaper_heading_repaired(text).as_deref(),
				Some(out),
				"{text}"
			);
		}
		for text in [
			"wallpaper: /p/a.png\nwallpaper_opacity: 0.4\n".to_string(),
			"wallpaper: /p/a.png\n".to_string(),
			"# wallpaper: /p/a.png\n\trotate:\n".to_string(),
			"wallpaper:\n\trotate:\n\t\tenabled: true\n".to_string(),
			"wallpaper: /p/a.png\n# note\n\n\t# rotate:\nfont_size: 13\n".to_string(),
			"\twallpaper: /p/a.png\n\t\trotate:\n".to_string(),
			format!("{damaged}wallpaper: /p/b.png\n\topacity: 0.4\n"),
			// an old flat name indented by hand is the conversion's to read
			"wallpaper: /p/a.png\n\twallpaper_opacity: 0.4\nfont_size: 13\n".to_string(),
			"wallpaper: /p/a.png\n\twallpaper_opacity: 0.4\n".to_string(),
			// the old flat `opacity` is also a name inside the block
			"wallpaper: /p/a.png\nopacity: 0.4\n".to_string(),
			"wallpaper: /p/a.png\nrotate:\n\tenabled: true\n".to_string(),
			"wallpaper: /p/a.png\n\t* /p/b.png\n".to_string(),
			"notes: ```\nwallpaper: /p/a.png\n\trotate:\n```\n".to_string(),
			"wallpaper: ```\n\trotate:\n```\n".to_string(),
			"wallpaper:  ## mine\n\trotate:\n".to_string(),
			"wallpaper:\r\n\trotate:\r\n\t\tenabled: true\r\n".to_string(),
			"wallpaper: /p/a.png\r\nfont_size: 13\r\n".to_string(),
			default_config().to_string(),
		] {
			assert_eq!(wallpaper_heading_repaired(&text), None, "{text}");
		}
	}

	// A launch that finds the settings file open in another program leaves it as it
	// is, and the next launch that does not repairs it.
	// Test ID: EpZtWPY
	#[cfg(target_os = "linux")]
	#[test]
	fn a_busy_launch_defers_the_wallpaper_repair() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_wpbusy_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		let damaged = misplaced_image(default_config());
		std::fs::write(&path, &damaged).unwrap();
		set_config_override(path.clone());
		let baks = || {
			std::fs::read_dir(&dir)
				.unwrap()
				.flatten()
				.filter(|e| e.file_name().to_string_lossy().contains(".bak"))
				.count()
		};

		// a child with the file as its stdin holds it open until it exits
		let hold = std::fs::File::open(&path).unwrap();
		let mut child = std::process::Command::new("sleep")
			.arg("30")
			.stdin(std::process::Stdio::from(hold))
			.spawn()
			.unwrap();
		let seen = (0..50).any(|_| {
			if config_open_elsewhere(&path) {
				return true;
			}
			std::thread::sleep(std::time::Duration::from_millis(20));
			false
		});
		let busy = seen.then(|| (load().wallpaper_raw, std::fs::read_to_string(&path)));
		let _ = child.kill();
		let _ = child.wait();

		let (image, text) = busy.expect("the holder was never seen");
		assert_eq!(image, "", "a busy launch loads no image");
		assert_eq!(
			text.unwrap(),
			damaged,
			"a busy launch leaves the file alone"
		);
		assert_eq!(baks(), 0, "a busy launch takes no backup");
		assert_eq!(
			load().wallpaper_raw,
			"/home/x/Pictures/a.png",
			"the next launch repairs it"
		);
		let doc = shcl::Document::parse(&std::fs::read_to_string(&path).unwrap());
		assert_eq!(doc.count("wallpaper.image"), 1, "one image line");
		assert_eq!(baks(), 0, "the repair takes no backup");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The repair rewrites the settings file at launch, so a linked file stays
	// linked to the same file, and a private one stays private.
	// Test ID: EpZts0G
	#[cfg(unix)]
	#[test]
	fn a_repair_keeps_a_linked_private_settings_file() {
		use std::os::unix::fs::PermissionsExt;
		let dir = crate::testdir::run_dir().join(format!("silkterm_wplink_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let real = dir.join("real.shcl");
		let damaged = misplaced_image(default_config());
		std::fs::write(&real, &damaged).unwrap();
		std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
		let link = dir.join("config.shcl");
		std::os::unix::fs::symlink(&real, &link).unwrap();

		repair_wallpaper_heading(&link);

		assert!(
			std::fs::symlink_metadata(&link)
				.unwrap()
				.file_type()
				.is_symlink(),
			"still a link"
		);
		assert_eq!(std::fs::read_link(&link).unwrap(), real);
		assert_eq!(
			std::fs::read_to_string(&real).unwrap(),
			wallpaper_heading_repaired(&damaged).unwrap(),
			"the linked file is repaired"
		);
		assert_eq!(
			std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
			0o600,
			"the file stays private"
		);
		let names: Vec<String> = std::fs::read_dir(&dir)
			.unwrap()
			.flatten()
			.map(|e| e.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names.len(), 2, "no backup and no temp file: {names:?}");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The conversion rewrites the settings file where it is: a link stays a link
	// to the same file, a private file stays private, its backup is as private,
	// and a link sitting at a backup name is never written through.
	// Test ID: EpZfVmi
	#[cfg(unix)]
	#[test]
	fn a_conversion_keeps_a_linked_private_settings_file() {
		use std::os::unix::fs::PermissionsExt;
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_convlink_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let real = dir.join("real.shcl");
		let flat = "font_size: 13\n";
		std::fs::write(&real, flat).unwrap();
		std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
		let link = dir.join("config.shcl");
		std::os::unix::fs::symlink(&real, &link).unwrap();
		let victim = dir.join("victim.txt");
		std::fs::write(&victim, "untouched\n").unwrap();
		std::os::unix::fs::symlink(&victim, dir.join("config.shcl.bak")).unwrap();

		convert_legacy_config(&link);

		assert!(
			std::fs::symlink_metadata(&link)
				.unwrap()
				.file_type()
				.is_symlink(),
			"still a link"
		);
		assert_eq!(std::fs::read_link(&link).unwrap(), real);
		assert!(
			std::fs::read_to_string(&real)
				.unwrap()
				.contains("\tsize: 13"),
			"the linked file is converted"
		);
		assert_eq!(
			std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
			0o600
		);
		assert_eq!(std::fs::read_to_string(&victim).unwrap(), "untouched\n");
		let bak = dir.join("config.shcl.bak2");
		assert!(
			std::fs::symlink_metadata(&bak)
				.unwrap()
				.file_type()
				.is_file(),
			"the backup is a plain file"
		);
		assert_eq!(std::fs::read_to_string(&bak).unwrap(), flat);
		assert_eq!(
			std::fs::metadata(&bak).unwrap().permissions().mode() & 0o777,
			0o600
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A conversion that cannot write leaves the file as it was and keeps no
	// backup of it, or a file that stays unwritable gains one at every launch.
	// Test ID: EpZfVmj
	#[cfg(unix)]
	#[test]
	fn a_conversion_that_cannot_write_keeps_no_backup() {
		use std::os::unix::fs::PermissionsExt;
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_convfail_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		let locked = dir.join("locked");
		std::fs::create_dir_all(&locked).unwrap();
		let real = locked.join("real.shcl");
		let flat = "font_size: 13\n";
		std::fs::write(&real, flat).unwrap();
		std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
		let unlock = || {
			let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
			let _ = std::fs::remove_dir_all(&dir);
		};
		// a run with the rights to write there anyway has nothing to test
		if std::fs::write(locked.join("probe"), "").is_ok() {
			unlock();
			return;
		}
		let link = dir.join("config.shcl");
		std::os::unix::fs::symlink(&real, &link).unwrap();

		convert_legacy_config(&link);

		let still_link = std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink());
		let text = std::fs::read_to_string(&real).unwrap_or_default();
		let backup = std::fs::symlink_metadata(dir.join("config.shcl.bak")).is_ok();
		unlock();
		assert!(still_link, "the settings file is still a link");
		assert_eq!(text, flat, "the file is as it was");
		assert!(!backup, "no backup of a file that was not converted");
	}

	// A failed write is not proof the file is as it was. ReplaceFile can fail after
	// the file it replaces is gone, and then the backup is the only copy left. So a
	// failed conversion drops its backup only when the file reads back whole.
	// Test ID: EpZszNA
	#[test]
	fn a_failed_conversion_keeps_the_backup_unless_the_file_is_whole() {
		type Write = fn(&std::path::Path, &str) -> Result<(), String>;
		let refuse: Write = |_, _| Err("refused".to_string());
		let remove: Write = |path, _| {
			std::fs::remove_file(path).map_err(|e| e.to_string())?;
			Err("replaced file gone".to_string())
		};
		let cut: Write = |path, _| {
			std::fs::write(path, "font_").map_err(|e| e.to_string())?;
			Err("cut short".to_string())
		};
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_convkeep_{}", std::process::id()));
		let flat = "font_size: 13\n";
		// (what, writer, the settings file after, the backup kept)
		for (what, write, file, kept) in [
			("the file as it was", refuse, Some(flat), false),
			("the file gone", remove, None, true),
			("the file changed", cut, Some("font_"), true),
		] {
			let _ = std::fs::remove_dir_all(&dir);
			std::fs::create_dir_all(&dir).unwrap();
			let path = dir.join("config.shcl");
			std::fs::write(&path, flat).unwrap();

			convert_legacy_config_with(&path, write);

			assert_eq!(
				std::fs::read_to_string(&path).ok().as_deref(),
				file,
				"{what}: the settings file"
			);
			assert_eq!(
				std::fs::read_to_string(dir.join("config.shcl.bak"))
					.ok()
					.as_deref(),
				kept.then_some(flat),
				"{what}: the backup"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A hand-edited config is where a home-relative path gets typed, so `~` has
	// to expand. `~user` has nothing to resolve against and stays literal.
	// Test ID: Ellevxg
	#[test]
	fn tilde_expands_to_home_but_only_for_this_user() {
		let home = super::home_string();
		assert!(!home.is_empty(), "this box names no home directory");
		assert_eq!(expand_vars("~/pics"), format!("{home}/pics"));
		assert_eq!(expand_vars("~"), home);
		for literal in ["~someone/pics", "/abs/pics", "rel/pics", "wallpaper/"] {
			assert_eq!(
				expand_vars(literal),
				literal,
				"{literal} should stay literal"
			);
		}
	}

	// The first path-level rename since the config went nested. A rename rewrites
	// the key on its own line and must keep everything else about it: the value,
	// the active/commented state, and the indentation that says which block it
	// belongs to. Renames may not cross blocks - the machinery rewrites lines, it
	// does not move them - so this one stays inside `scroll:`.
	// Test ID: ElpQBii
	#[test]
	fn a_renamed_setting_keeps_its_value_and_its_block() {
		let out = migrate_config_text("scroll:\n\tinview_tau_ms: 45.0\n").expect("should migrate");
		assert_eq!(out, "scroll:\n\tsingle_screen_tau_ms: 45.0\n");
		// the value has to survive the trip through the loader, not just the text
		let s = resolve(read_raw(&out, std::path::Path::new("test.shcl")).0);
		assert!((s.scroll_single_screen_tau_ms - 45.0).abs() < f32::EPSILON);
		// the dotted spelling reads and rewrites the same way
		assert_eq!(
			migrate_config_text("scroll.inview_tau_ms: 45.0\n").as_deref(),
			Some("scroll.single_screen_tau_ms: 45.0\n")
		);
		// a commented line is renamed too, so the file keeps documenting itself
		assert_eq!(
			migrate_config_text("scroll:\n\t# inview_tau_ms: 60.0  ## Default\n").as_deref(),
			Some("scroll:\n\t# single_screen_tau_ms: 60.0  ## Default\n")
		);
		// and a config that already carries the new spelling is left alone
		assert!(migrate_config_text("scroll:\n\tsingle_screen_tau_ms: 45.0\n").is_none());
	}

	// Renames nest: this one sits two blocks deep, so the machinery has to match
	// on the whole path rather than the leaf.
	// Test ID: EoqHGQy
	#[test]
	fn a_renamed_setting_two_blocks_deep_is_found() {
		assert_eq!(
			migrate_config_text(
				"scroll:\n\tminimap:\n\t\t# keep_for: \"less tmux screen\"  ## Default\n"
			)
			.as_deref(),
			Some(
				"scroll:\n\tminimap:\n\t\t# tui_process_whitelist: \"less tmux screen\"  ## Default\n"
			)
		);
		let out = migrate_config_text("scroll:\n\tminimap:\n\t\tkeep_for: \"less vim\"\n")
			.expect("should migrate");
		let s = resolve(read_raw(&out, std::path::Path::new("test.shcl")).0);
		assert_eq!(s.minimap_tui_whitelist, "less vim");
	}

	// A rename can hand its old name to a NEW setting - `colors.focus` became
	// `colors.highlight` and the freed name now holds the vivid focus color.
	// Once both spellings are in the file the old line must be left exactly
	// where it is: it is no longer stale, it is the new setting's own line, and
	// dropping or re-renaming it would delete a user's color on every launch.
	// Test ID: Em3Pif2
	#[test]
	fn a_renamed_key_frees_its_old_name_for_a_new_setting() {
		// first launch: the one color there is becomes the calm one
		let once = migrate_config_text("colors:\n\tfocus: \"#abcdef\"\n").expect("should migrate");
		assert_eq!(once, "colors:\n\thighlight: \"#abcdef\"\n");
		let s = resolve(read_raw(&once, std::path::Path::new("test.shcl")).0);
		assert_eq!(s.highlight, [0xab, 0xcd, 0xef]);
		assert_eq!(
			s.focus,
			Settings::default().focus,
			"the new one starts fresh"
		);

		// after backfill both spellings are present, and every launch after that
		// is a no-op - the file has reached its resting state
		let both = "colors:\n\thighlight: \"#abcdef\"\n\tfocus: \"#123456\"\n";
		assert!(migrate_config_text(both).is_none());
		let s = resolve(read_raw(both, std::path::Path::new("test.shcl")).0);
		assert_eq!(s.highlight, [0xab, 0xcd, 0xef]);
		assert_eq!(s.focus, [0x12, 0x34, 0x56]);

		// a stale line carrying the pre-theme default still refreshes, under the
		// path it ends on rather than the one it was written under
		let stale = migrate_config_text("colors:\n\t# focus: \"#5580c8\"  ## Default\n")
			.expect("should refresh");
		assert!(
			stale.contains("# highlight: \"#c8a05a\"  ## Default"),
			"got {stale}"
		);
	}

	// The retired speed knobs leave existing configs entirely: tau_ms has no
	// successor, and ease_in changed units (fraction -> milliseconds), so its
	// old value must not be carried into the new key. Active and stale
	// commented lines both go; the settings around them stay put.
	// Test ID: Elqv6O0
	#[test]
	fn retired_scroll_knobs_are_removed_not_carried() {
		let out = migrate_config_text(
			"scroll:\n\ttau_ms: 120.0\n\tease_in: 0.5\n\tramp_up_ms: 200.0\n\t# tau_ms: 230.0  ## Default\n",
		)
		.expect("should migrate");
		assert_eq!(out, "scroll:\n\tramp_up_ms: 200.0\n");
		let s = resolve(read_raw(&out, std::path::Path::new("test.shcl")).0);
		assert!((s.scroll_ramp_up_ms - 200.0).abs() < f32::EPSILON);
		// the old fraction never leaks into the new duration
		assert!((s.scroll_ease_in_ms - Settings::default().scroll_ease_in_ms).abs() < f32::EPSILON);
	}

	// A config with nothing to migrate is left untouched (no needless rewrite).
	// Test ID: Eimg8fI
	#[test]
	fn migrate_config_noop_when_current() {
		assert!(
			migrate_config_text("transparency.opacity: 0.7\ncursor.animation: \"phase\"\n")
				.is_none()
		);
		assert!(migrate_config_text(default_config()).is_none());
	}

	// Backfill only ever adds a missing key, so a config written when an older
	// stack was the default kept that stack forever. Migration refreshes exactly
	// the shipped defaults and nothing the user chose themselves - now keyed on
	// the nested font.family path.
	// Test ID: ElEvh0S
	#[test]
	fn migrate_refreshes_a_superseded_default_font_stack() {
		let stale = SUPERSEDED_FONT_STACKS[0];
		let out = migrate_config_text(&format!(
			"font:\n\tuse_system_family: true\n\tfamily: \"{stale}\"\n"
		))
		.expect("stale default should be refreshed");
		assert!(
			out.contains(&format!("\tfamily: \"{DEFAULT_FONT_STACK}\"")),
			"{out:?}"
		);
		assert!(!out.contains(stale));

		// the current value is already right, so nothing to do
		let current = format!("font:\n\tfamily: \"{DEFAULT_FONT_STACK}\"\n");
		assert!(migrate_config_text(&current).is_none());
		// a stack the user edited, or one they commented out, is theirs - leave it
		let edited = format!("font:\n\tfamily: \"Iosevka, {stale}\"\n");
		assert!(migrate_config_text(&edited).is_none());
		assert!(migrate_config_text(&format!("font:\n\t# family: \"{stale}\"\n")).is_none());
		// a top-level dotted spelling refreshes too
		assert!(migrate_config_text(&format!("font.family: \"{stale}\"\n")).is_some());
		// a save swaps single quotes for double, so either reads as the old default
		let out = migrate_config_text(&format!("font:\n\tfamily: '{stale}'\n"))
			.expect("a single-quoted stale default should be refreshed");
		assert!(
			out.contains(&format!("\tfamily: \"{DEFAULT_FONT_STACK}\"")),
			"{out:?}"
		);
		assert!(migrate_config_text(&format!("font.family: '{stale}'\n")).is_some());
		let crlf = migrate_config_text(&format!("font:\r\n\tfamily: '{stale}'\r\n"))
			.expect("a single-quoted stale default with CRLF endings should be refreshed");
		assert!(crlf.contains(DEFAULT_FONT_STACK), "{crlf:?}");
		// a note on the line keeps it the user's
		assert!(migrate_config_text(&format!("font:\n\tfamily: '{stale}'  # mine\n")).is_none());
		// shcl reads a bare comma list as another string, so it never matches
		assert!(migrate_config_text(&format!("font:\n\tfamily: {stale}\n")).is_none());
	}

	// An active line's path is the one shcl reads, whatever comments sit above it.
	// A save moves a comment to the depth of the setting below it, so a path taken
	// from the comment changed at the first save.
	// Test ID: EpZCRku
	#[test]
	fn an_active_line_takes_no_path_from_a_comment() {
		let setting = |text: &str, at: usize| {
			walk_settings(text)
				.into_iter()
				.find_map(|w| match w {
					WalkLine::Setting {
						index,
						path,
						active,
						..
					} if index == at => Some((path, active)),
					_ => None,
				})
				.unwrap_or_else(|| panic!("no setting on line {at} of {text:?}"))
		};
		// a commented-out block still names its children
		for text in [
			"wallpaper:\n\t# rotate:\n\t\t# enabled: true\n",
			"wallpaper:\r\n\t# rotate:\r\n\t\t# enabled: true\r\n",
		] {
			assert_eq!(
				setting(text, 2),
				("wallpaper.rotate.enabled".to_string(), false),
				"{text:?}"
			);
		}
		let cases = [
			(
				"colors:\n\t# x:\n\t\tfocus: \"#112233\"\n",
				2,
				"colors.focus",
			),
			(
				"colors:\n\tbackground: \"#000000\"\n# see: below\n\tfocus: \"#112233\"\n",
				3,
				"colors.focus",
			),
			("# old:\n\tfont_family: \"x\"\n", 1, "font_family"),
		];
		for (lf, at, want) in cases {
			for text in [lf.to_string(), lf.replace('\n', "\r\n")] {
				let (path, active) = setting(&text, at);
				assert!(active, "{text:?}");
				assert_eq!(path, want, "{text:?}");
				assert!(
					shcl::Document::parse(&text).lines(want).contains(&(at + 1)),
					"shcl reads line {} of {text:?} as {want}",
					at + 1
				);
			}
		}
	}

	// Off since `saved_paths` judges by what a save writes. It compared against
	// `to_canonical()`, which moves the column-0 comment into the block, but a
	// save that keeps the lines leaves it where it is, so the rename fires
	// before and after a save alike and `colors.focus` is renamed.
	// `a_commented_new_name_counts_where_the_save_that_runs_puts_it` covers it.
	// // Whether a rename's new name is already present is judged where a save puts
	// // a commented line, or the first save turns the rename on or off.
	// // Test ID: EpZCS14
	// #[test]
	// fn a_commented_new_name_counts_where_a_save_puts_it() {
	// 	let saved = |t: &str| shcl::Document::parse(t).to_canonical();
	// 	let load = |t: &str| {
	// 		shcl::Document::parse(&migrate_config_text(t).unwrap_or_else(|| t.to_string()))
	// 	};
	// 	let blocked =
	// 		"colors:\n\tfocus: \"#112233\"\n# highlight: \"#aabbcc\"\n\tbackground: \"#000000\"\n";
	// 	let free = "colors:\n\tbackground: \"#000000\"\n\tfocus: \"#112233\"\n";
	// 	for ending in ["\n", "\r\n"] {
	// 		let blocked = blocked.replace('\n', ending);
	// 		assert!(
	// 			saved(&blocked).contains("\t# highlight:"),
	// 			"a save no longer moves this comment, so the case proves nothing"
	// 		);
	// 		let (raw, after) = (load(&blocked), load(&saved(&blocked)));
	// 		for path in ["colors.focus", "colors.highlight"] {
	// 			assert_eq!(
	// 				raw.get_string(path),
	// 				after.get_string(path),
	// 				"{path} loads the same before and after a save ({ending:?})"
	// 			);
	// 		}
	// 		assert_eq!(raw.get_string("colors.focus").as_deref(), Ok("#112233"));
	//
	// 		// nothing blocks the rename here, so it still fires
	// 		let free = free.replace('\n', ending);
	// 		for doc in [load(&free), load(&saved(&free))] {
	// 			assert_eq!(
	// 				doc.get_string("colors.highlight").as_deref(),
	// 				Ok("#112233"),
	// 				"{ending:?}"
	// 			);
	// 		}
	// 	}
	// }

	// Off since `saved_paths` judges by the canonical form again. It held that a
	// save keeps a column-0 comment at column 0, but shcl falls back to the
	// canonical form by what a save changes, and that moves the comment into the
	// block. `a_commented_new_name_counts_the_same_whichever_save_runs` covers it.
	// // A rename's new name counts where the save that actually runs leaves a
	// // commented line. A save that keeps the lines leaves a column-0 comment at
	// // column 0, so it never blocks the rename, before a save or after one.
	// // Test ID: ErUrgUh
	// #[test]
	// fn a_commented_new_name_counts_where_the_save_that_runs_puts_it() {
	// 	let load = |t: &str| {
	// 		shcl::Document::parse(&migrate_config_text(t).unwrap_or_else(|| t.to_string()))
	// 	};
	// 	// what a Settings save of an unrelated value writes
	// 	let saved = |t: &str| {
	// 		let mut doc = parse_kept(t);
	// 		assert!(doc.set_string("colors.background", "#010101"));
	// 		saved_text(&doc)
	// 	};
	// 	let column_0 =
	// 		"colors:\n\tfocus: \"#112233\"\n# highlight: \"#aabbcc\"\n\tbackground: \"#000000\"\n";
	// 	for ending in ["\n", "\r\n"] {
	// 		let text = column_0.replace('\n', ending);
	// 		let after = saved(&text);
	// 		assert!(
	// 			after.contains(&format!("{ending}# highlight:")),
	// 			"the save keeps the comment where it is: {after:?}"
	// 		);
	// 		for doc in [load(&text), load(&after)] {
	// 			assert_eq!(
	// 				doc.get_string("colors.highlight").as_deref(),
	// 				Ok("#112233"),
	// 				"{ending:?}"
	// 			);
	// 		}
	// 	}
	// 	// a commented new name inside the block still blocks it, before and after
	// 	let blocked = "colors:\n\tfocus: \"#112233\"\n\t# highlight: \"#aabbcc\"\n\tbackground: \"#000000\"\n";
	// 	for doc in [load(blocked), load(&saved(blocked))] {
	// 		assert_eq!(doc.get_string("colors.focus").as_deref(), Ok("#112233"));
	// 		assert!(doc.get_string("colors.highlight").is_err());
	// 	}
	// }

	// A rename's new name counts the same whichever save runs. shcl keeps the
	// lines or falls back to the canonical form by what the save changes, so one
	// file can get either, and the canonical form moves a column-0 comment into
	// the block. Here a font size save keeps the lines, and a window size save
	// falls back past the space-indented comment that closes `window:`.
	// Test ID: ErfAH9f
	#[test]
	fn a_commented_new_name_counts_the_same_whichever_save_runs() {
		let load = |t: &str| shcl::Document::parse(&next_launch_text(t));
		let file = "window:\n\trows: 30\n    # x:\ncolors:\n\tfocus: \"#112233\"\n# highlight: \"#aabbcc\"\n\tbackground: \"#000000\"\n";
		for ending in ["\n", "\r\n"] {
			let text = file.replace('\n', ending);
			let mut font = parse_kept(&text);
			assert!(font.set_float("font.size", 15.5));
			let (font, kept) = font.to_text_keep_lines();
			assert!(kept, "the font size save keeps the lines: {font:?}");
			let mut size = parse_kept(&text);
			assert!(size.set_int("window.columns", 100));
			let (size, kept) = size.to_text_keep_lines();
			assert!(
				!kept && size.contains("\n\t# highlight:"),
				"the window size save falls back and moves the comment: {size:?}"
			);
			let before = load(&text);
			for after in [load(&font), load(&size)] {
				for path in ["colors.focus", "colors.highlight"] {
					assert_eq!(
						before.get_string(path),
						after.get_string(path),
						"{path} {ending:?}"
					);
				}
			}
			// the canonical form has the comment in the block, so it blocks
			assert_eq!(before.get_string("colors.focus").as_deref(), Ok("#112233"));
		}
		// a commented new name inside the block blocks it, before and after
		let blocked = "colors:\n\tfocus: \"#112233\"\n\t# highlight: \"#aabbcc\"\n\tbackground: \"#000000\"\n";
		let mut doc = parse_kept(blocked);
		assert!(doc.set_string("colors.background", "#010101"));
		for doc in [load(blocked), load(&saved_text(&doc))] {
			assert_eq!(doc.get_string("colors.focus").as_deref(), Ok("#112233"));
			assert!(doc.get_string("colors.highlight").is_err());
		}
	}

	// A Settings save may tidy quotes and indentation, and the launch after it
	// must load every value as the launch before it did.
	// Test ID: EpZCS15
	#[test]
	fn a_settings_save_moves_no_value_at_the_next_launch() {
		type Reader = fn(&Settings) -> String;
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_savemoves_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());

		let stale = SUPERSEDED_FONT_STACKS[0];
		let template: Vec<String> = default_config().lines().map(str::to_string).collect();
		let line_of = |path: &str, active: bool| {
			walk_settings(default_config())
				.into_iter()
				.find_map(|w| match w {
					WalkLine::Setting {
						index,
						path: p,
						active: a,
						..
					} if p == path && a == active => Some(index),
					_ => None,
				})
				.unwrap_or_else(|| panic!("no {path} (active {active}) in the template"))
		};
		let respell = |lines: &mut Vec<String>, path: &str, value: &str| {
			let at = line_of(path, true);
			let line = &lines[at];
			let indent = &line[..line.len() - line.trim_start().len()];
			let key = line_setting_key(line).unwrap();
			lines[at] = format!("{indent}{key}: {value}");
		};
		let joined = |lines: &[String]| lines.join("\n") + "\n";

		let mut nested = template.clone();
		respell(&mut nested, "font.family", &format!("'{stale}'"));
		respell(&mut nested, "font.use_system_family", "false");

		let font: Reader = |s| format!("{:?}", s.font_family);
		#[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
		let mut cases: Vec<(&str, String, Reader, bool)> = vec![
			(
				"an old default font list in single quotes",
				joined(&nested),
				font,
				false,
			),
			(
				"a pre-nesting file with that list",
				format!("font_family: '{stale}'\nuse_system_font: false\n"),
				font,
				false,
			),
		];
		// the rename reach needs a launch whose writes deferred, and only Linux
		// can tell that the file is held
		#[cfg(target_os = "linux")]
		{
			let focus = line_of("colors.focus", false);
			let highlight = line_of("colors.highlight", false);
			let colors = line_of("colors", true);
			let mut lines: Vec<String> = template
				.iter()
				.enumerate()
				.filter(|(at, _)| *at != focus && *at != highlight)
				.map(|(_, line)| line.clone())
				.collect();
			let header = lines
				.iter()
				.position(|line| *line == template[colors])
				.unwrap();
			lines.insert(header + 1, "\t# x:".to_string());
			lines.insert(header + 2, "\t\tfocus: \"#112233\"".to_string());
			cases.push((
				"a renamed color under a commented heading",
				joined(&lines),
				|s| format!("{:?} {:?}", s.focus, s.highlight),
				true,
			));
		}

		// every case runs before the verdict, so a failure names all that moved
		let mut moved: Vec<String> = Vec::new();
		for (what, text, reader, held) in cases {
			let canonical = shcl::Document::parse(&text).to_canonical();
			let respelled = if held {
				text.contains("\n\t# x:\n\t\tfocus") && !canonical.contains("\n\t# x:\n\t\tfocus")
			} else {
				text.contains(&format!("'{stale}'")) && canonical.contains(&format!("\"{stale}\""))
			};
			assert!(
				respelled,
				"{what}: a save no longer respells this line, so the case proves nothing"
			);

			std::fs::write(&path, &text).unwrap();
			let launched = if held {
				let hold = std::fs::File::open(&path).unwrap();
				let mut child = std::process::Command::new("sleep")
					.arg("30")
					.stdin(std::process::Stdio::from(hold))
					.spawn()
					.unwrap();
				let seen = (0..100).any(|_| {
					std::thread::sleep(std::time::Duration::from_millis(20));
					config_open_elsewhere(&path)
				});
				let launched = reload_from_disk();
				let _ = child.kill();
				let _ = child.wait();
				assert!(seen, "{what}: the holder never showed up");
				launched
			} else {
				reload_from_disk()
			};
			let before = reader(&launched);

			let mut edited = launched.clone();
			edited.font_size += 1.0;
			assert!(persist(&launched, &edited), "{what}: the save was refused");
			let after = reader(&reload_from_disk());
			if after != before {
				moved.push(format!("{what}: {before} -> {after}"));
			}
		}
		let _ = std::fs::remove_dir_all(&dir);
		assert!(
			moved.is_empty(),
			"loads as it did before the save:\n{}",
			moved.join("\n")
		);
	}

	// A commented line still echoing an outgoing default is brought up to the
	// template's current one; an active line, or one the user annotated, is theirs.
	// Every entry refreshes, including a second one for a path whose default has
	// been retuned twice - the lookup must not stop at the first match.
	// Test ID: Elzq5VQ
	#[test]
	fn migrate_refreshes_a_superseded_commented_default() {
		// the path's blocks, one indent level each, with the leaf last
		let nest = |path: &str, line: &str| {
			let parts: Vec<&str> = path.split('.').collect();
			let leaf_depth = parts.len() - 1;
			let mut out: Vec<String> = parts[..leaf_depth]
				.iter()
				.enumerate()
				.map(|(depth, block)| "\t".repeat(depth) + block + ":")
				.collect();
			out.push("\t".repeat(leaf_depth) + line);
			out.join("\n") + "\n"
		};
		for (path, stale) in SUPERSEDED_DEFAULTS {
			let leaf = path.rsplit('.').next().unwrap();
			let current = setting_lines(default_config())
				.into_iter()
				.find_map(|(name, line)| (name == *path).then_some(line))
				.unwrap_or_else(|| panic!("{path} has no template line"));
			let out = migrate_config_text(&nest(path, &format!("# {leaf}: {stale}")))
				.unwrap_or_else(|| panic!("{path}: stale default should be refreshed"));
			assert!(out.lines().any(|l| l == current), "{path}: {out:?}");
			// their own choice, either way they made it
			assert!(migrate_config_text(&nest(path, &format!("{leaf}: {stale}"))).is_none());
			let noted = nest(path, &format!("# {leaf}: {stale}  ## mine"));
			assert!(migrate_config_text(&noted).is_none(), "{path}");
		}
	}

	// The shipped folder names the usual place in this platform's spelling, and
	// the template's commented line has to say the same, or the first save
	// rewrites the file just written (G69, G72).
	// Test ID: Er1vmpM
	#[test]
	fn the_shipped_wallpaper_folder_is_this_platforms_usual_place() {
		let want = if cfg!(windows) {
			r"%LOCALAPPDATA%\silkterm\wallpaper"
		} else if cfg!(target_os = "macos") {
			"$HOME/Library/Application Support/silkterm/wallpaper"
		} else {
			"$XDG_CONFIG_HOME/silkterm/wallpaper"
		};
		assert_eq!(WALLPAPER_DIR_TOKEN, want);
		assert!(WALLPAPER_DIR_TOKEN.contains(APP_DIR));
		assert_eq!(
			Settings::default().wallpaper_folder_raw,
			WALLPAPER_DIR_TOKEN
		);
		let line = format!("folder: \"{}\"  ## Default", wallpaper_dir_escaped());
		assert!(
			default_config().contains(&line),
			"template says something else"
		);
		// the Windows spelling keeps its backslashes through a read, whatever
		// box reads it
		let doc = shcl::Document::parse("folder: \"%LOCALAPPDATA%\\\\silkterm\\\\wallpaper\"\n");
		assert_eq!(
			doc.get_string("folder").unwrap(),
			r"%LOCALAPPDATA%\silkterm\wallpaper"
		);
	}

	// Where the usual place is: the data dir, which on Windows is Local, since a
	// pack is bulk and has no business roaming. `--config` and XDG_CONFIG_HOME
	// keep everything in one tree.
	// Test ID: Er1vmpN
	#[test]
	fn each_platform_keeps_its_wallpaper_where_it_keeps_bulk_data() {
		let config = PathBuf::from("/c/silkterm");
		let local = PathBuf::from("C:/Users/u/AppData/Local");
		let answer = |layout, one_tree, local: Option<&std::path::Path>| {
			data_dir_for(layout, one_tree, local, Some(config.clone()))
		};
		assert_eq!(
			answer(Layout::Windows, false, Some(&local)),
			Some(local.join(APP_DIR))
		);
		assert_eq!(
			answer(Layout::Windows, true, Some(&local)),
			Some(config.clone())
		);
		assert_eq!(answer(Layout::Windows, false, None), Some(config.clone()));
		assert_eq!(
			answer(Layout::MacOs, false, Some(&local)),
			Some(config.clone())
		);
		assert_eq!(
			answer(Layout::Xdg, false, Some(&local)),
			Some(config.clone())
		);
	}

	// The same on this box, end to end. A stocked folder in the usual place is
	// found with the value at its default, commented, uncommented or emptied, and
	// under the older spellings. A named image outranks it, and a named folder
	// outranks the image.
	// Test ID: Er1vmpO
	#[test]
	fn the_default_wallpaper_folder_is_found_in_the_usual_place() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_wpdir_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let write = |text: &str| std::fs::write(&path, text).unwrap();
		let folder = |text: &str| {
			let text = text.replace('\\', r"\\");
			format!("wallpaper:\n\trotate:\n\t\tfolder: \"{text}\"\n")
		};

		write(default_config());
		assert_eq!(
			load().wallpaper_folder,
			None,
			"nothing there, nothing found"
		);
		for spelling in ["wallpaper", "wallpapers", "backgrounds"] {
			let stocked = dir.join(spelling);
			std::fs::create_dir_all(&stocked).unwrap();
			for (how, text) in [
				("commented", default_config().to_string()),
				("uncommented", folder(WALLPAPER_DIR_TOKEN)),
				("emptied", folder("")),
			] {
				write(&text);
				let s = load();
				assert_eq!(
					s.wallpaper_folder.as_ref(),
					Some(&stocked),
					"{spelling}, {how}"
				);
				assert!(s.wallpaper_folder_auto, "{spelling}, {how}");
				assert_eq!(s.wallpaper_folder_raw, WALLPAPER_DIR_TOKEN, "{how}");
			}
			if spelling != "backgrounds" {
				std::fs::remove_dir(&stocked).unwrap();
			}
		}

		write("wallpaper:\n\timage: /x.png\n");
		assert_eq!(load().wallpaper_folder, None, "a named image outranks it");
		// "/elsewhere" is rooted but not absolute on Windows, which puts it on a drive
		let elsewhere = if cfg!(windows) {
			"C:/elsewhere"
		} else {
			"/elsewhere"
		};
		write(&format!(
			"wallpaper:\n\timage: /x.png\n\trotate:\n\t\tfolder: {elsewhere}\n"
		));
		let s = load();
		assert_eq!(s.wallpaper_folder, Some(PathBuf::from(elsewhere)));
		assert!(!s.wallpaper_folder_auto);
		assert_eq!(s.wallpaper_folder_raw, elsewhere);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// With no image named and nothing rotating, a wallpaper in one of the usual
	// places is found, the current spelling first. A relative name is taken from
	// the config's own folder.
	// Test ID: Er2UFeU
	#[test]
	fn a_wallpaper_in_the_usual_place_is_found() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_wpfile_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(dir.join("backgrounds")).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());

		assert_eq!(resolve_wallpaper(None), None, "nothing there yet");
		let older = dir.join("backgrounds").join("background.png");
		std::fs::write(&older, b"").unwrap();
		assert_eq!(resolve_wallpaper(None), Some(older.clone()));
		std::fs::write(&path, "wallpaper:\n\trotate:\n\t\tenabled: false\n").unwrap();
		assert_eq!(load().wallpaper, Some(older), "found at launch");

		std::fs::create_dir_all(dir.join("wallpaper")).unwrap();
		let current = dir.join("wallpaper").join("wallpaper.jpg");
		std::fs::write(&current, b"").unwrap();
		assert_eq!(resolve_wallpaper(None), Some(current));

		assert_eq!(
			resolve_wallpaper(Some("x.png".to_string())),
			Some(dir.join("x.png"))
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// --reset-config moves the file aside and never overwrites an earlier
	// backup; with no file left there is nothing to do.
	// Test ID: Er2UFeV
	#[test]
	fn a_reset_keeps_every_earlier_config() {
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir = crate::testdir::run_dir().join(format!("silkterm_reset_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		set_config_override(path.clone());
		let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();

		std::fs::write(&path, "a").unwrap();
		assert_eq!(reset_config(), Some(dir.join("config.shcl.bak")));
		assert!(!path.exists(), "the config was moved, not copied");
		assert_eq!(read("config.shcl.bak"), "a");

		std::fs::write(&path, "b").unwrap();
		assert_eq!(reset_config(), Some(dir.join("config.shcl.bak2")));
		assert_eq!(read("config.shcl.bak"), "a", "the first backup kept");
		assert_eq!(read("config.shcl.bak2"), "b");

		assert_eq!(reset_config(), None);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Hiding a lone tab is off by default and is not a dialog row, so only the
	// View menu's save reaches the file.
	// Test ID: Er2UFeW
	#[test]
	fn hide_single_tab_is_off_and_survives_a_save() {
		assert!(!Settings::default().hide_single_tab);
		let _guard = super::test_config_lock();
		let _ = settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_hidetab_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(&path, default_config()).unwrap();
		set_config_override(path.clone());

		let orig = load();
		assert!(!orig.hide_single_tab);
		let mut new = orig.clone();
		new.hide_single_tab = true;
		assert!(persist(&orig, &new));
		assert!(load().hide_single_tab);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// An existing config's commented line names the old empty default, and is
	// refreshed to the place it meant.
	// Test ID: Er1vmpP
	#[test]
	fn an_existing_config_learns_where_the_wallpaper_folder_is() {
		let out = migrate_config_text("wallpaper:\n\trotate:\n\t\t# folder: \"\"  ## Default\n")
			.expect("the outgoing default should be refreshed");
		assert!(
			out.contains(&format!(
				"# folder: \"{}\"  ## Default",
				wallpaper_dir_escaped()
			)),
			"{out:?}"
		);
	}

	// The table above is kept by hand, so nothing can catch an entry that was
	// simply never added. This names the one default that changed most recently.
	// Test ID: Erg8g2c
	#[test]
	fn an_existing_config_learns_that_the_idle_release_ships_on() {
		let out = migrate_config_text("window:\n\t# idle_release: false  ## Default\n")
			.expect("the outgoing default should be refreshed");
		assert!(out.contains("# idle_release: true  ## Default"), "{out:?}");
		// an active line is their own choice
		assert!(migrate_config_text("window:\n\tidle_release: false\n").is_none());
	}

	// Test ID: EreA6Db
	#[test]
	fn an_existing_config_learns_that_copy_on_select_ships_on() {
		let out = migrate_config_text("shell:\n\t# copy_on_select: false  ## Default\n")
			.expect("the outgoing default should be refreshed");
		assert!(
			out.contains("# copy_on_select: true  ## Default"),
			"{out:?}"
		);
	}

	// Test ID: EqSm2Rl
	#[test]
	fn an_existing_config_learns_that_wallpaper_text_colors_ship_on() {
		let out = migrate_config_text("colors:\n\t# from_wallpaper: false  ## Default\n")
			.expect("the outgoing default should be refreshed");
		assert!(
			out.contains("# from_wallpaper: true  ## Default"),
			"{out:?}"
		);
	}

	// The coverage exponent is gone and its number means nothing as an amount,
	// so the line goes rather than carrying a value over. An active one has to
	// go too: left in place it would read as a setting nothing answers for.
	// Test ID: EqWXhBA
	#[test]
	fn an_existing_config_loses_the_coverage_exponent() {
		for line in [
			"\t# dark_on_light_gamma: 0.65  ## Default\n",
			"\tdark_on_light_gamma: 0.5\n",
		] {
			let out = migrate_config_text(&format!("text:\n{line}\toutline: 1.0\n"))
				.expect("the retired line should go");
			assert!(!out.contains("dark_on_light_gamma"), "{out:?}");
			assert!(out.contains("outline: 1.0"), "{out:?}");
		}
	}

	// The walker is what gives every line its full nested path - the whole
	// line-oriented machinery keys on it.
	// Test ID: ElmIYG2
	#[test]
	fn walker_resolves_nested_paths() {
		let text = "top: 1\nwallpaper:\n\t# enabled: true\n\trotate:\n\t\t# folder: \"x\"\n\t\tinterval_s: 2.0\n\t# opacity: 0.5\ncolors.focus: \"#123456\"\n";
		let got: Vec<(String, bool)> = walk_settings(text)
			.into_iter()
			.filter_map(|w| match w {
				WalkLine::Setting { path, active, .. } => Some((path, active)),
				_ => None,
			})
			.collect();
		let want = [
			("top", true),
			("wallpaper", true),
			("wallpaper.enabled", false),
			("wallpaper.rotate", true),
			("wallpaper.rotate.folder", false),
			("wallpaper.rotate.interval_s", true),
			("wallpaper.opacity", false),
			("colors.focus", true),
		];
		let want: Vec<(String, bool)> = want.iter().map(|(p, a)| ((*p).to_string(), *a)).collect();
		assert_eq!(got, want);
	}

	// A dialog save on a fresh nested config: the new active value goes inside
	// its block, and everything else in the file is left exactly as it stands.
	// The whole-file diff is the strong form of that - a save may only ever add
	// the lines it was asked to add - and the spot checks below say WHICH shapes
	// are being relied on, so a failure names the one that moved.
	// Test ID: ElmIYG3
	#[test]
	fn a_save_keeps_nested_comment_layout() {
		let mut doc = shcl::Document::parse(default_config());
		assert!(doc.set_float("wallpaper.opacity", 0.5));
		assert!(doc.set_bool("text.scrim.enabled", false));
		let out = doc.to_canonical();

		let added: Vec<&str> = out
			.lines()
			.filter(|l| !default_config().lines().any(|d| d == *l))
			.collect();
		assert_eq!(
			added,
			vec!["\topacity: 0.5", "\t\tenabled: false"],
			"a save changed lines it was not asked to:\n{out}"
		);

		assert!(
			out.contains("\topacity: 0.5"),
			"new value lands in the wallpaper block:\n{out}"
		);
		assert!(
			out.contains("\t\tenabled: false"),
			"new value lands in the scrim block:\n{out}"
		);
		assert!(
			out.contains("\t\t# random: true  ## Default"),
			"commented defaults keep their nesting depth:\n{out}"
		);
		assert!(
			out.contains("\t# blur: 10.0  ## Default"),
			"comment runs after the change point keep depth too:\n{out}"
		);
		assert!(
			out.contains("\n\n## •"),
			"blank lines before section rules survive:\n{out}"
		);
		assert!(
			out.contains("\t# dialog_foreground: \"#e2e2ea\"  ## Default"),
			"the trailing colors block keeps its indentation:\n{out}"
		);
	}

	// A line whose indentation matches no level is dropped by the parse, and a
	// save would then delete it. The write refuses instead: a hand-written line
	// is worth more than the one setting the save was carrying.
	// Test ID: EoGez3w
	#[test]
	fn a_save_that_would_drop_a_line_is_refused() {
		let _guard = super::test_config_lock();
		let dir = crate::testdir::run_dir().join(format!("silk-lostgate-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		let text = "window:\n\t\tmargin: 8.0\n\trows: 40\n";
		std::fs::write(&path, text).unwrap();
		let mut doc = shcl::Document::parse(text);
		assert_eq!(
			doc.lost_count(),
			1,
			"the line stepping back to no level is the one dropped"
		);
		doc.put_float("window.margin", 4.0);
		let _ = write_doc(&path, &doc);
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			text,
			"the file was rewritten despite the dropped line"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The launch names a line it cannot read, and the rating then writes two
	// lines above it once the window is up. The last word on the console has to
	// name the line where the file has it after that.
	// Test ID: ErUrgB9
	#[test]
	fn a_rating_write_restates_the_lines_the_launch_named() {
		let _guard = super::test_config_lock();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_cfglines_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		std::fs::write(&path, "window:\n\t\topacity: 1.0\n\tmargin: 4\n").unwrap();
		set_config_override(path.clone());
		let _ = load();
		let said = || {
			LAUNCH_SAID
				.lock()
				.unwrap()
				.clone()
				.map(|(_, said)| said)
				.unwrap_or_default()
		};
		let at = |text: &str| text.lines().position(|l| l == "\tmargin: 4").unwrap() + 1;
		let before = at(&std::fs::read_to_string(&path).unwrap());
		assert!(
			said()
				.iter()
				.any(|m| m.contains(&format!("line {before}:"))),
			"{:?}",
			said()
		);

		let kept = keep_rating(&RatingLines {
			profile: Some("low"),
			rated_hardware: Some("0123456789abcdef"),
			check_next_run: None,
		});
		assert_eq!(kept, Kept::Written);
		let now = at(&std::fs::read_to_string(&path).unwrap());
		assert!(now > before, "the rating went in above the line");
		let said = said();
		for cites in [
			format!("could not be read line {now} "),
			format!("line {now}: indentation"),
			format!("`window.opacity` line {} ", now - 1),
		] {
			assert!(said.iter().any(|m| m.contains(&cites)), "{cites}: {said:?}");
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A stray indented with spaces used to be dropped too, and every save of
	// the file refused. shcl 3.0 keeps it as written, so the save goes through
	// and the line is still there, still reported, and still sets nothing.
	// Test ID: EqtUcp6
	#[test]
	fn a_save_keeps_a_space_indented_stray() {
		let dir = crate::testdir::run_dir().join(format!("silk-keptstray-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		let text = "window:\n\tmargin: 8.0\n  rows: 40\n";
		std::fs::write(&path, text).unwrap();
		let mut doc = shcl::Document::parse(text);
		assert_eq!(doc.lost_count(), 0);
		assert_eq!(unreadable_lines(&doc), vec![3]);
		doc.put_float("window.margin", 4.0);
		assert!(write_doc(&path, &doc));
		assert_eq!(
			std::fs::read_to_string(&path).unwrap(),
			"window:\n\tmargin: 4\n  rows: 40\n"
		);
		let said = config_complaints(text);
		assert!(
			said.iter()
				.any(|m| m.contains("line 3") && m.contains("set nothing")),
			"{said:?}"
		);
		assert!(
			!said.iter().any(|m| m.contains("cannot be saved")),
			"{said:?}"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A new key added to a group the file already has PART of must go beside
	// its siblings INSIDE their block, at the right depth, NOT be appended with
	// a second copy of the group's comment block - that paragraph is already in
	// the file, attached to the siblings.
	// Test ID: EllvVHE
	#[test]
	fn a_straggler_lands_beside_its_siblings_not_with_a_second_paragraph() {
		let path = crate::testdir::run_dir().join("silkterm_backfill_straggler_test.shcl");
		// interval_s is missing from an otherwise-present rotation block
		let drifted = "wallpaper:\n\
			\n\
			\t## Rotation\n\
			\t## Rotate the wallpaper through a folder of images.\n\
			\trotate:\n\
			\t\t# enabled: true  ## Default\n\
			\t\t# folder: \"wallpaper/\"  ## Default\n\
			\t\t# random: true  ## Default\n";
		std::fs::write(&path, drifted).unwrap();
		backfill_config(&path);
		let out = std::fs::read_to_string(&path).unwrap();

		let paragraphs = out.matches("Rotate the wallpaper through a folder").count();
		assert_eq!(paragraphs, 1, "comment block duplicated:\n{out}");
		let straggler = out
			.find("\t\t# interval_s: 0.0")
			.expect("straggler backfilled at depth");
		let folder = out.find("\t\t# folder:").expect("sibling still there");
		let random = out.find("\t\t# random:").expect("sibling still there");
		assert!(
			folder < straggler && straggler < random,
			"template order among siblings not kept:\n{out}"
		);
		// a group the file has never seen still arrives whole, comments and all.
		// Anchored on the shape, not the wording: two comment passes have broken
		// this by rewording the line it used to quote.
		let fit = out
			.find("\t# default_fit: \"stretch\"  ## Default")
			.expect("new group backfilled");
		assert!(
			out[..fit]
				.lines()
				.next_back()
				.is_some_and(|l| l.trim_start().starts_with("##")),
			"new group needs its comments:\n{out}"
		);
		// and a wholly-missing top-level section arrives as a block
		assert!(
			out.contains("font:") && out.contains("\tuse_system_family: true"),
			"missing sections backfilled whole:\n{out}"
		);

		backfill_config(&path);
		assert_eq!(
			out,
			std::fs::read_to_string(&path).unwrap(),
			"backfill not idempotent"
		);
		let _ = std::fs::remove_file(&path);
	}

	// The real on-disk load pipeline (convert -> migrate -> backfill) on a
	// pre-nesting config: values end at their nested paths, the original file
	// is kept as .bak, missing keys arrive, and the chain is stable.
	// Test ID: ElmIYG4
	#[test]
	fn pipeline_convert_migrate_backfill_on_disk() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_pipeline_{}", std::process::id()));
		let _ = std::fs::create_dir_all(&dir);
		let path = dir.join("config.shcl");
		let drifted = "scrollback: 5000\n\
			cursor_size_vertical: 40\n\
			cursor_shape: \"block\"\n\
			margin: 12.0\n\
			opacity: 0.8\n\
			\n\
			themes.mine.dark.background: \"#010203\"\n\
			\n\
			colors.focus: \"#abcdef\"\n";
		std::fs::write(&path, drifted).unwrap();
		convert_legacy_config(&path);
		migrate_config(&path);
		backfill_config(&path);
		let out = std::fs::read_to_string(&path).unwrap();

		assert!(
			!out.contains("cursor_shape"),
			"obsolete key dropped:\n{out}"
		);
		assert!(
			out.contains("\t\theight: 40"),
			"renamed key kept its value:\n{out}"
		);
		assert!(
			out.contains("\tmargin: 12.0") && out.contains("\topacity: 0.8"),
			"values landed nested:\n{out}"
		);
		assert!(out.contains("\tscrollback: 5000"), "scrollback value kept");
		assert!(out.contains("\tfocus: \"#abcdef\""), "color override kept");
		assert!(
			out.contains("themes.mine.dark.background"),
			"unknown key kept"
		);
		assert!(
			out.contains("\tuse_system_family: true"),
			"missing key present with its default"
		);

		// stable: a second pass changes nothing.
		convert_legacy_config(&path);
		migrate_config(&path);
		backfill_config(&path);
		assert_eq!(
			out,
			std::fs::read_to_string(&path).unwrap(),
			"pipeline not idempotent"
		);
		let _ = std::fs::remove_file(&path);
		let _ = std::fs::remove_file(dir.join("config.shcl.bak"));
	}

	// The scan must not offer the loader a file it can't decode: `image` is built
	// with only png + jpeg, so a wider list picks a wallpaper that then fails.
	// Test ID: EllHdNA
	#[test]
	fn image_scan_only_accepts_what_the_decoder_has() {
		use std::path::Path;
		for ok in ["a.png", "a.jpg", "a.jpeg", "a.JPG", "a.PNG"] {
			assert!(is_image_file(Path::new(ok)), "{ok} should be scanned");
		}
		for no in ["a.webp", "a.bmp", "a.gif", "a.tiff", "a.tif", "a.txt", "a"] {
			assert!(
				!is_image_file(Path::new(no)),
				"{no} has no decoder - scanning it picks an unloadable wallpaper"
			);
		}
	}

	// The config file is the one thing here a person edits by hand, so it arrives
	// however they left it: half-typed, pasted with the wrong indentation, a key
	// written twice, a value from a config the format used to have. None of that
	// may take the program down, and saving a file the program itself wrote must
	// not change it a second time.
	mod fuzz {
		use super::super::{
			CONFIG_REMOVED, CONFIG_RENAMES, LEGACY_KEYS, RatingLines, SUPERSEDED_FONT_STACKS,
			adopt_default_shell, config_complaints, convert_legacy_config, default_config,
			disabled_text, line_setting_key, migrate_config_text, next_launch_text, parse_kept,
			read_raw, resolve, reverted_text, saved_text, setting_groups, setting_lines,
			walk_settings, with_rating_lines, with_shcl_banner,
		};
		use crate::fuzz;

		// The settings a save case is built from: every renamed and removed path,
		// the font list, and one ordinary setting beside the renames in each of
		// their blocks. Read from the tables, so a new rename joins on its own.
		static SAVE_PATHS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
			let mut out: Vec<String> = CONFIG_RENAMES
				.iter()
				.flat_map(|(old, new)| [*old, *new])
				.chain(CONFIG_REMOVED.iter().copied())
				.chain(["font.family"])
				.map(str::to_string)
				.collect();
			let template: Vec<String> = setting_lines(default_config())
				.into_iter()
				.map(|(path, _)| path)
				.collect();
			for block in ["colors.", "window.", "scroll.", "shell."] {
				if let Some(other) = template
					.iter()
					.find(|p| p.starts_with(block) && !out.contains(p))
				{
					out.push(other.clone());
				}
			}
			out
		});

		// A comment line above a setting, at any depth, in the shapes that have
		// moved a value: a heading, a setting-like note, plain words, and a
		// commented rename target.
		fn save_comment(rng: &mut fuzz::Rng) -> String {
			const INDENTS: [&str; 5] = ["", "\t", "\t\t", "\t\t\t", "    "];
			let indent = rng.pick(&INDENTS);
			let body = match rng.below(4) {
				0 => "# x:".to_string(),
				1 => "# see: below".to_string(),
				2 => "# just words".to_string(),
				_ => {
					let (_, new) = rng.pick(CONFIG_RENAMES);
					let leaf = new.rsplit('.').next().unwrap_or(new);
					format!("# {leaf}: \"#aabbcc\"")
				}
			};
			format!("{indent}{body}\n")
		}

		fn save_value(rng: &mut fuzz::Rng, path: &str) -> String {
			let leaf = path.rsplit('.').next().unwrap_or(path);
			match leaf {
				"family" => {
					let stale = SUPERSEDED_FONT_STACKS[0];
					// single quotes most: the save respells them
					match rng.below(6) {
						0..=2 => format!("'{stale}'"),
						3 => format!("\"{stale}\""),
						4 => stale.to_string(),
						_ => "\"Iosevka\"".to_string(),
					}
				}
				"default" => "pwsh".to_string(),
				"startup_directory" => "C:\\Users\\x".to_string(),
				_ if path.starts_with("colors.") => "\"#112233\"".to_string(),
				_ => (5 + rng.below(300)).to_string(),
			}
		}

		// A file a Settings save could be asked to keep: blocks of renamed, removed
		// and ordinary settings with comments above them at any depth, and now and
		// then the pre-nesting font list. A leaf is never deeper than its siblings,
		// since shcl reads such a line as a child of the setting above it.
		fn save_case(rng: &mut fuzz::Rng) -> String {
			use std::fmt::Write;
			let paths = &*SAVE_PATHS;
			let mut out = String::new();
			// Rarer than the rest, with the shell block below: converting a file or
			// moving the default shell scans every process, and is most of a case's
			// time.
			if rng.chance(16) {
				let _ = writeln!(out, "font_family: '{}'", SUPERSEDED_FONT_STACKS[0]);
				if rng.chance(2) {
					out.push_str("use_system_font: false\n");
				}
			}
			for _ in 0..=rng.below(3) {
				let mut path = rng.pick(paths);
				if path.starts_with("shell.") && rng.chance(2) {
					path = rng.pick(paths);
				}
				let (parent, _) = path.rsplit_once('.').unwrap_or(("", path));
				let heads: Vec<&str> = parent.split('.').collect();
				for (depth, head) in heads.iter().enumerate() {
					let _ = writeln!(out, "{}{head}:", "\t".repeat(depth));
				}
				let siblings: Vec<&String> = paths
					.iter()
					.filter(|p| p.rsplit_once('.').is_some_and(|(up, _)| up == parent))
					.collect();
				let indent = "\t".repeat(heads.len());
				for _ in 0..=rng.below(4) {
					let leaf_path = *rng.pick(&siblings);
					if rng.chance(2) {
						out.push_str(&save_comment(rng));
					}
					let leaf = leaf_path.rsplit('.').next().unwrap_or(leaf_path);
					let value = save_value(rng, leaf_path);
					let _ = writeln!(out, "{indent}{leaf}: {value}");
				}
				if rng.chance(3) {
					out.push_str(&save_comment(rng));
				}
			}
			out
		}

		// What the next launch parses: the conversion and the default-shell move
		// when the file needs them, then the renames, removals and refreshes.
		fn next_load(text: &str) -> shcl::Document {
			use std::sync::atomic::{AtomicU64, Ordering};
			static CASE: AtomicU64 = AtomicU64::new(0);
			let flat = text.lines().any(|line| {
				!line.trim_start().starts_with('#')
					&& line_setting_key(line)
						.is_some_and(|key| LEGACY_KEYS.iter().any(|(old, _)| *old == key))
			});
			let shell = shcl::Document::parse(text)
				.get_string("shell.default")
				.is_ok();
			let text = if flat || shell {
				let dir = crate::testdir::run_dir().join(format!(
					"silkterm_savefuzz_{}_{}",
					std::process::id(),
					CASE.fetch_add(1, Ordering::Relaxed)
				));
				let _ = std::fs::remove_dir_all(&dir);
				std::fs::create_dir_all(&dir).unwrap();
				let path = dir.join("config.shcl");
				std::fs::write(&path, text).unwrap();
				convert_legacy_config(&path);
				adopt_default_shell(&path);
				let out = std::fs::read_to_string(&path).unwrap();
				let _ = std::fs::remove_dir_all(&dir);
				out
			} else {
				text.to_string()
			};
			shcl::Document::parse(&migrate_config_text(&text).unwrap_or(text))
		}

		// A save keeps the lines it can and falls back to the canonical form, which
		// tidies quotes and indentation. After either, the next launch loads every
		// setting the save did not write as the launch before it would have. Cases
		// come from the generator only: a mutated file reaches a line indented under
		// a key that holds a value, which shcl and the walk read differently, and
		// this does not change that.
		fn save_check(case: &[u8]) {
			let text = String::from_utf8_lossy(case).into_owned();
			let raw = parse_kept(&text);
			if raw.lost_count() > 0 {
				return;
			}
			let mut doc = raw.clone();
			if !doc.set_float("font.size", 15.5) {
				return;
			}
			save_check_as(&text, &raw, &doc.to_canonical());
			save_check_as(&text, &raw, &saved_text(&doc));
		}

		fn save_check_as(text: &str, raw: &shcl::Document, saved: &str) {
			let raw_saved = shcl::Document::parse(saved);
			let (then, now) = (next_load(text), next_load(saved));
			// a path none of the four holds reads the same everywhere
			let mut paths: Vec<String> = Vec::new();
			for doc in [raw, &raw_saved, &then, &now] {
				for path in doc.paths() {
					if !paths.contains(&path) {
						paths.push(path);
					}
				}
			}
			for path in paths {
				if path == "font" || path == "font.size" {
					continue;
				}
				let read = |doc: &shcl::Document| (doc.get_string(&path), doc.count(&path));
				if read(raw) != read(&raw_saved) {
					continue;
				}
				assert_eq!(
					read(&then),
					read(&now),
					"{path} loads differently at the next launch after a save\nfile:\n{text}\nsaved:\n{saved}"
				);
			}
		}

		// Test ID: EpZCS16
		#[test]
		fn a_settings_save_moves_no_value_at_the_next_launch() {
			// the next launch adopts a default shell, a save that can be refused
			let _guard = super::super::test_config_lock();
			let corpus = fuzz::corpus("config");
			for case in &corpus {
				save_check(case);
			}
			fuzz::soak("config-save", |seed| {
				let case = save_case(&mut fuzz::Rng::new(seed));
				// taken from the seed rather than the generator, so no other case moves
				let case = if seed % 4 == 3 {
					case.replace('\n', "\r\n")
				} else {
					case
				};
				save_check(case.as_bytes());
			});
		}

		// A generator that stops making the shapes that moved a value passes
		// forever, so each is looked for by name.
		// Test ID: EpZCS17
		#[test]
		fn the_save_generator_reaches_every_shape() {
			let indent = |line: &str| line.len() - line.trim_start().len();
			let comment = |line: &str| line.trim_start().starts_with('#');
			let (mut flat, mut quoted, mut under, mut above) = (false, false, false, false);
			// Read through shcl and the raw text, never the walk, so the check does
			// not lean on the code it gates.
			for seed in 0..200 {
				let text = save_case(&mut fuzz::Rng::new(seed));
				let doc = shcl::Document::parse(&text);
				let lines: Vec<&str> = text.lines().collect();
				quoted |= text.contains(&format!("'{}'", SUPERSEDED_FONT_STACKS[0]));
				flat |= LEGACY_KEYS.iter().any(|(old, _)| doc.count(old) > 0);
				// an active old name below a commented heading that is shallower
				// than it, with no active line between them that closes the block
				for (old, _) in CONFIG_RENAMES {
					for at in doc.lines(old).into_iter().filter(|n| *n > 0) {
						let line = lines[at - 1];
						for up in lines[..at - 1].iter().rev() {
							if indent(up) >= indent(line) {
								continue;
							}
							if !comment(up) {
								break;
							}
							if up.trim_end().ends_with(':') {
								under = true;
							}
						}
					}
				}
				// a commented new name shallower than the setting below it
				for pair in lines.windows(2) {
					let key = line_setting_key(pair[0]).unwrap_or_default();
					if comment(pair[0])
						&& CONFIG_RENAMES
							.iter()
							.any(|(_, new)| new.rsplit('.').next() == Some(key))
						&& !comment(pair[1])
						&& indent(pair[1]) > indent(pair[0])
					{
						above = true;
					}
				}
			}
			assert!(flat, "no active pre-nesting setting");
			assert!(quoted, "no old font list in single quotes");
			assert!(under, "no old name under a commented heading");
			assert!(above, "no commented new name above a deeper setting");
		}

		// The keys the shipped template actually carries, read out of it rather
		// than listed here, so a new setting joins the fuzz on its own. The file
		// is nested, so these are the leaf names and the block headers.
		static KEYS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
			default_config()
				.lines()
				.filter_map(|line| {
					let bare = line.trim_start().trim_start_matches("# ");
					let key = bare.split(':').next()?.trim();
					let ok = !key.is_empty()
						&& key
							.chars()
							.all(|c| c.is_ascii_lowercase() || c == '_' || c == '.');
					ok.then(|| key.to_string())
				})
				.collect()
		});

		// Values that have broken a reader before, plus the shapes a hand-edited
		// file grows: an unclosed quote, a stray tab, a number too big for any
		// width, a list where a scalar belongs.
		#[rustfmt::skip]
		const VALUES: [&str; 22] = [
			"", " ", "0", "-1", "1e400", "-1e400", "nan", "inf", "99999999999999",
			"true", "TRUE", "yes", "\"", "\"unclosed", "[1, 2", "{", "#", "null",
			"0x10", "1_000", "\t", "a: b",
		];

		// A file, not a line: indentation that changes mid-block, a key written
		// twice with children under the second, a comment where a value belongs.
		fn config(rng: &mut fuzz::Rng) -> Vec<u8> {
			use std::fmt::Write;
			let keys = &*KEYS;
			// Pin the wallpaper so resolving a case does not go looking around the
			// filesystem for one. The parse half sees the wallpaper keys anyway.
			let mut out = String::from("wallpaper:\n\timage: \"/nonexistent/silkfuzz\"\n");
			for _ in 0..=rng.below(24) {
				match rng.below(10) {
					0 => out.push_str("## a comment\n"),
					1 => out.push('\n'),
					2 => {
						out.push_str(&fuzz::text(rng));
						out.push('\n');
					}
					3 => {
						// A block header, then children under it - sometimes under
						// one that was already written above.
						let _ = writeln!(out, "{}:", rng.pick(keys));
						for _ in 0..=rng.below(3) {
							// Mixed indentation on purpose: a pasted block arrives
							// with spaces where the rest of the file has tabs.
							let indent = if rng.chance(3) { "    " } else { "\t" };
							let _ =
								writeln!(out, "{indent}{}: {}", rng.pick(keys), rng.pick(&VALUES));
						}
					}
					_ => {
						let lead = if rng.chance(5) { "\t" } else { "" };
						let _ = writeln!(out, "{lead}{}: {}", rng.pick(keys), rng.pick(&VALUES));
					}
				}
			}
			out.into_bytes()
		}

		fn check(case: &[u8]) {
			let text = String::from_utf8_lossy(case).into_owned();
			let path = std::path::Path::new("fuzz.shcl");

			// Every pure reader over the file's text, in one place: if one of them
			// can be made to panic, the program dies before it draws anything.
			let _ = walk_settings(&text);
			let _ = config_complaints(&text);
			let _ = setting_lines(&text);
			let _ = setting_groups(&text);
			let _ = migrate_config_text(&text);
			let _ = with_shcl_banner(&text);
			let _ = reverted_text(&text, &["font.size", "window.rows"]);
			let _ = disabled_text(&text, &["font.size", "window.rows"]);
			let _ = resolve(read_raw(&text, path).0);

			// Saving is writing the canonical form of what was read. Doing that
			// twice must give the same file, or every save walks the config away
			// from what the person typed.
			let once = shcl::Document::parse(&text).to_canonical();
			let twice = shcl::Document::parse(&once).to_canonical();
			assert_eq!(twice, once, "a second save changed the file");
		}

		// A generator that quietly stops generating passes forever. This is what
		// says the template is still being read.
		// Test ID: EpQN0oC
		#[test]
		fn the_generator_still_finds_the_settings_it_draws_from() {
			assert!(
				KEYS.len() > 60,
				"only {} keys came out of the template",
				KEYS.len()
			);
			for want in ["size", "scrollback", "opacity", "wallpaper", "performance"] {
				assert!(
					KEYS.iter().any(|k| k == want),
					"no '{want}' in the template"
				);
			}
			let mut rng = fuzz::Rng::new(0);
			let case = String::from_utf8(config(&mut rng)).expect("utf-8");
			assert!(case.len() > 60, "the generator produced {case:?}");
			assert!(
				!CONFIG_RENAMES.is_empty(),
				"no renames left, and the migrated shapes pick from them"
			);
			assert!(
				!SUPERSEDED_FONT_STACKS.is_empty(),
				"no superseded font lists left, and the migrated shapes pick from them"
			);
		}

		// Test ID: EpQN0oD
		#[test]
		fn no_config_file_can_take_the_program_down() {
			let corpus = fuzz::corpus("config");
			for case in &corpus {
				check(case);
			}
			fuzz::soak("config", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				check(&fuzz::input(&mut rng, &corpus, config));
			});
		}

		// Free-form bytes, with no config shape imposed at all. The generator
		// above never emits a lone `:` at depth four or a value that is only a
		// byte-order mark, and the parser has to hold up under those too.
		// Test ID: EpQN0oE
		#[test]
		fn arbitrary_bytes_parse_without_panicking() {
			let corpus = fuzz::corpus("config-bytes");
			for case in &corpus {
				let _ = walk_settings(&String::from_utf8_lossy(case));
			}
			fuzz::soak("config-bytes", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let case = fuzz::input(&mut rng, &corpus, |rng| {
					(0..rng.below(400)).map(|_| rng.byte()).collect()
				});
				let text = String::from_utf8_lossy(&case);
				let _ = walk_settings(&text);
				let _ = config_complaints(&text);
				let _ = migrate_config_text(&text);
				let _ = shcl::Document::parse(&text).to_canonical();
			});
		}

		// A flat file of old names, the newest spelling winning, converts with each
		// value readable at its new path, every block heading still a heading, and
		// no second conversion.
		// Test ID: EpZcBUQ
		#[test]
		fn a_flat_file_carries_every_value_to_its_path() {
			use super::super::{CONFIG_REMOVED, LEGACY_KEYS, converted_config_text};
			use super::active_headings;
			use std::fmt::Write;
			let template = active_headings(default_config());
			fuzz::soak("config-flat", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let mut text = String::new();
				let mut want: std::collections::HashMap<&str, (usize, String)> =
					std::collections::HashMap::new();
				let mut expect = |rank: usize, value: &str| {
					let new = LEGACY_KEYS[rank].1;
					if CONFIG_REMOVED.contains(&new) {
						return;
					}
					let slot = want.entry(new).or_insert((rank, value.to_string()));
					if rank < slot.0 {
						*slot = (rank, value.to_string());
					}
				};
				for i in 0..=rng.below(8) {
					let rank = rng.below(LEGACY_KEYS.len());
					let value = format!("v{seed}x{i}");
					let _ = writeln!(text, "{}: {value}", LEGACY_KEYS[rank].0);
					expect(rank, &value);
				}
				if rng.chance(3) {
					let (_, head) = rng.pick(&template);
					let value = format!("y{seed}");
					let _ = writeln!(text, "{head}: {value}");
					// a block name that is also an old flat name carries like one
					if let Some(rank) = LEGACY_KEYS.iter().position(|(old, _)| old == head) {
						expect(rank, &value);
					}
				}
				// a current shell list rides along on some seeds, and carries whole
				let shells: Vec<crate::shells::ShellEntry> = if rng.chance(3) {
					(0..=rng.below(3))
						.map(|i| crate::shells::ShellEntry {
							slug: format!("s{i}"),
							title: format!("t{seed}x{i}"),
							command: format!("/bin/c{i}"),
							active: rng.chance(2),
							comment: String::new(),
							last_seen: String::new(),
						})
						.collect()
				} else {
					Vec::new()
				};
				if !shells.is_empty() {
					let mut doc = shcl::Document::parse("");
					super::super::write_shells(&mut doc, &[], &shells);
					text.insert_str(0, &doc.to_canonical());
				}
				let text = if seed % 4 == 3 {
					text.replace('\n', "\r\n")
				} else {
					text
				};
				let out = converted_config_text(&text).expect("a flat file converts");
				let doc = shcl::Document::parse(&out);
				assert_eq!(doc.lost_count(), 0, "file:\n{text}\nconverted:\n{out}");
				assert!(
					super::super::read_shells(&doc) == shells,
					"shells\nfile:\n{text}\nconverted:\n{out}"
				);
				for (new, (_, value)) in &want {
					assert_eq!(
						doc.get_string(new).ok().as_deref(),
						Some(value.as_str()),
						"{new}\nfile:\n{text}\nconverted:\n{out}"
					);
				}
				let headings: Vec<(usize, String)> = active_headings(&out)
					.into_iter()
					.filter(|(_, h)| h != "shells" && !h.starts_with("shells."))
					.collect();
				assert_eq!(
					headings, template,
					"a heading moved\nfile:\n{text}\nconverted:\n{out}"
				);
				assert_eq!(
					converted_config_text(&out),
					None,
					"converted twice\nfile:\n{text}"
				);
			});
		}

		// Backfill only adds lines, so whatever a file holds, every value that
		// loaded before it loads the same after, or nothing is written. Settings
		// are re-indented at random here, since a line indented deeper than its
		// block needs is what an added line can take over.
		// Test ID: EpZCS18
		#[test]
		fn backfill_changes_nothing_that_loaded() {
			use super::super::backfilled_text;
			fuzz::soak("config-backfill", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let text = String::from_utf8_lossy(&config(&mut rng)).into_owned();
				let text: String = text
					.lines()
					.map(|line| {
						if rng.chance(6) && line.starts_with('\t') && !line.trim().is_empty() {
							format!("\t{line}\n")
						} else {
							format!("{line}\n")
						}
					})
					.collect();
				let Ok(Some(out)) = backfilled_text(&text) else {
					return;
				};
				let before = shcl::Document::parse(&text);
				let after = shcl::Document::parse(&out);
				assert_eq!(
					after.lost_count(),
					before.lost_count(),
					"a line was lost, or a lost one found a place\nbefore:\n{text}\nafter:\n{out}"
				);
				for path in before.paths() {
					if before.get_string(&path).is_ok() {
						assert_eq!(
							after.get_string(&path),
							before.get_string(&path),
							"{path}\nbefore:\n{text}\nafter:\n{out}"
						);
					}
				}
			});
		}

		// Whatever backfill adds is where the next launch looks for it, so a
		// second pass finds nothing missing.
		// Test ID: EquUFK5
		#[test]
		fn backfill_settles_in_one_pass() {
			use super::super::backfilled_text;
			fuzz::soak("config-backfill-settles", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let text = String::from_utf8_lossy(&config(&mut rng)).into_owned();
				let Ok(Some(out)) = backfilled_text(&text) else {
					return;
				};
				assert_eq!(
					backfilled_text(&out),
					Ok(None),
					"\nfile:\n{text}\nafter one pass:\n{out}"
				);
			});
		}

		// Whatever an old file holds, what it converts to is current: the next
		// launch converts nothing, and a file written new has no line shcl cannot
		// read. Bytes that are not UTF-8 and a raw block that never closes are the
		// files shcl's own migration gives up on.
		// Test ID: ErgHEfO
		#[test]
		fn an_upgrade_settles_in_one_pass() {
			use super::super::{Rewrite, Upgrade, from_shcl2_text, unreadable_lines, upgrade};
			fuzz::soak("config-upgrade", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let mut case = config(&mut rng);
				if rng.chance(3) && !case.is_empty() {
					let at = rng.below(case.len());
					case.insert(at, 0xe9);
				}
				if rng.chance(3) {
					case.extend_from_slice(b"notes: ```\nnever closed\n");
				}
				let shown = String::from_utf8_lossy(&case).into_owned();
				match upgrade(&case) {
					Upgrade::Current => {}
					Upgrade::InPlace { text, .. } => {
						assert_eq!(
							from_shcl2_text(&text),
							None,
							"\nfile:\n{shown}\nconverted:\n{text}"
						);
					}
					Upgrade::Rewritten { text, .. }
					| Upgrade::Dropped {
						text,
						rewrite: Rewrite::Template,
						..
					} => {
						assert_eq!(
							from_shcl2_text(&text),
							None,
							"\nfile:\n{shown}\nwritten:\n{text}"
						);
						let unread = unreadable_lines(&shcl::Document::parse(&text));
						assert!(
							unread.is_empty(),
							"lines {unread:?}\nfile:\n{shown}\nwritten:\n{text}"
						);
					}
					// the file's own lines, so any it could not read before stay
					Upgrade::Dropped {
						text,
						rewrite: Rewrite::Kept,
						..
					} => {
						assert_eq!(
							from_shcl2_text(&text),
							None,
							"\nfile:\n{shown}\nwritten:\n{text}"
						);
					}
				}
			});
		}

		// A current file with bytes that are not UTF-8 is written again in one pass:
		// what is written is UTF-8 and current, so the next launch does nothing.
		// Kept in its own layout it only loses lines, bar the footer going back as
		// shipped, and written from the template it has no line shcl cannot read.
		// Test ID: ErgZKGo
		#[test]
		fn a_rewrite_for_lines_that_are_not_utf8_settles() {
			use super::super::{
				Rewrite, SHCL_BANNER, Upgrade, format_of, garbled_indexes, unreadable_lines,
				upgrade,
			};
			fuzz::soak("config-dropped", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let mut case = config(&mut rng);
				for _ in 0..=rng.below(3) {
					let at = rng.below(case.len() + 1);
					case.insert(at, *rng.pick(&[0xe9, 0xff, 0xc3]));
				}
				case.extend_from_slice(b"\n");
				let footer = if rng.chance(2) {
					SHCL_BANNER.replacen('\u{a9}', "\u{fffd}", 1)
				} else {
					SHCL_BANNER.to_string()
				};
				// the copyright sign as an editor saving in Latin-1 writes it
				case.extend(
					footer.replace('\u{fffd}', "\u{1}").bytes().map(
						|b| {
							if b == 1 { 0xa9 } else { b }
						},
					),
				);
				let shown = String::from_utf8_lossy(&case).into_owned();
				let garbled = garbled_indexes(&case);
				match upgrade(&case) {
					Upgrade::Dropped { text, rewrite, .. } => {
						assert_eq!(
							upgrade(text.as_bytes()),
							Upgrade::Current,
							"\nfile:\n{shown}\nwritten:\n{text}"
						);
						match rewrite {
							Rewrite::Kept => {
								let mut left = shown
									.split('\n')
									.enumerate()
									.filter(|(index, _)| !garbled.contains(index))
									.map(|(_, line)| line);
								for line in text.split('\n') {
									let banner = SHCL_BANNER.lines().any(|b| b == line.trim_end());
									assert!(
										banner || left.any(|l| l == line),
										"{line:?} is new\nfile:\n{shown}\nwritten:\n{text}"
									);
								}
							}
							Rewrite::Template => {
								let unread = unreadable_lines(&shcl::Document::parse(&text));
								assert!(
									unread.is_empty(),
									"lines {unread:?}\nfile:\n{shown}\nwritten:\n{text}"
								);
							}
						}
					}
					// an inserted byte that lands where it completes a character
					// decodes after all
					Upgrade::Current => assert!(garbled.is_empty(), "{shown}"),
					// a raw block that never closes swallows the footer, so the file
					// is not a current one; `an_upgrade_settles_in_one_pass` has those
					Upgrade::InPlace { .. } | Upgrade::Rewritten { .. } => {
						assert!(format_of(&shown) < shcl::FORMAT_MAJOR, "{shown}");
					}
				}
			});
		}

		// The launch-time repair of a wallpaper heading, over the shapes around it:
		// it settles in one pass, touches only the heading and the line it adds, and
		// every other setting loads as before.
		// Test ID: EpZefUo
		#[test]
		fn a_wallpaper_repair_changes_nothing_else() {
			fuzz::soak("config-repair", wallpaper_repair_case);
		}

		// Seed 107671 writes `wallpaper.rotate` three times over two blocks, and the
		// repair folds that to two. A plain test run's soak stops short of it.
		// Test ID: ErCFlaa
		#[test]
		fn a_key_written_three_times_survives_the_wallpaper_repair() {
			wallpaper_repair_case(107_671);
		}

		fn wallpaper_repair_case(seed: u64) {
			use super::super::{valued_wallpaper_line, wallpaper_heading_repaired};
			const IMAGES: [&str; 5] = [
				"/p/a.png",
				"\"C:\\\\Users\\\\x\\\\a b.png\"",
				"/p/a.png  # mine",
				"''",
				"'/p/#1.png'",
			];
			let mut rng = fuzz::Rng::new(seed);
			let mut text = default_config().to_string();
			// valued `wallpaper:` lines above a block the template has
			let mut valued = 0;
			if !rng.chance(5) {
				let image = rng.pick(&IMAGES);
				text = text.replacen("\nwallpaper:\n", &format!("\nwallpaper: {image}\n"), 1);
				valued += 1;
			}
			// shapes the repair must work through
			if rng.chance(3) {
				text.push_str("wallpaper.image: /p/b.png\n");
			}
			if rng.chance(3) {
				text.push_str("font_size: 13\n");
			}
			if rng.chance(3) {
				text.push_str("notes: ```\nwallpaper: /p/c.png\n\trotate:\n```\n");
			}
			if rng.chance(4) {
				text = shcl::Document::parse(&text).to_canonical();
			}
			// shapes that rule it out
			let mut ruled_out = false;
			if rng.chance(6) {
				let flat = if rng.chance(2) {
					"wallpaper: /p/d.png\n\twallpaper_opacity: 0.4\n"
				} else {
					"wallpaper: /p/d.png\nopacity: 0.4\n"
				};
				text.insert_str(0, flat);
				ruled_out = true;
			}
			if rng.chance(6) {
				text.push_str("wallpaper: /p/e.png\n\topacity: 0.4\n");
				valued += 1;
			}
			// any shape at all, so no expectation either way
			let mut unknown = false;
			if rng.chance(4) {
				text.push_str(&String::from_utf8_lossy(&config(&mut rng)));
				unknown = true;
			}
			if seed % 4 == 3 {
				text = text.replace('\n', "\r\n");
			}
			let Some(out) = wallpaper_heading_repaired(&text) else {
				assert!(
					valued != 1 || ruled_out || unknown,
					"a damaged heading was not repaired\nfile:\n{text}"
				);
				return;
			};
			assert!(
				unknown || (valued == 1 && !ruled_out),
				"repaired a file with {valued} valued headings:\n{text}"
			);
			assert_eq!(
				wallpaper_heading_repaired(&out),
				None,
				"not settled\nfile:\n{text}\nrepaired:\n{out}"
			);

			// line endings may change (a repair writes LF), line contents may not
			let old: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
			let new: Vec<&str> = out.lines().map(|l| l.trim_end_matches('\r')).collect();
			let at = old
				.iter()
				.zip(&new)
				.position(|(a, b)| a != b)
				.expect("the heading line changes");
			assert!(
				valued_wallpaper_line(old[at]) && new[at] == "wallpaper:",
				"line {at} changed\nfile:\n{text}\nrepaired:\n{out}"
			);
			let added = new.len().checked_sub(old.len()).expect("no line removed");
			assert!(added <= 1, "{added} lines added\nfile:\n{text}");
			if added == 1 {
				assert!(
					new[at + 1].trim_start().starts_with("image: "),
					"added {:?}",
					new[at + 1]
				);
			}
			assert_eq!(
				old[at + 1..],
				new[at + 1 + added..],
				"another line changed\nfile:\n{text}\nrepaired:\n{out}"
			);

			let before = shcl::Document::parse(&text);
			let after = shcl::Document::parse(&out);
			let mut paths = before.paths();
			paths.extend(after.paths());
			// A heading in two `wallpaper:` blocks reads as written twice until the
			// repair empties the first block and shcl folds the two. An empty value
			// loads as the default either way, so only a value is compared.
			let folded = |p: &str| {
				matches!(after.get_string(p), Err(shcl::Status::Empty))
					&& matches!(
						before.get_string(p),
						Err(shcl::Status::Empty | shcl::Status::Multiple)
					)
			};
			// A key written more than once loads as a duplicate however many times it
			// is there, and the same fold can take one of those away.
			let loads = |doc: &shcl::Document, p: &str| match doc.get_string(p) {
				Err(shcl::Status::Multiple) => format!("{:?}", doc.get_string(p)),
				got => format!("{got:?} {}", doc.count(p)),
			};
			for path in paths
				.iter()
				.filter(|p| *p != "wallpaper" && *p != "wallpaper.image" && !folded(p))
			{
				assert_eq!(
					loads(&before, path),
					loads(&after, path),
					"{path} loads differently\nfile:\n{text}\nrepaired:\n{out}"
				);
			}
			// the file may hold a second `wallpaper` block, so read the line alone
			let image = if before.count("wallpaper.image") > 0 {
				before.get_string("wallpaper.image")
			} else {
				shcl::Document::parse(old[at]).get_string("wallpaper")
			};
			assert_eq!(
				after.get_string("wallpaper.image"),
				image,
				"the image\nfile:\n{text}\nrepaired:\n{out}"
			);
		}

		// Children for a performance block the way hand edits leave them: mixed
		// depths, the rating's own keys commented, bare, dotted or with children
		// under them, and values with a comment or a list in them.
		fn rating_children(rng: &mut fuzz::Rng) -> String {
			use std::fmt::Write;
			const INDENTS: [&str; 5] = ["\t", "\t", "    ", "\t\t", "  "];
			#[rustfmt::skip]
			const LEAVES: [&str; 6] = [
				"automatic", "profile", "rated_hardware", "check_next_run", "check_hardware", "other",
			];
			#[rustfmt::skip]
			const VALUES: [&str; 9] = [
				"true", "false", "high", "\"max\"", "0000000000000000", "\"\"", "[1, 2]",
				"x  ## note", "\"a # b\"",
			];
			let mut out = String::new();
			for _ in 0..=rng.below(6) {
				let indent = rng.pick(&INDENTS);
				let leaf = rng.pick(&LEAVES);
				let _ = match rng.below(8) {
					0 | 1 => writeln!(out, "{indent}# {leaf}: \"\"  ## Default"),
					2 => writeln!(out, "{indent}{leaf}:"),
					3 => writeln!(out, "{indent}{leaf}:\n{indent}\t- a"),
					4 => writeln!(out),
					5 => writeln!(out, "performance.{leaf}: {}", rng.pick(&VALUES)),
					_ => writeln!(out, "{indent}{leaf}: {}", rng.pick(&VALUES)),
				};
			}
			out
		}

		// Somewhere a rating has to go: the template with lines typed into its own
		// performance block, or a hand-written file with one or two blocks spliced in.
		fn rating_case(rng: &mut fuzz::Rng) -> Vec<u8> {
			#[rustfmt::skip]
			const HEADERS: [&str; 4] = [
				"performance:", "performance:  ## note", "  performance:", "performance: 5",
			];
			let mut lines: Vec<String> = if rng.chance(3) {
				default_config().lines().map(str::to_string).collect()
			} else {
				String::from_utf8_lossy(&config(rng))
					.lines()
					.map(str::to_string)
					.collect()
			};
			let pieces = if rng.chance(3) { 0 } else { 1 + rng.below(2) };
			for _ in 0..pieces {
				let header = lines.iter().position(|line| line == "performance:");
				let (at, piece) = match header {
					Some(h) if rng.chance(2) => {
						let at = h + 1 + rng.below((lines.len() - h).min(40));
						(at, rating_children(rng))
					}
					_ => {
						let at = rng.below(lines.len() + 1);
						let head = rng.pick(&HEADERS).to_string();
						(at, format!("{head}\n{}", rating_children(rng)))
					}
				};
				for (n, line) in piece.lines().enumerate() {
					lines.insert(at + n, line.to_string());
				}
			}
			if !rng.chance(2) {
				let at = rng.below(lines.len() + 1);
				for (n, line) in migrated_shape(rng).lines().enumerate() {
					lines.insert(at + n, line.to_string());
				}
			}
			let mut out = lines.join("\n");
			out.push('\n');
			out.into_bytes()
		}

		// Lines the launch's migration reads by how they are written as well as by
		// value, taken from its own tables so a new rename or stack joins on its own.
		fn migrated_shape(rng: &mut fuzz::Rng) -> String {
			let old = if rng.chance(4) {
				*rng.pick(CONFIG_REMOVED)
			} else {
				rng.pick(CONFIG_RENAMES).0
			};
			let (block, leaf) = old.rsplit_once('.').unwrap_or(("", old));
			match rng.below(7) {
				0 => format!("font:\n\tfamily: '{}'\n", rng.pick(SUPERSEDED_FONT_STACKS)),
				1 => format!("{block}:\n\t# x:\n\t\t{leaf}: 12\n"),
				2 => format!("{block}:\n\t\t# x:\n\t{leaf}: 12\n"),
				3 => format!("{block}:\n    # x:\n\t{leaf}: 12\n"),
				// the steps before the renames: a file from before the nested
				// layout, and a default shell still to be moved into the list
				4 => format!("font_family: '{}'\n", rng.pick(SUPERSEDED_FONT_STACKS)),
				5 => format!(
					"use_system_font: false\nfont_family: '{}'\nwallpaper_folder: C:\\Users\\x\n",
					rng.pick(SUPERSEDED_FONT_STACKS)
				),
				_ => "shell:\n\tdefault: /bin/sh\nfont:\n\tfamily: 'Some Mono'\n".to_string(),
			}
		}

		// A rating writes its own lines and nothing else, so whatever a file holds,
		// an answer it accepts reads each value back, loses no more lines, and loads
		// every setting it did not write as before, as a launch reads the file after
		// its renames and refreshes. The last is read here path by
		// path over both parses, apart from the writer's own check, though both ask
		// shcl. Canonical text is no measure of what loads: it keeps a line shcl
		// skipped, drops one it lost, and moves a stray line with the setting after
		// it. A file that reads clean is never called unreadable, and wherever
		// shcl's own setters would keep the rating, the writer keeps it too.
		fn rating_check(case: &[u8], rng: &mut fuzz::Rng) {
			const PROFILES: [&str; 4] = ["max", "high", "low", "standard"];
			const IDS: [&str; 3] = ["0123456789abcdef", "1234567890123456", "ffffffffffffffff"];
			let text = String::from_utf8_lossy(case).into_owned();
			let lines = RatingLines {
				profile: (!rng.chance(4)).then(|| *rng.pick(&PROFILES)),
				rated_hardware: (!rng.chance(4)).then(|| *rng.pick(&IDS)),
				check_next_run: rng.chance(3).then(|| rng.chance(2)),
			};
			let written: Vec<&str> = [
				("performance.profile", lines.profile.is_some()),
				("performance.rated_hardware", lines.rated_hardware.is_some()),
				("performance.check_next_run", lines.check_next_run.is_some()),
			]
			.into_iter()
			.filter_map(|(key, on)| on.then_some(key))
			.collect();
			// Each setting the rating does not write, as it reads in two parses.
			let reads = |a: &shcl::Document, b: &shcl::Document| {
				let mut paths = a.paths();
				for path in b.paths() {
					if !paths.contains(&path) {
						paths.push(path);
					}
				}
				paths.retain(|path| {
					!written
						.iter()
						.any(|key| path == key || path.starts_with(&format!("{key}.")))
				});
				let read = |doc: &shcl::Document| {
					paths
						.iter()
						// a block the write may have created, with no value of its own
						.filter(|path| {
							let holds = written
								.iter()
								.any(|key| key.starts_with(&format!("{path}.")));
							!(holds
								&& matches!(
									doc.get_string(path),
									Err(shcl::Status::Empty | shcl::Status::NotFound)
								))
						})
						// the quoted flag is left out, since no read sees it; a quote or
						// indent the migration reads shows in the migrated parse
						.map(|path| (path.clone(), doc.get_string(path), doc.count(path)))
						.collect::<Vec<_>>()
				};
				(read(a), read(b))
			};
			// the parse the next launch makes, where its rewrites change the text
			let migrated = |t: &str| match next_launch_text(t) {
				std::borrow::Cow::Owned(t) => Some(shcl::Document::parse(&t)),
				std::borrow::Cow::Borrowed(_) => None,
			};
			let before = shcl::Document::parse(&text);
			let before_migrated = migrated(&text);
			let before_loads = before_migrated.as_ref().unwrap_or(&before);
			let answer = with_rating_lines(&text, &lines);

			if before.lost_count() == 0 {
				assert_ne!(
					answer,
					Err(super::super::Kept::Unreadable),
					"a file that reads clean is called unreadable\nbefore:\n{text}"
				);
				// what the dialog's save would leave: shcl's setters, then canonical text
				let mut saved = before.clone();
				let set = lines
					.profile
					.is_none_or(|word| saved.set_string("performance.profile", word))
					&& lines
						.rated_hardware
						.is_none_or(|word| saved.set_string("performance.rated_hardware", word))
					&& lines
						.check_next_run
						.is_none_or(|flag| saved.set_bool("performance.check_next_run", flag));
				let saved_text = saved.to_canonical();
				let saved = shcl::Document::parse(&saved_text);
				let saved_migrated = migrated(&saved_text);
				// The rating has to be there for the next launch to read. A file
				// from before the nested layout is written fresh by that launch,
				// which carries no rating over, so a write into one keeps nothing.
				let saved_loads = saved_migrated.as_ref().unwrap_or(&saved);
				let kept_in = |doc: &shcl::Document| {
					lines.profile.is_none_or(|word| {
						doc.get_string("performance.profile").as_deref() == Ok(word)
					}) && lines.rated_hardware.is_none_or(|word| {
						doc.get_string("performance.rated_hardware").as_deref() == Ok(word)
					}) && lines
						.check_next_run
						.is_none_or(|flag| doc.get_bool("performance.check_next_run") == Ok(flag))
				};
				let kept_by_save =
					set && saved.lost_count() == 0 && kept_in(&saved) && kept_in(saved_loads) && {
						let (then, now) =
							reads(before_loads, saved_migrated.as_ref().unwrap_or(&saved));
						then == now
					};
				if kept_by_save {
					assert!(
						answer.is_ok(),
						"the save keeps this rating and the writer answers {answer:?}\nbefore:\n{text}"
					);
				}
			}

			let Ok(out) = answer else {
				return;
			};
			let after = shcl::Document::parse(&out);
			let after_migrated = migrated(&out);
			let shown = format!("before:\n{text}\nafter:\n{out}");
			assert!(
				after.lost_count() <= before.lost_count(),
				"a line was lost\n{shown}"
			);
			for (key, word) in [
				("performance.profile", lines.profile),
				("performance.rated_hardware", lines.rated_hardware),
			] {
				if let Some(word) = word {
					assert_eq!(after.get_string(key).as_deref(), Ok(word), "{key}\n{shown}");
				}
			}
			if let Some(flag) = lines.check_next_run {
				assert_eq!(
					after.get_bool("performance.check_next_run"),
					Ok(flag),
					"check_next_run\n{shown}"
				);
			}
			let (then, now) = reads(before_loads, after_migrated.as_ref().unwrap_or(&after));
			assert_eq!(now, then, "another setting loads differently\n{shown}");
		}

		// Test ID: EpXeZQX
		#[test]
		fn a_rating_changes_nothing_else_in_any_file() {
			let corpus = fuzz::corpus("config");
			for case in &corpus {
				rating_check(case, &mut fuzz::Rng::new(0));
			}
			fuzz::soak("config-rating", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let case = fuzz::input(&mut rng, &corpus, rating_case);
				rating_check(&case, &mut rng);
			});
		}
	}
}
