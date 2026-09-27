// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Performance profiles: one setting that decides how much the look is allowed
//! to cost, and the rating that picks it on a machine that cannot keep up.
//!
//! A profile sits ON TOP of the stored settings rather than in them. The file
//! and the dialog keep the user's own values; `apply` overwrites the fields a
//! profile governs when settings go live and keeps the originals in a shadow,
//! so any code that reads the live settings and writes them back cannot leak
//! a profile's value into the file. Choosing Custom is then just a profile
//! that governs nothing.
//!
//! The rating watches how a scroll ease is paced. A display that keeps its
//! refresh rate paces one frame per refresh; one that cannot stretches every
//! frame, and a run of stretched frames steps the profile down, no further than
//! Low and only for the session. It never steps back up within one: a lighter
//! profile renders less, so a fast-looking run under it says nothing about the
//! heavier one.

use crate::config::Settings;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
	Custom,
	Max,
	High,
	Low,
	Standard,
	// Standard's values, chosen for a remote screen and never written down:
	// it lives in `Settings::remote_override`, not in `performance_profile`
	Remote,
}

impl Profile {
	// dialog order, which is also the order they cost in
	pub const ALL: [Profile; 6] = [
		Profile::Custom,
		Profile::Max,
		Profile::High,
		Profile::Low,
		Profile::Standard,
		Profile::Remote,
	];

	// the spelling the config file uses
	pub fn key(self) -> &'static str {
		match self {
			Profile::Custom => "custom",
			Profile::Max => "max",
			Profile::High => "high",
			Profile::Low => "low",
			Profile::Standard => "standard",
			Profile::Remote => "remote",
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			Profile::Custom => "Custom",
			Profile::Max => "Max silk",
			Profile::High => "High",
			Profile::Low => "Low",
			Profile::Standard => "Standard terminal",
			Profile::Remote => "Remote (temporary)",
		}
	}

	// An unknown spelling reads as the shipped default, the way every other
	// named option in the config does.
	pub fn parse(text: &str) -> Profile {
		Profile::ALL
			.into_iter()
			.find(|p| p.key().eq_ignore_ascii_case(text.trim()))
			.unwrap_or(Profile::Max)
	}

	pub fn index(self) -> usize {
		Profile::ALL.iter().position(|p| *p == self).unwrap_or(0)
	}

	pub fn from_index(index: usize) -> Profile {
		Profile::ALL.get(index).copied().unwrap_or(Profile::Max)
	}

	// The next cheaper profile, or None at the bottom. Custom has no neighbor:
	// the user's own values are not on the ladder.
	pub fn lower(self) -> Option<Profile> {
		match self {
			Profile::Max => Some(Profile::High),
			Profile::High => Some(Profile::Low),
			Profile::Low => Some(Profile::Standard),
			Profile::Standard | Profile::Remote | Profile::Custom => None,
		}
	}

	// Where the display watch may step to. It stops at Low: Low keeps the
	// wallpaper, which costs nothing per frame, and Standard turns off the eased
	// frames the watch measures, so a step there could never be checked again.
	pub fn watched_lower(self) -> Option<Profile> {
		self.lower().filter(|next| *next != Profile::Standard)
	}
}

// The user's own values of every field a profile governs, kept beside the
// live settings while a profile is in force. `put` is the whole of "choose
// Custom and everything comes back".
#[derive(Clone, PartialEq, Debug)]
pub struct Shadow {
	scroll_smooth: bool,
	scroll_ease_in_ms: f32,
	scroll_ramp_up_ms: f32,
	scroll_single_screen_tau_ms: f32,
	scroll_ramp_down_ms: f32,
	scroll_ease_out_ms: f32,
	smooth_scroll_apps: bool,
	cursor_animation: String,
	text_scrim: bool,
	text_scrim_radius: f32,
	text_scrim_strength: f32,
	text_scrim_softness: f32,
	text_scrim_function: String,
	text_outline: f32,
	wallpaper_enabled: bool,
	wallpaper_blur: f32,
	wallpaper_contrast_mask: bool,
}

impl Shadow {
	fn of(s: &Settings) -> Shadow {
		Shadow {
			scroll_smooth: s.scroll_smooth,
			scroll_ease_in_ms: s.scroll_ease_in_ms,
			scroll_ramp_up_ms: s.scroll_ramp_up_ms,
			scroll_single_screen_tau_ms: s.scroll_single_screen_tau_ms,
			scroll_ramp_down_ms: s.scroll_ramp_down_ms,
			scroll_ease_out_ms: s.scroll_ease_out_ms,
			smooth_scroll_apps: s.smooth_scroll_apps,
			cursor_animation: s.cursor_animation.clone(),
			text_scrim: s.text_scrim,
			text_scrim_radius: s.text_scrim_radius,
			text_scrim_strength: s.text_scrim_strength,
			text_scrim_softness: s.text_scrim_softness,
			text_scrim_function: s.text_scrim_function.clone(),
			text_outline: s.text_outline,
			wallpaper_enabled: s.wallpaper_enabled,
			wallpaper_blur: s.wallpaper_blur,
			wallpaper_contrast_mask: s.wallpaper_contrast_mask,
		}
	}

