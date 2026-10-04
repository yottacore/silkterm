// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Control socket: each running instance listens on a per-process Unix socket
//! and exports its path to child shells via `SILKTERM_SOCKET`, so `silkterm
//! --wallpaper`/`--reload-settings` run from a shell inside a window reaches
//! exactly that window's process. Protocol: one text line per connection
//! (`reload`, or `wallpaper` + optional tab + path), reply `ok` / `err <msg>`.
//! Unix only for now (std has no `AF_UNIX` on Windows).

use std::path::PathBuf;

use winit::event_loop::EventLoopProxy;

use crate::term::UserEvent;

#[cfg_attr(not(unix), allow(dead_code))] // ctl is Unix-only (AF_UNIX)
pub const ENV_SOCK: &str = "SILKTERM_SOCKET";

// Holds the socket path so the file goes away with the process. The drop only
// covers a clean return from main, so `remove_on_any_exit` covers the rest.
#[cfg(unix)]
#[derive(Debug)]
pub struct CtlServer {
	path: PathBuf,
}

#[cfg(unix)]
impl Drop for CtlServer {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.path);
	}
}

#[cfg(not(unix))]
#[derive(Debug)]
pub struct CtlServer;

// Bind the socket, export SILKTERM_SOCKET, and serve commands on a background
// thread. Call before any PTY spawns so shells inherit the variable. Failure is
// non-fatal (the app just isn't remotely controllable).
#[cfg(unix)]
pub fn serve(proxy: EventLoopProxy<UserEvent>) -> Option<CtlServer> {
	use std::io::{BufRead, BufReader, Write};

	let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
	let path = dir.join(format!("silkterm-ctl-{}.sock", std::process::id()));
	let _ = std::fs::remove_file(&path); // stale leftover from a recycled pid
	let listener = match std::os::unix::net::UnixListener::bind(&path) {
		Ok(listener) => listener,
		Err(e) => {
			eprintln!("{}: control socket: {e}", crate::config::APP_NAME);
			return None;
		}
	};
	remove_on_any_exit(&path);
	// Sound here: no PTY or render thread exists yet (set_var is unsafe under
	// edition 2024 because of concurrent readers).
	unsafe { std::env::set_var(ENV_SOCK, &path) };
	std::thread::spawn(move || {
		for stream in listener.incoming() {
			let Ok(stream) = stream else { continue };
			let mut reader = BufReader::new(stream);
			let mut line = String::new();
			if reader.read_line(&mut line).is_err() {
				continue;
			}
			let reply = match parse(line.trim_end()) {
				Ok(event) => {
					if proxy.send_event(event).is_err() {
						return; // event loop gone; the process is exiting
					}
					"ok\n".to_string()
				}
				Err(e) => format!("err {e}\n"),
			};
			let mut stream = reader.into_inner();
			let _ = stream.write_all(reply.as_bytes());
		}
	});
	Some(CtlServer { path })
}

// The path as C text, leaked, so a signal handler can reach it with nothing but
// an atomic load.
#[cfg(unix)]
static SOCK_C_PATH: std::sync::atomic::AtomicPtr<libc::c_char> =
	std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(unix)]
extern "C" fn unlink_socket() {
	let path = SOCK_C_PATH.load(std::sync::atomic::Ordering::Relaxed);
	if !path.is_null() {
		// SAFETY: a NUL-terminated string leaked for the life of the process.
		// unlink is async-signal-safe.
		unsafe { libc::unlink(path) };
	}
}

#[cfg(unix)]
extern "C" fn unlink_and_die(signal: libc::c_int) {
	unlink_socket();
	// SAFETY: signal and raise are async-signal-safe. The default action then
	// ends the process the way the signal would have with no handler.
	unsafe {
		libc::signal(signal, libc::SIG_DFL);
		libc::raise(signal);
	}
}

// A window that ended any other way than a clean return left its socket file:
// `process::exit` when the first shell or the window could not start, SIGTERM or
// SIGHUP, and a panic, which a release build turns into an abort. exit() runs
// atexit handlers, an abort runs only the panic hook, and a signal runs neither.
#[cfg(unix)]
fn remove_on_any_exit(path: &std::path::Path) {
	use std::os::unix::ffi::OsStrExt;
	let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
		return;
	};
	let old = SOCK_C_PATH.swap(c_path.into_raw(), std::sync::atomic::Ordering::Relaxed);
	if !old.is_null() {
		return; // already hooked; the handlers read the new path
	}
	let handler = unlink_and_die as extern "C" fn(libc::c_int) as libc::sighandler_t;
	// SAFETY: plain libc registration calls, made before any other thread exists.
	unsafe {
		libc::atexit(unlink_socket);
		for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
			libc::signal(signal, handler);
		}
	}
	let previous = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		unlink_socket();
		previous(info);
	}));
}

