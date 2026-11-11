// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! When an idle or hidden window lets its GPU device go, and how it gets it
//! back. A hidden window also builds no frames until it is shown again.

use super::{EnvFlag, FREEZE_MINIMIZED, Gpu, State, VRAM_CHECK_IVL, env_flag};
use crate::bgimage::ImageRenderer;
use crate::config;
use crate::gfx::{Gfx, RectRenderer, Retry};
use std::time::{Duration, Instant};
use winit::event::WindowEvent;

/// When the window last saw a person or a shell: input, focus either way, or
/// output while it could be seen. The idle release counts from `since` (see
/// `release_deadline`). `wake_owed` says something wants the window back since
/// it let its device go, so the device is owed as soon as there is a screen to
/// draw on.
#[derive(Debug)]
pub(super) struct IdleClock {
	since: Instant,
	wake_owed: bool,
	// owed only to answer the desktop's repaint, and let go again once drawn
	repaint: bool,
	// a rebuild the GPU refused, tried again on this
	retry: Retry,
}

impl IdleClock {
	pub(super) fn new() -> Self {
		IdleClock {
			since: Instant::now(),
			wake_owed: false,
			repaint: false,
			retry: Retry::default(),
		}
	}

	/// The device back now: it is owed, there is a screen to draw on, and no
	/// refused rebuild is waiting out its backoff.
	pub(super) fn rebuild_due(&self, hidden: bool, now: Instant) -> bool {
		self.wake_owed && !hidden && self.retry.at.is_none_or(|at| now >= at)
	}

	// A busy or full GPU can refuse the device, and only input used to try
	// again, so a window left alone stayed blank after the load was gone. It
	// stays owed and is tried again on a backoff instead. True on the first
	// refusal of a run.
	fn rebuild_failed(&mut self, now: Instant) -> bool {
		self.wake_owed = true;
		self.retry
			.missed(now, REBUILD_RETRY_FIRST, REBUILD_RETRY_MAX);
		self.retry.misses == 1
	}

	fn rebuilt(&mut self) {
		self.wake_owed = false;
		self.retry = Retry::default();
	}

	/// When the loop has to wake for a refused rebuild.
	pub(super) fn rebuild_wake(&self, hidden: bool) -> Option<Instant> {
		self.retry.at.filter(|_| self.wake_owed && !hidden)
	}

	// A sign of life. True when it is the one that makes the device owed.
	fn active(&mut self, released: bool) -> bool {
		self.since = Instant::now();
		self.repaint = false;
		self.owe(released)
	}

	// The pointer moving over the window, with nothing pressed. It starts the
	// wait over while the device is held, since someone may be reading along.
	// It doesn't take a released device back, nor make a repaint's one frame
	// a whole wake: on a desktop that doesn't focus on hover, a pointer that
	// crosses a window behind others is on its way somewhere else, and the
	// last frame is still up. A click, a wheel, a key, or focus wakes it.
	// True when the wait started over.
	fn pointer(&mut self, released: bool) -> bool {
		if released || self.repaint {
			return false;
		}
		self.since = Instant::now();
		true
	}

	// The desktop wants pixels, with nobody known to be there: X11 drops a
	// window's pixels while it is on another desktop. The device is owed for
	// one frame and the wait isn't started over. Owed already, as for output
	// at the reveal, it stays a whole wake. True when this makes it owed.
	fn repaint(&mut self, released: bool) -> bool {
		let newly = self.owe(released);
		self.repaint |= newly;
		newly
	}

	pub(super) fn owe(&mut self, released: bool) -> bool {
		let newly = released && !self.wake_owed;
		self.wake_owed |= released;
		newly
	}

	// A shell printing. Output nobody can see doesn't keep a hidden window's
	// device, or a program that prints forever would hold it for good. Output
	// in the tab in front is still owed at the reveal, so the window shows
	// what changed once it can be seen. A tab in the back is out of sight
	// either way, and nothing it prints changes the picture, so it isn't owed
	// the device: that would take it back and let it go again once per wait
	// for as long as the tab prints. Switching to the tab is input, which
	// brings it back.
	fn output(&mut self, released: bool, hidden: bool, in_front: bool) -> bool {
		if !in_front {
			false
		} else if hidden {
			self.owe(released)
		} else {
			self.active(released)
		}
	}
}

// A refused rebuild waits longer than a refused frame (`Retry`), since each
// try is a whole device and maybe an adapter.
const REBUILD_RETRY_FIRST: Duration = Duration::from_millis(250);
const REBUILD_RETRY_MAX: Duration = Duration::from_secs(5);

/// What the window title says about the device. Nothing normally, a note while
/// it is let go, another while it comes back, and a last one for a few seconds
/// after. The wallpaper is the last thing a rebuild waits on, and the only part
/// slow enough for anyone to see, so coming back lasts until it arrives.
/// Any rebuild counts, a return to this console as much as the idle release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Conserve {
	Off,
	Saving,
	Restoring,
	Restored(Instant),
}

const RESTORED_SHOWN: Duration = Duration::from_secs(5);

impl Conserve {
	pub(super) fn note(self, now: Instant) -> Option<&'static str> {
		match self {
			Conserve::Off => None,
			Conserve::Saving => Some("resource conservation mode"),
			Conserve::Restoring => Some("restoring resources ..."),
			Conserve::Restored(at) => (now < at + RESTORED_SHOWN).then_some("resources restored"),
		}
	}

	/// When the title next changes on its own.
	pub(super) fn wake(self) -> Option<Instant> {
		match self {
			Conserve::Restored(at) => Some(at + RESTORED_SHOWN),
			_ => None,
		}
	}

	pub(super) fn wallpaper_answered(&mut self, now: Instant) {
		if *self == Conserve::Restoring {
			*self = Conserve::Restored(now);
		}
	}
}

// What the idle release reads of the window (see `release_deadline`).
struct Idle {
	focused: bool,
	hidden: bool,        // minimized, or covered where the desktop says so
	revealed: bool,      // shown at all yet
	bench_busy: bool,    // a rating owed or running
	since: Instant,      // the last sign of life
	keeps_picture: bool, // still shows its last frame once let go
}

