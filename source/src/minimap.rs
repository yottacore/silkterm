// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Minimap: the whole scroll buffer in miniature, in its own column beside the
//! text. The buffer always maps linearly onto the column and never slides. See
//! the minimap design doc under `project/design_docs`.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

use crate::config;
use crate::palette;
use crate::pane::Rect;

// Narrowest column worth taking from the text, in DIP.
const MIN_W: f32 = 16.0;
// Shortest the viewport handle may draw, so a deep buffer still leaves
// something to grab.
const MIN_HANDLE: f32 = 14.0;
// Tallest one buffer line draws. A short buffer stops short of the column's
// bottom rather than stretching to fill it.
const MAX_LINE_PX: f32 = 1.5;
// What the densest glyph contributes to its pixel. Every other character gets
// a share of it, per `ink_share`, and that variation is what keeps a run of
// text from reading as a slab - so this no longer has to hold back to do it.
const INK: f32 = 1.0;
// A text line does not fill its own height, and the gap above and below is
// what keeps a page of text from reading as one block. This is the ink's share
// of the line at the tallest a line ever draws.
const BAND: f32 = 0.5;
// How far down the line the ink starts, at that same tallest.
const BAND_TOP: f32 = 0.1;
// Line heights the band ramps between. Under the first there is no room for a
// gap and the line is taken whole; over the second it gets the full BAND. The
// pair sits below one pixel so a line near the cap lands across two pixel rows
// at partial coverage rather than filling one.
const BAND_FLOOR: f32 = 0.6;
const BAND_FULL: f32 = 1.2;
// A pixel row's ink is what actually fell in it, so mostly blank lines read
// dimmer than a solid page. One line among many still has to be findable, so
// it never falls below this share of its own strength.
const LONE: f32 = 0.45;
// Preview opacity. The column sits over the pane background (and the wallpaper
// through it), so the miniature stays a hint rather than a second screen.
const PREVIEW_A: f32 = 0.72;
// Slowest the preview is allowed to recompose. Under a flood the buffer shifts
// every frame and every pixel of the map moves with it, so this is what keeps
// a feature that is only a hint from costing what the text costs.
const COMPOSE_MS: u64 = 90;
// A compose holds the term lock the PTY reader waits on while it rasterizes,
// and over a deep scrollback the pixel work runs to a million lines, on a
// thread of its own. So the next one waits this many times as long as the
// last one took, both parts, which keeps the map to about a twentieth of a
// core however deep the buffer is.
const COMPOSE_SHARE: u32 = 20;
// A cache that has fallen behind the grid is rebuilt whole, at most this often.
const RESYNC_MS: u64 = 400;
// Cells one compose may rasterize from history, about 4 ms here. A deep
// scrollback under a flood, or rebuilt whole, is more than a compose can read
// without stopping the terminal: 100,000 lines took 150 ms. Past this, one
// line stands in for its neighbors until later composes have the time.
const RASTER_CELLS: usize = 300_000;

// Weights under this are dropped from a cell's span list. The tails of a tent
// cost as much to walk as its middle and change nothing.
const SPAN_MIN: f32 = 0.01;

// Rasterized buffer line: one RGBA byte group per preview pixel, straight
// (not premultiplied) - the shader premultiplies in linear light.
type Row = Vec<u8>;

// How much of a unit tent centered on `at` falls between x0 and x1. The tent
// has area 1, so a run of cells that tiles the line hands each pixel a total
// weight of 1.
fn tent_over(x0: f32, x1: f32, at: f32) -> f32 {
	// integral of max(0, 1 - |u|) from -inf to u
	let upto = |x: f32| {
		let u = x - at;
		if u <= -1.0 {
			0.0
		} else if u <= 0.0 {
			(u + 1.0) * (u + 1.0) * 0.5
		} else if u < 1.0 {
			0.5 + u - u * u * 0.5
		} else {
			1.0
		}
	};
	upto(x1) - upto(x0)
}

/// The column's pieces for one pane, in absolute window px. `handle` is the
/// viewport marker; None on the alt screen, where there is nothing to scroll.
#[derive(Clone, Copy, Debug)]
pub struct Geom {
	pub preview: Rect,
	pub handle: Option<Rect>,
}

/// Should this pane show the column at all? A full-screen program draws on the
/// alt screen, which has no scroll buffer behind it, so the map would be a
/// rectangle at the top and the room is better spent on text. Some programs do
/// their own scrolling in a way the map can still follow, and the setting names
/// those. A program with nothing to compare against (Windows cannot always say
/// what is running) is treated as not listed.
pub fn wanted(cfg: &config::Settings, alt_screen: bool, program: Option<&str>) -> bool {
	if !alt_screen {
		return true;
	}
	let Some(program) = program else {
		return false;
	};
	let running = trim_exe(program);
	cfg.minimap_tui_whitelist
		.split_whitespace()
		.any(|name| trim_exe(name).eq_ignore_ascii_case(running))
}

// The name to compare on: no directory, and no .exe, so one list works on both
// platforms and a user who writes either spelling is understood.
fn trim_exe(name: &str) -> &str {
	let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
	base.strip_suffix(".exe")
		.or_else(|| base.strip_suffix(".EXE"))
		.unwrap_or(base)
}

/// Width of the column, 0 when the minimap is off, the pane is too narrow to
/// give up the room, or a full-screen program has it.
pub fn column_w(cfg: &config::Settings, pane_w: f32, scale: f32, wanted: bool) -> f32 {
	if !cfg.minimap || !wanted {
		return 0.0;
	}
	let w = config::dip(cfg.minimap_width, scale).min((pane_w * 0.5).floor());
	if w < config::dip(MIN_W, scale) {
		0.0
	} else {
		w
	}
}

/// The part of a pane's area the terminal text gets.
pub fn text_rect(full: Rect, cfg: &config::Settings, scale: f32, wanted: bool) -> Rect {
	Rect {
		w: (full.w - column_w(cfg, full.w, scale, wanted)).max(0.0),
		..full
	}
}

// How tall one buffer line draws. Capped, so a buffer shorter than the column
// simply does not reach the bottom of it. The cap is scaled but deliberately
// not rounded to whole pixels: a line has to be able to sit at a fraction of
// one, or its ink band lands inside a single pixel row and a page of text
// goes back to reading as a slab.
fn line_px(track_h: f32, total: usize, scale: f32) -> f32 {
	if total == 0 {
		return 0.0;
	}
	(track_h / total as f32).min(MAX_LINE_PX * scale)
}

// Where the viewport marker sits, as (y offset down the track, height). `pos`
// is the scroll model's lines-back-from-the-bottom, the same number the
// scrollbar rides. Both the height and the offset are measured at the map's
// own pitch, so the marker covers the lines the image draws under it. The
// image stops at `shown`, since under a flood the eased text has not reached
// the rest, and measuring the marker against the whole buffer instead left it
// drifting above the lines it stood for (F148).
fn handle_span(
	track_h: f32,
	total: usize,
	shown: usize,
	rows: usize,
	pos: f32,
	scale: f32,
) -> (f32, f32) {
	let pitch = line_px(track_h, shown, scale);
	let used = pitch * shown as f32;
	let covers = rows as f32 * pitch; // what the viewport really takes up
	let h = covers.max(config::dip(MIN_HANDLE, scale)).min(used);
	// A deep buffer puts the viewport under the height floor. A marker taller
	// than the lines it stands for grows both ways from their middle, so it
	// still reads as pointing at them.
	let first = total.saturating_sub(rows) as f32 - pos;
	let y = first * pitch + (covers - h) * 0.5;
	(y.clamp(0.0, (used - h).max(0.0)), h)
}

// Lines to one preview pixel.
fn lines_per_px(track_h: f32, shown: usize, scale: f32) -> f32 {
	let pitch = line_px(track_h, shown, scale);
	if pitch > 0.0 { 1.0 / pitch } else { 0.0 }
}

/// The column's geometry for a pane. `pos` rides the eased scroll position;
/// `alt` drops the marker, and `on` is whether the pane is showing the column
/// at all (see `wanted`).
pub fn geom(
	full: Rect,
	margin: f32,
	scale: f32,
	cfg: &config::Settings,
	total: usize,
	shown: usize,
	rows: usize,
	pos: f32,
	alt: bool,
	on: bool,
) -> Option<Geom> {
	let w = column_w(cfg, full.w, scale, on);
	let h = (full.h - 2.0 * margin).max(0.0);
	if w <= 0.0 || h <= 0.0 {
		return None;
	}
	let preview = Rect {
		x: full.x + full.w - w,
		y: full.y + margin,
		w,
		h,
	};
	let handle = (!alt && total > rows).then(|| {
		let (y, hh) = handle_span(h, total, shown, rows, pos, scale);
		Rect {
			x: preview.x,
			y: preview.y + y,
			w,
			h: hh,
		}
	});
	Some(Geom { preview, handle })
}

/// Where a press in the column fell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
	Handle,
	Track,
}

/// Where a press at (x, y) fell, if it hit the column at all.
pub fn hit(g: &Geom, x: f32, y: f32) -> Option<Hit> {
	if !g.preview.contains(x, y) {
		return None;
	}
	let handle = g.handle?;
	Some(if y >= handle.y && y < handle.y + handle.h {
		Hit::Handle
	} else {
		Hit::Track
	})
}

/// The scroll position a click at `y` should center the viewport on. The bottom
/// of the map stands for the newest output as well as the last line it drew, so
/// a click there means the bottom of the buffer rather than the line the trim
/// stopped on. That is also what makes both ends of the track reachable where
/// the marker is shorter than the viewport it stands for.
pub fn center_on(g: &Geom, total: usize, shown: usize, rows: usize, y: f32, scale: f32) -> f32 {
	let per_px = lines_per_px(g.preview.h, shown, scale);
	if per_px <= 0.0 {
		return 0.0;
	}
	let back = total.saturating_sub(rows) as f32;
	let first = (y - g.preview.y) * per_px - rows as f32 * 0.5;
	let pos = back - first;
	if pos <= total.saturating_sub(shown) as f32 {
		0.0
	} else {
		pos.min(back)
	}
}

/// Drag: the pointer has moved from where it grabbed the marker, so the view
/// moves the matching number of lines from where it sat then. `grab` is that
/// pointer y and that position. Reading the marker's drawn top back instead
/// cannot work, because the height floor makes the marker taller than the lines
/// it covers on a deep buffer, so its top is a rounded reading of the position
/// rather than the position itself.
pub fn drag_to(
	g: &Geom,
	total: usize,
	shown: usize,
	rows: usize,
	grab: (f32, f32),
	y: f32,
	scale: f32,
) -> f32 {
	let (from_y, from_pos) = grab;
	let moved = (y - from_y) * lines_per_px(g.preview.h, shown, scale);
	(from_pos - moved).clamp(0.0, total.saturating_sub(rows) as f32)
}

// Per-pane cache

/// A pane's rasterized buffer plus the image composed from it. History lines
/// never change, so each one rasterizes once, at the first compose after it
/// scrolls off; the live screen rows are redone at each compose.
///
/// Rasterizing reads the grid, so it runs here under the term lock, and the
/// budget bounds it. The rows and the pixel work live in `store`, and a whole
/// compose over a deep scrollback takes the store to a thread of its own,
/// since at a million lines it is most of a second. While the store is away
/// the builds only count, and what they owe it goes along with the next one.
#[derive(Default)]
pub struct Minimap {
	store: Store,
	job: Option<Job>,
	// when to look for that thread's answer again
	poll_at: Option<Instant>,
	took: Duration, // how long the last compose on its own thread ran
	// One per history row the store has: a stand-in copied from a neighbor,
	// not rasterized from its own line yet. `rough_n` counts them.
	rough: VecDeque<bool>,
	rough_n: usize,
	// No stand-ins from here up, so the search for them starts here.
	clean_from: usize,
	// Edits owed to the store: drop every row, or this many off the top.
	owed_clear: bool,
	owed_front: usize,
	// History moved or was rebuilt since the last compose, so the next one
	// redoes the whole image rather than the rows it rasterized.
	moved: bool,
	spare: Vec<Row>,
	hist: usize, // history lines the cache accounts for
	// Buffer lines the map actually draws: the whole buffer less the lines the
	// eased text has not reached yet. Under a flood the view sits behind the
	// newest output, and drawing past it would put lines in the column that
	// are not on screen. 0 until the first compose, which is what
	// `shown_lines` falls back on.
	shown: usize,
	// The newest `fresh` of those are not rasterized yet. A build only counts
	// them and the compose does the work, because under a flood most lines
	// leave history before any compose shows them, and the build holds the
	// term lock the PTY reader waits on.
	fresh: usize,
	width: usize,
	cols: usize,
	lines: usize,
	// composed image, `img_w` px wide by `img_h` tall, straight RGBA. Its own
	// width, not `width`, which the next compose will use: the two differ from
	// the moment the column is resized until that compose, and a caller that
	// believed `width` uploaded a short buffer as a wider texture.
	img: Vec<u8>,
	img_w: usize,
	img_h: usize,
	// the lines and scale it was composed over, so a compose that moved
	// nothing knows it may redo only part of it
	img_total: usize,
	img_scale: f32,
	pub rev: u64, // bumped on every compose, so the renderer can skip re-uploads
	// rows changed since the last compose, and the compose was throttled out -
	// the pane reports this as animation so the frame after picks it up
	pending: bool,
	last_compose: Option<Instant>,
	spent: Duration, // how long the last compose took
	stale_since: Option<Instant>,
	tail: u64, // fingerprint of the newest history line
	acc: Acc,
	// which preview pixels each column covers and by how much, flattened, with
	// `span_at[c]..span_at[c + 1]` for column c; made for `spans_for`
	spans: Vec<(usize, f32)>,
	span_at: Vec<usize>,
	spans_for: (usize, usize),
	#[cfg(test)]
	rastered: usize,
	#[cfg(test)]
	budget: Option<usize>,
	#[cfg(test)]
	here_px: Option<usize>,
}

