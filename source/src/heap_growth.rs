// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Heap growth over rounds of the program's main actions, in-process, with dhat
//! as the allocator. Each action runs N rounds to fill its caches, then N more,
//! and the heap it holds at round 2N may not be more than a margin over round
//! N. Something that keeps growing past its warm-up, such as a cache in a
//! static or a list nobody trims, fails here. The sanitizers see nothing
//! there, since the memory is still reachable.
//!
//! It needs the `dhat-heap` feature, which `cicd.bash --leakcheck` turns on, and
//! a run of its own: dhat counts every thread, so `--test-threads=1` and this
//! module's name as the filter. The report goes to `SILK_HEAP_REPORT`, or the
//! run's test folder, and the dhat profile beside it when the test fails.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::Processor;

// Rounds before the first reading, and again before the second.
const ROUNDS: usize = 20;
// Bytes an action may gain between round N and round 2N, and blocks. A leak of
// a few hundred bytes a round, or one block a round, goes over.
const MARGIN_BYTES: usize = 4096;
const MARGIN_BLOCKS: usize = ROUNDS / 2;

struct Quiet;
impl EventListener for Quiet {
	fn send_event(&self, _: Event) {}
}

struct Action {
	name: &'static str,
	// A backlog item that already has this growth, so it is reported and not
	// failed. The item's fix takes it out.
	known: Option<&'static str>,
	// A leak planted on purpose. The test fails unless it is caught.
	planted: bool,
	round: Box<dyn FnMut(usize)>,
}

#[derive(Debug, Clone, Copy)]
struct Reading {
	bytes: usize,
	blocks: usize,
}

fn reading() -> Reading {
	let stats = dhat::HeapStats::get();
	Reading {
		bytes: stats.curr_bytes,
		blocks: stats.curr_blocks,
	}
}

// What a shell session writes: prompts, colors, a wide and a combining char, a
// link, a title and its stack, a scroll region, and a full-screen program that
// comes and goes.
fn session_bytes(round: usize) -> String {
	let mut out = String::new();
	let _ = write!(out, "\x1b]0;round {}\x07\x1b[22;0t", round % 5);
	for line in 0..30 {
		let _ = write!(
			out,
			"\x1b[1;3{}m~/src\x1b[0m $ ls -l\r\n\x1b[38;2;{};90;200mfile_{line}.txt\x1b[0m \u{4e2d}e\u{301} \x1b]8;;https://example.com/{}\x1b\\link\x1b]8;;\x1b\\\r\n",
			line % 8,
			(line * 9) % 256,
			line % 3,
		);
	}
	out.push_str("\x1b[?1049h\x1b[2J\x1b[3;20r");
	for line in 0..40 {
		let _ = write!(out, "\x1b[20;1Hframe {line} of a pager\r\n");
	}
	out.push_str("\x1b[r\x1b[?1049l\x1b[23;0t");
	out
}

fn grid_dims(cols: usize, lines: usize) -> crate::term::TermDimensions {
	crate::term::TermDimensions {
		columns: cols,
		screen_lines: lines,
	}
}

// A new tab's engine: opened, fed, resized and closed.
fn engine_round(round: usize) {
	let cfg = TermConfig {
		scrolling_history: 2000,
		..Default::default()
	};
	let mut term = Term::new(cfg, &grid_dims(100, 30), Quiet);
	let mut parser = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
	parser.advance(&mut term, session_bytes(round).as_bytes());
	term.resize(grid_dims(80 + round % 3, 24));
	parser.advance(&mut term, session_bytes(round + 1).as_bytes());
}

// Output arriving in a tab whose scrollback is full, one frame per read: the
// engine, the output ease, the scroll record and the row-print detector.
fn output_action() -> Box<dyn FnMut(usize)> {
	let mut frame = crate::pane::replay::frames(100, 30, 500);
	for round in 0..20 {
		frame(&session_bytes(round));
	}
	Box::new(move |round| {
		for chunk in session_bytes(round)
			.split_inclusive("\r\n")
			.collect::<Vec<_>>()
			.chunks(8)
		{
			frame(&chunk.concat());
		}
	})
}

// The minimap folding new output into a full scrollback, its composes on their
// own thread as in a window, each round waited out so the reading has no
// compose in flight.
fn minimap_action() -> Box<dyn FnMut(usize)> {
	let (cols, lines) = (100, 30);
	let cfg = TermConfig {
		scrolling_history: 1000,
		..Default::default()
	};
	let mut term = Term::new(cfg, &grid_dims(cols, lines), Quiet);
	let mut parser = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
	for round in 0..30 {
		parser.advance(&mut term, session_bytes(round).as_bytes());
	}
	let settings = crate::config::Settings::default();
	let mut map = crate::minimap::Minimap::default();
	let start = Instant::now();
	let mut step = 0u64;
	Box::new(move |round| {
		let before = term.grid().history_size();
		parser.advance(&mut term, session_bytes(round).as_bytes());
		let mut advanced = term.grid().history_size().saturating_sub(before).max(60);
		// a minute a step, so the throttle never decides a compose
		for _ in 0..200 {
			step += 1;
			map.update(
				term.grid(),
				term.colors(),
				&settings,
				60,
				900,
				1.0,
				lines,
				cols,
				advanced,
				0,
				false,
				false,
				start + Duration::from_secs(60 * step),
			);
			advanced = 0;
			if map.wake().is_none() {
				break;
			}
			std::thread::sleep(Duration::from_millis(2));
		}
	})
}