	fn put(&self, s: &mut Settings) {
		s.scroll_smooth = self.scroll_smooth;
		s.scroll_ease_in_ms = self.scroll_ease_in_ms;
		s.scroll_ramp_up_ms = self.scroll_ramp_up_ms;
		s.scroll_single_screen_tau_ms = self.scroll_single_screen_tau_ms;
		s.scroll_ramp_down_ms = self.scroll_ramp_down_ms;
		s.scroll_ease_out_ms = self.scroll_ease_out_ms;
		s.smooth_scroll_apps = self.smooth_scroll_apps;
		s.cursor_animation.clone_from(&self.cursor_animation);
		s.text_scrim = self.text_scrim;
		s.text_scrim_radius = self.text_scrim_radius;
		s.text_scrim_strength = self.text_scrim_strength;
		s.text_scrim_softness = self.text_scrim_softness;
		s.text_scrim_function.clone_from(&self.text_scrim_function);
		s.text_outline = self.text_outline;
		s.wallpaper_enabled = self.wallpaper_enabled;
		s.wallpaper_blur = self.wallpaper_blur;
		s.wallpaper_contrast_mask = self.wallpaper_contrast_mask;
	}
}

// Put the user's own values back. Safe on settings that carry no profile.
pub fn unapply(s: &mut Settings) {
	if let Some(shadow) = s.profile_shadow.take() {
		shadow.put(s);
	}
}

// Overwrite the governed fields with the profile's, keeping the user's values
// in the shadow. Idempotent: a live copy that already carries a profile is
// unwound first, so a changed profile field is honored rather than stacked.
pub fn apply(s: &mut Settings) {
	unapply(s);
	let profile = current(s);
	if profile == Profile::Custom {
		return;
	}
	let shadow = Shadow::of(s);
	values(profile, s);
	s.profile_shadow = Some(Box::new(shadow));
}

// Keep what a profile is showing and make it the user's own, then drop to
// Custom. This is what editing a governed setting means: the change starts from
// the values on screen, not from whatever the file held before the profile went
// on. Also switches the automatic choice off, since a machine still picking for
// itself would overwrite the edit at the next launch.
pub fn adopt(s: &mut Settings) {
	let profile = current(s);
	if profile == Profile::Custom && !s.performance_automatic {
		return;
	}
	if profile != Profile::Custom {
		unapply(s);
		values(profile, s);
	}
	s.remote_override = false;
	s.stepped_profile = None;
	s.performance_profile = Profile::Custom.key().to_string();
	s.performance_automatic = false;
}

// The profile in force: the remote override while it is on, then a step the
// display watch took this session, then the stored one. A step only ever makes
// a ladder rung cheaper, and only while automatic is on - it is the automatic
// choice's own correction, so a hand pick or Custom is never overridden by it.
pub fn current(s: &Settings) -> Profile {
	if s.remote_override {
		return Profile::Remote;
	}
	let stored = Profile::parse(&s.performance_profile);
	match s.stepped_profile {
		Some(step)
			if s.performance_automatic
				&& matches!(stored, Profile::Max | Profile::High | Profile::Low)
				&& matches!(step, Profile::High | Profile::Low | Profile::Standard)
				&& step.index() > stored.index() =>
		{
			step
		}
		_ => stored,
	}
}

// What each profile sets. Every profile starts from the shipped defaults, so
// Max is exactly "the defaults for everything" and the others name only what
// they change.
fn values(profile: Profile, s: &mut Settings) {
	let defaults = Settings::default();
	Shadow::of(&defaults).put(s);
	match profile {
		Profile::Custom | Profile::Max => {}
		Profile::High => quicker(s),
		// the wallpaper is decoded once and costs nothing per frame, so Low keeps
		// it and drops the halo, which is paid on every frame
		Profile::Low => {
			quicker(s);
			s.cursor_animation = "none".to_string();
			s.text_scrim = false;
			s.text_outline = 1.0;
		}
		Profile::Standard | Profile::Remote => {
			s.scroll_smooth = false;
			s.smooth_scroll_apps = false;
			s.cursor_animation = "none".to_string();
			s.text_scrim = false;
			s.text_outline = 0.0;
			s.wallpaper_enabled = false;
			s.wallpaper_blur = 0.0;
			s.wallpaper_contrast_mask = false;
		}
	}
}

// Shorter eases on the three stretches a slow display shows most, and a halo
// that costs fewer taps: the square metric with a smaller reach.
fn quicker(s: &mut Settings) {
	s.scroll_ease_in_ms /= 2.0;
	s.scroll_ease_out_ms /= 2.0;
	s.scroll_single_screen_tau_ms /= 2.0;
	s.text_scrim_function = "dilate".to_string();
	// the same share of the shipped radius it has always been, so a cheaper
	// profile still looks like the same halo
	s.text_scrim_radius = 5.0;
}

// Names the adapter closely enough that a new card or a switch to software
// rendering reads as new hardware, and a driver update does not.
pub fn fingerprint(info: &wgpu::AdapterInfo) -> String {
	format!(
		"{} ({:?}, {:?})",
		info.name.trim(),
		info.device_type,
		info.backend
	)
}

// The names a driver gives itself when there is no card behind it. wgpu reports
// most of these as DeviceType::Cpu already; the ones that do not are the remote
// and virtual display drivers, which is exactly where the pick was going wrong.
const SOFTWARE_ADAPTERS: &[&str] = &[
	"llvmpipe",
	"softpipe",
	"swrast",
	"lavapipe",
	"basic render",
	"remote display",
	"microsoft remote",
];

