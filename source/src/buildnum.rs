// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The build number: whole minutes since 2000 began, written in Crockford base 32.
// Five characters until 2063, it sorts in the order the builds were made, and it
// decodes back to the minute one was built - which is what a copy's file date
// could never be trusted for, since every producer stamps that differently.
//
// build.rs include!s this file, so the number baked into the binary and the tests
// below are the same code. That is why nothing here has a `use` line and why the
// module is only compiled into the crate under cfg(test): at run time the answer
// is already a string in the environment (config::BUILD_ID).

// Crockford's alphabet, lowercase. No i, l, o or u, so nothing in a build number
// copied out of a bug report can be read back as a digit or as another letter.
const CROCKFORD_LOWER: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

// What a binary is built from besides the compiler, relative to this crate. A
// change to any of them makes a new binary, so it has to make a new number. Only
// `src` was watched at first, so a dependency update or a new logo kept the old
// number on a different binary.
const BUILD_INPUTS: &[&str] = &[
	"src",
	"assets",
	"Cargo.toml",
	"../Cargo.toml", // the workspace, which owns the profiles and the patches
	"../Cargo.lock",
	"../shell-integration.md",
];

// 2000-01-01T00:00:00Z, as unix time.
const EPOCH_2000_UNIX: u64 = 946_684_800;

fn crockford32(value: u64) -> String {
	if value == 0 {
		return "0".to_string();
	}
	let mut digits = Vec::new();
	let mut left = value;
	while left > 0 {
		digits.push(CROCKFORD_LOWER[(left % 32) as usize] as char);
		left /= 32;
	}
	digits.iter().rev().collect()
}

// Whole elapsed minutes, so the number only ever goes up. A clock set before 2000
// gives 0 instead of wrapping.
fn minutes_since_2000(unix_secs: u64) -> u64 {
	unix_secs.saturating_sub(EPOCH_2000_UNIX) / 60
}

#[cfg(test)]
mod tests {
	use super::*;

	fn build_number_at(unix_secs: u64) -> String {
		crockford32(minutes_since_2000(unix_secs))
	}

	// Test ID: Eo4auD2
	#[test]
	fn the_alphabet_is_crockfords_with_the_ambiguous_letters_left_out() {
		let alphabet = std::str::from_utf8(CROCKFORD_LOWER).unwrap();
		assert_eq!(alphabet, "0123456789abcdefghjkmnpqrstvwxyz");
		for skipped in ['i', 'l', 'o', 'u'] {
			assert!(!alphabet.contains(skipped), "{skipped} is ambiguous");
		}
		assert_eq!(alphabet.len(), 32);
	}

	// Test ID: Eo4auD3
	#[test]
	fn small_values_encode_digit_by_digit() {
		assert_eq!(crockford32(0), "0");
		assert_eq!(crockford32(9), "9");
		// 10 is where the letters start, and h is 17 - one past the skipped i.
		assert_eq!(crockford32(10), "a");
		assert_eq!(crockford32(17), "h");
		assert_eq!(crockford32(18), "j");
		assert_eq!(crockford32(31), "z");
		assert_eq!(crockford32(32), "10");
		assert_eq!(crockford32(33), "11");
		assert_eq!(crockford32(1024), "100");
	}

	// Test ID: Eo4auD4
	#[test]
	fn a_build_number_decodes_back_to_the_minute_it_was_built() {
		// Round-trip every digit position through a hand-rolled decode, so an
		// encoder that quietly reversed itself would not pass.
		let decode = |text: &str| -> u64 {
			text.bytes().fold(0u64, |sum, byte| {
				let digit = CROCKFORD_LOWER.iter().position(|c| *c == byte).unwrap();
				sum * 32 + digit as u64
			})
		};
		for minutes in [0, 1, 31, 32, 1_000, 14_000_000, u32::MAX as u64] {
			assert_eq!(decode(&crockford32(minutes)), minutes);
		}
	}

