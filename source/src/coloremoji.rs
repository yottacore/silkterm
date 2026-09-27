// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Color glyph (`COLRv1`) rasterization.
//!
//! swash - the rasterizer cosmic-text drives - only reads COLR **v0**, and every
//! current color emoji font ships v1 only. Those glyphs come back as an empty
//! image, so the fallback path picks a monochrome face instead. skrifa (already
//! in the tree, under swash) walks the v1 paint graph; this module is the 2D back
//! end it paints into - transform/clip/layer stacks over zeno's coverage
//! rasterizer - producing straight-alpha sRGB RGBA, which glyphon uploads to its
//! color atlas as a custom glyph.
//!
//! Rasters are built ahead of `prepare` (see `TextCtx`), because glyphon holds the
//! `FontSystem` mutably during it and the font bytes come from that same database.

use std::collections::HashMap;

use glyphon::cosmic_text::fontdb;
use glyphon::{ContentType, RasterizeCustomGlyphRequest, RasterizedCustomGlyph};
use skrifa::color::{Brush, ColorPainter, ColorPalettes, ColorStop, CompositeMode, Extend};
use skrifa::color::{PaintError, Transform};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlineGlyphCollection, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::raw::types::BoundingBox;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use zeno::{Command, Format, Mask, Origin, Point as ZPoint};

// Gradient color-line resolution. A 256-entry lookup beats searching the stop
// list per pixel and is finer than the 8-bit output can show.
const RAMP_LEN: usize = 256;
// Layer nesting cap. skrifa's decycler stops paint-graph cycles; this bounds the
// scratch a pathological (or hostile) font can make us allocate.
const MAX_LAYERS: usize = 24;
// Raster cache cap (distinct glyph + pixel size). A screen of nothing but
// distinct emoji is the worst case: cells/2 of them, so this has to clear a
// large grid comfortably or the sweep below runs every frame.
const MAX_RASTERS: usize = 4096;
// Frames a raster is pinned for once warmed. glyphon re-requests a glyph when
// its atlas entry is evicted and trimmed, which can be a frame or two after we
// last warmed it, and answering None to something we previously answered Some
// is a panic inside glyphon - so the sweep only ever drops older stamps.
const RASTER_PIN_FRAMES: u64 = 2;

// A color glyph resolved for one char: which face holds it, and the design box
// the raster covers (font units) so the caller can fit it to a cell.
#[derive(Clone, Copy)]
pub struct ColorMetrics {
	pub id: u16,
	pub box_w: f32,
	pub box_h: f32,
}

#[derive(Clone, Copy)]
struct Resolved {
	// glyphon custom-glyph id, assigned on first resolve (chars index)
	id: u16,
	face: fontdb::ID,
	gid: GlyphId,
	box_x: f32,
	box_y: f32,
	box_w: f32,
	box_h: f32,
}

pub struct ColorGlyphs {
	// Faces with a COLR table, found by one lazy pass over the db (only paid once
	// a color candidate actually appears on screen).
	faces: Option<Vec<fontdb::ID>>,
	// Per-char resolution, including the misses - a char with no color glyph is
	// looked up on every cell of every frame otherwise. A hit carries its
	// glyphon custom-glyph id (u16, handed out per char, never reused).
	lookup: HashMap<char, Option<Resolved>>,
	chars: Vec<char>,
	// Value carries the frame it was last warmed on, so the overflow sweep can
	// tell a raster this frame still needs from one nothing references.
	rasters: HashMap<(u16, u16, u16), (u64, Vec<u8>)>,
	frame: u64,
}

