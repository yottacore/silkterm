// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::grid::{
	BidirectionalIterator, Dimensions, Grid, Row, Scroll as GridScroll,
};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::CursorShape;
use glyphon::cosmic_text::{LineEnding, Scroll as TextScroll};
use glyphon::{
	Attrs, AttrsList, Buffer, BufferLine, Color as GColor, CustomGlyph, Shaping, Style, TextArea,
	TextBounds, Weight,
};
use winit::event_loop::EventLoopProxy;

use crate::config;
use crate::gfx::RectInstance;
use crate::minimap::{self, Minimap};
use crate::palette;
use crate::scroll::Scroll;
use crate::term::{PaneId, TermInstance, UserEvent};
use crate::text::{TextCtx, mono_attrs};

// Pane ids must be unique across ALL tabs (each tab is a separate PaneManager),
// not just within one: the shell-exit event carries only the id, so a collision
// closes the wrong tab and cascades. Allocate from one global counter.
static PANE_ID_SEQ: AtomicU64 = AtomicU64::new(1);
fn alloc_pane_id() -> PaneId {
	PANE_ID_SEQ.fetch_add(1, Ordering::Relaxed)
}

// SILK_SCROLLDBG: per-frame app-scroll trace (dev-only, gated by the env var).
static DBG_FRAME: AtomicU64 = AtomicU64::new(0);
fn scroll_dbg() -> bool {
	use std::sync::OnceLock;
	static ON: OnceLock<bool> = OnceLock::new();
	*ON.get_or_init(|| std::env::var_os("SILK_SCROLLDBG").is_some())
}

// Cursor animation tunables (internal).
const CURSOR_MOVE_TAU_MS: f32 = 55.0; // horizontal slide responsiveness (lower = snappier)
// Catch-up: the slide's time-constant shrinks the farther the cursor trails its
// real column, so a fast burst/paste doesn't leave it lagging across the line
// (per-cell factor); and it never trails more than CURSOR_MAX_LAG cells.
const CURSOR_CATCHUP: f32 = 0.45; // tau divisor per cell of lag
const CURSOR_MAX_LAG: f32 = 8.0; // hard cap on how far behind the slide may sit (cells)
pub(crate) const CURSOR_ALPHA: f32 = 0.55; // solid block-cursor alpha; theme.rs tests cursor colors against it
// Escape hatch: true restores the old always-running animation (the removed
// cursor_animation_input = "continuous"), bypassing the pause/park machinery.
const CURSOR_ANIM_CONTINUOUS: bool = false;
// A cursor move this soon after the user sent input is that input's echo; a
// later move is the program's own output. Only echoed input carries the full
// resume delay - see resume_delay.
const TYPED_ECHO_S: f32 = 0.2;
// Resume delay for a pause that output caused: just enough stillness that a
// gap between two writes cannot unpark it, short enough to read as "the moment
// the prompt came back". The configured delay is for typing, not for output.
const OUTPUT_RESUME_S: f32 = 0.05;
// Freeze knob (one line rolls it back): only the focused pane of the focused
// window animates its cursor; every other pane parks at full size (same
// glide-to-full / resume-from-full machinery, so the size stays continuous).
const FREEZE_UNFOCUSED_BLINK: bool = true;
const BELL_BRIGHTEN: f32 = 0.6; // max lerp of text toward white at the bell flash peak

// Alt-screen app-scroll tunables.
const APP_SCROLL_MAX: usize = 24; // max per-step shift the fingerprint slide detector will believe
// Whether the smooth slide engages for full-screen apps that keep a static TOP
// band (title bar: nano, muffer). Was off while the reveal strip was filled from
// a single retained frame: the strip could under-fill by the ease lag and its
// re-capture repositioned it every step - the band/scrim bounce. The scrolled-off
// strip (OffStrip below) fills the gap exactly and never repositions, so the
// slide is on for top-band apps again. Apps that fill from the top with only a
// bottom status line (less, vim) have no top band and slide regardless.
const SLIDE_TOP_BAND_APPS: bool = true;
// Whether a slide DOWN into rows that were blank to the bottom of the screen
// eases. That is content making room for itself, not a scroll: a paste growing
// an input box pushes the footer under it into empty rows, and sliding it drops
// the new lines out from behind the rows above, last line first. A terminal
// just shows them, so they pop in. True brings the slide back.
const SLIDE_DOWN_INTO_ROOM: bool = false;

const PROMPT_ABOVE_MAX: usize = 4; // rows above the prompt considered for multi-line-prompt learning
// Skeleton segments a row must carry before it can be learned as a prompt row.
// The skeleton collapses every run of alphanumerics to one 'a' - that is what
// lets a prompt's clock and cwd change without breaking the match - so EVERY
// one-word row hashes alike. Two commands in a row that each printed a single
// word therefore taught the learner that the row above the input line was
// prompt, and the strip then ate the real last line of every copy after that
// (measured driving PowerShell: two of five commands copied nothing at all).
// The bias has to run this way round: a row wrongly learned as prompt DELETES
// output, while one wrongly left as output only puts a prompt line on the
// clipboard.
const PROMPT_SKEL_MIN: usize = 6;

// Consecutive frames that may reuse the last built frame before build() stops
// trying and waits for the terminal (see the lock in build). 2 keeps the pane
// under ~3 frames stale while costing nothing when nothing is contending.
const LOCK_WAIT_AFTER: u32 = 2;

// The two independent auto-copy triggers a pane can have on (session-only, never
// persisted). Each is a per-pane bool; the enum just names which one a UI action
// or menu row refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyKind {
	Select, // copy the highlighted selection the moment a select finishes
	Output, // copy a command's output once the pane settles back at the prompt
}

// One styled cell captured for the scrolled-off strip. Colors are resolved at
// capture time (the palette/theme can change later; the strip shows what was on
// screen). `wide` is the cell count: 0 = wide-char spacer (skip), 1, or 2.
#[derive(Clone, Copy, PartialEq, Debug)]
struct StripCell {
	c: char,
	fg: [u8; 3],
	bg: Option<[u8; 3]>,
	bold: bool,
	italic: bool,
	wide: u8,
}

// Scrolled-off strip: the rows a scroll pushed out of an app's region, retained
// styled and in visual order (top to bottom) so the slide can draw them in the
// gap it reveals. The strip is welded to the content edge and grows by exactly
// each step's shift while app_off grows by the same amount, so the gap is always
// exactly filled - no under-fill, no re-capture jump, and no furniture bleed
// (only region rows are ever captured). `dir`: +1 = strip above the content
// (content moved up), -1 = below. `region` is the screen rows (top, one past
// the bottom) the rows came from; a step from another region starts over.
// `shaped` is the run of rows currently in strip_buf (see shape_strip).
struct OffStrip {
	rows: std::collections::VecDeque<Vec<StripCell>>,
	dir: i8,
	region: (usize, usize),
	shaped: std::ops::Range<usize>,
}

impl OffStrip {
	// the most a slide can lag, so older rows are invisible
	const CAP: usize = crate::scroll::SLIDE_ROWS;

	fn new() -> Self {
		Self {
			rows: std::collections::VecDeque::new(),
			dir: 0,
			region: (0, 0),
			shaped: 0..0,
		}
	}

	fn len(&self) -> usize {
		self.rows.len()
	}

	fn clear(&mut self) {
		self.rows.clear();
		self.dir = 0;
		self.shaped = 0..0;
	}

	// Append the rows a step pushed off `region` (`chunk` in visual order). A
	// direction flip or a region sharing no row with this one discards the old
	// strip. A region that overlaps carries on with the rows it has: the ledger
	// narrows its region to what both scrolls moved and records what crossed
	// that edge, so the strip stays welded where it was (nano's edit window,
	// which ncurses extends over the blank status row every so often).
	fn push_step(&mut self, dir: i8, region: (usize, usize), chunk: Vec<Vec<StripCell>>) {
		let overlap = region.0 < self.region.1 && self.region.0 < region.1;
		if self.dir != dir || !overlap {
			self.clear();
			self.dir = dir;
		}
		self.region = region;
		if dir > 0 {
			// content moved up: rows left off the top of the region, the newest
			// chunk nearest the content = at the strip's bottom
			self.rows.extend(chunk);
			while self.rows.len() > Self::CAP {
				self.rows.pop_front();
			}
		} else {
			// content moved down: rows left off the bottom, the newest chunk at
			// the strip's top (nearest the content), keeping its internal order
			for row in chunk.into_iter().rev() {
				self.rows.push_front(row);
			}
			while self.rows.len() > Self::CAP {
				self.rows.pop_back();
			}
		}
	}

	// The rows a slide of `app_off` lines can show: those nearest the content,
	// one more than the offset covers so the part-row at the far edge is drawn.
	fn visible(&self, app_off: f32) -> std::ops::Range<usize> {
		let want = (app_off.abs().ceil() as usize + 1).min(self.rows.len());
		if app_off > 0.0 {
			self.rows.len() - want..self.rows.len()
		} else {
			0..want
		}
	}
}

// The term for this frame, or None to reuse the last one: an unfair try while
// the misses stay under LOCK_WAIT_AFTER, then the fair lock (see build).
fn lock_for_frame<'a, T>(
	term: &'a FairMutex<T>,
	misses: &mut u32,
) -> Option<impl std::ops::DerefMut<Target = T> + use<'a, T>> {
	let guard = if *misses >= LOCK_WAIT_AFTER {
		term.lock()
	} else if let Some(guard) = term.try_lock_unfair() {
		guard
	} else {
		*misses += 1;
		crate::perf::bump(&crate::perf::LOCK_MISS);
		return None;
	};
	*misses = 0;
	Some(guard)
}

// Lines that entered the scrollback between two depth samples. A DROP can only
// mean the buffer was cleared (`clear`'s E3), and everything left in it arrived
// after that, so the whole of it is new; anything else is ordinary growth. The
// count alone can't distinguish a clear-and-refill that ends on the same depth,
// which is why this is sampled per PTY read cycle rather than per frame.
fn pushed_since(history: usize, baseline: usize) -> usize {
	if history < baseline {
		history
	} else {
		history - baseline
	}
}

// The scrollback depth as the PTY wakeups and the builds both sample it, against
// one baseline: whichever reaches the grid first banks the growth, and the other
// finds nothing left.
#[derive(Default)]
struct DepthSampler {
	hist: usize,
	pushed: usize,
}

impl DepthSampler {
	fn sample(&mut self, depth: usize) {
		self.pushed += pushed_since(depth, self.hist);
		self.hist = depth;
	}

	// A build's sample: everything banked so far plus its own, handed over.
	fn take(&mut self, depth: usize) -> usize {
		self.sample(depth);
		std::mem::take(&mut self.pushed)
	}

	fn rebaseline(&mut self, depth: usize) {
		self.pushed = 0;
		self.hist = depth;
	}
}

// The rows a detected step pushed out of the scroll region, as a range into the
// PREVIOUS frame's rows. shift > 0 = content moved up, rows left off the top of
// the region (just under any title band); shift < 0 = off the bottom.
fn vanished_range(shift: i32, st: usize, sb: usize, lines: usize) -> std::ops::Range<usize> {
	let region_top = st.min(lines);
	let region_bot = lines.saturating_sub(sb).max(region_top);
	let k = (shift.unsigned_abs() as usize).min(region_bot - region_top);
	if shift > 0 {
		region_top..region_top + k
	} else {
		region_bot - k..region_bot
	}
}

// The slide's region clip: band boundaries tightened to the shifted content's
// extent. The gap between a band and the content edge belongs to the strip;
// without the weld, band rows translated by voff render inside the band clip
// as ghost copies (see the Slide doc).
fn weld_region_clip(
	top_split_y: f32,
	split_y: f32,
	content_top_y: f32,
	content_bot_y: f32,
) -> (f32, f32) {
	(top_split_y.max(content_top_y), split_y.min(content_bot_y))
}

// Fingerprint every visible row (FNV-1a over the chars) and, when `styled` is
// given, snapshot the styled cells too - the scrolled-off strip's source data.
// Colors resolve the same way build()'s cell loop does (minus the transient
// bell flash and selection, which don't belong in a retained row). Recycles the
// caller's row allocations. One entry per column; a wide-char spacer stays as a
// wide=0 placeholder so indexes keep matching columns.
fn snapshot_rows(
	grid: &Grid<Cell>,
	lines: usize,
	cols: usize,
	styled: Option<(&Colors, &config::Settings, &mut Vec<Vec<StripCell>>)>,
) -> Vec<u64> {
	let mut rows: Vec<u64> = Vec::with_capacity(lines);
	let mut styled = styled;
	let mut readable = palette::Readable::default();
	if let Some((_, _, out)) = &mut styled {
		out.resize_with(lines, Vec::new);
	}
	for i in 0..lines as i32 {
		let row = &grid[Line(i)];
		let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
		for c in 0..cols {
			hash = (hash ^ row[Column(c)].c as u64).wrapping_mul(0x100_0000_01b3);
		}
		rows.push(hash);
		if let Some((colors, settings, out)) = &mut styled {
			let out_row = &mut out[i as usize];
			out_row.clear();
			out_row.extend(
				(0..cols).map(|c| strip_cell(&row[Column(c)], colors, settings, &mut readable)),
			);
		}
	}
	rows
}

// One styled cell for the strip. Colors resolve the way build()'s cell loop
// does, minus the transient bell flash and selection, which don't belong in a
// retained row.
fn strip_cell(
	cell: &Cell,
	colors: &Colors,
	settings: &config::Settings,
	readable: &mut palette::Readable,
) -> StripCell {
	let flags = cell.flags;
	if flags.contains(Flags::WIDE_CHAR_SPACER) {
		return StripCell {
			c: ' ',
			fg: [0; 3],
			bg: None,
			bold: false,
			italic: false,
			wide: 0,
		};
	}
	let mut fg = palette::resolve(cell.fg, colors, settings);
	let mut cell_bg = palette::resolve(cell.bg, colors, settings);
	if flags.contains(Flags::INVERSE) {
		std::mem::swap(&mut fg, &mut cell_bg);
	}
	if flags.contains(Flags::HIDDEN) {
		fg = cell_bg;
	}
	if flags.contains(Flags::DIM) {
		fg = [
			fg[0] / 2 + fg[0] / 4,
			fg[1] / 2 + fg[1] / 4,
			fg[2] / 2 + fg[2] / 4,
		];
	}
	fg = readable.get(fg, cell_bg, settings.text_min_contrast);
	StripCell {
		c: cell.c,
		fg,
		bg: (cell_bg != settings.bg).then_some(cell_bg),
		bold: flags.contains(Flags::BOLD)
			|| (settings.embolden_inverse && flags.contains(Flags::INVERSE)),
		italic: flags.contains(Flags::ITALIC),
		wide: if flags.contains(Flags::WIDE_CHAR) {
			2
		} else {
			1
		},
	}
}

// The engine's record of the rows a scroll pushed off its region, styled for
// the strip. A row of another width (a resize in between) is left out.
fn strip_rows(
	rows: &std::collections::VecDeque<Row<Cell>>,
	cols: usize,
	colors: &Colors,
	settings: &config::Settings,
) -> Vec<Vec<StripCell>> {
	let mut readable = palette::Readable::default();
	rows.iter()
		.filter(|row| row.len() == cols)
		.map(|row| {
			(0..cols)
				.map(|c| strip_cell(&row[Column(c)], colors, settings, &mut readable))
				.collect()
		})
		.collect()
}

// Whether to draw a cursor at all, and as what. DECTCEM (CSI ?25 l/h) lives in
// the terminal MODE - `cursor_style()` reports the shape an app asked for and
// says nothing about whether the app wants one drawn - so both have to be asked,
// the way the engine's own renderable content does. Miss this and a TUI that
// hides the cursor to repaint still gets one painted wherever the paint left it.
fn shown_cursor_shape(mode: TermMode, shape: CursorShape) -> CursorShape {
	if mode.contains(TermMode::SHOW_CURSOR) {
		shape
	} else {
		CursorShape::Hidden
	}
}

// The rendered cursor geometry as (width, height) fractions of the cell. An
// app-set Beam/Underline (DECSCUSR) maps to a thin bar / underline; a plain Block
// uses the configured cursor_size_* - except on the alt screen, where the app
// (vim, less, ...) owns a full block.
fn cursor_geometry(shape: CursorShape, alt_screen: bool) -> (f32, f32) {
	match shape {
		CursorShape::Beam => (0.15, 1.0),      // thin vertical bar
		CursorShape::Underline => (1.0, 0.15), // thin bottom strip
		_ if alt_screen => (1.0, 1.0),         // alt-screen app owns its block cursor
		_ => {
			let settings = config::settings();
			(
				(settings.cursor_size_width / 100.0).clamp(0.02, 1.0), // width, from left
				(settings.cursor_size_height / 100.0).clamp(0.02, 1.0), // height, from bottom
			)
		}
	}
}

// Pulse envelope over one cycle: grow, hold full, shrink, then a brief disappear.
fn pulse_env(phase: f32) -> f32 {
	let smooth = |t: f32| {
		let t = t.clamp(0.0, 1.0);
		t * t * (3.0 - 2.0 * t)
	};
	if phase < 0.40 {
		smooth(phase / 0.40) // grow 0 -> 1
	} else if phase < 0.60 {
		1.0 // hold at full
	} else if phase < 0.90 {
		1.0 - smooth((phase - 0.60) / 0.30) // shrink 1 -> 0
	} else {
		0.0 // disappear momentarily
	}
}

// What the animation makes of the cursor at `phase` of its cycle: the (width,
// height) fractions, the alpha, and whether each axis pulses. "phase" fades
// the alpha on a cosine; the pulses scale an axis by `pulse_env`; "none" and
// anything unknown leave the cursor as it is.
fn cursor_envelope(anim: &str, phase: f32, geom: (f32, f32)) -> (f32, f32, f32, bool, bool) {
	let (mut w_frac, mut h_frac) = geom;
	let mut alpha = CURSOR_ALPHA;
	let (pulsing_w, pulsing_h) = match anim {
		"phase" => {
			alpha = CURSOR_ALPHA * (0.5 + 0.5 * (phase * std::f32::consts::TAU).cos());
			(false, false)
		}
		"pulse_vertical" => {
			h_frac *= pulse_env(phase);
			(false, true)
		}
		"pulse_horizontal" => {
			w_frac *= pulse_env(phase);
			(true, false)
		}
		"pulse_both" => {
			let envelope = pulse_env(phase);
			w_frac *= envelope;
			h_frac *= envelope;
			(true, true)
		}
		_ => (false, false),
	};
	(w_frac, h_frac, alpha, pulsing_w, pulsing_h)
}

// Lerp a text color toward white by `t` (0..1) of the BELL_BRIGHTEN ceiling, for
// the visual-bell flash. Identity at t<=0.
fn bell_brighten(color: [u8; 3], t: f32) -> [u8; 3] {
	if t <= 0.0 {
		return color;
	}
	let t = (t * BELL_BRIGHTEN).clamp(0.0, 1.0);
	let up = |v: u8| (v as f32 + (255.0 - v as f32) * t).round() as u8;
	[up(color[0]), up(color[1]), up(color[2])]
}

// FNV-1a over a row's chars: the fingerprint copy-output uses to re-find the
// arm-time prompt row at capture time (same constants as build()'s inline rows).
// How long to leave a running command alone before asking again whether it has
// finished. Matches the throttle the tab-title work already uses; on unix each
// ask costs a `tcgetpgrp`.
const CAPTURE_RETRY: std::time::Duration = std::time::Duration::from_millis(250);

// When the next poll is worth doing. Both halves matter: the settle window, and
// a floor a busy poll pushes forward. Without the floor the deadline sat in the
// past for the whole life of a command that printed nothing, and the event loop
// woke on it every pass.
fn next_capture_poll(
	last_output: std::time::Instant,
	settle: std::time::Duration,
	retry_at: std::time::Instant,
) -> std::time::Instant {
	(last_output + settle).max(retry_at)
}

fn fnv_row(chars: impl Iterator<Item = char>) -> u64 {
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for c in chars {
		hash = (hash ^ c as u64).wrapping_mul(0x100_0000_01b3);
	}
	hash
}

// Skeleton fingerprint for prompt learning/stripping: runs of alphanumerics
// collapse to one marker and runs of spaces to one space, so a prompt row whose
// content changes per command (cwd, git branch, clock, right-aligned segments)
// still prints the same while its punctuation/box-drawing structure must match
// exactly. An exact-content compare here misses every dynamic multi-line
// prompt, which then gets copied as output.
fn fnv_row_skel(chars: impl Iterator<Item = char>) -> (u64, usize) {
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	let mut last = '\0';
	let mut segments = 0;
	for c in chars {
		let k = if c.is_alphanumeric() { 'a' } else { c };
		if (k == 'a' || k == ' ') && k == last {
			continue;
		}
		last = k;
		segments += 1;
		hash = (hash ^ k as u64).wrapping_mul(0x100_0000_01b3);
	}
	(hash, segments)
}

// The char to feed the shaper for a grid cell. A cell may hold a literal control
// char - alacritty leaves the '\t' in the first tab cell and fills to the tab
// stop with spaces - and cosmic-text shapes a raw tab as a full 8-col stop,
// shifting the rest of the row off the col*cell_w grid so the visible text no
// longer lines up with the selectable cells (misaligned double-click on tabbed
// output like `zpool status`). The cell already carries the padding, so render
// any control char as a plain 1-cell space.
fn render_char(c: char) -> char {
	if c.is_control() { ' ' } else { c }
}

// Edge autoscroll while a selection is being dragged. Lines a second right at
// the edge, plus this much more for every cell the pointer is held past it, up
// to the cap. The creep at the edge is what lets a careful drag pick up one more
// line without overshooting; the cap stops a pointer flung to the top of the
// screen from crossing the whole buffer before the button comes up.
const EDGE_SCROLL_MIN: f32 = 3.0;
const EDGE_SCROLL_PER_CELL: f32 = 9.0;
const EDGE_SCROLL_MAX_CELLS: f32 = 6.0;
// Seconds of holding past the edge to reach the same speed as pushing the
// pointer the full distance. A maximized window has its top edge against the top
// of the screen, so there is nowhere to push the pointer and distance alone
// would leave that window creeping forever.
const EDGE_SCROLL_RAMP_S: f32 = 2.0;

// Lines a second the view should crawl while a drag-selection is held past the
// top (positive, back into history) or bottom (negative) of its pane. Zero while
// the pointer is inside. `held_s` is how long it has been past the edge.
pub fn edge_scroll_rate(y: f32, top: f32, bottom: f32, cell_h: f32, held_s: f32) -> f32 {
	let over = if y < top {
		top - y
	} else if y > bottom {
		bottom - y
	} else {
		return 0.0;
	};
	let cells = (over.abs() / cell_h.max(1.0)).min(EDGE_SCROLL_MAX_CELLS);
	let ramp = (held_s / EDGE_SCROLL_RAMP_S).clamp(0.0, 1.0) * EDGE_SCROLL_MAX_CELLS;
	(EDGE_SCROLL_MIN + cells.max(ramp) * EDGE_SCROLL_PER_CELL) * over.signum()
}

// Rows a hyperlink may span. A logical line can be the entire scrollback (one
// `cat` of a huge line wraps forever) and this scan runs per pointer move, so
// the wrap walk is capped instead of following the line to its real ends.
const LINK_WRAP_ROWS: i32 = 4;

// The link under window pixel (px, py), as a grid span. `display_offset` and the
// grid come from the frame being built, so the answer can't disagree with what
// is on screen. Strict bounds, unlike point_at's clamping: the pointer sitting
// in the margin is over no cell at all, not over the first one.
#[allow(clippy::too_many_arguments)]
fn link_at(
	grid: &Grid<Cell>,
	colors: &Colors,
	settings: &config::Settings,
	rect: Rect,
	px: f32,
	py: f32,
	metrics: (f32, f32, f32), // cell_w, cell_h, margin
	dims: (usize, usize),     // cols, lines
	display_offset: i32,
) -> Option<LinkHit> {
	let (cell_w, cell_h, margin) = metrics;
	let (cols, lines) = dims;
	if cols == 0 || lines == 0 || cell_w <= 0.0 || cell_h <= 0.0 {
		return None;
	}
	let (rel_x, rel_y) = (px - rect.x - margin, py - rect.y - margin);
	if rel_x < 0.0 || rel_y < 0.0 {
		return None;
	}
	let col = (rel_x / cell_w).floor() as usize;
	let screen_row = (rel_y / cell_h).floor() as i32;
	if col >= cols || screen_row < 0 || screen_row >= lines as i32 {
		return None;
	}
	let line = screen_row - display_offset;
	let (top, bot) = (-(grid.history_size() as i32), lines as i32 - 1);
	if line < top || line > bot {
		return None;
	}
	// A soft-wrapped URL is one logical line, so the scan spans the wrap.
	let end_col = Column(cols - 1);
	let wraps = |l: i32| grid[Line(l)][end_col].flags.contains(Flags::WRAPLINE);
	let mut first_line = line;
	while first_line > top && line - first_line < LINK_WRAP_ROWS && wraps(first_line - 1) {
		first_line -= 1;
	}
	let mut last_line = line;
	while last_line < bot && last_line - line < LINK_WRAP_ROWS && wraps(last_line) {
		last_line += 1;
	}
	let rows = (last_line - first_line + 1) as usize;
	let mut text = Vec::with_capacity(rows * cols);
	for l in first_line..=last_line {
		let row = &grid[Line(l)];
		text.extend((0..cols).map(|c| render_char(row[Column(c)].c)));
	}
	let hit = (line - first_line) as usize * cols + col;
	let (start, end, url) = crate::links::find_at(&text, hit)?;
	let point_of = |i: usize| Point::new(Line(first_line + (i / cols) as i32), Column(i % cols));
	let start_pt = point_of(start);
	let cell = &grid[start_pt.line][start_pt.column];
	// Underline in the link's own color, so a colored URL keeps its identity -
	// including the minimum-contrast lift, or the rule would sit off the text.
	let (fg, bg) = if cell.flags.contains(Flags::INVERSE) {
		(cell.bg, cell.fg)
	} else {
		(cell.fg, cell.bg)
	};
	let fg = palette::readable(
		palette::resolve(fg, colors, settings),
		palette::resolve(bg, colors, settings),
		settings.text_min_contrast,
	);
	Some(LinkHit {
		url,
		start: start_pt,
		end: point_of(end - 1),
		fg,
	})
}

// Same spans at a single weight - what the scrim's de-bolded buffer wants. Derived
// on demand rather than built alongside the real list, so a screen with no bold on
// it pays nothing.
fn debold_attrs(src: &AttrsList, weight: Weight) -> AttrsList {
	let mut out = AttrsList::new(&src.defaults());
	for (range, attrs) in src.spans_iter() {
		let mut plain = attrs.as_attrs();
		plain.weight = weight;
		out.add_span(range.clone(), &plain);
	}
	out
}

// Assign one built row per buffer line. `set_text` compares against what the line
// already holds and only drops its cached shaping when something differs, so an
// unchanged row costs a string compare instead of a re-shape. Mirrors what
// set_rich_text does around the lines themselves (reset scroll, no alignment).
fn set_buffer_rows<'a>(
	buf: &mut Buffer,
	rows: impl ExactSizeIterator<Item = (&'a str, AttrsList)>,
) {
	let count = rows.len();
	for (i, (text, attrs)) in rows.enumerate() {
		if let Some(line) = buf.lines.get_mut(i) {
			line.set_text(text, LineEnding::default(), attrs);
		} else {
			buf.lines.push(BufferLine::new(
				text,
				LineEnding::default(),
				attrs,
				Shaping::Advanced,
			));
		}
	}
	buf.lines.truncate(count);
	buf.set_scroll(TextScroll::default());
}

// One step of the cursor's horizontal slide toward `target` (visual columns).
// Exponential easing whose time-constant shrinks with the gap, so the cursor
// speeds up the farther it trails its real column (a burst/paste catches up
// instead of dragging across the line) while a single-cell move keeps the gentle
// slide - plus a hard cap so it never sits more than CURSOR_MAX_LAG cells behind.
fn cursor_slide_step(cursor_x: f32, target: f32, dt: f32) -> f32 {
	let gap = (target - cursor_x).abs();
	let tau = CURSOR_MOVE_TAU_MS / (1.0 + gap * CURSOR_CATCHUP);
	let mut next = cursor_x + (target - cursor_x) * (1.0 - (-dt * 1000.0 / tau).exp());
	let lag = target - next;
	if lag.abs() > CURSOR_MAX_LAG {
		next = target - CURSOR_MAX_LAG * lag.signum();
	}
	next
}

// Advance the blink phase one frame toward `full_phase` (the point in the cycle
// where the cursor is full-size) at its normal speed. Returns the new blink_t
// and whether it reached full this step.
fn glide_to_full(blink_t: f32, dt: f32, period: f32, full_phase: f32) -> (f32, bool) {
	let prev = (blink_t / period).fract();
	let next = blink_t + dt;
	let now = (next / period).fract();
	let crossed = if prev <= now {
		prev <= full_phase && full_phase <= now
	} else {
		full_phase >= prev || full_phase <= now // wrapped past 1.0 this step
	};
	if crossed {
		(full_phase * period, true)
	} else {
		(next, false)
	}
}

// One animation cycle: (period seconds, the phase where the cursor is at its
// largest). That full-size phase is the only point a pause ever parks at and
// the only point a resume ever starts from - "phase" fades from full at 0, the
// pulses peak mid-cycle. Both the pause and the refocus resume read it here so
// they cannot drift apart.
fn cursor_cycle(anim: &str, blink_rate_ms: f32) -> (f32, f32) {
	let period = (blink_rate_ms / 1000.0 * 2.0).max(0.05); // full on->off->on
	let full_phase = if anim == "phase" { 0.0 } else { 0.5 };
	(period, full_phase)
}

// Was a cursor move the echo of user input, or the program's own output? The
// echo of a keystroke arrives within a frame or two of the write; anything later
// belongs to whatever is running.
fn move_is_input(typed_at: Option<std::time::Instant>, now: std::time::Instant) -> bool {
	typed_at.is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() < TYPED_ECHO_S)
}

// How long a pause holds at full size before the cycle resumes. Typing gets the
// configured delay, so the cursor stays still between keystrokes. Output gets
// almost none: a command scrolling the screen is not the user doing something,
// so once it stops - the prompt is back - the cursor comes straight back to life.
fn resume_delay(by_input: bool, configured_s: f32) -> f32 {
	if by_input {
		configured_s
	} else {
		OUTPUT_RESUME_S
	}
}

// Seconds until a parked cursor resumes on its own, or None when no timer may
// end the park: a long-idle stop or a pane that is not the active one waits for
// activity or focus instead, and a cursor that is not parked needs no wake.
fn cursor_wake_after(
	parked: bool,
	idle_stopped: bool,
	blocked: bool,
	resume_s: f32,
	idle_t: f32,
	hold_t: f32,
) -> Option<f32> {
	(parked && !idle_stopped && !blocked)
		.then(|| (resume_s - idle_t).max(resume_s - hold_t).max(0.0) + 0.02)
}

// Cursor animation pause. On input (or long idle) the cycle keeps running at
// its normal speed until it next reaches the full-size phase, parks there,
// then resumes the cycle from that same point - so the size is continuous at
// every step: no snap to full on a keystroke, and no snap to small on resume
// even when the glide outlasts the idle window (slow blink rates). While
// parked no frames flow (the CPU win), so the timers take a wall-clock dt.
#[derive(Default)]
struct PauseState {
	active: bool, // an episode is in progress (gliding or holding)
	parked: bool, // reached the full-size phase, holding there
	hold_t: f32,  // seconds parked at full (wall clock)
}

impl PauseState {
	#[allow(clippy::too_many_arguments)] // private helper; the inputs are the whole story
	fn advance(
		&mut self,
		blink_t: f32,
		dt: f32,
		wall_dt: f32,
		period: f32,
		full_phase: f32,
		resume_s: f32,
		idle_stop_s: f32,
		moved: bool,
		idle_t: f32,
		input_idle_t: f32,
		blocked: bool,
	) -> f32 {
		// past the long-idle threshold the animation stops outright: park (via
		// the same glide, so it stops at full) and stay parked until activity.
		// A blocked pane (not the focused pane of the focused window) parks the
		// same way and holds until it is active again - nothing times it out.
		// The threshold is measured from the last input, not from the cursor's
		// own stillness: a clock in a prompt or a TUI redrawing on its own timer
		// moves the cursor every few seconds, and reading that as the user being
		// here left the animation running for as long as the program did.
		let idle_stopped = idle_stop_s > 0.0 && input_idle_t >= idle_stop_s;
		let held = idle_stopped || blocked;
		if (moved || held) && !self.active {
			self.active = true;
			self.parked = false;
			self.hold_t = 0.0;
		}
		if !self.active {
			return blink_t + dt;
		}
		if !self.parked {
			let (next, parked) = glide_to_full(blink_t, dt, period, full_phase);
			self.parked = parked;
			return next;
		}
		// parked: hold at full until input has been idle AND the hold has lasted
		// the resume delay (a long glide can eat the whole idle window). A cursor
		// move resets idle_t, so a long-idle park then resumes the resume delay
		// after it; a refocus skips the wait entirely via resume().
		self.hold_t += wall_dt;
		if !held && self.hold_t >= resume_s && idle_t >= resume_s {
			self.active = false;
			return full_phase * period + dt; // resume the cycle from full
		}
		full_phase * period
	}

	// End the episode now instead of waiting out the resume delay. Only for a
	// refocus, where the caller also parks blink_t at the full-size phase - so
	// this still resumes from full, like every other resume.
	fn resume(&mut self) {
		self.active = false;
		self.parked = false;
		self.hold_t = 0.0;
	}
}

// Expand `line` up and down across soft-wrapped rows, clamped to [top, bot].
// `wrapped(l)` is true when grid row l's last cell carries WRAPLINE (the logical
// line continues into row l+1). Returns the (first, last) grid row of the whole
// logical line - used for triple-click line selection.
fn logical_line_bounds(line: i32, top: i32, bot: i32, wrapped: impl Fn(i32) -> bool) -> (i32, i32) {
	let line = line.clamp(top, bot);
	let mut start = line;
	while start > top && wrapped(start - 1) {
		start -= 1;
	}
	let mut end = line;
	while end < bot && wrapped(end) {
		end += 1;
	}
	(start, end)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
	pub x: f32,
	pub y: f32,
	pub w: f32,
	pub h: f32,
}

impl Rect {
	pub fn contains(&self, x: f32, y: f32) -> bool {
		x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
	// children laid out left | right
	Vertical,
	// children laid out top / bottom
	Horizontal,
}

// ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
// Scrollbar
// ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

// Seconds the bar stays up after the last user scroll before it starts fading.
const BAR_HOLD_S: f32 = 1.1;
// Fade time constants: appearing is quick, leaving is unhurried.
const BAR_IN_TAU_S: f32 = 0.06;
const BAR_OUT_TAU_S: f32 = 0.22;
// Below this the bar is treated as fully gone: no quads, and no hit-testing (so
// a faded-out bar never steals a click meant for the text under it).
const BAR_VISIBLE_EPS: f32 = 0.02;
// Shortest thumb, as a multiple of the bar's thickness. A huge scrollback would
// otherwise grind the thumb down to a sliver nobody can grab.
const BAR_MIN_THUMB: f32 = 1.6;
// How far outside the bar the pointer still counts as "near" it, so the bar
// fades in slightly before you get there rather than under the cursor. DIP, like
// the configured thickness it widens.
const BAR_HOVER_SLOP: f32 = 6.0;

// Where a press hit the scrollbar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarHit {
	// on the handle: `f32` is the grab offset from the thumb's top edge
	Thumb,
	// on the track above/below the thumb: pages toward the click
	TrackUp,
	TrackDown,
}

// A pane's scrollbar geometry for this frame, in absolute window px.
#[derive(Clone, Copy, Debug)]
pub struct Bar {
	pub track: Rect,
	pub thumb: Rect,
}

// Is a scrollbar meaningful for a pane in this state? A full-screen app owns its
// screen and keeps no scrollback of its own, so there is nothing a bar could
// report - and one pinned full-height would only be a lie. Nothing to scroll,
// same answer. Free fn so the rule is testable without a live PTY.
fn bar_applies_to(cfg: &config::Settings, alt: bool, max_lines: f32) -> bool {
	cfg.scrollbar && !alt && max_lines > 0.0
}

// The bar's track: the far right edge of the pane's whole area, so past the
// minimap column when there is one, the height of the text's content area.
fn bar_track(full: Rect, rect: Rect, thickness: f32, margin: f32) -> Rect {
	Rect {
		x: full.x + full.w - thickness,
		y: rect.y + margin,
		w: thickness,
		h: (rect.h - 2.0 * margin).max(0.0),
	}
}

// Thumb length and position for a scroll state. Split out from the pane so the
// mapping (and its inverse, `bar_pos_to_lines`) can be tested directly.
//
// `pos` runs 0 at the bottom (following new output) to 1 at the oldest line, so
// it matches the scroll model's "lines back from the bottom" rather than the
// screen's y axis - the caller flips it.
fn bar_thumb_span(track_h: f32, thickness: f32, rows: f32, max: f32, pos_lines: f32) -> (f32, f32) {
	// the viewport's share of everything there is to look at
	let visible = (rows / (rows + max)).clamp(0.0, 1.0);
	let thumb_h = (track_h * visible)
		.max(thickness * BAR_MIN_THUMB)
		.min(track_h);
	let pos = if max > 0.0 {
		(pos_lines / max).clamp(0.0, 1.0)
	} else {
		0.0
	};
	// y is measured down the track, so pos 1 (oldest) sits at the top
	let y = (track_h - thumb_h) * (1.0 - pos);
	(y, thumb_h)
}

// Either handle being dragged pins both. The two show the same position, so one
// riding the pointer while the other chased the ease left it trailing the whole
// way down.
fn handle_is_dragged(bar: bool, map: bool) -> bool {
	bar || map
}

// Where a handle sits this frame, in lines. A handle that is being dragged, or
// has just been let go, sits on the target; otherwise it rides the eased text.
// Letting go used to hand it straight back to `visual`, which is still on its
// way, so the handle sprang back the way it came and then crawled forward again.
fn handle_pos(dragged: bool, dropped: bool, target: f32, visual: f32) -> f32 {
	if dragged || dropped { target } else { visual }
}

// The hold after a drop is over once the text has caught up, or at once if
// anything else has moved the target since (a wheel, a jump, fresh output) -
// from there on the handle belongs to the content again.
fn drop_hold_over(animating: bool, dropped_at: f32, target: f32) -> bool {
	!animating || (target - dropped_at).abs() > config::SETTLE_EPS
}

// Inverse of `bar_thumb_span`: a thumb-top offset down the track, back to a
// scroll position in lines. Used while dragging.
fn bar_pos_to_lines(track_h: f32, thumb_h: f32, max: f32, thumb_y: f32) -> f32 {
	let span = track_h - thumb_h;
	if span <= 0.0 {
		return 0.0;
	}
	(1.0 - (thumb_y / span).clamp(0.0, 1.0)) * max
}

enum Node {
	Leaf(PaneId),
	Split {
		dir: Dir,
		ratio: f32,
		// true once the user has dragged this divider: auto even-distribution stops
		// for its same-direction run (successive splits there stay 50/50).
		manual: bool,
		a: Box<Node>,
		b: Box<Node>,
	},
}

// result of building one pane's frame: text lives in pane.buffer, the
// quads come back here for the shared rect renderer
#[derive(Clone)]
pub struct PaneDraw {
	pub top: f32,
	pub bg: Vec<RectInstance>,
	// Underline quads for the hovered hyperlink. Kept out of `bg` deliberately:
	// those double as the scrim's "this cell paints its own background" mask, and
	// an underline is not a cell background - filing it there would punch the
	// readability halo out of every line holding a link.
	pub links: Vec<RectInstance>,
	pub cursor: Option<RectInstance>,
	// App-scroll slide (None = common case: whole pane at `top`). While a
	// full-screen app's scroll eases, the current frame draws shifted at `top`
	// and the scrolled-off strip (pane.strip_buf) fills the revealed gap.
	pub slide: Option<Slide>,
}

// One frame of an easing app-scroll slide. The current frame renders at
// `PaneDraw.top`, clipped to `[region_clip_t, region_clip_b]`; the scrolled-off
// strip renders at `strip_top` clipped to the scroll region `[top_split_y,
// split_y]` (it holds only region rows, so nothing can bleed into the bands);
// and the fixed bands - a bottom status/input line (`has_band`, below `split_y`)
// and a top title bar (`has_top_band`, above `top_split_y`) - redraw unshifted
// at `band_top`. `top_split_y` is f32::MIN when there's no top band (open clip).
//
// The region clip is WELDED to the shifted content's extent, not just the band
// boundaries: the current-frame draw is the whole buffer translated by voff, so
// band rows ride into the region during a slide - the title's glyphs (and their
// scrim) sit voff below the real title, the status rows sit voff above theirs
// - rendering as ghost copies that bounce with the ease. Clipping at the
// content edge cuts them off; the strip owns the gap on the other side of the
// weld.
#[derive(Clone)]
pub struct Slide {
	pub strip_top: f32,
	// the strip rows on screen this frame (see OffStrip::visible), and where the
	// first of them draws
	pub strip_rows: std::ops::Range<usize>,
	pub strip_text_top: f32,
	pub top_split_y: f32,
	pub split_y: f32,
	pub region_clip_t: f32,
	pub region_clip_b: f32,
	pub band_top: f32,
	pub has_band: bool,
	pub has_top_band: bool,
}

// A shaped fallback glyph, cached per (char, bold, italic). Color is NOT baked
// (the TextArea's default_color tints it), so one shaped buffer serves every
// cell/color drawing that glyph - the shaping (harfbuzz fallback matching) is
// the expensive part and only pays once per distinct glyph instead of per cell
// per frame. `ink_w`/`ink_off` are the rasterized ink box for cell-fit scaling.
struct FallbackGlyph {
	buf: Buffer,
	ink_w: f32,
	ink_off: f32,
	// what to add to y so it sits on the terminal font's baseline, not its own
	base_dy: f32,
}

// The hyperlink under the pointer: the URL, and the grid span it occupies so the
// underline can be drawn (inclusive, absolute grid lines - negative in history).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkHit {
	pub url: String,
	pub start: Point,
	pub end: Point,
	pub fg: [u8; 3],
}

pub struct Pane {
	pub id: PaneId,
	pub term: TermInstance,
	pub scroll: Scroll,
	pub buffer: Buffer,
	// Scrolled-off strip (see OffStrip): styled rows the app's scroll pushed out
	// of its region, shaped into `strip_buf` and drawn welded to the content edge
	// so the slide's reveal gap is always exactly filled. `strip_dirty` re-shapes
	// the buffer on the next build (rows changed, or a font rebuild).
	strip: OffStrip,
	strip_buf: Buffer,
	strip_dirty: bool,
	// Previous frame's styled cells (captured only in alt-screen smooth-scroll
	// mode): the rows a step pushes off the region are gone from the grid by the
	// time the step is detected, so they must be captured a frame ahead.
	// `cells_scratch` recycles the row allocations frame to frame.
	last_cells: Vec<Vec<StripCell>>,
	cells_scratch: Vec<Vec<StripCell>>,
	// Recycles the per-row Strings the build's attr-run assembler fills each
	// rebuilt frame (set_text copies out of them, so fresh ones were pure churn).
	rows_scratch: Vec<(String, AttrsList)>,
	// `rect` is the text area; `full` is everything the layout gave this pane,
	// which is wider by the minimap column when one is showing.
	pub rect: Rect,
	pub full: Rect,
	pub title: String,
	pub read_only: bool, // accept no PTY input/paste; selection + copy still work
	// --keep-open: hold this pane after its shell exits instead of closing it.
	// `held` is that pane once the shell has gone - it shows the exit line and
	// waits for a key.
	pub keep_open: bool,
	pub held: bool,
	// launch argv (None = default shell); a split inherits this so a new pane
	// runs the same shell as the one it forked off (see design.md).
	command: Option<Vec<String>>,
	last_draw: PaneDraw,
	// Frames in a row that reused last_draw because the terminal was busy.
	lock_misses: u32,
	// Lines pushed into scrollback since the last build. Sampled per PTY wakeup
	// and again by each build, both against the one baseline - see
	// `note_history` and the growth step in build().
	depth: DepthSampler,
	// On-screen row fingerprints from the last build, used to detect an app's
	// repaint-scroll where the engine recorded none (app-scroll easing). See build().
	last_rows: Vec<u64>,
	// Rows of static bottom band (status/input line) that must NOT slide during the
	// current alt-screen app-scroll ease. Captured when a scroll is detected.
	slide_static: usize,
	// Rows of static TOP band (title bar - nano, muffer) that must NOT slide, the
	// mirror of slide_static. The scrolling region is between the two bands.
	slide_static_top: usize,
	// Rows at the bottom a program redrew in place while the output ease chases
	// new lines above them, drawn still (`output_band`). 0 = none.
	out_band: usize,
	// Last detected step's shift (signed lines). Only feeds the SILK_SCROLLDBG
	// trace now - the strip is positioned by app_off alone - but the harness
	// regex reads the field, so it stays.
	slide_sh: f32,
	// Previous frame's alt-screen state. An enter/exit is an instant screen swap,
	// not a scroll - detected here to hard-cut it instead of animating the swap.
	last_alt: bool,
	// Set by hard_cut() when this pane comes back from a freeze (hidden tab,
	// minimized window): the next build treats the gap like an alt-screen swap -
	// rebaseline the scroll detectors, suppress the nudge, jump instantly.
	pending_cut: bool,
	// Fallback glyphs (not in the primary mono font) pulled out of `buffer` and
	// drawn one-per-cell so their font advance can't shift the row. `glyph_cache`
	// holds each distinct glyph shaped once (keyed by char + bold + italic);
	// `glyphs` is this frame's placements - (key, x, y, color, scale) - `scale`
	// shrinks an over-wide fallback glyph to fit its cell box. The cache persists
	// across frames (dropped on a font/size rebuild, see rebuild_buffers).
	glyph_cache: HashMap<(char, bool, bool), FallbackGlyph>,
	glyphs: Vec<((char, bool, bool), f32, f32, GColor, f32)>,
	// Color (COLRv1) glyphs this frame, placed in absolute screen px and drawn
	// through glyphon's color atlas - see coloremoji.rs. They ride a TextArea
	// that carries no text of its own, hence `empty_buf`.
	emoji: Vec<CustomGlyph>,
	empty_buf: Buffer,
	// Scrim source with bold stripped (text_scrim_regular_weight): shaped alongside
	// the main buffer only on rebuild frames that actually contain bold runs.
	// `scrim_debold` says the buffer is valid for the current content.
	// Per-build memo for the minimum-contrast lift (see palette::Readable).
	readable: palette::Readable,
	scrim_buf: Option<Buffer>,
	scrim_debold: bool,
	// Cursor animation: `cursor_x` (visual column) eases toward the target column
	// so the cursor slides as you type; `blink_t` drives a smooth fade-blink while
	// it sits idle. Snaps on a row change so it doesn't slide diagonally on a newline.
	cursor_x: f32,
	cursor_col: f32,
	cursor_row: i32,
	cursor_init: bool,
	blink_t: f32,
	cursor_idle_t: f32, // seconds since the cursor last moved or was poked (pause gating)
	// wall clock behind cursor_idle_t/hold_t: a parked cursor renders no frames,
	// so frame-dt accumulation would freeze the timers across the sleep
	cursor_step_at: Option<std::time::Instant>,
	// When this pane was last worth animating for: input, or a refocus. The long
	// idle stop reads it rather than the cursor's own stillness, since a program
	// nudging the cursor is not the user being here.
	cursor_active_at: std::time::Instant,
	// when the user last sent input here (keys, paste), and whether the cursor's
	// last move was that input's echo rather than program output - the two get
	// different resume delays
	typed_at: Option<std::time::Instant>,
	cursor_by_input: bool,
	cursor_pause: PauseState,
	pub cursor_animating: bool,
	// parked cursor: when the loop must wake to resume the cycle (no frames flow
	// while parked). None while animating, or when parked with no timed resume.
	pub cursor_wake: Option<std::time::Instant>,
	// Scrollbar fade: `bar_alpha` eases toward 0 or 1, `bar_hold` keeps it up for
	// a moment after the last user scroll. `bar_drag` is the grab offset from the
	// thumb's top edge while the handle is being dragged (Some = dragging).
	bar_alpha: f32,
	bar_hold: f32,
	pub bar_hover: bool,
	pub bar_drag: Option<f32>,
	pub bar_animating: bool,
	// Minimap column: the rasterized buffer and the image composed from it, plus
	// where a drag of the viewport marker started - the pointer y and the scroll
	// position the view sat at then.
	map: Minimap,
	pub map_drag: Option<(f32, f32)>,
	// Where the target sat when a handle drag ended, while the text is still
	// easing up to it. None once the two agree. See `handle_lines`.
	handle_drop_at: Option<f32>,
	// Hyperlink hover: the pointer in window px (None = not over this pane), the
	// link it hit, and a request to re-scan. The scan needs the grid, so it
	// runs in build() where the term lock is already held - on the frame the
	// pointer changed cell, and on any frame that re-shaped text (output moves the
	// span out from under a pointer that never moved).
	hover_px: Option<(f32, f32)>,
	link_probe: bool,
	pub link_hover: Option<LinkHit>,
	// false until the first full build (and reset on a buffer rebuild). When the
	// frame is a pure cursor animation (no content/scroll/bell change), build skips
	// the expensive text re-shape and reuses the cached buffer/bg/glyphs.
	text_built: bool,
	// Bumped on every full re-shape, so the renderer can tell "this pane's text
	// is byte-for-byte the frame before" without inspecting the buffers. Drives
	// the prepare/scrim skip in app.rs.
	pub shape_rev: u64,
	// TermMode snapshot from the last build, so per-keystroke/wheel input paths
	// read it lock-free (at worst one frame stale) instead of taking the term
	// lock the PTY reader may hold across a whole read cycle.
	pub mode: TermMode,
	// Is the minimap column showing? A full-screen program can take it (see
	// minimap::wanted), and that changes the text width, so it is layout state
	// rather than something the draw decides.
	pub map_on: bool,
	// This pane's PTY produced output since the last successful build. Set by
	// the Wakeup(id) event, cleared in build() once the term lock is acquired
	// (a busy-term frame keeps it, so the rebuild retries next frame). Scopes
	// re-shaping to panes that changed: one busy pane no longer forces its
	// idle siblings through set_rich_text every frame.
	pub content_dirty: bool,
	// Auto-copy triggers, independent and session-only (never persisted). A new
	// pane inherits both from the pane it split off (see split_at); a new tab or
	// window starts with both off. Only the focused pane of the active tab in the
	// focused window actually copies - the flags stay set otherwise (see the copy
	// gating in app.rs), so leaving them on across background tabs/windows is fine.
	// copy_output drives the command-output capture (see arm_capture / poll_capture):
	// on Enter at the shell prompt we arm and record `cmd_start` (the line after the
	// prompt); when the terminal settles back at the prompt, the lines since are
	// copied. `last_output` is refreshed on every Wakeup so the settle timer measures
	// true idle. This catches both instant (ls) and long commands.
	pub copy_select: bool,
	pub copy_output: bool,
	capture_armed: bool,
	// floor under the next poll, pushed out by a poll that found the shell busy
	capture_retry_at: std::time::Instant,
	cmd_start: usize,
	// Fingerprint of the arm-time prompt row. cmd_start is "history + row", an
	// index whose origin MOVES once scrollback is at cap (each pushed line evicts
	// the oldest), so capture re-finds the prompt row by content instead and only
	// falls back to cmd_start when it can't (evicted, or redrawn on Enter).
	cmd_anchor: Option<u64>,
	// Multi-line prompt learning: the rows a prompt paints ABOVE its input line
	// keep their structure before every command, while output above the prompt
	// changes run to run. Skeleton fingerprints (fnv_row_skel) so dynamic prompt
	// content still matches. `prompt_above` holds the last arm's above-cursor
	// fingerprints; the contiguous match against the current arm's becomes
	// `prompt_block`, and capture strips the same rows off the resumed prompt
	// (see prompt_strip).
	prompt_above: Vec<u64>,
	prompt_block: Vec<u64>,
	last_output: std::time::Instant,
}

impl Pane {
	// The program this pane was started with, so a title naming it can be known.
	pub fn launched(&self) -> Option<&str> {
		self.command.as_deref()?.first().map(String::as_str)
	}

	pub fn build(
		&mut self,
		ctx: &mut TextCtx,
		dt: f32,
		bell: f32,
		force_rebuild: bool,
		active: bool, // the focused pane of the focused window (cursor animates only there)
	) {
		// Result goes in self.last_draw (read via draw()) - returning it by value
		// cloned the whole bg-quad Vec per pane per frame.
		let cell_w = ctx.cell_w;
		let cell_h = ctx.cell_h;
		let margin = ctx.margin;
		let content_x = self.rect.x + margin;
		let lines = self.term.lines;
		let settings = config::settings(); // snapshot once, not per cell

		// Don't block the render thread while the PTY reader is mid-burst: it holds
		// the terminal across a whole read cycle, so reuse the last built frame and
		// come back next frame. But an unfair try can lose that race forever (the
		// reader re-acquires the instant it lets go), so after LOCK_WAIT_AFTER misses
		// take the FAIR lock instead: it queues on the lease, which puts us in at the
		// end of the current read cycle and makes the reader's next cycle wait behind
		// us. That caps the stale-frame run - without it a heavy `cat` freezes the
		// pane for seconds - at the cost of blocking for one cycle (measured under
		// 5ms). See design.md.
		let Some(mut guard) = lock_for_frame(&self.term.term, &mut self.lock_misses) else {
			return;
		};
		self.mode = *guard.mode();
		self.content_dirty = false;

		// Alt-screen enter/exit is an instant full-screen swap, not a scroll. Flag the
		// transition so the scroll probes below hard-cut it: on enter the app-scroll
		// probe would match blank rows between the old and new screens (nano "jiggles"/
		// scrolls in on launch); on exit the history_size jump (the alt grid carries no
		// scrollback) would fire an output-ease that scrolls the restored screen back
		// in. `gesture_active` (an alt-scroll slide already easing) freezes the band
		// sizes across a continuous scroll - see the app-scroll block. The output
		// ease itself needs no cutting here: the alt grid has no scrollback, so the
		// set_max(0) below stops it (see Scroll::set_max for the wobble that came
		// from letting it run on).
		let alt = self.mode.contains(TermMode::ALT_SCREEN);
		// a freeze catch-up (pending_cut) is the same shape as the screen swap:
		// the grid moved arbitrarily far while nothing was built, so every
		// detector must rebaseline and nothing may ease
		let cut = alt != self.last_alt || std::mem::take(&mut self.pending_cut);
		self.last_alt = alt;
		let gesture_active = self.scroll.app_offset() != 0.0;

		let cols = self.term.cols;
		let history = guard.grid().history_size();

		// Output easing: nudge the smooth offset when the viewport advanced while
		// following the bottom. Pre-cap, scrollback growth IS the line-advance
		// count (and an in-place status line that uses no newline doesn't grow it,
		// so it doesn't bounce). Once the scrollback buffer fills, history_size
		// flatlines - old lines drop off the top as fast as new ones arrive - so
		// growth reads 0 even though the screen still scrolls; the engine's own
		// count (`scrolled`, below) carries the advance from there.
		// The depth is sampled per PTY wakeup (`note_history`) and again here,
		// both against the ONE baseline. A build that reaches the grid before
		// that cycle's wakeup is handled banks the growth itself, and the wakeup
		// then finds nothing left. Measuring it here against a baseline of its
		// own counted a line twice whenever the build won that race, and under a
		// steady in-place progress line (rar's percent) the second count showed
		// as the view hopping down a line and easing back up. A screen swap's
		// depth swing is banked the same way and thrown out by `!cut` below.
		let grew = self.depth.take(history);
		self.scroll.set_max(history as f32);
		// What the engine recorded scrolling since the last build: the region that
		// moved, by how much, and the rows that left it. Exact and uncapped, so the
		// alt screen (no scrollback to measure) eases off it, and so does a scroll
		// region on either screen. Read here, cleared below whichever way this
		// frame goes.
		let (region, scrolled, pushed) = {
			let ledger = guard.scroll_ledger();
			let clamp = |line: Line| (line.0.max(0) as usize).min(lines);
			let r = ledger.region();
			(
				clamp(r.start)..clamp(r.end),
				ledger.lines(),
				ledger.pushed(),
			)
		};
		if cut {
			// hard-cut the screen swap (or freeze catch-up): drop any in-flight
			// slide and rebaseline the row fingerprints (and the styled snapshot
			// the strip captures from) to the NEW screen, so the app-scroll probe
			// can't diff across the gap.
			self.scroll.cancel_app_scroll();
			self.strip.clear();
			self.last_rows = if settings.smooth_apps() {
				let mut cur_cells = std::mem::take(&mut self.cells_scratch);
				let rows = snapshot_rows(
					guard.grid(),
					lines,
					cols,
					Some((guard.colors(), &settings, &mut cur_cells)),
				);
				self.cells_scratch = std::mem::replace(&mut self.last_cells, cur_cells);
				rows
			} else {
				snapshot_rows(guard.grid(), lines, cols, None)
			};
		}
		let whole_screen = region == (0..lines);
		let (follow, advanced, was_chasing) =
			ease_output(&mut self.scroll, alt, cut, grew, pushed, lines);
		if cut || alt || !self.scroll.chasing_output() {
			self.out_band = 0;
		}

		// Minimap: fold this build's grid into the column's cache. Switched off it
		// is one bool, and the cache is freed at relayout. A pure cursor frame
		// changes nothing it shows, so it is skipped there.
		if settings.minimap
			&& (force_rebuild || !self.text_built || advanced > 0 || self.map.pending())
		{
			// Where the map has to stop: the output ease is holding the text this
			// far above the newest line. `Scroll` owns the rule, because a user
			// gesture aimed at the bottom reads exactly like a lag from out here
			// (F150/F153).
			let lag = self.scroll.unshown_lines().round() as usize;
			if let Some(g) = minimap::geom(
				self.full,
				margin,
				ctx.scale,
				&settings,
				history + lines,
				self.map.shown_lines(history, lines),
				lines,
				self.scroll.visual_lines(),
				alt,
				self.map_on,
			) {
				self.map.update(
					guard.grid(),
					guard.colors(),
					&settings,
					g.preview.w.round() as usize,
					g.preview.h.round() as usize,
					ctx.scale,
					lines,
					cols,
					advanced,
					lag,
					self.scroll.unshown_draining(),
					cut,
					std::time::Instant::now(),
				);
			}
		}

		// App-scroll easing: a full-screen app owns its screen and scrolls a region
		// of it in place, with no scrollback behind it to ease through. The engine's
		// record says exactly what moved (`ledger_step`): the region is the band
		// split, the rows that left it are the strip that fills the revealed gap,
		// and the slide offset grows by the count so the frame eases into place
		// instead of snapping. Where the engine recorded nothing - an app that
		// repaints its lines rather than scrolling them, or ConPTY re-emitting a
		// normal-screen TUI's region-scroll as an in-place repaint - a clean
		// vertical translate between this repaint and the last is inferred from
		// the row fingerprints instead (`scroll_shift_signed`, up to APP_SCROLL_MAX
		// rows; in-place redraws and big page-jumps don't match, so they hard-cut).
		// Skipped on pure cursor-animation frames (the fast path below): a shift can
		// only appear when the grid content changed, and that always forces a full
		// build - so the styled snapshot isn't paid per blink frame. The snapshot
		// refreshes on EVERY content frame, grew>0 frames included: those animate
		// via the output ease, and a stale snapshot would make the next repaint
		// frame diff across that eased scroll and slide it AGAIN - a spurious extra
		// down-then-up on every output line (the shell redraws its prompt right
		// after the scroll).
		let mut shift_dbg = 0i32;
		let step = ledger_step(alt, follow, advanced, whole_screen, scrolled);
		let chunk = step.map(|_| {
			strip_rows(
				guard.scroll_ledger().rows(),
				cols,
				guard.colors(),
				&settings,
			)
		});
		// While a slide is in flight the ledger keeps its region and direction, so
		// a scroll of an overlapping region next frame carries on from it rather
		// than starting the strip over (nano's odd status-row step).
		if !cut && (gesture_active || step.is_some()) {
			guard.scroll_ledger_mut().drain();
		} else {
			guard.scroll_ledger_mut().clear();
		}
		let step = step.filter(|&step| {
			let chunk = chunk.as_deref().unwrap_or_default();
			let room = !SLIDE_DOWN_INTO_ROOM
				&& ledger_makes_room(step, &region, lines, &self.last_cells, chunk);
			if room {
				// still traced, so the harness can tell a pop from no scroll at all
				shift_dbg = step;
			}
			slide_is_visible(step, &region, &self.last_cells, chunk) && !room
		});
		// The fingerprints also feed the output band, which belongs to the output
		// ease and so works with app easing off. The styled cells stay app-only.
		let apps = settings.smooth_apps();
		if (apps || (settings.scroll_smooth && !alt)) && !cut && (force_rebuild || !self.text_built)
		{
			let mut cur_cells = std::mem::take(&mut self.cells_scratch);
			let rows = snapshot_rows(
				guard.grid(),
				lines,
				cols,
				apps.then_some((guard.colors(), &settings, &mut cur_cells)),
			);
			if let (true, Some(step)) = (apps, step) {
				shift_dbg = step;
				// The bands are the region's edges, except that a row at the edge away
				// from the strip which reads the same as last frame is held still: a
				// pager's prompt, redrawn after every scroll with no region of its own
				// (less). Visually lossless either way - the same text ends up in the
				// same place - and frozen for the gesture, like the inferred bands, so
				// a row that matches by chance can't un-pin mid-ease. A blank row is
				// never it: a build can fall between an app's scroll and its draw of
				// the freed row (tmux flushes exactly that way), and pinning the row
				// then makes its new line pop in while the rest slides.
				if !gesture_active || self.strip.region != (region.start, region.end) {
					let blank = fnv_row(std::iter::repeat_n(' ', cols));
					let same = |i: usize| {
						self.last_rows.len() == lines
							&& rows[i] == self.last_rows[i]
							&& rows[i] != blank
					};
					let (mut top, mut bottom) = (region.start, region.end);
					if step > 0 {
						while bottom > top + 1 && same(bottom - 1) {
							bottom -= 1;
						}
					} else {
						while top + 1 < bottom && same(top) {
							top += 1;
						}
					}
					self.slide_static_top = top;
					self.slide_static = lines - bottom;
				}
				// An overlay repainted over the strip-side edge is held as well. It may
				// grow mid-gesture, since a build can fall between the scroll and the
				// repaint.
				let over = repainted_edge(&rows, &self.last_rows, &region, step);
				let held = if step > 0 {
					self.slide_static_top = self.slide_static_top.max(region.start + over);
					self.slide_static_top > region.start
				} else {
					self.slide_static = self.slide_static.max(lines - region.end + over);
					lines - self.slide_static < region.end
				};
				self.slide_sh = step as f32;
				// The recorded rows hold the overlay's old row, which would draw it a
				// second time in the reveal, so a held edge takes last frame's cells.
				let chunk = if !held {
					chunk.unwrap_or_default()
				} else if self.last_cells.len() == lines
					&& self.last_cells.first().is_none_or(|r| r.len() == cols)
				{
					let range =
						vanished_range(step, self.slide_static_top, self.slide_static, lines);
					self.last_cells[range]
						.iter_mut()
						.map(std::mem::take)
						.collect()
				} else {
					Vec::new()
				};
				if !chunk.is_empty() {
					self.strip
						.push_step(step.signum() as i8, (region.start, region.end), chunk);
					self.strip_dirty = true;
				}
				// the offset may not open a gap the strip cannot fill
				let cover = self.strip.len() as f32;
				let region_rows = (region.end - region.start) as f32;
				self.scroll.app_scroll(step as f32, cover, region_rows);
			} else if apps {
				let shift = if fingerprint_frame(alt, follow, grew, scrolled) {
					let shift = scroll_shift_signed(&rows, &self.last_rows, APP_SCROLL_MAX);
					let blank = fnv_row(std::iter::repeat_n(' ', cols));
					shift_dbg = shift;
					if SLIDE_DOWN_INTO_ROOM
						|| !shift_makes_room(&rows, &self.last_rows, shift, blank)
					{
						shift
					} else {
						0
					}
				} else {
					0
				};
				if shift != 0 {
					// Freeze the band sizes on the gesture's first step (a clean
					// settled-vs-scrolled diff); re-measuring per step fluctuates by a row
					// whenever a blank/matching line abuts a band. Held while the slide eases.
					if !gesture_active {
						let (st, sb) = slide_bands(&rows, &self.last_rows, shift);
						self.slide_static = sb;
						self.slide_static_top = st;
					}
					if SLIDE_TOP_BAND_APPS || self.slide_static_top == 0 {
						// ACCUMULATE the visual offset so the CURRENT content stays continuous
						// across overlapping steps: screen row = grid_row + app_off, the grid
						// already advanced by shift, so app_off must GROW by shift to hold a
						// line fixed for that instant. The strip grows by the same rows the
						// step pushed off the region (from the frame-old snapshot; a stale or
						// resized snapshot just skips the fill for this one step), so the gap
						// the accumulated offset opens is always exactly covered.
						self.slide_sh = shift as f32;
						if self.last_cells.len() == lines
							&& self.last_cells.first().is_none_or(|r| r.len() == cols)
						{
							let range = vanished_range(
								shift,
								self.slide_static_top,
								self.slide_static,
								lines,
							);
							// move the rows out, don't clone: last_cells is replaced wholesale
							// below, and snapshot_rows recycles emptied slots next capture
							let chunk: Vec<Vec<StripCell>> = self.last_cells[range]
								.iter_mut()
								.map(std::mem::take)
								.collect();
							if !chunk.is_empty() {
								let region = (self.slide_static_top, lines - self.slide_static);
								self.strip.push_step(shift.signum() as i8, region, chunk);
								self.strip_dirty = true;
							}
						}
						self.scroll
							.app_scroll(shift as f32, OffStrip::CAP as f32, lines as f32);
					}
				}
			}
			if advanced > 0 && follow && !alt && settings.scroll_smooth {
				// rows outside a recorded scroll region stayed put by definition
				let hint = if scrolled > 0 && region.start == 0 && region.end < lines {
					lines - region.end
				} else {
					0
				};
				let blank = fnv_row(std::iter::repeat_n(' ', cols));
				let measured = output_band(&rows, &self.last_rows, advanced, hint, blank);
				self.out_band = adopt_band(self.out_band, measured, was_chasing);
			}
			self.last_rows = rows;
			if apps {
				self.cells_scratch = std::mem::replace(&mut self.last_cells, cur_cells);
			} else {
				self.cells_scratch = cur_cells;
			}
		}

		// snap the integer grid offset to the floor of the smooth position
		let desired = self.scroll.desired_offset().min(history);
		let current = guard.grid().display_offset();
		let delta = desired as i32 - current as i32;
		if delta != 0 {
			guard.scroll_display(GridScroll::Delta(delta));
		}

		let frac = self.scroll.frac();
		// alt-screen slide rides on top of the fractional scrollback offset (which
		// is 0 on the alt screen); + shifts content down, revealing bg at the top.
		let app_off = self.scroll.app_offset();
		let voff = frac + app_off;
		// an app slide keeps its own bands and wins over the output band
		let ob = if app_off == 0.0 {
			self.out_band.min(lines / 2)
		} else {
			0
		};
		// Dev trace for the alt-screen slide (SILK_SCROLLDBG). Off = one cached bool
		// check per frame. The per-frame (sh, app_off, slide_sh, st, sb) sequence is
		// the deterministic proof that the slide eases smoothly (app_off monotonic, no
		// bounce) without needing to eyeball a render - see the headless bounce harness.
		// `rgn` and `strip` are there because the two of them are what a truncated
		// slide looks like: a region that differs from the strip's throws the strip
		// away, and the offset is then capped to what one step can fill.
		if scroll_dbg()
			&& ((settings.smooth_apps() && (alt || app_off != 0.0 || shift_dbg != 0))
				|| (!alt && frac > 0.0))
		{
			let frame = DBG_FRAME.fetch_add(1, Ordering::Relaxed);
			eprintln!(
				"SCROLLDBG f={frame} pane={} sh={shift_dbg} app_off={app_off:.4} slide_sh={:.4} st={} sb={} frac={frac:.4} alt={} ob={ob} rgn={}..{} strip={}",
				self.id,
				self.slide_sh,
				self.slide_static_top,
				self.slide_static,
				alt as u8,
				region.start,
				region.end,
				self.strip.len(),
			);
		}
		// Region-aware slide: only the middle scroll region shifts by voff; a static
		// bottom band (status/input line) and a static top band (title bar) hold their
		// fractional-only position. `split_row` is the first row of the bottom band;
		// `top_split_row` is the first row of the scroll region (just below the title).
		// No bands (or no active slide) => whole pane at voff.
		let static_rows = if app_off == 0.0 {
			ob
		} else {
			self.slide_static.min(lines)
		};
		let static_top = if app_off == 0.0 {
			0
		} else {
			self.slide_static_top.min(lines.saturating_sub(static_rows))
		};
		let split_row = (lines - static_rows) as i32;
		let top_split_row = static_top as i32;
		// An app slide redraws its bands at the fractional offset. The output band
		// is the live bottom of the grid, which is where it already belongs.
		let band_off = if app_off == 0.0 { 0.0 } else { frac };
		let voff_of = |screen_row: i32| {
			if (static_top > 0 && screen_row < top_split_row)
				|| (static_rows > 0 && screen_row >= split_row)
			{
				band_off
			} else {
				voff
			}
		};
		let display_offset = desired as i32;
		let hist = history as i32;
		// fractional scroll shifts content DOWN by frac of a cell; we render an
		// extra row above (screen row -1) so the revealed strip is filled.
		let y_of = |screen_row: i32| {
			self.rect.y + margin + (screen_row as f32 + voff_of(screen_row)) * cell_h
		};
		// The scroll-region draw origin is always the SHIFTED position, independent of
		// the bands (which are redrawn unshifted at band_top); grid elements use y_of.
		let top = self.rect.y + margin + (-1.0 + voff) * cell_h;
		// Slide geometry (only while app_off is easing). The current frame draws at
		// `top` (scroll region, clipped to [top_split_y, split_y]); the scrolled-off
		// strip fills the gap the shift opens - above the content when sliding down
		// (app_off > 0), below it when sliding up. The strip is welded to the content
		// edge and rides the same eased offset, so it never moves relative to the
		// content: its last row ends exactly at the region's first row (up-scroll),
		// or its first row starts one past the region's last (down-scroll).
		let slide = if app_off == 0.0 && ob == 0 {
			None
		} else if app_off == 0.0 {
			// The output band: no strip, and the eased text keeps the overscan row
			// that fills the top, so only the band's own edge clips it.
			let split_y = self.rect.y + margin + split_row as f32 * cell_h;
			Some(Slide {
				strip_top: 0.0,
				strip_rows: 0..0,
				strip_text_top: 0.0,
				top_split_y: f32::MIN,
				split_y,
				region_clip_t: f32::MIN,
				region_clip_b: split_y,
				band_top: self.rect.y + margin - cell_h,
				has_band: true,
				has_top_band: false,
			})
		} else {
			// split_y bounds the scroll region below; top_split_y bounds it above (a
			// static top band sits above it; f32::MIN = no band, so the clip is open).
			let split_y = if static_rows > 0 {
				self.rect.y + margin + (split_row as f32 + frac) * cell_h
			} else {
				self.rect.y + self.rect.h
			};
			let top_split_y = if static_top > 0 {
				self.rect.y + margin + (top_split_row as f32 + frac) * cell_h
			} else {
				f32::MIN
			};
			let band_top = self.rect.y + margin + (-1.0 + frac) * cell_h;
			let strip_top = if app_off > 0.0 {
				self.rect.y
					+ margin + (top_split_row as f32 + voff - self.strip.len() as f32) * cell_h
			} else {
				self.rect.y + margin + (split_row as f32 + voff) * cell_h
			};
			// content extent = first/one-past-last region row at the shifted position
			let content_top_y = self.rect.y + margin + (top_split_row as f32 + voff) * cell_h;
			let content_bot_y = self.rect.y + margin + (split_row as f32 + voff) * cell_h;
			let (region_clip_t, region_clip_b) =
				weld_region_clip(top_split_y, split_y, content_top_y, content_bot_y);
			let strip_rows = self.strip.visible(app_off);
			let strip_text_top = strip_top + strip_rows.start as f32 * cell_h;
			Some(Slide {
				strip_top,
				strip_rows,
				strip_text_top,
				top_split_y,
				split_y,
				region_clip_t,
				region_clip_b,
				band_top,
				has_band: static_rows > 0,
				has_top_band: static_top > 0,
			})
		};
		// Hyperlink under the pointer. The scan wants the grid, which is locked
		// right here - so it runs on the frame the pointer changed cell, and on
		// every frame that re-shapes text (output slides the span out from under a
		// pointer that never moved). A missed lock leaves the probe pending.
		if self.link_probe || force_rebuild || !self.text_built {
			self.link_probe = false;
			let hit = if settings.hyperlinks {
				self.hover_px.and_then(|(px, py)| {
					link_at(
						guard.grid(),
						guard.colors(),
						&settings,
						self.rect,
						px,
						py,
						(cell_w, cell_h, margin),
						(cols, lines),
						display_offset,
					)
				})
			} else {
				None
			};
			if hit != self.link_hover {
				self.link_hover = hit;
			}
		}
		// Underline quads, rebuilt every frame: they ride the eased scroll offset
		// through y_of, so a cached set would lag the text it belongs to.
		let mut link_rects = Vec::new();
		if let Some(link) = &self.link_hover {
			// A link underline wants to sit just under the baseline; the mono
			// baseline isn't published, so it's placed off the cell box instead -
			// close enough for a 1px rule, and it scales with the font.
			let thick = (cell_h * 0.06).round().max(1.0);
			let gap = (cell_h * 0.10).round();
			let color = config::srgb_f32(link.fg);
			for grid_line in link.start.line.0..=link.end.line.0 {
				let screen_row = if ob > 0 && grid_line >= split_row {
					grid_line
				} else {
					grid_line + display_offset
				};
				// a scrolled-view line that sits under the band is hidden by it
				if screen_row < 0
					|| screen_row >= lines as i32
					|| (ob > 0 && grid_line < split_row && screen_row >= split_row)
				{
					continue;
				}
				let first_col = if grid_line == link.start.line.0 {
					link.start.column.0
				} else {
					0
				};
				let last_col = if grid_line == link.end.line.0 {
					link.end.column.0
				} else {
					cols.saturating_sub(1)
				};
				if last_col < first_col {
					continue;
				}
				link_rects.push(RectInstance {
					pos: [
						content_x + first_col as f32 * cell_w,
						y_of(screen_row) + cell_h - thick - gap,
					],
					size: [(last_col - first_col + 1) as f32 * cell_w, thick],
					color,
					..Default::default()
				});
			}
		}

		// Cursor position/shape as plain values (no lasting borrow of the lock), so
		// the fast path below can drop the term lock immediately.
		let cursor_pt = guard.grid().cursor.point;
		let cursor_shape = if self.held {
			CursorShape::Hidden // nothing is reading; a blinking cursor would lie
		} else {
			shown_cursor_shape(*guard.mode(), guard.cursor_style().shape)
		};
		// Alt-screen apps own their cursor shape; on the primary screen it's the
		// configured geometry (or the app's DECSCUSR). See cursor_geometry.
		let cursor_geom =
			cursor_geometry(cursor_shape, guard.mode().contains(TermMode::ALT_SCREEN));
		let following = desired == 0;

		// Fast path: a pure cursor-animation frame (blink/slide, no content/scroll/
		// bell change). Reuse the cached buffer + glyphs + bg from the last full
		// build and recompute only the cursor - skips set_rich_text + shaping, the
		// expensive part, so a blinking cursor doesn't re-shape text every frame.
		//
		// `delta` has to be nil for that to hold: the caller decides `force_rebuild`
		// from `scroll.animating()`, which is already false on the frame an ease
		// LANDS, and if that last step crossed a line the grid has just moved under
		// a buffer nobody re-shaped - text drawn a row off its own backgrounds.
		if !force_rebuild && self.text_built && delta == 0 {
			drop(guard);
			let cursor = self.cursor_quad(
				cursor_pt,
				cursor_shape,
				cursor_geom,
				display_offset,
				lines,
				following,
				content_x,
				cell_w,
				cell_h,
				margin,
				voff,
				dt,
				settings.cursor,
				active,
			);
			// the fast path is a pure cursor frame - never taken while a slide eases
			// (that forces a rebuild), so there is never a slide here
			self.last_draw.top = top;
			self.last_draw.cursor = cursor;
			self.last_draw.links = link_rects;
			self.last_draw.slide = None;
			return;
		}
		self.text_built = true;
		self.shape_rev = self.shape_rev.wrapping_add(1);

		let colors = guard.colors();
		let sel_range = guard.selection.as_ref().and_then(|s| s.to_range(&*guard));
		let grid = guard.grid();

		let mut bg = Vec::new();
		// fallback glyphs to draw per-cell: (char, fg, bold, italic, col, screen-row, cells)
		let mut glyph_specs: Vec<(char, [u8; 3], bool, bool, usize, i32, u8)> = Vec::new();
		let default_attrs = mono_attrs();

		// While a slide eases, region rows shift by voff but rect quads only get the
		// per-pane scissor (no per-area clip like text) - clamp region-row rects to
		// the region so an own-bg row (inverse video, a colored block) can't poke
		// into the title/status bands mid-slide.
		let region_rect_clip = slide.as_ref().map(|sl| {
			(
				if sl.has_top_band {
					sl.top_split_y
				} else if app_off == 0.0 {
					self.rect.y // the output band keeps the overscan row
				} else {
					self.rect.y + margin
				},
				if sl.has_band {
					sl.split_y
				} else {
					self.rect.y + self.rect.h - margin
				},
			)
		});

		// Build attr-runs, but keep them grouped BY ROW (viewport + 1 overscan row)
		// rather than as one newline-joined blob. Handing cosmic-text the rows one
		// at a time lets it compare each against what that line already holds and
		// re-shape only the ones that really changed - a keystroke touches one row,
		// not a screenful. See the buffer update after this loop.
		let mut rows_out = std::mem::take(&mut self.rows_scratch);
		rows_out.reserve(lines + 1);
		let mut rows_used = 0usize;
		let mut line_text = String::new();
		let mut line_attrs = AttrsList::new(&default_attrs);
		let mut run = String::new();
		let mut run_color = settings.fg;
		let mut run_bold = false;
		let mut run_italic = false;
		let mut saw_bold = false;
		// hoisted: mono_attrs() takes an RwLock read, too hot per attribute run
		let bold_weight = crate::text::mono_bold_weight();

		macro_rules! flush_run {
			() => {
				if !run.is_empty() {
					let mut attrs = default_attrs.clone();
					attrs.color_opt = Some(GColor::rgb(run_color[0], run_color[1], run_color[2]));
					if run_bold {
						attrs.weight = bold_weight;
					}
					if run_italic {
						attrs.style = Style::Italic;
					}
					let start = line_text.len();
					line_text.push_str(&run);
					run.clear();
					// same rule cosmic-text uses: a run matching the defaults needs no span
					if attrs != default_attrs {
						line_attrs.add_span(start..line_text.len(), &attrs);
					}
				}
			};
		}
		// End the current row and start a fresh one. Run state (color/bold/italic)
		// deliberately carries over: `run` is empty here, so an unchanged attribute
		// at the start of the next row just keeps appending. Rows go in recycled
		// scratch slots so their String capacity survives across frames.
		macro_rules! flush_row {
			() => {
				flush_run!();
				let attrs = std::mem::replace(&mut line_attrs, AttrsList::new(&default_attrs));
				if rows_used < rows_out.len() {
					let slot = &mut rows_out[rows_used];
					std::mem::swap(&mut slot.0, &mut line_text);
					slot.1 = attrs;
					line_text.clear(); // recycled slot String, capacity kept
				} else {
					rows_out.push((std::mem::take(&mut line_text), attrs));
				}
				rows_used += 1;
			};
		}

		for screen_row in -1..(lines as i32) {
			let grid_line = band_row_line(screen_row, display_offset, split_row, ob);
			// off the top/bottom of real content: blank row (still emitted, so the
			// row count always matches the buffer's line count)
			if grid_line < -hist || grid_line > (lines as i32 - 1) {
				flush_row!();
				continue;
			}
			let row = &grid[Line(grid_line)];
			let y = y_of(screen_row);
			for c in 0..cols {
				let cell = &row[Column(c)];
				let flags = cell.flags;
				if flags.contains(Flags::WIDE_CHAR_SPACER) {
					continue;
				}
				let mut fg = palette::resolve(cell.fg, colors, &settings);
				let mut cell_bg = palette::resolve(cell.bg, colors, &settings);
				if flags.contains(Flags::INVERSE) {
					std::mem::swap(&mut fg, &mut cell_bg);
				}
				if flags.contains(Flags::HIDDEN) {
					fg = cell_bg;
				}
				if flags.contains(Flags::DIM) {
					fg = [
						fg[0] / 2 + fg[0] / 4,
						fg[1] / 2 + fg[1] / 4,
						fg[2] / 2 + fg[2] / 4,
					];
				}
				let selected =
					sel_range.is_some_and(|r| r.contains(Point::new(Line(grid_line), Column(c))));
				// What is actually painted behind the glyph - the selection quad
				// where there is one. The bell flash rides on top of the result, so
				// a flash can brighten but never undoes the lift.
				let behind = if selected {
					config::SELECTION_BG
				} else {
					cell_bg
				};
				fg = self.readable.get(fg, behind, settings.text_min_contrast);
				if bell > 0.0 {
					fg = bell_brighten(fg, bell); // visual-bell flash
				}

				let bg_color = if selected {
					Some(config::SELECTION_BG)
				} else if cell_bg != settings.bg {
					Some(cell_bg)
				} else {
					None
				};
				if let Some(col) = bg_color {
					let (mut rect_top, mut rect_bot) = (y, y + cell_h);
					if let Some((clip_t, clip_b)) = region_rect_clip {
						if screen_row >= top_split_row && screen_row < split_row {
							rect_top = rect_top.max(clip_t);
							rect_bot = rect_bot.min(clip_b);
						}
					}
					if rect_bot > rect_top {
						bg.push(RectInstance {
							pos: [content_x + c as f32 * cell_w, rect_top],
							size: [cell_w, rect_bot - rect_top],
							color: config::srgb_f32(col),
							..Default::default()
						});
					}
				}

				// reverse-video (dark-on-light) text renders visually thinner than the
				// same weight light-on-dark; embolden it so inverse chrome (nano/vim
				// title+status bars) reads as strongly as normal text.
				let bold = flags.contains(Flags::BOLD)
					|| (settings.embolden_inverse && flags.contains(Flags::INVERSE));
				let italic = flags.contains(Flags::ITALIC);
				saw_bold |= bold;
				// A glyph the primary mono font lacks renders via a fallback font
				// whose advance may not equal the grid width, drifting the rest of
				// the row. Pull it out, draw it per-cell, leave space placeholders.
				// Same for one the font does carry but advances by the wrong number
				// of cells - a mono face routinely holds a double-width char at its
				// ordinary single advance.
				let w = if flags.contains(Flags::WIDE_CHAR) {
					2
				} else {
					1
				};
				if !cell.c.is_ascii() && !ctx.covered_at(cell.c, w) {
					for _ in 0..w {
						run.push(' ');
					}
					glyph_specs.push((cell.c, fg, bold, italic, c, screen_row, w));
				} else {
					if (fg, bold, italic) != (run_color, run_bold, run_italic) {
						flush_run!();
						run_color = fg;
						run_bold = bold;
						run_italic = italic;
					}
					run.push(render_char(cell.c));
				}
			}
			flush_row!();
		}
		rows_out.truncate(rows_used); // drop stale slots from a taller prior frame

		drop(guard);
		let mut cursor = self.cursor_quad(
			cursor_pt,
			cursor_shape,
			cursor_geom,
			display_offset,
			lines,
			following,
			content_x,
			cell_w,
			cell_h,
			margin,
			voff_of(cursor_pt.line.0 + display_offset),
			dt,
			settings.cursor,
			active,
		);
		// the cursor rides the sliding region too - clamp it like the bg rects
		// (only when it's a region row; nano parks it in the status band on ^W)
		if let Some((clip_t, clip_b)) = region_rect_clip {
			let cursor_row = cursor_pt.line.0 + display_offset;
			if cursor_row >= top_split_row && cursor_row < split_row {
				if let Some(q) = &mut cursor {
					let bot = (q.pos[1] + q.size[1]).min(clip_b);
					q.pos[1] = q.pos[1].max(clip_t);
					q.size[1] = (bot - q.pos[1]).max(0.0);
				}
			}
		}
		// Update the buffer a line at a time instead of through set_rich_text.
		// set_rich_text rebuilds every line unconditionally, which drops each one's
		// cached shaping - so a single changed cell used to re-shape the whole
		// screen. Per-line assignment compares text and attributes first and only
		// invalidates lines that actually differ; shape_until_scroll then re-shapes
		// just those. Shaping is the expensive half of a frame.
		// Advanced (not Basic) so missing glyphs fall back to other fonts
		// (CJK/emoji/math/RTL) instead of rendering tofu. cosmic-text 0.18.2's
		// fallback loop is bounded and keeps monospace alignment; earlier 0.18
		// could hang here (see git history) but no longer does (stress-tested).
		//
		// Scrim source with uniform weight: bold ink is wider, so its halo reads
		// heavier than the neighbors'. When text_scrim_regular_weight is on and
		// bold is on screen, shape a parallel buffer with bold stripped for the
		// scrim pass (crisp text on top keeps its real weight). Costs a second
		// shape only on rebuild frames that contain bold. Per-cell fallback
		// glyphs keep their weight - rare, and not worth a second glyph pool.
		// ctx.debold_safe guards a font (Windows default faces) whose bold advance
		// differs from cell_w: there the de-bold buffer drifts from the display
		// buffer along the line, so the scrim would sit wider than the text.
		// Runs first because it reads the rows the display buffer then consumes.
		self.scrim_debold = settings.text_scrim
			&& settings.text_scrim_radius > 0.0
			&& settings.text_scrim_regular_weight
			&& ctx.debold_safe
			&& saw_bold;
		if self.scrim_debold {
			let (buf_w, buf_h) = self.buffer.size();
			let scrim_buffer = self.scrim_buf.get_or_insert_with(|| {
				let mut buf = Buffer::new(&mut ctx.font_system, ctx.metrics);
				buf.set_wrap(&mut ctx.font_system, glyphon::Wrap::None);
				buf.set_monospace_width(&mut ctx.font_system, Some(cell_w));
				buf
			});
			scrim_buffer.set_metrics(&mut ctx.font_system, ctx.metrics);
			scrim_buffer.set_size(&mut ctx.font_system, buf_w, buf_h);
			// borrow the row text; only the attrs differ (bold stripped)
			set_buffer_rows(
				scrim_buffer,
				rows_out.iter().map(|(text, attrs)| {
					(text.as_str(), debold_attrs(attrs, default_attrs.weight))
				}),
			);
			scrim_buffer.shape_until_scroll(&mut ctx.font_system, false);
		}
		set_buffer_rows(
			&mut self.buffer,
			rows_out.iter_mut().map(|(text, attrs)| {
				(
					text.as_str(),
					std::mem::replace(attrs, AttrsList::new(&default_attrs)),
				)
			}),
		);
		self.buffer.shape_until_scroll(&mut ctx.font_system, false);
		self.rows_scratch = rows_out; // keep the row Strings for the next frame

		// Re-shape the scrolled-off strip when the rows on screen changed: new rows
		// pushed, or the ease moved the window (see OffStrip::visible). A screenful
		// at most, like the main buffer.
		if let Some(sl) = &slide {
			if !sl.strip_rows.is_empty() && (self.strip_dirty || self.strip.shaped != sl.strip_rows)
			{
				self.strip_dirty = false;
				self.shape_strip(ctx, &settings, sl.strip_rows.clone());
			}
		}
		// Strip cells with their own background (inverse video, colored bg) keep it
		// while revealed: emit their rects at the strip's slide position, clamped to
		// the region clip like the sliding content's rects above.
		if let (Some(sl), Some((clip_t, clip_b))) = (&slide, region_rect_clip) {
			for j in sl.strip_rows.clone() {
				let row = &self.strip.rows[j];
				let y = sl.strip_top + j as f32 * cell_h;
				let (rect_top, rect_bot) = (y.max(clip_t), (y + cell_h).min(clip_b));
				if rect_bot <= rect_top {
					continue;
				}
				for (c, cell) in row.iter().enumerate() {
					if cell.wide == 0 {
						continue;
					}
					if let Some(col) = cell.bg {
						bg.push(RectInstance {
							pos: [content_x + c as f32 * cell_w, rect_top],
							size: [cell_w, rect_bot - rect_top],
							color: config::srgb_f32(col),
							..Default::default()
						});
					}
				}
			}
		}

		// Scrim source with uniform weight: bold ink is wider, so its halo reads
		// heavier than the neighbors'. When text_scrim_regular_weight is on and
		// bold is on screen, shape a parallel buffer with bold stripped for the
		// scrim pass (crisp text on top keeps its real weight). Costs a second
		// shape only on rebuild frames that contain bold. Per-cell fallback
		// glyphs keep their weight - rare, and not worth a second glyph pool.
		// ctx.debold_safe guards a font (Windows default faces) whose bold advance
		// differs from cell_w: there the de-bold buffer drifts from the display
		// buffer along the line, so the scrim would sit wider than the text.
		// place the per-cell fallback glyphs. Each distinct glyph is shaped once
		// (harfbuzz fallback matching is the cost) and cached; every cell/color
		// drawing it reuses that buffer, tinted per-cell via TextArea.default_color.
		self.glyphs.clear();
		self.emoji.clear();
		let rect_y = self.rect.y;
		for (ch, color, bold, italic, c, screen_row, cells) in glyph_specs {
			let cell_x = content_x + c as f32 * cell_w;
			let row_y = rect_y + margin + (screen_row as f32 + voff_of(screen_row)) * cell_h;
			// A color glyph is a self-contained image, so it goes to the color
			// atlas whole rather than through the monochrome fallback face below.
			let color_glyph = if settings.color_emoji {
				ctx.color_metrics(ch)
			} else {
				None
			};
			if let Some(metrics) = color_glyph {
				// Fit the design box inside the cell box, keeping its aspect, and
				// center it there. Color glyphs are drawn, not typeset - fitting the
				// cell reads better than sitting them on the text baseline.
				let target_w = cells as f32 * cell_w;
				let fit = (target_w / metrics.box_w).min(cell_h / metrics.box_h);
				let w = (metrics.box_w * fit).round().max(1.0);
				let h = (metrics.box_h * fit).round().max(1.0);
				ctx.color_warm(metrics.id, w as u16, h as u16);
				self.emoji.push(CustomGlyph {
					id: metrics.id,
					left: cell_x + (target_w - w) / 2.0,
					top: row_y + (cell_h - h) / 2.0,
					width: w,
					height: h,
					color: None,
					// An image wants whole pixels; this also keeps one raster per
					// size instead of one per subpixel bin.
					snap_to_physical_pixel: true,
					metadata: 0,
				});
				continue;
			}
			let key = (ch, bold, italic);
			let glyph = self.glyph_cache.entry(key).or_insert_with(|| {
				let mut attrs = mono_attrs(); // color left unset - the TextArea tints it
				if bold {
					attrs.weight = crate::text::mono_bold_weight();
				}
				if italic {
					attrs.style = Style::Italic;
				}
				let mut buf = ctx.new_plain_buffer();
				let (ink_w, ink_off, base_dy) = ctx.fill_glyph(&mut buf, ch, &attrs);
				FallbackGlyph {
					buf,
					ink_w,
					ink_off,
					base_dy,
				}
			});
			// Fit the ink inside its cell box (cells * cell_w wide), only ever
			// shrinking, and center it there - a fallback face's wider-than-a-cell
			// ink would otherwise spill over the next cell and collide with its
			// text. Back out the ink offset so centering is on the ink, not the pen.
			let target = cells as f32 * cell_w;
			let scale = if glyph.ink_w > target {
				target / glyph.ink_w
			} else {
				1.0
			};
			let x = cell_x + (target - glyph.ink_w * scale) / 2.0 - glyph.ink_off * scale;
			// plus the shift that puts a fallback face's glyph on the same baseline
			// as the text beside it
			let y = row_y + cell_h * (1.0 - scale) / 2.0 + glyph.base_dy * scale;
			self.glyphs
				.push((key, x, y, GColor::rgb(color[0], color[1], color[2]), scale));
		}

		self.last_draw = PaneDraw {
			top,
			bg,
			links: link_rects,
			cursor,
			slide,
		};
	}

	// The frame build() just produced (or the retained one on a lock miss).
	pub fn draw(&self) -> &PaneDraw {
		&self.last_draw
	}

	// Where the pointer sits over this pane (None = elsewhere). Only asks for a
	// re-scan when the CELL changed, so sweeping across a row costs one scan per
	// cell rather than one per pixel; the scan itself happens in build().
	pub fn set_hover(&mut self, px: Option<(f32, f32)>, ctx: &TextCtx) {
		let cell_of = |(x, y): (f32, f32)| {
			(
				((x - self.rect.x - ctx.margin) / ctx.cell_w).floor() as i32,
				((y - self.rect.y - ctx.margin) / ctx.cell_h).floor() as i32,
			)
		};
		if self.hover_px.map(cell_of) != px.map(cell_of) {
			self.link_probe = true;
		}
		self.hover_px = px;
		if px.is_none() {
			self.link_hover = None;
		}
	}

	// A re-scan is pending, so the frame it falls on must actually be drawn.
	pub fn link_probing(&self) -> bool {
		self.link_probe
	}

	// The link at window pixel (x, y) as of RIGHT NOW - its own scan under the
	// term lock. `link_hover` is a frame behind the pointer (it is filled in by
	// build), which is fine for an underline and not fine for a click: the paths
	// that act on a link ask here instead, so they work even where hover never
	// ran (a mouse-tracking app owns the pointer, but our menu still wins the
	// right-click).
	pub fn link_at_px(&self, x: f32, y: f32, ctx: &TextCtx) -> Option<LinkHit> {
		let settings = config::settings();
		if !settings.hyperlinks || !self.rect.contains(x, y) {
			return None;
		}
		let guard = self.term.term.lock_unfair();
		let display_offset = guard.grid().display_offset() as i32;
		link_at(
			guard.grid(),
			guard.colors(),
			&settings,
			self.rect,
			x,
			y,
			(ctx.cell_w, ctx.cell_h, ctx.margin),
			(self.term.cols, self.term.lines),
			display_offset,
		)
	}

	// The user sent input here (a keystroke, a paste). Stamps the moment so the
	// cursor move it echoes is told apart from a program's own output.
	pub fn note_typed(&mut self) {
		let now = std::time::Instant::now();
		self.typed_at = Some(now);
		self.cursor_active_at = now;
	}

	// A window/tab/pane refocus resumes the cursor animation AT ONCE - no resume
	// delay, unlike a keystroke - starting from the top of the cycle, which is
	// the same full-size point a pause always parks at.
	pub fn poke_cursor(&mut self) {
		self.cursor_idle_t = 0.0;
		self.cursor_step_at = Some(std::time::Instant::now());
		self.cursor_active_at = std::time::Instant::now();
		let settings = config::settings();
		let (period, full_phase) =
			cursor_cycle(&settings.cursor_animation, settings.cursor_blink_rate_ms);
		self.blink_t = full_phase * period;
		self.cursor_pause.resume();
	}

	// ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
	// Scrollbar
	// ••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

	fn bar_applies(&self, cfg: &config::Settings) -> bool {
		bar_applies_to(
			cfg,
			self.mode.contains(TermMode::ALT_SCREEN),
			self.scroll.max_lines(),
		)
	}

	// The user scrolled this pane: show the bar, and hold it up briefly afterwards
	// so a flick of the wheel doesn't leave it flickering. Deliberately NOT called
	// for output-driven scrolling - that happens constantly and would pin the bar
	// on-screen for the life of any busy pane.
	pub fn poke_scrollbar(&mut self) {
		self.bar_hold = BAR_HOLD_S;
	}

	// Advance the fade. Runs once per pane per frame, before the geometry is asked
	// for, and sets `bar_animating` so the event loop keeps rendering while it
	// moves (a bar resting at full or fully gone costs no frames).
	pub fn scrollbar_tick(&mut self, dt: f32, cfg: &config::Settings) {
		self.bar_hold = (self.bar_hold - dt).max(0.0);
		if let Some(at) = self.handle_drop_at
			&& drop_hold_over(self.scroll.animating(), at, self.scroll.target_lines())
		{
			self.handle_drop_at = None;
		}
		// Held up while: dragged, hovered, just scrolled, or parked in the
		// scrollback - up there, where you are IS the thing you want to see.
		let want = if !self.bar_applies(cfg) {
			0.0
		} else if !cfg.scrollbar_auto_hide
			|| self.bar_drag.is_some()
			|| self.bar_hover
			|| self.bar_hold > 0.0
			|| !self.scroll.following()
		{
			1.0
		} else {
			0.0
		};
		let tau = if want > self.bar_alpha {
			BAR_IN_TAU_S
		} else {
			BAR_OUT_TAU_S
		};
		self.bar_alpha += (want - self.bar_alpha) * (1.0 - (-dt / tau).exp());
		if (want - self.bar_alpha).abs() < BAR_VISIBLE_EPS {
			self.bar_alpha = want;
		}
		self.bar_animating = self.bar_alpha != want || self.bar_hold > 0.0;
	}

	// This frame's fade level, 0..1. Zero means the bar is not drawn AND not
	// clickable - the two must agree, or an invisible bar eats selection clicks.
	pub fn bar_fade(&self) -> f32 {
		self.bar_alpha
	}

	// Track and thumb in absolute window px, or None when there's nothing to show.
	// The bar hugs the pane's far right edge, past the minimap when there is one
	// (overlay, so it costs the grid no columns), and runs the height of the
	// content area.
	pub fn scrollbar(&self, ctx: &TextCtx, cfg: &config::Settings) -> Option<Bar> {
		if !self.bar_applies(cfg) || self.bar_alpha <= 0.0 {
			return None;
		}
		let (_, _, _, rows) = content_dims(self.rect, ctx);
		let thickness = ctx.dip(cfg.scrollbar_thickness).min(self.full.w);
		let track = bar_track(self.full, self.rect, thickness, ctx.margin);
		if track.h <= 0.0 || track.w <= 0.0 {
			return None;
		}
		let pos_lines = self.handle_lines();
		let (thumb_y, thumb_h) = bar_thumb_span(
			track.h,
			thickness,
			rows as f32,
			self.scroll.max_lines(),
			pos_lines,
		);
		Some(Bar {
			track,
			thumb: Rect {
				x: track.x,
				y: track.y + thumb_y,
				w: track.w,
				h: thumb_h,
			},
		})
	}

	// Would a press at (x, y) hit the bar, and where? None when the bar is
	// faded out, so clicks fall through to selection exactly when it's invisible.
	pub fn bar_hit(&self, x: f32, y: f32, ctx: &TextCtx, cfg: &config::Settings) -> Option<BarHit> {
		let bar = self.scrollbar(ctx, cfg)?;
		if self.bar_alpha < BAR_VISIBLE_EPS || !bar.track.contains(x, y) {
			return None;
		}
		if bar.thumb.contains(x, y) {
			Some(BarHit::Thumb)
		} else if y < bar.thumb.y {
			Some(BarHit::TrackUp)
		} else {
			Some(BarHit::TrackDown)
		}
	}

	// Is the pointer on (or just beside) the bar's strip? Drives the fade-in, so
	// it uses the strip rather than a hit: with auto-hide on there is no bar to
	// hit until this has already brought one back.
	pub fn bar_near(&self, x: f32, y: f32, ctx: &TextCtx, cfg: &config::Settings) -> bool {
		if !self.bar_applies(cfg) {
			return false;
		}
		let thickness = ctx.dip(cfg.scrollbar_thickness).min(self.full.w);
		let slop = ctx.dip(BAR_HOVER_SLOP);
		let strip = Rect {
			x: self.full.x + self.full.w - thickness - slop,
			y: self.rect.y,
			w: thickness + slop,
			h: self.rect.h,
		};
		strip.contains(x, y)
	}

	// Start a thumb drag, remembering where inside the handle it was grabbed so
	// the thumb doesn't jump under the pointer.
	pub fn bar_grab(&mut self, y: f32, ctx: &TextCtx, cfg: &config::Settings) {
		if let Some(bar) = self.scrollbar(ctx, cfg) {
			self.bar_drag = Some(y - bar.thumb.y);
			self.poke_scrollbar();
		}
	}

	// Continue a thumb drag: put the thumb's top where the pointer says, and map
	// that back to a scroll position.
	pub fn bar_drag_to(&mut self, y: f32, ctx: &TextCtx, cfg: &config::Settings) {
		let Some(grab) = self.bar_drag else { return };
		let Some(bar) = self.scrollbar(ctx, cfg) else {
			return;
		};
		let lines = bar_pos_to_lines(
			bar.track.h,
			bar.thumb.h,
			self.scroll.max_lines(),
			y - grab - bar.track.y,
		);
		self.scroll.scroll_to(lines);
		self.poke_scrollbar();
	}

	// Click on the track above/below the thumb: page that way, like every other
	// scrollbar. A page is the viewport less one line of overlap.
	pub fn bar_page(&mut self, up: bool, ctx: &TextCtx) {
		let (_, _, _, rows) = content_dims(self.rect, ctx);
		let page = (rows as f32 - 1.0).max(1.0);
		self.scroll.wheel(if up { page } else { -page });
		self.poke_scrollbar();
	}

	// Where the scrollbar thumb and the minimap marker both sit this frame. While
	// either one is being dragged they ride `target`, so the one under the pointer
	// tracks it exactly and the other stays level with it. Off a drag they ride
	// `visual`, so they move with the content they describe.
	fn handle_lines(&self) -> f32 {
		handle_pos(
			handle_is_dragged(self.bar_drag.is_some(), self.map_drag.is_some()),
			self.handle_drop_at.is_some(),
			self.scroll.target_lines(),
			self.scroll.visual_lines(),
		)
	}

	// End a handle drag. The handle stays where it was let go and the text eases
	// up to it, instead of snapping back onto text that is still travelling.
	pub fn release_handle(&mut self) {
		self.bar_drag = None;
		self.map_drag = None;
		self.handle_drop_at = Some(self.scroll.target_lines());
		self.poke_scrollbar();
	}

	// The minimap's pieces for this frame, or None when the column is off.
	pub fn minimap(&self, ctx: &TextCtx, cfg: &config::Settings) -> Option<minimap::Geom> {
		let rows = self.term.lines;
		let pos = self.handle_lines();
		let (total, shown) = self.map_span();
		minimap::geom(
			self.full,
			ctx.margin,
			ctx.scale,
			cfg,
			total,
			shown,
			rows,
			pos,
			self.mode.contains(TermMode::ALT_SCREEN),
			self.map_on,
		)
	}

	// The whole buffer and the part of it the map draws. The second stops where
	// the eased text has reached, so under a flood the column does not run on
	// past the last line on screen.
	fn map_span(&self) -> (usize, usize) {
		let hist = self.scroll.max_lines() as usize;
		let rows = self.term.lines;
		(hist + rows, self.map.shown_lines(hist, rows))
	}

	// The rasterized buffer and the image composed from it, for the renderer.
	pub fn map_cache(&self) -> &Minimap {
		&self.map
	}

	// When a compose the throttle deferred comes due.
	pub fn map_wake(&self) -> Option<std::time::Instant> {
		self.map.wake()
	}

	// Start a marker drag, remembering the pointer and what the view sat at.
	// Working from there rather than from the marker's drawn top is what keeps
	// a press with no movement from scrolling anything.
	pub fn map_grab(&mut self, y: f32, ctx: &TextCtx, cfg: &config::Settings) {
		if self.minimap(ctx, cfg).and_then(|g| g.handle).is_some() {
			self.map_drag = Some((y, self.scroll.visual_lines()));
			self.poke_scrollbar();
		}
	}

	pub fn map_drag_to(&mut self, y: f32, ctx: &TextCtx, cfg: &config::Settings) {
		let Some(grab) = self.map_drag else { return };
		let Some(g) = self.minimap(ctx, cfg) else {
			return;
		};
		let rows = self.term.lines;
		let (total, shown) = self.map_span();
		self.scroll
			.scroll_to(minimap::drag_to(&g, total, shown, rows, grab, y, ctx.scale));
		self.poke_scrollbar();
	}

	// A press outside the marker: center the view there, eased rather than cut.
	pub fn map_jump(&mut self, y: f32, ctx: &TextCtx, cfg: &config::Settings) {
		let Some(g) = self.minimap(ctx, cfg) else {
			return;
		};
		let rows = self.term.lines;
		let (total, shown) = self.map_span();
		self.scroll
			.scroll_to(minimap::center_on(&g, total, shown, rows, y, ctx.scale));
		self.poke_scrollbar();
	}

	// Coming back from a freeze (hidden tab shown, minimized window restored):
	// everything that happened meanwhile arrives as one instant cut. Snap any
	// leftover motion now and flag the next build to rebaseline its scroll
	// detectors instead of easing across the gap - that ease is the bounce class.
	pub fn hard_cut(&mut self) {
		self.pending_cut = true;
		self.scroll.snap();
	}

	// Sample the scrollback depth for this PTY read cycle. `history_size()` is a
	// COUNT, and `clear` (E3) TRUNCATES it, so growth measured between two frames
	// reads zero across a clear-and-refill that pushed a whole screenful past -
	// repeating `clear; ls -lA ~/` eased the first time and snapped every time
	// after, because the identical listing refilled the buffer to the identical
	// depth. A build never sees the dip (the clear and the output arrive in one
	// parse cycle), but a wakeup does, so accumulate here instead: a DROP can only
	// mean the scrollback was cleared, and everything left in it arrived after
	// that, so the whole of it is new. `try_lock_unfair` and give up on a miss -
	// this must never contend with the reader; the baseline only advances on a
	// successful sample, so the next one spans the cycles that were missed and
	// nothing is lost.
	// After a reflow the depth can change with nothing having scrolled, so
	// re-baseline rather than let the next sample read that as a clear. Resizes
	// are rare and this is bounded (one read cycle), so take the fair lock.
	pub fn rebaseline_history(&mut self) {
		self.depth
			.rebaseline(self.term.term.lock().grid().history_size());
	}

	pub fn note_history(&mut self) {
		let Some(guard) = self.term.term.try_lock_unfair() else {
			return;
		};
		let history = guard.grid().history_size();
		drop(guard);
		self.depth.sample(history);
	}

	// The cursor quad: visual column eased toward the target (slides as you type,
	// snaps on a row change), fade-blink alpha when idle, or None when hidden /
	// scrolled into history. Cheap; called every frame (incl. the cursor-only fast
	// path). Must run after the term lock is dropped (it takes &mut self).
	#[allow(clippy::too_many_arguments)]
	fn cursor_quad(
		&mut self,
		cursor_pt: Point,
		cursor_shape: CursorShape,
		cursor_geom: (f32, f32),
		display_offset: i32,
		lines: usize,
		following: bool,
		content_x: f32,
		cell_w: f32,
		cell_h: f32,
		margin: f32,
		voff: f32,
		dt: f32,
		cursor_rgb: [u8; 3],
		active: bool,
	) -> Option<RectInstance> {
		let cursor_screen_row = cursor_pt.line.0 + display_offset;
		let shown = following
			&& cursor_shape != CursorShape::Hidden
			&& cursor_screen_row >= 0
			&& (cursor_screen_row as usize) < lines;
		self.cursor_animating = false;
		if !shown {
			self.cursor_wake = None;
			return None;
		}
		// wall-clock dt for the pause timers: while parked no frames flow, so the
		// one wake/event frame after a sleep must account for the whole gap
		let step_now = std::time::Instant::now();
		let wall_dt = self
			.cursor_step_at
			.map_or(dt, |at| (step_now - at).as_secs_f32());
		self.cursor_step_at = Some(step_now);
		let target_col = cursor_pt.column.0 as f32;
		let row_jump = !self.cursor_init || cursor_screen_row != self.cursor_row;
		let moved = row_jump || (target_col - self.cursor_col).abs() > 0.001;
		if row_jump {
			self.cursor_x = target_col; // snap on first sight / newline (no diagonal slide)
		}
		if moved {
			self.cursor_idle_t = 0.0; // reset idle timer on any cursor move
			// Classify only on a move, so the verdict holds while the cursor sits
			// still - re-testing every frame would expire the echo window mid-pause
			// and cut typing's delay short.
			self.cursor_by_input = move_is_input(self.typed_at, step_now);
		} else {
			self.cursor_idle_t += wall_dt;
		}
		self.cursor_init = true;
		self.cursor_row = cursor_screen_row;
		self.cursor_col = target_col;
		// Ease toward the target column, speeding up the farther behind it is (a
		// burst catches up fast, a single-cell move keeps the gentle slide), never
		// trailing more than CURSOR_MAX_LAG cells.
		self.cursor_x = cursor_slide_step(self.cursor_x, target_col, dt);
		let easing = (target_col - self.cursor_x).abs() > 0.01;
		if !easing {
			self.cursor_x = target_col;
		}
		// Animation: "none" = steady; "phase" = smooth cosine fade; "pulse_*" =
		// grow/shrink a dimension over one cycle. The envelope applies whenever the
		// animation is on - including during a horizontal slide - so the size never
		// jumps on a keystroke. PauseState parks the cycle at full size while
		// typing (resuming cursor_animation_resume_s after input goes idle) and
		// again cursor_animation_idle_stop_s after the last input, indefinitely; both
		// pause AND resume happen at the cursor's full size, always. Output parks
		// it the same way, but carries no delay of its own (resume_delay), so the
		// cursor is alive again as soon as a command stops writing. A parked
		// cursor renders no frames - the timed resume comes from cursor_wake, and
		// a refocus ends the park outright (poke_cursor).
		let settings = config::settings();
		let anim = settings.cursor_animation.as_str();
		let (period, full_phase) = cursor_cycle(anim, settings.cursor_blink_rate_ms);
		let anim_on = anim != "none";
		let mut parked = false;
		self.cursor_wake = None;
		if anim_on && !CURSOR_ANIM_CONTINUOUS {
			let resume_s = resume_delay(self.cursor_by_input, settings.cursor_animation_resume_s);
			let idle_stop_s = settings.cursor_animation_idle_stop_s;
			// only the focused pane of the focused window animates; everyone else
			// parks at full and holds until they're the active pane again
			let blocked = FREEZE_UNFOCUSED_BLINK && !active;
			let input_idle_t = (step_now - self.cursor_active_at).as_secs_f32();
			self.blink_t = self.cursor_pause.advance(
				self.blink_t,
				dt,
				wall_dt,
				period,
				full_phase,
				resume_s,
				idle_stop_s,
				moved,
				self.cursor_idle_t,
				input_idle_t,
				blocked,
			);
			parked = self.cursor_pause.active && self.cursor_pause.parked;
			let idle_stopped = idle_stop_s > 0.0 && input_idle_t >= idle_stop_s;
			if let Some(wait) = cursor_wake_after(
				parked,
				idle_stopped,
				blocked,
				resume_s,
				self.cursor_idle_t,
				self.cursor_pause.hold_t,
			) {
				self.cursor_wake = Some(step_now + std::time::Duration::from_secs_f32(wait));
			}
		} else {
			self.blink_t += dt;
		}
		let phase = (self.blink_t / period).fract();
		let (w_frac, h_frac, alpha, pulsing_w, pulsing_h) =
			cursor_envelope(anim, phase, cursor_geom);
		// keep frames flowing while the cursor slides or the cycle runs. A parked
		// cursor is static at full size, so it needs NO frames - that is the whole
		// idle-CPU win; the timed resume is driven by cursor_wake instead
		self.cursor_animating = easing || (anim_on && !parked);
		let mut cursor_color = config::srgb_f32(cursor_rgb);
		cursor_color[3] = alpha;
		let cell_y = self.rect.y + margin + (cursor_screen_row as f32 + voff) * cell_h;
		let cell_x = content_x + self.cursor_x * cell_w;
		// Width grows from the left, height from the bottom - but a *pulsing*
		// dimension grows from the cell center (the "line in the middle") and may
		// shrink to nothing (the momentary disappear), so it skips the 2px floor.
		let w = if pulsing_w {
			cell_w * w_frac
		} else {
			(cell_w * w_frac).max(2.0)
		};
		let h = if pulsing_h {
			cell_h * h_frac
		} else {
			(cell_h * h_frac).max(2.0)
		};
		let x = if pulsing_w {
			cell_x + (cell_w - w) / 2.0
		} else {
			cell_x
		};
		let y = if pulsing_h {
			cell_y + (cell_h - h) / 2.0
		} else {
			cell_y + cell_h - h
		};
		Some(RectInstance {
			pos: [x, y],
			size: [w, h],
			color: cursor_color,
			..Default::default()
		})
	}

	// Same as `text_area` but for the scrim source pass: uses the de-bolded buffer
	// when it was built this frame (text_scrim_regular_weight + bold on screen), so
	// the halo weight matches non-bold text while the crisp text keeps its weight.
	pub fn scrim_text_area(&self, top: f32, margin: f32) -> TextArea<'_> {
		let mut area = self.text_area(top, margin);
		if self.scrim_debold {
			if let Some(scrim_buffer) = &self.scrim_buf {
				area.buffer = scrim_buffer;
			}
		}
		area
	}

	// scrim_text_area with the band clip of text_area_band (see there).
	pub fn scrim_text_area_band(
		&self,
		top: f32,
		margin: f32,
		clip_top: f32,
		clip_bottom: f32,
	) -> TextArea<'_> {
		let mut area = self.scrim_text_area(top, margin);
		area.bounds.top = area.bounds.top.max(clip_top as i32);
		area.bounds.bottom = area.bounds.bottom.min(clip_bottom as i32);
		area
	}

	fn buf_area<'a>(&'a self, buf: &'a Buffer, top: f32, margin: f32) -> TextArea<'a> {
		TextArea {
			buffer: buf,
			left: self.rect.x + margin,
			top,
			scale: 1.0,
			// clip to the content area (pane inset by the margin)
			bounds: TextBounds {
				left: (self.rect.x + margin) as i32,
				top: (self.rect.y + margin) as i32,
				right: (self.rect.x + self.rect.w - margin) as i32,
				bottom: (self.rect.y + self.rect.h - margin) as i32,
			},
			default_color: {
				// one settings() (RwLock read + Arc clone), not three
				let fg = config::settings().fg;
				GColor::rgb(fg[0], fg[1], fg[2])
			},
			custom_glyphs: &[],
		}
	}

	pub fn text_area(&self, top: f32, margin: f32) -> TextArea<'_> {
		self.buf_area(&self.buffer, top, margin)
	}

	// Same buffer as text_area, positioned at `top`, but with its vertical clip
	// narrowed to [clip_top, clip_bottom]. Used by the app-scroll slide to draw the
	// current buffer clipped to the scroll region and the static band separately.
	pub fn text_area_band(
		&self,
		top: f32,
		margin: f32,
		clip_top: f32,
		clip_bottom: f32,
	) -> TextArea<'_> {
		let mut area = self.text_area(top, margin);
		area.bounds.top = area.bounds.top.max(clip_top as i32);
		area.bounds.bottom = area.bounds.bottom.min(clip_bottom as i32);
		area
	}

	// The scrolled-off strip at its slide position, clipped to the scroll region
	// exactly like the current content (it holds only region rows, so the bands
	// need no protection from it; descender spill across the weld matches what
	// adjacent rows in one buffer do). None while the strip is empty. Serves the
	// scrim pass too - the strip is always scrim-safe, unlike the old retained
	// frame whose own-bg furniture had to be guarded out.
	pub fn strip_text_area<'a>(&'a self, slide: &Slide, margin: f32) -> Option<TextArea<'a>> {
		if slide.strip_rows.is_empty() || self.strip.shaped.is_empty() {
			return None;
		}
		let mut area = self.buf_area(&self.strip_buf, slide.strip_text_top, margin);
		area.bounds.top = area.bounds.top.max(slide.top_split_y as i32);
		area.bounds.bottom = area.bounds.bottom.min(slide.split_y as i32);
		Some(area)
	}

	// Re-shape the scrolled-off strip buffer from the captured rows in `window`.
	// Same span rules as build()'s main loop: runs merged by (color, bold, italic),
	// newlines embedded into non-empty runs, never empty/standalone spans (they
	// make set_rich_text loop forever). Glyphs the primary mono face lacks stay
	// space placeholders - the strip is transient reveal content, not worth a
	// per-cell fallback pool.
	fn shape_strip(
		&mut self,
		ctx: &mut TextCtx,
		settings: &config::Settings,
		window: std::ops::Range<usize>,
	) {
		fn flush(spans: &mut Vec<(String, Attrs)>, run: &mut String, style: ([u8; 3], bool, bool)) {
			if run.is_empty() {
				return;
			}
			let mut attrs = mono_attrs();
			attrs.color_opt = Some(GColor::rgb(style.0[0], style.0[1], style.0[2]));
			if style.1 {
				attrs.weight = crate::text::mono_bold_weight();
			}
			if style.2 {
				attrs.style = Style::Italic;
			}
			spans.push((std::mem::take(run), attrs));
		}
		self.strip.shaped = window.clone();
		if window.is_empty() {
			return;
		}
		let mut spans: Vec<(String, Attrs)> = Vec::with_capacity(window.len() + 1);
		let mut run = String::new();
		let mut run_style = (settings.fg, false, false);
		for (j, row) in self.strip.rows.range(window.clone()).enumerate() {
			if j != 0 {
				run.push('\n');
			}
			for cell in row {
				if cell.wide == 0 {
					continue; // wide-char spacer
				}
				if !cell.c.is_ascii() && !ctx.covered_at(cell.c, cell.wide) {
					for _ in 0..cell.wide {
						run.push(' ');
					}
					continue;
				}
				let style = (cell.fg, cell.bold, cell.italic);
				if style != run_style {
					flush(&mut spans, &mut run, run_style);
					run_style = style;
				}
				run.push(render_char(cell.c));
			}
		}
		flush(&mut spans, &mut run, run_style);
		ctx.resize_buffer(
			&mut self.strip_buf,
			self.rect.w.max(1.0),
			(window.len() as f32 + 1.0) * ctx.cell_h,
		);
		let span_refs = spans.iter().map(|(s, a)| (s.as_str(), a.clone()));
		self.strip_buf.set_rich_text(
			&mut ctx.font_system,
			span_refs,
			&mono_attrs(),
			Shaping::Advanced,
			None,
		);
		self.strip_buf
			.shape_until_scroll(&mut ctx.font_system, false);
	}

	// Per-cell fallback glyphs, already positioned (see Pane::build). Drawn in
	// the same text pass as `text_area`, on top of their space placeholders.
	// Iterator, not a Vec: both callers extend() into their own area list, so a
	// materialized intermediate was two throwaway allocations per frame.
	pub fn glyph_areas(&self, margin: f32) -> impl Iterator<Item = TextArea<'_>> {
		// content clip, same as buf_area: an edge row's fallback glyph (ink
		// taller than its cell, or shifted by a scroll fraction) must not
		// paint the margin - the main buffer's text never can
		let bounds = TextBounds {
			left: (self.rect.x + margin) as i32,
			top: (self.rect.y + margin) as i32,
			right: (self.rect.x + self.rect.w - margin) as i32,
			bottom: (self.rect.y + self.rect.h - margin) as i32,
		};
		self.glyphs
			.iter()
			.filter_map(move |&(key, x, y, color, scale)| {
				Some(TextArea {
					buffer: &self.glyph_cache.get(&key)?.buf,
					left: x,
					top: y,
					scale,
					bounds,
					default_color: color,
					custom_glyphs: &[],
				})
			})
	}

	// This frame's color glyphs (see Pane::build). One text area carries them all:
	// their coordinates are absolute, so it sits at the origin with an empty
	// buffer and exists only to hand glyphon the custom-glyph list.
	pub fn emoji_area(&self, margin: f32) -> Option<TextArea<'_>> {
		if self.emoji.is_empty() {
			return None;
		}
		Some(TextArea {
			buffer: &self.empty_buf,
			left: 0.0,
			top: 0.0,
			scale: 1.0,
			// content clip, same as glyph_areas
			bounds: TextBounds {
				left: (self.rect.x + margin) as i32,
				top: (self.rect.y + margin) as i32,
				right: (self.rect.x + self.rect.w - margin) as i32,
				bottom: (self.rect.y + self.rect.h - margin) as i32,
			},
			// unused: a color glyph carries its own pixels
			default_color: GColor::rgb(255, 255, 255),
			custom_glyphs: &self.emoji,
		})
	}

	// Copy-output: Enter was pressed at the shell prompt, so a command is (maybe)
	// about to run. Record where its output will begin (the line after the prompt/
	// echoed command) and arm the settle-based capture. Only arms at the shell
	// prompt, so an Enter inside a foreground app (vim, a REPL) doesn't arm.
	// Blocking (unfair) lock: a try_lock here silently skipped that command's
	// copy whenever Enter raced a PTY burst.
	pub fn arm_capture(&mut self) {
		if !self.term.at_shell_prompt() {
			return;
		}
		let guard = self.term.term.lock_unfair();
		let grid = guard.grid();
		let cursor_line = grid.cursor.point.line;
		self.cmd_start = grid.history_size() + cursor_line.0.max(0) as usize + 1;
		// fingerprint the prompt row so capture can re-find it (see cmd_anchor);
		// an all-blank row is too ambiguous to anchor on (blank output lines match)
		let cols = grid.columns();
		let row = &grid[cursor_line];
		let blank = (0..cols).all(|c| row[Column(c)].c == ' ');
		self.cmd_anchor = (!blank).then(|| fnv_row((0..cols).map(|c| row[Column(c)].c)));
		// learn the multi-line prompt block: rows above the input line that were
		// also there (same skeleton) at the previous arm are prompt, not output
		let hist = grid.history_size() as i32;
		let above: Vec<u64> = (1..=PROMPT_ABOVE_MAX as i32)
			.map_while(|up| {
				let line = Line(cursor_line.0 - up);
				if line.0 < -hist {
					return None;
				}
				let (skel, segments) = fnv_row_skel((0..cols).map(|c| grid[line][Column(c)].c));
				(segments >= PROMPT_SKEL_MIN).then_some(skel)
			})
			.collect();
		let confirmed = above
			.iter()
			.zip(&self.prompt_above)
			.take_while(|(cur, prev)| cur == prev)
			.count();
		self.prompt_block = above[..confirmed].to_vec();
		self.prompt_above = above;
		self.capture_armed = true;
		self.last_output = std::time::Instant::now();
	}

	// Cancel a pending capture. Called when the pane stops being the active copy
	// target (window unfocused, tab switched, focus moved, trigger turned off):
	// output that finished while the user was elsewhere must not copy late on
	// refocus - only a command launched after returning copies.
	pub fn disarm_capture(&mut self) {
		self.capture_armed = false;
	}

	// New PTY output arrived: push the settle deadline out so capture waits for the
	// command (and its prompt) to finish before copying.
	pub fn note_output(&mut self) {
		self.last_output = std::time::Instant::now();
		// windows: a returning prompt is itself output, so this is where the
		// at-prompt probe's cached answer goes stale (see TermInstance).
		self.term.note_activity();
	}

	// While armed, the instant the settle timer would fire (so the loop can wake to
	// check) - None when nothing is pending.
	pub fn capture_deadline(&self, settle: std::time::Duration) -> Option<std::time::Instant> {
		self.capture_armed
			.then(|| next_capture_poll(self.last_output, settle, self.capture_retry_at))
	}

	// If armed and the terminal has settled (no output for `settle`) back at the
	// shell prompt, return the command's output as plain Unicode text (control/
	// color codes are already gone - it's read from the parsed grid) and disarm.
	// Returns None otherwise, and skips empty output (e.g. a bare Enter or `cd`).
	pub fn poll_capture(&mut self, settle: std::time::Duration) -> Option<String> {
		if !self.capture_armed || self.last_output.elapsed() < settle {
			return None;
		}
		if !self.term.at_shell_prompt() {
			// a foreground app is still running; ask again later rather than
			// straight away - the deadline is otherwise in the past for the whole
			// life of a command that prints nothing, and the loop spins on it
			self.capture_retry_at = std::time::Instant::now() + CAPTURE_RETRY;
			return None;
		}
		let Some(guard) = self.term.term.try_lock_unfair() else {
			self.capture_retry_at = std::time::Instant::now() + CAPTURE_RETRY;
			return None;
		};
		self.capture_armed = false;
		let end = {
			let grid = guard.grid();
			grid.history_size() + grid.cursor.point.line.0.max(0) as usize
		};
		let start = capture_start(&guard, self.cmd_start, self.cmd_anchor, end);
		let end = prompt_strip(&guard, start, end, &self.prompt_block);
		let text = capture_grid_text(&guard, start, end);
		(!text.trim().is_empty()).then_some(text)
	}

	// Map a window pixel to a 0-based on-screen cell (col, row) within this pane's
	// viewport, for mouse reporting. Clamped to the grid; None if outside the pane.
	pub fn screen_cell_at(&self, x: f32, y: f32, ctx: &TextCtx) -> Option<(usize, usize)> {
		if !self.rect.contains(x, y) {
			return None;
		}
		let cols = self.term.cols as i32;
		let lines = self.term.lines as i32;
		let rel_x = (x - self.rect.x - ctx.margin).max(0.0);
		let col = ((rel_x / ctx.cell_w).floor() as i32).clamp(0, cols - 1);
		let row = ((y - self.rect.y - ctx.margin) / ctx.cell_h)
			.floor()
			.clamp(0.0, (lines - 1) as f32) as i32;
		Some((col as usize, row as usize))
	}

	// Map a window pixel to a grid point + which half of the cell, for selection.
	// Returns None if the pixel is outside this pane.
	pub fn point_at(&self, x: f32, y: f32, ctx: &TextCtx) -> Option<(Point, Side)> {
		self.rect
			.contains(x, y)
			.then(|| self.point_clamped(x, y, ctx))
	}

	// Same, but a pixel outside the pane is pulled to the nearest edge cell. This
	// is what a drag held past an edge wants: the selection runs on to the end of
	// what is on screen instead of stopping dead, and a drag that strays into a
	// neighboring pane still belongs to the one it started in.
	pub fn point_clamped(&self, x: f32, y: f32, ctx: &TextCtx) -> (Point, Side) {
		let display_offset = self.term.term.lock_unfair().grid().display_offset() as i32;
		grid_point(
			x,
			y,
			self.rect,
			ctx.margin,
			(ctx.cell_w, ctx.cell_h),
			(self.term.cols, self.term.lines),
			display_offset,
		)
	}

	// If a double-click `point` sits inside a matched pair on its line, return
	// the inside span (start..=end, same line) of the highest-precedence
	// enclosing non-empty pair. Single line only (multi-line pairs aren't
	// handled). `pairs` is (open, close) in precedence order.
	pub fn pair_span(&self, point: Point, pairs: &[(char, char)]) -> Option<(Point, Point)> {
		let cols = self.term.cols;
		let col = point.column.0;
		if col >= cols {
			return None;
		}
		let row: Vec<char> = {
			let guard = self.term.term.lock_unfair();
			let grid = guard.grid();
			(0..cols).map(|c| grid[point.line][Column(c)].c).collect()
		};
		let (start, end) = pair_inside(&row, col, pairs)?;
		Some((
			Point::new(point.line, Column(start)),
			Point::new(point.line, Column(end)),
		))
	}

	// A double-click on a bracket, when neither a shape nor a pair took it
	// (bracket_reach).
	pub fn bracket_span(&self, point: Point) -> Option<(Point, Point)> {
		let guard = self.term.term.lock_unfair();
		bracket_reach(guard.grid(), point)
	}

	// The shape (URL, path, scp target) covering `point`, if there is one, as
	// (first, last) cells. Spans a soft-wrapped line the way a hyperlink does,
	// since a long path is exactly the thing that wraps.
	pub fn shape_span(&self, point: Point) -> Option<(Point, Point)> {
		let cols = self.term.cols;
		if cols == 0 || point.column.0 >= cols {
			return None;
		}
		let guard = self.term.term.lock_unfair();
		let grid = guard.grid();
		let (top, bot) = (-(grid.history_size() as i32), self.term.lines as i32 - 1);
		let line = point.line.0;
		if line < top || line > bot {
			return None;
		}
		let end_col = Column(cols - 1);
		let wraps = |l: i32| grid[Line(l)][end_col].flags.contains(Flags::WRAPLINE);
		let mut first = line;
		while first > top && line - first < LINK_WRAP_ROWS && wraps(first - 1) {
			first -= 1;
		}
		let mut last = line;
		while last < bot && last - line < LINK_WRAP_ROWS && wraps(last) {
			last += 1;
		}
		let mut text = Vec::with_capacity((last - first + 1) as usize * cols);
		for l in first..=last {
			let row = &grid[Line(l)];
			text.extend((0..cols).map(|c| render_char(row[Column(c)].c)));
		}
		let hit = (line - first) as usize * cols + point.column.0;
		let (start, end) = crate::shapes::span_at(&text, hit)?;
		let point_of = |i: usize| Point::new(Line(first + (i / cols) as i32), Column(i % cols));
		Some((point_of(start), point_of(end - 1)))
	}

	// The whole logical line containing `point`, spanning soft-wrapped rows, as
	// (top-row col 0 .. bottom-row last col) - the span a triple-click selects.
	pub fn line_span(&self, point: Point) -> (Point, Point) {
		let cols = self.term.cols;
		let last_col = Column(cols.saturating_sub(1));
		let guard = self.term.term.lock_unfair();
		let grid = guard.grid();
		let top = -(grid.history_size() as i32);
		let bot = self.term.lines as i32 - 1;
		let wrapped = |l: i32| cols > 0 && grid[Line(l)][last_col].flags.contains(Flags::WRAPLINE);
		let (start, end) = logical_line_bounds(point.line.0, top, bot, wrapped);
		(
			Point::new(Line(start), Column(0)),
			Point::new(Line(end), last_col),
		)
	}

	pub fn begin_selection(&self, point: Point, side: Side, ty: SelectionType) {
		self.term.term.lock_unfair().selection = Some(Selection::new(ty, point, side));
	}

	pub fn update_selection(&self, point: Point, side: Side) {
		let mut guard = self.term.term.lock_unfair();
		if let Some(sel) = guard.selection.as_mut() {
			sel.update(point, side);
		}
	}

	pub fn clear_selection(&self) {
		self.term.term.lock_unfair().selection = None;
	}

	pub fn copy_enabled(&self, kind: CopyKind) -> bool {
		match kind {
			CopyKind::Select => self.copy_select,
			CopyKind::Output => self.copy_output,
		}
	}

	pub fn set_copy(&mut self, kind: CopyKind, on: bool) {
		match kind {
			CopyKind::Select => self.copy_select = on,
			CopyKind::Output => self.copy_output = on,
		}
	}

	pub fn selection_text(&self) -> Option<String> {
		self.term
			.term
			.lock_unfair()
			.selection_to_string()
			.filter(|s| !s.is_empty())
	}

	// Anything the user's own hands sent - typing, a mouse report, the wheel
	// driving an app's cursor keys. Read-only withholds all of it. A reply the
	// terminal owes the program is not this, and goes straight to `term.write`.
	pub fn write_input<B: Into<Vec<u8>>>(&self, bytes: B) {
		if self.read_only {
			return;
		}
		self.term.write(bytes);
	}

	// Write pasted text to the PTY (wrapped in bracketed paste when the app
	// enabled it, and put through paste_payload either way). No-op when the
	// pane is read-only.
	pub fn paste(&mut self, text: &str) {
		if self.read_only || text.is_empty() {
			return;
		}
		let bracket = self.mode.contains(TermMode::BRACKETED_PASTE);
		let payload = paste_payload(text, bracket);
		let mut bytes = Vec::with_capacity(payload.len() + 12);
		if bracket {
			bytes.extend_from_slice(b"\x1b[200~");
		}
		bytes.extend_from_slice(payload.as_bytes());
		if bracket {
			bytes.extend_from_slice(b"\x1b[201~");
		}
		self.term.write(bytes);
		self.note_typed();
	}
}

pub struct PaneManager {
	pub panes: HashMap<PaneId, Pane>,
	root: Node,
	pub focused: PaneId,
	// CLI `--title` for this tab; overrides the computed "<shell> [program]".
	pub title_override: Option<String>,
	// When this tab was opened, for the tip's elapsed time. A tab, not a pane:
	// splitting one does not start it over.
	pub created: std::time::Instant,
}

impl PaneManager {
	pub fn new(
		ctx: &mut TextCtx,
		proxy: &EventLoopProxy<UserEvent>,
		area: Rect,
		command: Option<Vec<String>>,
		cwd: Option<std::path::PathBuf>,
	) -> anyhow::Result<Self> {
		let id = alloc_pane_id();
		let pane = spawn_pane(ctx, proxy, id, area, command, cwd)?;
		let mut panes = HashMap::new();
		panes.insert(id, pane);
		Ok(Self {
			panes,
			root: Node::Leaf(id),
			focused: id,
			title_override: None,
			created: std::time::Instant::now(),
		})
	}

	// Interactive split (menu/keyboard): even ratio, new pane after; inherits the
	// source pane's command and current directory, so the new pane runs the same
	// shell it forked off, starting where that shell is now.
	pub fn split(
		&mut self,
		ctx: &mut TextCtx,
		proxy: &EventLoopProxy<UserEvent>,
		id: PaneId,
		dir: Dir,
		area: Rect,
	) {
		let cmd = self.panes.get(&id).and_then(|p| p.command.clone());
		self.split_cmd(ctx, proxy, id, dir, cmd, area);
	}

	// The same split, but the new pane runs `command` - a shell picked off the
	// menu - and still starts where the source pane's shell is.
	pub fn split_with(
		&mut self,
		ctx: &mut TextCtx,
		proxy: &EventLoopProxy<UserEvent>,
		id: PaneId,
		dir: Dir,
		command: Vec<String>,
		area: Rect,
	) {
		self.split_cmd(ctx, proxy, id, dir, Some(command), area);
	}

	fn split_cmd(
		&mut self,
		ctx: &mut TextCtx,
		proxy: &EventLoopProxy<UserEvent>,
		id: PaneId,
		dir: Dir,
		command: Option<Vec<String>>,
		area: Rect,
	) {
		let cwd = self.panes.get(&id).and_then(|p| p.term.cwd());
		// interactive splits even-distribute the same-direction run (unless a divider
		// in it was hand-dragged)
		self.split_at(ctx, proxy, id, dir, false, None, command, cwd, area);
	}

	// What the tab has to say about itself: the command its focused pane was
	// launched with (None = whatever the default shell is), what that shell is
	// running, and where it is now. `&mut` because asking what is running costs a
	// probe, which the term throttles and caches for itself.
	pub fn tab_facts(
		&mut self,
	) -> (
		Option<Vec<String>>,
		crate::term::Task,
		Option<std::path::PathBuf>,
	) {
		let focused_id = self.focused;
		self.panes.get_mut(&focused_id).map_or_else(
			|| (None, crate::term::Task::Idle, None),
			|pane| (pane.command.clone(), pane.term.task(), pane.term.cwd()),
		)
	}

	// What a new tab/window spawned "from" the focused pane should inherit:
	// its launch command (None = default shell) and the shell's current dir.
	pub fn inherit_spawn(&self) -> (Option<Vec<String>>, Option<std::path::PathBuf>) {
		self.panes
			.get(&self.focused)
			.map_or((None, None), |pane| (pane.command.clone(), pane.term.cwd()))
	}

	// General split used by the CLI: split `id` along `dir`, the new pane on the
	// `before` side (a) or after (b); runs `command`. Returns the new pane id (None
	// if `id` wasn't a leaf). `new_ratio` is the new pane's share, kept as given;
	// None evens out the same-direction run it joined, see `place_split`.
	pub fn split_at(
		&mut self,
		ctx: &mut TextCtx,
		proxy: &EventLoopProxy<UserEvent>,
		id: PaneId,
		dir: Dir,
		before: bool,
		new_ratio: Option<f32>,
		command: Option<Vec<String>>,
		cwd: Option<std::path::PathBuf>,
		area: Rect,
	) -> Option<PaneId> {
		// leaves mirror `panes`, so this is also "is id a leaf" - checked up
		// front so a doomed insert can't spawn (then kill) a shell
		if !self.panes.contains_key(&id) {
			return None;
		}
		let new_id = alloc_pane_id();
		// a new pane inherits the auto-copy flags of the pane it split off (the
		// "tab setting" the user sees); a new tab/window starts from the
		// copy_on_select config default (output always off)
		let (inherit_select, inherit_output) = self
			.panes
			.get(&id)
			.map_or((false, false), |src| (src.copy_select, src.copy_output));
		// spawn BEFORE touching the tree: a failed spawn must not leave a
		// phantom leaf that reserves layout space with no pane behind it
		let mut pane = match spawn_pane(ctx, proxy, new_id, area, command, cwd) {
			Ok(p) => p,
			Err(e) => {
				eprintln!("split: failed to spawn shell: {e}");
				return None;
			}
		};
		pane.copy_select = inherit_select;
		pane.copy_output = inherit_output;
		if !place_split(&mut self.root, id, dir, new_id, before, new_ratio) {
			return None;
		}
		self.panes.insert(new_id, pane);
		self.focused = new_id;
		self.relayout(ctx, area);
		Some(new_id)
	}

	// returns true when the last pane closed (caller should exit)
	pub fn close(&mut self, ctx: &mut TextCtx, id: PaneId, area: Rect) -> bool {
		if let Some(n) = prune(std::mem::replace(&mut self.root, Node::Leaf(0)), id) {
			self.root = n;
			self.panes.remove(&id);
			if self.focused == id {
				self.focused = first_leaf(&self.root);
			}
			self.relayout(ctx, area);
			false
		} else {
			self.panes.remove(&id);
			true
		}
	}

	// Recreate each pane's text buffer from `ctx`'s font system. Needed after a
	// TextCtx rebuild (font size / line height change) since buffers are tied to
	// the FontSystem they were made with. Follow with `relayout`.
	pub fn rebuild_buffers(&mut self, ctx: &mut TextCtx) {
		for pane in self.panes.values_mut() {
			pane.buffer = ctx.new_buffer(pane.rect.w.max(1.0), pane.rect.h.max(1.0));
			pane.strip_buf = ctx.new_buffer(pane.rect.w.max(1.0), ctx.cell_h);
			pane.strip.clear(); // metrics changed; a mid-slide strip would misalign
			pane.strip_dirty = false;
			pane.glyph_cache.clear(); // cached glyphs are tied to the old font/metrics
			// this frame's keys point into the cache just emptied, and `build` only
			// clears them past its lock-miss early return
			pane.glyphs.clear();
			pane.scrim_buf = None; // ditto the de-bold scrim buffer
			pane.text_built = false; // fresh empty buffer: force a full rebuild next frame
		}
	}

	// The minimap column comes and goes with what is running, and losing it gives
	// the text its columns back - so it is a relayout, not a draw-time choice.
	// Answers whether any pane changed its mind, which is the caller's cue to do
	// one. Both probes behind it are cheap: the mode is already cached from the
	// last build, and the program lookup is throttled inside `task`.
	pub fn sync_minimap(&mut self, cfg: &config::Settings) -> bool {
		let mut moved = false;
		for pane in self.panes.values_mut() {
			let alt = pane.mode.contains(TermMode::ALT_SCREEN);
			let running = match pane.term.task() {
				crate::term::Task::Running(name) => Some(name),
				_ => None,
			};
			let want = minimap::wanted(cfg, alt, running.as_deref());
			if want != pane.map_on {
				pane.map_on = want;
				moved = true;
			}
		}
		moved
	}

	pub fn relayout(&mut self, ctx: &mut TextCtx, area: Rect) {
		let mut out = Vec::new();
		layout(&self.root, area, ctx.scale, &mut out);
		let cfg = config::settings();
		for (id, rect) in out {
			if let Some(pane) = self.panes.get_mut(&id) {
				pane.full = rect;
				pane.rect = minimap::text_rect(rect, &cfg, ctx.scale, pane.map_on);
				if !cfg.minimap {
					pane.map.clear();
				}
				let rect = pane.rect;
				let (cw, ch, cols, lines) = content_dims(rect, ctx);
				pane.term
					.resize(cols, lines, ctx.cell_w as u16, ctx.cell_h as u16);
				ctx.resize_buffer(&mut pane.buffer, cw, text_buffer_h(ch, ctx.cell_h));
				// a resize invalidates the strip's captured columns and the
				// frame-old styled snapshot it fills from
				pane.strip.clear();
				// a reflow can shrink history with nothing scrolled, which would
				// otherwise read as a scrollback clear
				pane.rebaseline_history();
				pane.out_band = 0;
				pane.strip_dirty = false;
				pane.last_cells.clear();
			}
		}
	}

	pub fn pane_at(&self, x: f32, y: f32) -> Option<PaneId> {
		self.panes
			.iter()
			.find(|(_, p)| p.full.contains(x, y))
			.map(|(id, _)| *id)
	}

	// A grabbable divider under the cursor: its path in the split-tree and
	// orientation (for the resize cursor).
	pub fn divider_at(&self, x: f32, y: f32, area: Rect, scale: f32) -> Option<(Vec<bool>, Dir)> {
		let mut path = Vec::new();
		divider_at(&self.root, area, x, y, scale, &mut path).map(|dir| (path, dir))
	}

	// Drag a divider (identified by `path`) to the cursor and relayout.
	pub fn drag_divider(&mut self, ctx: &mut TextCtx, path: &[bool], area: Rect, x: f32, y: f32) {
		set_ratio(&mut self.root, area, path, x, y, ctx.scale);
		self.relayout(ctx, area);
	}

	// Swap two panes' positions in the split-tree (drag-and-drop reorder).
	pub fn swap_panes(&mut self, ctx: &mut TextCtx, a: PaneId, b: PaneId, area: Rect) {
		if a == b {
			return;
		}
		swap_leaves(&mut self.root, a, b);
		self.relayout(ctx, area);
	}
}

fn swap_leaves(node: &mut Node, a: PaneId, b: PaneId) {
	match node {
		Node::Leaf(id) => {
			if *id == a {
				*id = b;
			} else if *id == b {
				*id = a;
			}
		}
		Node::Split {
			a: child_a,
			b: child_b,
			..
		} => {
			swap_leaves(child_a, a, b);
			swap_leaves(child_b, a, b);
		}
	}
}

// The grid point and cell half under a window pixel, pulled to the nearest edge
// cell from outside the pane. `cell` is (width, height) and `grid` (cols,
// lines). A view scrolled back gives a history line, which is negative.
fn grid_point(
	x: f32,
	y: f32,
	rect: Rect,
	margin: f32,
	cell: (f32, f32),
	grid: (usize, usize),
	display_offset: i32,
) -> (Point, Side) {
	let (cell_w, cell_h) = cell;
	let cols = grid.0 as i32;
	let lines = grid.1 as i32;
	let rel_x = (x - rect.x - margin).max(0.0);
	// clamp the column BEFORE the half-cell test, so a pointer past the right
	// edge reads as the far half of the last cell and takes it whole
	let colf = (rel_x / cell_w).floor().clamp(0.0, (cols - 1) as f32);
	let col = colf as i32;
	let side = if rel_x - colf * cell_w < cell_w / 2.0 {
		Side::Left
	} else {
		Side::Right
	};
	let screen_row = ((y - rect.y - margin) / cell_h)
		.floor()
		.clamp(0.0, (lines - 1) as f32) as i32;
	(
		Point::new(Line(screen_row - display_offset), Column(col as usize)),
		side,
	)
}

// The height a pane's text buffer is given: the content plus two rows of slack.
// `build` lays out lines+1 rows (the overscan row above the viewport, then the
// screen), so the last one sits right at the content height, and when that is
// an exact multiple of the cell height the text layout drops it - the bottom
// line goes invisible. The pane's text bounds still clip the drawing.
fn text_buffer_h(content_h: f32, cell_h: f32) -> f32 {
	content_h + 2.0 * cell_h
}

// content area (pane inset by the margin) in pixels and in cells
fn content_dims(rect: Rect, ctx: &TextCtx) -> (f32, f32, usize, usize) {
	let cw = (rect.w - 2.0 * ctx.margin).max(ctx.cell_w);
	let ch = (rect.h - 2.0 * ctx.margin).max(ctx.cell_h);
	let cols = (cw / ctx.cell_w).floor().max(1.0) as usize;
	let lines = (ch / ctx.cell_h).floor().max(1.0) as usize;
	(cw, ch, cols, lines)
}

fn spawn_pane(
	ctx: &mut TextCtx,
	proxy: &EventLoopProxy<UserEvent>,
	id: PaneId,
	rect: Rect,
	command: Option<Vec<String>>,
	cwd: Option<std::path::PathBuf>,
) -> anyhow::Result<Pane> {
	// A pane with no command of its own runs the default shell - resolved HERE
	// and then remembered, rather than left as "whatever the default is". The
	// list can change under a running pane: the background scan fills it a few
	// seconds after launch, and the Shells tab reorders it. Leaving it unresolved
	// made the tab name whichever shell was first at the moment somebody LOOKED,
	// which is how a pane running PowerShell came to be labelled Command Prompt.
	// It stays None only when nothing is switched on at all, where the engine
	// picks its own default and there is genuinely nothing to report.
	let command = command.or_else(config::default_shell_argv);
	let text = minimap::text_rect(rect, &config::settings(), ctx.scale, true);
	let (cw, ch, cols, lines) = content_dims(text, ctx);
	let term = TermInstance::spawn(
		id,
		cols,
		lines,
		ctx.cell_w as u16,
		ctx.cell_h as u16,
		proxy.clone(),
		command.clone(),
		cwd,
	)?;
	let buffer = ctx.new_buffer(cw, text_buffer_h(ch, ctx.cell_h));
	let strip_buf = ctx.new_buffer(cw, ctx.cell_h);
	let empty_buf = ctx.new_plain_buffer(); // never given text - see emoji_area
	Ok(Pane {
		id,
		term,
		scroll: Scroll::new(),
		buffer,
		strip: OffStrip::new(),
		strip_buf,
		strip_dirty: false,
		last_cells: Vec::new(),
		cells_scratch: Vec::new(),
		rows_scratch: Vec::new(),
		rect: text,
		full: rect,
		title: String::new(), // what the running program asked the window be called
		read_only: false,
		keep_open: false,
		held: false,
		command,
		last_draw: PaneDraw {
			top: rect.y,
			bg: Vec::new(),
			links: Vec::new(),
			cursor: None,
			slide: None,
		},
		lock_misses: 0,
		depth: DepthSampler::default(),
		last_rows: Vec::new(),
		slide_static: 0,
		slide_static_top: 0,
		out_band: 0,
		slide_sh: 0.0,
		last_alt: false,
		pending_cut: false,
		glyph_cache: HashMap::new(),
		glyphs: Vec::new(),
		emoji: Vec::new(),
		empty_buf,
		readable: palette::Readable::default(),
		scrim_buf: None,
		scrim_debold: false,
		cursor_x: 0.0,
		cursor_col: 0.0,
		cursor_row: i32::MIN,
		cursor_init: false,
		blink_t: 0.0,
		cursor_idle_t: 0.0,
		cursor_step_at: None,
		cursor_active_at: std::time::Instant::now(),
		typed_at: None,
		cursor_by_input: false,
		cursor_pause: PauseState::default(),
		cursor_wake: None,
		bar_alpha: 0.0,
		bar_hold: 0.0,
		bar_hover: false,
		bar_drag: None,
		bar_animating: false,
		map: Minimap::default(),
		map_drag: None,
		handle_drop_at: None,
		hover_px: None,
		link_probe: false,
		link_hover: None,
		cursor_animating: false,
		text_built: false,
		shape_rev: 0,
		mode: TermMode::empty(),
		map_on: true,
		content_dirty: true,
		copy_select: config::settings().copy_on_select,
		copy_output: false,
		prompt_above: Vec::new(),
		prompt_block: Vec::new(),
		capture_armed: false,
		capture_retry_at: std::time::Instant::now(),
		cmd_start: 0,
		cmd_anchor: None,
		last_output: std::time::Instant::now(),
	})
}

// Where the captured output starts, as a capture-time absolute line index.
// `cmd_start` was recorded at arm time in "history + row" coordinates, but that
// origin moves once the scrollback is at cap: each pushed line evicts the
// oldest, shifting every absolute index down, so the stale index falls past the
// start and the copy silently drops the first lines of the output. Re-find the
// arm-time prompt row by its content hash instead, scanning back from the end
// (the nearest match is the arm-time prompt unless the output itself repeats
// that exact row); the output starts on the next line. Fall back to `cmd_start`
// when there's no anchor or no match (blank prompt row, the row was evicted, or
// the shell redrew it on Enter).
fn capture_start<T: alacritty_terminal::event::EventListener>(
	term: &Term<T>,
	cmd_start: usize,
	anchor: Option<u64>,
	end_abs: usize,
) -> usize {
	let Some(anchor) = anchor else {
		return cmd_start;
	};
	let grid = term.grid();
	let hist = grid.history_size() as i64;
	let cols = grid.columns();
	for abs in (0..end_abs).rev() {
		let row = &grid[Line((abs as i64 - hist) as i32)];
		if fnv_row((0..cols).map(|c| row[Column(c)].c)) == anchor {
			return abs + 1;
		}
	}
	cmd_start
}

// Drop a multi-line prompt's extra rows off the capture end. `end_abs` already
// excludes the resumed prompt's input line (the cursor row); strip the rows
// above it whose skeleton matches the learned prompt block (see prompt_block
// on Pane), so e.g. a two-line prompt's decoration line isn't copied as output
// even when its content (cwd, clock) changed since the arm. Fail-safe: a row
// whose structure doesn't match (nothing learned yet, genuinely different row)
// stops the strip and stays in the copy - the prior behavior.
fn prompt_strip<T: alacritty_terminal::event::EventListener>(
	term: &Term<T>,
	start_abs: usize,
	end_abs: usize,
	block: &[u64],
) -> usize {
	let grid = term.grid();
	let hist = grid.history_size() as i64;
	let cols = grid.columns();
	let mut end = end_abs;
	for &fingerprint in block {
		if end <= start_abs {
			break;
		}
		let row = &grid[Line((end as i64 - 1 - hist) as i32)];
		if fnv_row_skel((0..cols).map(|c| row[Column(c)].c)).0 != fingerprint {
			break;
		}
		end -= 1;
	}
	end
}

// Extract the grid text for absolute line range [start_abs, end_abs) as plain
// Unicode. Absolute index 0 is the oldest line currently in the buffer; screen
// row 0 sits at absolute `history_size`. Trailing pad spaces are trimmed and a
// newline is emitted per grid row, except rows flagged WRAPLINE (a soft-wrapped
// long line) which join to the next. Lines evicted from scrollback (only when a
// command's output exceeds the scrollback limit) are skipped.
fn capture_grid_text<T: alacritty_terminal::event::EventListener>(
	term: &Term<T>,
	start_abs: usize,
	end_abs: usize,
) -> String {
	let grid = term.grid();
	let hist = grid.history_size() as i64;
	let cols = grid.columns();
	let mut out = String::new();
	let mut abs_line = start_abs;
	while abs_line < end_abs {
		let grid_line = abs_line as i64 - hist; // screen top is absolute `hist`; history is negative
		if grid_line < -hist {
			abs_line += 1; // scrolled out of the buffer (output longer than scrollback)
			continue;
		}
		let row = &grid[Line(grid_line as i32)];
		let mut row_text = String::new();
		for c in 0..cols {
			let cell = &row[Column(c)];
			if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
				continue; // the trailing half of a wide glyph has no char of its own
			}
			row_text.push(cell.c);
		}
		if cols > 0 && row[Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
			out.push_str(&row_text); // soft-wrapped: continue the logical line, no newline
		} else {
			out.push_str(row_text.trim_end());
			out.push('\n');
		}
		abs_line += 1;
	}
	out
}

// What a paste actually puts on the wire.
//
// UNBRACKETED, the application cannot tell a paste from typing, so a line break
// has to arrive the way the Enter key delivers one - a lone CR. Sending the LF
// as well leaves a shell sitting on a continuation line after every row, and a
// Windows clipboard is CRLF by nature, so that is the ORDINARY case there and
// not an edge one.
//
// BRACKETED, the text goes over as the application asked for it - line breaks
// included - EXCEPT for ESC: one carried in the payload closes the bracket
// early (the application is watching for `ESC[201~`), and everything after it
// is then read as keystrokes rather than as data, which is how a paste runs a
// command nobody typed. Dropping ESC is what makes the bracket a real boundary.
fn paste_payload(text: &str, bracket: bool) -> String {
	if bracket {
		text.replace('\x1b', "")
	} else {
		text.replace("\r\n", "\r").replace('\n', "\r")
	}
}

// Inside span (start..=end columns) of the highest-precedence matched pair that
// encloses `col` on `row`. `pairs` is (open, close) in precedence order; the
// first enclosing non-empty pair wins (so e.g. inside `()` selects the `()`
// contents even if a lower-precedence `[]` is nested within). None -> no pair.
// How far a double-click on a bracket looks for its partner, in rows.
const BRACKET_REACH_ROWS: usize = 200;

// A double-click on a bracket selects from it to its partner, or the bracket
// alone when none is in reach, and None means the cell is not a bracket. The
// engine does the same when handed a word selection on one, but it looks
// through the whole scrollback under the terminal lock, and again on every
// rebuild while the selection stands: 86 ms a time at 100,000 lines.
fn bracket_reach(grid: &Grid<Cell>, point: Point) -> Option<(Point, Point)> {
	const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
	let here = grid[point].c;
	let (forward, partner) = PAIRS.iter().find_map(|&(open, close)| {
		if here == open {
			Some((true, close))
		} else if here == close {
			Some((false, open))
		} else {
			None
		}
	})?;
	let mut cells = grid.iter_from(point);
	let mut nested = 0usize;
	for _ in 0..BRACKET_REACH_ROWS * grid.columns() {
		let Some(cell) = (if forward { cells.next() } else { cells.prev() }) else {
			break;
		};
		if cell.c == partner {
			if nested == 0 {
				return Some(if forward {
					(point, cell.point)
				} else {
					(cell.point, point)
				});
			}
			nested -= 1;
		} else if cell.c == here {
			nested += 1;
		}
	}
	Some((point, point))
}

fn pair_inside(row: &[char], col: usize, pairs: &[(char, char)]) -> Option<(usize, usize)> {
	for &(open, close) in pairs {
		let found = if open == close {
			same_char_pair(row, col, open)
		} else {
			distinct_pair(row, col, open, close)
		};
		if let Some((open_idx, close_idx)) = found {
			if close_idx > open_idx + 1 {
				// Exclude runs of spaces directly against the delimiters (keep any
				// interior spaces): `" Now is the time. "` selects `Now is the time.`.
				let (mut start, mut end) = (open_idx + 1, close_idx - 1);
				while start < end && row[start] == ' ' {
					start += 1;
				}
				while end > start && row[end] == ' ' {
					end -= 1;
				}
				// all-spaces inside: fall back to the full inside span
				return Some(if row[start] == ' ' {
					(open_idx + 1, close_idx - 1)
				} else {
					(start, end)
				});
			}
		}
	}
	None
}

// Innermost matched (open,close) pair enclosing `col` on `row`, for distinct
// open/close chars. The char at `col` itself isn't treated as an endpoint.
fn distinct_pair(row: &[char], col: usize, open: char, close: char) -> Option<(usize, usize)> {
	let mut depth = 0i32;
	let mut open_idx = None;
	for i in (0..col).rev() {
		if row[i] == close {
			depth += 1;
		} else if row[i] == open {
			if depth == 0 {
				open_idx = Some(i);
				break;
			}
			depth -= 1;
		}
	}
	let open_idx = open_idx?;
	let mut depth = 0i32;
	for (i, &ch) in row.iter().enumerate().skip(col + 1) {
		if ch == open {
			depth += 1;
		} else if ch == close {
			if depth == 0 {
				return Some((open_idx, i));
			}
			depth -= 1;
		}
	}
	None
}

// Pair of identical chars (quotes) enclosing `col`: occurrences pair off
// left-to-right; `col` is inside the pair strictly between two of them.
fn same_char_pair(row: &[char], col: usize, ch: char) -> Option<(usize, usize)> {
	let pos: Vec<usize> = row
		.iter()
		.enumerate()
		.filter(|&(_, &c)| c == ch)
		.map(|(i, _)| i)
		.collect();
	let mut i = 0;
	while i + 1 < pos.len() {
		if pos[i] < col && col < pos[i + 1] {
			return Some((pos[i], pos[i + 1]));
		}
		i += 2;
	}
	None
}

// Split the leaf `id` into a `dir` Split. `before` puts the new pane on the
// a-side (left/top); `ratio_a` is child-a's fraction of the split.
fn insert_split_at(
	node: &mut Node,
	id: PaneId,
	dir: Dir,
	new_id: PaneId,
	before: bool,
	ratio_a: f32,
) -> bool {
	match node {
		Node::Leaf(i) if *i == id => {
			let old = *i;
			let (a, b) = if before { (new_id, old) } else { (old, new_id) };
			*node = Node::Split {
				dir,
				ratio: ratio_a,
				manual: false,
				a: Box::new(Node::Leaf(a)),
				b: Box::new(Node::Leaf(b)),
			};
			true
		}
		Node::Leaf(_) => false,
		Node::Split { a, b, .. } => {
			insert_split_at(a, id, dir, new_id, before, ratio_a)
				|| insert_split_at(b, id, dir, new_id, before, ratio_a)
		}
	}
}

// Put leaf `new_id` beside leaf `id`. A given share is kept: the split counts as
// sized by hand, so no later split in its run evens it out. With none the new
// pane takes half, then the same-direction run it joined is evened out, unless a
// divider in it was sized by hand (then successive splits stay 50/50).
fn place_split(
	root: &mut Node,
	id: PaneId,
	dir: Dir,
	new_id: PaneId,
	before: bool,
	new_ratio: Option<f32>,
) -> bool {
	let share = new_ratio.unwrap_or(0.5);
	// child-a's ratio: if the new pane is 'a' (before) it takes the share, else
	// 'a' is the old pane and keeps the remainder
	let ratio_a = if before { share } else { 1.0 - share };
	if !insert_split_at(root, id, dir, new_id, before, ratio_a.clamp(0.05, 0.95)) {
		return false;
	}
	if new_ratio.is_none() {
		equalize_dir_run(root, new_id, dir);
	} else if let Some(path) = path_to(root, new_id) {
		if let Node::Split { manual, .. } = node_at_mut(root, &path[..path.len() - 1]) {
			*manual = true;
		}
	}
	true
}

// Path (false = a-child, true = b-child) from `node` down to leaf `id`, if present.
fn path_to(node: &Node, id: PaneId) -> Option<Vec<bool>> {
	match node {
		Node::Leaf(i) => (*i == id).then(Vec::new),
		Node::Split { a, b, .. } => {
			if let Some(mut path) = path_to(a, id) {
				path.insert(0, false);
				return Some(path);
			}
			if let Some(mut path) = path_to(b, id) {
				path.insert(0, true);
				return Some(path);
			}
			None
		}
	}
}

// Follow `path` from `node` (defensively stops at a leaf).
fn node_at_mut<'a>(mut node: &'a mut Node, path: &[bool]) -> &'a mut Node {
	for &take_b in path {
		let Node::Split { a, b, .. } = node else {
			break;
		};
		node = if take_b { b } else { a };
	}
	node
}

// Is the node at `path` a Split oriented along `dir`?
fn is_dir_split(root: &Node, path: &[bool], dir: Dir) -> bool {
	let mut node = root;
	for &take_b in path {
		let Node::Split { a, b, .. } = node else {
			return false;
		};
		node = if take_b { b } else { a };
	}
	matches!(node, Node::Split { dir: node_dir, .. } if *node_dir == dir)
}

// Leaves in the same-direction run rooted at `node`: a nested `dir` split counts
// its members; a leaf or a differently-oriented split counts as one unit (its own
// internal layout is separate).
fn group_leaf_count(node: &Node, dir: Dir) -> usize {
	match node {
		Node::Split {
			dir: node_dir,
			a,
			b,
			..
		} if *node_dir == dir => group_leaf_count(a, dir) + group_leaf_count(b, dir),
		_ => 1,
	}
}

// Has any divider in the same-direction run been hand-dragged?
fn group_has_manual(node: &Node, dir: Dir) -> bool {
	match node {
		Node::Split {
			dir: node_dir,
			manual,
			a,
			b,
			..
		} if *node_dir == dir => *manual || group_has_manual(a, dir) || group_has_manual(b, dir),
		_ => false,
	}
}

// Set every ratio in the same-direction run so all its member leaves are equal:
// a split gives its a-child a share proportional to the leaves under it.
fn equalize(node: &mut Node, dir: Dir) {
	if let Node::Split {
		dir: node_dir,
		ratio,
		a,
		b,
		..
	} = node
	{
		if *node_dir == dir {
			let leaves_a = group_leaf_count(a, dir);
			let leaves_b = group_leaf_count(b, dir);
			*ratio = leaves_a as f32 / (leaves_a + leaves_b) as f32;
			equalize(a, dir);
			equalize(b, dir);
		}
	}
}

// After splitting to create leaf `id` along `dir`, even-distribute the whole
// same-direction run it joined - unless a divider in that run was hand-dragged
// (then the run keeps its sizes and the new 50/50 split stands).
fn equalize_dir_run(root: &mut Node, id: PaneId, dir: Dir) {
	let Some(path) = path_to(root, id) else {
		return;
	};
	if path.is_empty() {
		return; // the tree is a lone leaf
	}
	// walk up from the new pane's parent while ancestors stay same-direction; that
	// topmost same-direction split is the run's root
	let mut k = path.len() - 1;
	while k > 0 && is_dir_split(root, &path[..k - 1], dir) {
		k -= 1;
	}
	let top = node_at_mut(root, &path[..k]);
	if !group_has_manual(top, dir) {
		equalize(top, dir);
	}
}

fn prune(node: Node, id: PaneId) -> Option<Node> {
	match node {
		Node::Leaf(i) if i == id => None,
		Node::Leaf(i) => Some(Node::Leaf(i)),
		Node::Split {
			dir,
			ratio,
			manual,
			a,
			b,
		} => {
			let pruned_a = prune(*a, id);
			let pruned_b = prune(*b, id);
			match (pruned_a, pruned_b) {
				(Some(a), Some(b)) => Some(Node::Split {
					dir,
					ratio,
					manual,
					a: Box::new(a),
					b: Box::new(b),
				}),
				(Some(survivor), None) | (None, Some(survivor)) => Some(survivor),
				(None, None) => None,
			}
		}
	}
}

fn first_leaf(node: &Node) -> PaneId {
	match node {
		Node::Leaf(id) => *id,
		Node::Split { a, .. } => first_leaf(a),
	}
}

fn layout(node: &Node, area: Rect, scale: f32, out: &mut Vec<(PaneId, Rect)>) {
	match node {
		Node::Leaf(id) => out.push((*id, area)),
		Node::Split {
			dir, ratio, a, b, ..
		} => {
			let (a_area, b_area) = child_areas(area, *dir, *ratio, scale);
			layout(a, a_area, scale, out);
			layout(b, b_area, scale, out);
		}
	}
}

// The two child rects of a split, with the gap strip between them. The gap is
// DIP, so the divider keeps its weight as the display's scale factor rises.
fn child_areas(area: Rect, dir: Dir, ratio: f32, scale: f32) -> (Rect, Rect) {
	let gap = config::dip(config::PANE_GAP_PX, scale);
	match dir {
		Dir::Vertical => {
			let a_width = ((area.w - gap) * ratio).floor();
			(
				Rect {
					x: area.x,
					y: area.y,
					w: a_width,
					h: area.h,
				},
				Rect {
					x: area.x + a_width + gap,
					y: area.y,
					w: area.w - gap - a_width,
					h: area.h,
				},
			)
		}
		Dir::Horizontal => {
			let a_height = ((area.h - gap) * ratio).floor();
			(
				Rect {
					x: area.x,
					y: area.y,
					w: area.w,
					h: a_height,
				},
				Rect {
					x: area.x,
					y: area.y + a_height + gap,
					w: area.w,
					h: area.h - gap - a_height,
				},
			)
		}
	}
}

// Find the split whose divider is under (x, y), within a grab tolerance.
// Returns a path of child choices (false = a, true = b) from the root to that
// split, plus its orientation (for the resize cursor).
fn divider_at(
	node: &Node,
	area: Rect,
	x: f32,
	y: f32,
	scale: f32,
	path: &mut Vec<bool>,
) -> Option<Dir> {
	let Node::Split {
		dir, ratio, a, b, ..
	} = node
	else {
		return None;
	};
	let (a_area, b_area) = child_areas(area, *dir, *ratio, scale);
	let tol = config::dip(config::DIVIDER_GRAB_PX, scale);
	let on_divider =
		match dir {
			Dir::Vertical => {
				x >= a_area.x + a_area.w - tol
					&& x <= b_area.x + tol
					&& y >= area.y && y <= area.y + area.h
			}
			Dir::Horizontal => {
				y >= a_area.y + a_area.h - tol
					&& y <= b_area.y + tol
					&& x >= area.x && x <= area.x + area.w
			}
		};
	if on_divider {
		return Some(*dir);
	}
	if a_area.contains(x, y) {
		path.push(false);
		if let Some(found_dir) = divider_at(a, a_area, x, y, scale, path) {
			return Some(found_dir);
		}
		path.pop();
	}
	if b_area.contains(x, y) {
		path.push(true);
		if let Some(found_dir) = divider_at(b, b_area, x, y, scale, path) {
			return Some(found_dir);
		}
		path.pop();
	}
	None
}

// Walk `path` to a split node and set its ratio from the mouse position.
fn set_ratio(node: &mut Node, area: Rect, path: &[bool], x: f32, y: f32, scale: f32) {
	let Node::Split {
		dir,
		ratio,
		manual,
		a,
		b,
	} = node
	else {
		return;
	};
	if let [first, rest @ ..] = path {
		let (a_area, b_area) = child_areas(area, *dir, *ratio, scale);
		if *first {
			set_ratio(b, b_area, rest, x, y, scale);
		} else {
			set_ratio(a, a_area, rest, x, y, scale);
		}
		return;
	}
	let gap = config::dip(config::PANE_GAP_PX, scale);
	let new_ratio = match dir {
		Dir::Vertical => (x - area.x) / (area.w - gap),
		Dir::Horizontal => (y - area.y) / (area.h - gap),
	};
	*ratio = new_ratio.clamp(0.05, 0.95);
	*manual = true; // dragged: stop auto even-distribution for this run
}

// Lines the output ease has to cover this frame: the scrollback's growth, or
// with the scrollback full, the engine's own count of whole-screen scrolls.
fn output_advance(alt: bool, follow: bool, grew: usize, pushed: usize) -> usize {
	if follow && !alt {
		grew.max(pushed)
	} else {
		grew
	}
}

// The output ease's share of a build: whether the view follows the bottom, the
// lines this frame advanced, and whether the ease was already chasing before
// any nudge. A cut frame never nudges: the depth an alt-screen exit gives back,
// or the backlog a frozen pane took while hidden, is not output to ease through.
fn ease_output(
	scroll: &mut Scroll,
	alt: bool,
	cut: bool,
	grew: usize,
	pushed: usize,
	lines: usize,
) -> (bool, usize, bool) {
	let follow = scroll.following();
	let advanced = output_advance(alt, follow, grew, pushed);
	// read before the nudge: a band is measured fresh when the ease was at rest
	let was_chasing = scroll.chasing_output();
	if advanced > 0 && follow && !cut {
		scroll.nudge_output(advanced as f32, lines as f32);
	}
	(follow, advanced, was_chasing)
}

// What the engine's scroll record means for this frame's slide: the signed
// step, or None when the output ease owns the motion (`advanced` > 0) or
// nothing on screen can show it (a region scroll while scrolled back).
fn ledger_step(
	alt: bool,
	follow: bool,
	advanced: usize,
	whole_screen: bool,
	scrolled: i32,
) -> Option<i32> {
	if scrolled == 0 || advanced > 0 {
		return None;
	}
	(alt || (follow && !whole_screen)).then_some(scrolled)
}

// A recorded scroll is only worth easing when something visible moved: a row
// with ink on screen before it (`last`, the previous frame's styled rows), or
// one that left (`chunk`). ble.sh makes room for its prompt with insert- and
// delete-line on the blank rows under the cursor; the engine records that as a
// region scroll, and sliding it drags the prompt drawn right after down from
// behind the rows above. A turnover past the region's height is a scroll
// whatever was there (a burst into a fresh tmux pane), and a snapshot of
// another shape cannot say.
fn slide_is_visible(
	step: i32,
	region: &std::ops::Range<usize>,
	last: &[Vec<StripCell>],
	chunk: &[Vec<StripCell>],
) -> bool {
	let height = region.end.saturating_sub(region.start);
	let moved = step.unsigned_abs() as usize;
	if moved > height || last.len() < region.end {
		return true;
	}
	let sources = if step > 0 {
		region.start + moved..region.end
	} else {
		region.start..region.end - moved
	};
	chunk.iter().any(|r| has_ink(r)) || last[sources].iter().any(|r| has_ink(r))
}

fn has_ink(row: &[StripCell]) -> bool {
	row.iter().any(|c| c.c != ' ' || c.bg.is_some())
}

// A recorded scroll down whose rows pushed off the bottom were blank: room, per
// SLIDE_DOWN_INTO_ROOM. Blank both in the last frame (`last`) and as the engine
// kept them (`chunk`). less blanks its prompt row and scrolls back in one write,
// so the kept row alone reads as room. The region has to reach the bottom of the
// screen: vim and nano scroll back inside a region above their own status rows,
// and a blank file line leaving it is not room.
fn ledger_makes_room(
	step: i32,
	region: &std::ops::Range<usize>,
	lines: usize,
	last: &[Vec<StripCell>],
	chunk: &[Vec<StripCell>],
) -> bool {
	let k = (step.unsigned_abs() as usize).min(region.len());
	step < 0
		&& region.end == lines
		&& last.len() == lines
		&& !last[region.end - k..].iter().any(|r| has_ink(r))
		&& !chunk.is_empty()
		&& !chunk.iter().any(|r| has_ink(r))
}

// The same rule for a shift read off the row fingerprints, which is how an app
// that repaints its frame shows up (muffer's input box on the normal screen).
// The rows the shift pushed out of its span, and every row under them, were
// blank in the last frame.
fn shift_makes_room(cur: &[u64], last: &[u64], shift: i32, blank: u64) -> bool {
	let k = shift.unsigned_abs() as usize;
	if shift >= 0 {
		return false;
	}
	match translate_span(cur, last, shift) {
		Some((_, hi)) if hi + 1 >= k => last[hi + 1 - k..].iter().all(|&r| r == blank),
		_ => false,
	}
}

// Rows at the strip-side edge of a recorded region scroll that the scroll does
// not account for: an overlay the app repaints over the region's edge after
// moving it, like muffer's "1 new message" pill (the ledger path's side of
// `slide_bands`). Counted from the edge, and only when the row past them did
// move as recorded, so a frame repainted wholesale holds nothing.
fn repainted_edge(cur: &[u64], last: &[u64], region: &std::ops::Range<usize>, step: i32) -> usize {
	let n = cur.len();
	let k = step.unsigned_abs() as usize;
	let height = region.end.saturating_sub(region.start);
	if last.len() != n || region.end > n || k == 0 || k >= height {
		return 0;
	}
	let cap = height / 4;
	for over in 0..=cap {
		let moved = if step > 0 {
			let i = region.start + over;
			i + k < region.end && cur[i] == last[i + k]
		} else {
			let i = region.end - 1 - over;
			i >= region.start + k && cur[i] == last[i - k]
		};
		if moved {
			return over;
		}
	}
	0
}

// Whether this frame may read the row diff as a scroll (`scroll_shift_signed`).
// Only where the engine recorded none - a recorded scroll is exact and handled
// first - and then the alt screen always, the normal screen only on a repaint
// frame: following, no scrollback growth (the ConPTY case, where a TUI's
// region-scroll above a fixed input line arrives as an in-place repaint).
fn fingerprint_frame(alt: bool, follow: bool, grew: usize, scrolled: i32) -> bool {
	scrolled == 0 && (alt || (follow && grew == 0))
}

// Detect a clean vertical translate between two frames for the app-scroll
// slide, in either direction, up to `max` lines.
// +k = scrolled forward (content moved up k rows), -k = scrolled back (down k).
// Real full-screen apps keep static chrome bands - a status/input line at the
// BOTTOM (less, vim) and often a title bar at the TOP (nano, muffer) - so the
// scrolling region is a middle block, not a top prefix. We therefore count, for
// each candidate k, how many rows translate cleanly ANYWHERE (cur[i]==last[i+k]),
// and pick the k with the most. A shift counts only if a solid block translates
// (>= `need`) AND enough of those rows actually MOVED (>= MOVED_MIN) - where a row
// only counts as moved when the content appeared at its new position AND left its
// old one. Both halves matter: a static or blank field matches positionally but
// hasn't scrolled (easing that produces the apt/blank-jitter bounce), and a row
// whose source still holds the old content is a COPY, not a move - a repeated
// command's output on a half-empty screen re-prints the previous listing lower
// down, which reads as a perfect downward translate (the blank field supplies the
// positional matches, the repeat supplies the changed targets) and slid brand-new
// output down out from under the prompt. A shift must also EXPLAIN the frame: at
// least two thirds of the changed rows beyond the k it reveals must be rows the
// translation moved. An option-list TUI that swaps in a taller/shorter block for
// the highlighted entry only pushes the short footer under it - the rewritten
// block and marker rows above sit unexplained, so that relayout (which passes
// both moved tests: the footer really moves and really vacates) is rejected,
// while a real scroll accounts for nearly everything that changed (a couple of
// live status/spinner rows fit inside the third). Otherwise 0 (in-place redraw,
// content change, or a jump bigger than `max`) and the caller hard-cuts. 64-bit
// row fingerprints make a coincidental non-translation match vanishingly
// unlikely, so no contiguity check is needed. It never guesses a full turnover -
// easing a non-scroll looks wrong.
const MOVED_MIN: usize = 3; // a real scroll must move at least this many rows
fn scroll_shift_signed(cur: &[u64], last: &[u64], max: usize) -> i32 {
	let n = cur.len();
	if n == 0 || last.len() != n {
		return 0;
	}
	// a quarter of the screen, since static top+bottom bands shrink the middle
	let need = (n / 4).max(3);
	let changed = cur.iter().zip(last).filter(|(a, b)| a != b).count();
	let explains = |moved: usize, k: usize| moved * 3 >= changed.saturating_sub(k) * 2;
	let limit = max.min(n - 1);
	let (mut best, mut best_score) = (0i32, 0usize);
	for k in 1..=limit {
		// forward: content moved up k rows -> cur[i] == last[i+k]. Moved = the row
		// changed at its new position (i) and vacated its old one (i+k).
		let (mut matched, mut moved) = (0usize, 0usize);
		for i in 0..n - k {
			if cur[i] == last[i + k] {
				matched += 1;
				if cur[i] != last[i] && cur[i + k] != last[i + k] {
					moved += 1;
				}
			}
		}
		if matched >= need && moved >= MOVED_MIN && explains(moved, k) && matched > best_score {
			best_score = matched;
			best = k as i32;
		}
		// backward: content moved down k rows -> cur[i+k] == last[i]. Moved = changed
		// at the new position (i+k) and vacated the old one (i).
		let (mut matched, mut moved) = (0usize, 0usize);
		for i in 0..n - k {
			if cur[i + k] == last[i] {
				matched += 1;
				if cur[i + k] != last[i + k] && cur[i] != last[i] {
					moved += 1;
				}
			}
		}
		if matched >= need && moved >= MOVED_MIN && explains(moved, k) && matched > best_score {
			best_score = matched;
			best = -(k as i32);
		}
	}
	best
}

// Count the static (unchanged) rows at the top and bottom edges between two
// frames: a fixed title bar (nano/muffer) above and a status/help band below the
// scrolling region. Returns (top, bottom); zeroed if the two would meet or cover
// the whole screen (no distinct scroll region). Measured only on a gesture's first
// step - see build() - so mid-scroll fluctuation can't jitter the band boundary.
fn static_bands(cur: &[u64], last: &[u64]) -> (usize, usize) {
	let n = cur.len();
	if last.len() != n {
		return (0, 0);
	}
	let mut st = 0;
	while st < n && cur[st] == last[st] {
		st += 1;
	}
	let mut sb = 0;
	while sb < n && cur[n - 1 - sb] == last[n - 1 - sb] {
		sb += 1;
	}
	if st + sb >= n { (0, 0) } else { (st, sb) }
}

// The span a detected shift actually covers, as inclusive screen rows, or None if
// the shift explains nothing. A row a real scroll owns either translates cleanly
// or is one of the k rows the step reveals at the far edge; the span is the moved
// rows (widened outward through rows that merely match, since a blank line inside
// the region translates without ever counting as moved) plus that reveal.
fn translate_span(cur: &[u64], last: &[u64], shift: i32) -> Option<(usize, usize)> {
	let n = cur.len();
	let k = shift.unsigned_abs() as usize;
	if n == 0 || last.len() != n || k == 0 || k >= n {
		return None;
	}
	// Destination row of the pair anchored at i, and the two tests, in destination
	// space: forward (shift > 0) moves content up, so dest == i; backward moves it
	// down, so dest == i + k.
	let matches = |dest: usize| {
		if shift > 0 {
			dest < n - k && cur[dest] == last[dest + k]
		} else {
			dest >= k && cur[dest] == last[dest - k]
		}
	};
	let (mut lo, mut hi) = (usize::MAX, 0usize);
	for i in 0..n - k {
		let (dest, src) = if shift > 0 { (i, i + k) } else { (i + k, i) };
		// moved = arrived at the new row AND vacated the old one, as the detectors
		// count it: a copy left in place never established a scroll region
		if matches(dest) && cur[dest] != last[dest] && cur[src] != last[src] {
			lo = lo.min(dest);
			hi = hi.max(dest);
		}
	}
	if lo == usize::MAX {
		return None;
	}
	while lo > 0 && matches(lo - 1) {
		lo -= 1;
	}
	while hi + 1 < n && matches(hi + 1) {
		hi += 1;
	}
	if shift > 0 {
		Some((lo, (hi + k).min(n - 1)))
	} else {
		Some((lo.saturating_sub(k), hi))
	}
}

// The rows a slide must NOT translate. `static_bands` finds them by "didn't
// change", which misses fixed-position chrome that CHANGES while it sits still:
// muffer paints a "N new messages"/"Jump to bottom" pill at the bottom edge of its
// transcript, composited OVER the last region row, so that row differs every step,
// the unchanged-suffix walk stops below it, and the pill rode the ease - the same
// ghost the title bar used to produce. So also pin whatever the shift's own extent
// leaves out. Combined by MAX, which is what makes this safe: a band can only grow,
// so no row that slid before starts sliding differently, and since the span always
// contains every moved row a band can never swallow one that genuinely scrolled.
// The span also takes in rows past its far edge that moved the same way, only
// further (`moved_further`).
fn slide_bands(cur: &[u64], last: &[u64], shift: i32) -> (usize, usize) {
	let n = cur.len();
	let (mut st, mut sb) = static_bands(cur, last);
	if let Some((top, bot)) = translate_span(cur, last, shift) {
		let (top, bot) = if shift > 0 {
			(top, bot + moved_further(cur, last, shift, bot))
		} else {
			(top - moved_further(cur, last, shift, top), bot)
		};
		st = st.max(top);
		sb = sb.max(n - 1 - bot);
	}
	if st + sb >= n { (0, 0) } else { (st, sb) }
}

// Rows just past the span's far edge (`edge` is its last row that way) that moved
// in the shift's direction by more rows than it: muffer's input box collapses on
// Enter, so the entered lines climb further than the transcript above them. Left
// to the band, the message's lower lines snapped into place while the upper ones
// eased up under them. Sliding at the smaller shift they jump part of the way
// and ease the rest, and a row that moved further can never run backward.
fn moved_further(cur: &[u64], last: &[u64], shift: i32, edge: usize) -> usize {
	let n = cur.len();
	let k = shift.unsigned_abs() as usize;
	let mut best = 0;
	for s in k + 1..=APP_SCROLL_MAX.min(n.saturating_sub(1)) {
		let run = (0..)
			.map_while(|d| {
				let (i, src) = if shift > 0 {
					let i = edge + 1 + d;
					(i, i + s)
				} else {
					let i = edge.checked_sub(1 + d)?;
					(i, i.checked_sub(s)?)
				};
				(src < n && cur[i] == last[src] && cur[i] != last[i]).then_some(())
			})
			.count();
		best = best.max(run);
	}
	best
}

// Rows at the bottom a program redrew in place while `k` lines scrolled into
// history above them: a progress line, apt's status bar, a live block under a
// transcript. The output ease would drop them a row and slide them back. Rows
// under the step's span neither moved nor are new, but that alone would also hold
// the last chunk of plain output still, so one of them must be ink that stayed
// put. `region_hint` is the rows below a recorded scroll region, exact on its own.
fn output_band(cur: &[u64], last: &[u64], k: usize, region_hint: usize, blank: u64) -> usize {
	let n = cur.len();
	if k == 0 || k >= n || last.len() != n {
		return 0;
	}
	let content = match translate_span(cur, last, k as i32) {
		Some((_, bot)) => {
			let band = n - 1 - bot;
			let anchored = (n - band..n).any(|i| cur[i] != blank && cur[i] == last[i]);
			if anchored { band } else { 0 }
		}
		None => 0,
	};
	let band = region_hint.max(content);
	if band >= n / 2 { 0 } else { band }
}

// Unlike an app slide's bands (G8), the output band is re-measured every step but
// only grows while the ease runs. Continuous output keeps one ease going, so a
// band frozen at its first step stays 0 for the whole run when that step caught a
// half-drawn frame, and shrinking would drop a held row back into the eased text.
fn adopt_band(current: usize, measured: usize, easing: bool) -> usize {
	if easing {
		current.max(measured)
	} else {
		measured
	}
}

// Grid line drawn at `screen_row`. The output band shows the live bottom of the
// grid, which the scrolled view does not reach while the ease runs.
fn band_row_line(screen_row: i32, display_offset: i32, split_row: i32, ob: usize) -> i32 {
	if ob > 0 && screen_row >= split_row {
		screen_row
	} else {
		screen_row - display_offset
	}
}

#[cfg(test)]
mod tests {
	use super::{
		APP_SCROLL_MAX, AttrsList, BAR_MIN_THUMB, BRACKET_REACH_ROWS, CURSOR_ALPHA, CURSOR_MAX_LAG,
		DepthSampler, Dir, GColor, LinkHit, Node, OffStrip, PROMPT_SKEL_MIN, PauseState, Rect,
		SLIDE_TOP_BAND_APPS, StripCell, Style, Weight, adopt_band, alloc_pane_id, band_row_line,
		bar_applies_to, bar_pos_to_lines, bar_thumb_span, bar_track, bell_brighten, bracket_reach,
		capture_grid_text, capture_start, child_areas, cursor_cycle, cursor_envelope,
		cursor_geometry, cursor_slide_step, cursor_wake_after, debold_attrs, distinct_pair,
		divider_at, drop_hold_over, ease_output, edge_scroll_rate, equalize_dir_run,
		fingerprint_frame, fnv_row, fnv_row_skel, glide_to_full, grid_point, handle_is_dragged,
		handle_pos, has_ink, layout, ledger_makes_room, ledger_step, link_at, lock_for_frame,
		logical_line_bounds, minimap, mono_attrs, move_is_input, next_capture_poll, output_advance,
		output_band, pair_inside, paste_payload, place_split, prompt_strip, pulse_env,
		pushed_since, render_char, repainted_edge, resume_delay, same_char_pair,
		scroll_shift_signed, set_buffer_rows, set_ratio, shift_makes_room, shown_cursor_shape,
		slide_bands, slide_is_visible, snapshot_rows, static_bands, strip_cell, strip_rows,
		swap_leaves, text_buffer_h, translate_span, vanished_range, weld_region_clip,
	};
	use crate::config;
	use alacritty_terminal::event::{Event, EventListener};
	use alacritty_terminal::grid::Dimensions;
	use alacritty_terminal::index::{Column, Line, Point, Side};
	use alacritty_terminal::term::{Config as TermConfig, Term};
	use alacritty_terminal::vte::ansi::{CursorShape, Processor};

	struct VoidListener;
	impl EventListener for VoidListener {
		fn send_event(&self, _e: Event) {}
	}

	// A full-screen app has no scrollback of its own, so a bar there could only
	// report a fiction. Same answer when there is simply nothing to scroll.
	// Copy-on-output used to hold a core for the life of any command that printed
	// nothing: the settle deadline stayed in the past, so the loop woke on it
	// every pass and asked the shell again each time.
	// `glyphs` holds this frame's keys into `glyph_cache`, and a font-size change
	// while the reader holds the term lease empties the cache with the keys still
	// in hand. Indexing there aborts the process; the draw skips instead.
	// Test ID: EpHSLij
	#[test]
	fn a_glyph_whose_raster_went_away_is_skipped_not_indexed() {
		let body = include_str!("pane.rs")
			.split("\nmod tests {")
			.next()
			.expect("the file above its own tests");
		assert!(
			!body.contains("glyph_cache[&"),
			"indexing aborts on a key the clear took out"
		);
		assert!(body.contains("glyph_cache.get(&"), "the draw asks instead");
	}

	// Test ID: EpHQNBw
	#[test]
	fn a_command_that_prints_nothing_does_not_spin_the_loop() {
		let settle = std::time::Duration::from_millis(300);
		let now = std::time::Instant::now();
		let long_ago = now
			.checked_sub(std::time::Duration::from_secs(60))
			.expect("a minute ago");
		// nothing printed for a minute, and a poll just found the shell still busy
		let next = next_capture_poll(long_ago, settle, now + super::CAPTURE_RETRY);
		assert!(next > now, "the next poll is in the future");
		// and once nothing has pushed it, the settle window is what decides
		assert_eq!(next_capture_poll(now, settle, long_ago), now + settle);
	}

	// Test ID: EloGKCm
	#[test]
	fn no_scrollbar_without_scrollback_or_on_the_alt_screen() {
		let mut cfg = crate::config::Settings {
			scrollbar: true,
			..Default::default()
		};
		assert!(
			bar_applies_to(&cfg, false, 500.0),
			"normal screen with history"
		);
		assert!(
			!bar_applies_to(&cfg, true, 500.0),
			"alt screen owns its screen"
		);
		assert!(!bar_applies_to(&cfg, false, 0.0), "nothing to scroll");
		cfg.scrollbar = false;
		assert!(!bar_applies_to(&cfg, false, 500.0), "turned off");
	}

	// Dragging maps thumb position -> scroll position; the two must be inverses,
	// or the thumb creeps away from the pointer over a long drag.
	// Test ID: EloGKCn
	#[test]
	fn thumb_position_round_trips_through_a_drag() {
		let (track_h, thickness, rows, max) = (400.0, 16.0, 40.0, 1000.0);
		for lines in [0.0, 1.0, 250.0, 999.0, 1000.0] {
			let (y, thumb_h) = bar_thumb_span(track_h, thickness, rows, max, lines);
			let back = bar_pos_to_lines(track_h, thumb_h, max, y);
			assert!(
				(back - lines).abs() < 0.01,
				"{lines} lines -> y {y} -> {back} lines"
			);
		}
		// bottom of the track is the bottom of the scrollback, top is the oldest line
		let (bottom, _) = bar_thumb_span(track_h, thickness, rows, max, 0.0);
		let (top, _) = bar_thumb_span(track_h, thickness, rows, max, max);
		assert!(top < bottom, "the oldest line sits at the top of the track");
		assert_eq!(top, 0.0);
	}

	// Dragging either handle has to pin the other to it. The thumb and the marker
	// say the same thing, so one riding the pointer while the other chased the
	// ease read as the second one lagging.
	// Test ID: EqReddY
	#[test]
	fn dragging_either_handle_pins_both_to_the_pointer() {
		assert!(handle_is_dragged(true, false), "bar drag pins the marker");
		assert!(handle_is_dragged(false, true), "marker drag pins the thumb");
		assert!(handle_is_dragged(true, true));
		assert!(!handle_is_dragged(false, false), "off a drag both ease");
	}

	// Letting a handle go must not hand it back to the text before the text has
	// arrived, or it springs back the way it came and then eases in again.
	// Test ID: EqSDfqz
	#[test]
	fn a_dropped_handle_waits_where_it_was_let_go() {
		// dropped at line 600, text still back at 480
		assert_eq!(handle_pos(false, true, 600.0, 480.0), 600.0);
		// still dragging: the same answer, by the other arm
		assert_eq!(handle_pos(true, false, 600.0, 480.0), 600.0);
		// no drag and no drop: the handle belongs to the text
		assert_eq!(handle_pos(false, false, 600.0, 480.0), 480.0);
	}

	// Test ID: EqSDfr0
	#[test]
	fn a_drop_hold_ends_when_the_text_arrives_or_the_target_moves() {
		assert!(!drop_hold_over(true, 600.0, 600.0), "text still travelling");
		assert!(drop_hold_over(false, 600.0, 600.0), "text has arrived");
		// a wheel, a jump or output moved the target: the drop is stale
		assert!(drop_hold_over(true, 600.0, 540.0), "target moved away");
		assert!(
			!drop_hold_over(true, 600.0, 600.0 + config::SETTLE_EPS / 2.0),
			"a settled-size wobble is not a new gesture"
		);
	}

	// A huge scrollback would grind the thumb down to an ungrabbable sliver.
	// Test ID: EloGKCo
	#[test]
	fn thumb_never_shrinks_below_the_grab_minimum() {
		let thickness = 16.0;
		let (_, thumb_h) = bar_thumb_span(400.0, thickness, 40.0, 500_000.0, 0.0);
		assert!(thumb_h >= thickness * BAR_MIN_THUMB, "thumb_h={thumb_h}");
		// ... but it can never outgrow the track it rides in
		let (y, tall) = bar_thumb_span(20.0, thickness, 40.0, 1.0, 0.0);
		assert!(tall <= 20.0, "thumb_h={tall}");
		assert!(y >= 0.0);
	}

	// Test ID: EjwWLhA
	#[test]
	fn cursor_slide_catches_up_faster_when_farther() {
		let dt = 1.0 / 60.0;
		// one step covers more ground, proportionally, from a big lag than a small one
		let near = cursor_slide_step(0.0, 1.0, dt) / 1.0; // fraction of a 1-cell move
		let far = (cursor_slide_step(0.0, 20.0, dt) - 0.0) / 20.0; // fraction of a 20-cell move
		assert!(
			far > near,
			"a farther-behind cursor should close a larger fraction per step: near={near} far={far}"
		);
		// monotone approach, never overshoots
		let next = cursor_slide_step(2.0, 5.0, dt);
		assert!(
			next > 2.0 && next < 5.0,
			"eases toward target without overshoot: {next}"
		);
	}

	// Test ID: EjwWLhB
	#[test]
	fn cursor_slide_never_trails_past_the_cap() {
		// A tiny dt (high refresh) closes little of a big gap per step, so the clamp
		// engages: whatever the frame rate, the cursor sits at most CURSOR_MAX_LAG behind.
		let tiny = 0.001;
		let next = cursor_slide_step(0.0, 40.0, tiny);
		assert!(
			(40.0 - next) <= CURSOR_MAX_LAG + 0.001,
			"lag capped at {CURSOR_MAX_LAG}, got {}",
			40.0 - next
		);
		// symmetric for a leftward jump
		let back = cursor_slide_step(40.0, 0.0, tiny);
		assert!(
			back <= CURSOR_MAX_LAG + 0.001,
			"leftward lag capped too: {back}"
		);
	}

	// A TUI hides the cursor to repaint (CSI ?25l) and parks it wherever the paint
	// ended - commonly the far bottom-right cell. Ignore DECTCEM and that parked
	// position gets a cursor drawn on it, alternating with the real one at repaint
	// rate, which is faster than any blink. Visibility is a MODE; the shape carries
	// nothing about it, so asking the shape alone can never answer this.
	// Test ID: EnPgdFY
	#[test]
	fn a_hidden_cursor_is_not_drawn_where_the_app_parked_it() {
		let shape = |t: &Term<VoidListener>| shown_cursor_shape(*t.mode(), t.cursor_style().shape);
		let mut term = term_fed(20, 6, 0, "");
		assert_ne!(
			shape(&term),
			CursorShape::Hidden,
			"shown until an app says otherwise"
		);

		// hide, then park at the bottom-right the way a repaint leaves it
		feed(&mut term, "[?25l[6;20H");
		assert_eq!(
			shape(&term),
			CursorShape::Hidden,
			"hidden while the app repaints"
		);
		feed(&mut term, "[?25h");
		assert_ne!(
			shape(&term),
			CursorShape::Hidden,
			"back when the app shows it again"
		);

		// a shape the app set (DECSCUSR) survives while shown and is still suppressed
		// while hidden - the two are independent, which is the whole point
		feed(&mut term, "[5 q");
		assert_eq!(shape(&term), CursorShape::Beam, "DECSCUSR beam");
		feed(&mut term, "[?25l");
		assert_eq!(
			shape(&term),
			CursorShape::Hidden,
			"hidden outranks the shape"
		);
	}

	// Combining marks all go on the one cell before them, and nothing that trims
	// the scrollback ever trims a cell. The 0.26.0 engine kept every one, so a
	// program printing them in a loop grew the terminal until it was killed.
	// Test ID: Epxtj8q
	#[test]
	fn combining_marks_on_one_cell_are_capped() {
		let flood = format!("a{}", "\u{301}".repeat(10_000));
		let term = term_fed(10, 2, 0, &flood);
		let kept = term.grid()[Line(0)][Column(0)]
			.zerowidth()
			.map_or(0, <[char]>::len);
		assert!(kept <= 16, "one cell kept {kept} combining marks");

		let accented = term_fed(10, 2, 0, "e\u{301}");
		assert_eq!(
			accented.grid()[Line(0)][Column(0)].zerowidth(),
			Some(&['\u{301}'][..]),
			"an ordinary accent is kept"
		);
	}

	// A double-click on an unmatched bracket had the engine look through the
	// whole scrollback for its partner, under the lock and on every rebuild.
	// Test ID: EqBSso4
	#[test]
	fn a_bracket_looks_for_its_partner_only_so_far() {
		let text = "x (a\r\nb) c )\r\n";
		let term = term_fed(20, 3, 10, text);
		let grid = term.grid();
		let at = |line: i32, col: usize| Point::new(Line(line), Column(col));
		let top = -(grid.history_size() as i32);
		// across a line break, from either end
		assert_eq!(
			bracket_reach(grid, at(top, 2)),
			Some((at(top, 2), at(top + 1, 1)))
		);
		assert_eq!(
			bracket_reach(grid, at(top + 1, 1)),
			Some((at(top, 2), at(top + 1, 1)))
		);
		assert_eq!(bracket_reach(grid, at(top, 0)), None, "not a bracket");

		// a partner past the reach is not looked for, so the bracket stands alone
		let far = BRACKET_REACH_ROWS + 50;
		let mut text = String::from("(\r\n");
		text.push_str(&"-\r\n".repeat(far));
		text.push(')');
		let term = term_fed(4, 3, far + 10, &text);
		let grid = term.grid();
		let close = Point::new(Line(2), Column(0));
		assert_eq!(grid[close].c, ')');
		assert_eq!(bracket_reach(grid, close), Some((close, close)));
		assert!(
			term.bracket_search(close).is_some(),
			"the engine would have found it"
		);
	}

	// A small live Term fed via the real parser, for the copy-output tests.
	fn term_fed(cols: usize, lines: usize, scrollback: usize, input: &str) -> Term<VoidListener> {
		let cfg = TermConfig {
			scrolling_history: scrollback,
			..Default::default()
		};
		let dims = crate::term::TermDimensions {
			columns: cols,
			screen_lines: lines,
		};
		let mut term = Term::new(cfg, &dims, VoidListener);
		let mut parser: Processor = Processor::new();
		parser.advance(&mut term, input.as_bytes());
		term
	}
	fn feed(term: &mut Term<VoidListener>, input: &str) {
		let mut parser: Processor = Processor::new();
		parser.advance(term, input.as_bytes());
	}
	fn row_hash(term: &Term<VoidListener>, line: i32) -> u64 {
		let grid = term.grid();
		let cols = grid.columns();
		fnv_row((0..cols).map(|c| grid[Line(line)][Column(c)].c))
	}
	fn row_skel(term: &Term<VoidListener>, line: i32) -> u64 {
		let grid = term.grid();
		let cols = grid.columns();
		fnv_row_skel((0..cols).map(|c| grid[Line(line)][Column(c)].c)).0
	}

	fn leaf(id: u64) -> Node {
		Node::Leaf(id)
	}
	fn split(dir: Dir, ratio: f32, manual: bool, a: Node, b: Node) -> Node {
		Node::Split {
			dir,
			ratio,
			manual,
			a: Box::new(a),
			b: Box::new(b),
		}
	}
	fn widths(root: &Node, w: f32) -> Vec<(u64, f32)> {
		let mut out = Vec::new();
		layout(
			root,
			Rect {
				x: 0.0,
				y: 0.0,
				w,
				h: 100.0,
			},
			1.0,
			&mut out,
		);
		out.sort_by_key(|(id, _)| *id);
		out.into_iter().map(|(id, r)| (id, r.w)).collect()
	}

	// The strip of background between two panes is one DIP, so it keeps its
	// weight as the display's DPI rises instead of thinning to a hairline that
	// disappears - and the tolerance for grabbing it has to widen with it, or the
	// divider becomes progressively harder to catch on a high-DPI screen.
	// Test ID: EnLuU55
	#[test]
	fn the_pane_gap_and_its_grab_zone_scale_with_the_display() {
		let area = Rect {
			x: 0.0,
			y: 0.0,
			w: 400.0,
			h: 200.0,
		};
		let gap_at = |scale: f32| {
			let (a, b) = child_areas(area, Dir::Vertical, 0.5, scale);
			b.x - (a.x + a.w)
		};
		assert_eq!(gap_at(1.0), config::PANE_GAP_PX);
		assert_eq!(gap_at(2.0), config::PANE_GAP_PX * 2.0);
		// the two children still tile the whole area, gap included, at either scale
		for scale in [1.0, 2.0] {
			let (a, b) = child_areas(area, Dir::Vertical, 0.5, scale);
			assert_eq!(a.w + gap_at(scale) + b.w, area.w);
		}

		// the grab zone: a press this far off the seam still finds the divider
		let root = split(Dir::Vertical, 0.5, false, leaf(1), leaf(2));
		let seam = {
			let (a, _) = child_areas(area, Dir::Vertical, 0.5, 2.0);
			a.x + a.w
		};
		let reach = config::dip(config::DIVIDER_GRAB_PX, 2.0);
		let mut path = Vec::new();
		assert!(
			matches!(
				divider_at(&root, area, seam - reach + 0.5, 100.0, 2.0, &mut path),
				Some(Dir::Vertical)
			),
			"the grab zone must widen with the gap it catches"
		);
		// and one that falls well clear of it does not
		path.clear();
		assert!(
			divider_at(&root, area, seam - reach * 3.0, 100.0, 2.0, &mut path).is_none(),
			"a press clear of the seam is not a divider grab"
		);
	}

	// Test ID: EjKleO0
	#[test]
	fn logical_line_bounds_spans_wrapped_rows() {
		// rows 2 and 3 each wrap into the next, so 2..=4 is one logical line
		let w = |l: i32| l == 2 || l == 3;
		assert_eq!(logical_line_bounds(3, -10, 9, w), (2, 4));
		assert_eq!(logical_line_bounds(2, -10, 9, w), (2, 4));
		assert_eq!(logical_line_bounds(4, -10, 9, w), (2, 4));
		// an unwrapped row is its own line
		assert_eq!(logical_line_bounds(6, -10, 9, w), (6, 6));
		// clamps to [top, bot]
		assert_eq!(logical_line_bounds(100, 0, 9, |_| false), (9, 9));
		assert_eq!(logical_line_bounds(-100, 0, 9, |_| false), (0, 0));
		// never walks past top, and walks the full run downward
		assert_eq!(logical_line_bounds(0, 0, 9, |_| true), (0, 9));
	}

	// Test ID: EjN7Pqa
	#[test]
	fn glide_to_full_runs_at_normal_speed_and_flags_arrival() {
		let period = 1.0;
		// pulse: full_phase 0.5. Starting mid-shrink (0.7) it advances plain +dt
		// (no speed change, no snap), wraps, and flags arrival crossing 0.5.
		let (t, arrived) = glide_to_full(0.7, 0.01, period, 0.5);
		assert!(!arrived);
		assert!((t - 0.71).abs() < 1e-6);
		let mut t = 0.7;
		let mut steps = 0;
		loop {
			let (next, arrived) = glide_to_full(t, 0.01, period, 0.5);
			t = next;
			steps += 1;
			if arrived {
				break;
			}
			assert!(steps < 1000, "never reached full");
		}
		// 0.7 -> wrap -> 0.5 is 0.8 of a period at 0.01/step (float slack of one)
		assert!((79..=81).contains(&steps), "steps = {steps}");
		assert!(((t / period).fract() - 0.5).abs() < 1e-6);
		// phase mode: full_phase 0.0 - arrival falls on a whole-period multiple
		let (p, arrived) = glide_to_full(0.95, 0.1, period, 0.0);
		assert!(arrived);
		assert!((p / period).fract().abs() < 1e-6 || ((p / period).fract() - 1.0).abs() < 1e-6);
	}

	// Test ID: EjN7Pqb
	#[test]
	fn pause_state_glides_holds_then_resumes_from_full() {
		let period = 1.0;
		let timeout = 0.35;
		let mut st = PauseState::default();
		// input mid-shrink: the cycle keeps running forward at normal speed - the
		// very next frame is a plain +dt, not a jump to full
		let mut t = st.advance(
			0.7, 0.01, 0.01, period, 0.5, timeout, 0.0, true, 0.0, 0.0, false,
		);
		assert!((t - 0.71).abs() < 1e-6);
		assert!(st.active && !st.parked);
		// runs on around the cycle and parks exactly at the full-size phase, even
		// though the idle timeout expires long before it gets there
		let mut idle = 0.01;
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, timeout, 0.0, false, idle, idle, false,
			);
			idle += 0.01;
			if st.parked {
				break;
			}
		}
		assert!(st.parked);
		assert!(((t / period).fract() - 0.5).abs() < 1e-6);
		// typing while parked keeps it parked at full
		t = st.advance(
			t, 0.01, 0.01, period, 0.5, timeout, 0.0, true, 0.0, 0.0, false,
		);
		assert!(st.parked && ((t / period).fract() - 0.5).abs() < 1e-6);
		// holds through the timeout after the last input, then resumes from full:
		// the first resumed frame is full_phase + dt, so the size is continuous
		idle = 0.01;
		let mut resumed = None;
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, timeout, 0.0, false, idle, idle, false,
			);
			idle += 0.01;
			if !st.active {
				resumed = Some(t);
				break;
			}
		}
		let t = resumed.expect("should resume");
		assert!((t - (0.5 * period + 0.01)).abs() < 1e-6);
		// and once resumed it just accumulates
		let t2 = st.advance(
			t, 0.01, 0.01, period, 0.5, timeout, 0.0, false, 1.0, 1.0, false,
		);
		assert!((t2 - (t + 0.01)).abs() < 1e-6);
	}

	// Test ID: EjN7Pqc
	#[test]
	fn pause_state_hold_needs_both_idle_and_hold_timeouts() {
		let period = 1.0;
		let timeout = 0.35;
		let mut st = PauseState::default();
		// start already near full so it parks on the first step
		let mut t = st.advance(
			0.49, 0.02, 0.02, period, 0.5, timeout, 0.0, true, 0.0, 0.0, false,
		);
		assert!(st.parked);
		// idle long past the timeout, but the hold itself must also last it: a
		// glide that ate the idle window still gets a real pause at full
		t = st.advance(
			t, 0.01, 0.01, period, 0.5, timeout, 0.0, false, 10.0, 10.0, false,
		);
		assert!(st.active && ((t / period).fract() - 0.5).abs() < 1e-6);
		// conversely, held long enough but input still recent keeps it parked
		for _ in 0..100 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, timeout, 0.0, false, 0.1, 0.1, false,
			);
		}
		assert!(st.active && ((t / period).fract() - 0.5).abs() < 1e-6);
	}

	// Test ID: ElclCcq
	#[test]
	fn pause_state_long_idle_parks_at_full_and_resumes_on_activity() {
		let period = 1.0;
		let (resume, stop) = (0.35, 60.0);
		let mut st = PauseState::default();
		// running free, idle crosses the stop threshold with no input: an episode
		// starts anyway and the glide carries it to the full-size phase
		let mut t = st.advance(
			0.7, 0.01, 0.01, period, 0.5, resume, stop, false, 0.0, stop, false,
		);
		assert!(st.active && !st.parked);
		let mut idle = stop;
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, resume, stop, false, idle, idle, false,
			);
			idle += 0.01;
			if st.parked {
				break;
			}
		}
		assert!(st.parked && ((t / period).fract() - 0.5).abs() < 1e-6);
		// stays parked indefinitely - a big wall-clock gap (the sleeping loop
		// catching up) satisfies the hold but the input idle past the stop pins it
		t = st.advance(
			t,
			0.01,
			300.0,
			period,
			0.5,
			resume,
			stop,
			false,
			idle + 300.0,
			idle + 300.0,
			false,
		);
		assert!(st.active && ((t / period).fract() - 0.5).abs() < 1e-6);
		// activity (keystroke or refocus poke) resets idle_t: still parked for
		// the resume delay, then the cycle resumes from full - never mid-cycle
		t = st.advance(
			t, 0.01, 0.01, period, 0.5, resume, stop, false, 0.0, 0.0, false,
		);
		assert!(st.active && ((t / period).fract() - 0.5).abs() < 1e-6);
		let mut idle = 0.01;
		let mut resumed = None;
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, resume, stop, false, idle, idle, false,
			);
			idle += 0.01;
			if !st.active {
				resumed = Some(t);
				break;
			}
		}
		let t = resumed.expect("should resume after the delay");
		assert!((t - (0.5 * period + 0.01)).abs() < 1e-6);
	}

	// A prompt with a clock in it, or any TUI on its own timer, moves the cursor
	// every few seconds while the user is not there. That kept resetting the
	// cursor's idle clock, so the long stop never arrived and the animation ran
	// for as long as the program did.
	// Test ID: EqA5qAq
	#[test]
	fn pause_state_long_idle_stops_although_a_program_keeps_moving_the_cursor() {
		let period = 1.0;
		let (resume, stop) = (0.35, 5.0);
		let mut st = PauseState::default();
		let mut input_idle = 0.0f32;
		let mut cursor_idle = 0.0f32;
		let mut t = 0.7;
		// 20 seconds at 100 fps, with the program nudging the cursor every 2s -
		// well inside the 5s stop, so the cursor's own idle never gets there
		for step in 0..2000 {
			let moved = step % 200 == 0;
			cursor_idle = if moved { 0.0 } else { cursor_idle + 0.01 };
			t = st.advance(
				t,
				0.01,
				0.01,
				period,
				0.5,
				resume,
				stop,
				moved,
				cursor_idle,
				input_idle,
				false,
			);
			input_idle += 0.01;
		}
		assert!(cursor_idle < stop, "the cursor's own idle must stay short");
		assert!(
			st.active && st.parked && ((t / period).fract() - 0.5).abs() < 1e-6,
			"parked at full after the stop, whatever the program is doing"
		);
	}

	// Test ID: ElcxMmu
	#[test]
	fn pause_state_blocked_parks_at_full_until_unblocked() {
		let period = 1.0;
		let (resume, stop) = (0.35, 60.0);
		let mut st = PauseState::default();
		// pane loses active status mid-cycle: an episode starts with no input and
		// the glide carries it to the full-size phase (never a snap)
		let mut t = st.advance(
			0.7, 0.01, 0.01, period, 0.5, resume, stop, false, 0.0, 0.0, true,
		);
		assert!(st.active && !st.parked);
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, resume, stop, false, 0.0, 0.0, true,
			);
			if st.parked {
				break;
			}
		}
		assert!(st.parked && ((t / period).fract() - 0.5).abs() < 1e-6);
		// held indefinitely while blocked: output moving the cursor (moved) and
		// long holds satisfy nothing - only becoming active again can end it
		for _ in 0..300 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, resume, stop, true, 5.0, 5.0, true,
			);
		}
		assert!(st.active && st.parked && ((t / period).fract() - 0.5).abs() < 1e-6);
		// unblocked with only idle_t reset: holds out the resume delay, then the
		// cycle resumes from full - size continuous throughout. (In the app a
		// refocus also calls resume(), which skips this wait - see below.)
		let mut idle = 0.0;
		let mut resumed = None;
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, resume, stop, false, idle, idle, false,
			);
			idle += 0.01;
			if !st.active {
				resumed = Some(t);
				break;
			}
		}
		let t = resumed.expect("should resume once unblocked");
		assert!((t - (0.5 * period + 0.01)).abs() < 1e-6);
	}

	// A refocus ends the park at once - it must not sit out the resume delay the
	// way input does - and still resumes from the cursor's full size.
	// Test ID: EldrZxY
	#[test]
	fn pause_state_resume_skips_the_delay() {
		let period = 1.0;
		let (resume_s, stop) = (1.0, 60.0);
		let mut st = PauseState::default();
		let mut t = st.advance(
			0.7, 0.01, 0.01, period, 0.5, resume_s, stop, true, 0.0, 0.0, false,
		);
		for _ in 0..200 {
			t = st.advance(
				t, 0.01, 0.01, period, 0.5, resume_s, stop, false, 0.0, 0.0, false,
			);
			if st.parked {
				break;
			}
		}
		assert!(st.parked, "typing should park the cycle at full");
		// a fraction of the delay in, the timed path is still holding
		st.advance(
			t, 0.01, 0.01, period, 0.5, resume_s, stop, false, 0.1, 0.1, false,
		);
		assert!(st.active && st.parked);
		st.resume();
		assert!(!st.active && !st.parked);
		// poke_cursor puts blink_t at the full-size phase; the cycle runs on from there
		let t = st.advance(
			0.5 * period,
			0.01,
			0.01,
			period,
			0.5,
			resume_s,
			stop,
			false,
			0.0,
			0.0,
			false,
		);
		assert!((t - (0.5 * period + 0.01)).abs() < 1e-6);
	}

	// The pause and the refocus resume must agree on where "full size" is: mid
	// cycle for the pulses, phase 0 for the fade.
	// Test ID: EldrZxZ
	#[test]
	fn cursor_cycle_full_phase_matches_the_animation() {
		assert_eq!(cursor_cycle("pulse_vertical", 500.0), (1.0, 0.5));
		assert_eq!(cursor_cycle("phase", 500.0), (1.0, 0.0));
		assert_eq!(cursor_cycle("pulse_both", 250.0).0, 0.5);
		assert_eq!(cursor_cycle("phase", 0.0).0, 0.05); // period never reaches zero
	}

	// Test ID: EleFUn2
	#[test]
	fn only_a_keystrokes_echo_counts_as_input() {
		use std::time::{Duration, Instant};
		let now = Instant::now();
		let ago = |ms| now.checked_sub(Duration::from_millis(ms)).unwrap();
		assert!(!move_is_input(None, now), "nothing typed yet");
		assert!(move_is_input(Some(ago(30)), now));
		// a command's output starts within a frame or two of Enter but keeps
		// coming long after it - that later stretch is the program, not the user
		assert!(!move_is_input(Some(ago(800)), now));
	}

	// Test ID: EleFUn3
	#[test]
	fn output_gives_the_cursor_straight_back() {
		// typing holds the configured delay; output holds only long enough not to
		// unpark between two writes, so the prompt returning revives the cursor
		assert_eq!(resume_delay(true, 1.0), 1.0);
		assert!(resume_delay(false, 1.0) < 0.1);

		// drive the park itself: same state machine, output's delay
		let (period, full) = cursor_cycle("pulse_vertical", 500.0);
		let delay = resume_delay(false, 1.0);
		let mut st = PauseState::default();
		let mut blink = 0.2;
		let dt = 1.0 / 60.0;
		// a command writing: the cursor keeps moving, so the cycle glides to full
		// and stays parked there for as long as the output lasts
		for _ in 0..300 {
			blink = st.advance(
				blink, dt, dt, period, full, delay, 0.0, true, 0.0, 0.0, false,
			);
		}
		assert!(st.parked, "output parks the cursor at full size");
		assert!((blink - full * period).abs() < 1e-6);
		// output stops (the prompt is back): no more moves, and the cycle picks up
		let mut idle = 0.0;
		let mut frames = 0;
		while st.active && frames < 60 {
			idle += dt;
			blink = st.advance(
				blink, dt, dt, period, full, delay, 0.0, false, idle, idle, false,
			);
			frames += 1;
		}
		assert!(!st.active, "the animation resumed");
		assert!(
			idle < 0.2,
			"resumed at once, not after the typing delay: {idle}"
		);
		assert!(
			(blink - (full * period + dt)).abs() < 1e-6,
			"resumed from full size"
		);
	}

	// Test ID: EjNOyIU
	#[test]
	fn capture_finds_output_start_at_full_scrollback() {
		// 3 rows, scrollback cap 4 - the command's output fills the buffer to cap
		// and evicts old lines, the long-lived-shell case. The arm-time absolute
		// index goes stale with each eviction; the content anchor must not.
		let mut term = term_fed(20, 3, 4, "h1\r\nh2\r\nh3\r\nh4\r\nuser$ cmd");
		// arm at the prompt (before Enter reaches the terminal)
		let grid = term.grid();
		let cmd_start = grid.history_size() + grid.cursor.point.line.0.max(0) as usize + 1;
		let anchor = Some(row_hash(&term, grid.cursor.point.line.0));
		// the command echoes Enter, prints 4 lines, and a fresh prompt appears
		feed(&mut term, "\r\nO1\r\nO2\r\nO3\r\nO4\r\nuser$ ");
		let grid = term.grid();
		assert_eq!(grid.history_size(), 4, "buffer must have hit the cap");
		let end = grid.history_size() + grid.cursor.point.line.0.max(0) as usize;
		// the anchor recovers the true start; the stale index alone drops lines
		let start = capture_start(&term, cmd_start, anchor, end);
		assert_eq!(capture_grid_text(&term, start, end), "O1\nO2\nO3\nO4\n");
		assert_ne!(
			capture_grid_text(&term, cmd_start, end),
			"O1\nO2\nO3\nO4\n",
			"the stale index should demonstrate the bug this guards against"
		);
		// no anchor (blank prompt row) or no match (row evicted/redrawn): the
		// recorded index is the fallback, never a panic
		assert_eq!(capture_start(&term, cmd_start, None, end), cmd_start);
		assert_eq!(capture_start(&term, cmd_start, Some(1), end), cmd_start);
	}

	// Test ID: EjtKjM0
	#[test]
	fn capture_strips_multiline_prompt_rows() {
		// two-line prompt: the decoration row the prompt paints above its input
		// line must not be copied as output once the block is learned
		let mut term = term_fed(20, 6, 100, "==info==\r\nuser$ cmd");
		let grid = term.grid();
		let cmd_start = grid.history_size() + grid.cursor.point.line.0.max(0) as usize + 1;
		let block = vec![row_skel(&term, grid.cursor.point.line.0 - 1)];
		feed(&mut term, "\r\nA\r\nB\r\n==info==\r\nuser$ ");
		let grid = term.grid();
		let end = grid.history_size() + grid.cursor.point.line.0.max(0) as usize;
		let stripped = prompt_strip(&term, cmd_start, end, &block);
		assert_eq!(capture_grid_text(&term, cmd_start, stripped), "A\nB\n");
		// nothing learned yet, or a non-matching row: strip nothing (fail-safe)
		assert_eq!(prompt_strip(&term, cmd_start, end, &[]), end);
		assert_eq!(prompt_strip(&term, cmd_start, end, &[123]), end);
	}

	// Test ID: EkoOBPk
	#[test]
	fn capture_strips_dynamic_prompt_rows() {
		// the decoration row's content changes per command (cwd, clock) but its
		// structure doesn't; the skeleton match must still strip it - an exact
		// compare left every dynamic prompt's rows in the copy (the reported bug)
		let mut term = term_fed(30, 8, 100, "[~/proj git:main 10:01]\r\nuser$ cmd");
		let grid = term.grid();
		let cmd_start = grid.history_size() + grid.cursor.point.line.0.max(0) as usize + 1;
		let block = vec![row_skel(&term, grid.cursor.point.line.0 - 1)];
		feed(
			&mut term,
			"\r\nA\r\nB\r\n[~/other git:fixup 10:47]\r\nuser$ ",
		);
		let grid = term.grid();
		let end = grid.history_size() + grid.cursor.point.line.0.max(0) as usize;
		let stripped = prompt_strip(&term, cmd_start, end, &block);
		assert_eq!(capture_grid_text(&term, cmd_start, stripped), "A\nB\n");
		// an output row with different structure must NOT match the block
		assert_ne!(
			row_skel(&term, grid.cursor.point.line.0 - 2),
			block[0],
			"plain output must not skeleton-match the prompt decoration"
		);
	}

	// Test ID: EnL1S0e
	#[test]
	fn a_plain_row_is_too_thin_to_learn_as_a_prompt() {
		// the skeleton cannot tell one one-word row from another - by design, so a
		// prompt's cwd and clock can change - which is exactly why such a row must
		// never be learned as prompt: the strip would then eat a real output line.
		let word = fnv_row_skel("alpha    ".chars());
		let year = fnv_row_skel("2026     ".chars());
		assert_eq!(word.0, year.0, "unrelated one-word rows hash alike");
		assert!(word.1 < PROMPT_SKEL_MIN, "so neither may be learned");
		// a real decoration row still carries enough structure to be learnable
		assert!(fnv_row_skel("[~/proj git:main 10:01]".chars()).1 >= PROMPT_SKEL_MIN);
		assert!(fnv_row_skel("==info==            ".chars()).1 >= PROMPT_SKEL_MIN);
	}

	// Test ID: EnPFQSu
	#[test]
	fn a_pasted_line_break_arrives_the_way_enter_delivers_one() {
		// Unbracketed, the application cannot tell the paste from typing, so every
		// flavour of line break has to reduce to the lone CR the Enter key sends. A
		// Windows clipboard is CRLF, so this is the ordinary case there: sending the
		// LF too leaves the shell on a continuation line after every row.
		assert_eq!(
			paste_payload("one\r\ntwo\r\nthree", false),
			"one\rtwo\rthree"
		);
		assert_eq!(paste_payload("one\ntwo", false), "one\rtwo");
		assert_eq!(
			paste_payload("one\r\ntwo\nthree\r", false),
			"one\rtwo\rthree\r"
		);
		// nothing to do without a line break
		assert_eq!(paste_payload("plain text", false), "plain text");
		// and not one LF may survive to reach the shell
		assert!(!paste_payload("a\r\nb\nc", false).contains('\n'));
	}

	// Test ID: EnPFQSv
	#[test]
	fn a_bracketed_paste_cannot_be_closed_from_inside() {
		// The application is watching for ESC[201~, so an ESC carried in the payload
		// ends the bracket early and everything after it is read as keystrokes -
		// which is how a paste runs a command nobody typed. The ESC has to go.
		let attack = "safe\x1b[201~rm -rf ~\r";
		let out = paste_payload(attack, true);
		assert!(
			!out.contains('\x1b'),
			"no ESC may survive into a bracketed paste"
		);
		assert_eq!(out, "safe[201~rm -rf ~\r");
		// Bracketed, the application asked for the text itself, so line breaks pass
		// through untouched - only ESC is taken out.
		assert_eq!(paste_payload("one\r\ntwo", true), "one\r\ntwo");
	}

	// Test ID: EkoOBPl
	#[test]
	fn skeleton_hash_collapses_dynamic_runs() {
		// same punctuation structure, different-length words/digits/spacing = same
		let a = fnv_row_skel("[~/proj git:main 10:01]".chars());
		let b = fnv_row_skel("[~/another git:x 9:47]".chars());
		assert_eq!(a, b);
		// right-aligned segment shifting with left content length = same
		let c = fnv_row_skel("u@h ~/a          ok".chars());
		let d = fnv_row_skel("u@h ~/longer   ok".chars());
		assert_eq!(c, d);
		// different punctuation structure = different
		assert_ne!(fnv_row_skel("[a/b]".chars()), fnv_row_skel("(a/b)".chars()));
		assert_ne!(
			fnv_row_skel("hello world".chars()),
			fnv_row_skel("   ".chars())
		);
	}

	// Test ID: EjNOyIV
	#[test]
	fn capture_below_scrollback_cap_matches_either_way() {
		// plenty of scrollback: no eviction, so the stale-index and anchor paths
		// agree - the anchor must not regress the common case
		let mut term = term_fed(20, 3, 100, "user$ cmd");
		let grid = term.grid();
		let cmd_start = grid.history_size() + grid.cursor.point.line.0.max(0) as usize + 1;
		let anchor = Some(row_hash(&term, grid.cursor.point.line.0));
		feed(&mut term, "\r\nA\r\nB\r\nuser$ ");
		let grid = term.grid();
		let end = grid.history_size() + grid.cursor.point.line.0.max(0) as usize;
		let start = capture_start(&term, cmd_start, anchor, end);
		assert_eq!(start, cmd_start);
		assert_eq!(capture_grid_text(&term, start, end), "A\nB\n");
	}

	// Test ID: EjKhdge
	#[test]
	fn render_char_maps_controls_to_space() {
		// a tab (or any control) left in a cell must shape as a 1-cell space, else
		// the row shifts off the grid and double-click selection misaligns
		assert_eq!(render_char('\t'), ' ');
		assert_eq!(render_char('\0'), ' ');
		assert_eq!(render_char('\r'), ' ');
		assert_eq!(render_char('a'), 'a');
		assert_eq!(render_char(' '), ' ');
		assert_eq!(render_char('世'), '世');
	}

	// Test ID: EivUOFc
	#[test]
	fn equalize_three_in_a_row() {
		// split A vertically then split the new pane again: 50/25/25 -> equalize
		let mut root = split(
			Dir::Vertical,
			0.5,
			false,
			leaf(1),
			split(Dir::Vertical, 0.5, false, leaf(2), leaf(3)),
		);
		equalize_dir_run(&mut root, 3, Dir::Vertical);
		let ws = widths(&root, 900.0);
		for (_, w) in &ws {
			assert!((w - 300.0).abs() <= 2.0, "not equal thirds: {ws:?}");
		}
	}

	// Test ID: EivUOFd
	#[test]
	fn equalize_four_in_a_row() {
		let mut root = split(
			Dir::Vertical,
			0.5,
			false,
			leaf(1),
			split(
				Dir::Vertical,
				0.5,
				false,
				leaf(2),
				split(Dir::Vertical, 0.5, false, leaf(3), leaf(4)),
			),
		);
		equalize_dir_run(&mut root, 4, Dir::Vertical);
		let ws = widths(&root, 1200.0);
		for (_, w) in &ws {
			assert!((w - 300.0).abs() <= 3.0, "not equal quarters: {ws:?}");
		}
	}

	// The command line evens out a run of splits with no size, the same as the
	// keyboard does, and keeps a size it was given.
	// Test ID: ErCAz1p
	#[test]
	fn a_run_of_command_line_splits_with_no_size_comes_out_even() {
		let mut root = leaf(1);
		assert!(place_split(&mut root, 1, Dir::Vertical, 2, false, None));
		assert!(place_split(&mut root, 2, Dir::Vertical, 3, false, None));
		assert!(place_split(&mut root, 3, Dir::Vertical, 4, false, None));
		for (id, w) in widths(&root, 1200.0) {
			assert!(
				(w - 300.0).abs() <= 3.0,
				"pane {id} is {w} wide, not a quarter"
			);
		}
	}

	// Test ID: ErCAz5H
	#[test]
	fn a_command_line_split_with_a_size_keeps_it() {
		let mut root = leaf(1);
		assert!(place_split(
			&mut root,
			1,
			Dir::Vertical,
			2,
			false,
			Some(0.3)
		));
		assert!(place_split(&mut root, 2, Dir::Vertical, 3, false, None));
		let ws = widths(&root, 1000.0);
		assert!(
			(ws[0].1 - 700.0).abs() <= 3.0,
			"the sized split was evened out: {ws:?}"
		);
		assert!(
			(ws[1].1 - ws[2].1).abs() <= 3.0,
			"the next one halves what it split: {ws:?}"
		);
		// a pane put before the one it splits takes its share on that side
		let mut before = leaf(1);
		assert!(place_split(
			&mut before,
			1,
			Dir::Vertical,
			2,
			true,
			Some(0.25)
		));
		let ws = widths(&before, 1000.0);
		assert!((ws[1].1 - 250.0).abs() <= 3.0, "{ws:?}");
	}

	// Test ID: EivUOFe
	#[test]
	fn manual_divider_stops_equalization() {
		// the outer divider was hand-dragged (manual): a later split must not
		// re-equalize - the 0.7 ratio is preserved
		let mut root = split(
			Dir::Vertical,
			0.7,
			true,
			leaf(1),
			split(Dir::Vertical, 0.5, false, leaf(2), leaf(3)),
		);
		equalize_dir_run(&mut root, 3, Dir::Vertical);
		let Node::Split { ratio, .. } = &root else {
			panic!()
		};
		assert_eq!(*ratio, 0.7, "manual run must keep its sizes");
	}

	// Test ID: EivUOFf
	#[test]
	fn different_direction_counts_as_one_unit() {
		// a vertical run whose second member is a horizontal split: 2 units -> 50/50,
		// and the inner horizontal ratio is left untouched
		let mut root = split(
			Dir::Vertical,
			0.3,
			false,
			leaf(1),
			split(Dir::Horizontal, 0.4, false, leaf(2), leaf(3)),
		);
		equalize_dir_run(&mut root, 1, Dir::Vertical);
		let Node::Split { ratio, b, .. } = &root else {
			panic!()
		};
		assert!((ratio - 0.5).abs() < 0.01, "two units -> half each");
		let Node::Split { ratio: hr, .. } = b.as_ref() else {
			panic!()
		};
		assert_eq!(*hr, 0.4, "nested other-direction split is untouched");
	}

	// default pairs in precedence order: backtick, ", ', {}, (), [], <>
	const PAIRS: &[(char, char)] = &[
		('`', '`'),
		('"', '"'),
		('\'', '\''),
		('{', '}'),
		('(', ')'),
		('[', ']'),
		('<', '>'),
	];

	fn row(s: &str) -> Vec<char> {
		s.chars().collect()
	}

	// Test ID: Ei9srC4
	#[test]
	fn distinct_innermost() {
		let r = row("a (b [c] d) e");
		// click on 'c' (index 6): [] is inner, () is the outer
		assert_eq!(distinct_pair(&r, 6, '[', ']'), Some((5, 7)));
		assert_eq!(distinct_pair(&r, 6, '(', ')'), Some((2, 10)));
	}

	// Test ID: Ei9srC5
	#[test]
	fn precedence_paren_over_bracket() {
		let r = row("a (b [c] d) e");
		// inside both () and []; () has higher precedence -> select () contents
		// contents columns are 3..=9 ("b [c] d")
		assert_eq!(pair_inside(&r, 6, PAIRS), Some((3, 9)));
		assert_eq!(r[3..=9].iter().collect::<String>(), "b [c] d");
	}

	// Test ID: Ei9srC6
	#[test]
	fn bracket_only() {
		let r = row("x [y] z");
		assert_eq!(pair_inside(&r, 3, PAIRS), Some((3, 3))); // just "y"
	}

	// Test ID: Ei9srC7
	#[test]
	fn quotes_pair_left_to_right() {
		let r = row(r#"say "hello world" now"#);
		// click inside the quotes (e.g. index 8)
		assert_eq!(same_char_pair(&r, 8, '"'), Some((4, 16)));
		let (s, e) = pair_inside(&r, 8, PAIRS).unwrap();
		assert_eq!(r[s..=e].iter().collect::<String>(), "hello world");
	}

	// Test ID: Ei9srC8
	#[test]
	fn quote_beats_paren() {
		let r = row(r#"(a "b" c)"#);
		// inside both () and ""; "" higher precedence -> "b"
		let (s, e) = pair_inside(&r, 4, PAIRS).unwrap();
		assert_eq!(r[s..=e].iter().collect::<String>(), "b");
	}

	// Test ID: Ei9srC9
	#[test]
	fn outside_any_pair() {
		let r = row("just words here");
		assert_eq!(pair_inside(&r, 5, PAIRS), None);
	}

	// Test ID: Ei9srCA
	#[test]
	fn empty_pair_skipped() {
		// click between empty () - nothing inside, so no pair selection
		let r = row("a () b");
		assert_eq!(pair_inside(&r, 2, PAIRS), None);
	}

	// Test ID: EiEcYLo
	#[test]
	fn pair_trims_adjacent_spaces() {
		// spaces directly inside the delimiters are excluded; interior spaces kept
		let r = row(r#" " Now is the time. " "#);
		let (s, e) = pair_inside(&r, 6, PAIRS).unwrap();
		assert_eq!(r[s..=e].iter().collect::<String>(), "Now is the time.");
		// brackets too
		let r2 = row("a [   hi   ] b");
		let (s, e) = pair_inside(&r2, 6, PAIRS).unwrap();
		assert_eq!(r2[s..=e].iter().collect::<String>(), "hi");
		// all-spaces inside: nothing to trim to, keep the full inside span
		let r3 = row("a (   ) b");
		let (s, e) = pair_inside(&r3, 4, PAIRS).unwrap();
		assert_eq!(r3[s..=e].iter().collect::<String>(), "   ");
	}

	// Test ID: Ei9srCB
	#[test]
	fn on_open_char_uses_outer() {
		let r = row("(a [b] c)");
		// click exactly on '[' (index 3): not inside [], but inside () -> () contents
		let (s, e) = pair_inside(&r, 3, PAIRS).unwrap();
		assert_eq!(r[s..=e].iter().collect::<String>(), "a [b] c");
	}

	// Test ID: EorTe2K
	#[test]
	fn ledger_step_follows_the_engine_not_a_guess() {
		// alt screen: every recorded scroll slides, whatever its size - a burst
		// that replaced the whole screen between frames included
		assert_eq!(ledger_step(true, true, 0, true, 183), Some(183));
		assert_eq!(ledger_step(true, true, 0, false, -3), Some(-3));
		// nothing recorded: an in-place repaint (top, a progress bar) never slides
		assert_eq!(ledger_step(true, true, 0, false, 0), None);
		// normal screen: scrollback growth is the output ease's
		assert_eq!(ledger_step(false, true, 4, true, 4), None);
		// at the cap the whole-screen count is the output ease's too
		assert_eq!(ledger_step(false, true, 0, true, 2), None);
		// a region scroll (a status line pinned below) slides while following,
		// and nothing shows it while scrolled back
		assert_eq!(ledger_step(false, true, 0, false, 1), Some(1));
		assert_eq!(ledger_step(false, false, 0, false, 1), None);
	}

	// Test ID: EorTe2L
	#[test]
	fn fingerprint_runs_only_where_the_engine_recorded_nothing() {
		assert!(fingerprint_frame(true, true, 0, 0));
		assert!(!fingerprint_frame(true, true, 0, 5));
		assert!(fingerprint_frame(false, true, 0, 0)); // the ConPTY repaint
		assert!(!fingerprint_frame(false, true, 3, 0));
		assert!(!fingerprint_frame(false, false, 0, 0));
	}

	// Test ID: Ej1072G
	#[test]
	fn signed_shift_detects_both_directions_and_hard_cuts_the_rest() {
		let last = [10u64, 20, 30, 40, 50];
		// scrolled forward: content moved up 2 (cur top == last[2..])
		let fwd = [30, 40, 50, 60, 70];
		assert_eq!(scroll_shift_signed(&fwd, &last, 8), 2);
		// scrolled back: content moved down 1 (cur[1..] == last[..n-1])
		let back = [5, 10, 20, 30, 40];
		assert_eq!(scroll_shift_signed(&back, &last, 8), -1);
		// no motion, in-place change, and full turnover all hard-cut (0), never a guess
		assert_eq!(scroll_shift_signed(&last, &last, 8), 0);
		assert_eq!(scroll_shift_signed(&[11, 20, 30, 40, 50], &last, 8), 0);
		assert_eq!(scroll_shift_signed(&[60, 70, 80, 90, 99], &last, 8), 0);
		// a jump bigger than max is not eased
		assert_eq!(scroll_shift_signed(&fwd, &last, 1), 0);
		// real-app shape: the middle scrolls but a static status/input band at the
		// bottom stays put - the middle block still translates, so it's detected
		let last_s = [10u64, 20, 30, 40, 900, 901];
		let cur_s = [20u64, 30, 40, 50, 900, 901];
		assert_eq!(scroll_shift_signed(&cur_s, &last_s, 8), 1);
	}

	// Test ID: Ej4R4TA
	#[test]
	fn signed_shift_tolerates_static_top_band_and_rejects_static_fields() {
		// nano/muffer shape: a static title bar at the TOP and a status band at the
		// BOTTOM, with the middle region scrolling up by 1. The old top-anchored
		// matcher returned 0 here (row 0 never moved); the block matcher detects it.
		let last = [700u64, 701, 10, 20, 30, 40, 900, 901];
		let cur = [700u64, 701, 20, 30, 40, 50, 900, 901];
		assert_eq!(scroll_shift_signed(&cur, &last, 8), 1);
		// backward (middle slid down 1) with the same static bands
		let back = [700u64, 701, 5, 10, 20, 30, 900, 901];
		assert_eq!(scroll_shift_signed(&back, &last, 8), -1);
		// a large static/blank field matches positionally but hasn't MOVED - must not
		// be read as a scroll (this is the apt/blank-jitter guard). Here rows 1..6 are
		// all identical (a blank band); only row 0 changed, in place. No real scroll.
		let bl_last = [1u64, 5, 5, 5, 5, 5, 5, 9];
		let bl_cur = [2u64, 5, 5, 5, 5, 5, 5, 9];
		assert_eq!(scroll_shift_signed(&bl_cur, &bl_last, 8), 0);
	}

	// Test ID: EleVq3c
	#[test]
	fn repeated_output_on_a_half_empty_screen_is_not_a_scroll() {
		// A cleared screen, then the same short command run twice: the second
		// listing re-prints the first one's rows lower down while the originals
		// stay put. That's a COPY, not a translate - the blank field below
		// supplies enough positional matches to clear `need`, and the repeated
		// rows fall on formerly-blank targets, so the old moved test passed and
		// the brand-new output slid down out from under the prompt. The vacated
		// half of the moved test rejects it: the sources never left.
		const B: u64 = 77; // blank row fingerprint (identical for every blank row)
		let listing = [101u64, 102, 103, 104, 105, 106];
		let (p1, p2, p2c) = (200u64, 201, 202); // prompts; p2c = p2 + typed command
		let n = 24;
		let mut last = vec![p1];
		last.extend_from_slice(&listing);
		last.push(p2);
		last.resize(n, B);
		let mut cur = vec![p1];
		cur.extend_from_slice(&listing);
		cur.push(p2c);
		cur.extend_from_slice(&listing);
		cur.push(p2); // fresh prompt, same text as the last one
		cur.resize(n, B);
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 0);
	}

	// An option list (a question/select TUI): moving the highlight swaps in a
	// description block of a DIFFERENT height, so the short footer below it (hint
	// line, rule, key help) genuinely translates a row - and vacates its sources,
	// so the vacated-move test passes - while the rewritten description and the
	// two marker rows sit above it unexplained. Blank rows supply the positional
	// matches. A real scroll explains nearly all of a frame's change; this
	// explains about half, so the explanation gate rejects it in both detectors.
	fn list_relayout_frames() -> (Vec<u64>, Vec<u64>) {
		const B: u64 = 9; // blank rows all share one fingerprint
		let conv: Vec<u64> = (2000..2015).collect(); // static conversation above
		let mut small = conv.clone(); // highlight on option 1, two desc lines
		small.extend_from_slice(&[100, 110, B, 300, 301, B, 400, B, 500, 501, 502]);
		small.resize(30, B);
		let mut big = conv; // highlight on option 2, three desc lines
		big.extend_from_slice(&[101, 111, B, 310, 311, 312, B, 400, B, 500, 501, 502]);
		big.resize(30, B);
		(small, big)
	}

	// Test ID: EliKD6W
	#[test]
	fn list_relayout_description_height_change_is_not_a_scroll() {
		let (small, big) = list_relayout_frames();
		// grow: the footer slid DOWN one row; shrink: back UP one. Neither is a
		// scroll - the pane must hard-cut, not glide the list around.
		assert_eq!(scroll_shift_signed(&big, &small, APP_SCROLL_MAX), 0);
		assert_eq!(scroll_shift_signed(&small, &big, APP_SCROLL_MAX), 0);
	}

	// Test ID: EjFiQFc
	#[test]
	fn static_bands_measures_title_and_status() {
		// nano shape: static title (rows 0..2), scroll region (2..6), status band (6..8)
		let last = [700u64, 701, 10, 20, 30, 40, 900, 901];
		let cur = [700u64, 701, 20, 30, 40, 50, 900, 901];
		assert_eq!(static_bands(&cur, &last), (2, 2));
		// no bands: every row changed
		let a = [1u64, 2, 3, 4];
		let b = [5u64, 6, 7, 8];
		assert_eq!(static_bands(&a, &b), (0, 0));
		// a fully static frame would have the bands meet -> zeroed (no scroll region)
		assert_eq!(static_bands(&last, &last), (0, 0));
		// length mismatch is not measurable
		assert_eq!(static_bands(&a, &last), (0, 0));
	}

	// Test ID: ElvenXk
	#[test]
	fn a_scrollback_clear_reads_as_everything_that_refilled_it() {
		// ordinary growth
		assert_eq!(pushed_since(75, 0), 75);
		assert_eq!(pushed_since(76, 75), 1);
		assert_eq!(pushed_since(75, 75), 0);
		// `clear` truncates, then the same listing refills to the SAME depth. The
		// per-frame difference reads 0 here, which is the whole bug; sampled per
		// read cycle the pre-clear depth is still 76, and the drop says all 75
		// lines now in the buffer arrived after the clear.
		assert_eq!(pushed_since(75, 76), 75);
		// a bare `clear` with no output that follows must not arm anything
		assert_eq!(pushed_since(0, 76), 0);
		// entering the alt screen drops the depth to zero the same way
		assert_eq!(pushed_since(0, 500), 0);
	}

	// Test ID: ElvenXl
	#[test]
	fn missed_samples_are_recovered_not_lost() {
		// The sampler gives up rather than contend with the reader, so a busy pane
		// skips cycles. The baseline only advances on a successful sample, so the
		// next one spans everything that was missed.
		let (mut pushed, mut baseline) = (0usize, 10usize);
		for depth in [12usize, 15, 20] {
			// pretend the samples between these were all lock misses
			pushed += pushed_since(depth, baseline);
			baseline = depth;
		}
		assert_eq!(pushed, 10, "20 - 10, however many samples were dropped");
		// and a truncation inside the gap still arrives in full
		let mut pushed = 0usize;
		pushed += pushed_since(30, 20); // grew
		pushed += pushed_since(8, 30); // cleared, refilled to 8
		assert_eq!(pushed, 18);
	}

	// Build and wakeup sample the same baseline, in whichever order they reach
	// the grid, so a line pushed between them is banked by the first and found
	// already counted by the second. Two baselines counted it twice, and a
	// settled view hopped down a line and eased back.
	// Test ID: EoAik9o
	#[test]
	fn a_build_that_beats_the_wakeup_counts_a_line_once() {
		let mut depth = DepthSampler::default();
		depth.rebaseline(168);
		// the build first to the grid, then that read cycle's wakeup
		assert_eq!(depth.take(169), 1);
		depth.sample(169);
		assert_eq!(depth.take(169), 0);
		// the wakeup first, then the build
		depth.sample(170);
		assert_eq!(depth.take(170), 1);
		assert_eq!(depth.take(170), 0);
		// a clear-and-refill a wakeup saw is all new, even at the same depth
		depth.sample(20);
		depth.sample(170);
		assert_eq!(depth.take(170), 170);
		// a reflow's change of depth is not output
		depth.rebaseline(150);
		assert_eq!(depth.take(150), 0);
	}

	// A live overlay pinned to the bottom edge of a scrolling viewport, composited
	// OVER the last region row so that row changes on every step - muffer's "N new
	// messages"/"Jump to bottom" pill, captured from a real session: 30 rows, the
	// transcript sliding down one row per wheel notch, the pill at row 24, and five
	// rows of input box and hints below it. The unchanged-suffix walk stops below
	// the pill, so it used to sit inside the sliding region and ghost.
	fn muffer_pill_frames(pill_last: u64, pill_cur: u64) -> ([u64; 30], [u64; 30]) {
		let mut last = [0u64; 30];
		let mut cur = [0u64; 30];
		for row in 0..24 {
			last[row] = 100 + row as u64;
			// content moved DOWN one row (wheel up): row 0 is freshly revealed
			cur[row] = if row == 0 { 99 } else { 100 + row as u64 - 1 };
		}
		last[24] = pill_last;
		cur[24] = pill_cur;
		for row in 25..30 {
			last[row] = 900 + row as u64; // input box + hint rows, unchanged
			cur[row] = 900 + row as u64;
		}
		(cur, last)
	}

	// Test ID: ElvRIOW
	#[test]
	fn a_live_bottom_overlay_is_pinned_not_slid() {
		// the pill's text changes every step (it is composited over scrolling content)
		let (cur, last) = muffer_pill_frames(700, 701);
		let shift = scroll_shift_signed(&cur, &last, APP_SCROLL_MAX);
		assert_eq!(
			shift, -1,
			"a wheel notch back is still a clean one-row scroll"
		);
		// what the old unchanged-suffix walk saw: the pill row inside the region
		assert_eq!(static_bands(&cur, &last), (0, 5));
		// the shift's own extent stops at row 23, so the pill row is band
		assert_eq!(translate_span(&cur, &last, shift), Some((0, 23)));
		assert_eq!(slide_bands(&cur, &last, shift), (0, 6));
	}

	// Test ID: ElvRIOX
	#[test]
	fn pinning_the_overlay_keeps_it_out_of_the_strip() {
		// the rows a step retires must come from the region, never from the pinned
		// overlay - a strip row holding the pill would redraw it inside the reveal
		let (cur, last) = muffer_pill_frames(700, 701);
		let (st, sb) = slide_bands(&cur, &last, -1);
		let range = vanished_range(-1, st, sb, 30);
		assert_eq!(
			range,
			23..24,
			"the region's own last row leaves, not the pill's"
		);
		assert!(!range.contains(&24));
		// with the unchanged-suffix walk alone the pill's own row was retired into
		// the strip, which is what drew it a second time inside the reveal
		let (ost, osb) = static_bands(&cur, &last);
		assert!(vanished_range(-1, ost, osb, 30).contains(&24));
	}

	// Test ID: ElvRIOY
	#[test]
	fn a_band_never_swallows_a_row_that_scrolled() {
		// the safety property: an overlay stranded MID-region (muffer also floats a
		// dim scroll-hint arrow inside the viewport) must not hand every row past it
		// to the band - the span is anchored on the moved rows, so it can only be
		// widened outward, never pulled in across one.
		let (mut cur, mut last) = muffer_pill_frames(700, 701);
		last[12] = 555; // a live one-row overlay floating mid-transcript
		cur[12] = 556;
		let shift = scroll_shift_signed(&cur, &last, APP_SCROLL_MAX);
		assert_eq!(shift, -1);
		let (st, sb) = slide_bands(&cur, &last, shift);
		assert_eq!(
			(st, sb),
			(0, 6),
			"the mid-region overlay does not collapse the region"
		);
		assert!(30 - sb > 13, "rows below the stranded overlay still slide");
	}

	// Muffer on Enter with a five-line message, as it repaints at 50 rows: the
	// transcript climbs 3 rows, and the message climbs 6 out of the collapsing input
	// box, with the queued hint, spinner and a fresh box under it.
	fn muffer_enter_frames() -> (Vec<u64>, Vec<u64>) {
		let (rule, spin, foot, hint, prompt, busy) = (7, 8, 9, 11, 12, 13);
		let mut last = vec![0u64; 50];
		for (row, v) in last.iter_mut().enumerate().take(39) {
			*v = 1062 + row as u64;
		}
		last[40] = spin;
		last[42] = rule;
		for j in 0..5 {
			last[43 + j] = 2000 + j as u64;
		}
		last[48] = rule;
		last[49] = foot;
		let mut cur = vec![0u64; 50];
		for (row, v) in cur.iter_mut().enumerate().take(36) {
			*v = 1065 + row as u64;
		}
		for j in 0..5 {
			cur[37 + j] = 2000 + j as u64;
		}
		cur[42] = hint;
		cur[44] = spin;
		cur[46] = rule;
		cur[47] = prompt;
		cur[48] = rule;
		cur[49] = busy;
		(cur, last)
	}

	// Test ID: Eqj2FoO
	#[test]
	fn an_entered_message_slides_whole() {
		let (cur, last) = muffer_enter_frames();
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 3);
		assert_eq!(translate_span(&cur, &last, 3), Some((0, 39)));
		// the message ends on row 41: every line of it slides, the hint under it holds
		assert_eq!(slide_bands(&cur, &last, 3), (0, 8));
		assert_eq!(
			vanished_range(3, 0, 8, 50),
			0..3,
			"the strip is still the top rows"
		);
		// the same frames upside down: a step back takes rows above the span
		let (mut rc, mut rl) = (cur.clone(), last.clone());
		rc.reverse();
		rl.reverse();
		assert_eq!(scroll_shift_signed(&rc, &rl, APP_SCROLL_MAX), -3);
		assert_eq!(slide_bands(&rc, &rl, -3), (8, 0));
	}

	// Test ID: Eqj2FoP
	#[test]
	fn a_row_that_moved_less_or_nowhere_stays_band() {
		let (mut cur, mut last) = muffer_enter_frames();
		// the fourth and fifth lines climbed only 2: sliding them from 3 would drop
		// them a row first
		last[42] = 2003;
		last[43] = 2004;
		last[46] = 31;
		last[47] = 32;
		assert_eq!(slide_bands(&cur, &last, 3), (0, 10));
		// and a row redrawn in place is never taken
		let (c2, l2) = muffer_enter_frames();
		cur = c2;
		last = l2;
		last[40] = cur[40];
		assert_eq!(slide_bands(&cur, &last, 3), (0, 10));
	}

	// Test ID: ElvRIOZ
	#[test]
	fn bands_only_ever_grow_against_the_unchanged_walk() {
		// combining by MAX is what keeps this from regressing the settled shapes: for
		// every app in the matrix the pinned rows must be at least what they were.
		for (top, bot) in [(0usize, 1usize), (0, 2), (1, 2), (2, 2), (0, 0)] {
			for shift in [1i32, 2, -1, -2] {
				let (last, cur) = app_frames(24, top, bot, shift);
				let (sst, ssb) = static_bands(&cur, &last);
				let (st, sb) = slide_bands(&cur, &last, shift);
				assert!(
					st >= sst && sb >= ssb,
					"top={top} bot={bot} shift={shift}: bands shrank {sst},{ssb} -> {st},{sb}"
				);
				// and the settled app shapes are unchanged by the addition
				assert_eq!((st, sb), (top, bot), "top={top} bot={bot} shift={shift}");
			}
		}
	}

	// Test ID: EpyCuGg
	#[test]
	fn a_recorded_scroll_holds_an_overlay_repainted_at_its_strip_edge() {
		// the same pill, with the engine's record saying rows 0..25 scrolled: the rows
		// under the region are held by the region alone, the pill row by this
		let (cur, last) = muffer_pill_frames(700, 701);
		assert_eq!(repainted_edge(&cur, &last, &(0..25), -1), 1);
		// a clean scroll moved the edge row too
		let (mut cur, last) = muffer_pill_frames(700, 701);
		cur[24] = last[23];
		assert_eq!(repainted_edge(&cur, &last, &(0..25), -1), 0);
	}

	// Test ID: EpyCuGh
	#[test]
	fn an_overlay_over_the_top_edge_is_held_scrolling_forward() {
		let last: Vec<u64> = (0..24).map(|i| 100 + i).collect();
		let mut cur = last.clone();
		cur[1..19].copy_from_slice(&last[3..21]);
		cur[19] = 7;
		cur[20] = 8;
		cur[1] = 555;
		assert_eq!(repainted_edge(&cur, &last, &(1..21), 2), 1);
	}

	// Test ID: EpyCuGi
	#[test]
	fn a_frame_repainted_wholesale_holds_nothing() {
		let last: Vec<u64> = (0..30).map(|i| 100 + i).collect();
		let cur: Vec<u64> = (0..30).map(|i| 5000 + i).collect();
		assert_eq!(repainted_edge(&cur, &last, &(0..25), -1), 0);
		assert_eq!(repainted_edge(&cur, &last, &(0..25), 3), 0);
		assert_eq!(repainted_edge(&last, &last, &(0..25), 25), 0);
	}

	// ---- App-scroll scenario matrix -------------------------------------------
	// Per-app regression coverage for the alt-screen slide: each real full-screen
	// app repaints in a characteristic shape, and the (shift, static-band) pair the
	// detector extracts decides whether the pane slides smoothly or hard-cuts. These
	// model the shapes so a change to the detector/bands (or the SLIDE_TOP_BAND_APPS
	// toggle) is caught without a live GL run. The committed headless harness
	// (cicd/tests/scroll) exercises the same shapes end-to-end via SILK_SCROLLDBG.

	// Build a (last, cur) frame pair modeling a full-screen app whose middle scroll
	// region moved up by `shift` rows (forward = content scrolls up, newer rows in at
	// the bottom), with `top` static title rows above and `bot` static status rows
	// below. Row fingerprints are arbitrary distinct u64s viewing a rolling window, so
	// a shift reuses neighboring content rows exactly as a real repaint does.
	fn app_frames(rows: usize, top: usize, bot: usize, shift: i32) -> (Vec<u64>, Vec<u64>) {
		let pool: Vec<u64> = (1000u64..1000 + rows as u64 * 4).collect(); // content pool
		let title: Vec<u64> = (1u64..=top as u64).collect(); // static top band
		let status: Vec<u64> = (900u64..900 + bot as u64).collect(); // static bottom band
		let mid = rows - top - bot;
		let frame = |off: usize| -> Vec<u64> {
			let mut v = title.clone();
			v.extend_from_slice(&pool[off..off + mid]);
			v.extend_from_slice(&status);
			v
		};
		let base = rows; // window origin with room to move either way
		let last = frame(base);
		let cur = frame((base as i32 + shift) as usize);
		(last, cur)
	}

	// The build() decision: engage the smooth slide only when there's no static top
	// band, unless the top-band toggle is on. Mirrors the gate in build().
	fn slide_engages(top_band: usize) -> bool {
		SLIDE_TOP_BAND_APPS || top_band == 0
	}

	// Test ID: EjICKOW
	#[test]
	fn less_slides_no_top_band() {
		// less fills from the top and keeps only a bottom status line, so there's no
		// static top band: the middle scrolls, the detector sees it, and build slides.
		let (last, cur) = app_frames(24, 0, 1, 1);
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 1);
		let (st, sb) = static_bands(&cur, &last);
		assert_eq!(st, 0, "less has no static top band");
		assert_eq!(sb, 1, "less keeps a single-row status line");
		assert!(slide_engages(st), "less must slide smoothly");
	}

	// Test ID: EjICKOX
	#[test]
	fn vim_slides_no_top_band() {
		// vim/vim.tiny paints text from row 0 with a status + command line at the
		// bottom (two static rows), no title bar: same "no top band -> slide" as less.
		let (last, cur) = app_frames(24, 0, 2, 2);
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 2);
		let (st, sb) = static_bands(&cur, &last);
		assert_eq!(st, 0, "vim has no static top band");
		assert_eq!(sb, 2, "vim keeps a status + command line");
		assert!(slide_engages(st), "vim must slide smoothly");
	}

	// Test ID: EjOT3qq
	#[test]
	fn nano_slides_with_top_band() {
		// nano keeps a title bar at the top and a two-row help band at the bottom, so
		// the middle scroll region has a static top band. With SLIDE_TOP_BAND_APPS on
		// (the scrolled-off strip fills the reveal gap exactly) the slide engages; the
		// expectation tracks the toggle, and the band detection asserted below is the
		// real surface either way.
		let (last, cur) = app_frames(24, 1, 2, 1);
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 1);
		let (st, sb) = static_bands(&cur, &last);
		assert_eq!(st, 1, "nano keeps a title bar (static top band)");
		assert_eq!(sb, 2, "nano keeps a two-row help band");
		assert_eq!(
			slide_engages(st),
			SLIDE_TOP_BAND_APPS,
			"top-band app slides per the toggle"
		);
	}

	// Test ID: EjOT3qr
	#[test]
	fn muffer_slides_with_top_band() {
		// muffer (the TUI) keeps a static header, so like nano it has a top band and
		// follows the toggle. Model a two-row header + one-row footer.
		let (last, cur) = app_frames(30, 2, 1, 1);
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 1);
		let (st, _sb) = static_bands(&cur, &last);
		assert_eq!(st, 2, "muffer keeps a static header (top band)");
		assert_eq!(
			slide_engages(st),
			SLIDE_TOP_BAND_APPS,
			"top-band app slides per the toggle"
		);
	}

	// Test ID: EjICKOY
	#[test]
	fn app_wheel_multi_line_jump_still_detected() {
		// a wheel notch in a mouse-tracking app repaints a several-line jump, not one
		// line: it must still be detected as a clean scroll (up to APP_SCROLL_MAX), not
		// hard-cut as a page turnover. less-shaped so it slides.
		let (last, cur) = app_frames(40, 0, 1, 6);
		assert_eq!(scroll_shift_signed(&cur, &last, APP_SCROLL_MAX), 6);
		// but a jump past the window is not eased (hard-cut) - it isn't a clean scroll
		let (last2, cur2) = app_frames(40, 0, 1, (APP_SCROLL_MAX + 5) as i32);
		assert_eq!(scroll_shift_signed(&cur2, &last2, APP_SCROLL_MAX), 0);
	}

	// Test ID: EorTe2M
	#[test]
	fn strip_rows_style_the_rows_the_engine_kept() {
		// alt screen, no scrollback: the rows a scroll pushes off come back styled
		// for the strip, in screen order, in the color they were drawn in
		let (cols, lines) = (8usize, 4usize);
		let mut term = term_fed(cols, lines, 1000, "");
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		feed(
			&mut term,
			"\x1b[?1049h\x1b[31mred\x1b[0m\r\nnext\r\nthird\r\nlast",
		);
		feed(&mut term, "\r\n\r\n");
		assert_eq!(term.scroll_ledger().lines(), 2);
		let settings = config::Settings::default();
		let rows = strip_rows(term.scroll_ledger().rows(), cols, term.colors(), &settings);
		let text: Vec<String> = rows
			.iter()
			.map(|r| {
				r.iter()
					.map(|c| c.c)
					.collect::<String>()
					.trim_end()
					.to_string()
			})
			.collect();
		assert_eq!(text, ["red", "next"]);
		assert_ne!(rows[0][0].fg, rows[1][0].fg);
		// a resize forgets them: nothing of another width may reach the strip
		term.resize(crate::term::TermDimensions {
			columns: cols + 1,
			screen_lines: lines,
		});
		assert!(strip_rows(term.scroll_ledger().rows(), cols, term.colors(), &settings).is_empty());
	}

	// Test ID: EqAQm2a
	#[test]
	fn a_slide_survives_nanos_odd_region_step() {
		// nano scrolling up: ncurses scrolls the edit window (rows 2 to 45 of 48)
		// and, whenever the line leaving it is blank, the blank status row under
		// it as well (2 to 46). Each frame the pane reads the ledger, adds its rows
		// to the strip and grows the slide by the step. The odd step used to start
		// the ledger and the strip over, and the offset was then held to the one
		// row the fresh strip could cover: a jump of the whole lag, every other
		// step. The run opens on the wider region so the strip is also seen
		// keeping its rows when the ledger narrows.
		let _g = config::test_store_lock();
		config::update(config::Settings::default());
		let (cols, lines) = (12usize, 48usize);
		let mut term = term_fed(cols, lines, 1000, "\x1b[?1049h");
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		for i in 0..lines {
			feed(&mut term, &format!("\x1b[{};1Hrow {i}", i + 1));
		}
		let settings = config::Settings::default();
		let mut strip = OffStrip::new();
		let mut scroll = crate::scroll::Scroll::new();
		let mut offsets = Vec::new();
		let ends = [46, 45, 45, 46, 45, 45, 46, 46, 45];
		for end in ends {
			feed(&mut term, &format!("\x1b[2;{end}r\x1b[2;1H\x1bM\x1b[1;48r"));
			let ledger = term.scroll_ledger();
			let region = ledger.region().start.0 as usize..ledger.region().end.0 as usize;
			let step = ledger.lines();
			let chunk = strip_rows(ledger.rows(), cols, term.colors(), &settings);
			term.scroll_ledger_mut().drain(); // a slide is in flight from here on
			assert_eq!(step, -1);
			let want = if offsets.is_empty() { end } else { 45 };
			assert_eq!(region, 1..want, "narrowed to what both moved");
			strip.push_step(-1, (region.start, region.end), chunk);
			let rows = (region.end - region.start) as f32;
			scroll.app_scroll(step as f32, strip.len() as f32, rows);
			offsets.push(scroll.app_offset());
		}
		// every step grew the slide by its line; none pulled it back toward rest
		let expect: Vec<f32> = (1..=ends.len()).map(|n| -(n as f32)).collect();
		assert_eq!(offsets, expect);
		// the strip holds the rows in the order they sat, nearest the content first:
		// the row that left row 44 each step, plus the one row 45 gave up when the
		// region narrowed
		let text: Vec<String> = strip
			.rows
			.iter()
			.map(|r| {
				r.iter()
					.map(|c| c.c)
					.collect::<String>()
					.trim_end()
					.to_string()
			})
			.collect();
		assert_eq!(text.len(), ends.len() + 1);
		assert_eq!(text.first().map(String::as_str), Some("row 36"));
		assert_eq!(text.last().map(String::as_str), Some("row 45"));
		assert!(
			text.windows(2).all(|w| w[0] < w[1]),
			"screen order: {text:?}"
		);
	}

	// An input box on a half-empty screen: three transcript rows, a border, `h`
	// input lines, a border and two footer rows, the rest blank.
	fn input_box(h: usize) -> String {
		use std::fmt::Write as _;
		let mut input = String::new();
		for i in 1..=h {
			let _ = write!(input, "\r\n| in {i} |");
		}
		format!(
			"\x1b[2J\x1b[H  one\r\n  two\r\n  three\r\n+------+{input}\r\n+------+\r\n  ? help\r\n  footer"
		)
	}

	// Test ID: EqLbxfU
	#[test]
	fn a_box_growing_into_blank_rows_by_insert_line_pops_in() {
		// muffer's input box taking a paste: insert-line pushes the border and
		// footer down into rows that were blank, which the engine records as a
		// region scroll down. Sliding it dropped the new lines out from behind the
		// rows above, last line first.
		let (cols, lines) = (12usize, 16usize);
		let mut term = term_fed(cols, lines, 1000, &input_box(1));
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		let settings = config::Settings::default();
		let mut last = Vec::new();
		snapshot_rows(
			term.grid(),
			lines,
			cols,
			Some((term.colors(), &settings, &mut last)),
		);
		term.scroll_ledger_mut().clear();
		feed(&mut term, "\x1b[6;1H\x1b[2L| in 2 |\r\n| in 3 |");
		let ledger = term.scroll_ledger();
		let region = ledger.region().start.0 as usize..ledger.region().end.0 as usize;
		assert_eq!((ledger.lines(), region.clone()), (-2, 5..lines));
		let chunk = strip_rows(ledger.rows(), cols, term.colors(), &settings);
		assert_eq!(ledger_step(false, true, 0, false, -2), Some(-2));
		assert!(
			slide_is_visible(-2, &region, &last, &chunk),
			"the footer moved, so the old rule slid it"
		);
		assert!(ledger_makes_room(-2, &region, lines, &last, &chunk));

		// the same insert on a screen with no room pushes the footer's last row off
		let mut full = term_fed(cols, 8, 1000, &input_box(1));
		full.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		let mut full_last = Vec::new();
		snapshot_rows(
			full.grid(),
			8,
			cols,
			Some((full.colors(), &settings, &mut full_last)),
		);
		full.scroll_ledger_mut().clear();
		feed(&mut full, "\x1b[6;1H\x1b[L| in 2 |");
		let ledger = full.scroll_ledger();
		let region = ledger.region().start.0 as usize..ledger.region().end.0 as usize;
		let chunk = strip_rows(ledger.rows(), cols, full.colors(), &settings);
		assert_eq!(ledger.lines(), -1);
		assert!(!ledger_makes_room(-1, &region, 8, &full_last, &chunk));

		// vim and nano scroll back in a region above their status rows; a blank
		// file line leaving it is not room
		let empty = vec![cells("", cols); lines];
		let gone = [cells("", cols)];
		let whole = 1..lines;
		assert!(!ledger_makes_room(
			-1,
			&(1..lines - 2),
			lines,
			&empty,
			&gone
		));
		assert!(ledger_makes_room(-1, &whole, lines, &empty, &gone));
		assert!(!ledger_makes_room(1, &whole, lines, &empty, &gone), "up");
		assert!(
			!ledger_makes_room(-1, &whole, lines, &empty, &[]),
			"none kept"
		);
		assert!(
			!ledger_makes_room(-1, &whole, lines, &[], &gone),
			"no frame"
		);
	}

	// Test ID: EqLgrmK
	#[test]
	fn a_pager_scrolling_back_is_not_room() {
		// less (and man) answer a step back by clearing the prompt row, then a
		// reverse index at the top, then the prompt again, all in one write. The
		// row the engine keeps is the prompt row already blanked, so only the last
		// frame shows it held ink.
		use std::fmt::Write as _;
		let (cols, lines) = (12usize, 24usize);
		let mut text = String::from("\x1b[?1049h\x1b[H");
		for i in 0..lines - 1 {
			let _ = write!(text, "\x1b[{};1Hline {}", i + 1, 100 + i);
		}
		let _ = write!(text, "\x1b[{lines};1H:");
		let mut term = term_fed(cols, lines, 1000, &text);
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		let settings = config::Settings::default();
		let mut last = Vec::new();
		snapshot_rows(
			term.grid(),
			lines,
			cols,
			Some((term.colors(), &settings, &mut last)),
		);
		term.scroll_ledger_mut().clear();
		feed(
			&mut term,
			"\r\x1b[K\x1b[H\x1bMline 99\r\n\x1b[24;1H\r\x1b[K:\x1b[K",
		);
		let ledger = term.scroll_ledger();
		let region = ledger.region().start.0 as usize..ledger.region().end.0 as usize;
		assert_eq!((ledger.lines(), region.clone()), (-1, 0..lines));
		let chunk = strip_rows(ledger.rows(), cols, term.colors(), &settings);
		assert!(
			!chunk.iter().any(|r| has_ink(r)),
			"the kept row was blanked"
		);
		assert!(slide_is_visible(-1, &region, &last, &chunk));
		assert!(!ledger_makes_room(-1, &region, lines, &last, &chunk));
	}

	// Test ID: EqLbxfV
	#[test]
	fn a_repainted_box_growing_into_blank_rows_pops_in() {
		// The same box redrawn in place, the way muffer paints: nothing scrolls the
		// grid, and the fingerprints read the border and footer moving down.
		let (cols, lines) = (12usize, 16usize);
		let rows = |text: &str| {
			let term = term_fed(cols, lines, 1000, text);
			snapshot_rows(term.grid(), lines, cols, None)
		};
		let blank = fnv_row(std::iter::repeat_n(' ', cols));
		let (last, cur) = (rows(&input_box(1)), rows(&input_box(3)));
		let shift = scroll_shift_signed(&cur, &last, APP_SCROLL_MAX);
		assert_eq!(shift, -2, "the detector reads it as a scroll down");
		assert!(shift_makes_room(&cur, &last, shift, blank));
		// shrinking back is a scroll up into the same blank rows, and slides
		let shift = scroll_shift_signed(&last, &cur, APP_SCROLL_MAX);
		assert_eq!(shift, 2);
		assert!(
			!shift_makes_room(&last, &cur, shift, blank),
			"up is never room"
		);

		// A box with no room under it scrolls the grid instead, which the output
		// ease owns. vim scrolling back over its file with a status row under it
		// is a shift down that is not room. Lines from 21 on are blank, so the
		// rows leaving the span are blank while the status row under them is not.
		let file = |first: usize| {
			use std::fmt::Write as _;
			let mut text = String::new();
			for i in (0..lines - 1).filter(|i| first + i < 21) {
				let _ = write!(text, "\x1b[{};1Hline {}", i + 1, first + i);
			}
			format!("\x1b[2J\x1b[H{text}\x1b[{lines};1H-- status --")
		};
		let (last, cur) = (rows(&file(10)), rows(&file(8)));
		let shift = scroll_shift_signed(&cur, &last, APP_SCROLL_MAX);
		assert_eq!(shift, -2);
		assert_eq!((last[13], last[14]), (blank, blank));
		assert!(
			!shift_makes_room(&cur, &last, shift, blank),
			"the status row"
		);
	}

	fn cells(text: &str, cols: usize) -> Vec<StripCell> {
		(0..cols)
			.map(|i| StripCell {
				c: text.chars().nth(i).unwrap_or(' '),
				fg: [200; 3],
				bg: None,
				bold: false,
				italic: false,
				wide: 1,
			})
			.collect()
	}

	fn band_rows(term: &Term<VoidListener>, lines: usize, cols: usize) -> Vec<u64> {
		snapshot_rows(term.grid(), lines, cols, None)
	}
	fn blank_row(cols: usize) -> u64 {
		fnv_row(std::iter::repeat_n(' ', cols))
	}

	// Test ID: EpaxTdo
	#[test]
	fn a_progress_line_redrawn_under_new_output_is_held_still() {
		let (cols, lines) = (30usize, 8usize);
		let mut s = (0..20)
			.map(|i| format!("   Compiling c{i}\r\n"))
			.collect::<Vec<_>>()
			.concat();
		s.push_str("    Building [=>   ] 0/9");
		let mut term = term_fed(cols, lines, 1000, &s);
		let last = band_rows(&term, lines, cols);
		let h0 = term.grid().history_size();
		feed(
			&mut term,
			"\r\x1b[K   Compiling b\r\n    Building [=>   ] 0/9",
		);
		let k = term.grid().history_size() - h0;
		assert_eq!(k, 1);
		let cur = band_rows(&term, lines, cols);
		assert_eq!(output_band(&cur, &last, k, 0, blank_row(cols)), 1);

		// the bar's text changed on the same tick: nothing anchors it (a known limit)
		let mut term = term_fed(cols, lines, 1000, &s);
		let last = band_rows(&term, lines, cols);
		feed(
			&mut term,
			"\r\x1b[K   Compiling b\r\n    Building [==>  ] 1/9",
		);
		let cur = band_rows(&term, lines, cols);
		assert_eq!(output_band(&cur, &last, 1, 0, blank_row(cols)), 0);
	}

	// Test ID: EpaxTdp
	#[test]
	fn a_live_block_under_a_transcript_is_held_still() {
		let (cols, lines) = (24usize, 12usize);
		let block = |n: usize| format!("+--------+\r\n| work {n} |\r\n+--------+\r\n");
		let mut s = (0..30)
			.map(|i| format!("t{i:02}\r\n"))
			.collect::<Vec<_>>()
			.concat();
		s.push_str(&block(1));
		let mut term = term_fed(cols, lines, 1000, &s);
		let last = band_rows(&term, lines, cols);
		let h0 = term.grid().history_size();
		feed(&mut term, &format!("\x1b[3A\r\x1b[Jt30\r\n{}", block(2)));
		let k = term.grid().history_size() - h0;
		assert_eq!(k, 1);
		let cur = band_rows(&term, lines, cols);
		// three block rows and the blank cursor row under them, anchored by a border
		assert_eq!(output_band(&cur, &last, k, 0, blank_row(cols)), 4);
	}

	// Test ID: EpaxTdq
	#[test]
	fn a_prompt_that_scrolls_up_is_not_held() {
		let (cols, lines) = (20usize, 8usize);
		let s = (0..20)
			.map(|i| format!("out {i}\r\n"))
			.collect::<Vec<_>>()
			.concat();
		for prompt in ["$ ", "user@host\r\n$ "] {
			let mut term = term_fed(cols, lines, 1000, &format!("{s}{prompt}"));
			let last = band_rows(&term, lines, cols);
			let h0 = term.grid().history_size();
			feed(&mut term, &format!("\r\n{prompt}"));
			let k = term.grid().history_size() - h0;
			assert!(k > 0);
			let cur = band_rows(&term, lines, cols);
			assert_eq!(
				output_band(&cur, &last, k, 0, blank_row(cols)),
				0,
				"{prompt:?}"
			);
		}
	}

	// Test ID: EpaxTdr
	#[test]
	fn a_line_finished_across_two_reads_is_not_held() {
		let (cols, lines) = (20usize, 8usize);
		let mut s = (0..20)
			.map(|i| format!("out {i}\r\n"))
			.collect::<Vec<_>>()
			.concat();
		s.push_str("123");
		let mut term = term_fed(cols, lines, 1000, &s);
		let last = band_rows(&term, lines, cols);
		let h0 = term.grid().history_size();
		feed(&mut term, "4\r\n1235\r\n12");
		let k = term.grid().history_size() - h0;
		assert_eq!(k, 2);
		let cur = band_rows(&term, lines, cols);
		assert_eq!(output_band(&cur, &last, k, 0, blank_row(cols)), 0);
	}

	// Test ID: EpaxTds
	#[test]
	fn output_band_never_holds_plain_output() {
		let blank = 0u64;
		let last: Vec<u64> = (1..=8).collect();
		let cur: Vec<u64> = (3..=10).collect();
		assert_eq!(output_band(&cur, &last, 2, 0, blank), 0, "plain output");
		assert_eq!(output_band(&[7; 8], &[7; 8], 2, 0, blank), 0, "yes");
		assert_eq!(output_band(&cur, &last, 8, 0, blank), 0, "k >= n");
		assert_eq!(output_band(&cur, &last, 0, 3, blank), 0, "k == 0");
		assert_eq!(output_band(&cur[..7], &last, 2, 0, blank), 0, "lengths");
		// half the screen or more is not a status line
		let last = [1, 2, 3, 4, 50, 60, 70, 80];
		let cur = [2, 3, 4, 9, 50, 60, 70, 80];
		assert_eq!(output_band(&cur, &last, 1, 0, blank), 0);
		// blank rows alone anchor nothing
		let last = [1, 2, 3, 4, 5, 6, 0, 0];
		let cur = [2, 3, 4, 5, 6, 9, 0, 0];
		assert_eq!(output_band(&cur, &last, 1, 0, blank), 0);
	}

	// Test ID: EpaxTdt
	#[test]
	fn a_status_bar_below_the_scroll_region_is_held_still() {
		// apt: a region over all but the last row, its bar redrawn on that row
		let (cols, lines) = (20usize, 8usize);
		let mut term = term_fed(cols, lines, 1000, "\x1b[8;1Hbar 1/9\x1b[1;7r\x1b[7;1H");
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		for i in 0..10 {
			feed(&mut term, &format!("\r\nline {i}"));
		}
		term.scroll_ledger_mut().clear();
		let last = band_rows(&term, lines, cols);
		let h0 = term.grid().history_size();
		feed(&mut term, "\r\nline x\x1b7\x1b[8;1H\x1b[Kbar 2/9\x1b8");
		let k = term.grid().history_size() - h0;
		assert_eq!(k, 1, "the region's top row went to history");
		let ledger = term.scroll_ledger();
		let (start, end) = (
			ledger.region().start.0.max(0) as usize,
			(ledger.region().end.0.max(0) as usize).min(lines),
		);
		assert!(ledger.lines() > 0 && start == 0 && end < lines);
		let cur = band_rows(&term, lines, cols);
		assert_eq!(output_band(&cur, &last, k, lines - end, blank_row(cols)), 1);
	}

	// What a build makes of one burst of output: the lines the output ease has
	// to cover, and the region slide if the ease does not own the motion.
	fn advance_and_step(
		term: &Term<VoidListener>,
		lines: usize,
		baseline: usize,
	) -> (usize, Option<i32>) {
		let ledger = term.scroll_ledger();
		let region = ledger.region();
		let whole = region.start.0 <= 0 && region.end.0.max(0) as usize >= lines;
		let grew = pushed_since(term.grid().history_size(), baseline);
		let advanced = output_advance(false, true, grew, ledger.pushed());
		(
			advanced,
			ledger_step(false, true, advanced, whole, ledger.lines()),
		)
	}

	// Test ID: EqHIGdM
	#[test]
	fn a_status_bar_is_held_still_at_a_full_scrollback_too() {
		// The same apt stream with the scrollback full, which is where a long
		// install spends its time. The depth stops growing there, so the
		// engine's own count has to carry the line.
		let (cols, lines) = (20usize, 8usize);
		for scrollback in [1000usize, 20] {
			let mut term = term_fed(cols, lines, scrollback, "");
			term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
			for i in 0..40 {
				feed(&mut term, &format!("\r\nfill {i}"));
			}
			feed(&mut term, "\x1b[8;1Hbar 1/9\x1b[1;7r\x1b[7;1H");
			for i in 0..10 {
				feed(&mut term, &format!("\r\nline {i}"));
			}
			term.scroll_ledger_mut().clear();
			let last = band_rows(&term, lines, cols);
			let h0 = term.grid().history_size();
			assert_eq!(h0 == scrollback, scrollback == 20);
			feed(&mut term, "\r\nline x\x1b7\x1b[8;1H\x1b[Kbar 2/9\x1b8");
			let (advanced, step) = advance_and_step(&term, lines, h0);
			assert_eq!(
				(advanced, step),
				(1, None),
				"scrollback {scrollback}: the line eases as output"
			);
			let end = (term.scroll_ledger().region().end.0.max(0) as usize).min(lines);
			let cur = band_rows(&term, lines, cols);
			assert_eq!(
				output_band(&cur, &last, advanced, lines - end, blank_row(cols)),
				1
			);
		}
	}

	// Test ID: EqHIGdN
	#[test]
	fn a_clear_eases_the_same_at_a_full_scrollback() {
		let (cols, lines) = (20usize, 8usize);
		for scrollback in [1000usize, 20] {
			let mut term = term_fed(cols, lines, scrollback, "");
			term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
			for i in 0..40 {
				feed(&mut term, &format!("\r\nfill {i}"));
			}
			feed(&mut term, "\x1b[H\x1b[J\r\none\r\ntwo\r\nthree");
			term.scroll_ledger_mut().clear();
			let h0 = term.grid().history_size();
			feed(&mut term, "\x1b[2J");
			let (advanced, step) = advance_and_step(&term, lines, h0);
			assert_eq!((advanced, step), (4, None), "scrollback {scrollback}");
		}
	}

	// Test ID: EqHIGdO
	#[test]
	fn the_scroll_record_costs_a_full_screen_program_little() {
		// tmux, vim and less scroll a screen that keeps no history, so the engine
		// keeps the rows that leave for the slide to draw. It copied each one, and
		// that took a third off the parse. Same flood with the rows kept and
		// without, turn about, best of each.
		let (cols, lines) = (160usize, 42usize);
		let line = format!("{}\r\n", "0123456789".repeat(8));
		let flood = line.repeat(4000);
		let run = |keep: usize| {
			let mut term = term_fed(cols, lines, 1000, "\x1b[?1049h");
			term.set_scroll_ledger_rows(keep);
			let mut parser: Processor = Processor::new();
			let start = std::time::Instant::now();
			for chunk in flood.as_bytes().chunks(64 * 1024) {
				parser.advance(&mut term, chunk);
			}
			assert_eq!(
				term.scroll_ledger().rows().len(),
				keep.min(crate::scroll::SLIDE_ROWS)
			);
			start.elapsed()
		};
		// The rest of the suite runs beside this, so a slow pass gets more turns
		// before it counts. The copy was 28% over in every one.
		let (mut kept, mut bare) = (std::time::Duration::MAX, std::time::Duration::MAX);
		let within = |kept: std::time::Duration, bare: std::time::Duration| {
			kept.as_secs_f64() <= bare.as_secs_f64() * 1.10
		};
		for _ in 0..4 {
			for _ in 0..7 {
				kept = kept.min(run(crate::scroll::SLIDE_ROWS));
				bare = bare.min(run(0));
			}
			if within(kept, bare) {
				break;
			}
		}
		eprintln!("rows kept {kept:?}, none {bare:?}");
		assert!(
			within(kept, bare),
			"keeping rows cost {kept:?} against {bare:?}"
		);
	}

	// Test ID: EpaxTdu
	#[test]
	fn the_output_band_grows_while_easing_and_is_taken_fresh_at_rest() {
		assert_eq!(adopt_band(0, 3, false), 3);
		assert_eq!(adopt_band(4, 1, false), 1, "at rest the measurement stands");
		assert_eq!(adopt_band(1, 4, true), 4);
		assert_eq!(adopt_band(4, 0, true), 4, "never shrinks mid-ease");
	}

	// Test ID: EpaxTdv
	#[test]
	fn band_rows_draw_the_live_grid_and_the_rest_the_scrolled_view() {
		assert_eq!(band_row_line(3, 2, 6, 2), 1);
		assert_eq!(band_row_line(6, 2, 6, 2), 6);
		assert_eq!(band_row_line(7, 2, 6, 2), 7);
		assert_eq!(band_row_line(-1, 2, 6, 2), -3);
		assert_eq!(band_row_line(7, 2, 6, 0), 5, "no band");
	}

	// Test ID: Ep3UXRI
	#[test]
	fn a_scroll_of_blank_rows_is_not_slid() {
		// ble.sh, after a command: two linefeeds, then insert- and delete-line on
		// the blank rows under the cursor to make room for the prompt. The engine
		// records the last of those as a region scroll of three; nothing visible
		// moved, so there is nothing to ease - and easing it slid the prompt drawn
		// right after down from behind the rows above.
		let (cols, lines) = (10usize, 8usize);
		let mut term = term_fed(cols, lines, 1000, "one\r\ntwo\r\n$ cmd\r\nout\r\n");
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		let settings = config::Settings::default();
		let mut last = Vec::new();
		snapshot_rows(
			term.grid(),
			lines,
			cols,
			Some((term.colors(), &settings, &mut last)),
		);
		feed(
			&mut term,
			"\r\n\r\n\x1b[1A\x1b[2L\x1b[2K\x1b[1A\x1b[2K\x1b[3M\x1b[3L\x1b[2Kline one\r\n$ ",
		);
		let ledger = term.scroll_ledger();
		assert_eq!(ledger.lines(), -3);
		let region = ledger.region().start.0 as usize..ledger.region().end.0 as usize;
		assert_eq!(region, 4..8);
		assert_eq!(
			ledger.pushed(),
			0,
			"the screen was not full: nothing left it"
		);
		let chunk = strip_rows(ledger.rows(), cols, term.colors(), &settings);
		assert!(!slide_is_visible(-3, &region, &last, &chunk));
		assert_eq!(
			ledger_step(false, true, 0, false, -3),
			Some(-3),
			"the record alone would slide"
		);

		// the same shape over rows that hold text is a real scroll, either way round
		let mut inked = last.clone();
		inked[4] = cells("stale", cols);
		assert!(slide_is_visible(-3, &region, &inked, &chunk));
		assert!(slide_is_visible(3, &(1..8), &inked, &chunk));
		assert!(
			!slide_is_visible(3, &(4..8), &last, &chunk),
			"up: sources are the rows below"
		);
		// so is one whose rows that left carried ink
		assert!(slide_is_visible(-3, &region, &last, &[cells("gone", cols)]));
		// a background counts as ink
		let mut bg = last.clone();
		bg[4][0].bg = Some([1, 2, 3]);
		assert!(slide_is_visible(-3, &region, &bg, &chunk));
		// a turnover past the region is a scroll whatever was there, one to the
		// exact height is not (insert-line clamps to the rows it has)
		assert!(slide_is_visible(5, &region, &last, &chunk));
		assert!(!slide_is_visible(4, &region, &last, &chunk));
		// no snapshot to read: slide, as before
		assert!(slide_is_visible(-3, &region, &[], &chunk));
	}

	// Test ID: Ep3UXRJ
	#[test]
	fn the_output_ease_keeps_its_count_through_a_prompt_redraw() {
		// With the scrollback full the growth reads 0 and the whole-screen count
		// is the only record of what left; a prompt's region churn after it used
		// to start the ledger over and lose it.
		assert_eq!(output_advance(false, true, 0, 2), 2);
		assert_eq!(output_advance(false, true, 3, 3), 3);
		assert_eq!(
			output_advance(false, true, 1, 2),
			2,
			"the cap fills mid-gap"
		);
		assert_eq!(
			output_advance(false, false, 0, 2),
			0,
			"scrolled back: nothing to chase"
		);
		assert_eq!(
			output_advance(true, true, 0, 2),
			0,
			"alt screen: the slide's, not the ease's"
		);
		assert_eq!(
			ledger_step(false, true, 2, false, -3),
			None,
			"the ease owns the frame"
		);
	}

	// Test ID: EkmETLs
	#[test]
	fn output_frame_refreshes_snapshot_so_repaint_probe_cannot_reslide() {
		// One shell "enter", as frames: the scroll arrives in a grew=1 frame (animated
		// by the output ease), the prompt redraw arrives one frame later with grew=0 -
		// a slide frame. A snapshot left stale across the grew frame makes that
		// slide frame read the eased scroll as a fresh 1-line repaint shift and
		// slide it a second time - the "down one line, then up two" output judder.
		// The gate refreshes the snapshot on the grew frame, so the slide frame
		// diffs only the in-place prompt redraw: no shift.
		use std::fmt::Write as _;
		let (cols, lines) = (20usize, 12usize);
		let mut prime = String::new();
		for i in 0..lines {
			let _ = write!(prime, "prime row {i} qz{i}{i}\r\n");
		}
		let mut term = term_fed(cols, lines, 1000, &prime);
		let stale = snapshot_rows(term.grid(), lines, cols, None);
		// grew frame: a new output line scrolls the screen; snapshot, don't slide
		feed(&mut term, "output line abcdef\r\n");
		assert!(!fingerprint_frame(false, true, 1, 0));
		let fresh = snapshot_rows(term.grid(), lines, cols, None);
		// slide frame: the prompt redraw, in place
		feed(&mut term, "$ ");
		let cur = snapshot_rows(term.grid(), lines, cols, None);
		let vs_stale = scroll_shift_signed(&cur, &stale, APP_SCROLL_MAX);
		let vs_fresh = scroll_shift_signed(&cur, &fresh, APP_SCROLL_MAX);
		assert_eq!(vs_stale, 1, "a stale snapshot re-reads the eased scroll");
		assert_eq!(
			vs_fresh, 0,
			"a fresh snapshot sees only the in-place redraw"
		);
	}

	// ---- Scrolled-off strip -----------------------------------------------------

	// a marker row for strip tests: one cell whose char encodes the row identity
	fn strip_row(tag: char) -> Vec<StripCell> {
		vec![StripCell {
			c: tag,
			fg: [255; 3],
			bg: None,
			bold: false,
			italic: false,
			wide: 1,
		}]
	}

	fn strip_tags(s: &OffStrip) -> String {
		s.rows.iter().map(|r| r[0].c).collect()
	}

	// Test ID: EjOT3qs
	#[test]
	fn vanished_range_picks_the_rows_a_step_pushed_off() {
		// 10 lines, title 1 row, status 2 rows -> region rows 1..8
		// content moved up 2: the region's top two rows left off the top
		assert_eq!(vanished_range(2, 1, 2, 10), 1..3);
		// content moved down 2: the region's bottom two rows left off the bottom
		assert_eq!(vanished_range(-2, 1, 2, 10), 6..8);
		// no bands (less): rows come off the screen edges
		assert_eq!(vanished_range(1, 0, 0, 10), 0..1);
		assert_eq!(vanished_range(-1, 0, 0, 10), 9..10);
		// a shift bigger than the region clamps to it (nothing panics)
		assert_eq!(vanished_range(50, 1, 2, 10), 1..8);
		assert_eq!(vanished_range(-50, 1, 2, 10), 1..8);
	}

	// Test ID: EjRuzdg
	#[test]
	fn region_clip_welds_to_the_content_edge() {
		// down-slide (voff +2 cells, cell_h 20): bands at y=20 (title) / y=160
		// (status); content starts at 20+40=60. The gap 20..60 belongs to the
		// strip - the clip must start at the content edge so the title's
		// translated copy (drawn at 40..60) is cut off. Bottom stays band-bound.
		assert_eq!(weld_region_clip(20.0, 160.0, 60.0, 200.0), (60.0, 160.0));
		// up-slide (voff -2): content ends at 160-40=120; the status rows'
		// translated copies (drawn just above 120) must be cut, gap 120..160 is
		// the strip's. Top stays band-bound.
		assert_eq!(weld_region_clip(20.0, 160.0, -20.0, 120.0), (20.0, 120.0));
		// no top band: f32::MIN stays open until the content edge
		assert_eq!(
			weld_region_clip(f32::MIN, 160.0, 40.0, 200.0),
			(40.0, 160.0)
		);
	}

	// Test ID: EjOT3qt
	#[test]
	fn off_strip_accumulates_in_visual_order() {
		// up-scroll: each step's rows leave off the region's top, newest nearest the
		// content = at the strip's bottom
		let mut s = OffStrip::new();
		s.push_step(1, (0, 10), vec![strip_row('a'), strip_row('b')]);
		s.push_step(1, (0, 10), vec![strip_row('c')]);
		assert_eq!(strip_tags(&s), "abc");
		// a slide shows the rows nearest the content, plus one for the part-row
		assert_eq!(s.visible(1.0), 1..3);
		assert_eq!(s.visible(0.4), 1..3);
		assert_eq!(s.visible(50.0), 0..3);
		// down-scroll: rows leave off the bottom, newest at the strip's top,
		// each chunk keeping its internal order
		let mut d = OffStrip::new();
		d.push_step(-1, (0, 10), vec![strip_row('y'), strip_row('z')]);
		d.push_step(-1, (0, 10), vec![strip_row('w'), strip_row('x')]);
		assert_eq!(strip_tags(&d), "wxyz");
		assert_eq!(d.visible(-1.2), 0..3);
	}

	// Test ID: EjOT3qu
	#[test]
	fn off_strip_direction_flip_discards_and_cap_trims_oldest() {
		let mut s = OffStrip::new();
		s.push_step(1, (0, 10), vec![strip_row('a'), strip_row('b')]);
		// flipping direction discards the old strip (it belongs on the other side)
		s.push_step(-1, (0, 10), vec![strip_row('c')]);
		assert_eq!(strip_tags(&s), "c");
		assert_eq!(s.dir, -1);
		// Until the ledger merged overlapping regions, so did a step from any
		// other region:
		//   s.push_step(-1, (1, 9), vec![strip_row('d')]);
		//   assert_eq!(strip_tags(&s), "d");
		// Now one that shares rows carries on (the ledger has narrowed to what
		// both moved), and only one sharing none starts over.
		s.push_step(-1, (1, 9), vec![strip_row('d')]);
		assert_eq!(strip_tags(&s), "dc");
		assert_eq!(s.region, (1, 9));
		s.push_step(-1, (9, 12), vec![strip_row('e')]);
		assert_eq!(strip_tags(&s), "e");
		// the cap trims the rows farthest from the content (oldest)
		let mut long = OffStrip::new();
		for i in 0..(OffStrip::CAP + 3) {
			long.push_step(
				1,
				(0, 10),
				vec![strip_row(char::from(b'a' + (i % 26) as u8))],
			);
		}
		assert_eq!(long.len(), OffStrip::CAP);
		// the newest row (nearest the content, strip bottom) survives
		assert_eq!(
			long.rows.back().unwrap()[0].c,
			char::from(b'a' + ((OffStrip::CAP + 2) % 26) as u8)
		);
	}

	// (cell_w, cell_h, margin) for the link tests - round numbers so a pixel
	// coordinate reads as a cell without arithmetic.
	const LINK_METRICS: (f32, f32, f32) = (10.0, 20.0, 5.0);

	// Pointer at the center of grid cell (row, col), in window pixels.
	fn cell_px(row: i32, col: usize) -> (f32, f32) {
		let (cw, ch, margin) = LINK_METRICS;
		(
			margin + col as f32 * cw + cw / 2.0,
			margin + row as f32 * ch + ch / 2.0,
		)
	}

	fn link_probe(
		term: &Term<VoidListener>,
		cols: usize,
		lines: usize,
		row: i32,
		col: usize,
	) -> Option<LinkHit> {
		let settings = config::Settings::default();
		let rect = Rect {
			x: 0.0,
			y: 0.0,
			w: 400.0,
			h: 200.0,
		};
		let (px, py) = cell_px(row, col);
		link_at(
			term.grid(),
			term.colors(),
			&settings,
			rect,
			px,
			py,
			LINK_METRICS,
			(cols, lines),
			0,
		)
	}

	// Test ID: Em1S9yw
	#[test]
	fn a_url_in_the_grid_maps_back_to_the_cells_it_occupies() {
		let term = term_fed(40, 4, 100, "see http://example.com/x now");
		let hit = link_probe(&term, 40, 4, 0, 8).expect("link under the pointer");
		assert_eq!(hit.url, "http://example.com/x");
		assert_eq!(hit.start, Point::new(Line(0), Column(4)));
		assert_eq!(hit.end, Point::new(Line(0), Column(23)));
		// the words either side are not the link
		assert!(link_probe(&term, 40, 4, 0, 1).is_none(), "before");
		assert!(link_probe(&term, 40, 4, 0, 25).is_none(), "after");
		// nor is a blank row below it
		assert!(link_probe(&term, 40, 4, 1, 8).is_none(), "next row");
	}

	// A URL that runs past the right edge is ONE logical line, so the scan has to
	// span the wrap - otherwise hovering the tail half finds a fragment, or
	// nothing, depending on where the break fell.
	// Test ID: Em1S9yx
	#[test]
	fn a_wrapped_url_is_found_whole_from_either_half() {
		let cols = 20;
		let term = term_fed(cols, 4, 100, "https://example.com/ab");
		let head = link_probe(&term, cols, 4, 0, 3).expect("first row");
		let tail = link_probe(&term, cols, 4, 1, 1).expect("second row");
		assert_eq!(head.url, "https://example.com/ab");
		assert_eq!(head, tail, "both halves report the same link");
		assert_eq!(head.start, Point::new(Line(0), Column(0)));
		assert_eq!(head.end, Point::new(Line(1), Column(1)));
	}

	// point_at CLAMPS a stray pixel onto the nearest cell, which is right for
	// dragging a selection and wrong here: the margin is over no cell at all, and
	// clamping there would underline a link the pointer is not on.
	// Test ID: Em1S9yy
	#[test]
	fn the_margin_is_over_no_link() {
		let term = term_fed(40, 4, 100, "http://example.com/x");
		let settings = config::Settings::default();
		let rect = Rect {
			x: 0.0,
			y: 0.0,
			w: 400.0,
			h: 200.0,
		};
		let probe = |px, py| {
			link_at(
				term.grid(),
				term.colors(),
				&settings,
				rect,
				px,
				py,
				LINK_METRICS,
				(40, 4),
				0,
			)
		};
		assert!(probe(2.0, 12.0).is_none(), "left margin");
		assert!(probe(12.0, 2.0).is_none(), "top margin");
		assert!(
			probe(12.0, 12.0).is_some(),
			"and the cell itself still hits"
		);
	}

	// Test ID: EieUsc4
	#[test]
	fn bell_brighten_lightens_and_is_identity_at_zero() {
		let c = [100, 120, 140];
		assert_eq!(bell_brighten(c, 0.0), c); // no flash -> unchanged
		let b = bell_brighten(c, 1.0);
		assert!(b[0] > c[0] && b[1] > c[1] && b[2] > c[2]); // peak flash brightens
		assert!(b.iter().zip(&c).all(|(&n, &o)| n >= o)); // never darkens
	}

	// A paste is the one thing a person hands the shell without reading it byte by
	// byte, and the shell cannot tell a pasted byte from a typed one. Bracketing
	// is what draws the line, so the payload must not be able to step over it.
	mod fuzz {
		use super::super::{pair_inside, paste_payload, same_char_pair};
		use crate::fuzz;

		fn clipboard(rng: &mut fuzz::Rng) -> Vec<u8> {
			#[rustfmt::skip]
			const PIECES: [&str; 14] = [
				"\u{1b}", "\u{1b}[201~", "\u{1b}[200~", "\r\n", "\n", "\r",
				"rm -rf ~", "safe", " ", "\u{0}", "\u{7f}", "(", ")", "\"",
			];
			let mut out = String::new();
			for _ in 0..rng.below(20) {
				if rng.chance(4) {
					out.push_str(&fuzz::text(rng));
				} else {
					out.push_str(rng.pick(&PIECES));
				}
			}
			out.into_bytes()
		}

		// Test ID: EpQN0oM
		#[test]
		fn a_paste_can_never_step_outside_its_brackets() {
			let corpus = fuzz::corpus("paste");
			let check = |case: &[u8]| {
				let text = String::from_utf8_lossy(case);
				// Bracketed: an ESC would end the bracket early and everything
				// after it would be read as keystrokes.
				assert!(
					!paste_payload(&text, true).contains('\u{1b}'),
					"an ESC survived into a bracketed paste"
				);
				// Unbracketed: the application cannot tell the paste from typing,
				// so a line break has to be the one the Enter key sends.
				assert!(
					!paste_payload(&text, false).contains('\n'),
					"an LF survived into an unbracketed paste"
				);
			};
			for case in &corpus {
				check(case);
			}
			fuzz::soak("paste", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				check(&fuzz::input(&mut rng, &corpus, clipboard));
			});
		}

		// A double-click asks about pairs before it falls back to words, and the
		// span it comes back with indexes straight into the row.
		// Test ID: EpQN0oN
		#[test]
		fn a_selected_pair_stays_inside_its_row() {
			const PAIRS: [(char, char); 5] =
				[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];
			let corpus = fuzz::corpus("pairs");
			fuzz::soak("pairs", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let case = fuzz::input(&mut rng, &corpus, clipboard);
				let row: Vec<char> = String::from_utf8_lossy(&case).chars().collect();
				for col in 0..row.len() {
					if let Some((a, b)) = pair_inside(&row, col, &PAIRS) {
						assert!(
							a <= b && b < row.len(),
							"{a}..{b} outside a row of {}",
							row.len()
						);
					}
					for &(open, _) in &PAIRS {
						if let Some((a, b)) = same_char_pair(&row, col, open) {
							assert!(
								a <= b && b < row.len(),
								"{a}..{b} outside a row of {}",
								row.len()
							);
						}
					}
				}
			});
		}
	}

	// A drag-selection held past the top or bottom of the pane has to keep the
	// view moving, and a pointer inside must never move it.
	// Test ID: EqQXsMy
	#[test]
	fn a_drag_past_an_edge_scrolls_and_one_inside_does_not() {
		let (top, bottom, cell) = (100.0, 500.0, 20.0);
		let rate = |y: f32| edge_scroll_rate(y, top, bottom, cell, 0.0);
		for y in [100.0, 200.0, 499.0, 500.0] {
			assert_eq!(rate(y), 0.0, "pointer at {y} is inside the pane");
		}
		// above the top scrolls back into history, below the bottom scrolls down
		assert!(rate(99.0) > 0.0);
		assert!(rate(501.0) < 0.0);
		// right at the edge it creeps rather than standing still, so a careful
		// drag can pick up one more line
		assert!(rate(99.0) >= 1.0);
		// and it speeds up the further past the edge the pointer is held
		let (near, far) = (rate(90.0), rate(40.0));
		assert!(far > near, "{far} should beat {near}");
		// but not without limit: a pointer flung off the screen is capped
		let off = rate(-4000.0);
		assert_eq!(off, rate(top - 6.0 * cell));
		assert!(off < 100.0, "{off} lines a second is a jump, not a scroll");
		// symmetric: the same distance either way moves at the same speed
		assert_eq!(rate(top - 55.0), -rate(bottom + 55.0));
	}

	// A maximized window has its top edge against the top of the screen, so the
	// pointer can barely get past it. Holding there has to build speed anyway, or
	// that window could only ever creep.
	// Test ID: EqQXsMz
	#[test]
	fn holding_at_an_edge_with_no_room_still_builds_speed() {
		let (top, bottom, cell) = (0.0, 500.0, 20.0);
		let at = |held: f32| edge_scroll_rate(-1.0, top, bottom, cell, held);
		assert!(at(0.0) > 0.0, "it moves from the start");
		assert!(at(1.0) > at(0.0), "and picks up while held");
		// it tops out at what a full-distance push gives, and goes no further
		let full = edge_scroll_rate(top - 20.0 * cell, top, bottom, cell, 0.0);
		assert_eq!(at(10.0), full);
		assert_eq!(at(2.0), full);
		// coming back inside the pane stops it however long it was held
		assert_eq!(edge_scroll_rate(250.0, top, bottom, cell, 10.0), 0.0);
	}

	// Reverse video renders thinner than the same weight the right way round, so
	// nano's title and status bars are drawn bold - unless the setting says not.
	// Test ID: Er2VGX3
	#[test]
	fn inverse_video_is_drawn_bold_unless_turned_off() {
		let term = term_fed(8, 2, 100, "\x1b[7mX\x1b[0mY");
		let grid = term.grid();
		let cell = |settings: &config::Settings, col: usize| {
			strip_cell(
				&grid[Line(0)][Column(col)],
				term.colors(),
				settings,
				&mut crate::palette::Readable::default(),
			)
		};
		let on = config::Settings::default();
		assert!(on.embolden_inverse, "on by default");
		assert_eq!((cell(&on, 0).c, cell(&on, 0).bold), ('X', true));
		assert_eq!((cell(&on, 1).c, cell(&on, 1).bold), ('Y', false));
		let off = config::Settings {
			embolden_inverse: false,
			..Default::default()
		};
		assert!(!cell(&off, 0).bold);
	}

	// A block cursor is any shape the two size settings describe: width from the
	// left, height from the bottom. An app's beam or underline, and the block a
	// full-screen app owns, ignore them.
	// Test ID: Er2VGX4
	#[test]
	fn a_block_cursor_takes_its_size_from_the_settings() {
		let _g = config::test_store_lock();
		config::update(config::Settings {
			cursor_size_width: 40.0,
			cursor_size_height: 60.0,
			..Default::default()
		});
		assert_eq!(cursor_geometry(CursorShape::Block, false), (0.4, 0.6));
		assert_eq!(cursor_geometry(CursorShape::Block, true), (1.0, 1.0));
		assert_eq!(cursor_geometry(CursorShape::Beam, false), (0.15, 1.0));
		assert_eq!(cursor_geometry(CursorShape::Underline, false), (1.0, 0.15));
		// a zero never makes the cursor vanish
		config::update(config::Settings {
			cursor_size_width: 0.0,
			cursor_size_height: 0.0,
			..Default::default()
		});
		assert_eq!(cursor_geometry(CursorShape::Block, false), (0.02, 0.02));
		config::update(config::Settings::default());
	}

	// One pulse: grow from nothing, hold at full, shrink, then gone for a moment
	// before the next. Each pulse style moves only its own axes, from the center.
	// Test ID: Er2VGX5
	#[test]
	fn a_pulse_grows_holds_shrinks_then_disappears() {
		assert_eq!(pulse_env(0.0), 0.0);
		let rising: Vec<f32> = (0..40).map(|i| pulse_env(i as f32 / 100.0)).collect();
		assert!(
			rising.windows(2).all(|w| w[1] > w[0]),
			"grows the whole way"
		);
		for phase in [0.40, 0.45, 0.5, 0.55, 0.599] {
			assert_eq!(pulse_env(phase), 1.0, "held full at {phase}");
		}
		let falling: Vec<f32> = (60..90).map(|i| pulse_env(i as f32 / 100.0)).collect();
		assert!(
			falling.windows(2).all(|w| w[1] < w[0]),
			"shrinks the whole way"
		);
		for phase in [0.90, 0.95, 0.999] {
			assert_eq!(pulse_env(phase), 0.0, "gone at {phase}");
		}

		let geom = (0.8, 0.6);
		let env = pulse_env(0.2);
		let (w, h, alpha, pw, ph) = cursor_envelope("pulse_vertical", 0.2, geom);
		assert_eq!(
			(w, h, alpha, pw, ph),
			(0.8, 0.6 * env, CURSOR_ALPHA, false, true)
		);
		let (w, h, alpha, pw, ph) = cursor_envelope("pulse_horizontal", 0.2, geom);
		assert_eq!(
			(w, h, alpha, pw, ph),
			(0.8 * env, 0.6, CURSOR_ALPHA, true, false)
		);
		let (w, h, alpha, pw, ph) = cursor_envelope("pulse_both", 0.2, geom);
		assert_eq!(
			(w, h, alpha, pw, ph),
			(0.8 * env, 0.6 * env, CURSOR_ALPHA, true, true)
		);
	}

	// Dragging a divider sets its split's ratio from the pointer, and marks the
	// run as hand-sized. Neither side can be squeezed away.
	// Test ID: Er2VGX6
	#[test]
	fn dragging_a_divider_sets_its_ratio_from_the_pointer() {
		fn ratio(node: &Node) -> (f32, bool) {
			match node {
				Node::Split { ratio, manual, .. } => (*ratio, *manual),
				Node::Leaf(_) => panic!("not a split"),
			}
		}
		let area = Rect {
			x: 0.0,
			y: 0.0,
			w: 400.0,
			h: 200.0,
		};
		let usable = area.w - config::PANE_GAP_PX;
		let mut root = split(Dir::Vertical, 0.5, false, leaf(1), leaf(2));
		set_ratio(&mut root, area, &[], usable / 4.0, 50.0, 1.0);
		assert_eq!(ratio(&root), (0.25, true));
		set_ratio(&mut root, area, &[], 0.0, 50.0, 1.0);
		assert_eq!(ratio(&root).0, 0.05);
		set_ratio(&mut root, area, &[], area.w, 50.0, 1.0);
		assert_eq!(ratio(&root).0, 0.95);

		// a path reaches the nested split and leaves the outer one alone
		let mut root = split(
			Dir::Horizontal,
			0.5,
			false,
			leaf(1),
			split(Dir::Vertical, 0.5, false, leaf(2), leaf(3)),
		);
		set_ratio(&mut root, area, &[true], usable * 0.75, 150.0, 1.0);
		assert_eq!(ratio(&root), (0.5, false));
		let Node::Split { b, .. } = &root else {
			unreachable!()
		};
		assert_eq!(ratio(b), (0.75, true));
	}

	// A drag-and-drop swap trades the two panes' places and keeps every split's
	// size as it was.
	// Test ID: Er2VGX7
	#[test]
	fn swapping_two_panes_trades_their_places_and_nothing_else() {
		fn splits(node: &Node, out: &mut Vec<(f32, bool)>) {
			if let Node::Split {
				ratio,
				manual,
				a,
				b,
				..
			} = node
			{
				out.push((*ratio, *manual));
				splits(a, out);
				splits(b, out);
			}
		}
		let order = |node: &Node| {
			let mut out = Vec::new();
			let area = Rect {
				x: 0.0,
				y: 0.0,
				w: 400.0,
				h: 200.0,
			};
			layout(node, area, 1.0, &mut out);
			out.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
		};
		let mut root = split(
			Dir::Vertical,
			0.3,
			true,
			leaf(1),
			split(Dir::Horizontal, 0.6, false, leaf(2), leaf(3)),
		);
		let mut before = Vec::new();
		splits(&root, &mut before);
		swap_leaves(&mut root, 1, 3);
		assert_eq!(order(&root), [3, 2, 1]);
		let mut after = Vec::new();
		splits(&root, &mut after);
		assert_eq!(after, before);
	}

	// Arming copy-output at Enter used to try the term lock and give up when the
	// reader held it, so a command that raced a burst was never copied. Waiting
	// is bounded by one read cycle. The race needs real PTY timing, so the lock
	// call itself is what is pinned.
	// Test ID: Er2VGX8
	#[test]
	fn arming_a_copy_waits_for_the_term_instead_of_giving_up() {
		let body = include_str!("pane.rs")
			.split("pub fn arm_capture(&mut self) {")
			.nth(1)
			.and_then(|rest| rest.split("\n\t}\n").next())
			.expect("the body of arm_capture");
		assert!(body.contains(".lock_unfair()"), "it waits for the lock");
		assert!(!body.contains("try_lock"), "a try can lose the race");
	}

	// Each tab is its own PaneManager, and a shell-exit event carries only the
	// pane's id, so a per-tab count closed the wrong tab and cascaded. Every pane
	// takes its id from the one counter for the whole program.
	// Test ID: Er2VGX9
	#[test]
	fn pane_ids_come_from_one_counter_for_the_whole_program() {
		let body = include_str!("pane.rs")
			.split("\nmod tests {")
			.next()
			.expect("the file above its own tests");
		assert!(body.contains("static PANE_ID_SEQ: AtomicU64"));
		let spawns = body.matches("spawn_pane(ctx, proxy, ").count();
		assert!(spawns >= 2, "a new tab and a split both spawn");
		assert_eq!(
			body.matches(" = alloc_pane_id();").count(),
			spawns,
			"every pane spawned takes a program-wide id"
		);
		// and the counter hands out no id twice, from however many threads
		let ids: Vec<u64> = std::thread::scope(|scope| {
			let workers: Vec<_> = (0..4)
				.map(|_| scope.spawn(|| (0..500).map(|_| alloc_pane_id()).collect::<Vec<_>>()))
				.collect();
			workers
				.into_iter()
				.flat_map(|w| w.join().expect("worker"))
				.collect()
		});
		let unique: std::collections::HashSet<_> = ids.iter().collect();
		assert_eq!(unique.len(), ids.len());
	}

	// The phase blink fades on a cosine: full at the start of the cycle, gone at
	// its middle, and partway between them. An on/off blink has no partway.
	// Test ID: Er2VGXA
	#[test]
	fn the_phase_blink_fades_instead_of_switching() {
		let alpha = |phase: f32| cursor_envelope("phase", phase, (1.0, 1.0)).2;
		assert_eq!(alpha(0.0), CURSOR_ALPHA);
		assert!(alpha(0.5).abs() < 1e-6, "gone at mid-cycle: {}", alpha(0.5));
		for phase in [0.25, 0.75] {
			let a = alpha(phase);
			assert!(
				a > 0.1 * CURSOR_ALPHA && a < 0.9 * CURSOR_ALPHA,
				"partway at {phase}: {a}"
			);
		}
		assert!(alpha(0.1) > alpha(0.2) && alpha(0.2) > alpha(0.3));
	}

	// "none" leaves the cursor exactly as configured at every point of the cycle,
	// and "phase" only ever touches its alpha, never its size.
	// Test ID: Er2VGXB
	#[test]
	fn no_animation_leaves_the_cursor_alone_and_phase_only_fades_it() {
		let geom = (0.4, 0.6);
		for phase in [0.0, 0.2, 0.5, 0.95] {
			assert_eq!(
				cursor_envelope("none", phase, geom),
				(0.4, 0.6, CURSOR_ALPHA, false, false)
			);
			let (w, h, _, pw, ph) = cursor_envelope("phase", phase, geom);
			assert_eq!((w, h, pw, ph), (0.4, 0.6, false, false));
		}
		assert!(cursor_envelope("phase", 0.5, geom).2 < CURSOR_ALPHA);
	}

	// Leaving the alt screen hands the normal screen's scrollback back in one
	// frame. That depth is not new output, so the frame cuts rather than easing
	// the old screen in from below. Ordinary output after it still eases.
	// Test ID: Er2VGXC
	#[test]
	fn an_alt_screen_exit_hands_back_its_depth_without_easing() {
		let _g = config::test_store_lock();
		config::update(config::Settings::default());
		let mut s = crate::scroll::Scroll::new();
		s.set_max(0.0); // the alt screen has no scrollback
		s.set_max(500.0);
		let (follow, advanced, _) = ease_output(&mut s, false, true, 500, 0, 24);
		assert!(follow);
		assert_eq!(advanced, 500);
		assert!(!s.animating(), "the exit is a cut");
		assert_eq!((s.desired_offset(), s.frac()), (0, 0.0));

		s.set_max(503.0);
		let (_, advanced, was_chasing) = ease_output(&mut s, false, false, 3, 0, 24);
		assert_eq!(advanced, 3);
		assert!(!was_chasing);
		assert!(s.chasing_output(), "three new lines ease in");
	}

	// A pane whose tab was hidden took 2000 lines while nothing was built. The
	// switch back hard-cuts (`hard_cut` snaps, and the next build is a cut frame),
	// so it arrives at the bottom with no motion, and whatever ease was in flight
	// when it was hidden is gone too.
	// Test ID: Er2VGXD
	#[test]
	fn a_frozen_pane_catches_up_without_motion() {
		let _g = config::test_store_lock();
		config::update(config::Settings::default());
		let mut s = crate::scroll::Scroll::new();
		s.set_max(100.0);
		s.nudge_output(12.0, 24.0);
		assert!(s.animating());
		s.snap();
		s.set_max(2100.0);
		let (_, advanced, _) = ease_output(&mut s, false, true, 2000, 0, 24);
		assert_eq!(advanced, 2000);
		assert!(!s.animating());
		assert_eq!((s.desired_offset(), s.frac()), (0, 0.0));
	}

	// The PTY reader holds the term across a whole read cycle and takes it again
	// the moment it lets go, so a frame that only ever tries can lose for
	// seconds. After a couple of misses the frame queues on the fair lock and
	// gets in at the end of the cycle.
	// Test ID: Er2VGXE
	#[test]
	fn a_frame_waits_its_turn_once_trying_keeps_losing() {
		use alacritty_terminal::sync::FairMutex;
		use std::sync::atomic::{AtomicBool, Ordering};
		let term = FairMutex::new(0u32);
		let (started, stop) = (AtomicBool::new(false), AtomicBool::new(false));
		// uncontended, the first try takes it
		let mut misses = 0;
		assert!(lock_for_frame(&term, &mut misses).is_some());
		assert_eq!(misses, 0);
		std::thread::scope(|scope| {
			scope.spawn(|| {
				while !stop.load(Ordering::Relaxed) {
					let _lease = term.lease();
					let mut data = term.lock_unfair();
					started.store(true, Ordering::Relaxed);
					*data += 1;
					std::thread::sleep(std::time::Duration::from_millis(2));
				}
			});
			while !started.load(Ordering::Relaxed) {
				std::thread::yield_now();
			}
			let got = (0..=super::LOCK_WAIT_AFTER)
				.find_map(|_| lock_for_frame(&term, &mut misses).map(|data| *data));
			stop.store(true, Ordering::Relaxed);
			assert!(got.is_some(), "a frame got the term within the wait");
			assert_eq!(misses, 0, "and the misses start over");
		});
	}

	// Only a pause for input schedules its own wake. A long-idle stop and a pane
	// that is not the active one draw no frames and wait for activity or focus.
	// Test ID: Er2VGXF
	#[test]
	fn only_an_input_pause_schedules_its_own_wake() {
		assert_eq!(cursor_wake_after(false, false, false, 0.5, 0.1, 0.1), None);
		assert_eq!(cursor_wake_after(true, true, false, 0.5, 0.1, 0.1), None);
		assert_eq!(cursor_wake_after(true, false, true, 0.5, 0.1, 0.1), None);
		// the later of the two resume points, plus a little margin
		let wait = cursor_wake_after(true, false, false, 0.5, 0.2, 0.1).expect("a wake");
		assert!((wait - 0.42).abs() < 1e-6, "{wait}");
		let now = cursor_wake_after(true, false, false, 0.5, 3.0, 3.0).expect("a wake");
		assert!(
			(now - 0.02).abs() < 1e-6,
			"an overdue resume wakes at once: {now}"
		);
	}

	// One scrollbar, at the far right of the pane's whole area: past the minimap
	// column when there is one, which used to carry a bar of its own.
	// Test ID: Er2VGXG
	#[test]
	fn the_scrollbar_sits_at_the_far_edge_past_the_minimap() {
		let full = Rect {
			x: 10.0,
			y: 20.0,
			w: 800.0,
			h: 600.0,
		};
		let (thickness, margin) = (16.0, 4.0);
		let map = config::Settings {
			minimap: true,
			minimap_width: 100.0,
			..Default::default()
		};
		let text = minimap::text_rect(full, &map, 1.0, true);
		assert!(text.w < full.w, "the column takes its room");
		let track = bar_track(full, text, thickness, margin);
		assert_eq!(track.x + track.w, full.x + full.w);
		assert!(
			track.x >= text.x + text.w,
			"over the column, clear of the text"
		);
		assert_eq!((track.y, track.h), (text.y + margin, text.h - 2.0 * margin));
		// no column: the same edge, now over the text
		let bare = config::Settings {
			minimap: false,
			..map
		};
		let text = minimap::text_rect(full, &bare, 1.0, true);
		let track = bar_track(full, text, thickness, margin);
		assert_eq!(track.x + track.w, text.x + text.w);
	}

	// Bold ink is wider, so its glow reads heavier than its neighbors'. The glow
	// is shaped from the same spans with the weight taken out and everything
	// else kept, while the text drawn on top keeps its bold.
	// Test ID: Er2VGXH
	#[test]
	fn the_glow_source_loses_bold_and_nothing_else() {
		let base = mono_attrs();
		let mut bold = base.clone();
		bold.weight = Weight::BOLD;
		let bold_italic = bold
			.clone()
			.style(Style::Italic)
			.color(GColor::rgb(200, 10, 10));
		let mut crisp = AttrsList::new(&base);
		crisp.add_span(0..4, &bold);
		crisp.add_span(4..8, &bold_italic);
		let glow = debold_attrs(&crisp, base.weight);
		for i in [0, 3, 4, 7] {
			assert_eq!(glow.get_span(i).weight, base.weight, "span at {i}");
			assert_eq!(crisp.get_span(i).weight, Weight::BOLD, "crisp at {i}");
		}
		let styled = glow.get_span(5);
		assert_eq!(styled.style, Style::Italic);
		assert_eq!(styled.color_opt, Some(GColor::rgb(200, 10, 10)));
		assert_eq!(glow.get_span(9).weight, base.weight);
	}

	// A pixel becomes a grid point and the half of the cell it fell in, which is
	// where a selection starts and ends. Outside the pane it is pulled to the
	// nearest edge cell, and a view scrolled back points into history.
	// Test ID: Er2VGXI
	#[test]
	fn a_pixel_maps_to_a_cell_half_and_clamps_at_the_edges() {
		let rect = Rect {
			x: 10.0,
			y: 20.0,
			w: 820.0,
			h: 500.0,
		};
		let at = |x: f32, y: f32, offset: i32| {
			let (p, side) = grid_point(x, y, rect, 4.0, (10.0, 20.0), (80, 24), offset);
			(p.line.0, p.column.0, side)
		};
		// column 5, row 2: left then right half
		let (x0, y0) = (10.0 + 4.0 + 5.0 * 10.0, 20.0 + 4.0 + 2.0 * 20.0 + 5.0);
		assert_eq!(at(x0 + 2.0, y0, 0), (2, 5, Side::Left));
		assert_eq!(at(x0 + 7.0, y0, 0), (2, 5, Side::Right));
		// past each edge: the edge cell, the far half of the last column whole
		assert_eq!(at(0.0, 0.0, 0), (0, 0, Side::Left));
		assert_eq!(at(5000.0, 5000.0, 0), (23, 79, Side::Right));
		// scrolled back 100 lines, row 2 is a history line
		assert_eq!(at(x0 + 2.0, y0, 100), (-98, 5, Side::Left));
	}

	// The pane's buffer holds one row more than the screen, above it. When the
	// content height is an exact multiple of the cell height that last row sits
	// right at the buffer's edge, and at the old exact height the layout dropped
	// it: the bottom line went invisible while the cursor still moved.
	// Test ID: Er2VGXJ
	#[test]
	fn the_bottom_row_survives_an_exact_multiple_height() {
		let mut ctx = crate::text::TextCtx::new_cpu(1.0);
		if !matches!(mono_attrs().family, glyphon::Family::Name(_)) {
			eprintln!("no concrete mono family on this box; skipping");
			return;
		}
		let lines = 24;
		let ch = lines as f32 * ctx.cell_h;
		let mut buf = ctx.new_buffer(400.0, text_buffer_h(ch, ctx.cell_h));
		let rows: Vec<String> = (0..=lines).map(|i| format!("row {i}")).collect();
		set_buffer_rows(
			&mut buf,
			rows.iter()
				.map(|row| (row.as_str(), AttrsList::new(&mono_attrs()))),
		);
		buf.shape_until_scroll(&mut ctx.font_system, false);
		assert_eq!(buf.layout_runs().count(), lines + 1);
	}
}
