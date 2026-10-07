// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::collections::HashMap;
use std::sync::RwLock;

use glyphon::cosmic_text::fontdb;
use glyphon::{
	Attrs, Buffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
	SwashContent, TextArea, TextAtlas, TextRenderer, Viewport, Wrap,
};

use crate::coloremoji::{ColorGlyphs, ColorMetrics};
use crate::config;

// Concrete family name behind `Family::Monospace`, re-resolved on each TextCtx
// build (so the Settings font field / "Use system font" apply live). cosmic-text
// picks the best face *per query*, so a BOLD run can end up in a different family
// than the regular run; pinning one name keeps every weight in it. `Attrs` needs
// a 'static name, so the resolved string is leaked (rare - only on a font change).
static MONO_FAMILY: RwLock<Option<&'static str>> = RwLock::new(None);
// Nearest-to-Bold weight the pinned mono family actually ships. Terminal bold
// runs request THIS, not a literal 700: a mono family with no bold face would
// otherwise eject the whole run into a proportional bold fallback, whose
// advances set_monospace_width can't snap - skewing space-based alignment (the
// muffer startup screen, box-drawing, etc.). Mirrors chrome's UI_WEIGHT_BOLD.
static MONO_WEIGHT_BOLD: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(700);

fn mono_family() -> Option<&'static str> {
	*crate::locks::read(&MONO_FAMILY)
}

// Re-resolve and pin the monospace family for the current config + font system.
fn pin_mono_family(fs: &FontSystem) {
	use std::sync::atomic::Ordering;
	let name = resolve_mono_family(fs).map(|family| &*Box::leak(family.into_boxed_str()));
	*crate::locks::write(&MONO_FAMILY) = name;
	// Snap "bold" to the family's boldest available face so it can never eject the
	// family. No pinned name (generic Monospace) -> keep 700, nothing to snap to.
	let bold = match name {
		Some(family) => nearest_face(fs.db(), family, 700, false).map_or(700, |(w, _)| w),
		None => 700,
	};
	MONO_WEIGHT_BOLD.store(bold, Ordering::Relaxed);
}

/// What the text pass needs to blend glyph coverage the way an sRGB blend would,
/// which is the weight the font was drawn for: the pair as sRGB grays of the same
/// brightness, and how much of the correction to apply.
///
/// Coverage is blended in linear light, so a half covered pixel comes out near
/// three quarters brightness whichever way round the two colors are. On a dark
/// background that reads as a strong edge; on a light one it is almost no ink at
/// all, and the thin parts of every letter go with it. Only text darker than what
/// is behind it needs the correction - the other way round is already heavy
/// enough - and that side is decided on Oklab lightness, the measure minimum
/// contrast uses.
///
/// One alpha serves all three channels, so the curve is built from grays of the
/// pair's own brightness. A glyph in some other color takes the same curve, which
/// is off by up to about 20 levels on its partly covered pixels and always in the
/// direction of more ink.
///
/// The shader also lifts the partly covered pixels first, by
/// `config::DARK_ON_LIGHT_CONTRAST`, since light on dark still looks heavier than
/// dark on light at the same blend. A glyph lighter than the gray halfway between
/// the pair, such as a menu label on dark chrome, gets neither.
pub fn text_blend(fg: [u8; 3], bg: [u8; 3], amount: f32) -> (f32, f32, f32) {
	let (fg_gray, bg_gray) = (gray_of(fg), gray_of(bg));
	let on = crate::palette::to_oklab(fg).0 < crate::palette::to_oklab(bg).0;
	(
		fg_gray,
		bg_gray,
		if on {
			amount.clamp(0.0, crate::config::MAX_DARK_ON_LIGHT)
		} else {
			0.0
		},
	)
}

// A color as the sRGB gray of the same brightness. Rec.709 luma in linear light,
// encoded back, so the curve built from it runs over the range the real pair
// does.
fn gray_of(c: [u8; 3]) -> f32 {
	crate::config::from_linear(crate::config::luma(c))
}

/// Weight a terminal bold cell should request: the closest weight to Bold the
/// pinned mono family really ships. Use instead of a literal `Weight::BOLD`, which
/// kicks the family out (into a proportional fallback) when it has no bold face.
pub fn mono_bold_weight() -> glyphon::Weight {
	use std::sync::atomic::Ordering;
	glyphon::Weight(MONO_WEIGHT_BOLD.load(Ordering::Relaxed))
}

pub fn mono_attrs() -> Attrs<'static> {
	let mut attrs = Attrs::new();
	attrs.family = match mono_family() {
		Some(name) => Family::Name(name),
		None => Family::Monospace,
	};
	attrs
}

// Concrete family + style behind chrome's `ui_attrs`, pinned alongside the mono
// family. Chrome follows the DESKTOP interface font - family, weight and slant -
// serif or not (for example "GentiumAlt Bold"); a sans is only the fallback
// when no desktop setting is readable.
//
// The weights are pinned as exact face weights, not the desktop's nominal ones:
// cosmic-text only uses the requested family when a face matches the requested
// weight EXACTLY (its fallback filters font_weight_diff == 0), so asking for
// Bold in a family that ships no bold face silently swaps in a bold fallback
// sans - the family must win over the weight. It also compares family names
// case-SENSITIVELY, so the db's own spelling is what gets pinned.
static UI_FAMILY: RwLock<Option<&'static str>> = RwLock::new(None);
static UI_WEIGHT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(400);
static UI_WEIGHT_BOLD: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(700);
static UI_ITALIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn ui_family() -> Option<&'static str> {
	*crate::locks::read(&UI_FAMILY)
}

// Nearest face the family actually has to (weight, slant); None when the family
// has no faces at all (shouldn't happen for a db-validated name).
fn nearest_face(
	db: &fontdb::Database,
	fam: &str,
	want_weight: u16,
	want_italic: bool,
) -> Option<(u16, bool)> {
	let mut best: Option<(u16, bool)> = None;
	for face in db.faces() {
		if !face.families.iter().any(|(name, _)| name == fam) {
			continue;
		}
		let is_italic = face.style == fontdb::Style::Italic;
		let candidate = (
			is_italic != want_italic,
			face.weight.0.abs_diff(want_weight),
			face.weight.0,
		);
		let beats = best.is_none_or(|(best_weight, best_italic)| {
			candidate
				< (
					best_italic != want_italic,
					best_weight.abs_diff(want_weight),
					best_weight,
				)
		});
		if beats {
			best = Some((face.weight.0, is_italic));
		}
	}
	best
}

fn pin_ui_family(fs: &FontSystem) {
	use std::sync::atomic::Ordering;
	let sys_font = crate::sysfont::interface();
	let name = resolve_ui_family(fs).map(|family| &*Box::leak(family.into_boxed_str()));
	*crate::locks::write(&UI_FAMILY) = name;
	// honor the desktop's weight/slant only when its family actually resolved
	// (a fallback sans shouldn't inherit "Bold" meant for another face)
	let using_sys = match (name, &sys_font.family) {
		(Some(resolved), Some(family)) => resolved.eq_ignore_ascii_case(family),
		_ => false,
	};
	let want_weight: u16 = if using_sys && sys_font.bold { 700 } else { 400 };
	let want_italic = using_sys && sys_font.italic;
	let (body_weight, body_italic, title_weight) = match name {
		Some(family_name) => {
			let db = fs.db();
			let (weight, italic) =
				nearest_face(db, family_name, want_weight, want_italic).unwrap_or((400, false));
			// emphasis (dialog titles/headers): the family's boldest-available
			// take on 700, again snapped so it can't eject the family
			let title_weight = nearest_face(db, family_name, 700, italic)
				.map_or(weight, |(nearest_weight, _)| nearest_weight);
			(weight, italic && want_italic, title_weight)
		}
		None => (400, false, 700),
	};
	UI_WEIGHT.store(body_weight, Ordering::Relaxed);
	UI_WEIGHT_BOLD.store(title_weight, Ordering::Relaxed);
	UI_ITALIC.store(body_italic, Ordering::Relaxed);
}

