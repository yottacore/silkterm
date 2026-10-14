// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Which program is in front: the one a person sees in the pane.
//!
//! The terminal's foreground process group says what runs, but a group is
//! named for its first process, and in a pipeline that is the first stage.
//! `cat file | less` shows less, and cat has often exited by the time anyone
//! asks. So the answer is the group's last stage that is still alive.
//!
//! tmux hides one more level. SilkTerm sees only the tmux client, while the
//! program on screen runs under the tmux server, so tmux itself is asked which
//! pane its client shows and whether that pane is on the alt screen.

#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

/// What runs in the pane a multiplexer shows, for the minimap column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inner {
	/// The program in front there, the shell itself at its prompt.
	pub program: Option<String>,
	pub own_screen: bool,
}

/// The pid of the program on screen in foreground group `pgid` of the terminal
/// whose shell is `root`. A stage is a member whose parent is outside the
/// group, so a program's own children, such as vim's `:!make`, never count,
/// and the newest stage is the last one in the pipeline. None when no member
/// can be found, and the caller falls back to the group's leader.
#[cfg(target_os = "linux")]
pub fn group_front(root: u32, pgid: u32) -> Option<u32> {
	// a pane's tree is small; this only bounds a strange one
	const MAX_WALK: usize = 64;
	let mut todo = children(root);
	let mut best: Option<(u64, u32)> = None;
	let mut walked = 0;
	while let Some(pid) = todo.pop() {
		walked += 1;
		if walked > MAX_WALK {
			break;
		}
		let Some(stat) = read_stat(pid) else {
			continue;
		};
		if stat.pgrp != pgid {
			// a nested shell runs its jobs a level down
			todo.extend(children(pid));
			continue;
		}
		if !stat.gone && best.is_none_or(|newest| (stat.start, pid) > newest) {
			best = Some((stat.start, pid));
		}
	}
	best.map(|(_, pid)| pid)
}

// macOS lists a group outright; there is no tree to walk.
#[cfg(target_os = "macos")]
pub fn group_front(_root: u32, pgid: u32) -> Option<u32> {
	let pgid_c = libc::pid_t::try_from(pgid).ok().filter(|&p| p > 0)?;
	let mut pids = [0 as libc::pid_t; 128];
	let bytes = libc::c_int::try_from(std::mem::size_of_val(&pids)).ok()?;
	// SAFETY: the call writes at most `bytes` into the buffer
	let got = unsafe { libc::proc_listpgrppids(pgid_c, pids.as_mut_ptr().cast(), bytes) };
	let got = usize::try_from(got).ok()?.min(pids.len());
	let members: Vec<(u32, libc::proc_bsdinfo)> = pids[..got]
		.iter()
		.filter_map(|&pid| u32::try_from(pid).ok().filter(|&pid| pid > 0))
		.filter_map(|pid| bsd_info(pid).map(|info| (pid, info)))
		.collect();
	members
		.iter()
		.filter(|(_, info)| info.pbi_status != libc::SZOMB)
		.filter(|(_, info)| !members.iter().any(|(pid, _)| *pid == info.pbi_ppid))
		.max_by_key(|(pid, info)| (info.pbi_start_tvsec, info.pbi_start_tvusec, *pid))
		.map(|(pid, _)| *pid)
}

// Another user's process, as under sudo, answers nothing here; the leader's
// short info still names it.
#[cfg(target_os = "macos")]
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
	let pid = libc::c_int::try_from(pid).ok()?;
	let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
	// SAFETY: all-integer C struct, so zeroed is a valid value
	let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
	// SAFETY: the call writes at most `size` bytes and says how many it wrote
	let wrote = unsafe {
		libc::proc_pidinfo(
			pid,
			libc::PROC_PIDTBSDINFO,
			0,
			std::ptr::from_mut(&mut info).cast(),
			size,
		)
	};
	(wrote == size).then_some(info)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub fn group_front(_root: u32, _pgid: u32) -> Option<u32> {
	None
}

// One process as /proc/<pid>/stat has it.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
struct Stat {
	comm: String,
	// a zombie, or dead and not yet gone from /proc
	gone: bool,
	pgrp: u32,
	// the foreground group of its terminal
	tpgid: i32,
	start: u64,
}