// The store holds a raster for every history row, so leave it out.
impl std::fmt::Debug for Minimap {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Minimap")
			.field("hist", &self.hist)
			.field("shown", &self.shown)
			.finish_non_exhaustive()
	}
}

// The rows, and what composing them needs, in one piece that can go to
// another thread and come back.
#[derive(Default)]
struct Store {
	rows: VecDeque<Row>,
	screen: usize, // the screen rows at the end of `rows`
	spare: Vec<Row>,
	acc: Acc,
	img: Vec<u8>, // always the newest image; `Minimap::img` is a copy
}

// A compose that has the store on its own thread.
struct Job {
	thread: std::thread::JoinHandle<(Store, Duration)>,
	plan: Plan,
	here: Duration, // what building the plan cost this thread
	whole: bool,
}

// One compose: the edits the builds since the last one owe the store, the
// rows rasterized for it, and the image to make.
#[derive(Default)]
struct Plan {
	clear: bool,
	front: usize,
	// a drawn line and how many lines it stands for, itself included, oldest
	// first
	groups: Vec<(Row, usize)>,
	refined: Vec<(usize, Row)>,
	screen: Vec<Row>,
	// redo the whole image, or only the rows `refined` and `screen` touch
	full: bool,
	width: usize,
	img_h: usize,
	total: usize,
	scale: f32,
}

impl Plan {
	// Rows times pixels a compose on this thread may take, about 4 ms here.
	// Over it, the store goes to a thread of its own.
	const HERE_PX: usize = 2_000_000;

	fn heavy(&self, limit: usize) -> bool {
		self.full && self.total * self.width > limit
	}

	// The plan without its rows, which is all that is needed to show the
	// image it made.
	fn numbers(&self) -> Plan {
		Plan {
			clear: self.clear,
			front: self.front,
			full: self.full,
			width: self.width,
			img_h: self.img_h,
			total: self.total,
			scale: self.scale,
			..Default::default()
		}
	}
}

// Per-dest-row accumulators, kept so a compose allocates nothing.
#[derive(Default)]
struct Acc {
	rgb: Vec<f32>,
	weight: Vec<f32>,
	alpha: Vec<f32>,
	// Lines that fall wholly inside the pixel row, added up as integers per
	// color plane: red, green and blue times alpha, then alpha. `peak` is the
	// strongest alpha. Flushed into the three above.
	sum: Vec<u32>,
	peak: Vec<u32>,
}

// Whole lines added up before a flush, so `Acc::sum` cannot overflow.
const WHOLE_MAX: usize = 65_000;

// How often to look again for a compose that has run past what the last one
// took.
const POLL_MS: u64 = 8;

// Replaced rows this close together are redone as one run.
const RUN_GAP: usize = 64;

impl Acc {
	fn heap_bytes(&self) -> usize {
		(self.rgb.capacity() + self.weight.capacity() + self.alpha.capacity()) * 4
			+ (self.sum.capacity() + self.peak.capacity()) * 4
	}
}

impl Store {
	fn heap_bytes(&self) -> usize {
		let row_head = std::mem::size_of::<Row>();
		let rows: usize = self.rows.iter().chain(&self.spare).map(Vec::capacity).sum();
		rows + (self.rows.capacity() + self.spare.capacity()) * row_head
			+ self.img.capacity()
			+ self.acc.heap_bytes()
	}
}

impl Minimap {
	/// What the cache holds on the heap, for `SILK_MEMDBG`. A store that is away on
	/// a compose thread counts as empty until it comes back.
	pub fn heap_bytes(&self) -> usize {
		let row_head = std::mem::size_of::<Row>();
		let spare: usize = self.spare.iter().map(Vec::capacity).sum();
		self.store.heap_bytes()
			+ spare + self.spare.capacity() * row_head
			+ self.img.capacity()
			+ self.acc.heap_bytes()
			+ self.rough.capacity()
			+ self.spans.capacity() * std::mem::size_of::<(usize, f32)>()
			+ self.span_at.capacity() * std::mem::size_of::<usize>()
	}

	pub fn image(&self) -> (&[u8], usize, usize) {
		(&self.img, self.img_w, self.img_h)
	}

	/// How many buffer lines the column maps, for the callers that place the
	/// marker and turn a click back into a scroll position. It is what the last
	/// compose drew, not what the ease is doing right now, or the marker would
	/// be measured against a picture nobody composed. Before the first compose
	/// there is nothing to ask, so it is the whole buffer.
	pub fn shown_lines(&self, hist: usize, lines: usize) -> usize {
		if self.shown == 0 {
			hist + lines
		} else {
			self.shown
		}
	}

	/// Free everything. Called when the column goes away. A compose still on
	/// its thread finishes there and is dropped.
	pub fn clear(&mut self) {
		*self = Self::default();
	}

	/// Fold this build's grid into the cache and recompose if it is time.
	/// `advanced` is the count of lines that entered history since the last
	/// build - the same number the output ease rides. `lag` is how far behind
	/// the newest output the eased view still sits, in whole lines, which is
	/// where the map has to stop, and `draining` is whether that lag is still
	/// falling.
	#[allow(clippy::too_many_arguments)]
	pub fn update(
		&mut self,
		grid: &Grid<Cell>,
		colors: &Colors,
		cfg: &config::Settings,
		width: usize,
		img_h: usize,
		scale: f32,
		lines: usize,
		cols: usize,
		advanced: usize,
		lag: usize,
		draining: bool,
		cut: bool,
		now: Instant,
	) {
		if width == 0 || img_h == 0 || lines == 0 || cols == 0 {
			return;
		}
		let collected = self.collect(now);
		let hist = grid.history_size();
		let mut rebuild = cut
			|| self.width != width
			|| self.cols != cols
			|| self.lines != lines
			|| advanced > hist
			|| self.hist + advanced < hist;
		// Scrolled back with a full scrollback: nothing reports the push count, so
		// a changed newest-history line is the only sign the cache has fallen
		// behind. Rebuilding is the whole cache, so it waits out RESYNC_MS.
		let tail = if hist > 0 {
			row_hash(grid, Line(-1), cols)
		} else {
			0
		};
		if !rebuild && advanced == 0 && tail != self.tail {
			let since = *self.stale_since.get_or_insert(now);
			rebuild = now.duration_since(since) >= Duration::from_millis(RESYNC_MS);
		}

		self.width = width;
		self.cols = cols;
		self.lines = lines;
		self.moved |= rebuild || advanced > 0;
		if rebuild {
			self.owed_clear = true;
			self.owed_front = 0;
			self.rough.clear();
			self.rough_n = 0;
			self.hist = hist;
			self.fresh = hist;
		} else {
			self.hist += advanced;
			self.fresh += advanced;
			// the oldest lines leave first, and those are the rasterized ones
			while self.hist > hist {
				if self.hist > self.fresh {
					self.owed_front += 1;
					self.clean_from = self.clean_from.saturating_sub(1);
					if self.rough.pop_front() == Some(true) {
						self.rough_n -= 1;
					}
				} else {
					self.fresh -= 1;
				}
				self.hist -= 1;
			}
		}
		self.tail = tail;
		self.stale_since = None;

		if self.job.is_some() {
			self.pending = true;
			return;
		}
		let due = self
			.last_compose
			.is_none_or(|t| now.duration_since(t) >= gap(self.spent));
		// a resized column composes at once: the map is stretched to the new
		// width until it does, and at a deep scrollback the wait is seconds
		if due || self.img_h != img_h || self.img_w != width {
			self.last_compose = Some(now);
			// A whole redraw, after a screen swap, a resize or a resync, costs
			// far more than the ordinary ones after it. Twenty times its cost
			// left the map standing still for seconds, so the wait follows the
			// ordinary cost.
			let whole = self.fresh >= self.hist;
			let began = Instant::now();
			let mut plan = self.plan(grid, colors, cfg, lag, img_h, scale);
			#[cfg(test)]
			let limit = self.here_px.unwrap_or(Plan::HERE_PX);
			#[cfg(not(test))]
			let limit = Plan::HERE_PX;
			if plan.heavy(limit) {
				self.send(plan, began.elapsed(), whole, now);
				self.pending = true;
				return;
			}
			self.store.run(&mut plan);
			self.publish(&plan);
			if !whole {
				self.spent = began.elapsed();
			}
		} else if !collected {
			self.pending = true;
			return;
		}
		// Short of the bottom AND the lag is still falling, so another
		// compose is owed even if no more output arrives: the ease drains
		// on its own and the map has to follow it down. `pending` drives
		// the build gate and the timed wake, so this is the whole
		// mechanism - and it is why the drain half matters. A view parked
		// in the scrollback freezes the lag, and owing a compose there
		// asked for a frame and a full recompose several times a second
		// for as long as the pane sat there (F155). Stand-ins left to
		// replace owe one too, and that runs out.
		self.pending =
			(draining && self.shown < self.hist + self.lines) || self.rough_n > 0 || self.moved;
	}

	/// A compose is owed. The build gate reads this so the next pass pays it,
	/// rather than leaving the map a step behind once output stops.
	pub fn pending(&self) -> bool {
		self.pending
	}

	/// When that compose comes due. A timed wake rather than an animation flag:
	/// marking the window animating would bring it straight back, find the
	/// throttle still closed, and spin at the frame rate. The same goes for a
	/// compose on its own thread, which is looked for when it should be done.
	pub fn wake(&self) -> Option<Instant> {
		if self.job.is_some() {
			return self.poll_at;
		}
		let at = self.last_compose?;
		self.pending.then(|| at + gap(self.spent))
	}

	// Hand the store to a thread for the whole compose. If no thread can be
	// had, the cache starts over.
	fn send(&mut self, mut plan: Plan, here: Duration, whole: bool, now: Instant) {
		let mut store = std::mem::take(&mut self.store);
		let numbers = plan.numbers();
		let spawned = std::thread::Builder::new()
			.name("minimap".into())
			.spawn(move || {
				let began = Instant::now();
				store.run(&mut plan);
				(store, began.elapsed())
			});
		match spawned {
			Ok(thread) => {
				self.poll_at = Some(now + self.took.max(Duration::from_millis(POLL_MS)));
				self.job = Some(Job {
					thread,
					plan: numbers,
					here,
					whole,
				});
			}
			// the closure owned the store, and a failed spawn drops it
			Err(_) => self.lost(),
		}
	}

	// Take the store back from a compose that has finished on its thread.
	// Answers whether one did.
	fn collect(&mut self, now: Instant) -> bool {
		let Some(job) = self.job.take_if(|job| job.thread.is_finished()) else {
			if self.job.is_some() {
				self.poll_at = Some(now + Duration::from_millis(POLL_MS));
			}
			return false;
		};
		let Ok((store, took)) = job.thread.join() else {
			self.lost();
			return false;
		};
		self.store = store;
		self.took = took;
		self.poll_at = None;
		self.publish(&job.plan);
		if !job.whole {
			self.spent = job.here + took;
		}
		true
	}

	// The store did not come back, so the next build rebuilds the cache.
	fn lost(&mut self) {
		self.store = Store::default();
		self.job = None;
		self.poll_at = None;
		self.width = 0;
		self.pending = true;
	}

