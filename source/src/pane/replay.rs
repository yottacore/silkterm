// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The scroll half of `Pane::build`, frame by frame over fed bytes, with no
//! window: the output ease, the engine's scroll record and the row-print
//! detector, each through the helpers build calls, in build's order. A change
//! to that order in build wants the same change here.
//!
//! The tests pin the prompt-slide class (2026101415085143 and the items it
//! names) on bytes ble.sh and bash wrote, with the prompt text made up.
//! `replay_a_recorded_session` runs a `script --log-out/--log-timing`
//! recording through it, one frame per read.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;

use super::{
	APP_SCROLL_MAX, DepthSampler, Output, SLIDE_DOWN_INTO_ROOM, StripCell, WIPE_MAX_SLIDE,
	blank_went_up, ease_output, fingerprint_frame, fnv_row, ledger_makes_room, ledger_step,
	region_wiped, repaint_shift, row_print, scroll_shift_signed, shift_makes_room, skel_rows,
	slide_is_visible, snapshot_rows, strip_rows,
};
use crate::scroll::Scroll;
use crate::{config, palette};

struct Quiet;
impl EventListener for Quiet {
	fn send_event(&self, _: Event) {}
}

// What one frame decided.
#[derive(Debug, Clone, Copy, Default)]
struct Frame {
	// the signed step a region slide took this frame, 0 for none
	slid: i32,
	// lines the output ease took on
	advanced: usize,
	alt: bool,
}

struct Replay {
	term: Term<Quiet>,
	parser: Processor,
	scroll: Scroll,
	depth: DepthSampler,
	last_rows: Vec<u64>,
	last_cells: Vec<Vec<StripCell>>,
	last_skel: Vec<u64>,
	last_alt: bool,
	cols: usize,
	lines: usize,
	slides: Vec<(usize, i32)>,
	frames: usize,
}

impl Replay {
	fn new(cols: usize, lines: usize, scrollback: usize) -> Self {
		let dims = crate::term::TermDimensions {
			columns: cols,
			screen_lines: lines,
		};
		let cfg = TermConfig {
			scrolling_history: scrollback,
			..Default::default()
		};
		let mut term = Term::new(cfg, &dims, Quiet);
		term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		Self {
			term,
			parser: Processor::new(),
			scroll: Scroll::new(),
			depth: DepthSampler::default(),
			last_rows: Vec::new(),
			last_cells: Vec::new(),
			last_skel: Vec::new(),
			last_alt: false,
			cols,
			lines,
			slides: Vec::new(),
			frames: 0,
		}
	}

	// Bytes in with no frame drawn, as when a read lands mid-build.
	fn feed(&mut self, bytes: &[u8]) {
		self.parser.advance(&mut self.term, bytes);
	}

	// Bytes in, then one frame. Every frame here has new content, so each one
	// takes a snapshot, the way a forced build does.
	fn frame(&mut self, bytes: &str) -> Frame {
		self.feed(bytes.as_bytes());
		self.build()
	}

