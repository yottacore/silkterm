// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Where Windows looks for a DLL that's loaded by bare name.
//!
//! The default search goes on past the program's folder and System32 to the
//! working folder and every folder on PATH. A DLL Windows doesn't have, such
//! as `conpty.dll` or `dxcompiler.dll`, is then taken from whatever program
//! put one there, or from a planted copy. The `conpty.dll` another terminal
//! put on PATH changed how every pane behaved.

/// Keep every later load by bare name to the program's folder and System32.
/// Runs first in `main`, before the engine or the GPU code loads anything.
pub fn restrict() {
	#[cfg(windows)]
	// SAFETY: takes a constant and only changes where later loads look. It fails
	// only on Windows 7 without KB2533623, which this doesn't run on anyway.
	unsafe {
		use windows_sys::Win32::System::LibraryLoader::{
			LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, SetDefaultDllDirectories,
		};
		SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
	}
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
	use std::os::windows::ffi::{OsStrExt, OsStringExt};
	use std::path::{Path, PathBuf};
	use std::process::Command;
	use windows_sys::Win32::Foundation::FreeLibrary;
	use windows_sys::Win32::System::LibraryLoader::{
		GetModuleFileNameW, GetModuleHandleW, LoadLibraryW,
	};

	fn wide(s: &str) -> Vec<u16> {
		std::ffi::OsStr::new(s).encode_wide().chain([0]).collect()
	}

	// Any harmless DLL does; it only has to be found. A copy of one Windows
	// has, under a name it doesn't.
	fn plant(dir: &Path, name: &str) {
		std::fs::create_dir_all(dir).unwrap();
		let root = std::env::var_os("SystemRoot").unwrap();
		let real = PathBuf::from(root).join("System32").join("version.dll");
		std::fs::copy(real, dir.join(name)).unwrap();
	}

	fn loaded_from(name: &str) -> Option<PathBuf> {
		let w = wide(name);
		// SAFETY: a null-terminated name; the buffer's length goes with it.
		unsafe {
			let module = GetModuleHandleW(w.as_ptr());
			if module.is_null() {
				return None;
			}
			let mut buf = [0u16; 1024];
			let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
			Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
		}
	}

	/// What the child said, from a child run in `cwd` with `extra` first on PATH.
	fn child(what: &str, cwd: &Path, extra: &Path) -> String {
		let path = std::env::var_os("PATH").unwrap_or_default();
		let mut dirs = vec![extra.to_path_buf()];
		dirs.extend(std::env::split_paths(&path));
		let out = Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"dllsearch::tests::dll_probe_child",
				"--nocapture",
			])
			.env("SILK_DLL_PROBE", what)
			.env("PATH", std::env::join_paths(dirs).unwrap())
			.current_dir(cwd)
			.output()
			.unwrap();
		let text = String::from_utf8_lossy(&out.stdout).into_owned();
		assert!(
			out.status.success(),
			"{text}{}",
			String::from_utf8_lossy(&out.stderr)
		);
		text.lines()
			.find_map(|l| l.strip_prefix("probe "))
			.unwrap_or_else(|| panic!("the child answered: {text}"))
			.to_owned()
	}

	// A DLL Windows doesn't have was taken from PATH or the working folder,
	// which is how WezTerm's conpty.dll got into SilkTerm.
	// Test ID: EshQYN6
	#[test]
	fn a_dll_on_path_or_in_the_working_folder_is_not_loaded() {
		let base = crate::testdir::run_dir().join("dllsearch");
		let on_path = base.join("on_path");
		let here = base.join("here");
		let elsewhere = base.join("elsewhere");
		plant(&on_path, "silk_plant.dll");
		plant(&here, "silk_plant.dll");
		std::fs::create_dir_all(&elsewhere).unwrap();
		// the plants are found at all without the rule
		assert_eq!(child("open", &elsewhere, &on_path), "loaded");
		assert_eq!(child("open", &here, &elsewhere), "loaded");
		assert_eq!(child("restricted", &elsewhere, &on_path), "none");
		assert_eq!(child("restricted", &here, &elsewhere), "none");
	}

	// The engine loaded conpty.dll by bare name. The fix is in the engine fork,
	// so an engine update is what would bring this back. The rule above would
	// hide it in the program, so this one runs without it.
	// Test ID: EshQYPc
	#[test]
	fn the_engine_takes_no_conpty_dll_from_path_or_the_working_folder() {
		let base = crate::testdir::run_dir().join("dllsearch_engine");
		let on_path = base.join("on_path");
		let here = base.join("here");
		let elsewhere = base.join("elsewhere");
		plant(&on_path, "conpty.dll");
		plant(&here, "conpty.dll");
		std::fs::create_dir_all(&elsewhere).unwrap();
		let base = std::fs::canonicalize(&base).unwrap();
		for (cwd, extra) in [(&elsewhere, &on_path), (&here, &elsewhere)] {
			let got = child("engine", cwd, extra);
			if got == "none" {
				continue;
			}
			let got = std::fs::canonicalize(&got).unwrap();
			assert!(
				!got.starts_with(&base),
				"the engine loaded {}",
				got.display()
			);
		}
	}

	// Test ID: EshQYRz
	#[test]
	fn dll_probe_child() {
		let Some(what) = std::env::var_os("SILK_DLL_PROBE") else {
			return;
		};
		let what = what.to_string_lossy().into_owned();
		let answer = if what == "engine" {
			use alacritty_terminal::event::WindowSize;
			use alacritty_terminal::tty::{self, Options, Shell};
			let options = Options {
				shell: Some(Shell::new(
					"cmd.exe".into(),
					vec!["/c".into(), "exit".into()],
				)),
				..Options::default()
			};
			let size = WindowSize {
				num_lines: 24,
				num_cols: 80,
				cell_width: 8,
				cell_height: 16,
			};
			drop(tty::new(&options, size, 0).unwrap());
			loaded_from("conpty.dll").map_or_else(|| "none".to_owned(), |p| p.display().to_string())
		} else {
			if what == "restricted" {
				super::restrict();
			}
			let w = wide("silk_plant.dll");
			// SAFETY: a null-terminated name. A module that loads is let go.
			unsafe {
				let module = LoadLibraryW(w.as_ptr());
				if module.is_null() {
					"none".to_owned()
				} else {
					FreeLibrary(module);
					"loaded".to_owned()
				}
			}
		};
		println!("probe {answer}");
	}
}