	// Test ID: Eo4auD5
	#[test]
	fn the_epoch_is_the_start_of_2000_and_the_count_is_whole_minutes() {
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX), 0);
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX + 59), 0); // part of a minute doesn't count
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX + 60), 1);
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX + 61), 1);
		// A day, and a non-leap year.
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX + 86_400), 1_440);
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX + 365 * 86_400), 525_600);
	}

	// Test ID: Eo4auD6
	#[test]
	fn a_clock_behind_the_epoch_gives_zero_rather_than_wrapping() {
		assert_eq!(minutes_since_2000(0), 0);
		assert_eq!(minutes_since_2000(EPOCH_2000_UNIX - 1), 0);
		assert_eq!(build_number_at(0), "0");
	}

	// Test ID: Eo4auD7
	#[test]
	fn a_later_build_always_sorts_after_an_earlier_one() {
		// Same length means plain string order works; the length only grows, so
		// it holds across a rollover too.
		let earlier = build_number_at(EPOCH_2000_UNIX + 14_000_000 * 60);
		let later = build_number_at(EPOCH_2000_UNIX + 14_000_001 * 60);
		assert!(earlier < later, "{earlier} should sort before {later}");
		assert_eq!(earlier.len(), later.len());
		assert!(build_number_at(EPOCH_2000_UNIX) < build_number_at(EPOCH_2000_UNIX + 60));
	}

	// Test ID: Eo4auD8
	#[test]
	fn the_number_stays_five_characters_for_the_life_of_this_program() {
		// 32^5 minutes past 2000 is partway through 2063; anything sooner is five
		// characters, which is what the About panel and the release notes assume.
		let year_2026 = EPOCH_2000_UNIX + 26 * 365 * 86_400;
		let year_2060 = EPOCH_2000_UNIX + 60 * 365 * 86_400;
		assert_eq!(build_number_at(year_2026).len(), 5);
		assert_eq!(build_number_at(year_2060).len(), 5);
	}

	// Every file the code pulls in with include_bytes! or include_str! sits under
	// one of the inputs, and so do the lock file and both manifests.
	// Test ID: Eq4Yrbe
	#[test]
	fn the_build_inputs_cover_every_included_file_and_the_lock() {
		let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
		let real = |path: &std::path::Path| {
			path.canonicalize()
				.unwrap_or_else(|e| panic!("{}: {e}", path.display()))
		};
		let inputs: Vec<std::path::PathBuf> = BUILD_INPUTS
			.iter()
			.map(|input| real(&crate_dir.join(input)))
			.collect();
		let covered = |file: &std::path::Path| inputs.iter().any(|input| file.starts_with(input));
		for needed in ["Cargo.toml", "../Cargo.toml", "../Cargo.lock"] {
			assert!(
				covered(&real(&crate_dir.join(needed))),
				"{needed} is not watched"
			);
		}
		let mut dirs = vec![crate_dir.join("src")];
		let mut seen = 0;
		while let Some(dir) = dirs.pop() {
			for entry in std::fs::read_dir(&dir).unwrap().flatten() {
				let path = entry.path();
				if path.is_dir() {
					dirs.push(path);
					continue;
				}
				if path.extension().is_none_or(|ext| ext != "rs") {
					continue;
				}
				let text = std::fs::read_to_string(&path).unwrap();
				for macro_name in ["include_bytes!(\"", "include_str!(\""] {
					for (at, _) in text.match_indices(macro_name) {
						let rest = &text[at + macro_name.len()..];
						let Some(end) = rest.find('"') else { continue };
						let included = real(&path.parent().unwrap().join(&rest[..end]));
						assert!(
							covered(&included),
							"{} includes {}, which no build input covers",
							path.display(),
							included.display()
						);
						seen += 1;
					}
				}
			}
		}
		assert!(seen > 0, "found no includes at all, so the scan is broken");
	}

	// Every source file names the license the crate is published under, as the
	// manifest does.
	// Test ID: Er2UFeX
	#[test]
	fn every_source_file_carries_the_license_header() {
		let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
		let manifest = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap();
		assert!(
			manifest
				.lines()
				.any(|line| line.trim() == "license = \"GPL-2.0-or-later\""),
			"Cargo.toml names another license"
		);
		let mut seen = 0;
		for entry in std::fs::read_dir(crate_dir.join("src")).unwrap().flatten() {
			let path = entry.path();
			if path.extension().is_none_or(|ext| ext != "rs") {
				continue;
			}
			let text = std::fs::read_to_string(&path).unwrap();
			let mut lines = text.lines();
			assert_eq!(
				lines.next(),
				Some("// SPDX-License-Identifier: GPL-2.0-or-later"),
				"{}",
				path.display()
			);
			assert!(
				lines
					.next()
					.is_some_and(|line| line.starts_with("// Copyright © ")),
				"{} has no copyright line under the license",
				path.display()
			);
			seen += 1;
		}
		assert!(seen > 0, "found no source files, so the scan is broken");
	}

	// The text after a line's comment marker, or None for a line of code.
	fn comment_text(line: &str) -> Option<&str> {
		let line = line.trim_start();
		if let Some(rest) = line.strip_prefix("//").or_else(|| line.strip_prefix("::")) {
			return Some(rest.trim_start());
		}
		line.starts_with('#')
			.then(|| line.trim_start_matches('#').trim_start())
	}

	fn is_history_heading(text: &str) -> bool {
		let text = text.strip_prefix("- ").unwrap_or(text);
		text.starts_with("History") || text.starts_with("Script history")
	}

	// A copyright line's text after "Copyright ": the sign, the year or span,
	// then the holder. Answers whether it is the Bubbles form, or why it is
	// neither form.
	fn copyright_form(text: &str, marker: &str) -> Result<bool, String> {
		let (bubbles, rest) = if let Some(rest) = text.strip_prefix("© ") {
			(false, rest)
		} else if let Some(rest) = text.strip_prefix("(c) ") {
			(true, rest)
		} else {
			return Err(format!("copyright sign in neither form: {text}"));
		};
		let (years, holder) = rest.split_once(' ').unwrap_or_default();
		let year_ok =
			|year: &str| year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit());
		if !years.split('-').all(year_ok) {
			return Err(format!("copyright year {years:?}"));
		}
		let expected = if bubbles {
			"Bubbles".to_string()
		} else {
			format!("Jim Collier {marker}")
		};
		if holder != expected {
			return Err(format!("copyright holder {holder:?}"));
		}
		Ok(bubbles)
	}

	// Not a heading such as "Copyright and license:".
	fn is_copyright_line(text: &str) -> bool {
		text.starts_with("Copyright ©") || text.starts_with("Copyright (")
	}

	// What is wrong with one script's header and History, if anything.
	fn script_header_faults(lines: &[&str], cmd: bool, marker: &str) -> Vec<String> {
		let at = lines
			.iter()
			.position(|line| comment_text(line).is_some_and(is_copyright_line));
		let Some(at) = at else {
			return vec!["no copyright line".into()];
		};
		let mut faults = Vec::new();
		for (number, line) in lines[..at].iter().enumerate() {
			let preamble = cmd && (line.starts_with('@') || line.eq_ignore_ascii_case("setlocal"));
			match comment_text(line) {
				None if !line.trim().is_empty() && !preamble => {
					faults.push(format!(
						"line {} is code above the copyright line",
						number + 1
					));
				}
				Some(text) if is_history_heading(text) && !text.contains("bottom") => {
					faults.push(format!(
						"line {} starts a History in the header",
						number + 1
					));
				}
				_ => {}
			}
		}
		let copyright = &comment_text(lines[at]).unwrap()["Copyright ".len()..];
		let bubbles = match copyright_form(copyright, marker) {
			Ok(bubbles) => bubbles,
			Err(fault) => {
				faults.push(fault);
				false
			}
		};
		let license = lines[at + 1..lines.len().min(at + 5)]
			.iter()
			.filter_map(|line| comment_text(line))
			.find_map(|text| text.strip_prefix("SPDX-License-Identifier: "));
		match license {
			Some("MIT") => {}
			Some("GPL-2.0-or-later") if !bubbles => {}
			other => faults.push(format!("license under the copyright line is {other:?}")),
		}
		let history = lines
			.iter()
			.rposition(|line| comment_text(line).is_some_and(is_history_heading))
			.filter(|&history| history > at);
		match history {
			None => faults.push("no History at the bottom".into()),
			Some(history) => {
				let code_after = lines[history..]
					.iter()
					.any(|line| !line.trim().is_empty() && comment_text(line).is_none());
				if code_after {
					faults.push("code after the History".into());
				}
			}
		}
		faults
	}

	// The rest of the tree: build.rs, the Rust outside src/, and every script
	// git tracks. A copyright line in the Jim Collier or the Bubbles form, the
	// license within a few lines under it, nothing but comments above it, and
	// History at the bottom of the file, not in the header. Tracked files only,
	// so a scratch file in a working tree is never judged.
	// Test ID: ErloT4L
	#[test]
	fn every_script_carries_the_license_header_with_history_at_the_bottom() {
		// shell_integration.ps1 is written whole into a user's PowerShell
		// profile, so it names no license there. It ships inside the binary,
		// which does.
		const EXEMPT: &[&str] = &["source/src/shell_integration.ps1"];
		let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
		let repo = crate_dir.parent().unwrap();
		let own = std::fs::read_to_string(crate_dir.join("src/buildnum.rs")).unwrap();
		let marker = own
			.lines()
			.nth(1)
			.and_then(|line| line.split_once(" Jim Collier "))
			.map(|(_, marker)| marker.to_string())
			.unwrap();
		if !repo.join(".git").exists() {
			eprintln!("not a git checkout, so there is no list of tracked scripts");
			return;
		}
		let listing = std::process::Command::new("git")
			.args(["ls-files", "-z"])
			.current_dir(repo)
			.output()
			.unwrap();
		assert!(listing.status.success(), "git ls-files failed");
		let mut faults = Vec::new();
		let mut seen = 0;
		for name in String::from_utf8(listing.stdout).unwrap().split('\0') {
			let path = repo.join(name);
			let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
			if name.is_empty() || EXEMPT.contains(&name) || !path.is_file() {
				continue;
			}
			let Ok(text) = std::fs::read_to_string(&path) else {
				continue;
			};
			// cicd-win.ps1 starts with a byte-order mark.
			let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
			let lines: Vec<&str> = text
				.lines()
				.map(|line| line.trim_end_matches('\r'))
				.collect();
			if ext == "rs" {
				if lines.first() != Some(&"// SPDX-License-Identifier: GPL-2.0-or-later") {
					faults.push(format!("{name}: line 1 is not the license"));
				}
				let copyright = lines
					.get(1)
					.and_then(|line| line.strip_prefix("// Copyright "));
				match copyright.map(|text| copyright_form(text, &marker)) {
					Some(Ok(false)) => {}
					Some(Ok(true)) => {
						faults.push(format!("{name}: a GPL file in the Bubbles form"))
					}
					Some(Err(fault)) => faults.push(format!("{name}: {fault}")),
					None => faults.push(format!("{name}: no copyright line under the license")),
				}
				seen += 1;
				continue;
			}
			if !matches!(ext, "bash" | "sh" | "py" | "ps1" | "cmd") && !text.starts_with("#!") {
				continue;
			}
			for fault in script_header_faults(&lines, ext == "cmd", &marker) {
				faults.push(format!("{name}: {fault}"));
			}
			seen += 1;
		}
		assert!(
			seen > 100,
			"found only {seen} scripts, so the scan is broken"
		);
		assert!(
			faults.is_empty(),
			"{} header faults:\n{}",
			faults.len(),
			faults.join("\n")
		);
	}
}