	fn build(&mut self) -> Frame {
		let settings = config::settings();
		let (cols, lines) = (self.cols, self.lines);
		let alt = self.term.mode().contains(TermMode::ALT_SCREEN);
		let cut = alt != self.last_alt;
		self.last_alt = alt;
		let gesture_active = self.scroll.app_offset() != 0.0;
		let history = self.term.grid().history_size();
		let grew = self.depth.take(history);
		let cleared = self.depth.take_cleared();
		self.scroll.set_max(history as f32);
		let (region, scrolled, pushed, since_clear) = {
			let ledger = self.term.scroll_ledger();
			let clamp = |line: Line| (line.0.max(0) as usize).min(lines);
			let r = ledger.region();
			(
				clamp(r.start)..clamp(r.end),
				ledger.lines(),
				ledger.pushed(),
				ledger.since_clear(),
			)
		};
		if cut {
			self.scroll.cancel_app_scroll();
		}
		let blank = fnv_row(std::iter::repeat_n(' ', cols));
		let output = Output {
			grew,
			pushed,
			since_clear,
			was_blank: self.last_rows.iter().all(|&row| row == blank),
		};
		let term = &self.term;
		let (last_rows, last_skel) = (&self.last_rows, &self.last_skel);
		let (follow, advanced, _) = ease_output(
			&mut self.scroll,
			alt,
			cut,
			output,
			lines,
			|k| {
				repaint_shift(term.grid(), last_rows, last_skel, k, cols, blank)
					.filter(|_| !cleared)
			},
			|k| blank_went_up(term.grid(), k, cols, blank),
		);
		let whole = region == (0..lines);
		let step = ledger_step(alt, follow, advanced, whole, scrolled);
		let kept = step.map(|_| {
			strip_rows(
				self.term.scroll_ledger().rows(),
				cols,
				self.term.colors(),
				&settings,
				&mut palette::Readable::default(),
			)
		});
		let wiped = self.scroll.app_offset().abs() < WIPE_MAX_SLIDE
			&& step.is_some_and(|step| {
				region_wiped(step, &region, |i| {
					row_print(&self.term.grid()[Line(i as i32)], cols) != blank
				})
			});
		if wiped {
			self.scroll.cancel_app_scroll();
		}
		if !cut && !wiped && (gesture_active || step.is_some()) {
			self.term.scroll_ledger_mut().drain();
		} else {
			self.term.scroll_ledger_mut().clear();
		}
		let step = step.filter(|&step| {
			let kept = kept.as_deref().unwrap_or_default();
			let room = !SLIDE_DOWN_INTO_ROOM
				&& ledger_makes_room(step, &region, lines, &self.last_cells, kept);
			let arrived = |from: usize, to: usize| {
				self.last_rows.len() == lines
					&& row_print(&self.term.grid()[Line(to as i32)], cols) == self.last_rows[from]
			};
			let unknown = !alt && self.last_cells.len() != lines;
			!unknown
				&& !wiped && slide_is_visible(step, &region, &self.last_cells, kept, arrived)
				&& !room
		});
		let mut cells = Vec::new();
		let rows = snapshot_rows(
			self.term.grid(),
			lines,
			cols,
			Some((
				self.term.colors(),
				&settings,
				&mut cells,
				&mut palette::Readable::default(),
			)),
		);
		let mut slid = 0;
		if !cut {
			if let Some(step) = step {
				slid = step;
				self.scroll.app_scroll(
					step as f32,
					crate::scroll::SLIDE_ROWS as f32,
					region.len() as f32,
				);
			} else if fingerprint_frame(alt, follow, grew, scrolled) {
				let shift = scroll_shift_signed(&rows, &self.last_rows, APP_SCROLL_MAX);
				if shift != 0
					&& (SLIDE_DOWN_INTO_ROOM
						|| !shift_makes_room(&rows, &self.last_rows, shift, blank))
				{
					slid = shift;
					self.scroll
						.app_scroll(shift as f32, super::OffStrip::CAP as f32, lines as f32);
				}
			}
		}
		if slid != 0 && !alt {
			self.slides.push((self.frames, slid));
		}
		self.last_rows = rows;
		self.last_cells = cells;
		skel_rows(self.term.grid(), lines, cols, &mut self.last_skel);
		self.scroll.advance(1.0 / 60.0);
		self.frames += 1;
		Frame {
			slid,
			advanced,
			alt,
		}
	}

	// What `PaneManager::relayout` does to a pane: a new size, the depth
	// re-baselined, and no snapshot until the next build takes one.
	fn relayout(&mut self, cols: usize) {
		let dims = crate::term::TermDimensions {
			columns: cols,
			screen_lines: self.lines,
		};
		self.term.resize(dims);
		self.cols = cols;
		self.depth.rebaseline(self.term.grid().history_size());
		self.last_cells.clear();
		self.last_skel.clear();
	}

	// Frames until both eases are at rest.
	fn settle(&mut self) {
		for _ in 0..240 {
			if !self.scroll.animating() && self.scroll.app_offset() == 0.0 {
				return;
			}
			self.scroll.advance(1.0 / 60.0);
		}
	}

