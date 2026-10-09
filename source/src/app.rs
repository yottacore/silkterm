// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::index::Side;
use alacritty_terminal::selection::SelectionType;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use alacritty_terminal::term::{ClipboardType, TermMode};
use glyphon::{Buffer, Color as GColor, Shaping, TextArea, TextBounds};

use crate::bgimage::{ImageRenderer, WpProbe};
use crate::clipboard::Clipboard;
use crate::config;
use crate::gfx::{
	Drawn, FRAME_RETRY_FIRST, FRAME_RETRY_MAX, Gfx, NoFrame, QuadMode, Rebirth, RectInstance,
	RectRenderer, Retry, VramProbe,
};
use crate::input::{self, ClickSelect, CopyFrom, Hotkey, WheelRoute, is_copy_chord};
use crate::pane::{BarHit, Dir, Pane, Rect};
use crate::settings_ui::EditCmd;
use crate::term::{PaneId, UserEvent};
use crate::text::TextCtx;
use cmdline::{build_layout, new_window_command, settings_after_reload};
use dialogs::notice_due;
pub(crate) use idle::tune_heap;
use idle::{Conserve, IdleClock, MinimizedProbe, idledbg, restore_sign};
use menubar::{bar_menu_for, bar_title_underlines, menu_bar_at_launch, menubar_text_slot};
use menus::{ContextMenu, entry_accel, entry_label};
pub(crate) use menus::{Entry, MenuAction};
#[cfg(any(test, target_os = "macos"))]
pub(crate) use menus::{mac_entries, menu_hotkey, without_rows};
#[cfg(test)]
pub(crate) use menus::{
	sample_window_menus, sample_window_menus_copying, sample_window_menus_with,
};
use rating::{RatingStep, rate_hardware, rating_step, remote_override_at_launch, session_step};
use tab_edit::TabEdit;
use tabs::{
	TabLabels, TabLayout, TabTip, Tabs, tab_button_v, tab_close_box, tab_edit_box, tab_title_w,
};
use vt::{VtHeal, spawn_vt_watch, vramdbg};
pub(crate) use window_size::request_size;
use window_size::{MonitorWatch, fit_px, launch_maximized, window_px};

mod cmdline;
mod dialogs;
mod idle;
mod menubar;
mod menus;
mod rating;
mod rotation;
mod tab_edit;
mod tabs;
mod vt;
mod window_size;

// Delayed re-assertions of "terminal stays under the dialog" after the dialog is
// focused, to win the race against the WM's own activation restacking (Compiz).
// The window must outlast the WM's raise/focus animation (Compiz fade/zoom can
// keep re-stacking for a few hundred ms), so it spans ~1.2s - a too-short window
// let the animation re-bury the terminal after the last retry (About showed this;
// Settings happened to settle in time). Each retry is one cheap X message.
const RAISE_REASSERTS: u8 = 24;
const RAISE_REASSERT_IVL: Duration = Duration::from_millis(50);

// Reopening Settings this soon after closing it resumes the tab and scroll it
// was left on - long enough to cover "closed it, went to look at the result,
// came back", short enough that a later visit still starts from the top.
const SETTINGS_RESUME: Duration = Duration::from_mins(1);

pub struct App {
	proxy: EventLoopProxy<UserEvent>,
	state: Option<State>,
	cli: crate::cli::Cli,
	// pop-out dialog window (About/Settings), if open. Its own surface + text
	// context, so it can be larger than the main window.
	dialog: Option<crate::dialog::DialogWin>,
	dialog_dirty: bool,
	// A save that could not be written, waiting to be said, and the notice
	// saying it or the one below. It is its own window so it can stand over an
	// open Settings.
	// Windows shows the system's message box instead, and `notice` stays None.
	notice: Option<crate::dialog::DialogWin>,
	notice_dirty: bool,
	notice_owed: Option<config::Refusal>,
	// settings a conversion of the file could not keep, said the same way
	loss_owed: Option<config::ConversionLoss>,
	// files already reported this session (see notice_due)
	told: Vec<std::path::PathBuf>,
	// where the Settings dialog was when it last closed, and when that was
	settings_view: Option<(Instant, crate::settings_ui::View)>,
	// and the size it was dragged to, which outlives the view above and lasts
	// the whole session. Deliberately never written to the config.
	settings_size: Option<(f32, f32)>,
	// after the dialog is focused, re-assert "keep the terminal under me" a few
	// times: the WM's own activation (raising the dialog) can come just after our
	// first restack and re-bury the terminal, so a couple of delayed retries
	// settle it (see handle_dialog_event / about_to_wait).
	raise_reassert: u8,
	raise_next: Instant,
	// VT watcher spawned (once per process; GL path only)
	vt_watch: bool,
	// GPU context the pop-out dialogs draw on, warmed on a worker thread once the
	// terminal is on screen (see gfx::DialogGpu for why they can't share the
	// terminal's) and then kept, so no dialog open pays for it.
	gpu_warm: crate::gfx::GpuWarm,
	// SILK_DLGDBG: when the open Settings was asked for, until its first frame
	settings_asked: Option<(Instant, bool)>,
	// SILK_MEMDBG: what was last printed, and when to look again
	memdbg: crate::memdbg::Printer,
	memdbg_next: Instant,
	// cicd profiler stage: when SILK_PROFILE_OUT is set the app runs a workload
	// (via --shell) for SILK_PROFILE_SECS then exits, so main can dump a flamegraph.
	#[cfg(feature = "profiling")]
	profile_secs: u64,
	#[cfg(feature = "profiling")]
	profile_deadline: Option<std::time::Instant>,
}

impl std::fmt::Debug for App {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("App").finish_non_exhaustive()
	}
}

impl App {
	pub fn new(proxy: EventLoopProxy<UserEvent>, cli: crate::cli::Cli) -> Self {
		Self {
			proxy,
			state: None,
			cli,
			dialog: None,
			dialog_dirty: false,
			notice: None,
			notice_dirty: false,
			notice_owed: None,
			loss_owed: None,
			told: Vec::new(),
			settings_view: None,
			settings_size: None,
			raise_reassert: 0,
			raise_next: Instant::now(),
			vt_watch: false,
			gpu_warm: crate::gfx::GpuWarm::idle(),
			settings_asked: None,
			memdbg: crate::memdbg::Printer::default(),
			memdbg_next: Instant::now(),
			#[cfg(feature = "profiling")]
			profile_secs: std::env::var("SILK_PROFILE_SECS")
				.ok()
				.and_then(|raw| raw.parse().ok())
				.unwrap_or(8),
			#[cfg(feature = "profiling")]
			profile_deadline: None,
		}
	}

	// SILK_MEMDBG: one line per device and per pane, printed when it changed.
	// The dialog's device is the warm one whenever that was ready to lend.
	fn memdbg_report(&mut self) {
		if let Some(state) = self.state.as_ref() {
			let size = state.window.inner_size();
			let panes: usize = state.tabs.list.iter().map(|pm| pm.panes.len()).sum();
			self.memdbg.say(
				"window",
				format!(
					"window: {}x{} px, {} tabs, {panes} panes",
					size.width,
					size.height,
					state.tabs.len()
				),
			);
			let main = state.gpu.as_ref().map_or_else(
				|| "main: device let go".to_string(),
				|gpu| crate::memdbg::gpu_line("main", &gpu.gfx.device),
			);
			self.memdbg.say("main", main);
			let wallpaper = state
				.gpu
				.as_ref()
				.and_then(|gpu| gpu.wallpaper_img.as_ref())
				.map_or_else(|| "wallpaper: none".to_string(), ImageRenderer::memdbg_line);
			self.memdbg.say("wallpaper", wallpaper);
			let kept = state.wp_standin.as_ref().map_or_else(
				|| "stand-in: none".to_string(),
				|small| {
					let (w, h) = small.rgba.dimensions();
					format!("stand-in: {w}x{h}, {} bytes", small.rgba.len())
				},
			);
			self.memdbg.say("standin", kept);
			self.memdbg.say("glyphs", state.text.memdbg_line());
			for pm in &state.tabs.list {
				for (id, pane) in &pm.panes {
					self.memdbg.say(&format!("pane {id}"), pane.memdbg_line());
				}
			}
		}
		let dialog = match (self.dialog.as_ref(), self.gpu_warm.ready_device()) {
			(Some(d), _) => crate::memdbg::gpu_line("dialog", d.device()),
			(None, Some(device)) => crate::memdbg::gpu_line("dialog", device),
			(None, None) => "dialog: no device".to_string(),
		};
		self.memdbg.say("dialog", dialog);
	}
}

// The background shell scan came back (shells.rs). It reports what it FOUND;
// the fold into the stored list happens here, on the winit thread, against the
// list as it stands right now - so a scan cannot carry a snapshot that went
// stale while it ran. Nothing on screen changes (menus are built when they
// open), so this only has to put the list in the live settings and in the file.
// A scan that found nothing new compares equal and writes nothing at all; if the
// config looks open in another program the write is skipped and the list still
// applies for this session.
//
// A Settings dialog open at that moment is folded into as well, on BOTH of its
// copies - see `Dialog::fold_shells`.
fn fold_shells(found: &[crate::shells::Found]) {
	save_live(|live| {
		let shells = crate::shells::merge(&live.shells, found);
		if shells == live.shells {
			return false;
		}
		live.shells = shells;
		true
	});
}

// The live settings with `edit` made, for this session only.
fn set_live(edit: impl FnOnce(&mut config::Settings)) {
	let mut new = (*config::settings()).clone();
	edit(&mut new);
	config::update(new);
}

// The same, and what changed written to the file. `edit` answers false to leave
// both alone. A config open in another program skips the write, and the
// session keeps the value anyway.
fn save_live(edit: impl FnOnce(&mut config::Settings) -> bool) {
	let orig = (*config::settings()).clone();
	let mut new = orig.clone();
	if edit(&mut new) {
		let _ = config::persist(&orig, &new);
		config::update(new);
	}
}

// argv for the stored shell at `index`. None when the list moved under an open
// menu, in which case the new tab falls back to the default shell rather than
// running something the user did not pick.
fn shell_argv(index: usize) -> Option<Vec<String>> {
	let command = config::settings().shells.get(index)?.command.clone();
	config::command_argv(&command)
}

// What Close pane closes, in the cascade the menu and a dead shell both follow.
// `present` is whether the pane the menu was opened for is still there: a menu
// left standing after its pane went must not reach another tab's panes, which is
// how one could take the whole window down with live shells in it.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum CloseScope {
	Pane,
	Tab,
	Window,
	Nothing,
}

fn close_scope(present: bool, panes_in_tab: usize, tabs: usize) -> CloseScope {
	if !present {
		CloseScope::Nothing
	} else if panes_in_tab > 1 {
		CloseScope::Pane
	} else if tabs > 1 {
		CloseScope::Tab
	} else {
		CloseScope::Window
	}
}

// Shaped chrome text, kept frame to frame: menu-bar titles + the copybox label,
// the tab close-"x", and per-tab title buffers. Re-shaping these every rendered
// frame was constant background work during any animation (even the idle cursor
// pulse). Rebuilt when the menu color changes; a tab entry re-shapes only when
// its title or the tab width changes; the whole cache is dropped on a
// text-context rebuild (buffers are tied to the FontSystem they were made with).
struct ChromeCache {
	menu_fg: [u8; 3],
	menubar: Vec<Buffer>, // MENU_BAR titles + trailing "Copy output" label
	// per shown tab: the title, the width it was shaped for, and the buffer
	tabs: Vec<(String, f32, Buffer)>,
}

// The lists a frame fills, kept between frames so each frame clears them
// rather than growing them from empty. `render_with` takes them out for the
// frame and puts them back emptied; an early return just drops them.
#[derive(Default)]
struct FrameBufs {
	instances: Vec<RectInstance>,
	pane_fulls: Vec<Rect>,
	cursors: Vec<(Rect, RectInstance)>,
	scrim_cursors: Vec<RectInstance>,
	group_ranges: Vec<(Rect, u32, u32)>,
	link_ranges: Vec<(Rect, u32, u32)>,
	cursor_ranges: Vec<(Rect, u32, u32)>,
}

impl FrameBufs {
	fn emptied(mut self) -> Self {
		self.instances.clear();
		self.pane_fulls.clear();
		self.cursors.clear();
		self.scrim_cursors.clear();
		self.group_ranges.clear();
		self.link_ranges.clear();
		self.cursor_ranges.clear();
		self
	}
}

// Menu/tab bars auto-size to the menu (proportional) font: height = the text line
// height (cell_h) + this vertical padding, so a larger font isn't clipped (#124).
// Chrome measurements are DIP and convert at their use site - see config::dip.
const MENU_BAR_VPAD: f32 = 6.0;
const TAB_BAR_VPAD: f32 = 6.0; // text is metric-centered in the bar; descenders clear via that
const BELL_TAU_S: f32 = 0.18; // visual-bell flash fade time-constant (~0.8s to settle)
// Text scrim "Strength" is a percent; this much of it is one doubling of the
// finished halo's alpha. The scrim design doc and the dialog's comment both quote the
// number, and a test holds them to it.
const SCRIM_PCT_PER_DOUBLING: f32 = 20.0;
// Freeze knob (one line rolls it back): a minimized window builds no frames -
// PTY reading never stops - and catches up in one hard-cut frame on restore.
// Covers WMs that never report Occluded for an iconified window.
const FREEZE_MINIMIZED: bool = true;
// Warm knob (one line rolls it back): build the dialogs' GPU context on a worker
// thread once the terminal is up, instead of on the click that opens one. Off
// means every dialog open pays for its own instance + adapter + device again.
const WARM_DIALOG_GPU: bool = true;
const SIZE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500); // remember-size settle time before hitting disk
// A window moved to another monitor takes that monitor's size once it has sat
// still this long, and not while a mouse button is still down.
const MONITOR_SETTLE: Duration = Duration::from_millis(750);
const MONITOR_RECHECK: Duration = Duration::from_millis(250);
// A resize this soon after one the window asked for, or one the system made
// for a new scale, is not the user's.
const OWN_RESIZE_GRACE: Duration = Duration::from_millis(1500);
// How long after the window is genuinely on screen the background shell scan
// starts (shells.rs). Long enough to be clear of the first prompt and whatever
// the shell reads at startup, short enough that the Tabs menu has its list well
// before anyone opens it.
const SHELL_SCAN_DELAY: Duration = Duration::from_secs(3);
// The scan waits for the wallpaper to be on screen (see `wp_shown`), and a
// wallpaper on a share that never answers would otherwise hold it off for the
// life of the window - leaving the Tabs menu with no shells in it. This is the
// backstop, measured from the reveal.
const SHELL_SCAN_MAX_WAIT: Duration = Duration::from_secs(20);
// The performance benchmark waits for the wallpaper too: it is one of the
// things being timed, and starting before it is on screen would rate a lighter
// window than the one that ends up in front of the person. The backstop is for
// an image on a share that never answers.
const BENCH_DELAY: Duration = Duration::from_millis(600);
const BENCH_MAX_WAIT: Duration = Duration::from_secs(10);
// The banner goes up as soon as a run is due, so the wait for the wallpaper
// happens behind it rather than in front of a window that then stops taking
// input. And it stays up this long at least: a run that answers on the first
// rung is over in under a second, which is not enough time to notice a box has
// appeared, let alone read it.
const BENCH_BANNER_MIN: Duration = Duration::from_secs(4);
const MEMDBG_IVL: Duration = Duration::from_secs(2); // SILK_MEMDBG: how often to look for a change
const VRAM_CHECK_IVL: Duration = Duration::from_secs(2); // GL sentinel probe tick (VT-switch texture loss)
// How long a resize has to be still before the wallpaper is prepared again at
// the new size. Until then the one held is drawn scaled.
const WP_RESIZE_WAIT: Duration = Duration::from_millis(500);
const CAPTURE_SETTLE: Duration = Duration::from_millis(120); // copy-output: idle-at-prompt debounce marking a command done
// Chrome geometry, all DIP (see config::dip).
const MENU_BAR_PAD: f32 = 10.0; // around each top-level title
const TAB_TIP_REFRESH: Duration = Duration::from_millis(500); // how often an open tip re-reads what it says
const TAB_TIP_PAD: f32 = 8.0; // inside the tip box, DIP
const MENU_TIP_PAD: f32 = 6.0; // inside a menu row's tip box, DIP
const MENU_TIP_GAP: f32 = 6.0; // between the popup's edge and the tip beside it, DIP
const MENU_TIP_MAX_W: f32 = 320.0; // widest a menu tip's text gets before it wraps, DIP
const BENCH_BANNER_PAD: f32 = 22.0; // inside the benchmark banner's box, DIP
const BENCH_SCRIM_ALPHA: f32 = 0.55; // how far the window dims under the banner
const TAB_TIP_GAP: f32 = 4.0; // between the tab bar and the tip below it, DIP
const TAB_CLOSE_W: f32 = 26.0; // right-edge close-button region per tab (title clips before it)
const TAB_CLOSE_M: f32 = 6.0; // balanced top/right/bottom margin around the close button box
const TAB_TITLE_PAD: f32 = 8.0; // tab title's left inset
const TAB_EDIT_INSET: f32 = 4.0; // rename box's inset inside the tab button
const TAB_EDIT_PAD: f32 = 4.0; // text inset inside the rename box (INSET + PAD == TAB_TITLE_PAD,
// so the label does not move when a rename starts)
const TAB_GAP: f32 = 1.0; // gap between adjacent tab buttons (each side of the seam)
const TAB_TOP_PAD: f32 = 2.0; // tab button's inset from the top of the bar
const CHROME_HAIRLINE: f32 = 1.0; // 1px rules: accelerator underlines, menu/checkbox borders
const COPYBOX_BOX_GAP: f32 = 6.0; // checkbox to its own word
const COPYBOX_PAIR_GAP: f32 = 14.0; // one checkbox pair to the next
const COPYBOX_LEAD_GAP: f32 = 10.0; // "Copy on:" lead-in to the first checkbox
const COPYBOX_TICK_INSET: f32 = 3.0; // checked fill's inset inside its box
const MENUBAR_TEXT_W: f32 = 240.0; // shaping width for a menu-bar title buffer
const MENU_ACCEL_DROP: f32 = 3.0; // accelerator underline's rise off the item's line box
// Input knob (one line rolls it back): a key that arrives while the window is
// unfocused is never typed. A WM hotkey grab (Ctrl+Alt+Arrow for desktop
// switching) brackets the chord with a focus-out/in pair, and winit zeroes the
// modifiers on the way out - so a grab that still passes the key through hands
// us an arrow with nothing held, which would encode as a bare arrow.
const IGNORE_KEYS_WHILE_UNFOCUSED: bool = true;

// Winit replays every key already held down whenever focus changes, flagged
// `is_synthetic`, so an app can track what is physically pressed. That is
// state, not typing - and on X11 the replay arrives BEFORE winit re-queries the
// modifiers, so a held Ctrl+Alt+Arrow comes back through it as a bare arrow.
fn key_is_typed(state: ElementState, is_synthetic: bool) -> bool {
	state == ElementState::Pressed && !is_synthetic
}

/// Debug switches consulted per frame or per event. Each reads the environment
/// once, since `var_os` takes the env lock and scans environ every call. Same
/// pattern as pane.rs `scroll_dbg`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EnvFlag {
	Dump,
	DlgDbg,
	KeyDbg,
	IdleDbg,
	MemDbg,
}
impl EnvFlag {
	#[cfg(test)]
	const ALL: [EnvFlag; 5] = [
		EnvFlag::Dump,
		EnvFlag::DlgDbg,
		EnvFlag::KeyDbg,
		EnvFlag::IdleDbg,
		EnvFlag::MemDbg,
	];
	fn var(self) -> &'static str {
		match self {
			EnvFlag::Dump => "SILK_DUMP",
			EnvFlag::DlgDbg => "SILK_DLGDBG",
			EnvFlag::KeyDbg => "SILK_KEYDBG",
			EnvFlag::IdleDbg => "SILK_IDLEDBG",
			EnvFlag::MemDbg => "SILK_MEMDBG",
		}
	}
}
pub(crate) fn env_flag(flag: EnvFlag) -> bool {
	use std::sync::OnceLock;
	static DUMP: OnceLock<bool> = OnceLock::new();
	static DLGDBG: OnceLock<bool> = OnceLock::new();
	static KEYDBG: OnceLock<bool> = OnceLock::new();
	static IDLEDBG: OnceLock<bool> = OnceLock::new();
	static MEMDBG: OnceLock<bool> = OnceLock::new();
	let cell = match flag {
		EnvFlag::Dump => &DUMP,
		EnvFlag::DlgDbg => &DLGDBG,
		EnvFlag::KeyDbg => &KEYDBG,
		EnvFlag::IdleDbg => &IDLEDBG,
		EnvFlag::MemDbg => &MEMDBG,
	};
	*cell.get_or_init(|| std::env::var_os(flag.var()).is_some())
}

// SILK_MAX_FPS pins the animation frame rate instead of letting vblank set it.
// Unset (every ordinary run) this is None and nothing below it changes: the GL
// path keeps swap interval 1 and a scroll ease keeps rendering on Poll.
//
// It exists for the demo recorder, which samples the X screen at a fixed rate.
// Whatever paces the app has to divide that rate evenly or frames fall off the
// sampling grid on a strict period - a source of 60 into a capture of 50 drops
// one frame in six, so every fifth stored frame carries two frames of travel,
// and a regular hitch like that is exactly what reads as the picture jumping.
// Pinning the source to the capture rate takes the host's refresh rate out of
// the answer entirely. Also useful for measuring a fixed frame budget.
fn max_fps() -> Option<f64> {
	use std::sync::OnceLock;
	static FPS: OnceLock<Option<f64>> = OnceLock::new();
	*FPS.get_or_init(|| {
		std::env::var("SILK_MAX_FPS")
			.ok()
			.and_then(|raw| raw.trim().parse::<f64>().ok())
			.filter(|fps| *fps > 0.0 && fps.is_finite())
	})
}

// The display's refresh rate, which is what a scroll ease is paced against.
// Unknown reads as 60: the common case, and a wrong guess only moves the
// budget a little.
fn refresh_hz(window: &Window) -> f32 {
	window
		.current_monitor()
		.and_then(|m| m.refresh_rate_millihertz())
		.map_or(60.0, |mhz| mhz as f32 / 1000.0)
}

// A wait that starts over once the wallpaper is on screen (the shell scan, the
// benchmark). It moves out to `due` but never past its backstop, and a wait that
// already ran stays gone: a late wallpaper must not start a second one.
fn push_back(at: Option<Instant>, cap: Option<Instant>, due: Instant) -> Option<Instant> {
	at.map(|_| cap.map_or(due, |cap| due.min(cap)))
}

// Whether a window still hidden is shown now. Normally once a frame is on screen
// at the size asked for, and at the deadline whatever the size. On macOS a hidden
// window is never given a frame at all, so there it is shown before one is drawn.
fn reveal_due(drawn_at_size: bool, past_deadline: bool, occluded: bool) -> bool {
	occluded || past_deadline || drawn_at_size
}

// Next frame on a FIXED schedule, not `now + interval` - the latter adds each
// frame's own render time to the period and runs slow. Falling behind resyncs
// rather than trying to catch up in a burst.
fn pace_frame(next: &mut Option<Instant>, ivl: Duration) -> ControlFlow {
	let now = Instant::now();
	let mut at = next.unwrap_or(now) + ivl;
	if at <= now {
		at = now + ivl;
	}
	*next = Some(at);
	ControlFlow::WaitUntil(at)
}

// `flow`, woken by `wake` as well: a wait takes the earlier of the two, and a
// loop already polling has nothing to add.
fn wake_by(flow: ControlFlow, wake: Option<Instant>) -> ControlFlow {
	match (flow, wake) {
		(ControlFlow::Wait, Some(wake)) => ControlFlow::WaitUntil(wake),
		(ControlFlow::WaitUntil(until), Some(wake)) => ControlFlow::WaitUntil(until.min(wake)),
		(flow, _) => flow,
	}
}

// The times a pane asks the loop to come back at, apart from animating. A
// new one goes here too, or `pane_wake` never sees it.
trait PaneWakes {
	fn cursor_wake_at(&self) -> Option<Instant>;
	fn map_wake_at(&self) -> Option<Instant>;
}

impl PaneWakes for Pane {
	fn cursor_wake_at(&self) -> Option<Instant> {
		self.cursor_wake
	}

	fn map_wake_at(&self) -> Option<Instant> {
		self.map_wake()
	}
}

// The earliest pane wake still ahead of `now`, read after the frame, since
// drawing is what sets them. One already due is left out: the pass before the
// frame acts on those, and a window that is not drawing would spin on one.
fn pane_wake<'a, P: PaneWakes + 'a>(
	panes: impl IntoIterator<Item = &'a P>,
	now: Instant,
) -> Option<Instant> {
	panes
		.into_iter()
		.flat_map(|pane| [pane.cursor_wake_at(), pane.map_wake_at()])
		.flatten()
		.filter(|&wake| wake > now)
		.min()
}

// Two presses on a tab this close together are a double-click.
const TAB_DBL_CLICK: Duration = Duration::from_millis(400);

const MENU_BAR: [&str; 6] = ["File", "Edit", "View", "Tabs", "Panes", "Help"];

const COPYBOX_LABELS: [&str; 3] = ["Copy on:", "select", "output"]; // menu-bar auto-copy checkboxes

// Everything that lives on the GPU device, held together so an idle window can
// let the whole lot go at once and take it back later (see `release_gpu`).
struct Gpu {
	gfx: Gfx,
	rects: RectRenderer,
	minimap: crate::minimap::MapRenderer,
	wallpaper_img: Option<ImageRenderer>,
	scrim: crate::scrim::Scrim, // text readability scrim (used only when config.text_scrim)
}

impl Gpu {
	// The renderers first and the device last: `Gfx::release` needs every
	// handle on the device gone before it runs.
	fn release(self) -> Rebirth {
		let Self {
			gfx,
			rects,
			minimap,
			wallpaper_img,
			scrim,
		} = self;
		drop((wallpaper_img, minimap, scrim, rects));
		gfx.release()
	}
}