// Should this char be PAINTED, or set in the terminal font? An emoji face
// covers far more than it ought to be used for - the heavy check mark and the
// copyright sign are both in Noto Color Emoji - and painting one loses the
// ANSI color it was set in, which is how a green check in a git prompt came
// out purple. Unicode answers with Emoji_Presentation, and the answer is No
// for nearly everything below U+1F000, so the exceptions are listed. Above
// that it is Yes for nearly everything, and the handful of No's up there are
// pictures rather than text, so they are left to the font.
pub fn wants_color(ch: char) -> bool {
	#[rustfmt::skip]
	const PAINTED: &[(u32, u32)] = &[
		(0x231A, 0x231B), (0x23E9, 0x23EC), (0x23F0, 0x23F0), (0x23F3, 0x23F3),
		(0x25FD, 0x25FE), (0x2614, 0x2615), (0x2648, 0x2653), (0x267F, 0x267F),
		(0x2693, 0x2693), (0x26A1, 0x26A1), (0x26AA, 0x26AB), (0x26BD, 0x26BE),
		(0x26C4, 0x26C5), (0x26CE, 0x26CE), (0x26D4, 0x26D4), (0x26EA, 0x26EA),
		(0x26F2, 0x26F3), (0x26F5, 0x26F5), (0x26FA, 0x26FA), (0x26FD, 0x26FD),
		(0x2705, 0x2705), (0x270A, 0x270B), (0x2728, 0x2728), (0x274C, 0x274C),
		(0x274E, 0x274E), (0x2753, 0x2755), (0x2757, 0x2757), (0x2795, 0x2797),
		(0x27B0, 0x27B0), (0x27BF, 0x27BF), (0x2B1B, 0x2B1C), (0x2B50, 0x2B50),
		(0x2B55, 0x2B55),
	];
	let c = ch as u32;
	c >= 0x1_F000 || PAINTED.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

impl ColorGlyphs {
	pub fn new() -> Self {
		Self {
			faces: None,
			lookup: HashMap::new(),
			chars: Vec::new(),
			rasters: HashMap::new(),
			frame: 0,
		}
	}

	// Does `ch` have a color glyph, and how big is it in design units? None for
	// the overwhelming majority of chars, so the miss is cached too.
	pub fn metrics(&mut self, db: &fontdb::Database, ch: char) -> Option<ColorMetrics> {
		if !wants_color(ch) {
			return None;
		}
		if let Some(hit) = self.lookup.get(&ch) {
			let hit = (*hit)?;
			return Some(ColorMetrics {
				id: hit.id,
				box_w: hit.box_w,
				box_h: hit.box_h,
			});
		}
		// u16 ids: 65k distinct color glyphs is far past any real font's coverage,
		// but refuse (cache as a miss) rather than wrap onto a live id.
		let found = self.resolve(db, ch).and_then(|mut hit| {
			hit.id = u16::try_from(self.chars.len()).ok()?;
			self.chars.push(ch);
			Some(hit)
		});
		self.lookup.insert(ch, found);
		let found = found?;
		Some(ColorMetrics {
			id: found.id,
			box_w: found.box_w,
			box_h: found.box_h,
		})
	}

	fn resolve(&mut self, db: &fontdb::Database, ch: char) -> Option<Resolved> {
		let faces = self.faces.get_or_insert_with(|| color_faces(db));
		for &face in faces.iter() {
			let hit = db.with_face_data(face, |data, index| {
				let font = FontRef::from_index(data, index).ok()?;
				let gid = font.charmap().map(ch)?;
				let glyph = font.color_glyphs().get(gid)?;
				// A ClipBox gives the exact painted extent; without one, fall back to
				// the glyph's advance and the font's vertical extent - a color font
				// fills that box closely enough for cell fitting.
				let (x, y, w, h) =
					match glyph.bounding_box(LocationRef::default(), Size::unscaled()) {
						Some(bb) if bb.x_max > bb.x_min && bb.y_max > bb.y_min => {
							(bb.x_min, bb.y_min, bb.x_max - bb.x_min, bb.y_max - bb.y_min)
						}
						_ => {
							let metrics = font.metrics(Size::unscaled(), LocationRef::default());
							let adv = font
								.glyph_metrics(Size::unscaled(), LocationRef::default())
								.advance_width(gid)
								.unwrap_or(metrics.units_per_em.into());
							let top = metrics.ascent;
							let bot = metrics.descent;
							if adv <= 0.0 || top <= bot {
								return None;
							}
							(0.0, bot, adv, top - bot)
						}
					};
				Some(Resolved {
					id: 0, // assigned by metrics() on first sight
					face,
					gid,
					box_x: x,
					box_y: y,
					box_w: w,
					box_h: h,
				})
			});
			if let Some(hit @ Some(_)) = hit {
				return hit;
			}
		}
		None
	}

	// Start of a frame's warming. Rasters warmed from here on are pinned against
	// the overflow sweep until they are RASTER_PIN_FRAMES old.
	pub fn begin_frame(&mut self) {
		self.frame = self.frame.wrapping_add(1);
	}

	// Build the raster for `id` at exactly `w`x`h` px if it isn't cached. Called
	// from the frame build, so `prepare`'s callback only ever does a lookup.
	pub fn warm(&mut self, db: &fontdb::Database, id: u16, w: u16, h: u16) {
		if w == 0 || h == 0 {
			return;
		}
		// A hit still has to re-stamp: an emoji on screen every frame must not
		// age out from under the atlas just because it was cached long ago.
		if let Some(entry) = self.rasters.get_mut(&(id, w, h)) {
			entry.0 = self.frame;
			return;
		}
		let Some(&ch) = self.chars.get(id as usize) else {
			return;
		};
		let Some(Some(res)) = self.lookup.get(&ch).copied() else {
			return;
		};
		let rgba = db
			.with_face_data(res.face, |data, index| paint(data, index, &res, w, h))
			.flatten();
		if let Some(rgba) = rgba {
			self.sweep();
			self.rasters.insert((id, w, h), (self.frame, rgba));
		}
	}

	// Drop only what has aged out. Clearing wholesale is what made a screenful
	// of distinct emoji abort: the sweep fell mid-frame, between glyphon
	// rasterizing a glyph and asking for it again. If everything is still
	// pinned the cache simply grows - bounded by what fits on screen, which is
	// a great deal cheaper than a crash.
	fn sweep(&mut self) {
		if self.rasters.len() < MAX_RASTERS {
			return;
		}
		let cutoff = self.frame.saturating_sub(RASTER_PIN_FRAMES);
		self.rasters.retain(|_, (stamp, _)| *stamp > cutoff);
	}

	// glyphon's rasterize callback: a pure lookup (see `warm`).
	//
	// A miss answers with transparent pixels rather than None. glyphon
	// re-rasterizes every entry it holds when its atlas grows, and a glyph that
	// stops answering aborts it - the same crash a wholesale clear caused. The
	// sweep can still drop a key glyphon is holding, so the answer has to be
	// pixels; the glyph simply is not drawn.
	pub fn raster(&self, req: RasterizeCustomGlyphRequest) -> Option<RasterizedCustomGlyph> {
		if req.width == 0 || req.height == 0 {
			return None;
		}
		let data = self
			.rasters
			.get(&(req.id, req.width, req.height))
			.map_or_else(
				|| vec![0u8; req.width as usize * req.height as usize * 4],
				|(_, data)| data.clone(),
			);
		Some(RasterizedCustomGlyph {
			data,
			content_type: ContentType::Color,
		})
	}
}

// Faces carrying a COLR table. fontdb memory-maps its sources, so this touches
// little more than each file's table directory.
// Families that can draw `ch` with no color in it, monospaced ones first.
// cosmic-text picks a fallback face on its own and will take an emoji face for
// a character Unicode presents as text, which then paints in the font's own
// colors and ignores the color the cell was set in.
pub fn text_families(db: &fontdb::Database, ch: char) -> Vec<String> {
	let mut found: Vec<(bool, String)> = db
		.faces()
		.filter_map(|info| {
			let plain = db.with_face_data(info.id, |data, index| {
				FontRef::from_index(data, index).is_ok_and(|font| {
					font.charmap()
						.map(ch)
						.is_some_and(|gid| font.color_glyphs().get(gid).is_none())
				})
			})?;
			let name = info.families.first()?.0.clone();
			plain.then_some((info.monospaced, name))
		})
		.collect();
	found.sort_by_key(|(mono, _)| !mono);
	found.dedup_by(|a, b| a.1 == b.1);
	found.into_iter().map(|(_, name)| name).collect()
}

fn color_faces(db: &fontdb::Database) -> Vec<fontdb::ID> {
	db.faces()
		.filter(|info| {
			db.with_face_data(info.id, |data, index| {
				FontRef::from_index(data, index)
					.ok()
					.is_some_and(|font| font.colr().is_ok())
			}) == Some(true)
		})
		.map(|info| info.id)
		.collect()
}

// Paint one color glyph into a straight-alpha sRGB RGBA buffer of `w`x`h`.
fn paint(data: &[u8], index: u32, res: &Resolved, w: u16, h: u16) -> Option<Vec<u8>> {
	let font = FontRef::from_index(data, index).ok()?;
	let glyph = font.color_glyphs().get(res.gid)?;
	let (w, h) = (w as usize, h as usize);
	// Design box -> pixels, flipping the font's y-up axis to the bitmap's y-down.
	let sx = w as f32 / res.box_w;
	let sy = h as f32 / res.box_h;
	let base = Transform {
		xx: sx,
		yx: 0.0,
		xy: 0.0,
		yy: -sy,
		dx: -res.box_x * sx,
		dy: (res.box_y + res.box_h) * sy,
	};
	let palette = ColorPalettes::new(&font)
		.get(0)
		.map(|p| {
			p.colors()
				.iter()
				.map(|c| {
					[
						f32::from(c.red) / 255.0,
						f32::from(c.green) / 255.0,
						f32::from(c.blue) / 255.0,
						f32::from(c.alpha) / 255.0,
					]
				})
				.collect()
		})
		.unwrap_or_default();
	let mut painter = Painter {
		outlines: font.outline_glyphs(),
		palette,
		w,
		h,
		layers: vec![vec![0.0; w * h * 4]],
		clips: Vec::new(),
		xforms: vec![base],
		overflowed: false,
	};
	glyph.paint(LocationRef::default(), &mut painter).ok()?;
	Some(painter.finish())
}

// One 2D target: premultiplied RGBA, four floats per pixel. Premultiplied so
// layer compositing is a plain lerp; the sRGB values are blended as-is, which is
// what every other COLRv1 renderer does.
type Layer = Vec<f32>;

struct Painter<'a> {
	outlines: OutlineGlyphCollection<'a>,
	palette: Vec<[f32; 4]>,
	w: usize,
	h: usize,
	layers: Vec<Layer>,
	// Each entry is already intersected with its parent, so a fill only reads the
	// top one. Empty means "unclipped".
	clips: Vec<Vec<u8>>,
	xforms: Vec<Transform>,
	// A layer push past MAX_LAYERS draws nothing rather than allocating; the
	// matching pop must then also do nothing.
	overflowed: bool,
}