// Is there a real graphics processor behind this adapter?
pub fn software_adapter(info: &wgpu::AdapterInfo) -> bool {
	if info.device_type == wgpu::DeviceType::Cpu {
		return true;
	}
	let name = info.name.to_ascii_lowercase();
	SOFTWARE_ADAPTERS.iter().any(|s| name.contains(s))
}

// Is the screen this window draws to somewhere else? Every frame is then encoded
// and shipped over a network, so what the graphics card can do says nothing about
// what the person sees, and timing it would only ever flatter the machine. A
// remote session takes the Remote profile for as long as it lasts and writes
// nothing down, so the console keeps the rating it had.
pub fn remote_session() -> bool {
	#[cfg(windows)]
	{
		// SM_REMOTESESSION. The environment check is the backstop for a session
		// the metric misses, a service-hosted one in particular.
		const SM_REMOTESESSION: i32 = 0x1000;
		let metric = unsafe {
			windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_REMOTESESSION)
		};
		metric != 0
			|| std::env::var("SESSIONNAME")
				.is_ok_and(|name| name.to_ascii_uppercase().starts_with("RDP-"))
	}
	#[cfg(not(windows))]
	{
		// A VNC or xrdp server says so in the environment it starts the session
		// with. A forwarded X display names a host before the colon, where a local
		// one is bare or says unix.
		["VNCDESKTOP", "XRDP_SESSION", "RFB_PORT"]
			.iter()
			.any(|key| std::env::var_os(key).is_some())
			|| std::env::var("DISPLAY").is_ok_and(|d| forwarded_display(&d))
	}
}

// Does this DISPLAY reach its screen over a network? ":0" and "unix:0" are
// local; "somebox:0" is not, and neither is "localhost:10.0", which is what
// ssh -X sets. X servers stopped listening on TCP by default years ago, so a
// display on localhost is a tunnel, and one that is not still never draws on
// this machine's card directly.
#[cfg(not(windows))]
fn forwarded_display(display: &str) -> bool {
	let host = display.split(':').next().unwrap_or("");
	!matches!(host, "" | "unix")
}

// The parts of the id that need no graphics adapter, read once on a worker so
// the first frame never waits on a file read. Started from main.
static MACHINE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn probe_machine() {
	std::thread::spawn(|| {
		let _ = MACHINE.set(machine_parts());
	});
}

fn machine_parts() -> String {
	format!("{}|{}", cpu_name(), memory_gib())
}

// What a written rating means. Hashed into the id, so a change here re-rates
// every machine once. 2: a step-down is no longer written, and a profile
// written by one before could not be told apart from a measured answer.
const RATING_VERSION: u32 = 2;

// Everything the pick depends on, as one short id: the processor, the graphics
// adapter and how much memory there is. Change any of them and the machine has
// to be rated again. Hashed rather than spelled out, so the config carries no
// description of the box it is on.
pub fn hardware_id(info: &wgpu::AdapterInfo) -> String {
	// the worker starts with the process and answers in microseconds; reading it
	// here rather than waiting is the cheaper way to handle "not yet"
	let machine = MACHINE.get().cloned().unwrap_or_else(machine_parts);
	let parts = format!("r{RATING_VERSION}|{machine}|{}", fingerprint(info));
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for byte in parts.as_bytes() {
		hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
	}
	format!("{hash:016x}")
}

// The processor's own name where the OS offers one, plus how many cores are
// usable. The count alone would miss a swap between two chips of one size, and
// the name alone misses a core count the OS was told to restrict.
fn cpu_name() -> String {
	let threads = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
	#[cfg(windows)]
	let model = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default();
	#[cfg(not(windows))]
	let model = std::fs::read_to_string("/proc/cpuinfo")
		.ok()
		.and_then(|text| {
			text.lines()
				.find(|line| line.starts_with("model name"))
				.and_then(|line| line.split_once(':'))
				.map(|(_, value)| value.trim().to_string())
		})
		.unwrap_or_default();
	format!("{model}/{threads}")
}

// Installed memory in whole GiB. Rounded, so the few MiB a driver or a firmware
// update reserves does not read as new hardware.
fn memory_gib() -> u64 {
	#[cfg(windows)]
	{
		use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
		let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
		status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).unwrap_or(0);
		if unsafe { GlobalMemoryStatusEx(&raw mut status) } != 0 {
			return status.ullTotalPhys / (1 << 30);
		}
		0
	}
	#[cfg(not(windows))]
	{
		let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
		let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
		if pages > 0 && page > 0 {
			(pages as u64).saturating_mul(page as u64) / (1 << 30)
		} else {
			0
		}
	}
}

// Where a machine starts before anything is measured, and where it stays when
// there is nothing worth measuring: an adapter with no card behind it is not
// going to hold any rung above Low.
pub fn first_pick(info: &wgpu::AdapterInfo) -> Profile {
	if software_adapter(info) {
		Profile::Low
	} else {
		Profile::Max
	}
}