	// Show what the store just composed.
	fn publish(&mut self, plan: &Plan) {
		self.img.clone_from(&self.store.img);
		self.img_w = plan.width;
		self.img_h = plan.img_h;
		self.img_total = plan.total;
		self.img_scale = plan.scale;
		self.shown = plan.total;
		self.rev = self.rev.wrapping_add(1);
		// the store gets back more rows than it hands out, and the raster here
		// wants some for the next compose
		let want = (self.budget() + self.lines).saturating_sub(self.spare.len());
		let from = self.store.spare.len().saturating_sub(want);
		self.spare.extend(self.store.spare.drain(from..));
	}

	// History lines one compose may rasterize.
	fn budget(&self) -> usize {
		#[cfg(test)]
		if let Some(lines) = self.budget {
			return lines;
		}
		(RASTER_CELLS / self.cols.max(1)).max(1)
	}

	// Rasterize the history lines the builds only counted, then the screen.
	// Bounded by the budget however deep the history is and however much
	// output went by: past it, every `step`th line is drawn and stands in for
	// the rows around it. What is left of the budget replaces stand-ins,
	// newest first, since the oldest are the first to leave. Also settles how
	// far down the buffer the map draws.
	fn plan(
		&mut self,
		grid: &Grid<Cell>,
		colors: &Colors,
		cfg: &config::Settings,
		lag: usize,
		img_h: usize,
		scale: f32,
	) -> Plan {
		self.fit_spans();
		let mut readable = palette::Readable::default();
		let mut plan = Plan {
			clear: std::mem::take(&mut self.owed_clear),
			front: std::mem::take(&mut self.owed_front),
			..Default::default()
		};
		let mut left = self.budget();
		let fresh = std::mem::take(&mut self.fresh);
		let step = fresh.div_ceil(left).max(1);
		let mut k = fresh;
		while k > 0 {
			// lines k (the oldest) down to k - group + 1, drawn from the newest
			let group = step.min(k);
			let mut row = self.take_row();
			let line = Line(-((k - group + 1) as i32));
			self.fill(grid, line, colors, cfg, &mut readable, &mut row);
			if group > 1 {
				self.rough.extend(std::iter::repeat_n(true, group - 1));
				self.rough_n += group - 1;
				self.clean_from = usize::MAX;
			}
			self.rough.push_back(false);
			plan.groups.push((row, group));
			left = left.saturating_sub(1);
			k -= group;
		}
		let mut i = self.clean_from.min(self.hist);
		while self.rough_n > 0 && left > 0 && i > 0 {
			i -= 1;
			if !self.rough[i] {
				continue;
			}
			let mut row = self.take_row();
			let line = Line(i as i32 - self.hist as i32);
			self.fill(grid, line, colors, cfg, &mut readable, &mut row);
			plan.refined.push((i, row));
			self.rough[i] = false;
			self.rough_n -= 1;
			left -= 1;
		}
		self.clean_from = i;
		for line in 0..self.lines as i32 {
			let mut row = self.take_row();
			self.fill(grid, Line(line), colors, cfg, &mut readable, &mut row);
			plan.screen.push(row);
		}
		// The rows are all rasterized either way, since the ease drains and the
		// map has to reach them without re-reading the grid. One line is kept
		// whatever the lag, so the column never disappears mid-flood.
		plan.total = (self.hist + self.lines).saturating_sub(lag).max(1);
		plan.width = self.width;
		plan.img_h = img_h;
		plan.scale = scale;
		plan.full = self.moved
			|| self.img_w != plan.width
			|| self.img_h != img_h
			|| self.img_total != plan.total
			|| self.img_scale != scale;
		self.moved = false;
		plan
	}

	// The same for every line, so worked out once per width and column count
	// rather than per cell.
	fn fit_spans(&mut self) {
		if self.spans_for == (self.width, self.cols) {
			return;
		}
		self.spans_for = (self.width, self.cols);
		self.spans.clear();
		self.span_at.clear();
		let per_cell = self.width as f32 / self.cols as f32;
		let edge = self.width as f32 + 2.0;
		for c in 0..self.cols {
			self.span_at.push(self.spans.len());
			// A cell spreads over a tent a pixel wide each side, not over the one
			// pixel it happens to fall in. At the ratios a column runs at, that
			// pixel alternates between cells as you go along the line, and the
			// cell grid beats against the pixel grid into a comb that is not in
			// the text. The end cells reach past the edge so the first and last
			// pixel are covered as fully as the rest.
			let x0 = if c == 0 { -2.0 } else { c as f32 * per_cell };
			let x1 = if c + 1 == self.cols {
				edge
			} else {
				(c + 1) as f32 * per_cell
			};
			let first = (x0 - 1.0).max(0.0) as usize;
			let last = ((x1 + 1.0).ceil() as usize).min(self.width);
			for px in first..last {
				let w = tent_over(x0, x1, px as f32 + 0.5);
				if w > SPAN_MIN {
					self.spans.push((px, w));
				}
			}
		}
		self.span_at.push(self.spans.len());
	}

	fn take_row(&mut self) -> Row {
		self.spare.pop().unwrap_or_default()
	}

	// One grid line to one strip of preview pixels. Blank cells are skipped
	// before any color is resolved, which is most of a terminal buffer.
	// Answers whether the line laid down any ink at all.
	fn fill(
		&mut self,
		grid: &Grid<Cell>,
		line: Line,
		colors: &Colors,
		cfg: &config::Settings,
		readable: &mut palette::Readable,
		out: &mut Row,
	) -> bool {
		let width = self.width;
		#[cfg(test)]
		{
			self.rastered += 1;
		}
		out.clear();
		out.resize(width * 4, 0);
		let acc = &mut self.acc;
		acc.reset(width);
		let row = &grid[line][..];
		let (spans, span_at) = (&self.spans, &self.span_at);
		// A line is mostly runs of one style, so the last cell's colors are kept
		// rather than resolved again. The character's own share is cheap and
		// varies cell to cell, so it is not part of the key.
		let mut last: Option<(Style, Ink)> = None;
		for (c, cell) in row.iter().take(self.cols).enumerate() {
			if blank(cell) {
				continue;
			}
			let style = Style {
				fg: cell.fg,
				bg: cell.bg,
				flags: cell.flags & (Flags::INVERSE | Flags::HIDDEN),
			};
			let ink = match last {
				Some((seen, ink)) if seen == style => ink,
				_ => {
					let ink = paint(&style, colors, cfg, readable);
					last = Some((style, ink));
					ink
				}
			};
			let (rgb, alpha) = ink.at(ink_share(cell.c));
			if alpha <= 0.0 {
				continue;
			}
			for &(px, cover) in &spans[span_at[c]..span_at[c + 1]] {
				let w = cover * alpha;
				acc.rgb[px * 3] += rgb[0] as f32 * w;
				acc.rgb[px * 3 + 1] += rgb[1] as f32 * w;
				acc.rgb[px * 3 + 2] += rgb[2] as f32 * w;
				acc.weight[px] += w;
			}
		}
		let mut any = false;
		for px in 0..width {
			let w = acc.weight[px];
			if w <= 0.0 {
				continue;
			}
			any = true;
			out[px * 4] = to_u8(acc.rgb[px * 3] / w);
			out[px * 4 + 1] = to_u8(acc.rgb[px * 3 + 1] / w);
			out[px * 4 + 2] = to_u8(acc.rgb[px * 3 + 2] / w);
			out[px * 4 + 3] = to_u8(w.min(1.0) * 255.0);
		}
		any
	}

	// Where a line's ink sits inside the `lh` pixels the line occupies, as
	// (offset, height). Ramped between whole-line and BAND so the map does not
	// change brightness as a growing buffer compresses its lines.
	fn band(lh: f32) -> (f32, f32) {
		let t = ((lh - BAND_FLOOR) / (BAND_FULL - BAND_FLOOR)).clamp(0.0, 1.0);
		(lh * BAND_TOP * t, lh * (1.0 - t * (1.0 - BAND)))
	}
}

impl Store {
	// Make the edits the plan carries, then compose.
	fn run(&mut self, plan: &mut Plan) {
		for _ in 0..self.screen {
			if let Some(row) = self.rows.pop_back() {
				self.spare.push(row);
			}
		}
		if plan.clear {
			self.spare.extend(self.rows.drain(..));
		}
		for _ in 0..plan.front {
			if let Some(row) = self.rows.pop_front() {
				self.spare.push(row);
			}
		}
		for (row, stands_for) in plan.groups.drain(..) {
			for _ in 1..stands_for {
				let mut copy = self.spare.pop().unwrap_or_default();
				copy.clone_from(&row);
				self.rows.push_back(copy);
			}
			self.rows.push_back(row);
		}
		// the rows replaced, newest first, as runs, so two far apart do not
		// redo everything between them
		let mut runs: Vec<Range<usize>> = Vec::new();
		for (i, mut row) in plan.refined.drain(..) {
			if let Some(old) = self.rows.get_mut(i) {
				std::mem::swap(old, &mut row);
			}
			self.spare.push(row);
			match runs.last_mut() {
				Some(run) if run.start <= i + RUN_GAP => run.start = i.min(run.start),
				_ => runs.push(i..i + 1),
			}
		}
		self.screen = plan.screen.len();
		self.rows.extend(plan.screen.drain(..));

		let (width, img_h, scale) = (plan.width, plan.img_h, plan.scale);
		if plan.full || self.img.len() != width * img_h * 4 {
			self.img.clear();
			self.img.resize(width * img_h * 4, 0);
			self.compose(0..plan.total, plan, scale);
			return;
		}
		for run in runs {
			self.compose(run, plan, scale);
		}
		let hist = self.rows.len() - self.screen;
		self.compose(hist..self.rows.len(), plan, scale);
	}

	// Squash the cached rows into the column image: the pixel rows that rows
	// `lines` fall in. Each pixel row is worked out on its own, so redoing a
	// few matches what a whole compose draws there. Colour is the average of
	// the lines that actually have ink, so a lone red line is not washed out
	// by its blank neighbours; how bright the pixel gets is how much ink fell
	// in it.
	fn compose(&mut self, lines: Range<usize>, plan: &Plan, scale: f32) {
		let (width, img_h) = (plan.width, plan.img_h);
		let total = plan.total.min(self.rows.len());
		if total == 0 || width == 0 || lines.start >= lines.end.min(total) {
			return;
		}
		let lh = line_px(img_h as f32, total, scale);
		if lh <= 0.0 {
			return;
		}
		let used = (lh * total as f32).ceil().min(img_h as f32) as usize;
		// The band takes ink out of the line; putting it back concentrated keeps
		// a solid page as bright as it was, with the gap between lines showing.
		let (band_top, band_h) = Minimap::band(lh);
		let gain = if band_h > 0.0 { lh / band_h } else { 0.0 };
		// A line is spread over a tent a pixel each side rather than clipped to
		// the pixel row it falls in. A box leaves the line grid beating against
		// the pixel grid, and at these pitches the beat is slow enough to draw
		// broad bands down the column that are not in the text. Under
		// BAND_FLOOR a pixel row already averages more than a whole line, the
		// gap between lines is switched off and the column is even anyway, so
		// the box is kept there - the wider filter would cost three times as
		// much on the deep buffer where a compose is already the expensive one.
		let soft = lh >= BAND_FLOOR;
		let reach = if soft { 1.0 } else { 0.0 };
		let py0 = (lines.start as f32 * lh - reach - 1.0).floor().max(0.0) as usize;
		let py1 = ((lines.end.min(total) as f32 * lh + reach + 1.0).ceil() as usize).min(used);
		for py in py0..py1 {
			self.img[py * width * 4..(py + 1) * width * 4].fill(0);
			let acc = &mut self.acc;
			acc.reset(width);
			let y0 = py as f32;
			let y1 = y0 + 1.0;
			let first = (((y0 - reach - band_top - band_h) / lh).floor().max(0.0) as usize)
				.min(total.saturating_sub(1));
			let last =
				(((y1 + reach - band_top) / lh).ceil().max(0.0) as usize).clamp(first + 1, total);
			let mut whole = 0;
			for i in first..last {
				let top = i as f32 * lh + band_top;
				let row = &self.rows[i];
				// Most lines of a deep buffer fall wholly inside their pixel row
				// and all take the same share, so those add up as integers and
				// the share is applied once. That is most of the cost of a
				// compose at depth.
				if !soft && top >= y0 && top + band_h <= y1 {
					acc.add_whole(row);
					whole += 1;
					if whole == WHOLE_MAX {
						acc.flush(band_h * gain);
						whole = 0;
					}
					continue;
				}
				let cover = if soft {
					tent_over(top, top + band_h, y0 + 0.5) * gain
				} else {
					((top + band_h).min(y1) - top.max(y0)).max(0.0) * gain
				};
				if cover <= 0.0 {
					continue;
				}
				for px in 0..width {
					let a = row[px * 4 + 3] as f32 / 255.0;
					if a <= 0.0 {
						continue;
					}
					let w = a * cover;
					acc.rgb[px * 3] += row[px * 4] as f32 * w;
					acc.rgb[px * 3 + 1] += row[px * 4 + 1] as f32 * w;
					acc.rgb[px * 3 + 2] += row[px * 4 + 2] as f32 * w;
					acc.weight[px] += w;
					if a > acc.alpha[px] {
						acc.alpha[px] = a;
					}
				}
			}
			acc.flush(band_h * gain);
			let base = py * width * 4;
			for px in 0..width {
				let w = acc.weight[px];
				if w <= 0.0 {
					continue;
				}
				self.img[base + px * 4] = to_u8(acc.rgb[px * 3] / w);
				self.img[base + px * 4 + 1] = to_u8(acc.rgb[px * 3 + 1] / w);
				self.img[base + px * 4 + 2] = to_u8(acc.rgb[px * 3 + 2] / w);
				let a = w.min(1.0).max(acc.alpha[px] * LONE);
				self.img[base + px * 4 + 3] = to_u8(a * 255.0);
			}
		}
	}
}

