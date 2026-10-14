// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! A probe for 2026101318475899: can a history be rebuilt for a full-screen program
//! by watching what it draws? `Probe` keeps a transcript of the rows it has seen and
//! where the program's scroll region sits in it. Each frame the ledger, or the row
//! fingerprints where the ledger is empty, says how far the region moved. A frame
//! with no shift is placed again by matching its rows against the transcript, and a
//! frame that matches nothing starts a new stretch after a gap. Test code only: the
//! numbers are in design_docs/20261013-213000_virtual-history.md.

use std::fmt::Write as _;
use std::ops::Range;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::Processor;

use super::{APP_SCROLL_MAX, fnv_row, scroll_shift_signed, translate_span};

struct Quiet;
impl EventListener for Quiet {
	fn send_event(&self, _: Event) {}
}

#[derive(Debug, Default)]
struct Tally {
	frames: usize,
	by_ledger: usize,
	by_print: usize,
	placed: usize,
	gaps: usize,
	rewrites: usize,
	misplaced: usize,
	inside: usize,
	pinned: usize,
}

struct Probe {
	rows: Vec<String>,
	top: Option<usize>,
	region: Range<usize>,
	last: Vec<u64>,
	pending: Option<Vec<String>>,
	last_text: Vec<String>,
	tally: Tally,
}

fn screen(term: &Term<Quiet>) -> (Vec<u64>, Vec<String>) {
	let grid = term.grid();
	let cols = grid.columns();
	(0..grid.screen_lines())
		.map(|i| {
			let row = &grid[Line(i as i32)];
			let chars = || (0..cols).map(|c| row[Column(c)].c);
			(
				fnv_row(chars()),
				chars().collect::<String>().trim_end().to_string(),
			)
		})
		.unzip()
}

impl Probe {
	fn new(lines: usize) -> Self {
		Self {
			rows: Vec::new(),
			top: None,
			region: 0..lines,
			last: Vec::new(),
			pending: None,
			last_text: Vec::new(),
			tally: Tally::default(),
		}
	}

	// Where in the transcript the region's rows sit, by the best run of matches.
	fn find(&self, text: &[String]) -> Option<usize> {
		let want: Vec<(usize, &String)> = text[self.region.clone()]
			.iter()
			.enumerate()
			.filter(|(_, t)| !t.is_empty())
			.collect();
		let need = (want.len() / 2).max(3);
		let (&(first, key), _) = want.split_first()?;
		let mut best = None;
		let mut best_score = 0;
		for (p, row) in self.rows.iter().enumerate() {
			if row != key || p < first {
				continue;
			}
			let at = p - first;
			let score = want
				.iter()
				.filter(|(i, t)| self.rows.get(at + i) == Some(t))
				.count();
			if score >= need && score > best_score {
				(best, best_score) = (Some(at), score);
			}
		}
		best
	}