// When an idle window may let its device go, or None while something keeps
// it: the switch off, the window not yet shown, a rating due or running, or a
// window on screen that has focus or would go blank without its device. There
// are 2 waits, because a hidden window is known to be out of sight while a
// merely unfocused one may be on a second monitor being read.
fn release_deadline(cfg: &config::Settings, idle: &Idle) -> Option<Instant> {
	let rule = idle_rule(cfg);
	if kept_by(&rule, idle).is_some() {
		return None;
	}
	let wait = if idle.hidden {
		rule.hidden
	} else {
		rule.otherwise
	};
	Some(idle.since + wait)
}

// What keeps the device, in words for SILK_IDLEDBG, or None when nothing does.
fn kept_by(rule: &IdleRule, idle: &Idle) -> Option<&'static str> {
	if !rule.on {
		Some("switched off")
	} else if !idle.revealed {
		Some("not shown yet")
	} else if idle.bench_busy {
		Some("a rating is owed or running")
	} else if !idle.hidden && idle.focused {
		Some("focused on screen")
	} else if !idle.hidden && !idle.keeps_picture {
		Some("on screen, and would go blank")
	} else {
		None
	}
}

#[derive(Debug, PartialEq, Eq)]
struct IdleRule {
	on: bool,
	hidden: Duration,
	otherwise: Duration,
}

// The setting's answer, unless SILK_IDLE_SECS names one wait in seconds for
// every case - which is how the release is exercised without leaving a window
// alone for half an hour.
fn idle_rule(cfg: &config::Settings) -> IdleRule {
	if let Some(wait) = idle_secs() {
		return IdleRule {
			on: true,
			hidden: wait,
			otherwise: wait,
		};
	}
	let minutes = |m: usize| Duration::from_secs(m as u64 * 60);
	IdleRule {
		on: cfg.idle_release,
		hidden: minutes(cfg.idle_release_hidden_min),
		otherwise: minutes(cfg.idle_release_min),
	}
}

// SILK_IDLE_SECS, read once, since every loop pass asks for the idle rule.
fn idle_secs() -> Option<Duration> {
	use std::sync::OnceLock;
	static WAIT: OnceLock<Option<Duration>> = OnceLock::new();
	*WAIT.get_or_init(|| {
		std::env::var("SILK_IDLE_SECS")
			.ok()
			.and_then(|raw| raw.parse::<f32>().ok())
			.filter(|secs| secs.is_finite() && *secs >= 0.0)
			.map(Duration::from_secs_f32)
	})
}

/// `SILK_IDLEDBG`: what restarts the wait and what holds the device, so a log
/// from a real desktop shows why a window never let go, and what a released
/// window heard without waking. A sign of life comes often, so each kind is
/// said at most once per `TRACE_QUIET`, with a count of the ones not said. A
/// hold is said when it changes.
#[derive(Debug, Default)]
pub(super) struct IdleTrace {
	said: Vec<(&'static str, &'static str, Instant, u32)>,
	hold: Option<Option<&'static str>>,
}

const TRACE_QUIET: Duration = Duration::from_secs(30);

impl IdleTrace {
	fn restart(&mut self, why: &'static str) {
		// only while the wait runs: with focus, every key would be one
		if self.hold == Some(None) {
			self.say("wait restarted", why);
		}
	}

	// heard by a released window, and left released
	fn not_a_wake(&mut self, why: &'static str) {
		self.say("not a wake", why);
	}

	fn say(&mut self, what: &'static str, why: &'static str) {
		if !env_flag(EnvFlag::IdleDbg) {
			return;
		}
		let now = Instant::now();
		match self
			.said
			.iter_mut()
			.find(|(t, w, ..)| (*t, *w) == (what, why))
		{
			Some((.., at, quiet)) if now.duration_since(*at) < TRACE_QUIET => *quiet += 1,
			Some((.., at, quiet)) => {
				idledbg(&format!("{what}: {why} (+{quiet} not said)"));
				(*at, *quiet) = (now, 0);
			}
			None => {
				idledbg(&format!("{what}: {why}"));
				self.said.push((what, why, now, 0));
			}
		}
	}

	fn hold(&mut self, hold: Option<&'static str>, wait: (&str, Duration)) {
		if self.hold == Some(hold) {
			return;
		}
		self.hold = Some(hold);
		match hold {
			Some(why) => idledbg(&format!("kept: {why}")),
			None => idledbg(&format!("waiting {}s ({})", wait.1.as_secs(), wait.0)),
		}
	}
}

/// `SILK_IDLEDBG=1`: the idle release's comings and goings on stderr, stamped
/// with seconds since the first call so a log can be read against a timeline.
pub(super) fn idledbg(msg: &str) {
	use std::sync::OnceLock;
	static T0: OnceLock<Instant> = OnceLock::new();
	if !env_flag(EnvFlag::IdleDbg) {
		return;
	}
	let t = T0.get_or_init(Instant::now).elapsed().as_secs_f32();
	eprintln!("[idle {t:7.2}s] {msg}");
}

// Hand freed heap back to the OS. glibc keeps what it was given unless asked,
// so a release that dropped tens of MB would still show them as resident.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_heap() {
	// SAFETY: no arguments to get wrong, and it only touches the allocator's
	// own free lists.
	unsafe {
		libc::malloc_trim(0);
	}
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_heap() {}

/// A big buffer comes straight from the OS and goes straight back. glibc's
/// mmap threshold starts at 128 KB but moves: freeing a mapped buffer raises it
/// to that buffer's size, after which a wallpaper's decode (several buffers of
/// megabytes each) is carved out of the thread's own arena and stays resident
/// there once freed, because `malloc_trim` never shrinks an arena that isn't
/// the main one. Measured on a 1920x993 wallpaper: about 40 MB kept per decode,
/// one per rebuild after an idle release, and the first decode's 50 MB kept for
/// the life of every window. Setting the threshold pins it. 4 MB keeps a
/// frame's own vectors in the arena on any grid and puts only the image buffers
/// on the mapping path. Pinning it also stops the trim threshold moving, so
/// that is set too, high enough that the main heap's top isn't given back and
/// asked for again around every frame. Called before the first thread exists,
/// like the environment fixes.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn tune_heap() {
	// SAFETY: plain allocator parameters, set before any other thread runs.
	unsafe {
		libc::mallopt(libc::M_MMAP_THRESHOLD, 4 << 20);
		libc::mallopt(libc::M_TRIM_THRESHOLD, 8 << 20);
	}
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(crate) fn tune_heap() {}

