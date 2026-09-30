// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! What gets set up in the shells this terminal starts: a directory-reporting
//! block in PowerShell profiles, and a git-aware prompt for both PowerShell and
//! bash.
//!
//! The PowerShell half:
//!
//! Every other shell moves its own process when it moves, so the operating
//! system can be asked where it is and nothing needs setting up (see `cwd.rs`).
//! PowerShell keeps its location to itself, so it has to say where it is - and
//! asking every user to paste a block into a file to make new tabs open in the
//! right place is a poor trade when the block can be put there for them.
//!
//! What that licenses is narrow, and the limits are the design:
//!
//! - Only a profile that reports NOTHING is touched. Our own marker, or any
//!   other OSC 7 / OSC 9;9 already in the file (a Windows Terminal setup, say),
//!   means somebody has this in hand and the file is left alone.
//! - Only ever APPENDED to, after a copy is kept beside it, and only once - the
//!   marker is what makes a second launch a no-op. The one exception is the
//!   block ITSELF, which is kept current in place: it gains things over time
//!   (the version prompt did), and an install that only ever appends would
//!   leave everyone who already had it on the first version forever. That edit
//!   is only safe because the region is delimited by our own two markers and
//!   was written by us - which is exactly the signal a stored shell entry
//!   lacks, and why THAT list may only ever be added to.
//! - Deleting the block is how it is switched off. A note beside the config
//!   records which profiles were written to, so a profile that carried the block
//!   and no longer does is left alone - without it the next launch simply put the
//!   block back, since the marker went with it. `shell.integration` switches the
//!   whole thing off before it starts.
//! - A profile that does not decode as UTF-8 is left alone: 5.1 writes UTF-16 by
//!   default, and appending to a file we cannot read replaces it.
//! - A shell that would refuse to load the profile is left alone. Measured on
//!   this box: Windows PowerShell 5.1 sits at a policy that blocks script
//!   files, so a profile written for it turned every launch into a red
//!   execution-policy error. Writing a file a shell cannot read is worse than
//!   doing nothing, and changing somebody's execution policy is not ours to do.
//! - It runs on the shell-scan thread, well after the window is up, because it
//!   asks PowerShell itself where its profile is - which means starting one.
//!
//! The block also carries the prompt, rather than pointing at a script written
//! beside the config the way the bash half does. A prompt is drawn after every
//! command, and on Windows starting a process that often is not free.
//!
//! The bash half is a much smaller thing, and deliberately so. It is off by
//! default. When on, bash picks up `PROMPT_COMMAND` from its environment, and
//! that sets PS1 before every prompt, so it replaces a PS1 from the rc files -
//! which Debian's own files set, so yielding to one would mean never showing.
//! An rc file that sets a `PROMPT_COMMAND` of its own still wins. Nothing is
//! written into anyone's rc file, and switching it off is a setting rather
//! than an uninstall.

use std::path::{Path, PathBuf};

use crate::config;
use crate::shells::Found;

// The block, and the marker that says it is already there. Compiled in so the
// binary is the one source of it - `shell-integration.md` documents the same
// text for anyone adding it by hand, and a test holds the two together.
//
// It is written out as plain UTF-8 with no byte-order mark, and Windows
// PowerShell 5.1 reads such a file as ANSI - so the block itself has to stay
// ASCII, and the glyphs its prompt draws are spelled as code points.
pub const SNIPPET: &str = include_str!("shell_integration.ps1");
// Named rather than spelled at each use: this module compares and rewrites
// line endings constantly, and an escape is easy to get subtly wrong.
const LF: &str = "\n";
const CRLF: &str = "\r\n";
const NL: char = '\n';

pub const MARKER: &str = "# >>> SilkTerm shell integration >>>";
pub const END_MARKER: &str = "# <<< SilkTerm shell integration <<<";

// A file that already carries either sequence is reporting - by our block or by
// somebody else's setup - and is not ours to edit.
pub fn already_reports(profile: &str) -> bool {
	profile.contains(MARKER) || profile.contains("]9;9;") || profile.contains("]7;file:")
}

// The profile with the block on the end, separated by a blank line and starting
// on one of its own. Existing content is never rewritten, only followed.
pub fn with_block(profile: &str, newline: &str) -> String {
	let block = SNIPPET.replace("\r\n", "\n").replace('\n', newline);
	if profile.trim().is_empty() {
		return block;
	}
	let mut out = profile.to_string();
	if !out.ends_with('\n') {
		out.push_str(newline);
	}
	out.push_str(newline);
	out.push_str(&block);
	out
}

// The PowerShell programs among a scan's findings, one per program. A shell is
// named by its argv, so the program is the first word of it; the no-startup-file
// twins collapse into the same program, and their profile is the same file.
pub fn powershells(found: &[Found]) -> Vec<String> {
	let mut out: Vec<String> = Vec::new();
	for entry in found {
		let Ok(argv) = crate::cli::shell_split(&entry.command) else {
			continue;
		};
		let Some(program) = argv.first() else {
			continue;
		};
		if !is_powershell(program) {
			continue;
		}
		// Windows hands the same file back under more than one spelling of its
		// path (%SystemRoot% is C:\WINDOWS, PATH says C:\Windows), and each
		// one costs a shell launched to ask it the same question and a second
		// copy of the same diagnostic.
		let same = |seen: &String| {
			if cfg!(windows) {
				seen.eq_ignore_ascii_case(program)
			} else {
				seen == program
			}
		};
		if !out.iter().any(same) {
			out.push(program.clone());
		}
	}
	out
}

// Is this program a PowerShell? Matched on the base name the way the shell
// table is (lowercased, `.exe` dropped), so a full path answers the same as a
// bare name - and `pwsh-preview` and the like answer yes as well.
fn is_powershell(program: &str) -> bool {
	// split on both separators: a Windows path reaches this on any platform, and
	// Path would hand back the whole string for one on unix
	let base = program
		.rsplit(['/', '\\'])
		.next()
		.unwrap_or(program)
		.to_ascii_lowercase();
	let base = base.strip_suffix(".exe").unwrap_or(&base);
	base == "powershell" || base == "pwsh" || base.starts_with("pwsh-")
}

// Put the block in every PowerShell profile that reports nothing. Called on the
// shell-scan thread; every failure is a diagnostic, never a stop - a profile we
// cannot read or write is somebody else's business.
pub fn install(found: &[Found]) {
	if !config::settings().shell_integration {
		return;
	}
	let mut done: Vec<PathBuf> = Vec::new();
	for program in powershells(found) {
		let Some((profile, policy)) = ask_shell(&program) else {
			continue;
		};
		if !policy_runs_scripts(&policy) {
			let answer = if policy.is_empty() {
				"no answer"
			} else {
				&policy
			};
			eprintln!(
				"{}: {program} will not run profile scripts ({answer}), so its profile was left alone - see shell-integration.md",
				config::APP_NAME
			);
			continue;
		}
		// two PowerShells can share a profile; only look at each file once
		if done.contains(&profile) {
			continue;
		}
		done.push(profile.clone());
		install_into(&profile);
	}
}

