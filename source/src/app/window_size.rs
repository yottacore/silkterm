// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The window's size: the grid it opens at, what gets remembered, and the
//! monitor checks after a move.

use super::{
	MONITOR_RECHECK, MONITOR_SETTLE, OWN_RESIZE_GRACE, SAFE_MAX_DIM, SIZE_SAVE_DEBOUNCE, State,
	save_live,
};
use crate::config;
use std::time::Instant;
use winit::window::Window;

impl State {
	/// Track the live window size as columns/rows so "remember last size" can
	/// restore it next launch. Kept separate from the user's defined columns/rows
	/// (unchecking the option reverts to those). The inverse of the launch sizing.
	pub(super) fn save_window_size(&mut self, w: u32, h: u32) {
		// skip the creation/programmatic resizes that fire before the first frame,
		// so they don't clobber the remembered size with the launch size
		if !self.size_tracked {
			return;
		}
		if Instant::now() < self.watch.ignore_resize_until {
			return;
		}
		let now = self.window.inner_size();
		if !resize_is_current((w, h), (now.width, now.height)) {
			return;
		}
		self.watch.size_pinned = false;
		self.note_grid(w, h);
	}

	/// The grid the window shows at this size, waiting to be saved.
	pub(super) fn note_grid(&mut self, w: u32, h: u32) {
		let px_to_cells = |px: f32, cell: f32, chrome: f32| {
			(((px - 2.0 * self.text.margin - chrome) / cell).floor() as i64).max(1) as usize
		};
		let cols = px_to_cells(w as f32, self.text.cell_w, 0.0);
		let rows = px_to_cells(h as f32, self.text.cell_h, self.chrome_h());
		// debounce: an interactive drag fires many Resized events; writing
		// config.shcl on each would be dozens of file writes/sec. Persist in
		// flush_window_size once the size has held (or on exit).
		self.pending_size = Some((cols, rows));
		self.pending_size_at = Instant::now();
	}

	pub(super) fn flush_window_size(&mut self, force: bool) {
		let Some((cols, rows)) = self.pending_size else {
			return;
		};
		if !force && self.pending_size_at.elapsed() < SIZE_SAVE_DEBOUNCE {
			return;
		}
		// which monitor the size is for is not known until a move settles
		if !force && self.watch.check_at.is_some() {
			return;
		}
		self.pending_size = None;
		// Read once the size has held, not in the resize event: the window
		// manager may set the maximized state after the resize it caused.
		let fullscreen = self.window.fullscreen().is_some();
		let maximized = self.window.is_maximized();
		let grid =
			remember_resize(self.size_tracked, fullscreen, maximized).then_some((cols, rows));
		let zoom = (!self.watch.font_pinned).then(config::font_zoom_px);
		let kept = |s: &config::Settings| {
			(
				s.remembered_columns,
				s.remembered_rows,
				s.remembered_maximized,
				s.remembered_font_zoom,
				s.monitor_sizes.clone(),
			)
		};
		// If the file's open elsewhere the write is skipped (retried on the next
		// resize or at exit); the live size still updates in memory either way.
		save_live(|live| {
			let before = kept(live);
			config::remember_window(live, self.watch.key.as_deref(), grid, zoom);
			// a fullscreen window hides whether the one under it is maximized
			if !fullscreen {
				live.remembered_maximized = maximized;
			}
			kept(live) != before
		});
	}

	/// A move starts the wait for the window to settle.
	pub(super) fn note_moved(&mut self) {
		let now = Instant::now();
		if now < self.watch.ignore_moves_until {
			return;
		}
		self.start_settle(now);
	}

	/// Nothing before the window is shown: a resize then would hold up the
	/// reveal, which waits for the launch size. The reveal looks once itself.
	pub(super) fn start_settle(&mut self, now: Instant) {
		if !self.revealed {
			return;
		}
		self.watch.moved_at.get_or_insert(now);
		self.watch.check_at = Some(now + MONITOR_SETTLE);
	}

	pub(super) fn check_monitor(&mut self) {
		let Some(at) = self.watch.check_at else {
			return;
		};
		let now = Instant::now();
		if now < at {
			return;
		}
		if crate::monitor::button_held(&self.window) {
			self.watch.check_at = Some(now + MONITOR_RECHECK);
			return;
		}
		self.watch.check_at = None;
		let moved_at = self.watch.moved_at.take();
		let now_on = crate::monitor::MonitorId::of_window(&self.window).map(|m| m.key());
		let pending_at = self.pending_size.map(|_| self.pending_size_at);
		let Settle::Arrive {
			save_first,
			take_size,
		} = settle(
			self.watch.key.as_deref(),
			now_on.as_deref(),
			moved_at,
			pending_at,
		)
		else {
			return;
		};
		if save_first {
			self.flush_window_size(true);
		}
		self.watch.key = now_on;
		if take_size {
			self.take_monitor_size();
		}
	}

