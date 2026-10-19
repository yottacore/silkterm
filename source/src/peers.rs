// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Windows on one settings file find each other here, so a save in one shows
//! in the rest at once. They meet in a folder named for the file,
//! `silkterm-<key>` in the runtime folder. Each process leaves an empty file
//! there named by its pid, and a save holds the `lock` file there while it
//! reads and writes. A window on another file has another key and never hears
//! about it. See design.md, "Settings across windows".

use std::cell::Cell;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// Long enough for any real save to finish, short enough that a window stopped
// in a debugger mid-save costs the others a pause and not a hang.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Where the control sockets and meeting folders go: the session's runtime
/// folder, else the temp folder. A test run keeps its own.
pub fn runtime_dir() -> PathBuf {
	#[cfg(test)]
	return crate::testdir::run_dir().to_path_buf();
	#[cfg(not(test))]
	std::env::var_os("XDG_RUNTIME_DIR")
		.filter(|dir| !dir.is_empty())
		.map_or_else(std::env::temp_dir, PathBuf::from)
}

/// The settings file as a short name. Two spellings of one file get one key,
/// and the key is short enough to leave room in a socket path.
pub fn key_for(path: &Path) -> String {
	let real = real_path(path);
	let text = real.to_string_lossy();
	// NTFS ignores case, so one file can be named many ways
	#[cfg(windows)]
	let text = text.to_lowercase();
	format!("{:016x}", crate::wpcache::fnv(text.as_bytes()))
}

// A file not written yet still sits in a real folder.
fn real_path(path: &Path) -> PathBuf {
	if let Ok(real) = std::fs::canonicalize(path) {
		return real;
	}
	let dir = path
		.parent()
		.and_then(|dir| std::fs::canonicalize(dir).ok());
	match (dir, path.file_name()) {
		(Some(dir), Some(name)) => dir.join(name),
		_ => std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
	}
}

/// This process's key. The settings file is picked once, before any window,
/// so it never changes after.
pub fn own_key() -> Option<&'static str> {
	static KEY: OnceLock<Option<String>> = OnceLock::new();
	KEY.get_or_init(|| crate::config::settings_file().map(|path| key_for(&path)))
		.as_deref()
}

/// The meeting folder for `key`, made if need be. None when it cannot be made
/// or is not ours, and then this window saves and listens alone.
pub fn meeting_dir(key: &str) -> Option<PathBuf> {
	let dir = runtime_dir().join(format!("silkterm-{key}"));
	match make_private_dir(&dir) {
		Ok(()) => Some(dir),
		Err(e) => {
			warn_once(&format!("{}: {e}", dir.display()));
			None
		}
	}
}

#[cfg(unix)]
fn make_private_dir(dir: &Path) -> std::io::Result<()> {
	use std::os::unix::fs::{DirBuilderExt, MetadataExt};
	match std::fs::DirBuilder::new().mode(0o700).create(dir) {
		Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e),
		_ => {}
	}
	// The fallback is a shared /tmp, where someone else can make the folder first.
	let meta = std::fs::symlink_metadata(dir)?;
	// SAFETY: no arguments, cannot fail.
	let uid = unsafe { libc::geteuid() };
	if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
		return Err(std::io::Error::new(
			std::io::ErrorKind::PermissionDenied,
			"not a private folder of this user",
		));
	}
	Ok(())
}

// %TEMP% is the user's own.
#[cfg(not(unix))]
fn make_private_dir(dir: &Path) -> std::io::Result<()> {
	std::fs::create_dir_all(dir)
}

fn warn_once(what: &str) {
	static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
	if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
		eprintln!(
			"{}: other windows on this settings file will not hear of saves: {what}",
			crate::config::APP_NAME
		);
	}
}

/// This process's marker in a meeting folder. Gone with the process on a clean
/// exit. A crash leaves it, and the next save that finds nobody behind it
/// removes it.
#[derive(Debug)]
pub struct Marker(PathBuf);

impl Drop for Marker {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.0);
	}
}

pub fn join(dir: &Path, pid: u32) -> Option<Marker> {
	let path = dir.join(pid.to_string());
	File::create(&path).ok()?;
	Some(Marker(path))
}

/// The pids with a marker in `dir`, this one left out.
pub fn others(dir: &Path, own: u32) -> Vec<u32> {
	let Ok(entries) = std::fs::read_dir(dir) else {
		return Vec::new();
	};
	entries
		.flatten()
		.filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
		.filter(|pid| *pid != own)
		.collect()
}

/// What came of telling one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Told {
	Sent,
	/// Nothing listens there any more: the process is gone.
	Gone,
	/// Busy or refused for some other reason. The marker stays.
	Missed,
}

/// Tell every other window on this settings file that the file changed. Only
/// a folder read when no other window is open.
pub fn tell_others() {
	let Some(key) = own_key() else { return };
	let Some(dir) = meeting_dir(key) else { return };
	let own = std::process::id();
	if others(&dir, own).is_empty() {
		return;
	}
	// a window that hangs mid-connect costs this thread, not the frame
	std::thread::spawn(move || {
		tell_in(&dir, own, key, crate::ctl::poke);
	});
}

/// The same for a given folder and sender, so a test can stand in for both.
pub fn tell_in(
	dir: &Path,
	own: u32,
	key: &str,
	poke: impl Fn(u32, &str) -> Told,
) -> Vec<(u32, Told)> {
	let line = format!("saved\t{key}");
	others(dir, own)
		.into_iter()
		.map(|pid| {
			let told = poke(pid, &line);
			if told == Told::Gone {
				let _ = std::fs::remove_file(dir.join(pid.to_string()));
			}
			(pid, told)
		})
		.collect()
}

