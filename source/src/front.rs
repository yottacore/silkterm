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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

/// macOS lists a group outright; there is no tree to walk.
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
#[cfg(any(target_os = "linux", target_os = "macos"))]
const ASK_GAP: Duration = Duration::from_millis(500);
// how often to look whether an ask has come back
#[cfg(any(target_os = "linux", target_os = "macos"))]
const ASK_POLL: Duration = Duration::from_millis(15);
// a server that has stopped answering
#[cfg(any(target_os = "linux", target_os = "macos"))]
const ASK_GIVE_UP: Duration = Duration::from_secs(2);

/// Asks tmux what the client in this pane shows. The ask runs as a tmux
/// command in the background and is read on a later frame, never waited on.
/// It is made when the client first shows up, then again only after output
/// arrived, at most every `ASK_GAP`, since nothing tmux shows changes without
/// it drawing.
#[cfg(any(target_os = "linux", target_os = "macos"))]
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
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct Query {
	exe: std::path::PathBuf,
	socket: Vec<std::ffi::OsString>,
	env: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

// The pane a client shows.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
	pid: u32,
	alt: bool,
	mouse: bool,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for Tmux {
	fn drop(&mut self) {
		if let Some((child, _)) = &mut self.asking {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Query {
	fn of(client: u32) -> Option<Self> {
		let (exe, argv, environ) = launched_as(client)?;
		Self::new(exe, &argv, &environ)
	}

	// `environ` is NUL-separated NAME=value pairs.
	fn new(
		exe: Option<std::path::PathBuf>,
		argv: &[std::ffi::OsString],
		environ: &[u8],
	) -> Option<Self> {
		use std::os::unix::ffi::OsStrExt;
		let var = |name: &str| {
			environ
				.split(|&b| b == 0)
				.find_map(|pair| pair.strip_prefix(name.as_bytes())?.strip_prefix(b"="))
				.map(|value| std::ffi::OsStr::from_bytes(value).to_owned())
		};
		let exe = exe
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
			socket: socket_args(argv),
			env: vec![("TMUX", var("TMUX")), ("TMUX_TMPDIR", var("TMUX_TMPDIR"))],
		})
	}
}

// A process's binary, arguments and starting environment.
#[cfg(target_os = "linux")]
fn launched_as(pid: u32) -> Option<(Option<std::path::PathBuf>, Vec<std::ffi::OsString>, Vec<u8>)> {
	use std::os::unix::ffi::OsStrExt;
	let argv = std::fs::read(format!("/proc/{pid}/cmdline"))
		.ok()?
		.split(|&b| b == 0)
		.filter(|arg| !arg.is_empty())
		.map(|arg| std::ffi::OsStr::from_bytes(arg).to_owned())
		.collect();
	let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
	// a binary replaced since it started reads "(deleted)", which is_file drops
	let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
	Some((exe, argv, environ))
}

// macOS has no /proc. sysctl hands back the same as one block (KERN_PROCARGS2).
#[cfg(target_os = "macos")]
fn launched_as(pid: u32) -> Option<(Option<std::path::PathBuf>, Vec<std::ffi::OsString>, Vec<u8>)> {
	use std::os::unix::ffi::OsStrExt;
	let pid = libc::c_int::try_from(pid).ok().filter(|&pid| pid > 0)?;
	let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
	let mut size: libc::size_t = 0;
	let null = std::ptr::null_mut();
	// SAFETY: a null buffer asks only for the size
	if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, null, &raw mut size, null, 0) } != 0 {
		return None;
	}
	let mut buf = vec![0u8; size];
	// SAFETY: the call writes at most `size` bytes and sets it to how many
	if unsafe {
		libc::sysctl(
			mib.as_mut_ptr(),
			3,
			buf.as_mut_ptr().cast(),
			&raw mut size,
			null,
			0,
		)
	} != 0
	{
		return None;
	}
	buf.truncate(size);
	let (argv, environ) = procargs(&buf)?;
	let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
	let cap = u32::try_from(path.len()).ok()?;
	// SAFETY: the call writes at most `cap` bytes and returns the length
	let got = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), cap) };
	let exe = usize::try_from(got)
		.ok()
		.filter(|&n| n > 0)
		.map(|n| std::ffi::OsStr::from_bytes(&path[..n]).into());
	Some((exe, argv, environ.to_vec()))
}

