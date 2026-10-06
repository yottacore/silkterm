// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Embed the app icon + version info into the Windows PE, so Explorer, taskbar
//! pins and the installer show the real icon, and Properties > Details shows the
//! version/product strings. The .rc is generated from assets/silkterm.rc.in with
//! the version + description filled in from Cargo metadata (so they never drift
//! from Cargo.toml), then compiled by embed-resource - which finds the resource
//! compiler via the cc crate (rc.exe for msvc, windres for gnu/gnullvm), the same
//! way rustc finds the linker, so it works natively and cross from Linux. It
//! no-ops on non-windows targets. Non-fatal: if no resource compiler can be found
//! for the target, warn and build on iconless.
use std::{env, fs, path::Path};

// The build number generator, shared with the crate so the number baked in here
// and the tests over there can't be two different implementations.
include!("src/buildnum.rs");

fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("cargo:rerun-if-changed=assets/silkterm.rc.in");
	println!("cargo:rerun-if-changed=assets/icon.ico");
	println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
	println!("cargo:rerun-if-env-changed=CARGO_PKG_DESCRIPTION");

	emit_build_number();

	// Nothing here belongs in a non-Windows binary, and the no-op has to be
	// explicit: on a WINDOWS host, embed-resource picks its compiler from the
	// host rather than the target, so it happily ran rc.exe and handed the
	// resulting COFF .lib to whatever linker was in play - a Linux cross-build
	// from this box then died with "invalid token in LD script" on it. On a Linux
	// host the same call already came to nothing, so this changes no behavior
	// anywhere; it just stops the one host that got it wrong.
	let target = env::var("TARGET").unwrap_or_default();
	if !target.contains("windows") {
		return Ok(());
	}

	let manifest = cargo_env("CARGO_MANIFEST_DIR");
	let out = cargo_env("OUT_DIR");

	let major = cargo_env("CARGO_PKG_VERSION_MAJOR");
	let minor = cargo_env("CARGO_PKG_VERSION_MINOR");
	let patch = cargo_env("CARGO_PKG_VERSION_PATCH");
	let ver_str = cargo_env("CARGO_PKG_VERSION");
	let desc = env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();

	// forward slashes so the absolute path needs no backslash escaping, and works
	// under both rc.exe and windres (incl. windres running on the Linux cross-build)
	let icon = Path::new(&manifest)
		.join("assets/icon.ico")
		.to_string_lossy()
		.replace('\\', "/");

	let template = fs::read_to_string(Path::new(&manifest).join("assets/silkterm.rc.in"))?;
	let rc = template
		.replace("@ICON@", &icon)
		.replace("@VER_CSV@", &format!("{major},{minor},{patch},0"))
		.replace("@VER_STR@", &ver_str)
		.replace("@DESC@", &desc);

	let rc_path = Path::new(&out).join("silkterm.rc");
	fs::write(&rc_path, rc)?;

	// The compiler is chosen for the TARGET. embed-resource chooses off the build
	// host instead, and that went wrong twice: an msvc host runs rc.exe even for a
	// gnu target, whose .res mingw's ld can't link; and for aarch64 it found no
	// compiler at all and answered "not attempted", which manifest_optional() reads
	// as success - so the ARM64 exe carried no icon and no version strings and
	// nothing said so. msvc still goes through embed-resource, which knows how to
	// find rc.exe.
	if target.ends_with("-windows-msvc") {
		let result = embed_resource::compile(&rc_path, embed_resource::NONE);
		if let Err(err) = result.manifest_required() {
			println!("cargo:warning=windows resources not embedded: {err}");
		}
		return Ok(());
	}
	if let Err(err) = windres_compile(&out, &rc_path) {
		println!("cargo:warning=windows resources not embedded: {err}");
	}
	Ok(())
}

#[allow(
	clippy::expect_used,
	reason = "cargo sets these for every build script run"
)]
fn cargo_env(name: &str) -> String {
	env::var(name).expect("set by cargo for a build script")
}

// A version alone can't tell two builds apart - every dogfood build of a release
// shares it - so bake in a number that can. Watching what goes into the binary is
// what keeps it honest: without it cargo would only re-run this script when the
// icon or the .rc changed, and the number would sit frozen at whatever it was the
// first time. Unchanged inputs produce the same binary and keep the same number,
// which is the point. The list is BUILD_INPUTS in buildnum.rs, where a test keeps
// it covering every file the code includes.
//
// SILK_BUILD_MINUTES pins the value. cicd sets it once per run so all four target
// builds report one build instead of one per link, minutes apart.
fn emit_build_number() {
	for input in BUILD_INPUTS {
		println!("cargo:rerun-if-changed={input}");
	}
	println!("cargo:rerun-if-env-changed=SILK_BUILD_MINUTES");

	let pinned = env::var("SILK_BUILD_MINUTES").unwrap_or_default();
	let pinned = pinned.trim();
	let minutes = if pinned.is_empty() {
		minutes_since_2000(unix_now())
	} else {
		pinned.parse::<u64>().unwrap_or_else(|_| {
			println!(
				"cargo:warning=SILK_BUILD_MINUTES is not a number ({pinned}); using the clock"
			);
			minutes_since_2000(unix_now())
		})
	};
	println!("cargo:rustc-env=SILK_BUILD={}", crockford32(minutes));
}