	fn frame(&mut self, term: &mut Term<Quiet>, quiet: bool) {
		self.tally.frames += 1;
		let lines = term.grid().screen_lines();
		let (cur, text) = screen(term);
		let ledger = term.scroll_ledger();
		let clamp = |l: Line| (l.0.max(0) as usize).min(lines);
		let mut shift = 0i32;
		let mut region = self.region.clone();
		let mut gone: Vec<String> = Vec::new();
		if ledger.lines() != 0 {
			let r = ledger.region();
			region = clamp(r.start)..clamp(r.end);
			shift = ledger.lines();
			let cols = term.grid().columns();
			gone = ledger
				.rows()
				.iter()
				.map(|row| {
					let n = cols.min(row.len());
					(0..n)
						.map(|c| row[Column(c)].c)
						.collect::<String>()
						.trim_end()
						.to_string()
				})
				.collect();
			self.tally.by_ledger += 1;
		} else if self.last.len() == cur.len() && cur != self.last {
			shift = scroll_shift_signed(&cur, &self.last, APP_SCROLL_MAX);
			if let Some((lo, hi)) = translate_span(&cur, &self.last, shift) {
				region = lo..hi + 1;
				self.tally.by_print += 1;
			} else {
				shift = 0;
			}
		}
		// a scroll of rows inside the transcript's region, below its top, is an edit
		// in place (vim deleting a line): nothing went into the history
		if shift != 0
			&& self.top.is_some()
			&& region.start > self.region.start
			&& region.end <= self.region.end
		{
			self.tally.inside += 1;
			shift = 0;
			gone.clear();
			region = self.region.clone();
		}
		// a row at the region's edge that reads the same after a scroll is chrome
		// the program draws again (less's prompt), as the slide's bands hold it
		if shift != 0 && self.last_text.len() == text.len() && region.len() > 2 {
			let edge = |i: usize| !text[i].is_empty() && text[i] == self.last_text[i];
			if edge(region.end - 1) {
				region.end -= 1;
				self.tally.pinned += 1;
				if shift < 0 {
					gone.clear();
				}
			}
			if edge(region.start) {
				region.start += 1;
				self.tally.pinned += 1;
				if shift > 0 {
					gone.clear();
				}
			}
		}
		if std::env::var_os("SILK_VHIST_TRACE").is_some() {
			eprintln!(
				"frame {} shift {shift} region {region:?} gone {} top {:?} first {:?}",
				self.tally.frames,
				gone.len(),
				self.top,
				text.get(3)
			);
		}
		term.scroll_ledger_mut().clear();
		let changed = self.last.len() != cur.len() || cur != self.last;
		self.last = cur;
		self.last_text.clone_from(&text);
		// nothing is kept until a first scroll says which rows are the transcript,
		// so a program's chrome never starts it
		if self.top.is_none() && shift == 0 {
			return;
		}
		if shift == 0 {
			// a frame that changed with no scroll waits until it stops changing, so
			// a repaint caught half drawn is not written over good rows
			if changed && !quiet {
				self.pending = Some(text);
			} else if changed {
				self.pending = None;
				self.place(&text, 0, self.region.clone(), &[]);
			} else if let Some(settled) = self.pending.take() {
				self.place(&settled, 0, self.region.clone(), &[]);
			}
			return;
		}
		// the last frame never settled before this scroll; it is all there is of it
		if let Some(unsettled) = self.pending.take() {
			self.place(&unsettled, 0, self.region.clone(), &[]);
		}
		if region.is_empty() {
			region = 0..lines;
		}
		self.place(&text, shift, region, &gone);
	}

	fn place(&mut self, text: &[String], shift: i32, region: Range<usize>, gone: &[String]) {
		let start_moved = region.start as isize - self.region.start as isize;
		self.region = region.clone();
		let height = region.len();
		let mut top = self.top.map_or(0, |t| t as isize + start_moved) + shift as isize;
		// check the guess against what is on screen; a frame that disagrees is placed
		// by search, and one that changed in place (a spinner, a pill, a line still
		// streaming) stays where it was
		let known: Vec<bool> = (0..height)
			.filter_map(|i| {
				let idx = top + i as isize;
				let row = &text[region.start + i];
				(idx >= 0 && (idx as usize) < self.rows.len() && !row.is_empty())
					.then(|| self.rows[idx as usize] == *row)
			})
			.collect();
		let agrees = known.len() < 3 || known.iter().filter(|&&m| m).count() * 2 >= known.len();
		if !agrees {
			if shift != 0 {
				self.tally.misplaced += 1;
			}
			if let Some(p) = self.find(text) {
				top = p as isize;
				self.tally.placed += 1;
			} else {
				if !self.rows.is_empty() {
					self.tally.gaps += 1;
				}
				top = self.rows.len() as isize;
				if shift > 0 {
					top += gone.len() as isize;
				}
			}
		}
		// the ledger's rows are the ones that crossed the region's edge, the last of
		// them nearest it; past a screenful between frames, some were never drawn
		for (i, row) in gone.iter().enumerate() {
			let at = if shift > 0 {
				top - (gone.len() - i) as isize
			} else {
				top + (height + i) as isize
			};
			self.put(at, row, &mut top);
		}
		for i in 0..height {
			self.put(top + i as isize, &text[region.start + i], &mut top);
		}
		self.top = Some(top.max(0) as usize);
	}

	// Write one row into the transcript at `at`, which may be before its start or
	// past its end; `top` moves with anything put in front.
	fn put(&mut self, at: isize, row: &str, top: &mut isize) {
		if at < 0 {
			let n = (-at) as usize;
			self.rows
				.splice(0..0, std::iter::repeat_n(String::new(), n));
			*top += n as isize;
			self.rows[0] = row.to_string();
			return;
		}
		let at = at as usize;
		if at >= self.rows.len() {
			self.rows.resize(at + 1, String::new());
		}
		// a blank row on screen says nothing: it may be one not drawn yet
		let old = &mut self.rows[at];
		if old != row && !row.is_empty() {
			if !old.is_empty() {
				self.tally.rewrites += 1;
			}
			*old = row.to_string();
		}
	}
}

