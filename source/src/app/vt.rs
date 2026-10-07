// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The virtual console watcher. A return to this console rebuilds the GPU side
//! twice, once straight away and again after the display settles.

use crate::term::UserEvent;
use std::time::{Duration, Instant};
use winit::event_loop::EventLoopProxy;

// After a return to this console, how long until the second heal (VtHeal).
const VT_SETTLE: Duration = Duration::from_secs(3);

/// VT-switch field diagnostics: `touch ~/silk_vramdbg.on` (no relaunch needed)
/// makes the sentinel probes append their results to `~/silk_vramdbg.txt`, so a
/// desktop repro can show whether loss detection fired. The marker is re-checked
/// per call - probes tick every 2s, so the stat costs nothing.
pub(super) fn vramdbg(msg: &str) {
	use std::io::Write;
	let Some(home) = std::env::var_os("HOME") else {
		return;
	};
	let home = std::path::PathBuf::from(home);
	if !home.join("silk_vramdbg.on").exists() {
		return;
	}
	let path = home.join("silk_vramdbg.txt");
	// a forgotten marker must not grow the log unbounded
	if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 4_000_000) {
		return;
	}
	let epoch = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_secs());
	if let Ok(mut f) = std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(&path)
	{
		let _ = writeln!(f, "{epoch} pid={} {msg}", std::process::id());
	}
}

/// A return to this console is healed twice: at once, and again once the X
/// server has had time to take the display back. The watcher sees the console
/// change before the mode set, so a purge that comes after the first rebuild
/// would spoil it too.
#[derive(Debug, Default)]
pub(super) struct VtHeal {
	pub(super) again: Option<Instant>,
}

impl VtHeal {
	pub(super) fn returned(&mut self, now: Instant) {
		self.again = Some(now + VT_SETTLE);
	}

	/// True once, when the second heal comes due.
	pub(super) fn due(&mut self, now: Instant) -> bool {
		if self.again.is_some_and(|at| now >= at) {
			self.again = None;
			return true;
		}
		false
	}
}

/// Watch the active virtual console (/sys/class/tty/tty0/active). A VT switch
/// away and back breaks sampling of long-lived textures in ways the readback
/// probes cannot see (field logs: every witness read back intact across a switch
/// that blacked the window - the driver restores readback contents while the
/// sampled copies stay garbage). So detect the switch itself: the value at spawn
/// is the console this display lives on; when the file returns to it after being
/// elsewhere, send `VtSwitched` so the sampled textures are rebuilt. Only returns
/// are signaled - a rebuild done while parked on another console could itself be
/// purged on the way back. `SILK_VTFILE` overrides the watched path so a headless
/// test can drive the mechanism (Xvfb has no VTs).
#[cfg(target_os = "linux")]
pub(super) fn spawn_vt_watch(proxy: EventLoopProxy<UserEvent>) -> bool {
	let path = std::env::var_os("SILK_VTFILE").map_or_else(
		|| std::path::PathBuf::from("/sys/class/tty/tty0/active"),
		std::path::PathBuf::from,
	);
	let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().map(|s| s.trim().to_owned());
	// unreadable (container, odd kernel) -> no watcher; probes remain as fallback
	let Some(home_vt) = read(&path) else {
		return false;
	};
	std::thread::spawn(move || {
		let mut watch = VtWatch::new(home_vt);
		loop {
			std::thread::sleep(Duration::from_millis(500));
			let Some(cur) = read(&path) else {
				continue;
			};
			if cur != watch.last {
				vramdbg(&format!("vt switch: {} -> {cur}", watch.last));
			}
			if watch.step(cur) && proxy.send_event(UserEvent::VtSwitched).is_err() {
				return; // event loop gone - exit with the app
			}
		}
	});
	true
}

// The console this display lives on, and the one seen last.
#[cfg(any(target_os = "linux", test))]
struct VtWatch {
	home: String,
	last: String,
}

#[cfg(any(target_os = "linux", test))]
impl VtWatch {
	fn new(home: String) -> Self {
		let last = home.clone();
		Self { home, last }
	}

	// Take the console now active; true only when that is a return home.
	fn step(&mut self, cur: String) -> bool {
		let returned = cur == self.home && self.last != self.home;
		self.last = cur;
		returned
	}
}

#[cfg(not(target_os = "linux"))]
pub(super) fn spawn_vt_watch(_proxy: EventLoopProxy<UserEvent>) -> bool {
	false
}

#[cfg(test)]
mod tests {
	use super::{VT_SETTLE, VtHeal};
	use std::time::Instant;
	// A return to this console is healed at once and once more when the X
	// server has settled, since a purge after the first rebuild spoiled it
	// (20260917: text gone and a gray background after a switch to VT 1).
	// Test ID: EqBP7iS
	#[test]
	fn a_return_to_this_console_is_healed_again_once_settled() {
		let back = Instant::now();
		let mut heal = VtHeal::default();
		assert!(!heal.due(back), "nothing owed before a switch");
		heal.returned(back);
		assert!(!heal.due(back), "the first heal is the event's own");
		assert!(!heal.due(back + VT_SETTLE / 2));
		assert!(heal.due(back + VT_SETTLE), "the second heal");
		assert!(!heal.due(back + VT_SETTLE * 2), "and only once");
		// a second switch before the first settled pushes the pass out
		heal.returned(back);
		heal.returned(back + VT_SETTLE / 2);
		assert!(!heal.due(back + VT_SETTLE));
		assert!(heal.due(back + VT_SETTLE + VT_SETTLE / 2));
	}

	// Test ID: Er2UiYV
	#[test]
	fn only_a_return_to_this_console_is_signalled() {
		let mut watch = super::VtWatch::new("tty7".into());
		let seen: Vec<bool> = ["tty7", "tty1", "tty7", "tty7", "tty2", "tty3", "tty7"]
			.into_iter()
			.map(|vt| watch.step(vt.into()))
			.collect();
		assert_eq!(seen, [false, false, true, false, false, false, true]);
	}
}