// Ask a PowerShell where its own profile is. There is no way to work this out
// from outside - the Documents folder it sits under can be redirected, and per
// host and per version it differs - so the shell is asked, which means starting
// one. On Windows that starts a console with it, hence CREATE_NO_WINDOW: a
// console flashing over the terminal a few seconds after launch would be a
// mystery to anyone who saw it.
fn ask_shell(program: &str) -> Option<(PathBuf, String)> {
	query_shell(program, PROFILE_QUERY)
}

// Both facts in one launch: where the profile is, and whether this shell would
// even run it. A piped answer is written in the console's code page, IBM437 on
// an English Windows, so a path outside ASCII came back with U+FFFD in it and
// the block went into a new file no shell reads. The path comes back as the hex
// of its UTF-8 bytes instead, which no code page can change.
const PROFILE_QUERY: &str =
	"[BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($PROFILE)); Get-ExecutionPolicy";

fn query_shell(program: &str, query: &str) -> Option<(PathBuf, String)> {
	let mut command = std::process::Command::new(program);
	command.args(["-NoProfile", "-NonInteractive", "-Command", query]);
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		const CREATE_NO_WINDOW: u32 = 0x0800_0000;
		command.creation_flags(CREATE_NO_WINDOW);
	}
	// The exit status is deliberately not consulted. On a locked-down box the
	// policy question ANSWERS ITSELF by failing: Get-ExecutionPolicy lives in a
	// module whose manifest is a script file, so Restricted stops it loading and
	// the shell exits non-zero - while the profile path still comes back on
	// stdout. Reading only the status would turn that into silence, which is
	// how this first went wrong.
	let output = command.output().ok()?;
	parse_answer(&String::from_utf8_lossy(&output.stdout))
}

// Anything that is not the hex this asked for writes nothing, since a guess at
// a path is how a profile nobody loads gets created.
fn parse_answer(answer: &str) -> Option<(PathBuf, String)> {
	let mut lines = answer
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty());
	let bytes = lines
		.next()?
		.split('-')
		.map(|pair| {
			(pair.len() == 2)
				.then(|| u8::from_str_radix(pair, 16).ok())
				.flatten()
		})
		.collect::<Option<Vec<u8>>>()?;
	let path = String::from_utf8(bytes).ok()?;
	let policy = lines.next().unwrap_or_default().to_string();
	Some((PathBuf::from(path), policy))
}

// Would this shell actually load a profile it found? A block written into a
// profile the shell then refuses to run is worse than none at all: measured
// here, Windows PowerShell 5.1 sits at Restricted, and a profile written for it
// turned every launch into a red execution-policy error. An answer we do not
// recognise - including no answer at all - is treated as "no": the cost of
// being wrong that way is a feature that does not switch itself on, against an
// error on somebody's every prompt.
fn policy_runs_scripts(policy: &str) -> bool {
	matches!(
		policy.trim().to_ascii_lowercase().as_str(),
		"remotesigned" | "unrestricted" | "bypass"
	)
}

// The profile with our block brought up to date, or None when there is nothing
// to do - no block of ours in the file, or the one there is already current.
// Only the text BETWEEN the two markers is touched; whatever the user wrote
// above or below it is carried through untouched. An opening marker with no
// closing one is not a block we finished writing, so it is not a region we may
// replace either.
pub fn refreshed_block(profile: &str, newline: &str) -> Option<String> {
	let start = profile.find(MARKER)?;
	let end_marker = profile[start..].find(END_MARKER)? + start;
	let end = profile[end_marker..]
		.find(NL)
		.map_or(profile.len(), |nl| end_marker + nl + 1);
	let block = SNIPPET.replace(CRLF, LF).replace(NL, newline);
	if profile[start..end].replace(CRLF, LF) == block.replace(CRLF, LF) {
		return None;
	}
	Some(format!("{}{block}{}", &profile[..start], &profile[end..]))
}

// What is in a profile now, ready to be edited. `Ok(None)` is "nothing there
// yet". An error means the file is not ours to touch: a profile that does not
// decode as UTF-8 used to arrive here as an empty string, and appending to that
// replaced the whole file. Windows PowerShell 5.1 writes UTF-16 by default, so
// that is an ordinary profile rather than a broken one.
fn read_profile(profile: &Path) -> Result<Option<String>, String> {
	match std::fs::read(profile) {
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(format!("could not read {}: {e}", profile.display())),
		Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
			format!(
				"{} is not UTF-8 - left it alone, add the block by hand (see shell-integration.md)",
				profile.display()
			)
		}),
	}
}

// Keep what is there under a name that says where it came from, and never over a
// backup already made - that one is the copy worth keeping. Only a plain file
// counts as that copy. A link at the name, dangling or not, is not followed: a
// profile often holds tokens, and the copy gets the profile's own mode.
fn backup_once(profile: &Path, existing: &str) -> bool {
	use std::io::Write;
	if existing.trim().is_empty() {
		return true;
	}
	let backup = profile.with_extension("ps1.silkterm-backup");
	let refuse = |why: String| {
		eprintln!(
			"{}: could not back up {}: {why} - left it alone",
			config::APP_NAME,
			profile.display()
		);
		false
	};
	match std::fs::symlink_metadata(&backup) {
		Ok(meta) if meta.is_file() => return true,
		Ok(_) => return refuse(format!("{} is not a plain file", backup.display())),
		Err(_) => {}
	}
	let mut opts = std::fs::OpenOptions::new();
	opts.write(true).create_new(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
	let mut file = match opts.open(&backup) {
		Ok(file) => file,
		Err(e) => return refuse(e.to_string()),
	};
	#[cfg(unix)]
	if let Ok(meta) = std::fs::metadata(profile) {
		let _ = file.set_permissions(meta.permissions());
	}
	if let Err(e) = file.write_all(existing.as_bytes()) {
		drop(file);
		let _ = std::fs::remove_file(&backup);
		return refuse(e.to_string());
	}
	true
}

// The settings file's writer: an interrupted write cannot leave a profile
// half-replaced, a linked profile is written through its link, the mode is kept,
// and no link at a temp name is written through. A read-only profile is somebody
// saying no, so it is refused rather than replaced.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
	if std::fs::metadata(path).is_ok_and(|meta| meta.permissions().readonly()) {
		return Err("it is read-only".to_string());
	}
	config::write_config_atomic(path, text)
}

// Profiles we have written to before, one path per line, kept beside the config.
//
// Deleting the block is documented as how to switch this off, and without a note
// of our own it was not: with the block gone there is no marker and no reporting
// sequence, so the next launch put it straight back. A profile on this list that
// no longer carries the block was emptied on purpose.
fn installed_record() -> Option<PathBuf> {
	Some(config::data_dir()?.join("shell-integration.installed"))
}

