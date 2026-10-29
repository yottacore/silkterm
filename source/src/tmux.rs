// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Panes that run in a tmux session of their own: finding tmux, naming the
//! session, the command line that starts it, and ending it again. A split
//! starts one while `shell.tmux.always_new_session` is on, and so does a tab
//! opened with "New tab in tmux". See design.md, "Panes in tmux".

use crate::config::Choice;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The macros a session name template knows. Each is a whole name, so there's
/// no month-or-minute question for `mm` against `MM` when case doesn't count.
pub const NAME_MACROS: &[&str] = &["YYYYmmDD", "HHMMSS", "HHMMSSNN"];

pub const DEFAULT_SESSION_NAME: &str = "%YYYYmmDD%-%HHMMSSNN%";

// a tmux that doesn't answer by then is treated as having said no
const ASK_FOR: Duration = Duration::from_millis(800);

/// What closing a tab or pane does to the tmux sessions SilkTerm started in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnClose {
	Detach,
	End,
	Prompt,
}

impl Choice for OnClose {
	// the order the dialog's options are in
	const ALL: &'static [Self] = &[Self::Detach, Self::End, Self::Prompt];

	fn key(self) -> &'static str {
		match self {
			Self::Detach => "detach",
			Self::End => "end",
			Self::Prompt => "prompt",
		}
	}
}

/// A tmux session SilkTerm started for a pane. Ending one goes by this name,
/// never by whatever the pane shows now, so a session the user switched to
/// inside the pane is left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
	pub tmux: PathBuf,
	pub name: String,
}

/// Where tmux is, or None where it isn't installed. Never on Windows, which has
/// no tmux of its own. Looked up again after a while, so installing it doesn't
/// need a restart, and cheap enough for a menu to ask each time it opens.
pub fn program() -> Option<PathBuf> {
	const KEEP: Duration = Duration::from_secs(10);
	static FOUND: std::sync::Mutex<Option<(Instant, Option<PathBuf>)>> =
		std::sync::Mutex::new(None);
	if cfg!(windows) {
		return None;
	}
	let mut found = FOUND
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	if let Some((at, path)) = found.as_ref()
		&& at.elapsed() < KEEP
	{
		return path.clone();
	}
	let path = look_for_tmux();
	*found = Some((Instant::now(), path.clone()));
	path
}

// A Mac app started from the Dock gets a PATH without Homebrew or MacPorts on
// it, so their folders are tried after PATH.
fn look_for_tmux() -> Option<PathBuf> {
	const ALSO: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"];
	crate::shells::which("tmux").or_else(|| {
		ALSO.iter()
			.map(|dir| Path::new(dir).join("tmux"))
			.find(|path| runs(path))
	})
}

