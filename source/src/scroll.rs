// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use crate::config;

/// Fractional scroll overlay. The crate's grid only knows whole-line offsets,
/// so everything under a line is here.
///
/// `target` and `visual` are lines of scrollback from the bottom, where 0.0 is
/// following new output. Each frame the grid is snapped to `visual.floor()` and
/// the renderer shifts by the fraction.
///
/// An output burst is chased at a speed (`chase`, lines/s) that runs through
/// the feel settings in turn, each starting where the last one ended. Ease-in
/// lifts it from rest to `KNEE_LPS`, and Ramp-up doubles it toward the cap. Max
/// speed is the cap: the single-screen speed while the burst's first line is on
/// screen, and unbounded after. Ramp-down is a braking curve applied the whole
/// time, and Ease-out is the stop inside `STOP_BAND`. The backlog isn't capped
/// in lines, since a line cap jumped the view to full speed the moment it
/// filled.
///
/// The chase is only for output while following the bottom. The wheel and the
/// scrollbar use the plain `NAV_TAU_MS` ease, and a jump back to the bottom
/// sweeps at full ease speed (`sweep`). Ease-out is stored as a duration so
/// both ease sliders read "higher = crisper". The smooth scrolling design doc,
/// "The output chase", has the whole curve.
pub const MAX_BACKLOG: f32 = 16.0; // reference depth for output_ease_lines' clamp and the turnover guess
const CHASE_GROW_MIN: f32 = 2.0; // gap (lines) under which a user sweep counts as caught up
// Where Ease-in hands off to Ramp-up (lines/s). An exponential ramp can't
// leave zero, so the first KNEE_LPS of speed is a linear lift, and past it the
// doubling takes over. When the cap lifts mid-burst, the knee is set this far
// above the speed at that moment.
const KNEE_LPS: f32 = 6.0;
// User-navigation ease time constant (wheel, scrollbar, jumps) and the
// alt-screen slide's base tau. Fixed: the one "Initial scroll speed" knob that
// used to feed this also fed the chase start and the ease-in attack, which is
// how every slider ended up influencing every other.
const NAV_TAU_MS: f32 = 230.0;
// Sharper stop: within this many lines of the target the exponential tail is
// replaced by a glide that covers the band in `scroll_ease_out_ms`, so the last
// few pixels sweep in instead of crawling. Above the band the ease-out is
// untouched.
const STOP_BAND: f32 = 0.4;
/// Alt-screen app-scroll easing: a full-screen app (less, vim, tmux, ...) owns
/// its screen and scrolls a region of it in place. `app_off` is a transient
/// visual offset (in lines, signed: + shifts content down) grown by each scroll
/// step, then eased to 0 with the output chase's own curve so the frame slides
/// into place. The rows the scroll pushed off the region fill the gap (the strip
/// in pane.rs), so the offset may never exceed what that strip has: the caller
/// passes the cover, and `SLIDE_ROWS` is the most either side keeps.
pub const SLIDE_ROWS: usize = 128;

// The output chase's speed state. One drives the scrollback view and one the
// alt-screen slide, so an app's region scroll eases with the same curve as
// plain output and the 5 feel sliders shape both.
#[derive(Debug, Clone, Copy)]
struct Chase {
	speed: f32,     // catch-up speed, lines/s (the segment the curve is in falls out of it)
	knee: f32,      // speed where the current Ease-in segment hands off to Ramp-up
	burst: f32,     // lines this burst has advanced (resets once settled at rest)
	overflow: bool, // burst topped a screenful: its first line is off - uncapped
}

impl Chase {
	const REST: Chase = Chase {
		speed: 0.0,
		knee: KNEE_LPS,
		burst: 0.0,
		overflow: false,
	};

	// `grown` more lines arrived; `view_rows` is the height they scroll in.
	fn nudge(&mut self, grown: f32, view_rows: f32) {
		if self.burst <= 0.0 {
			// fresh burst: the curve starts at 0 and Ease-in runs first
			self.speed = 0.0;
			self.knee = KNEE_LPS;
		}
		self.burst += grown;
		if view_rows > 0.0 && self.burst >= view_rows && !self.overflow {
			self.overflow = true;
			// the cap lifts: Ease-in runs once more from the speed it found
			// itself at (same slope, same delta), then Ramp-up resumes
			self.knee = self.speed + KNEE_LPS;
		}
	}

	// One frame of the chase, one segment at a time: advance the curve and return
	// the speed the view may close at, given the `backlog` still to cover. Ease-in
	// lifts the speed linearly to the knee; Ramp-up doubles it toward the current
	// cap; the braking curve ("Ramp-down", traced backwards from "Ease-out") caps
	// it the whole way at what can still be wound down within the backlog. That
	// sets the reserve behind a live burst and the slowdown once output stops.
	fn limit(&mut self, backlog: f32, dt_s: f32, cfg: &config::Settings) -> f32 {
		if self.speed < self.knee {
			let ease_in_s = cfg.scroll_ease_in_ms.max(1.0) / 1000.0;
			self.speed = (self.speed + KNEE_LPS * dt_s / ease_in_s).min(self.knee);
		} else {
			let double_s = cfg.scroll_ramp_up_ms.max(1.0) / 1000.0;
			self.speed *= (dt_s / double_s).exp2();
		}
		if !self.overflow {
			let onscreen_v = 1000.0 / cfg.scroll_single_screen_tau_ms.max(1.0);
			self.speed = self.speed.min(onscreen_v);
		}
		// braking: halving per "Ramp-down" from here must meet the ease-out
		// handoff (the STOP_BAND edge, at its closing speed) with no backlog left
		let v_land = STOP_BAND * 1000.0 / cfg.scroll_ease_out_ms.max(1.0);
		let brake_tau_s = cfg.scroll_ramp_down_ms.max(1.0) / 1000.0 / std::f32::consts::LN_2;
		self.speed = self
			.speed
			.min(v_land + (backlog - STOP_BAND).max(0.0) / brake_tau_s);
		self.speed
	}
}

#[derive(Debug)]
pub struct Scroll {
	target: f32,
	visual: f32,
	mid: f32, // cascade stage between target and visual: gives the ease its ease-in
	max: f32,
	chase: Chase,     // the output chase: how fast the view may close on the target
	app_off: f32,     // alt-screen slide offset, eased toward 0 (see SLIDE_ROWS)
	app_mid: f32,     // the slide's cascade stage, as `mid` is to `visual`
	app_chase: Chase, // the slide's own chase, so an app's scroll eases like output
	sweep: bool,      // user jumped back to the bottom: full ease speed until caught up
	wheel_dir: f32,   // sign of the last wheel motion, so the detent sits ahead of it
	// Output lines below the view that it has never come down to. Arriving
	// output raises it and the view gives it back as it gets to them, so it is
	// a history rather than a state. See `unshown_lines`.
	unshown: f32,
}

impl Scroll {
	pub fn new() -> Self {
		Self {
			target: 0.0,
			visual: 0.0,
			mid: 0.0,
			max: 0.0,
			chase: Chase::REST,
			app_off: 0.0,
			app_mid: 0.0,
			app_chase: Chase::REST,
			sweep: false,
			wheel_dir: 0.0,
			unshown: 0.0,
		}
	}