	fn row(&self, line: i32) -> String {
		let grid = self.term.grid();
		(0..grid.columns())
			.map(|c| grid[Line(line)][Column(c)].c)
			.collect::<String>()
			.trim_end()
			.to_string()
	}

	fn cursor_line(&self) -> usize {
		self.term.grid().cursor.point.line.0.max(0) as usize
	}
}

// The x9ps1-git prompt in a git repo, as ble.sh draws it. Two rows.
const TOP: &str = "\x1b[0;36m12:00:00\x1b(B\x1b[m user@host:~/repo [ main ]";
const BOTTOM: &str = "-> $ ";

// Enter: ble.sh rewrites the command line from the prompt's first row, then
// moves off it.
fn accept(cmd: &str) -> String {
	format!("\x1b[1A\r{TOP}\r\r\r\n{BOTTOM}{cmd}\r\n\x1b[?2004l")
}

// What ble.sh writes once a command is done: the end-of-output mark, put up
// and wiped, then room for the prompt made with insert- and delete-line on the
// rows under the cursor, then the prompt.
fn prompt_after(cols: usize) -> String {
	format!(
		"\x1b7[ble: EOF]\x1b8\x1b[{}C  \r\x1b[K\x1b[?2004h\r\n\x1b[1L\x1b[2K\x1b[1A\x1b[2K\x1b[2M\x1b[2L\x1b[2K{TOP}\r\r\r\n{BOTTOM}",
		cols - 2
	)
}

// ble.sh's redraw of a prompt already on screen, after a resize or a program
// that drew its own screen: up to its first row, erase down, then the same
// insert- and delete-line dance.
const REDRAW: &str = "\x1b[1A\r\r\x1b[J\r\n\x1b[1A\x1b[2L\x1b[2K\x1b[2K\x1b[2M\x1b[2L\x1b[2K";

fn redraw() -> String {
	format!("{REDRAW}{TOP}\r\r\r\n{BOTTOM}")
}

fn lines_of(n: usize, tag: &str) -> String {
	use std::fmt::Write as _;
	(1..=n).fold(String::new(), |mut out, i| {
		let _ = write!(out, "{tag} {i}\r\n");
		out
	})
}

// One command the way the two reads usually fall: Enter and the output in one
// frame, ble.sh's prompt in the next.
fn run(r: &mut Replay, cmd: &str, out: &str) -> (Frame, Frame) {
	let first = r.frame(&format!("{}{out}", accept(cmd)));
	let cols = r.cols;
	let second = r.frame(&prompt_after(cols));
	(first, second)
}

// Test ID: Esib1t0
#[test]
fn a_prompt_coming_back_under_blesh_pops_in() {
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (60usize, 30usize);
	for scrollback in [10_000usize, 30] {
		let mut r = Replay::new(cols, lines, scrollback);
		r.frame(&format!("{TOP}\r\r\r\n{BOTTOM}"));
		// a half-empty screen: the prompt is drawn in place
		for (cmd, n) in [("echo hi", 1), ("seq 3", 3), ("ls", 5), ("echo hi", 1)] {
			let (_, prompt) = run(&mut r, cmd, &lines_of(n, "out"));
			assert_eq!(prompt.advanced, 0, "{cmd}: nothing went up yet");
			r.settle();
		}
		assert!(r.cursor_line() < lines - 2, "still half empty");
		assert!(r.slides.is_empty(), "sb {scrollback}: slid {:?}", r.slides);
		// a full screen: the prompt comes up from the bottom with the output,
		// and never slides out of the rows above it
		run(&mut r, "seq 40", &lines_of(40, "row"));
		r.settle();
		for (cmd, n) in [("echo hi", 1), ("seq 3", 3), ("printf x", 0), ("ls", 5)] {
			let out = if n == 0 {
				"x".to_string()
			} else {
				lines_of(n, "out")
			};
			let (_, prompt) = run(&mut r, cmd, &out);
			assert!(
				prompt.advanced > 0 && r.scroll.chasing_output(),
				"sb {scrollback}, {cmd}: the prompt eases up from the bottom"
			);
			r.settle();
		}
		assert_eq!(r.row(lines as i32 - 1), BOTTOM.trim_end());
		assert!(r.slides.is_empty(), "sb {scrollback}: slid {:?}", r.slides);
	}
}