impl Painter<'_> {
	fn cur(&self) -> Transform {
		self.xforms.last().copied().unwrap_or_default()
	}

	// Rasterize a device-space path to 8-bit coverage over the whole bitmap.
	fn cover(&self, path: &[Command]) -> Vec<u8> {
		let mut buf = vec![0u8; self.w * self.h];
		if !path.is_empty() {
			Mask::new(path)
				.format(Format::Alpha)
				.origin(Origin::TopLeft)
				.size(self.w as u32, self.h as u32)
				.render_into(&mut buf, None);
		}
		buf
	}

	fn push_cover(&mut self, mask: Vec<u8>) {
		let merged = match self.clips.last() {
			Some(parent) => parent
				.iter()
				.zip(&mask)
				.map(|(&a, &b)| ((u16::from(a) * u16::from(b) + 127) / 255) as u8)
				.collect(),
			None => mask,
		};
		self.clips.push(merged);
	}

	// Color line as a lookup table over t in 0..1, straight sRGB.
	fn ramp(&self, stops: &[ColorStop]) -> Vec<[f32; 4]> {
		let mut lut = vec![[0.0; 4]; RAMP_LEN];
		if stops.is_empty() {
			return lut;
		}
		for (i, slot) in lut.iter_mut().enumerate() {
			let t = i as f32 / (RAMP_LEN - 1) as f32;
			*slot = self.stop_at(stops, t);
		}
		lut
	}

	fn stop_at(&self, stops: &[ColorStop], t: f32) -> [f32; 4] {
		let first = stops[0];
		if t <= first.offset || stops.len() == 1 {
			return self.stop_color(first);
		}
		let last = stops[stops.len() - 1];
		if t >= last.offset {
			return self.stop_color(last);
		}
		for pair in stops.windows(2) {
			let (a, b) = (pair[0], pair[1]);
			if t >= a.offset && t <= b.offset {
				let span = b.offset - a.offset;
				// Coincident stops are a hard color break; take the later one.
				let f = if span > f32::EPSILON {
					(t - a.offset) / span
				} else {
					1.0
				};
				let (ca, cb) = (self.stop_color(a), self.stop_color(b));
				let mut out = [0.0; 4];
				for c in 0..4 {
					out[c] = ca[c] + (cb[c] - ca[c]) * f;
				}
				return out;
			}
		}
		self.stop_color(last)
	}

	fn stop_color(&self, stop: ColorStop) -> [f32; 4] {
		let mut color = self.color(stop.palette_index);
		color[3] *= stop.alpha;
		color
	}

	// CPAL lookup. 0xFFFF means "the text foreground color" - the raster is
	// cached independently of the cell color, so it resolves to white (which
	// leaves such a paint tintable later if that's ever wanted).
	fn color(&self, index: u16) -> [f32; 4] {
		if index == 0xFFFF {
			return [1.0, 1.0, 1.0, 1.0];
		}
		self.palette
			.get(index as usize)
			.copied()
			.unwrap_or([1.0, 1.0, 1.0, 1.0])
	}

	fn finish(mut self) -> Vec<u8> {
		let layer = self.layers.swap_remove(0);
		let mut out = vec![0u8; self.w * self.h * 4];
		for (px, chunk) in layer.chunks_exact(4).enumerate() {
			let a = chunk[3].clamp(0.0, 1.0);
			// glyphon blends straight alpha, so undo the premultiply.
			let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
			for c in 0..3 {
				out[px * 4 + c] = to_u8(chunk[c] * inv);
			}
			out[px * 4 + 3] = to_u8(a);
		}
		out
	}
}

