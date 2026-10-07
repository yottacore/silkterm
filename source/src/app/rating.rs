// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The performance benchmark and the profile it picks, and the display watch
//! that steps a slow session down.

use super::{BENCH_BANNER_MIN, BENCH_BANNER_PAD, State, set_live};
use crate::config;
use crate::gfx::Drawn;
use crate::pane::Rect;
use std::time::Instant;

/// A remote screen wears the Remote profile for the session. Nothing is written
/// and nothing is rated: the console keeps the profile it had, and the override
/// lifts at the next launch unless that one is remote too.
pub(super) fn remote_override_at_launch() {
	if !crate::profile::remote_session() {
		return;
	}
	set_live(|live| live.remote_override = true);
}

/// What the performance watch does with a pass of the event loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RatingStep {
	Note,
	Pause,
}

/// A frame is evidence about the hardware only when this window's own eased
/// rendering paced it. A benchmark is timing the same frames itself, a pinned
/// rate paces itself, and a window without focus is one nobody is watching, so
/// it gets no say in the profile.
#[allow(clippy::fn_params_excessive_bools)] // four independent gates, all sixteen cases tested
pub(super) fn rating_step(
	bench: bool,
	scroll_anim: bool,
	pinned_fps: bool,
	focused: bool,
) -> RatingStep {
	if !bench && scroll_anim && !pinned_fps && focused {
		RatingStep::Note
	} else {
		RatingStep::Pause
	}
}

/// With the profile on automatic, hardware the config has not seen gets a fresh
/// pick, written down against that hardware so the next launch on it leaves the
/// profile where the rating left it. Answers the id a benchmark should write
/// when it finishes, or None where there is nothing to time - a software
/// renderer is decided here and written now, and a remote screen is left alone.
pub(super) fn rate_hardware(info: &wgpu::AdapterInfo) -> Option<String> {
	let live = config::settings();
	if !live.performance_automatic || live.remote_override {
		return None;
	}
	let hardware = crate::profile::hardware_id(info);
	if !rating_due(&live, &hardware) {
		return None;
	}
	// The one-shot check counts as new hardware, and clears itself here rather
	// than with the answer: a window closed mid-run has still had its run.
	if live.performance_check_next_run {
		let kept = config::keep_rating(&config::RatingLines {
			check_next_run: Some(false),
			..config::RatingLines::default()
		});
		note_rating_not_kept(&kept);
		set_live(|live| live.performance_check_next_run = false);
	}
	if crate::profile::worth_measuring(info) {
		// Nothing is written yet. The id goes down with the measured answer, so a
		// window closed mid-run is measured again next launch instead of leaving
		// the heaviest rung recorded as this machine's rating.
		return Some(hardware);
	}
	let pick = crate::profile::first_pick(info);
	// no banner on this path, and redoing it next launch costs nothing
	let kept = config::keep_rating(&config::RatingLines {
		profile: Some(pick.key()),
		rated_hardware: Some(&hardware),
		check_next_run: None,
	});
	note_rating_not_kept(&kept);
	set_live(|live| {
		live.rated_hardware = hardware;
		live.performance_profile = pick;
	});
	None
}

// Whether this launch owes a rating. A machine already rated once is only
// re-rated when asked for; the first rating has to happen either way, or there
// is no profile at all.
fn rating_due(live: &config::Settings, hardware: &str) -> bool {
	if !live.performance_automatic || live.remote_override {
		return false;
	}
	if live.performance_check_next_run {
		return true;
	}
	if live.rated_hardware == hardware {
		return false;
	}
	live.performance_check_hardware || live.rated_hardware.is_empty()
}

/// The session's stepped profile for a device drawn `drawn`, or None to leave
/// it. Software on a machine with a card is not new hardware, so the card keeps
/// its rating and the session steps down to Low, the way a remote screen takes
/// Remote. A step made here (`ours`) comes off when the card draws again, and
/// a deeper step the display watch took since stays.
pub(super) fn session_step(
	live: &config::Settings,
	drawn: Drawn,
	ours: bool,
) -> Option<Option<crate::profile::Profile>> {
	use crate::profile::Profile;
	if !live.performance_automatic {
		return None;
	}
	if drawn.instead_of_card() {
		let deeper = live
			.stepped_profile
			.is_some_and(|step| step.index() >= Profile::Low.index());
		return (!deeper).then_some(Some(Profile::Low));
	}
	(ours && live.stepped_profile == Some(Profile::Low)).then_some(None)
}