// Test ID: Esib1y3
#[test]
fn blesh_redrawing_its_prompt_pops_in() {
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (60usize, 20usize);
	// half empty, the prompt two rows and one row above the bottom, and full,
	// each redrawn in place and after a program that drew on the alt screen.
	// One row above the bottom is the case 2026101415085143 found: the old
	// prompt is erased before the rows move, and the row under it is not room.
	for filled in [3usize, lines - 4, lines - 3, lines + 10] {
		let mut r = Replay::new(cols, lines, 10_000);
		r.frame(&format!("{}{TOP}\r\r\r\n{BOTTOM}", lines_of(filled, "row")));
		r.settle();
		r.frame(&redraw());
		r.settle();
		r.frame(&format!(
			"{}\x1b[?1049h\x1b[H\x1b[2J  GNU nano",
			accept("nano")
		));
		r.frame("\x1b[?1049l\r");
		r.frame(&prompt_after(cols));
		r.settle();
		r.frame(&redraw());
		r.settle();
		assert!(r.slides.is_empty(), "{filled} rows: slid {:?}", r.slides);
		// the command line grew a row and shrank back, with insert- and
		// delete-line under it
		r.frame("\r\r\n\x1b[1L\x1b[2K\x1b[1A\x1b[5Ca long command that wraps onto the next row of the screen ok\x1b[K");
		r.frame("\x1b[1B\r\x1b[2K\x1b[1M\x1b[1A\x1b[5Cls\x1b[K");
		r.settle();
		assert!(
			r.slides.is_empty(),
			"{filled} rows, editing: slid {:?}",
			r.slides
		);
	}
}

// Test ID: Esib22C
#[test]
fn a_listing_repeated_under_a_plain_prompt_pops_in() {
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (40usize, 24usize);
	let mut r = Replay::new(cols, lines, 10_000);
	let listing = lines_of(4, "file");
	r.frame("$ ");
	for _ in 0..3 {
		r.frame("ls\r\n");
		r.frame(&listing);
		r.frame("$ ");
		r.settle();
	}
	assert!(r.slides.is_empty(), "slid {:?}", r.slides);
}

// Test ID: Esib26L
#[test]
fn a_table_redraw_snaps_and_a_prompt_still_eases() {
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (40usize, 10usize);
	for scrollback in [1000usize, 50] {
		let mut r = Replay::new(cols, lines, scrollback);
		r.frame(&lines_of(60, "fill"));
		r.settle();
		// flatpak's table taller than the screen, redrawn: a repaint
		let tick = |t: usize| {
			let row = |i: usize| {
				let mark = if i < t { 'x' } else { ' ' };
				format!("{:>2}. [{mark}] org.silk.App{i:02}  stable  u", i + 1)
			};
			let mut s = String::from("\x1b[1;1H\x1b[0J");
			s.extend((0..23).map(row));
			for i in 23..30 {
				s.push_str("\r\n");
				s.push_str(&row(i));
			}
			s + &format!("\r\n\r\nUpdating {}/30", t + 1)
		};
		r.frame(&tick(0));
		r.settle();
		let f = r.frame(&tick(1));
		assert!(f.advanced >= lines, "sb {scrollback}: {}", f.advanced);
		assert!(!r.scroll.animating(), "sb {scrollback}: the redraw eased");
		// and the shell's prompt after it still eases
		r.frame("\r\n");
		r.settle();
		let f = r.frame("done\r\n$ ");
		assert!(
			f.advanced > 0 && r.scroll.chasing_output(),
			"sb {scrollback}"
		);
		assert!(r.slides.is_empty(), "sb {scrollback}: slid {:?}", r.slides);
	}
}