fn to_u8(v: f32) -> u8 {
	(v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

impl ColorPainter for Painter<'_> {
	fn push_transform(&mut self, transform: Transform) {
		// `A * B` applies B first, so this concatenates below the current matrix.
		let next = self.cur() * transform;
		self.xforms.push(next);
	}

	fn pop_transform(&mut self) {
		self.xforms.pop();
	}

	fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
		let path = self.glyph_path(glyph_id);
		let mask = self.cover(&path);
		self.push_cover(mask);
	}

	fn push_clip_box(&mut self, clip_box: BoundingBox<f32>) {
		let m = self.cur();
		// The box is axis-aligned in paint space but can arrive rotated here, so
		// walk its corners rather than transforming two of them.
		let corners = [
			(clip_box.x_min, clip_box.y_min),
			(clip_box.x_max, clip_box.y_min),
			(clip_box.x_max, clip_box.y_max),
			(clip_box.x_min, clip_box.y_max),
		];
		let mut path = Vec::with_capacity(5);
		for (i, &(x, y)) in corners.iter().enumerate() {
			let (dx, dy) = m.transform(x, y);
			let p = ZPoint::new(dx, dy);
			path.push(if i == 0 {
				Command::MoveTo(p)
			} else {
				Command::LineTo(p)
			});
		}
		path.push(Command::Close);
		let mask = self.cover(&path);
		self.push_cover(mask);
	}

	fn pop_clip(&mut self) {
		self.clips.pop();
	}

	fn fill(&mut self, brush: Brush<'_>) {
		let m = self.cur();
		let shade = Shade::new(self, &brush, m);
		let Self {
			w,
			h,
			layers,
			clips,
			..
		} = self;
		let (w, h) = (*w, *h);
		let Some(dst) = layers.last_mut() else {
			return;
		};
		let clip = clips.last();
		for y in 0..h {
			for x in 0..w {
				let i = y * w + x;
				let cov = match clip {
					Some(mask) => f32::from(mask[i]) / 255.0,
					None => 1.0,
				};
				if cov <= 0.0 {
					continue;
				}
				let Some(color) = shade.at(x as f32 + 0.5, y as f32 + 0.5) else {
					continue;
				};
				let sa = color[3] * cov;
				if sa <= 0.0 {
					continue;
				}
				let inv = 1.0 - sa;
				let base = i * 4;
				for c in 0..3 {
					dst[base + c] = color[c] * sa + dst[base + c] * inv;
				}
				dst[base + 3] = sa + dst[base + 3] * inv;
			}
		}
	}

	fn push_layer(&mut self, _composite_mode: CompositeMode) {
		if self.layers.len() >= MAX_LAYERS {
			self.overflowed = true;
			return;
		}
		self.layers.push(vec![0.0; self.w * self.h * 4]);
	}

	fn pop_layer_with_mode(&mut self, composite_mode: CompositeMode) {
		if self.overflowed {
			// One skipped push, one skipped pop - the next pop is a real one.
			self.overflowed = false;
			return;
		}
		if self.layers.len() < 2 {
			return;
		}
		let src = self.layers.pop().unwrap_or_default();
		let Some(dst) = self.layers.last_mut() else {
			return;
		};
		composite(dst, &src, composite_mode);
	}

	fn paint_cached_color_glyph(
		&mut self,
		_glyph: GlyphId,
	) -> Result<skrifa::color::PaintCachedColorGlyph, PaintError> {
		// No sub-glyph cache here; let skrifa traverse the referenced subgraph.
		Ok(skrifa::color::PaintCachedColorGlyph::Unimplemented)
	}
}