	/// An app's region scrolled `shift` lines (signed, + = content moved up). The
	/// slide grows by it, so the content on screen stays continuous across the
	/// step, and eases home from there the way `nudge_output` does, with the same
	/// floor and cascade. `cover` is how many rows the strip can fill, which
	/// bounds the offset, since a gap past it would show background. `view_rows`
	/// is the region's height, for the chase's screenful test. A step the other
	/// way starts a new slide, with its strip on the other side.
	pub fn app_scroll(&mut self, shift: f32, cover: f32, view_rows: f32) {
		if shift == 0.0 {
			return;
		}
		if self.app_off != 0.0 && (self.app_off < 0.0) != (shift < 0.0) {
			self.cancel_app_scroll();
		}
		let cfg = config::settings();
		if !cfg.scroll_smooth {
			return;
		}
		let floor = cfg.output_ease_lines.clamp(0.0, MAX_BACKLOG);
		let cover = cover.clamp(0.0, SLIDE_ROWS as f32);
		let before = self.app_off.abs();
		let lag = (before + shift.abs()).max(floor).min(cover);
		self.app_mid = (self.app_mid + (lag - before)).clamp(0.0, lag);
		self.app_chase.nudge(shift.abs(), view_rows);
		self.app_off = shift.signum() * lag;
	}

	/// Hard-cut any in-flight alt-screen slide. An alt-screen enter/exit is an
	/// instant full-screen swap, not a scroll, so a slide left easing across it
	/// would drag the wrong screen's content.
	pub fn cancel_app_scroll(&mut self) {
		self.app_off = 0.0;
		self.app_mid = 0.0;
		self.app_chase = Chase::REST;
	}

	/// Freeze catch-up (hidden tab shown, minimized window restored): be at rest
	/// instantly. Anything left easing was invisible while frozen, and the pending
	/// backlog mustn't ease in, or the view bounces.
	pub fn snap(&mut self) {
		self.visual = self.target;
		self.mid = self.target;
		self.chase = Chase::REST;
		self.cancel_app_scroll();
		self.sweep = false;
		self.unshown = self.unshown.min(self.visual);
	}

	/// Current alt-screen slide offset in lines (added to the render's vertical
	/// offset; 0 except briefly after an app repaint-scroll).
	pub fn app_offset(&self) -> f32 {
		self.app_off
	}

	/// The view may never sit past the grid. The renderer snaps the grid to
	/// `visual.floor()` and draws the fraction, so a `visual` beyond `max` has its
	/// whole part pinned while the fraction keeps cycling, and every wrap of it
	/// is a whole-cell hop. That was nano's wobble: a burst still easing when the
	/// alt screen (no scrollback, so max 0) took over hopped once per line of the
	/// leftover backlog. Clamping here also stops the ease the instant the screen
	/// swaps, which is the cut an alt-screen entry wants anyway.
	pub fn set_max(&mut self, history_lines: f32) {
		self.max = history_lines.max(0.0);
		self.target = self.target.clamp(0.0, self.max);
		self.visual = self.visual.clamp(0.0, self.max);
		self.mid = self.mid.clamp(0.0, self.max);
		self.unshown = self.unshown.min(self.visual);
	}

	pub fn following(&self) -> bool {
		self.target <= config::SETTLE_EPS
	}

	pub fn wheel(&mut self, lines: f32) {
		if lines != 0.0 {
			self.wheel_dir = lines.signum();
		}
		self.target = (self.target + lines).clamp(0.0, self.max);
		// a wheel that comes back to the bottom is still the user driving: the
		// remaining gap sweeps at full ease speed, not the output chase
		self.sweep = true;
	}

	/// Scrollback extent in lines (0 = nothing to scroll).
	pub fn max_lines(&self) -> f32 {
		self.max
	}

	/// Where the view is right now, mid-ease. The scrollbar thumb rides
	/// this so it tracks the content rather than the pending destination.
	pub fn visual_lines(&self) -> f32 {
		self.visual
	}

	/// Where the view is headed. A thumb being dragged rides this instead: the
	/// handle must follow the pointer exactly (direct manipulation), while the
	/// content eases in behind it.
	pub fn target_lines(&self) -> f32 {
		self.target
	}

	/// Scroll to a position in lines from the bottom, for scrollbar drags and
	/// track clicks. Eases like every other scroll.
	pub fn scroll_to(&mut self, lines: f32) {
		self.wheel_dir = 0.0;
		self.target = lines.clamp(0.0, self.max);
		self.sweep = true;
	}

	pub fn jump_bottom(&mut self) {
		self.target = 0.0;
		self.sweep = true;
	}

	/// New output grew the scrollback by `grown` lines while following the bottom.
	/// It goes into the visual backlog, so a fast burst lags and the chase scrolls
	/// through it. The backlog isn't capped, for the reason on `Scroll`. Sporadic
	/// output stays at about `output_ease_lines` and eases in at the first speed.
	/// `view_rows` is the pane's height. Once a burst has moved that far, its
	/// first line has scrolled off the top, which lifts the in-view speed cap.
	pub fn nudge_output(&mut self, grown: f32, view_rows: f32) {
		if self.following() {
			let cfg = config::settings();
			if !cfg.scroll_smooth {
				return; // master off: the grid already sits at the bottom, no lag to ease
			}
			self.chase.nudge(grown, view_rows);
			// the floor is capped by the history there is to ease through: a
			// fresh terminal or one just cleared has no room past the grid
			let floor = cfg.output_ease_lines.clamp(0.0, MAX_BACKLOG);
			let before = self.visual;
			self.visual = (self.visual + grown).max(floor).min(self.max);
			// the nudge is a coordinate shift, since content moved under the view,
			// so the cascade stage moves with it. That keeps its lead over `visual`,
			// which keeps the eased speed across a burst.
			self.mid = (self.mid + (self.visual - before)).clamp(0.0, self.visual);
			// the new lines printed below the view, so it hasn't gotten to them.
			// The delta is `visual - before` and not `grown`: the floor and the
			// history clamp above can both make them differ.
			self.unshown = (self.unshown + (self.visual - before)).min(self.visual);
		}
	}

	/// A nudge that goes back no further than the new lines, floor or not:
	/// output after a clear, which must not ease through the screen it cleared.
	pub fn nudge_output_within(&mut self, grown: f32, view_rows: f32) {
		let max = self.max;
		self.set_max(grown.min(max));
		self.nudge_output(grown, view_rows);
		self.max = max;
	}

	// The whole line the view comes to rest on. A wheel goes to the line ahead of
	// where it stopped, in the direction it was going, since rounding to nearest
	// sent a scroll that stopped most of a line past a boundary forward and then
	// back. A thumb drag or a track click has no direction and rounds to nearest.
	fn detent(&self) -> f32 {
		let whole = match self.wheel_dir {
			d if d > 0.0 => self.target.ceil(),
			d if d < 0.0 => self.target.floor(),
			_ => self.target.round(),
		};
		whole.clamp(0.0, self.max)
	}

