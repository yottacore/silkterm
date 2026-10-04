// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The directory a shell SAYS it is in, and the tap that hears it.
//!
//! A new tab, split or window starts where the pane it came from is, which
//! means asking a shell where that is. The OS can answer for a shell that
//! moves itself (`/proc/<pid>/cwd`, `proc_pidinfo` on macOS, or the PEB on
//! Windows - see `term.rs`), and cannot answer at all for one that keeps its
//! own idea of where it is: PowerShell's `Set-Location` never tells the OS.
//! Every terminal has that hole and every terminal fills it the same way - the
//! shell announces the directory in an escape sequence and the terminal
//! listens.
//!
//! Both spellings in use are read. OSC 7 (`ESC ] 7 ; file://host/path`) is
//! what the unix shells emit, hostname and percent-encoding included. OSC 9;9
//! (`ESC ] 9 ; 9 ; C:\path`) is the `ConEmu` spelling that Windows Terminal
//! documents, so a PowerShell profile already set up for that terminal works
//! here unchanged - which is the whole reason for reading two.
//!
//! The listening is done by wrapping the PTY rather than by forking the VT
//! parser: `vte` handles neither sequence (OSC 7 goes to its `unhandled`
//! arm), but `EventedPty` is a public trait and `EventLoop` is generic over
//! it, so `TappedPty` sits in front of the real one and scans what it reads.
//! Nothing else about the stream changes - the bytes go on to the parser
//! exactly as they arrived.

use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event as PollEvent, PollMode, Poller};

// A payload longer than this is not a directory anybody meant to send, and the
// scanner must never grow a buffer to fit whatever an OSC 52 clipboard write
// happens to carry.
const MAX_PAYLOAD: usize = 4096;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

// What the shell last said about where it is. Written by the PTY reader thread
// and read by the window thread, so it is behind a lock - one taken only when a
// prompt reports and when a new tab/pane/window opens, never per byte and never
// per frame.
//
// On unix it also keeps who said it: the terminal's foreground process group
// when the report arrived. A zsh started from dash reports, then exits, and dash
// reports nothing - so the answer has to go with the program that gave it.
#[derive(Debug, Clone, Default)]
pub struct Reported {
	slot: Arc<Mutex<Option<(PathBuf, Option<u32>)>>>,
	#[cfg(unix)]
	tty: Option<std::os::unix::io::RawFd>,
}

impl Reported {
	// Reads the speaker from this PTY master. The fd stays valid for as long as
	// the loop that feeds the tap owns the PTY.
	#[cfg(unix)]
	pub fn for_tty(tty: std::os::unix::io::RawFd) -> Self {
		Self {
			tty: Some(tty),
			..Self::default()
		}
	}

	// The directory as last reported, or None where no shell has said anything,
	// whoever said it. The scan tests want that; the window wants `live`.
	#[cfg(test)]
	pub fn get(&self) -> Option<PathBuf> {
		Some(crate::locks::lock(&self.slot).as_ref()?.0.clone())
	}

	// The last report, while the program that sent it is still running.
	pub fn live(&self) -> Option<PathBuf> {
		let (dir, speaker) = crate::locks::lock(&self.slot).clone()?;
		speaker.is_none_or(still_running).then_some(dir)
	}

	fn set(&self, dir: PathBuf) {
		#[cfg(unix)]
		let speaker = self
			.tty
			// SAFETY: takes no pointer, and the fd is held open by the PTY (see `for_tty`).
			.map(|fd| unsafe { libc::tcgetpgrp(fd) })
			.filter(|pgid| *pgid > 0)
			.map(|pgid| pgid as u32);
		#[cfg(not(unix))]
		let speaker = None;
		self.set_from(dir, speaker);
	}

	fn set_from(&self, dir: PathBuf, speaker: Option<u32>) {
		*crate::locks::lock(&self.slot) = Some((dir, speaker));
	}
}