impl Painter<'_> {
	// A glyph outline in device space: the pen applies the current matrix, so
	// zeno rasterizes straight into bitmap coordinates.
	fn glyph_path(&self, gid: GlyphId) -> Vec<Command> {
		let Some(outline) = self.outlines.get(gid) else {
			return Vec::new();
		};
		let mut pen = PathPen {
			m: self.cur(),
			path: Vec::new(),
		};
		let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
		if outline.draw(settings, &mut pen).is_err() {
			return Vec::new();
		}
		pen.path
	}
}

struct PathPen {
	m: Transform,
	path: Vec<Command>,
}

impl PathPen {
	fn map(&self, x: f32, y: f32) -> ZPoint {
		let (dx, dy) = self.m.transform(x, y);
		ZPoint::new(dx, dy)
	}
}

impl OutlinePen for PathPen {
	fn move_to(&mut self, x: f32, y: f32) {
		self.path.push(Command::MoveTo(self.map(x, y)));
	}

	fn line_to(&mut self, x: f32, y: f32) {
		self.path.push(Command::LineTo(self.map(x, y)));
	}

	fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
		let c = self.map(cx0, cy0);
		self.path.push(Command::QuadTo(c, self.map(x, y)));
	}

	fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
		let c0 = self.map(cx0, cy0);
		let c1 = self.map(cx1, cy1);
		self.path.push(Command::CurveTo(c0, c1, self.map(x, y)));
	}

	fn close(&mut self) {
		self.path.push(Command::Close);
	}
}

// A brush, ready to evaluate at a device pixel. Gradient geometry is given in
// paint space, so gradient shades carry the inverse matrix and map each pixel
// back rather than transforming the gradient itself.
enum Shade {
	Solid([f32; 4]),
	Linear {
		inv: Transform,
		p0: (f32, f32),
		d: (f32, f32),
		len2: f32,
		ramp: Vec<[f32; 4]>,
		extend: Extend,
	},
	Radial {
		inv: Transform,
		c0: (f32, f32),
		r0: f32,
		cd: (f32, f32),
		dr: f32,
		ramp: Vec<[f32; 4]>,
		extend: Extend,
	},
	Sweep {
		inv: Transform,
		c0: (f32, f32),
		start: f32,
		sweep: f32,
		ramp: Vec<[f32; 4]>,
		extend: Extend,
	},
	None,
}

impl Shade {
	fn new(painter: &Painter<'_>, brush: &Brush<'_>, m: Transform) -> Self {
		match brush {
			Brush::Solid {
				palette_index,
				alpha,
			} => {
				let mut color = painter.color(*palette_index);
				color[3] *= alpha;
				Shade::Solid(color)
			}
			Brush::LinearGradient {
				p0,
				p1,
				color_stops,
				extend,
			} => {
				let Some(inv) = invert(m) else {
					return Shade::None;
				};
				let d = (p1.x - p0.x, p1.y - p0.y);
				let len2 = d.0 * d.0 + d.1 * d.1;
				if len2 <= f32::EPSILON {
					return Shade::Solid(painter.stop_at(color_stops, 0.0));
				}
				Shade::Linear {
					inv,
					p0: (p0.x, p0.y),
					d,
					len2,
					ramp: painter.ramp(color_stops),
					extend: *extend,
				}
			}
			Brush::RadialGradient {
				c0,
				r0,
				c1,
				r1,
				color_stops,
				extend,
			} => {
				let Some(inv) = invert(m) else {
					return Shade::None;
				};
				Shade::Radial {
					inv,
					c0: (c0.x, c0.y),
					r0: *r0,
					cd: (c1.x - c0.x, c1.y - c0.y),
					dr: r1 - r0,
					ramp: painter.ramp(color_stops),
					extend: *extend,
				}
			}
			Brush::SweepGradient {
				c0,
				start_angle,
				end_angle,
				color_stops,
				extend,
			} => {
				let Some(inv) = invert(m) else {
					return Shade::None;
				};
				let sweep = end_angle - start_angle;
				if sweep.abs() <= f32::EPSILON {
					return Shade::Solid(painter.stop_at(color_stops, 0.0));
				}
				Shade::Sweep {
					inv,
					c0: (c0.x, c0.y),
					start: *start_angle,
					sweep,
					ramp: painter.ramp(color_stops),
					extend: *extend,
				}
			}
		}
	}