	/// The output ease is chasing new lines, as opposed to resting or a user sweep.
	pub fn chasing_output(&self) -> bool {
		self.chase.burst > 0.0 && self.following() && !self.sweep
	}

	/// How far above the newest line the eased view still sits, in lines, which
	/// is where the minimap has to stop drawing: past it is text the pane hasn't
	/// shown. Only output raises this, and only the view arriving lowers it, so a
	/// gesture neither shortens the map nor turns the trim off.
	///
	/// It can't be worked out from `visual`, `following()`, or `chasing_output()`
	/// after the fact. `visual` includes a gesture's remaining travel as well as
	/// the chase's backlog and both read the same from outside; `following()`
	/// is true the moment a gesture aims at the bottom, and `chasing_output()`
	/// goes false for the rest of a flood at the first keystroke. Those were
	/// F150 and F153, the same site in opposite directions.
	pub fn unshown_lines(&self) -> f32 {
		self.unshown
	}

	/// Whether that count is still falling. It only falls as `visual` comes down
	/// past it, and `visual` is heading for `target`, so a target below it will
	/// drain it and a target above it leaves it frozen. The minimap asks because
	/// a map held short by a frozen count has nothing more to draw until the view
	/// moves, and asking for a compose anyway recomposed forever (F155).
	pub fn unshown_draining(&self) -> bool {
		self.target < self.unshown
	}

	pub fn advance(&mut self, dt_s: f32) {
		// one settings() snapshot per call, since this runs per pane per frame
		let cfg = config::settings();
		if !cfg.scroll_smooth {
			// smooth scrolling off: every scroll is instant, on a whole line
			self.target = self.target.round().clamp(0.0, self.max);
			self.snap();
			return;
		}
		// The output chase (see Chase): a speed the view may not close faster than
		// while output is still ahead of it.
		let chasing = self.chasing_output();
		let limit = chasing.then(|| {
			let backlog = (self.visual - self.target).max(0.0);
			self.chase.limit(backlog, dt_s, &cfg)
		});

		// A 2-stage ease: `mid` chases the target at NAV_TAU_MS and `visual`
		// chases `mid` over the ease-in attack, so motion builds from rest
		// instead of spiking on the first frame. Neither stage can pass its input,
		// so the cascade can't overshoot.
		let smoothing = 1.0 - (-dt_s * 1000.0 / NAV_TAU_MS).exp();
		self.mid += (self.target - self.mid) * smoothing;
		let attack_tau = cfg.scroll_ease_in_ms.max(1.0);
		let attack = 1.0 - (-dt_s * 1000.0 / attack_tau).exp();
		let before = self.visual;
		self.visual += (self.mid - self.visual) * attack;
		if let Some(speed) = limit {
			// While a burst is in flight the chase is the motion rather than a cap on
			// the navigation ease. Its segments already end where the stop band begins,
			// so nothing is left for the cascade to finish. Capping instead gave any
			// advance short enough that NAV_TAU_MS was slower, such as a returning
			// prompt, to a fixed 230ms decay no setting changes. It crawled almost to
			// a stop and then jumped when the stop band took over.
			self.visual = (before - speed * dt_s).max(self.target);
			self.mid = self.mid.min(self.visual);
		}
		if self.sweep && self.visual - self.target < CHASE_GROW_MIN {
			self.sweep = false; // caught up: output easing owns the view again
		}
		// Sharper stop: inside the band, close on the target fast enough to cover
		// the whole band within "Ease-out" (never past it) so the tail sweeps in
		// instead of crawling. Storing the duration rather than the speed is what
		// lets the setting read the same direction as its Ease-in partner.
		let stop_min_lps = STOP_BAND * 1000.0 / cfg.scroll_ease_out_ms.max(1.0);
		let gap = self.target - before;
		if gap.abs() < STOP_BAND {
			let floored = if gap >= 0.0 {
				(before + stop_min_lps * dt_s).min(self.target)
			} else {
				(before - stop_min_lps * dt_s).max(self.target)
			};
			if (self.target - floored).abs() < (self.target - self.visual).abs() {
				self.visual = floored;
			}
			// keep the leading stage at least as close to the target as `visual`,
			// or the cascade would pull the glide back
			if (self.target - self.mid).abs() > (self.target - self.visual).abs() {
				self.mid = self.visual;
			}
		}
		if (self.target - self.visual).abs() < config::SETTLE_EPS {
			// Rest on a whole line: a pixel-delta wheel (touchpad, hi-res wheel)
			// accumulates a fractional target, and resting there draws every row
			// shifted by part of a cell. The top of the first clipped row then shows
			// at the pane's bottom edge as garbage by the divider. Glide to the
			// nearest line instead.
			let detent = self.detent();
			if (self.visual - detent).abs() < config::SETTLE_EPS {
				self.target = detent;
				self.visual = detent;
				self.mid = detent;
				// at rest the burst is over; the next output starts a fresh one
				self.chase = Chase::REST;
				self.sweep = false;
				self.wheel_dir = 0.0;
			} else {
				self.target = detent; // still a fraction away: keep easing to the detent
			}
		}

		// `visual` is final for the frame, so whatever the view came down to has
		// now been shown. Ratcheting it here rather than clamping in the getter
		// is what makes it a history: a later scroll back raises `visual` again,
		// and a getter that only took a `min` would hand back a count the view
		// had already reached.
		self.unshown = self.unshown.min(self.visual);

		// Ease the alt-screen slide home: the cascade and the chase above, run
		// against a target of 0, so an app's region scroll reads exactly like
		// plain output does.
		if self.app_off != 0.0 {
			let sign = self.app_off.signum();
			let before = self.app_off.abs();
			let limit =
				(self.app_chase.burst > 0.0).then(|| self.app_chase.limit(before, dt_s, &cfg));
			self.app_mid -= self.app_mid * smoothing;
			let mut lag = before + (self.app_mid - before) * attack;
			if let Some(speed) = limit {
				lag = (before - speed * dt_s).max(0.0);
				self.app_mid = self.app_mid.min(lag);
			}
			if before < STOP_BAND {
				lag = lag.min((before - stop_min_lps * dt_s).max(0.0));
				self.app_mid = self.app_mid.min(lag);
			}
			if lag < config::SETTLE_EPS {
				lag = 0.0;
				self.app_mid = 0.0;
				self.app_chase = Chase::REST;
			}
			self.app_off = sign * lag;
		}
	}

	/// whole-line scrollback position the grid should snap to
	pub fn desired_offset(&self) -> usize {
		self.visual.floor().max(0.0) as usize
	}

	/// sub-line remainder in [0,1)
	pub fn frac(&self) -> f32 {
		self.visual - self.visual.floor()
	}

	pub fn animating(&self) -> bool {
		(self.target - self.visual).abs() > config::SETTLE_EPS || self.app_off != 0.0
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::MutexGuard;

	// The settings store starts from the live user config rather than the
	// built-in defaults, and a config with a fast tau made the whole module fail.
	// Each test pins the defaults first, and the guard serializes the module so a
	// test that pins something else can't race the rest. It's the store's shared
	// lock, because tests elsewhere put a rated profile live too.
	fn pin() -> MutexGuard<'static, ()> {
		let guard = config::test_store_lock();
		config::update(config::Settings::default());
		guard
	}