// A group leader's pid is the group id. EPERM still means someone is there.
#[cfg(unix)]
fn still_running(pid: u32) -> bool {
	let Ok(pid) = libc::pid_t::try_from(pid) else {
		return false;
	};
	// SAFETY: signal 0 sends nothing; it only asks whether the process exists.
	let answer = unsafe { libc::kill(pid, 0) };
	answer == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn still_running(_: u32) -> bool {
	true
}

// The PTY, with its read side scanned on the way past. Everything else is
// forwarded untouched, including the child-exit channel and resize.
pub struct TappedPty<P> {
	pty: P,
	scan: Scan,
	reported: Reported,
}

impl<P> std::fmt::Debug for TappedPty<P> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TappedPty")
			.field("reported", &self.reported)
			.finish_non_exhaustive()
	}
}

impl<P> TappedPty<P> {
	pub fn new(pty: P, reported: Reported) -> Self {
		Self {
			pty,
			scan: Scan::default(),
			reported,
		}
	}
}

// The tapped PTY IS its own reader: `reader()` hands back a borrow, and the
// wrapper cannot hold one of the inner reader without borrowing itself. The
// event loop takes the reader and the writer in separate statements, so this
// costs nothing.
impl<P: EventedReadWrite> Read for TappedPty<P> {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		let count = self.pty.reader().read(buf)?;
		self.scan.feed(&buf[..count], &self.reported);
		Ok(count)
	}
}

impl<P: EventedReadWrite> EventedReadWrite for TappedPty<P> {
	type Reader = Self;
	type Writer = P::Writer;

	unsafe fn register(
		&mut self,
		poll: &Arc<Poller>,
		interest: PollEvent,
		mode: PollMode,
	) -> io::Result<()> {
		// SAFETY: the caller's contract (the sources outlive the registration)
		// is passed straight through to the PTY that owns them.
		unsafe { self.pty.register(poll, interest, mode) }
	}

	fn reregister(
		&mut self,
		poll: &Arc<Poller>,
		interest: PollEvent,
		mode: PollMode,
	) -> io::Result<()> {
		self.pty.reregister(poll, interest, mode)
	}

	fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
		self.pty.deregister(poll)
	}

	fn reader(&mut self) -> &mut Self::Reader {
		self
	}

	fn writer(&mut self) -> &mut Self::Writer {
		self.pty.writer()
	}
}

impl<P: EventedPty> EventedPty for TappedPty<P> {
	fn next_child_event(&mut self) -> Option<ChildEvent> {
		self.pty.next_child_event()
	}
}

impl<P: OnResize> OnResize for TappedPty<P> {
	fn on_resize(&mut self, window_size: WindowSize) {
		self.pty.on_resize(window_size);
	}
}

// Where in an escape sequence the scanner is. A read returns whatever the pipe
// happened to hold, so a sequence arrives in as many pieces as it likes and the
// state has to survive between reads.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum State {
	#[default]
	Ground,
	Esc,
	// collecting a payload that still might be one of ours
	Osc,
	OscEsc,
	// past the point it could be: read to the terminator, storing nothing
	Skip,
	SkipEsc,
}

#[derive(Default)]
struct Scan {
	state: State,
	payload: Vec<u8>,
}

impl Scan {
	fn feed(&mut self, bytes: &[u8], reported: &Reported) {
		let mut rest = bytes;
		while !rest.is_empty() {
			// Ordinary output is the overwhelming majority of any stream, so
			// Ground skips to the next ESC in one pass rather than walking the
			// match arm per byte.
			if self.state == State::Ground {
				match rest.iter().position(|&byte| byte == ESC) {
					Some(at) => {
						self.state = State::Esc;
						rest = &rest[at + 1..];
					}
					None => return,
				}
				continue;
			}
			let byte = rest[0];
			rest = &rest[1..];
			self.step(byte, reported);
		}
	}