// Seconds since the unix epoch. A clock set before 1970 reads as 0, which comes
// out the far end as build number "0" rather than as a failed build.
fn unix_now() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |since| since.as_secs())
}

// Compile the .rc to a COFF object and hand it to the linker. Non-fatal by
// contract (see the caller): if nothing on the box can compile a resource for
// this architecture, warn and let the exe build iconless.
fn windres_compile(out: &str, rc_path: &Path) -> Result<(), String> {
	let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
	let obj = Path::new(out).join("silkterm-res.o");
	let mut tried: Vec<String> = Vec::new();
	for (prog, name) in windres_candidates(&arch) {
		// -c 65001: the .rc is UTF-8 (the © in the copyright string). -O coff: a
		// linkable object, not a raw .res.
		let run = std::process::Command::new(&prog)
			.args(["-c", "65001", "-O", "coff", "--target", name, "-I"])
			.arg(out)
			.arg("-i")
			.arg(rc_path)
			.arg("-o")
			.arg(&obj)
			.output();
		match run {
			Ok(done) if done.status.success() => {
				println!("cargo:rustc-link-arg-bins={}", obj.display());
				return Ok(());
			}
			Ok(done) => {
				let said = String::from_utf8_lossy(&done.stderr);
				let said = said.lines().next().unwrap_or("failed").trim().to_string();
				tried.push(format!("{prog}: {said}"));
			}
			Err(err) => tried.push(format!("{prog}: {err}")),
		}
	}
	if tried.is_empty() {
		return Err(format!("no windres target name known for arch {arch}"));
	}
	Err(format!(
		"no resource compiler worked for {arch}: {}",
		tried.join("; ")
	))
}

// What to try, best first. A binutils windres only speaks the architecture it
// was built for, so the triple-prefixed one comes first; llvm-windres does every
// architecture but wants a plain arch name rather than a bfd one. SILK_WINDRES
// names one outright, for a toolchain spelled some other way.
fn windres_candidates(arch: &str) -> Vec<(String, &'static str)> {
	println!("cargo:rerun-if-env-changed=SILK_WINDRES");
	let (bfd, plain, triple) = match arch {
		"x86_64" => ("pe-x86-64", "x86_64", "x86_64-w64-mingw32"),
		"aarch64" => ("pe-aarch64-little", "aarch64", "aarch64-w64-mingw32"),
		"x86" => ("pe-i386", "i386", "i686-w64-mingw32"),
		_ => return Vec::new(),
	};
	let named = env::var("SILK_WINDRES").unwrap_or_default();
	let named = named.trim();
	if !named.is_empty() {
		// Taken at its word about which spelling it wants.
		let name = if named.contains("llvm") { plain } else { bfd };
		return vec![(named.to_string(), name)];
	}
	let mut out = vec![
		(format!("{triple}-windres"), bfd),
		("windres".to_string(), bfd),
	];
	// Debian ships llvm-windres under a version suffix and nothing else, so there
	// is no plain name to call and the versions have to be found on PATH.
	let mut llvm: Vec<(u32, String)> = Vec::new();
	for dir in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
		let Ok(entries) = fs::read_dir(&dir) else {
			continue;
		};
		for entry in entries.flatten() {
			let file = entry.file_name().to_string_lossy().into_owned();
			let stem = file.strip_suffix(".exe").unwrap_or(&file);
			let Some(tail) = stem.strip_prefix("llvm-windres") else {
				continue;
			};
			let version = if tail.is_empty() {
				u32::MAX
			} else {
				match tail.strip_prefix('-').and_then(|v| v.parse::<u32>().ok()) {
					Some(version) => version,
					None => continue,
				}
			};
			let name = stem.to_string();
			if !llvm.iter().any(|(_, have)| *have == name) {
				llvm.push((version, name));
			}
		}
	}
	llvm.sort_by_key(|found| std::cmp::Reverse(found.0));
	out.extend(llvm.into_iter().map(|(_, name)| (name, plain)));
	out
}