struct State {
	window: Arc<Window>,
	// None while the window has let its device go after a long idle, with what
	// is needed to build it again waiting in `rebirth` (see `release_gpu`).
	// `render` takes it out for the length of a frame, so nothing a frame calls
	// may ask for it.
	gpu: Option<Gpu>,
	rebirth: Option<Rebirth>,
	text: TextCtx,
	// posts worker results (wallpaper) back into this event loop
	proxy: EventLoopProxy<UserEvent>,
	tabs: Tabs,
	mods: ModifiersState,
	mouse: (f32, f32),
	mouse_btn: Option<input::MouseBtn>, // button held after a reported press (mouse-tracking apps)
	mouse_cell: Option<(usize, usize)>, // last cell reported, to de-dupe motion
	selecting: Option<PaneId>,          // pane with an in-progress drag-select
	select_edge_held: f32,              // seconds that drag has been past a pane edge
	last_click: Option<(Instant, f32, f32)>, // for multi-click detection
	click_count: u32,                   // consecutive clicks in the same spot (2=double, 3=triple)
	// (active tab, focused pane, window focused) at the last frame - a change
	// while focused pokes the focused pane's cursor, so a long-idle-parked
	// animation resumes on any window/tab/pane refocus
	cursor_focus_sig: Option<(usize, PaneId, bool)>,
	resizing: Option<Vec<bool>>, // split-tree path of the divider being dragged
	dragging_pane: Option<PaneId>, // pane being drag-reordered (Shift+drag)
	bar_dragging: Option<PaneId>, // pane whose scrollbar thumb is being dragged
	map_dragging: Option<PaneId>, // pane whose minimap marker is being dragged
	// A Ctrl+press hit a hyperlink: the release over the same link opens it,
	// a release anywhere else drops it (drag off to cancel, like the tab close
	// button). The URL is captured at press time - output can scroll it away in
	// between - and `menu_link` is the same for the right-click menu's two items.
	link_arm: Option<(PaneId, String)>,
	menu_link: Option<String>,
	cursor_icon: CursorIcon,
	clipboard: Clipboard,
	last_frame: Instant,
	// how a scroll ease is being paced, and the period a frame may not exceed
	// before it counts as a miss (profile.rs)
	rating: crate::profile::Rating,
	frame_budget: crate::profile::FrameBudget,
	// The startup benchmark: a run in flight, and when one is due. While a run
	// is in flight the window renders flat out. bench_banner is when the banner
	// went up, which is earlier than either - it covers the wait as well as the
	// run, and the window takes no input for as long as it is up (bench_layout).
	bench: Option<crate::profile::Bench>,
	bench_at: Option<Instant>,
	bench_cap: Option<Instant>,
	bench_banner: Option<Instant>,
	// Set while a run is owed or in flight; written down with the answer.
	bench_id: Option<String>,
	// Why the answer was not kept, while the banner says so.
	bench_kept: Option<config::Kept>,
	// The profile in force before the run, to go back to if it gives no answer,
	// and whether it gave none (the banner says that instead).
	bench_from: Option<crate::profile::Profile>,
	bench_stalled: bool,
	dirty: bool,
	bell_flash: f32,    // visual-bell brightness, set to 1.0 on BEL, decays to 0
	size_tracked: bool, // false until the first frame, so startup/programmatic resizes don't overwrite remembered_size
	// The window is born hidden and revealed once a real frame is on screen at its
	// final (grid-derived) size, so it never flashes the default size / blank client
	// before painting. reveal_want is the physical size to wait for when the startup
	// resize was async (None = reveal on the first frame); reveal_deadline is a hard
	// fallback so an async or WM-adjusted resize can't strand the window hidden.
	revealed: bool,
	// When the background shell scan is due: set when the window is revealed,
	// cleared when the scan is away. None the rest of the time - it runs once.
	shell_scan_at: Option<Instant>,
	reveal_want: Option<winit::dpi::PhysicalSize<u32>>,
	reveal_deadline: Instant,
	pending_size: Option<(usize, usize)>, // debounced remember-size: persisted after the size holds, not per resize tick
	maximize_on_reveal: bool,
	watch: MonitorWatch,
	pending_size_at: Instant,
	menu: Option<ContextMenu>,
	tab_close_arm: Option<usize>, // tab whose close button is held down (closes on release)
	tab_hover: crate::tip::Dwell<usize>, // tab under the pointer, and since when
	// menu row under the pointer, as (how deep in the open chain, which row)
	menu_tip: crate::tip::Dwell<(usize, usize)>,
	// the row whose tip is up. Kept because ripeness is reached by the clock
	// rather than by an event, so "has it changed" has to be asked against what
	// was last drawn, not against what the timer said a moment ago.
	menu_tip_up: Option<(usize, usize)>,
	// The tab last opened beside the active one, and that tab, each by a pane
	// it held then. Closing the new one goes back (see `active_after_close`).
	tab_opener: Option<(PaneId, PaneId)>,
	tab_first: usize,                  // tab the strip is paged to (clamped on read)
	tab_followed: usize,               // active tab the page last followed (see rebuild_tab_layout)
	tab_layout: TabLayout,             // the strip as measured (see rebuild_tab_layout)
	tab_labels: TabLabels,             // each tab's label forms, kept between frames
	tab_tip: Option<TabTip>,           // the hover tip currently up, if any
	tab_edit: Option<TabEdit>,         // tab title being renamed in place (double-click)
	tab_dbl: Option<(Instant, usize)>, // last tab-strip click, for the double
	decorated: bool,                   // window frame shown (winit has no getter, so track it)
	menu_bar: bool,                    // window menu bar (File/Edit/...) shown
	bare: bool, // frame, menu bar and tab strip all off at once (View > Bare window); never written
	bare_saved: (bool, bool), // (decorated, menu_bar) from before bare, to put back
	bar_open: Option<usize>, // which top-level menu's dropdown is open, if any
	quit: bool, // set by File->Quit; the event handler exits after applying
	win_opacity: Option<f32>, // CLI --background-opacity override (this window only)
	win_title: Option<String>, // CLI --title override (else "AppName - <tab title>")
	last_win_title: String, // last string set on the window (skip redundant set_title)
	focused: bool, // window has keyboard focus (gates copy-output: never copy from a background window)
	pending_about: bool, // request to open the About window (App acts on it; needs the event loop)
	pending_settings: Option<Instant>, // request to open the Settings window, and when
	chrome: Option<ChromeCache>, // shaped menu/tab text, reused across frames
	chrome_rev: u64, // bumped whenever a chrome buffer is (re)shaped
	// Signature of everything feeding the prepared text set, from the last frame
	// that actually prepared. A pure cursor frame matches it, and then both
	// glyphon prepares (the bulk of per-frame CPU) and the atlas trim are skipped
	// - the retained vertex buffers are still correct. None = must prepare.
	text_sig: Option<u64>,
	// same idea for the context-menu overlay (skip re-shaping an open menu)
	overlay_sig: Option<u64>,
	// The scrim's blurred source is valid for this signature. The halo depends on
	// the text alone (the cursor has its own coverage texture), so a cursor-only
	// frame reuses it instead of re-rendering and re-blurring the whole window.
	scrim_sig: Option<u64>,
	// light mode's halo curve, solved again only when its inputs move
	halo_memo: crate::visibility::HaloMemo,
	// the lists render_with fills, kept for the next frame
	frame_bufs: FrameBufs,
	occluded: bool, // window fully hidden: skip rendering entirely until it comes back
	// The last resize was to nothing (minimized on Windows). Read by `hidden`,
	// since a restore stops reporting minimized before the size comes back.
	no_area: bool,
	// The window still shows its last frame once its device is gone (see
	// `release_deadline`). A Windows window with no redirection bitmap does not.
	keeps_picture: bool,
	// last cycle's hidden answer; its false edge is the unfreeze - one dirty
	// catch-up frame, hard-cut. Written only by freeze_sync, which both render
	// entry points go through.
	was_hidden: bool,
	minimized: MinimizedProbe,
	// Deadline of the next animation frame while SILK_MAX_FPS pins the rate; None
	// otherwise, which is every ordinary run. See `max_fps`.
	next_frame: Option<Instant>,
	// Rotation state as of the last scan; the folder itself, the shuffle history
	// and the picking all live in the worker (wallpaper.rs), so nothing here reads
	// the filesystem.
	wp_count: usize, // images the last scan found (<2 = nothing to rotate to)
	wp_current: Option<PathBuf>, // image showing now, so order mode advances from it
	wp_next: Option<Instant>, // when to rotate next (None = no timer / startup-only)
	wp_locked: bool, // a command-line wallpaper owns this session; don't rotate
	// The window options the command line gave at launch, folded in again after a
	// reload so a reread file does not drop them.
	cli_style: crate::cli::Style,
	// Request stamp, shared with the workers: a result with an older one is
	// stale, and a worker whose stamp is no longer the newest stops early.
	wp_seq: Arc<std::sync::atomic::AtomicU64>,
	wp_pacing: crate::wallpaper::Pacing, // holds a tick while a request is working
	// When to prepare the wallpaper again for the window's new size. Pushed back
	// by every resize, so it comes once the resizing stops.
	wp_resize_at: Option<Instant>,
	// A few KiB of the picture showing, kept through an idle release and drawn
	// at the rebuild until the real one is prepared again (`rebuild_gpu`).
	wp_standin: Option<crate::wallpaper::Prepared>,
	// A worker has answered - with an image, or with the news that there is none.
	wp_answered: bool,
	// ...and a frame has been drawn since, so whatever it said is ON SCREEN. This
	// is what the shell scan waits for: the scan is filesystem and registry work
	// and the wallpaper is hundreds of ms of decode and blur off-thread, so letting
	// them overlap puts a stall between the window appearing and the wallpaper
	// arriving in it - the one moment anyone is looking.
	wp_shown: bool,
	// The hard deadline for that wait (see SHELL_SCAN_MAX_WAIT).
	shell_scan_cap: Option<Instant>,
	vram_next: Instant, // next GL VRAM sentinel probe (VT-switch content-loss detection)
	vramloss_test: bool, // SILK_VRAMLOSS one-shot: fake a loss to exercise the rebuild path
	// The surface's size, kept here because layout still needs it while there
	// is no surface to ask.
	surface_px: (u32, u32),
	gl: bool, // born on the glutin GL path (X11): the one with a VT watcher and sentinels
	adapter_info: wgpu::AdapterInfo, // for the About dialog, which may open before a rebuild
	drawn: Drawn,
	// the session's step to Low is this window's, for a software device
	software_step: bool,
	idle: IdleClock,
	// a frame the surface refused, drawn again on this (see `Retry`)
	frame_retry: Retry,
	conserve: Conserve,
	vt_heal: VtHeal,
}

// (window frame, menu bar) saved and shown after a bare-window toggle, `bare`
// being the new state. Coming back puts back only what is still off, so one
// switched on in the meantime stays on.
fn bare_chrome(bare: bool, saved: (bool, bool), now: (bool, bool)) -> ((bool, bool), (bool, bool)) {
	if bare {
		(now, (false, false))
	} else {
		(saved, (now.0 | saved.0, now.1 | saved.1))
	}
}

impl State {
	// Pixels reserved at the very top by the menu bar (0 when hidden).
	// Bar heights track the menu font's line height so they scale with font size.
	fn menu_bar_h(&self) -> f32 {
		self.text.ui_line_h + self.text.dip(MENU_BAR_VPAD)
	}

	fn tab_bar_h(&self) -> f32 {
		self.text.ui_line_h + self.text.dip(TAB_BAR_VPAD)
	}
	fn menubar_h(&self) -> f32 {
		if self.menu_bar {
			self.menu_bar_h()
		} else {
			0.0
		}
	}

	// The tab bar shows for >1 tab always; for a single tab unless the user
	// opts out (hide_single_tab, View menu / config).
	fn tab_bar_visible(&self) -> bool {
		!self.bare && (self.tabs.len() > 1 || !config::settings().hide_single_tab)
	}

	// Everything above the panes: the menu bar when shown, then the tab strip when
	// visible. A row count asks for the cells below this, so the launch sizing and
	// save_window_size both have to account for it.
	fn chrome_h(&self) -> f32 {
		self.menubar_h()
			+ if self.tab_bar_visible() {
				self.tab_bar_h()
			} else {
				0.0
			}
	}

	fn area(&self) -> Rect {
		// Panes sit below the menu bar (always when shown) and the tab bar
		// (when visible), stacked in that order.
		let bar = self.chrome_h();
		Rect {
			x: 0.0,
			y: bar,
			w: self.surface_px.0 as f32,
			h: (self.surface_px.1 as f32 - bar).max(1.0),
		}
	}

	fn focus_at(&mut self, x: f32, y: f32) {
		if let Some(id) = self.tabs.cur().pane_at(x, y) {
			self.tabs.cur_mut().focused = id;
			self.update_title();
		}
	}