impl Acc {
	fn reset(&mut self, width: usize) {
		self.rgb.clear();
		self.rgb.resize(width * 3, 0.0);
		self.weight.clear();
		self.weight.resize(width, 0.0);
		self.alpha.clear();
		self.alpha.resize(width, 0.0);
		self.sum.clear();
		self.sum.resize(width * 4, 0);
		self.peak.clear();
		self.peak.resize(width, 0);
	}

	// Planar, so the loop runs over several pixels at once.
	fn add_whole(&mut self, row: &[u8]) {
		let n = self.peak.len();
		let (red, rest) = self.sum.split_at_mut(n);
		let (green, rest) = rest.split_at_mut(n);
		let (blue, alpha) = rest.split_at_mut(n);
		let (alpha, peak, row) = (&mut alpha[..n], &mut self.peak[..n], &row[..n * 4]);
		for i in 0..n {
			let px =
				u32::from_le_bytes([row[i * 4], row[i * 4 + 1], row[i * 4 + 2], row[i * 4 + 3]]);
			let a = px >> 24;
			red[i] += (px & 0xff) * a;
			green[i] += ((px >> 8) & 0xff) * a;
			blue[i] += ((px >> 16) & 0xff) * a;
			alpha[i] += a;
			peak[i] = peak[i].max(a);
		}
	}

	// Fold the whole lines in, each taking `cover` of the pixel row.
	fn flush(&mut self, cover: f32) {
		let k = cover / 255.0;
		let n = self.peak.len();
		for px in 0..n {
			let a = self.sum[3 * n + px];
			if a > 0 {
				self.rgb[px * 3] += self.sum[px] as f32 * k;
				self.rgb[px * 3 + 1] += self.sum[n + px] as f32 * k;
				self.rgb[px * 3 + 2] += self.sum[2 * n + px] as f32 * k;
				self.weight[px] += a as f32 * k;
				self.alpha[px] = self.alpha[px].max(self.peak[px] as f32 / 255.0);
			}
		}
		self.sum.fill(0);
		self.peak.fill(0);
	}
}

// What decides a cell's preview color, apart from where it is.
#[derive(Clone, Copy, PartialEq)]
struct Style {
	fg: Color,
	bg: Color,
	flags: Flags,
}

// A style's two resolved colors, kept so a run of one style resolves once and
// each cell in it only has to mix its own character's share in.
#[derive(Clone, Copy)]
struct Ink {
	fg: [u8; 3],
	bg: [u8; 3],
	own_bg: bool,
}

impl Ink {
	// The pixel color and how much of it shows, for a character covering
	// `share` of what the densest one covers.
	fn at(self, share: f32) -> ([u8; 3], f32) {
		let ink = INK * share;
		// A cell with its own background paints solid; otherwise only its ink
		// shows, so an indented or short line reads as one.
		if self.own_bg {
			(mix(self.bg, self.fg, ink), 1.0)
		} else {
			(self.fg, ink)
		}
	}
}

// A cell's colors in the preview.
fn paint(
	style: &Style,
	colors: &Colors,
	cfg: &config::Settings,
	readable: &mut palette::Readable,
) -> Ink {
	let mut fg = palette::resolve(style.fg, colors, cfg);
	let mut bg = palette::resolve(style.bg, colors, cfg);
	if style.flags.contains(Flags::INVERSE) {
		std::mem::swap(&mut fg, &mut bg);
	}
	if style.flags.contains(Flags::HIDDEN) {
		fg = bg;
	}
	Ink {
		fg: readable.get(fg, bg, cfg.min_contrast()),
		bg,
		own_bg: bg != cfg.bg,
	}
}

// How much of its cell a character inks, against the densest ones at 1.0.
// Flat ink is what made a run of text read as a bar; a period and a hash are
// nothing alike from across the room. Eyeballed from a monospace face rather
// than measured, since the map is a hint and the face is not known here.
#[rustfmt::skip]
const INK_SHARE: [f32; 95] = [
	// ' '   !     "     #     $     %     &     '     (     )     *     +
	0.00, 0.40, 0.30, 1.00, 0.90, 0.85, 0.90, 0.22, 0.45, 0.45, 0.45, 0.50,
	// ,     -     .     /     0     1     2     3     4     5     6     7
	0.25, 0.30, 0.22, 0.50, 0.90, 0.55, 0.80, 0.80, 0.80, 0.80, 0.85, 0.60,
	// 8     9     :     ;     <     =     >     ?     @     A     B     C
	0.90, 0.85, 0.30, 0.35, 0.50, 0.45, 0.50, 0.60, 1.00, 0.85, 0.90, 0.75,
	// D     E     F     G     H     I     J     K     L     M     N     O
	0.85, 0.80, 0.70, 0.85, 0.85, 0.40, 0.50, 0.80, 0.60, 1.00, 0.90, 0.85,
	// P     Q     R     S     T     U     V     W     X     Y     Z     [
	0.75, 0.90, 0.85, 0.75, 0.60, 0.80, 0.75, 1.00, 0.80, 0.65, 0.75, 0.40,
	// \     ]     ^     _     `     a     b     c     d     e     f     g
	0.50, 0.40, 0.30, 0.30, 0.20, 0.70, 0.75, 0.60, 0.75, 0.70, 0.55, 0.80,
	// h     i     j     k     l     m     n     o     p     q     r     s
	0.70, 0.35, 0.40, 0.70, 0.35, 0.90, 0.65, 0.70, 0.75, 0.75, 0.45, 0.60,
	// t     u     v     w     x     y     z     {     |     }     ~
	0.50, 0.65, 0.60, 0.85, 0.60, 0.65, 0.60, 0.45, 0.35, 0.45, 0.30,
];

fn ink_share(c: char) -> f32 {
	if c.is_ascii_graphic() || c == ' ' {
		return INK_SHARE[c as usize - 0x20];
	}
	if c.is_whitespace() || c < ' ' {
		return 0.0;
	}
	match c as u32 {
		// box drawing: thin strokes, but they run the width of the cell
		0x2500..=0x257F => 0.70,
		// block elements and shades
		0x2580..=0x259F => 1.00,
		// private use, where a patched font keeps its powerline glyphs
		0xE000..=0xF8FF => 0.90,
		// everything else reads as an ordinary letter
		_ => 0.80,
	}
}

// Time between composes, given what the last one took.
fn gap(spent: Duration) -> Duration {
	Duration::from_millis(COMPOSE_MS).max(spent * COMPOSE_SHARE)
}

// Nothing to draw: an unstyled space. Checked before any color is resolved,
// which is what keeps rasterizing a mostly-empty buffer cheap.
fn blank(cell: &Cell) -> bool {
	cell.c == ' '
		&& cell.bg == Color::Named(NamedColor::Background)
		&& !cell.flags.intersects(Flags::INVERSE)
}

// Nearest byte for a value in 0..=255. `f32::round` is a library call on
// baseline x86-64, and this runs for every pixel of every line in a compose.
fn to_u8(x: f32) -> u8 {
	(x + 0.5) as u8
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
	let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
	[m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

fn row_hash(grid: &Grid<Cell>, line: Line, cols: usize) -> u64 {
	let row = &grid[line];
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for c in 0..cols {
		hash = (hash ^ row[Column(c)].c as u64).wrapping_mul(0x100_0000_01b3);
	}
	hash
}

// Renderer

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
	resolution: [f32; 2],
	pos: [f32; 2],
	size: [f32; 2],
	alpha: f32,
	_pad: f32,
}

struct PaneTex {
	texture: wgpu::Texture,
	bind: wgpu::BindGroup,
	uniform: wgpu::Buffer,
	w: u32,
	h: u32,
	rev: u64,
	used: bool,
}

/// One textured quad per pane. Each pane owns its texture and uniform, so the
/// column can be drawn with one draw call each inside the main pass.
pub struct MapRenderer {
	pipeline: wgpu::RenderPipeline,
	layout: wgpu::BindGroupLayout,
	sampler: wgpu::Sampler,
	panes: HashMap<u64, PaneTex>,
}

impl std::fmt::Debug for MapRenderer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("MapRenderer")
			.field("panes", &self.panes.len())
			.finish_non_exhaustive()
	}
}

impl MapRenderer {
	pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
		let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("minimap bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("minimap sampler"),
			mag_filter: wgpu::FilterMode::Nearest,
			min_filter: wgpu::FilterMode::Nearest,
			..Default::default()
		});
		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("minimap shader"),
			source: wgpu::ShaderSource::Wgsl(MAP_WGSL.into()),
		});
		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("minimap layout"),
			bind_group_layouts: &[Some(&layout)],
			immediate_size: 0,
		});
		let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("minimap pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs"),
				compilation_options: Default::default(),
				buffers: &[],
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs"),
				compilation_options: Default::default(),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
					write_mask: wgpu::ColorWrites::ALL,
				})],
			}),
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleStrip,
				..Default::default()
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			multiview_mask: None,
			cache: None,
		});
		Self {
			pipeline,
			layout,
			sampler,
			panes: HashMap::new(),
		}
	}

	pub fn begin_frame(&mut self) {
		for p in self.panes.values_mut() {
			p.used = false;
		}
	}

	/// Upload a pane's column image and place its quad. Must run before the pass
	/// that draws it.
	#[allow(clippy::too_many_arguments)]
	pub fn prepare(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		id: u64,
		at: Rect,
		res: (f32, f32),
		map: &Minimap,
	) {
		let (pixels, w, h) = map.image();
		if pixels.is_empty() || w == 0 || h == 0 {
			return;
		}
		let (w, h) = (w as u32, h as u32);
		if self.panes.get(&id).is_none_or(|p| p.w != w || p.h != h) {
			self.panes
				.insert(id, make_tex(device, &self.layout, &self.sampler, w, h));
		}
		let Some(entry) = self.panes.get_mut(&id) else {
			return;
		};
		entry.used = true;
		if entry.rev != map.rev {
			entry.rev = map.rev;
			queue.write_texture(
				wgpu::TexelCopyTextureInfo {
					texture: &entry.texture,
					mip_level: 0,
					origin: wgpu::Origin3d::ZERO,
					aspect: wgpu::TextureAspect::All,
				},
				pixels,
				wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(4 * w),
					rows_per_image: Some(h),
				},
				wgpu::Extent3d {
					width: w,
					height: h,
					depth_or_array_layers: 1,
				},
			);
		}
		queue.write_buffer(
			&entry.uniform,
			0,
			bytemuck::bytes_of(&Uniform {
				resolution: [res.0, res.1],
				pos: [at.x, at.y],
				size: [at.w, at.h],
				alpha: PREVIEW_A,
				_pad: 0.0,
			}),
		);
	}

	pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, id: u64) {
		let Some(p) = self.panes.get(&id) else { return };
		if !p.used {
			return;
		}
		pass.set_pipeline(&self.pipeline);
		pass.set_bind_group(0, &p.bind, &[]);
		pass.draw(0..4, 0..1);
	}

	/// Release the textures of panes that drew nothing this frame (closed, or
	/// the column switched off).
	pub fn end_frame(&mut self) {
		self.panes.retain(|_, p| p.used);
	}
}