fn already_installed(record: Option<&Path>, profile: &Path) -> bool {
	let Some(record) = record else {
		return false;
	};
	let Ok(text) = std::fs::read_to_string(record) else {
		return false;
	};
	let want = profile.to_string_lossy();
	text.lines().any(|line| line.trim() == want)
}

fn note_installed(record: Option<&Path>, profile: &Path) {
	let Some(record) = record else {
		return;
	};
	if already_installed(Some(record), profile) {
		return;
	}
	if let Some(parent) = record.parent() {
		let _ = std::fs::create_dir_all(parent);
	}
	let line = format!("{}\n", profile.display());
	let opened = std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(record);
	if let Ok(mut file) = opened {
		use std::io::Write;
		let _ = file.write_all(line.as_bytes());
	}
}

fn install_into(profile: &Path) {
	install_into_with(profile, installed_record().as_deref());
}

fn install_into_with(profile: &Path, record: Option<&Path>) {
	let existing = match read_profile(profile) {
		Ok(text) => text.unwrap_or_default(),
		Err(why) => {
			eprintln!("{}: {why}", config::APP_NAME);
			return;
		}
	};
	// a profile is read by the platform's own shell, so it gets the platform's
	// line ending rather than whatever the compiled-in copy carries
	let newline = if cfg!(windows) { CRLF } else { LF };
	// already ours: the only thing left to do is bring it up to date
	if existing.contains(MARKER) {
		// An earlier build may have put it there without noting it, and a deleted
		// block with no note comes straight back.
		note_installed(record, profile);
		if let Some(updated) = refreshed_block(&existing, newline) {
			if !backup_once(profile, &existing) {
				return;
			}
			match write_atomic(profile, &updated) {
				Ok(()) => eprintln!(
					"{}: updated the shell integration block in {}",
					config::APP_NAME,
					profile.display()
				),
				Err(e) => eprintln!(
					"{}: could not write {}: {e}",
					config::APP_NAME,
					profile.display()
				),
			}
		}
		return;
	}
	if already_reports(&existing) {
		return;
	}
	// put there once and taken out since: that is how this is switched off
	if already_installed(record, profile) {
		return;
	}
	if !backup_once(profile, &existing) {
		return;
	}
	if let Some(parent) = profile.parent() {
		if let Err(e) = std::fs::create_dir_all(parent) {
			eprintln!("{}: {}: {e}", config::APP_NAME, parent.display());
			return;
		}
	}
	match write_atomic(profile, &with_block(&existing, newline)) {
		Ok(()) => {
			note_installed(record, profile);
			eprintln!(
				"{}: added shell integration to {} - new tabs and panes will open where the shell is (see shell-integration.md)",
				config::APP_NAME,
				profile.display()
			);
		}
		Err(e) => eprintln!(
			"{}: could not write {}: {e}",
			config::APP_NAME,
			profile.display()
		),
	}
}

// The prompt script itself, compiled in so the binary is the one source of it.
// x9ps1-git is a separate MIT project of the same author; this is a copy of its
// `bin/x9ps1-git`, and the version it carries is in its own header.
const BASH_PROMPT: &str = include_str!("x9ps1-git.bash");

// What the script is called once it is on disk. No extension, because it is
// also perfectly usable by hand from a PATH directory.
const BASH_PROMPT_FILE: &str = "x9ps1-git";

// Is this program bash? Same base-name matching as `is_powershell`, so Git Bash
// and a full path both answer yes, while `sh` (which may well be dash) does not.
fn is_bash(program: &str) -> bool {
	let base = program
		.rsplit(['/', '\\'])
		.next()
		.unwrap_or(program)
		.to_ascii_lowercase();
	base.strip_suffix(".exe").unwrap_or(&base) == "bash"
}

// The PROMPT_COMMAND a bash pane is given, for a script sitting at `path`.
//
// `$BASH` is bash's own path, so the script runs under the same bash the pane
// does with no dependency on what is on PATH and no execute bit needed. The
// path is spelled with forward slashes and single-quoted, which every bash
// takes - including a Windows one, where a backslash inside quotes would
// otherwise arrive as an escape.
fn prompt_command(path: &Path) -> String {
	let quoted = path
		.display()
		.to_string()
		.replace('\\', "/")
		.replace('\'', "'\\''");
	format!("PS1=$(\"$BASH\" '{quoted}')")
}

// Put the script in the data directory, once per run, and say where it is.
// Rewritten whenever it differs, so an updated SilkTerm carries an updated
// prompt rather than leaving the first copy standing forever.
fn bash_prompt_path() -> Option<&'static Path> {
	static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
	PATH.get_or_init(|| {
		let dir = config::data_dir()?;
		let path = dir.join(BASH_PROMPT_FILE);
		if std::fs::read_to_string(&path).is_ok_and(|held| held == BASH_PROMPT) {
			return Some(path);
		}
		std::fs::create_dir_all(&dir).ok()?;
		match std::fs::write(&path, BASH_PROMPT) {
			Ok(()) => Some(path),
			Err(e) => {
				eprintln!(
					"{}: could not write {}: {e}",
					config::APP_NAME,
					path.display()
				);
				None
			}
		}
	})
	.as_deref()
}

