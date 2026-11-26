// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The directory a shell reports it's in, and the tap that reads it.
//!
//! A new tab, split, or window starts where its source pane is. The OS knows
//! that for a shell that changes its own cwd (see `term.rs`), but not for one
//! that keeps its own idea of it, such as PowerShell with `Set-Location`. Every
//! terminal fills that gap the same way: the shell sends the directory in an
//! escape sequence.
//!
//! Both sequences in use are read. OSC 7 (`ESC ] 7 ; file://host/path`) is what
//! the unix shells send. OSC 9;9 (`ESC ] 9 ; 9 ; C:\path`) is the `ConEmu` form
//! that Windows Terminal documents, so a PowerShell profile set up for that
//! terminal works here unchanged.
//!
//! `vte` handles neither sequence, so `TappedPty` wraps the PTY and scans what
//! it reads, rather than forking the parser. `EventLoop` is generic over the
//! public `EventedPty` trait, which allows that. The bytes go on to the parser
//! unchanged, but for one guard: past `OSC_HELD_MAX`, the rest of an OSC's
//! payload is blanked, since the parser would keep all of it (see `OscCap`).

use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event as PollEvent, PollMode, Poller};

// A payload longer than this isn't a directory anybody meant to send, and the
// scanner must never grow a buffer to fit whatever an OSC 52 clipboard write
// happens to contain.
const MAX_PAYLOAD: usize = 4096;

/// The parser keeps an OSC's whole payload until the sequence ends, with no
/// limit of its own outside a `no_std` build, and keeps the memory after it.
/// This is the most it's allowed to keep. It has room for the largest clipboard
/// write that's kept, in base64, and it's a power of 2, so the buffer's doubling
/// stops at it rather than past it.
pub const OSC_HELD_MAX: usize = 2 << 20;
const _: () = assert!(OSC_HELD_MAX.is_power_of_two());
const _: () = assert!(OSC_HELD_MAX >= crate::term::CLIPBOARD_STORE_MAX.div_ceil(3) * 4 + 16);

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// What the shell last said about where it is. Written by the PTY reader thread
/// and read by the window thread, so it's behind a lock. The lock is taken only
/// when a prompt reports and when a new tab, pane, or window opens, never per
/// byte or per frame.
///
/// On unix it also keeps who said it: the terminal's foreground process group
/// when the report arrived. A zsh started from dash reports, then exits, and dash
/// reports nothing, so the report has to go with the program that sent it.
#[derive(Debug, Clone, Default)]
pub struct Reported {
	slot: Arc<Mutex<Option<(PathBuf, Option<u32>)>>>,
	#[cfg(unix)]
	tty: Option<std::os::unix::io::RawFd>,
}

impl Reported {
	/// Reads the speaker from this PTY master. The fd stays valid for as long as
	/// the loop that feeds the tap owns the PTY.
	#[cfg(unix)]
	pub fn for_tty(tty: std::os::unix::io::RawFd) -> Self {
		Self { tty: Some(tty), ..Self::default() }
	}

	// The directory as last reported, or None where no shell has said anything,
	// whoever said it. The scan tests want that; the window wants `live`.
	#[cfg(test)]
	pub fn get(&self) -> Option<PathBuf> {
		Some(crate::locks::lock(&self.slot).as_ref()?.0.clone())
	}

	/// The last report, while the program that sent it is still running.
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

/// The PTY, with its read side scanned and an overlong OSC cut on the way past.
/// Everything else is forwarded untouched, including the child-exit channel and
/// resize.
pub struct TappedPty<P> {
	pty: P,
	cap: OscCap,
	scan: Scan,
	reported: Reported,
}

impl<P> std::fmt::Debug for TappedPty<P> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TappedPty").field("reported", &self.reported).finish_non_exhaustive()
	}
}

impl<P> TappedPty<P> {
	pub fn new(pty: P, reported: Reported) -> Self {
		Self { pty, cap: OscCap::new(OSC_HELD_MAX), scan: Scan::default(), reported }
	}
}

// The tapped PTY is its own reader: `reader()` hands back a borrow, and the
// wrapper can't hold one of the inner reader without borrowing itself. The
// event loop takes the reader and the writer in separate statements, so this
// costs nothing.
impl<P: EventedReadWrite> Read for TappedPty<P> {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		let count = self.pty.reader().read(buf)?;
		self.cap.feed(&mut buf[..count]);
		self.scan.feed(&buf[..count], &self.reported);
		Ok(count)
	}
}

impl<P: EventedReadWrite> EventedReadWrite for TappedPty<P> {
	type Reader = Self;
	type Writer = P::Writer;

