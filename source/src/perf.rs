// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Where a burst of output actually goes (`SILK_PERF=1`).
//!
//! Throughput on Windows reads as one number - "it takes N seconds to swallow a
//! large `cat`" - and that number is the sum of three parties: the console host
//! delivering bytes, the engine parsing them, and this program reacting to each
//! batch. Only the last is ours to change, and it is invisible from outside the
//! process: a wall-clock stopwatch cannot say whether the time went into the
//! grid or into waking the event loop forty thousand times.
//!
//! The counters are plain atomics behind one cached flag, so an ordinary run
//! pays a relaxed load per site and nothing else. The report goes to stderr when
//! the loop exits.
//!
//! `SILK_LATENCY=1` answers the other question - how long a typed character
//! takes to appear - and is separate because it is a stopwatch per keystroke
//! rather than a running total.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn on() -> bool {
	static ON: OnceLock<bool> = OnceLock::new();
	*ON.get_or_init(|| std::env::var_os("SILK_PERF").is_some())
}

macro_rules! counters {
	($($name:ident),* $(,)?) => {
		$(pub static $name: AtomicU64 = AtomicU64::new(0);)*
	};
}

counters!(
	WAKEUPS,    // output notices delivered to the window (UserEvent::Wakeup)
	PASSES,     // about_to_wait passes (one per event-loop iteration)
	FRAMES,     // frames actually rendered
	BUILD_NS,   // Pane::build, summed over panes
	RENDER_NS,  // State::render, whole frame including build
	NOTE_NS,    // per-wakeup bookkeeping (note_output/note_history)
	LOCK_MISS,  // builds that gave up on the term lock
	PREP_NS,    // glyphon prepare (text + scrim source)
	ACQUIRE_NS, // waiting for a swapchain image
	ENCODE_NS,  // recording the passes
	SUBMIT_NS,  // submit + present
	EVENT_NS,   // our user_event handler, whole
	PASS_NS,    // our about_to_wait, whole
);

pub fn add(counter: &AtomicU64, n: u64) {
	if on() {
		counter.fetch_add(n, Relaxed);
	}
}

pub fn bump(counter: &AtomicU64) {
	add(counter, 1);
}

// Start of a stretch that can't be a scope (an early return in the middle, or a
// span that ends in another block). None when the counters are off, so a normal
// run doesn't even read the clock.
pub fn mark() -> Option<Instant> {
	on().then(Instant::now)
}

pub fn since(counter: &AtomicU64, mark: Option<Instant>) {
	if let Some(start) = mark {
		counter.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
	}
}

// Time a block into a counter. Cheap enough to leave in the hot path: without
// the flag it is a load and a branch, no clock read at all.
pub fn timed<T>(counter: &AtomicU64, body: impl FnOnce() -> T) -> T {
	if !on() {
		return body();
	}
	let start = Instant::now();
	let out = body();
	counter.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
	out
}

// CPU seconds this thread (the window/render thread) and the whole process have
// burned. The split is the point: a frame that BLOCKS on the display costs wall
// time and no CPU, so a wall-clock render figure on its own cannot say whether
// the main thread is working or waiting.
#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)] // the other platforms answer None
fn cpu_seconds() -> Option<(f64, f64)> {
	use std::ptr::from_mut;
	use windows_sys::Win32::Foundation::FILETIME;
	use windows_sys::Win32::System::Threading::{
		GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes,
	};

	let secs = |t: FILETIME| {
		((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) as f64 / 1e7
	};
	let zero = FILETIME {
		dwLowDateTime: 0,
		dwHighDateTime: 0,
	};
	let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
	let (created, exited, kernel, user) = (
		from_mut(&mut created),
		from_mut(&mut exited),
		from_mut(&mut kernel),
		from_mut(&mut user),
	);
	// SAFETY: plain Win32 calls on pseudo-handles; every output is owned here.
	unsafe {
		GetThreadTimes(GetCurrentThread(), created, exited, kernel, user);
		let thread = secs(*kernel) + secs(*user);
		GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user);
		Some((thread, secs(*kernel) + secs(*user)))
	}
}