#[cfg(target_os = "linux")]
fn read_stat(pid: u32) -> Option<Stat> {
	parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

// The name is in parens and may hold spaces or parens of its own, so the
// numbers are counted from the last `)`.
#[cfg(target_os = "linux")]
fn parse_stat(text: &str) -> Option<Stat> {
	let open = text.find('(')?;
	let close = text.rfind(')')?;
	let comm = text.get(open + 1..close)?.to_string();
	let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
	Some(Stat {
		comm,
		gone: matches!(*rest.first()?, "Z" | "X" | "x"),
		pgrp: rest.get(2)?.parse().ok()?,
		tpgid: rest.get(5)?.parse().ok()?,
		start: rest.get(19)?.parse().ok()?,
	})
}

// Each thread keeps its own list of the children it started.
#[cfg(target_os = "linux")]
fn children(pid: u32) -> Vec<u32> {
	let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
		return Vec::new();
	};
	let mut out = Vec::new();
	for task in tasks.flatten() {
		if let Ok(text) = std::fs::read_to_string(task.path().join("children")) {
			out.extend(
				text.split_whitespace()
					.filter_map(|pid| pid.parse::<u32>().ok()),
			);
		}
	}
	out
}

// How often to ask tmux again while what it shows keeps changing. Each ask
// starts a tmux client, a few ms of CPU.
#[cfg(target_os = "linux")]
const ASK_GAP: Duration = Duration::from_millis(500);
// how often to look whether an ask has come back
#[cfg(target_os = "linux")]
const ASK_POLL: Duration = Duration::from_millis(15);
// a server that has stopped answering
#[cfg(target_os = "linux")]
const ASK_GIVE_UP: Duration = Duration::from_secs(2);

/// Asks tmux what the client in this pane shows. The ask runs as a tmux
/// command in the background and is read on a later frame, never waited on.
/// It is made when the client first shows up, then again only after output
/// arrived, at most every `ASK_GAP`, since nothing tmux shows changes without
/// it drawing.
#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
pub struct Tmux {
	// the client pid all of this is for, 0 when there is none
	client: u32,
	query: Option<Query>,
	asking: Option<(std::process::Child, Instant)>,
	poll_at: Option<Instant>,
	asked: Option<Instant>,
	stirred: std::cell::Cell<bool>,
	inner: Option<Inner>,
}

// How to reach the server the client talks to: its own binary, its own -L or
// -S, and its own TMUX and TMUX_TMPDIR, since those pick the socket.
#[cfg(target_os = "linux")]
#[derive(Debug)]
struct Query {
	exe: std::path::PathBuf,
	socket: Vec<std::ffi::OsString>,
	env: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

// The pane a client shows.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
	pid: u32,
	alt: bool,
	mouse: bool,
}

#[cfg(target_os = "linux")]
impl Tmux {
	/// Output arrived or a key went in, so what tmux shows may have changed.
	pub fn stir(&self) {
		self.stirred.set(true);
	}

	/// The tmux client went away, or nobody needs the answer.
	pub fn forget(&mut self) {
		if self.client != 0 {
			*self = Self::default();
		}
	}

	/// What runs in the pane tmux client `client` shows. None until tmux has
	/// answered once, or when it cannot be asked.
	pub fn inner(&mut self, client: u32, now: Instant) -> Option<Inner> {
		if client != self.client {
			*self = Self::default();
			self.client = client;
			self.query = Query::of(client);
		}
		self.collect(now);
		let due = self
			.asked
			.is_none_or(|at| self.stirred.get() && now >= at + ASK_GAP);
		if self.asking.is_none() && due {
			self.ask(now);
		}
		self.inner.clone()
	}

	/// When to look again: an ask out, or one held back by `ASK_GAP`.
	pub fn wake(&self) -> Option<Instant> {
		if self.client == 0 {
			return None;
		}
		if self.asking.is_some() {
			return self.poll_at;
		}
		self.asked
			.filter(|_| self.stirred.get())
			.map(|at| at + ASK_GAP)
	}

	fn ask(&mut self, now: Instant) {
		self.asked = Some(now);
		self.stirred.set(false);
		let Some(query) = &self.query else {
			return;
		};
		let mut cmd = std::process::Command::new(&query.exe);
		cmd.args(&query.socket)
			.args([
				"list-clients",
				"-F",
				"#{client_pid} #{pane_pid} #{alternate_on} #{mouse_any_flag}",
			])
			.stdin(std::process::Stdio::null())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::null());
		for (name, value) in &query.env {
			match value {
				Some(value) => cmd.env(name, value),
				None => cmd.env_remove(name),
			};
		}
		if let Ok(child) = cmd.spawn() {
			self.asking = Some((child, now));
			self.poll_at = Some(now + ASK_POLL);
		}
	}

	fn collect(&mut self, now: Instant) {
		let Some((child, at)) = &mut self.asking else {
			return;
		};
		match child.try_wait() {
			Ok(Some(_)) => {
				let mut out = String::new();
				if let Some(mut stdout) = child.stdout.take() {
					let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
				}
				// The program is read with the answer and not between answers. A
				// program read later beside a screen tmux reported earlier took
				// the column from the shell for a moment after less quit.
				self.inner = shown_by(&out, self.client).map(Shown::inner);
				self.asking = None;
			}
			Ok(None) if now < *at + ASK_GIVE_UP => self.poll_at = Some(now + ASK_POLL),
			Ok(None) | Err(_) => {
				let _ = child.kill();
				let _ = child.wait();
				self.asking = None;
			}
		}
	}
}