	fn at(&self, x: f32, y: f32) -> Option<[f32; 4]> {
		match self {
			Shade::None => None,
			Shade::Solid(color) => Some(*color),
			Shade::Linear {
				inv,
				p0,
				d,
				len2,
				ramp,
				extend,
			} => {
				let (u, v) = inv.transform(x, y);
				let t = ((u - p0.0) * d.0 + (v - p0.1) * d.1) / len2;
				Some(sample(ramp, t, *extend))
			}
			Shade::Radial {
				inv,
				c0,
				r0,
				cd,
				dr,
				ramp,
				extend,
			} => {
				let (u, v) = inv.transform(x, y);
				let t = conical(*c0, *r0, *cd, *dr, u, v)?;
				Some(sample(ramp, t, *extend))
			}
			Shade::Sweep {
				inv,
				c0,
				start,
				sweep,
				ramp,
				extend,
			} => {
				let (u, v) = inv.transform(x, y);
				// skrifa hands back clockwise angles; font space is y-up, so negate.
				let mut deg = -(v - c0.1).atan2(u - c0.0).to_degrees();
				if deg < 0.0 {
					deg += 360.0;
				}
				Some(sample(ramp, (deg - start) / sweep, *extend))
			}
		}
	}
}

// Two-point conical gradient: the largest t whose interpolated circle passes
// through the point AND has a non-negative radius. None where no such circle
// exists (the cone's shadow), which must stay transparent.
fn conical(c0: (f32, f32), r0: f32, cd: (f32, f32), dr: f32, x: f32, y: f32) -> Option<f32> {
	let pd = (x - c0.0, y - c0.1);
	let a = cd.0 * cd.0 + cd.1 * cd.1 - dr * dr;
	let b = pd.0 * cd.0 + pd.1 * cd.1 + r0 * dr;
	let c = pd.0 * pd.0 + pd.1 * pd.1 - r0 * r0;
	if a.abs() < 1e-6 {
		// Equal radii (or a degenerate cone): the quadratic collapses to linear.
		if b.abs() < 1e-9 {
			return None;
		}
		let t = c / (2.0 * b);
		return (r0 + t * dr >= 0.0).then_some(t);
	}
	let disc = b * b - a * c;
	if disc < 0.0 {
		return None;
	}
	let root = disc.sqrt();
	// Prefer the larger t. Which root that is flips with the sign of `a` (the
	// radius growing faster than the center moves), so order them rather than
	// assuming - taking the wrong one paints the far side of the cone.
	let (mut hi, mut lo) = ((b + root) / a, (b - root) / a);
	if hi < lo {
		core::mem::swap(&mut hi, &mut lo);
	}
	if r0 + hi * dr >= 0.0 {
		Some(hi)
	} else if r0 + lo * dr >= 0.0 {
		Some(lo)
	} else {
		None
	}
}

fn sample(ramp: &[[f32; 4]], t: f32, extend: Extend) -> [f32; 4] {
	let t = match extend {
		Extend::Repeat => t.rem_euclid(1.0),
		Extend::Reflect => {
			let m = t.rem_euclid(2.0);
			if m > 1.0 { 2.0 - m } else { m }
		}
		// Pad, and anything a malformed table reports
		_ => t.clamp(0.0, 1.0),
	};
	let idx = ((t * (RAMP_LEN - 1) as f32) as usize).min(RAMP_LEN - 1);
	ramp[idx]
}

fn invert(m: Transform) -> Option<Transform> {
	let det = m.xx * m.yy - m.xy * m.yx;
	if det.abs() < 1e-12 {
		return None;
	}
	let inv = 1.0 / det;
	Some(Transform {
		xx: m.yy * inv,
		yx: -m.yx * inv,
		xy: -m.xy * inv,
		yy: m.xx * inv,
		dx: (m.xy * m.dy - m.yy * m.dx) * inv,
		dy: (m.yx * m.dx - m.xx * m.dy) * inv,
	})
}

// Merge `src` onto `dst`, both premultiplied. Porter-Duff modes reduce to a pair
// of coverage factors; the separable blend modes need the unpremultiplied
// channels, per the W3C compositing model.
fn composite(dst: &mut [f32], src: &[f32], mode: CompositeMode) {
	for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
		let (sa, ba) = (s[3], d[3]);
		if let Some((fa, fb)) = pd_factors(mode, sa, ba) {
			for c in 0..4 {
				d[c] = (s[c] * fa + d[c] * fb).clamp(0.0, 1.0);
			}
			continue;
		}
		let ao = sa + ba - sa * ba;
		for c in 0..3 {
			let cs = if sa > 0.0 { s[c] / sa } else { 0.0 };
			let cb = if ba > 0.0 { d[c] / ba } else { 0.0 };
			let mixed = blend_channel(mode, cb, cs);
			d[c] = ((1.0 - ba) * s[c] + (1.0 - sa) * d[c] + sa * ba * mixed).clamp(0.0, 1.0);
		}
		d[3] = ao;
	}
}