// Nothing equivalent exists elsewhere yet, and a pair of zeroes would read
// as "this thread did nothing" rather than "not measured here".
#[cfg(not(windows))]
fn cpu_seconds() -> Option<(f64, f64)> {
	None
}

// Scope timer for a whole function, where wrapping the body in a closure would
// fight the borrow checker.
#[derive(Debug)]
pub struct Span<'a> {
	counter: &'a AtomicU64,
	start: Option<Instant>,
}

impl<'a> Span<'a> {
	pub fn new(counter: &'a AtomicU64) -> Self {
		Self {
			counter,
			start: on().then(Instant::now),
		}
	}
}

impl Drop for Span<'_> {
	fn drop(&mut self) {
		if let Some(start) = self.start {
			self.counter
				.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
		}
	}
}

pub fn report() {
	latency_report();
	if !on() {
		return;
	}
	let cpu = cpu_seconds();
	let ms = |counter: &AtomicU64| counter.load(Relaxed) as f64 / 1e6;
	eprintln!(
		"[perf] wakeups {} passes {} frames {} | render {:.0}ms (build {:.0}ms) note {:.0}ms | lock misses {}",
		WAKEUPS.load(Relaxed),
		PASSES.load(Relaxed),
		FRAMES.load(Relaxed),
		ms(&RENDER_NS),
		ms(&BUILD_NS),
		ms(&NOTE_NS),
		LOCK_MISS.load(Relaxed),
	);
	eprintln!(
		"[perf] our handlers: user_event {:.0}ms about_to_wait {:.0}ms",
		ms(&EVENT_NS),
		ms(&PASS_NS),
	);
	eprintln!(
		"[perf] frame parts: prepare {:.0}ms acquire {:.0}ms encode {:.0}ms submit+present {:.0}ms",
		ms(&PREP_NS),
		ms(&ACQUIRE_NS),
		ms(&ENCODE_NS),
		ms(&SUBMIT_NS),
	);
	if let Some((thread_cpu, process_cpu)) = cpu {
		eprintln!("[perf] cpu: this thread {thread_cpu:.2}s of process {process_cpu:.2}s");
	}
}

// Keypress to pixel (`SILK_LATENCY=1`), timed over three legs rather than as
// one number: getting the key to the pty, the shell echoing it back, and us
// putting that on screen. Only the last leg is ours, and a single total cannot
// say which of the three is the slow one.
//
// Two things it cannot do. Nothing after `present` returns is visible from in
// here, so the compositor and the display are missing and a figure is a floor,
// not the whole wait. And it cannot tell a shell's echo from output nobody
// typed for, so it belongs at a settled prompt; anything still unanswered after
// LATENCY_WINDOW is dropped rather than reported as a very slow keystroke.
pub fn latency_on() -> bool {
	static ON: OnceLock<bool> = OnceLock::new();
	*ON.get_or_init(|| std::env::var_os("SILK_LATENCY").is_some())
}

const LATENCY_WINDOW: Duration = Duration::from_millis(750);