// KERN_PROCARGS2 is argc as a native int, the exec path, NUL padding, then argc
// arguments and the environment, all NUL-terminated. The environment ends at
// an empty string or the end of the block.
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
fn procargs(buf: &[u8]) -> Option<(Vec<std::ffi::OsString>, &[u8])> {
	use std::os::unix::ffi::OsStrExt;
	let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
	let argc = usize::try_from(argc).ok()?;
	let mut rest = buf.get(4..)?;
	// the exec path, then its padding
	let path_end = rest.iter().position(|&b| b == 0)?;
	rest = &rest[path_end..];
	rest = &rest[rest.iter().position(|&b| b != 0).unwrap_or(rest.len())..];
	let mut argv = Vec::with_capacity(argc);
	for _ in 0..argc {
		let end = rest.iter().position(|&b| b == 0)?;
		argv.push(std::ffi::OsStr::from_bytes(&rest[..end]).to_owned());
		rest = &rest[end + 1..];
	}
	let mut env_end = 0;
	while let Some(len) = rest
		.get(env_end..)
		.and_then(|tail| tail.iter().position(|&b| b == 0))
	{
		if len == 0 {
			break;
		}
		env_end += len + 1;
	}
	Some((argv, &rest[..env_end]))
}

// tmux's -L and -S, read the way its getopt reads them. The other options that
// take a value are skipped with it, and the command starts at the first word
// that is not an option.
#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Shown {
	fn inner(self) -> Inner {
		// A pane started on a program rather than a shell has that program in
		// its own group, so it is named whether a command runs or not.
		let groups = groups_of(self.pid);
		let tpgid = groups
			.and_then(|(_, tpgid)| u32::try_from(tpgid).ok())
			.filter(|&g| g > 0);
		let running = groups.zip(tpgid).is_some_and(|((pgrp, _), g)| g != pgrp);
		let program = tpgid.and_then(|g| {
			let pid = if running {
				group_front(self.pid, g).unwrap_or(g)
			} else {
				self.pid
			};
			comm_of(pid)
		});
		Inner {
			own_screen: self.alt || self.mouse && running,
			program,
		}
	}
}

// A process's own group and its terminal's foreground group.
#[cfg(target_os = "linux")]
fn groups_of(pid: u32) -> Option<(u32, i32)> {
	read_stat(pid).map(|stat| (stat.pgrp, stat.tpgid))
}

#[cfg(target_os = "macos")]
fn groups_of(pid: u32) -> Option<(u32, i32)> {
	// no terminal reads 0
	bsd_info(pid).map(|info| (info.pbi_pgid, info.e_tpgid.cast_signed()))
}

#[cfg(target_os = "linux")]
fn comm_of(pid: u32) -> Option<String> {
	crate::term::named(&read_stat(pid)?.comm)
}

#[cfg(target_os = "macos")]
fn comm_of(pid: u32) -> Option<String> {
	crate::term::proc_comm(pid)
}

// The line for our client out of `list-clients`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
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

/// Nothing to ask elsewhere yet, so tmux keeps the column as listed.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Debug, Default)]
pub struct Tmux {}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

	fn procargs_block(argc: i32, strings: &[u8]) -> Vec<u8> {
		let mut buf = argc.to_ne_bytes().to_vec();
		buf.extend_from_slice(strings);
		buf
	}

	// Test ID: EsgscfK
	#[test]
	fn sysctl_process_args_split_into_argv_and_environment() {
		let buf = procargs_block(
			3,
			b"/usr/local/bin/tmux\0\0\0\0tmux\0-L\0work\0HOME=/Users/x\0TMUX_TMPDIR=/tmp/t\0\0ptr_munge=\0",
		);
		let (argv, environ) = procargs(&buf).unwrap();
		assert_eq!(argv, ["tmux", "-L", "work"]);
		assert_eq!(environ, b"HOME=/Users/x\0TMUX_TMPDIR=/tmp/t\0");
		// no empty string after the environment
		let buf = procargs_block(1, b"/bin/tmux\0tmux\0TMUX=/s,1,0\0");
		assert_eq!(procargs(&buf).unwrap().1, b"TMUX=/s,1,0\0");
		// an empty argument still counts
		let buf = procargs_block(2, b"/bin/tmux\0\0tmux\0\0A=1\0");
		assert_eq!(procargs(&buf).unwrap().0, ["tmux", ""]);
		assert!(procargs(&procargs_block(4, b"/bin/tmux\0tmux\0-L\0")).is_none());
		assert!(procargs(&procargs_block(-1, b"/bin/tmux\0")).is_none());
		assert!(procargs(b"\x01\0").is_none());
	}

	// Test ID: Esgscji
	#[test]
	fn a_client_s_socket_and_tmux_settings_come_from_how_it_was_launched() {
		let buf = procargs_block(
			4,
			b"/nowhere/tmux\0tmux\0-S\0/tmp/sock\0attach\0TMUX_TMPDIR=/tmp/t\0PATH=/bin\0",
		);
		let (argv, environ) = procargs(&buf).unwrap();
		let query = Query::new(Some("/nowhere/tmux".into()), &argv, environ).unwrap();
		assert_eq!(
			query.exe,
			std::path::PathBuf::from("tmux"),
			"a missing binary is found on PATH"
		);
		assert_eq!(query.socket, ["-S", "/tmp/sock"]);
		assert_eq!(
			query.env,
			[("TMUX", None), ("TMUX_TMPDIR", Some("/tmp/t".into()))]
		);
		let sh = std::path::PathBuf::from("/bin/sh");
		assert!(Query::new(Some(sh), &argv, environ).is_none(), "not tmux");
	}
}