// Test ID: Esib2AU
#[test]
fn a_burst_still_easing_is_at_rest_on_the_alt_screen() {
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (40usize, 20usize);
	let mut r = Replay::new(cols, lines, 10_000);
	r.frame(&lines_of(400, "burst"));
	r.frame(&lines_of(50, "more"));
	assert!(
		r.scroll.animating(),
		"nothing was easing, so nothing was tested"
	);
	// git commit opening nano mid-ease
	let f = r.frame("\x1b[?1049h\x1b[H\x1b[2J  GNU nano");
	assert!(f.alt);
	for _ in 0..5 {
		assert_eq!(
			(r.scroll.desired_offset(), r.scroll.frac()),
			(0, 0.0),
			"the view moved on the alt screen"
		);
		r.frame("\x1b[2;1Htyping");
	}
}

// Test ID: EsiiYQ2
#[test]
fn a_prompt_drawn_across_two_reads_pops_in() {
	// A read can end anywhere in what ble.sh writes, and a frame can fall in
	// between, so every split of the prompt and of its redraw is tried, with the
	// prompt at each row from half-empty to full.
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (60usize, 12usize);
	for (what, bytes) in [("prompt", prompt_after(cols)), ("redraw", redraw())] {
		for filled in 2..lines + 3 {
			for cut in 1..bytes.len() {
				let mut r = Replay::new(cols, lines, 1000);
				r.frame(&format!("{}{TOP}\r\r\r\n{BOTTOM}", lines_of(filled, "row")));
				r.settle();
				if what == "prompt" {
					r.frame(&format!("{}out\r\n", accept("ls")));
				}
				r.feed(&bytes.as_bytes()[..cut]);
				r.build();
				r.feed(&bytes.as_bytes()[cut..]);
				r.build();
				assert!(
					r.slides.is_empty(),
					"{what}, {filled} rows, split at {cut} {:?}: slid {:?}",
					String::from_utf8_lossy(&bytes.as_bytes()[..cut]),
					r.slides
				);
			}
		}
	}
}

// Test ID: Esiikpm
#[test]
fn a_prompt_redrawn_right_after_a_resize_pops_in() {
	// The minimap column coming back as a program exits is a relayout, which
	// drops the snapshot a slide is judged against. ble.sh redraws its prompt
	// for the new width; if the frame after the relayout lost the term lock to
	// the reader, the redraw arrives with nothing to compare.
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let lines = 12usize;
	for filled in 2..lines + 3 {
		let mut r = Replay::new(70, lines, 1000);
		r.frame(&format!("{}{TOP}\r\r\r\n{BOTTOM}", lines_of(filled, "row")));
		r.settle();
		r.relayout(60);
		r.frame(&redraw());
		r.settle();
		assert!(r.slides.is_empty(), "{filled} rows: slid {:?}", r.slides);
	}
}

// tmux on the outer terminal, as tmux 3.6b writes it with its status line on:
// the alt screen, the status line on the last row, and the pane above it as
// the scroll region. `pane` is the pane's height, a row less than the screen.
fn tmux_open(lines: usize) -> String {
	let pane = lines - 1;
	format!(
		"\x1b[?1049h\x1b[H\x1b[2J\x1b[1;{lines}r\x1b[{lines};1H\x1b[30m\x1b[42m[0] 0:bash*\x1b(B\x1b[m\x1b[1;{pane}r\x1b[1;{lines}r\x1b[1;1H"
	)
}

// A burst the way tmux passes it on: the pane scrolled away whole, then drawn.
fn tmux_burst(lines: usize, from: usize) -> String {
	let pane = lines - 1;
	let rows = (from..from + pane)
		.map(|i| format!("{i}\x1b[K"))
		.collect::<Vec<_>>()
		.join("\r\n");
	format!("\x1b[1;{pane}r\x1b[1;1H\x1b[{pane}S{rows}\x1b[1;{lines}r\x1b[{pane};1H")
}