// Is there anything to time here, or is the first pick already the answer?
// SILK_BENCH=1 forces a run: the banner and the ladder walk are otherwise only
// reachable by putting a different graphics card in the machine.
pub fn worth_measuring(info: &wgpu::AdapterInfo) -> bool {
	std::env::var_os("SILK_BENCH").is_some() || !software_adapter(info)
}

// A short measured run over the ladder, in place of guessing from the adapter's
// name. Each rung gets a moment of full-rate frames with its own settings live,
// and the first whose median frame period fits the display's budget is the
// answer. Standard is not timed on the way down: if Low cannot hold the rate,
// nothing below it is in question. It is timed once where a rung stalls, to tell
// a slow machine from a display that is not drawing (see `Step::Stalled`).
const BENCH_WARMUP: usize = 3; // frames discarded while a rung's settings settle
const BENCH_FRAMES: usize = 40; // frames measured per rung...
const BENCH_RUNG_MS: f32 = 800.0; // ...or this long, whichever comes first
const BENCH_MIN_FRAMES: usize = 5; // never judge a rung on fewer than this
// How far past the budget a frame has to run before the profile cannot be what
// is pacing it. The profiles change the per-pixel work by around half, so a
// period several times over says something else is holding the display, and no
// step down would rescue it. One constant for both users on purpose: the bench
// stops timing the ladder there (the case that would take longest to measure,
// for a foregone answer), and the watch refuses to count such a frame at all -
// a monitor asleep under the NVIDIA driver paces a GL client at 1 fps.
//
// The bench used to answer Standard there and save it, which left a machine
// rated with its monitor asleep on Standard from then on. Not saving on a stall
// would test a truly slow machine at every launch. So a stall is settled by
// timing Standard, which has no effects to pay for: a slow machine draws that
// well enough and gets Standard, and a display that still stalls is what is
// pacing the frames, so nothing is learned and nothing is saved.
pub const STALL_FACTOR: f32 = 4.0;

// What the caller does with the frame it just measured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
	Measuring,
	Rung(Profile), // this rung missed: put the next one live and keep going
	Done(Profile), // the answer
	Stalled,       // the display is pacing the frames, not the profile: no answer
}

pub struct Bench {
	rungs: &'static [Profile],
	at: usize,
	seen: usize,
	periods: Vec<f32>,
	last: Option<Instant>,
	rung_start: Option<Instant>,
	floor: bool, // a rung stalled, and Standard is being timed to see why
}

impl Bench {
	const LADDER: &'static [Profile] = &[Profile::Max, Profile::High, Profile::Low];

	pub fn new() -> Bench {
		Bench {
			rungs: Bench::LADDER,
			at: 0,
			seen: 0,
			periods: Vec::with_capacity(BENCH_FRAMES),
			last: None,
			rung_start: None,
			floor: false,
		}
	}

	// The profile whose settings must be live while this rung is measured.
	pub fn profile(&self) -> Profile {
		if self.floor {
			return Profile::Standard;
		}
		self.rungs
			.get(self.at)
			.copied()
			.unwrap_or(Profile::Standard)
	}

	// A frame went out; answer what to do next.
	pub fn note(&mut self, now: Instant, budget_ms: f32) -> Step {
		self.seen += 1;
		if self.seen <= BENCH_WARMUP {
			self.last = Some(now);
			self.rung_start = Some(now);
			return Step::Measuring;
		}
		if let Some(last) = self.last {
			self.periods.push((now - last).as_secs_f32() * 1000.0);
		}
		self.last = Some(now);
		let elapsed = self
			.rung_start
			.map_or(0.0, |start| (now - start).as_secs_f32() * 1000.0);
		let enough = self.periods.len() >= BENCH_FRAMES
			|| (elapsed >= BENCH_RUNG_MS && self.periods.len() >= BENCH_MIN_FRAMES);
		if !enough {
			return Step::Measuring;
		}
		let period = median(&mut self.periods);
		let stalled = period > budget_ms * STALL_FACTOR;
		if self.floor {
			return if stalled {
				Step::Stalled
			} else {
				Step::Done(Profile::Standard)
			};
		}
		if period <= budget_ms {
			return Step::Done(self.profile());
		}
		self.periods.clear();
		self.seen = 0;
		self.last = None;
		self.rung_start = None;
		if stalled {
			self.floor = true;
			return Step::Rung(Profile::Standard);
		}
		self.at += 1;
		match self.rungs.get(self.at) {
			Some(next) => Step::Rung(*next),
			None => Step::Done(Profile::Standard),
		}
	}
}

// Frames an ease has to pace before its median says anything.
pub const WINDOW: usize = 48;

// How far past the refresh period a frame may run before it counts as a miss:
// half again, so the occasional stretched frame of a busy desktop passes and
// a display dropping every third frame does not.
pub fn budget_ms(refresh_hz: f32) -> f32 {
	1000.0 / refresh_hz.max(1.0) * 1.5
}

// The budget of the monitor the window is on now. A window can be dragged to a
// monitor with another rate, and a budget kept from launch then reads every
// frame on a slower one as a miss. Asked again four times a second rather than
// every frame, since on Windows the answer comes from enumerating display
// modes. That is too few frames at the old budget to fill half a window.
pub struct FrameBudget {
	ms: f32,
	read_at: Instant,
}