	// Size the window for the monitor it has arrived on: that monitor's own
	// size, else the last size set anywhere, as a launch there would.
	fn take_monitor_size(&mut self) {
		let live = config::settings();
		if !live.remember_size || !live.remember_per_monitor || self.watch.size_pinned {
			return;
		}
		if self.window.fullscreen().is_some() || self.window.is_maximized() {
			return;
		}
		// another window may have kept a size for this monitor since this one loaded
		config::refresh_window_memory();
		let kept = config::remembered_window(&config::settings(), self.watch.key.as_deref());
		// the zoom first, since the grid's size in pixels depends on it
		if !self.watch.font_pinned && kept.font_zoom != config::font_zoom_px() {
			config::set_font_zoom(kept.font_zoom);
			self.rebuild_text(config::display_scale(self.window.scale_factor()));
			self.dirty = true;
		}
		let now = Instant::now();
		self.watch.ignore_resize_until = now + OWN_RESIZE_GRACE;
		self.watch.ignore_moves_until = now + OWN_RESIZE_GRACE;
		self.request_grid(kept.columns, kept.rows);
	}

	/// Ask for the window size that shows this grid with the chrome as it is.
	pub(super) fn request_grid(&mut self, cols: usize, rows: usize) {
		let (w, h) = window_px(
			cols,
			rows,
			self.text.cell_w,
			self.text.cell_h,
			self.text.margin,
			self.chrome_h(),
		);
		let max_dim = self.gpu.as_ref().map_or(SAFE_MAX_DIM, |gpu| {
			gpu.gfx.device.limits().max_texture_dimension_2d
		});
		let (w, h) = fit_px(w, h, max_dim);
		if (w, h) == self.surface_px {
			return;
		}
		let want = winit::dpi::PhysicalSize::new(w, h);
		// A size the window can honor straight away answers here and sends no
		// `Resized`, so this is the only chance to move everything the window
		// event moves - the scrim included, which was left at the old size.
		if let Some(applied) = request_size(&self.window, want) {
			self.resize_surface(applied.width, applied.height);
			self.relayout_all();
			self.invalidate_prepared();
			self.dirty = true;
		}
	}
}

/// The window a grid asks for: the cells, the margins either side, and the chrome
/// above them. `chrome` counts the menu bar and the tab strip where they show, or
/// the shell gets fewer rows than were asked for.
pub(super) fn window_px(
	cols: usize,
	rows: usize,
	cell_w: f32,
	cell_h: f32,
	margin: f32,
	chrome: f32,
) -> (u32, u32) {
	(
		(cols as f32 * cell_w + 2.0 * margin).ceil() as u32,
		(rows as f32 * cell_h + 2.0 * margin + chrome).ceil() as u32,
	)
}

/// A window may be no bigger than the largest texture the device will make: the
/// GL path renders the scene into an offscreen texture at the window's size, and
/// wgpu treats a refusal as fatal. So a count out of the config or the command
/// line is held here, or it ends the launch in `create_texture`.
pub(super) fn fit_px(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
	(w.clamp(1, max_dim), h.clamp(1, max_dim))
}

/// Open maximized? Only when the last window was left that way and the
/// setting is on. A size or fullscreen asked for on the command line wins.
pub(super) fn launch_maximized(settings: &config::Settings, cli: &crate::cli::WindowOpts) -> bool {
	settings.remember_maximized
		&& settings.remembered_maximized
		&& cli.columns.is_none()
		&& cli.rows.is_none()
		&& cli.pixel_width.is_none()
		&& cli.pixel_height.is_none()
		&& !cli.fullscreen.unwrap_or(false)
}

/// Which monitor the window's size is kept for, and the wait for a move to
/// another one to settle (monitor.rs, `config::remembered_window`).
#[derive(Debug)]
pub(super) struct MonitorWatch {
	pub(super) key: Option<String>,
	pub(super) check_at: Option<Instant>,
	pub(super) moved_at: Option<Instant>, // when the move being waited out began
	pub(super) ignore_resize_until: Instant,
	pub(super) ignore_moves_until: Instant, // the window's own resize can move it, too
	// The command line set the size, or the font size, and it stays until
	// the user resizes the window, or zooms the font.
	pub(super) size_pinned: bool,
	pub(super) font_pinned: bool,
	/// Wayland tells a window neither where it is nor that it moved, so the
	/// pointer coming back after a drag is the sign to look.
	pub(super) positionless: bool,
}