// Chrome ascent/descent scaled to `ui_px`, read the SAME way cosmic-text does
// (`ui_px * ascent/units_per_em`), so `ui_text_top` predicts the real baseline.
// A proportional fallback if the pinned face can't be read.
fn ui_vmetrics(fs: &mut FontSystem, ui_px: f32) -> (f32, f32) {
	use std::sync::atomic::Ordering;
	let want_weight = fontdb::Weight(UI_WEIGHT.load(Ordering::Relaxed));
	let id = {
		let query = fontdb::Query {
			families: &[ui_family().map_or(fontdb::Family::SansSerif, fontdb::Family::Name)],
			weight: want_weight,
			..Default::default()
		};
		fs.db().query(&query)
	};
	let fallback = (ui_px * 0.8, ui_px * 0.2);
	let Some(font) = id.and_then(|id| fs.get_font(id, want_weight)) else {
		return fallback;
	};
	let metrics = font.as_swash().metrics(&[]);
	let scale = ui_px / f32::from(metrics.units_per_em).max(1.0);
	(metrics.ascent * scale, metrics.descent * scale)
}

// Baseline y within a single-line UI buffer of height `ui_line_h`: cosmic-text
// centers the ascent+descent box in the line, so the baseline sits at the line
// center shifted by (ascent-descent)/2. `vmetrics` is (ascent, descent).
fn ui_baseline_in_buf(ui_line_h: f32, vmetrics: (f32, f32)) -> f32 {
	let (ascent, descent) = vmetrics;
	ui_line_h / 2.0 + (ascent - descent) / 2.0
}

// Buffer `top` that centers chrome text's visible box in a bar
// `[bar_top, bar_top+bar_h]`. Chrome titles (File/Edit/tab names) have no
// descenders but do have ascenders (l/d/h/i), so their visible extent is
// ascender-top..baseline; centering THAT (not cap..baseline) is what actually
// looks balanced - cap-centering leaves the empty descent reading as space
// below. The rare descender then just dips into the natural descent room.
fn ui_visible_center_top(ui_line_h: f32, vmetrics: (f32, f32), bar_top: f32, bar_h: f32) -> f32 {
	let (ascent, _) = vmetrics;
	bar_top + bar_h / 2.0 - ui_baseline_in_buf(ui_line_h, vmetrics) + ascent / 2.0
}

// Buffer `top` that centers chrome text's FULL ink box (ascender-top to
// descender-bottom) in a bar. For a tab title - a path, so mostly lowercase and
// slashes - the descenders carry as much weight as the caps, and the
// ascender..baseline rule above leaves them hanging on the bottom edge.
fn ui_ink_center_top(ui_line_h: f32, vmetrics: (f32, f32), bar_top: f32, bar_h: f32) -> f32 {
	let (ascent, descent) = vmetrics;
	bar_top + bar_h / 2.0 - ui_baseline_in_buf(ui_line_h, vmetrics) + (ascent - descent) / 2.0
}

/// Emphasis weight for chrome (dialog titles, section headers): the closest
/// weight to Bold the pinned family really ships. Use this instead of a literal
/// `Weight::BOLD`, which kicks the whole family out when no 700 face exists.
pub fn ui_bold_weight() -> glyphon::Weight {
	use std::sync::atomic::Ordering;
	glyphon::Weight(UI_WEIGHT_BOLD.load(Ordering::Relaxed))
}

/// Proportional attrs for chrome - menus, the menu bar, dialogs - in the pinned
/// desktop interface font (family/weight/slant), so chrome reads like the rest
/// of the user's desktop rather than terminal text.
pub fn ui_attrs() -> Attrs<'static> {
	use std::sync::atomic::Ordering;
	let mut attrs = Attrs::new();
	attrs.family = match ui_family() {
		Some(name) => Family::Name(name),
		None => Family::SansSerif,
	};
	attrs.weight = glyphon::Weight(UI_WEIGHT.load(Ordering::Relaxed));
	if UI_ITALIC.load(Ordering::Relaxed) {
		attrs.style = glyphon::Style::Italic;
	}
	attrs
}

// Resolve a concrete family for chrome: the desktop interface font first (the
// whole point - serif or not), else the OS sans-serif, else a known-good sans.
// Everything is validated against the db so a bad name can't slip through -
// generic `Family::SansSerif` can't be trusted (fontdb defaults it to "Arial"
// and falls through to whatever matches when that's absent).
fn resolve_ui_family(fs: &FontSystem) -> Option<String> {
	let db = fs.db();
	// returns the db's canonical spelling of the family (cosmic-text's fallback
	// compares face family names case-sensitively - the candidate string won't do)
	let installed = |fam: &str| {
		let query = fontdb::Query {
			families: &[fontdb::Family::Name(fam)],
			..Default::default()
		};
		db.query(&query)
			.and_then(|id| db.face(id))
			.and_then(|face| {
				face.families
					.iter()
					.find(|(name, _)| name.eq_ignore_ascii_case(fam))
					.map(|(name, _)| name.clone())
			})
	};
	let curated = [
		"DejaVu Sans",
		"Noto Sans",
		"Liberation Sans",
		"Cantarell",
		"Ubuntu",
		"Segoe UI",
		"Helvetica Neue",
		"Arial",
	];
	crate::sysfont::interface()
		.family
		.clone()
		.into_iter()
		.chain(crate::sysfont::sans_serif().map(str::to_string))
		.chain(curated.iter().map(std::string::ToString::to_string))
		.find_map(|fam| installed(&fam))
}

// The family search order, in full, whether or not any of it is installed. Same
// on every platform: the OS monospace leads while `use_system_font` is on and
// trails the configured stack otherwise, and the built-in stack is always last.
// A platform only shows through in what it reports - Windows has no monospace
// setting, so it passes None and resolution simply starts at `font_family`
// there, with no special case for it. Kept pure so the order can be tested
// without a font db.
fn mono_candidates(
	os_family: Option<&str>,
	configured: Option<&str>,
	follow_os: bool,
) -> Vec<String> {
	let split = |list: Option<&str>| {
		list.into_iter()
			.flat_map(|l| l.split(','))
			.map(|name| name.trim().to_string())
			.filter(|name| !name.is_empty())
			.collect::<Vec<_>>()
	};
	let os = os_family.map(str::to_string);
	let mut candidates = Vec::new();
	if follow_os {
		candidates.extend(os.clone());
	}
	candidates.extend(split(configured));
	if !follow_os {
		candidates.extend(os);
	}
	// Built-in stack as the last resort everywhere, ahead of the bare
	// Family::Monospace query: that query is a db lottery whose winner may lack a
	// bold face, and cosmic-text only keeps a family when a face matches the
	// requested weight exactly - so bold runs would silently eject to an
	// arbitrary (often proportional) fallback. Known-good families avoid that.
	candidates.extend(split(Some(config::DEFAULT_FONT_STACK)));
	candidates
}