	// Point every pane at the pointer (only the one under it gets a position), so
	// the next build can look for a hyperlink there. Marking dirty on a pending
	// probe is what makes the underline appear at all - the frame that scans is
	// also the frame that draws the result - and it costs at most one frame per
	// cell crossed, never one per pixel.
	fn update_link_hover(&mut self, at: Option<(f32, f32)>) {
		if !config::settings().hyperlinks {
			return;
		}
		// An app watching the pointer owns it: no underline flickering through a
		// TUI that uses the mouse itself. This has to key on the app's MODE, not on
		// whether this particular event was reported - the report is throttled to
		// cell changes, so a pointer that settles inside one cell would slip
		// through and underline anyway. Shift is the local-action bypass, the same
		// one that already lets a selection through a tracking app.
		let shift = self.mods.shift_key();
		let over = at
			.and_then(|(x, y)| self.tabs.cur().pane_at(x, y))
			.filter(|id| {
				shift
					|| !self.tabs.cur().panes.get(id).is_some_and(|p| {
						p.mode
							.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG)
					})
			});
		let text = &self.text;
		let mut probing = false;
		for (id, p) in &mut self.tabs.cur_mut().panes {
			p.set_hover(at.filter(|_| over == Some(*id)), text);
			probing |= p.link_probing();
		}
		self.dirty |= probing;
	}

	// Whether the pointer is over an underlined link - the pane's own hover state,
	// so this is only the pointer shape's question. Anything that ACTS on a link
	// re-scans through link_at_pointer instead of trusting a frame-old answer.
	fn hovering_link(&self) -> bool {
		let (x, y) = self.mouse;
		self.tabs
			.cur()
			.pane_at(x, y)
			.and_then(|id| self.tabs.cur().panes.get(&id))
			.is_some_and(|p| p.link_hover.is_some())
	}

	// Fresh scan for the link under the pointer, with the pane it belongs to.
	fn link_at_pointer(&self) -> Option<(PaneId, crate::pane::LinkHit)> {
		let (x, y) = self.mouse;
		let id = self.tabs.cur().pane_at(x, y)?;
		let hit = self
			.tabs
			.cur()
			.panes
			.get(&id)?
			.link_at_px(x, y, &self.text)?;
		Some((id, hit))
	}

	// One owner for the pointer shape: a drag beats a divider, a divider beats a
	// link. Called from the pointer move AND from the frame, since a link found
	// under a pointer that has stopped moving still has to change the cursor.
	fn sync_cursor_icon(&mut self) {
		let (x, y) = self.mouse;
		let icon = if self.dragging_pane.is_some() {
			CursorIcon::Grabbing
		} else {
			match self
				.tabs
				.cur()
				.divider_at(x, y, self.area(), self.text.scale)
			{
				Some((_, Dir::Vertical)) => CursorIcon::ColResize,
				Some((_, Dir::Horizontal)) => CursorIcon::RowResize,
				None if self.hovering_link() => CursorIcon::Pointer,
				None => CursorIcon::Default,
			}
		};
		if icon != self.cursor_icon {
			self.window.set_cursor(icon);
			self.cursor_icon = icon;
		}
	}

	// Refresh every pane's scrollbar-hover flag from the pointer. Only the pane the
	// pointer is actually over can be hovered, so this also clears the one it just
	// left. Marks dirty only on a change - the fade is what needs the frames, and
	// this runs on every mouse move.
	fn update_bar_hover(&mut self, x: f32, y: f32) {
		let cfg = config::settings();
		if !cfg.scrollbar {
			return;
		}
		let over = self.tabs.cur().pane_at(x, y);
		let mut changed = false;
		let text = &self.text;
		for (id, p) in &mut self.tabs.cur_mut().panes {
			let near = over == Some(*id) && p.bar_near(x, y, text, &cfg);
			if p.bar_hover != near {
				p.bar_hover = near;
				changed = true;
			}
		}
		if changed {
			self.dirty = true;
		}
	}

	// Mouse reporting: forward a button press/release to the pane under the cursor
	// when the app has mouse tracking on (see `input::press_is_reported` for what
	// stays local). Returns true when the event was reported (and should not be
	// handled locally). Records the held button for drag + release.
	fn report_mouse_button(&mut self, button: MouseButton, state: ElementState) -> bool {
		let Some(btn) = input::mouse_btn_of(button) else {
			return false;
		};
		let (x, y) = self.mouse;
		if state == ElementState::Pressed {
			if !input::press_is_reported(btn, self.menu.is_some(), self.mods.shift_key()) {
				return false;
			}
			let cur = self.tabs.cur();
			let Some(id) = cur.pane_at(x, y) else {
				return false;
			};
			let Some(p) = cur.panes.get(&id) else {
				return false;
			};
			if !input::wants_mouse(p.mode) {
				return false;
			}
			let Some((col, row)) = p.screen_cell_at(x, y, &self.text) else {
				return false;
			};
			if let Some(seq) = input::mouse_report(p.mode, btn, true, false, col, row, self.mods) {
				p.write_input(seq);
			}
			self.mouse_btn = Some(btn);
			self.mouse_cell = Some((col, row));
			true
		} else {
			// only our business if we owned the matching press
			if self.mouse_btn.take().is_none() {
				return false;
			}
			let cur = self.tabs.cur();
			if let Some(p) = cur.pane_at(x, y).and_then(|id| cur.panes.get(&id)) {
				if input::wants_mouse(p.mode) {
					if let Some((col, row)) = p.screen_cell_at(x, y, &self.text) {
						if let Some(seq) =
							input::mouse_report(p.mode, btn, false, false, col, row, self.mods)
						{
							p.write_input(seq);
						}
					}
				}
			}
			self.mouse_cell = None;
			true
		}
	}

	// Mouse reporting: forward cursor motion when the app requests it - MOUSE_MOTION
	// (any move) or MOUSE_DRAG (only while a button is held). De-duped per cell so a
	// pixel jiggle inside one cell doesn't flood the PTY. Returns true when reported.
	fn report_mouse_motion(&mut self) -> bool {
		if self.mods.shift_key() {
			return false;
		}
		let (x, y) = self.mouse;
		let held = self.mouse_btn;
		let last = self.mouse_cell;
		let new_cell = {
			let cur = self.tabs.cur();
			let Some(id) = cur.pane_at(x, y) else {
				return false;
			};
			let Some(p) = cur.panes.get(&id) else {
				return false;
			};
			let motion = p.mode.contains(TermMode::MOUSE_MOTION);
			let drag = p.mode.contains(TermMode::MOUSE_DRAG) && held.is_some();
			if !(motion || drag) {
				return false;
			}
			let Some((col, row)) = p.screen_cell_at(x, y, &self.text) else {
				return false;
			};
			if last == Some((col, row)) {
				return false;
			}
			let btn = held.unwrap_or(input::MouseBtn::None);
			if let Some(seq) = input::mouse_report(p.mode, btn, true, true, col, row, self.mods) {
				p.write_input(seq);
			}
			(col, row)
		};
		self.mouse_cell = Some(new_cell);
		true
	}

	// Copy-output: when the focused pane's foreground command finishes, copy its
	// output text to the desktop clipboard. A pending capture only survives while
	// its pane stays the active copy target (window focused, tab active, pane
	// focused, trigger on) - anything else disarms it, so output that finished
	// while the user was elsewhere never copies late on refocus; only a command
	// launched after returning does. Runs every event-loop pass, and every way
	// eligibility can break is itself an event, so the disarm always comes before
	// a refocus could re-poll.
	fn poll_output_copy(&mut self) {
		let cur_focused = self.tabs.cur().focused;
		for pm in &mut self.tabs.list {
			for (id, pane) in &mut pm.panes {
				// ids are unique across tabs, so this is the active tab's pane too
				let in_use = *id == cur_focused;
				if !(input::copy_allowed(CopyFrom::Output, self.focused, in_use)
					&& pane.copy_output)
				{
					pane.disarm_capture();
				}
			}
		}
		let keep = self.focused.then_some(cur_focused);
		let Some(focused_id) = keep else {
			return;
		};
		let text = {
			let Some(pane) = self.tabs.cur_mut().panes.get_mut(&focused_id) else {
				return;
			};
			pane.poll_capture(CAPTURE_SETTLE)
		};
		if let Some(text) = text {
			self.clipboard.set_clipboard(text);
		}
	}

	// When the focused pane is armed for copy-output, the instant its settle timer
	// should fire, so an idle loop wakes to run the capture check.
	fn capture_wake(&self) -> Option<Instant> {
		if !self.focused {
			return None;
		}
		let focused_id = self.tabs.cur().focused;
		let p = self.tabs.cur().panes.get(&focused_id)?;
		p.copy_output
			.then(|| p.capture_deadline(CAPTURE_SETTLE))
			.flatten()
	}

	// Every tip's clock in one place: raise one whose pointer has rested, and
	// keep an open tab tip's contents current. Returns true when the frame has
	// to be redrawn.
	fn update_tips(&mut self) -> bool {
		self.menu_tip.point_at(self.menu_tip_target());
		let up = self.menu_tip.ripe();
		let menu_changed = up != self.menu_tip_up;
		self.menu_tip_up = up;
		self.update_tab_tip() || menu_changed
	}

	// A drag-selection held past the top or bottom of its pane crawls the view
	// that way and keeps extending, so a selection can run past what is on screen.
	// Returns true while it is scrolling, which keeps frames coming - the pointer
	// is stationary, so nothing else would ask for one.
	fn autoscroll_selection(&mut self, dt: f32) -> bool {
		let Some(id) = self.selecting else {
			self.select_edge_held = 0.0;
			return false;
		};
		let (x, y) = self.mouse;
		let cell_h = self.text.cell_h;
		let held = self.select_edge_held;
		let Some(pane) = self.tabs.cur_mut().panes.get_mut(&id) else {
			return false;
		};
		let rate =
			crate::pane::edge_scroll_rate(y, pane.rect.y, pane.rect.y + pane.rect.h, cell_h, held);
		if rate == 0.0 {
			self.select_edge_held = 0.0;
			return false;
		}
		self.select_edge_held = held + dt;
		let was = pane.scroll.target_lines();
		pane.scroll.wheel(rate * dt);
		let moved = pane.scroll.target_lines() != was;
		if moved {
			pane.poke_scrollbar();
		}
		// The pointer sits still while the content moves under it, so the far end
		// of the selection has to be re-read against the rows now on screen.
		pane.drag_selection_to(x, y, &self.text);
		// Pinned at either end there is nothing left to reveal, so don't ask for
		// another frame - held past the bottom, that would be a spin.
		moved
	}

	// When the loop next has to wake for any tip.
	fn tip_wake(&self) -> Option<Instant> {
		match (self.tab_tip_wake(), self.menu_tip.wake()) {
			(Some(a), Some(b)) => Some(a.min(b)),
			(only, None) | (None, only) => only,
		}
	}

	// The active tab's title, in full - the window title has the whole title bar
	// and the OS elides it itself.
	fn active_tab_title(&mut self) -> String {
		let active = self.tabs.active;
		self.refresh_tab_labels(Some(active));
		self.tab_labels
			.forms(active)
			.first()
			.cloned()
			.unwrap_or_else(|| config::APP_NAME.to_string())
	}

	// What the window title says after the app name - see tabtitle::window_suffix
	// for the order the three sources come in.
	fn title_suffix(&mut self) -> Option<String> {
		let typed = self.tabs.cur().title_override.clone();
		let program = self.pane_title();
		let pm = self.tabs.cur();
		let launched = pm
			.panes
			.get(&pm.focused)
			.and_then(|pane| pane.launched())
			.map(str::to_string);
		crate::tabtitle::window_suffix(
			config::rights(),
			typed.as_deref(),
			program.as_deref(),
			launched.as_deref(),
			config::settings().title_shows_tab,
			|| self.active_tab_title(),
		)
	}

	// The title the focused pane's program asked for, if it asked for one.
	fn pane_title(&self) -> Option<String> {
		let pm = self.tabs.cur();
		let title = &pm.panes.get(&pm.focused)?.title;
		(!title.trim().is_empty()).then(|| title.clone())
	}

	// The window title (taskbar / alt-tab): a CLI --title override verbatim, else
	// the app name (with the dogfood build when this is one) and whatever the
	// active tab has to say. Called on tab/focus change and each rendered frame;
	// set_title only fires when the string actually changed (avoids WM flicker).
	fn update_title(&mut self) {
		let custom = self.win_title.clone();
		// A --title is the whole answer, so nothing else is worked out.
		let suffix = match custom {
			Some(_) => None,
			None => self.title_suffix(),
		};
		let title = crate::tabtitle::window_title(
			config::rights(),
			custom.as_deref(),
			&config::title_prefix(),
			suffix.as_deref(),
		);
		let title = crate::tabtitle::with_note(title, self.conserve.note(Instant::now()));
		if title != self.last_win_title {
			self.window.set_title(&title);
			self.last_win_title = title;
		}
	}

	// Effective window opacity: a CLI --background-opacity override for this
	// window, else the configured value.
	fn opacity(&self) -> f32 {
		self.win_opacity
			.unwrap_or_else(|| config::settings().opacity)
	}

	// relayout every tab (not just the active one) - needed when the tab bar
	// appears/disappears (1<->2 tabs) and the pane area changes.
	fn relayout_all(&mut self) {
		let area = self.area();
		for pm in &mut self.tabs.list {
			pm.relayout(&mut self.text, area);
		}
	}

	// New window = a fresh process (each window is its own process), started in
	// the focused pane's current directory so it picks up where you are. The
	// child sets up its own ctl socket/env at startup; a reaper thread waits on
	// it so a closed window can't linger as a zombie.
	fn new_window(&mut self) {
		let cwd = self
			.tabs
			.cur()
			.panes
			.get(&self.tabs.cur().focused)
			.and_then(|p| p.term.cwd());
		let exe = match std::env::current_exe() {
			Ok(p) => p,
			Err(e) => {
				eprintln!("new window: {e}");
				return;
			}
		};
		let config = config::config_override();
		let mut cmd = new_window_command(&exe, cwd.as_deref(), config.as_deref());
		match cmd.spawn() {
			Ok(mut child) => {
				std::thread::spawn(move || {
					let _ = child.wait();
				});
			}
			Err(e) => eprintln!("new window: {e}"),
		}
	}

	// A pane whose shell has gone: close just that pane, then its tab, then the
	// window. Mirrors the Close-Pane menu cascade. The pane can be in any tab -
	// a background tab's shell exits the same way.
	fn close_dead_pane(&mut self, id: PaneId) {
		let area = self.area();
		let Some(tab_idx) = self
			.tabs
			.list
			.iter()
			.position(|pm| pm.panes.contains_key(&id))
		else {
			return;
		};
		match close_scope(true, self.tabs.list[tab_idx].panes.len(), self.tabs.len()) {
			CloseScope::Pane => {
				self.tabs.list[tab_idx].close(&mut self.text, id, area);
			}
			CloseScope::Tab => self.close_tab_at(tab_idx),
			CloseScope::Window => self.quit = true,
			CloseScope::Nothing => {}
		}
		self.forget_dead_menu();
		self.dirty = true;
	}

	// A popup acts on the pane it was opened for. A shell ending closes that pane,
	// and can take its tab with it, so the popup goes too rather than standing
	// open over a pane that is not there.
	fn forget_dead_menu(&mut self) {
		let Some(menu) = &self.menu else {
			return;
		};
		let target = menu.target;
		if !self
			.tabs
			.list
			.iter()
			.any(|pm| pm.panes.contains_key(&target))
		{
			self.menu = None;
			self.bar_open = None;
		}
	}

	// --keep-open: the shell is gone but the pane stays, saying how it ended and
	// waiting for a key. Answers whether the pane is being held, so the caller
	// knows not to close it. ChildExit carries the status and Exit follows with
	// nothing, so the second call finds it already held and leaves it alone.
	fn hold_dead_pane(&mut self, id: PaneId, status: &str) -> bool {
		let Some(p) = self
			.tabs
			.list
			.iter_mut()
			.find_map(|pm| pm.panes.get_mut(&id))
		else {
			return false;
		};
		if p.held {
			return true;
		}
		if !p.keep_open {
			return false;
		}
		p.held = true;
		p.read_only = true;
		p.term.feed(
			format!(
				"\r\n{}: exit {status} - press a key to close\r\n",
				config::APP_NAME
			)
			.as_bytes(),
		);
		self.dirty = true;
		true
	}

	// The surface's new size, and the device's copies of it where there is one.
	fn resize_surface(&mut self, w: u32, h: u32) {
		if w == 0 || h == 0 {
			return;
		}
		self.surface_px = (w, h);
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.gfx.resize(w, h);
			gpu.scrim.resize(&gpu.gfx.device, w, h);
		}
	}

	// The performance profile after a rebuild that changed what draws. Software
	// on a machine with a card steps the session to Low, and a step this made
	// comes off once the card draws again. Nothing is written either way.
	fn follow_renderer(&mut self, drawn: Drawn) {
		self.drawn = drawn;
		let live = config::settings();
		let Some(step) = session_step(&live, drawn, self.software_step) else {
			return;
		};
		idledbg(&format!("profile step for {drawn:?}: {step:?}"));
		self.software_step = step.is_some();
		let mut next = (*live).clone();
		next.stepped_profile = step;
		self.apply_new_settings(&live, next, false);
	}

	// Every piece of chrome off at once, and back the way it was.
	fn toggle_bare(&mut self) {
		self.bare = !self.bare;
		let (saved, now) = bare_chrome(self.bare, self.bare_saved, (self.decorated, self.menu_bar));
		self.bare_saved = saved;
		(self.decorated, self.menu_bar) = now;
		self.window.set_decorations(self.decorated);
		self.bar_open = None;
		self.relayout_all();
	}

	fn toggle_fullscreen(&self) {
		let fullscreen = match self.window.fullscreen() {
			Some(_) => None,
			None => Some(Fullscreen::Borderless(None)),
		};
		self.window.set_fullscreen(fullscreen);
	}

	// Request the Settings window (App opens it; window creation needs the loop).
	fn open_settings(&mut self) {
		self.pending_settings.get_or_insert_with(Instant::now);
		self.menu = None;
		self.bar_open = None;
	}

	// Live-apply edited settings (from the dialog), persist, and rebuild whatever
	// the change touched (text metrics, background image, opacity, window size).
	// Returns false if the config file looked open elsewhere so the write was
	// skipped - the caller (dialog OK) then keeps the dialog open instead of
	// closing over an unsaved change. The values still apply live regardless.
	fn apply_settings_values(&mut self, orig: &config::Settings, edited: config::Settings) -> bool {
		// The Shells tab edits the list now, so this is the one path allowed to
		// write it - and it is the dialog's copy that wins. The baseline is the
		// LIVE list rather than the dialog's own `orig`: a scan that arrived while
		// the dialog was open has already been folded into both of its copies
		// (Dialog::fold_shells), so the two agree, and taking the live one is what
		// keeps them honest if they ever do not. They do not when another window
		// changed the list after this one loaded; the dialog's list is then
		// written whole, and it already holds that change.
		//
		// An entry with no command names nothing to run, so it is dropped here
		// rather than written - that is the whole of the grid's "Command is
		// required" rule at the point the list leaves the dialog.
		let live = config::settings();
		let mut orig = orig.clone();
		let mut edited = edited;
		orig.shells.clone_from(&live.shells);
		edited.shells.retain(|e| !e.command.trim().is_empty());
		// Remote and a watch step are live state the dialog only copied when it
		// opened, so a change to either since then must survive the Apply.
		config::keep_session_on_apply(&live, &orig, &mut edited);
		config::keep_wallpaper_on_apply(&live, self.wp_locked, &mut orig, &mut edited);
		// The dialog opened on the file, so the file gets only what was edited,
		// while this window takes everything that differs from what it runs -
		// including what another window saved since this one loaded.
		let wrote = config::persist(&orig, &edited);
		self.apply_new_settings(&live, edited, false);
		wrote
	}

	// What Settings opens on: the file as it is now, not what this window loaded,
	// with this session's own choices folded in the way a reload folds them.
	fn settings_for_dialog(&self) -> config::Settings {
		settings_after_reload(
			&config::settings(),
			config::reload_from_disk(),
			&self.cli_style,
			self.wp_locked,
		)
	}

	// Re-read config.shcl from disk and live-apply it (the "internal command" for
	// picking up hand-edits without a file watcher). The file is the source here,
	// so nothing is persisted back.
	fn reload_config(&mut self) {
		let orig = config::settings().as_ref().clone();
		let edited = settings_after_reload(
			&orig,
			config::reload_from_disk(),
			&self.cli_style,
			self.wp_locked,
		);
		// Force the background image to re-read even when its path is unchanged:
		// the user may have swapped the file contents under the same name (#167).
		self.apply_new_settings(&orig, edited, true);
	}

	// Font zoom (hotkeys / View menu): step the zoom offset and rebuild the
	// text context at the new effective size. Window-wide, and remembered with
	// the window's size.
	fn font_zoom(&mut self, dir: i32) {
		config::nudge_font_zoom(dir);
		let scale = config::display_scale(self.window.scale_factor());
		self.rebuild_text(scale);
		self.dirty = true;
		self.note_zoom();
	}

	fn font_zoom_reset(&mut self) {
		if config::font_zoom_px() == 0 {
			return; // already at the configured size
		}
		config::reset_font_zoom();
		let scale = config::display_scale(self.window.scale_factor());
		self.rebuild_text(scale);
		self.dirty = true;
		self.note_zoom();
	}

	// The window keeps its size and shows a different grid, so both are
	// saved: the next launch opens at this zoom and at this size, not at the
	// old grid drawn bigger.
	fn note_zoom(&mut self) {
		if !self.size_tracked {
			return;
		}
		self.watch.font_pinned = false;
		let (w, h) = self.surface_px;
		self.note_grid(w, h);
	}

	// Force the next frame through a full prepare + scrim build. Call whenever
	// something outside the signature's reach makes the retained GPU state stale
	// (new atlases, recreated textures, lost VRAM).
	fn invalidate_prepared(&mut self) {
		self.text_sig = None;
		self.scrim_sig = None;
	}

	// Rebuild the text context (cell metrics, chrome, pane buffers) for a new
	// scale factor or font, then relayout. Shared by settings-driven font
	// rebuilds and DPI scale-factor changes. The surface itself is reconfigured
	// separately (a Resized event follows a scale change).
	fn rebuild_text(&mut self, scale: f32) {
		self.text = TextCtx::new_cpu(scale);
		if let Some(gpu) = &self.gpu {
			self.text
				.attach_gpu(&gpu.gfx.device, &gpu.gfx.queue, gpu.gfx.format);
		}
		self.chrome = None; // cached chrome buffers are tied to the old FontSystem
		self.invalidate_prepared(); // fresh atlases hold nothing to reuse
		for pm in &mut self.tabs.list {
			pm.rebuild_buffers(&mut self.text);
		}
		self.relayout_all();
	}

	// Swap in `edited` and rebuild whatever changed vs `orig` (text metrics,
	// background image, window opacity). Shared by the dialog and config reload.
	// `force_bg` re-reads the image even if the path string didn't change.
	fn apply_new_settings(
		&mut self,
		orig: &config::Settings,
		edited: config::Settings,
		force_bg: bool,
	) {
		// a size set by hand, or one gone back to automatic; an automatic size
		// that only follows the window is the size it already has
		let size_of = |s: &config::Settings| {
			[config::auto::Setting::Columns, config::auto::Setting::Rows].map(|setting| {
				(!config::auto::automatic(s, setting))
					.then(|| config::auto::value(s, setting, config::auto::Place::default()))
			})
		};
		let resize = size_of(&edited) != size_of(orig);
		// copy_on_select changed -> apply to every existing pane too, so the
		// dialog toggle takes effect now, not only for panes spawned later
		if edited.copy_on_select != orig.copy_on_select {
			for pm in &mut self.tabs.list {
				for pane in pm.panes.values_mut() {
					pane.copy_select = edited.copy_on_select;
				}
			}
		}
		// What changed is judged on the LIVE values, before and after: a
		// performance profile sets the wallpaper and the halo on top of the
		// stored settings, so the dialog's own diff can be empty while the
		// picture changes completely.
		let before = config::settings();
		config::update(edited);
		let after = config::settings();
		let rebuild = crate::settings_ui::needs_text_rebuild(&before, &after);
		let bg = force_bg || crate::settings_ui::wallpaper_changed(&before, &after);
		let blur_changed = after.transparent_background_blur != before.transparent_background_blur;
		if crate::profile::current(&after) != crate::profile::current(&before) {
			self.rating.reset();
		}

		// Backdrop-blur hint toggled -> set/clear the compositor property live.
		if blur_changed {
			set_blur_behind(&self.window, config::settings().transparent_background_blur);
		}

		// Transparency is per-pixel (terminal background only) - never whole-window.
		// Nothing to do here; the bg fill picks up the new opacity on the next frame.
		// window dimensions changed in Settings -> resize to the new cell grid
		if resize {
			let (columns, rows) =
				config::auto::grid(&config::settings(), self.watch.key.as_deref());
			self.request_grid(columns, rows);
		}
		if rebuild {
			self.rebuild_text(config::display_scale(self.window.scale_factor()));
		} else if after.minimap != before.minimap || after.minimap_width != before.minimap_width {
			// the column takes real columns from the grid, so this is a layout change
			self.relayout_all();
		}
		if bg {
			// Turning the wallpaper or its rotation back on: the folder has to be
			// re-read, since the pick and its timer went with it on the way off.
			let resumed = after.rotation_folder().is_some() && before.rotation_folder().is_none();
			self.request_wallpaper(resumed);
		}
		self.dirty = true;
	}

	fn reveal_window(&mut self) {
		self.revealed = true;
		// Maximized only now: X11 drops the request for a window not yet
		// mapped, and on Windows it would show the window early. The
		// remembered size stays underneath as the restored size.
		if self.maximize_on_reveal && cfg!(windows) {
			self.window.set_maximized(true);
		}
		// On X11 the state also goes on the window itself before it maps, which
		// the window manager reads as it maps it. A request after the map alone
		// shows the window at the restored size first. winit holds a fullscreen
		// asked for while hidden until after the map, too.
		let mut states: Vec<&[u8]> = Vec::new();
		if self.maximize_on_reveal {
			states.extend([
				b"_NET_WM_STATE_MAXIMIZED_VERT".as_slice(),
				b"_NET_WM_STATE_MAXIMIZED_HORZ",
			]);
		}
		if self.window.fullscreen().is_some() {
			states.push(b"_NET_WM_STATE_FULLSCREEN");
		}
		preset_wm_state(&self.window, &states);
		self.window.set_visible(true);
		if self.maximize_on_reveal && !cfg!(windows) {
			self.window.set_maximized(true);
		}
		// The window manager places the window as it maps, and may put it on
		// another monitor than the one it was sized for. On Wayland this is
		// the first the window hears of which monitor it is on at all.
		self.note_moved();
		self.shell_scan_at = Some(Instant::now() + SHELL_SCAN_DELAY);
		self.shell_scan_cap = Some(Instant::now() + SHELL_SCAN_MAX_WAIT);
		if self.bench_id.is_some() {
			self.bench_at = Some(Instant::now() + BENCH_DELAY);
			self.bench_cap = Some(Instant::now() + BENCH_MAX_WAIT);
			self.bench_banner = Some(Instant::now());
		}
	}

	// returns true while any pane is still animating (caller keeps frames coming).
	// `force_rebuild` = the frame changed content/scroll/bell (not a pure cursor
	// animation), so panes re-shape text; false lets them reuse the cached frame.
	// A frame, on the device the window has. No device means nothing drawn and
	// no animation to keep frames coming for.
	fn render(&mut self, force_rebuild: bool) -> bool {
		let Some(mut gpu) = self.gpu.take() else {
			return false;
		};
		let animating = self.render_with(&mut gpu, force_rebuild);
		self.gpu = Some(gpu);
		animating
	}

	fn render_with(&mut self, gpu: &mut Gpu, force_rebuild: bool) -> bool {
		// once a frame has been drawn, later resizes are user-driven and may update
		// the remembered window size (startup/programmatic ones happen before this)
		self.size_tracked = true;
		let area = self.area();
		if area.w < 1.0 || area.h < 1.0 {
			return false;
		}
		// keep the window title tracking the active tab's foreground program
		// (deduped inside update_title, so this is cheap when nothing changed)
		self.update_title();

		let now = Instant::now();
		let dt = (now - self.last_frame).as_secs_f32().min(0.1);
		self.last_frame = now;
		let cfg = config::settings(); // one snapshot per frame, not per use/pane
		// chrome colors from the same snapshot, read once a frame
		let menu_fg_rgb = config::auto::color(&cfg, config::auto::Setting::MenuForeground);
		let menu_bg_rgb = config::auto::color(&cfg, config::auto::Setting::MenuBackground);
		let menu_border_rgb = config::menu_border_of(menu_bg_rgb);
		let menu_hover_rgb = config::menu_hover_of(menu_bg_rgb);

		// A full-screen program takes the minimap column and gives its width back to
		// the text, so the answer has to be settled before anything is laid out.
		if self.tabs.cur_mut().sync_minimap(&cfg) {
			self.relayout_all();
		}

		// Regaining the window, switching tab, or moving pane focus pokes the
		// focused pane: its cursor animation resumes immediately, from the top of
		// the cycle - no resume delay, that one is for input.
		let focus_sig = (self.tabs.active, self.tabs.cur().focused, self.focused);
		if self.focused && self.cursor_focus_sig != Some(focus_sig) {
			let id = self.tabs.cur().focused;
			if let Some(pane) = self.tabs.cur_mut().panes.get_mut(&id) {
				pane.poke_cursor();
			}
		}
		self.cursor_focus_sig = Some(focus_sig);

		// Visual-bell flash decays toward 0; while >0 the text is brightened (in
		// build) and we keep rendering so the fade is smooth.
		if self.bell_flash > 0.0 {
			self.bell_flash = (self.bell_flash * (-dt / BELL_TAU_S).exp()).max(0.0);
			if self.bell_flash < 0.01 {
				self.bell_flash = 0.0;
			}
		}
		let bell = self.bell_flash;

		// translucent background only when the surface supports it AND the user has
		// Transparency on - and it only ever affects the bg, never text/chrome.
		let bg_alpha = if gpu.gfx.transparent && cfg.transparent_background {
			self.opacity()
		} else {
			1.0
		};

		// Every list below comes out of the last frame's, emptied, and goes back at
		// the end. `pane_fulls` is what each pane's fill covers: the light-mode
		// wallpaper draws the fill itself and has to be clipped to it. Cursors are
		// drawn separately, above the scrim, so its halo can't obscure them.
		let FrameBufs {
			mut instances,
			mut pane_fulls,
			mut cursors,
			mut scrim_cursors,
			mut group_ranges,
			mut link_ranges,
			mut cursor_ranges,
		} = std::mem::take(&mut self.frame_bufs);
		let mut animating = bell > 0.0;
		if self.autoscroll_selection(dt) {
			animating = true;
		}
		// The outline shares the scrim's source and composite, so the pass runs
		// for either; only the blur is the halo's alone.
		let halo_on = cfg.text_scrim && cfg.text_scrim_radius > 0.0;
		let scrim_on = halo_on || cfg.text_outline > 0.0;
		// With both off nothing here draws, and its full-screen textures have no
		// business being allocated, nor the blur's with the halo off. Turning
		// either on grows them back.
		let scrim_use = crate::scrim::Use::of(halo_on, cfg.text_outline > 0.0);
		if gpu.scrim.set_use(&gpu.gfx.device, scrim_use) {
			self.invalidate_prepared();
		}

		self.text.color_frame();
		let win_focused = self.focused;
		let active_pane = self.tabs.cur().focused;
		// pane fill color is loop-invariant
		let pane_bg = {
			let mut c =
				config::srgb_f32(config::auto::color(&cfg, config::auto::Setting::Background));
			c[3] = bg_alpha;
			c
		};
		for (id, pane) in &mut self.tabs.cur_mut().panes {
			pane.scroll.advance(dt);
			pane.scrollbar_tick(dt, &cfg);
			if pane.bar_animating {
				animating = true;
			}
			let rect = pane.rect;
			// scope the expensive re-shape to panes that actually changed: fresh
			// PTY output (content_dirty), an active scroll ease, or a global
			// cause (bell flash, chrome/UI change) - idle siblings reuse their
			// cached frame instead of re-shaping at the busy pane's rate
			let force = force_rebuild || pane.content_dirty || pane.scroll.animating();
			crate::perf::timed(&crate::perf::BUILD_NS, || {
				pane.build(
					&mut self.text,
					dt,
					bell,
					force,
					win_focused && *id == active_pane,
				);
			});
			if pane.scroll.animating() || pane.cursor_animating {
				animating = true;
			}
			let draw = pane.draw();
			instances.push(RectInstance {
				pos: [pane.full.x, pane.full.y],
				size: [pane.full.w, pane.full.h],
				color: crate::gfx::see_through(pane_bg),
				..Default::default()
			});
			pane_fulls.push(pane.full);
			if let Some(cursor_quad) = draw.cursor {
				cursors.push((rect, cursor_quad));
			}
		}

		// The builds above are where a hyperlink hover is resolved, so the pointer
		// shape can only be settled after them - a link found under a pointer that
		// has stopped moving gets no further pointer event to react to.
		self.sync_cursor_icon();

		let under_len = instances.len() as u32;
		// per-pane bg quads (scissored to the pane so overscan rows don't bleed
		// into neighbors), copied once from each pane's retained frame. The
		// text-scrim color map reads the same run, so a glyph's halo takes its own
		// cell color, not always the global.
		for p in self.tabs.cur().panes.values() {
			let start = instances.len() as u32;
			instances.extend_from_slice(&p.draw().bg);
			group_ranges.push((p.rect, start, instances.len() as u32));
		}

		let ring_start = instances.len() as u32;
		// Scrollbars, drawn with the ring (after the text, so they overlay it) but
		// pushed first so the focus ring stays on top where they meet at the corner.
		for p in self.tabs.cur().panes.values() {
			if let Some(bar) = p.scrollbar(&self.text, &cfg) {
				let active = p.bar_drag.is_some() || p.bar_hover;
				instances.extend(scrollbar_insts(&bar, p.bar_fade(), active));
			}
			if let Some(g) = p.minimap(&self.text, &cfg) {
				instances.extend(minimap_insts(&g, p.map_drag.is_some()));
			}
		}
		// Focus ring only distinguishes panes when there's more than one; with a
		// single pane it's just an unwanted border line around the whole content
		// (the user wants background all the way to the edge), so skip it.
		if self.tabs.cur().panes.len() > 1 {
			if let Some(p) = self.tabs.cur().panes.get(&self.tabs.cur().focused) {
				instances.extend(focus_ring(p.full, self.text.scale));
			}
		}
		// drop-target tint while drag-reordering a pane
		if let Some(src) = self.dragging_pane {
			if let Some(target_id) = self.tabs.cur().pane_at(self.mouse.0, self.mouse.1) {
				if target_id != src {
					if let Some(p) = self.tabs.cur().panes.get(&target_id) {
						let mut color = config::srgb_f32(config::DROP_TARGET);
						color[3] = 0.30;
						instances.push(RectInstance {
							pos: [p.full.x, p.full.y],
							size: [p.full.w, p.full.h],
							color,
							..Default::default()
						});
					}
				}
			}
		}
		let ring_end = instances.len() as u32;

		// Column images: one texture per pane, uploaded only when the compose
		// behind them moved. Direct field access - `tabs.cur()` would borrow the
		// whole of self and the renderer needs it mutably.
		gpu.minimap.begin_frame();
		if cfg.minimap {
			let mm = &mut gpu.minimap;
			let (device, queue) = (&gpu.gfx.device, &gpu.gfx.queue);
			let res = (gpu.gfx.config.width as f32, gpu.gfx.config.height as f32);
			for (id, p) in &self.tabs.list[self.tabs.active].panes {
				if let Some(g) = p.minimap(&self.text, &cfg) {
					mm.prepare(device, queue, *id, g.preview, res, p.map_cache());
				}
			}
		}

		// cursor quads also feed the scrim's cursor-coverage texture (its own tex,
		// so cursor_scrim/cursor_outline gate it independently); the cursor still
		// draws crisp ABOVE the composite below. Collect them whenever the scrim is
		// on - the shader flags decide whether they reach the halo and/or outline.
		if scrim_on {
			scrim_cursors.extend(cursors.iter().map(|(_, q)| *q));
		}

		// Hyperlink underlines sit with the cursor, AFTER the scrim composite - they
		// are chrome about the text, not a cell background. Filed with the bg quads
		// they were painted over by the halo, which is densest right under the
		// glyphs, so a solid rule came out as a barcode tracing the letterforms.
		// They stay out of the scrim's coverage map either way (an underline should
		// cast no halo of its own), and stay under the cursor as before.
		for p in self.tabs.cur().panes.values() {
			let link_quads = &p.draw().links;
			if link_quads.is_empty() {
				continue;
			}
			let start = instances.len() as u32;
			instances.extend_from_slice(link_quads);
			link_ranges.push((p.rect, start, instances.len() as u32));
		}

		// cursor quads get their own per-pane ranges, drawn after the scrim composite
		for &(rect, cursor_quad) in &cursors {
			let start = instances.len() as u32;
			instances.push(cursor_quad);
			cursor_ranges.push((rect, start, instances.len() as u32));
		}

		let win_w = gpu.gfx.config.width as f32;
		let menu_h = self.menu_bar_h();
		let tab_h = self.tab_bar_h();

		// menu bar (File/Edit/...), drawn in the main pass at the very top; the
		// open menu's title is highlighted.
		let menubar_range = if self.menu_bar {
			let start = instances.len() as u32;
			instances.push(rect_inst(0.0, 0.0, win_w, menu_h, config::TAB_BAR_BG));
			let layout = self.menubar_layout();
			if let Some(idx) = self.bar_open {
				if let Some(&(x, w)) = layout.get(idx) {
					instances.push(rect_inst(x, 0.0, w, menu_h, menu_hover_rgb));
				}
			}
			// Alt held alone (no dropdown open): underline each title's
			// accelerator letter, like the open-dropdown items do (press the
			// letter to open).
			let marks =
				bar_title_underlines(input::opens_menu_title(self.mods), self.bar_open, &MENU_BAR);
			if !marks.is_empty() {
				let attrs = crate::text::ui_attrs();
				let rule = self.text.dip(CHROME_HAIRLINE);
				let underline_y = self.text.ui_baseline(0.0, menu_h) + rule;
				let title_pad = self.text.dip(MENU_BAR_PAD);
				for (i, c) in marks {
					let Some(&(x, _)) = layout.get(i) else {
						continue;
					};
					let mut buf = [0u8; 4];
					let letter_w = self.text.measure_ui_text(c.encode_utf8(&mut buf), &attrs);
					instances.push(rect_inst(
						x + title_pad,
						underline_y,
						letter_w,
						rule,
						menu_fg_rgb,
					));
				}
			}
			// always-visible copy-mode checkboxes (right side): outlines always,
			// filled per the focused pane's two independent triggers, so the state
			// is never hidden. Dimmed when this window isn't focused - the flags
			// stay set, but nothing copies until it regains focus.
			let fp = self.tabs.cur().panes.get(&self.tabs.cur().focused);
			let checked = [
				fp.is_some_and(|p| p.copy_select),
				fp.is_some_and(|p| p.copy_output),
			];
			if let Some(cb) = self.copybox_layout() {
				let border = copy_dim(menu_border_rgb, self.focused);
				let fill = copy_dim(menu_fg_rgb, self.focused);
				let box_rule = self.text.dip(CHROME_HAIRLINE);
				let tick_inset = self.text.dip(COPYBOX_TICK_INSET);
				for (checkbox, on) in cb.boxes.iter().zip(checked) {
					instances.push(rect_inst(
						checkbox.x - box_rule,
						checkbox.y - box_rule,
						checkbox.w + 2.0 * box_rule,
						checkbox.h + 2.0 * box_rule,
						border,
					));
					instances.push(rect_inst(
						checkbox.x,
						checkbox.y,
						checkbox.w,
						checkbox.h,
						config::TAB_BAR_BG,
					));
					if on {
						instances.push(rect_inst(
							checkbox.x + tick_inset,
							checkbox.y + tick_inset,
							checkbox.w - 2.0 * tick_inset,
							checkbox.h - 2.0 * tick_inset,
							fill,
						));
					}
				}
			}
			Some((start, instances.len() as u32))
		} else {
			None
		};

		// tab bar (only with >1 tab), drawn just below the menu bar. Its labels are
		// brought up to date here, before anything this frame reads the strip, and
		// only probed while it is hidden.
		let tab_bar_y = self.menubar_h();
		let tabbar_range = if self.tab_bar_visible() {
			self.refresh_tab_labels(None);
			self.tab_layout();
			let start = instances.len() as u32;
			instances.push(rect_inst(0.0, tab_bar_y, win_w, tab_h, config::TAB_BAR_BG));
			let first = self.tab_layout.first;
			// per-tab loop invariants
			let x_rgb = close_x_rgb();
			let tab_gap = self.text.dip(TAB_GAP);
			let (btn_y, btn_h) = tab_button_v(tab_bar_y, tab_h, self.text.scale);
			let cb_rule = self.text.dip(CHROME_HAIRLINE);
			// Where the caret and the selection sit inside the label of a tab being
			// renamed, measured once before the loop borrows nothing (measuring
			// wants &mut self.text).
			let edit_marks = self.tab_edit.as_ref().map(|edit| {
				let (from, to) = edit.range();
				(edit.tab, edit.text.clone(), from, to, edit.caret)
			});
			let edit_marks = edit_marks.map(|(tab, text, from, to, caret)| {
				let attrs = crate::text::ui_attrs();
				let mut upto = |at: usize| self.text.measure_ui_text(&text[..at], &attrs);
				(tab, upto(from), upto(to), upto(caret))
			});
			let edit_pad = self.text.dip(TAB_EDIT_PAD);
			let caret_w = self.text.dip(CHROME_HAIRLINE).max(1.0);
			let mut x = 0.0;
			for (slot, tab_w) in self.tab_layout.widths.iter().copied().enumerate() {
				let i = first + slot;
				let color = if i == self.tabs.active {
					config::TAB_ACTIVE
				} else {
					config::TAB_INACTIVE
				};
				// the button sits inside the bar: a gap each side of the seam, and it
				// runs to the bar's bottom edge less one hairline
				instances.push(rect_inst(
					x + tab_gap,
					btn_y,
					tab_w - 2.0 * tab_gap,
					btn_h,
					color,
				));
				// A rename in progress draws a text box in place of the label: a
				// recessed well, an outline in the focus color, then the selection
				// and the caret. All of it goes down before the text pass, so the
				// label itself sits on top.
				if let Some((edit_tab, sel_from, sel_to, caret)) = edit_marks {
					if i == edit_tab {
						let field = tab_edit_box(
							x,
							tab_w,
							tab_bar_y,
							tab_h,
							self.text.ui_line_h,
							self.text.scale,
						);
						instances.push(rect_inst(
							field.x - cb_rule,
							field.y - cb_rule,
							field.w + 2.0 * cb_rule,
							field.h + 2.0 * cb_rule,
							config::auto::color(&cfg, config::auto::Setting::Highlight),
						));
						instances.push(rect_inst(
							field.x,
							field.y,
							field.w,
							field.h,
							mix_rgb(color, [0x00, 0x00, 0x00], 0.45),
						));
						let text_x = field.x + edit_pad;
						let (top, h) = (field.y + cb_rule, field.h - 2.0 * cb_rule);
						let inside = |at: f32| (text_x + at).clamp(field.x, field.x + field.w);
						if sel_to > sel_from {
							let (from, to) = (inside(sel_from), inside(sel_to));
							instances.push(rect_inst(
								from,
								top,
								to - from,
								h,
								config::SELECTION_BG,
							));
						}
						instances.push(rect_inst(
							inside(caret).min(field.x + field.w - caret_w),
							top,
							caret_w,
							h,
							x_rgb,
						));
					}
				}
				// close-button box: a 1px outline (border rect + inner tab-bg fill).
				// The active tab's box fill leans faintly toward a pastel red - just
				// past noticeable, so the current tab reads at a glance without a
				// clashing accent.
				let cb = tab_close_box(x, tab_w, tab_bar_y, tab_h, self.text.scale);
				instances.push(rect_inst(
					cb.x - cb_rule,
					cb.y - cb_rule,
					cb.w + 2.0 * cb_rule,
					cb.h + 2.0 * cb_rule,
					menu_border_rgb,
				));
				let box_fill = if self.tab_close_arm == Some(i) {
					// held down: light the button (press feedback; closes on release)
					mix_rgb(color, [0xff, 0xff, 0xff], 0.28)
				} else if i == self.tabs.active {
					mix_rgb(color, [0xd0, 0x80, 0x80], 0.12)
				} else {
					color
				};
				instances.push(rect_inst(cb.x, cb.y, cb.w, cb.h, box_fill));
				instances.push(close_x_inst(cb, x_rgb));
				x += tab_w;
			}
			Some((start, instances.len() as u32))
		} else {
			self.probe_tabs();
			None
		};

		// context menu quads (drawn in a second pass, on top of everything). A
		// submenu is just another popup in the same pass, drawn after its parent.
		let menu_range = if let Some(root) = &self.menu {
			let start = instances.len() as u32;
			for menu in root.chain() {
				let popup_h = menu.height();
				let border = self.text.dip(CHROME_HAIRLINE);
				instances.push(rect_inst(
					menu.x - border,
					menu.y - border,
					menu.w + 2.0 * border,
					popup_h + 2.0 * border,
					menu_border_rgb,
				));
				instances.push(rect_inst(menu.x, menu.y, menu.w, popup_h, menu_bg_rgb));
				if let Some(i) = menu.hover {
					instances.push(rect_inst(
						menu.x,
						menu.row_top(i),
						menu.w,
						menu.item_h,
						menu_hover_rgb,
					));
				}
				// faint separator lines between logical groups
				for (i, entry) in menu.entries.iter().enumerate() {
					if matches!(entry, Entry::Sep) {
						let sep_y = menu.row_top(i) + menu.sep_h / 2.0;
						let pad_x = self.text.dip(config::MENU_PAD_X);
						instances.push(rect_inst(
							menu.x + pad_x,
							sep_y,
							menu.w - pad_x * 2.0,
							self.text.dip(CHROME_HAIRLINE),
							config::menu_sep_of(menu_bg_rgb),
						));
					}
				}
				// the arrow marking a row that opens a submenu, drawn rather than set
				// in text: a font's own metrics decide where a glyph sits, and there
				// is no arrow every interface font carries (same reason as the tab
				// close mark)
				for (i, entry) in menu.entries.iter().enumerate() {
					if matches!(entry, Entry::Sub { .. }) {
						let h = (menu.item_h * 0.38).round().max(4.0);
						let w = (h * 0.62).round().max(3.0);
						let arrow = Rect {
							x: menu.x + menu.w - self.text.dip(config::MENU_PAD_X) - w,
							y: menu.row_top(i) + (menu.item_h - h) / 2.0,
							w,
							h,
						};
						instances.push(sub_arrow_inst(arrow, menu_fg_rgb));
					}
				}
				// accelerator underline under each item's accelerator letter (press it
				// to pick); items without one draw no underline
				let acc_attrs = crate::text::ui_attrs();
				let line_h = self.text.ui_line_h;
				let acc_rule = self.text.dip(CHROME_HAIRLINE);
				let acc_x =
					menu.x + self.text.dip(config::MENU_PAD_X) + self.text.dip(config::MENU_GUTTER);
				for (i, entry) in menu.entries.iter().enumerate() {
					if let Some((label, pos)) = entry_accel(entry) {
						if let Some(c) = label[pos..].chars().next() {
							let prefix_w = self.text.measure_ui_text(&label[..pos], &acc_attrs);
							let mut buf = [0u8; 4];
							let letter_w = self
								.text
								.measure_ui_text(c.encode_utf8(&mut buf), &acc_attrs);
							let top = menu.row_top(i) + (menu.item_h - line_h) / 2.0;
							instances.push(rect_inst(
								acc_x + prefix_w,
								top + line_h - self.text.dip(MENU_ACCEL_DROP),
								letter_w,
								acc_rule,
								menu_fg_rgb,
							));
						}
					}
				}
			}
			Some((start, instances.len() as u32))
		} else {
			None
		};

		// A tip's box, in the same overlay pass as the menus so it sits over
		// everything - including a pane's own text, which it sits on top of. The
		// tab strip's and a menu row's are the same two quads; only what they say
		// and where they sit differ.
		let tip_layout = self.tab_tip_layout();
		let menu_tip = self.menu_tip_layout();
		let bench_banner = self.bench_layout();
		let border = self.text.dip(CHROME_HAIRLINE);
		let tip_range = (tip_layout.is_some() || menu_tip.is_some() || bench_banner.is_some())
			.then(|| {
				let start = instances.len() as u32;
				// the banner dims the whole window first, which is the visible half of
				// "this window is busy"
				if bench_banner.is_some() {
					instances.push(RectInstance {
						pos: [0.0, 0.0],
						size: [gpu.gfx.config.width as f32, gpu.gfx.config.height as f32],
						color: [0.0, 0.0, 0.0, BENCH_SCRIM_ALPHA],
						..Default::default()
					});
				}
				// Flyover help has its own fill, a warmed lift off the menu color, so a
				// tip does not read as more of the chrome it hangs off. The banner is
				// a modal notice rather than a tip, so it keeps the menu's.
				let tip_edge = config::tip_border_of(menu_bg_rgb);
				let tip_fill = config::tip_bg_of(menu_bg_rgb);
				let boxes = tip_layout
					.iter()
					.chain(menu_tip.iter())
					.map(|(rect, _)| (rect, tip_edge, tip_fill))
					.chain(
						bench_banner
							.iter()
							.map(|(rect, _)| (rect, menu_border_rgb, menu_bg_rgb)),
					);
				for (box_rect, edge, fill) in boxes {
					instances.push(rect_inst(
						box_rect.x - border,
						box_rect.y - border,
						box_rect.w + 2.0 * border,
						box_rect.h + 2.0 * border,
						edge,
					));
					instances.push(rect_inst(
						box_rect.x, box_rect.y, box_rect.w, box_rect.h, fill,
					));
				}
				(start, instances.len() as u32)
			});
		let overlay_range = match (menu_range, tip_range) {
			(Some((start, _)), Some((_, end))) | (Some((start, end)), None) => Some((start, end)),
			(None, other) => other,
		};

		let margin = self.text.margin;
		let menu_fg = GColor::rgb(menu_fg_rgb[0], menu_fg_rgb[1], menu_fg_rgb[2]);
		// copy-mode labels dim with their checkboxes when the window is unfocused
		let copy_label_fg = {
			let c = copy_dim(menu_fg_rgb, self.focused);
			GColor::rgb(c[0], c[1], c[2])
		};
		// compute before borrowing panes for `areas` (menubar_layout takes &mut self)
		let bar_layout = self.menubar_layout();
		let copyboxes = self.copybox_layout();
		// tab titles, as measured above. Each is fitted to the space its own tab
		// has, which is where a path gets shortened.
		let (tab_widths, tab_titles): (&[f32], &[String]) = if self.tab_bar_visible() {
			(&self.tab_layout.widths, &self.tab_layout.labels)
		} else {
			(&[], &[])
		};
		// keep the shaped chrome text current (see ChromeCache) - a color change
		// rebuilds it all, otherwise only changed tab titles re-shape
		if self
			.chrome
			.as_ref()
			.is_some_and(|cache| cache.menu_fg != menu_fg_rgb)
		{
			self.chrome = None;
		}
		if self.chrome.is_none() {
			let shape_ui = |text: &mut TextCtx, s: &str, w: f32, h: f32, color: GColor| {
				let mut buf = text.new_ui_buffer(w, h);
				let mut attrs = crate::text::ui_attrs();
				attrs.color_opt = Some(color);
				buf.set_text(&mut text.font_system, s, &attrs, Shaping::Advanced, None);
				buf.shape_until_scroll(&mut text.font_system, false);
				buf
			};
			// menu-bar titles (one per top-level menu) plus the trailing
			// "Copy on / select / output" labels for the always-visible checkboxes
			let menubar = MENU_BAR
				.iter()
				.chain(COPYBOX_LABELS.iter())
				.map(|title| {
					let w = self.text.dip(MENUBAR_TEXT_W);
					shape_ui(&mut self.text, title, w, menu_h, menu_fg)
				})
				.collect();
			self.chrome = Some(ChromeCache {
				menu_fg: menu_fg_rgb,
				menubar,
				tabs: Vec::new(),
			});
			self.chrome_rev = self.chrome_rev.wrapping_add(1);
		}
		{
			#[allow(clippy::unwrap_used, reason = "set just above when it was None")]
			let cache = self.chrome.as_mut().unwrap();
			let mut reshaped = cache.tabs.len() > tab_titles.len();
			cache.tabs.truncate(tab_titles.len());
			let scale = self.text.scale;
			for (i, title) in tab_titles.iter().enumerate() {
				let title_w = tab_title_w(tab_widths[i], scale);
				// an unchanged title in an unchanged tab keeps its shaped buffer;
				// a width change re-wraps it
				if cache.tabs.get(i).is_some_and(|(cached, cached_w, _)| {
					cached == title && (*cached_w - title_w).abs() < 0.01
				}) {
					continue;
				}
				reshaped = true;
				let mut buf = self.text.new_ui_buffer(title_w, tab_h);
				let mut attrs = crate::text::ui_attrs();
				attrs.color_opt = Some(menu_fg);
				buf.set_text(
					&mut self.text.font_system,
					title,
					&attrs,
					Shaping::Advanced,
					None,
				);
				buf.shape_until_scroll(&mut self.text.font_system, false);
				if i < cache.tabs.len() {
					cache.tabs[i] = (title.clone(), title_w, buf);
				} else {
					cache.tabs.push((title.clone(), title_w, buf));
				}
			}
			if reshaped {
				self.chrome_rev = self.chrome_rev.wrapping_add(1);
			}
		}

		// Fingerprint every input to the prepared text set. A pure cursor frame
		// reproduces it exactly, which is the signal that glyphon's retained
		// buffers are still correct and both prepares can be skipped. Anything
		// missed here shows up as an extra prepare, never as stale text - so err
		// toward including a value rather than reasoning that it can't change.
		let text_sig = {
			use std::hash::{Hash, Hasher};
			let mut hasher = std::collections::hash_map::DefaultHasher::new();
			self.chrome_rev.hash(&mut hasher);
			gpu.gfx.config.width.hash(&mut hasher);
			gpu.gfx.config.height.hash(&mut hasher);
			margin.to_bits().hash(&mut hasher);
			for w in tab_widths {
				w.to_bits().hash(&mut hasher);
			}
			self.menu_bar.hash(&mut hasher);
			self.tab_bar_visible().hash(&mut hasher);
			self.tabs.active.hash(&mut hasher);
			self.tab_edit.as_ref().map(|e| e.tab).hash(&mut hasher); // moves the label into its box
			self.focused.hash(&mut hasher); // dims the copy-mode labels
			scrim_on.hash(&mut hasher);
			// one pointer covers every setting: a change swaps the whole snapshot
			(std::sync::Arc::as_ptr(&cfg) as usize).hash(&mut hasher);
			for (id, p) in &self.tabs.cur().panes {
				id.hash(&mut hasher);
				p.shape_rev.hash(&mut hasher); // bumped by every full re-shape
				p.draw().top.to_bits().hash(&mut hasher);
				for v in [p.rect.x, p.rect.y, p.rect.w, p.rect.h] {
					v.to_bits().hash(&mut hasher);
				}
				match &p.draw().slide {
					None => 0u8.hash(&mut hasher),
					Some(s) => {
						1u8.hash(&mut hasher);
						s.has_band.hash(&mut hasher);
						s.has_top_band.hash(&mut hasher);
						for v in [
							s.band_top,
							s.split_y,
							s.top_split_y,
							s.region_clip_t,
							s.region_clip_b,
						] {
							v.to_bits().hash(&mut hasher);
						}
					}
				}
			}
			hasher.finish()
		};
		let prep = crate::perf::mark();
		let text_same = self.text_sig == Some(text_sig);

		// All rect instances and the bg-image shader work in absolute
		// framebuffer pixels (matching the glyphon viewport), so the resolution
		// is the whole window - NOT the content `area`, which is shorter by the
		// menu/tab bars and would shift cell bg + cursor down relative to text.
		let (frame_w, frame_h) = (gpu.gfx.config.width as f32, gpu.gfx.config.height as f32);
		self.text
			.update_viewport(&gpu.gfx.queue, gpu.gfx.config.width, gpu.gfx.config.height);
		self.text.set_text_blend(
			&gpu.gfx.queue,
			crate::text::text_blend(
				cfg.fg,
				config::auto::color(&cfg, config::auto::Setting::Background),
				cfg.text_dark_on_light,
			),
		);
		gpu.rects.set_resolution(&gpu.gfx.queue, frame_w, frame_h);
		// How the picture is mixed with the background. Light mode needs a different
		// blend, not a different number (visibility.rs). Re-read per frame: the mode
		// can flip under a running window and nothing reloads a wallpaper for it.
		let wp_mix = gpu.wallpaper_img.as_ref().map(|img| {
			crate::visibility::wallpaper_mix(
				&cfg,
				img.opacity(),
				cfg.wallpaper_summary.map(|s| s.picture()),
			)
		});
		let wp_writes_fill = wp_mix.is_some_and(|m| m.perceptual);
		if let (Some(img), Some(mix)) = (&gpu.wallpaper_img, wp_mix) {
			img.set_look(&gpu.gfx.queue, frame_w, frame_h, mix, pane_bg);
		}
		gpu.rects
			.upload(&gpu.gfx.device, &gpu.gfx.queue, &instances);

		// Nothing that feeds the text changed, so glyphon's prepared buffers from
		// the last frame still describe this one exactly. Skipping the whole area
		// build + both prepares is the point of the signature: shaping and
		// glyph-cache lookups are over half the per-frame cost, and an idle cursor
		// pulse repeats them 30x a second for no visual difference.
		if !text_same {
			#[allow(
				clippy::unwrap_used,
				reason = "set at the top of the frame when it was None"
			)]
			let chrome = self.chrome.as_ref().unwrap();
			let mut areas: Vec<TextArea> = Vec::new();
			for p in self.tabs.cur().panes.values() {
				// app-scroll slide: fill the revealed gap from the scrolled-off strip,
				// draw the current scroll region over it, then the static bands unshifted
				let draw = p.draw();
				match &draw.slide {
					Some(slide) => {
						if let Some(strip) = p.strip_text_area(slide, margin) {
							areas.push(strip);
						}
						areas.push(p.text_area_band(
							draw.top,
							margin,
							slide.region_clip_t,
							slide.region_clip_b,
						));
						if slide.has_top_band {
							areas.push(p.text_area_band(
								slide.band_top,
								margin,
								f32::MIN,
								slide.top_split_y,
							));
						}
						if slide.has_band {
							areas.push(p.text_area_band(
								slide.band_top,
								margin,
								slide.split_y,
								f32::MAX,
							));
						}
					}
					None => areas.push(p.text_area(draw.top, margin)),
				}
				areas.extend(p.glyph_areas(margin));
				areas.extend(p.emoji_area(margin));
			}
			if self.menu_bar {
				for (i, buf) in chrome.menubar.iter().enumerate() {
					let Some((left, left_bound, right_bound, top)) =
						menubar_text_slot(&self.text, menu_h, &bar_layout, copyboxes.as_ref(), i)
					else {
						continue;
					};
					// trailing buffers are the copy-mode labels - dim them off-focus
					let color = if i < bar_layout.len() {
						menu_fg
					} else {
						copy_label_fg
					};
					areas.push(TextArea {
						buffer: buf,
						left,
						top,
						scale: 1.0,
						bounds: TextBounds {
							left: left_bound as i32,
							top: 0,
							right: right_bound as i32,
							bottom: menu_h as i32,
						},
						default_color: color,
						custom_glyphs: &[],
					});
				}
			}
			let editing = self.tab_edit.as_ref().map(|e| e.tab);
			let mut x = 0.0;
			for (slot, (_, _, buf)) in chrome.tabs.iter().enumerate() {
				let tab_w = tab_widths.get(slot).copied().unwrap_or(0.0);
				let close_x = x + tab_w - self.text.dip(TAB_CLOSE_W);
				// A tab being renamed reads inside its own box, at the same left
				// inset as the label it replaced.
				let field = (editing == Some(self.tab_layout.first + slot)).then(|| {
					tab_edit_box(
						x,
						tab_w,
						tab_bar_y,
						tab_h,
						self.text.ui_line_h,
						self.text.scale,
					)
				});
				let (left, clip_l, clip_r) = match field {
					Some(f) => (
						f.x + self.text.dip(TAB_EDIT_PAD),
						f.x + self.text.dip(TAB_EDIT_PAD),
						f.x + f.w - self.text.dip(TAB_EDIT_PAD),
					),
					None => (x + self.text.dip(TAB_TITLE_PAD), x, close_x),
				};
				let (btn_y, btn_h) = tab_button_v(tab_bar_y, tab_h, self.text.scale);
				areas.push(TextArea {
					buffer: buf,
					left,
					// a title is a path, so center its whole ink box in the button
					top: self.text.ui_ink_top(btn_y, btn_h),
					scale: 1.0,
					bounds: TextBounds {
						left: clip_l as i32,
						top: tab_bar_y as i32,
						right: clip_r as i32, // leave room for the close "X"
						bottom: (tab_bar_y + tab_h) as i32,
					},
					default_color: menu_fg,
					custom_glyphs: &[],
				});
				// the close "X" itself is a shader-drawn rect instance (tab bar pass)
				x += tab_w;
			}

			if let Err(e) = self.text.prepare(&gpu.gfx.device, &gpu.gfx.queue, areas) {
				// Atlas full (after a long session of varied glyphs). The normal per-frame
				// trim is at the END of render, below this early return - so without
				// trimming here the atlas never recovers and ALL text goes black for good
				// (cursor/cell-bg quads use a separate renderer, so they still show). Trim
				// now to free space; the next frame re-prepares with room and recovers.
				eprintln!(
					"{}: text prepare failed; trimming atlas to recover: {e:?}",
					config::APP_NAME
				);
				self.text.trim_atlas();
				self.text_sig = None;
				self.scrim_sig = None;
				return animating;
			}
			// scrim source pass has its own prepared set: pane text only (no chrome),
			// with de-bolded buffers where a pane built one (text_scrim_regular_weight)
			if scrim_on {
				let mut scrim_areas: Vec<TextArea> = Vec::new();
				for p in self.tabs.cur().panes.values() {
					// scrim follows the current frame's slide, INCLUDING the scrolled-off
					// strip filling the reveal gap - without it the strip's text (e.g. the
					// row just below a static header) loses its readability halo mid-slide
					// and the halo "pops" when the slide settles, reading as a shadow that
					// jumps at the band boundary. The strip holds only region rows, so it
					// is always scrim-safe (no furniture to guard out of the scrim).
					let draw = p.draw();
					match &draw.slide {
						Some(slide) => {
							if let Some(strip) = p.strip_text_area(slide, margin) {
								scrim_areas.push(strip);
							}
							scrim_areas.push(p.scrim_text_area_band(
								draw.top,
								margin,
								slide.region_clip_t,
								slide.region_clip_b,
							));
							if slide.has_top_band {
								scrim_areas.push(p.scrim_text_area_band(
									slide.band_top,
									margin,
									f32::MIN,
									slide.top_split_y,
								));
							}
							if slide.has_band {
								scrim_areas.push(p.scrim_text_area_band(
									slide.band_top,
									margin,
									slide.split_y,
									f32::MAX,
								));
							}
						}
						None => scrim_areas.push(p.scrim_text_area(draw.top, margin)),
					}
					scrim_areas.extend(p.glyph_areas(margin));
					scrim_areas.extend(p.emoji_area(margin));
				}
				if let Err(e) =
					self.text
						.prepare_scrim(&gpu.gfx.device, &gpu.gfx.queue, scrim_areas)
				{
					eprintln!(
						"{}: scrim prepare failed; trimming atlas to recover: {e:?}",
						config::APP_NAME
					);
					self.text.trim_atlas();
					self.text_sig = None;
					self.scrim_sig = None;
					return animating;
				}
			}
		} // !text_same
		self.text_sig = Some(text_sig);

		// lay out the menu into the overlay renderer: one proportional buffer
		// per item label (at the gutter), plus a checkmark buffer for checked toggles.
		// Re-shaping every frame while a menu sits open (the cursor blink keeps
		// frames coming) is skippable: the overlay text only depends on the menu's
		// geometry/labels/color. Only skip alongside text_same - the end-of-frame
		// atlas trim (which runs when !text_same) drops glyphs the retained overlay
		// vertex buffers still reference, so a trim frame must re-prepare.
		if self.menu.is_some()
			|| tip_layout.is_some()
			|| menu_tip.is_some()
			|| bench_banner.is_some()
		{
			let overlay_sig = {
				use std::hash::{Hash, Hasher};
				let mut hasher = std::collections::hash_map::DefaultHasher::new();
				gpu.gfx.config.width.hash(&mut hasher);
				gpu.gfx.config.height.hash(&mut hasher);
				self.chrome_rev.hash(&mut hasher); // covers a menu color change
				for (_, placed) in tip_layout
					.iter()
					.chain(menu_tip.iter())
					.chain(bench_banner.iter())
				{
					for (left, top, line) in placed {
						left.to_bits().hash(&mut hasher);
						top.to_bits().hash(&mut hasher);
						line.hash(&mut hasher);
					}
				}
				for menu in self.menu.iter().flat_map(ContextMenu::chain) {
					menu.x.to_bits().hash(&mut hasher);
					menu.y.to_bits().hash(&mut hasher);
					menu.w.to_bits().hash(&mut hasher);
					menu.item_h.to_bits().hash(&mut hasher);
					for entry in &menu.entries {
						if let Some(label) = entry_label(entry) {
							label.hash(&mut hasher);
						}
						if let Entry::Item { check, .. } = entry {
							check.hash(&mut hasher);
						}
					}
				}
				hasher.finish()
			};
			if text_same && self.overlay_sig == Some(overlay_sig) {
				// prepared overlay from the last frame still matches
			} else {
				self.overlay_sig = Some(overlay_sig);
				// (left, top, buffer) collected first so the borrow of self.text ends
				let mut specs: Vec<(f32, f32, Buffer)> = Vec::new();
				let mut attrs = crate::text::ui_attrs();
				attrs.color_opt = Some(menu_fg);
				let tip_fg = config::tip_fg_of(menu_fg_rgb);
				let tip_col = Some(GColor::rgb(tip_fg[0], tip_fg[1], tip_fg[2]));
				// The tip alone shapes in the terminal font: its lines are a table
				// padded with spaces, which no proportional face can align.
				if let Some((box_rect, placed)) = &tip_layout {
					let mut tip_attrs = crate::text::mono_attrs();
					tip_attrs.color_opt = tip_col;
					let line_h = self.text.cell_h;
					for (left, top, line) in placed {
						let mut buf = self.text.new_buffer(box_rect.w, line_h);
						buf.set_text(
							&mut self.text.font_system,
							line,
							&tip_attrs,
							Shaping::Advanced,
							None,
						);
						buf.shape_until_scroll(&mut self.text.font_system, false);
						specs.push((*left, *top, buf));
					}
				}
				// a menu row's tip and the benchmark banner shape in the interface
				// font, like the rows and the chrome they sit among. The tip takes the
				// tip's text color with its box; the banner stays with the menu.
				let ui_lines = menu_tip
					.iter()
					.map(|(rect, placed)| (rect, placed, tip_col))
					.chain(
						bench_banner
							.iter()
							.map(|(rect, placed)| (rect, placed, attrs.color_opt)),
					);
				for (box_rect, placed, color) in ui_lines {
					let mut attrs = attrs.clone();
					attrs.color_opt = color;
					for (left, top, line) in placed {
						let mut buf = self.text.new_ui_buffer(box_rect.w, self.text.ui_line_h);
						buf.set_text(
							&mut self.text.font_system,
							line,
							&attrs,
							Shaping::Advanced,
							None,
						);
						buf.shape_until_scroll(&mut self.text.font_system, false);
						specs.push((*left, *top, buf));
					}
				}
				for menu in self.menu.iter().flat_map(ContextMenu::chain) {
					for (i, entry) in menu.entries.iter().enumerate() {
						let Some(label) = entry_label(entry) else {
							continue;
						};
						let top = menu.row_top(i) + (menu.item_h - self.text.ui_line_h) / 2.0;
						let mut buf = self.text.new_ui_buffer(menu.w, menu.item_h);
						buf.set_text(
							&mut self.text.font_system,
							label,
							&attrs,
							Shaping::Advanced,
							None,
						);
						buf.shape_until_scroll(&mut self.text.font_system, false);
						let pad_x = self.text.dip(config::MENU_PAD_X);
						let gutter = self.text.dip(config::MENU_GUTTER);
						specs.push((menu.x + pad_x + gutter, top, buf));
						if matches!(
							entry,
							Entry::Item {
								check: Some(true),
								..
							}
						) {
							let mut check_buf = self.text.new_ui_buffer(gutter, menu.item_h);
							check_buf.set_text(
								&mut self.text.font_system,
								"\u{2713}",
								&attrs,
								Shaping::Advanced,
								None,
							);
							check_buf.shape_until_scroll(&mut self.text.font_system, false);
							specs.push((menu.x + pad_x, top, check_buf));
						}
					}
				}
				let (sw, sh) = (gpu.gfx.config.width as i32, gpu.gfx.config.height as i32);
				let areas: Vec<TextArea> = specs
					.iter()
					.map(|(left, top, buf)| TextArea {
						buffer: buf,
						left: *left,
						top: *top,
						scale: 1.0,
						bounds: TextBounds {
							left: 0,
							top: 0,
							right: sw,
							bottom: sh,
						},
						default_color: menu_fg,
						custom_glyphs: &[],
					})
					.collect();
				let _ = self
					.text
					.prepare_overlay(&gpu.gfx.device, &gpu.gfx.queue, areas);
			}
		}

		crate::perf::since(&crate::perf::PREP_NS, prep);
		let acquire = crate::perf::mark();
		let frame = match gpu.gfx.begin_frame() {
			Ok(frame) => frame,
			Err(why) => {
				// The prepares above wrote buffers that wgpu keeps until a submit, so
				// a frame that kept failing to acquire kept every one of them. A Mac
				// lost tens of MB a second that way, until the driver hung.
				gpu.gfx.queue.submit(std::iter::empty());
				if !self.revealed
					&& reveal_due(
						false,
						Instant::now() >= self.reveal_deadline,
						why == NoFrame::Occluded,
					) {
					self.reveal_window();
				}
				// The pass already cleared what asked for this frame, so it is owed
				// here or nothing asks again until the desktop does (a busy or full
				// GPU times out the acquire). The backoff paces the retry, not the
				// animation, so a surface that keeps refusing cannot spin.
				let wait =
					self.frame_retry
						.missed(Instant::now(), FRAME_RETRY_FIRST, FRAME_RETRY_MAX);
				idledbg(&format!("frame refused ({why:?}), again in {wait:?}"));
				return false;
			}
		};
		crate::perf::since(&crate::perf::ACQUIRE_NS, acquire);
		let encode = crate::perf::mark();
		let view = gpu.gfx.frame_view(&frame);
		let mut encoder = gpu
			.gfx
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("frame"),
			});

		// Text readability scrim: build the per-pixel color map, render the prepared
		// text to the scrim texture, blur it, then composite under the crisp text.
		// "Softness" 0..1 -> coverage boost: 0 = hard/solid (x10), 1 = soft/faint (x1)
		let scrim_intensity = 10.0 - cfg.text_scrim_softness.clamp(0.0, 1.0) * 9.0;
		// "Strength" 0..100% -> doublings of the finished halo alpha (0 = as built),
		// so the top of the slider is x32.
		let scrim_strength = cfg.text_scrim_strength.clamp(0.0, 100.0) / SCRIM_PCT_PER_DOUBLING;
		// In light mode the halo is a pale plate on whatever the picture darkened,
		// and the same alpha moves that much further than in dark mode, so the
		// composite redraws each alpha to match (visibility.rs). None in dark mode
		// and with no picture up.
		let halo_match = gpu.wallpaper_img.as_ref().and_then(|img| {
			self.halo_memo.get(
				&cfg,
				img.opacity(),
				cfg.wallpaper_summary.map(|s| s.picture()),
				cfg.wallpaper_summary
					.as_ref()
					.map_or(&[][..], |s| &s.spread[..]),
			)
		});
		// distance paths measure the halo extent in px; keep it a touch wider than
		// the (sigma-based) gaussian look so switching functions doesn't shrink it.
		let scrim_ext = crate::scrim::clamp_ext(cfg.text_scrim_radius * 2.0);
		// The halo is built from the text alone - the cursor lives in its own
		// coverage texture and only joins at the blur (cursor_scrim) or the
		// composite (cursor_outline). So when the text is unchanged the color map,
		// the text-coverage pass and the blur can all be reused from last frame;
		// every scrim texture is stored, not transient. That is most of the idle
		// GPU cost. The blur still has to re-run if the cursor feeds it.
		let scrim_cached = scrim_on && text_same && self.scrim_sig == Some(text_sig);
		let blur_cached = scrim_cached && !cfg.cursor_scrim;
		if scrim_on {
			if !scrim_cached {
				gpu.scrim.render_bgcolor(
					&gpu.gfx.device,
					&gpu.gfx.queue,
					&mut encoder,
					&instances[under_len as usize..ring_start as usize],
					config::srgb_f32(config::auto::color(&cfg, config::auto::Setting::Background)),
				);
			}
			if cfg.cursor_scrim || cfg.cursor_outline {
				gpu.scrim
					.upload_cursors(&gpu.gfx.device, &gpu.gfx.queue, &scrim_cursors);
			}
			if !scrim_cached {
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("scrim text"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: gpu.scrim.text_view(),
						resolve_target: None,
						depth_slice: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: None,
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				let _ = self.text.render_scrim(&mut pass);
			}
			// cursor coverage in its own texture (kept apart from the text so the
			// halo and the outline can each include it independently). Skipped -
			// full-res clear included - when neither flag samples it.
			if cfg.cursor_scrim || cfg.cursor_outline {
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("scrim cursor"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: gpu.scrim.cursor_view(),
						resolve_target: None,
						depth_slice: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: None,
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				gpu.scrim.draw_cursors(&mut pass);
			}
			if halo_on && !blur_cached {
				gpu.scrim.blur(
					&gpu.gfx.queue,
					&mut encoder,
					cfg.text_scrim_radius,
					scrim_ext,
					cfg.text_scrim_ramp,
					if cfg.cursor_scrim { 1.0 } else { 0.0 },
					cfg.text_scrim_function,
				);
			}
			self.scrim_sig = Some(text_sig);
		} else {
			self.scrim_sig = None;
		}

		{
			let divider = config::srgb_f32(config::DIVIDER);
			// transparent base only when the background is actually see-through
			// (same gate as bg_alpha): pane-gap dividers then show the desktop.
			// Otherwise the clear must be opaque - the X11 window is always an
			// ARGB visual, so any alpha<1 pixel (the 1px divider slits, AA edges
			// of fractional pane rects) lets the compositor blend the desktop
			// through as bright speckles along the split lines.
			let clear = if gpu.gfx.transparent && cfg.transparent_background {
				wgpu::Color::TRANSPARENT
			} else {
				wgpu::Color {
					r: divider[0] as f64,
					g: divider[1] as f64,
					b: divider[2] as f64,
					a: 1.0,
				}
			};
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("main pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});

			let (sw, sh) = (gpu.gfx.config.width, gpu.gfx.config.height);
			// pane backgrounds (exactly pane-sized, no clip needed) - skipped where the
			// wallpaper writes them itself
			if !wp_writes_fill {
				gpu.rects.draw(&mut pass, 0..under_len);
			}
			// background image over the pane fill, under cells/text
			if let Some(img) = &gpu.wallpaper_img {
				if wp_writes_fill {
					// The mix replaces the fill rather than blending over it, so it has
					// to stop at the pane edge - otherwise the divider slits between
					// panes would take the pane's background color instead of their own.
					for full in &pane_fulls {
						let (x, y, w, h) = scissor(*full, sw, sh);
						if w == 0 || h == 0 {
							continue;
						}
						pass.set_scissor_rect(x, y, w, h);
						img.draw(&mut pass);
					}
					pass.set_scissor_rect(0, 0, sw, sh);
				} else {
					img.draw(&mut pass);
				}
			}
			// per-pane cell bg + cursor, clipped to the pane
			for (rect, start, end) in &group_ranges {
				let (x, y, w, h) = scissor(*rect, sw, sh);
				if w == 0 || h == 0 {
					continue;
				}
				pass.set_scissor_rect(x, y, w, h);
				gpu.rects.draw(&mut pass, *start..*end);
			}
			pass.set_scissor_rect(0, 0, sw, sh);
			// minimap previews over the pane fill and the wallpaper, under the
			// marker and thumb (which ride with the scrollbars, after the text)
			if cfg.minimap {
				for id in self.tabs.cur().panes.keys() {
					gpu.minimap.draw(&mut pass, *id);
				}
			}
			// menu/tab-bar quads before the text so their titles draw on top
			if let Some((start, end)) = menubar_range {
				gpu.rects.draw(&mut pass, start..end);
			}
			if let Some((start, end)) = tabbar_range {
				gpu.rects.draw(&mut pass, start..end);
			}
			// scrim goes under the crisp text, over the cell backgrounds. Clip it to
			// the content area so the halo only affects terminal text, never the
			// menu bar / tab titles above it.
			if scrim_on {
				// frame-invariant composite args: upload the uniform once, not per pane
				gpu.scrim.write_comp_uniform(
					&gpu.gfx.queue,
					scrim_intensity,
					cfg.text_outline,
					if cfg.cursor_outline { 1.0 } else { 0.0 },
					cfg.text_scrim_function,
					cfg.text_scrim_ramp,
					scrim_ext,
					scrim_strength,
					if halo_on { 1.0 } else { 0.0 },
					halo_match,
				);
				// The scrim is a full-frame blur - each glyph's halo spreads ~scrim_ext
				// px every direction. Composite it PER-PANE, clipped per-side: an edge that
				// borders ANOTHER pane (internal divider) clips at the content edge (rect
				// inset by the margin) so the halo can't reach the inter-pane gutter - the
				// "garbage around split lines"; an edge at the WINDOW border clips at the rect
				// edge so the outer halo still fills the window margin. The gutter (margin +
				// gap + margin) is wider than the halo reach, so no pane's halo touches a
				// neighbor's content region.
				let area = self.area();
				for (rect, _, _) in &group_ranges {
					// external = sits on the content-area boundary (window edge); otherwise it
					// borders a gap/another pane -> pull the clip in by the margin.
					let l = if rect.x <= area.x + 0.5 {
						rect.x
					} else {
						rect.x + margin
					};
					let t = if rect.y <= area.y + 0.5 {
						rect.y
					} else {
						rect.y + margin
					};
					let r = if rect.x + rect.w >= area.x + area.w - 0.5 {
						rect.x + rect.w
					} else {
						rect.x + rect.w - margin
					};
					let b = if rect.y + rect.h >= area.y + area.h - 0.5 {
						rect.y + rect.h
					} else {
						rect.y + rect.h - margin
					};
					let clip = Rect {
						x: l,
						y: t,
						w: (r - l).max(0.0),
						h: (b - t).max(0.0),
					};
					let (cx, cy, cw, ch) = scissor(clip, sw, sh);
					if cw == 0 || ch == 0 {
						continue;
					}
					pass.set_scissor_rect(cx, cy, cw, ch);
					gpu.scrim.composite(&mut pass);
				}
				pass.set_scissor_rect(0, 0, sw, sh);
			}
			// link underlines above the scrim (halo can't eat them), under the cursor
			for (rect, start, end) in &link_ranges {
				let (x, y, w, h) = scissor(*rect, sw, sh);
				if w == 0 || h == 0 {
					continue;
				}
				pass.set_scissor_rect(x, y, w, h);
				gpu.rects.draw(&mut pass, *start..*end);
			}
			// cursor above the scrim (halo can't obscure it), still under the crisp text
			for (rect, start, end) in &cursor_ranges {
				let (x, y, w, h) = scissor(*rect, sw, sh);
				if w == 0 || h == 0 {
					continue;
				}
				pass.set_scissor_rect(x, y, w, h);
				gpu.rects.draw(&mut pass, *start..*end);
			}
			pass.set_scissor_rect(0, 0, sw, sh);
			if let Err(e) = self.text.render(&mut pass) {
				eprintln!("{}: text render failed: {e:?}", config::APP_NAME);
			}
			gpu.rects.draw(&mut pass, ring_start..ring_end);
		}

		// second pass: context menu / menu-bar dropdown on top (preserves main pass)
		if let Some((mstart, mend)) = overlay_range {
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("overlay pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &view,
					resolve_target: None,
					depth_slice: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Load,
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			gpu.rects.draw(&mut pass, mstart..mend);
			let _ = self.text.render_overlay(&mut pass);
		}

		gpu.minimap.end_frame();
		crate::perf::since(&crate::perf::ENCODE_NS, encode);
		let submit = crate::perf::mark();
		gpu.gfx.queue.submit(Some(encoder.finish()));
		// the GL path's twin of the refused acquire above: a swap that failed
		let presented = gpu.gfx.end_frame(frame).is_ok();
		if presented {
			self.frame_retry = Retry::default();
		} else {
			let wait = self
				.frame_retry
				.missed(Instant::now(), FRAME_RETRY_FIRST, FRAME_RETRY_MAX);
			idledbg(&format!("frame not presented, again in {wait:?}"));
		}
		crate::perf::since(&crate::perf::SUBMIT_NS, submit);
		crate::perf::painted();
		// The window was created hidden; reveal it once a real frame is on screen at
		// the final size (no default-size/blank flash). reveal_want (async resize)
		// holds off until the surface reaches the grid size; the deadline is a hard
		// fallback so a WM that grants a different size can't leave it stuck hidden.
		if !self.revealed {
			let settled = self.reveal_want.is_none_or(|w| {
				gpu.gfx.config.width == w.width && gpu.gfx.config.height == w.height
			});
			if reveal_due(settled, Instant::now() >= self.reveal_deadline, false) {
				self.reveal_window();
			}
		}
		// This frame carried whatever the wallpaper worker answered, so the scan's
		// clock starts from HERE rather than from the reveal - a wallpaper that took
		// two seconds to decode used to have the scan running underneath it. Only
		// ever pushed back, never re-armed: once the scan has gone (shell_scan_at
		// cleared, which the backstop guarantees) a late wallpaper must not start a
		// second one.
		if self.revealed && self.wp_answered && !self.wp_shown {
			self.wp_shown = true;
			self.shell_scan_at = push_back(
				self.shell_scan_at,
				self.shell_scan_cap,
				Instant::now() + SHELL_SCAN_DELAY,
			);
			self.bench_at = push_back(self.bench_at, self.bench_cap, Instant::now() + BENCH_DELAY);
		}
		if env_flag(EnvFlag::Dump) {
			gpu.gfx.dump_offscreen("/tmp/silk_offscreen.png");
		}
		// Trim only on a frame that prepared. The trim clears glyphon's in-use set,
		// and a later allocation evicts whatever isn't in it - so trimming after a
		// skipped prepare would let the atlas drop glyphs the retained buffers are
		// still pointing at.
		if !text_same {
			self.text.trim_atlas();
		}
		self.frame_bufs = FrameBufs {
			instances,
			pane_fulls,
			cursors,
			scrim_cursors,
			group_ranges,
			link_ranges,
			cursor_ranges,
		}
		.emptied();
		animating && presented
	}
}

fn rect_inst(x: f32, y: f32, w: f32, h: f32, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [x, y],
		size: [w, h],
		color: config::srgb_f32(color),
		..Default::default()
	}
}

// The close-"X" mark: a shader-drawn quad (mode 1) whose two diagonal bars
// center exactly in `cb` at any size/DPI. Stroke scales with the box.
fn close_x_inst(cb: Rect, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [cb.x, cb.y],
		size: [cb.w, cb.h],
		color: config::srgb_f32(color),
		params: [QuadMode::CloseMark.code(), (cb.w * 0.14).max(1.4)],
	}
}