// The number after `tag` in a row, as the recorded file and the scenes number them.
fn number(row: &str, tag: char) -> Option<usize> {
	let at = row.find(tag)?;
	let digits: String = row[at + 1..]
		.chars()
		.take_while(char::is_ascii_digit)
		.collect();
	(digits.len() == 5).then(|| digits.parse().ok()).flatten()
}

#[derive(Debug)]
struct Score {
	printed: usize,
	kept: usize,
	breaks: usize,
	junk: usize,
}

fn score(rows: &[String], printed: &std::collections::BTreeSet<usize>, tag: char) -> Score {
	let mut seq = Vec::new();
	let mut junk = 0;
	for row in rows.iter().filter(|r| !r.is_empty()) {
		match number(row, tag) {
			Some(n) => seq.push(n),
			None => junk += 1,
		}
	}
	let kept: std::collections::BTreeSet<usize> = seq.iter().copied().collect();
	Score {
		printed: printed.len(),
		kept: kept.intersection(printed).count(),
		breaks: seq.windows(2).filter(|w| w[1] != w[0] + 1).count(),
		junk,
	}
}

fn printed_numbers(bytes: &[u8], tag: char) -> std::collections::BTreeSet<usize> {
	let text = String::from_utf8_lossy(bytes);
	text.match_indices(tag)
		.filter_map(|(at, _)| number(&text[at..], tag))
		.collect()
}

// When the pane builds a frame: after every read, after every n reads, or at most
// so many times a second, taking every read that came in meanwhile.
#[derive(Clone, Copy, Debug)]
enum Frames {
	EachRead,
	Every(usize),
	Hz(f64),
}

fn replay(
	chunks: &[(f64, Vec<u8>)],
	cols: usize,
	lines: usize,
	frames: Frames,
) -> (Probe, std::time::Duration) {
	let dims = crate::term::TermDimensions {
		columns: cols,
		screen_lines: lines,
	};
	let cfg = TermConfig {
		scrolling_history: 1000,
		..Default::default()
	};
	let mut term = Term::new(cfg, &dims, Quiet);
	term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
	let mut parser: Processor = Processor::new();
	let mut probe = Probe::new(lines);
	let mut spent = std::time::Duration::ZERO;
	let mut i = 0;
	while i < chunks.len() {
		let end = match frames {
			Frames::EachRead => i + 1,
			Frames::Every(n) => i + n.max(1),
			Frames::Hz(rate) => {
				let until = chunks[i].0 + 1.0 / rate;
				i + chunks[i..]
					.iter()
					.take_while(|(t, _)| *t < until)
					.count()
					.max(1)
			}
		}
		.min(chunks.len());
		for (_, chunk) in &chunks[i..end] {
			parser.advance(&mut term, chunk);
		}
		// output that stops for a moment has finished drawing
		let quiet = chunks
			.get(end)
			.is_none_or(|(t, _)| t - chunks[end - 1].0 >= 0.05);
		let t = std::time::Instant::now();
		probe.frame(&mut term, quiet);
		spent += t.elapsed();
		i = end;
	}
	(probe, spent)
}

// Every numbered row that was on screen after any read: what the program showed.
fn shown_numbers(
	chunks: &[(f64, Vec<u8>)],
	cols: usize,
	lines: usize,
	tag: char,
) -> std::collections::BTreeSet<usize> {
	let dims = crate::term::TermDimensions {
		columns: cols,
		screen_lines: lines,
	};
	let mut term = Term::new(TermConfig::default(), &dims, Quiet);
	let mut parser: Processor = Processor::new();
	let mut shown = std::collections::BTreeSet::new();
	for (_, chunk) in chunks {
		parser.advance(&mut term, chunk);
		shown.extend(screen(&term).1.iter().filter_map(|row| number(row, tag)));
	}
	shown
}

fn timed(chunks: Vec<Vec<u8>>) -> Vec<(f64, Vec<u8>)> {
	chunks
		.into_iter()
		.enumerate()
		.map(|(i, c)| (i as f64 * 0.01, c))
		.collect()
}