// Text shaped in the terminal font, with the fallback and color-glyph lookups a
// frame makes for what the font lacks.
fn text_action() -> Box<dyn FnMut(usize)> {
	let mut ctx = crate::text::TextCtx::new_cpu(1.0);
	let lines = [
		"drwxr-xr-x  2 user user 4096 Oct 19 10:00 src",
		"\u{4e2d}\u{6587} mixed with ASCII and e\u{301}",
		"emoji \u{1f600} \u{1f680} and box \u{2500}\u{2502}\u{250c}",
		"\u{5d0}\u{5d1}\u{5d2} right to left, then \u{3b1}\u{3b2}\u{3b3}",
	];
	Box::new(move |round| {
		for line in &lines {
			std::hint::black_box(ctx.measure_mono_text(line));
			for ch in line.chars() {
				std::hint::black_box(ctx.covered_at(ch, 1 + u8::from(round % 2 == 0)));
				std::hint::black_box(ctx.color_metrics(ch));
			}
		}
	})
}

fn actions() -> Vec<Action> {
	let mut planted: Vec<Vec<u8>> = Vec::new();
	vec![
		Action {
			name: "a planted leak, 1 KiB a round",
			known: None,
			planted: true,
			round: Box::new(move |round| planted.push(vec![round as u8; 1024])),
		},
		Action {
			name: "a tab's engine opened, fed, resized and closed",
			known: None,
			planted: false,
			round: Box::new(engine_round),
		},
		Action {
			name: "output into a full scrollback, a frame per read",
			known: None,
			planted: false,
			round: output_action(),
		},
		Action {
			name: "the minimap folding new output",
			known: None,
			planted: false,
			round: minimap_action(),
		},
		Action {
			name: "text shaped, with fallback and color glyphs",
			known: None,
			planted: false,
			round: text_action(),
		},
		Action {
			name: "the config read at launch and a value saved",
			known: None,
			planted: false,
			round: Box::new(|round| {
				std::hint::black_box(crate::config::heap_round(1.0 + (round % 4) as f64));
			}),
		},
		Action {
			name: "the Settings dialog's spec parsed",
			known: None,
			planted: false,
			round: Box::new(|_| {
				std::hint::black_box(crate::ui_spec::heap_round());
			}),
		},
	]
}

fn report_path() -> PathBuf {
	std::env::var_os("SILK_HEAP_REPORT")
		.filter(|path| !path.is_empty())
		.map_or_else(
			|| crate::testdir::run_dir().join("heap_growth.txt"),
			PathBuf::from,
		)
}

// The growth past the warm-up, one action at a time, so a report names the
// action that grew.
// Test ID: EtBDWu8
#[test]
#[cfg_attr(
	not(feature = "dhat-heap"),
	ignore = "needs the dhat-heap feature, as cicd.bash --leakcheck runs it"
)]
fn the_heap_stops_growing_after_a_warm_up() {
	let report = report_path();
	let _profiler = dhat::Profiler::builder()
		.testing()
		.file_name(report.with_extension("dhat.json"))
		.build();
	// dhat has to be the allocator, or every reading is 0 and nothing can fail
	let probe = vec![0u8; 4096];
	assert!(
		reading().bytes >= probe.len(),
		"dhat isn't counting; the test build needs --features dhat-heap and a debug build"
	);
	drop(probe);

	let mut text = format!(
		"Heap held by each action at round {ROUNDS} and round {}, margin {MARGIN_BYTES} bytes or {MARGIN_BLOCKS} blocks.\n\n",
		ROUNDS * 2
	);
	let mut failed: Vec<String> = Vec::new();
	for mut action in actions() {
		let began = Instant::now();
		for round in 0..ROUNDS {
			(action.round)(round);
		}
		let at_n = reading();
		for round in ROUNDS..ROUNDS * 2 {
			(action.round)(round);
		}
		let at_2n = reading();
		let bytes = at_2n.bytes as i64 - at_n.bytes as i64;
		let blocks = at_2n.blocks as i64 - at_n.blocks as i64;
		let grew = bytes > MARGIN_BYTES as i64 || blocks > MARGIN_BLOCKS as i64;
		let verdict = match (grew, action.planted, action.known) {
			(true, true, _) => "caught, as planted".to_string(),
			(false, true, _) => {
				failed.push(format!("{}: not caught", action.name));
				"NOT CAUGHT".to_string()
			}
			(true, false, Some(id)) => format!("grew, known: {id}"),
			(true, false, None) => {
				failed.push(format!("{}: {bytes} bytes, {blocks} blocks", action.name));
				"GREW".to_string()
			}
			(false, false, _) => "flat".to_string(),
		};
		let _ = writeln!(
			text,
			"{:<52} {:>10} -> {:>10} bytes ({bytes:+}), {:>6} -> {:>6} blocks ({blocks:+}), {:.1}s: {verdict}",
			action.name,
			at_n.bytes,
			at_2n.bytes,
			at_n.blocks,
			at_2n.blocks,
			began.elapsed().as_secs_f64(),
		);
	}
	print!("{text}");
	if let Some(dir) = report.parent() {
		let _ = std::fs::create_dir_all(dir);
	}
	let _ = std::fs::write(&report, &text);
	dhat::assert!(
		failed.is_empty(),
		"the heap grew past the warm-up: {}; report in {}",
		failed.join("; "),
		report.display()
	);
}