// The submenu arrow: a shader-drawn quad (mode 3) holding a right-pointing
// triangle that centers exactly in `at` at any size and DPI.
fn sub_arrow_inst(at: Rect, color: [u8; 3]) -> RectInstance {
	RectInstance {
		pos: [at.x, at.y],
		size: [at.w, at.h],
		color: config::srgb_f32(color),
		params: [QuadMode::Triangle.code(), 0.0],
	}
}

// One scrollbar piece: a rounded quad (mode 2) with the pill radius its short
// side implies, at `alpha` (the pane's fade times the piece's own weight).
fn bar_inst(r: Rect, color: [u8; 3], alpha: f32) -> RectInstance {
	let mut c = config::srgb_f32(color);
	c[3] = alpha;
	RectInstance {
		pos: [r.x, r.y],
		size: [r.w, r.h],
		color: c,
		params: [QuadMode::Rounded.code(), r.w.min(r.h) * 0.5],
	}
}

// The minimap's viewport marker. The pane's own scrollbar sits at the far edge,
// over the preview, so the column draws no bar of its own.
fn minimap_insts(g: &crate::minimap::Geom, active: bool) -> Option<RectInstance> {
	let cfg = config::settings();
	let handle = g.handle?;
	let alpha = if active {
		config::SCROLLBAR_ACTIVE_A
	} else {
		config::SCROLLBAR_IDLE_A
	};
	// a wash, not a lid: it marks where you are without hiding what is under it
	Some(RectInstance {
		pos: [handle.x, handle.y],
		size: [g.preview.w, handle.h],
		color: {
			let mut c = config::srgb_f32(config::auto::color(
				&cfg,
				config::auto::Setting::ScrollbarThumb,
			));
			c[3] = alpha * 0.28;
			c
		},
		..Default::default()
	})
}