	fn ease_lines() -> f32 {
		config::settings().output_ease_lines.max(0.0)
	}

	// Test ID: dcg3v
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn chasing_output_is_the_output_ease_only() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(100.0);
		assert!(!s.chasing_output());
		s.nudge_output(3.0, 24.0);
		assert!(s.chasing_output());
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert!(!s.chasing_output(), "landed");
		s.nudge_output(3.0, 24.0);
		s.wheel(-1.0); // the user drives it back down: a sweep, not the chase
		assert!(!s.chasing_output());
	}

	// F150. A gesture aimed at the bottom eases in exactly the way output does,
	// so `following()` and `visual` both read as a lag while one is in flight.
	// Nothing printed, so nothing is unshown, whatever the view is doing.
	// Test ID: dcwcx
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_gesture_to_the_bottom_is_not_unshown_output() {
		let _g = pin();
		for jump in [true, false] {
			let mut s = Scroll::new();
			s.set_max(10_000.0);
			s.scroll_to(if jump { 500.0 } else { 120.0 });
			for _ in 0..2000 {
				s.advance(0.016);
			}
			assert_eq!(s.unshown_lines(), 0.0, "settled back, nothing printed");
			if jump {
				s.jump_bottom();
			} else {
				s.wheel(-120.0);
			}
			for _ in 0..400 {
				assert_eq!(
					s.unshown_lines(),
					0.0,
					"jump {jump}: trimmed at {}",
					s.visual_lines()
				);
				s.advance(0.016);
				if s.visual_lines() == 0.0 {
					break;
				}
			}
			assert_eq!(
				s.visual_lines(),
				0.0,
				"jump {jump}: the gesture never arrived"
			);
		}
	}

	// F153. Every keystroke calls `jump_bottom`, and a wheel notch at the bottom
	// does the same. Neither shows the user anything, so neither may cancel
	// a flood's trim. The fix for F150 read `chasing_output()`, which goes false
	// for the rest of the flood at the first one.
	// Test ID: dcwcy
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_flood_stays_trimmed_through_a_keystroke() {
		let _g = pin();
		for keystroke in [true, false] {
			let mut s = Scroll::new();
			s.set_max(10_000.0);
			for _ in 0..60 {
				s.nudge_output(10.0, 48.0);
				s.advance(0.016);
			}
			let built = s.unshown_lines();
			assert!(built > 100.0, "the flood built only {built} lines of lag");
			if keystroke {
				s.jump_bottom();
			} else {
				s.wheel(-1.0);
			}
			for frame in 0..240 {
				s.nudge_output(10.0, 48.0);
				s.advance(0.016);
				assert!(
					s.unshown_lines() > 100.0,
					"keystroke {keystroke}: trim gone at frame {frame}, {} of {}",
					s.unshown_lines(),
					s.visual_lines()
				);
				assert!(
					s.unshown_lines() <= s.visual_lines() + 1e-3,
					"keystroke {keystroke}: {} past the view at {}",
					s.unshown_lines(),
					s.visual_lines()
				);
			}
		}
	}

	// The count is what the map trims by, so it has to be the ease's own backlog
	// under a plain flood, and it has to drain to nothing once output stops. The
	// second half walks a mixed script and checks the invariants over all of it.
	// Test ID: dcwcz
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn unshown_output_drains_as_the_view_reaches_it() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(10_000.0);
		for _ in 0..60 {
			s.nudge_output(10.0, 48.0);
			s.advance(0.016);
		}
		assert!(
			(s.unshown_lines() - s.visual_lines()).abs() < 1e-3,
			"a plain flood is all backlog"
		);
		for _ in 0..4000 {
			s.advance(0.016);
		}
		assert_eq!(s.visual_lines(), 0.0, "the ease never arrived");
		assert_eq!(
			s.unshown_lines(),
			0.0,
			"the view reached the bottom with lines still unshown"
		);

		// flood, park 1000 lines back, flood stops, jump to the bottom
		let mut s = Scroll::new();
		s.set_max(10_000.0);
		let mut last = 0.0;
		let check = |s: &Scroll, last: &mut f32, printed: bool, note: &str| {
			let u = s.unshown_lines();
			assert!(
				u >= 0.0 && u <= s.visual_lines() + 1e-3,
				"{note}: {u} of {}",
				s.visual_lines()
			);
			if !printed {
				assert!(
					u <= *last + 1e-3,
					"{note}: rose to {u} from {last} with nothing printed"
				);
			}
			*last = u;
		};
		for _ in 0..40 {
			s.nudge_output(10.0, 48.0);
			s.advance(0.016);
			check(&s, &mut last, true, "flood");
		}
		assert!(
			s.unshown_draining(),
			"at the bottom with a backlog still to show"
		);
		s.scroll_to(1000.0);
		for _ in 0..600 {
			s.nudge_output(10.0, 48.0); // ignored while scrolled back
			s.advance(0.016);
			check(&s, &mut last, false, "parked");
		}
		// parked above the lines it hasn't reached, the count is frozen, so a
		// map held short by it has nothing more to draw until the view moves
		assert!(s.unshown_lines() > 0.0, "the flood left nothing unshown");
		assert!(!s.unshown_draining(), "parked, but the count still drains");
		s.jump_bottom();
		assert!(
			s.unshown_draining(),
			"heading back down past the unshown lines"
		);
		for _ in 0..4000 {
			s.advance(0.016);
			check(&s, &mut last, false, "coming back");
		}
		assert_eq!(s.visual_lines(), 0.0, "never settled");
		assert_eq!(
			s.unshown_lines(),
			0.0,
			"settled at the bottom with lines still unshown"
		);

		// the alt screen pins the view to 0, so nothing can be unshown there
		let mut s = Scroll::new();
		s.set_max(10_000.0);
		for _ in 0..40 {
			s.nudge_output(10.0, 48.0);
			s.advance(0.016);
		}
		assert!(s.unshown_lines() > 0.0);
		s.set_max(0.0);
		assert_eq!(s.unshown_lines(), 0.0);
	}

	// Smooth scrolling off: the grid already sits at the bottom, so there is no
	// lag to ease and nothing for the map to hold back.
	// Test ID: dcwd0
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn smooth_off_leaves_nothing_unshown() {
		let _g = pin();
		// a profile would mask the master switch, so pin the feel to custom
		with(config::Settings {
			scroll_smooth: false,
			..config::Settings::default()
		});
		let mut s = Scroll::new();
		s.set_max(10_000.0);
		for _ in 0..40 {
			s.nudge_output(10.0, 48.0);
			s.advance(0.016);
			assert_eq!(s.unshown_lines(), 0.0);
		}
		// leave the shared store on defaults for tests outside this module
		config::update(config::Settings::default());
	}

	// Test ID: d9b1z
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn starts_following() {
		let _g = pin();
		let s = Scroll::new();
		assert!(s.following());
		assert!(!s.animating());
		assert_eq!(s.desired_offset(), 0);
	}

	// Test ID: d9b20
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn wheel_clamps_to_history() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(10.0);
		s.wheel(25.0);
		assert!(!s.following());
		// target is private; observe via advance converging onto max
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert_eq!(s.desired_offset(), 10);
		s.jump_bottom();
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert!(s.following());
		assert_eq!(s.desired_offset(), 0);
		assert!(s.frac().abs() < 1e-3);
	}

	// Test ID: dabej
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn fractional_wheel_rests_on_a_whole_line() {
		let _g = pin();
		// pixel-delta wheels (touchpad, hi-res) send fractional line amounts; the
		// rest position must still be a whole line or every row renders sub-cell
		// shifted and the first clipped row's top peeks out at the content bottom
		let mut s = Scroll::new();
		s.set_max(100.0);
		s.wheel(2.6);
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert!(!s.animating());
		assert_eq!(s.desired_offset(), 3);
		assert!(s.frac().abs() < 1e-6, "frac {} at rest", s.frac());
		// accumulating many small fractional notches also ends on a line
		let mut t = Scroll::new();
		t.set_max(100.0);
		for _ in 0..7 {
			t.wheel(0.3);
		}
		for _ in 0..2000 {
			t.advance(0.016);
		}
		assert_eq!(t.desired_offset(), 3);
		assert!(t.frac().abs() < 1e-6);
	}

	// A gesture that stops just past a line used to be rounded back onto it, so
	// the view went all the way forward and then hopped a line the other way.
	// That's under a line of travel, but a visible reversal.
	// Test ID: dbsyc
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_wheel_settles_on_the_line_it_was_heading_for() {
		let _g = pin();
		let settle = |lines: f32| {
			let mut s = Scroll::new();
			s.set_max(100.0);
			s.wheel(lines.abs().ceil() + 4.0); // somewhere to scroll back down from
			for _ in 0..2000 {
				s.advance(0.016);
			}
			let from = s.desired_offset();
			s.wheel(lines);
			for _ in 0..2000 {
				s.advance(0.016);
			}
			(from, s.desired_offset())
		};
		let (from, to) = settle(2.1);
		assert_eq!(
			to,
			from + 3,
			"scrolling back ends on the far line, not back"
		);
		let (from, to) = settle(-2.1);
		assert_eq!(to, from - 3, "and the same going the other way");
	}

	// Test ID: daqtt
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn nudge_accumulates_without_a_line_cap() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(1000.0);
		s.nudge_output(1.0, 40.0);
		let after_one = s.frac() + s.desired_offset() as f32;
		assert!(after_one >= ease_lines().min(1.0) - 1e-3);
		// the backlog isn't capped: a line cap forces the view onto
		// the raw output rate the moment it fills, which defeats the slow start
		for _ in 0..100 {
			s.nudge_output(5.0, 40.0);
		}
		let lag = s.desired_offset() as f32 + s.frac();
		assert!(lag > MAX_BACKLOG, "backlog capped at {lag}");
		assert!(lag <= 501.0 + 1e-3);
	}

	// Test ID: d9b21
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn nudge_ignored_when_scrolled_back() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(100.0);
		s.wheel(50.0);
		let before = s.desired_offset() as f32 + s.frac();
		s.nudge_output(10.0, 40.0);
		let after = s.desired_offset() as f32 + s.frac();
		assert_eq!(before, after); // no-snap rule: output must not move a reader
	}

	// Test ID: d9b22
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn output_backlog_settles_to_bottom() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(1000.0);
		for _ in 0..10 {
			s.nudge_output(3.0, 40.0);
		}
		assert!(s.animating());
		for _ in 0..2000 {
			s.advance(0.016);
		}
		// eased all the way back down to following the live bottom
		assert!(s.following());
		assert_eq!(s.desired_offset(), 0);
		assert!(s.frac().abs() < 1e-3);
	}

	// Test ID: dar9v
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_burst_leaves_rest_slowly_with_the_ramp_still_ahead() {
		let _g = pin();
		// A dump used to hit full speed within ~100ms. The first quarter second
		// of even a deep, overflowed burst has to move, but only through
		// Ease-in's lift and the start of the ramp.
		let mut s = Scroll::new();
		s.set_max(10_000.0);
		s.nudge_output(200.0, 24.0); // a dump: deep and instantly overflowed
		let start = s.desired_offset() as f32 + s.frac();
		for _ in 0..15 {
			s.advance(0.016);
		}
		let covered = start - (s.desired_offset() as f32 + s.frac());
		assert!(covered > 0.1, "never started moving ({covered} lines)");
		// generous ceiling: the knee speed for the whole window, plus slack for
		// the ramp's first doublings
		assert!(
			covered < KNEE_LPS * 0.24 * 2.0,
			"first 0.24s covered {covered} lines - that is a jump, not a slow start"
		);
	}

	// Test ID: d9b23
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn burst_ramps_faster_than_trickle() {
		let _g = pin();
		// a sustained deep backlog must be moving measurably faster later in the
		// burst than the first speed a trickle eases at, from the exponential ramp
		let mut burst = Scroll::new();
		burst.set_max(10_000.0);
		burst.nudge_output(300.0, 24.0); // deep, overflowed: the chase is uncapped
		for _ in 0..62 {
			burst.advance(0.016); // ~1s: the chase has doubled a few times
		}
		let at_1s = burst.desired_offset() as f32 + burst.frac();
		for _ in 0..15 {
			burst.advance(0.016); // measure a 0.24s window at speed
		}
		let burst_window = at_1s - (burst.desired_offset() as f32 + burst.frac());
		assert!(
			burst_window > KNEE_LPS * 0.24 * 3.0,
			"one second in, a deep burst should far outrun the knee speed \
			 (covered {burst_window} lines in 0.24s; the knee would cover {})",
			KNEE_LPS * 0.24
		);
	}

	// Test ID: dc55n
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn app_scroll_grows_within_the_cover_and_eases_to_rest() {
		let _g = pin();
		let mut s = Scroll::new();
		s.app_scroll(3.0, 10.0, 40.0);
		assert_eq!(s.app_offset(), 3.0);
		assert!(s.animating());
		// steps stack, so the content on screen stays continuous; the strip's
		// cover is the ceiling
		s.app_scroll(3.0, 10.0, 40.0);
		assert_eq!(s.app_offset(), 6.0);
		s.app_scroll(99.0, 10.0, 40.0);
		assert_eq!(s.app_offset(), 10.0);
		// a step the other way starts over on that side
		s.app_scroll(-2.0, 10.0, 40.0);
		assert_eq!(s.app_offset(), -2.0);
		// eases back to 0 and stops animating
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert_eq!(s.app_offset(), 0.0);
		assert!(!s.animating());
	}

	// Test ID: dc55p
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn app_slide_keeps_up_with_a_fast_app() {
		let _g = pin();
		// A line every other frame for a while: the chase must ramp to the app's
		// rate (the lag stays well inside the strip) and, once it stops, wind the
		// slide down without a bounce.
		let mut s = Scroll::new();
		let mut max_lag = 0.0f32;
		for _ in 0..80 {
			s.app_scroll(1.0, SLIDE_ROWS as f32, 40.0);
			s.advance(0.016);
			s.advance(0.016);
			max_lag = max_lag.max(s.app_offset().abs());
		}
		assert!(max_lag < SLIDE_ROWS as f32 / 2.0, "lag {max_lag} ran away");
		let mut prev = s.app_offset();
		let mut frames = 0;
		while s.app_offset() != 0.0 {
			s.advance(0.016);
			assert!(
				s.app_offset() <= prev + 1e-4,
				"offset grew {prev} -> {}",
				s.app_offset()
			);
			prev = s.app_offset();
			frames += 1;
			assert!(frames < 240, "slide never settled ({prev} left after 4s)");
		}
	}

	// Test ID: dc55q
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn app_slide_and_output_chase_run_the_same_curve() {
		let _g = pin();
		// An app scrolling its region by N lines eases exactly like N lines of
		// plain output. Same burst into both, same frames, same path.
		let mut view = Scroll::new();
		view.set_max(400.0);
		let mut slide = Scroll::new();
		for step in [3.0, 1.0, 40.0, 2.0] {
			view.nudge_output(step, 40.0);
			slide.app_scroll(step, SLIDE_ROWS as f32, 40.0);
			for _ in 0..5 {
				view.advance(0.016);
				slide.advance(0.016);
				let gap = (view.visual_lines() - slide.app_offset()).abs();
				assert!(
					gap < 1e-3,
					"channels diverged: view {} slide {}",
					view.visual_lines(),
					slide.app_offset()
				);
			}
		}
		for _ in 0..600 {
			view.advance(0.016);
			slide.advance(0.016);
			let gap = (view.visual_lines() - slide.app_offset()).abs();
			assert!(
				gap < 1e-3,
				"channels diverged: view {} slide {}",
				view.visual_lines(),
				slide.app_offset()
			);
		}
		assert_eq!(slide.app_offset(), 0.0);
		assert_eq!(view.visual_lines(), 0.0);
	}

	// Test ID: d9j4c
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn output_ease_descends_monotonically() {
		let _g = pin();
		// After output stops, the view has to ease straight down to the live
		// bottom and never rise again. A rise mid-ease is the "page jumps around"
		// bug. The position mustn't go up on any frame and has to reach the
		// bottom, at both the in-view and the overflowed speeds.
		for rows in [40.0f32, 8.0] {
			let mut s = Scroll::new();
			s.set_max(1000.0);
			for _ in 0..4 {
				s.nudge_output(4.0, rows); // build a backlog to ease down from
			}
			let mut prev = s.desired_offset() as f32 + s.frac();
			assert!(prev > 0.0, "backlog should lift the view off the bottom");
			for _ in 0..3000 {
				s.advance(0.016);
				let pos = s.desired_offset() as f32 + s.frac();
				assert!(pos <= prev + 1e-4, "position rose {prev} -> {pos} (bounce)");
				prev = pos;
			}
			assert!(s.following());
			assert_eq!(s.desired_offset(), 0);
		}
	}

	// Test ID: daqya
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_single_screen_burst_eases_gentler_than_an_overflowed_one() {
		let _g = pin();
		// Same backlog, 2 screen heights: a burst that stays on screen, such as a
		// short listing, has to clear measurably slower than one whose first line
		// has scrolled off the top.
		let mut inview = Scroll::new();
		inview.set_max(1000.0);
		let mut over = Scroll::new();
		over.set_max(1000.0);
		for _ in 0..20 {
			inview.nudge_output(2.0, 60.0); // 40 lines on a 60-row pane: in view
			over.nudge_output(2.0, 10.0); // same lines on 10 rows: top scrolled off
		}
		let start = inview.desired_offset() as f32 + inview.frac();
		assert_eq!(start, over.desired_offset() as f32 + over.frac());
		for _ in 0..75 {
			// ~1.2s: past where the uncapped chase overtakes the in-view ceiling
			inview.advance(0.016);
			over.advance(0.016);
		}
		let left_i = inview.desired_offset() as f32 + inview.frac();
		let left_o = over.desired_offset() as f32 + over.frac();
		assert!(
			left_o < left_i,
			"overflowed burst ({left_o} left) should outrun the in-view one ({left_i} left)"
		);
	}

	// Test ID: daqpv
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_burst_ends_at_rest_and_the_next_starts_in_view() {
		let _g = pin();
		// Overflow must not outlive its burst: once the ease has settled at the
		// bottom, fresh output is a new burst and gets the gentle profile again.
		// A settled-then-nudged scroll must track a never-overflowed twin exactly.
		let mut s = Scroll::new();
		s.set_max(1000.0);
		s.nudge_output(12.0, 5.0); // overflows a 5-row pane at once
		for _ in 0..3000 {
			s.advance(0.016);
		}
		assert!(
			!s.animating(),
			"must be fully settled before the second burst"
		);
		let mut fresh = Scroll::new();
		fresh.set_max(1000.0);
		for _ in 0..4 {
			s.nudge_output(2.0, 40.0);
			fresh.nudge_output(2.0, 40.0);
			for _ in 0..3 {
				s.advance(0.016);
				fresh.advance(0.016);
			}
		}
		let pos_s = s.desired_offset() as f32 + s.frac();
		let pos_f = fresh.desired_offset() as f32 + fresh.frac();
		assert!(
			(pos_s - pos_f).abs() < 1e-4,
			"stale overflow leaked into the next burst: {pos_s} vs fresh {pos_f}"
		);
	}

	// Test ID: d9j4d
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn app_slide_eases_monotonically_without_overshoot() {
		let _g = pin();
		// A single app-scroll step has to glide to rest in one direction. The
		// offset only shrinks and never flips sign, which would bounce the content
		// back the other way.
		let mut s = Scroll::new();
		s.app_scroll(4.0, SLIDE_ROWS as f32, 40.0);
		let mut prev = s.app_offset();
		for _ in 0..3000 {
			s.advance(0.016);
			let off = s.app_offset();
			assert!(off >= -1e-4, "offset flipped negative: {off} (bounce)");
			assert!(off <= prev + 1e-4, "offset grew {prev} -> {off}");
			prev = off;
			if off == 0.0 {
				break;
			}
		}
		assert_eq!(s.app_offset(), 0.0);
	}

	// Test ID: d9hh7
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn cancel_app_scroll_hard_cuts_the_slide() {
		let _g = pin();
		// an alt-screen enter/exit must drop any in-flight slide at once (no ease)
		let mut s = Scroll::new();
		s.app_scroll(7.0, SLIDE_ROWS as f32, 40.0);
		assert!(s.animating());
		s.cancel_app_scroll();
		assert_eq!(s.app_offset(), 0.0);
		assert!(!s.animating());
	}

	// Test ID: damyc
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn snap_comes_to_rest_instantly() {
		let _g = pin();
		// unfreeze catch-up: pending output backlog and any slide drop at once
		let mut s = Scroll::new();
		s.set_max(50.0);
		s.nudge_output(12.0, 40.0);
		s.app_scroll(4.0, SLIDE_ROWS as f32, 40.0);
		assert!(s.animating());
		s.snap();
		assert!(!s.animating());
		assert_eq!(s.app_offset(), 0.0);
		assert_eq!(s.desired_offset(), 0);
		// scrolled back: snap leaves the position alone, kills only the motion
		s.wheel(20.0);
		s.snap();
		assert!(!s.animating());
		assert_eq!(s.desired_offset(), 20);
	}

	// Test ID: daqtv
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn smooth_off_finishes_every_scroll_instantly() {
		let _g = pin();
		// the master switch: no eased wheel, no output lag, no app slide
		with(config::Settings {
			scroll_smooth: false,
			..config::Settings::default()
		});
		let mut s = Scroll::new();
		s.set_max(100.0);
		s.wheel(30.0);
		s.advance(0.016);
		assert_eq!(s.desired_offset(), 30, "wheel must take effect instantly");
		assert!(!s.animating());
		s.jump_bottom();
		s.advance(0.016);
		assert_eq!(s.desired_offset(), 0);
		s.nudge_output(10.0, 40.0); // output produces no visual lag at all
		assert_eq!(s.desired_offset(), 0);
		assert!(s.frac().abs() < 1e-6);
		s.app_scroll(3.0, SLIDE_ROWS as f32, 40.0); // a slide request is dropped
		s.advance(0.016);
		assert_eq!(s.app_offset(), 0.0);
		assert!(!s.animating());
		// leave the shared store on defaults for tests outside this module
		config::update(config::Settings::default());
	}

	// Test ID: daqtw
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_user_jump_to_bottom_is_not_chase_limited() {
		let _g = pin();
		// Returning from deep scrollback is the user driving: the gap closes at
		// the full ease speed, never throttled to the output chase's slow start.
		let mut s = Scroll::new();
		s.set_max(1000.0);
		s.wheel(200.0);
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert_eq!(s.desired_offset(), 200);
		s.jump_bottom();
		// output keeps arriving while the jump eases home
		for _ in 0..125 {
			s.nudge_output(0.2, 40.0);
			s.advance(0.016);
		}
		// the chase's opening segments would take most of a minute over 200
		// lines; the configured ease does it in about a second
		let left = s.desired_offset() as f32 + s.frac();
		assert!(left < 30.0, "jump home crawled: {left} lines left after 2s");
	}

	// Test ID: daqrv
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_fresh_motion_builds_speed_instead_of_spiking() {
		let _g = pin();
		// The first frame of a new scroll has to be gentler than the motion a few
		// frames in. A bare exponential fails this, since its first frame is the
		// fastest of the whole ease.
		let mut s = Scroll::new();
		s.set_max(100.0);
		s.wheel(8.0);
		let dt = 0.016;
		let mut prev = 0.0f32;
		let mut steps = Vec::new();
		for _ in 0..8 {
			s.advance(dt);
			let pos = s.desired_offset() as f32 + s.frac();
			steps.push(pos - prev);
			prev = pos;
		}
		let first = steps[0];
		let peak = steps.iter().copied().fold(0.0f32, f32::max);
		assert!(
			first < peak * 0.6,
			"first frame moved {first} of peak {peak}: no ease-in"
		);
	}

	// Test ID: daqrw
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn the_tail_sweeps_in_instead_of_crawling() {
		let _g = pin();
		// Once within STOP_BAND of the target the rest has to close in well under
		// half a second. The bare exponential took over a second to close the
		// same distance at the default tau.
		let mut s = Scroll::new();
		s.set_max(100.0);
		s.wheel(6.0);
		let dt = 0.016;
		let mut in_band = 0;
		for _ in 0..3000 {
			s.advance(dt);
			if !s.animating() {
				break;
			}
			let pos = s.desired_offset() as f32 + s.frac();
			if (6.0 - pos).abs() < STOP_BAND {
				in_band += 1;
			}
		}
		assert!(!s.animating(), "never settled");
		let band_s = in_band as f32 * dt;
		assert!(
			band_s < 0.35,
			"tail took {band_s}s inside the stop band (crawl)"
		);
	}

	// The 4 feel knobs each have to change something, since 2 speed sliders were
	// once released doing nothing. Each test drives one setting's 2 extremes and
	// checks that they differ.
	fn with(mut cfg: config::Settings) {
		// the feel is read off the live settings, where a profile would mask it
		cfg.performance_profile = crate::profile::Profile::Custom;
		config::update(cfg);
	}

	// Test ID: daqyb
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn ease_in_sets_how_gently_motion_leaves_rest() {
		let _g = pin();
		let traveled = |ease_in_ms: f32| {
			with(config::Settings {
				scroll_ease_in_ms: ease_in_ms,
				..config::Settings::default()
			});
			let mut s = Scroll::new();
			s.set_max(100.0);
			s.wheel(20.0);
			for _ in 0..4 {
				s.advance(0.016);
			}
			s.visual
		};
		let abrupt = traveled(8.0);
		let gentle = traveled(800.0);
		with(config::Settings::default()); // other modules read this same store
		assert!(
			abrupt > gentle * 1.5,
			"leaving rest at ease-in 8ms ({abrupt}) should far outpace 800ms ({gentle})"
		);
	}

	// Test ID: dar9w
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn ease_in_also_paces_a_bursts_first_moments() {
		let _g = pin();
		// The same knob shapes the output curve's leave-from-rest: a longer
		// Ease-in is a shallower lift to the knee, so the burst's opening
		// covers less ground. Everything past the knee is Ramp-up's business.
		let covered = |ease_in_ms: f32| {
			with(config::Settings {
				scroll_ease_in_ms: ease_in_ms,
				..config::Settings::default()
			});
			let mut s = Scroll::new();
			s.set_max(10_000.0);
			s.nudge_output(200.0, 24.0);
			let start = s.desired_offset() as f32 + s.frac();
			for _ in 0..9 {
				s.advance(0.016); // ~0.14s: inside the gentle arm's lift
			}
			start - (s.desired_offset() as f32 + s.frac())
		};
		let crisp = covered(8.0);
		let soft = covered(800.0);
		with(config::Settings::default());
		assert!(
			crisp > soft * 3.0,
			"a crisp ease-in ({crisp} lines) should far outpace a soft one ({soft}) leaving rest"
		);
	}

	// Test ID: daqyc
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn ramp_up_sets_how_fast_catch_up_accelerates() {
		let _g = pin();
		let left = |ramp_up_ms: f32| {
			with(config::Settings {
				scroll_ramp_up_ms: ramp_up_ms,
				..config::Settings::default()
			});
			let mut s = Scroll::new();
			s.set_max(1000.0);
			s.nudge_output(300.0, 24.0); // deep and instantly overflowed: uncapped
			for _ in 0..60 {
				s.advance(0.016); // ~1s of ramping
			}
			s.visual
		};
		let hard = left(50.0);
		let soft = left(1500.0);
		with(config::Settings::default());
		assert!(
			hard < soft * 0.5,
			"a hard ramp ({hard} lines left) should outrun a soft one ({soft} left)"
		);
	}

	// Test ID: dar9x
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn ramp_down_sets_how_gradually_speed_winds_down() {
		let _g = pin();
		// Ramp-down is the braking curve, traced backwards from the ease-out
		// handoff: a gentler (longer) setting leaves the view a longer braking
		// distance behind the live bottom and takes correspondingly longer to
		// wind the same backlog down once output has ceased. Frames from
		// "output stopped" to rest is the measurement.
		let frames_to_rest = |ramp_down_ms: f32| {
			with(config::Settings {
				scroll_ramp_down_ms: ramp_down_ms,
				..config::Settings::default()
			});
			let mut s = Scroll::new();
			s.set_max(5000.0);
			s.nudge_output(400.0, 24.0); // deep, instantly overflowed
			for _ in 0..80 {
				s.advance(0.016); // ~1.3s: well up to speed
			}
			let mut frames = 0;
			while s.animating() && frames < 20_000 {
				s.advance(0.016);
				frames += 1;
			}
			assert!(!s.animating(), "never settled at ramp-down {ramp_down_ms}");
			frames
		};
		let quick = frames_to_rest(60.0);
		let slow = frames_to_rest(3000.0);
		with(config::Settings::default());
		assert!(
			slow > quick * 2,
			"a gentle ramp-down should wind down far longer ({slow} frames) than a hard one ({quick})"
		);
	}

	// Test ID: dar9y
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_stopped_burst_decelerates_instead_of_stopping_dead() {
		let _g = pin();
		// The reserve: while output pours in at a steady rate, the braking cap
		// leaves the view a wind-down's distance behind the bottom, so when
		// output stops the speed follows the ramp-down curve instead of dropping
		// at once. Reach the steady state first, then stop feeding and watch.
		let mut s = Scroll::new();
		s.set_max(50_000.0);
		for _ in 0..250 {
			s.nudge_output(1.0, 24.0); // ~62 lines/s, long enough to equalize
			s.advance(0.016);
		}
		let reserve = s.desired_offset() as f32 + s.frac();
		assert!(
			reserve > 5.0,
			"at speed the view should trail by a braking distance, not {reserve} lines"
		);
		let mut prev_pos = reserve;
		let mut prev_v = f32::MAX;
		let mut frames_moving = 0;
		for _ in 0..3000 {
			s.advance(0.016);
			let pos = s.desired_offset() as f32 + s.frac();
			let v = (prev_pos - pos) / 0.016;
			if v <= 0.5 {
				break;
			}
			assert!(
				v < prev_v * 1.05 + 0.1,
				"speed rose mid wind-down: {prev_v} -> {v} lines/s"
			);
			frames_moving += 1;
			prev_v = v;
			prev_pos = pos;
		}
		assert!(
			frames_moving > 20,
			"the wind-down lasted only {frames_moving} frames - a cliff, not a ramp-down"
		);
	}

	// Test ID: daqyd
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn ease_out_sets_how_gently_the_tail_settles() {
		let _g = pin();
		// Frames spent inside the stop band, which is only the tail. The approach
		// above the band is the same either way.
		let band_frames = |ease_out_ms: f32| {
			with(config::Settings {
				scroll_ease_out_ms: ease_out_ms,
				..config::Settings::default()
			});
			let mut s = Scroll::new();
			s.set_max(100.0);
			s.wheel(6.0);
			let mut in_band = 0;
			for _ in 0..3000 {
				s.advance(0.016);
				if !s.animating() {
					break;
				}
				if (6.0 - s.visual).abs() < STOP_BAND {
					in_band += 1;
				}
			}
			assert!(!s.animating(), "never settled at ease-out {ease_out_ms}");
			in_band
		};
		let crisp = band_frames(20.0);
		let soft = band_frames(800.0);
		with(config::Settings::default());
		assert!(
			soft > crisp,
			"a soft ease-out should linger in the tail ({soft} frames) longer than a crisp one ({crisp})"
		);
	}

	// Test ID: d9b24
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn set_max_clamps_positions() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(100.0);
		s.wheel(80.0);
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert_eq!(s.desired_offset(), 80);
		// history shrank (e.g. clear/reset): both target and visual clamp
		s.set_max(5.0);
		for _ in 0..2000 {
			s.advance(0.016);
		}
		assert!(s.desired_offset() <= 5);
	}

	// A burst still easing when an alt-screen app takes over: the alt grid has no
	// scrollback, so there is nothing to ease through. The view must be at rest
	// on the spot. Left past the grid, its fraction wrapped through a whole cell
	// once per line of leftover backlog, which was the nano wobble.
	// Test ID: dbtpf
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn an_alt_screen_entry_puts_a_running_ease_at_rest() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(1000.0);
		s.nudge_output(400.0, 34.0);
		s.advance(0.016);
		assert!(s.animating());
		s.set_max(0.0);
		assert!(!s.animating());
		assert_eq!(s.desired_offset(), 0);
		assert!(s.frac().abs() < 1e-6);
		s.advance(0.016);
		assert!(!s.animating() && s.frac().abs() < 1e-6);
	}

	// A prompt coming back after a command is a 2-line advance, and it used to
	// be the one motion no setting could change: the chase only capped the
	// navigation ease, so any advance the fixed 230ms cascade was slower than
	// got the cascade instead. It crawled almost to a stop, then visibly
	// restarted when the stop band took over. Both halves are checked here.
	// Test ID: dc6ed
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_short_advance_is_one_motion_and_the_settings_reach_it() {
		let _g = pin();
		let steps = |cfg: config::Settings| {
			with(cfg);
			let mut s = Scroll::new();
			s.set_max(1000.0);
			s.nudge_output(2.0, 45.0);
			let mut out = vec![];
			let mut last = s.visual;
			for _ in 0..600 {
				s.advance(1.0 / 60.0);
				out.push(last - s.visual);
				last = s.visual;
				if !s.animating() {
					break;
				}
			}
			out
		};
		let brisk = steps(config::Settings::default());
		// speed rises to a peak and then only falls: no stall-and-restart
		let peak = brisk
			.iter()
			.enumerate()
			.max_by(|a, b| a.1.total_cmp(b.1))
			.map(|(i, _)| i)
			.unwrap();
		for w in brisk[peak..].windows(2) {
			assert!(
				w[1] <= w[0] + 1e-4,
				"speed picked back up after slowing: {brisk:?}"
			);
		}
		let slack = steps(config::Settings {
			scroll_ease_in_ms: 600.0,
			scroll_ramp_up_ms: 600.0,
			scroll_ramp_down_ms: 600.0,
			scroll_ease_out_ms: 600.0,
			..config::Settings::default()
		});
		with(config::Settings::default()); // other modules read this same store
		assert!(
			slack.len() > brisk.len() * 2,
			"the feel settings must govern a 2-line advance: {} frames brisk vs {} slack",
			brisk.len(),
			slack.len()
		);
	}

	// Same rule at the other end: a shallow history caps the ease floor, so the
	// view never renders a line the grid can't show.
	// Test ID: dbtpg
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn the_ease_floor_is_capped_by_the_history() {
		let _g = pin();
		let mut s = Scroll::new();
		s.set_max(1.0);
		s.nudge_output(1.0, 34.0);
		for _ in 0..500 {
			let pos = s.desired_offset() as f32 + s.frac();
			assert!(pos <= 1.0 + 1e-6, "view sits past the grid at {pos}");
			s.advance(0.016);
		}
		assert!(s.following());
	}
}