fn make_tex(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	sampler: &wgpu::Sampler,
	w: u32,
	h: u32,
) -> PaneTex {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("minimap tex"),
		size: wgpu::Extent3d {
			width: w,
			height: h,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba8UnormSrgb,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	let uniform = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("minimap uniform"),
		size: std::mem::size_of::<Uniform>() as u64,
		usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("minimap bind"),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: uniform.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::TextureView(&view),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: wgpu::BindingResource::Sampler(sampler),
			},
		],
	});
	PaneTex {
		texture,
		bind,
		uniform,
		w,
		h,
		rev: u64::MAX,
		used: false,
	}
}

const MAP_WGSL: &str = r"
struct Uniform {
    resolution: vec2<f32>,
    pos: vec2<f32>,
    size: vec2<f32>,
    alpha: f32,
    pad: f32,
};
@group(0) @binding(0) var<uniform> u: Uniform;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let px = u.pos + corner * u.size;
    var out: VOut;
    out.pos = vec4<f32>(px.x / u.resolution.x * 2.0 - 1.0, 1.0 - px.y / u.resolution.y * 2.0, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    let a = c.a * u.alpha;
    return vec4<f32>(c.rgb * a, a); // premultiplied
}
";

#[cfg(test)]
impl Minimap {
	// Seed the cache directly, so the compose can be driven without a live grid.
	fn seed(&mut self, width: usize, rows: Vec<Row>) {
		self.width = width;
		self.shown = rows.len();
		self.hist = rows.len();
		self.rough = vec![false; rows.len()].into();
		self.store.rows = rows.into();
		self.store.screen = 0;
	}

	// A whole compose of the seeded rows, here and now.
	fn compose(&mut self, img_h: usize, scale: f32) {
		let plan = Plan {
			full: true,
			width: self.width,
			img_h,
			total: self.shown.min(self.store.rows.len()),
			scale,
			..Default::default()
		};
		let len = self.width * img_h * 4;
		self.store.img.clear();
		self.store.img.resize(len, 0);
		self.store.compose(0..plan.total, &plan, scale);
		self.publish(&plan);
	}

	// A compose that went to its own thread, waited for.
	fn wait(&mut self) {
		while let Some(job) = &self.job {
			if job.thread.is_finished() {
				self.collect(Instant::now());
			} else {
				std::thread::sleep(Duration::from_millis(1));
			}
		}
	}
	fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
		let i = (y * self.img_w + x) * 4;
		[
			self.img[i],
			self.img[i + 1],
			self.img[i + 2],
			self.img[i + 3],
		]
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::fuzz::Rng;
	use alacritty_terminal::event::{Event, EventListener};
	use alacritty_terminal::term::{Config as TermConfig, Term};
	use alacritty_terminal::vte::ansi::Processor;
	use std::fmt::Write as _;

	struct VoidListener;
	impl EventListener for VoidListener {
		fn send_event(&self, _e: Event) {}
	}

	// A grid with the cursor still on its first row, so what is written lands
	// at the top and the rows under it stay blank.
	fn fresh_term(cols: usize, lines: usize, scrollback: usize) -> (Term<VoidListener>, Processor) {
		let cfg = TermConfig {
			scrolling_history: scrollback,
			..Default::default()
		};
		let dims = crate::term::TermDimensions {
			columns: cols,
			screen_lines: lines,
		};
		(Term::new(cfg, &dims, VoidListener), Processor::new())
	}

	// A live grid with the cursor already on its bottom row, so from here every
	// newline pushes exactly one line into history.
	fn live_term(cols: usize, lines: usize, scrollback: usize) -> (Term<VoidListener>, Processor) {
		let (mut term, mut parser) = fresh_term(cols, lines, scrollback);
		parser.advance(&mut term, "\r\n".repeat(lines - 1).as_bytes());
		(term, parser)
	}

	// One line of mixed text and styles, short enough never to wrap.
	fn styled_line(rng: &mut Rng, cols: usize) -> String {
		let mut out = String::from("\r");
		for _ in 0..rng.below(cols) {
			// writing to a String cannot fail
			let _ = match rng.below(12) {
				0 => write!(out, "\x1b[{}m", 30 + rng.below(8)),
				1 => write!(out, "\x1b[{}m", 40 + rng.below(8)),
				2 => write!(out, "\x1b[38;5;{}m", rng.below(256)),
				_ => Ok(()),
			};
			match rng.below(12) {
				3 => out += "\x1b[7m",
				4 => out += "\x1b[0m",
				5 | 6 => out.push(' '),
				_ => out.push(*rng.pick(&['a', 'm', 'W', '#', '.', '|'])),
			}
		}
		out
	}

	// One rasterized line: `ink` pixels of `rgb` at full coverage, rest empty.
	fn row(width: usize, ink: usize, rgb: [u8; 3]) -> Row {
		let mut out = vec![0u8; width * 4];
		for px in 0..ink {
			out[px * 4] = rgb[0];
			out[px * 4 + 1] = rgb[1];
			out[px * 4 + 2] = rgb[2];
			out[px * 4 + 3] = 255;
		}
		out
	}

	fn cfg(on: bool, width: f32) -> config::Settings {
		config::Settings {
			minimap: on,
			minimap_width: width,
			..Default::default()
		}
	}

	// A full-screen program draws on its own screen, which has no scroll buffer,
	// so the column steps aside - unless the program is one that was named.
	// A page of identical lines has to compose to an evenly lit column. The
	// gap drawn between lines beats against the pixel grid, and at these
	// pitches the beat is slow enough to read as broad bands running down the
	// column that are not in the text at all.
	// Test ID: EqRBEHA
	#[test]
	fn a_page_of_one_line_composes_evenly() {
		let settings = config::Settings::default();
		let (cols, lines) = (80, 40);
		let (width, img_h) = (60, 900);
		// 0.85, 1.10 and 1.15 px per line, the pitches where the beat is both
		// strong and slow
		for &total in &[1020usize, 780, 744] {
			let (mut term, mut parser) = live_term(cols, lines, 20_000);
			for _ in 0..total {
				parser.advance(&mut term, "#".repeat(cols).as_bytes());
				parser.advance(&mut term, b"\r\n");
			}
			let mut map = Minimap::default();
			map.update(
				term.grid(),
				term.colors(),
				&settings,
				width,
				img_h,
				1.0,
				lines,
				cols,
				total,
				0,
				false,
				true,
				Instant::now(),
			);
			let lh = line_px(img_h as f32, map.shown, 1.0);
			// the screen it started on is pushed up ahead of the page, and the
			// screen it ends on sits below it, so measure between the two
			let from = (lh * (lines + 1) as f32) as usize;
			let used = (lh * (map.shown - lines) as f32) as usize;
			let rows: Vec<f32> = (from..used - 2)
				.map(|y| (0..width).map(|x| map.pixel(x, y)[3] as f32).sum::<f32>() / width as f32)
				.collect();
			let mean = rows.iter().sum::<f32>() / rows.len() as f32;
			let lo = rows.iter().copied().fold(f32::MAX, f32::min);
			let hi = rows.iter().copied().fold(0.0f32, f32::max);
			assert!(
				(hi - lo) / mean < 0.12,
				"{total} lines at {lh:.2} px: {lo} to {hi} about {mean}"
			);
		}
	}

	// The same across the column. Cells do not line up with pixels either, and
	// a filter no wider than one pixel leaves the cell grid beating against
	// them - a comb down the column that is not in the text.
	// Test ID: EqRBEHB
	#[test]
	fn a_repeating_line_composes_without_a_comb() {
		let cfg = config::Settings::default();
		let (cols, width) = (100, 90);
		let (mut term, mut parser) = fresh_term(cols, 4, 10);
		// every other cell inked, which is the worst case for the beat
		parser.advance(&mut term, "#\u{a0}".repeat(cols / 2).as_bytes());
		let mut map = Minimap {
			width,
			cols,
			..Default::default()
		};
		map.fit_spans();
		let mut strip = map.take_row();
		map.fill(
			term.grid(),
			Line(0),
			term.colors(),
			&cfg,
			&mut palette::Readable::default(),
			&mut strip,
		);
		// a five-pixel average holds no cell-scale detail, so what is left in
		// it is the slow beat
		let smooth: Vec<f32> = (4..width - 4)
			.map(|x| {
				(x - 2..=x + 2)
					.map(|i| strip[i * 4 + 3] as f32)
					.sum::<f32>() / 5.0
			})
			.collect();
		let mean = smooth.iter().sum::<f32>() / smooth.len() as f32;
		let lo = smooth.iter().copied().fold(f32::MAX, f32::min);
		let hi = smooth.iter().copied().fold(0.0f32, f32::max);
		assert!(
			(hi - lo) / mean < 0.15,
			"{cols} columns into {width} px: {lo} to {hi} about {mean}"
		);
	}

	// Test ID: EopeF4y
	#[test]
	fn a_full_screen_program_takes_the_column_unless_it_is_named() {
		let mut s = cfg(true, 100.0);
		s.minimap_tui_whitelist = "less tmux screen".to_string();
		// nothing full-screen is running, so it does not matter what is
		assert!(wanted(&s, false, None));
		assert!(wanted(&s, false, Some("vim")));
		// on the alt screen only the named ones keep it
		assert!(wanted(&s, true, Some("less")));
		assert!(wanted(&s, true, Some("tmux")));
		assert!(!wanted(&s, true, Some("vim")));
		assert!(!wanted(&s, true, Some("nano")));
		// a program nobody can name is not one that was named
		assert!(!wanted(&s, true, None));
		// either spelling of the same program, on either platform
		assert!(wanted(&s, true, Some("LESS.EXE")));
		assert!(wanted(&s, true, Some("/usr/bin/less")));
		s.minimap_tui_whitelist = r"C:	ools\less.exe".to_string();
		assert!(wanted(&s, true, Some("less")));
	}