thread_local! {
	static HELD: Cell<u32> = const { Cell::new(0) };
}

/// Held while a settings or state file is read and written back, so two
/// windows saving at the same moment each keep the other's change. A take
/// inside one already held on this thread is free.
#[must_use]
#[derive(Debug)]
pub struct SaveLock {
	// held for its drop, which lets the next save in
	#[cfg_attr(not(test), expect(dead_code))]
	file: Option<File>,
}

impl Drop for SaveLock {
	fn drop(&mut self) {
		HELD.with(|held| held.set(held.get().saturating_sub(1)));
	}
}

pub fn save_lock() -> SaveLock {
	let outer = HELD.with(|held| {
		held.set(held.get() + 1);
		held.get() == 1
	});
	let file = outer
		.then(crate::config::settings_file)
		.flatten()
		.and_then(|path| meeting_dir(&key_for(&path)))
		.and_then(|dir| take(&dir.join("lock"), LOCK_WAIT));
	SaveLock { file }
}

// Each take opens the file afresh, so two threads of one process shut each
// other out the same way two processes do.
fn take(path: &Path, wait: Duration) -> Option<File> {
	let file = OpenOptions::new()
		.create(true)
		.truncate(false)
		.write(true)
		.open(path)
		.ok()?;
	let until = Instant::now() + wait;
	loop {
		match file.try_lock() {
			Ok(()) => return Some(file),
			Err(TryLockError::WouldBlock) if Instant::now() < until => {
				std::thread::sleep(Duration::from_millis(5));
			}
			Err(e) => {
				eprintln!(
					"{}: saving without the settings lock ({}: {e})",
					crate::config::APP_NAME,
					path.display()
				);
				return None;
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn scratch(name: &str) -> PathBuf {
		let dir = crate::testdir::run_dir().join(format!("peers_{name}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		dir
	}

	// Test ID: EsaCcC0
	#[test]
	fn one_file_is_one_key_however_it_is_named() {
		let dir = scratch("key");
		let file = dir.join("config.shcl");
		std::fs::write(&file, "").unwrap();
		let other = dir.join("other.shcl");
		let roundabout = dir.join(".").join("config.shcl");
		assert_eq!(key_for(&file), key_for(&roundabout));
		assert_ne!(key_for(&file), key_for(&other), "a file not made yet");
		#[cfg(unix)]
		{
			let link = dir.join("link.shcl");
			std::os::unix::fs::symlink(&file, &link).unwrap();
			assert_eq!(key_for(&file), key_for(&link));
		}
		assert_eq!(key_for(&file).len(), 16);
	}

	// A marker nobody answers for goes; one that answers stays; this process's
	// own is never told.
	// Test ID: EsaCcC1
	#[test]
	fn a_save_reaches_the_others_and_clears_the_dead() {
		let dir = scratch("tell");
		let _me = join(&dir, 10).unwrap();
		let _alive = join(&dir, 11).unwrap();
		// left behind, as a crash leaves it
		File::create(dir.join("12")).unwrap();
		std::fs::write(dir.join("lock"), "").unwrap();
		let heard = std::sync::Mutex::new(Vec::new());
		let mut told = tell_in(&dir, 10, "abc", |pid, line| {
			heard.lock().unwrap().push((pid, line.to_string()));
			if pid == 11 { Told::Sent } else { Told::Gone }
		});
		told.sort_unstable_by_key(|(pid, _)| *pid);
		assert_eq!(told, vec![(11, Told::Sent), (12, Told::Gone)]);
		assert!(
			heard
				.lock()
				.unwrap()
				.iter()
				.all(|(_, line)| line == "saved\tabc")
		);
		assert!(dir.join("11").exists());
		assert!(!dir.join("12").exists(), "the dead one's marker stays");
		assert!(dir.join("10").exists() && dir.join("lock").exists());
	}

	// Test ID: EsaCcC2
	#[test]
	fn a_marker_goes_with_its_window() {
		let dir = scratch("marker");
		let marker = join(&dir, 77).unwrap();
		assert_eq!(others(&dir, 1), vec![77]);
		assert!(others(&dir, 77).is_empty());
		drop(marker);
		assert!(others(&dir, 1).is_empty());
	}

	// The second taker waits for the first, here a thread standing in for the
	// other window, and a take inside a held one does not wait on itself.
	// Test ID: EsaCcC3
	#[test]
	fn a_second_save_waits_for_the_first() {
		let dir = scratch("lock");
		let path = dir.join("lock");
		let first = take(&path, LOCK_WAIT).unwrap();
		let other = path.clone();
		let waited = std::thread::spawn(move || {
			let start = Instant::now();
			let got = take(&other, Duration::from_secs(20)).is_some();
			(got, start.elapsed())
		});
		std::thread::sleep(Duration::from_millis(300));
		drop(first);
		let (got, waited) = waited.join().unwrap();
		assert!(got);
		assert!(waited >= Duration::from_millis(250), "{waited:?}");
		assert!(take(&path, Duration::ZERO).is_some(), "free again");

		let _guard = crate::config::test_config_lock();
		let cfg = dir.join("config.shcl");
		crate::config::set_config_override(cfg);
		let outer = save_lock();
		assert!(outer.file.is_some());
		let start = Instant::now();
		let inner = save_lock();
		assert!(inner.file.is_none());
		assert!(start.elapsed() < Duration::from_secs(1));
		drop(inner);
		drop(outer);
		assert!(save_lock().file.is_some());
	}
}