	fn step(&mut self, byte: u8, reported: &Reported) {
		self.state = match self.state {
			State::Ground if byte == ESC => State::Esc,
			State::Ground => State::Ground,
			State::Esc => match byte {
				b']' => {
					self.payload.clear();
					State::Osc
				}
				ESC => State::Esc,
				_ => State::Ground,
			},
			State::Osc => match byte {
				// BEL and ST both end an OSC
				BEL => {
					self.finish(reported);
					State::Ground
				}
				ESC => State::OscEsc,
				_ => {
					self.payload.push(byte);
					if self.payload.len() > MAX_PAYLOAD || !wanted(&self.payload) {
						self.payload.clear();
						State::Skip
					} else {
						State::Osc
					}
				}
			},
			State::OscEsc => match byte {
				b'\\' => {
					self.finish(reported);
					State::Ground
				}
				// an ESC that is not ST: malformed, so give up on this sequence
				// rather than guess where it ends
				ESC => State::Esc,
				_ => State::Skip,
			},
			State::Skip => match byte {
				BEL => State::Ground,
				ESC => State::SkipEsc,
				_ => State::Skip,
			},
			State::SkipEsc => match byte {
				b'\\' => State::Ground,
				ESC => State::Esc,
				_ => State::Skip,
			},
		};
	}

	fn finish(&mut self, reported: &Reported) {
		let payload = std::mem::take(&mut self.payload);
		if let Ok(text) = std::str::from_utf8(&payload) {
			if let Some(dir) = directory(text, local_host()) {
				reported.set(dir);
			}
		}
	}
}

// Could this payload still become one we care about? Anything else is dropped
// the moment it can be, so a title change or a clipboard write costs a couple
// of comparisons and no memory.
fn wanted(payload: &[u8]) -> bool {
	[b"7;".as_slice(), b"9;9;".as_slice()].iter().any(|prefix| {
		if payload.len() < prefix.len() {
			prefix.starts_with(payload)
		} else {
			payload.starts_with(prefix)
		}
	})
}

// The directory an OSC payload names, in either spelling. `local_host` is what
// an OSC 7 URL has to name to be believed.
pub fn directory(payload: &str, local_host: &str) -> Option<PathBuf> {
	if let Some(rest) = payload.strip_prefix("7;") {
		return from_url(rest, local_host);
	}
	// ConEmu's: a plain path, occasionally quoted, never encoded and with no
	// host to check - it is only ever emitted by a shell on this machine.
	let rest = payload.strip_prefix("9;9;")?;
	as_path(rest.trim_matches('"'))
}

// `file://<host>/<path>`, percent-encoded. A bare path is taken as well: it is
// not the spelling anyone documents, but it costs one line to accept and a
// shell that writes one plainly means the same thing.
fn from_url(url: &str, local_host: &str) -> Option<PathBuf> {
	let Some(rest) = url.strip_prefix("file://") else {
		return url
			.starts_with('/')
			.then(|| as_path(&decode(url)))
			.flatten();
	};
	// the host runs to the path's leading separator, which the path keeps
	let split = rest.find('/')?;
	let (host, path) = rest.split_at(split);
	// A directory on another machine is not one we can open. Rejecting it
	// matters most where the two agree by accident: an ssh session whose remote
	// path also exists locally would otherwise open a pane in the wrong one.
	if !(host.is_empty()
		|| host.eq_ignore_ascii_case("localhost")
		|| host.eq_ignore_ascii_case(local_host))
	{
		return None;
	}
	as_path(&decode(path))
}

