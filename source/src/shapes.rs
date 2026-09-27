// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// What a double-click takes when the text under the pointer is a shape we can
// name: a URL, a Windows or posix path, an scp target. Word selection cuts those
// in the wrong places - a space in a folder name ends the path early, a `:12`
// line number gets dragged in, a bracket in a wiki URL splits it - so a shape
// that is recognized here is taken whole and outranks both the pair rule and the
// word rule.
//
// Everything a shape can start with is anchored: a drive letter, a UNC prefix, a
// leading slash or `~/`. Nothing else is a path, which is what keeps prose out.

// How far past a space a path separator may sit and still read as part of the
// same path. A folder name runs to a few words; prose does not reach a slash
// that soon.
const SPACE_REACH: usize = 40;

// The longest run of characters after a dot that still reads as a file
// extension. Long enough for the ones people use, short enough that a sentence
// ending in a dot plus a word does not qualify.
const EXT_MAX: usize = 8;

/// The shape covering char `hit`, as a char range, or None when the text there
/// is just text. `text` is one logical line's chars.
pub fn span_at(text: &[char], hit: usize) -> Option<(usize, usize)> {
	if hit >= text.len() {
		return None;
	}
	url_span(text, hit).or_else(|| path_span(text, hit))
}

fn url_span(text: &[char], hit: usize) -> Option<(usize, usize)> {
	crate::links::find_at(text, hit).map(|(start, end, _)| (start, end))
}

fn is_sep(c: char) -> bool {
	c == '\\' || c == '/'
}

// Characters a path may carry. Whitespace ends a run (crossing one is decided
// separately) and the rest are the ones Windows forbids in a name outright plus
// the double quote a shell would have wrapped the path in.
fn is_path_char(c: char) -> bool {
	!c.is_whitespace() && !c.is_control() && !matches!(c, '"' | '<' | '>' | '|' | '*' | '?')
}

// A shape cannot start mid-word: "xC:\a" is not a drive path and "and/or" is not
// an absolute one. Listed the other way round on purpose - what may sit in front
// of a path is a short closed set, and everything else is part of a name.
fn boundary_before(text: &[char], start: usize) -> bool {
	match start.checked_sub(1).and_then(|i| text.get(i)) {
		Some(&c) => {
			c.is_whitespace()
				|| matches!(
					c,
					'"' | '\'' | '(' | '[' | '{' | '<' | '=' | ',' | ';' | '|' | '`'
				)
		}
		None => true,
	}
}

// How a shape starts, and whether a file extension is allowed to end it.
struct Anchor {
	prefix: usize,
	ext_ends: bool,
}

// Does a path start exactly at `at`?
fn anchor(text: &[char], at: usize) -> Option<Anchor> {
	if !boundary_before(text, at) {
		return None;
	}
	let path = |prefix| {
		Some(Anchor {
			prefix,
			ext_ends: true,
		})
	};
	let c = *text.get(at)?;
	// C:\ or C:/
	if c.is_ascii_alphabetic()
		&& text.get(at + 1) == Some(&':')
		&& text.get(at + 2).is_some_and(|&c| is_sep(c))
	{
		return path(3);
	}
	// \\server\share
	if c == '\\' && text.get(at + 1) == Some(&'\\') && text.get(at + 2).is_some_and(|&c| !is_sep(c))
	{
		return path(2);
	}
	// ~/ and ~\
	if c == '~' && text.get(at + 1).is_some_and(|&c| is_sep(c)) {
		return path(2);
	}
	// /usr/... - a lone slash is not a path, so the next char has to carry one
	if c == '/'
		&& text
			.get(at + 1)
			.is_some_and(|&c| is_path_char(c) && !is_sep(c))
	{
		return path(1);
	}
	// git@github.com:owner/repo.git, and the userless spelling a git prompt shows.
	// The `.git` does NOT end it: a prompt writes the branch after it as
	// `repo.git:dev`, and that whole field is the thing being clicked on.
	remote_len(text, at).map(|prefix| Anchor {
		prefix,
		ext_ends: false,
	})
}

