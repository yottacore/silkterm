// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// One folder per test run, `<temp>/test_silkterm_YYYYmmDD-HHMMSSNN`, that every
// file a test writes goes under. A runner that sets SILKTERM_TEST_DIR hands its
// own folder down instead, so a whole pipeline run shares one. The scripts
// under cicd/tests keep the same contract in _testdir.bash, .py and .ps1.
// A folder this process made is removed when the run passes, and kept when a
// test panicked. One named by SILKTERM_TEST_DIR is never removed.

use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const OWNER_FILE: &str = ".test_silkterm_owner";
static PANICKED: AtomicBool = AtomicBool::new(false);
static OWNED: OnceLock<Owned> = OnceLock::new();

#[derive(Debug)]
struct Owned {
	dir: PathBuf,
	token: String,
}

#[derive(Debug)]
enum Removal {
	Removed,
	NotOurs(&'static str),
	Failed(io::Error),
}

unsafe extern "C" {
	fn atexit(callback: extern "C" fn()) -> std::ffi::c_int;
}

/// The run's test folder: made once per process, shared by every test in it.
pub fn run_dir() -> &'static Path {
	static RUN_DIR: OnceLock<PathBuf> = OnceLock::new();
	RUN_DIR.get_or_init(|| {
		let given = std::env::var_os("SILKTERM_TEST_DIR")
			.filter(|dir| !dir.is_empty())
			.map(PathBuf::from);
		let adopted = given.is_some();
		let dir = make_run_dir(&std::env::temp_dir(), given, local_stamp).expect("test run folder");
		if adopted {
			return dir;
		}
		let token = mark_owned(&dir).expect("test run folder");
		// Cloned so the exit handler never reads the path back from anywhere a
		// test could change.
		let _ = OWNED.set(Owned {
			dir: dir.clone(),
			token,
		});
		// A failing test stops before its own cleanup, so its files are the
		// record of what it wrote. Any panic keeps the folder.
		let previous = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |info| {
			PANICKED.store(true, Ordering::Relaxed);
			previous(info);
		}));
		// The test harness has no end-of-run hook. A pass returns from main and
		// the C runtime's exit runs this. A failed registration only leaves the
		// folder behind.
		// SAFETY: atexit only stores the function pointer.
		let _ = unsafe { atexit(remove_at_exit) };
		dir
	})
}

fn mark_owned(dir: &Path) -> io::Result<String> {
	let nanos = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	let token = format!("{}-{nanos}", std::process::id());
	let mut marker = std::fs::File::create_new(dir.join(OWNER_FILE))?;
	writeln!(marker, "{token}")?;
	Ok(token)
}

// Only the folder this process made and marked. remove_dir_all removes a link
// inside as a link and never follows it.
fn remove_if_owned(dir: &Path, token: &str) -> Removal {
	let Ok(meta) = std::fs::symlink_metadata(dir) else {
		return Removal::NotOurs("it is gone");
	};
	if meta.file_type().is_symlink() {
		return Removal::NotOurs("it is a link");
	}
	#[cfg(windows)]
	{
		use std::os::windows::fs::MetadataExt;
		use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
		if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			return Removal::NotOurs("it is a link");
		}
	}
	if !meta.is_dir() {
		return Removal::NotOurs("not a folder");
	}
	let marker = dir.join(OWNER_FILE);
	if !std::fs::symlink_metadata(&marker).is_ok_and(|meta| meta.is_file()) {
		return Removal::NotOurs("no owner mark");
	}
	let Ok(text) = std::fs::read_to_string(&marker) else {
		return Removal::NotOurs("owner mark unreadable");
	};
	if text.lines().next().map(str::trim) != Some(token) {
		return Removal::NotOurs("another run's owner mark");
	}
	match std::fs::remove_dir_all(dir) {
		Ok(()) => Removal::Removed,
		Err(e) => Removal::Failed(e),
	}
}

// Never eprintln! or unwrap here: a panic in an extern "C" function aborts.
extern "C" fn remove_at_exit() {
	let Some(owned) = OWNED.get() else {
		return;
	};
	let mut stderr = io::stderr();
	if PANICKED.load(Ordering::Relaxed) {
		let _ = writeln!(stderr, "test files kept in {}", owned.dir.display());
		return;
	}
	match remove_if_owned(&owned.dir, &owned.token) {
		Removal::Removed => {}
		Removal::NotOurs(reason) => {
			let _ = writeln!(
				stderr,
				"test run folder: left {} in place: {reason}",
				owned.dir.display()
			);
		}
		Removal::Failed(e) => {
			let _ = writeln!(
				stderr,
				"test run folder: could not remove {}: {e}",
				owned.dir.display()
			);
		}
	}
}