fn joined(chunks: &[(f64, Vec<u8>)]) -> Vec<u8> {
	chunks.iter().flat_map(|(_, c)| c.iter().copied()).collect()
}

// A transcript program on the alt screen with 2 header rows, a 3-row input box and
// a footer: muffer's layout. Lines arrive by a region scroll (`CSI S`) or by a
// repaint shifted up, the wheel scrolls back with `CSI T`, a pill is painted over
// the region's last row while scrolled back, Enter repaints the transcript 2 rows
// up, and one long tool output jumps further than the detector will believe.
fn transcript_scene(lines: usize, repaint: bool) -> (Vec<Vec<u8>>, usize) {
	let (top, bottom) = (3, lines - 4);
	let height = bottom - top + 1;
	let line = |n: usize| format!("M{n:05} tool output for step {n} with some text");
	let mut chunks = vec![format!(
		"\x1b[?1049h\x1b[1;1H\x1b[7m header \x1b[0m\x1b[K\x1b[2;1H session\x1b[K\x1b[{};1H> \x1b[K\x1b[{lines};1H footer\x1b[K",
		lines - 3
	)];
	let paint = |first: usize, last_shown: usize| {
		let mut s = String::from("\x1b[?2026h");
		for r in 0..height {
			let n = last_shown as isize + 1 - height as isize + r as isize;
			let text = if n >= first as isize {
				line(n as usize)
			} else {
				String::new()
			};
			let _ = write!(s, "\x1b[{};1H{text}\x1b[K", top + r);
		}
		s + "\x1b[?2026l"
	};
	let mut newest = 0usize;
	let add = |chunks: &mut Vec<String>, newest: &mut usize, k: usize| {
		if repaint {
			*newest += k;
			chunks.push(paint(1, *newest));
		} else {
			for _ in 0..k {
				*newest += 1;
				chunks.push(format!(
					"\x1b[{top};{bottom}r\x1b[{bottom};1H\x1b[S\x1b[r\x1b[{bottom};1H{}\x1b[K",
					line(*newest)
				));
			}
		}
	};
	for _ in 0..200 {
		add(&mut chunks, &mut newest, 1);
	}
	// the wheel: 10 notches of 3 rows back, the pill on the last row, then forward
	let mut shown = newest;
	for _ in 0..10 {
		shown -= 3;
		let first = shown + 1 - height;
		chunks.push(format!(
			"\x1b[{top};{bottom}r\x1b[{top};1H\x1b[3T\x1b[r\x1b[{top};1H{}\x1b[K\x1b[{};1H{}\x1b[K\x1b[{};1H{}\x1b[K\x1b[{bottom};1H{}\x1b[7m 1 new message \x1b[0m\x1b[K",
			line(first),
			top + 1,
			line(first + 1),
			top + 2,
			line(first + 2),
			line(shown)
		));
	}
	for _ in 0..10 {
		shown += 3;
		chunks.push(format!(
			"\x1b[{top};{bottom}r\x1b[{bottom};1H\x1b[3S\x1b[r\x1b[{};1H{}\x1b[K\x1b[{};1H{}\x1b[K\x1b[{bottom};1H{}\x1b[K",
			bottom - 2,
			line(shown - 2),
			bottom - 1,
			line(shown - 1),
			line(shown)
		));
	}
	// Enter: the box collapses and the transcript is repainted 2 rows up
	newest += 2;
	chunks.push(paint(1, newest));
	for _ in 0..20 {
		add(&mut chunks, &mut newest, 1);
	}
	// a long tool output lands in one repaint
	newest += 40;
	chunks.push(paint(1, newest));
	for _ in 0..20 {
		add(&mut chunks, &mut newest, 1);
	}
	(chunks.into_iter().map(String::into_bytes).collect(), newest)
}

// Test ID: Ese21WF
#[test]
fn a_transcript_scrolled_by_region_is_rebuilt_whole() {
	let (lines, cols) = (30, 80);
	let height = lines - 6;
	for repaint in [false, true] {
		let (chunks, newest) = transcript_scene(lines, repaint);
		let chunks = timed(chunks);
		let (probe, _) = replay(&chunks, cols, lines, Frames::EachRead);
		let printed = printed_numbers(&joined(&chunks), 'M');
		let s = score(&probe.rows, &printed, 'M');
		// every line that was ever on screen is kept, in order, once, with no
		// header, box or pill left in the transcript; the rows the long output
		// jumped past were never drawn, so they are the one gap
		let never_drawn = 40 - height;
		let missing = printed.len() - s.kept;
		assert_eq!(missing, 0, "repaint {repaint}: {s:?} {:?}", probe.tally);
		assert_eq!(s.printed, newest - never_drawn, "repaint {repaint}");
		assert_eq!(s.junk, 0, "repaint {repaint}: {s:?}");
		assert_eq!(s.breaks, 1, "repaint {repaint}: {s:?} {:?}", probe.tally);
		assert_eq!(probe.tally.gaps, 1, "repaint {repaint}: {:?}", probe.tally);
	}
}

