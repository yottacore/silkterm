// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Hyperlinks in terminal output: find URLs in a row of grid text, and hand one
//! to the desktop's handler.
//!
//! Detection is allowlisted BY SCHEME and that is load-bearing twice over: it
//! keeps the false-positive rate near zero (a bare word with a slash in it is not
//! a link), and it is what stops `javascript:` / `data:` from ever reaching the
//! opener. A scheme absent from SCHEMES is not a link, so it cannot be opened.

use std::io;
use std::process::{Command, Stdio};

// (scheme, needs "//"). Ordered longest-first within a family so "https" is
// tested before "http" - every candidate is tried anyway, but the order keeps
// the common case one comparison.
const SCHEMES: [(&str, bool); 8] = [
	("https", true),
	("http", true),
	("ftps", true),
	("ftp", true),
	("file", true),
	("sftp", true),
	("ssh", true),
	("mailto", false),
];

// Characters a URL may carry. ASCII only: percent-encoding covers the rest, and
// admitting non-ASCII would swallow the CJK sentence that follows a link. The
// exclusions are the delimiters RFC 3986 leaves out plus the quotes a shell
// wraps a URL in.
fn is_url_char(c: char) -> bool {
	c.is_ascii_graphic()
		&& !matches!(
			c,
			'"' | '\'' | '<' | '>' | '`' | '{' | '}' | '|' | '\\' | '^'
		)
}

// A link can't start mid-word, or "xhttp://x" reads as a link one char in.
fn boundary_before(text: &[char], start: usize) -> bool {
	match start.checked_sub(1).and_then(|i| text.get(i)) {
		Some(&c) => !c.is_ascii_alphanumeric() && !matches!(c, '+' | '-' | '.'),
		None => true,
	}
}

fn matches_scheme(text: &[char], start: usize, scheme: &str) -> bool {
	scheme.chars().enumerate().all(|(i, want)| {
		text.get(start + i)
			.is_some_and(|&c| c.eq_ignore_ascii_case(&want))
	})
}

// Sentence punctuation clings to the end of a URL far more often than it belongs
// to one, and a link inside brackets picks up the closer. Both come off; a
// closer that the URL itself opened (wikipedia's "(disambiguation)") stays.
fn trim_tail(text: &[char], body: usize, mut end: usize) -> usize {
	while end > body {
		let c = text[end - 1];
		if matches!(c, '.' | ',' | ';' | ':' | '!' | '?') {
			end -= 1;
			continue;
		}
		if let Some(open) = match c {
			')' => Some('('),
			']' => Some('['),
			_ => None,
		} {
			let opened = text[body..end].iter().filter(|&&x| x == open).count();
			let closed = text[body..end].iter().filter(|&&x| x == c).count();
			if closed > opened {
				end -= 1;
				continue;
			}
		}
		break;
	}
	end
}

// The link starting exactly at `start`, as a char range.
fn link_from(text: &[char], start: usize) -> Option<(usize, usize)> {
	if !boundary_before(text, start) {
		return None;
	}
	SCHEMES.iter().find_map(|&(scheme, slashes)| {
		if !matches_scheme(text, start, scheme) {
			return None;
		}
		let mut i = start + scheme.len();
		if text.get(i) != Some(&':') {
			return None;
		}
		i += 1;
		if slashes {
			if text.get(i) != Some(&'/') || text.get(i + 1) != Some(&'/') {
				return None;
			}
			i += 2;
		}
		let body = i;
		while text.get(i).is_some_and(|&c| is_url_char(c)) {
			i += 1;
		}
		let end = trim_tail(text, body, i);
		(end > body).then_some((start, end))
	})
}

/// The link covering char `hit`, as (start, end, url). `text` is one logical
/// line's chars; the caller maps the range back to grid cells.
pub fn find_at(text: &[char], hit: usize) -> Option<(usize, usize, String)> {
	if hit >= text.len() {
		return None;
	}
	let mut i = 0;
	while i < text.len() {
		match link_from(text, i) {
			Some((start, end)) => {
				if (start..end).contains(&hit) {
					return Some((start, end, text[start..end].iter().collect()));
				}
				i = end;
			}
			None => i += 1,
		}
	}
	None
}

/// Hand `url` to the desktop. `open_command` (config) overrides the platform
/// default: argv-split, the URL appended as the last argument. Runs detached -
/// the child is reaped on its own thread so a browser launch can't zombie.
pub fn open(url: &str, open_command: &str) -> io::Result<()> {
	let mut cmd = if open_command.trim().is_empty() {
		default_command(url)
	} else {
		let argv = crate::config::command_argv(open_command)
			.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad open command"))?;
		let (program, args) = argv
			.split_first()
			.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty open command"))?;
		let mut cmd = Command::new(program);
		cmd.args(args).arg(url);
		cmd
	};
	let mut child = cmd
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.spawn()?;
	std::thread::spawn(move || {
		let _ = child.wait();
	});
	Ok(())
}

