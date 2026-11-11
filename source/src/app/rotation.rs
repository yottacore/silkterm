// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The wallpaper: asking the worker for one, taking its answer, and moving on
//! through a rotation folder.

use super::idle::{Conserve, idledbg};
use super::{State, WP_RESIZE_WAIT, set_live};
use crate::bgimage::ImageRenderer;
use crate::config;
use std::time::{Duration, Instant};

// A rotation folder with nothing picked from it has to be read, or the request
// comes back with no picture, since a configured folder replaces the built-in
// stand-in. A command-line wallpaper keeps rotation out of the session, so it's
// exempt.
fn needs_folder_read(
	locked: bool,
	showing: Option<&std::path::Path>,
	folder: Option<&std::path::PathBuf>,
) -> bool {
	!locked && showing.is_none() && folder.is_some()
}

/// Whether a settings change picks a new picture from the folder: rotation
/// turned back on, since the pick and its timer went with it on the way off,
/// or another folder. The same folder keeps what it showed.
pub(super) fn picks_again(before: &config::Settings, after: &config::Settings) -> bool {
	after.rotation_folder().is_some() && after.rotation_folder() != before.rotation_folder()
}

/// Whether rotation has anywhere to go: not held by a command-line wallpaper,
/// and the last scan found more than the one image showing.
pub(super) fn rotation_live(locked: bool, count: usize, folder: bool) -> bool {
	!locked && count >= 2 && folder
}

// Where the rotation timer goes when a tick fires. It moves off `now` here rather
// than waiting for the worker, since the worker's result is dropped unless it's
// still the newest request. A timer left in the past fired again on every pass,
// and each pass started another decode thread.
fn rotation_next(now: Instant, live: bool, interval_s: f32) -> Option<Instant> {
	(live && interval_s > 0.0).then(|| now + Duration::from_secs_f32(interval_s))
}

impl State {
	// A wallpaper change from the control socket. Like --background-image at launch,
	// it's live only and for this window, and nothing is written to config.shcl.
	fn set_wallpaper(&mut self, image: Option<std::path::PathBuf>) {
		let orig = config::settings().as_ref().clone();
		let mut edited = orig.clone();
		config::name_wallpaper(&mut edited, image);
		self.apply_new_settings(&orig, edited, true);
	}

	/// Hand the wallpaper to a worker thread and carry on drawing. `scan` also reads
	/// the rotation folder again and picks from it. The folder, the image, and its tags
	/// can all be on a slow share, so none of it runs on this thread.
	pub(super) fn request_wallpaper(&mut self, scan: bool) {
		self.post_wallpaper(scan, None);
	}

	// `summary` is set when this only re-sizes the picture showing now.
	fn post_wallpaper(&mut self, scan: bool, summary: Option<crate::autotheme::Summary>) {
		let settings = config::settings();
		let scan = scan
			|| needs_folder_read(
				self.wp_locked,
				self.wp_current.as_deref(),
				settings.rotation_folder(),
			);
		// a scan can pick another picture, which needs its own summary
		let summary = summary.filter(|_| !scan);
		self.wp_resize_at = None;
		// a lock that names nothing is a bare flag asking for no picture
		let cleared = self.wp_locked && settings.wallpaper.is_none();
		// the new stamp retires anything already in flight. A result that comes in after
		// a newer request is dropped, and the worker sees the new stamp and stops at its
		// next stage
		let seq = self
			.wp_seq
			.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
			.wrapping_add(1);
		self.wp_pacing.sent(seq);
		crate::wallpaper::spawn(
			&self.proxy,
			crate::wallpaper::Request {
				seq,
				newest: self.wp_seq.clone(),
				settings,
				scan,
				current: self.wp_current.clone(),
				cleared,
				window: self.surface_px,
				summary,
				kept: config::cache_dir().map(|dir| dir.join("wallpaper")),
			},
		);
	}