fn pd_factors(mode: CompositeMode, sa: f32, ba: f32) -> Option<(f32, f32)> {
	use CompositeMode as M;
	Some(match mode {
		M::Clear => (0.0, 0.0),
		M::Src => (1.0, 0.0),
		M::Dest => (0.0, 1.0),
		M::SrcOver => (1.0, 1.0 - sa),
		M::DestOver => (1.0 - ba, 1.0),
		M::SrcIn => (ba, 0.0),
		M::DestIn => (0.0, sa),
		M::SrcOut => (1.0 - ba, 0.0),
		M::DestOut => (0.0, 1.0 - sa),
		M::SrcAtop => (ba, 1.0 - sa),
		M::DestAtop => (1.0 - ba, sa),
		M::Xor => (1.0 - ba, 1.0 - sa),
		M::Plus => (1.0, 1.0),
		// HSL modes need all three channels at once; they're vanishingly rare in
		// real fonts, so they use the ordinary over.
		M::HslHue | M::HslSaturation | M::HslColor | M::HslLuminosity | M::Unknown => {
			(1.0, 1.0 - sa)
		}
		_ => return None,
	})
}

fn blend_channel(mode: CompositeMode, cb: f32, cs: f32) -> f32 {
	use CompositeMode as M;
	match mode {
		M::Multiply => cb * cs,
		M::Screen => cb + cs - cb * cs,
		M::Overlay => hard_light(cs, cb),
		M::Darken => cb.min(cs),
		M::Lighten => cb.max(cs),
		M::ColorDodge => {
			if cb <= 0.0 {
				0.0
			} else if cs >= 1.0 {
				1.0
			} else {
				(cb / (1.0 - cs)).min(1.0)
			}
		}
		M::ColorBurn => {
			if cb >= 1.0 {
				1.0
			} else if cs <= 0.0 {
				0.0
			} else {
				1.0 - ((1.0 - cb) / cs).min(1.0)
			}
		}
		M::HardLight => hard_light(cb, cs),
		M::SoftLight => soft_light(cb, cs),
		M::Difference => (cb - cs).abs(),
		M::Exclusion => cb + cs - 2.0 * cb * cs,
		_ => cs,
	}
}

fn hard_light(cb: f32, cs: f32) -> f32 {
	if cs <= 0.5 {
		cb * 2.0 * cs
	} else {
		let s = 2.0 * cs - 1.0;
		cb + s - cb * s
	}
}