struct Pending {
	pane: u64,
	key: Instant,
	wrote: Instant,
	out: Option<Instant>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static SAMPLES: Mutex<Vec<(u32, u32, u32)>> = Mutex::new(Vec::new());

// When a key event arrived. Taken at the top of the handler, before any of the
// hotkey and menu work, so what the key spends in this program is inside the
// figure rather than beside it.
pub fn key_mark() -> Option<Instant> {
	latency_on().then(Instant::now)
}

// The key's bytes are on their way to the shell. A key pressed while one is
// still in flight replaces it: the echo that follows belongs to the last thing
// typed, and timing from the first would read the whole burst as one keystroke.
pub fn typed(key: Option<Instant>, pane: u64) {
	let Some(key) = key else {
		return;
	};
	*crate::locks::lock(&PENDING) = Some(Pending {
		pane,
		key,
		wrote: Instant::now(),
		out: None,
	});
}

// Output arrived from the pane that was typed at. Only the first notice counts:
// a shell that answers in several reads is still one echo.
pub fn echoed(pane: u64) {
	if !latency_on() {
		return;
	}
	let mut slot = crate::locks::lock(&PENDING);
	let Some(pending) = slot.as_mut() else {
		return;
	};
	if pending.pane != pane {
		return;
	}
	if pending.key.elapsed() > LATENCY_WINDOW {
		*slot = None;
		return;
	}
	pending.out.get_or_insert_with(Instant::now);
}

// A frame just went out, so the echo is on screen. Says its own line as it goes
// - the point is to watch this while typing, and printing after the frame costs
// the frame nothing.
pub fn painted() {
	if !latency_on() {
		return;
	}
	let mut slot = crate::locks::lock(&PENDING);
	let Some(pending) = slot.as_ref() else {
		return;
	};
	if pending.key.elapsed() > LATENCY_WINDOW {
		*slot = None;
		return;
	}
	// Typing marks the window dirty, so frames go out between the key and the
	// echo. Those are not the frame being waited for - leave the stopwatch
	// running rather than taking one of them as the answer.
	let Some(out) = pending.out else {
		return;
	};
	let us = |d: Duration| d.as_micros() as u32;
	let (send, echo, draw) = (
		us(pending.wrote - pending.key),
		us(out - pending.wrote),
		us(out.elapsed()),
	);
	*slot = None;
	drop(slot);
	let ms = |us: u32| f64::from(us) / 1e3;
	eprintln!(
		"[latency] send {:.2}ms  echo {:.2}ms  draw {:.2}ms  total {:.2}ms",
		ms(send),
		ms(echo),
		ms(draw),
		ms(send + echo + draw),
	);
	crate::locks::lock(&SAMPLES).push((send, echo, draw));
}

// Median and p95 of `sorted`, which must already be sorted.
fn percentiles(sorted: &[u32]) -> (u32, u32) {
	let at = |pct: usize| sorted[(sorted.len() * pct / 100).min(sorted.len() - 1)];
	(at(50), at(95))
}

fn latency_report() {
	if !latency_on() {
		return;
	}
	let samples = crate::locks::lock(&SAMPLES);
	if samples.is_empty() {
		eprintln!("[latency] nothing timed - a keystroke needs an echo to measure against");
		return;
	}
	let mean = |leg: fn(&(u32, u32, u32)) -> u32| {
		samples.iter().map(|s| u64::from(leg(s))).sum::<u64>() as f64 / samples.len() as f64 / 1e3
	};
	let mut totals: Vec<u32> = samples.iter().map(|s| s.0 + s.1 + s.2).collect();
	totals.sort_unstable();
	let (median, p95) = percentiles(&totals);
	#[allow(clippy::unwrap_used, reason = "`samples` is not empty, checked above")]
	let worst = *totals.last().unwrap();
	let ms = |us: u32| f64::from(us) / 1e3;
	eprintln!(
		"[latency] {} keystrokes: median {:.2}ms  p95 {:.2}ms  worst {:.2}ms",
		samples.len(),
		ms(median),
		ms(p95),
		ms(worst),
	);
	eprintln!(
		"[latency] mean legs: send {:.2}ms  echo {:.2}ms  draw {:.2}ms",
		mean(|s| s.0),
		mean(|s| s.1),
		mean(|s| s.2),
	);
}

#[cfg(test)]
mod tests {
	use super::percentiles;

	// One sample is the whole distribution, and 95% of one still has to fall on
	// it rather than one past the end.
	// Test ID: EoTIJ8i
	#[test]
	fn percentiles_never_step_past_the_last_sample() {
		assert_eq!(percentiles(&[7]), (7, 7));
		assert_eq!(percentiles(&[1, 2]), (2, 2));
		let hundred: Vec<u32> = (1..=100).collect();
		assert_eq!(percentiles(&hundred), (51, 96));
	}
}