	// Test ID: EoXOeuW
	#[test]
	fn off_costs_the_pane_nothing() {
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: 800.0,
			h: 600.0,
		};
		let text = text_rect(full, &cfg(false, 100.0), 1.0, true);
		assert_eq!(text.w, full.w);
		assert_eq!(column_w(&cfg(false, 100.0), full.w, 1.0, true), 0.0);
	}

	// Test ID: EpyCuGf
	#[test]
	fn the_column_takes_its_width_and_no_more() {
		let s = cfg(true, 100.0);
		assert_eq!(column_w(&s, 800.0, 1.0, true), 100.0);
		let full = Rect {
			x: 10.0,
			y: 20.0,
			w: 800.0,
			h: 600.0,
		};
		assert_eq!(text_rect(full, &s, 1.0, true).w, 700.0);
	}

	// Test ID: EoXOeuX
	#[test]
	fn a_narrow_pane_gives_up_the_column() {
		// half a pane is the most the column may take, and below the narrowest
		// column there is nothing worth showing
		assert_eq!(column_w(&cfg(true, 100.0), 20.0, 1.0, true), 0.0);
		assert_eq!(column_w(&cfg(true, 100.0), 120.0, 1.0, true), 60.0);
	}

	// Test ID: EoXOeuY
	#[test]
	fn a_short_buffer_does_not_stretch_to_fill() {
		// 50 lines in a 600px column: capped, so well short of the 600
		let lh = line_px(600.0, 50, 1.0);
		assert_eq!(lh, MAX_LINE_PX);
		assert!(lh * 50.0 < 300.0);
		// 10,000 lines compress instead
		assert!(line_px(600.0, 10_000, 1.0) < 0.1);
	}

	// Test ID: EoXOeuZ
	#[test]
	fn the_handle_rides_the_scroll_position() {
		let (top_y, _) = handle_span(600.0, 1000, 1000, 40, 960.0, 1.0);
		assert_eq!(top_y, 0.0); // scrolled to the oldest line
		let (bot_y, bot_h) = handle_span(600.0, 1000, 1000, 40, 0.0, 1.0);
		let used = line_px(600.0, 1000, 1.0) * 1000.0;
		assert!((bot_y + bot_h - used).abs() < 0.01); // at the newest
	}

	// A drag moves the view at the map's own rate, from where it started. This
	// used to read the marker's drawn top back through the inverse of where it
	// was drawn; the height floor makes that reading approximate, so the drag
	// works from the grab instead.
	// Test ID: EoXOeua
	#[test]
	fn a_drag_round_trips_through_the_mapping() {
		let track = 600.0;
		let (total, rows) = (1000, 40);
		let g = track_geom(track, total, total, rows);
		let pitch = line_px(track, total, 1.0);
		for pos in [0.0, 120.0, 500.0, 960.0] {
			let (y, _) = handle_span(track, total, total, rows, pos, 1.0);
			let still = drag_to(&g, total, total, rows, (y, pos), y, 1.0);
			assert!(
				(still - pos).abs() < 1e-3,
				"{pos} moved to {still} on its own"
			);
			// ten lines' worth of pixels up the column is ten lines back
			let want = (pos + 10.0).min((total - rows) as f32);
			let moved = drag_to(&g, total, total, rows, (y, pos), y - 10.0 * pitch, 1.0);
			assert!(
				(moved - want).abs() < 0.01,
				"{pos} -> {moved}, wanted {want}"
			);
		}
	}

	// The marker covers the lines that are on screen. Under a flood the map
	// stops where the eased text has reached, and measuring the marker against
	// the whole buffer instead left it well above those lines, with a click in
	// the column landing off center (F148). Where the height floor makes the
	// marker taller than the rows it stands for, it grows from their middle,
	// so the slack is half that excess and no more.
	// Test ID: EqR4Ye0
	#[test]
	fn the_marker_sits_over_the_lines_it_stands_for() {
		let track = 900.0;
		let rows = 48;
		for &(total, shown) in &[(148, 101), (1048, 1001), (10_048, 9_858)] {
			let pitch = line_px(track, shown, 1.0);
			let lag = (total - shown) as f32;
			let top = (total - rows) as f32;
			let used = pitch * shown as f32;
			for step in 0..=10 {
				let pos = lag + (top - lag) * step as f32 / 10.0;
				let (y, h) = handle_span(track, total, shown, rows, pos, 1.0);
				let first = top - pos;
				let want = (first + rows as f32 * 0.5) * pitch;
				let note = format!("{total}/{shown} at {pos}: {y}+{h} of {used}");
				if want - h * 0.5 >= 0.01 && want + h * 0.5 <= used - 0.01 {
					assert!(
						(y + h * 0.5 - want).abs() < 0.6,
						"{note}: middle {} wants {want}",
						y + h * 0.5
					);
				} else {
					// within half a floored marker of an end, so it sits flush
					// with that end rather than hanging off the map
					let flush = y < 0.01 || (y + h - used).abs() < 0.01;
					assert!(flush, "{note}: neither centered nor flush");
				}
			}
		}
	}

	// A grab stores the pointer and the position it stood for, and the first
	// drag event feeds the same pointer back, so it has to come out unmoved.
	// Where it does not, a press and one pixel of movement scrolls the view on
	// its own.
	// Test ID: EqMZXTc
	#[test]
	fn a_marker_reads_back_the_position_it_was_drawn_at() {
		let track = 900.0;
		// a full screen, a screen holding only a prompt at two history depths,
		// and a deep buffer where the height floor eats most of the travel
		for &(total, shown, rows) in &[
			(1000, 1000, 48),
			(148, 101, 48),
			(1048, 1001, 48),
			(10_048, 9_858, 48),
			(10_048, 10_048, 48),
			(58, 11, 48),
		] {
			let g = track_geom(track, total, shown, rows);
			let back = (total - rows) as f32;
			for step in 0..=20 {
				let pos = back * step as f32 / 20.0;
				let (y, h) = handle_span(track, total, shown, rows, pos, 1.0);
				let read = drag_to(&g, total, shown, rows, (y, pos), y, 1.0);
				assert!(
					(read - pos).abs() < 0.5,
					"{total}/{shown}: drawn at {pos} reads back {read}"
				);
				// and it stays inside the map it rides
				let used = line_px(track, shown, 1.0) * shown as f32;
				assert!(
					y >= 0.0 && y + h <= used + 0.01,
					"{total}/{shown}: {y}+{h} of {used}"
				);
			}
			// the bottom of the buffer is reachable by clicking the track, not
			// only by dragging past the end of it
			let used = line_px(track, shown, 1.0) * shown as f32;
			assert_eq!(
				center_on(&g, total, shown, rows, used, 1.0),
				0.0,
				"{total}/{shown}: a click on the last drawn line misses the newest output"
			);
		}
	}

	// A column of the given height, for the tests that need a `Geom`.
	fn track_geom(track: f32, total: usize, shown: usize, rows: usize) -> Geom {
		geom(
			Rect {
				x: 0.0,
				y: 0.0,
				w: 400.0,
				h: track,
			},
			0.0,
			1.0,
			&cfg(true, 60.0),
			total,
			shown,
			rows,
			0.0,
			false,
			true,
		)
		.unwrap()
	}

	// Test ID: EoXOeub
	#[test]
	fn the_handle_stays_grabbable_on_a_deep_buffer() {
		let (_, h) = handle_span(600.0, 100_000, 100_000, 40, 0.0, 1.0);
		assert!(h >= MIN_HANDLE);
	}

	// Test ID: EoXOeuc
	#[test]
	fn a_line_composes_where_the_mapping_puts_it() {
		// 100 lines in a 200px column, line 40 the only red one
		let width = 8;
		let mut rows: Vec<Row> = (0..100).map(|_| row(width, 4, [0, 200, 0])).collect();
		rows[40] = row(width, 4, [200, 0, 0]);
		let mut map = Minimap::default();
		map.seed(width, rows);
		map.compose(200, 1.0);
		let lh = line_px(200.0, 100, 1.0);
		let red = (40.0 * lh) as usize;
		assert_eq!(map.pixel(0, red), [200, 0, 0, 255]);
		assert_eq!(map.pixel(0, red - 2)[0..3], [0, 200, 0]);
		// the gap between lines: over a page of them some pixel rows are short
		// of full, so it reads as lines rather than one block
		let dim = (0..100).filter(|&y| map.pixel(0, y)[3] < 250).count();
		assert!(dim > 20, "{dim} of 100 pixel rows fell short of solid");
		// past the ink the row is clear, so the wallpaper shows through
		assert_eq!(map.pixel(6, red)[3], 0);
	}

	// Test ID: EoXOeud
	#[test]
	fn a_short_buffer_leaves_the_bottom_of_the_column_empty() {
		let width = 4;
		let rows: Vec<Row> = (0..50).map(|_| row(width, 4, [0, 200, 0])).collect();
		let mut map = Minimap::default();
		map.seed(width, rows);
		map.compose(400, 1.0);
		// 50 lines at the capped height fill a fraction of the 400
		let used = (line_px(400.0, 50, 1.0) * 50.0) as usize;
		assert!(used < 200);
		assert!(map.pixel(0, used - 1)[3] > 0);
		assert_eq!(map.pixel(0, used + 1)[3], 0);
	}

	// Test ID: EoXOeue
	#[test]
	fn one_inked_line_among_many_still_shows() {
		// 5,000 lines into 500px: 10 lines to a pixel row, and the single red one
		// must keep its color and stay findable, dimmer than a solid page
		let width = 4;
		let mut rows: Vec<Row> = (0..5000).map(|_| vec![0u8; width * 4]).collect();
		rows[2500] = row(width, 4, [200, 0, 0]);
		let mut map = Minimap::default();
		map.seed(width, rows);
		map.compose(500, 1.0);
		let lone = map.pixel(0, 250);
		assert_eq!(lone[0..3], [200, 0, 0]);
		assert!(lone[3] > 80 && lone[3] < 200, "{lone:?}");
	}

	// What the map is for is reading density from a distance, so a stretch of
	// mostly blank lines has to look different from a solid page.
	// Test ID: Eolyf2O
	#[test]
	fn a_sparse_stretch_reads_dimmer_than_a_full_one() {
		let width = 4;
		let solid: Vec<Row> = (0..5000).map(|_| row(width, 4, [0, 200, 0])).collect();
		let mut sparse: Vec<Row> = (0..5000).map(|_| vec![0u8; width * 4]).collect();
		for i in (0..5000).step_by(4) {
			sparse[i] = row(width, 4, [0, 200, 0]);
		}
		let alpha = |rows: Vec<Row>| {
			let mut map = Minimap::default();
			map.seed(width, rows);
			map.compose(500, 1.0);
			map.pixel(0, 250)[3]
		};
		let (full, thin) = (alpha(solid), alpha(sparse));
		assert_eq!(full, 255);
		assert!(thin < full - 40, "solid {full}, sparse {thin}");
		assert!(thin > 0);
	}

	// Under a flood most lines leave history before any compose shows them, and
	// the build that folds them in holds the term lock the PTY reader waits on.
	// So a build that does not compose rasterizes nothing, and a compose does no
	// more than the history and the screen, however much output went by.
	// Test ID: EqLtc6K
	#[test]
	fn a_flood_rasterizes_only_what_a_compose_shows() {
		let (cols, lines, scrollback) = (80, 24, 1000);
		let (mut term, mut parser) = live_term(cols, lines, scrollback);
		let cfg = config::Settings::default();
		let line = format!("\x1b[32m{}\x1b[0m\r\n", "y".repeat(cols - 2));
		let chunk = 300;
		let mut map = Minimap::default();
		let start = Instant::now();
		let mut composes = 0;
		for frame in 0..100u64 {
			parser.advance(&mut term, line.repeat(chunk).as_bytes());
			let (rastered, rev) = (map.rastered, map.rev);
			let now = start + Duration::from_millis(frame * 10);
			map.update(
				term.grid(),
				term.colors(),
				&cfg,
				16,
				300,
				1.0,
				lines,
				cols,
				chunk,
				0,
				true,
				false,
				now,
			);
			let did = map.rastered - rastered;
			if map.rev == rev {
				assert_eq!(
					did, 0,
					"frame {frame}: a build that did not compose rasterized"
				);
			} else {
				composes += 1;
				let most = term.grid().history_size() + lines;
				assert!(
					did <= most,
					"frame {frame}: {did} rows for one compose, at most {most}"
				);
			}
		}
		// a second of frames; how many composes depends on how long each took
		assert!((2..=13).contains(&composes), "{composes} composes");
		// 30,000 lines went by
		assert!(
			map.rastered <= composes * (scrollback + lines),
			"{}",
			map.rastered
		);
		assert!(map.pixel(0, 290)[3] > 0, "the map shows the flood");
	}

	// A compose holds the term lock, and how long it takes grows with the
	// scrollback. So the wait after one grows with it, keeping the map to a
	// small share of the time at any depth.
	// Test ID: EqLtc6L
	#[test]
	fn a_slow_compose_waits_its_share_out() {
		assert_eq!(gap(Duration::ZERO), Duration::from_millis(COMPOSE_MS));
		for ms in [5, 40, 700] {
			let spent = Duration::from_millis(ms);
			let share = spent.as_secs_f64() / gap(spent).as_secs_f64();
			assert!(share <= 0.05, "{ms} ms spent, {share:.3} of the time");
		}
		let (cols, lines) = (40, 5);
		let (mut term, mut parser) = live_term(cols, lines, 100);
		let cfg = config::Settings::default();
		let t0 = Instant::now();
		let mut map = Minimap::default();
		let mut build = |map: &mut Minimap, term: &mut Term<VoidListener>, at: u64| {
			parser.advance(term, b"\rsome output\r\n");
			let now = t0 + Duration::from_millis(at);
			map.update(
				term.grid(),
				term.colors(),
				&cfg,
				8,
				50,
				1.0,
				lines,
				cols,
				1,
				0,
				true,
				false,
				now,
			);
		};
		build(&mut map, &mut term, 0);
		let first = map.rev;
		map.spent = Duration::from_millis(50);
		build(&mut map, &mut term, 500);
		assert_eq!(map.rev, first, "composed again inside its wait");
		assert_eq!(map.wake(), Some(t0 + Duration::from_secs(1)));
		build(&mut map, &mut term, 1000);
		assert_ne!(map.rev, first, "the wait is over and the map is behind");
	}

	// A 445 ms whole redraw at 30,000 lines held the next 22 ms one back for
	// nearly nine seconds.
	// Test ID: ErCaBEG
	#[test]
	fn a_whole_redraw_does_not_set_the_wait() {
		let (cols, lines) = (40, 5);
		let (mut term, mut parser) = live_term(cols, lines, 100);
		let cfg = config::Settings::default();
		let t0 = Instant::now();
		let mut map = Minimap::default();
		let mut build = |map: &mut Minimap, term: &mut Term<VoidListener>, at: u64, cut: bool| {
			parser.advance(term, b"\rsome output\r\n");
			let now = t0 + Duration::from_millis(at);
			map.update(
				term.grid(),
				term.colors(),
				&cfg,
				8,
				50,
				1.0,
				lines,
				cols,
				1,
				0,
				true,
				cut,
				now,
			);
		};
		build(&mut map, &mut term, 0, false);
		let slow = Duration::from_millis(50);
		map.spent = slow;
		let before = map.rev;
		build(&mut map, &mut term, 1000, true);
		assert_ne!(map.rev, before, "the whole redraw composed");
		assert_eq!(map.spent, slow, "a whole redraw set the wait");
		let before = map.rev;
		build(&mut map, &mut term, 2000, false);
		assert_ne!(map.rev, before, "the ordinary one composed");
		assert!(map.spent < slow, "an ordinary compose sets the wait");
	}

	// At 100,000 lines one compose held the terminal for 150 ms, reading every
	// line of a rebuilt or flooded scrollback and composing all of it. Here a
	// compose reads no more than its budget, and a whole image goes to a
	// thread of its own while the builds carry on.
	// Test ID: ErEKIJ0
	#[test]
	fn a_deep_redraw_reads_only_its_budget_here() {
		let (cols, lines, scrollback) = (40, 10, 3000);
		let (mut term, mut parser) = live_term(cols, lines, scrollback);
		let mut rng = Rng::new(3);
		let flood =
			|term: &mut Term<VoidListener>, parser: &mut Processor, rng: &mut Rng, n: usize| {
				let mut text = String::new();
				for _ in 0..n {
					text += &styled_line(rng, cols);
					text += "\r\n";
				}
				parser.advance(term, text.as_bytes());
			};
		flood(&mut term, &mut parser, &mut rng, scrollback + 500);
		let cfg = config::Settings::default();
		let budget = 100;
		let mut map = Minimap {
			budget: Some(budget),
			here_px: Some(0),
			..Default::default()
		};
		// a minute a step, so the throttle is never what decides a compose
		let start = Instant::now();
		let update =
			|map: &mut Minimap, term: &Term<VoidListener>, pushed: usize, at: u64, cut: bool| {
				let rastered = map.rastered;
				map.update(
					term.grid(),
					term.colors(),
					&cfg,
					16,
					300,
					1.0,
					lines,
					cols,
					pushed,
					0,
					false,
					cut,
					start + Duration::from_secs(at * 60),
				);
				let read = map.rastered - rastered;
				assert!(read <= budget + lines, "{read} lines read at {at} s");
			};

		// the scrollback rebuilt whole, as after a screen swap or a resize
		update(&mut map, &term, 0, 0, true);
		assert!(map.job.is_some(), "the whole image was composed here");
		assert!(map.pending(), "and nothing owed while it is away");
		assert!(map.wake().is_some(), "or a time to look for it");
		let rev = map.rev;
		map.wait();
		assert_ne!(map.rev, rev, "the image never came back");
		let (px, w, h) = map.image();
		assert_eq!(px.len(), w * h * 4);
		assert!(map.rough_n > scrollback / 2, "{} stand-ins", map.rough_n);

		// then a flood that turns the scrollback over between composes
		for at in 1..20 {
			flood(&mut term, &mut parser, &mut rng, scrollback / 2);
			update(&mut map, &term, scrollback / 2, at, false);
			map.wait();
		}
		assert!(map.pixel(0, 290)[3] > 0, "the map shows the flood");
	}

	// A stand-in is only there until a compose has the time, and once the
	// output stops every line is drawn from itself: the image ends up the one
	// a cache with no budget makes. The composes that replace them redo only
	// the pixel rows they touch, so this also holds that to a whole compose,
	// under both filters.
	// Test ID: ErEKIcQ
	#[test]
	fn stand_ins_give_way_to_the_lines_they_stand_for() {
		let cfg = config::Settings::default();
		let (cols, lines, img_h) = (40, 10, 300);
		// about 0.1 px a line, and about 0.7
		for scrollback in [3000, 400] {
			let (mut term, mut parser) = live_term(cols, lines, scrollback);
			let mut rng = Rng::new(scrollback as u64);
			let mut text = String::new();
			for _ in 0..scrollback * 2 {
				text += &styled_line(&mut rng, cols);
				text += "\r\n";
			}
			parser.advance(&mut term, text.as_bytes());
			// a minute a step, well past the throttle
			let start = Instant::now();
			let update = |map: &mut Minimap, at: u64, cut: bool| {
				map.update(
					term.grid(),
					term.colors(),
					&cfg,
					16,
					img_h,
					1.0,
					lines,
					cols,
					0,
					0,
					false,
					cut,
					start + Duration::from_secs(at * 60),
				);
				map.wait();
			};
			let mut map = Minimap {
				budget: Some(50),
				here_px: Some(0),
				..Default::default()
			};
			update(&mut map, 0, true);
			assert!(map.rough_n > 0, "{scrollback}: nothing stood in");
			let mut at = 0;
			while map.pending() {
				at += 1;
				assert!(at < 1000, "{scrollback}: still {} stand-ins", map.rough_n);
				update(&mut map, at, false);
			}
			assert_eq!(map.rough_n, 0, "{scrollback}");
			let mut whole = Minimap::default();
			update(&mut whole, at, true);
			assert_eq!(whole.rough_n, 0);
			assert!(
				map.img == whole.img,
				"{scrollback}: not the image every line makes"
			);
		}
	}

	// The column's width can change between composes, and the image is still
	// the one composed at the old width. A caller that took the new width made
	// a texture the pixels could not fill, and wgpu killed the window.
	// Test ID: EqLzJNY
	#[test]
	fn the_map_reports_the_size_its_pixels_have() {
		let (cols, lines) = (20, 4);
		let (mut term, mut parser) = live_term(cols, lines, 50);
		parser.advance(&mut term, b"\rline of output\r\n");
		let cfg = config::Settings::default();
		let t0 = Instant::now();
		let mut map = Minimap::default();
		let build = |map: &mut Minimap, width: usize, at: u64| {
			let now = t0 + Duration::from_millis(at);
			map.update(
				term.grid(),
				term.colors(),
				&cfg,
				width,
				60,
				1.0,
				lines,
				cols,
				1,
				0,
				true,
				false,
				now,
			);
			let (px, w, h) = map.image();
			assert_eq!(px.len(), w * h * 4, "width {width} at {at} ms");
		};
		build(&mut map, 8, 0);
		// inside the wait, where nothing used to recompose
		build(&mut map, 20, 10);
		assert_eq!(map.image().1, 20, "the wider column is what shows");
	}

	// The plain per-cell pass the raster is written to be faster than. It keeps
	// no style memo and no span table, so a stale memo key, or a coverage table
	// made for another width, shows up as a difference.
	fn reference_row(
		grid: &Grid<Cell>,
		line: Line,
		colors: &Colors,
		cfg: &config::Settings,
		width: usize,
		cols: usize,
	) -> Row {
		let mut out = vec![0u8; width * 4];
		let mut rgb = vec![0f32; width * 3];
		let mut weight = vec![0f32; width];
		let mut readable = palette::Readable::default();
		let row = &grid[line];
		let per_cell = width as f32 / cols as f32;
		for c in 0..cols {
			let cell = &row[Column(c)];
			if blank(cell) {
				continue;
			}
			let style = Style {
				fg: cell.fg,
				bg: cell.bg,
				flags: cell.flags & (Flags::INVERSE | Flags::HIDDEN),
			};
			let (ink, alpha) = paint(&style, colors, cfg, &mut readable).at(ink_share(cell.c));
			let x0 = if c == 0 { -2.0 } else { c as f32 * per_cell };
			let x1 = if c + 1 == cols {
				width as f32 + 2.0
			} else {
				(c + 1) as f32 * per_cell
			};
			let first = (x0 - 1.0).max(0.0) as usize;
			let last = ((x1 + 1.0).ceil() as usize).min(width);
			for px in first..last {
				let cover = tent_over(x0, x1, px as f32 + 0.5);
				if cover <= SPAN_MIN {
					continue;
				}
				let w = cover * alpha;
				if w <= 0.0 {
					continue;
				}
				rgb[px * 3] += ink[0] as f32 * w;
				rgb[px * 3 + 1] += ink[1] as f32 * w;
				rgb[px * 3 + 2] += ink[2] as f32 * w;
				weight[px] += w;
			}
		}
		for px in 0..width {
			let w = weight[px];
			if w <= 0.0 {
				continue;
			}
			out[px * 4] = to_u8(rgb[px * 3] / w);
			out[px * 4 + 1] = to_u8(rgb[px * 3 + 1] / w);
			out[px * 4 + 2] = to_u8(rgb[px * 3 + 2] / w);
			out[px * 4 + 3] = to_u8(w.min(1.0) * 255.0);
		}
		out
	}

	// Test ID: EqLzJNZ
	#[test]
	fn a_rasterized_line_matches_a_plain_per_cell_pass() {
		let cfg = config::Settings::default();
		for seed in 0..24 {
			let mut rng = Rng::new(seed);
			let (cols, lines) = (4 + rng.below(60), 2 + rng.below(6));
			let width = 1 + rng.below(30);
			let (mut term, mut parser) = live_term(cols, lines, 10);
			for _ in 0..lines {
				let text = format!("{}\r\n", styled_line(&mut rng, cols));
				parser.advance(&mut term, text.as_bytes());
			}
			let mut map = Minimap {
				width,
				cols,
				..Default::default()
			};
			map.fit_spans();
			let mut readable = palette::Readable::default();
			for line in 0..lines as i32 {
				let mut got = map.take_row();
				map.fill(
					term.grid(),
					Line(line),
					term.colors(),
					&cfg,
					&mut readable,
					&mut got,
				);
				let want = reference_row(term.grid(), Line(line), term.colors(), &cfg, width, cols);
				assert!(
					got == want,
					"seed {seed}, line {line}, {cols} columns into {width} px"
				);
			}
		}
	}

	// A character is not a flat block. A period inks a fraction of its cell and
	// a hash most of it, and that difference is what stops a run of text
	// reading as one bar.
	// Test ID: EqMTH4y
	#[test]
	fn a_glyphs_weight_follows_how_much_it_inks() {
		let cfg = config::Settings::default();
		let (cols, width) = (40, 40); // one pixel per cell, so alpha arrives unmixed
		let strip = |text: &str| -> Row {
			let (mut term, mut parser) = fresh_term(cols, 4, 10);
			parser.advance(&mut term, text.as_bytes());
			let mut map = Minimap {
				width,
				cols,
				..Default::default()
			};
			map.fit_spans();
			let mut row = map.take_row();
			map.fill(
				term.grid(),
				Line(0),
				term.colors(),
				&cfg,
				&mut palette::Readable::default(),
				&mut row,
			);
			row
		};
		let run = |c: char| strip(&c.to_string().repeat(cols - 1))[3];
		let (dots, letters, hashes) = (run('.'), run('e'), run('#'));
		// the densest glyph inks its whole cell, so the column is no lighter
		// overall than it was under the flat model it replaced
		assert_eq!(hashes, 255);
		assert!(dots * 3 < hashes, "dots {dots}, hashes {hashes}");
		assert!(
			dots < letters && letters < hashes,
			"{dots} {letters} {hashes}"
		);

		// so a line of ordinary text is no longer one alpha across its pixels
		let text = strip("the quick brown fox. i, l; W#@ M.");
		let seen: Vec<u8> = (0..width).map(|px| text[px * 4 + 3]).collect();
		let inked: Vec<u8> = seen.iter().copied().filter(|&a| a > 0).collect();
		assert!(inked.len() > 20, "only {} inked pixels", inked.len());
		let (lo, hi) = (*inked.iter().min().unwrap(), *inked.iter().max().unwrap());
		assert!(hi - lo > 80, "alphas {lo}..{hi} are near enough flat");

		// a cell with its own background still paints solid, whatever is in it
		let bg = |text: &str| strip(&format!("\x1b[41m{text}"))[3];
		assert_eq!(bg(&" ".repeat(cols - 1)), 255);
		assert_eq!(bg(&"#".repeat(cols - 1)), 255);
	}

	// At the capped height a line's ink is a band narrower than a pixel, so it
	// falls across two pixel rows at part strength rather than filling one.
	// That is what a page of text looks like from across the room.
	// Test ID: EqMTH4z
	#[test]
	fn a_line_at_the_cap_is_softer_than_a_solid_row() {
		let width = 4;
		let rows: Vec<Row> = (0..60).map(|_| row(width, 4, [0, 200, 0])).collect();
		let mut map = Minimap::default();
		map.seed(width, rows);
		map.compose(300, 1.0);
		let lh = line_px(300.0, 60, 1.0);
		assert_eq!(lh, MAX_LINE_PX);
		let used = (lh * 60.0) as usize;
		let alphas: Vec<u8> = (0..used).map(|y| map.pixel(0, y)[3]).collect();
		// no pixel row of a solid page is blank, and not every one is solid
		assert!(alphas.iter().all(|&a| a > 0), "{alphas:?}");
		assert!(
			alphas.iter().any(|&a| a < 255),
			"a solid page with no texture"
		);
		// and it stays a page rather than fading out
		let mean = alphas.iter().map(|&a| a as u32).sum::<u32>() / used as u32;
		assert!(mean > 170, "mean alpha {mean}");
	}

	// Under a flood the eased view sits behind the newest output. The map stops
	// where the view has reached, so the column shows nothing the text has not.
	// With the ease at rest it draws the whole buffer again.
	// Test ID: EqN59w0
	#[test]
	fn the_map_stops_where_the_eased_text_has_reached() {
		let settings = config::Settings::default();
		let (cols, lines) = (40, 48);
		let (width, img_h) = (16, 300);
		// Short enough that lines draw at the capped height, so a trim actually
		// shortens the column rather than just compressing it less.
		let compose = |lag: usize| {
			let (mut term, mut parser) = live_term(cols, lines, 1000);
			parser.advance(&mut term, "x\r\n".repeat(100).as_bytes());
			parser.advance(&mut term, b"tail");
			let hist = term.grid().history_size();
			let mut map = Minimap::default();
			map.update(
				term.grid(),
				term.colors(),
				&settings,
				width,
				img_h,
				1.0,
				lines,
				cols,
				0,
				lag,
				true,
				true,
				Instant::now(),
			);
			(map, hist)
		};
		let ink_ends = |map: &Minimap| {
			(0..img_h)
				.rev()
				.find(|&y| map.pixel(0, y)[3] > 0)
				.map_or(0, |y| y + 1)
		};

		// at rest the map is the whole buffer, blank screen rows and all
		let (rest, hist) = compose(0);
		assert_eq!(rest.shown_lines(hist, lines), hist + lines);
		let whole = line_px(img_h as f32, hist + lines, 1.0);
		assert_eq!(whole, MAX_LINE_PX, "the buffer has to be short enough");

		// behind by 12 lines, the map is 12 lines shorter and ends sooner
		let (eased, hist) = compose(12);
		let shown = eased.shown_lines(hist, lines);
		assert_eq!(shown, hist + lines - 12);
		assert!(
			ink_ends(&eased) < ink_ends(&rest),
			"ink ends at {} either way",
			ink_ends(&eased)
		);
		let used = (line_px(img_h as f32, shown, 1.0) * shown as f32).ceil() as usize;
		assert!(ink_ends(&eased) <= used, "ink past the last drawn line");

		// and the marker still rides the whole buffer inside the shorter track
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: 400.0,
			h: img_h as f32,
		};
		let g = geom(
			full,
			0.0,
			1.0,
			&cfg(true, 60.0),
			hist + lines,
			shown,
			lines,
			0.0,
			false,
			true,
		)
		.unwrap();
		let handle = g.handle.unwrap();
		let track = line_px(img_h as f32, shown, 1.0) * shown as f32;
		assert!(
			(handle.y + handle.h - track).abs() < 0.01,
			"marker ends at {}, track at {track}",
			handle.y + handle.h
		);
		// and the whole scrollback is still reachable from the top of it
		let back = center_on(&g, hist + lines, shown, lines, full.y, 1.0);
		assert_eq!(back, hist as f32);
	}

	// The other reading of the same backlog sentence, which shipped for a day
	// and is out again (SR1): the map used to stop at the last screen row with
	// ink, so the blank rows under a short prompt took no track. They are part
	// of the buffer, and the marker reaches over them.
	// Test ID: EqQ3t6m
	#[test]
	fn blank_rows_under_a_prompt_are_part_of_the_map() {
		let settings = config::Settings::default();
		let (cols, lines) = (40, 48);
		let (width, img_h) = (16, 300);
		// 100 lines of output, then a clear - which pushes the screen it erases
		// into history - and a prompt alone on the top row.
		let (mut term, mut parser) = fresh_term(cols, lines, 1000);
		parser.advance(&mut term, "x\r\n".repeat(100).as_bytes());
		parser.advance(&mut term, b"\x1b[H\x1b[2J");
		parser.advance(&mut term, b"prompt");
		let hist = term.grid().history_size();
		let mut map = Minimap::default();
		map.update(
			term.grid(),
			term.colors(),
			&settings,
			width,
			img_h,
			1.0,
			lines,
			cols,
			0,
			0,
			true,
			true,
			Instant::now(),
		);
		let shown = map.shown_lines(hist, lines);
		assert_eq!(shown, hist + lines, "the blank rows are buffer too");

		// the track runs the whole buffer, well past where the ink stops
		let track = line_px(img_h as f32, shown, 1.0) * shown as f32;
		let inked_only = line_px(img_h as f32, hist + 1, 1.0) * (hist + 1) as f32;
		assert!(
			track > inked_only + 1.0,
			"track {track} no longer than the inked part {inked_only}"
		);

		// and the marker rides to the end of it, so the prompt is reachable
		let full = Rect {
			x: 0.0,
			y: 0.0,
			w: 200.0,
			h: img_h as f32,
		};
		let g = geom(
			full,
			0.0,
			1.0,
			&cfg(true, 60.0),
			hist + lines,
			shown,
			lines,
			0.0,
			false,
			true,
		)
		.unwrap();
		let handle = g.handle.unwrap();
		assert!(
			(handle.y + handle.h - track).abs() < 0.01,
			"marker ends at {}, track at {track}",
			handle.y + handle.h
		);
		assert_eq!(
			center_on(&g, hist + lines, shown, lines, full.y, 1.0),
			hist as f32
		);
	}

	// The ease drains whether or not more output arrives, so a map that came
	// out short has to ask for another compose or it stays short.
	// Test ID: EqN59w1
	#[test]
	fn a_trimmed_compose_needs_another() {
		let settings = config::Settings::default();
		let (cols, lines) = (40, 24);
		let composed = |lag: usize| {
			let (mut term, mut parser) = live_term(cols, lines, 500);
			parser.advance(&mut term, "x\r\n".repeat(60).as_bytes());
			let mut map = Minimap::default();
			map.update(
				term.grid(),
				term.colors(),
				&settings,
				16,
				300,
				1.0,
				lines,
				cols,
				0,
				lag,
				true,
				true,
				Instant::now(),
			);
			map
		};
		assert!(composed(6).pending(), "a short map owes a compose");
		assert!(!composed(0).pending(), "a whole map owes nothing");
		// a lag past the whole buffer still leaves a line, so the column stays
		let deep = composed(10_000);
		assert_eq!(deep.shown_lines(0, lines), 1);
	}

	// F155. A view parked in the scrollback freezes the lag, so no later compose
	// can draw anything new. Owing one anyway had `wake()` asking app.rs for a
	// frame and a full recompose about eleven times a second for as long as the
	// pane sat there, with no output and nobody touching it.
	// Test ID: EqQ9f4K
	#[test]
	fn a_parked_view_stops_asking_for_composes() {
		let _g = config::test_store_lock();
		config::update(config::Settings::default());
		let settings = config::Settings::default();
		let (cols, lines) = (40, 24);
		let (width, img_h) = (16, 300);

		// a flood at the bottom, then the user scrolls back and stays there
		let mut scroll = crate::scroll::Scroll::new();
		scroll.set_max(10_000.0);
		for _ in 0..60 {
			scroll.nudge_output(10.0, lines as f32);
			scroll.advance(0.016);
		}
		scroll.scroll_to(1000.0);
		for _ in 0..2000 {
			scroll.advance(0.016);
		}
		let lag = scroll.unshown_lines().round() as usize;
		assert!(lag > 100, "the flood left only {lag} lines unshown");
		assert!(
			!scroll.unshown_draining(),
			"parked, but the lag still drains"
		);

		let (mut term, mut parser) = live_term(cols, lines, 10_000);
		parser.advance(&mut term, "x\r\n".repeat(2000).as_bytes());
		let start = Instant::now();
		// Well past the throttle, whose gap is the larger of 90 ms and twenty
		// times the last compose. At a step near the floor this test read a
		// deferred compose as an owed one and went red on a busy box (F158).
		let drive = |map: &mut Minimap, draining: bool, step: u64| {
			map.update(
				term.grid(),
				term.colors(),
				&settings,
				width,
				img_h,
				1.0,
				lines,
				cols,
				0,
				lag,
				draining,
				false,
				start + Duration::from_secs(step * 5),
			);
		};

		// parked: one compose settles it and nothing more is owed, however long
		// the pane sits there
		let mut map = Minimap::default();
		for step in 0..50 {
			drive(&mut map, scroll.unshown_draining(), step);
			assert!(!map.pending(), "step {step}: a frozen lag owes a compose");
			assert!(map.wake().is_none(), "step {step}: and asks for a frame");
		}
		assert!(
			map.shown_lines(0, lines) < 2000,
			"the map was not trimmed at all"
		);

		// and the control: the same short map while the ease is still draining
		// does owe one, or it would never follow the ease down
		let mut draining = Minimap::default();
		drive(&mut draining, true, 0);
		assert!(draining.pending(), "a draining lag owes a compose");
		assert!(draining.wake().is_some(), "a draining lag asks for a frame");
	}

	// Test ID: EqLzJNa
	#[test]
	fn a_pixel_takes_the_nearest_byte() {
		assert_eq!(to_u8(0.4), 0);
		assert_eq!(to_u8(0.5), 1);
		assert_eq!(to_u8(127.6), 128);
		assert_eq!(to_u8(255.0), 255);
	}

	// Rasterizing late must not change what is drawn: every compose matches the
	// one a fresh cache makes from the same grid, whatever went by in between.
	// Test ID: EqLtc6M
	#[test]
	fn a_late_raster_composes_the_image_a_fresh_one_would() {
		let cfg = config::Settings::default();
		for seed in 0..40 {
			let mut rng = Rng::new(seed);
			let (cols, lines, scrollback) = (8 + rng.below(40), 2 + rng.below(8), rng.below(120));
			let (mut term, mut parser) = live_term(cols, lines, scrollback);
			let mut map = Minimap::default();
			let start = Instant::now();
			let mut ms = 0;
			let mut width = 1 + rng.below(12);
			let img_h = 10 + rng.below(200);
			for step in 0..100 {
				let mut text = String::new();
				// a history clear, then lines pushed after it in the same build
				if rng.chance(25) {
					text += "\x1b[3J";
				}
				let pushed = if rng.chance(8) {
					rng.below(3 * scrollback + 10)
				} else {
					rng.below(8)
				};
				for _ in 0..pushed {
					text += &styled_line(&mut rng, cols);
					text += "\r\n";
				}
				// the bottom row, which is screen and not history
				text += &styled_line(&mut rng, cols);
				parser.advance(&mut term, text.as_bytes());
				if rng.chance(30) {
					width = 1 + rng.below(12);
				}
				ms += rng.below(150) as u64;
				let now = start + Duration::from_millis(ms);
				let cut = rng.chance(40);
				let rev = map.rev;
				let lag = rng.below(lines + 8);
				let args = (width, img_h, 1.0, lines, cols);
				map.update(
					term.grid(),
					term.colors(),
					&cfg,
					args.0,
					args.1,
					args.2,
					args.3,
					args.4,
					pushed,
					lag,
					true,
					cut,
					now,
				);
				let (px, w, h) = map.image();
				assert_eq!(px.len(), w * h * 4, "seed {seed}, step {step}");
				if map.rev == rev {
					continue;
				}
				let mut fresh = Minimap::default();
				fresh.update(
					term.grid(),
					term.colors(),
					&cfg,
					args.0,
					args.1,
					args.2,
					args.3,
					args.4,
					0,
					lag,
					true,
					false,
					now,
				);
				assert!(map.img == fresh.img, "seed {seed}, step {step}");
			}
		}
	}

	// Below a pixel a line has no room for a gap, so it is taken whole and a
	// full page is as bright as it ever was.
	// Test ID: Eolyf2P
	#[test]
	fn a_line_under_a_pixel_keeps_its_whole_height() {
		assert_eq!(Minimap::band(0.4), (0.0, 0.4));
		assert_eq!(Minimap::band(0.5), (0.0, 0.5));
		// at the cap the ink is a band, so it falls across two pixel rows
		let (top, h) = Minimap::band(MAX_LINE_PX);
		assert!(top > 0.0 && h < MAX_LINE_PX * 0.75);
		assert!(
			top.fract() + h < 1.0,
			"band {top} + {h} should not fill a row"
		);
	}

	// SILK_MEMDBG's minimap figure, which the reducing resources doc quotes,
	// counts every stored row.
	// Test ID: Ern2oTz
	#[test]
	fn the_memory_count_covers_every_stored_row() {
		let width = 100;
		let mut map = Minimap::default();
		let empty = map.heap_bytes();
		map.seed(
			width,
			(0..1000).map(|_| row(width, 4, [0, 200, 0])).collect(),
		);
		assert!(map.heap_bytes() >= empty + 1000 * width * 4);
	}
}