// A URL path to a native one. On Windows that means dropping the separator that
// precedes a drive letter (`/C:/x`) and turning the rest around.
fn as_path(text: &str) -> Option<PathBuf> {
	// An OSC payload is untrusted text, and this is the funnel both spellings pass
	// through. A control character is turned away here because a reported
	// directory is shown as well as used: it names the tab and reaches the window
	// title, where the desktop draws it as text. A directory really called that is
	// possible on unix and vanishingly rare, and refusing one only means the new
	// pane starts where the old one did.
	//
	// Whether the path is absolute is NOT asked here. A pane on Windows can report
	// a posix path, and wsl.exe wants exactly that spelling; usable_cwd is the one
	// gate on what becomes a working directory.
	if text.is_empty() || text.chars().any(char::is_control) {
		return None;
	}
	#[cfg(windows)]
	{
		let bytes = text.as_bytes();
		let text = if bytes.len() >= 3
			&& bytes[0] == b'/'
			&& bytes[1].is_ascii_alphabetic()
			&& bytes[2] == b':'
		{
			&text[1..]
		} else {
			text
		};
		Some(PathBuf::from(text.replace('/', "\\")))
	}
	#[cfg(not(windows))]
	Some(PathBuf::from(text))
}

// Percent-decoding, tolerant: an escape that is not one is left standing rather
// than throwing away a path over it. Bytes are decoded, not chars, so a
// multi-byte character split across escapes reassembles.
fn decode(text: &str) -> String {
	let bytes = text.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut at = 0;
	while at < bytes.len() {
		let hex = |index: usize| (bytes[index] as char).to_digit(16);
		if bytes[at] == b'%' && at + 2 < bytes.len() {
			if let (Some(high), Some(low)) = (hex(at + 1), hex(at + 2)) {
				out.push(u8::try_from(high * 16 + low).unwrap_or(b'?'));
				at += 3;
				continue;
			}
		}
		out.push(bytes[at]);
		at += 1;
	}
	String::from_utf8_lossy(&out).into_owned()
}

// This machine's name, for the OSC 7 host check. Asked once: it cannot change
// under a running process in any way that matters here.
fn local_host() -> &'static str {
	static HOST: OnceLock<String> = OnceLock::new();
	HOST.get_or_init(host_name)
}

#[cfg(unix)]
fn host_name() -> String {
	// c_char is signed on x86_64 and UNSIGNED on aarch64, so it has to be named
	// rather than spelled i8 - a bare i8 fails to compile for the ARM targets.
	let mut buf = [0 as libc::c_char; 256];
	// SAFETY: the buffer outlives the call and its length is passed with it.
	let ok = unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len() - 1) } == 0;
	if !ok {
		return String::new();
	}
	let bytes: Vec<u8> = buf
		.iter()
		.take_while(|&&byte| byte != 0)
		.map(|&byte| u8::try_from(byte).unwrap_or(b'?'))
		.collect();
	String::from_utf8_lossy(&bytes).into_owned()
}

// %COMPUTERNAME% rather than GetComputerNameEx: it is always set, and it saves
// a windows-sys feature for a string nothing critical depends on.
#[cfg(windows)]
fn host_name() -> String {
	std::env::var("COMPUTERNAME").unwrap_or_default()
}