// The scrollbar's quads for one pane: a faint track with the handle on it. The
// thumb brightens while hovered or dragged, the usual affordance.
fn scrollbar_insts(bar: &crate::pane::Bar, fade: f32, active: bool) -> [RectInstance; 2] {
	let cfg = config::settings();
	let thumb_a = if active {
		config::SCROLLBAR_ACTIVE_A
	} else {
		config::SCROLLBAR_IDLE_A
	};
	[
		bar_inst(
			bar.track,
			config::auto::color(&cfg, config::auto::Setting::ScrollbarTrough),
			fade * config::SCROLLBAR_TROUGH_A,
		),
		bar_inst(
			bar.thumb,
			config::auto::color(&cfg, config::auto::Setting::ScrollbarThumb),
			fade * thumb_a,
		),
	]
}

// close-"X" stroke color: menu fg dimmed toward the tab bg (~0.6), so it reads
// as a quiet button mark rather than a title character
fn close_x_rgb() -> [u8; 3] {
	let fg = config::menu_fg();
	let dim = |v: u8| ((v as u16 * 3) / 5) as u8;
	[dim(fg[0]), dim(fg[1]), dim(fg[2])]
}

fn mix_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
	let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
	[mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

// Dim a chrome color toward the bar background when the window isn't focused;
// used on the copy-mode checkboxes + labels to signal auto-copy is inert until
// the window regains focus (the pane's flags stay set meanwhile). Focused = no
// change.
fn copy_dim(color: [u8; 3], focused: bool) -> [u8; 3] {
	if focused {
		return color;
	}
	let bg = config::TAB_BAR_BG;
	let mix = |a: u8, b: u8| (a as f32 * 0.4 + b as f32 * 0.6) as u8;
	[
		mix(color[0], bg[0]),
		mix(color[1], bg[1]),
		mix(color[2], bg[2]),
	]
}

// X11 session? (Per-pixel transparency needs the glutin GL path only on X11;
// Wayland's wgpu surface already does premultiplied alpha.)
fn is_x11(el: &ActiveEventLoop) -> bool {
	use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
	el.owned_display_handle()
		.display_handle()
		.is_ok_and(|handle| {
			matches!(
				handle.as_raw(),
				RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_)
			)
		})
}

// Stable X11 WM_CLASS (+ Wayland app_id) so the window is identifiable to the
// WM/taskbar and matchable in compositor rules - e.g. Compiz's blur "Blur
// Windows" = class=SilkTerm. winit's with_name(general, instance) yields
// WM_CLASS = "instance", "general", so res_class="SilkTerm", res_name="silkterm".
#[cfg(target_os = "linux")]
fn with_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
	use winit::platform::wayland::WindowAttributesExtWayland;
	use winit::platform::x11::WindowAttributesExtX11;
	let attrs = WindowAttributesExtX11::with_name(attrs, "SilkTerm", "silkterm");
	WindowAttributesExtWayland::with_name(attrs, "SilkTerm", "silkterm")
}
#[cfg(not(target_os = "linux"))]
fn with_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
	attrs
}

// Ask a KWin/picom-style compositor to blur the desktop behind the window's
// translucent regions (frosted glass) via _KDE_NET_WM_BLUR_BEHIND_REGION: a
// single 0 cardinal = blur the whole window, deleting the property turns it off.
// X11-only and compositor-dependent - Compiz/GNOME ignore the hint (there the
// user enables blur in the compositor), and the compositor, not us, owns the
// blur radius. Opens a throwaway connection; called only at startup / on toggle.
#[cfg(target_os = "linux")]
fn set_blur_behind(window: &Window, enable: bool) {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use x11rb::connection::Connection;
	use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
	use x11rb::wrapper::ConnectionExt as _;

	let Ok(handle) = window.window_handle() else {
		return;
	};
	let xid = match handle.as_raw() {
		RawWindowHandle::Xlib(h) => h.window as u32,
		RawWindowHandle::Xcb(h) => h.window.get(),
		_ => return, // not X11 (Wayland/other): the hint is X11-only
	};
	let Ok((conn, _)) = x11rb::connect(None) else {
		return;
	};
	let Ok(cookie) = conn.intern_atom(false, b"_KDE_NET_WM_BLUR_BEHIND_REGION") else {
		return;
	};
	let Ok(reply) = cookie.reply() else {
		return;
	};
	let atom = reply.atom;
	if enable {
		let _ = conn.change_property32(PropMode::REPLACE, xid, atom, AtomEnum::CARDINAL, &[0u32]);
	} else {
		let _ = conn.delete_property(xid, atom);
	}
	let _ = conn.flush();
}
#[cfg(not(target_os = "linux"))]
fn set_blur_behind(_window: &Window, _enable: bool) {}

// The initial _NET_WM_STATE of a window not mapped yet. EWMH has a client
// set the property itself then, since a window manager ignores a state
// message for a window it does not manage yet.
#[cfg(target_os = "linux")]
fn preset_wm_state(window: &Window, states: &[&[u8]]) {
	use raw_window_handle::{HasWindowHandle, RawWindowHandle};
	use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
	use x11rb::wrapper::ConnectionExt as _;

	if states.is_empty() {
		return;
	}
	let Ok(handle) = window.window_handle() else {
		return;
	};
	let xid = match handle.as_raw() {
		RawWindowHandle::Xlib(h) => h.window as u32,
		RawWindowHandle::Xcb(h) => h.window.get(),
		_ => return,
	};
	let Ok((conn, _)) = x11rb::connect(None) else {
		return;
	};
	let atom = |name: &[u8]| {
		conn.intern_atom(false, name)
			.ok()
			.and_then(|cookie| cookie.reply().ok())
			.map(|reply| reply.atom)
	};
	let Some(property) = atom(b"_NET_WM_STATE") else {
		return;
	};
	let Some(values) = states
		.iter()
		.map(|name| atom(name))
		.collect::<Option<Vec<_>>>()
	else {
		return;
	};
	// checked, so the server has it before winit's own connection maps the window
	if let Ok(cookie) =
		conn.change_property32(PropMode::APPEND, xid, property, AtomEnum::ATOM, &values)
	{
		let _ = cookie.check();
	}
}
#[cfg(not(target_os = "linux"))]
fn preset_wm_state(_window: &Window, _states: &[&[u8]]) {}

// wgpu's guaranteed floor for max_texture_dimension_2d. The window is born
// before the device exists, so a birth size is held to the floor, and the
// grid-derived resize after it to what the device actually reports.
const SAFE_MAX_DIM: u32 = 8192;