// Resolve the monospace family to pin for every weight: the first family from
// `mono_candidates` that is actually installed, else whatever `Family::Monospace`
// maps to. Validated against the db so a bad name doesn't silently fall back to
// an unrelated font.
fn resolve_mono_family(fs: &FontSystem) -> Option<String> {
	use glyphon::cosmic_text::fontdb;
	let db = fs.db();

	let installed = |fam: &str| {
		let query = fontdb::Query {
			families: &[fontdb::Family::Name(fam)],
			..Default::default()
		};
		db.query(&query)
			.and_then(|id| db.face(id))
			.is_some_and(|face| {
				face.families
					.iter()
					.any(|(name, _)| name.eq_ignore_ascii_case(fam))
			})
	};

	let settings = config::settings();
	for fam in mono_candidates(
		crate::sysfont::monospace().family.as_deref(),
		settings.font_family.as_deref(),
		config::system_font_face_active(&settings),
	) {
		if installed(&fam) {
			return Some(fam);
		}
	}

	let query = fontdb::Query {
		families: &[fontdb::Family::Monospace],
		..Default::default()
	};
	db.query(&query)
		.and_then(|id| db.face(id))?
		.families
		.first()
		.map(|(name, _)| name.clone())
}

pub struct TextCtx {
	pub font_system: FontSystem,
	pub swash_cache: SwashCache,
	// The half that lives on the device. Absent while the window has let its
	// GPU go (see app/idle.rs, `release_gpu`); the metrics and the font system stay,
	// since layout and input keep asking for them.
	gpu: Option<TextGpu>,
	/// The display's scale factor this context was built at. Every chrome
	/// measurement in the main window is written in DIP and converted through
	/// `dip` at its use site, since chrome shares a coordinate space with the
	/// terminal grid and has no boundary to convert at.
	pub scale: f32,
	pub cell_w: f32,
	pub cell_h: f32,
	/// Whether a bold run shapes to the same per-cell advance as regular. When
	/// false (font ignores `set_monospace_width` and its bold face has a different
	/// advance - common on Windows), the de-bold scrim buffer would drift from the
	/// display buffer along the line, so panes reuse the display buffer for the
	/// scrim instead (see `Pane::build`, `text_scrim_regular_weight`).
	pub debold_safe: bool,
	/// physical-px inset between content and pane edge
	pub margin: f32,
	pub metrics: Metrics,
	/// Chrome (menus/tabs/dialogs) renders at the DESKTOP interface font size,
	/// independent of the terminal font size; bars and rows size from this.
	pub ui_line_h: f32,
	ui_metrics: Metrics,
	// chrome vertical metrics at ui_px (ascent, descent) in the units cosmic-text
	// lays the line out in, so a bar can center chrome text on its real visible
	// box (see `ui_text_top`).
	ui_vmetrics: (f32, f32),
	// primary monospace face + a coverage cache, so the pane can tell which
	// glyphs fall back to another font (those drift from the cell grid and get
	// rendered per-cell instead - see Pane::build). The cached value is how many
	// grid cells the face's own advance for that char spans, 0 for "no glyph".
	mono_face: Option<fontdb::ID>,
	cover_cache: HashMap<char, u8>,
	// COLRv1 color glyphs, which swash can't rasterize (see coloremoji.rs). Panes
	// route an emoji cell here instead of to a monochrome fallback face.
	color_glyphs: ColorGlyphs,
	// Per-char monochrome fallback family, misses cached too (see text_family).
	text_families: HashMap<char, Option<&'static str>>,
	// Measured chrome-text widths. Keyed by text only: every chrome measurement
	// uses the base UI attrs (color varies, which doesn't affect width), and the
	// font is fixed for this TextCtx's life. Measuring shapes a throwaway buffer,
	// and the menu bar re-measures its titles every rendered frame - the memo
	// turns that into a lookup. Bounded (cleared) so dynamic tab titles can't
	// grow it without limit.
	ui_measure_cache: HashMap<String, f32>,
	/// Which context this is. A font, size or scale change builds a new one, so
	/// anything measured with the old one can tell it is stale by this alone.
	pub generation: u64,
}

impl std::fmt::Debug for TextCtx {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TextCtx")
			.field("scale", &self.scale)
			.field("cell_w", &self.cell_w)
			.field("cell_h", &self.cell_h)
			.finish_non_exhaustive()
	}
}

// Everything of a TextCtx that is made on a wgpu device.
struct TextGpu {
	atlas: TextAtlas,
	// The scrim renders into its own coverage texture, a different format
	// than the surface, so its glyphon renderer needs its own same-format atlas.
	scrim_atlas: TextAtlas,
	viewport: Viewport,
	// last resolution given to the viewport (skip the per-frame re-write)
	viewport_size: (u32, u32),
	renderer: TextRenderer,
	// separate renderer for the context-menu overlay (second pass, on top)
	overlay: TextRenderer,
	// separate renderer for the scrim source pass: pane text only (no chrome), and
	// panes may substitute a de-bolded buffer (text_scrim_regular_weight)
	scrim: TextRenderer,
}

const DETACHED: &str = "text drawn while its GPU half is released";

#[allow(
	clippy::expect_used,
	reason = "the window draws only between attach_gpu and detach_gpu (G114)"
)]
fn attached(gpu: &mut Option<TextGpu>) -> &mut TextGpu {
	gpu.as_mut().expect(DETACHED)
}

#[allow(
	clippy::expect_used,
	reason = "the window draws only between attach_gpu and detach_gpu (G114)"
)]
fn attached_ref(gpu: Option<&TextGpu>) -> &TextGpu {
	gpu.expect(DETACHED)
}

impl TextGpu {
	fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
		let cache = Cache::new(device);
		let mut atlas = TextAtlas::new(device, queue, &cache, format);
		// The scrim text pass renders into its own coverage texture
		// (crate::scrim::TEXT_FMT), so its glyphon renderer must target THAT format.
		// A shared atlas targets the window's format, and wgpu rejects it as
		// "incompatible color attachments" on the first scrim frame. One Cache backs
		// both atlases (it's built to serve multiple target formats).
		let mut scrim_atlas = TextAtlas::new(device, queue, &cache, crate::scrim::TEXT_FMT);
		let mut viewport = Viewport::new(device, &cache);
		// Acts only where `set_text_blend` turns the correction on.
		viewport.set_text_contrast(queue, crate::config::DARK_ON_LIGHT_CONTRAST);
		let renderer =
			TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
		let overlay =
			TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
		let scrim = TextRenderer::new(
			&mut scrim_atlas,
			device,
			wgpu::MultisampleState::default(),
			None,
		);
		Self {
			atlas,
			scrim_atlas,
			viewport,
			viewport_size: (0, 0),
			renderer,
			overlay,
			scrim,
		}
	}
}

// Round a glyph's advance to whole cells, `unit` being the face's own advance
// for an ASCII cell. A face that reports nothing usable answers 1, so an
// unmeasurable font behaves the way it always did.
fn advance_cells(advance: f32, unit: f32) -> u8 {
	if unit <= 0.0 {
		return 1;
	}
	(advance / unit).round().clamp(0.0, 8.0) as u8
}

impl TextCtx {
	/// `SILK_MEMDBG`: rasterized glyphs kept on the heap, and the font faces known.
	pub fn memdbg_line(&self) -> String {
		let images = &self.swash_cache.image_cache;
		let bytes: usize = images
			.values()
			.flatten()
			.map(|image| image.data.len())
			.sum();
		format!(
			"glyphs: {} rasterized, {:.1} MiB; {} font faces",
			images.len(),
			crate::memdbg::mib(bytes),
			self.font_system.db().len()
		)
	}

	pub fn new(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		format: wgpu::TextureFormat,
		scale: f32,
	) -> Self {
		let mut ctx = Self::new_cpu(scale);
		ctx.attach_gpu(device, queue, format);
		ctx
	}