#[cfg(not(any(unix, windows)))]
fn host_name() -> String {
	String::new()
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use super::{Reported, Scan, directory};

	// Separators are the platform's, so a fixture reads the same in both places.
	fn native(text: &str) -> PathBuf {
		#[cfg(windows)]
		return PathBuf::from(text.replace('/', "\\"));
		#[cfg(not(windows))]
		PathBuf::from(text)
	}

	// A zsh started from dash reports, exits, and leaves dash, which says
	// nothing. The next tab started in zsh's last directory.
	// Test ID: Eq4qaCW
	#[cfg(unix)]
	#[test]
	fn a_report_is_dropped_once_the_program_that_sent_it_exits() {
		let mut child = std::process::Command::new("sleep")
			.arg("30")
			.spawn()
			.expect("start sleep");
		let reported = Reported::default();
		reported.set_from(native("/srv"), Some(child.id()));
		assert_eq!(reported.live(), Some(native("/srv")), "while it runs");
		child.kill().expect("kill");
		child.wait().expect("reap");
		assert_eq!(reported.live(), None, "after it exits");
		assert_eq!(reported.get(), Some(native("/srv")), "still what was said");

		// a speaker nobody could name stays trusted
		reported.set_from(native("/usr"), None);
		assert_eq!(reported.live(), Some(native("/usr")));
	}

	fn feed(chunks: &[&[u8]]) -> Option<PathBuf> {
		let reported = Reported::default();
		let mut scan = Scan::default();
		for chunk in chunks {
			scan.feed(chunk, &reported);
		}
		reported.get()
	}

	// The spelling the unix shells emit: a URL, so the path is percent-encoded
	// and the machine is named. Both terminators are in use.
	// Test ID: EnXWnI0
	#[test]
	fn an_osc7_url_names_the_directory_it_encodes() {
		assert_eq!(
			directory("7;file://box/home/u/two%20words", "box"),
			Some(native("/home/u/two words"))
		);
		// no host and "localhost" both mean this machine
		assert_eq!(
			directory("7;file:///srv/log", "box"),
			Some(native("/srv/log"))
		);
		assert_eq!(
			directory("7;file://localhost/srv/log", "box"),
			Some(native("/srv/log"))
		);
		// the name is not case-sensitive, and a bare path is taken as meant
		assert_eq!(directory("7;file://BOX/srv", "box"), Some(native("/srv")));
		assert_eq!(directory("7;/srv", "box"), Some(native("/srv")));
		// a stray percent is left standing rather than sinking the path
		assert_eq!(directory("7;file:///50%", "box"), Some(native("/50%")));
	}

	// A directory on another machine is not one we can open, and the case that
	// makes this matter is the one where the two agree by accident: an ssh
	// session whose remote path exists here too would open the wrong pane.
	// Test ID: EnXWnI1
	#[test]
	fn a_directory_on_another_machine_is_not_ours() {
		assert_eq!(directory("7;file://elsewhere/srv/log", "box"), None);
		assert_eq!(directory("7;file://elsewhere/srv/log", ""), None);
	}

	// The ConEmu spelling, which is what Windows Terminal documents for
	// PowerShell - so a profile already set up for that terminal works here.
	// Test ID: EnXWnI2
	#[test]
	fn the_conemu_spelling_is_read_too() {
		assert_eq!(directory("9;9;/srv/log", "box"), Some(native("/srv/log")));
		// it is usually sent quoted, and it is never encoded
		assert_eq!(
			directory("9;9;\"/srv/two words\"", "box"),
			Some(native("/srv/two words"))
		);
		assert_eq!(directory("9;9;", "box"), None);
	}

	// A read returns whatever the pipe held, so a report arrives in as many
	// pieces as it likes - including one split mid-escape and mid-terminator.
	// Test ID: EnXWnI3
	#[test]
	fn a_report_split_across_reads_still_arrives() {
		let whole = b"ok\x1b]7;file:///srv/log\x07more";
		assert_eq!(feed(&[whole]), Some(native("/srv/log")));
		assert_eq!(
			feed(&[b"ok\x1b", b"]7;file://", b"/srv/log\x07"]),
			Some(native("/srv/log"))
		);
		// ST, split between its two bytes
		assert_eq!(
			feed(&[b"\x1b]7;file:///srv/log\x1b", b"\\rest"]),
			Some(native("/srv/log"))
		);
		// the last report wins
		assert_eq!(
			feed(&[b"\x1b]7;file:///srv\x07", b"\x1b]7;file:///tmp\x07"]),
			Some(native("/tmp"))
		);
	}

	// Every other OSC goes past constantly - titles on every prompt, clipboard
	// writes carrying whole selections - and none of it may be collected, or a
	// paste-sized payload would be buffered for nothing.
	// Test ID: EnXWnI4
	#[test]
	fn nothing_but_the_two_sequences_is_collected() {
		let reported = Reported::default();
		let mut scan = Scan::default();
		// Left UNTERMINATED on purpose, and looked at while it is still open:
		// once a sequence ends its payload is taken either way, so a check
		// after the fact would pass however much had been buffered.
		scan.feed(b"\x1b]52;c;", &reported);
		assert!(scan.payload.is_empty(), "a clipboard write was collected");
		let big = "a".repeat(200_000);
		scan.feed(big.as_bytes(), &reported);
		assert!(scan.payload.is_empty(), "a payload grew to fit a paste");
		scan.feed(b"\x07\x1b]0;a ti", &reported);
		assert!(scan.payload.is_empty(), "a title was collected");
		scan.feed(b"tle\x07\x1b]777;notify;hi\x07", &reported);
		assert_eq!(reported.get(), None, "something else was believed");
		// and the scanner is still in step afterwards
		scan.feed(b"\x1b]7;file:///srv\x07", &reported);
		assert_eq!(reported.get(), Some(native("/srv")));
	}

	// The Windows spellings: a drive letter arrives behind the URL separator
	// that a file:// path always has, and the separators turn around.
	// Test ID: EnXWnI5
	#[cfg(windows)]
	#[test]
	fn a_windows_drive_letter_survives_the_url_it_arrived_in() {
		assert_eq!(
			directory("7;file:///C:/Users/u", "box"),
			Some(PathBuf::from(r"C:\Users\u"))
		);
		assert_eq!(
			directory("9;9;\"C:\\Users\\u\"", "box"),
			Some(PathBuf::from(r"C:\Users\u"))
		);
	}

	// A reported directory names the tab and reaches the window title, so it has
	// to be text. Percent-decoding is how a control character gets in: the shell
	// need not have printed one for the payload to decode into one.
	// Test ID: EpQN0oF
	#[test]
	fn a_reported_directory_carrying_a_control_character_is_refused() {
		assert_eq!(directory("9;9;/srv/lo\u{0}g", "box"), None);
		assert_eq!(directory("7;file:///srv/lo%00g", "box"), None);
		assert_eq!(directory("7;file:///srv/two%0Alines", "box"), None);
		assert_eq!(directory("7;file:///srv/tab%09bed", "box"), None);
		// and the ordinary case still arrives
		assert_eq!(directory("9;9;/srv/log", "box"), Some(native("/srv/log")));
	}

	// Where a shell says it is decides where the next pane starts, and the shell
	// is not the only thing that can print an OSC 7. So the payload is untrusted
	// text that turns into a path, and the parse has to hold up on anything.
	mod fuzz {
		use super::super::directory;
		use crate::fuzz;

		#[rustfmt::skip]
		const PIECES: [&str; 22] = [
			"7;", "9;9;", "file://", "//", "/", "localhost", "box", "elsewhere",
			"%20", "%2f", "%00", "%", "%zz", "%c3", "%c3%a9", "C:", ":", "\"",
			"..", "\u{0}", "srv", "\u{4e2d}",
		];

		// Test ID: EpQN0oG
		#[test]
		fn any_reported_directory_parses_into_a_usable_path() {
			let corpus = fuzz::corpus("cwd");
			let check = |case: &[u8]| {
				let payload = String::from_utf8_lossy(case);
				for host in ["box", "", "localhost"] {
					let Some(dir) = directory(&payload, host) else {
						continue;
					};
					let text = dir.to_string_lossy();
					assert!(!text.is_empty(), "{payload:?} gave an empty path");
					// A control character reaches the tab label and the window
					// title, so it may not come out of here.
					assert!(
						!text.chars().any(char::is_control),
						"{payload:?} gave {text:?}"
					);
				}
			};
			for case in &corpus {
				check(case);
			}
			fuzz::soak("cwd", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let case = fuzz::input(&mut rng, &corpus, |rng| {
					let mut out = String::new();
					for _ in 0..=rng.below(14) {
						if rng.chance(5) {
							out.push_str(&fuzz::text(rng));
						} else {
							out.push_str(rng.pick(&PIECES));
						}
					}
					out.into_bytes()
				});
				check(&case);
			});
		}
	}
}