// A measured answer into the settings file. Its own function so a test can run
// the same write the banner's run does.
fn keep_measured(pick: crate::profile::Profile, id: Option<&str>) -> config::Kept {
	config::keep_rating(&config::RatingLines {
		profile: Some(pick.key()),
		rated_hardware: id,
		check_next_run: None,
	})
}

// Every rating write reports a failure, since a rating that is not kept is a
// test again at the next launch, and a Windows release build shows no stderr
// (G37) - which is why the banner says it too.
fn note_rating_not_kept(kept: &config::Kept) {
	let reason = match kept {
		config::Kept::Written => return,
		config::Kept::Busy => "the settings file is open in another program",
		config::Kept::Unreadable => "the settings file has a line that cannot be read",
		config::Kept::Unplaced => {
			"the performance section of the settings file could not be updated"
		}
		config::Kept::Unwritable(why) => why.as_str(),
	};
	eprintln!(
		"{}: performance rating not saved ({reason}); the test runs again at the next launch",
		config::APP_NAME
	);
}

// What the benchmark's banner says: that a run is on, or, for its last few
// seconds, why its answer could not be kept.
fn bench_banner_lines(kept: Option<&config::Kept>) -> &'static [&'static str] {
	const AGAIN: &str = "The test runs again at the next launch.";
	match kept {
		None | Some(config::Kept::Written) => &["Testing performance", "This takes a few seconds."],
		Some(config::Kept::Busy) => &[
			"Could not save the result",
			"The settings file is open in another program.",
			AGAIN,
		],
		Some(config::Kept::Unreadable) => &[
			"Could not save the result",
			"The settings file has a line that cannot be read.",
			AGAIN,
		],
		Some(config::Kept::Unplaced) => &[
			"Could not save the result",
			"The performance section of the settings file could not be updated.",
			AGAIN,
		],
		Some(config::Kept::Unwritable(_)) => &[
			"Could not save the result",
			"The settings file cannot be written.",
			AGAIN,
		],
	}
}

// What the banner says after a run the display stalled.
const BENCH_STALLED_LINES: &[&str] = &[
	"Could not test performance",
	"The display was not drawing at its usual rate.",
	"The test runs again at the next launch.",
];

// The user's own settings with a measured profile stored in them: a benchmark
// rung while it is timed, or the answer once the run ends. A measurement replaces
// any step the display watch took, and a step left in place would sit over the
// rung and time the wrong one.
fn with_measured_profile(
	live: &config::Settings,
	profile: crate::profile::Profile,
) -> config::Settings {
	let mut next = live.clone();
	crate::profile::unapply(&mut next);
	next.performance_profile = profile;
	next.stepped_profile = None;
	next
}

// The live settings with the display watch's next step in force, or None when
// automatic is off or the watch has no rung left. Only the session field moves:
// a step written to the file became every later launch's profile.
fn watch_step_down(live: &config::Settings) -> Option<config::Settings> {
	if !live.performance_automatic {
		return None;
	}
	let lower = crate::profile::current(live).watched_lower()?;
	let mut next = live.clone();
	next.stepped_profile = Some(lower);
	Some(next)
}