// Test ID: Ese21WG
#[test]
fn a_burst_past_a_screenful_between_frames_loses_nothing() {
	// a frame every 30 new lines on a 24-row transcript: 6 rows each time were
	// never on screen when a frame was built, and only the ledger kept them
	let (lines, cols) = (30, 80);
	let (chunks, newest) = transcript_scene(lines, false);
	let chunks = timed(chunks.into_iter().take(201).collect());
	let (probe, _) = replay(&chunks, cols, lines, Frames::Every(30));
	let printed = printed_numbers(&joined(&chunks), 'M');
	let s = score(&probe.rows, &printed, 'M');
	assert!(newest > 200 && printed.len() == 200, "{}", printed.len());
	assert_eq!(s.kept, 200, "{s:?} {:?}", probe.tally);
	assert_eq!((s.breaks, s.junk), (0, 0), "{s:?}");
}

// Recordings made by cicd/utility/vhist/record.py, one `<app>.rec` per program in
// the folder named by SILK_VHIST_REC, 100 columns by 30 rows. Prints a table.
// Test ID: Ese21WH
#[test]
#[ignore = "needs recordings; run by hand with SILK_VHIST_REC"]
fn recorded_programs_rebuild_what_they_showed() {
	// the made-up transcript with a frame every 7 writes: the wheel turns and
	// repaints fall inside one frame
	for repaint in [false, true] {
		let (chunks, _) = transcript_scene(30, repaint);
		let chunks = timed(chunks);
		let printed = printed_numbers(&joined(&chunks), 'M');
		let (probe, _) = replay(&chunks, 80, 30, Frames::Every(7));
		let s = score(&probe.rows, &printed, 'M');
		println!("scene repaint={repaint} every 7: {s:?} {:?}", probe.tally);
	}
	let Ok(dir) = std::env::var("SILK_VHIST_REC") else {
		return;
	};
	println!(
		"app\tframe\tshown\tkept\tbreaks\tjunk\tledger\tprint\tinside\tpinned\tplaced\tgaps\trewrite\tmisplace\trows\tus/frame"
	);
	for app in ["less", "vim", "nano", "htop"] {
		let Ok(bytes) = std::fs::read(format!("{dir}/{app}.rec")) else {
			continue;
		};
		let mut chunks = Vec::new();
		let mut at = 0;
		while at + 12 <= bytes.len() {
			let t = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or_default());
			let n =
				u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap_or_default()) as usize;
			chunks.push((t, bytes[at + 12..(at + 12 + n).min(bytes.len())].to_vec()));
			at += 12 + n;
		}
		let printed = shown_numbers(&chunks, 100, 30, 'L');
		for per in [Frames::EachRead, Frames::Hz(60.0), Frames::Hz(20.0)] {
			let (probe, spent) = replay(&chunks, 100, 30, per);
			let s = score(&probe.rows, &printed, 'L');
			let t = &probe.tally;
			println!(
				"{app}\t{per:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.1}",
				s.printed,
				s.kept,
				s.breaks,
				s.junk,
				t.by_ledger,
				t.by_print,
				t.inside,
				t.pinned,
				t.placed,
				t.gaps,
				t.rewrites,
				t.misplaced,
				probe.rows.iter().filter(|r| !r.is_empty()).count(),
				spent.as_secs_f64() * 1e6 / t.frames.max(1) as f64
			);
			let kept: std::collections::BTreeSet<usize> =
				probe.rows.iter().filter_map(|r| number(r, 'L')).collect();
			let missing: Vec<&usize> = printed.difference(&kept).collect();
			println!("\tmissing {missing:?}");
			if std::env::var_os("SILK_VHIST_DUMP").is_some() {
				for row in &probe.rows {
					println!("  |{row}");
				}
			}
		}
	}
}