#[cfg(unix)]
fn runs(path: &Path) -> bool {
	use std::os::unix::fs::PermissionsExt;
	std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn runs(_path: &Path) -> bool {
	false
}

/// The template's name for a session started now. `date` is `YYYYmmDD`,
/// `time` is `HHMMSS`, both local, and `hundredths` the part of a second past
/// it. A template that can't be read, or that comes out empty, gives the
/// default's name.
pub fn session_name(template: &str, date: &str, time: &str, hundredths: u32) -> String {
	let hhmmssnn = format!("{time}{:02}", hundredths.min(99));
	let values = [date, time, hhmmssnn.as_str()];
	let expand = |text: &str| {
		crate::template::Template::parse(text, NAME_MACROS)
			.ok()
			.map(|t| clean_name(&t.expand(&values)))
			.filter(|name| !name.is_empty())
	};
	expand(template)
		.or_else(|| expand(DEFAULT_SESSION_NAME))
		.unwrap_or_else(|| hhmmssnn.clone())
}

/// A session name tmux keeps as given. tmux itself swaps `.` and `:` for `_`,
/// since they separate the parts of a target, and expands `#` as one of its
/// formats. The name has to be the one tmux ends up with, or ending the session
/// by name later finds nothing, so all three are swapped here. Control
/// characters are dropped, since tmux would show them escaped.
pub fn clean_name(name: &str) -> String {
	name.chars()
		.filter(|c| !c.is_control())
		.map(|c| if matches!(c, '.' | ':' | '#') { '_' } else { c })
		.collect::<String>()
		.trim()
		.to_string()
}

/// The command line a pane runs to start `session`: tmux, told the name, the
/// directory and the variables the pane would have set for the shell, then the
/// shell itself. None for the shell leaves it to tmux's own default-shell.
pub fn launch_argv(
	session: &Session,
	cwd: Option<&Path>,
	env: &[(String, String)],
	shell: Option<&[String]>,
) -> Vec<String> {
	let mut argv = vec![
		session.tmux.to_string_lossy().into_owned(),
		"new-session".to_string(),
		"-s".to_string(),
		session.name.clone(),
	];
	if let Some(dir) = cwd {
		argv.extend(["-c".to_string(), dir.to_string_lossy().into_owned()]);
	}
	// a session joins a running server, whose environment is the first
	// client's, so the pane's own variables have to be handed over
	for (name, value) in env {
		argv.extend(["-e".to_string(), format!("{name}={value}")]);
	}
	if let Some(shell) = shell.filter(|shell| !shell.is_empty()) {
		argv.push("--".to_string());
		// One word is run by tmux's default-shell as `-c` text, where a path with
		// a space in it breaks. Two or more are run as they are.
		if shell.len() == 1 {
			argv.push("/usr/bin/env".to_string());
		}
		argv.extend(shell.iter().cloned());
	}
	argv
}

/// A session for a new pane and the command line that starts it, or None when
/// tmux isn't installed. The name comes from `template`, made unique against
/// the sessions tmux already has and the ones handed out here.
pub fn start(
	template: &str,
	shell: Option<&[String]>,
	cwd: Option<&Path>,
	env: &[(String, String)],
) -> Option<(Session, Vec<String>)> {
	let tmux = program()?;
	let (stamp, hundredths) = crate::config::local_stamp();
	let (date, time) = stamp.split_once('-').unwrap_or((stamp.as_str(), ""));
	let base = session_name(template, date, time, hundredths);
	let name = free_name(&base, |name| issued(name) || has_session(&tmux, name));
	let session = Session { tmux, name };
	let argv = launch_argv(&session, cwd, env, shell);
	Some((session, argv))
}

/// `base`, or `base-2`, `base-3` and on, the first that `taken` says is free.
pub fn free_name(base: &str, mut taken: impl FnMut(&str) -> bool) -> String {
	let mut name = base.to_string();
	let mut n = 1;
	while taken(&name) && n < 1000 {
		n += 1;
		name = format!("{base}-{n}");
	}
	name
}

// Names handed out by this process. A new pane's tmux makes its session a
// moment after the pane starts, so two splits in the same hundredth of a
// second would otherwise both find the name free.
fn issued(name: &str) -> bool {
	static ISSUED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
	let mut issued = ISSUED
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	if issued.iter().any(|seen| seen == name) {
		return true;
	}
	issued.push(name.to_string());
	false
}

// `=` makes tmux match the name exactly, where it would otherwise take a
// prefix or a pattern.
fn exact(name: &str) -> String {
	format!("={name}")
}

fn has_session(tmux: &Path, name: &str) -> bool {
	let mut cmd = command(tmux);
	cmd.args(["has-session", "-t", &exact(name)]);
	run(cmd).is_some_and(|out| out.status.success())
}

/// Kill `session`, and nothing else. It goes by the name SilkTerm gave it, so
/// if the user renamed it, or it is gone, nothing is killed. Waited on from a
/// thread of its own, so a tmux that hangs can't stop the window.
pub fn end(session: &Session) {
	let mut cmd = command(&session.tmux);
	cmd.args(["kill-session", "-t", &exact(&session.name)]);
	let Ok(mut child) = cmd.spawn() else {
		return;
	};
	std::thread::spawn(move || {
		let _ = child.wait();
	});
}

/// The directory of the pane tmux shows in `session` now, for a split or new
/// tab to start in. The tmux client the pane runs never moves, so asking the
/// OS where it is only gives where the session started.
pub fn current_path(session: &Session) -> Option<PathBuf> {
	let mut cmd = command(&session.tmux);
	cmd.args([
		"display-message",
		"-p",
		"-t",
		&format!("{}:", exact(&session.name)),
		"#{pane_current_path}",
	]);
	let out = run(cmd).filter(|out| out.status.success())?;
	let text = String::from_utf8_lossy(&out.stdout);
	let dir = PathBuf::from(text.trim_end_matches(['\r', '\n']));
	(!dir.as_os_str().is_empty() && dir.is_dir()).then_some(dir)
}

// TMUX names the server of the tmux SilkTerm itself was started in. A pane
// starts its session with TMUX empty (`term::pane_options`), which sends it to
// the user's usual server, so the asks here go there too.
fn command(tmux: &Path) -> Command {
	let mut cmd = Command::new(tmux);
	cmd.env_remove("TMUX")
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::null());
	// A test always goes by -S. With a socket path too long to fit, -S fails,
	// where TMUX_TMPDIR falls back to the user's own server and a test's
	// kill-server once took every session on it. A test that set no socket
	// gets one that can't exist.
	#[cfg(test)]
	cmd.arg("-S").arg(
		TEST_SOCKET
			.with(|sock| sock.borrow().clone())
			.unwrap_or_else(|| PathBuf::from(NO_SOCKET)),
	);
	cmd
}