	/// The window changed size, or a picture arrived prepared for an older one.
	/// Each call pushes the wait back, so a drag prepares it once, at the end.
	pub(super) fn note_wallpaper_size(&mut self) {
		let off_size = self
			.gpu
			.as_ref()
			.and_then(|gpu| gpu.wallpaper_img.as_ref())
			.is_some_and(|img| img.needs_resize(self.surface_px));
		self.wp_resize_at = off_size.then(|| Instant::now() + WP_RESIZE_WAIT);
	}

	/// The resize wait is up. A request still working is left to finish, and its
	/// arrival checks the size again.
	pub(super) fn resize_wallpaper(&mut self) {
		self.wp_resize_at = None;
		if self.wp_pacing.busy() {
			return;
		}
		let held = self
			.gpu
			.as_ref()
			.and_then(|gpu| gpu.wallpaper_img.as_ref())
			.filter(|img| img.needs_resize(self.surface_px));
		if held.is_some() {
			self.post_wallpaper(false, config::settings().wallpaper_summary);
		}
	}

	/// Wallpaper rotation: scan the folder and pick one, unless a wallpaper came in on
	/// the command line, which keeps rotation out of this session. The timer starts
	/// when the scan comes back, since only then is it known whether there's anything
	/// to rotate through.
	pub(super) fn init_wallpaper(&mut self, lock: bool) {
		self.wp_locked = lock;
		self.request_wallpaper(!lock);
	}

	pub(super) fn can_rotate(&self) -> bool {
		rotation_live(
			self.wp_locked,
			self.wp_count,
			config::settings().rotation_folder().is_some(),
		)
	}

	/// Rotate to the next image. The worker re-scans, so images added to or removed
	/// from the folder since launch are picked up. Next wallpaper comes here too,
	/// so the timer starts over from the pick it asked for.
	pub(super) fn advance_wallpaper(&mut self) {
		// locked, switched off since the timer was armed, or one image (or none):
		// nothing to rotate to, so drop the timer
		let settings = config::settings();
		let live = self.can_rotate();
		self.wp_next = rotation_next(Instant::now(), live, settings.wallpaper_rotate_interval_s);
		if !live {
			return;
		}
		// A request still working (an image slower to prepare than the interval,
		// or a folder on a slow share) is left to finish. Its arrival re-arms the
		// timer, or brings the tick back if it wasn't a rotation itself.
		if !self.wp_pacing.tick() {
			self.wp_next = None;
			return;
		}
		self.request_wallpaper(true);
	}