#[cfg(all(test, target_os = "macos"))]
mod mac_tests {
	use super::*;
	use std::process::{Command, Stdio};

	// Polls the real ask until it answers `want`, stirring as output would.
	fn wait_for(tmux: &mut Tmux, client: u32, want: &Inner) -> Option<Inner> {
		let deadline = Instant::now() + Duration::from_secs(10);
		let mut seen = None;
		while Instant::now() < deadline {
			tmux.stir();
			seen = tmux.inner(client, Instant::now());
			if seen.as_ref() == Some(want) {
				break;
			}
			std::thread::sleep(Duration::from_millis(50));
		}
		seen
	}

	// Test ID: EsgtJUn
	#[test]
	#[ignore = "needs tmux; run by hand on a Mac"]
	fn a_live_tmux_says_what_its_client_shows() {
		let sock = format!("silk{}", std::process::id());
		// The server lives under its own TMUX_TMPDIR, which this process lacks,
		// so the ask finds it only through what the client was started with.
		let dir = format!("/tmp/{sock}");
		std::fs::create_dir_all(&dir).unwrap();
		let tmux = |args: &[&str]| {
			Command::new("tmux")
				.env("TMUX_TMPDIR", &dir)
				.args(["-L", &sock])
				.args(args)
				.output()
				.unwrap()
		};
		// macOS sh runs bash under its own name
		let made = tmux(&["-f", "/dev/null", "new-session", "-d", "/bin/bash --norc"]);
		assert!(made.status.success(), "{made:?}");
		// a client needs a terminal; stdin stays open, or script sends ^D
		let mut script = Command::new("script")
			.args(["-q", "/dev/null", "tmux", "-L", &sock, "attach"])
			.env("TMUX_TMPDIR", &dir)
			.env("TERM", "xterm-256color")
			.stdin(Stdio::piped())
			.stdout(Stdio::null())
			.spawn()
			.unwrap();
		let mut client = 0;
		for _ in 0..100 {
			let out = tmux(&["list-clients", "-F", "#{client_pid}"]);
			client = String::from_utf8_lossy(&out.stdout)
				.trim()
				.parse()
				.unwrap_or(0);
			if client != 0 {
				break;
			}
			std::thread::sleep(Duration::from_millis(50));
		}
		let query = Query::of(client);
		let mut state = Tmux::default();
		let shell = Inner {
			program: Some("bash".into()),
			own_screen: false,
		};
		let at_prompt = wait_for(&mut state, client, &shell);
		tmux(&["send-keys", "seq 100000 | less", "Enter"]);
		let less = Inner {
			program: Some("less".into()),
			own_screen: true,
		};
		let in_less = wait_for(&mut state, client, &less);
		tmux(&["send-keys", "q"]);
		let back = wait_for(&mut state, client, &shell);
		// only a server under the test's own folder, never the user's
		let said = tmux(&["display-message", "-p", "#{socket_path}"]);
		let said = std::path::PathBuf::from(String::from_utf8_lossy(&said.stdout).trim_end());
		let mine = std::fs::canonicalize(&dir).unwrap();
		if said.starts_with(&mine) || said.starts_with(&dir) {
			tmux(&["kill-server"]);
		}
		let _ = script.kill();
		let _ = script.wait();
		let _ = std::fs::remove_dir_all(&dir);
		assert_ne!(client, 0, "no client attached");
		let query = query.unwrap();
		assert!(query.exe.is_absolute(), "{query:?}");
		assert_eq!(query.socket, ["-L", sock.as_str()]);
		assert_eq!(query.env[1], ("TMUX_TMPDIR", Some(dir.clone().into())));
		assert_eq!(at_prompt, Some(shell.clone()));
		assert_eq!(in_less, Some(less));
		assert_eq!(back, Some(shell));
	}
}