#[cfg(not(unix))]
pub fn serve(_proxy: EventLoopProxy<UserEvent>) -> Option<CtlServer> {
	None
}

// One command line -> the event the app applies. Tab separates verb from value
// so paths with spaces survive.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse(line: &str) -> Result<UserEvent, String> {
	let (verb, value) = match line.split_once('\t') {
		Some((verb, rest)) => (verb, Some(rest)),
		None => (line, None),
	};
	match verb {
		"reload" => Ok(UserEvent::ReloadSettings),
		"wallpaper" => Ok(UserEvent::SetWallpaper(value.map(PathBuf::from))),
		_ => Err(format!("unknown command: {verb}")),
	}
}

// Client side: deliver one command to the window this shell runs inside.
#[cfg(unix)]
pub fn send(cmd: &str) -> Result<(), String> {
	use std::io::{Read, Write};

	let sock =
		std::env::var(ENV_SOCK).map_err(|_| "not inside a running SilkTerm window".to_string())?;
	let mut stream =
		std::os::unix::net::UnixStream::connect(&sock).map_err(|e| format!("{sock}: {e}"))?;
	stream
		.write_all(cmd.as_bytes())
		.and_then(|()| stream.write_all(b"\n"))
		.map_err(|e| e.to_string())?;
	let _ = stream.shutdown(std::net::Shutdown::Write);
	let mut reply = String::new();
	stream
		.read_to_string(&mut reply)
		.map_err(|e| e.to_string())?;
	let reply = reply.trim();
	match reply.strip_prefix("err ") {
		Some(e) => Err(e.to_string()),
		None if reply == "ok" => Ok(()),
		None => Err(format!("unexpected reply: {reply}")),
	}
}

#[cfg(not(unix))]
pub fn send(_cmd: &str) -> Result<(), String> {
	Err("control commands aren't supported on this platform yet".into())
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: EjpedZh
	#[test]
	fn parse_commands() {
		assert!(matches!(parse("reload"), Ok(UserEvent::ReloadSettings)));
		match parse("wallpaper\t/a dir/pic 1.png") {
			Ok(UserEvent::SetWallpaper(Some(p))) => {
				assert_eq!(p, PathBuf::from("/a dir/pic 1.png"));
			}
			other => panic!("{other:?}"),
		}
		assert!(matches!(
			parse("wallpaper"),
			Ok(UserEvent::SetWallpaper(None))
		));
		assert!(parse("bogus").is_err());
		assert!(parse("").is_err());
	}

	// Each way out a window can take, driven in a child copy of this test binary
	// (`socket_exit_child` below), which binds a socket and then leaves.
	// Test ID: Eq4Yrbg
	#[cfg(unix)]
	#[test]
	fn the_socket_file_goes_away_however_the_process_ends() {
		use std::os::unix::process::ExitStatusExt;
		let exe = std::env::current_exe().unwrap();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ctl_exit_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		for how in ["exit", "sigterm", "sighup", "abort"] {
			// Bound by name from inside the folder, since a full path under the
			// run folder can pass the 107-byte limit on a Unix socket path.
			let name = format!("{how}.sock");
			let path = dir.join(&name);
			let status = std::process::Command::new(&exe)
				.args(["--exact", "ctl::tests::socket_exit_child", "--nocapture"])
				.current_dir(&dir)
				.env("SILK_CTL_EXIT", how)
				.env("SILK_CTL_PATH", &name)
				.stdout(std::process::Stdio::null())
				.stderr(std::process::Stdio::null())
				.status()
				.unwrap();
			match how {
				"sigterm" => assert_eq!(status.signal(), Some(libc::SIGTERM)),
				"sighup" => assert_eq!(status.signal(), Some(libc::SIGHUP)),
				"abort" => assert_eq!(status.signal(), Some(libc::SIGABRT)),
				_ => assert_eq!(status.code(), Some(2)),
			}
			assert!(!path.exists(), "{how} left {}", path.display());
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: Eq4Yrbh
	#[cfg(unix)]
	#[test]
	fn socket_exit_child() {
		let (Some(how), Some(path)) = (
			std::env::var_os("SILK_CTL_EXIT"),
			std::env::var_os("SILK_CTL_PATH"),
		) else {
			return; // only does anything when the test above starts it
		};
		let path = PathBuf::from(path);
		let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
		remove_on_any_exit(&path);
		assert!(path.exists());
		match how.to_str() {
			Some("exit") => std::process::exit(2),
			Some("sigterm") => unsafe {
				libc::raise(libc::SIGTERM);
			},
			Some("sighup") => unsafe {
				libc::raise(libc::SIGHUP);
			},
			// a panic that cannot unwind aborts, as every panic does in a release
			// build, and nothing but the panic hook runs
			Some("abort") => no_unwind(),
			_ => {}
		}
		unreachable!();
	}

	#[cfg(unix)]
	extern "C" fn no_unwind() {
		panic!("abort on purpose");
	}
}