// The environment a pane about to run `command` should start with, on top of
// what it inherits. Empty for anything that is not bash, and for a bash pane
// when the setting is off.
pub fn pane_env(command: Option<&[String]>) -> Vec<(String, String)> {
	if !config::settings().bash_prompt {
		return Vec::new();
	}
	let Some(program) = command.and_then(<[String]>::first) else {
		return Vec::new();
	};
	if !is_bash(program) {
		return Vec::new();
	}
	bash_prompt_path()
		.map(|path| vec![("PROMPT_COMMAND".to_string(), prompt_command(path))])
		.unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::{
		BASH_PROMPT, END_MARKER, LF, MARKER, SNIPPET, already_reports, is_bash, is_powershell,
		powershells, prompt_command, refreshed_block, with_block,
	};
	use crate::shells::Found;

	// sh may well be dash, and a shell named by a full path is the ordinary case
	// on Windows - so both have to answer the way a bare `bash` does.
	// Test ID: EoTbwMS
	#[test]
	fn only_bash_is_offered_the_bash_prompt() {
		assert!(is_bash("bash"));
		assert!(is_bash("/usr/bin/bash"));
		assert!(is_bash("C:\\Program Files\\Git\\bin\\bash.exe"));
		assert!(!is_bash("sh"));
		assert!(!is_bash("zsh"));
		assert!(!is_bash("wsl.exe"));
	}

	// Windows answers in the console's code page, so a path is only believed as
	// the hex the query asks for. An empty profile gives an empty first line, and
	// the policy that then comes first is not hex.
	// Test ID: Eq4upT6
	#[test]
	fn a_profile_path_is_read_from_its_hex_and_nothing_else() {
		let jose =
			"C:\\Users\\Jos\u{e9}\\Documents\\WindowsPowerShell\\Microsoft.PowerShell_profile.ps1";
		let hex = jose
			.bytes()
			.map(|byte| format!("{byte:02X}"))
			.collect::<Vec<_>>()
			.join("-");
		let (path, policy) = super::parse_answer(&format!("{hex}\r\nRemoteSigned\r\n")).unwrap();
		assert_eq!(path, std::path::PathBuf::from(jose));
		assert_eq!(policy, "RemoteSigned");
		// what the old query got back on vm925w: the path itself, in IBM437
		let mangled = String::from_utf8_lossy(b"C:\\Users\\Jos\x82\\Documents\r\nRemoteSigned\r\n");
		assert_eq!(super::parse_answer(&mangled), None);
		assert_eq!(super::parse_answer("\r\nRemoteSigned\r\n"), None);
		assert_eq!(
			super::parse_answer("C3-28\r\nRemoteSigned\r\n"),
			None,
			"not UTF-8"
		);
		assert_eq!(super::parse_answer(""), None);
	}

	// The same through each real PowerShell installed. On Windows this is the
	// case that failed, in both 5.1 and 7; elsewhere it checks the query parses.
	// Test ID: Eq4upT7
	#[test]
	fn a_powershell_names_a_profile_outside_ascii() {
		let query = super::PROFILE_QUERY.replace(
			"$PROFILE",
			"('C:\\Users\\Jos' + [char]0xE9 + '\\Documents\\profile.ps1')",
		);
		let want = std::path::PathBuf::from("C:\\Users\\Jos\u{e9}\\Documents\\profile.ps1");
		for program in ["pwsh", "powershell.exe"] {
			let runs = std::process::Command::new(program)
				.args(["-NoProfile", "-NonInteractive", "-Command", "exit 0"])
				.output()
				.is_ok_and(|out| out.status.success());
			if !runs {
				eprintln!("no {program} here, skipped");
				continue;
			}
			let (path, _) = super::query_shell(program, &query).expect("an answer");
			assert_eq!(path, want, "{program}");
		}
	}

	// It replaces a PS1 set in .bashrc, and Debian's own files set one, so it is
	// on only for somebody who asked for it.
	// Test ID: Eq4uQkq
	#[test]
	fn the_bash_prompt_is_off_until_asked_for() {
		assert!(!crate::config::Settings::default().bash_prompt);
	}

	// The value is handed to bash as a command string, so a Windows path has to
	// arrive as something bash reads rather than as a run of escapes.
	// Test ID: EoTbwMT
	#[test]
	fn a_prompt_command_survives_a_windows_path() {
		let win = prompt_command(std::path::Path::new("C:\\Users\\me\\x9ps1-git"));
		assert_eq!(win, "PS1=$(\"$BASH\" 'C:/Users/me/x9ps1-git')");
		let unix = prompt_command(std::path::Path::new("/home/me/.config/silkterm/x9ps1-git"));
		assert_eq!(
			unix,
			"PS1=$(\"$BASH\" '/home/me/.config/silkterm/x9ps1-git')"
		);
	}

	// The compiled-in copy is what gets written out and then run by bash, so a
	// truncated or mangled vendoring should not reach anybody's prompt.
	// Test ID: EoTbwMU
	#[test]
	fn the_bash_prompt_script_is_a_whole_script() {
		assert!(BASH_PROMPT.starts_with("#!/bin/bash"));
		assert!(BASH_PROMPT.contains("x9ps1-git v"));
		assert!(BASH_PROMPT.contains("fMain"));
	}

	// Anyone who publishes a repository picks its branch names, and bash expands
	// the prompt text at every prompt. A branch named `$(cmd)` ran cmd in any
	// bash pane opened in a clone.
	// Test ID: EpxrTRY
	#[cfg(unix)]
	#[test]
	fn a_branch_name_is_shown_and_never_run_by_the_prompt() {
		use std::process::Command;
		let dir = crate::testdir::run_dir().join(format!("silkterm_x9ps1_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		let repo = dir.join("repo");
		std::fs::create_dir_all(&repo).expect("temp dir");
		let script = dir.join(super::BASH_PROMPT_FILE);
		std::fs::write(&script, BASH_PROMPT).expect("write script");

		let branch = "$(touch${IFS}PWNED1)`touch${IFS}PWNED2`${HOME}";
		let remote = "https://example.com/$(touch PWNED3)/`touch PWNED4`/a\\b.git";
		let run = |program: &str, args: &[&str]| {
			let out = Command::new(program)
				.args(args)
				.current_dir(&repo)
				.env("GIT_CONFIG_GLOBAL", "/dev/null")
				.env("GIT_CONFIG_NOSYSTEM", "1")
				.env_remove("GIT_DIR")
				.env_remove("GIT_WORK_TREE")
				.env_remove("GIT_INDEX_FILE")
				.env_remove("X9PS1_STANDARD")
				.output()
				.unwrap_or_else(|e| panic!("run {program}: {e}"));
			assert!(out.status.success(), "{program} {args:?}: {out:?}");
			String::from_utf8_lossy(&out.stdout).into_owned()
		};
		run("git", &["init", "-q", "-b", branch]);
		let identity = ["-c", "user.name=test", "-c", "user.email=test@example.com"];
		run(
			"git",
			&[
				&identity[..],
				&["commit", "-q", "--allow-empty", "-m", "test"],
			]
			.concat(),
		);
		run("git", &["config", "remote.origin.url", remote]);

		// What the pane's PROMPT_COMMAND does, then the expansion bash does to show it
		let shown = run(
			"bash",
			&[
				"--noprofile",
				"--norc",
				"-c",
				r#"PS1=$("$BASH" "$1") && printf '%s' "${PS1@P}""#,
				"bash",
				&script.to_string_lossy(),
			],
		);
		let ran: Vec<_> = std::fs::read_dir(&repo)
			.expect("read repo")
			.filter_map(Result::ok)
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.filter(|name| name.starts_with("PWNED"))
			.collect();
		let _ = std::fs::remove_dir_all(&dir);

		assert!(ran.is_empty(), "the prompt ran commands: {ran:?}");
		assert!(
			shown.contains(branch),
			"branch not shown as text: {shown:?}"
		);
		assert!(
			shown.contains(remote),
			"remote not shown as text: {shown:?}"
		);
	}

	// The git part used to need a remote named origin, and read its marks from
	// git's English status text, so it never showed a count.
	// Test ID: EqBmHyq
	#[cfg(unix)]
	#[test]
	fn the_prompt_shows_any_repository_and_how_far_it_is_from_upstream() {
		use std::path::Path;
		use std::process::Command;
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_x9ps1git_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		let script = dir.join(super::BASH_PROMPT_FILE);
		std::fs::write(&script, BASH_PROMPT).expect("write script");

		let run = |cwd: &Path, program: &str, args: &[&str]| {
			let out = Command::new(program)
				.args(args)
				.current_dir(cwd)
				.env("GIT_CONFIG_GLOBAL", "/dev/null")
				.env("GIT_CONFIG_NOSYSTEM", "1")
				.env("GIT_AUTHOR_NAME", "test")
				.env("GIT_AUTHOR_EMAIL", "test@example.com")
				.env("GIT_COMMITTER_NAME", "test")
				.env("GIT_COMMITTER_EMAIL", "test@example.com")
				.env_remove("GIT_DIR")
				.env_remove("GIT_WORK_TREE")
				.env_remove("GIT_INDEX_FILE")
				.env_remove("X9PS1_STANDARD")
				.output()
				.unwrap_or_else(|e| panic!("run {program}: {e}"));
			assert!(out.status.success(), "{program} {args:?}: {out:?}");
			String::from_utf8_lossy(&out.stdout).into_owned()
		};
		let git = |cwd: &Path, args: &[&str]| run(cwd, "git", args);
		let commit =
			|cwd: &Path, msg: &str| git(cwd, &["commit", "-q", "--allow-empty", "-m", msg]);

		// No remote at all
		git(&dir, &["init", "-q", "-b", "lonebranch", "lone"]);

		// A clone whose remote is not origin, two ahead of its upstream and one behind
		let up = dir.join("up.git");
		let up_str = up.to_string_lossy().into_owned();
		git(&dir, &["init", "-q", "--bare", "-b", "main", "up.git"]);
		git(&dir, &["init", "-q", "-b", "main", "other"]);
		let other = dir.join("other");
		commit(&other, "one");
		git(&other, &["push", "-q", &up_str, "main"]);
		git(&dir, &["clone", "-q", "-o", "upstream", &up_str, "tracked"]);
		commit(&other, "two");
		git(&other, &["push", "-q", &up_str, "main"]);
		let tracked = dir.join("tracked");
		git(&tracked, &["fetch", "-q", "upstream"]);
		commit(&tracked, "three");
		commit(&tracked, "four");

		let shown = |cwd: &Path| {
			run(
				cwd,
				"bash",
				&[
					"--noprofile",
					"--norc",
					"-c",
					r#"PS1=$("$BASH" "$1") && printf '%s' "${PS1@P}""#,
					"bash",
					&script.to_string_lossy(),
				],
			)
		};
		let lone = shown(&dir.join("lone"));
		let far = shown(&tracked);
		let _ = std::fs::remove_dir_all(&dir);

		assert!(
			lone.contains("lonebranch"),
			"no git part without a remote: {lone:?}"
		);
		assert!(
			far.contains(&up_str),
			"remote not named origin not shown: {far:?}"
		);
		assert!(
			far.contains("\u{2191}2\u{2193}1"),
			"no ahead and behind count: {far:?}"
		);
	}

	// A profile is a script run at every shell start, and it often holds tokens.
	// The write used to replace a linked profile with a copy at the umask's mode,
	// and wrote through a link left at its temp or backup name.
	// Test ID: Eq4qaCX
	#[cfg(unix)]
	#[test]
	fn a_profile_write_keeps_its_link_and_mode_and_follows_no_planted_link() {
		use std::os::unix::fs::{PermissionsExt, symlink};
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_intlink_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		let record = dir.join("shell-integration.installed");
		let name = "Microsoft.PowerShell_profile.ps1";
		let before = "Set-Alias ll Get-ChildItem\n";
		let mode =
			|path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
		let make = |sub: &str, perms: u32| {
			let path = dir.join(sub).join(name);
			std::fs::create_dir_all(path.parent().unwrap()).unwrap();
			std::fs::write(&path, before).unwrap();
			std::fs::set_permissions(&path, std::fs::Permissions::from_mode(perms)).unwrap();
			path
		};

		// linked, and private
		let real = make("dotfiles", 0o600);
		let linked = dir.join("linked").join(name);
		std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
		symlink(&real, &linked).unwrap();
		super::install_into_with(&linked, Some(&record));
		assert!(
			std::fs::symlink_metadata(&linked).unwrap().is_symlink(),
			"the link was replaced by a copy"
		);
		assert!(
			std::fs::read_to_string(&real).unwrap().contains(MARKER),
			"the linked file did not get the block"
		);
		assert_eq!(mode(&real), 0o600, "the profile's mode changed");
		let backup = linked.with_extension("ps1.silkterm-backup");
		assert_eq!(mode(&backup), 0o600, "the backup is readable by others");

		// read-only means no
		let locked = make("locked", 0o400);
		super::install_into_with(&locked, Some(&record));
		assert_eq!(std::fs::read_to_string(&locked).unwrap(), before);
		assert_eq!(mode(&locked), 0o400);

		// a link at the old temp name
		let victim = dir.join("victim");
		std::fs::write(&victim, "victim\n").unwrap();
		let planted = make("planted", 0o644);
		symlink(&victim, planted.with_extension("ps1.silkterm-new")).unwrap();
		super::install_into_with(&planted, Some(&record));
		assert_eq!(std::fs::read_to_string(&victim).unwrap(), "victim\n");
		assert!(!std::fs::symlink_metadata(&planted).unwrap().is_symlink());
		assert!(std::fs::read_to_string(&planted).unwrap().contains(MARKER));

		// a dangling link at the backup name
		let nowhere = dir.join("nowhere");
		let dangling = make("dangling", 0o644);
		symlink(&nowhere, dangling.with_extension("ps1.silkterm-backup")).unwrap();
		super::install_into_with(&dangling, Some(&record));
		assert!(!nowhere.exists(), "the backup went through a link");
		assert_eq!(
			std::fs::read_to_string(&dangling).unwrap(),
			before,
			"a profile that could not be backed up was written anyway"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Builds before the record existed, beta3 included, put the block in without
	// noting it. Deleting such a block put it straight back at the next launch.
	// Test ID: Eq4qaCY
	#[test]
	fn a_block_already_there_is_noted_so_deleting_it_sticks() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_intnote_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		let own = "Set-Alias ll Get-ChildItem\n";
		let current = with_block(own, LF);
		let stale = format!("{own}\n{MARKER}\nWrite-Host 'an older block'\n{END_MARKER}\n");
		for (label, text) in [("current", current), ("stale", stale)] {
			let profile = dir.join(format!("{label}.ps1"));
			let record = dir.join(format!("{label}.installed"));
			std::fs::write(&profile, text).unwrap();
			super::install_into_with(&profile, Some(&record));
			std::fs::write(&profile, own).unwrap();
			super::install_into_with(&profile, Some(&record));
			assert_eq!(
				std::fs::read_to_string(&profile).unwrap(),
				own,
				"a {label} block came back after it was deleted"
			);
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The block's own comment used to invite a host color added inside the
	// markers, which the next refresh deleted with no copy kept.
	// Test ID: Eq4qaCZ
	#[test]
	fn a_host_color_is_set_where_a_refresh_leaves_it() {
		assert!(!SNIPPET.contains("Add your own"));
		assert!(SNIPPET.contains("$global:SilkTermHostColor"));
		let mine = "$SilkTermHostColor = '1;33'\n";
		let stale = format!("{mine}\n{MARKER}\nWrite-Host 'an older block'\n{END_MARKER}\n");
		let updated = refreshed_block(&stale, LF).expect("a stale block");
		assert!(updated.starts_with(mine));
	}

	fn a_pwsh() -> Option<&'static str> {
		["pwsh", "pwsh.exe"].into_iter().find(|program| {
			std::process::Command::new(program)
				.args(["-NoProfile", "-NonInteractive", "-Command", "exit 0"])
				.output()
				.is_ok_and(|out| out.status.success())
		})
	}

	// Runs the block the way a profile does, where a PowerShell is installed. The
	// hook holds a delegate that `&` cannot call, so an earlier handler broke every
	// directory change, and a second load wrapped its own wrapper. Both arms are
	// run: the 5.1 one by forcing the test that picks it.
	// Test ID: Eq4qaCa
	#[test]
	fn the_block_keeps_an_earlier_hook_and_survives_loading_twice() {
		let Some(pwsh) = a_pwsh() else {
			eprintln!("no pwsh here, skipped");
			return;
		};
		let dir = crate::testdir::run_dir().join(format!("silkterm_intps_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		// output is piped here, which the block reads as not a terminal
		let block = SNIPPET.replace("-not [Console]::IsOutputRedirected", "$true");
		let hook = "if ($null -ne $ExecutionContext.SessionState.InvokeCommand.PSObject.Properties['LocationChangedAction'])";
		assert!(block.contains(hook));
		let target = crate::testdir::run_dir();
		let target = target.to_string_lossy();
		for (arm, text) in [
			("hook", block.clone()),
			("wrap", block.replace(hook, "if ($false)")),
		] {
			let path = dir.join(format!("{arm}.ps1"));
			std::fs::write(&path, text).unwrap();
			let quoted = path.to_string_lossy().replace('\'', "''");
			let script = format!(
				"$ErrorActionPreference = 'Continue'
$ExecutionContext.SessionState.InvokeCommand.LocationChangedAction = {{ Write-Host 'mine' }}
$SilkTermHostColor = '1;33'
. '{quoted}'
. '{quoted}'
Write-Host 'LOADED'
Set-Location -LiteralPath '{}'
$null = prompt
Write-Host \"COLOR=$global:__SilkTermHostColor\"",
				target.replace('\'', "''")
			);
			let out = std::process::Command::new(pwsh)
				.args(["-NoProfile", "-NonInteractive", "-Command", &script])
				.output()
				.expect("run pwsh");
			let stdout = String::from_utf8_lossy(&out.stdout);
			let stderr = String::from_utf8_lossy(&out.stderr);
			assert!(stderr.trim().is_empty(), "{arm}: {stderr}");
			let after = stdout.split("LOADED").nth(1).expect("the block loaded");
			let reports = after.matches("]9;9;").count();
			if arm == "hook" {
				assert_eq!(after.matches("mine").count(), 1, "{arm}: {stdout}");
				// the change, and nothing from a prompt the hook leaves alone
				assert_eq!(reports, 1, "{arm}: {stdout}");
			} else {
				assert_eq!(reports, 1, "{arm}: {stdout}");
			}
			assert!(after.contains("COLOR=1;33"), "{arm}: {stdout}");
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The PowerShell prompt is a port of the bash one and should read the same.
	// It named only an origin remote and showed no count, a day after bash got both.
	// Test ID: EqFy7VA
	#[test]
	fn the_powershell_prompt_shows_any_repository_and_how_far_it_is_from_upstream() {
		use std::path::Path;
		use std::process::Command;
		let Some(pwsh) = a_pwsh() else {
			eprintln!("no pwsh here, skipped");
			return;
		};
		let dir = crate::testdir::run_dir().join(format!("silkterm_psgit_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		let block = dir.join("block.ps1");
		std::fs::write(
			&block,
			SNIPPET.replace("-not [Console]::IsOutputRedirected", "$true"),
		)
		.unwrap();

		let run = |cwd: &Path, program: &str, args: &[&str]| {
			let out = Command::new(program)
				.args(args)
				.current_dir(cwd)
				.env(
					"GIT_CONFIG_GLOBAL",
					if cfg!(windows) { "NUL" } else { "/dev/null" },
				)
				.env("GIT_CONFIG_NOSYSTEM", "1")
				.env("GIT_AUTHOR_NAME", "test")
				.env("GIT_AUTHOR_EMAIL", "test@example.com")
				.env("GIT_COMMITTER_NAME", "test")
				.env("GIT_COMMITTER_EMAIL", "test@example.com")
				.env_remove("GIT_DIR")
				.env_remove("GIT_WORK_TREE")
				.env_remove("GIT_INDEX_FILE")
				.env_remove("X9PS1_STANDARD")
				.output()
				.unwrap_or_else(|e| panic!("run {program}: {e}"));
			assert!(out.status.success(), "{program} {args:?}: {out:?}");
			String::from_utf8_lossy(&out.stdout).into_owned()
		};
		let git = |cwd: &Path, args: &[&str]| run(cwd, "git", args);
		let commit =
			|cwd: &Path, msg: &str| git(cwd, &["commit", "-q", "--allow-empty", "-m", msg]);

		// No remote at all
		git(&dir, &["init", "-q", "-b", "lonebranch", "lone"]);

		// A clone whose remote is not origin, two ahead of its upstream and one behind,
		// with an origin added after that the branch does not track
		let up = dir.join("up.git");
		let up_str = up.to_string_lossy().into_owned();
		git(&dir, &["init", "-q", "--bare", "-b", "main", "up.git"]);
		git(&dir, &["init", "-q", "-b", "main", "other"]);
		let other = dir.join("other");
		commit(&other, "one");
		git(&other, &["push", "-q", &up_str, "main"]);
		git(&dir, &["clone", "-q", "-o", "upstream", &up_str, "tracked"]);
		commit(&other, "two");
		git(&other, &["push", "-q", &up_str, "main"]);
		let tracked = dir.join("tracked");
		git(&tracked, &["fetch", "-q", "upstream"]);
		git(
			&tracked,
			&["remote", "add", "origin", "https://example.com/decoy.git"],
		);
		commit(&tracked, "three");
		commit(&tracked, "four");

		// A branch with no upstream, where origin is the one to name
		git(&dir, &["init", "-q", "-b", "main", "untracked"]);
		let untracked = dir.join("untracked");
		git(
			&untracked,
			&["remote", "add", "aaa", "https://example.com/first.git"],
		);
		git(
			&untracked,
			&["remote", "add", "origin", "https://example.com/origin.git"],
		);

		let shown = |cwd: &Path| {
			let quoted = |path: &Path| path.to_string_lossy().replace('\'', "''");
			let script = format!(
				". '{}'\nSet-Location -LiteralPath '{}'\nWrite-Host 'PROMPT'\n__SilkTermPrompt",
				quoted(&block),
				quoted(cwd)
			);
			let out = run(
				&dir,
				pwsh,
				&["-NoProfile", "-NonInteractive", "-Command", &script],
			);
			out.split("PROMPT").nth(1).unwrap_or_default().to_string()
		};
		let lone = shown(&dir.join("lone"));
		let far = shown(&tracked);
		let no_upstream = shown(&untracked);
		let _ = std::fs::remove_dir_all(&dir);

		assert!(
			lone.contains("lonebranch"),
			"no git part without a remote: {lone:?}"
		);
		assert!(
			far.contains(&up_str) && !far.contains("decoy"),
			"not the remote the branch tracks: {far:?}"
		);
		assert!(
			far.contains("\u{2191}2\u{2193}1"),
			"no ahead and behind count: {far:?}"
		);
		assert!(
			no_upstream.contains("example.com/origin.git") && !no_upstream.contains('\u{2191}'),
			"origin not preferred with no upstream: {no_upstream:?}"
		);
	}

	fn found(title: &str, command: &str) -> Found {
		Found::new(title, command.to_string(), "")
	}

	// A profile that already reports is not ours to edit, whoever set it up -
	// and this is also what makes a second launch a no-op rather than a second
	// copy of the block.
	// Test ID: EnalvtQ
	#[test]
	fn a_profile_that_already_reports_is_left_alone() {
		assert!(already_reports(&with_block("", "\n")), "our own block");
		assert!(
			already_reports("Write-Host \"$e]9;9;`\"$p`\"$e\\\""),
			"somebody else's OSC 9;9"
		);
		assert!(
			already_reports("printf '\\033]7;file://%s%s' $h $p"),
			"somebody else's OSC 7"
		);
		assert!(!already_reports(
			"# just a profile\nSet-Alias ll Get-ChildItem"
		));
		assert!(!already_reports(""));
	}

	// What is there is followed, never rewritten - so a prompt the profile sets
	// up further down is the one this block wraps.
	// Test ID: EnalvtR
	#[test]
	fn the_block_lands_at_the_end_and_keeps_what_was_there() {
		let before = "Import-Module Cows\r\nSet-Alias ll Get-ChildItem\r\n";
		let after = with_block(before, "\r\n");
		assert!(after.starts_with(before), "existing content moved");
		assert!(after.contains(MARKER));
		assert!(after.ends_with(&SNIPPET.replace("\r\n", "\n").replace('\n', "\r\n")));
		// a file with no trailing newline still gets the block on its own line
		let joined = with_block("Set-Alias ll Get-ChildItem", "\n");
		assert!(joined.contains("Get-ChildItem\n\n# >>> SilkTerm"));
		// and an empty profile is just the block
		assert_eq!(with_block("   \n", "\n"), SNIPPET.replace("\r\n", "\n"));
	}

	// PowerShell 5.1 writes UTF-16 by default, so a profile that does not decode
	// as UTF-8 is an ordinary one. It used to read back as an empty string, which
	// skipped the backup and replaced the file with the block alone.
	// Test ID: EpHO1lo
	#[test]
	fn a_profile_that_is_not_utf8_is_left_alone() {
		let dir = crate::testdir::run_dir().join(format!("silkterm_int16_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		let record = dir.join("shell-integration.installed");
		let profile = dir.join("Microsoft.PowerShell_profile.ps1");
		// "Set-Alias ll Get-ChildItem" as UTF-16LE with a byte-order mark
		let mut bytes = vec![0xff, 0xfe];
		for unit in "Set-Alias ll Get-ChildItem\r\n".encode_utf16() {
			bytes.extend_from_slice(&unit.to_le_bytes());
		}
		std::fs::write(&profile, &bytes).expect("write profile");

		super::install_into_with(&profile, Some(&record));

		assert_eq!(
			std::fs::read(&profile).expect("read profile"),
			bytes,
			"the profile is untouched"
		);
		assert!(
			!dir.join("Microsoft.PowerShell_profile.ps1.silkterm-new")
				.exists(),
			"no half-written file left beside it"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Deleting the block is documented as how to switch this off, in four places
	// including the block's own first line. It was not: with the block gone there
	// was no marker to match and the next launch put it straight back.
	// Test ID: EpHWuEq
	#[test]
	fn a_deleted_block_stays_deleted() {
		let dir = crate::testdir::run_dir().join(format!("silkterm_intoff_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("temp dir");
		let profile = dir.join("Microsoft.PowerShell_profile.ps1");
		let record = dir.join("shell-integration.installed");
		std::fs::write(&profile, "Set-Alias ll Get-ChildItem\n").expect("write profile");

		super::install_into_with(&profile, Some(&record));
		assert!(
			std::fs::read_to_string(&profile).unwrap().contains(MARKER),
			"the block went in"
		);

		// the user takes it out again
		std::fs::write(&profile, "Set-Alias ll Get-ChildItem\n").expect("write profile");
		super::install_into_with(&profile, Some(&record));
		assert_eq!(
			std::fs::read_to_string(&profile).unwrap(),
			"Set-Alias ll Get-ChildItem\n",
			"and stays out"
		);

		// a profile we have never touched still gets it
		let other = dir.join("other_profile.ps1");
		std::fs::write(&other, "# theirs\n").expect("write profile");
		super::install_into_with(&other, Some(&record));
		assert!(
			std::fs::read_to_string(&other).unwrap().contains(MARKER),
			"a profile we have not written to before is not affected"
		);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The whole of what a launch does to a file that is not ours: keep a copy,
	// follow what is there, and never do it twice.
	// Test ID: EnalvtS
	#[test]
	fn a_profile_is_backed_up_once_and_added_to_once() {
		let dir = crate::testdir::run_dir().join("silkterm-integration-test");
		let _ = std::fs::remove_dir_all(&dir);
		// its own record, never the live one: install_into would write into the
		// user's data directory
		let record = dir.join("shell-integration.installed");
		std::fs::create_dir_all(&dir).expect("temp dir");
		let profile = dir.join("Microsoft.PowerShell_profile.ps1");
		let before = "Set-Alias ll Get-ChildItem\n";
		std::fs::write(&profile, before).expect("write profile");

		super::install_into_with(&profile, Some(&record));
		let after = std::fs::read_to_string(&profile).expect("read profile");
		assert!(after.starts_with(before), "what was there survived");
		assert!(after.contains(MARKER), "the block went in");
		let backup = dir.join("Microsoft.PowerShell_profile.ps1.silkterm-backup");
		assert_eq!(
			std::fs::read_to_string(&backup).ok().as_deref(),
			Some(before),
			"the copy beside it is the file as it was"
		);

		// a second launch is a no-op, and cannot overwrite the copy either
		std::fs::write(&profile, format!("{after}# a line the user added\n")).unwrap();
		super::install_into_with(&profile, Some(&record));
		let twice = std::fs::read_to_string(&profile).expect("read profile");
		assert_eq!(twice.matches(MARKER).count(), 1, "a second block went in");
		assert!(
			twice.ends_with("# a line the user added\n"),
			"their line went"
		);
		assert_eq!(
			std::fs::read_to_string(&backup).ok().as_deref(),
			Some(before),
			"the copy was overwritten"
		);

		// and a profile that never existed is created with just the block
		let fresh = dir.join("fresh").join("Microsoft.PowerShell_profile.ps1");
		super::install_into_with(&fresh, Some(&record));
		assert!(
			std::fs::read_to_string(&fresh).unwrap().contains(MARKER),
			"a missing profile was not created"
		);
		assert!(!fresh.with_extension("ps1.silkterm-backup").exists());
		let _ = std::fs::remove_dir_all(&dir);
	}

	// A block in a profile the shell will not run is worse than no block: it is
	// an execution-policy error on every launch, which is what a first cut of
	// this did to Windows PowerShell 5.1 on the box it was written on.
	// Test ID: EnalvtT
	#[test]
	fn a_shell_that_will_not_run_scripts_keeps_its_profile() {
		for allowed in ["RemoteSigned", "Unrestricted", "Bypass", " bypass "] {
			assert!(super::policy_runs_scripts(allowed), "{allowed}");
		}
		for blocked in ["Restricted", "AllSigned", "Undefined", "", "who knows"] {
			assert!(!super::policy_runs_scripts(blocked), "{blocked}");
		}
	}

	// Only PowerShell needs this: every other shell moves its own process, so
	// the OS can be asked. A full path answers the same as a bare name.
	// Test ID: EnalvtU
	#[test]
	fn only_the_powershells_are_offered_a_profile() {
		assert!(is_powershell("pwsh"));
		assert!(is_powershell("pwsh.exe"));
		assert!(is_powershell(r"C:\Program Files\PowerShell\7\pwsh.exe"));
		assert!(is_powershell("PowerShell.EXE"));
		assert!(is_powershell("/usr/bin/pwsh"));
		assert!(is_powershell("pwsh-preview"));
		assert!(!is_powershell("bash"));
		assert!(!is_powershell("cmd.exe"));
		assert!(!is_powershell("powershell-ise.exe"), "a different program");

		// one entry per PROGRAM: the no-startup-file twin is the same shell with
		// the same profile, and a scan lists both
		let scan = [
			found("Bash", "/bin/bash"),
			found("PowerShell 7", r"C:\pwsh.exe"),
			found("PowerShell 7 (no profile)", r"C:\pwsh.exe -NoProfile"),
			found("Windows PowerShell 5", "powershell.exe"),
		];
		assert_eq!(
			powershells(&scan),
			vec![r"C:\pwsh.exe".to_string(), "powershell.exe".to_string()]
		);
	}

	// The block gains things over time, so an install that only ever appended
	// would leave anyone who already has it on whatever version they first got.
	// Test ID: EnbYSzy
	#[test]
	fn an_existing_block_is_brought_up_to_date_in_place() {
		let stale = format!(
			"# mine, above\n\n{MARKER}\nWrite-Host 'an older block'\n{END_MARKER}\n\n# mine, below\n"
		);
		let updated = refreshed_block(&stale, LF).expect("a stale block needs replacing");
		assert!(
			updated.starts_with("# mine, above\n"),
			"lost what was above"
		);
		assert!(updated.ends_with("# mine, below\n"), "lost what was below");
		assert!(
			!updated.contains("an older block"),
			"kept the old block: {updated}"
		);
		assert!(updated.contains(SNIPPET.replace("\r\n", LF).trim_end()));
		// ...and having done it once, there is nothing left to do.
		assert_eq!(refreshed_block(&updated, LF), None);
	}

	// Test ID: EnbYSzz
	#[test]
	fn a_profile_with_no_block_of_ours_is_not_rewritten() {
		assert_eq!(refreshed_block("# just my own profile\n", LF), None);
		assert_eq!(
			refreshed_block(&format!("{MARKER}\nhalf a block\n"), LF),
			None
		);
	}

	// A profile with no byte-order mark is read as ANSI by Windows PowerShell
	// 5.1, so a single accented character or box-drawing glyph in here arrives
	// mangled on the one version that cannot be told otherwise.
	// Test ID: EoTnK8W
	#[test]
	fn the_block_is_plain_ascii() {
		let stray: String = SNIPPET.chars().filter(|c| !c.is_ascii()).collect();
		assert!(stray.is_empty(), "non-ascii in the block: {stray}");
	}

	// The prompt is part of the block rather than a script beside it, and both
	// halves of the version split have to reach it.
	// Test ID: EoTnK8X
	#[test]
	fn the_block_carries_the_prompt() {
		assert!(SNIPPET.contains("function global:__SilkTermPrompt"));
		assert!(SNIPPET.contains("git status --porcelain=v2 --branch"));
		// the 6+ hook branch, and the 5.1 wrap
		assert_eq!(SNIPPET.matches("__SilkTermPrompt }").count(), 2);
	}

	// A light check beside a heavy cross looked mismatched in the prompt. The
	// light pair is the same weight, and neither is one an emoji face takes.
	// Test ID: Er2UJef
	#[test]
	fn the_prompt_marks_are_the_light_pair() {
		let glyphs = SNIPPET
			.lines()
			.find(|l| l.contains("$global:__SilkTermGlyphs = @{"))
			.expect("the glyph table");
		assert!(glyphs.contains("Yes = [string][char]0x2713;"), "{glyphs}");
		assert!(glyphs.contains("No = [string][char]0x2717;"), "{glyphs}");
		assert!(!SNIPPET.contains("0x2714") && !SNIPPET.contains("0x2718"));
	}

	// The block people are told to paste in by hand has to be the block that
	// gets installed, or one of the two quietly stops being true.
	// Test ID: EnalvtV
	#[test]
	fn the_documented_snippet_is_the_one_that_is_installed() {
		let doc = include_str!("../../shell-integration.md").replace("\r\n", "\n");
		let snippet = SNIPPET.replace("\r\n", "\n");
		assert!(
			doc.contains(snippet.trim_end()),
			"shell-integration.md no longer carries the snippet verbatim"
		);
	}
}
