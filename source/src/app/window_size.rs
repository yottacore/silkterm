// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The window's size: the grid it opens at, what gets remembered, and the
//! monitor checks after a move.

use super::{
	MONITOR_RECHECK, MONITOR_SETTLE, NO_OUTPUT_YET, OWN_RESIZE_GRACE, SAFE_MAX_DIM,
	SIZE_SAVE_DEBOUNCE, State, save_live,
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
		// The next resize may be the way back, even when this one is in a
		// grace: sway sends a scale with fullscreen, and the grace after it
		// hid going fullscreen. Fullscreen the program asked for is known
		// now; waiting for the save to see it lost F11 twice in a row. A
		// window minimized on Windows has no area.
		if w == 0 || h == 0 || self.window.fullscreen().is_some() || self.window.is_maximized() {
			self.watch.noted.full = true;
		}
		if Instant::now() < self.watch.ignore_resize_until {
			return;
		}
		let now = self.window.inner_size();
		if !resize_is_current((w, h), (now.width, now.height)) {
			return;
		}
		// the way back from fullscreen is no size the user set here
		if !self.watch.noted.full {
			self.look_unseen();
		}
		self.watch.noted.resized = true;
		self.note_grid(w, h);
	}

	/// The grid the window shows at this size, waiting to be saved.
	pub(super) fn note_grid(&mut self, w: u32, h: u32) {
		let (cols, rows) = self.grid_at(w, h);
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
		let fullscreen = crate::monitor::fullscreen(&self.window);
		let maximized = self.window.is_maximized();
		let noted = std::mem::take(&mut self.watch.noted);
		let keep = what_to_keep(
			noted,
			remember_resize(self.size_tracked, fullscreen, maximized),
		);
		self.watch.noted = keep.carry;
		if keep.unpin {
			self.watch.size_pinned = false;
		}
		if saves_size(self.cli_size, &self.cli_style) {
			self.save_size((cols, rows), &keep, fullscreen, maximized);
		} else if keep.grid || keep.zoom {
			mondbg(
				"unsaved",
				self.watch.key.as_deref(),
				Some((cols, rows)),
				None,
			);
		}
		// Moved while fullscreen or maximized, and now back: a look takes the
		// size. Taken at once, a slow frame at the size back from fullscreen
		// came back from sway late and undid it (2026101417045856).
		if !force && keep.back && self.watch.take_owed {
			self.watch.check_at = Some(Instant::now() + MONITOR_SETTLE);
		}
	}

	fn save_size(&mut self, grid: (usize, usize), keep: &Keep, fullscreen: bool, maximized: bool) {
		let zoom = (keep.zoom && !self.watch.font_pinned).then(config::font_zoom_px);
		let grid = keep.grid.then(|| self.grid_to_open_at(grid, zoom));
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
		if grid.is_some() || zoom.is_some() {
			mondbg("save", self.watch.key.as_deref(), grid, zoom);
		}
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

	/// The grid to save, for the font the window opens at next time. That's
	/// the font now, unless `--font-size` or `--font-name` gave it: then the
	/// grid is kept if it fits the screen at the font a launch without them
	/// has, and else the grid that font shows in the window's size now
	/// (2026101412292756, 2026101422190936).
	fn grid_to_open_at(&mut self, grid: (usize, usize), zoom: Option<i32>) -> (usize, usize) {
		let zoom = zoom.unwrap_or_else(|| {
			config::remembered_window(&config::settings(), self.watch.key.as_deref()).font_zoom
		});
		let font = config::unheld_font(zoom);
		if font.same(&config::font_now()) {
			return grid;
		}
		let cell = self.text.cell_in(&font);
		let now = self.grid_px(grid.0, grid.1);
		let room = room_on(&self.window, self.watch.key.as_deref());
		let kept = grid_in_font(grid, now, cell, self.text.margin, self.chrome_h(), room);
		mondbg("refont", self.watch.key.as_deref(), Some(kept), Some(zoom));
		kept
	}

	/// Wayland tells a window nothing of a move between outputs at one scale,
	/// so it may still count itself on the one it left. A size the user sets
	/// is for the output the window is on as it's set, so it looks then. Not
	/// just before saving: a size set just before a move would go to the new
	/// output. A look already on its way decides as usual (2026101413420431).
	pub(super) fn look_unseen(&mut self) {
		if !self.watch.positionless || self.watch.check_at.is_some() || self.watch.put_off {
			return;
		}
		let Some(now_on) = crate::monitor::MonitorId::of_window(&self.window).map(|m| m.key())
		else {
			return;
		};
		if self
			.watch
			.key
			.as_deref()
			.is_some_and(|kept| crate::monitor::same_monitor(kept, &now_on))
		{
			return;
		}
		// a size set before the move is for the output it left
		self.flush_window_size(true);
		mondbg("found", Some(&now_on), None, None);
		self.watch.key = Some(now_on);
	}

	/// A move starts the wait for the window to settle.
	pub(super) fn note_moved(&mut self) {
		let now = Instant::now();
		if !move_counts(
			now < self.watch.ignore_moves_until,
			&self.watch.layout,
			|| crate::monitor::layout(&self.window),
		) {
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
		self.watch.stirred_at = Some(now);
		self.watch.put_off = false;
		self.watch.check_at = Some(now + MONITOR_SETTLE);
	}

	/// Signs the person is at the window again, which a window that can't see
	/// the buttons needs before it looks: the pointer over it, a click, a key,
	/// or focus.
	pub(super) fn note_presence(&mut self, event: &winit::event::WindowEvent) {
		use winit::event::WindowEvent;
		match event {
			WindowEvent::CursorLeft { .. } => {
				self.watch.pointer = Pointer::Away;
				return;
			}
			WindowEvent::CursorEntered { .. } => {
				// a Wayland compositor knows where its pointer is
				if !self.watch.positionless {
					self.watch.pointer = Pointer::Entered(None);
					return;
				}
				self.watch.pointer = Pointer::Over;
			}
			WindowEvent::CursorMoved { position, .. } => {
				self.watch.pointer = pointer_moved(self.watch.pointer, (position.x, position.y));
				if self.watch.pointer != Pointer::Over {
					return;
				}
			}
			WindowEvent::KeyboardInput { .. }
			| WindowEvent::ModifiersChanged(_)
			| WindowEvent::Focused(true)
			| WindowEvent::MouseInput { .. }
			| WindowEvent::MouseWheel { .. } => {}
			_ => return,
		}
		let now = Instant::now();
		self.watch.heard_at = Some(now);
		if std::mem::take(&mut self.watch.put_off) {
			self.watch.check_at = Some(now + MONITOR_SETTLE);
		}
	}

	pub(super) fn check_monitor(&mut self) {
		let Some(at) = self.watch.check_at else {
			return;
		};
		let now = Instant::now();
		if now < at {
			return;
		}
		let heard = matches!(
			(self.watch.heard_at, self.watch.stirred_at),
			(Some(heard), Some(stirred)) if heard > stirred
		);
		let layout = crate::monitor::layout(&self.window);
		let drag = drag_state(
			crate::monitor::button_held(&self.window),
			self.watch.opening || layout != self.watch.layout_looked,
			self.watch.pointer == Pointer::Over,
			heard,
		);
		self.watch.check_at = None;
		match drag {
			Drag::Held => {
				self.watch.check_at = Some(now + MONITOR_RECHECK);
				return;
			}
			Drag::MaybeHeld => {
				mondbg("wait", self.watch.key.as_deref(), None, None);
				self.watch.put_off = true;
				return;
			}
			Drag::Over => {}
		}
		let moved_at = self.watch.moved_at.take();
		let now_on = crate::monitor::MonitorId::of_window(&self.window).map(|m| m.key());
		let on_some = now_on.is_some() || self.window.current_monitor().is_some();
		if look_again(on_some, self.watch.positionless, moved_at, now) {
			self.watch.moved_at = moved_at;
			self.watch.check_at = Some(now + MONITOR_RECHECK);
			return;
		}
		self.watch.opening = false;
		self.watch.layout_looked = layout;
		// the way back from fullscreen is no size the user set on arrival
		let pending_at = self
			.pending_size
			.filter(|_| self.watch.noted.users())
			.map(|_| self.pending_size_at);
		let settled = settle(
			self.watch.key.as_deref(),
			now_on.as_deref(),
			moved_at,
			pending_at,
		);
		mondbg(
			if settled == Settle::Stay {
				"stay"
			} else {
				"arrive"
			},
			now_on.as_deref(),
			None,
			None,
		);
		let Settle::Arrive {
			save_first,
			take_size,
		} = settled
		else {
			if self.watch.take_owed {
				self.take_monitor_size();
			}
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
		if self.keeps_size() {
			mondbg("keep", self.watch.key.as_deref(), None, None);
			return;
		}
		// taken on the way back, at the window's own size
		if crate::monitor::fullscreen(&self.window) || self.window.is_maximized() {
			mondbg("later", self.watch.key.as_deref(), None, None);
			self.watch.take_owed = true;
			return;
		}
		self.watch.take_owed = false;
		// another window may have kept a size for this monitor since this one loaded
		config::refresh_window_memory();
		let kept = config::remembered_window(&config::settings(), self.watch.key.as_deref());
		mondbg(
			"take",
			self.watch.key.as_deref(),
			Some((kept.columns, kept.rows)),
			(!self.watch.font_pinned).then_some(kept.font_zoom),
		);
		// The zoom first, since the grid's size in pixels depends on it. Laid
		// out in the new cells only at the new size, by `request_grid`: in the
		// old size the shell was told a grid well off for a moment.
		if !self.watch.font_pinned && kept.font_zoom != config::font_zoom_px() {
			config::set_font_zoom(kept.font_zoom);
			self.retext(config::display_scale(self.window.scale_factor()));
			self.dirty = true;
		}
		let chrome = self.chrome_h();
		let (cols, rows) = kept_grid_here(
			&mut self.text,
			&self.window,
			self.watch.key.as_deref(),
			chrome,
			(kept.columns, kept.rows),
			kept.font_zoom,
		);
		let now = Instant::now();
		self.watch.ignore_resize_until = now + OWN_RESIZE_GRACE;
		self.watch.ignore_moves_until = now + OWN_RESIZE_GRACE;
		self.watch.layout = crate::monitor::layout(&self.window);
		self.request_grid(cols, rows);
	}

	// The grid a window this size shows, with the cells and chrome as they are.
	fn grid_at(&self, w: u32, h: u32) -> (usize, usize) {
		(
			cells_in(w, self.text.cell_w, self.text.margin, 0.0),
			cells_in(h, self.text.cell_h, self.text.margin, self.chrome_h()),
		)
	}

	// The window size that shows this grid, with the cells and chrome as they are.
	fn grid_px(&self, cols: usize, rows: usize) -> (u32, u32) {
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
		fit_px(w, h, max_dim)
	}

	// No monitor's own size is taken: the switches are off, or the command
	// line set the size and the user hasn't resized since.
	fn keeps_size(&self) -> bool {
		let live = config::settings();
		!live.remember_size || !live.remember_per_monitor || self.watch.size_pinned
	}

	/// A new scale keeps the grid where no size kept for a monitor will take
	/// over. Left to the system, the window keeps its size on the desktop, and
	/// a font's cells don't grow in step with the scale, so the grid moved a
	/// few cells. A Wayland window learns its scale after it opens, so it
	/// opened at 78x23 for 80x24 (2026101412003308). Where a kept size will
	/// take over, the window is left to the system: on sway a resize of the
	/// window's own comes back from the compositor late, and undid the take.
	/// Either way the panes are laid out for the size coming, not the old one,
	/// or the shell is told a grid half or twice the size until the resize
	/// arrives (2026101412311921).
	pub(super) fn rescale(&mut self, scale: f32, writer: &mut winit::event::InnerSizeWriter) {
		// a launch size still on its way is the grid that was asked for
		let (w, h) = match self.reveal_want {
			Some(want) if !self.revealed => (want.width, want.height),
			_ => self.surface_px,
		};
		let ratio = scale / self.text.scale;
		let free = !self.no_area
			&& !crate::monitor::fullscreen(&self.window)
			&& !self.window.is_maximized();
		let grid = (free && self.keeps_size()).then(|| self.grid_at(w, h));
		self.retext(scale);
		let mut asked = None;
		if let Some((cols, rows)) = grid {
			let want = self.grid_px(cols, rows);
			let px = winit::dpi::PhysicalSize::new(want.0, want.1);
			if writer.request_inner_size(px).is_ok() {
				asked = Some(want);
			}
		}
		let told = self.window.inner_size();
		let px = asked.unwrap_or_else(|| coming_px((w, h), (told.width, told.height), ratio, free));
		mondbg(
			"rescale",
			self.watch.key.as_deref(),
			grid.filter(|_| asked.is_some()),
			None,
		);
		self.relayout_all_in(px);
	}

	/// Ask for the window size that shows this grid with the chrome as it is,
	/// and lay the panes out, since the cells may have just changed.
	pub(super) fn request_grid(&mut self, cols: usize, rows: usize) {
		let (w, h) = self.grid_px(cols, rows);
		let want = winit::dpi::PhysicalSize::new(w, h);
		// A size the window can honor straight away answers here and sends no
		// `Resized`, so this is the only chance to move everything the window
		// event moves - the scrim included, which was left at the old size.
		if (w, h) != self.surface_px
			&& let Some(applied) = request_size(&self.window, want)
		{
			self.resize_surface(applied.width, applied.height);
			self.invalidate_prepared();
		}
		self.relayout_all();
		self.dirty = true;
	}
}

/// `SILK_MONDBG=1`: the monitor the window counts itself on, and each size it
/// takes or saves there, one line each on stderr.
pub(super) fn mondbg(
	event: &str,
	key: Option<&str>,
	grid: Option<(usize, usize)>,
	zoom: Option<i32>,
) {
	use std::sync::OnceLock;
	static T0: OnceLock<Instant> = OnceLock::new();
	if !super::env_flag(super::EnvFlag::MonDbg) {
		return;
	}
	let t = T0.get_or_init(Instant::now).elapsed().as_secs_f32();
	let grid = grid.map_or_else(|| "-".to_string(), |(c, r)| format!("{c}x{r}"));
	let zoom = zoom.map_or_else(|| "-".to_string(), |z| z.to_string());
	eprintln!(
		"[mon {t:.2}s] {event} key={} grid={grid} zoom={zoom}",
		key.unwrap_or("-")
	);
}

// The size a window left to the system has once a scale change is through.
// Wayland and macOS already answer with it during the change: the same size
// on the desktop at the new scale. Windows and X11 still answer the old one,
// and go on to scale it by the change. A maximized or fullscreen window keeps
// what it's given.
fn coming_px(was: (u32, u32), told: (u32, u32), ratio: f32, free: bool) -> (u32, u32) {
	if told.0 == 0 || told.1 == 0 {
		return was;
	}
	if !free || told != was {
		return told;
	}
	let scaled = |px: u32| ((f64::from(px) * f64::from(ratio)).round() as u32).max(1);
	(scaled(was.0), scaled(was.1))
}

// Whole cells in a window this many pixels across, after the margins either
// side and the chrome; one at least. The inverse of `window_px`.
fn cells_in(px: u32, cell: f32, margin: f32, chrome: f32) -> usize {
	(((px as f32 - 2.0 * margin - chrome) / cell).floor() as i64).max(1) as usize
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

/// What the command line gave that sets the window's size, but for the font
/// options, which `State::cli_style` has. While any of them is left, the
/// window saves nothing of its size: not the size, the font zoom or
/// maximized, for a monitor or as the last size anywhere (2026101422190936).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct CliSize {
	pub(super) columns: bool,  // --columns or --pixel-width
	pub(super) rows: bool,     // --rows or --pixel-height
	pub(super) remember: bool, // --remember-size saves as usual anyway
}

impl CliSize {
	pub(super) fn of(cli: &crate::cli::WindowOpts) -> Self {
		Self {
			columns: cli.columns.is_some() || cli.pixel_width.is_some(),
			rows: cli.rows.is_some() || cli.pixel_height.is_some(),
			remember: cli.remember_size.unwrap_or(false),
		}
	}
}

/// Whether a size the user sets is saved.
pub(super) fn saves_size(size: CliSize, style: &crate::cli::Style) -> bool {
	size.remember
		|| !(size.columns || size.rows || style.font_size.is_some() || style.font_name.is_some())
}

/// A Settings change to a value the command line gave hands that value back,
/// and the window goes on as if it was never given. The font family and size
/// go back once Settings no longer holds them, by a change or the reset
/// arrow, and columns and rows once either is changed. True when the font
/// size went back.
pub(super) fn hand_back(
	size: &mut CliSize,
	style: &mut crate::cli::Style,
	orig: &config::Settings,
	edited: &config::Settings,
) -> bool {
	use crate::ui_spec::Key;
	let let_go = |key: Key| {
		orig.model.values.held.contains_key(key.name())
			&& !edited.model.values.held.contains_key(key.name())
	};
	if style.font_name.is_some() && let_go(Key::FontFamily) {
		style.font_name = None;
	}
	let font_size = style.font_size.is_some() && let_go(Key::FontSize);
	if font_size {
		style.font_size = None;
	}
	// One size in Settings. Columns and rows are set by hand once Remember
	// last size is off, and turning it off keeps the size it showed.
	if !edited.remember_size && (edited.columns, edited.rows) != (orig.columns, orig.rows) {
		size.columns = false;
		size.rows = false;
	}
	font_size
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
	pub(super) layout: Vec<(i64, i64, i64, i64)>, // the monitors when that resize was asked for
	pub(super) layout_looked: Vec<(i64, i64, i64, i64)>, // the monitors at the last look
	// The command line set the size, or the font size, and it stays until
	// the user resizes the window, or zooms the font.
	pub(super) size_pinned: bool,
	pub(super) font_pinned: bool,
	/// Wayland tells a window neither where it is nor that it moved, so the
	/// pointer coming back after a drag is the sign to look.
	pub(super) positionless: bool,
	// What tells a drag held still from one that's over, where the buttons
	// can't be seen (`drag_state`).
	pub(super) pointer: Pointer,
	pub(super) heard_at: Option<Instant>, // the last key, focus or pointer over the window
	pub(super) stirred_at: Option<Instant>, // the last move or scale change
	pub(super) put_off: bool,             // a look waits for `note_presence`
	pub(super) opening: bool,             // the first look, which no drag can be part of
	pub(super) noted: Noted,              // why a size waits to be saved
	pub(super) take_owed: bool,           // arrived while fullscreen or maximized
}

/// What the size waiting to be saved came from, since the last save.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct Noted {
	pub(super) resized: bool,
	pub(super) zoomed: bool,
	/// Fullscreen, maximized or minimized at some point, so a resize may be
	/// the way back.
	pub(super) full: bool,
}

impl Noted {
	fn users(self) -> bool {
		self.zoomed || (self.resized && !self.full)
	}
}

#[derive(Debug, PartialEq)]
struct Keep {
	grid: bool,
	zoom: bool,
	unpin: bool, // the user's resize ends a size from the command line
	back: bool,  // back from fullscreen or maximized
	carry: Noted,
}

// What a save keeps. Going fullscreen or maximized and back are the window
// manager's resizes, not the user's. Taken for one, the way back kept the
// size for the monitor, written over the one kept there, and a zoom saved
// alone while fullscreen made an entry for a monitor nobody sized
// (2026101412392529). A zoom while fullscreen is kept, and its grid once the
// window is back, since the grid shown at the old size has changed.
fn what_to_keep(noted: Noted, free: bool) -> Keep {
	let back = free && noted.full;
	let grid = free && (noted.zoomed || (noted.resized && !back));
	Keep {
		grid,
		zoom: noted.zoomed || grid,
		unpin: grid && noted.resized && !back,
		back,
		carry: Noted {
			resized: false,
			zoomed: !free && noted.zoomed,
			full: !free,
		},
	}
}

/// A size kept at the font a launch without `--font-size` and `--font-name`
/// has, for a window at another font: the same grid if it fits the screen at
/// the font the window has, else the grid that keeps the kept window's size
/// (2026101422190936).
pub(super) fn kept_grid_here(
	text: &mut crate::text::TextCtx,
	window: &Window,
	key: Option<&str>,
	chrome: f32,
	grid: (usize, usize),
	zoom: i32,
) -> (usize, usize) {
	let font = config::unheld_font(zoom);
	if font.same(&config::font_now()) {
		return grid;
	}
	let here = grid_from_font(
		grid,
		text.cell_in(&font),
		(text.cell_w, text.cell_h),
		text.margin,
		chrome,
		room_on(window, key),
	);
	mondbg("atfont", key, Some(here), Some(zoom));
	here
}

// A grid kept at the cell `was`, for a window at the cell `cell`.
fn grid_from_font(
	grid: (usize, usize),
	was: (f32, f32),
	cell: (f32, f32),
	margin: f32,
	chrome: f32,
	room: Option<(u32, u32)>,
) -> (usize, usize) {
	let px = window_px(grid.0, grid.1, was.0, was.1, margin, chrome);
	grid_in_font(grid, px, cell, margin, chrome, room)
}

// The screen of the monitor a size is kept for, which on Wayland
// `current_monitor` may not be, less the window's frame.
fn room_on(window: &Window, key: Option<&str>) -> Option<(u32, u32)> {
	let (w, h) = key
		.and_then(crate::monitor::MonitorId::from_key)
		.map(|m| (m.width, m.height))
		.or_else(|| {
			let size = window.current_monitor()?.size();
			Some((size.width, size.height))
		})?;
	let frame = window.outer_size();
	let inner = window.inner_size();
	Some((
		w.saturating_sub(frame.width.saturating_sub(inner.width)),
		h.saturating_sub(frame.height.saturating_sub(inner.height)),
	))
}

// A size set at one font and used at another is kept in columns and rows if
// the window would fit the screen at the font it opens at, else in pixels.
// The store has only columns and rows, so pixels are the grid that font
// shows in this window, the way a zoom keeps the size.
fn grid_in_font(
	grid: (usize, usize),
	px: (u32, u32),
	cell: (f32, f32),
	margin: f32,
	chrome: f32,
	room: Option<(u32, u32)>,
) -> (usize, usize) {
	let (w, h) = window_px(grid.0, grid.1, cell.0, cell.1, margin, chrome);
	if room.is_some_and(|(room_w, room_h)| w <= room_w && h <= room_h) {
		return grid;
	}
	(
		cells_in(px.0, cell.0, margin, 0.0),
		cells_in(px.1, cell.1, margin, chrome),
	)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Pointer {
	Away,
	Entered(Option<(f64, f64)>), // where the first move after it said it was
	Over,
}

// Xwayland's own idea of where the pointer is goes stale while the compositor
// has it, and a window raised or moved over that spot is told the pointer
// entered, mid-drag too, with one move to that spot and none after. So on X11
// the pointer is over the window once it moves inside it.
fn pointer_moved(pointer: Pointer, at: (f64, f64)) -> Pointer {
	match pointer {
		Pointer::Entered(None) => Pointer::Entered(Some(at)),
		Pointer::Entered(Some(first)) if first != at => Pointer::Over,
		other => other,
	}
}

#[derive(Debug, PartialEq)]
enum Drag {
	Held,
	MaybeHeld,
	Over,
}

// Could the window still be held in a drag? Where the buttons can be seen
// they say. Wayland and Xwayland show a window none held on the compositor's
// title bar, and nothing at all comes while a drag is held still, so looking
// once things went quiet took a monitor's size before the drop
// (2026101411555009). There the pointer over the window, or a key or focus
// since the last move, says the person has let go. A keyboard move ends on a
// modifier coming up, so it isn't held up. A move nobody touches the window
// after waits until they do. Except the first look, or one after the monitors
// changed: that's the window manager herding windows as one goes dark or
// comes back, and with nobody at the window the wait never ended, so it kept
// the size of the monitor it left.
fn drag_state(buttons: Option<bool>, unheld: bool, pointer_in: bool, heard: bool) -> Drag {
	match buttons {
		Some(true) => Drag::Held,
		Some(false) => Drag::Over,
		None if unheld || pointer_in || heard => Drag::Over,
		None => Drag::MaybeHeld,
	}
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
	if kept_for.is_some_and(|kept| crate::monitor::same_monitor(kept, now_on)) {
		return Settle::Stay;
	}
	let save_first = matches!((pending_at, moved_at), (Some(set), Some(moved)) if set < moved);
	Settle::Arrive {
		save_first,
		take_size: pending_at.is_none() || save_first,
	}
}

// Look again soon? A Wayland window is told which output it is on only once
// its first frame is up there, which can come seconds after the window was
// told its scale. Looking once 0.75 s after the window showed found no output
// and gave up, so the window never took its monitor's size.
fn look_again(on_some: bool, positionless: bool, moved_at: Option<Instant>, now: Instant) -> bool {
	!on_some && positionless && moved_at.is_some_and(|at| now < at + NO_OUTPUT_YET)
}

// Does a move start the wait to see where the window ended up? Not one in
// the grace after the window's own resize, which the window manager may move
// it for, so a window that straddles two monitors does not swap sizes back
// and forth. A scale change counts as a move here too. On Wayland it's the
// only sign of one, and a take there flipped which output most of the window
// was on, and so its scale, again and again (2026101420262300). Unless the
// monitors changed in the meantime: then the window manager is herding
// windows as one goes dark or comes back after a power save, and the window
// may be back where it started (2026100907341816).
fn move_counts(
	in_grace: bool,
	layout_then: &[(i64, i64, i64, i64)],
	layout_now: impl FnOnce() -> Vec<(i64, i64, i64, i64)>,
) -> bool {
	!in_grace || layout_now() != layout_then
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
		Settle, cells_in, coming_px, fit_px, launch_maximized, look_again, move_counts,
		remember_resize, resize_is_current, settle, size_taken, window_px,
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

	// A scale change keeps the grid by reading it off the window at the old
	// cells and asking for it in the new ones, so the two have to agree for any
	// cell size, or the grid drifts a cell per change. Cells here are the odd
	// fractions a font gives at 125%, 150% and 200%, and a small font with a
	// margin wider than its cell.
	// Test ID: Esiq2QP
	#[test]
	fn a_grid_reads_back_from_the_window_it_asked_for() {
		for (cell_w, cell_h, margin, chrome) in [
			(10.0, 21.0, 4.0, 0.0),
			(6.0, 13.0, 9.0, 20.0),
			(12.578_125, 26.25, 5.0, 31.0),
			(15.09375, 31.5, 6.0, 47.0),
			(20.125, 42.0, 8.0, 62.5),
		] {
			for (cols, rows) in [(1, 1), (80, 24), (111, 37), (300, 90)] {
				let (w, h) = window_px(cols, rows, cell_w, cell_h, margin, chrome);
				assert_eq!(
					(
						cells_in(w, cell_w, margin, 0.0),
						cells_in(h, cell_h, margin, chrome)
					),
					(cols, rows),
					"{cell_w}x{cell_h}"
				);
			}
		}
		assert_eq!(cells_in(0, 10.0, 4.0, 30.0), 1, "never no cells");
	}

	// Panes laid out for the old size in the new cells told the shell a grid
	// half or twice the size until the resize came. Laid out for this size, a
	// window left to the system is told its new grid once.
	// Test ID: Esisgw4
	#[test]
	fn a_scale_change_lays_out_for_the_size_on_its_way() {
		// Wayland and macOS: the same size on the desktop, already at 1.5
		assert_eq!(coming_px((860, 578), (1290, 867), 1.5, true), (1290, 867));
		// Windows and X11 still say the old size, and scale it next
		assert_eq!(coming_px((860, 578), (860, 578), 1.5, true), (1290, 867));
		assert_eq!(coming_px((1720, 1156), (1720, 1156), 0.5, true), (860, 578));
		// maximized keeps what it has
		assert_eq!(
			coming_px((1920, 1050), (1920, 1050), 2.0, false),
			(1920, 1050)
		);
		assert_eq!(
			coming_px((860, 578), (0, 0), 1.5, true),
			(860, 578),
			"minimized"
		);
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

	// The order of a wake from power save, on b23's two monitors: the portrait
	// one goes dark, the window manager herds the window onto the landscape one,
	// which sizes it, and the portrait one comes back about a second later and
	// the window is put back. That move fell in the grace after the window's own
	// resize and was dropped, so it stayed at the landscape size. The portrait
	// monitor also read back to the cm, which named a monitor with another size.
	// Test ID: EsDZDNJ
	#[test]
	fn a_window_herded_off_a_monitor_and_back_takes_its_size_again() {
		use crate::config::{KeptWindow, MonitorSize, Settings, remembered_window};
		let t0 = Instant::now();
		let at = |ms| Some(t0 + Duration::from_millis(ms));
		let (portrait, landscape) = ((0, 0, 1600, 2560), (1600, 554, 3440, 1440));
		let (both, one) = (vec![portrait, landscape], vec![landscape]);
		let (p_mm, p_cm) = ("1600x2560_100pct_401x641mm", "1600x2560_100pct_400x640mm");
		let l = "3440x1440_100pct_819x346mm";
		let s = Settings {
			monitor_sizes: vec![
				MonitorSize {
					key: p_cm.to_string(),
					columns: 152,
					rows: 28,
					font_zoom: -1,
				},
				MonitorSize {
					key: l.to_string(),
					columns: 172,
					rows: 39,
					font_zoom: 0,
				},
			],
			..Settings::default()
		};
		let arrive = Settle::Arrive {
			save_first: false,
			take_size: true,
		};

		// herded onto the landscape monitor, outside any grace
		assert!(move_counts(false, &[], || one.clone()));
		assert_eq!(settle(Some(p_cm), Some(l), at(0), None), arrive);
		// its own resize there may move it too, and that is not counted
		let layout_then = one.clone();
		assert!(!move_counts(true, &layout_then, || one.clone()));
		// put back 1.2 s in, inside the grace, with the portrait monitor lit
		assert!(move_counts(true, &layout_then, || both.clone()));
		assert_eq!(settle(Some(l), Some(p_mm), at(1200), None), arrive);
		assert_eq!(
			remembered_window(&s, Some(p_mm)),
			KeptWindow {
				columns: 152,
				rows: 28,
				font_zoom: -1
			}
		);
		// read to the cm again without a move: the same monitor, so no resize
		assert_eq!(settle(Some(p_mm), Some(p_cm), at(5000), None), Settle::Stay);
	}

	// A Wayland window shown with its first frame still being drawn is on no
	// output yet. It looks again until it is, and only for a while.
	// Test ID: EshcYxg
	#[test]
	fn a_wayland_window_on_no_output_yet_looks_again() {
		let t0 = Instant::now();
		let later = |ms| t0 + Duration::from_millis(ms);
		assert!(look_again(false, true, Some(t0), later(750)));
		assert!(look_again(false, true, Some(t0), later(9_000)));
		assert!(!look_again(false, true, Some(t0), later(10_000)), "gave up");
		assert!(!look_again(true, true, Some(t0), later(750)), "on one");
		assert!(!look_again(false, false, Some(t0), later(750)), "X11 says");
		assert!(!look_again(false, true, None, later(750)));
	}

	// A window that can't see the buttons took a monitor's size while held
	// still in a drag over it, before the drop. It waits for the pointer back
	// over it, a key or focus. One that can see them goes by them. A window
	// herded off a monitor that went dark waited for nobody.
	// Test ID: EsjYipm
	#[test]
	fn a_drag_held_still_is_not_taken_for_a_drop() {
		use super::{Drag, drag_state};
		assert_eq!(drag_state(None, false, false, false), Drag::MaybeHeld);
		assert_eq!(
			drag_state(None, false, true, false),
			Drag::Over,
			"pointer back"
		);
		assert_eq!(drag_state(None, false, false, true), Drag::Over, "key");
		assert_eq!(
			drag_state(None, true, false, false),
			Drag::Over,
			"opening or herded"
		);
		assert_eq!(drag_state(Some(true), false, true, true), Drag::Held);
		assert_eq!(drag_state(Some(false), false, false, false), Drag::Over);
	}

	// Under Xwayland the press on the compositor's title bar told the window
	// the pointer entered, at a spot it had left long before, so the drag
	// counted as over. A pointer moved there moves again.
	// Test ID: Esjcgsw
	#[test]
	fn a_stale_enter_is_not_the_pointer_back() {
		use super::{Pointer, pointer_moved};
		let entered = pointer_moved(Pointer::Entered(None), (1472.0, 463.0));
		assert_eq!(entered, Pointer::Entered(Some((1472.0, 463.0))));
		assert_eq!(
			pointer_moved(entered, (1472.0, 463.0)),
			entered,
			"same spot"
		);
		assert_eq!(pointer_moved(entered, (1473.0, 463.0)), Pointer::Over);
		assert_eq!(
			pointer_moved(Pointer::Away, (5.0, 5.0)),
			Pointer::Away,
			"focus says where"
		);
		assert_eq!(pointer_moved(Pointer::Over, (5.0, 5.0)), Pointer::Over);
	}

	// Fullscreen and back, or maximized and back, are the window manager's
	// resizes. The way back was saved as the user's size, over the one kept
	// for the monitor, and a zoom saved alone while fullscreen made an entry
	// for a monitor nobody sized.
	// Test ID: EsjoVV7
	#[test]
	fn fullscreen_and_back_saves_nothing() {
		use super::{Noted, what_to_keep};
		let resized = Noted {
			resized: true,
			..Noted::default()
		};
		let by_hand = what_to_keep(resized, true);
		assert!(by_hand.grid && by_hand.zoom && by_hand.unpin && !by_hand.back);
		assert!(resized.users());

		let full = what_to_keep(resized, false);
		assert!(!full.grid && !full.zoom && !full.unpin, "{full:?}");
		let way_back = Noted {
			resized: true,
			..full.carry
		};
		assert!(!way_back.users());
		let back = what_to_keep(way_back, true);
		assert!(
			!back.grid && !back.zoom && !back.unpin && back.back,
			"{back:?}"
		);
		assert_eq!(
			back.carry,
			Noted::default(),
			"the next resize is the user's"
		);

		// F11 twice before a save
		let twice = Noted {
			resized: true,
			full: true,
			zoomed: false,
		};
		assert!(!what_to_keep(twice, true).grid);

		// a zoom while fullscreen keeps the zoom, and the grid once back
		let zoomed = what_to_keep(
			Noted {
				zoomed: true,
				..Noted::default()
			},
			false,
		);
		assert!(!zoomed.grid && zoomed.zoom);
		let back = what_to_keep(
			Noted {
				resized: true,
				..zoomed.carry
			},
			true,
		);
		assert!(
			back.grid && back.zoom && !back.unpin && back.back,
			"{back:?}"
		);
	}

	// A size set with --font-size 11 opened at the normal font next time, in
	// the same columns and rows, far larger and wider than the monitor. It's
	// kept in columns and rows when they'd fit the screen at the normal font,
	// and else as the size the window is now.
	// Test ID: EsjoVZu
	#[test]
	fn a_size_set_at_a_font_from_the_command_line_fits_at_the_normal_font() {
		use super::grid_in_font;
		let (margin, chrome) = (4.0, 30.0);
		let (small, normal) = ((8.0, 17.0), (10.0, 21.0));
		let screen = Some((1920, 1050));
		let px = |grid: (usize, usize)| window_px(grid.0, grid.1, small.0, small.1, margin, chrome);

		let fits = (100, 30);
		assert_eq!(
			grid_in_font(fits, px(fits), normal, margin, chrome, screen),
			fits
		);
		// 1508x1088 at the normal font: the window's 1208x888 instead
		let too_big = (150, 50);
		assert_eq!(px(too_big), (1208, 888));
		let kept = grid_in_font(too_big, px(too_big), normal, margin, chrome, screen);
		assert_eq!(kept, (120, 40));
		let (w, h) = window_px(kept.0, kept.1, normal.0, normal.1, margin, chrome);
		assert!(w <= 1208 && h <= 888);
		assert_eq!(
			grid_in_font(fits, px(fits), normal, margin, chrome, None),
			(80, 24),
			"no screen to fit"
		);
		// a bigger font on the command line comes back smaller, and fits
		let big = |grid: (usize, usize)| window_px(grid.0, grid.1, 14.0, 29.0, margin, chrome);
		let wide = (120, 40);
		assert_eq!(big(wide), (1688, 1198), "past the screen's height");
		assert_eq!(
			grid_in_font(wide, big(wide), normal, margin, chrome, screen),
			wide
		);
	}

	// A size kept at the normal font opened in the same columns and rows at a
	// font from the command line, much larger or smaller than it was kept,
	// and could be wider than the monitor. It keeps its columns and rows where
	// they fit the screen at that font, and else the kept window's size.
	// Test ID: Eso2weD
	#[test]
	fn a_size_kept_at_the_normal_font_fits_at_a_font_from_the_command_line() {
		use super::grid_from_font;
		let (margin, chrome) = (4.0, 30.0);
		let normal = (9.9, 22.0);
		let screen = Some((1920, 1050));
		let kept = (100, 30);
		assert_eq!(
			window_px(kept.0, kept.1, normal.0, normal.1, margin, chrome),
			(998, 698)
		);
		let open =
			|cell: (f32, f32), room| grid_from_font(kept, normal, cell, margin, chrome, room);
		// --font-size 14 fits as it is, and so does a wider --font-name
		assert_eq!(open((11.5, 26.0), screen), kept);
		assert_eq!(open((12.0, 22.0), screen), kept);
		// --font-size 28 would be 2308 px wide, so it keeps the kept window's size
		let big = (23.0, 51.0);
		let got = open(big, screen);
		assert_eq!(got, (43, 12));
		let (w, h) = window_px(got.0, got.1, big.0, big.1, margin, chrome);
		assert!(w <= 998 && h <= 698, "{w}x{h}");
		// a smaller font fits, and keeps the grid rather than the pixels
		assert_eq!(open((7.0, 16.0), screen), kept);
		assert_eq!(open(normal, screen), kept, "the same font");
		assert_eq!(open(normal, None), kept, "the same font, no screen");
	}

	// A window opened with a size or a font from the command line saved the
	// size the user set, and a launch without them opened at it in another
	// font. Such a window saves nothing until each of those is changed in
	// Settings, unless --remember-size asks for the usual saves.
	// Test ID: EsthbIT
	#[test]
	fn a_size_or_font_from_the_command_line_saves_nothing_until_changed_in_settings() {
		use super::{CliSize, hand_back, saves_size};
		use crate::config::Settings;
		use crate::fields;
		use crate::ui_spec::Key;
		use knobs::Value;
		let cli = |line: &str| {
			crate::cli::parse(line.split_whitespace().map(String::from))
				.unwrap()
				.win
		};
		let saves = |line: &str| {
			let win = cli(line);
			saves_size(CliSize::of(&win), &win.style)
		};
		for line in [
			"--columns 90",
			"--rows 30",
			"--pixel-width 800",
			"--pixel-height 600",
			"--font-size 14",
			"--font-name Mono",
		] {
			assert!(!saves(line), "{line}");
			assert!(saves(&format!("{line} --remember-size")), "{line}");
			assert!(!saves(&format!("{line} --remember-size=no")), "{line}");
		}
		assert!(saves(""));
		assert!(saves("--fullscreen --background-color #102030 --title T"));
		assert!(saves("--new-tab --font-size 14"), "a tab's font isn't used");

		let win = cli("--font-size 14 --font-name Mono --columns 90 --rows 30");
		let (mut size, mut style) = (CliSize::of(&win), win.style.clone());
		let mut opened = Settings::default();
		crate::cli::fold_window_style(&mut opened, &style);
		let set = |key: Key, v: Value| {
			let mut edited = opened.clone();
			fields::set(&mut edited, key, &v, None);
			edited
		};
		macro_rules! back {
			($edited:expr) => {
				hand_back(&mut size, &mut style, &opened, &$edited)
			};
		}
		assert!(
			!back!(set(Key::Margin, Value::Float(7.0))),
			"another setting"
		);
		assert!(!back!(opened), "nothing changed");
		assert!(back!(set(Key::FontSize, Value::Float(16.0))));
		let mut reset = opened.clone();
		fields::reset(&mut reset, Key::FontFamily, None);
		assert!(!back!(reset), "the arrow, on the family");
		assert_eq!((style.font_size, style.font_name.as_deref()), (None, None));
		assert!(!back!(set(Key::RememberSize, Value::Bool(false))));
		assert!(
			!saves_size(size, &style),
			"the switch over the size isn't it"
		);
		assert!(!back!(set(Key::Rows, Value::Int(40))));
		assert!(saves_size(size, &style), "{size:?} {style:?}");
		size = CliSize::of(&cli("--columns 90"));
		assert!(!back!(set(Key::Columns, Value::Int(100))));
		assert!(saves_size(size, &style), "{size:?}");
		size = CliSize::of(&cli("--rows 20"));
		let by_hand = set(Key::Rows, Value::Int(25));
		assert!(!by_hand.remember_size);
		let mut on = by_hand.clone();
		fields::set(&mut on, Key::RememberSize, &Value::Bool(true), None);
		assert!(!hand_back(&mut size, &mut style, &by_hand, &on));
		assert!(!saves_size(size, &style), "nor turning it back on");
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