impl FrameBudget {
	pub fn new(now: Instant, refresh_hz: f32) -> FrameBudget {
		FrameBudget {
			ms: budget_ms(refresh_hz),
			read_at: now,
		}
	}

	pub fn at(&mut self, now: Instant, refresh_hz: impl FnOnce() -> f32) -> f32 {
		if now.saturating_duration_since(self.read_at).as_secs_f32() >= 0.25 {
			self.ms = budget_ms(refresh_hz());
			self.read_at = now;
		}
		self.ms
	}
}

// A verdict has to come from one sitting. Frames eased this long after the last
// counted one start a new window rather than finishing a half-full one left
// from hours ago.
const STALE_S: f32 = 30.0;

pub struct Rating {
	periods: Vec<f32>,
	last: Option<Instant>,
	// the last frame counted, kept across a pause so the gap to the next one
	// can be judged stale
	noted: Option<Instant>,
	// the budget the window is being filled under
	budget: f32,
}

impl Rating {
	pub fn new() -> Rating {
		Rating {
			periods: Vec::with_capacity(WINDOW),
			last: None,
			noted: None,
			budget: 0.0,
		}
	}

	// A frame just went out while an ease was running. Only the gap to the
	// previous such frame is a period; the first after a pause is a start. A gap
	// past the stall ceiling is not a slow frame, and the frames either side of
	// it were paced under the same condition, so the window goes with it.
	pub fn note(&mut self, now: Instant, budget_ms: f32) {
		if self
			.noted
			.is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() > STALE_S)
		{
			self.periods.clear();
		}
		// Frames paced by another monitor say nothing against this one's budget.
		// A 60 Hz window moved to 144 Hz would read as missing every frame.
		if budget_ms != self.budget {
			self.periods.clear();
			self.last = None;
			self.budget = budget_ms;
		}
		if let Some(last) = self.last {
			let period = now.saturating_duration_since(last).as_secs_f32() * 1000.0;
			if period > budget_ms * STALL_FACTOR {
				self.periods.clear();
			} else {
				self.periods.push(period);
			}
		}
		self.last = Some(now);
		self.noted = Some(now);
	}

	// The ease stopped, so the next frame's gap means nothing.
	pub fn pause(&mut self) {
		self.last = None;
	}

	// Start over, with nothing measured - the profile changed, so what was
	// measured was measured under another workload.
	pub fn reset(&mut self) {
		self.periods.clear();
		self.last = None;
		self.noted = None;
	}

	// Once a window is full: did the display miss its budget? Empties the
	// window either way, so a verdict is one window's worth of evidence.
	pub fn verdict(&mut self, budget_ms: f32) -> Option<bool> {
		if self.periods.len() < WINDOW {
			return None;
		}
		let over = median(&mut self.periods) > budget_ms;
		self.periods.clear();
		Some(over)
	}
}

fn median(values: &mut [f32]) -> f32 {
	values.sort_by(f32::total_cmp);
	values[values.len() / 2]
}

#[cfg(test)]
mod tests {
	use super::{
		Bench, FrameBudget, Profile, Rating, Step, WINDOW, apply, budget_ms, first_pick,
		software_adapter, unapply,
	};
	use crate::config::Settings;
	use std::time::{Duration, Instant};

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

	// Test ID: Ep17Tqy
	#[test]
	fn a_missing_card_is_picked_for_rather_than_timed() {
		let card = adapter("NVIDIA GeForce RTX 3060 Ti", wgpu::DeviceType::DiscreteGpu);
		let none = adapter("llvmpipe (LLVM 19.1.7, 256 bits)", wgpu::DeviceType::Cpu);
		assert_eq!(first_pick(&card), Profile::Max);
		assert_eq!(first_pick(&none), Profile::Low);
		// wgpu labels most software renderers Cpu, but the remote and virtual
		// display drivers arrive as something else and have to be named
		assert!(software_adapter(&adapter(
			"Anything At All",
			wgpu::DeviceType::Cpu
		)));
		assert!(software_adapter(&adapter(
			"llvmpipe (LLVM 19.1.7, 256 bits)",
			wgpu::DeviceType::Other
		)));
		assert!(software_adapter(&adapter(
			"Microsoft Basic Render Driver",
			wgpu::DeviceType::VirtualGpu
		)));
		assert!(!software_adapter(&adapter(
			"NVIDIA GeForce RTX 3060 Ti",
			wgpu::DeviceType::DiscreteGpu
		)));
		assert!(!software_adapter(&adapter(
			"Intel(R) UHD Graphics",
			wgpu::DeviceType::IntegratedGpu
		)));
	}

	// Test ID: EowvGeu
	#[cfg(not(windows))]
	#[test]
	fn a_display_naming_another_host_is_a_remote_screen() {
		use super::forwarded_display;
		for local in [":0", ":98.0", "unix:0"] {
			assert!(!forwarded_display(local), "{local} is this machine");
		}
		// ssh -X points DISPLAY at a port on localhost
		for away in [
			"b29w:0",
			"192.168.1.9:0.0",
			"localhost:10.0",
			"127.0.0.1:10.0",
		] {
			assert!(forwarded_display(away), "{away} is somewhere else");
		}
	}

	// Frames at `period` ms until the run answers, so a whole run can be walked
	// without a clock.
	fn run_bench(period: f32, budget_ms: f32) -> (Profile, Vec<Profile>) {
		let (pick, rungs) = run_bench_by(|_| period, budget_ms);
		(pick.expect("the run stalled"), rungs)
	}