/// The window/taskbar icon, decoded from the bundled logo (downscaled so the
/// _`NET_WM_ICON` payload stays small). The logo is wider than it is tall and every
/// place an icon is shown reserves a square, so it is stretched to fill one
/// rather than left floating in a band of nothing. None if it can't be decoded.
pub fn load_icon() -> Option<winit::window::Icon> {
	let img = image::load_from_memory(include_bytes!("../assets/logo.png")).ok()?;
	let img = img
		.resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
		.into_rgba8();
	let (w, h) = img.dimensions();
	winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

// Open a URL in the user's default browser (fire-and-forget, per platform).
fn open_url(url: &str) {
	let mut cmd = if cfg!(target_os = "macos") {
		let mut command = std::process::Command::new("open");
		command.arg(url);
		command
	} else if cfg!(target_os = "windows") {
		let mut command = std::process::Command::new("cmd");
		command.args(["/C", "start", "", url]);
		command
	} else {
		let mut command = std::process::Command::new("xdg-open");
		command.arg(url);
		command
	};
	let _ = cmd.spawn();
}

// Hand a clicked link to the desktop. A failure is the opener's (no xdg-open, a
// bad open_command) and is worth saying out loud once, not worth an alert.
fn open_link(url: &str) {
	let cfg = config::settings();
	let command = config::auto::text(&cfg, config::auto::Setting::OpenCommand);
	// automatic goes to the desktop's own opener, which takes the URL its own way
	let command = if config::auto::automatic(&cfg, config::auto::Setting::OpenCommand) {
		""
	} else {
		&command
	};
	if let Err(e) = crate::links::open(url, command) {
		eprintln!("{}: could not open {url}: {e}", config::APP_NAME);
	}
}

// clamp a pane rect to an integer scissor box inside the surface
fn scissor(rect: Rect, sw: u32, sh: u32) -> (u32, u32, u32, u32) {
	let x = rect.x.max(0.0).min(sw as f32) as u32;
	let y = rect.y.max(0.0).min(sh as f32) as u32;
	let right = (rect.x + rect.w).max(0.0).min(sw as f32) as u32;
	let bottom = (rect.y + rect.h).max(0.0).min(sh as f32) as u32;
	(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

fn focus_ring(rect: Rect, scale: f32) -> [RectInstance; 4] {
	// the calm one: the ring marks which pane is live, alongside the dialog's own
	// sliders and revert arrows, rather than the single keyboard-focused control
	let color = config::srgb_f32(config::auto::color(
		&config::settings(),
		config::auto::Setting::Highlight,
	));
	let thickness = config::dip(config::FOCUS_RING_PX, scale);
	[
		RectInstance {
			pos: [rect.x, rect.y],
			size: [rect.w, thickness],
			color,
			..Default::default()
		},
		RectInstance {
			pos: [rect.x, rect.y + rect.h - thickness],
			size: [rect.w, thickness],
			color,
			..Default::default()
		},
		RectInstance {
			pos: [rect.x, rect.y],
			size: [thickness, rect.h],
			color,
			..Default::default()
		},
		RectInstance {
			pos: [rect.x + rect.w - thickness, rect.y],
			size: [thickness, rect.h],
			color,
			..Default::default()
		},
	]
}

impl ApplicationHandler<UserEvent> for App {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.state.is_some() {
			return;
		}
		// AppKit's own window tabs would add a second set of tab rows to the
		// menus, one of them taking Ctrl+Tab from the shell. SilkTerm has tabs
		// of its own.
		#[cfg(target_os = "macos")]
		{
			use winit::platform::macos::ActiveEventLoopExtMacOS;
			event_loop.set_allows_automatic_window_tabbing(false);
		}
		let cli_win = &self.cli.win;
		let decorated = !cli_win.hide_frame.unwrap_or(false);
		let menu_bar = menu_bar_at_launch(cli_win.hide_menu, cfg!(target_os = "macos"));
		let win_title = cli_win.title.clone();
		let win_opacity = cli_win.opacity;
		// When both pixel dims are given, the window must be BORN at that size, not
		// resized into it: some EGL presents (VirtualGL's, for one) latch the surface
		// size at creation and never see later resizes, leaving a stale-offset blit.
		// Held to the floor every device meets, since the real limit is not known
		// until one exists; the grid-derived resize below uses that limit.
		let initial_size: winit::dpi::Size = match (cli_win.pixel_width, cli_win.pixel_height) {
			(Some(w), Some(h)) => {
				let (w, h) = fit_px(w, h, SAFE_MAX_DIM);
				winit::dpi::PhysicalSize::new(w, h).into()
			}
			_ => winit::dpi::LogicalSize::new(1000.0, 640.0).into(),
		};
		// On Windows, requesting transparency forces a no-redirection-bitmap
		// (layered) window that some virtual-desktop managers - VirtuaWin - won't
		// track, so it sits still across workspace switches. Alpha only reaches the
		// screen there through the composited DX12 path (Gfx::new_composited), so
		// an always-transparent window buys nothing when Transparency is off. Ask
		// for it only when it's actually in use; X11/Wayland always request it so
		// the live toggle works (no such side effect there).
		let want_transparent =
			!cfg!(windows) || config::settings().transparent_background || win_opacity.is_some();
		let attrs = Window::default_attributes()
			.with_title(crate::tabtitle::window_title(
				config::rights(),
				win_title.as_deref(),
				&config::title_prefix(),
				None,
			))
			.with_window_icon(load_icon())
			.with_decorations(decorated)
			.with_transparent(want_transparent)
			.with_inner_size(initial_size);
		let attrs = with_app_id(attrs); // stable WM_CLASS/app_id
		// The composited swapchain (Gfx::new_composited) wants no redirection
		// surface under it: with one, the desktop manager composes that surface
		// too, and winit's blur-behind hack for it is not needed either.
		#[cfg(windows)]
		let attrs = {
			use winit::platform::windows::WindowAttributesExtWindows;
			attrs.with_no_redirection_bitmap(want_transparent)
		};
		// Born hidden, then resized to the grid-derived size and drawn once before
		// being shown (revealed after the first correct frame in render). Otherwise it
		// flashes the 1000x640 default with a blank client, then jumps to the real
		// size and paints - visible on X11/Wayland as well as Windows.
		let attrs = attrs.with_visible(false);

		// On X11 the wgpu surface can't do per-pixel alpha, so we ALWAYS take the
		// glutin GL path there (transparent-capable backend), regardless of the
		// current Transparency setting - that way the toggle works live without a
		// relaunch (the bg alpha is gated per-frame, not the backend). Off-X11 the
		// normal wgpu path is used (Wayland already supports premultiplied alpha).
		// If the GL context can't be created, fall back to the native wgpu surface.
		let want_gl = is_x11(event_loop);
		let want = crate::gfx::wanted();
		let (mut gfx, window) =
			match want_gl.then(|| Gfx::new_gl_transparent(event_loop, attrs.clone(), want)) {
				Some(Ok(pair)) => pair,
				other => {
					if let Some(Err(e)) = other {
						eprintln!(
							"{}: GL backend unavailable ({e}); using native surface (no transparency)",
							config::APP_NAME
						);
					}
					let window = Arc::new(event_loop.create_window(attrs).unwrap_or_else(|e| {
						eprintln!("{}: could not create a window: {e}", config::APP_NAME);
						std::process::exit(2);
					}));
					#[cfg(windows)]
					let gfx = if want_transparent {
						Gfx::new_composited(window.clone(), want)
					} else {
						Gfx::new(window.clone(), want)
					};
					#[cfg(not(windows))]
					let gfx = Gfx::new(window.clone(), want);
					let gfx = gfx.unwrap_or_else(|e| {
						eprintln!("{}: no usable GPU/renderer: {e}", config::APP_NAME);
						std::process::exit(2);
					});
					(gfx, window)
				}
			};
		// System theme mode: seed the OS dark/light bit before the first frame so a
		// system-mode theme resolves to the right palette immediately (no flash).
		config::reapply_for_os(!matches!(window.theme(), Some(winit::window::Theme::Light)));
		// New hardware starts the performance profile over, and answers with the id
		// to write down if the pick is worth measuring rather than assuming.
		remote_override_at_launch();
		let (bench_id, software_step) = if gfx.drawn.instead_of_card() {
			let live = config::settings();
			let step = session_step(&live, gfx.drawn, false);
			if let Some(step) = step {
				set_live(|live| live.stepped_profile = step);
			}
			(None, step.is_some_and(|step| step.is_some()))
		} else {
			(rate_hardware(&gfx.adapter_info), false)
		};
		// Window-level CLI style (--font-name/-size, colors, bg image/fit/opacity)
		// overrides the loaded settings before text + bg image are built. Applied
		// after the theme/OS palette settles so it isn't clobbered. Per-pane style
		// stays deferred (needs a per-pane renderer).
		cli_win.apply_style();
		if cli_win.fullscreen.unwrap_or(false) {
			window.set_fullscreen(Some(Fullscreen::Borderless(None)));
		}
		// Request compositor backdrop blur (KWin/picom) if the setting is on; no-op
		// off-X11 and on compositors that don't honor the hint.
		set_blur_behind(&window, config::settings().transparent_background_blur);

		// An automatic size opens at the last size and font zoom, this monitor's
		// own where they are kept. A size or font size on the command line wins.
		let settings = config::settings();
		let monitor = crate::monitor::MonitorId::of_new_window(&window).map(|m| m.key());
		let kept = config::remembered_window(&settings, monitor.as_deref());
		let font_pinned = cli_win.style.font_size.is_some();
		if config::auto::keeps_size(&settings) && !font_pinned {
			config::set_font_zoom(kept.font_zoom);
		}

		// Transparency only ever affects the terminal background (per-pixel), never
		// the whole window - so there's no compositor whole-window-opacity fallback.
		let scale = config::display_scale(window.scale_factor());
		let mut text = TextCtx::new(&gfx.device, &gfx.queue, gfx.format, scale);
		let rects = RectRenderer::new(&gfx.device, gfx.format);
		let minimap = crate::minimap::MapRenderer::new(&gfx.device, gfx.format);
		let scrim =
			crate::scrim::Scrim::new(&gfx.device, gfx.format, gfx.config.width, gfx.config.height);

		// Resize to the configured initial grid now that cell metrics are known.
		// cell_w/cell_h/margin are physical px; floor() in content_dims gives the
		// exact column/row count at this size. If the request applies
		// synchronously winit returns the new size (no Resized event), so adopt
		// it here; otherwise a Resized event reconfigures the surface.
		// CLI columns/rows override config; --pixel-width/height override either
		// dimension directly. Add the menu-bar height (when shown) so the content
		// still gets the requested row count (the tab bar only appears with >1 tab).
		let watch = MonitorWatch {
			key: monitor,
			check_at: None,
			moved_at: None,
			ignore_resize_until: Instant::now(),
			ignore_moves_until: Instant::now(),
			layout: Vec::new(),
			size_pinned: cli_win.columns.is_some()
				|| cli_win.rows.is_some()
				|| cli_win.pixel_width.is_some()
				|| cli_win.pixel_height.is_some(),
			font_pinned,
			positionless: window.outer_position().is_err(),
		};
		let (cols, rows) = config::auto::grid(&settings, watch.key.as_deref());
		let cols = cli_win.columns.unwrap_or(cols);
		let rows = cli_win.rows.unwrap_or(rows);
		let menu_bar_h = if menu_bar {
			text.ui_line_h + text.dip(MENU_BAR_VPAD)
		} else {
			0.0
		};
		let n_tabs = if self.cli.hierarchical() {
			self.cli.tabs.len().max(1)
		} else {
			1
		};
		// The strip shows for more than one tab, and for a single one unless the
		// user opts out - State::tab_bar_visible's rule.
		let tab_bar_h = if n_tabs > 1 || !settings.hide_single_tab {
			text.ui_line_h + text.dip(TAB_BAR_VPAD)
		} else {
			0.0
		};
		let (grid_w, grid_h) = window_px(
			cols,
			rows,
			text.cell_w,
			text.cell_h,
			text.margin,
			menu_bar_h + tab_bar_h,
		);
		let (want_w, want_h) = fit_px(
			cli_win.pixel_width.unwrap_or(grid_w),
			cli_win.pixel_height.unwrap_or(grid_h),
			gfx.device.limits().max_texture_dimension_2d,
		);
		let want = winit::dpi::PhysicalSize::new(want_w, want_h);
		let maximize_on_reveal = launch_maximized(&settings, cli_win);
		let mut scrim = scrim;
		// If the resize applies synchronously (Windows, macOS), the first frame is already at
		// the final size - reveal on it. Otherwise (async X11/Wayland) wait for the
		// surface to reach `want` before revealing, so the window never maps at the
		// default size first.
		let reveal_want = if let Some(applied) = request_size(&window, want) {
			gfx.resize(applied.width, applied.height);
			scrim.resize(&gfx.device, applied.width, applied.height);
			None
		} else {
			Some(want)
		};

		// initial content area, inset by the menu bar and the tab strip where each
		// shows, so panes start correctly sized.
		let top = menu_bar_h + tab_bar_h;
		let area = Rect {
			x: 0.0,
			y: top,
			w: gfx.config.width as f32,
			h: (gfx.config.height as f32 - top).max(1.0),
		};
		let list = build_layout(&self.cli, &mut text, &self.proxy, area);
		let frame_budget = crate::profile::FrameBudget::new(Instant::now(), refresh_hz(&window));
		let surface_px = (gfx.config.width, gfx.config.height);
		let gl = gfx.on_glutin_window();
		let adapter_info = gfx.adapter_info.clone();
		let drawn = gfx.drawn;

		self.state = Some(State {
			window,
			gpu: Some(Gpu {
				gfx,
				rects,
				minimap,
				// filled in when the worker answers; the window is not held up for it
				wallpaper_img: None,
				scrim,
			}),
			rebirth: None,
			text,
			proxy: self.proxy.clone(),
			tabs: Tabs { list, active: 0 },
			mods: ModifiersState::empty(),
			mouse: (0.0, 0.0),
			mouse_btn: None,
			mouse_cell: None,
			selecting: None,
			select_edge_held: 0.0,
			last_click: None,
			click_count: 0,
			cursor_focus_sig: None,
			resizing: None,
			dragging_pane: None,
			bar_dragging: None,
			map_dragging: None,
			link_arm: None,
			menu_link: None,
			cursor_icon: CursorIcon::Default,
			clipboard: Clipboard::new(),
			last_frame: Instant::now(),
			rating: crate::profile::Rating::new(),
			frame_budget,
			bench: None,
			bench_at: None,
			bench_cap: None,
			bench_banner: None,
			bench_id,
			bench_kept: None,
			bench_from: None,
			bench_stalled: false,
			dirty: true,
			bell_flash: 0.0,
			size_tracked: false,
			revealed: false,
			shell_scan_at: None,
			reveal_want,
			reveal_deadline: Instant::now() + Duration::from_millis(400),
			pending_size: None,
			pending_size_at: Instant::now(),
			maximize_on_reveal,
			watch,
			menu: None,
			tab_close_arm: None,
			tab_edit: None,
			tab_dbl: None,
			tab_opener: None,
			tab_hover: crate::tip::Dwell::default(),
			menu_tip: crate::tip::Dwell::default(),
			menu_tip_up: None,
			tab_first: 0,
			tab_layout: TabLayout::default(),
			tab_labels: TabLabels::default(),
			tab_followed: 0,
			tab_tip: None,
			decorated,
			menu_bar,
			bare: false,
			bare_saved: (false, false),
			bar_open: None,
			quit: false,
			win_opacity,
			win_title,
			last_win_title: String::new(),
			focused: true,
			pending_about: false,
			pending_settings: None,
			chrome: None,
			chrome_rev: 0,
			text_sig: None,
			overlay_sig: None,
			scrim_sig: None,
			halo_memo: crate::visibility::HaloMemo::default(),
			frame_bufs: FrameBufs::default(),
			occluded: false,
			no_area: false,
			keeps_picture: !(cfg!(windows) && want_transparent),
			was_hidden: false,
			minimized: MinimizedProbe::default(),
			next_frame: None,
			wp_count: 0,
			wp_current: None,
			wp_next: None,
			wp_locked: false,
			cli_style: self.cli.win.style.clone(),
			wp_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
			wp_pacing: crate::wallpaper::Pacing::default(),
			wp_resize_at: None,
			wp_standin: None,
			wp_answered: false,
			wp_shown: false,
			shell_scan_cap: None,
			vram_next: Instant::now() + VRAM_CHECK_IVL,
			vramloss_test: std::env::var_os("SILK_VRAMLOSS").is_some(),
			surface_px,
			gl,
			adapter_info,
			drawn,
			software_step,
			idle: IdleClock::new(),
			frame_retry: Retry::default(),
			conserve: Conserve::Off,
			vt_heal: VtHeal::default(),
		});
		// A wallpaper given on the command line (--wallpaper-file, incl. an explicit
		// clear) owns this session: rotation is skipped entirely, whatever the config
		// says, and the stored rotation settings are left untouched.
		let cli_wallpaper = self.cli.win.style.wallpaper_img.is_some();
		if let Some(state) = self.state.as_mut() {
			state.init_wallpaper(cli_wallpaper);
		}
		// GL path only: the native path's swapchain reports loss itself
		if !self.vt_watch && self.state.as_ref().is_some_and(|s| s.gl) {
			self.vt_watch = spawn_vt_watch(self.proxy.clone());
		}
	}

	fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
		let _t = crate::perf::Span::new(&crate::perf::EVENT_NS);
		let Some(state) = self.state.as_mut() else {
			return;
		};
		match event {
			UserEvent::WallpaperReady(loaded) => state.wallpaper_ready(*loaded),
			UserEvent::ShellsReady(found) => {
				fold_shells(&found);
				if let Some(dialog) = self.dialog.as_mut() {
					dialog.fold_shells(&found);
				}
			}
			UserEvent::Wakeup(id) => {
				crate::perf::bump(&crate::perf::WAKEUPS);
				crate::perf::echoed(id);
				// output easing is triggered in Pane::build when the screen
				// actually scrolls, not on every content change. Only the pane
				// that produced output is marked; a background tab's flag just
				// waits until its tab is shown (the switch forces a rebuild).
				crate::perf::timed(&crate::perf::NOTE_NS, || {
					if let Some(p) = state.tabs.find_pane_mut(id) {
						p.term.wake_handled();
						p.content_dirty = true;
						p.note_output(); // copy-output: push the settle deadline out
						// scrollback depth, sampled per read cycle - the only
						// granularity that sees a `clear` truncate it
						p.note_history();
					}
				});
				state.note_output(); // a shell that prints is not idle, if seen
			}
			UserEvent::PtyWrite(id, bytes) => {
				// a reply the terminal owes the program (cursor position, device
				// attributes), not something the user sent - so read-only does not
				// withhold it, and this is the one direct write left in this file
				if let Some(p) = state.tabs.find_pane(id) {
					p.term.write(bytes);
				}
			}
			UserEvent::Title(id, title) => {
				if let Some(p) = state.tabs.find_pane_mut(id) {
					// The one place a program's own title arrives.
					let said = crate::tabtitle::plain(&title);
					// Any tab's label can carry it now, not just the one in front,
					// so a background pane's title has to reach the strip too. Only
					// a title that actually moved costs a frame: a shell that sets
					// the same one on every prompt is common.
					if p.title != said {
						p.title = said;
						state.dirty = true;
					}
				}
				if id == state.tabs.cur().focused {
					state.update_title();
				}
			}
			UserEvent::ClipboardStore(id, kind, text) => {
				let in_use = id == state.tabs.cur().focused;
				if input::copy_allowed(CopyFrom::Program, state.focused, in_use) {
					match kind {
						ClipboardType::Clipboard => state.clipboard.set_clipboard(text),
						// set_primary falls back to the real clipboard where there is
						// no primary, which a store never asked for
						ClipboardType::Selection if cfg!(target_os = "linux") => {
							state.clipboard.set_primary(text);
						}
						ClipboardType::Selection => {}
					}
				}
			}
			UserEvent::ChildExit(id, status) => {
				state.hold_dead_pane(id, &status);
			}
			UserEvent::Exit(id) => {
				// A shell exited: close just its pane, not the whole app - unless
				// --keep-open asked for the pane to stay and say how it ended.
				if !state.hold_dead_pane(id, "unknown") {
					state.close_dead_pane(id);
				}
				state.dirty = true;
			}
			UserEvent::Bell => {
				// Visual bell: brighten all text, then smoothly fade back (render).
				state.bell_flash = 1.0;
				state.dirty = true;
			}
			UserEvent::SetWallpaper(image) => state.lock_wallpaper(image),
			UserEvent::ReloadSettings => state.reload_config(),
			// a pick from the macOS menu bar, run as the in-window menu runs one
			UserEvent::Menu(action) => {
				// with a dialog or notice up this only brings it forward, as a
				// click in the window does, so a second Settings cannot replace
				// the one open. Copy, Paste and the tab rows are Command+C, V and
				// Shift+[ ] there, so they go to the dialog.
				if let Some(n) = &self.notice {
					n.window.focus_window();
					return;
				}
				if let Some(d) = self.dialog.as_mut() {
					d.window.focus_window();
					let clip = Some(&mut state.clipboard);
					match action {
						MenuAction::Copy => d.menu_edit(EditCmd::Copy, clip),
						MenuAction::Paste => d.menu_edit(EditCmd::Paste, clip),
						MenuAction::PrevTab => d.switch_tab(false),
						MenuAction::NextTab => d.switch_tab(true),
						_ => return,
					}
					self.dialog_dirty = true;
					return;
				}
				state.note_active("menu bar");
				state.menu = None;
				state.bar_open = None;
				let target = state.tabs.cur().focused;
				state.apply_menu(action, target, &self.proxy);
				state.dirty = true;
			}
			UserEvent::VtSwitched => {
				// Return to our console (the watcher signals only returns).
				// Rebuild unconditionally: focus may move to another window or
				// nowhere, and an unfocused window must heal too.
				vramdbg("vt return -> heal_gpu");
				state.heal_gpu(self.dialog.is_some() || self.notice.is_some());
				state.vt_heal.returned(Instant::now());
			}
		}
	}

	fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
		if self.notice.as_ref().is_some_and(|n| n.id() == id) {
			self.handle_notice_event(event);
			return;
		}
		if self.notice_holds(&event) {
			return;
		}
		// route events for a pop-out dialog window to its own handler
		if self.dialog.as_ref().is_some_and(|d| d.id() == id) {
			self.handle_dialog_event(event);
			return;
		}
		// Simulated modality: while a dialog is open the main window takes no
		// input; a click on it re-raises/focuses the dialog instead.
		if let Some(d) = &self.dialog {
			match &event {
				WindowEvent::KeyboardInput { .. }
				| WindowEvent::MouseWheel { .. }
				| WindowEvent::Ime(_) => return,
				WindowEvent::MouseInput {
					state: ElementState::Pressed,
					..
				} => {
					d.window.focus_window();
					return;
				}
				_ => {}
			}
		}
		let Some(state) = self.state.as_mut() else {
			return;
		};
		if restore_sign(&event) {
			state.minimized.forget();
		}
		// The startup benchmark owns the window while its banner is up: anything
		// typed or clicked would change what is being timed. Closing still works.
		if state.bench_banner.is_some()
			&& matches!(
				event,
				WindowEvent::KeyboardInput { .. }
					| WindowEvent::MouseInput { .. }
					| WindowEvent::MouseWheel { .. }
					| WindowEvent::CursorMoved { .. }
					| WindowEvent::Ime(_)
			) {
			return;
		}
		// Anything a person does to the window is a sign of life (see
		// `release_deadline`); the hidden-to-shown edge is one too, in its arm.
		if matches!(
			event,
			WindowEvent::KeyboardInput { .. }
				| WindowEvent::MouseInput { .. }
				| WindowEvent::MouseWheel { .. }
				| WindowEvent::CursorMoved { .. }
				| WindowEvent::CursorEntered { .. }
				| WindowEvent::Touch(_)
				| WindowEvent::Ime(_)
				| WindowEvent::Focused(_)
		) {
			state.note_active("input or focus");
		}
		match event {
			WindowEvent::CloseRequested => event_loop.exit(),

			WindowEvent::Resized(size) => {
				// The surface follows the window, not the event. macOS sends the
				// creation size after the real one is in place (2026100517535929).
				let now = state.window.inner_size();
				state.no_area = now.width == 0 || now.height == 0;
				state.resize_surface(now.width, now.height);
				state.note_wallpaper_size();
				state.relayout_all();
				state.save_window_size(size.width, size.height);
				state.invalidate_prepared(); // scrim textures were just recreated
				state.dirty = true;
			}

			// DPI/scale changed (monitor move or a live scaling change). Re-scale
			// cell metrics + chrome for the new factor; winit preserves the logical
			// size, so a Resized event follows to reconfigure the surface + scrim.
			// That resize is the system's, not the user's, and the new scale may
			// mean another monitor.
			WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
				state.rebuild_text(config::display_scale(scale_factor));
				state.dirty = true;
				let now = Instant::now();
				state.watch.ignore_resize_until = now + OWN_RESIZE_GRACE;
				state.start_settle(now);
			}

			WindowEvent::Moved(_) => state.note_moved(),

			WindowEvent::CursorEntered { .. } if state.watch.positionless => state.note_moved(),

			WindowEvent::ModifiersChanged(mods) => {
				state.mods = mods.state();
				if env_flag(EnvFlag::KeyDbg) {
					eprintln!("[mods] {:?}", mods.state());
				}
				// Alt toggles the menu-bar accelerator underlines, so redraw.
				state.dirty = true;
			}

			// Window focus gates copy-output: a background window never copies.
			WindowEvent::Focused(focused) => {
				if env_flag(EnvFlag::KeyDbg) {
					eprintln!("[focus] {focused}");
				}
				state.focused = focused;
				if !focused {
					state.commit_tab_edit(); // a rename does not outlive the window's focus
				}
				// repaint: focus-dependent chrome (copybox dim) and the refocus
				// poke that resumes a long-idle-parked cursor both live in render
				state.dirty = true;
				// Regaining focus is the likely first moment back from a VT
				// switch/suspend - probe the GPU uploads now, not at the slow tick.
				if focused && state.gl {
					state.vram_next = Instant::now();
					vramdbg("focus regained -> immediate probe");
				}
			}

			// Becoming visible again (VT return, compositor remap) - probe now too;
			// a VT switch doesn't always hand focus straight back.
			WindowEvent::Occluded(occluded) => {
				state.occluded = occluded;
				if !occluded {
					// nothing was drawn while hidden, so catch up in one frame
					state.dirty = true;
					state.note_active("shown");
					if state.gl {
						state.vram_next = Instant::now();
						vramdbg("unoccluded -> immediate probe");
					}
				}
			}

			// OS switched dark/light: a "System" theme follows it live.
			WindowEvent::ThemeChanged(theme) => {
				let dark = !matches!(theme, winit::window::Theme::Light);
				if config::reapply_for_os(dark) {
					state.dirty = true;
				}
			}

			WindowEvent::CursorLeft { .. } => {
				// no pointer, no hover underline and no tab tip
				state.update_link_hover(None);
				state.note_tab_hover(f32::MIN, f32::MIN);
			}

			WindowEvent::CursorMoved { position, .. } => {
				state.mouse = (position.x as f32, position.y as f32);
				let (x, y) = state.mouse;
				// The tab bar is chrome and sits above every pane, so its tip is
				// tracked before anything that could claim the pointer - including a
				// mouse-tracking app, which never sees the bar at all.
				state.note_tab_hover(x, y);
				// A thumb drag in progress owns the pointer - before the mouse-report
				// path, so a tracking app can't swallow the drag half way down.
				if let Some(id) = state.bar_dragging {
					let cfg = config::settings();
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.bar_drag_to(y, &state.text, &cfg);
					}
					state.dirty = true;
					return;
				}
				if let Some(id) = state.map_dragging {
					let cfg = config::settings();
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.map_drag_to(y, &state.text, &cfg);
					}
					state.dirty = true;
					return;
				}
				// mouse-tracking app wants motion/drag reports; when it does, skip our
				// local hover/selection handling for this move. The report is
				// PTY-bound: nothing local changed, so no redraw - marking dirty here
				// forced a full re-shape of every pane per cell crossed.
				if state.report_mouse_motion() {
					// the app owns the pointer, so nothing of ours is hovering it
					state.update_link_hover(None);
					return;
				}
				state.update_bar_hover(x, y);
				state.update_link_hover(Some((x, y)));
				// hovering a different top-level title with a bar menu open
				// switches to it (standard menu-bar behavior)
				if state.bar_open.is_some() && y < state.menu_bar_h() {
					if let Some(i) = state.menubar_hit(x) {
						if state.bar_open != Some(i) {
							state.open_bar_menu(i);
							state.dirty = true;
						}
					}
				}
				if state.menu_hover(x, y) {
					state.dirty = true;
				}
				if state.tab_edit.as_ref().is_some_and(|e| e.dragging) {
					// a press held in a tab being renamed selects as it moves
					if let Some(at) = state.tab_edit_offset(x) {
						state.edit_tab(|edit| edit.drag_to(at));
					}
				} else if let Some(path) = state.resizing.clone() {
					// drag a pane divider
					let area = state.area();
					state
						.tabs
						.cur_mut()
						.drag_divider(&mut state.text, &path, area, x, y);
					state.dirty = true;
				} else if let Some(id) = state.selecting {
					// extend an in-progress drag-selection. Clamped, not bounded: a
					// pointer dragged off the pane keeps selecting to the edge cell,
					// and the per-frame step below scrolls to reveal more.
					if let Some(p) = state.tabs.cur().panes.get(&id) {
						p.drag_selection_to(x, y, &state.text);
					}
					state.dirty = true;
				} else if state.dragging_pane.is_some() {
					// redraw the drop-target highlight as the cursor moves
					state.dirty = true;
				} else {
					// resize cursor over a divider, hand over a link
					state.sync_cursor_icon();
				}
			}

			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button,
				..
			} => {
				// First, since every branch below reads the button: a Ctrl+click
				// on a Mac must reach the rename menu, and must not be reported
				// to an app tracking the mouse, as a right press is not.
				let button = input::acting_button(button, state.mods, cfg!(target_os = "macos"));
				let (x, y) = state.mouse;
				// A rename ends wherever the next click goes, unless it goes back to
				// the tab being renamed (the tab-strip branch below handles a left
				// press there) or to a menu, whose Copy and Paste reach the name.
				if state.tab_edit.is_some() {
					let on_edited = state.tab_bar_visible()
						&& y >= state.menubar_h()
						&& y < state.menubar_h() + state.tab_bar_h()
						&& state.tab_at(x) == state.tab_edit.as_ref().map(|e| e.tab);
					let on_menu = state.menu.as_ref().is_some_and(|m| m.hit_any(x, y))
						|| (state.menu_bar && y < state.menu_bar_h());
					if !on_menu {
						match (on_edited, button) {
							(false, _) => state.commit_tab_edit(),
							(true, MouseButton::Right) => {
								state.open_tab_edit_menu(x, y);
								return;
							}
							// pasted where it was clicked, as an X11 text box does
							(true, MouseButton::Middle) => {
								if let Some(at) = state.tab_edit_offset(x) {
									state.edit_tab(|edit| {
										edit.caret = at;
										edit.anchor = at;
									});
								}
								state.tab_edit_paste_primary();
								return;
							}
							_ => {}
						}
					}
				}
				// A popup tall enough to be clamped to the top of the window covers
				// the menu bar, and the click belongs to whatever is drawn on top -
				// otherwise its first item is unreachable. Same reason the tab-bar
				// branch below stands aside for an open menu.
				let on_popup = state.menu.as_ref().is_some_and(|m| m.hit_any(x, y));
				// click on the menu bar: toggle/open the top-level menu's dropdown
				if button == MouseButton::Left
					&& state.menu_bar
					&& !on_popup && y < state.menu_bar_h()
				{
					// the always-visible copy-mode checkboxes toggle the focused pane
					if let Some(kind) = state.copybox_hit(x) {
						let focused_id = state.tabs.cur().focused;
						state.toggle_copy(focused_id, kind);
						state.menu = None;
						state.bar_open = None;
						state.dirty = true;
						return;
					}
					match (state.menubar_hit(x), state.bar_open) {
						(Some(i), Some(open)) if i == open => {
							state.menu = None;
							state.bar_open = None;
						}
						(Some(i), _) => state.open_bar_menu(i),
						(None, _) => {
							state.menu = None;
							state.bar_open = None;
						}
					}
					state.dirty = true;
					return;
				}
				// click on the tab bar selects a tab
				let tab_bar_y = state.menubar_h();
				if button == MouseButton::Left
					&& input::tab_bar_takes_press(
						state.menu.is_some(),
						state.tab_bar_visible(),
						y,
						tab_bar_y,
						state.tab_bar_h(),
					) {
					if let Some(i) = state.tab_at(x) {
						// press in the close-button column only ARMS the close (the
						// button lights up); the close itself fires on release over
						// the same box, so a slipped press can be dragged off to
						// cancel - standard button feel. Elsewhere selects the tab.
						let bar_h = state.tab_bar_h();
						let on_close = state
							.tab_close_box_at(i, tab_bar_y, bar_h)
							.is_some_and(|cb| x >= cb.x);
						// second click on the same tab, soon enough: rename it
						let now = Instant::now();
						let again = state.tab_dbl.is_some_and(|(when, was)| {
							was == i && now.duration_since(when) < TAB_DBL_CLICK
						});
						state.tab_dbl = Some((now, i));
						let renaming = state.tab_edit.as_ref().is_some_and(|e| e.tab == i);
						if on_close {
							state.tab_close_arm = Some(i);
						} else if renaming {
							// a press in the name places the caret, as in any text box
							let extend = state.mods.shift_key();
							if let Some(at) = state.tab_edit_offset(x) {
								state.edit_tab(|edit| edit.press(at, now, extend));
							}
						} else if again {
							state.begin_tab_edit(i);
						} else {
							if state.tab_edit.as_ref().is_some_and(|e| e.tab != i) {
								state.commit_tab_edit();
							}
							if state.tabs.active != i {
								state.tabs.active = i;
								state.freeze_catchup();
							}
							state.update_title();
						}
						state.dirty = true;
					}
					return;
				}
				// A visible scrollbar takes the click before anything else - including a
				// mouse-tracking app, the same way the right-click menu does. `bar_hit`
				// answers None whenever the bar is faded out, so an invisible bar never
				// steals a click that belonged to the text under it.
				if button == MouseButton::Left && state.menu.is_none() {
					let cfg = config::settings();
					let hit = state.tabs.cur().pane_at(x, y).and_then(|id| {
						let p = state.tabs.cur().panes.get(&id)?;
						Some((id, p.bar_hit(x, y, &state.text, &cfg)?))
					});
					if let Some((id, hit)) = hit {
						state.focus_at(x, y);
						if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
							match hit {
								BarHit::Thumb => p.bar_grab(y, &state.text, &cfg),
								BarHit::TrackUp => p.bar_page(true, &state.text),
								BarHit::TrackDown => p.bar_page(false, &state.text),
							}
						}
						if hit == BarHit::Thumb {
							state.bar_dragging = Some(id);
						}
						state.dirty = true;
						return;
					}
					// The minimap column owns its presses the same way: drag the
					// marker, or press anywhere else in the column to go there.
					let hit = state.tabs.cur().pane_at(x, y).and_then(|id| {
						let p = state.tabs.cur().panes.get(&id)?;
						let g = p.minimap(&state.text, &cfg)?;
						Some((id, crate::minimap::hit(&g, x, y)?))
					});
					if let Some((id, hit)) = hit {
						state.focus_at(x, y);
						if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
							match hit {
								crate::minimap::Hit::Handle => p.map_grab(y, &state.text, &cfg),
								crate::minimap::Hit::Track => p.map_jump(y, &state.text, &cfg),
							}
						}
						if hit == crate::minimap::Hit::Handle {
							state.map_dragging = Some(id);
						}
						state.dirty = true;
						return;
					}
				}
				// mouse-tracking app owns the pointer: report the press, skip local
				// selection/paste/menu (Shift bypasses to the local action). An open
				// menu must get the click (operate/dismiss it), not the app underneath.
				if state.report_mouse_button(button, ElementState::Pressed) {
					state.dirty = true;
					return;
				}
				match button {
					MouseButton::Left => {
						if state.menu.is_some() {
							// click an item to act, a submenu row to open its popup,
							// anywhere else to dismiss
							state.menu_click(x, y, &self.proxy);
							state.dirty = true;
						} else if let Some((path, _)) =
							state
								.tabs
								.cur()
								.divider_at(x, y, state.area(), state.text.scale)
						{
							// grab a divider to resize instead of selecting
							state.resizing = Some(path);
						} else if state.mods.shift_key() {
							// Shift+drag a pane to reorder it
							if let Some(id) = state.tabs.cur().pane_at(x, y) {
								state.focus_at(x, y);
								state.dragging_pane = Some(id);
								state.window.set_cursor(CursorIcon::Grabbing);
								state.cursor_icon = CursorIcon::Grabbing;
							}
						} else if let Some((id, link)) =
							input::shortcut_held(state.mods, cfg!(target_os = "macos"))
								.then(|| state.link_at_pointer())
								.flatten()
						{
							// Ctrl+click a link, Command+click on a Mac: arm here, open
							// on the release over the same link, so a slipped press can
							// be dragged off to cancel. The same key elsewhere still
							// starts a block selection - only a press ON a link is taken.
							state.focus_at(x, y);
							state.link_arm = Some((id, link.url));
						} else {
							state.focus_at(x, y);
							let now = Instant::now();
							state.click_count = input::click_count(
								state.last_click,
								state.click_count,
								now,
								(x, y),
								(state.text.cell_w, state.text.cell_h),
							);
							state.last_click = Some((now, x, y));
							let kind = input::click_select(
								state.click_count,
								input::shortcut_held(state.mods, cfg!(target_os = "macos")),
							);
							let pairs = if kind == ClickSelect::Word {
								config::selection_pairs()
							} else {
								Vec::new()
							};
							let started = state.tabs.cur().pane_at(x, y).and_then(|id| {
								let p = state.tabs.cur().panes.get(&id)?;
								let (point, side) = p.point_at(x, y, &state.text)?;
								if kind == ClickSelect::Line {
									// whole logical line, spanning wrapped continuation rows
									let (start, end) = p.line_span(point);
									p.begin_selection(start, Side::Left, SelectionType::Simple);
									p.update_selection(end, Side::Right);
								} else if kind == ClickSelect::Word {
									// a shape we can name (URL, path) wins; else the
									// contents of a matched pair; else a bracket to
									// its partner; else the word
									match p
										.shape_span(point)
										.or_else(|| p.pair_span(point, &pairs))
										.or_else(|| p.bracket_span(point))
									{
										Some((start, end)) => {
											p.begin_selection(
												start,
												Side::Left,
												SelectionType::Simple,
											);
											p.update_selection(end, Side::Right);
										}
										None => {
											p.begin_selection(point, side, SelectionType::Semantic);
										}
									}
								} else {
									let sel_type = if kind == ClickSelect::Block {
										SelectionType::Block
									} else {
										SelectionType::Simple
									};
									p.begin_selection(point, side, sel_type);
								}
								Some(id)
							});
							if started.is_some() {
								state.selecting = started;
								state.dirty = true;
							}
						}
					}
					MouseButton::Middle => {
						// paste the primary selection into the pane under the cursor
						if let Some(text) = state.clipboard.get_primary() {
							let id = state
								.tabs
								.cur()
								.pane_at(x, y)
								.unwrap_or(state.tabs.cur().focused);
							if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
								p.paste(&text);
							}
						}
					}
					MouseButton::Right => {
						if let Some(id) = state.tabs.cur().pane_at(x, y) {
							state.open_menu(id, x, y);
							state.dirty = true;
						}
					}
					_ => {}
				}
			}

			// a mouse-tracking app owns the pointer: report the release we opened.
			// Only for the SAME button as the reported press - releasing a different
			// one must not clear the held state (the app would see an unbalanced
			// press) nor steal that button's local release handling below.
			WindowEvent::MouseInput {
				state: ElementState::Released,
				button,
				..
			} if input::release_is_reported(state.mouse_btn, button) => {
				if state.report_mouse_button(button, ElementState::Released) {
					state.dirty = true;
				}
			}

			WindowEvent::MouseInput {
				state: ElementState::Released,
				button: MouseButton::Left,
				..
			} => {
				state.resizing = None;
				if let Some(edit) = state.tab_edit.as_mut() {
					edit.dragging = false;
				}
				// armed link: open only if the release is still on the same one
				if let Some((armed_id, url)) = state.link_arm.take() {
					let same = state
						.link_at_pointer()
						.is_some_and(|(id, link)| id == armed_id && link.url == url);
					if same {
						open_link(&url);
					}
				}
				if let Some(id) = state.map_dragging.take() {
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.release_handle();
					}
					state.dirty = true;
					return;
				}
				// end a thumb drag; the hold keeps the bar up for a moment afterwards
				if let Some(id) = state.bar_dragging.take() {
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
						p.release_handle();
					}
					state.dirty = true;
				}
				if let Some(i) = state.tab_close_arm.take() {
					let (x, y) = state.mouse;
					let tab_bar_y = state.menubar_h();
					let bar_h = state.tab_bar_h();
					let in_bar = y >= tab_bar_y && y < tab_bar_y + bar_h;
					let close_x = (in_bar && i < state.tabs.len())
						.then(|| state.tab_close_box_at(i, tab_bar_y, bar_h))
						.flatten()
						.map(|cb| cb.x);
					if input::close_on_release(i, x, in_bar, close_x, state.tab_at(x)) {
						state.close_tab_at(i);
					}
					state.dirty = true;
				}
				// drop a dragged pane onto the pane under the cursor (swap)
				if let Some(src) = state.dragging_pane.take() {
					let (x, y) = state.mouse;
					let area = state.area();
					if let Some(target_id) = state.tabs.cur().pane_at(x, y) {
						state
							.tabs
							.cur_mut()
							.swap_panes(&mut state.text, src, target_id, area);
					}
					state.window.set_cursor(CursorIcon::Default);
					state.cursor_icon = CursorIcon::Default;
					state.dirty = true;
				}
				// finish a drag-select: copy to primary, or clear if it was a click
				if let Some(id) = state.selecting.take() {
					let text = state.tabs.cur().panes.get(&id).and_then(|p| {
						let sel_text = p.selection_text();
						if sel_text.is_none() {
							p.clear_selection();
						}
						sel_text
					});
					match text {
						Some(sel_text) => {
							// copy-on-select: a finished selection also goes to the
							// desktop clipboard when the pane opted in
							let in_use = id == state.tabs.cur().focused;
							if input::copy_allowed(CopyFrom::Select, state.focused, in_use)
								&& state
									.tabs
									.cur()
									.panes
									.get(&id)
									.is_some_and(|p| p.copy_select)
							{
								state.clipboard.set_clipboard(sel_text.clone());
							}
							state.clipboard.set_primary(sel_text);
						}
						None => state.dirty = true,
					}
				}
			}

			WindowEvent::MouseWheel { delta, .. } => {
				let (x, y) = state.mouse;
				// The tab bar takes the wheel first, and turns its own page. With
				// more tabs than fit, this is the only way to reach one past the
				// edge with the mouse alone.
				let bar_y = state.menubar_h();
				if state.tab_bar_visible() && y >= bar_y && y < bar_y + state.tab_bar_h() {
					let up = match delta {
						MouseScrollDelta::LineDelta(_, dy) => dy > 0.0,
						MouseScrollDelta::PixelDelta(pos) => pos.y > 0.0,
					};
					state.scroll_tab_strip(if up { 1.0 } else { -1.0 });
					return;
				}
				let id = state
					.tabs
					.cur()
					.pane_at(x, y)
					.unwrap_or(state.tabs.cur().focused);
				let cell_h = state.text.cell_h;
				// A mouse-tracking app (muffer, tmux, vim with mouse on, ...) wants
				// the wheel as button 64/65 reports, not our scrollback. Report one
				// notch per line, then stop here.
				let shift = state.mods.shift_key();
				let (up, notches) = match delta {
					MouseScrollDelta::LineDelta(_, y) => (y > 0.0, (y.abs().round() as u32).max(1)),
					MouseScrollDelta::PixelDelta(pos) => (
						(pos.y as f32) > 0.0,
						((pos.y.abs() as f32 / cell_h).round() as u32).max(1),
					),
				};
				if let Some(p) = state.tabs.cur().panes.get(&id) {
					// No cell under the pointer means it is over the minimap
					// column, not the text - fall through and scroll the buffer.
					if let Some((col, row)) = (input::wheel_route(p.mode, shift)
						== WheelRoute::Report)
						.then(|| p.screen_cell_at(x, y, &state.text))
						.flatten()
					{
						let btn = if up {
							input::MouseBtn::WheelUp
						} else {
							input::MouseBtn::WheelDown
						};
						for _ in 0..notches.min(8) {
							if let Some(seq) =
								input::mouse_report(p.mode, btn, true, false, col, row, state.mods)
							{
								p.write_input(seq);
							}
						}
						state.dirty = true;
						return;
					}
				}
				// smooth scrollback uses WHEEL_LINES; full-screen apps get their
				// own (tunable) lines-per-notch via ALT_SCROLL_LINES
				let (lines, alt_lines) = match delta {
					MouseScrollDelta::LineDelta(_, y) => (
						y * config::settings().wheel_lines,
						y * config::settings().alt_scroll_lines,
					),
					MouseScrollDelta::PixelDelta(pos) => {
						let lines = pos.y as f32 / cell_h;
						(lines, lines)
					}
				};
				if let Some(p) = state.tabs.cur_mut().panes.get_mut(&id) {
					let mode = p.mode;
					if input::wheel_route(mode, shift) == WheelRoute::CursorKeys {
						// full-screen apps (less, nano, ...) have no scrollback of
						// their own; the wheel drives their cursor-key scrolling
						let n = alt_lines.abs().round() as i32;
						if n > 0 {
							let letter = if alt_lines > 0.0 { b'A' } else { b'B' };
							let seq =
								input::cursor_seq(letter, mode.contains(TermMode::APP_CURSOR));
							let mut bytes = Vec::with_capacity(seq.len() * n as usize);
							for _ in 0..n {
								bytes.extend_from_slice(&seq);
							}
							p.write_input(bytes);
						}
					} else {
						p.scroll.wheel(lines);
						// user-driven scroll, so the bar comes up; output-driven
						// scrolling deliberately doesn't (it never stops)
						p.poke_scrollbar();
					}
				}
				state.dirty = true;
			}

			WindowEvent::KeyboardInput {
				event: key,
				is_synthetic,
				..
			} => {
				let key_at = crate::perf::key_mark();
				if env_flag(EnvFlag::KeyDbg) {
					eprintln!(
						"[key] {:?} {:?} synthetic={is_synthetic} focused={} mods=[{}{}{}]",
						key.logical_key,
						key.state,
						state.focused,
						if state.mods.control_key() { "C" } else { "" },
						if state.mods.alt_key() { "A" } else { "" },
						if state.mods.shift_key() { "S" } else { "" },
					);
				}
				// A replayed press is focus bookkeeping, and a release is the
				// shell's business, not ours - neither is typing.
				if !key_is_typed(key.state, is_synthetic) {
					return;
				}
				// A character handed to the window instead of typed at it arrives
				// with no key named; fill the key in so the rest of this reads it.
				let key = input::name_typed(key);
				// see IGNORE_KEYS_WHILE_UNFOCUSED. A copy types nothing, so it cannot be
				// the bare arrow that gate is for, and a flag lagging the WM must not
				// eat it while right-click Copy works.
				if IGNORE_KEYS_WHILE_UNFOCUSED && !state.focused {
					if state.menu.is_none()
						&& is_copy_chord(&config::settings().keys, state.mods, &key.logical_key)
					{
						state.copy_selection();
					}
					return;
				}
				// An open menu (context menu / menu-bar dropdown) captures the
				// navigation keys - they drive the menu, not the terminal pane.
				if state.menu.is_some() {
					// Every one of these drives the INNERMOST open popup - with a
					// submenu standing open, that is the submenu.
					match &key.logical_key {
						Key::Named(NamedKey::Escape) => {
							// back out one level at a time, the way a submenu is entered
							if !state.close_submenu() {
								state.menu = None;
								state.bar_open = None;
							}
						}
						Key::Named(NamedKey::ArrowDown) => {
							if let Some(menu) = state.menu_inner() {
								menu.hover = menu.step(menu.hover, 1);
							}
						}
						Key::Named(NamedKey::ArrowUp) => {
							if let Some(menu) = state.menu_inner() {
								menu.hover = menu.step(menu.hover, -1);
							}
						}
						// Right enters a submenu and Left leaves one; where there is
						// no submenu in the way they cycle between menu-bar dropdowns
						// (a no-op for a right-click context menu, which isn't
						// bar-anchored)
						Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
							let left = matches!(key.logical_key, Key::Named(NamedKey::ArrowLeft));
							let enter = (!left).then(|| state.submenu_row()).flatten();
							if let Some(row) = enter {
								state.menu_activate(row, &self.proxy);
							} else if !(left && state.close_submenu()) {
								if let Some(open_idx) = state.bar_open {
									let n = MENU_BAR.len();
									let next = if left {
										(open_idx + n - 1) % n
									} else {
										(open_idx + 1) % n
									};
									state.open_bar_menu(next);
								}
							}
						}
						Key::Named(NamedKey::Enter) => {
							if let Some(row) = state.menu_inner().and_then(|menu| menu.hover) {
								state.menu_activate(row, &self.proxy);
							}
						}
						// accelerator: a letter activates the item carrying it (the
						// underlined letter; unique per menu, some items have none)
						Key::Character(typed) => {
							let ch = typed.chars().next().map(|c| c.to_ascii_lowercase());
							let hit = ch.and_then(|ch| {
								state.menu.as_ref().and_then(|menu| {
									let chain = menu.chain();
									chain[chain.len() - 1].entries.iter().position(|entry| {
										entry_accel(entry).is_some_and(|(label, pos)| {
											label[pos..]
												.chars()
												.next()
												.map(|c| c.to_ascii_lowercase()) == Some(ch)
										})
									})
								})
							});
							if let Some(row) = hit {
								state.menu_activate(row, &self.proxy);
							}
						}
						_ => {}
					}
					state.dirty = true;
					return;
				}
				// A tab rename takes every key while it is up: the tab strip is the
				// only thing on screen accepting typing, so nothing reaches the shell.
				if state.tab_edit.is_some() {
					state.tab_edit_key(&key.logical_key);
					state.dirty = true;
					return;
				}
				match input::hotkey_for(&key.logical_key, state.mods, state.menu_bar) {
					Some(Hotkey::Settings) => {
						state.open_settings();
						return;
					}
					Some(Hotkey::Fullscreen) => {
						state.toggle_fullscreen();
						return;
					}
					// the Menu/Apps key opens the context menu on the focused pane
					Some(Hotkey::ContextMenu) => {
						let id = state.tabs.cur().focused;
						if let Some(p) = state.tabs.cur().panes.get(&id) {
							let (rect_x, rect_y) = (p.rect.x, p.rect.y);
							state.open_menu(id, rect_x + 12.0, rect_y + 12.0);
							state.dirty = true;
						}
						return;
					}
					// a letter no title starts with goes on to the shell
					Some(Hotkey::MenuTitle(ch)) => {
						if let Some(i) = bar_menu_for(ch) {
							state.open_bar_menu(i);
							state.dirty = true;
							return;
						}
					}
					Some(Hotkey::NewTab) => {
						state.new_tab(&self.proxy);
						return;
					}
					// the current tab, or the window if it is the last one
					Some(Hotkey::CloseTab) => {
						state.close_tab();
						return;
					}
					Some(Hotkey::NewWindow) => {
						state.new_window();
						return;
					}
					Some(Hotkey::Zoom(dir)) => {
						state.font_zoom(dir);
						return;
					}
					Some(Hotkey::ZoomReset) => {
						state.font_zoom_reset();
						return;
					}
					Some(hotkey @ (Hotkey::PrevTab | Hotkey::NextTab)) => {
						state.step_tab(hotkey == Hotkey::NextTab);
						return;
					}
					Some(Hotkey::MoveTab { forward }) => {
						state.tabs.move_active(forward); // same tab follows - nothing was frozen
						state.update_title();
						state.dirty = true;
						return;
					}
					Some(Hotkey::Copy) => {
						state.copy_selection();
						state.dirty = true;
						return;
					}
					Some(Hotkey::Paste) => {
						let focused = state.tabs.cur().focused;
						if let Some(text) = state.clipboard.get_clipboard() {
							if let Some(p) = state.tabs.cur_mut().panes.get_mut(&focused) {
								p.paste(&text);
							}
						}
						state.dirty = true;
						return;
					}
					Some(Hotkey::Quit) => {
						state.quit = true;
						return;
					}
					// the same as the Panes menu's rows, on the focused pane
					Some(hotkey @ (Hotkey::SplitRight | Hotkey::SplitDown | Hotkey::ClosePane)) => {
						let action = match hotkey {
							Hotkey::SplitRight => MenuAction::SplitVertical,
							Hotkey::SplitDown => MenuAction::SplitHorizontal,
							_ => MenuAction::Close,
						};
						let focused = state.tabs.cur().focused;
						state.apply_menu(action, focused, &self.proxy);
						state.dirty = true;
						return;
					}
					Some(Hotkey::Focus(toward)) => {
						if state.tabs.cur_mut().move_focus(toward) {
							state.update_title();
							state.dirty = true;
						}
						return;
					}
					None => {}
				}
				if !input::reaches_shell(state.mods, cfg!(target_os = "macos")) {
					return;
				}
				let focused = state.tabs.cur().focused;
				let app_cursor = state
					.tabs
					.cur()
					.panes
					.get(&focused)
					.is_some_and(|p| p.mode.contains(TermMode::APP_CURSOR));
				if let Some(bytes) = input::encode(&key, state.mods, app_cursor) {
					// a held pane has no shell left to type at, so any key that
					// would have gone to one closes it instead
					if state.tabs.cur().panes.get(&focused).is_some_and(|p| p.held) {
						state.close_dead_pane(focused);
						return;
					}
					// copy-output: Enter at the shell prompt may launch a command;
					// arm the capture so its output is copied once the pane settles.
					let is_enter = matches!(key.logical_key, Key::Named(NamedKey::Enter));
					if let Some(p) = state.tabs.cur_mut().panes.get_mut(&focused) {
						if !p.read_only {
							p.scroll.jump_bottom();
							p.write_input(bytes);
							crate::perf::typed(key_at, focused);
							p.note_typed();
							if is_enter && p.copy_output {
								p.arm_capture();
							}
						}
					}
					state.dirty = true;
				}
			}

			WindowEvent::RedrawRequested => {
				if state.freeze_sync() {
					return; // frozen - nothing on screen to paint
				}
				// The desktop wants pixels from a window that let its device go:
				// the rebuild is done where every other wake is (about_to_wait),
				// and the frame with it.
				if state.gpu.is_none() {
					state.note_active("redraw request");
					return;
				}
				let _ = state.render(true);
			}

			_ => {}
		}
	}

	fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
		// don't lose a resize done just before quitting
		if let Some(state) = self.state.as_mut() {
			state.flush_window_size(true);
		}
		// a warm-up still building would be in the loader as the process tears it down
		self.gpu_warm.release();
		crate::perf::report();
	}

	// request_redraw isn't reliable under some compositors, so we drive frames
	// here: render when something changed or an animation is in flight, and
	// poll only while animating (otherwise sleep until the next event).
	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		crate::perf::bump(&crate::perf::PASSES);
		let _t = crate::perf::Span::new(&crate::perf::PASS_NS);
		// One place to act on `quit`, so every path that sets it exits - menus,
		// hotkeys, and the tab-close box all reach here on the next pass.
		if self.state.as_ref().is_some_and(|state| state.quit) {
			event_loop.exit();
			return;
		}
		#[cfg(target_os = "macos")]
		if let Some(state) = self.state.as_ref() {
			crate::macmenu::refresh(
				state.bar_menus_key(),
				|| crate::macmenu::layout(state.bar_menus(), &config::settings().keys),
				&self.proxy,
			);
		}
		// cicd profiler: in profile mode run for SILK_PROFILE_SECS then exit, so
		// main can dump the flamegraph (the workload runs in the startup pane).
		#[cfg(feature = "profiling")]
		if std::env::var_os("SILK_PROFILE_OUT").is_some() {
			let now = std::time::Instant::now();
			let deadline = *self
				.profile_deadline
				.get_or_insert_with(|| now + std::time::Duration::from_secs(self.profile_secs));
			if now >= deadline {
				event_loop.exit();
				return;
			}
		}

		// Warm the dialogs' GPU context once the terminal is genuinely on screen,
		// so building it can't slow the path to the first frame. Idempotent.
		if WARM_DIALOG_GPU
			&& self
				.state
				.as_ref()
				.is_some_and(|state| state.revealed && state.gpu.is_some())
		{
			self.gpu_warm.start();
		}

		if env_flag(EnvFlag::MemDbg) && Instant::now() >= self.memdbg_next {
			self.memdbg_next = Instant::now() + MEMDBG_IVL;
			self.memdbg_report();
		}

		// Look for installed shells, once, a little after the window is genuinely
		// on screen. A PATH scan stats every directory the user has on it and the
		// Windows side reads the registry, so it runs on its own thread and comes
		// back as UserEvent::ShellsReady.
		if let Some(state) = self.state.as_mut() {
			if state.shell_scan_at.is_some_and(|at| Instant::now() >= at) {
				state.shell_scan_at = None;
				crate::shells::spawn(&self.proxy);
			}
		}

		// Raise a rested pointer's tab tip, and keep an open one's clock ticking.
		if let Some(state) = self.state.as_mut() {
			if state.update_tips() {
				state.dirty = true;
			}
		}

		// Open the About window if requested (window creation needs the event loop,
		// so State only signals and we act here).
		let open_about = self
			.state
			.as_mut()
			.is_some_and(|state| std::mem::take(&mut state.pending_about));
		// parent handle so the WM ties the dialog to the terminal window
		// (transient-for / owner)
		let parent = self.state.as_ref().and_then(|state| {
			use winit::raw_window_handle::HasWindowHandle;
			state
				.window
				.window_handle()
				.ok()
				.map(|handle| handle.as_raw())
		});
		// cloned (all wgpu handles, so refcount bumps) rather than borrowed: the
		// open arms below also take `&mut self` to store the dialog
		let warm = (open_about
			|| self
				.state
				.as_ref()
				.is_some_and(|s| s.pending_settings.is_some()))
		.then(|| self.gpu_warm.get())
		.flatten();
		if open_about {
			if let Some(info) = self.state.as_ref().map(|state| state.adapter_info.clone()) {
				match crate::dialog::DialogWin::new_about(event_loop, &info, parent, warm.as_ref())
				{
					Ok(d) => {
						self.dialog = Some(d);
						self.center_dialog();
						self.reveal_dialog();
						self.dialog_dirty = true;
					}
					Err(e) => eprintln!("{}: About window failed: {e}", config::APP_NAME),
				}
			}
		}
		let settings_base = self.state.as_mut().and_then(|state| {
			let asked = state.pending_settings.take()?;
			Some((asked, state.settings_for_dialog()))
		});
		if let Some((asked, base)) = settings_base {
			// a view older than the resume window is dead either way, so take it
			// unconditionally and discard it if it has expired
			let resume = self
				.settings_view
				.take()
				.filter(|(closed, _)| closed.elapsed() <= SETTINGS_RESUME)
				.map(|(_, view)| view);
			let sized = self.settings_size;
			match crate::dialog::DialogWin::new_settings(
				event_loop,
				parent,
				resume,
				sized,
				warm.as_ref(),
				base,
			) {
				Ok(d) => {
					self.dialog = Some(d);
					self.center_dialog();
					self.reveal_dialog();
					self.dialog_dirty = true;
					self.settings_asked = Some((asked, warm.is_some()));
				}
				Err(e) => eprintln!("{}: Settings window failed: {e}", config::APP_NAME),
			}
		}
		// a dialog with an animating field edit (view scroll / caret / blink) or a
		// tip coming due gets its frame when it is owed one, not on every pass,
		// or each pointer move would draw it again (see dlg_wake below)
		if self
			.dialog
			.as_ref()
			.is_some_and(|d| d.owes_frame(Instant::now()))
		{
			self.dialog_dirty = true;
		}
		if self
			.dialog
			.as_mut()
			.is_some_and(|d| d.refused.take_due(Instant::now()))
		{
			self.dialog_dirty = true;
		}
		if self.dialog_dirty {
			if let Some(d) = &mut self.dialog {
				d.render();
			}
			self.dialog_dirty = false;
			if let Some((asked, warm)) = self.settings_asked.take()
				&& env_flag(EnvFlag::DlgDbg)
			{
				eprintln!(
					"[dlg] Settings drawn {:.1} ms after it was asked for, on the {} context",
					asked.elapsed().as_secs_f64() * 1e3,
					if warm { "warm" } else { "cold" }
				);
			}
		}
		// A save that could not be written. Asked for from Settings, it was
		// taken there already (apply_dialog_settings); anything here is one of
		// the saves nobody asked for.
		if let Some(refusal) = config::take_refusal() {
			if notice_due(&mut self.told, &refusal.path, false) {
				self.notice_owed = Some(refusal);
			}
		}
		// A write converted the file and lost settings doing it: the launch's, or a
		// save that converted a file the launch left alone. Said once the terminal
		// is on screen, since nothing may hold up the first frame.
		if self.state.as_ref().is_some_and(|state| state.revealed) {
			if let Some(loss) = config::take_conversion_loss() {
				self.loss_owed = Some(loss);
			}
		}
		if self.notice_owed.is_some() || self.loss_owed.is_some() {
			self.show_notice(event_loop);
		}
		if self
			.notice
			.as_mut()
			.is_some_and(|n| n.refused.take_due(Instant::now()))
		{
			self.notice_dirty = true;
		}
		if self.notice_dirty {
			if let Some(n) = &mut self.notice {
				n.render();
			}
			self.notice_dirty = false;
		}
		let dlg_wake = self
			.dialog
			.as_ref()
			.and_then(super::dialog::DialogWin::wake_at);

		// re-assert the dialog->terminal stacking a few times after focus (see the
		// field comment). Cleared when the dialog closes.
		if self.dialog.is_none() {
			self.raise_reassert = 0;
		} else if self.raise_reassert > 0 && Instant::now() >= self.raise_next {
			if let Some(d) = &self.dialog {
				d.raise_parent();
			}
			self.raise_reassert -= 1;
			self.raise_next = Instant::now() + RAISE_REASSERT_IVL;
		}
		let raise_wake = (self.raise_reassert > 0).then_some(self.raise_next);

		let Some(state) = self.state.as_mut() else {
			return;
		};
		let scroll_anim = state
			.tabs
			.cur()
			.panes
			.values()
			.any(|p| p.scroll.animating());
		let cursor_anim = state.tabs.cur().panes.values().any(|p| p.cursor_animating);
		let content = state.tabs.cur().panes.values().any(|p| p.content_dirty);
		// Parked-cursor resume: no frames flow while a cursor is parked at full,
		// so consume any due wake (render one catch-up frame - the pause state
		// sees the timeouts met and resumes the cycle) and keep the earliest
		// pending one to fold into the control flow below.
		let mut cursor_wake: Option<Instant> = None;
		let wake_now = Instant::now();
		for pane in state.tabs.cur_mut().panes.values_mut() {
			if let Some(wake) = pane.cursor_wake {
				if wake_now >= wake {
					pane.cursor_wake = None; // consumed - or an occluded window would spin on it
					state.dirty = true;
				} else {
					cursor_wake = Some(cursor_wake.map_or(wake, |w| w.min(wake)));
				}
			}
			// a minimap compose the throttle deferred: due now, or wake for it
			if let Some(wake) = pane.map_wake() {
				if wake_now >= wake {
					state.dirty = true;
				} else {
					cursor_wake = Some(cursor_wake.map_or(wake, |w| w.min(wake)));
				}
			}
		}
		// copy-output: catch the focused pane's command finishing (see method)
		state.poll_output_copy();
		// wallpaper rotation: swap to the next image when its interval elapses
		// (sets state.dirty so the change renders this cycle)
		if state.wp_next.is_some_and(|next| Instant::now() >= next) && state.gpu.is_some() {
			state.advance_wallpaper();
		}
		if state.wp_resize_at.is_some_and(|at| Instant::now() >= at) {
			state.resize_wallpaper();
		}
		if state.vt_heal.due(Instant::now()) {
			vramdbg("vt return, second pass -> heal_gpu");
			state.heal_gpu(self.dialog.is_some() || self.notice.is_some());
		}
		// "resources restored" comes down on its own after a few seconds
		if state.conserve.wake().is_some_and(|at| Instant::now() >= at) {
			state.conserve = Conserve::Off;
			state.update_title();
		}
		// GL path: the VT watcher (spawn_vt_watch) is the real loss trigger; the
		// readback probes below stay as field evidence + a fallback for a missed
		// switch, since a real purge read back "intact" (driver restores readback
		// contents while sampled copies stay garbage).
		if let Some(gpu) = state.gpu.as_mut().filter(|gpu| gpu.gfx.is_gl()) {
			if state.vramloss_test && state.revealed {
				state.vramloss_test = false;
				gpu.gfx.vram_clobber();
				if let Some(wp) = &gpu.wallpaper_img {
					wp.vram_clobber(&gpu.gfx.queue);
				}
				vramdbg("SILK_VRAMLOSS: sentinels + wallpaper clobbered");
			}
			// Poll both probes; either detecting loss triggers the one rebuild.
			// Field logs showed EVERY witness - synthetic sentinels AND the
			// wallpaper's own uploaded block - reading back intact across a real
			// purge that blacked the window, so readback cannot be the primary
			// detector. Kept for the diagnostic trail and as a fallback.
			let mut lost = None;
			match gpu.gfx.vram_check_poll() {
				Some(VramProbe::Lost { uploaded, rendered }) => {
					lost = Some(format!(
						"sentinel uploaded={} rendered={}",
						if uploaded { "gone" } else { "ok" },
						if rendered { "gone" } else { "ok" }
					));
				}
				// not logged: every window every two seconds filled the log's
				// cap within a day, and it then missed the switches it was for
				Some(VramProbe::Intact) | None => {}
				Some(VramProbe::MapFailed) => {
					vramdbg("probe: sentinel readback map FAILED (inconclusive)");
				}
			}
			if let Some(wp) = gpu.wallpaper_img.as_mut() {
				match wp.vram_check_poll(&gpu.gfx.device) {
					Some(WpProbe::Lost) => lost = Some("wallpaper block gone".into()),
					Some(WpProbe::Intact) | None => {}
					Some(WpProbe::MapFailed) => {
						vramdbg("probe: wallpaper readback map FAILED (inconclusive)");
					}
				}
			}
			if Instant::now() >= state.vram_next {
				gpu.gfx.vram_check_start();
				if let Some(wp) = gpu.wallpaper_img.as_mut() {
					wp.vram_check_start(&gpu.gfx.device, &gpu.gfx.queue);
				}
				state.vram_next = Instant::now() + VRAM_CHECK_IVL;
			}
			if let Some(what) = lost {
				eprintln!(
					"{}: GPU texture contents lost (VT switch or resume?) - rebuilding",
					config::APP_NAME
				);
				vramdbg(&format!("probe: LOST ({what}) -> recover_gpu"));
				state.recover_gpu();
			}
		}
		let bell_anim = state.bell_flash > 0.0;
		// Until revealed (born hidden, shown at final size), keep rendering so the
		// reveal check runs each cycle; the deadline wake below guarantees it can't
		// stay hidden if no post-resize frame is otherwise triggered.
		if !state.revealed {
			state.dirty = true;
		}
		// The startup benchmark: due, so start it on the heaviest rung. It waits
		// for the wallpaper whatever the delay says, since the wallpaper is part
		// of what is being timed. A pinned frame rate would time itself rather
		// than the machine, so it cancels the run outright (the first pick stands).
		if state.bench_at.is_some_and(|at| Instant::now() >= at) && !state.bench_blocked() {
			state.bench_at = None;
			state.bench_cap = None;
			if max_fps().is_none() {
				state.bench_from = Some(crate::profile::current(&config::settings()));
				state.bench = Some(crate::profile::Bench::new());
				let rung = state
					.bench
					.as_ref()
					.map_or(crate::profile::Profile::Max, crate::profile::Bench::profile);
				state.set_live_profile(rung);
			} else {
				// nothing will be measured, so the banner has nothing to cover
				state.bench_banner = None;
				state.dirty = true;
			}
		}
		// A run renders flat out: the periods between frames ARE the measurement.
		if state.bench.is_some() {
			state.dirty = true;
		}
		if state
			.bench_banner_wake()
			.is_some_and(|wake| Instant::now() >= wake)
		{
			state.bench_banner = None;
			state.bench_kept = None;
			state.bench_stalled = false;
			state.dirty = true;
		}
		// Fully hidden window: don't build a frame nobody can see. PTY reading
		// never stops, so the grid keeps up and the reveal is one catch-up frame.
		let hidden = state.freeze_sync();
		// A window that let its device go takes it back the moment it is wanted
		// on screen, and not before: output into a hidden one waits for the
		// reveal. One left alone for long enough lets it go, unless a dialog is
		// up (on X11 the dialog's context cannot outlive the terminal's).
		let dialog_up = self.dialog.is_some() || self.notice.is_some();
		// Software rendering turned on or off: the device goes and comes back
		// on what the setting now asks, the dialogs' context with it. Not while
		// a dialog is up, since it draws on that context.
		if !dialog_up
			&& state
				.gpu
				.as_ref()
				.is_some_and(|gpu| gpu.gfx.want != crate::gfx::wanted())
		{
			idledbg("software rendering setting changed");
			state.release_gpu();
			self.gpu_warm.release();
			state.idle.owe(true);
		}
		if state.gpu.is_none() {
			if state.idle.rebuild_due(hidden, Instant::now()) {
				state.rebuild_gpu();
			}
		} else if !dialog_up
			&& state
				.release_deadline(&config::settings())
				.is_some_and(|due| Instant::now() >= due)
		{
			state.release_gpu();
			// the dialogs' warm context is a second device; it comes back
			// with the first (see the warm-up at the top of this pass)
			self.gpu_warm.release();
		}
		if !hidden && state.gpu.is_some() && state.frame_retry.take_due(Instant::now()) {
			state.dirty = true;
		}
		// A pass that draws nothing pauses the watch too, or the next ease's first
		// period would be the whole idle gap before it.
		let flow = if hidden || state.gpu.is_none() {
			state.rating.pause();
			ControlFlow::Wait
		} else if state.dirty || content || scroll_anim || cursor_anim || bell_anim {
			// UI/chrome changes and the bell force ALL panes to re-shape; fresh
			// output and scroll eases are scoped per pane inside render (a pure
			// cursor-animation frame lets every pane reuse its cached frame).
			let force = state.dirty || bell_anim;
			state.dirty = false;
			crate::perf::bump(&crate::perf::FRAMES);
			let allocs = crate::perf::thread_allocs();
			let animating = crate::perf::timed(&crate::perf::RENDER_NS, || state.render(force));
			crate::perf::frame_allocs(allocs);
			// how the ease is paced tells whether the display keeps up
			let step = rating_step(
				state.bench.is_some(),
				scroll_anim,
				max_fps().is_some(),
				state.focused,
			);
			if state.bench.is_some() {
				// a run is timing the rungs itself; the step-down would be reading
				// the same frames and moving the profile out from under it
				let budget = state
					.frame_budget
					.at(Instant::now(), || refresh_hz(&state.window));
				match state.bench.as_mut().map(|b| b.note(Instant::now(), budget)) {
					Some(crate::profile::Step::Rung(next)) => state.set_live_profile(next),
					Some(crate::profile::Step::Done(pick)) => state.finish_bench(pick),
					Some(crate::profile::Step::Stalled) => state.finish_bench_stalled(),
					_ => {}
				}
			}
			match step {
				RatingStep::Note => {
					let budget = state
						.frame_budget
						.at(Instant::now(), || refresh_hz(&state.window));
					state.rating.note(Instant::now(), budget);
					if state.rating.verdict(budget) == Some(true) {
						state.step_down_profile();
					}
				}
				RatingStep::Pause => state.rating.pause(),
			}
			// a pane whose term was locked kept its content_dirty (rebuild was
			// skipped) - retry shortly instead of waiting for the next event,
			// or the last wakeup of a burst could leave a stale frame up
			let retry = state.tabs.cur().panes.values().any(|p| p.content_dirty);
			let pace = max_fps().map(|fps| Duration::from_secs_f64(1.0 / fps));
			if state.bench.is_some() {
				ControlFlow::Poll
			} else if animating && (scroll_anim || bell_anim) {
				// scroll (the flagship smooth feature) and the bell flash render
				// at full rate; fresh content needs no Poll - each PTY read
				// batch arrives as its own Wakeup
				match pace {
					Some(ivl) => pace_frame(&mut state.next_frame, ivl),
					None => ControlFlow::Poll,
				}
			} else if retry {
				ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(5))
			} else if animating {
				// a lone idle cursor blink is capped to ~30fps so it isn't
				// re-rendering every frame just to pulse - but a pinned rate
				// covers the cursor too, or a scene where the cursor is the only
				// thing moving samples off the grid the rest of the run is on
				match pace {
					Some(ivl) => pace_frame(&mut state.next_frame, ivl),
					None => ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33)),
				}
			} else {
				ControlFlow::Wait
			}
		} else {
			state.rating.pause();
			ControlFlow::Wait
		};
		// A move between monitors is looked at once it has settled, before the
		// size is saved, so the size goes to the monitor the window ended on.
		state.check_monitor();
		// Debounced remember-size: persist once the size has held; while one is
		// pending, make sure the loop wakes up to flush it even when idle.
		state.flush_window_size(false);
		// Settings shows the automatic size, which is this one
		if let Some(dialog) = self.dialog.as_mut() {
			if dialog.follow_window(state.watch.key.as_deref(), &config::settings()) {
				self.dialog_dirty = true;
			}
		}
		let flow = if let (ControlFlow::Wait, Some(_)) = (flow, state.pending_size) {
			let due = state.pending_size_at + SIZE_SAVE_DEBOUNCE;
			ControlFlow::WaitUntil(state.watch.check_at.map_or(due, |check| due.max(check)))
		} else {
			flow
		};
		let flow = wake_by(flow, state.watch.check_at);
		// copy-output: while a capture is armed, make sure the loop wakes at its
		// settle deadline to run the capture check even when otherwise idle.
		let flow = wake_by(flow, state.capture_wake());
		// keep frames coming while a dialog field edit animates
		let flow = wake_by(flow, dlg_wake);
		// keep the loop waking while dialog-raise retries are pending
		let flow = wake_by(flow, raise_wake);
		// Read after the frame, since the frame and the rating step after it can
		// set a wake or owe another frame: a cursor that parks, a minimap compose
		// that owes another, the window revealed, a rating ended. Read before,
		// each waited for some unrelated event.
		if let Some(wake) = pane_wake(state.tabs.cur().panes.values(), Instant::now()) {
			cursor_wake = Some(cursor_wake.map_or(wake, |w| w.min(wake)));
		}
		let flow = wake_by(
			flow,
			(state.dirty && !hidden && state.gpu.is_some()).then(Instant::now),
		);
		// a frame or a device the GPU refused is tried again on its backoff
		let refused = if state.gpu.is_some() {
			state.frame_retry.at.filter(|_| !hidden)
		} else {
			state.idle.rebuild_wake(hidden)
		};
		let refused = [self.dialog.as_ref(), self.notice.as_ref()]
			.into_iter()
			.flatten()
			.filter_map(|d| d.refused.at)
			.chain(refused)
			.min();
		let flow = wake_by(flow, refused);
		// wake a parked cursor at its scheduled resume time, even when idle
		let flow = wake_by(flow, cursor_wake);
		// wake to let the device go once the window has sat idle long enough
		let idle_wake = (state.gpu.is_some() && !dialog_up)
			.then(|| state.release_deadline(&config::settings()))
			.flatten();
		let flow = wake_by(flow, idle_wake);
		let memdbg_wake = env_flag(EnvFlag::MemDbg).then_some(self.memdbg_next);
		let flow = wake_by(flow, memdbg_wake);
		// wake to take "resources restored" out of the title
		let flow = wake_by(flow, state.conserve.wake());
		// wake for the second heal after a return to this console
		let flow = wake_by(flow, state.vt_heal.again);
		// wake to rotate the wallpaper when its interval is up, even when idle
		// (not while the device is gone: the rebuild picks up where it left off)
		let flow = wake_by(flow, state.wp_next.filter(|_| state.gpu.is_some()));
		// wake to prepare the wallpaper for a new size once resizing stops
		let flow = wake_by(flow, state.wp_resize_at);
		// wake when the background shell scan comes due, even on an idle window
		let flow = wake_by(flow, state.shell_scan_at);
		// wake to raise a tab tip whose pointer has rested, and to keep an open
		// one current - the pointer sitting still generates no events of its own,
		// so nothing else would bring the window back
		let flow = wake_by(flow, state.tip_wake());
		// wake when the benchmark comes due, and again when its banner may come
		// down - an idle window generates nothing of its own to bring it back
		let bench_wake = match (state.bench_at, state.bench_cap) {
			(Some(_), Some(cap)) if state.bench_blocked() => Some(cap),
			(at, _) => at,
		};
		let flow = wake_by(flow, bench_wake.or(state.bench_banner_wake()));
		// wake at the reveal deadline so a hidden startup window is shown even if no
		// post-resize frame arrives
		let flow = wake_by(flow, (!state.revealed).then_some(state.reveal_deadline));
		// slow-tick wake so the VRAM sentinel probe runs even while fully idle
		let flow = wake_by(
			flow,
			(state.gl && state.gpu.is_some()).then_some(state.vram_next),
		);
		// Profiling keeps the loop hot so the workload is continuously exercised.
		#[cfg(feature = "profiling")]
		let flow = if std::env::var_os("SILK_PROFILE_OUT").is_some() {
			ControlFlow::Poll
		} else {
			flow
		};
		event_loop.set_control_flow(flow);
	}
}