// Enter on the pane's last row: tmux scrolls the pane a line and writes the
// command line again on the row above.
fn tmux_enter(lines: usize, cmd: &str) -> String {
	let pane = lines - 1;
	format!(
		"\x1b[1;{pane}r\x1b[{pane};1H\n\x1b[{};3H{cmd}\r\n\x1b[K\x1b[1;{lines}r\x1b[{pane};1H",
		pane - 1
	)
}

// What tmux sends for a clear inside the pane, by how the shell clears and
// what it draws next. tmux never passes the clear on. It scrolls away the
// rows under the prompt's first line, which the status line keeps it from
// erasing to the bottom, and erases that line.
fn tmux_clear(lines: usize, how: &str) -> String {
	let pane = lines - 1;
	let wipe = format!(
		"\x1b[1;{pane}r\x1b[1;1H\x1b[2;{pane}r\x1b[{}S\x1b[1;1H\x1b[K",
		pane - 1
	);
	let ble = format!("\x1b[1;1H\x1b[2M\x1b[1;1H\x1b[2L\x1b[1;1H{TOP}\r\n{BOTTOM}");
	match how {
		"clear" => format!("{wipe}\x1b[1;{lines}r\x1b[1;1H$ "),
		"ctrl-l" => format!("{wipe}$ \x1b[1;{lines}r\x1b[1;3H"),
		"ble clear" => format!(
			"{wipe}\x1b[1;{lines}r\x1b[1;1H\x1b[K\x1b[1;{pane}r\x1b[2;1H\x1b[1L{ble}\x1b[1;{lines}r\x1b[2;6H"
		),
		"ble ctrl-l" => format!("{wipe}\x1b[1;{pane}r{ble}\x1b[1;{lines}r\x1b[2;6H"),
		// a reset makes tmux scroll the whole pane away and redraw every row
		"reset" => format!(
			"\x1b[1;{pane}r\x1b[{pane}S\x1b[1;1H\x1b[1;{lines}r\x1b[1;1H$ {}\x1b[K\x1b[1;3H",
			"\x1b[K\r\n".repeat(pane - 1)
		),
		_ => unreachable!("{how}"),
	}
}

// Test ID: EstO7ww
#[test]
fn a_clear_inside_tmux_shows_at_once() {
	// 2026101611321631. tmux keeps the outer terminal on the alt screen and
	// redraws its pane itself, so the clear arrives as a region scrolled away
	// whole. The shell's next draw can fall in the same frame or the next. The
	// Enter that ran the clear is usually still sliding when it comes.
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (100usize, 30usize);
	for how in ["clear", "ctrl-l", "ble clear", "ble ctrl-l", "reset"] {
		let bytes = tmux_clear(lines, how);
		for rested in [false, true] {
			for cut in std::iter::once(bytes.len()).chain(1..bytes.len()) {
				let mut r = Replay::new(cols, lines, 10_000);
				r.frame(&tmux_open(lines));
				r.frame(&tmux_burst(lines, 1));
				r.frame(&tmux_burst(lines, 30));
				r.settle();
				if !how.ends_with("ctrl-l") {
					r.frame(&tmux_enter(lines, how));
					if rested {
						r.settle();
					} else {
						assert!(r.scroll.app_offset() != 0.0, "the Enter slides");
					}
				}
				// the Enter's own slide may run out, never grow
				let left = r.scroll.app_offset();
				r.feed(&bytes.as_bytes()[..cut]);
				let first = r.build();
				let moved = r.scroll.app_offset();
				r.feed(&bytes.as_bytes()[cut..]);
				let second = r.build();
				assert!(first.alt, "tmux keeps the alt screen");
				assert!(
					(first.slid, second.slid) == (0, 0)
						&& moved <= left && r.scroll.app_offset() <= left,
					"{how}, rested {rested}, split at {cut} {:?}: slid {} then {}",
					String::from_utf8_lossy(&bytes.as_bytes()[..cut]),
					first.slid,
					second.slid
				);
			}
		}
	}
}