// What a render entry point does, given whether the window was hidden at the
// last check and is now (see `State::freeze_sync`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
	Skip,    // nothing on screen: a frame now would bank the backlog into the ease
	CatchUp, // just shown again: one hard-cut frame
	Draw,
}

fn freeze_frame(was_hidden: bool, hidden: bool) -> Frame {
	if hidden {
		Frame::Skip
	} else if was_hidden {
		Frame::CatchUp
	} else {
		Frame::Draw
	}
}

// Nothing of the window is on screen. A window with no area counts, since on
// Windows a restore stops answering minimized a moment before its size comes
// back, and a device rebuilt then took 1x1 as the window's size. The grid
// shrank to 2 columns with it, and the console host's reflow lost the screen.
fn window_hidden(
	revealed: bool,
	occluded: bool,
	no_area: bool,
	minimized: impl FnOnce() -> bool,
) -> bool {
	revealed && (occluded || no_area || minimized())
}

// How long a minimized answer stands. On X11 the answer is a property read
// the loop waits on, and winit reports no event when a window is minimized.
// Events that come with a restore forget the answer (`restore_sign`), so a
// reveal isn't held up by it.
const MINIMIZED_RECHECK: Duration = Duration::from_millis(250);

/// An event a restore brings. The WM's redraw is one, and it gets to
/// `freeze_sync` before `about_to_wait` does (G89), so it must see the
/// window as shown, not a minimized answer from before.
pub(super) fn restore_sign(event: &WindowEvent) -> bool {
	matches!(
		event,
		WindowEvent::Focused(_)
			| WindowEvent::Occluded(_)
			| WindowEvent::Resized(_)
			| WindowEvent::RedrawRequested
	)
}

/// Whether an event is a person at the window, for the idle clock. The pointer
/// counts once it is over the window by the rule the look after a move uses
/// (`note_presence`), not on being told it entered: on X11 a pointer grab let
/// go by any program on the desktop, such as a menu, a window drag, or a screen
/// locker, tells the window under a pointer at rest that it entered, with one
/// move to where it already was. That kept the wait from ever running out
/// there, and took a released device back for nobody.
///
/// The pointer moving over the window is `POINTER`, which only starts the wait
/// over and never takes a released device back (`State::note_pointer`).
///
/// Focus counts only as a change. On X11, coming back to the desktop that
/// has an unfocused window tells it it lost focus, which it never had.
/// `focused` is the window's focus before this event. The answer names the
/// sign for `SILK_IDLEDBG`.
pub(super) fn sign_of_life(
	event: &WindowEvent,
	pointer_over: bool,
	focused: bool,
) -> Option<&'static str> {
	match event {
		WindowEvent::CursorMoved { .. } | WindowEvent::CursorEntered { .. } => {
			pointer_over.then_some(POINTER)
		}
		WindowEvent::Focused(now) => (*now != focused).then_some("focus"),
		WindowEvent::KeyboardInput { .. }
		| WindowEvent::MouseInput { .. }
		| WindowEvent::MouseWheel { .. }
		| WindowEvent::Touch(_)
		| WindowEvent::Ime(_) => Some("input"),
		_ => None,
	}
}

/// What `sign_of_life` calls the pointer moving over the window.
pub(super) const POINTER: &str = "pointer";

/// The window's minimized state as last asked, so a pass asks at most once per
/// `MINIMIZED_RECHECK` rather than every time.
#[derive(Debug, Default)]
pub(super) struct MinimizedProbe {
	answer: bool,
	asked: Option<Instant>,
}

impl MinimizedProbe {
	fn get(&mut self, now: Instant, ask: impl FnOnce() -> bool) -> bool {
		let stale = self
			.asked
			.is_none_or(|at| now.saturating_duration_since(at) >= MINIMIZED_RECHECK);
		if stale {
			self.answer = ask();
			self.asked = Some(now);
		}
		self.answer
	}

	pub(super) fn forget(&mut self) {
		self.asked = None;
	}
}

impl State {
	// Whether any of this window is on screen. Both render entry points check
	// it - a frame built while hidden would bank the whole buffered backlog
	// into the output ease, and the reveal would then play it back as if it
	// had just arrived.
	fn window_hidden(&mut self) -> bool {
		let window = &self.window;
		let minimized = &mut self.minimized;
		window_hidden(self.revealed, self.occluded, self.no_area, || {
			FREEZE_MINIMIZED
				&& minimized.get(Instant::now(), || window.is_minimized().unwrap_or(false))
		})
	}

	/// The freeze edge, owned in one place because both render entry points call
	/// it: on restore the WM's own redraw arrives before `about_to_wait` runs, so
	/// whichever gets here first has to be the one that catches up - otherwise that
	/// frame banks the whole backlog into the ease before anything cuts it.
	pub(super) fn freeze_sync(&mut self) -> bool {
		let hidden = self.window_hidden();
		let frame = freeze_frame(self.was_hidden, hidden);
		if frame == Frame::CatchUp {
			self.freeze_catchup();
			self.note_shown();
		}
		self.was_hidden = hidden;
		frame == Frame::Skip
	}

	/// A frozen surface coming back on screen: hidden tabs never build, and a
	/// minimized/occluded window builds nothing - so the reveal is one dirty
	/// catch-up frame, hard-cut so the gap closes instantly instead of easing in
	/// (that ease is the bounce class, and it also reads as output arriving now).
	/// Every pane is cut, not only the ones flagged dirty: the flag is cleared by
	/// whichever build got there first, so it answers "is a rebuild owed", not
	/// "did the grid move while nobody was looking". A pane that did sit
	/// still is snapping a scroll already at rest.
	pub(super) fn freeze_catchup(&mut self) {
		for pane in self.tabs.cur_mut().panes.values_mut() {
			pane.hard_cut();
		}
		self.dirty = true;
	}

	/// A sign of life: the idle clock starts over, and a window that let its
	/// device go is owed it back.
	pub(super) fn note_active(&mut self, why: &'static str) {
		if self.idle.active(self.gpu.is_none()) {
			idledbg(&format!("wake: {why}"));
		} else {
			self.idle_trace.restart(why);
		}
	}

	/// The pointer moving over the window (`IdleClock::pointer`).
	pub(super) fn note_pointer(&mut self) {
		if self.idle.pointer(self.gpu.is_none()) {
			self.idle_trace.restart(POINTER);
		} else if self.gpu.is_none() {
			self.idle_trace.not_a_wake(POINTER);
		}
	}