// Runs `cmd` to its end, or gives up on it after ASK_FOR.
fn run(mut cmd: Command) -> Option<std::process::Output> {
	let mut child = cmd.spawn().ok()?;
	let until = Instant::now() + ASK_FOR;
	loop {
		match child.try_wait() {
			Ok(Some(_)) => return child.wait_with_output().ok(),
			Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(2)),
			_ => {
				let _ = child.kill();
				let _ = child.wait();
				return None;
			}
		}
	}
}

// A test's own tmux server, so the user's sessions are never touched.
#[cfg(test)]
thread_local! {
	static TEST_SOCKET: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
const NO_SOCKET: &str = "/dev/null/no-tmux";

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: de306
	#[test]
	fn the_default_name_is_the_date_and_the_time_to_a_hundredth() {
		assert_eq!(
			session_name(DEFAULT_SESSION_NAME, "20261017", "173327", 4),
			"20261017-17332704"
		);
		// macro names don't care about case
		assert_eq!(
			session_name("work %hhmmss%", "20261017", "173327", 4),
			"work 173327"
		);
		assert_eq!(
			session_name("%YYYYMMDD%", "20261017", "173327", 4),
			"20261017"
		);
	}

	// tmux would turn these into something else, and the name kept for ending
	// the session has to be the one tmux made.
	// Test ID: de307
	#[test]
	fn a_name_tmux_would_change_is_changed_the_same_way_first() {
		assert_eq!(clean_name("a.b:c#d"), "a_b_c_d");
		assert_eq!(clean_name(" tab\there "), "tabhere");
		assert_eq!(
			session_name("v1.2 %HHMMSS%", "20261017", "173327", 0),
			"v1_2 173327"
		);
		assert_eq!(clean_name("naïve"), "naïve");
	}

	// Test ID: de308
	#[test]
	fn a_template_that_cant_be_read_or_comes_out_empty_gives_the_default_name() {
		let default = session_name(DEFAULT_SESSION_NAME, "20261017", "173327", 50);
		assert_eq!(session_name("%NOPE%", "20261017", "173327", 50), default);
		assert_eq!(
			session_name("{ %HHMMSS%", "20261017", "173327", 50),
			default
		);
		assert_eq!(session_name("   ", "20261017", "173327", 50), default);
		assert_eq!(session_name("...", "20261017", "173327", 50), "___");
	}

	// Test ID: de309
	#[test]
	fn a_taken_name_gets_a_number() {
		let taken = ["work", "work-2"];
		assert_eq!(free_name("work", |n| taken.contains(&n)), "work-3");
		assert_eq!(free_name("home", |n| taken.contains(&n)), "home");
	}

	fn session() -> Session {
		Session {
			tmux: PathBuf::from("/usr/bin/tmux"),
			name: "20261017-17332704".to_string(),
		}
	}

	// Test ID: de30a
	#[test]
	fn the_pane_runs_tmux_with_the_shell_after_its_own_options() {
		let shell = ["/bin/bash".to_string(), "-l".to_string()];
		let env = [("PROMPT_COMMAND".to_string(), "x=1".to_string())];
		assert_eq!(
			launch_argv(&session(), Some(Path::new("/srv/a b")), &env, Some(&shell)),
			[
				"/usr/bin/tmux",
				"new-session",
				"-s",
				"20261017-17332704",
				"-c",
				"/srv/a b",
				"-e",
				"PROMPT_COMMAND=x=1",
				"--",
				"/bin/bash",
				"-l"
			]
		);
		// no directory, no variables and tmux's own shell
		assert_eq!(
			launch_argv(&session(), None, &[], None),
			["/usr/bin/tmux", "new-session", "-s", "20261017-17332704"]
		);
	}

	// tmux hands a lone word to its default-shell as `-c` text, which splits a
	// path with a space in it.
	// Test ID: de30b
	#[test]
	fn a_shell_named_alone_is_run_as_it_is_not_as_shell_text() {
		let shell = ["/opt/my shells/zsh".to_string()];
		let argv = launch_argv(&session(), None, &[], Some(&shell));
		assert_eq!(
			argv[argv.len() - 3..],
			["--", "/usr/bin/env", "/opt/my shells/zsh"]
		);
	}

	// Test ID: de30c
	#[test]
	fn the_close_setting_reads_its_three_words() {
		assert_eq!(OnClose::parse("Detach"), Some(OnClose::Detach));
		assert_eq!(OnClose::parse(" end "), Some(OnClose::End));
		assert_eq!(OnClose::parse("prompt"), Some(OnClose::Prompt));
		assert_eq!(OnClose::parse("kill"), None);
		assert_eq!(
			OnClose::ALL.iter().map(|c| c.key()).collect::<Vec<_>>(),
			["detach", "end", "prompt"]
		);
	}

	// Without -S it would ask the user's own server, which has sessions.
	// Test ID: de541
	#[cfg(unix)]
	#[test]
	#[cfg_attr(miri, ignore = "Miri can't start a process")]
	fn a_test_with_no_socket_of_its_own_reaches_no_server() {
		TEST_SOCKET.with(|sock| *sock.borrow_mut() = None);
		let cmd = command(Path::new("tmux"));
		assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["-S", NO_SOCKET]);
		let Some(tmux) = program() else {
			return;
		};
		let mut cmd = command(&tmux);
		cmd.arg("list-sessions");
		assert!(!run(cmd).is_some_and(|out| out.status.success()));
	}

	// A socket path has to fit in 104 bytes on a Mac, so under a long TMPDIR
	// the test takes a folder of its own in /tmp.
	#[cfg(unix)]
	fn test_socket() -> PathBuf {
		const FITS: usize = 100;
		let mut dir = crate::testdir::run_dir().join("tmuxend");
		if dir.join("sock").as_os_str().len() >= FITS {
			dir = PathBuf::from(format!("/tmp/silktmux-{}", std::process::id()));
		}
		std::fs::create_dir_all(&dir).expect("socket folder");
		dir.join("sock")
	}

	// The test's tmux server goes when the test ends, even when an assert fails.
	// It's killed only when it says it's on the test's socket.
	#[cfg(unix)]
	struct Server<'a>(&'a dyn Fn(&[&str]) -> Option<std::process::Output>);

	#[cfg(unix)]
	impl Drop for Server<'_> {
		fn drop(&mut self) {
			let ours = TEST_SOCKET.with(|sock| sock.borrow().clone());
			let said = (self.0)(&["display-message", "-p", "#{socket_path}"])
				.filter(|out| out.status.success())
				.map(|out| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim_end()));
			let real = |path: &Path| std::fs::canonicalize(path).ok();
			if let (Some(ours), Some(said)) = (&ours, &said)
				&& real(ours).is_some_and(|ours| real(said) == Some(ours))
			{
				let _ = (self.0)(&["kill-server"]);
			}
			if let Some(sock) = &ours {
				let _ = std::fs::remove_file(sock);
				if let Some(dir) = sock.parent() {
					let _ = std::fs::remove_dir(dir);
				}
			}
			TEST_SOCKET.with(|sock| *sock.borrow_mut() = None);
		}
	}

	// Against a real tmux on a server of the test's own. Ending goes by the
	// name SilkTerm gave, so a session with a longer name that starts the same
	// way, and one the user made, both stay.
	// Test ID: de30d
	#[cfg(unix)]
	#[test]
	#[cfg_attr(miri, ignore = "Miri can't start a process")]
	fn ending_kills_the_session_started_and_no_other() {
		let Some(tmux) = program() else {
			eprintln!("no tmux here; skipped");
			return;
		};
		let sock = test_socket();
		TEST_SOCKET.with(|d| *d.borrow_mut() = Some(sock));
		let tmux_cmd = |args: &[&str]| {
			let mut cmd = command(&tmux);
			cmd.args(args);
			run(cmd)
		};
		let _server = Server(&tmux_cmd);
		let ours = Session {
			tmux: tmux.clone(),
			name: "silk-test.1".to_string(),
		};
		let name = clean_name(&ours.name);
		let ours = Session { name, ..ours };
		for name in [ours.name.as_str(), "silk-test_1-more", "theirs"] {
			let made = tmux_cmd(&["new-session", "-d", "-s", name, "-c", "/", "sleep 60"]);
			assert!(made.is_some_and(|out| out.status.success()), "{name}");
		}
		assert!(has_session(&tmux, &ours.name));
		assert!(
			!has_session(&tmux, "silk-test"),
			"a prefix isn't the session"
		);
		// for a moment after it starts, a pane's directory reads empty or as the
		// server's own
		let until = Instant::now() + Duration::from_secs(5);
		let mut path = current_path(&ours);
		while path.as_deref() != Some(Path::new("/")) && Instant::now() < until {
			std::thread::sleep(Duration::from_millis(20));
			path = current_path(&ours);
		}
		assert_eq!(path.as_deref(), Some(Path::new("/")));
		end(&ours);
		let until = Instant::now() + Duration::from_secs(5);
		while has_session(&tmux, &ours.name) && Instant::now() < until {
			std::thread::sleep(Duration::from_millis(20));
		}
		assert!(!has_session(&tmux, &ours.name));
		assert!(has_session(&tmux, "silk-test_1-more"));
		assert!(has_session(&tmux, "theirs"));
	}
}