	/// A worker finished; uploading the pixels is all that was left for this thread.
	pub(super) fn wallpaper_ready(&mut self, loaded: crate::wallpaper::Loaded) {
		if loaded.seq != self.wp_seq.load(std::sync::atomic::Ordering::Relaxed) {
			return; // superseded while it was working
		}
		if self.wp_pacing.arrived(loaded.scanned) {
			// a tick fired while this was working: due now, and the poll sends it
			// once there is a device to show it on
			self.wp_next = Some(Instant::now());
		}
		if loaded.scanned {
			// a scan decides rotation. No pick means the folder is empty or gone, so the
			// timer goes with it
			self.wp_count = 0;
			self.wp_current = None;
			self.wp_next = None;
			if let Some(rot) = &loaded.rotation {
				self.wp_count = rot.count;
				self.wp_current = Some(rot.current.clone());
				// live-only: nothing about the pick is written to config.shcl, and the
				// image setting stays automatic, since the folder chose it
				set_live(|live| live.wallpaper = Some(rot.current.clone()));
				let ivl = config::settings().wallpaper_rotate_interval_s;
				self.wp_next = (ivl > 0.0 && rot.count > 1)
					.then(|| Instant::now() + Duration::from_secs_f32(ivl));
			}
		}
		// The derived text and cursor colors follow the picture (autotheme.rs), so this
		// picture's summary goes live with it. None of it is written to the file, the
		// same as a rotated pick.
		let summary = loaded.image.as_ref().map(|img| img.summary);
		if config::settings().wallpaper_summary != summary {
			set_live(|live| {
				live.wallpaper_summary = summary;
				crate::fields::fill(live, None);
			});
			// the text is a different color now, so nothing retained is good
			self.invalidate_prepared();
			self.chrome = None;
		}
		// A window without a device drops the pixels: the rebuild asks for the
		// wallpaper again, and decoding it twice beats keeping a copy of it.
		// The stand-in is kept either way, so it is always the newest picture.
		self.wp_standin = loaded.standin;
		if self.conserve == Conserve::Restoring {
			idledbg("wallpaper prepared again");
		}
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.wallpaper_img = loaded.image.map(|img| {
				ImageRenderer::new(&gpu.gfx.device, &gpu.gfx.queue, gpu.gfx.format, &img)
			});
		}
		// prepared for the size the window had when it was asked for
		self.note_wallpaper_size();
		// Answered either way: an empty result means there's no wallpaper to
		// wait for.
		self.wp_answered = true;
		self.conserve.wallpaper_answered(Instant::now());
		self.update_title();
		self.dirty = true;
	}

	/// A wallpaper set from the command line while running: honor it for the rest
	/// of the session and stop rotating, without touching the stored settings.
	pub(super) fn lock_wallpaper(&mut self, image: Option<std::path::PathBuf>) {
		self.wp_locked = true;
		self.wp_next = None;
		// rotation is done for this session, so drop what it was showing. Otherwise an
		// explicit clear would fall back to it instead of clearing
		self.wp_current = None;
		self.set_wallpaper(image);
	}
}

#[cfg(test)]
mod tests {
	use super::{needs_folder_read, picks_again, rotation_next};
	use std::time::Instant;

	// Another folder picks again; the same folder, or a setting beside it, keeps
	// the picture on screen. Rotation turned back on picks too.
	// Test ID: ddwnh
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn only_a_new_folder_picks_a_new_picture() {
		let at = |folder: &str| crate::config::Settings {
			wallpaper_enabled: true,
			wallpaper_rotate_enabled: true,
			wallpaper_folder: Some(std::path::PathBuf::from(folder)),
			..Default::default()
		};
		let a = at("/w/a");
		let mut dimmer = a.clone();
		dimmer.wallpaper_opacity /= 2.0;
		assert!(!picks_again(&a, &dimmer));
		assert!(picks_again(&a, &at("/w/b")));
		let mut off = a.clone();
		off.wallpaper_rotate_enabled = false;
		assert!(picks_again(&off, &a));
		assert!(!picks_again(&a, &off), "nothing to pick from");
	}
	// Switching the wallpaper off and on again drops the rotation pick, and a
	// request that doesn't re-read the folder then comes back with nothing at all:
	// the folder suppresses the built-in, and there is no pick to fall back on.
	// Test ID: dc7k2
	#[test]
	fn a_rotation_folder_with_nothing_showing_is_read_again() {
		let pick = std::path::PathBuf::from("/w/a.jpg");
		let folder = std::path::PathBuf::from("/w");
		assert!(needs_folder_read(false, None, Some(&folder)));
		// something from the folder is already up, or there is no folder at all
		assert!(!needs_folder_read(false, Some(&pick), Some(&folder)));
		assert!(!needs_folder_read(false, None, None));
		// a command-line wallpaper owns the session, rotation stays out of it
		assert!(!needs_folder_read(true, None, Some(&folder)));
	}

	// A tick that leaves the timer where it was fires again on the next pass, and
	// each of those starts another decode thread.
	// Test ID: dcbcw
	#[test]
	fn a_rotation_tick_moves_the_timer_off_now() {
		let now = Instant::now();
		let next = rotation_next(now, true, 2.0).expect("a live rotation keeps its timer");
		assert!(next > now);
		// nothing to rotate to, or rotation switched off: no timer at all
		assert!(rotation_next(now, false, 2.0).is_none());
		assert!(rotation_next(now, true, 0.0).is_none());
	}
}