#[derive(Debug, PartialEq)]
enum Settle {
	Stay,
	Arrive { save_first: bool, take_size: bool },
}

// What a move does once it has settled. A size the user set before the move
// began belongs to the monitor it left, and is saved there first. One set
// after it is the user sizing the window on arrival, which the new monitor's
// own size must not undo.
fn settle(
	kept_for: Option<&str>,
	now_on: Option<&str>,
	moved_at: Option<Instant>,
	pending_at: Option<Instant>,
) -> Settle {
	let Some(now_on) = now_on else {
		return Settle::Stay;
	};
	if kept_for == Some(now_on) {
		return Settle::Stay;
	}
	let save_first = matches!((pending_at, moved_at), (Some(set), Some(moved)) if set < moved);
	Settle::Arrive {
		save_first,
		take_size: pending_at.is_none() || save_first,
	}
}

// Is this resize the size to launch at next time? Only once a frame has been
// drawn - before that it is the launch size arriving back - and never a
// fullscreen or maximized one, which is not a window to come back to.
fn remember_resize(size_tracked: bool, fullscreen: bool, maximized: bool) -> bool {
	size_tracked && !fullscreen && !maximized
}

// Is a resize the size the window has now? On macOS winit hands over the
// resize from the window's creation after the first frame, and taking it
// saved the default window's grid on every launch (2026100514211602). Every
// backend's event carries what `inner_size` answers at that moment, so one
// that disagrees is stale. A minimized window on Windows has no area, which
// is no size to open at.
fn resize_is_current(event: (u32, u32), now: (u32, u32)) -> bool {
	event == now && event.0 > 0 && event.1 > 0
}

/// Asks for a size and answers the one the window took, if it took one now.
/// winit on macOS answers nothing though the window resizes at once, and its
/// `Resized` comes later, behind a stale one from creation (2026100517535929).
pub(crate) fn request_size(
	window: &Window,
	want: winit::dpi::PhysicalSize<u32>,
) -> Option<winit::dpi::PhysicalSize<u32>> {
	let before = window.inner_size();
	let answered = window.request_inner_size(want);
	size_taken(answered, before, window.inner_size())
}

fn size_taken(
	answered: Option<winit::dpi::PhysicalSize<u32>>,
	before: winit::dpi::PhysicalSize<u32>,
	after: winit::dpi::PhysicalSize<u32>,
) -> Option<winit::dpi::PhysicalSize<u32>> {
	answered.or_else(|| (after != before && after.width > 0 && after.height > 0).then_some(after))
}

#[cfg(test)]
mod tests {
	use super::{
		Settle, fit_px, launch_maximized, remember_resize, resize_is_current, settle, size_taken,
		window_px,
	};
	use std::time::{Duration, Instant};
	// `window.rows: 1000` in the config, or --rows 1000, asked for a window taller
	// than the device's largest texture. The GL path's offscreen is made at the
	// window's size, and wgpu treats the refusal as fatal, so the launch died.
	// Test ID: Eq4SnxJ
	#[test]
	fn a_window_is_never_bigger_than_the_device_allows() {
		let (w, h) = window_px(1000, 1000, 8.0, 17.0, 4.0, 30.0);
		assert_eq!((w, h), (8008, 17038));
		assert_eq!(fit_px(w, h, 16384), (8008, 16384));
		assert_eq!(fit_px(0, 0, 16384), (1, 1));
	}

	// A requested row count is the shell's rows, so the tab strip counts against
	// the window's height while it shows. Left out of the sum, a window asked for
	// 24 rows gave its shell 22.
	// Test ID: Eq4SnxK
	#[test]
	fn the_tab_strip_counts_against_a_requested_row_count() {
		let shown = window_px(80, 24, 8.0, 17.0, 4.0, 20.0 + 24.0);
		let hidden = window_px(80, 24, 8.0, 17.0, 4.0, 20.0);
		assert_eq!(shown.0, hidden.0);
		assert_eq!(shown.1 - hidden.1, 24);
	}