	// The same with the period each profile draws at. None is a stalled run.
	fn run_bench_by(
		period: impl Fn(Profile) -> f32,
		budget_ms: f32,
	) -> (Option<Profile>, Vec<Profile>) {
		let mut bench = Bench::new();
		let mut now = Instant::now();
		let mut rungs = vec![bench.profile()];
		for _ in 0..4000 {
			now += Duration::from_micros((period(bench.profile()) * 1000.0) as u64);
			match bench.note(now, budget_ms) {
				Step::Measuring => {}
				Step::Rung(next) => rungs.push(next),
				Step::Done(pick) => return (Some(pick), rungs),
				Step::Stalled => return (None, rungs),
			}
		}
		panic!("the run never answered");
	}

	// Test ID: EowvGev
	#[test]
	fn the_run_stops_at_the_first_rung_that_holds_the_rate() {
		let budget = budget_ms(60.0);
		// comfortably inside the budget: the heaviest rung stands, and nothing
		// below it is ever put live
		let (pick, rungs) = run_bench(16.0, budget);
		assert_eq!(pick, Profile::Max);
		assert_eq!(rungs, vec![Profile::Max]);
		// a little over: the ladder is walked and Standard is what is left
		let (pick, rungs) = run_bench(budget * 1.2, budget);
		assert_eq!(pick, Profile::Standard);
		assert_eq!(
			rungs,
			vec![Profile::Max, Profile::High, Profile::Low],
			"each rung has to go live before it is judged"
		);
		// far over: nothing on the ladder can help, so the rest is not timed -
		// which is the case that would otherwise take longest. This used to stop
		// at Max and answer Standard whatever Standard drew at:
		//   let (pick, rungs) = run_bench(budget * 6.0, budget);
		//   assert_eq!(rungs, vec![Profile::Max]);
		// Standard is timed now, to see whether the machine or the display is slow.
		let slow = |profile| {
			if profile == Profile::Standard {
				budget * 0.9
			} else {
				budget * 6.0
			}
		};
		let (pick, rungs) = run_bench_by(slow, budget);
		assert_eq!(pick, Some(Profile::Standard));
		assert_eq!(rungs, vec![Profile::Max, Profile::Standard]);
	}

	// A monitor asleep paces every frame at about one a second, whatever is being
	// drawn. The run used to read that as a hopeless machine and save Standard,
	// which has no wallpaper, for every launch after. A stall that Standard does
	// not cure says nothing about the machine, so the run gives no answer.
	// Test ID: EqHVWh6
	#[test]
	fn a_display_that_is_not_drawing_gives_no_rating() {
		let budget = budget_ms(60.0);
		let (pick, rungs) = run_bench_by(|_| 1000.0, budget);
		assert_eq!(pick, None, "nothing to save");
		assert_eq!(rungs, vec![Profile::Max, Profile::Standard]);
		// a machine that is slow with no effects on either, but not stalled, is
		// still a slow machine
		let crawl = |profile| {
			if profile == Profile::Standard {
				budget * 3.0
			} else {
				budget * 6.0
			}
		};
		assert_eq!(run_bench_by(crawl, budget).0, Some(Profile::Standard));
		// a stall that starts partway down the ladder is settled the same way
		let late = |profile| {
			if profile == Profile::Max {
				budget * 1.2
			} else {
				1000.0
			}
		};
		let (pick, rungs) = run_bench_by(late, budget);
		assert_eq!(pick, None);
		assert_eq!(rungs, vec![Profile::Max, Profile::High, Profile::Standard]);
	}

	fn tuned() -> Settings {
		Settings {
			scroll_ease_in_ms: 300.0,
			scroll_smooth: false,
			cursor_animation: "phase".to_string(),
			text_scrim_radius: 9.0,
			wallpaper_enabled: false,
			..Settings::default()
		}
	}

	// Test ID: EorkTk1
	#[test]
	fn a_profile_masks_the_stored_values_and_custom_puts_them_back() {
		let mut s = tuned();
		s.performance_profile = "max".to_string();
		apply(&mut s);
		assert!(s.scroll_smooth, "Max is the shipped default");
		assert_eq!(s.scroll_ease_in_ms, Settings::default().scroll_ease_in_ms);
		assert_eq!(s.cursor_animation, "pulse_vertical");
		assert!(s.wallpaper_enabled);

		s.performance_profile = "custom".to_string();
		apply(&mut s);
		assert!(!s.scroll_smooth);
		assert_eq!(s.scroll_ease_in_ms, 300.0);
		assert_eq!(s.cursor_animation, "phase");
		assert_eq!(s.text_scrim_radius, 9.0);
		assert!(!s.wallpaper_enabled);
		assert!(s.profile_shadow.is_none());
	}

	// Test ID: EorkTk2
	#[test]
	fn applying_twice_does_not_stack() {
		let mut s = tuned();
		s.performance_profile = "low".to_string();
		apply(&mut s);
		s.performance_profile = "high".to_string();
		apply(&mut s);
		assert!(s.wallpaper_enabled, "High keeps the wallpaper");
		unapply(&mut s);
		assert_eq!(s.scroll_ease_in_ms, 300.0, "the user's value, not Low's");
		assert!(!s.wallpaper_enabled, "the user's value, not High's");
	}