#[cfg(target_os = "linux")]
impl Drop for Tmux {
	fn drop(&mut self) {
		if let Some((child, _)) = &mut self.asking {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

#[cfg(target_os = "linux")]
impl Query {
	fn of(client: u32) -> Option<Self> {
		use std::os::unix::ffi::OsStrExt;
		let argv: Vec<std::ffi::OsString> = std::fs::read(format!("/proc/{client}/cmdline"))
			.ok()?
			.split(|&b| b == 0)
			.filter(|arg| !arg.is_empty())
			.map(|arg| std::ffi::OsStr::from_bytes(arg).to_owned())
			.collect();
		let environ = std::fs::read(format!("/proc/{client}/environ")).ok()?;
		let var = |name: &str| {
			environ
				.split(|&b| b == 0)
				.find_map(|pair| pair.strip_prefix(name.as_bytes())?.strip_prefix(b"="))
				.map(|value| std::ffi::OsStr::from_bytes(value).to_owned())
		};
		// a binary replaced since it started reads "(deleted)"
		let exe = std::fs::read_link(format!("/proc/{client}/exe"))
			.ok()
			.filter(|exe| exe.is_file())
			.unwrap_or_else(|| "tmux".into());
		// Something else that calls itself tmux is not handed tmux's arguments.
		if !exe
			.file_name()
			.is_some_and(|name| name.as_bytes().starts_with(b"tmux"))
		{
			return None;
		}
		Some(Self {
			exe,
			socket: socket_args(&argv),
			env: vec![("TMUX", var("TMUX")), ("TMUX_TMPDIR", var("TMUX_TMPDIR"))],
		})
	}
}

// tmux's -L and -S, read the way its getopt reads them. The other options that
// take a value are skipped with it, and the command starts at the first word
// that is not an option.
#[cfg(target_os = "linux")]
fn socket_args(argv: &[std::ffi::OsString]) -> Vec<std::ffi::OsString> {
	use std::os::unix::ffi::{OsStrExt, OsStringExt};
	let mut out = Vec::new();
	let mut at = 1;
	while let Some(arg) = argv.get(at).map(|arg| arg.as_bytes()) {
		if arg == b"--" || arg.len() < 2 || arg[0] != b'-' {
			break;
		}
		for (i, &flag) in arg.iter().enumerate().skip(1) {
			if !b"cfLST".contains(&flag) {
				continue;
			}
			let value = if i + 1 < arg.len() {
				arg[i + 1..].to_vec()
			} else {
				at += 1;
				argv.get(at)
					.map(|v| v.as_bytes().to_vec())
					.unwrap_or_default()
			};
			if matches!(flag, b'L' | b'S') {
				out.push(format!("-{}", flag as char).into());
				out.push(std::ffi::OsString::from_vec(value));
			}
			break;
		}
		at += 1;
	}
	out
}

#[cfg(target_os = "linux")]
impl Shown {
	fn inner(self) -> Inner {
		// A pane started on a program rather than a shell has that program in
		// its own group, so it is named whether a command runs or not.
		let stat = read_stat(self.pid);
		let tpgid = stat
			.as_ref()
			.and_then(|stat| u32::try_from(stat.tpgid).ok())
			.filter(|&g| g > 0);
		let running = stat
			.as_ref()
			.zip(tpgid)
			.is_some_and(|(stat, g)| g != stat.pgrp);
		let program = tpgid.and_then(|g| {
			let pid = if running {
				group_front(self.pid, g).unwrap_or(g)
			} else {
				self.pid
			};
			crate::term::named(&read_stat(pid)?.comm)
		});
		Inner {
			own_screen: self.alt || self.mouse && running,
			program,
		}
	}
}

// The line for our client out of `list-clients`.
#[cfg(target_os = "linux")]
fn shown_by(out: &str, client: u32) -> Option<Shown> {
	out.lines().find_map(|line| {
		let mut words = line.split_whitespace();
		if words.next()?.parse::<u32>().ok()? != client {
			return None;
		}
		let pid = words.next()?.parse().ok()?;
		let mut flag = || words.next() == Some("1");
		Some(Shown {
			pid,
			alt: flag(),
			mouse: flag(),
		})
	})
}

/// Nothing to ask off Linux yet, so tmux keeps the column as listed.
#[cfg(not(target_os = "linux"))]
#[derive(Debug, Default)]
pub struct Tmux {}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::unused_self)]
#[cfg_attr(windows, allow(dead_code))]
impl Tmux {
	pub fn stir(&self) {}