// Which platform's opener to use. A parameter rather than a `cfg!` at the use
// site so every arm can be checked from any box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Host {
	Windows,
	MacOs,
	Other,
}

fn this_host() -> Host {
	if cfg!(target_os = "windows") {
		Host::Windows
	} else if cfg!(target_os = "macos") {
		Host::MacOs
	} else {
		Host::Other
	}
}

// The program that hands a URL to the desktop, and the URL as one plain argument.
//
// Windows deliberately does not go through `cmd /C start`. cmd's parser sees the
// whole command line before argument quoting means anything, and it expands
// percent variables before it processes any escaping - so a URL printed by a
// remote host could carry a variable in, and an expanded value holding `&` would
// start a second command. Terminal output is untrusted, and there is no escape
// that survives that order. explorer takes its arguments the ordinary way.
fn opener_argv(host: Host, url: &str) -> (&'static str, String) {
	let program = match host {
		Host::Windows => "explorer.exe",
		Host::MacOs => "open",
		Host::Other => "xdg-open",
	};
	(program, url.to_string())
}

fn default_command(url: &str) -> Command {
	let (program, arg) = opener_argv(this_host(), url);
	let mut cmd = Command::new(program);
	cmd.arg(arg);
	cmd
}

#[cfg(test)]
mod tests {
	use super::*;

	fn chars(s: &str) -> Vec<char> {
		s.chars().collect()
	}

	// Terminal output is untrusted, and the Windows opener used to go through cmd
	// - whose parser reads the command line before argument quoting means
	// anything, and expands percent variables before any escaping is processed.
	// Nothing escapes a URL for that, so nothing tries: every platform hands the
	// URL to its own opener as one plain argument.
	// Test ID: EpHYLCy
	#[test]
	fn a_url_reaches_the_opener_exactly_as_it_was_printed() {
		let nasty = "https://example.com/?a=%USERPROFILE%&b=x^y|z<>()\"'`$ q";
		for host in [Host::Windows, Host::MacOs, Host::Other] {
			let (program, arg) = opener_argv(host, nasty);
			assert_eq!(arg, nasty, "{host:?} altered the url");
			assert!(
				!program.contains("cmd") && !program.contains("sh"),
				"{host:?} opens through a shell ({program})"
			);
		}
		assert_eq!(opener_argv(Host::Windows, nasty).0, "explorer.exe");
	}

	// Find the link covering the first char of `needle`.
	fn at(line: &str, needle: &str) -> Option<String> {
		let text = chars(line);
		let hit = line[..line.find(needle).expect("needle")].chars().count();
		find_at(&text, hit).map(|(_, _, url)| url)
	}

	// Test ID: Em1S9yr
	#[test]
	fn a_plain_url_is_found_anywhere_inside_it() {
		let line = "see https://example.com/a?b=1&c=2 for more";
		let text = chars(line);
		let (start, end, url) = find_at(&text, 10).expect("link");
		assert_eq!(url, "https://example.com/a?b=1&c=2");
		assert_eq!((start, end), (4, 33));
		// every cell of the span answers with the same span
		for hit in start..end {
			assert_eq!(find_at(&text, hit).map(|l| l.2), Some(url.clone()));
		}
		assert_eq!(
			find_at(&text, start - 1),
			None,
			"the space before is not one"
		);
		assert_eq!(find_at(&text, end), None, "the space after is not one");
	}

	// Test ID: Em1S9ys
	#[test]
	fn sentence_punctuation_and_brackets_come_off_the_end() {
		assert_eq!(
			at("visit http://example.com/x.", "http"),
			Some("http://example.com/x".into())
		);
		assert_eq!(
			at("(see http://example.com/x)", "http"),
			Some("http://example.com/x".into())
		);
		// a closer the URL itself opened is part of it
		assert_eq!(
			at("http://en.wikipedia.org/wiki/Ruby_(gem)", "http"),
			Some("http://en.wikipedia.org/wiki/Ruby_(gem)".into())
		);
		assert_eq!(
			at("quoted 'https://example.com/q' here", "https"),
			Some("https://example.com/q".into())
		);
	}

	// The allowlist is the security boundary, not a convenience: a scheme that
	// isn't listed must never become clickable.
	// Test ID: Em1S9yt
	#[test]
	fn only_allowlisted_schemes_are_links() {
		assert_eq!(at("javascript:alert(1)", "javascript"), None);
		assert_eq!(at("data:text/html;base64,AAAA", "data"), None);
		assert_eq!(at("vbscript:msgbox", "vbscript"), None);
		assert_eq!(
			at("mail me at mailto:a@b.com now", "mailto"),
			Some("mailto:a@b.com".into())
		);
	}