fn soft_light(cb: f32, cs: f32) -> f32 {
	if cs <= 0.5 {
		cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
	} else {
		let d = if cb <= 0.25 {
			((16.0 * cb - 12.0) * cb + 4.0) * cb
		} else {
			cb.sqrt()
		};
		cb + (2.0 * cs - 1.0) * (d - cb)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// A prompt's check mark went to the emoji face and came out purple, ignoring
	// the color it was set in. These are the ones that must stay text.
	// Test ID: EolvnsW
	#[test]
	fn a_dingbat_is_not_an_emoji() {
		for ch in [
			'\u{2714}', '\u{2718}', '\u{2713}', '\u{2717}', '\u{00a9}', '\u{00ae}', '\u{2122}',
		] {
			assert!(
				!wants_color(ch),
				"{ch:?} should be set in the terminal font"
			);
		}
		for ch in [
			'\u{2705}',
			'\u{274c}',
			'\u{2728}',
			'\u{2b50}',
			'\u{1f600}',
			'\u{1f1fa}',
		] {
			assert!(wants_color(ch), "{ch:?} should be painted");
		}
	}

	fn stamped(cg: &mut ColorGlyphs, count: u16, frame: u64) {
		for i in 0..count {
			cg.rasters.insert((i, 20, 20), (frame, vec![0u8; 4]));
		}
	}

	// A screen of nothing but distinct emoji overflows the cache inside a single
	// frame. Anything warmed for that frame has to survive: glyphon panics if the
	// rasterize callback answers None where it previously answered Some, which is
	// exactly what a wholesale clear caused.
	// Test ID: ElKWljk
	#[test]
	fn overflow_keeps_every_raster_the_current_frame_warmed() {
		let mut cg = ColorGlyphs::new();
		cg.begin_frame();
		let want = MAX_RASTERS as u16 + 64;
		let now = cg.frame;
		stamped(&mut cg, want, now);
		cg.sweep();
		assert_eq!(cg.rasters.len(), want as usize);
	}

	// The sweep can drop a key glyphon is still holding, and glyphon aborts on a
	// glyph that stops answering. So a miss answers with pixels nobody can see.
	// Test ID: EpHSLii
	#[test]
	fn a_missing_raster_answers_with_transparent_pixels() {
		let cg = ColorGlyphs::new();
		let req = RasterizeCustomGlyphRequest {
			id: 7,
			width: 4,
			height: 3,
			x_bin: glyphon::cosmic_text::SubpixelBin::Zero,
			y_bin: glyphon::cosmic_text::SubpixelBin::Zero,
			scale: 1.0,
		};
		let got = cg.raster(req).expect("a miss still answers");
		assert_eq!(got.data.len(), 4 * 3 * 4);
		assert!(got.data.iter().all(|b| *b == 0), "and nothing shows");
	}

	// The cache must still shed genuinely dead entries, or a long session at a
	// changing font size would grow without limit.
	// Test ID: ElKWljl
	#[test]
	fn overflow_drops_rasters_no_recent_frame_touched() {
		let mut cg = ColorGlyphs::new();
		stamped(&mut cg, MAX_RASTERS as u16, 1);
		cg.frame = 1 + RASTER_PIN_FRAMES + 1;
		cg.sweep();
		assert!(cg.rasters.is_empty());
	}

	// An emoji that stays on screen is warmed every frame but inserted only once;
	// the hit path has to re-stamp it or it ages out from under the atlas.
	// Test ID: ElKWljm
	#[test]
	fn a_cache_hit_repins_the_raster() {
		let mut cg = ColorGlyphs::new();
		stamped(&mut cg, 1, 1);
		cg.frame = 50;
		// Same path warm() takes on a hit.
		if let Some(entry) = cg.rasters.get_mut(&(0, 20, 20)) {
			entry.0 = cg.frame;
		}
		cg.sweep();
		assert_eq!(cg.rasters.get(&(0, 20, 20)).map(|e| e.0), Some(50));
	}

	// The whole point of the module: a COLRv1-only emoji font must produce actual
	// color pixels, where swash hands back an empty image. Skipped where the box
	// has no color font at all.
	// Test ID: ElDEZZo
	#[test]
	fn colr_v1_emoji_rasterizes_in_colour() {
		let mut db = fontdb::Database::new();
		db.load_system_fonts();
		let mut glyphs = ColorGlyphs::new();
		let Some(metrics) = glyphs.metrics(&db, '\u{1F600}') else {
			eprintln!("no color glyph for U+1F600 on this box; skipping");
			return;
		};
		glyphs.warm(&db, metrics.id, 48, 48);
		let raster = glyphs
			.raster(RasterizeCustomGlyphRequest {
				id: metrics.id,
				width: 48,
				height: 48,
				x_bin: glyphon::cosmic_text::SubpixelBin::Zero,
				y_bin: glyphon::cosmic_text::SubpixelBin::Zero,
				scale: 1.0,
			})
			.expect("no raster produced");
		assert_eq!(raster.content_type, ContentType::Color);
		assert_eq!(raster.data.len(), 48 * 48 * 4);
		let opaque = raster.data.chunks_exact(4).filter(|p| p[3] > 128).count();
		assert!(
			opaque > 200,
			"only {opaque} solid pixels - glyph came out blank"
		);
		// "Color" means the channels actually differ somewhere; a monochrome
		// outline would pass the coverage check above on its own.
		let tinted = raster
			.data
			.chunks_exact(4)
			.filter(|p| p[3] > 128 && p[0].abs_diff(p[2]) > 24)
			.count();
		assert!(
			tinted > 100,
			"only {tinted} colored pixels - looks monochrome"
		);
	}

	// Porter-Duff SrcIn keeps only the part of the source covered by the backdrop,
	// and Noto Color Emoji leans on it 300+ times per font.
	// Test ID: ElDEZZp
	#[test]
	fn src_in_masks_the_source_to_the_backdrop() {
		let mut dst = vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.2];
		let src = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
		composite(&mut dst, &src, CompositeMode::SrcIn);
		assert_eq!(dst[3], 0.0, "no backdrop -> nothing survives");
		assert!(
			(dst[7] - 0.2).abs() < 1e-6,
			"backdrop alpha gates the source"
		);
	}

	// Extend modes drive what happens past the ends of a color line.
	// Test ID: ElDEZZq
	#[test]
	fn gradient_extend_modes_wrap_clamp_and_mirror() {
		let ramp: Vec<[f32; 4]> = (0..RAMP_LEN)
			.map(|i| {
				let v = i as f32 / (RAMP_LEN - 1) as f32;
				[v, v, v, 1.0]
			})
			.collect();
		assert!(sample(&ramp, -0.5, Extend::Pad)[0] < 0.01);
		assert!(sample(&ramp, 1.5, Extend::Pad)[0] > 0.99);
		assert!((sample(&ramp, 1.25, Extend::Repeat)[0] - 0.25).abs() < 0.02);
		assert!((sample(&ramp, 1.25, Extend::Reflect)[0] - 0.75).abs() < 0.02);
	}

	// A radial gradient's point must sit on a circle with a non-negative radius,
	// picking the LARGEST such t; points with no such circle stay transparent.
	// Test ID: ElDEZZr
	#[test]
	fn conical_picks_the_largest_valid_circle() {
		// Concentric, r 0 -> 1: the halfway point sits at t = 0.5.
		let t = conical((0.0, 0.0), 0.0, (0.0, 0.0), 1.0, 0.5, 0.0).expect("inside");
		assert!((t - 0.5).abs() < 1e-5, "t = {t}");
		// Two degenerate point-circles on the x axis: nothing off that axis is on
		// any interpolated circle.
		assert!(conical((0.0, 0.0), 0.0, (10.0, 0.0), 0.0, 0.0, 5.0).is_none());
		// Shrinking radius with a fixed center puts `a` negative, which flips which
		// root is the larger one. The larger (t = 6) needs radius -5, so the valid
		// answer is the backward extension at t = -4 (radius 5) - not None.
		let t = conical((0.0, 0.0), 1.0, (0.0, 0.0), -1.0, 5.0, 0.0).expect("on a circle");
		assert!((t + 4.0).abs() < 1e-5, "t = {t}");
	}
}