	/// The device half again, on a new device. The atlases start empty, so the
	/// next frame rasterizes what it shows.
	pub fn attach_gpu(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		format: wgpu::TextureFormat,
	) {
		self.gpu = Some(TextGpu::new(device, queue, format));
	}

	/// Let the device half go, and with it the rasterized glyphs, which are only
	/// worth keeping while there is an atlas to put them in. Nothing may draw
	/// until `attach_gpu`.
	pub fn detach_gpu(&mut self) {
		self.gpu = None;
		self.swash_cache = SwashCache::new();
	}

	fn gpu(&mut self) -> &mut TextGpu {
		attached(&mut self.gpu)
	}

	/// Fonts and metrics alone: everything the layout needs and nothing a device
	/// does.
	pub fn new_cpu(scale: f32) -> Self {
		let mut font_system = FontSystem::new();
		pin_mono_family(&font_system);
		pin_ui_family(&font_system);

		let font_size = (config::effective_font_size() * scale).round();
		let line_height = (font_size * config::settings().line_height_scale).round();
		let metrics = Metrics::new(font_size, line_height);

		let cell_w = measure_cell(&mut font_system, metrics);
		let cell_h = line_height.max(1.0);
		let debold_safe = bold_matches_cell(&mut font_system, metrics, cell_w);

		// Chrome follows the desktop UI font size, converted like the mono path;
		// terminal size is the fallback so the old chrome look is kept where no
		// desktop setting is readable.
		let ui_px = crate::sysfont::interface()
			.size_pt
			.map(crate::sysfont::px_from_pt)
			.filter(|px| *px >= 4.0)
			.unwrap_or_else(config::effective_font_size);
		let ui_px = (ui_px * scale).round().max(8.0);
		let ui_line_h = (ui_px * 1.35).round(); // roomy UI leading; descenders must clear buttons
		let ui_metrics = Metrics::new(ui_px, ui_line_h);
		let ui_vmetrics = ui_vmetrics(&mut font_system, ui_px);

		let mono_face = {
			let fam = mono_family();
			let query = fontdb::Query {
				families: &[fam.map_or(fontdb::Family::Monospace, fontdb::Family::Name)],
				..Default::default()
			};
			font_system.db().query(&query)
		};

		Self {
			font_system,
			swash_cache: SwashCache::new(),
			gpu: None,
			scale,
			cell_w,
			cell_h,
			debold_safe,
			margin: (config::settings().margin * scale).round(),
			metrics,
			ui_line_h,
			ui_metrics,
			ui_vmetrics,
			mono_face,
			cover_cache: HashMap::new(),
			color_glyphs: ColorGlyphs::new(),
			text_families: HashMap::new(),
			ui_measure_cache: HashMap::new(),
			generation: {
				static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
				NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
			},
		}
	}

	/// A chrome measurement written in DIP, in physical pixels at this display's
	/// scale factor. See `config::dip` - chrome is converted where it is used, not
	/// at a boundary, because it shares a coordinate space with the terminal grid.
	pub fn dip(&self, v: f32) -> f32 {
		config::dip(v, self.scale)
	}

	/// Can `ch` ride the shared row buffer, given the grid puts it in `cells`
	/// columns? ASCII always can. Coverage alone isn't enough: a monospace face
	/// can carry a double-width char (emoji, fullwidth punctuation) at its
	/// ordinary single advance, and then one glyph eats one column of layout
	/// where the grid gave it two - every later glyph
	/// on the row sits a cell left of the grid position its background, cursor
	/// and any per-cell glyph still use. Demanding the advance match the grid
	/// sends those to the per-cell path, which fits them to their real box.
	pub fn covered_at(&mut self, ch: char, cells: u8) -> bool {
		self.face_cells(ch) == cells
	}

	// Cells the primary face's own advance for `ch` spans, 0 when it has no
	// glyph. Measured against the face's ASCII advance so no pixel size is
	// involved; an unmeasurable face reports 1, keeping the old fast path.
	fn face_cells(&mut self, ch: char) -> u8 {
		if ch.is_ascii() {
			return 1;
		}
		if let Some(&cached) = self.cover_cache.get(&ch) {
			return cached;
		}
		let cells = self
			.mono_face
			.and_then(|id| self.font_system.get_font(id, fontdb::Weight::NORMAL))
			.map_or(0, |font| {
				let face = font.as_swash();
				let glyph = face.charmap().map(ch);
				if glyph == 0 {
					return 0;
				}
				let metrics = face.glyph_metrics(&[]);
				let unit = metrics.advance_width(face.charmap().map('M'));
				advance_cells(metrics.advance_width(glyph), unit)
			});
		self.cover_cache.insert(ch, cells);
		cells
	}

	/// Color glyph for `ch`, with the design box a caller fits to the cell. None
	/// for anything no installed color font paints - i.e. almost everything.
	/// Gated by the caller (`color_emoji`), which already holds the settings.
	pub fn color_metrics(&mut self, ch: char) -> Option<ColorMetrics> {
		self.color_glyphs.metrics(self.font_system.db(), ch)
	}

	/// Build the raster for a placed color glyph. Done here, during the frame
	/// build, because `prepare` holds the `FontSystem` (the font bytes) mutably.
	pub fn color_warm(&mut self, id: u16, w: u16, h: u16) {
		self.color_glyphs.warm(self.font_system.db(), id, w, h);
	}

	/// Open a frame's color-glyph warming, so the raster cache knows which of
	/// its entries this frame is about to depend on.
	pub fn color_frame(&mut self) {
		self.color_glyphs.begin_frame();
	}

	/// Buffer for a single fallback glyph: no monospace snapping (render at its
	/// natural width), positioned per-cell by the caller.
	pub fn new_plain_buffer(&mut self) -> Buffer {
		let mut buf = Buffer::new(&mut self.font_system, self.metrics);
		buf.set_wrap(&mut self.font_system, Wrap::None);
		buf.set_size(
			&mut self.font_system,
			Some(self.cell_w * 2.5),
			Some(self.cell_h),
		);
		buf
	}

	/// Shape one fallback glyph into `buf` and return its *ink* box (rasterized,
	/// at scale 1): `(width_px, left_px)` where `left_px` is the ink's x offset
	/// from the text-area origin. The caller fits this to the cell box - using
	/// the ink box, not the advance, because these fallback symbols routinely
	/// paint wider than they advance and would otherwise overlap the next cell.
	pub fn fill_glyph(&mut self, buf: &mut Buffer, ch: char, attrs: &Attrs) -> (f32, f32, f32) {
		// Where the terminal font puts the baseline in this line box. A fallback
		// face has its own ascent, so its glyph sits a pixel or two off the text
		// beside it - which is what made a check mark and a cross from two faces
		// read as misaligned. Shaping an 'x' first is the cheap way to ask, and it
		// is paid once per distinct glyph (the caller caches the buffer).
		let want = self.shape_ink(buf, 'x', attrs).map(|ink| ink.baseline);
		if let Some(ink) = self.shape_ink(buf, ch, attrs) {
			return self.unpaint(buf, ch, attrs, ink, want);
		}
		// The face the pinned family fell back to rasterizes nothing - a color
		// emoji font (Noto Color Emoji here) hands swash a strike it can't scale, so
		// every emoji came out as a blank cell. Reshape through the generic
		// monospace chain, which picks a face that does raster.
		let mut generic = attrs.clone();
		generic.family = Family::Monospace;
		match self.shape_ink(buf, ch, &generic) {
			Some(ink) => self.unpaint(buf, ch, &generic, ink, want),
			None => (self.cell_w, 0.0, 0.0),
		}
	}