	unsafe fn register(&mut self, poll: &Arc<Poller>, interest: PollEvent, mode: PollMode) -> io::Result<()> {
		// SAFETY: the caller's contract (the sources outlive the registration)
		// is passed straight through to the PTY that owns them.
		unsafe { self.pty.register(poll, interest, mode) }
	}

	fn reregister(&mut self, poll: &Arc<Poller>, interest: PollEvent, mode: PollMode) -> io::Result<()> {
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

/// Keeps vte's OSC buffer to `limit` bytes. It follows vte 0.15's states only as
/// far as they decide whether a byte goes into that buffer, and they decide it
/// exactly: ESC leads to Escape from every state, Escape leads to an OSC only on
/// `]`, and an OSC ends only on BEL, CAN, SUB, or ESC. Past the limit, a byte
/// that would be stored becomes a NUL, which vte drops inside an OSC. So the
/// sequence still ends where it would have, and is dispatched cut short, as
/// vte's own `no_std` build does it. Nothing outside an OSC is touched.
#[derive(Debug)]
struct OscCap {
	limit: usize,
	state: CapState,
	held: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapState {
	// any of vte's states but Escape and the OSC string, all of which go to
	// Escape on ESC
	Other,
	Esc,
	Osc,
}

impl OscCap {
	fn new(limit: usize) -> Self {
		Self { limit, state: CapState::Other, held: 0 }
	}

	fn feed(&mut self, bytes: &mut [u8]) {
		let mut at = 0;
		while at < bytes.len() {
			if self.state == CapState::Other {
				match bytes[at..].iter().position(|&byte| byte == ESC) {
					Some(skip) => {
						at += skip + 1;
						self.state = CapState::Esc;
					}
					None => return,
				}
				continue;
			}
			let slot = &mut bytes[at];
			at += 1;
			self.state = match (self.state, *slot) {
				(CapState::Esc, b']') => {
					self.held = 0;
					CapState::Osc
				}
				(CapState::Esc, CAN | SUB | 0x20..=0x7e) | (CapState::Osc, BEL | CAN | SUB) => CapState::Other,
				(CapState::Osc, ESC) => CapState::Esc,
				// vte keeps a parameter break as an index, not a byte
				(CapState::Osc, byte) if byte >= 0x20 && byte != b';' => {
					if self.held < self.limit {
						self.held += 1;
					} else {
						*slot = 0;
					}
					CapState::Osc
				}
				// the rest leave vte where it was, such as a C0 control, DEL, or a
				// high byte in Escape
				(state, _) => state,
			};
		}
	}
}

// Where in an escape sequence the scanner is. A read returns whatever the pipe
// happened to have, so a sequence arrives in as many pieces as it likes and the
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
			// Most of any stream is ordinary output, so Ground skips to the next
			// ESC in one pass rather than walking the match arm per byte.
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
				// an ESC that isn't ST: malformed, so give up on this sequence
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

	// The payload is left in place, so its buffer serves the next OSC.
	fn finish(&self, reported: &Reported) {
		if let Ok(text) = std::str::from_utf8(&self.payload)
			&& let Some(dir) = directory(text, local_host())
		{
			reported.set(dir);
		}
	}
}

// Could this payload still become one we care about? Anything else is dropped
// the moment it can be, so a title change or a clipboard write costs a couple
// of comparisons and no memory.
fn wanted(payload: &[u8]) -> bool {
	[b"7;".as_slice(), b"9;9;".as_slice()].iter().any(|prefix| if payload.len() < prefix.len() { prefix.starts_with(payload) } else { payload.starts_with(prefix) })
}

/// The directory an OSC payload names, in either spelling. `local_host` is what
/// an OSC 7 URL has to name to be believed.
pub fn directory(payload: &str, local_host: &str) -> Option<PathBuf> {
	if let Some(rest) = payload.strip_prefix("7;") {
		return from_url(rest, local_host);
	}
	// ConEmu's: a plain path, sometimes quoted, never encoded, and with no host
	// to check, since only a shell on this machine sends it.
	let rest = payload.strip_prefix("9;9;")?;
	as_path(rest.trim_matches('"'))
}

// `file://<host>/<path>`, percent-encoded. A bare path is taken too. Nobody
// documents it, but a shell that writes one means the same thing.
fn from_url(url: &str, local_host: &str) -> Option<PathBuf> {
	let Some(rest) = url.strip_prefix("file://") else {
		return url.starts_with('/').then(|| as_path(&decode(url))).flatten();
	};
	// the host runs to the path's leading separator, which the path keeps
	let split = rest.find('/')?;
	let (host, path) = rest.split_at(split);
	// A directory on another machine isn't one we can open. Rejecting it
	// matters most where the 2 agree by accident: an ssh session whose remote
	// path also exists locally would otherwise open a pane in the wrong one.
	if !(host.is_empty() || host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case(local_host)) {
		return None;
	}
	as_path(&decode(path))
}

// A URL path to a native one. On Windows that means dropping the separator that
// precedes a drive letter (`/C:/x`) and turning the rest around.
fn as_path(text: &str) -> Option<PathBuf> {
	// An OSC payload is untrusted text, and both forms pass through here. A
	// control character is refused because a reported directory names the tab and
	// shows in the window title. Refusing such a name only means the new pane
	// starts where the old one did.
	//
	// Whether the path is absolute isn't checked here. A pane on Windows can report
	// a posix path, which wsl.exe wants. usable_cwd is the one check on what
	// becomes a working directory.
	if text.is_empty() || text.chars().any(char::is_control) {
		return None;
	}
	#[cfg(windows)]
	{
		let bytes = text.as_bytes();
		let text = if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' { &text[1..] } else { text };
		Some(PathBuf::from(text.replace('/', "\\")))
	}
	#[cfg(not(windows))]
	Some(PathBuf::from(text))
}

// Percent-decoding, tolerant: an escape that isn't one is left standing rather
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

// This machine's name, for the OSC 7 host check. Asked once: it can't change
// under a running process in any way that matters here.
fn local_host() -> &'static str {
	static HOST: OnceLock<String> = OnceLock::new();
	HOST.get_or_init(host_name)
}

#[cfg(unix)]
fn host_name() -> String {
	let mut buf = [0u8; 256];
	// SAFETY: the buffer outlives the call and its length is passed with it.
	// The last byte stays 0, so the name always ends.
	let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len() - 1) } == 0;
	if !ok {
		return String::new();
	}
	std::ffi::CStr::from_bytes_until_nul(&buf).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