	// Test ID: Em1S9yu
	#[test]
	fn ordinary_text_with_a_colon_is_not_a_link() {
		assert_eq!(at("aspect ratio 3:4 here", "3:4"), None);
		assert_eq!(at("C:\\Users\\jim\\file.txt", "C:"), None);
		assert_eq!(at("std::vec::Vec", "std"), None);
		assert_eq!(at("error: http", "http"), None, "no scheme separator");
		assert_eq!(at("xhttps://example.com", "https"), None, "mid-word start");
		assert_eq!(at("https://", "https"), None, "no body");
	}

	// Test ID: Em1S9yv
	#[test]
	fn the_scheme_is_case_insensitive_and_several_links_coexist() {
		assert_eq!(
			at("HTTPS://Example.COM/A", "HTTPS"),
			Some("HTTPS://Example.COM/A".into())
		);
		let line = "a http://one.example b ftp://two.example c";
		assert_eq!(at(line, "http"), Some("http://one.example".into()));
		assert_eq!(at(line, "ftp"), Some("ftp://two.example".into()));
		assert_eq!(at(line, " c"), None);
	}

	// A hostile scheme is the whole reason detection is allowlisted, so it gets a
	// directed test as well as the fuzz below. These are the ones that turn a
	// printed line into code: two of them run script in a browser, one runs it in
	// the Windows shell, and the rest reach a handler no terminal should offer.
	// Test ID: EpQN0oK
	#[test]
	fn a_hostile_scheme_never_becomes_a_link() {
		#[rustfmt::skip]
		let schemes = [
			"javascript", "JavaScript", "data", "vbscript", "jar", "about",
			"chrome", "view-source", "ms-msdt", "search-ms", "shell", "res",
			"blob", "filesystem", "intent", "smb",
			// An allowlisted scheme with something hidden inside it. A zero-width
			// character reads as nothing on screen and as a different scheme here.
			"htt\u{200b}ps", "ht\u{feff}tps", "http\u{ad}s",
		];
		for scheme in schemes {
			for body in ["//example.com/x", ":alert(1)", "//x"] {
				let line = format!("{scheme}:{body}");
				let text: Vec<char> = line.chars().collect();
				for hit in 0..text.len() {
					assert!(
						find_at(&text, hit).is_none(),
						"{line:?} was offered as a link at char {hit}"
					);
				}
			}
		}
	}

	// A printed line is untrusted text, and the only thing standing between it and
	// a process launch is the scan below. So the scan gets hammered: whatever it
	// hands back must be a whole allowlisted URL, taken verbatim out of the row,
	// and made of nothing but the characters a URL may carry.
	mod fuzz {
		use super::super::{SCHEMES, find_at, is_url_char};
		use crate::fuzz;

		// Rows built from the pieces that sit around a link in real output: the
		// schemes themselves, near-misses, the delimiters that end one, and the
		// brackets and punctuation the tail trimmer has to reason about.
		#[rustfmt::skip]
		const PIECES: [&str; 30] = [
			"http", "https", "HTTPS", "ftp", "ftps", "sftp", "ssh", "file",
			"mailto", "javascript", "data", "httpss", "xhttp", ":", "//", "/",
			"://", ".", ",", ";", "?", "!", ")", "(", "[", "]", " ", "a1",
			"example.com", "\u{4e2d}",
		];

		fn row(rng: &mut fuzz::Rng) -> Vec<u8> {
			let mut out = String::new();
			for _ in 0..rng.below(24) {
				if rng.chance(6) {
					out.push_str(&fuzz::text(rng));
				} else {
					out.push_str(rng.pick(&PIECES));
				}
			}
			out.into_bytes()
		}

		fn check(line: &[u8]) {
			let text: Vec<char> = String::from_utf8_lossy(line).chars().collect();
			for hit in 0..text.len() {
				let Some((start, end, url)) = find_at(&text, hit) else {
					continue;
				};
				assert!(start < end && end <= text.len(), "range {start}..{end}");
				assert!(
					(start..end).contains(&hit),
					"{hit} is outside {start}..{end}"
				);
				assert_eq!(
					url,
					text[start..end].iter().collect::<String>(),
					"the url is not what was on the row"
				);
				let lower = url.to_ascii_lowercase();
				assert!(
					SCHEMES.iter().any(|&(scheme, slashes)| {
						let want = if slashes {
							format!("{scheme}://")
						} else {
							format!("{scheme}:")
						};
						lower.starts_with(&want) && lower.len() > want.len()
					}),
					"{url:?} is not an allowlisted scheme"
				);
				assert!(
					url.chars().skip_while(|&c| c != ':').all(is_url_char),
					"{url:?} carries a character a url may not"
				);
			}
		}

		// Test ID: EpQN0oL
		#[test]
		fn only_an_allowlisted_url_is_ever_offered_to_the_opener() {
			let corpus = fuzz::corpus("links");
			for case in &corpus {
				check(case);
			}
			fuzz::soak("links", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				check(&fuzz::input(&mut rng, &corpus, row));
			});
		}
	}
}