	pub fn forget(&mut self) {}

	pub fn inner(&mut self, _client: u32, _now: std::time::Instant) -> Option<Inner> {
		None
	}

	pub fn wake(&self) -> Option<std::time::Instant> {
		None
	}
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
	use super::*;
	use std::os::unix::process::CommandExt;

	fn reap(child: &mut std::process::Child) {
		let _ = child.kill();
		let _ = child.wait();
	}

	// Test ID: EsdqcVY
	#[test]
	fn a_pipeline_whose_first_stage_exited_is_named_for_the_last() {
		let mut first = std::process::Command::new("sleep")
			.arg("0.3")
			.process_group(0)
			.spawn()
			.unwrap();
		let pgid = first.id();
		let mut last = std::process::Command::new("sleep")
			.arg("30")
			.process_group(pgid.cast_signed())
			.spawn()
			.unwrap();
		first.wait().unwrap();
		let found = group_front(std::process::id(), pgid);
		reap(&mut last);
		assert_eq!(found, Some(last.id()));
	}

	// Test ID: Esdqcbw
	#[test]
	fn a_program_s_own_children_are_not_stages() {
		let mut sh = std::process::Command::new("sh")
			.args(["-c", "sleep 30; :"])
			.process_group(0)
			.spawn()
			.unwrap();
		let pgid = sh.id();
		// the sleep it starts is newer and in the group too
		let deadline = Instant::now() + Duration::from_secs(5);
		while children(pgid).is_empty() && Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(10));
		}
		let found = group_front(std::process::id(), pgid);
		let kids = children(pgid);
		// SAFETY: signals the group this test made
		unsafe { libc::kill(-pgid.cast_signed(), libc::SIGKILL) };
		reap(&mut sh);
		assert!(!kids.is_empty());
		assert_eq!(found, Some(pgid));
	}

	// Test ID: Esdqchj
	#[test]
	fn a_stat_line_parses_past_a_name_with_parens() {
		let line =
			"4242 (a) b (c) S 1 4240 4240 34816 4240 4194304 0 0 0 0 0 0 0 0 20 0 1 0 987654 0 0";
		let stat = parse_stat(line).unwrap();
		assert_eq!(stat.comm, "a) b (c");
		assert_eq!(
			(stat.pgrp, stat.tpgid, stat.start, stat.gone),
			(4240, 4240, 987_654, false)
		);
		let zombie = line.replace(") S ", ") Z ");
		assert!(parse_stat(&zombie).unwrap().gone);
	}

	// Test ID: Esdqcm7
	#[test]
	fn tmux_socket_options_are_read_like_its_getopt() {
		let args = |line: &str| -> Vec<String> {
			let argv: Vec<std::ffi::OsString> = line.split(' ').map(Into::into).collect();
			socket_args(&argv)
				.into_iter()
				.map(|a| a.into_string().unwrap())
				.collect()
		};
		assert!(args("tmux").is_empty());
		assert!(
			args("tmux new -L x").is_empty(),
			"past the command is the command's"
		);
		assert_eq!(args("tmux -L work attach"), ["-L", "work"]);
		assert_eq!(args("tmux -uLwork"), ["-L", "work"]);
		assert_eq!(args("tmux -f /dev/null -S /tmp/s new"), ["-S", "/tmp/s"]);
		assert_eq!(args("tmux -2 -c ls -S s"), ["-S", "s"]);
		assert!(args("tmux -- -L x").is_empty());
	}

	// Test ID: Esdqcqe
	#[test]
	fn the_answer_is_our_client_s_line() {
		let out = "100 7 1 0\n200 8 0 1\n";
		assert_eq!(
			shown_by(out, 200),
			Some(Shown {
				pid: 8,
				alt: false,
				mouse: true
			})
		);
		assert_eq!(
			shown_by(out, 100),
			Some(Shown {
				pid: 7,
				alt: true,
				mouse: false
			})
		);
		assert_eq!(shown_by(out, 300), None);
		// an older tmux with no mouse flag leaves it empty
		assert_eq!(
			shown_by("300 9 1 \n", 300),
			Some(Shown {
				pid: 9,
				alt: true,
				mouse: false
			})
		);
	}
}