	// Test ID: EorkTk3
	#[test]
	fn each_profile_costs_less_than_the_one_above() {
		let mut s = Settings::default();
		let mut radius = f32::MAX;
		for profile in [Profile::Max, Profile::High] {
			s.performance_profile = profile.key().to_string();
			apply(&mut s);
			assert!(s.scroll_smooth);
			assert!(s.text_scrim);
			assert!(s.text_scrim_radius <= radius);
			radius = s.text_scrim_radius;
		}
		s.performance_profile = "low".to_string();
		apply(&mut s);
		assert!(s.scroll_smooth);
		assert_eq!(s.cursor_animation, "none");
		assert!(s.wallpaper_enabled, "Low keeps the wallpaper");
		assert!(!s.text_scrim, "Low drops the halo");
		// was: assert_eq!(s.text_outline, 2.0, ...) - no built-in profile draws an
		// outline over a pixel wide any more, so Low leans on the shipped one
		assert_eq!(s.text_outline, 1.0, "and leans on the outline");
		for name in ["max", "high", "low", "standard", "remote"] {
			s.performance_profile = name.to_string();
			apply(&mut s);
			assert!(s.text_outline <= 1.0, "{name} draws a fat outline");
		}
		s.performance_profile = "low".to_string();
		apply(&mut s);
		for flat in ["standard", "remote"] {
			s.performance_profile = flat.to_string();
			apply(&mut s);
			assert!(!s.scroll_smooth);
			assert!(!s.smooth_scroll_apps);
			assert!(!s.text_scrim);
			assert_eq!(s.text_outline, 0.0);
		}
	}

	// The override is a profile that is never in the file: it sits over whatever
	// the stored one says and lifts off without touching it.
	// Test ID: Ep17Tqz
	#[test]
	fn the_remote_override_sits_over_the_stored_profile() {
		let mut s = tuned();
		s.performance_profile = "max".to_string();
		s.remote_override = true;
		apply(&mut s);
		assert_eq!(super::current(&s), Profile::Remote);
		assert!(!s.scroll_smooth);
		assert_eq!(
			s.performance_profile, "max",
			"the stored profile is untouched"
		);
		s.remote_override = false;
		apply(&mut s);
		assert_eq!(super::current(&s), Profile::Max);
		assert!(s.scroll_smooth);
	}

	// The display watch's step is session state over the stored rung: it never
	// makes a profile heavier, and a hand-set or Custom profile is left alone.
	// Test ID: EpWow4h
	#[test]
	fn a_session_step_sits_over_the_stored_profile() {
		let at = |stored: &str, step: Profile| {
			let mut s = tuned();
			s.performance_profile = stored.to_string();
			s.stepped_profile = Some(step);
			s
		};
		let mut s = at("max", Profile::Low);
		assert_eq!(super::current(&s), Profile::Low);
		apply(&mut s);
		assert!(s.wallpaper_enabled, "Low keeps the wallpaper");
		assert_eq!(
			s.performance_profile, "max",
			"the stored profile is untouched"
		);
		assert_eq!(
			super::current(&at("low", Profile::High)),
			Profile::Low,
			"never heavier"
		);
		assert_eq!(super::current(&at("custom", Profile::Low)), Profile::Custom);
		assert_eq!(
			super::current(&at("standard", Profile::Low)),
			Profile::Standard
		);
		let mut remote = at("max", Profile::Low);
		remote.remote_override = true;
		assert_eq!(super::current(&remote), Profile::Remote);
		let mut by_hand = at("max", Profile::Low);
		by_hand.performance_automatic = false;
		assert_eq!(
			super::current(&by_hand),
			Profile::Max,
			"a hand pick is not the watch's to change"
		);
	}

	// A rating written before the version was part of the id reads as another
	// machine's, once, so a profile an older build's step-down wrote is measured
	// again rather than kept.
	// Test ID: EpWpSam
	#[test]
	fn a_rating_from_before_the_version_is_stale() {
		let card = adapter("NVIDIA GeForce RTX 3060 Ti", wgpu::DeviceType::DiscreteGpu);
		let machine = super::MACHINE
			.get()
			.cloned()
			.unwrap_or_else(super::machine_parts);
		let unversioned = format!("{machine}|{}", super::fingerprint(&card));
		let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
		for byte in unversioned.as_bytes() {
			hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
		}
		let id = super::hardware_id(&card);
		assert_ne!(id, format!("{hash:016x}"), "an old id no longer matches");
		assert_eq!(
			super::hardware_id(&card),
			id,
			"and the new one is stable, so the rating happens once"
		);
	}

	// Test ID: EpWow4i
	#[test]
	fn watched_lower_stops_at_low() {
		assert_eq!(Profile::Max.watched_lower(), Some(Profile::High));
		assert_eq!(Profile::High.watched_lower(), Some(Profile::Low));
		for p in [
			Profile::Low,
			Profile::Standard,
			Profile::Custom,
			Profile::Remote,
		] {
			assert_eq!(p.watched_lower(), None, "{p:?}");
		}
	}