	/// The desktop asks a window that let its device go for pixels. One that
	/// keeps its picture takes the device back only for that frame
	/// (`repaint_done`). One that goes blank without it wakes for good.
	pub(super) fn note_repaint(&mut self) {
		if !self.keeps_picture {
			self.note_active("redraw request");
		} else if self.idle.repaint(self.gpu.is_none()) {
			idledbg("wake: redraw request, for one frame");
		}
	}

	/// A device taken back only to repaint has drawn its frame with the
	/// wallpaper, and can go again. The wallpaper is the last thing a rebuild
	/// waits on, and a frame drawn before it would leave the stand-in up.
	pub(super) fn repaint_done(&self) -> bool {
		self.idle.repaint
			&& self.gpu.is_some()
			&& matches!(self.conserve, Conserve::Restored(_))
			&& !self.dirty
			&& self.frame_retry.at.is_none()
	}

	/// Shown again after being out of sight. Nobody is known to be there, so
	/// the wait isn't started over, and a window that let its device go keeps
	/// its last frame up until something on screen changes (output it was owed
	/// while hidden is owed now). A window that goes blank without its device
	/// takes it back. Windows sends no occlusion events, and its repaint on a
	/// restore can come before the size does, so this is called from the
	/// catch-up edge as well as on an occlusion event.
	pub(super) fn note_shown(&mut self) {
		if !self.keeps_picture {
			self.note_active("shown");
		}
	}

	/// Output from `pane`, which counts only while it can be seen: the window
	/// shown and the pane in the tab in front (`IdleClock::output`). The hidden
	/// flag is the one the last pass settled on.
	pub(super) fn note_output(&mut self, pane: crate::term::PaneId) {
		let in_front = self.tabs.cur().panes.contains_key(&pane);
		let shown = !self.was_hidden && in_front;
		if self
			.idle
			.output(self.gpu.is_none(), self.was_hidden, in_front)
		{
			idledbg("wake: output");
		} else if shown {
			self.idle_trace.restart("output");
		}
	}

	/// `SILK_IDLEDBG`: say what holds a window with its device, each time that
	/// changes. `dialog_up` is the hold the loop owns.
	pub(super) fn trace_hold(&mut self, cfg: &config::Settings, dialog_up: bool) {
		if self.gpu.is_none() {
			return;
		}
		let idle = self.idle_view();
		let rule = idle_rule(cfg);
		let hold = if dialog_up {
			Some("a dialog is up")
		} else {
			kept_by(&rule, &idle)
		};
		let wait = if idle.hidden {
			("hidden", rule.hidden)
		} else {
			("otherwise", rule.otherwise)
		};
		self.idle_trace.hold(hold, wait);
	}

	fn idle_view(&self) -> Idle {
		Idle {
			focused: self.focused,
			hidden: self.was_hidden,
			revealed: self.revealed,
			bench_busy: self.bench.is_some() || self.bench_at.is_some(),
			since: self.idle.since,
			keeps_picture: self.keeps_picture,
		}
	}

	/// Reads the hidden answer the pass's `freeze_sync` settled on.
	pub(super) fn release_deadline(&self, cfg: &config::Settings) -> Option<Instant> {
		release_deadline(cfg, &self.idle_view())
	}

	/// Let the device and everything on it go. The window stays, the shells run
	/// on, and the grid keeps up; only drawing stops, and `rebuild_gpu` is the
	/// way back. What the CPU held only for the device's sake goes too: the
	/// rasterized glyphs, the shaped chrome.
	pub(super) fn release_gpu(&mut self) {
		let Some(gpu) = self.gpu.take() else {
			return;
		};
		let start = Instant::now();
		self.text.detach_gpu();
		self.chrome = None;
		self.invalidate_prepared();
		self.rebirth = Some(gpu.release());
		self.idle.rebuilt();
		self.idle.repaint = false;
		self.frame_retry = Retry::default();
		// no frame draws while released, so nothing else would update the title
		self.conserve = Conserve::Saving;
		self.update_title();
		trim_heap();
		idledbg(&format!("device released in {:?}", start.elapsed()));
	}

	/// The device again, on the same window, and everything that was on it
	/// built afresh. The wallpaper is prepared again from the file rather than
	/// kept, as after a VT switch (`recover_gpu`), and its small stand-in shows
	/// from the first frame until it arrives. A failure leaves the window
	/// released and owed, and it is tried again on a backoff while the window
	/// shows.
	pub(super) fn rebuild_gpu(&mut self) {
		let Some(rebirth) = self.rebirth.as_ref() else {
			return;
		};
		let start = Instant::now();
		let gfx = match Gfx::rebuild(rebirth, &self.window, crate::gfx::wanted()) {
			Ok(gfx) => gfx,
			Err(e) => {
				if self.idle.rebuild_failed(Instant::now()) {
					eprintln!(
						"{}: couldn't bring the GPU device back ({e}); trying again",
						config::APP_NAME
					);
				}
				idledbg(&format!("rebuild refused: {e}"));
				return;
			}
		};
		self.rebirth = None;
		let (w, h) = (gfx.config.width, gfx.config.height);
		self.surface_px = (w, h);
		self.adapter_info = gfx.adapter_info.clone();
		let drawn = gfx.drawn;
		self.text.attach_gpu(&gfx.device, &gfx.queue, gfx.format);
		let rects = RectRenderer::new(&gfx.device, gfx.format);
		let minimap = crate::minimap::MapRenderer::new(&gfx.device, gfx.format);
		let scrim = crate::scrim::Scrim::new(&gfx.device, gfx.format, w, h);
		let wallpaper_img = self.wp_standin.as_ref().map(|small| {
			ImageRenderer::new(&gfx.device, &gfx.queue, gfx.format, small).standing_in()
		});
		self.gpu = Some(Gpu {
			gfx,
			rects,
			minimap,
			wallpaper_img,
			scrim,
		});
		self.idle.rebuilt();
		// back only to repaint: nobody is known to be there, so the wait runs on
		if !self.idle.repaint {
			self.idle.since = Instant::now();
		}
		self.vram_next = Instant::now() + VRAM_CHECK_IVL;
		// the window may have been resized while there was no surface to follow
		self.relayout_all();
		self.request_wallpaper(false);
		self.conserve = Conserve::Restoring;
		self.update_title();
		// the grid moved while nothing drew: one hard-cut catch-up frame
		self.freeze_catchup();
		idledbg(&format!(
			"device rebuilt in {:?} at {w}x{h}, {drawn:?}",
			start.elapsed()
		));
		self.follow_renderer(drawn);
	}