// %COMPUTERNAME% rather than GetComputerNameEx: it's always set, and it saves
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

	use super::{OSC_HELD_MAX, OscCap, Reported, Scan, directory};

	// Separators are the platform's, so a fixture reads the same in both places.
	fn native(text: &str) -> PathBuf {
		#[cfg(windows)]
		return PathBuf::from(text.replace('/', "\\"));
		#[cfg(not(windows))]
		PathBuf::from(text)
	}

	// A zsh started from dash reports, exits, and leaves dash, which says
	// nothing. The next tab started in zsh's last directory.
	// Test ID: dcq9m
	#[cfg(unix)]
	#[test]
	#[cfg_attr(miri, ignore = "Miri can't start a process")]
	fn a_report_is_dropped_once_the_program_that_sent_it_exits() {
		let mut child = std::process::Command::new("sleep").arg("30").spawn().expect("start sleep");
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
	// Test ID: dbhew
	#[test]
	fn an_osc7_url_names_the_directory_it_encodes() {
		assert_eq!(directory("7;file://box/home/u/two%20words", "box"), Some(native("/home/u/two words")));
		// no host and "localhost" both mean this machine
		assert_eq!(directory("7;file:///srv/log", "box"), Some(native("/srv/log")));
		assert_eq!(directory("7;file://localhost/srv/log", "box"), Some(native("/srv/log")));
		// the name isn't case-sensitive, and a bare path is taken as meant
		assert_eq!(directory("7;file://BOX/srv", "box"), Some(native("/srv")));
		assert_eq!(directory("7;/srv", "box"), Some(native("/srv")));
		// a stray percent is left standing rather than sinking the path
		assert_eq!(directory("7;file:///50%", "box"), Some(native("/50%")));
	}

	// A directory on another machine isn't one we can open, and the case that
	// makes this matter is the one where the 2 agree by accident: an ssh
	// session whose remote path exists here too would open the wrong pane.
	// Test ID: dbhex
	#[test]
	fn a_directory_on_another_machine_is_not_ours() {
		assert_eq!(directory("7;file://elsewhere/srv/log", "box"), None);
		assert_eq!(directory("7;file://elsewhere/srv/log", ""), None);
	}

	// The ConEmu form, which Windows Terminal documents for PowerShell, so a
	// profile already set up for that terminal works here.
	// Test ID: dbhey
	#[test]
	fn the_conemu_spelling_is_read_too() {
		assert_eq!(directory("9;9;/srv/log", "box"), Some(native("/srv/log")));
		// it's usually sent quoted, and it's never encoded
		assert_eq!(directory("9;9;\"/srv/two words\"", "box"), Some(native("/srv/two words")));
		assert_eq!(directory("9;9;", "box"), None);
	}

	// A read returns whatever the pipe held, so a report can arrive in any number
	// of pieces, including one split mid-escape and mid-terminator.
	// Test ID: dbhez
	#[test]
	fn a_report_split_across_reads_still_arrives() {
		let whole = b"ok\x1b]7;file:///srv/log\x07more";
		assert_eq!(feed(&[whole]), Some(native("/srv/log")));
		assert_eq!(feed(&[b"ok\x1b", b"]7;file://", b"/srv/log\x07"]), Some(native("/srv/log")));
		// ST, split between its 2 bytes
		assert_eq!(feed(&[b"\x1b]7;file:///srv/log\x1b", b"\\rest"]), Some(native("/srv/log")));
		// the last report wins
		assert_eq!(feed(&[b"\x1b]7;file:///srv\x07", b"\x1b]7;file:///tmp\x07"]), Some(native("/tmp")));
	}

	// Every other OSC goes past all the time, such as a title on every prompt or
	// a clipboard write with a whole selection. None of it may be collected, or a
	// paste-sized payload would be buffered for nothing.
	// Test ID: dbhf0
	#[test]
	fn nothing_but_the_two_sequences_is_collected() {
		let reported = Reported::default();
		let mut scan = Scan::default();
		// Left unterminated on purpose, and looked at while it's still open:
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
	// Test ID: dbhf1
	#[cfg(windows)]
	#[test]
	fn a_windows_drive_letter_survives_the_url_it_arrived_in() {
		assert_eq!(directory("7;file:///C:/Users/u", "box"), Some(PathBuf::from(r"C:\Users\u")));
		assert_eq!(directory("9;9;\"C:\\Users\\u\"", "box"), Some(PathBuf::from(r"C:\Users\u")));
	}

	// A reported directory names the tab and shows in the window title, so it has
	// to be plain text. Percent-decoding can put a control character in even when
	// the shell didn't print one.
	// Test ID: dcdj6
	#[test]
	fn a_reported_directory_with_a_control_character_is_refused() {
		assert_eq!(directory("9;9;/srv/lo\u{0}g", "box"), None);
		assert_eq!(directory("7;file:///srv/lo%00g", "box"), None);
		assert_eq!(directory("7;file:///srv/two%0Alines", "box"), None);
		assert_eq!(directory("7;file:///srv/tab%09bed", "box"), None);
		// and the ordinary case still arrives
		assert_eq!(directory("9;9;/srv/log", "box"), Some(native("/srv/log")));
	}

	// What vte reports, with an OSC kept as its length and terminator only, so a
	// cut one can be told from the rest.
	#[derive(Debug, PartialEq, Eq)]
	enum Event {
		Print(char),
		Execute(u8),
		Hook(Vec<Vec<u16>>, Vec<u8>, bool, char),
		Put(u8),
		Unhook,
		Osc(usize, bool),
		Csi(Vec<Vec<u16>>, Vec<u8>, bool, char),
		Esc(Vec<u8>, bool, u8),
	}

	#[derive(Default)]
	struct Events(Vec<Event>);

	fn params(params: &alacritty_terminal::vte::Params) -> Vec<Vec<u16>> {
		params.iter().map(<[u16]>::to_vec).collect()
	}

	impl alacritty_terminal::vte::Perform for Events {
		fn print(&mut self, c: char) {
			self.0.push(Event::Print(c));
		}
		fn execute(&mut self, byte: u8) {
			self.0.push(Event::Execute(byte));
		}
		fn hook(&mut self, p: &alacritty_terminal::vte::Params, inter: &[u8], ignore: bool, action: char) {
			self.0.push(Event::Hook(params(p), inter.to_vec(), ignore, action));
		}
		fn put(&mut self, byte: u8) {
			self.0.push(Event::Put(byte));
		}
		fn unhook(&mut self) {
			self.0.push(Event::Unhook);
		}
		fn osc_dispatch(&mut self, p: &[&[u8]], bell: bool) {
			self.0.push(Event::Osc(p.iter().map(|x| x.len()).sum(), bell));
		}
		fn csi_dispatch(&mut self, p: &alacritty_terminal::vte::Params, inter: &[u8], ignore: bool, action: char) {
			self.0.push(Event::Csi(params(p), inter.to_vec(), ignore, action));
		}
		fn esc_dispatch(&mut self, inter: &[u8], ignore: bool, byte: u8) {
			self.0.push(Event::Esc(inter.to_vec(), ignore, byte));
		}
	}

	// `printf '\e]2;'; yes` grew the parser's buffer by every byte until memory
	// ran out, and it kept the memory after. Now the buffer stops at the cap,
	// the sequence still ends where it did, and what follows is read as before.
	// Test ID: dfeeg
	#[test]
	#[cfg_attr(miri, ignore = "8 MiB through the parser takes too long under Miri")]
	fn an_endless_osc_stops_growing_the_parser_buffer() {
		let mut cap = OscCap::new(OSC_HELD_MAX);
		let mut parser = alacritty_terminal::vte::Parser::new();
		let mut events = Events::default();
		let mut chunk = b"\x1b]2;".to_vec();
		chunk.extend(b"y\n".repeat(32 << 10));
		for _ in 0..128 {
			cap.feed(&mut chunk);
			parser.advance(&mut events, &chunk);
			chunk = b"y\n".repeat(32 << 10);
		}
		let mut end = b"\x07ok".to_vec();
		cap.feed(&mut end);
		parser.advance(&mut events, &end);
		assert_eq!(events.0, [Event::Osc(OSC_HELD_MAX, true), Event::Print('o'), Event::Print('k')]);

		// The largest clipboard write that's kept goes through whole.
		let mut cap = OscCap::new(OSC_HELD_MAX);
		let mut events = Events::default();
		let payload = "QUJD".repeat(crate::term::CLIPBOARD_STORE_MAX.div_ceil(3));
		let whole = format!("\x1b]52;c;{payload}\x1b\\").into_bytes();
		let mut cut = whole.clone();
		cap.feed(&mut cut);
		assert!(cut == whole, "a clipboard write that fits was changed");
		alacritty_terminal::vte::Parser::new().advance(&mut events, &cut);
		assert_eq!(events.0[0], Event::Osc(payload.len() + 3, false));
	}

	// The cap follows vte's states on its own, so it has to stay in step with the
	// parser through anything a program can send: a byte it blanks outside an
	// OSC would change what's drawn, and an OSC it misses would grow unchecked.
	// Test ID: dfeeh
	#[test]
	fn capped_stream_parses_the_same() {
		use crate::fuzz;

		const LIMIT: usize = 12;
		const OPENERS: [&[u8]; 3] = [b"\x1b]", b"\x1bP1|", b"\x1b["];
		#[rustfmt::skip]
		const BODY: [u8; 22] = [
			b'y', b';', b']', b'\\', b'[', b'P', b'X', b'_', b'0', b' ', 0x00,
			0x07, 0x18, 0x1a, 0x1b, 0x1b, 0x7f, 0x80, 0x9c, 0x9d, 0xc3, 0xff,
		];
		fuzz::soak("osccap", |seed| {
			let mut rng = fuzz::Rng::new(seed);
			let mut raw = Vec::new();
			for _ in 0..=rng.below(12) {
				if rng.chance(2) {
					raw.extend(fuzz::vt_stream(&mut rng));
				} else {
					if rng.chance(2) {
						raw.extend_from_slice(rng.pick(&OPENERS));
					}
					for _ in 0..rng.below(4 * LIMIT) {
						let byte = if rng.chance(3) { *rng.pick(&BODY) } else { b'y' };
						raw.push(byte);
					}
				}
			}
			let mut cap = OscCap::new(LIMIT);
			let (mut plain, mut capped) = (Events::default(), Events::default());
			let (mut p1, mut p2) = (alacritty_terminal::vte::Parser::new(), alacritty_terminal::vte::Parser::new());
			let mut rest = raw.as_slice();
			while !rest.is_empty() {
				let (chunk, tail) = rest.split_at(1 + rng.below(rest.len()));
				rest = tail;
				let mut cut = chunk.to_vec();
				cap.feed(&mut cut);
				p1.advance(&mut plain, chunk);
				p2.advance(&mut capped, &cut);
			}
			assert_eq!(plain.0.len(), capped.0.len(), "{raw:?}");
			for (a, b) in plain.0.iter().zip(&capped.0) {
				match (a, b) {
					(Event::Osc(whole, bell_a), Event::Osc(held, bell_b)) => {
						assert!(*held <= LIMIT, "{held} held from {raw:?}");
						assert!(held <= whole && bell_a == bell_b, "{raw:?}");
					}
					_ => assert_eq!(a, b, "{raw:?}"),
				}
			}
		});
	}

	// Where a shell says it is decides where the next pane starts, and the shell
	// isn't the only thing that can print an OSC 7. So the payload is untrusted
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

		// Test ID: dcdj7
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
					// A control character would end up in the tab label and the
					// window title, so it may not come out of here.
					assert!(!text.chars().any(char::is_control), "{payload:?} gave {text:?}");
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