impl State {
	/// The benchmark's banner: one box in the middle of the window, over a dimmed
	/// screen. It is what makes the run modal - the window behind it keeps drawing
	/// (that is the thing being timed, and it is worth seeing) but takes no input
	/// while it is up.
	pub(super) fn bench_layout(&mut self) -> Option<(Rect, Vec<(f32, f32, String)>)> {
		self.bench_banner.as_ref()?;
		let lines = if self.bench_stalled {
			BENCH_STALLED_LINES
		} else {
			bench_banner_lines(self.bench_kept.as_ref())
		};
		let attrs = crate::text::ui_attrs();
		let pad = self.text.dip(BENCH_BANNER_PAD);
		let line_h = self.text.ui_line_h;
		let text_w = lines.iter().fold(0.0f32, |widest, line| {
			widest.max(self.text.measure_ui_text(line, &attrs))
		});
		let w = text_w + 2.0 * pad;
		let h = line_h * lines.len() as f32 + 2.0 * pad;
		let (win_w, win_h) = (self.surface_px.0 as f32, self.surface_px.1 as f32);
		let (x, y) = (
			((win_w - w) / 2.0).max(0.0).round(),
			((win_h - h) / 2.0).max(0.0).round(),
		);
		let placed = lines
			.iter()
			.enumerate()
			.map(|(i, line)| {
				let left = x + (w - self.text.measure_ui_text(line, &attrs)) / 2.0;
				(
					left.round(),
					y + pad + line_h * i as f32,
					(*line).to_string(),
				)
			})
			.collect();
		Some((Rect { x, y, w, h }, placed))
	}

	/// The rating waits for the wallpaper, since that is part of what it times,
	/// but no longer than its cap.
	pub(super) fn bench_blocked(&self) -> bool {
		!self.wp_shown && self.bench_cap.is_some_and(|cap| Instant::now() < cap)
	}

	/// The banner comes down once the run is over and it has been up long
	/// enough to read.
	pub(super) fn bench_banner_wake(&self) -> Option<Instant> {
		self.bench_banner
			.filter(|_| self.bench.is_none() && self.bench_at.is_none())
			.map(|up| up + BENCH_BANNER_MIN)
	}

	/// The Remote profile on or off by hand. Live only: nothing about it reaches
	/// the file, so the next launch decides for itself.
	pub(super) fn toggle_remote(&mut self) {
		let before = config::settings();
		let mut next = (*before).clone();
		next.remote_override = !next.remote_override;
		self.apply_new_settings(&before, next, false);
	}

	/// Put a rung's settings live for the length of the benchmark. Nothing is
	/// written: the run is a measurement and only its answer reaches the file.
	pub(super) fn set_live_profile(&mut self, profile: crate::profile::Profile) {
		let before = config::settings();
		let next = with_measured_profile(&before, profile);
		self.apply_new_settings(&before, next, false);
	}

	/// The benchmark settled on a rung. Write it down against the hardware it was
	/// measured on, and give the window back.
	pub(super) fn finish_bench(&mut self, pick: crate::profile::Profile) {
		self.bench = None;
		self.rating.reset();
		eprintln!(
			"{}: performance profile measured for this hardware: {}",
			config::APP_NAME,
			pick.label()
		);
		let orig = (*config::settings()).clone();
		let mut new = with_measured_profile(&orig, pick);
		let id = self.bench_id.take();
		let kept = keep_measured(pick, id.as_deref());
		if let Some(id) = id {
			new.rated_hardware = id;
		}
		if kept == config::Kept::Written {
			self.bench_kept = None;
		} else {
			// The banner stays up long enough to read why, from now, since this is
			// the one place a person learns the test will run again.
			note_rating_not_kept(&kept);
			self.bench_kept = Some(kept);
			self.bench_banner = Some(Instant::now());
			self.dirty = true;
		}
		self.apply_new_settings(&orig, new, false);
	}

	/// The run could not tell the machine from the display (`Step::Stalled`), most
	/// likely a monitor asleep. Nothing is written, so the next launch tests again,
	/// and the session goes back to the profile it had. Saving Standard here once
	/// left a machine without its wallpaper from then on.
	pub(super) fn finish_bench_stalled(&mut self) {
		self.bench = None;
		self.bench_id = None;
		self.rating.reset();
		eprintln!(
			"{}: performance test gave no answer (the display was not drawing at its usual rate); it runs again at the next launch",
			config::APP_NAME
		);
		self.bench_stalled = true;
		self.bench_kept = None;
		self.bench_banner = Some(Instant::now());
		self.dirty = true;
		if let Some(from) = self.bench_from.take() {
			self.set_live_profile(from);
		}
	}