// Test ID: EstO8Ff
#[test]
fn output_inside_tmux_still_slides() {
	// What 2026101611321631 had to keep: a burst scrolls the pane away whole
	// too, and a line at a time scrolls it by one.
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let (cols, lines) = (100usize, 30usize);
	let pane = lines - 1;
	let mut r = Replay::new(cols, lines, 10_000);
	r.frame(&tmux_open(lines));
	r.frame(&tmux_burst(lines, 1));
	r.settle();
	let f = r.frame(&tmux_burst(lines, 30));
	assert_eq!(f.slid, pane as i32, "a burst slides");
	assert!(r.scroll.app_offset() != 0.0);
	r.settle();
	let f = r.frame(&tmux_enter(lines, "ls"));
	assert_eq!(f.slid, 1, "Enter slides a line");
	r.settle();
	// a frame that falls part way through drawing a burst still slides
	let burst = tmux_burst(lines, 60);
	let half = burst.find("75").expect("row 75");
	r.feed(&burst.as_bytes()[..half]);
	let f = r.build();
	assert_eq!(f.slid, pane as i32, "half drawn");
	r.feed(&burst.as_bytes()[half..]);
	r.build();
	assert!(r.scroll.app_offset() != 0.0);
	// a flood, with each frame falling just after a burst's scroll, while the
	// last one is still easing
	let mut next = 90;
	for _ in 0..6 {
		let burst = tmux_burst(lines, next);
		let early = burst.find(&format!("{}\x1b", next + 2)).expect("third row");
		r.feed(&burst.as_bytes()[..early]);
		let f = r.build();
		assert_eq!(f.slid, pane as i32, "flood from {next}");
		r.feed(&burst.as_bytes()[early..]);
		next += pane;
	}
}

fn chunks(dir: &str) -> Vec<Vec<u8>> {
	let data = std::fs::read(format!("{dir}/out.bin")).expect("out.bin");
	let timing = std::fs::read_to_string(format!("{dir}/timing.txt")).expect("timing.txt");
	let mut pos = data.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
	let mut out = Vec::new();
	for line in timing.lines() {
		let mut it = line.split_whitespace();
		let (Some(_), Some(n), None) = (it.next(), it.next(), it.next()) else {
			continue;
		};
		let n: usize = n.parse().unwrap_or(0);
		let end = (pos + n).min(data.len());
		out.push(data[pos..end].to_vec());
		pos = end;
	}
	out
}

// SILK_REPLAY=<dir with out.bin and timing.txt>, SILK_REPLAY_SIZE=120x42 (the
// size the session ran at), SILK_REPLAY_SB (scrollback), SILK_REPLAY_EVERY=n
// (a frame per n reads). Prints each frame that slid on the normal screen.
// Test ID: Esib2EY
#[test]
#[ignore = "needs a recording"]
fn replay_a_recorded_session() {
	let Ok(dir) = std::env::var("SILK_REPLAY") else {
		return;
	};
	let env = |name: &str, default: usize| {
		std::env::var(name)
			.ok()
			.and_then(|s| s.parse().ok())
			.unwrap_or(default)
	};
	let size = std::env::var("SILK_REPLAY_SIZE").unwrap_or_else(|_| "120x42".into());
	let (cols, lines) = size.split_once('x').expect("WxH");
	let (cols, lines): (usize, usize) = (cols.parse().unwrap(), lines.parse().unwrap());
	let every = env("SILK_REPLAY_EVERY", 1).max(1);
	let _g = config::test_store_lock();
	config::update(config::Settings::default());
	let mut r = Replay::new(cols, lines, env("SILK_REPLAY_SB", 10_000));
	let reads = chunks(&dir);
	for (i, read) in reads.iter().enumerate() {
		r.feed(read);
		if i % every != 0 {
			continue;
		}
		let f = r.build();
		if f.slid != 0 && !f.alt {
			println!("read {i}: slid {} adv {}", f.slid, f.advanced);
			println!("   bytes {:?}", String::from_utf8_lossy(read));
			let at = r.cursor_line();
			for l in at.saturating_sub(4)..(at + 3).min(lines) {
				println!("   {l:3} {}", r.row(l as i32));
			}
		}
	}
	println!("normal-screen slides: {}", r.slides.len());
}