impl State {
	// Ctrl+Shift+C or Command+C, focused window or not
	fn copy_selection(&mut self) {
		if !input::copy_allowed(CopyFrom::Chord, self.focused, true) {
			return;
		}
		let focused = self.tabs.cur().focused;
		if let Some(text) = self
			.tabs
			.cur()
			.panes
			.get(&focused)
			.and_then(super::pane::Pane::selection_text)
		{
			self.clipboard.set_clipboard(text);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::push_back;
	use super::tabs::tab_close_box;
	use super::{
		CloseScope, PaneWakes, SCRIM_PCT_PER_DOUBLING, TAB_CLOSE_M, close_scope, focus_ring,
		is_copy_chord, key_is_typed, pace_frame, pane_wake, reveal_due, wake_by,
	};
	use crate::config;
	use crate::gfx::{FRAME_RETRY_FIRST, FRAME_RETRY_MAX, Retry};
	use crate::shells::ShellEntry;
	use std::time::{Duration, Instant};
	use winit::event::ElementState;
	use winit::event_loop::ControlFlow;

	// Each debug switch keeps its own cached answer. SILK_IDLEDBG used to share
	// SILK_DLGDBG's, so whichever was read first answered for both. Run in a
	// child copy of this test binary, since the answers are cached per process.
	// Test ID: Erg4k2j
	#[test]
	fn each_debug_switch_reads_its_own_variable() {
		for set in super::EnvFlag::ALL {
			let mut child = std::process::Command::new(std::env::current_exe().unwrap());
			child.args(["--exact", "app::tests::debug_switch_child", "--nocapture"]);
			for flag in super::EnvFlag::ALL {
				child.env_remove(flag.var());
			}
			let out = child
				.env("SILK_DEBUG_SWITCH_CHILD", set.var())
				.env(set.var(), "1")
				.output()
				.unwrap();
			let text = String::from_utf8_lossy(&out.stdout);
			assert!(
				out.status.success(),
				"{set:?}: {text}{}",
				String::from_utf8_lossy(&out.stderr)
			);
			assert!(text.contains("debug switches checked"), "{set:?}: {text}");
		}
	}

	// Test ID: Erg4kMG
	#[test]
	fn debug_switch_child() {
		use super::{EnvFlag, env_flag};
		let Some(set) = std::env::var_os("SILK_DEBUG_SWITCH_CHILD") else {
			return; // only does anything when the test above starts it
		};
		// the one that is set goes first, so a shared cache would answer for the rest
		let set = EnvFlag::ALL.into_iter().find(|f| set == f.var()).unwrap();
		assert!(env_flag(set), "{set:?}");
		for flag in EnvFlag::ALL {
			assert_eq!(
				env_flag(flag),
				flag.var() == set.var(),
				"{flag:?} with {set:?} set"
			);
		}
		println!("debug switches checked");
	}

	// The strength scale went from 10% to 20% per doubling in August and design.md
	// kept the old numbers for weeks, with nothing to catch it. Both places that
	// quote the number are held against the code now.
	// Test ID: EqSDfqy
	#[test]
	fn the_docs_quote_the_scrim_strength_scale_the_code_uses() {
		let pct = SCRIM_PCT_PER_DOUBLING;
		let spelled = [
			"zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
		];
		let doublings = spelled[(100.0 / pct) as usize];
		let read = |rel: &str| {
			let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
			std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
		};
		let said =
			format!("each {pct:.0}% doubles its opacity, up to {doublings} doublings at 100%");
		// the doc's name starts with the time it was written, so find it by its tail
		let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../project/design_docs");
		let doc = std::fs::read_dir(&docs)
			.unwrap_or_else(|e| panic!("{}: {e}", docs.display()))
			.filter_map(Result::ok)
			.map(|e| e.path())
			.find(|p| p.to_string_lossy().ends_with("_scrim.md"))
			.expect("no scrim design doc");
		assert!(
			std::fs::read_to_string(&doc).unwrap().contains(&said),
			"{} does not say: {said}",
			doc.display()
		);
		let commented = format!("each {pct:.0}% is one doubling");
		assert!(
			read("src/settings_ui.shcl").contains(&commented),
			"settings_ui.shcl does not say: {commented}"
		);
	}

	// A menu outlives the pane it was opened for when that pane's shell ends, and
	// the tab can go with it. Close pane then found no such pane in the current tab
	// and fell through to closing the tab, or the window, with another tab's shells
	// still running.
	// Test ID: Eq4SnxI
	#[test]
	fn close_pane_on_a_pane_that_is_gone_closes_nothing() {
		assert_eq!(close_scope(false, 1, 1), CloseScope::Nothing);
		assert_eq!(close_scope(false, 2, 3), CloseScope::Nothing);
		// a live pane still cascades: the pane, then its tab, then the window
		assert_eq!(close_scope(true, 2, 1), CloseScope::Pane);
		assert_eq!(close_scope(true, 1, 2), CloseScope::Tab);
		assert_eq!(close_scope(true, 1, 1), CloseScope::Window);
	}

	// A frame the surface refused under GPU load was dropped, and with the pass
	// that asked for it already done, nothing drew again until the desktop asked.
	// Test ID: Erfy7et
	#[test]
	fn a_refused_frame_is_drawn_again_on_a_backoff() {
		let now = Instant::now();
		let mut retry = Retry::default();
		retry.missed(now, FRAME_RETRY_FIRST, FRAME_RETRY_MAX);
		let wake = retry.at.expect("a refused frame is owed");
		assert!(wake > now, "not on the same pass, or it spins");
		assert!(!retry.take_due(now));
		assert!(retry.take_due(wake), "due once the wait is over");
		assert!(!retry.take_due(wake), "taken once");
		assert_eq!(retry.at, None);

		let mut last = Duration::ZERO;
		for _ in 0..40 {
			let wait = retry.missed(now, FRAME_RETRY_FIRST, FRAME_RETRY_MAX);
			assert!(wait >= last, "each miss waits at least as long");
			assert!(wait <= FRAME_RETRY_MAX);
			last = wait;
		}
		assert_eq!(
			last, FRAME_RETRY_MAX,
			"a GPU that stays busy is asked at the cap"
		);

		retry = Retry::default();
		assert_eq!(
			retry.missed(now, FRAME_RETRY_FIRST, FRAME_RETRY_MAX),
			FRAME_RETRY_FIRST,
			"a drawn frame starts the backoff over"
		);
	}

	// The window starts hidden and is shown once a frame is drawn. Metal will not
	// hand a hidden window a frame, so waiting for one hung the Mac build.
	// Test ID: ErUBJ18
	#[test]
	fn a_hidden_window_that_cannot_draw_is_shown_anyway() {
		assert!(reveal_due(false, false, true));
		assert!(!reveal_due(false, false, false));
		assert!(reveal_due(false, true, false));
		assert!(reveal_due(true, false, false));
	}

	// Read-only means the pane takes nothing the user's hands sent. Typing and
	// paste were on that list; the mouse reports and the wheel's alt-screen
	// cursor keys were not, so one notch sent arrow keys to the job the pane said
	// it was protecting. Everything user-driven goes through `write_input` now,
	// and the only direct write left is the reply the terminal owes the program.
	// Test ID: EpHQ61Q
	#[test]
	fn a_read_only_pane_takes_nothing_the_user_sent() {
		// this file and the ones under app/, each above its own tests
		let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
		let mut texts = vec![include_str!("app.rs").to_string()];
		for entry in std::fs::read_dir(dir).unwrap().flatten() {
			texts.push(std::fs::read_to_string(entry.path()).unwrap());
		}
		assert!(texts.len() > 1, "found nothing under app/");
		let body: String = texts
			.iter()
			.map(|text| {
				text.split("\nmod tests {")
					.next()
					.expect("the file above its own tests")
			})
			.collect();
		let direct: Vec<&str> = body
			.lines()
			.filter(|l| l.contains("term.write("))
			.map(str::trim)
			.collect();
		assert_eq!(
			direct.len(),
			1,
			"these bypass the read-only gate: {direct:?}"
		);
		assert!(
			body.matches("write_input(").count() >= 5,
			"the mouse and wheel writes go through the gate"
		);
	}

	// converts it at a boundary the way the Settings dialog does - every piece
	// scales at its own use site, and a piece that misses out is exactly the
	// defect this pass fixed (chrome thinning out as the display's DPI rises).
	// So: the same geometry at 2x must come out at twice the size, everywhere.
	// Test ID: EnLuU52
	#[test]
	fn the_chrome_doubles_when_the_display_does() {
		// the tab's close button, measured against a bar that has itself doubled
		let one = tab_close_box(0.0, 200.0, 0.0, 30.0, 1.0);
		let two = tab_close_box(0.0, 400.0, 0.0, 60.0, 2.0);
		assert_eq!(two.w, one.w * 2.0, "close button width");
		assert_eq!(two.h, one.h * 2.0, "close button height");
		assert_eq!(two.y, one.y * 2.0, "close button top margin");
		// its right margin, which is what a raw-px inset would have left frozen
		assert_eq!(400.0 - (two.x + two.w), (200.0 - (one.x + one.w)) * 2.0);
		assert_eq!(one.y, TAB_CLOSE_M, "1x must still be the DIP value itself");

		// the live pane's focus ring
		let rect = super::Rect {
			x: 0.0,
			y: 0.0,
			w: 100.0,
			h: 100.0,
		};
		let thin = focus_ring(rect, 1.0)[0].size[1];
		let thick = focus_ring(rect, 2.0)[0].size[1];
		assert_eq!(thin, config::FOCUS_RING_PX);
		assert_eq!(thick, thin * 2.0);
	}

	// A WM hotkey grab (Ctrl+Alt+Arrow) brackets its chord with a focus change,
	// and winit replays every held key as a synthetic press on the way back in -
	// before it re-reads the modifiers. Taking that replay as typing is what put
	// a bare arrow into the shell, so only a real press may count.
	// Test ID: EmpzjSS
	#[test]
	fn a_replayed_key_is_not_typing() {
		assert!(key_is_typed(ElementState::Pressed, false));
		assert!(!key_is_typed(ElementState::Pressed, true));
		assert!(!key_is_typed(ElementState::Released, false));
		assert!(!key_is_typed(ElementState::Released, true));
	}

	// The copy chord skips the unfocused-key gate, so it must never match what
	// that gate is for: a key passed through a WM grab with the modifiers zeroed.
	// Test ID: EpyCuGe
	#[test]
	fn only_a_held_ctrl_shift_c_is_the_copy_chord() {
		use winit::keyboard::{Key, ModifiersState, NamedKey};
		let both = ModifiersState::CONTROL | ModifiersState::SHIFT;
		let keys = crate::keys::Bindings::defaults(false);
		assert!(is_copy_chord(&keys, both, &Key::Character("c".into())));
		assert!(is_copy_chord(&keys, both, &Key::Character("C".into())));
		assert!(!is_copy_chord(
			&keys,
			ModifiersState::empty(),
			&Key::Character("c".into())
		));
		assert!(!is_copy_chord(
			&keys,
			ModifiersState::CONTROL,
			&Key::Character("c".into())
		));
		assert!(!is_copy_chord(&keys, both, &Key::Character("v".into())));
		assert!(!is_copy_chord(&keys, both, &Key::Named(NamedKey::ArrowUp)));
		assert!(!is_copy_chord(
			&crate::keys::Bindings::defaults(true),
			ModifiersState::empty(),
			&Key::Character("c".into())
		));
	}

	// The demo capture samples on a fixed clock, so a pinned rate that drifts is
	// worse than none: it would wander off the sampling grid instead of sitting
	// on it. Each frame's deadline therefore has to come from the LAST DEADLINE,
	// never from "now" - which is the whole of what this pins.
	// Test ID: EmptHsG
	#[test]
	fn a_pinned_frame_rate_does_not_drift() {
		let ivl = Duration::from_millis(20);
		let mut next = None;
		let start = Instant::now();
		pace_frame(&mut next, ivl);
		let first = next.unwrap();
		// a frame that takes most of its budget must not push the next one out
		for step in 1..=10u32 {
			pace_frame(&mut next, ivl);
			assert_eq!(next.unwrap(), first + ivl * step, "drifted at frame {step}");
		}
		assert!(next.unwrap() >= start + ivl * 11);

		// falling behind resyncs to now rather than firing a catch-up burst
		let behind = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
		let mut late = Some(behind);
		pace_frame(&mut late, ivl);
		assert!(
			late.unwrap() > Instant::now(),
			"a stale deadline must be dropped"
		);
	}

	struct Wakes {
		cursor: Option<Instant>,
		map: Option<Instant>,
	}

	impl PaneWakes for Wakes {
		fn cursor_wake_at(&self) -> Option<Instant> {
			self.cursor
		}

		fn map_wake_at(&self) -> Option<Instant> {
			self.map
		}
	}

	// A cursor that parks in a frame sets its resume time in that frame. The
	// loop read the cursor's wake only before drawing, so the resume waited for
	// some other event. Every kind of pane wake is read after the frame now.
	// Test ID: ErJF0fr
	#[test]
	fn a_wake_set_while_drawing_is_kept() {
		let now = Instant::now();
		let soon = now + Duration::from_millis(300);
		let later = now + Duration::from_secs(2);
		let parked = [Wakes {
			cursor: Some(soon),
			map: None,
		}];
		assert_eq!(
			pane_wake(&parked, now),
			Some(soon),
			"a parked cursor's wake"
		);
		let mixed = [
			Wakes {
				cursor: Some(later),
				map: None,
			},
			Wakes {
				cursor: None,
				map: Some(soon),
			},
			Wakes {
				cursor: Some(soon + Duration::from_millis(1)),
				map: Some(later),
			},
		];
		assert_eq!(
			pane_wake(&mixed, now),
			Some(soon),
			"the earliest, any pane or kind"
		);
		// due ones belong to the pass before the frame, or an idle window spins
		let due = [Wakes {
			cursor: Some(now),
			map: now.checked_sub(Duration::from_millis(1)),
		}];
		assert_eq!(pane_wake(&due, now), None);
	}

	// Every wake the loop folds in goes through one merge. A wait takes the
	// earliest, a poll stays a poll, and no wake leaves the flow alone.
	// Test ID: ErzM4AJ
	#[test]
	fn a_wake_folds_into_the_flow_by_the_earliest() {
		let now = Instant::now();
		let soon = now + Duration::from_millis(5);
		let later = now + Duration::from_secs(1);
		let until = |flow| match flow {
			ControlFlow::WaitUntil(at) => Some(at),
			ControlFlow::Wait | ControlFlow::Poll => None,
		};
		assert_eq!(until(wake_by(ControlFlow::Wait, Some(soon))), Some(soon));
		assert_eq!(
			until(wake_by(ControlFlow::WaitUntil(later), Some(soon))),
			Some(soon)
		);
		assert_eq!(
			until(wake_by(ControlFlow::WaitUntil(soon), Some(later))),
			Some(soon)
		);
		assert!(matches!(
			wake_by(ControlFlow::Poll, Some(soon)),
			ControlFlow::Poll
		));
		assert!(matches!(
			wake_by(ControlFlow::Wait, None),
			ControlFlow::Wait
		));
		assert_eq!(
			until(wake_by(ControlFlow::WaitUntil(later), None)),
			Some(later)
		);
	}

	pub(super) fn shell(title: &str, active: bool) -> ShellEntry {
		ShellEntry {
			slug: title.to_lowercase(),
			title: title.into(),
			command: title.to_lowercase(),
			active,
			comment: String::new(),
			last_seen: String::new(),
		}
	}

	// Test ID: Er2UvPw
	#[test]
	fn the_wallpaper_pushes_a_wait_back_but_never_starts_one_again() {
		let now = Instant::now();
		let armed = Some(now);
		let due = now + Duration::from_secs(3);
		let cap = now + Duration::from_secs(20);
		assert_eq!(push_back(armed, Some(cap), due), Some(due));
		assert_eq!(push_back(armed, None, due), Some(due));
		let late = now + Duration::from_secs(30);
		assert_eq!(
			push_back(armed, Some(cap), late),
			Some(cap),
			"past the backstop"
		);
		assert_eq!(
			push_back(None, Some(cap), due),
			None,
			"the scan already ran"
		);
	}

	// The default shell is the first entry switched on, not the first entry.
	// Test ID: Er2VLed
	#[test]
	fn the_default_shell_is_the_first_active_entry() {
		let _store = config::test_store_lock();
		let saved = config::settings();
		let with = |shells: Vec<ShellEntry>| {
			config::update(config::Settings {
				shells,
				..(*saved).clone()
			});
			config::default_shell_argv()
		};
		let mut listed = vec![
			shell("fish", false),
			shell("Bash", true),
			shell("zsh", true),
		];
		listed[1].command = "bash -l".into();
		assert_eq!(
			with(listed),
			Some(vec!["bash".to_string(), "-l".to_string()])
		);
		assert_eq!(with(vec![shell("fish", false)]), None);
		config::update((*saved).clone());
	}

	// Test ID: Er2UiYX
	#[test]
	fn leaving_a_bare_window_puts_back_only_what_was_on() {
		use super::bare_chrome;
		// going bare saves what was on and hides both
		assert_eq!(
			bare_chrome(true, (false, false), (true, true)),
			((true, true), (false, false))
		);
		assert_eq!(
			bare_chrome(false, (true, true), (false, false)).1,
			(true, true)
		);
		// the frame was already off, so it stays off
		assert_eq!(
			bare_chrome(false, (false, true), (false, false)).1,
			(false, true)
		);
		// a menu bar switched on while bare stays on
		assert_eq!(
			bare_chrome(false, (true, false), (false, true)).1,
			(true, true)
		);
	}
}