fn make_run_dir(
	base: &Path,
	given: Option<PathBuf>,
	mut clock: impl FnMut() -> String,
) -> io::Result<PathBuf> {
	if let Some(given) = given {
		std::fs::create_dir_all(&given)?;
		return Ok(given);
	}
	// Never create_dir_all here: a folder or symlink planted at a guessable name
	// in a shared temp dir would be adopted. A taken name waits for the next stamp.
	#[cfg(unix)]
	let builder = {
		use std::os::unix::fs::DirBuilderExt;
		let mut builder = std::fs::DirBuilder::new();
		builder.mode(0o700);
		builder
	};
	#[cfg(not(unix))]
	let builder = std::fs::DirBuilder::new();
	let mut tries = 1;
	loop {
		let path = base.join(format!("test_silkterm_{}", clock()));
		match builder.create(&path) {
			Ok(()) => return Ok(path),
			Err(e) if e.kind() == ErrorKind::AlreadyExists && tries < 100 => {
				tries += 1;
				std::thread::sleep(Duration::from_millis(10));
			}
			Err(e) => return Err(io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
		}
	}
}

// Local time, like every other stamp the project writes by hand. NN is
// hundredths of a second.
fn local_stamp() -> String {
	let (stamp, hundredths) = crate::config::local_stamp();
	format!("{stamp}{hundredths:02}")
}

#[cfg(test)]
mod tests {
	use super::*;

	// A clean base of the test's own under the run folder.
	fn base_for(what: &str) -> PathBuf {
		let base = run_dir().join(format!("silkterm_testdir_{what}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&base);
		std::fs::create_dir_all(&base).unwrap();
		base
	}

	// Test ID: ErOj5oB
	#[test]
	fn a_run_folder_is_made_fresh_under_the_temp_base() {
		let base = base_for("fresh");
		let got = make_run_dir(&base, None, || "20260101-00000000".to_string()).unwrap();
		assert_eq!(got, base.join("test_silkterm_20260101-00000000"));
		assert!(got.is_dir());
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			assert_eq!(
				std::fs::metadata(&got).unwrap().permissions().mode() & 0o777,
				0o700
			);
		}
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ErOj5oC
	#[test]
	fn a_run_folder_already_there_is_never_adopted() {
		let base = base_for("planted");
		let planted = base.join("test_silkterm_20260101-00000001");
		std::fs::create_dir(&planted).unwrap();
		std::fs::write(planted.join("marker"), "planted").unwrap();
		let target = base.join("elsewhere");
		std::fs::create_dir(&target).unwrap();
		let linked = base.join("test_silkterm_20260101-00000002");
		#[cfg(unix)]
		std::os::unix::fs::symlink(&target, &linked).unwrap();
		// A junction, since anyone can make one there and a symlink needs a privilege.
		#[cfg(windows)]
		{
			let made = std::process::Command::new("cmd")
				.args(["/C", "mklink", "/J"])
				.arg(&linked)
				.arg(&target)
				.output()
				.unwrap();
			assert!(
				made.status.success(),
				"{}",
				String::from_utf8_lossy(&made.stdout)
			);
		}
		assert!(
			std::fs::symlink_metadata(&linked).is_ok(),
			"nothing planted at {}",
			linked.display()
		);
		let mut stamps = [
			"20260101-00000001",
			"20260101-00000002",
			"20260101-00000003",
		]
		.into_iter();
		let got = make_run_dir(&base, None, || stamps.next().unwrap().to_string()).unwrap();
		assert_eq!(got, base.join("test_silkterm_20260101-00000003"));
		let planted_entries: Vec<_> = std::fs::read_dir(&planted)
			.unwrap()
			.map(|entry| entry.unwrap().file_name())
			.collect();
		assert_eq!(planted_entries, ["marker"]);
		assert_eq!(
			std::fs::read_to_string(planted.join("marker")).unwrap(),
			"planted"
		);
		assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ErOj5oD
	#[test]
	fn a_run_folder_gives_up_when_every_name_is_taken() {
		let base = base_for("taken");
		std::fs::create_dir(base.join("test_silkterm_20260101-00000000")).unwrap();
		let started = std::time::Instant::now();
		let got = make_run_dir(&base, None, || "20260101-00000000".to_string());
		let err = got.unwrap_err();
		assert_eq!(err.kind(), ErrorKind::AlreadyExists);
		assert!(
			err.to_string().contains("test_silkterm_20260101-00000000"),
			"{err}"
		);
		assert!(
			started.elapsed() >= Duration::from_millis(900),
			"{:?}",
			started.elapsed()
		);
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ErOj5oE
	#[test]
	fn a_run_folder_the_runner_gives_is_used() {
		let base = base_for("given");
		let given = base.join("a").join("b");
		let got = make_run_dir(&base, Some(given.clone()), || -> String {
			unreachable!("no stamp when a folder is given")
		})
		.unwrap();
		assert_eq!(got, given);
		assert!(given.is_dir());
		std::fs::remove_dir_all(&base).unwrap();
	}

	fn clock_minute() -> String {
		#[cfg(unix)]
		let out = std::process::Command::new("date")
			.arg("+%Y%m%d-%H%M")
			.output()
			.unwrap();
		#[cfg(windows)]
		let out = std::process::Command::new("powershell")
			.args(["-NoProfile", "-Command", "Get-Date -Format yyyyMMdd-HHmm"])
			.output()
			.unwrap();
		String::from_utf8(out.stdout).unwrap().trim().to_string()
	}

	// Test ID: ErOj5oF
	#[test]
	fn a_run_folder_is_stamped_in_local_time() {
		let stamp = local_stamp();
		assert_eq!(stamp.len(), 17, "{stamp}");
		for (at, character) in stamp.chars().enumerate() {
			if at == 8 {
				assert_eq!(character, '-', "{stamp}");
			} else {
				assert!(character.is_ascii_digit(), "{stamp}");
			}
		}
		// Asked twice in case the minute turns over between the two reads.
		for last_try in [false, true] {
			let ours = local_stamp();
			let clock = clock_minute();
			if ours[..13] == clock || last_try {
				assert_eq!(&ours[..13], clock);
				return;
			}
		}
	}

	// A marked run folder under a base of the test's own.
	fn marked_folder(what: &str) -> (PathBuf, PathBuf, String) {
		let base = base_for(what);
		let dir = make_run_dir(&base, None, || "20260101-00000000".to_string()).unwrap();
		let token = mark_owned(&dir).unwrap();
		(base, dir, token)
	}

	// A junction on Windows, since anyone can make one there and a symlink needs
	// a privilege.
	fn link_folder(target: &Path, at: &Path) {
		#[cfg(unix)]
		std::os::unix::fs::symlink(target, at).unwrap();
		#[cfg(windows)]
		{
			let made = std::process::Command::new("cmd")
				.args(["/C", "mklink", "/J"])
				.arg(at)
				.arg(target)
				.output()
				.unwrap();
			assert!(
				made.status.success(),
				"{}",
				String::from_utf8_lossy(&made.stdout)
			);
		}
	}

	// Test ID: ErbiCgE
	#[test]
	fn a_marked_run_folder_is_removed() {
		let (base, dir, token) = marked_folder("removed");
		std::fs::create_dir_all(dir.join("a").join("b")).unwrap();
		std::fs::write(dir.join("a").join("b").join("file"), "x").unwrap();
		let read_only = dir.join("a").join("read-only");
		std::fs::write(&read_only, "x").unwrap();
		let mut permissions = std::fs::metadata(&read_only).unwrap().permissions();
		permissions.set_readonly(true);
		std::fs::set_permissions(&read_only, permissions).unwrap();
		let got = remove_if_owned(&dir, &token);
		assert!(matches!(got, Removal::Removed), "{got:?}");
		assert!(std::fs::symlink_metadata(&dir).is_err());
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ErbiCgF
	#[test]
	fn a_run_folder_without_this_runs_mark_is_left() {
		let (base, dir, token) = marked_folder("unmarked");
		std::fs::write(dir.join("file"), "x").unwrap();
		std::fs::remove_file(dir.join(OWNER_FILE)).unwrap();
		let got = remove_if_owned(&dir, &token);
		assert!(matches!(got, Removal::NotOurs(_)), "{got:?}");
		assert!(dir.join("file").is_file());
		std::fs::write(dir.join(OWNER_FILE), "1-2\n").unwrap();
		let got = remove_if_owned(&dir, &token);
		assert!(matches!(got, Removal::NotOurs(_)), "{got:?}");
		assert!(dir.join("file").is_file());
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ErbiCgG
	#[test]
	fn a_link_in_place_of_the_run_folder_is_left() {
		let (base, dir, token) = marked_folder("swapped");
		let target = base.join("elsewhere");
		std::fs::create_dir(&target).unwrap();
		std::fs::copy(dir.join(OWNER_FILE), target.join(OWNER_FILE)).unwrap();
		std::fs::write(target.join("file"), "x").unwrap();
		std::fs::remove_dir_all(&dir).unwrap();
		link_folder(&target, &dir);
		let got = remove_if_owned(&dir, &token);
		assert!(matches!(got, Removal::NotOurs(_)), "{got:?}");
		assert!(target.join("file").is_file());
		assert!(target.join(OWNER_FILE).is_file());
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ErbiCgH
	#[test]
	fn a_link_inside_the_run_folder_is_not_followed() {
		let (base, dir, token) = marked_folder("inner_link");
		let outside = base.join("outside");
		std::fs::create_dir(&outside).unwrap();
		std::fs::write(outside.join("file"), "x").unwrap();
		link_folder(&outside, &dir.join("link"));
		let got = remove_if_owned(&dir, &token);
		assert!(matches!(got, Removal::Removed), "{got:?}");
		assert!(outside.join("file").is_file());
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Started on its own by cicd/tests/testdir/run.bash, which checks the folder
	// is kept and named. Does nothing in a normal run.
	// Test ID: ErbiCgI
	#[test]
	fn a_run_that_fails_keeps_its_folder() {
		if std::env::var_os("SILKTERM_TEST_FAIL_ON_PURPOSE").is_none() {
			return;
		}
		std::fs::write(run_dir().join("kept"), "kept").unwrap();
		panic!("failed on purpose");
	}
}