// How much of an scp target starting at `at` is prefix - through the colon that
// separates the host from the path. None when this is not one.
fn remote_len(text: &[char], at: usize) -> Option<usize> {
	let host = at + user_len(text, at).unwrap_or(0);
	let colon = host_end(text, host)?;
	// The first segment has to be absent or carry a letter, or `notes.txt:12/34`
	// reads as a remote.
	let mut i = colon + 1;
	while text.get(i).is_some_and(|&c| is_path_char(c) && !is_sep(c)) {
		i += 1;
	}
	let segment = &text[colon + 1..i];
	if !segment.is_empty() && !segment.iter().any(char::is_ascii_alphabetic) {
		return None;
	}
	// a separator with something after it - a host and a bare name is not a target
	if !text.get(i).is_some_and(|&c| is_sep(c))
		|| !text.get(i + 1).is_some_and(|&c| is_path_char(c))
	{
		return None;
	}
	Some(colon + 1 - at)
}

// Length of a `user@` prefix at `at`, if there is one.
fn user_len(text: &[char], at: usize) -> Option<usize> {
	let mut i = at;
	while text
		.get(i)
		.is_some_and(|&c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
	{
		i += 1;
	}
	(i > at && text.get(i) == Some(&'@')).then_some(i + 1 - at)
}

// Where a dotted hostname starting at `at` meets its colon. Two labels at least,
// and an alphabetic last one, so `e.g.:` and a bare `host:` are not hostnames.
fn host_end(text: &[char], at: usize) -> Option<usize> {
	let mut i = at;
	let mut labels = 0;
	loop {
		let start = i;
		while text
			.get(i)
			.is_some_and(|&c| c.is_ascii_alphanumeric() || c == '-')
		{
			i += 1;
		}
		if i == start {
			return None;
		}
		labels += 1;
		if text.get(i) == Some(&'.') {
			i += 1;
			continue;
		}
		let tld = i - start >= 2 && text[start..i].iter().all(char::is_ascii_alphabetic);
		return (labels >= 2 && tld && text.get(i) == Some(&':')).then_some(i);
	}
}

// The nearest path start at or before `hit` whose span still reaches `hit`.
fn path_span(text: &[char], hit: usize) -> Option<(usize, usize)> {
	(0..=hit).rev().find_map(|start| {
		let anchor = anchor(text, start)?;
		let end = path_end(text, start, &anchor);
		(end > hit && end > start + anchor.prefix).then_some((start, end))
	})
}

// Where the path starting at `start` ends. Runs of path characters, plus any
// space a folder name turns out to have in it, stopping at the file extension
// once there is one.
fn path_end(text: &[char], start: usize, anchor: &Anchor) -> usize {
	let mut end = start + anchor.prefix;
	loop {
		while text.get(end).is_some_and(|&c| is_path_char(c)) {
			end += 1;
		}
		// A `:` after the extension is a line number, not part of the name.
		if anchor.ext_ends
			&& let Some(cut) = ends_at_extension(&text[start..end])
		{
			return start + cut;
		}
		if text.get(end) != Some(&' ') || !separator_within_reach(text, end + 1) {
			break;
		}
		end += 1;
	}
	trim_tail(text, start, end)
}

// Is there a path separator close enough after `from` to believe the space we
// just met is inside a folder name rather than after the path?
fn separator_within_reach(text: &[char], from: usize) -> bool {
	if !text.get(from).is_some_and(|&c| is_path_char(c)) {
		return false;
	}
	text.iter()
		.skip(from)
		.take(SPACE_REACH)
		.take_while(|c| !c.is_control())
		.any(|&c| is_sep(c))
}

// Where the name ends if this run already carries a file extension, as an offset
// into the run. Only the last segment counts, and what FOLLOWS the extension
// decides: another dot means a second part (`a.tar.gz`), a separator means it was
// a directory all along, and anything else means the name ended and the `:120:5`
// after it is somebody's line number.
fn ends_at_extension(run: &[char]) -> Option<usize> {
	let last_sep = run.iter().rposition(|&c| is_sep(c))?;
	let mut i = last_sep + 1;
	while i < run.len() {
		if run[i] != '.' || i == last_sep + 1 {
			i += 1;
			continue;
		}
		let mut j = i + 1;
		while j < run.len() && run[j].is_ascii_alphanumeric() {
			j += 1;
		}
		let ext = j - i - 1;
		if ext == 0 || ext > EXT_MAX {
			i += 1;
			continue;
		}
		match run.get(j) {
			Some(&'.') => i = j,
			Some(&c) if is_sep(c) => return None,
			_ => return Some(j),
		}
	}
	None
}

// Sentence punctuation clings to the end of a path as readily as to a URL, and a
// path inside brackets or quotes picks up the closer.
fn trim_tail(text: &[char], start: usize, mut end: usize) -> usize {
	while end > start {
		let c = text[end - 1];
		if matches!(c, '.' | ',' | ';' | ':' | '!' | '?') {
			end -= 1;
			continue;
		}
		let opener = match c {
			')' => '(',
			']' => '[',
			'}' => '{',
			'\'' => '\'',
			_ => break,
		};
		let body = &text[start..end - 1];
		let opened = body.iter().filter(|&&x| x == opener).count();
		let closed = body.iter().filter(|&&x| x == c).count();
		if opener == c {
			if opened % 2 == 1 {
				break; // the path opened this quote itself
			}
		} else if opened > closed {
			break;
		}
		end -= 1;
	}
	end
}

#[cfg(test)]
mod tests {
	use super::*;

	fn at(line: &str, needle: &str) -> Option<String> {
		let text: Vec<char> = line.chars().collect();
		let hit = line.find(needle).expect("needle not in line");
		let hit = line[..hit].chars().count();
		span_at(&text, hit).map(|(s, e)| text[s..e].iter().collect())
	}

	// Test ID: Eo6l7kW
	#[test]
	fn a_drive_path_keeps_its_drive_letter() {
		assert_eq!(
			at("open C:\\Users\\jim\\notes.txt now", "jim").as_deref(),
			Some("C:\\Users\\jim\\notes.txt")
		);
		assert_eq!(
			at("C:/Users/jim/notes.txt", "Users").as_deref(),
			Some("C:/Users/jim/notes.txt")
		);
	}

	// The case word selection cannot do at all: the space ends the word, so a
	// double-click used to give back "Files\app.exe".
	// Test ID: Eo6l7kX
	#[test]
	fn a_folder_name_may_have_spaces_in_it() {
		assert_eq!(
			at("path C:\\Program Files\\app.exe end", "app").as_deref(),
			Some("C:\\Program Files\\app.exe")
		);
		assert_eq!(
			at("C:\\Program Files (x86)\\App\\a.exe more", "App").as_deref(),
			Some("C:\\Program Files (x86)\\App\\a.exe")
		);
	}

	// Prose after a path is prose, not more path - there is no separator in it.
	// Test ID: Eo6l7kY
	#[test]
	fn a_space_with_no_separator_after_it_ends_the_path() {
		assert_eq!(
			at("/home/u/Documents and more words", "Documents").as_deref(),
			Some("/home/u/Documents")
		);
		assert_eq!(
			at("see /etc/hosts for details", "etc").as_deref(),
			Some("/etc/hosts")
		);
	}

	// Test ID: Eo6l7kZ
	#[test]
	fn a_line_number_after_the_extension_is_not_part_of_the_name() {
		assert_eq!(
			at("/src/app/main.rs:120:5", "main").as_deref(),
			Some("/src/app/main.rs")
		);
		assert_eq!(
			at("/tmp/x/backup.tar.gz done", "backup").as_deref(),
			Some("/tmp/x/backup.tar.gz")
		);
	}

	// Test ID: Eo6l7ka
	#[test]
	fn a_url_wins_over_everything_else() {
		assert_eq!(
			at("see https://example.com/a?b=c fine", "example").as_deref(),
			Some("https://example.com/a?b=c")
		);
		assert_eq!(
			at("https://en.wikipedia.org/wiki/Foo_(bar)", "wiki").as_deref(),
			Some("https://en.wikipedia.org/wiki/Foo_(bar)")
		);
		assert_eq!(
			at("file:///C:/Users/jim/a.txt", "Users").as_deref(),
			Some("file:///C:/Users/jim/a.txt")
		);
	}

	// Test ID: Eo6l7kb
	#[test]
	fn a_unc_path_and_a_home_path_are_shapes_too() {
		assert_eq!(
			at("\\\\server\\share\\file.txt", "share").as_deref(),
			Some("\\\\server\\share\\file.txt")
		);
		assert_eq!(at("~/bin/tool.sh", "bin").as_deref(), Some("~/bin/tool.sh"));
	}

	// Test ID: Eo6l7kc
	#[test]
	fn ordinary_text_is_left_to_the_word_rules() {
		assert_eq!(at("this and/or that", "and"), None);
		assert_eq!(at("a ratio of 3/4 here", "ratio"), None);
		assert_eq!(at("no shapes at all", "shapes"), None);
		assert_eq!(at("xC:\\notapath here", "notapath"), None);
	}

	// Test ID: Eo6l7kd
	#[test]
	fn a_quoted_or_bracketed_path_sheds_the_wrapper() {
		assert_eq!(
			at("'/home/u/a b/c.txt'", "home").as_deref(),
			Some("/home/u/a b/c.txt")
		);
		assert_eq!(
			at("(/home/u/notes.md), yes", "notes").as_deref(),
			Some("/home/u/notes.md")
		);
	}

	// A git prompt puts the remote in brackets beside its status marks, and the
	// pair rule would otherwise hand back the marks with it.
	// Test ID: EofxyNU
	#[test]
	fn a_git_remote_is_a_shape() {
		assert_eq!(
			at("[ github.com:jim-collier/silkterm.git:dev X ]", "collier").as_deref(),
			Some("github.com:jim-collier/silkterm.git:dev")
		);
		// clicking the branch has to give the same run, or it falls through to the
		// pair rule and comes back with the status marks
		assert_eq!(
			at("[ github.com:jim-collier/silkterm.git:dev X ]", "dev").as_deref(),
			Some("github.com:jim-collier/silkterm.git:dev")
		);
		assert_eq!(
			at(
				"origin git@github.com:jim-collier/silkterm.git (fetch)",
				"silkterm"
			)
			.as_deref(),
			Some("git@github.com:jim-collier/silkterm.git")
		);
		assert_eq!(
			at("scp jim@nas.local:/srv/data/a.txt .", "srv").as_deref(),
			Some("jim@nas.local:/srv/data/a.txt")
		);
	}

	// Test ID: EofxyNV
	#[test]
	fn a_colon_after_a_word_is_not_a_remote() {
		assert_eq!(at("see notes.txt:12/34 there", "notes"), None);
		assert_eq!(at("build:release/x here", "release"), None);
		assert_eq!(at("e.g.:foo/bar", "foo"), None);
		assert_eq!(at("host.com:noslash here", "noslash"), None);
	}

	// Test ID: Eo6l7ke
	#[test]
	fn a_click_past_the_end_of_the_row_finds_nothing() {
		let text: Vec<char> = "/etc/hosts".chars().collect();
		assert_eq!(span_at(&text, 99), None);
	}

	// The span comes straight back as indexes into the row it was given, and the
	// row is whatever a program printed. A span that runs past the end of it
	// would be read out of bounds by the caller.
	mod fuzz {
		use super::super::span_at;
		use crate::fuzz;

		#[rustfmt::skip]
		const PIECES: [&str; 24] = [
			"/home/u", "C:\\Users", "\\\\srv\\share", "http://x.example", "1.2.3.4",
			"a@b.example", "12:34:56", "2026-09-09", "0x1f", "#ff8800", "1e9",
			"github.com:jim/x.git:dev", "..", ".", "/", "\\", ":", "-", "_",
			" ", "(", ")", "\u{4e2d}", "\u{1f600}",
		];

		// Test ID: EpQN0oO
		#[test]
		fn a_shape_span_never_runs_past_the_row_it_was_found_in() {
			let corpus = fuzz::corpus("shapes");
			fuzz::soak("shapes", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let case = fuzz::input(&mut rng, &corpus, |rng| {
					let mut line = String::new();
					for _ in 0..rng.below(20) {
						if rng.chance(5) {
							line.push_str(&fuzz::text(rng));
						} else {
							line.push_str(rng.pick(&PIECES));
						}
					}
					line.into_bytes()
				});
				let row: Vec<char> = String::from_utf8_lossy(&case).chars().collect();
				for hit in 0..row.len() {
					// The end is exclusive here, where the pair rule beside it in
					// pane.rs answers inclusive. The caller takes end - 1.
					let Some((a, b)) = span_at(&row, hit) else {
						continue;
					};
					assert!((a..b).contains(&hit), "{hit} is outside {a}..{b}");
					assert!(b <= row.len(), "{a}..{b} runs past a row of {}", row.len());
				}
			});
		}
	}
}