	/// The display missed its budget over a whole window of eased frames: with
	/// the profile on automatic, take one step down for the rest of the session.
	/// Nothing is written, so the next launch starts from the rated profile.
	pub(super) fn step_down_profile(&mut self) {
		let live = config::settings();
		let Some(next) = watch_step_down(&live) else {
			return;
		};
		eprintln!(
			"{}: the display is not keeping up; performance profile stepped down to {} until {} restarts",
			config::APP_NAME,
			crate::profile::current(&next).label(),
			config::APP_NAME
		);
		self.apply_new_settings(&live, next, false);
	}
}

#[cfg(test)]
mod tests {
	use super::rating_step;
	use crate::config;
	// Only a focused window's own eased frame is evidence; every other pass
	// pauses the watch, so an idle gap is never read as a period.
	// Test ID: EpWnbLc
	#[test]
	fn only_a_focused_eased_unpinned_frame_is_counted() {
		use super::RatingStep;
		let mut notes = 0;
		for bits in 0..16u8 {
			let (bench, scroll, pinned, focused) =
				(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
			let step = rating_step(bench, scroll, pinned, focused);
			if (bench, scroll, pinned, focused) == (false, true, false, true) {
				assert_eq!(step, RatingStep::Note);
				notes += 1;
			} else {
				assert_eq!(
					step,
					RatingStep::Pause,
					"bench {bench} scroll {scroll} pinned {pinned} focused {focused}"
				);
			}
		}
		assert_eq!(notes, 1);
	}

	// A bench rung and the bench's answer are both measured, so a step the
	// display watch took goes, and the stored profile is the user's own values.
	// Test ID: EpWow4e
	#[test]
	fn a_measured_profile_replaces_a_session_step() {
		use crate::profile::Profile;
		let mut live = config::Settings {
			performance_profile: crate::profile::Profile::Max,
			stepped_profile: Some(Profile::Low),
			..config::Settings::default()
		};
		crate::profile::apply(&mut live);
		assert_eq!(crate::profile::current(&live), Profile::Low);
		let next = super::with_measured_profile(&live, Profile::High);
		assert_eq!(next.stepped_profile, None);
		assert_eq!(next.performance_profile, crate::profile::Profile::High);
		assert!(next.profile_shadow.is_none(), "the user's own values");
		assert_eq!(crate::profile::current(&next), Profile::High);
	}

	// The watch's step is session state. Stored in the profile or written out,
	// one stall became every later launch's profile and took the wallpaper.
	// Test ID: EpX5Wgq
	#[test]
	fn a_watch_step_never_reaches_the_stored_profile_or_the_file() {
		use crate::profile::Profile;
		let _guard = config::test_config_lock();
		let _ = config::settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_appstep_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		config::set_config_override(path.clone());
		let cases = [
			(true, "max", Some(Profile::High)),
			(true, "high", Some(Profile::Low)),
			(true, "low", None),
			(true, "standard", None),
			(true, "custom", None),
			(false, "max", None),
		];
		for (automatic, stored, want) in cases {
			std::fs::write(
				&path,
				format!("performance:\n\tautomatic: {automatic}\n\tprofile: \"{stored}\"\n"),
			)
			.unwrap();
			let mut live = config::reload_from_disk();
			crate::profile::apply(&mut live);
			let before = std::fs::read_to_string(&path).unwrap();
			let next = super::watch_step_down(&live);
			assert_eq!(
				std::fs::read_to_string(&path).unwrap(),
				before,
				"automatic {automatic}, stored {stored}: taking the step writes nothing"
			);
			assert_eq!(
				next.as_ref().and_then(|n| n.stepped_profile),
				want,
				"automatic {automatic}, stored {stored}"
			);
			let Some(next) = next else {
				continue;
			};
			assert_eq!(
				next.performance_profile.key(),
				stored,
				"the stored profile stays"
			);
			assert_eq!(Some(crate::profile::current(&next)), want);
			assert!(config::persist(&live, &next));
			assert_eq!(
				std::fs::read_to_string(&path).unwrap(),
				before,
				"stored {stored}: nor does saving the settings it is in"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	fn adapter(name: &str, device_type: wgpu::DeviceType) -> wgpu::AdapterInfo {
		wgpu::AdapterInfo {
			name: name.to_string(),
			vendor: 0,
			device: 0,
			device_type,
			device_pci_bus_id: String::new(),
			driver: String::new(),
			driver_info: String::new(),
			backend: wgpu::Backend::Gl,
			subgroup_min_size: 0,
			subgroup_max_size: 0,
			transient_saves_memory: false,
		}
	}

	// Which launches owe a rating. Pulled out of the launch path when every write
	// in it changed, so the decision itself provably did not.
	// Software on a machine with a card steps the session to Low and writes
	// nothing; the card drawing again takes off only a step made that way.
	// Test ID: ErnMaCH
	#[test]
	fn a_software_device_steps_the_session_and_the_card_takes_it_back() {
		use super::session_step;
		use crate::gfx::Drawn;
		use crate::profile::Profile;
		let live = |automatic: bool, stepped: Option<Profile>| config::Settings {
			performance_automatic: automatic,
			stepped_profile: stepped,
			..config::Settings::default()
		};
		for drawn in [Drawn::Software, Drawn::Fallback] {
			assert_eq!(
				session_step(&live(true, None), drawn, false),
				Some(Some(Profile::Low))
			);
			assert_eq!(
				session_step(&live(true, Some(Profile::High)), drawn, false),
				Some(Some(Profile::Low))
			);
			// the display watch already went deeper
			assert_eq!(
				session_step(&live(true, Some(Profile::Standard)), drawn, false),
				None
			);
			assert_eq!(
				session_step(&live(true, Some(Profile::Low)), drawn, true),
				None
			);
			// a profile picked by hand is left alone
			assert_eq!(session_step(&live(false, None), drawn, false), None);
		}
		for drawn in [Drawn::Card, Drawn::NoCard] {
			assert_eq!(
				session_step(&live(true, Some(Profile::Low)), drawn, true),
				Some(None)
			);
			// the watch's own step, or a deeper one since, stays
			assert_eq!(
				session_step(&live(true, Some(Profile::Low)), drawn, false),
				None
			);
			assert_eq!(
				session_step(&live(true, Some(Profile::Standard)), drawn, true),
				None
			);
			assert_eq!(session_step(&live(true, None), drawn, true), None);
		}
	}

	// Test ID: EpXN9p2
	#[test]
	fn rating_due_matches_the_launch_rules() {
		let hardware = "0123456789abcdef";
		let base = config::Settings {
			performance_automatic: true,
			performance_check_hardware: true,
			performance_check_next_run: false,
			rated_hardware: "fedcba9876543210".to_string(),
			remote_override: false,
			..config::Settings::default()
		};
		let matching = config::Settings {
			rated_hardware: hardware.to_string(),
			..base.clone()
		};
		let cases = [
			(
				"automatic off",
				config::Settings {
					performance_automatic: false,
					..base.clone()
				},
				false,
			),
			(
				"a remote screen",
				config::Settings {
					remote_override: true,
					..base.clone()
				},
				false,
			),
			(
				"asked for, even on the same hardware",
				config::Settings {
					performance_check_next_run: true,
					..matching.clone()
				},
				true,
			),
			("the same hardware", matching.clone(), false),
			("other hardware, checked", base.clone(), true),
			(
				"other hardware, not checked",
				config::Settings {
					performance_check_hardware: false,
					..base.clone()
				},
				false,
			),
			(
				"never rated, not checked",
				config::Settings {
					performance_check_hardware: false,
					rated_hardware: String::new(),
					..base.clone()
				},
				true,
			),
		];
		for (what, live, due) in cases {
			assert_eq!(super::rating_due(&live, hardware), due, "{what}");
		}
	}

	// A rating that did not reach the file was a test at every launch. Each file
	// here is one that used to lose it or read it back as nothing: a clean one, one
	// with a line the parse cannot place, and one with the key twice.
	// Test ID: EpXN9p3
	#[test]
	fn a_rating_survives_to_the_next_launch_whatever_else_is_in_the_file() {
		use crate::profile::Profile;
		let _guard = config::test_config_lock();
		let saved = config::settings();
		let _store = config::test_store_lock();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ratingkept_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("config.shcl");
		config::set_config_override(path.clone());
		let card = adapter("NVIDIA GeForce RTX 3060 Ti", wgpu::DeviceType::DiscreteGpu);
		let soft = adapter("llvmpipe (LLVM 19.1.7, 256 bits)", wgpu::DeviceType::Cpu);
		// what a launch does before it rates: read the file and put it live
		let install = || config::update(config::reload_from_disk());
		let read = || std::fs::read_to_string(&path).unwrap();
		let files = [
			(
				"clean",
				"performance:\n\t# rated_hardware: \"\"  ## Default\n",
			),
			(
				"an unreadable line",
				"window:\n\topacity: 1.0\n    margin: 4\n\nperformance:\n\t# rated_hardware: \"\"  ## Default\n",
			),
			(
				"the key twice",
				"performance:\n\trated_hardware: 0000000000000000\n\trated_hardware: 0000000000000000\n",
			),
		];
		for (name, file) in files {
			std::fs::write(&path, file).unwrap();
			install();
			let id = super::rate_hardware(&card)
				.unwrap_or_else(|| panic!("{name}: a card with no rating on file is measured"));
			assert_eq!(
				super::keep_measured(Profile::High, Some(&id)),
				config::Kept::Written,
				"{name}"
			);
			install();
			assert_eq!(
				super::rate_hardware(&card),
				None,
				"{name}: the next launch keeps the card's rating"
			);

			std::fs::write(&path, file).unwrap();
			install();
			match super::rate_hardware(&soft) {
				Some(id) if crate::profile::worth_measuring(&soft) => assert_eq!(
					super::keep_measured(Profile::Low, Some(&id)),
					config::Kept::Written,
					"{name}"
				),
				answer => assert_eq!(
					answer, None,
					"{name}: a software adapter is decided at launch"
				),
			}
			install();
			let stored = config::reload_from_disk();
			assert_eq!(
				stored.performance_profile,
				crate::profile::Profile::Low,
				"{name}"
			);
			assert_eq!(
				stored.rated_hardware,
				crate::profile::hardware_id(&soft),
				"{name}"
			);
			let before = read();
			assert_eq!(
				super::rate_hardware(&soft),
				None,
				"{name}: the next launch keeps the software adapter's rating"
			);
			assert_eq!(read(), before, "{name}: and writes nothing");
			if name == "an unreadable line" {
				assert!(
					before.contains("\n    margin: 4\n"),
					"the unreadable line is still there:\n{before}"
				);
			}
		}
		config::update((*saved).clone());
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A failed save used to leave the banner saying the test was running and then
	// run it again next launch. It says why now, for the seconds it stays up.
	// Test ID: EpXN9p4
	#[test]
	fn the_banner_says_why_a_rating_was_not_kept() {
		const AGAIN: &str = "The test runs again at the next launch.";
		let running = ["Testing performance", "This takes a few seconds."];
		assert_eq!(super::bench_banner_lines(None), running);
		assert_eq!(
			super::bench_banner_lines(Some(&config::Kept::Written)),
			running
		);
		for (kept, why) in [
			(
				config::Kept::Busy,
				"The settings file is open in another program.",
			),
			(
				config::Kept::Unreadable,
				"The settings file has a line that cannot be read.",
			),
			(
				config::Kept::Unplaced,
				"The performance section of the settings file could not be updated.",
			),
			(
				config::Kept::Unwritable("could not write x: denied".to_string()),
				"The settings file cannot be written.",
			),
		] {
			assert_eq!(
				super::bench_banner_lines(Some(&kept)),
				["Could not save the result", why, AGAIN],
				"{kept:?}"
			);
		}
	}
}