	/// Everything on the device again, after a return to this console. The whole
	/// device when nothing else shares it, since a switch can spoil any texture
	/// and `recover_gpu` only knows the text and the wallpaper. An open dialog's
	/// context can't outlive the terminal's on X11, so that case stays partial.
	pub(super) fn heal_gpu(&mut self, dialog_open: bool) {
		if self.gpu.is_none() {
			return; // a released window rebuilds from nothing anyway
		}
		if dialog_open {
			self.recover_gpu();
			return;
		}
		self.release_gpu();
		self.rebuild_gpu();
	}

	/// GPU texture contents were lost (VT switch / suspend; see the Sentinel note
	/// in gfx.rs). Re-upload everything that was uploaded once: fresh glyph
	/// atlases + chrome via `rebuild_text`, and the wallpaper. `rebuild_text` also drops
	/// the prepared/scrim signatures, so the next frame rebuilds the scrim source
	/// instead of reusing a texture that's now empty.
	pub(super) fn recover_gpu(&mut self) {
		if self.gpu.is_none() {
			return; // nothing uploaded to lose; the rebuild starts from nothing anyway
		}
		self.rebuild_text(config::display_scale(self.window.scale_factor()));
		// re-decoded rather than kept resident: a large wallpaper is tens of MB, and
		// a VT switch is rare enough not to trade that for a moment without one
		self.request_wallpaper(false);
		self.conserve = Conserve::Restoring;
		self.update_title();
		self.dirty = true;
	}
}

#[cfg(test)]
mod tests {
	use super::{
		Conserve, Idle, IdleClock, IdleRule, RESTORED_SHOWN, release_deadline, window_hidden,
	};
	use crate::config;
	use crate::gfx::Retry;
	use std::time::{Duration, Instant};
	// A pointer at rest told it entered, as a grab let go elsewhere on the
	// desktop does on X11, is nobody at the window. Moving inside it is, and so
	// is anything pressed or a change of focus.
	// Test ID: deyts
	#[test]
	fn a_pointer_only_told_it_entered_is_not_a_sign_of_life() {
		use super::sign_of_life;
		use winit::event::{DeviceId, WindowEvent};
		let device_id = DeviceId::dummy();
		let entered = WindowEvent::CursorEntered { device_id };
		let moved = WindowEvent::CursorMoved {
			device_id,
			position: winit::dpi::PhysicalPosition::new(10.0, 10.0),
		};
		let life = |e: &WindowEvent, over: bool| sign_of_life(e, over, false).is_some();
		assert!(!life(&entered, false));
		assert!(!life(&moved, false));
		assert!(life(&moved, true));
		// Wayland's compositor knows its pointer, so an enter there is over
		assert!(life(&entered, true));
		assert!(!life(&WindowEvent::CursorLeft { device_id }, true));
		assert!(life(
			&WindowEvent::MouseInput {
				device_id,
				state: winit::event::ElementState::Pressed,
				button: winit::event::MouseButton::Left,
			},
			false
		));
		assert!(!life(&WindowEvent::RedrawRequested, true));
	}

	// Focus counts only as a change. Coming back to the desktop that has an
	// unfocused window on X11 tells it it lost focus, which took a released
	// device back with nobody there (2026111010491300).
	// Test ID: df4dh
	#[test]
	fn focus_is_a_sign_of_life_only_as_a_change() {
		use super::sign_of_life;
		use winit::event::WindowEvent;
		assert_eq!(
			sign_of_life(&WindowEvent::Focused(true), false, false),
			Some("focus")
		);
		assert_eq!(
			sign_of_life(&WindowEvent::Focused(false), false, true),
			Some("focus")
		);
		assert_eq!(
			sign_of_life(&WindowEvent::Focused(false), false, false),
			None
		);
		assert_eq!(sign_of_life(&WindowEvent::Focused(true), false, true), None);
	}

	// A released window is owed its device at the reveal only for output it
	// can show: the tab in front. A tab in the back printing while the window
	// was hidden changes nothing anyone will see.
	// Test ID: df4dj
	#[test]
	fn hidden_output_is_owed_only_from_the_tab_in_front() {
		let mut clock = IdleClock::new();
		assert!(!clock.output(true, true, false), "a tab in the back");
		assert!(!clock.wake_owed);
		assert!(clock.output(true, true, true), "the tab in front");
		assert!(clock.wake_owed);
	}

	// The desktop's repaint takes a released device back for one frame, with
	// the wait left running, so it can be let go again once drawn. A sign of
	// life in that time makes it a whole wake, and so does a device already
	// owed, as for output while the window was hidden.
	// Test ID: df4e2
	#[test]
	fn a_repaint_takes_the_device_back_for_one_frame() {
		let long_ago = Instant::now()
			.checked_sub(Duration::from_secs(1))
			.expect("a second of uptime");
		let mut clock = IdleClock::new();
		clock.since = long_ago;
		assert!(clock.repaint(true));
		assert!(clock.wake_owed && clock.repaint);
		assert_eq!(clock.since, long_ago, "a repaint started the wait over");
		clock.active(true);
		assert!(!clock.repaint, "a sign of life during it");
		let mut owed = IdleClock::new();
		owed.output(true, true, true);
		assert!(!owed.repaint(true));
		assert!(!owed.repaint, "owed for output already");
		let mut kept = IdleClock::new();
		assert!(!kept.repaint(false));
		assert!(
			!kept.wake_owed && !kept.repaint,
			"a window that has its device"
		);
	}

