// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// One folder per test run, `<temp>/test_silkterm_YYYYmmDD-HHMMSSNN`, that every
// file a test writes goes under. A runner that sets SILKTERM_TEST_DIR hands its
// own folder down instead, so a whole pipeline run shares one. The scripts
// under cicd/tests keep the same contract in _testdir.bash, .py and .ps1.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The run's test folder: made once per process, shared by every test in it.
pub fn run_dir() -> &'static Path {
	static RUN_DIR: OnceLock<PathBuf> = OnceLock::new();
	RUN_DIR.get_or_init(|| {
		let given = std::env::var_os("SILKTERM_TEST_DIR")
			.filter(|dir| !dir.is_empty())
			.map(PathBuf::from);
		make_run_dir(&std::env::temp_dir(), given, local_stamp).expect("test run folder")
	})
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
#[cfg(unix)]
fn local_stamp() -> String {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default();
	let seconds = libc::time_t::try_from(now.as_secs()).unwrap_or(libc::time_t::MAX);
	// SAFETY: tm is plain data, so all zeros is a valid value, and localtime_r
	// only writes the tm it is handed.
	let fields = unsafe {
		let mut fields: libc::tm = std::mem::zeroed();
		libc::localtime_r(&raw const seconds, &raw mut fields);
		fields
	};
	format!(
		"{:04}{:02}{:02}-{:02}{:02}{:02}{:02}",
		fields.tm_year + 1900,
		fields.tm_mon + 1,
		fields.tm_mday,
		fields.tm_hour,
		fields.tm_min,
		fields.tm_sec,
		now.subsec_millis() / 10
	)
}

#[cfg(windows)]
fn local_stamp() -> String {
	use windows_sys::Win32::Foundation::SYSTEMTIME;
	use windows_sys::Win32::System::SystemInformation::GetLocalTime;
	// SAFETY: SYSTEMTIME is plain data, so all zeros is a valid value, and
	// GetLocalTime only fills the struct it is handed.
	let now = unsafe {
		let mut now: SYSTEMTIME = std::mem::zeroed();
		GetLocalTime(&raw mut now);
		now
	};
	format!(
		"{:04}{:02}{:02}-{:02}{:02}{:02}{:02}",
		now.wYear,
		now.wMonth,
		now.wDay,
		now.wHour,
		now.wMinute,
		now.wSecond,
		now.wMilliseconds / 10
	)
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
		#[cfg(unix)]
		std::os::unix::fs::symlink(&target, base.join("test_silkterm_20260101-00000002")).unwrap();
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
}