	// --fullscreen asks for fullscreen before the first frame, and the window
	// manager's resize arrives after it, so a one-off fullscreen launch stored the
	// whole screen as the size to open at next time.
	// Test ID: Eq4SnxL
	#[test]
	fn a_fullscreen_or_maximized_size_is_not_remembered() {
		assert!(remember_resize(true, false, false));
		assert!(!remember_resize(true, true, false));
		assert!(!remember_resize(true, false, true));
		// nothing before the first frame, as before
		assert!(!remember_resize(false, false, false));
	}

	// The order a Mac launch delivered: the creation size's resize after the
	// first frame, with the window already at its launch size, then the launch
	// size inside the scale change's grace. Only the window's own size counts.
	// Test ID: ErsO6GB
	#[test]
	fn a_resize_the_window_has_moved_past_is_not_saved() {
		let created = (2000, 1280);
		let launched = (2394, 828);
		assert!(!resize_is_current(created, launched));
		assert!(resize_is_current(launched, launched));
		assert!(!resize_is_current((0, 0), (0, 0)), "minimized");
		assert!(!resize_is_current((2394, 0), (2394, 0)));
	}

	// A request winit answers with nothing still counts when the window already
	// has a new size, so the first frame is drawn at it. The numbers are a b26
	// launch: created at 2000x1280, asked for 1988x1269, took 1988x1270.
	// Test ID: ErwgxDd
	#[test]
	fn a_size_the_window_took_without_saying_is_drawn_at() {
		let px = winit::dpi::PhysicalSize::new;
		assert_eq!(
			size_taken(None, px(2000, 1280), px(1988, 1270)),
			Some(px(1988, 1270)),
			"macOS"
		);
		assert_eq!(
			size_taken(None, px(2000, 1000), px(2000, 1000)),
			None,
			"still on its way"
		);
		assert_eq!(
			size_taken(Some(px(900, 600)), px(2000, 1000), px(900, 600)),
			Some(px(900, 600))
		);
		assert_eq!(size_taken(None, px(2000, 1000), px(0, 0)), None);
	}

	// A settled move to another monitor takes that monitor's size, unless the
	// user sized the window after the move began. A size set before the move
	// is saved for the monitor it was set on, and the new one's is taken.
	// Test ID: EreYcuU
	#[test]
	fn a_move_takes_the_new_monitors_size_but_never_undoes_the_user() {
		let t0 = Instant::now();
		let (before, moved, after) = (
			t0,
			t0 + Duration::from_millis(10),
			t0 + Duration::from_millis(20),
		);
		let (a, b) = (Some("2560x1440_125pct"), Some("1920x1080_100pct"));
		assert_eq!(
			settle(a, a, Some(moved), None),
			Settle::Stay,
			"same monitor"
		);
		assert_eq!(
			settle(a, None, Some(moved), None),
			Settle::Stay,
			"monitor unknown"
		);
		let arrive = |save_first, take_size| Settle::Arrive {
			save_first,
			take_size,
		};
		assert_eq!(settle(a, b, Some(moved), None), arrive(false, true));
		assert_eq!(settle(a, b, Some(moved), Some(before)), arrive(true, true));
		assert_eq!(settle(a, b, Some(moved), Some(after)), arrive(false, false));
		// the first look after launch on Wayland, where the monitor was unknown
		assert_eq!(settle(None, b, Some(moved), None), arrive(false, true));
	}

	// A window left maximized opens maximized, unless the setting is off or the
	// command line asked for a size or fullscreen of its own.
	// Test ID: ErPSaVM
	#[test]
	fn a_window_left_maximized_opens_maximized() {
		use crate::cli::WindowOpts;
		let mut s = crate::config::Settings::default();
		let plain = WindowOpts::default();
		assert!(
			!launch_maximized(&s, &plain),
			"a fresh config opens restored"
		);
		s.remembered_maximized = true;
		assert!(
			!launch_maximized(&s, &plain),
			"the setting is off by default"
		);
		s.remember_maximized = true;
		assert!(launch_maximized(&s, &plain));
		for cli in [
			WindowOpts {
				columns: Some(80),
				..Default::default()
			},
			WindowOpts {
				rows: Some(24),
				..Default::default()
			},
			WindowOpts {
				pixel_width: Some(800),
				..Default::default()
			},
			WindowOpts {
				pixel_height: Some(600),
				..Default::default()
			},
			WindowOpts {
				fullscreen: Some(true),
				..Default::default()
			},
		] {
			assert!(!launch_maximized(&s, &cli), "{cli:?}");
		}
		s.remember_maximized = false;
		assert!(!launch_maximized(&s, &plain), "the setting is off");
	}
}
