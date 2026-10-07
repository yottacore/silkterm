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
// answers with no picture at all: a configured folder owns the wallpaper and
// suppresses the built-in stand-in, so there is nothing left to show. A
// command-line wallpaper is exempt, since it keeps rotation out of the session.
fn needs_folder_read(
	locked: bool,
	showing: Option<&std::path::Path>,
	folder: Option<&std::path::PathBuf>,
) -> bool {
	!locked && showing.is_none() && folder.is_some()
}

/// Whether rotation has anywhere to go: not held by a command-line wallpaper,
/// and the last scan found more than the one image showing.
pub(super) fn rotation_live(locked: bool, count: usize, folder: bool) -> bool {
	!locked && count >= 2 && folder
}

// Where the rotation timer goes when a tick fires. It has to move off `now`
// here rather than waiting for the worker's answer: the answer is dropped
// unless it is still the newest request, and a timer left in the past fires
// again on the very next pass, so each pass started another decode thread.
fn rotation_next(now: Instant, live: bool, interval_s: f32) -> Option<Instant> {
	(live && interval_s > 0.0).then(|| now + Duration::from_secs_f32(interval_s))
}

impl State {
	// Control-socket wallpaper change: live-only and window-scoped, like the
	// launch-time --background-image - nothing is persisted to config.shcl.
	fn set_wallpaper(&mut self, image: Option<std::path::PathBuf>) {
		let orig = config::settings().as_ref().clone();
		let mut edited = orig.clone();
		config::name_wallpaper(&mut edited, image);
		self.apply_new_settings(&orig, edited, true);
	}

	/// Hand the wallpaper to a worker thread and carry on drawing. `scan` also
	/// (re)reads the rotation folder and picks from it. Nothing here waits: the
	/// folder, the image and its tags can all live on a share that answers slowly,
	/// which is precisely why none of it runs on this thread.
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
		// retires anything already in flight - a result arriving after a newer
		// request (a rotation tick overtaken by a settings change) is dropped, and
		// the worker sees the new stamp and stops at its next stage
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

	/// Wallpaper rotation: unless a wallpaper came in on the command line (a
	/// deliberate choice for this session, which leaves rotation out of it
	/// entirely), scan the folder and pick one. The timer arms when the scan
	/// answers - only then do we know whether there is anything to rotate through.
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
		// timer, or brings the tick back if it was not a rotation itself.
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
			// a scan is authoritative about rotation: no pick means the folder holds
			// nothing (or went away), so the timer goes with it
			self.wp_count = 0;
			self.wp_current = None;
			self.wp_next = None;
			if let Some(rot) = &loaded.rotation {
				self.wp_count = rot.count;
				self.wp_current = Some(rot.current.clone());
				// live-only, like a --wallpaper-file: the dialog shows what is on
				// screen, and nothing about the pick reaches config.shcl
				set_live(|live| {
					live.wallpaper_raw = rot.current.to_string_lossy().into_owned();
					live.wallpaper = Some(rot.current.clone());
				});
				let ivl = config::settings().wallpaper_rotate_interval_s;
				self.wp_next = (ivl > 0.0 && rot.count > 1)
					.then(|| Instant::now() + Duration::from_secs_f32(ivl));
			}
		}
		// The derived text and cursor colors follow the picture (autotheme.rs), so
		// what this one is worth goes live with it. Session state: none of it
		// reaches the file, the same as a rotated pick.
		let summary = loaded.image.as_ref().map(|img| img.summary);
		if config::settings().wallpaper_summary != summary {
			set_live(|live| live.wallpaper_summary = summary);
			// the text is a different color now, so nothing retained is good
			self.invalidate_prepared();
			self.chrome = None;
		}
		// A window without a device drops the pixels: the rebuild asks for the
		// wallpaper again, and decoding it twice beats holding a copy of it.
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
		// Answered either way: an empty result is the news that there is no
		// wallpaper to wait for, which settles the question just as well.
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
		// rotation is done for this session, so drop what it was showing - otherwise
		// an explicit clear would fall back to it instead of clearing
		self.wp_current = None;
		self.set_wallpaper(image);
	}
}

#[cfg(test)]
mod tests {
	use super::{needs_folder_read, rotation_next};
	use std::time::Instant;
	// Switching the wallpaper off and on again drops the rotation pick, and a
	// request that does not re-read the folder then answers with nothing at all:
	// the folder suppresses the built-in, and there is no pick to fall back on.
	// Test ID: Ep1Y6Tg
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
	// Test ID: EpHNMfQ
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