	// Test ID: EorkTk4
	#[test]
	fn the_ladder_ends_at_standard_and_custom_is_off_it() {
		assert_eq!(Profile::Max.lower(), Some(Profile::High));
		assert_eq!(Profile::Standard.lower(), None);
		assert_eq!(Profile::Remote.lower(), None);
		assert_eq!(Profile::Custom.lower(), None);
		assert_eq!(Profile::parse("LOW"), Profile::Low);
		assert_eq!(
			Profile::parse("silky"),
			Profile::Max,
			"unknown reads as the default"
		);
		for p in Profile::ALL {
			assert_eq!(Profile::from_index(p.index()), p);
		}
	}

	// Test ID: EorkTk5
	#[test]
	fn a_window_of_stretched_frames_is_a_miss_and_a_pause_breaks_the_chain() {
		let budget = budget_ms(60.0);
		let mut r = Rating::new();
		let mut t = Instant::now();
		for _ in 0..=WINDOW {
			r.note(t, budget);
			t += Duration::from_millis(16);
		}
		assert_eq!(r.verdict(budget), Some(false));
		assert_eq!(r.verdict(budget), None, "the window was spent");
		// a long gap while paused is not a period
		r.pause();
		t += Duration::from_secs(5);
		for _ in 0..=WINDOW {
			r.note(t, budget);
			t += Duration::from_millis(30);
		}
		assert_eq!(r.verdict(budget), Some(true));
	}

	// `count` more frames after `t`, each `step` ms after the one before.
	fn periods(r: &mut Rating, t: &mut Instant, count: usize, step: u64, budget: f32) {
		for _ in 0..count {
			*t += Duration::from_millis(step);
			r.note(*t, budget);
		}
	}

	// A monitor asleep under the NVIDIA driver paces a GL client at 1 fps. That
	// is forty times a 60 Hz budget, and it stepped a desktop down to Standard
	// overnight.
	// Test ID: EpWnbLd
	#[test]
	fn a_capped_display_is_not_a_slow_one() {
		let budget = budget_ms(60.0);
		let mut r = Rating::new();
		let mut t = Instant::now();
		r.note(t, budget);
		for _ in 0..200 {
			t += Duration::from_secs(1);
			r.note(t, budget);
			assert_ne!(r.verdict(budget), Some(true), "a stall is not a miss");
		}
	}

	// The budget was read once at launch. A window opened on 144 Hz and dragged
	// to 60 Hz timed every frame against 10.4 ms and stepped the profile down.
	// Test ID: EqFz3m4
	#[test]
	fn the_budget_follows_the_monitor_the_window_is_on() {
		let mut t = Instant::now();
		let mut budget = FrameBudget::new(t, 144.0);
		let mut r = Rating::new();
		r.note(t, budget.at(t, || unreachable!("asked again at once")));
		// dragged to a 60 Hz monitor, which paces every frame at its own period,
		// asked for a verdict at every frame the way the window does
		for _ in 0..=WINDOW * 3 {
			t += Duration::from_micros(16_667);
			let now = budget.at(t, || 60.0);
			r.note(t, now);
			assert_ne!(
				r.verdict(now),
				Some(true),
				"a display keeping up read as missing"
			);
		}
		assert_eq!(budget.at(t, || 60.0), budget_ms(60.0));
	}

	// Moved the other way, periods paced at 60 Hz would read as misses against a
	// 144 Hz budget, so a new budget starts the window over.
	// Test ID: EqFz3m5
	#[test]
	fn frames_paced_by_another_monitor_are_not_counted_against_this_one() {
		let slow = budget_ms(60.0);
		let fast = budget_ms(144.0);
		let mut r = Rating::new();
		let mut t = Instant::now();
		r.note(t, slow);
		periods(&mut r, &mut t, WINDOW - 1, 16, slow);
		periods(&mut r, &mut t, 2, 7, fast);
		assert_eq!(r.verdict(fast), None, "the 60 Hz frames were kept");
		periods(&mut r, &mut t, WINDOW, 7, fast);
		assert_eq!(r.verdict(fast), Some(false));
	}

	// Test ID: EpWnbLe
	#[test]
	fn a_stall_throws_away_the_window_around_it() {
		let budget = budget_ms(60.0);
		let mut r = Rating::new();
		let mut t = Instant::now();
		r.note(t, budget);
		periods(&mut r, &mut t, WINDOW - 1, 30, budget);
		periods(&mut r, &mut t, 1, 1000, budget);
		periods(&mut r, &mut t, 1, 30, budget);
		assert_eq!(r.verdict(budget), None, "the frames before it went too");
		periods(&mut r, &mut t, WINDOW, 30, budget);
		assert_eq!(r.verdict(budget), Some(true), "a slow display still is");
	}

	// Test ID: EpWnbLf
	#[test]
	fn a_window_does_not_span_a_long_pause() {
		let budget = budget_ms(60.0);
		for (gap, full) in [(31, false), (10, true)] {
			let mut r = Rating::new();
			let mut t = Instant::now();
			r.note(t, budget);
			periods(&mut r, &mut t, WINDOW - 1, 30, budget);
			r.pause();
			periods(&mut r, &mut t, 1, gap * 1000, budget);
			periods(&mut r, &mut t, 1, 30, budget);
			let want = full.then_some(true);
			assert_eq!(r.verdict(budget), want, "{gap} s between eases");
		}
	}
}
