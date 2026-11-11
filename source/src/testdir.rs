// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! One folder per test run, `<HOST_TEMP>/silkterm/YYYYmmDD-HHMMSS_NNN` with a
//! `run.pid`, the way the pipeline makes its own, that every file a test writes
//! goes under. Without `HOST_TEMP` it's under the temp dir. A runner that sets
//! `SILKTERM_TEST_DIR` hands its own folder down instead, so a whole pipeline
//! run shares one. The scripts under cicd/tests follow the same contract in
//! _testdir.bash, .py and .ps1. A folder this process made is removed when the
//! run passes. When a test panicked it's kept for the pipeline's stale sweep,
//! unless there's nothing in it but its marks. One named by
//! `SILKTERM_TEST_DIR` is never removed.

use std::cell::Cell;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

const OWNER_FILE: &str = ".test_silkterm_owner";
const PID_FILE: &str = "run.pid";
static PANICKED: AtomicBool = AtomicBool::new(false);
static OWNED: OnceLock<Owned> = OnceLock::new();

thread_local! {
	static EXPECTING_PANIC: Cell<bool> = const { Cell::new(false) };
}

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
		let dir = make_run_dir(&run_base(), given, local_stamp).expect("test run folder");
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
		// record of what it wrote. Any panic keeps the folder, except one a test
		// caught on purpose through catch_expected_panic.
		let previous = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |info| {
			if !EXPECTING_PANIC.try_with(Cell::get).unwrap_or(false) {
				PANICKED.store(true, Ordering::Relaxed);
			}
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

/// `catch_unwind` for a test that panics on purpose. The panic hook runs
/// before anything catches, so it can't tell a caught panic from a failed
/// test, and a bare `catch_unwind` in a test keeps a passing run's folder.
pub fn catch_expected_panic<R>(
	body: impl FnOnce() -> R + std::panic::UnwindSafe,
) -> std::thread::Result<R> {
	let outer = EXPECTING_PANIC.replace(true);
	let caught = std::panic::catch_unwind(body);
	EXPECTING_PANIC.set(outer);
	caught
}

// HOST_TEMP/silkterm, as cicd.bash puts its runs.
fn run_base() -> PathBuf {
	std::env::var_os("HOST_TEMP")
		.filter(|dir| !dir.is_empty())
		.map_or_else(std::env::temp_dir, PathBuf::from)
		.join("silkterm")
}

// The token proves the folder is ours at the end. run.pid tells the
// pipeline's stale sweep whether the run is still going.
fn mark_owned(dir: &Path) -> io::Result<String> {
	let nanos = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	let pid = std::process::id();
	let token = format!("{pid}-{nanos}");
	let mut marker = std::fs::File::create_new(dir.join(OWNER_FILE))?;
	writeln!(marker, "{token}")?;
	writeln!(std::fs::File::create_new(dir.join(PID_FILE))?, "{pid}")?;
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

// Nothing in the folder but the owner mark and run.pid, so a failed run has
// nothing there to look at.
fn only_marks(dir: &Path) -> bool {
	std::fs::read_dir(dir).is_ok_and(|entries| {
		entries.into_iter().all(|entry| {
			entry
				.is_ok_and(|entry| entry.file_name() == OWNER_FILE || entry.file_name() == PID_FILE)
		})
	})
}

// Never eprintln! or unwrap here: a panic in an extern "C" function aborts.
extern "C" fn remove_at_exit() {
	let Some(owned) = OWNED.get() else {
		return;
	};
	let mut stderr = io::stderr();
	if PANICKED.load(Ordering::Relaxed) && !only_marks(&owned.dir) {
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
	clock: impl FnOnce() -> String,
) -> io::Result<PathBuf> {
	if let Some(given) = given {
		std::fs::create_dir_all(&given)?;
		return Ok(given);
	}
	std::fs::create_dir_all(base)?;
	// Never create_dir_all for the run's own folder: a folder or symlink planted
	// at a guessable name would be adopted. A taken name counts up.
	#[cfg(unix)]
	let builder = {
		use std::os::unix::fs::DirBuilderExt;
		let mut builder = std::fs::DirBuilder::new();
		builder.mode(0o700);
		builder
	};
	#[cfg(not(unix))]
	let builder = std::fs::DirBuilder::new();
	let stamp = clock();
	let mut counter = 1;
	loop {
		let path = base.join(format!("{stamp}_{counter:03}"));
		match builder.create(&path) {
			Ok(()) => return Ok(path),
			Err(e) if e.kind() == ErrorKind::AlreadyExists && counter < 999 => counter += 1,
			Err(e) => return Err(io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
		}
	}
}

// Local time, like every other stamp the project writes by hand.
fn local_stamp() -> String {
	crate::config::local_stamp().0
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

	// Test ID: ddazr
	#[test]
	fn a_run_folder_is_made_fresh_under_the_temp_base() {
		let base = base_for("fresh");
		let got = make_run_dir(&base, None, || "20260101-000000".to_string()).unwrap();
		assert_eq!(got, base.join("20260101-000000_001"));
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

	// Test ID: ddazs
	#[test]
	fn a_run_folder_already_there_is_never_adopted() {
		let base = base_for("planted");
		let planted = base.join("20260101-000000_001");
		std::fs::create_dir(&planted).unwrap();
		std::fs::write(planted.join("marker"), "planted").unwrap();
		let target = base.join("elsewhere");
		std::fs::create_dir(&target).unwrap();
		let linked = base.join("20260101-000000_002");
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
		let got = make_run_dir(&base, None, || "20260101-000000".to_string()).unwrap();
		assert_eq!(got, base.join("20260101-000000_003"));
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

	// Test ID: ddazt
	#[test]
	fn a_run_folder_gives_up_when_every_name_is_taken() {
		let base = base_for("taken");
		for counter in 1..=999 {
			std::fs::create_dir(base.join(format!("20260101-000000_{counter:03}"))).unwrap();
		}
		let got = make_run_dir(&base, None, || "20260101-000000".to_string());
		let err = got.unwrap_err();
		assert_eq!(err.kind(), ErrorKind::AlreadyExists);
		assert!(err.to_string().contains("20260101-000000_999"), "{err}");
		// Commented out 20261101: a taken name counts up now, with no wait
		// between tries (2026103115564575).
		// assert!(
		// 	started.elapsed() >= Duration::from_millis(900),
		// 	"{:?}",
		// 	started.elapsed()
		// );
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: ddazv
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

	// Test ID: ddazw
	#[test]
	#[cfg_attr(miri, ignore = "Miri can't start a process")]
	fn a_run_folder_is_stamped_in_local_time() {
		let stamp = local_stamp();
		assert_eq!(stamp.len(), 15, "{stamp}");
		for (at, character) in stamp.chars().enumerate() {
			if at == 8 {
				assert_eq!(character, '-', "{stamp}");
			} else {
				assert!(character.is_ascii_digit(), "{stamp}");
			}
		}
		// Asked twice in case the minute turns over between the 2 reads.
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
		let dir = make_run_dir(&base, None, || "20260101-000000".to_string()).unwrap();
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

	// Test ID: deqts
	#[test]
	fn a_marked_run_folder_names_this_process_in_its_run_pid() {
		let (base, dir, _) = marked_folder("pid");
		assert_eq!(
			std::fs::read_to_string(dir.join(PID_FILE)).unwrap(),
			format!("{}\n", std::process::id())
		);
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: dez6x
	#[test]
	fn a_run_folder_with_nothing_but_its_marks_has_nothing_to_keep() {
		let (base, dir, _) = marked_folder("marks");
		assert!(only_marks(&dir));
		std::fs::write(dir.join("left.log"), "x").unwrap();
		assert!(!only_marks(&dir));
		std::fs::remove_file(dir.join("left.log")).unwrap();
		std::fs::create_dir(dir.join("silkterm-x")).unwrap();
		assert!(!only_marks(&dir));
		assert!(!only_marks(&base.join("not-there")));
		std::fs::remove_dir_all(&base).unwrap();
	}

	// Test ID: dde3p
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

	// Test ID: dde3q
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

	// Test ID: dde3r
	#[test]
	#[cfg_attr(miri, ignore = "Miri can't copy a file")]
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

	// Test ID: dde3s
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
	// Test ID: dde3t
	#[test]
	fn a_run_that_fails_keeps_its_folder() {
		if std::env::var_os("SILKTERM_TEST_FAIL_ON_PURPOSE").is_none() {
			return;
		}
		std::fs::write(run_dir().join("kept"), "kept").unwrap();
		panic!("failed on purpose");
	}

	// A real panic elsewhere at the same moment fails this too, but that run
	// has failed anyway.
	// Test ID: ddgzm
	#[test]
	fn a_panic_a_test_expects_does_not_keep_the_folder() {
		run_dir();
		if PANICKED.load(Ordering::Relaxed) {
			return;
		}
		let caught = catch_expected_panic(|| panic!("expected"));
		assert!(caught.is_err());
		assert!(
			!PANICKED.load(Ordering::Relaxed),
			"a caught panic marked the run as failed"
		);
		assert!(!EXPECTING_PANIC.get());
	}

	// Every panic a passing test makes on purpose goes through
	// catch_expected_panic. fuzz.rs catches only to name the seed, then fails.
	// Test ID: ddgzn
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn no_test_catches_a_panic_behind_the_run_folders_back() {
		let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
		let mut offenders = Vec::new();
		let mut scanned = 0;
		let mut folders = vec![src.clone()];
		while let Some(folder) = folders.pop() {
			for entry in std::fs::read_dir(&folder).unwrap() {
				let path = entry.unwrap().path();
				if path.is_dir() {
					folders.push(path);
					continue;
				}
				let shown = path.strip_prefix(&src).unwrap().display().to_string();
				if path.extension().is_none_or(|ext| ext != "rs")
					|| shown == "testdir.rs"
					|| shown == "fuzz.rs"
				{
					continue;
				}
				let text = std::fs::read_to_string(&path).unwrap();
				scanned += 1;
				for (at, line) in text.lines().enumerate() {
					if line.contains("catch_unwind") || line.contains("should_panic") {
						offenders.push(format!("{shown}:{}: {}", at + 1, line.trim()));
					}
				}
			}
		}
		// A walk that found nothing would pass.
		assert!(
			scanned > 10,
			"only {scanned} source files under {}",
			src.display()
		);
		assert!(
			offenders.is_empty(),
			"use testdir::catch_expected_panic:\n{}",
			offenders.join("\n")
		);
	}
}