	// A character Unicode presents as text must not come back painted: an emoji
	// face draws it in the font's own colors and ignores the color the cell was
	// set in, which is how a green check mark in a git prompt came out purple.
	// Reshape against a face that has no color glyph for it, and keep what the
	// first attempt gave if there is no such face.
	fn unpaint(
		&mut self,
		buf: &mut Buffer,
		ch: char,
		attrs: &Attrs,
		ink: Ink,
		want: Option<f32>,
	) -> (f32, f32, f32) {
		let placed = |ink: &Ink| {
			(
				ink.width,
				ink.left,
				want.map_or(0.0, |base| base - ink.baseline),
			)
		};
		if !ink.painted || crate::coloremoji::wants_color(ch) {
			return placed(&ink);
		}
		if let Some(name) = self.text_family(ch) {
			let mut plain = attrs.clone();
			plain.family = Family::Name(name);
			if let Some(retry) = self.shape_ink(buf, ch, &plain)
				&& !retry.painted
			{
				return placed(&retry);
			}
			// the retry left its own glyph in the buffer, so put the first one back
			self.shape_ink(buf, ch, attrs);
		}
		placed(&ink)
	}

	// First family that can draw `ch` without color. Leaked because `Attrs` wants
	// a 'static name, and bounded by the handful of such chars ever seen.
	fn text_family(&mut self, ch: char) -> Option<&'static str> {
		if let Some(hit) = self.text_families.get(&ch) {
			return *hit;
		}
		let found = crate::coloremoji::text_families(self.font_system.db(), ch)
			.into_iter()
			.next()
			.map(|name| &*Box::leak(name.into_boxed_str()));
		self.text_families.insert(ch, found);
		found
	}

	fn shape_ink(&mut self, buf: &mut Buffer, ch: char, attrs: &Attrs) -> Option<Ink> {
		shaped_ink(&mut self.font_system, &mut self.swash_cache, buf, ch, attrs)
	}

	/// `top` for a chrome text buffer so its VISIBLE box (cap-top to baseline)
	/// centers in a bar of height `bar_h` at `bar_top`. Uses the real font
	/// metrics, so it stays centered across font/size changes - unlike the old
	/// hand-tuned per-bar padding, which left menu titles (no descenders) riding
	/// high with empty descent space below.
	pub fn ui_text_top(&self, bar_top: f32, bar_h: f32) -> f32 {
		ui_visible_center_top(self.ui_line_h, self.ui_vmetrics, bar_top, bar_h)
	}

	/// As `ui_text_top`, but centering the whole ink box - for tab titles.
	pub fn ui_ink_top(&self, bar_top: f32, bar_h: f32) -> f32 {
		ui_ink_center_top(self.ui_line_h, self.ui_vmetrics, bar_top, bar_h)
	}

	/// How far to drop a chrome buffer that a caller centered by its LINE box, so
	/// what ends up centered is the text's visible box instead. Same rule as
	/// `ui_text_top`, as an offset for callers doing their own arithmetic.
	pub fn ui_center_dy(&self) -> f32 {
		self.ui_vmetrics.1 / 2.0
	}

	/// Screen-space baseline of chrome text placed with `ui_text_top` - for the
	/// Alt-accelerator underline.
	pub fn ui_baseline(&self, bar_top: f32, bar_h: f32) -> f32 {
		self.ui_text_top(bar_top, bar_h) + ui_baseline_in_buf(self.ui_line_h, self.ui_vmetrics)
	}

	/// Width in px of chrome `text` shaped with `attrs` at the UI font size.
	/// Sizes menus, bar titles, dialog labels to the real rendered text.
	/// Memoized by text (see `ui_measure_cache`).
	pub fn measure_ui_text(&mut self, text: &str, attrs: &Attrs) -> f32 {
		if let Some(&w) = self.ui_measure_cache.get(text) {
			return w;
		}
		let w = self.measure_at(text, attrs, self.ui_metrics);
		if self.ui_measure_cache.len() >= 512 {
			self.ui_measure_cache.clear();
		}
		self.ui_measure_cache.insert(text.to_string(), w);
		w
	}

	/// Width in px of `text` in the TERMINAL font. The tab hover tip is the one
	/// piece of chrome that uses it: its lines are key/value pairs padded to a
	/// column with spaces, which only aligns in a monospace face. Uncached - the
	/// tip keeps its width until its lines change (`TabTip` in app/tabs.rs).
	pub fn measure_mono_text(&mut self, text: &str) -> f32 {
		let attrs = mono_attrs();
		self.measure_at(text, &attrs, self.metrics)
	}

	fn measure_at(&mut self, text: &str, attrs: &Attrs, metrics: Metrics) -> f32 {
		let mut buf = Buffer::new(&mut self.font_system, metrics);
		buf.set_wrap(&mut self.font_system, Wrap::None);
		buf.set_size(&mut self.font_system, None, None);
		buf.set_text(&mut self.font_system, text, attrs, Shaping::Advanced, None);
		buf.shape_until_scroll(&mut self.font_system, false);
		buf.layout_runs().next().map_or(0.0, |run| run.line_w)
	}

	/// Chrome buffer: UI-font metrics, natural (proportional) advances - no
	/// cell-grid snap, chrome has no grid.
	pub fn new_ui_buffer(&mut self, w_px: f32, h_px: f32) -> Buffer {
		let mut buf = Buffer::new(&mut self.font_system, self.ui_metrics);
		buf.set_wrap(&mut self.font_system, Wrap::None);
		buf.set_size(
			&mut self.font_system,
			Some(w_px.max(1.0)),
			Some(h_px.max(1.0)),
		);
		buf
	}

	pub fn new_buffer(&mut self, w_px: f32, h_px: f32) -> Buffer {
		let mut buf = Buffer::new(&mut self.font_system, self.metrics);
		buf.set_wrap(&mut self.font_system, Wrap::None);
		// Snap every glyph to exactly one cell wide so the text lines up with
		// the cell grid (cursor / background quads at col*cell_w). Without this
		// glyphon lays out at the font's natural advance and the text drifts
		// from the grid across a line.
		buf.set_monospace_width(&mut self.font_system, Some(self.cell_w));
		buf.set_size(
			&mut self.font_system,
			Some(w_px.max(1.0)),
			Some(h_px.max(1.0)),
		);
		buf
	}

	pub fn resize_buffer(&mut self, buf: &mut Buffer, w_px: f32, h_px: f32) {
		buf.set_size(
			&mut self.font_system,
			Some(w_px.max(1.0)),
			Some(h_px.max(1.0)),
		);
	}

	/// Set the coverage correction for every renderer sharing this context. Cheap
	/// per frame: the uniform is only rewritten when the value moves.
	pub fn set_text_blend(&mut self, queue: &wgpu::Queue, blend: (f32, f32, f32)) {
		self.gpu()
			.viewport
			.set_text_blend(queue, blend.0, blend.1, blend.2);
	}

	pub fn update_viewport(&mut self, queue: &wgpu::Queue, w: u32, h: u32) {
		let gpu = self.gpu();
		// called per frame; only changes on resize
		if gpu.viewport_size == (w, h) {
			return;
		}
		gpu.viewport_size = (w, h);
		gpu.viewport.update(
			queue,
			Resolution {
				width: w,
				height: h,
			},
		);
	}

	pub fn prepare(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		areas: Vec<TextArea<'_>>,
	) -> Result<(), glyphon::PrepareError> {
		// Destructured so the color-glyph lookup can borrow alongside the renderer
		// and font system (disjoint fields of the same struct).
		let Self {
			gpu,
			font_system,
			swash_cache,
			color_glyphs,
			..
		} = self;
		let TextGpu {
			renderer,
			atlas,
			viewport,
			..
		} = attached(gpu);
		renderer.prepare_with_custom(
			device,
			queue,
			font_system,
			atlas,
			viewport,
			areas,
			swash_cache,
			|req| color_glyphs.raster(req),
		)
	}

	pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<(), glyphon::RenderError> {
		let gpu = attached_ref(self.gpu.as_ref());
		gpu.renderer.render(&gpu.atlas, &gpu.viewport, pass)
	}

	pub fn prepare_overlay(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		areas: Vec<TextArea<'_>>,
	) -> Result<(), glyphon::PrepareError> {
		let Self {
			gpu,
			font_system,
			swash_cache,
			..
		} = self;
		let gpu = attached(gpu);
		gpu.overlay.prepare(
			device,
			queue,
			font_system,
			&mut gpu.atlas,
			&gpu.viewport,
			areas,
			swash_cache,
		)
	}

	pub fn render_overlay(
		&self,
		pass: &mut wgpu::RenderPass<'_>,
	) -> Result<(), glyphon::RenderError> {
		let gpu = attached_ref(self.gpu.as_ref());
		gpu.overlay.render(&gpu.atlas, &gpu.viewport, pass)
	}

	pub fn prepare_scrim(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		areas: Vec<TextArea<'_>>,
	) -> Result<(), glyphon::PrepareError> {
		let Self {
			gpu,
			font_system,
			swash_cache,
			color_glyphs,
			..
		} = self;
		let TextGpu {
			scrim,
			scrim_atlas,
			viewport,
			..
		} = attached(gpu);
		scrim.prepare_with_custom(
			device,
			queue,
			font_system,
			scrim_atlas,
			viewport,
			areas,
			swash_cache,
			|req| color_glyphs.raster(req),
		)
	}

	pub fn render_scrim(
		&self,
		pass: &mut wgpu::RenderPass<'_>,
	) -> Result<(), glyphon::RenderError> {
		let gpu = attached_ref(self.gpu.as_ref());
		gpu.scrim.render(&gpu.scrim_atlas, &gpu.viewport, pass)
	}

	pub fn trim_atlas(&mut self) {
		let gpu = self.gpu();
		gpu.atlas.trim();
		gpu.scrim_atlas.trim();
	}
}