	// The pointer moving over a released window, partly behind others and
	// never focused, took its device back for nobody (2026111009560100). It
	// starts the wait over only while the device is held, and not during a
	// repaint's one frame, which would make that a whole wake.
	// Test ID: df4qc
	#[test]
	fn the_pointer_alone_never_takes_the_device_back() {
		let long_ago = Instant::now()
			.checked_sub(Duration::from_secs(1))
			.expect("a second of uptime");
		let mut released = IdleClock::new();
		released.since = long_ago;
		assert!(!released.pointer(true));
		assert!(!released.wake_owed, "a released window woke on the pointer");
		assert_eq!(released.since, long_ago);
		let mut held = IdleClock::new();
		held.since = long_ago;
		assert!(held.pointer(false));
		assert!(held.since > long_ago, "the wait didn't start over");
		assert!(!held.wake_owed);
		let mut drawing = IdleClock::new();
		drawing.since = long_ago;
		assert!(drawing.repaint(true));
		drawing.wake_owed = false; // the frame's device is back
		assert!(!drawing.pointer(false));
		assert!(drawing.repaint, "the pointer made a repaint a whole wake");
		assert_eq!(drawing.since, long_ago);
	}

	// SILK_IDLE_SECS is read once per process. The idle rule is asked twice on
	// every loop pass, and it went to the environment each time. Run in a child
	// copy of this test binary, since the answer is cached per process.
	// Test ID: ddf7w
	#[test]
	#[cfg_attr(miri, ignore = "Miri can't start a process")]
	fn the_idle_wait_switch_is_read_once() {
		let out = std::process::Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"app::idle::tests::idle_wait_child",
				"--nocapture",
			])
			.env("SILK_IDLE_WAIT_CHILD", "1")
			.env("SILK_IDLE_SECS", "5")
			.output()
			.unwrap();
		let text = String::from_utf8_lossy(&out.stdout);
		assert!(
			out.status.success(),
			"{text}{}",
			String::from_utf8_lossy(&out.stderr)
		);
		assert!(text.contains("idle wait checked"), "{text}");
	}

	// Test ID: ddf7x
	#[test]
	fn idle_wait_child() {
		if std::env::var_os("SILK_IDLE_WAIT_CHILD").is_none() {
			return; // only does anything when the test above starts it
		}
		let cfg = config::Settings::default();
		let five = Duration::from_secs(5);
		let every_wait_five = IdleRule {
			on: true,
			hidden: five,
			otherwise: five,
		};
		assert_eq!(super::idle_rule(&cfg), every_wait_five);
		// SAFETY: this child runs this one test, so no other thread is reading
		// the environment.
		unsafe { std::env::set_var("SILK_IDLE_SECS", "9") };
		assert_eq!(
			super::idle_rule(&cfg),
			every_wait_five,
			"read again after the first pass"
		);
		println!("idle wait checked");
	}

	// The minimized state is a round trip to the X server, and a pass at the
	// frame rate asked it every time. A second at 60 passes now asks at most
	// once per MINIMIZED_RECHECK, and an event that forgets the answer gets a
	// fresh one on the next pass.
	// Test ID: ddf7y
	#[test]
	fn the_minimized_state_is_asked_at_most_once_per_recheck() {
		use super::{MINIMIZED_RECHECK, MinimizedProbe};
		use winit::event::WindowEvent;
		let mut probe = MinimizedProbe::default();
		let start = Instant::now();
		let mut asks = 0;
		for pass in 0..60u32 {
			let now = start + Duration::from_secs(1) * pass / 60;
			// minimized a third of the way in, with nothing reported
			let minimized = pass >= 20;
			let seen = probe.get(now, || {
				asks += 1;
				minimized
			});
			if now >= start + Duration::from_millis(333) + MINIMIZED_RECHECK {
				assert!(seen, "still not seen at pass {pass}");
			}
		}
		let most = Duration::from_secs(1)
			.div_duration_f32(MINIMIZED_RECHECK)
			.ceil() as u32;
		assert!(asks <= most, "{asks} asks in a second, at most {most}");
		// a restore forgets the answer, so the next pass sees it at once
		assert!(super::restore_sign(&WindowEvent::RedrawRequested));
		assert!(super::restore_sign(&WindowEvent::Occluded(false)));
		assert!(super::restore_sign(&WindowEvent::Focused(true)));
		assert!(super::restore_sign(&WindowEvent::Resized(
			winit::dpi::PhysicalSize::new(640, 480)
		)));
		assert!(!super::restore_sign(&WindowEvent::Moved(
			winit::dpi::PhysicalPosition::new(0, 0)
		)));
		probe.forget();
		let after = start + Duration::from_secs(1);
		assert!(!probe.get(after, || false));
		assert!(
			!probe.get(after, || panic!("asked again straight after")),
			"the answer stands until the recheck"
		);
	}

	// The idle release: off by default, never while the window has focus on
	// screen, and a covered window waits the shorter of the 2 times. Everything
	// that vetoes it is a None, since a deadline that then had to be checked
	// again elsewhere is how a veto gets forgotten.
	// Test ID: dcr6f
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn the_idle_release_waits_on_the_window_and_only_an_unwatched_one() {
		let since = Instant::now();
		let idle = |focused, hidden| Idle {
			focused,
			hidden,
			revealed: true,
			bench_busy: false,
			since,
			keeps_picture: true,
		};
		// The release was off until 2026100312470540 turned it on by default,
		// which `idle_release_defaults_on` pins now.
		// let mut cfg = config::Settings::default();
		// assert!(
		// 	release_deadline(&cfg, &idle(false, true)).is_none(),
		// 	"off by default"
		// );
		let mut cfg = config::Settings {
			idle_release: false,
			..config::Settings::default()
		};
		assert!(
			release_deadline(&cfg, &idle(false, true)).is_none(),
			"switched off"
		);
		cfg.idle_release = true;
		cfg.idle_release_hidden_min = 30;
		cfg.idle_release_min = 240;
		assert!(
			release_deadline(&cfg, &idle(true, false)).is_none(),
			"focused and on screen"
		);
		assert_eq!(
			release_deadline(&cfg, &idle(false, false)),
			Some(since + Duration::from_hours(4))
		);
		assert_eq!(
			release_deadline(&cfg, &idle(false, true)),
			Some(since + Duration::from_mins(30))
		);
		// minimized or covered with focus still nominally on it: out of sight is
		// what counts
		assert_eq!(
			release_deadline(&cfg, &idle(true, true)),
			Some(since + Duration::from_mins(30))
		);
		let mut owed = idle(false, true);
		owed.bench_busy = true;
		assert!(
			release_deadline(&cfg, &owed).is_none(),
			"a rating in flight"
		);
		let mut unshown = idle(false, true);
		unshown.revealed = false;
		assert!(
			release_deadline(&cfg, &unshown).is_none(),
			"not on screen yet"
		);
	}

	// The idle trace names the one thing that keeps the device, in the order
	// the release checks them, and nothing once the wait runs. The names are
	// what a log from a real desktop is read by, so a new veto needs one.
	// Test ID: df4cn
	#[test]
	fn each_hold_on_the_device_names_itself() {
		use super::kept_by;
		let rule = IdleRule {
			on: true,
			hidden: Duration::from_mins(1),
			otherwise: Duration::from_mins(5),
		};
		let free = Idle {
			focused: false,
			hidden: false,
			revealed: true,
			bench_busy: false,
			since: Instant::now(),
			keeps_picture: true,
		};
		assert_eq!(kept_by(&rule, &free), None);
		let off = IdleRule { on: false, ..rule };
		assert_eq!(kept_by(&off, &free), Some("switched off"));
		let unshown = Idle {
			revealed: false,
			..free
		};
		assert_eq!(kept_by(&rule, &unshown), Some("not shown yet"));
		let rating = Idle {
			bench_busy: true,
			..free
		};
		assert_eq!(kept_by(&rule, &rating), Some("a rating is owed or running"));
		let focused = Idle {
			focused: true,
			..free
		};
		assert_eq!(kept_by(&rule, &focused), Some("focused on screen"));
		let blanks = Idle {
			keeps_picture: false,
			..free
		};
		assert_eq!(
			kept_by(&rule, &blanks),
			Some("on screen, and would go blank")
		);
		// out of sight, neither focus nor the picture keeps it
		let hidden = Idle {
			hidden: true,
			focused: true,
			keeps_picture: false,
			..free
		};
		assert_eq!(kept_by(&rule, &hidden), None);
	}

	// A Windows window drawn through a composition visual has no redirection
	// bitmap, so nothing is left on screen once its device goes: one let go in
	// view went black and stayed black until typed into. It waits until it is
	// out of sight, and only then for the hidden wait.
	// Test ID: ddga8
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_window_that_would_go_blank_is_never_let_go_in_view() {
		let since = Instant::now();
		let cfg = config::Settings {
			idle_release: true,
			idle_release_hidden_min: 30,
			idle_release_min: 240,
			..config::Settings::default()
		};
		let blanks = |focused, hidden| Idle {
			focused,
			hidden,
			revealed: true,
			bench_busy: false,
			since,
			keeps_picture: false,
		};
		assert!(
			release_deadline(&cfg, &blanks(false, false)).is_none(),
			"unfocused in view"
		);
		assert!(release_deadline(&cfg, &blanks(true, false)).is_none());
		assert_eq!(
			release_deadline(&cfg, &blanks(false, true)),
			Some(since + Duration::from_mins(30))
		);
	}

	// Off since the minimized wait was taken out again (2026100513581813): a
	// minimized window takes the hidden wait, now a minute by default.
	// `a_minimized_window_takes_the_hidden_wait` covers it.
	// // A minimized window lets go after its own wait, a minute by default,
	// // rather than the half hour a covered one waits. Whatever else the desktop
	// // says of it, and on Windows a window with no area, is minimized.
	// // Test ID: ddgs7
	// #[test]
	// fn a_minimized_window_waits_its_own_time() {
	// 	let since = Instant::now();
	// 	let defaults = config::Settings::default();
	// 	assert_eq!(defaults.idle_release_minimized_min, 1);
	// 	assert_eq!(
	// 		defaults.idle_release_hidden_min, 30,
	// 		"covered keeps its wait"
	// 	);
	// 	let cfg = config::Settings {
	// 		idle_release: true,
	// 		idle_release_minimized_min: 3,
	// 		idle_release_hidden_min: 30,
	// 		idle_release_min: 240,
	// 		..defaults
	// 	};
	// 	let idle = |focused, sight, keeps_picture| Idle {
	// 		focused,
	// 		sight,
	// 		revealed: true,
	// 		bench_busy: false,
	// 		since,
	// 		keeps_picture,
	// 	};
	// 	for focused in [false, true] {
	// 		for keeps_picture in [false, true] {
	// 			assert_eq!(
	// 				release_deadline(&cfg, &idle(focused, Sight::Minimized, keeps_picture)),
	// 				Some(since + Duration::from_mins(3)),
	// 				"focused {focused}, keeps picture {keeps_picture}"
	// 			);
	// 		}
	// 	}
	// 	assert_eq!(
	// 		release_deadline(&cfg, &idle(false, Sight::Covered, true)),
	// 		Some(since + Duration::from_mins(30))
	// 	);
	// 	let mut owed = idle(false, Sight::Minimized, true);
	// 	owed.bench_busy = true;
	// 	assert!(
	// 		release_deadline(&cfg, &owed).is_none(),
	// 		"a rating in flight"
	// 	);
	// 	let off = config::Settings {
	// 		idle_release: false,
	// 		..cfg
	// 	};
	// 	assert!(release_deadline(&off, &idle(false, Sight::Minimized, true)).is_none());
	//
	// 	// what the window reports, as (occluded, no area, minimized)
	// 	let sight =
	// 		|occluded, no_area, minimized| window_sight(true, occluded, no_area, || minimized);
	// 	assert_eq!(
	// 		sight(true, false, true),
	// 		Sight::Minimized,
	// 		"minimized and occluded"
	// 	);
	// 	assert_eq!(sight(false, true, false), Sight::Minimized, "no area");
	// 	assert_eq!(sight(true, false, false), Sight::Covered);
	// 	assert_eq!(sight(false, false, false), Sight::Shown);
	// }

	// A minimized window is out of sight like a covered one and takes the same
	// wait, a minute by default, whatever else holds the window. Whether the
	// desktop calls it covered as well makes no difference.
	// Test ID: ddj4z
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_minimized_window_takes_the_hidden_wait() {
		let since = Instant::now();
		let cfg = config::Settings {
			idle_release: true,
			..config::Settings::default()
		};
		assert_eq!(cfg.idle_release_hidden_min, 1);
		for occluded in [false, true] {
			let hidden = window_hidden(true, occluded, false, || true);
			assert!(hidden, "minimized, occluded {occluded}");
			for focused in [false, true] {
				for keeps_picture in [false, true] {
					let idle = Idle {
						focused,
						hidden,
						revealed: true,
						bench_busy: false,
						since,
						keeps_picture,
					};
					assert_eq!(
						release_deadline(&cfg, &idle),
						Some(since + Duration::from_mins(1)),
						"focused {focused}, keeps picture {keeps_picture}"
					);
				}
			}
		}
	}

	// A Windows restore stops answering minimized a moment before the size
	// comes back. A rebuild in that gap read the 0x0 client area, took 1x1 as
	// the window's size, and the grid went to 2 columns; the console host's
	// reflow then left only the prompt's last character on screen.
	// Test ID: ddga9
	#[test]
	fn a_window_with_no_area_is_hidden_whatever_the_minimized_answer() {
		assert!(window_hidden(true, false, true, || false));
		assert!(!window_hidden(true, false, false, || false));
		assert!(window_hidden(true, false, false, || true));
		assert!(window_hidden(true, true, false, || false));
		assert!(!window_hidden(false, true, true, || true), "not shown yet");
	}

	// A program printing in a minimized window held its device for good, since
	// every output started the idle clock over. Output nobody can see leaves the
	// clock alone, but a released window is still owed its device for the reveal.
	// Test ID: dcrvv
	#[test]
	fn output_into_a_hidden_window_does_not_keep_its_device() {
		let long_ago = Instant::now()
			.checked_sub(Duration::from_secs(1))
			.expect("a second of uptime");
		let mut clock = IdleClock {
			since: long_ago,
			wake_owed: false,
			repaint: false,
			retry: Retry::default(),
		};
		assert!(!clock.output(false, true, true));
		assert_eq!(clock.since, long_ago, "hidden output restarted the clock");
		assert!(!clock.wake_owed);
		assert!(
			clock.output(true, true, true),
			"a released window is owed it"
		);
		assert!(clock.wake_owed);
		assert_eq!(clock.since, long_ago);
		assert!(!clock.output(true, true, true), "owed once");

		let mut seen = IdleClock {
			since: long_ago,
			wake_owed: false,
			repaint: false,
			retry: Retry::default(),
		};
		assert!(!seen.output(false, false, true));
		assert!(seen.since > long_ago, "output on screen is a sign of life");
		assert!(seen.output(true, false, true));
		assert!(seen.wake_owed);
	}

	// A program printing now and then in a tab in the back kept an unfocused
	// window on screen from ever letting go after "Minutes otherwise". Only
	// the tab in front counts while the window shows, and a released window
	// isn't owed its device for a tab nobody sees.
	// Test ID: der5b
	#[test]
	fn output_from_a_tab_in_the_back_does_not_keep_the_device() {
		let long_ago = Instant::now()
			.checked_sub(Duration::from_secs(1))
			.expect("a second of uptime");
		let mut clock = IdleClock {
			since: long_ago,
			wake_owed: false,
			repaint: false,
			retry: Retry::default(),
		};
		assert!(!clock.output(false, false, false));
		assert_eq!(
			clock.since, long_ago,
			"a tab in the back restarted the clock"
		);
		assert!(!clock.output(true, false, false));
		assert!(!clock.wake_owed, "a tab in the back took the device back");
		// Hidden, it was owed at the reveal whichever tab printed. Since
		// 2026111010491300 only the tab in front is, since a tab in the back
		// can't be seen after the reveal either (df4dj pins that).
		// assert!(clock.output(true, true, false));
		// assert!(clock.wake_owed);
		assert!(!clock.output(true, true, false));
		assert!(!clock.wake_owed);
		assert_eq!(clock.since, long_ago);
	}

	// A device the GPU refused on the way back from the idle release used to
	// wait for input, so a window nobody touched stayed blank after the load.
	// Test ID: ddf4f
	#[test]
	fn a_refused_rebuild_stays_pending_and_is_tried_again() {
		let now = Instant::now();
		let mut clock = IdleClock::new();
		clock.owe(true);
		assert!(clock.rebuild_due(false, now));
		assert!(
			!clock.rebuild_due(true, now),
			"a hidden window waits for the reveal"
		);

		assert!(clock.rebuild_failed(now), "the first refusal is reported");
		assert!(!clock.rebuild_failed(now), "and only the first");
		let wake = clock
			.rebuild_wake(false)
			.expect("the loop wakes for the retry");
		assert!(wake > now);
		assert!(!clock.rebuild_due(false, now), "not before the backoff");
		assert!(clock.rebuild_due(false, wake), "tried again with no input");
		assert_eq!(clock.rebuild_wake(true), None, "no wakes while hidden");

		// a return to the console releases and rebuilds with nothing owed yet
		let mut healed = IdleClock::new();
		healed.rebuild_failed(now);
		assert!(healed.wake_owed);

		clock.rebuilt();
		assert!(!clock.wake_owed);
		assert_eq!(clock.retry, Retry::default());
	}

	// Test ID: dcsvt
	#[test]
	fn the_title_note_follows_the_device_out_and_back() {
		let now = Instant::now();
		assert_eq!(Conserve::Off.note(now), None);
		assert_eq!(
			Conserve::Saving.note(now),
			Some("resource conservation mode")
		);
		let mut state = Conserve::Saving;
		state.wallpaper_answered(now);
		assert_eq!(
			state,
			Conserve::Saving,
			"a wallpaper while released changes nothing"
		);
		state = Conserve::Restoring;
		assert_eq!(state.note(now), Some("restoring resources ..."));
		assert_eq!(
			state.wake(),
			None,
			"restoring waits on the wallpaper, not a clock"
		);
		state.wallpaper_answered(now);
		assert_eq!(state.note(now), Some("resources restored"));
		assert_eq!(state.wake(), Some(now + RESTORED_SHOWN));
		assert_eq!(state.note(now + RESTORED_SHOWN), None);
	}

	// A frame drawn while hidden banks the backlog into the ease, and the reveal
	// then plays it back as if it had just arrived.
	// Test ID: dd5ph
	#[test]
	fn a_hidden_window_draws_nothing_and_its_return_is_one_cut() {
		use super::{Frame, freeze_frame};
		assert_eq!(freeze_frame(false, true), Frame::Skip);
		assert_eq!(freeze_frame(true, true), Frame::Skip);
		assert_eq!(freeze_frame(true, false), Frame::CatchUp);
		assert_eq!(freeze_frame(false, false), Frame::Draw);
	}
}
