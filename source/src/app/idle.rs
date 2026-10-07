// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// When the window last saw a person or a shell: input, focus either way, or
// output while it could be seen. The idle release counts from `since` (see
// `release_deadline`). `wake_owed` says something wants the window back since
// it let its device go, so the device is owed as soon as there is a screen to
// draw on.
struct IdleClock {
	since: Instant,
	wake_owed: bool,
	// a rebuild the GPU refused, tried again on this
	retry: Retry,
}

impl IdleClock {
	fn new() -> Self {
		IdleClock {
			since: Instant::now(),
			wake_owed: false,
			retry: Retry::default(),
		}
	}

	// The device back now: it is owed, there is a screen to draw on, and no
	// refused rebuild is waiting out its backoff.
	fn rebuild_due(&self, hidden: bool, now: Instant) -> bool {
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

	// When the loop has to wake for a refused rebuild.
	fn rebuild_wake(&self, hidden: bool) -> Option<Instant> {
		self.retry.at.filter(|_| self.wake_owed && !hidden)
	}

	// A sign of life. True when it is the one that makes the device owed.
	fn active(&mut self, released: bool) -> bool {
		self.since = Instant::now();
		self.owe(released)
	}

	fn owe(&mut self, released: bool) -> bool {
		let newly = released && !self.wake_owed;
		self.wake_owed |= released;
		newly
	}

	// A shell printing. Output nobody can see does not keep a hidden window's
	// device, or a program that prints forever would hold it for good. It is
	// still owed at the reveal, so a desktop that says nothing about showing
	// the window again cannot leave old pixels up.
	fn output(&mut self, released: bool, hidden: bool) -> bool {
		if hidden {
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

// What the window title says about the device. Nothing normally, a note while
// it is let go, another while it comes back, and a last one for a few seconds
// after. The wallpaper is the last thing a rebuild waits on, and the only part
// slow enough for anyone to see, so coming back lasts until it answers.
// Any rebuild counts, a return to this console as much as the idle release.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Conserve {
	Off,
	Saving,
	Restoring,
	Restored(Instant),
}

const RESTORED_SHOWN: Duration = Duration::from_secs(5);

impl Conserve {
	fn note(self, now: Instant) -> Option<&'static str> {
		match self {
			Conserve::Off => None,
			Conserve::Saving => Some("resource conservation mode"),
			Conserve::Restoring => Some("restoring resources ..."),
			Conserve::Restored(at) => (now < at + RESTORED_SHOWN).then_some("resources restored"),
		}
	}

	// When the title next changes on its own.
	fn wake(self) -> Option<Instant> {
		match self {
			Conserve::Restored(at) => Some(at + RESTORED_SHOWN),
			_ => None,
		}
	}

	fn wallpaper_answered(&mut self, now: Instant) {
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
// it: the switch off, the window not yet shown, a rating owed or running, or a
// window on screen that has focus or would go blank without its device. Two
// waits, because a hidden window is known to be out of sight while a merely
// unfocused one may be on a second monitor being read.
fn release_deadline(cfg: &config::Settings, idle: &Idle) -> Option<Instant> {
	let rule = idle_rule(cfg);
	let kept_on_screen = !idle.hidden && (idle.focused || !idle.keeps_picture);
	if !rule.on || !idle.revealed || idle.bench_busy || kept_on_screen {
		return None;
	}
	let wait = if idle.hidden {
		rule.hidden
	} else {
		rule.otherwise
	};
	Some(idle.since + wait)
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

// SILK_IDLEDBG=1: the idle release's comings and goings on stderr, stamped
// with seconds since the first call so a log can be read against a timeline.
fn idledbg(msg: &str) {
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
/// there once freed, because `malloc_trim` never shrinks an arena that is not
/// the main one. Measured on a 1920x993 wallpaper: about 40 MB kept per decode,
/// one per rebuild after an idle release, and the first decode's 50 MB kept for
/// the life of every window. Setting the threshold pins it. 4 MB keeps a
/// frame's own vectors in the arena on any grid and puts only the image buffers
/// on the mapping path. Pinning it also stops the trim threshold moving, so
/// that is set too, high enough that the main heap's top is not given back and
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
// shrank to two columns with it, and the console host's reflow lost the screen.
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
// reveal is not held up by it.
const MINIMIZED_RECHECK: Duration = Duration::from_millis(250);

// An event a restore brings. The WM's redraw is one, and it reaches
// `freeze_sync` before `about_to_wait` does (G89), so it must see the
// window as shown, not a minimized answer from before.
fn restore_sign(event: &WindowEvent) -> bool {
	matches!(
		event,
		WindowEvent::Focused(_)
			| WindowEvent::Occluded(_)
			| WindowEvent::Resized(_)
			| WindowEvent::RedrawRequested
	)
}

// The window's minimized state as last asked, so a pass asks at most once per
// MINIMIZED_RECHECK rather than every time.
#[derive(Default)]
struct MinimizedProbe {
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

	fn forget(&mut self) {
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

	// The freeze edge, owned in one place because both render entry points reach
	// it: on restore the WM's own redraw arrives before `about_to_wait` runs, so
	// whichever gets here first has to be the one that catches up - otherwise that
	// frame banks the whole backlog into the ease before anything cuts it.
	fn freeze_sync(&mut self) -> bool {
		let hidden = self.window_hidden();
		let frame = freeze_frame(self.was_hidden, hidden);
		if frame == Frame::CatchUp {
			self.freeze_catchup();
			// Being shown again is a sign of life. Windows sends no occlusion
			// events, and its repaint on a restore can come before the size
			// does, while the window still counts as hidden.
			self.note_active("shown");
		}
		self.was_hidden = hidden;
		frame == Frame::Skip
	}

	// A frozen surface coming back on screen: hidden tabs never build, and a
	// minimized/occluded window builds nothing - so the reveal is one dirty
	// catch-up frame, hard-cut so the gap closes instantly instead of easing in
	// (that ease is the bounce class, and it also reads as output arriving now).
	// Every pane is cut, not just the ones flagged dirty: the flag is cleared by
	// whichever build got there first, so it answers "is a rebuild owed", not
	// "did the grid move while nobody was looking". A pane that really did sit
	// still is snapping a scroll already at rest.
	fn freeze_catchup(&mut self) {
		for pane in self.tabs.cur_mut().panes.values_mut() {
			pane.hard_cut();
		}
		self.dirty = true;
	}

	// A sign of life: the idle clock starts over, and a window that let its
	// device go is owed it back.
	fn note_active(&mut self, why: &'static str) {
		if self.idle.active(self.gpu.is_none()) {
			idledbg(&format!("wake: {why}"));
		}
	}

	// Output, which counts only while the window can be seen (IdleClock::output).
	// The hidden flag is the one the last pass settled on.
	fn note_output(&mut self) {
		if self.idle.output(self.gpu.is_none(), self.was_hidden) {
			idledbg("wake: output");
		}
	}

	// Reads the hidden answer the pass's `freeze_sync` settled on.
	fn release_deadline(&self, cfg: &config::Settings) -> Option<Instant> {
		release_deadline(
			cfg,
			&Idle {
				focused: self.focused,
				hidden: self.was_hidden,
				revealed: self.revealed,
				bench_busy: self.bench.is_some() || self.bench_at.is_some(),
				since: self.idle.since,
				keeps_picture: self.keeps_picture,
			},
		)
	}

	// Let the device and everything on it go. The window stays, the shells run
	// on and the grid keeps up; only drawing stops, and `rebuild_gpu` is the
	// way back. What the CPU held only for the device's sake goes too: the
	// rasterized glyphs, the shaped chrome.
	fn release_gpu(&mut self) {
		let Some(gpu) = self.gpu.take() else {
			return;
		};
		let start = Instant::now();
		self.text.detach_gpu();
		self.chrome = None;
		self.invalidate_prepared();
		self.rebirth = Some(gpu.release());
		self.idle.rebuilt();
		self.frame_retry = Retry::default();
		// no frame draws while released, so nothing else would update the title
		self.conserve = Conserve::Saving;
		self.update_title();
		trim_heap();
		idledbg(&format!("device released in {:?}", start.elapsed()));
	}

	// The device again, on the same window, and everything that lived on it
	// built afresh. The wallpaper is prepared again from the file rather than
	// kept, as after a VT switch (recover_gpu), and its small stand-in shows
	// from the first frame until it arrives. A failure leaves the window
	// released and owed, and it is tried again on a backoff while the window
	// shows.
	fn rebuild_gpu(&mut self) {
		let Some(rebirth) = self.rebirth.as_ref() else {
			return;
		};
		let start = Instant::now();
		let gfx = match Gfx::rebuild(rebirth, &self.window, crate::gfx::wanted()) {
			Ok(gfx) => gfx,
			Err(e) => {
				if self.idle.rebuild_failed(Instant::now()) {
					eprintln!(
						"{}: could not bring the GPU device back ({e}); trying again",
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
		self.idle.since = Instant::now();
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

	// Everything on the device again, after a return to this console. The whole
	// device when nothing else shares it, since a switch can spoil any texture
	// and `recover_gpu` only knows the text and the wallpaper. An open dialog's
	// context cannot outlive the terminal's on X11, so that case stays partial.
	fn heal_gpu(&mut self, dialog_open: bool) {
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

	// GPU texture contents were lost (VT switch / suspend; see the Sentinel note
	// in gfx.rs). Re-upload everything that was uploaded once: fresh glyph
	// atlases + chrome via rebuild_text, and the wallpaper. rebuild_text also drops
	// the prepared/scrim signatures, so the next frame rebuilds the scrim source
	// instead of reusing a texture that no longer holds anything.
	fn recover_gpu(&mut self) {
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
	// SILK_IDLE_SECS is read once per process. The idle rule is asked twice on
	// every loop pass, and it went to the environment each time. Run in a child
	// copy of this test binary, since the answer is cached per process.
	// Test ID: ErgPfvU
	#[test]
	fn the_idle_wait_switch_is_read_once() {
		let out = std::process::Command::new(std::env::current_exe().unwrap())
			.args(["--exact", "app::tests::idle_wait_child", "--nocapture"])
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

	// Test ID: ErgPfzH
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
	// Test ID: ErgPg2b
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
	// screen, and a covered window waits the shorter of the two times. Everything
	// that vetoes it is a None, since a deadline that then had to be checked
	// again elsewhere is how a veto gets forgotten.
	// Test ID: Eq8b2Gu
	#[test]
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
		// The release shipped off until 2026100312470540 turned it on by default,
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

	// A Windows window drawn through a composition visual has no redirection
	// bitmap, so nothing is left on screen once its device goes: one let go in
	// view went black and stayed black until typed into. It waits until it is
	// out of sight, and only then for the hidden wait.
	// Test ID: ErksiLn
	#[test]
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

	// A minimized window is out of sight like a covered one and takes the same
	// wait, a minute by default, whatever else holds the window. Whether the
	// desktop calls it covered as well makes no difference.
	// Test ID: ErsV4Jy
	#[test]
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
	// the window's size, and the grid went to two columns; the console host's
	// reflow then left only the prompt's last character on screen.
	// Test ID: Erksiin
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
	// Test ID: EqBNCpU
	#[test]
	fn output_into_a_hidden_window_does_not_keep_its_device() {
		let long_ago = Instant::now()
			.checked_sub(Duration::from_secs(1))
			.expect("a second of uptime");
		let mut clock = IdleClock {
			since: long_ago,
			wake_owed: false,
			retry: Retry::default(),
		};
		assert!(!clock.output(false, true));
		assert_eq!(clock.since, long_ago, "hidden output restarted the clock");
		assert!(!clock.wake_owed);
		assert!(clock.output(true, true), "a released window is owed it");
		assert!(clock.wake_owed);
		assert_eq!(clock.since, long_ago);
		assert!(!clock.output(true, true), "owed once");

		let mut seen = IdleClock {
			since: long_ago,
			wake_owed: false,
			retry: Retry::default(),
		};
		assert!(!seen.output(false, false));
		assert!(seen.since > long_ago, "output on screen is a sign of life");
		assert!(seen.output(true, false));
		assert!(seen.wake_owed);
	}

	// A device the GPU refused on the way back from the idle release used to
	// wait for input, so a window nobody touched stayed blank after the load.
	// Test ID: Erfy7yk
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

	// Test ID: EqFWtPs
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
	// Test ID: Er2UiYW
	#[test]
	fn a_hidden_window_draws_nothing_and_its_return_is_one_cut() {
		use super::{Frame, freeze_frame};
		assert_eq!(freeze_frame(false, true), Frame::Skip);
		assert_eq!(freeze_frame(true, true), Frame::Skip);
		assert_eq!(freeze_frame(true, false), Frame::CatchUp);
		assert_eq!(freeze_frame(false, false), Frame::Draw);
	}
}