// Measure the per-cell advance the *render* buffer actually produces. We shape
// with `Shaping::Advanced` (what panes use) over a long run so per-glyph hinting
// rounding averages out. The result is intentionally NOT rounded: cosmic-text's
// `set_monospace_width` only snaps advances for fonts that report a monospace
// em-width, so for many system fonts the real pitch is the font's natural
// advance (~fractional). Rounding cell_w away from that pitch made the cursor,
// cell backgrounds, and per-cell fallback glyphs (all placed at col*cell_w)
// drift right of the text, worsening with column count. Matching the real pitch
// keeps the drift sub-pixel (bounded by hinting, not accumulating).
fn measure_cell(fs: &mut FontSystem, metrics: Metrics) -> f32 {
	const N: usize = 40;
	let mut buf = Buffer::new(fs, metrics);
	buf.set_size(fs, None, None);
	let attrs = mono_attrs();
	buf.set_text(fs, &"M".repeat(N), &attrs, Shaping::Advanced, None);
	buf.shape_until_scroll(fs, false);
	buf.layout_runs()
		.next()
		.map_or(metrics.font_size * 0.6, |run| run.line_w / N as f32)
		.max(1.0)
}

// Does a bold run shape to the same advance as the regular pitch cell_w? Shaped
// exactly like the render/scrim buffers (monospace_width set, Advanced). True
// when the mono face honors the snap, or bold and regular naturally share an
// advance; false when they diverge - then the de-bold scrim buffer drifts from
// the display buffer and must not be used (see the debold_safe field).
fn bold_matches_cell(fs: &mut FontSystem, metrics: Metrics, cell_w: f32) -> bool {
	const N: usize = 40;
	let mut buf = Buffer::new(fs, metrics);
	buf.set_wrap(fs, Wrap::None);
	buf.set_monospace_width(fs, Some(cell_w));
	buf.set_size(fs, None, None);
	let mut attrs = mono_attrs();
	attrs.weight = fontdb::Weight(MONO_WEIGHT_BOLD.load(std::sync::atomic::Ordering::Relaxed));
	buf.set_text(fs, &"M".repeat(N), &attrs, Shaping::Advanced, None);
	buf.shape_until_scroll(fs, false);
	let adv = buf
		.layout_runs()
		.next()
		.map_or(cell_w, |run| run.line_w / N as f32);
	(adv - cell_w).abs() < 0.1
}

// A shaped fallback glyph: how wide its ink is, where that ink starts, and
// whether the face drew it in its own colors.
#[derive(Clone, Copy)]
struct Ink {
	width: f32,
	left: f32,
	painted: bool,
	// where the FACE put the baseline in the line box, which is not where the
	// terminal font puts it - see fill_glyph
	baseline: f32,
}

// Shape `ch` into `buf` and measure its rasterized ink as `(width_px, left_px)`.
// None when the face draws nothing - no glyph for it, or a glyph that rasterizes
// empty (which is what a color-bitmap emoji face does through swash here).
fn shaped_ink(
	fs: &mut FontSystem,
	swash: &mut SwashCache,
	buf: &mut Buffer,
	ch: char,
	attrs: &Attrs,
) -> Option<Ink> {
	let mut utf8_buf = [0u8; 4];
	buf.set_text(
		fs,
		ch.encode_utf8(&mut utf8_buf),
		attrs,
		Shaping::Advanced,
		None,
	);
	buf.shape_until_scroll(fs, false);
	let run = buf.layout_runs().next()?;
	let baseline = run.line_y;
	let phys = run.glyphs.first()?.physical((0.0, 0.0), 1.0);
	let image = swash.get_image(fs, phys.cache_key).as_ref()?;
	if image.placement.width == 0 || image.placement.height == 0 {
		return None;
	}
	Some(Ink {
		width: image.placement.width as f32,
		left: phys.x as f32 + image.placement.left as f32,
		painted: image.content == SwashContent::Color,
		baseline,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	// Only dark-on-light gets the correction. Applying it the other way would
	// thin text that linear blending has already made heavy enough.
	// Test ID: EqWXhBB
	#[test]
	fn only_text_darker_than_its_background_is_corrected() {
		let (black, white) = ([0, 0, 0], [255, 255, 255]);
		assert_eq!(text_blend(black, white, 1.0).2, 1.0);
		assert_eq!(text_blend(white, black, 1.0).2, 0.0);
		// a light theme's real pair, not just the extremes
		assert_eq!(
			text_blend([0x30, 0x2c, 0x28], [0xf2, 0xef, 0xe9], 1.0).2,
			1.0
		);
		assert_eq!(
			text_blend([0xd8, 0xd4, 0xcc], [0x1c, 0x1c, 0x22], 1.0).2,
			0.0
		);
		// nothing to correct where the two are the same
		assert_eq!(text_blend(white, white, 1.0).2, 0.0);
		// the setting off means the shader takes its old path either way
		assert_eq!(text_blend(black, white, 0.0).2, 0.0);
		// the headroom above the blend is real, and it stops somewhere
		assert_eq!(text_blend(black, white, 1.6).2, 1.6);
		assert_eq!(
			text_blend(black, white, 9.0).2,
			crate::config::MAX_DARK_ON_LIGHT
		);
	}

	// The pair the curve is built from spans the real pair's own brightness, and
	// the shader divides by the gap between the two - so a dark-on-light pair has
	// to come back the darker one first.
	// Test ID: EqWXhBC
	#[test]
	fn the_pair_reaches_the_shader_as_grays_of_its_own_brightness() {
		let (fg, bg, _) = text_blend([0, 0, 0], [255, 255, 255], 1.0);
		assert!(fg == 0.0 && (bg - 1.0).abs() < 1e-5, "{fg} against {bg}");
		let (fg, bg, on) = text_blend([0x30, 0x32, 0x38], [0xf6, 0xf5, 0xf0], 1.0);
		assert!(fg > 0.15 && fg < 0.25, "fg gray {fg}");
		assert!(bg > 0.93 && bg < 0.99, "bg gray {bg}");
		assert_eq!(on, 1.0);
		// a saturated pair still reads in the right order
		let (fg, bg, _) = text_blend([0x07, 0x3d, 0x14], [0xe9, 0xee, 0xe9], 1.0);
		assert!(fg < bg, "{fg} against {bg}");
	}

	// Ink a screen of text puts down, in sRGB levels as a share of the way from
	// the background to the text, averaged over every coverage. Dark mode blends
	// in linear light untouched; light mode goes through the shader's lift and
	// sRGB match, mirrored here.
	fn ink(fg: [u8; 3], bg: [u8; 3], contrast: f32) -> f32 {
		let (fg_g, bg_g, amount) = text_blend(fg, bg, 1.0);
		let (fg_l, bg_l) = (
			crate::config::to_linear_f32(fg_g),
			crate::config::to_linear_f32(bg_g),
		);
		let steps = 100;
		let mut sum = 0.0;
		for step in 0..steps {
			let coverage = (step as f32 + 0.5) / steps as f32;
			let alpha = if amount > 0.0 {
				let lifted = coverage * (contrast + 1.0) / (coverage * contrast + 1.0);
				let blended = lifted * fg_g + (1.0 - lifted) * bg_g;
				(crate::config::to_linear_f32(blended) - bg_l) / (fg_l - bg_l)
			} else {
				coverage
			};
			let out = crate::config::from_linear(alpha * fg_l + (1.0 - alpha) * bg_l);
			sum += (out - bg_g) / (fg_g - bg_g);
		}
		sum / steps as f32
	}

	// Light mode's text has to look as heavy as dark mode's, every built-in
	// theme. With the sRGB match alone it put down about three quarters of the
	// ink, which is what read as thin.
	// Test ID: ErwcyUJ
	#[test]
	fn light_text_puts_down_the_ink_dark_text_does() {
		for (name, t) in crate::theme::THEMES {
			let dark = ink(t.dark.fg, t.dark.bg, crate::config::DARK_ON_LIGHT_CONTRAST);
			let light = ink(
				t.light.fg,
				t.light.bg,
				crate::config::DARK_ON_LIGHT_CONTRAST,
			);
			let ratio = light / dark;
			assert!(
				(0.95..=1.12).contains(&ratio),
				"{name}: light mode puts down {ratio:.3} of dark mode's ink"
			);
			let plain = ink(t.light.fg, t.light.bg, 0.0) / dark;
			assert!(plain < 0.85, "{name}: {plain:.3} with no lift");
		}
	}

	// A monospace face routinely carries a double-width char at its ordinary
	// single advance (Monaspace Argon does it for 53 of them, emoji included).
	// Whole cells is what the row buffer lays out in, so the rounding has to
	// report that honestly rather than call anything close enough.
	// Test ID: Elm5uB6
	#[test]
	fn a_single_advance_never_reads_as_two_cells() {
		let unit = 1240.0; // Monaspace Argon's own ASCII advance, in font units
		assert_eq!(advance_cells(unit, unit), 1);
		assert_eq!(advance_cells(unit * 2.0, unit), 2);
		assert_eq!(
			advance_cells(unit * 1.02, unit),
			1,
			"hinting slack is not a cell"
		);
		assert_eq!(advance_cells(0.0, unit), 0);
		// A face that reports no usable advance keeps the shared-buffer path.
		assert_eq!(advance_cells(0.0, 0.0), 1);
	}

	// The search order is one list on every platform. `use_system_font` only
	// decides where the OS family sits in it - it must never drop the configured
	// stack, which is what made Linux and Windows resolve the same config
	// differently, and the built-in stack always backs both up.
	// Test ID: ElEvh0U
	#[test]
	fn mono_candidates_keep_one_order_on_every_platform() {
		let configured = Some("Alpha, Beta");
		let builtin: Vec<String> = config::DEFAULT_FONT_STACK
			.split(',')
			.map(|name| name.trim().to_string())
			.collect();

		let following = mono_candidates(Some("OS Mono"), configured, true);
		assert_eq!(following[..3], ["OS Mono", "Alpha", "Beta"]);
		let not_following = mono_candidates(Some("OS Mono"), configured, false);
		assert_eq!(not_following[..3], ["Alpha", "Beta", "OS Mono"]);
		for order in [&following, &not_following] {
			assert!(order.ends_with(&builtin), "built-in stack must back it up");
		}

		// Windows reports no OS family, so following it is a no-op there: the
		// order collapses onto the same list every other platform ends up with.
		assert_eq!(
			mono_candidates(None, configured, true),
			mono_candidates(None, configured, false)
		);
		assert_eq!(
			mono_candidates(None, configured, true)[..2],
			["Alpha", "Beta"]
		);

		// An empty or absent stack leaves no blank entries behind.
		assert_eq!(mono_candidates(None, Some(" , ,"), false), builtin);
		assert_eq!(mono_candidates(None, None, true), builtin);
	}

	// A pinned mono family falls back to a color emoji face that rasterizes to
	// nothing, which drew every emoji as an empty cell. The generic-monospace
	// retry must find a face that actually paints.
	// Test ID: ElBP6CG
	#[test]
	fn emoji_falls_back_to_a_face_that_rasterizes() {
		let mut fs = FontSystem::new();
		let mut swash = SwashCache::new();
		let mut buf = Buffer::new(&mut fs, Metrics::new(18.0, 22.0));
		let mut generic = mono_attrs();
		generic.family = Family::Monospace;
		for ch in ['\u{1F680}', '\u{1F525}', '\u{1F49A}'] {
			assert!(
				shaped_ink(&mut fs, &mut swash, &mut buf, ch, &generic).is_some(),
				"no ink for {ch:?}"
			);
		}
	}

	// Chrome must pin a concrete face, never fall back to generic
	// `Family::SansSerif` (which picks a serif when fontdb's "Arial" default
	// is absent). Only needs a FontSystem (no GPU), so it runs with no display.
	// A chrome line placed by ui_visible_center_top must sit with its visible
	// (ascender-top..baseline) box centered in the bar, for any bar height and
	// metrics - so a font/size change stays balanced without hand-tuned padding.
	// Test ID: EiyvOXw
	#[test]
	fn chrome_text_visible_box_centers_in_bar() {
		let vmetrics = (13.6f32, 3.4f32); // ascent, descent px
		let ui_line_h = 23.0;
		for &(bar_top, bar_h) in &[(0.0f32, 29.0f32), (29.0, 29.0), (10.0, 40.0)] {
			let top = ui_visible_center_top(ui_line_h, vmetrics, bar_top, bar_h);
			let baseline = top + ui_baseline_in_buf(ui_line_h, vmetrics);
			let visible_center = baseline - vmetrics.0 / 2.0; // midpoint of ascender..baseline
			assert!(
				(visible_center - (bar_top + bar_h / 2.0)).abs() < 0.01,
				"visible center {visible_center} != bar center {}",
				bar_top + bar_h / 2.0
			);
		}
	}

	// A tab title placed by ui_ink_center_top sits with ascender-top to
	// descender-bottom centered, so descenders don't crowd the button's edge.
	// Test ID: EoT6qwS
	#[test]
	fn chrome_ink_box_centers_in_bar() {
		let vmetrics = (14.6f32, 4.7f32);
		let ui_line_h = 23.0;
		for &(bar_top, bar_h) in &[(0.0f32, 26.0f32), (31.0, 26.0), (10.0, 40.0)] {
			let top = ui_ink_center_top(ui_line_h, vmetrics, bar_top, bar_h);
			let baseline = top + ui_baseline_in_buf(ui_line_h, vmetrics);
			let above = baseline - vmetrics.0 - bar_top;
			let below = bar_top + bar_h - (baseline + vmetrics.1);
			assert!(
				(above - below).abs() < 0.01,
				"above {above} != below {below}"
			);
		}
	}

	// Test ID: Eip4OOO
	#[test]
	fn ui_font_resolves_to_concrete_family() {
		let fs = FontSystem::new();
		let fam = resolve_ui_family(&fs);
		eprintln!("resolved chrome UI family: {fam:?}");
		assert!(fam.is_some(), "no concrete UI family resolved for chrome");
	}

	// Test ID: EipMwEi
	#[test]
	fn ui_attrs_shape_in_pinned_family() {
		let mut fs = FontSystem::new();
		pin_ui_family(&fs);
		let attrs = ui_attrs();
		let Family::Name(want) = attrs.family else {
			eprintln!("no pinned family on this box; skipping");
			return;
		};
		let mut b = Buffer::new(&mut fs, Metrics::new(17.0, 22.0));
		b.set_size(&mut fs, Some(400.0), Some(30.0));
		b.set_text(&mut fs, "File Edit", &attrs, Shaping::Advanced, None);
		b.shape_until_scroll(&mut fs, false);
		for run in b.layout_runs() {
			for g in run.glyphs {
				let fams: Vec<String> = fs
					.db()
					.face(g.font_id)
					.map(|f| f.families.iter().map(|(n, _)| n.clone()).collect())
					.unwrap_or_default();
				eprintln!("glyph font: {fams:?} weight_req={:?}", attrs.weight);
				assert!(
					fams.iter().any(|n| n == want),
					"chrome glyph shaped in {fams:?}, not the pinned family {want:?}"
				);
			}
		}
	}

	// A terminal bold run must stay in the pinned monospace family. A literal
	// Weight::BOLD in a mono family with no bold face ejects the run into a
	// proportional bold fallback (advances set_monospace_width can't snap), which
	// skews space-based alignment. mono_bold_weight() requests the family's
	// boldest available face instead, so bold never leaves the family.
	// Test ID: EkhEfku
	#[test]
	fn mono_bold_stays_in_pinned_family() {
		let mut fs = FontSystem::new();
		pin_mono_family(&fs);
		let mut attrs = mono_attrs();
		let Family::Name(want) = attrs.family else {
			eprintln!("no concrete mono family on this box; skipping");
			return;
		};
		attrs.weight = mono_bold_weight();
		// the pinned weight must be one the family actually ships
		let shipped: Vec<u16> = fs
			.db()
			.faces()
			.filter(|f| f.families.iter().any(|(n, _)| n == want))
			.map(|f| f.weight.0)
			.collect();
		assert!(
			shipped.contains(&mono_bold_weight().0),
			"bold weight {} not shipped by {want:?} (has {shipped:?})",
			mono_bold_weight().0
		);
		let mut b = Buffer::new(&mut fs, Metrics::new(16.0, 20.0));
		b.set_monospace_width(&mut fs, Some(9.0));
		b.set_size(&mut fs, Some(400.0), Some(30.0));
		b.set_text(&mut fs, "MMMM", &attrs, Shaping::Advanced, None);
		b.shape_until_scroll(&mut fs, false);
		for run in b.layout_runs() {
			for g in run.glyphs {
				let fams: Vec<String> = fs
					.db()
					.face(g.font_id)
					.map(|f| f.families.iter().map(|(n, _)| n.clone()).collect())
					.unwrap_or_default();
				assert!(
					fams.iter().any(|n| n == want),
					"bold mono glyph shaped in {fams:?}, not the pinned family {want:?}"
				);
			}
		}
	}

	// The cell width is the text's real pitch, unrounded. Everything placed on
	// the grid - cursor, cell backgrounds, fallback glyphs - sits at column times
	// cell width, so a rounded width drifted further off the text the longer the
	// line, and the cursor sat visibly past the end of it.
	// Test ID: Er2VGXK
	#[test]
	fn the_cell_width_is_the_texts_real_pitch() {
		let mut fs = FontSystem::new();
		pin_mono_family(&fs);
		if mono_family().is_none() {
			eprintln!("no concrete mono family on this box; skipping");
			return;
		}
		for size in [13.0, 15.0, 17.0] {
			let metrics = Metrics::new(size, (size * 1.2).round());
			let cw = measure_cell(&mut fs, metrics);
			let mut buf = Buffer::new(&mut fs, metrics);
			buf.set_size(&mut fs, None, None);
			buf.set_text(
				&mut fs,
				&"M".repeat(120),
				&mono_attrs(),
				Shaping::Advanced,
				None,
			);
			buf.shape_until_scroll(&mut fs, false);
			let line_w = buf.layout_runs().next().map_or(0.0, |run| run.line_w);
			assert!(
				(line_w - 120.0 * cw).abs() < 0.5,
				"at {size}px 120 cells of {cw} miss the text's {line_w}"
			);
		}
	}

	// The glow is drawn from a copy of the text with bold taken out. Where bold
	// shapes wider than the cell (some Windows faces ignore the fixed pitch),
	// that copy drifts from the text along the line, so it is used only when a
	// bold run lands on the cell pitch exactly.
	// Test ID: Er2VGXL
	#[test]
	fn bold_is_stripped_for_the_glow_only_where_it_keeps_the_pitch() {
		let mut fs = FontSystem::new();
		pin_mono_family(&fs);
		if mono_family().is_none() {
			eprintln!("no concrete mono family on this box; skipping");
			return;
		}
		let metrics = Metrics::new(15.0, 18.0);
		let cell_w = measure_cell(&mut fs, metrics);
		assert!(bold_matches_cell(&mut fs, metrics, cell_w));
		assert!(!bold_matches_cell(&mut fs, metrics, cell_w + 3.0));
	}

	// A mono face can carry a double-width char at its ordinary one-cell advance.
	// Laid out in the shared row buffer it takes one column where the grid gave
	// it two, and everything after it on the row sits a cell to the left. Only a
	// char whose advance matches the grid's cell count may ride the row buffer.
	// Test ID: Er2VGXM
	#[test]
	fn a_char_the_face_draws_one_cell_wide_is_not_taken_for_two() {
		let mut ctx = TextCtx::new_cpu(1.0);
		if mono_family().is_none() {
			eprintln!("no concrete mono family on this box; skipping");
			return;
		}
		assert!(ctx.covered_at('A', 1));
		// chars the grid gives two cells
		let wide = [
			'\u{FF01}', '\u{FF21}', '\u{2705}', '\u{26A1}', '\u{231B}', '\u{2615}', '\u{4E2D}',
		];
		let Some(ch) = wide.into_iter().find(|&ch| ctx.face_cells(ch) == 1) else {
			eprintln!("the mono face draws none of {wide:?} at one cell; skipping");
			return;
		};
		assert!(
			ctx.covered_at(ch, 1),
			"{ch:?} rides the row buffer as one cell"
		);
		assert!(
			!ctx.covered_at(ch, 2),
			"{ch:?} at one cell must not fill the grid's two"
		);
	}
}
