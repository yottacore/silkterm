// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Control socket: each running instance listens on a per-process endpoint
//! and exports its name to child shells via `SILKTERM_SOCKET`, so `silkterm
//! --wallpaper`/`--reload-settings` run from a shell inside a window reaches
//! exactly that window's process. Protocol: one text line per connection
//! (`reload`, `wallpaper` + optional tab + path, or `saved` + tab + key from
//! another window on the same settings file), reply `ok` / `err <msg>`.
//! A Unix socket on Unix, a named pipe on Windows.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use winit::event_loop::EventLoopProxy;

use crate::peers::Told;
use crate::term::UserEvent;

pub const ENV_SOCK: &str = "SILKTERM_SOCKET";

/// Holds the endpoint and the marker other windows find it by, so both go
/// away with the process. The drop only covers a clean return from main, so
/// `remove_on_any_exit` covers the socket for the rest, and a marker left
/// behind is cleared by the next save that finds nobody answering.
#[derive(Debug)]
pub struct CtlServer {
	#[cfg(unix)]
	path: PathBuf,
	_marker: Option<crate::peers::Marker>,
}

#[cfg(unix)]
impl Drop for CtlServer {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.path);
	}
}

/// Where process `pid` listens.
#[cfg(unix)]
pub fn endpoint(pid: u32) -> PathBuf {
	crate::peers::runtime_dir().join(format!("silkterm-ctl-{pid}.sock"))
}

/// Where process `pid` listens.
#[cfg(windows)]
pub fn endpoint(pid: u32) -> PathBuf {
	PathBuf::from(format!(r"\\.\pipe\silkterm-ctl-{pid}"))
}

// Other windows on this settings file find this one through its marker, left
// once the endpoint answers.
fn join_peers() -> Option<crate::peers::Marker> {
	let dir = crate::peers::meeting_dir(crate::peers::own_key()?)?;
	crate::peers::join(&dir, std::process::id())
}

// One connection: a line in, the event to the loop, a reply out. False once
// the loop is gone.
fn answer(stream: impl std::io::Read + Write, proxy: &EventLoopProxy<UserEvent>) -> bool {
	let mut reader = BufReader::new(stream);
	let mut line = String::new();
	if reader.read_line(&mut line).is_err() {
		return true;
	}
	let reply = match parse(line.trim_end(), crate::peers::own_key()) {
		Ok(event) => {
			if proxy.send_event(event).is_err() {
				return false; // event loop gone; the process is exiting
			}
			"ok\n".to_string()
		}
		Err(e) => format!("err {e:#}\n"),
	};
	// a window telling of a save does not wait for this
	let _ = reader.into_inner().write_all(reply.as_bytes());
	true
}

/// Bind the socket, export `SILKTERM_SOCKET`, and serve commands on a background
/// thread. Call before any PTY spawns so shells inherit the variable. Failure is
/// non-fatal (the app just isn't remotely controllable).
#[cfg(unix)]
pub fn serve(proxy: EventLoopProxy<UserEvent>) -> Option<CtlServer> {
	let path = endpoint(std::process::id());
	let _ = std::fs::remove_file(&path); // stale leftover from a recycled pid
	let listener = match std::os::unix::net::UnixListener::bind(&path) {
		Ok(listener) => listener,
		Err(e) => {
			eprintln!("{}: control socket: {e}", crate::config::APP_NAME);
			return None;
		}
	};
	remove_on_any_exit(&path);
	// SAFETY: no PTY or render thread exists yet (set_var is unsafe under
	// edition 2024 because of concurrent readers).
	unsafe { std::env::set_var(ENV_SOCK, &path) };
	std::thread::spawn(move || {
		for stream in listener.incoming() {
			let Ok(stream) = stream else { continue };
			if !answer(stream, &proxy) {
				return;
			}
		}
	});
	Some(CtlServer {
		path,
		_marker: join_peers(),
	})
}

/// The same on a named pipe. An instance is always waiting: the next one is
/// made before the current one is served, so the name only disappears when the
/// process does, and a client that finds no pipe knows the window is gone.
#[cfg(windows)]
pub fn serve(proxy: EventLoopProxy<UserEvent>) -> Option<CtlServer> {
	use std::os::windows::ffi::OsStrExt;
	let name = endpoint(std::process::id());
	let wide: Vec<u16> = name.as_os_str().encode_wide().chain(Some(0)).collect();
	let Some(first) = pipe::make(&wide, true) else {
		eprintln!(
			"{}: control pipe: {}",
			crate::config::APP_NAME,
			std::io::Error::last_os_error()
		);
		return None;
	};
	// SAFETY: no PTY or render thread exists yet (set_var is unsafe under
	// edition 2024 because of concurrent readers).
	unsafe { std::env::set_var(ENV_SOCK, &name) };
	std::thread::spawn(move || {
		let mut waiting = first;
		loop {
			let connected = pipe::wait_for_client(&waiting);
			let Some(next) = pipe::make(&wide, false) else {
				return;
			};
			let current = std::mem::replace(&mut waiting, next);
			if connected && !answer(pipe::Served(current), &proxy) {
				return;
			}
		}
	});
	Some(CtlServer {
		_marker: join_peers(),
	})
}

#[cfg(windows)]
mod pipe {
	use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
	use windows_sys::Win32::Foundation::{
		ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE,
	};
	use windows_sys::Win32::Storage::FileSystem::{
		FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX,
	};
	use windows_sys::Win32::System::Pipes::{
		ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
		PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
	};

	/// `first` refuses a name some other process already serves.
	pub(super) fn make(wide: &[u16], first: bool) -> Option<OwnedHandle> {
		let open = PIPE_ACCESS_DUPLEX
			| if first {
				FILE_FLAG_FIRST_PIPE_INSTANCE
			} else {
				0
			};
		let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
		// SAFETY: `wide` is NUL-terminated and outlives the call; no security
		// attributes means the default ones, which is this user's.
		let handle = unsafe {
			CreateNamedPipeW(
				wide.as_ptr(),
				open,
				mode,
				PIPE_UNLIMITED_INSTANCES,
				512,
				512,
				0,
				std::ptr::null(),
			)
		};
		// SAFETY: a fresh handle owned by nothing else.
		(handle != INVALID_HANDLE_VALUE).then(|| unsafe { OwnedHandle::from_raw_handle(handle) })
	}

	/// A client that came and went before the wait is still a client.
	pub(super) fn wait_for_client(pipe: &OwnedHandle) -> bool {
		// SAFETY: a live pipe handle, and no overlapped I/O.
		unsafe {
			ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) != 0
				|| GetLastError() == ERROR_PIPE_CONNECTED
		}
	}

	/// One served connection. Its reply is flushed before the drop
	/// disconnects, since a disconnect throws away what the client has not read.
	#[derive(Debug)]
	pub(super) struct Served(pub(super) OwnedHandle);

	impl std::io::Read for Served {
		fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
			std::fs::File::from(self.0.try_clone()?).read(buf)
		}
	}

	impl std::io::Write for Served {
		fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
			std::fs::File::from(self.0.try_clone()?).write(buf)
		}
		fn flush(&mut self) -> std::io::Result<()> {
			Ok(())
		}
	}

	impl Drop for Served {
		fn drop(&mut self) {
			// SAFETY: a live pipe handle. Both fail harmlessly once the client
			// has gone.
			unsafe {
				FlushFileBuffers(self.0.as_raw_handle());
				DisconnectNamedPipe(self.0.as_raw_handle());
			}
		}
	}
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

#[cfg(not(any(unix, windows)))]
pub fn serve(_proxy: EventLoopProxy<UserEvent>) -> Option<CtlServer> {
	None
}

// One command line -> the event the app applies. Tab separates verb from value
// so paths with spaces survive. A save is heard only from a window on this
// process's own settings file; a recycled pid can put another one behind a
// marker until that marker is cleared.
fn parse(line: &str, own_key: Option<&str>) -> anyhow::Result<UserEvent> {
	let (verb, value) = match line.split_once('\t') {
		Some((verb, rest)) => (verb, Some(rest)),
		None => (line, None),
	};
	match verb {
		"reload" => Ok(UserEvent::ReloadSettings),
		"wallpaper" => Ok(UserEvent::SetWallpaper(value.map(PathBuf::from))),
		"saved" if value.is_some() && value == own_key => Ok(UserEvent::PeerSaved),
		"saved" => Err(anyhow::anyhow!("another settings file")),
		_ => Err(anyhow::anyhow!("unknown command: {verb}")),
	}
}

/// Hand one line to process `pid` and do not wait for the answer.
pub fn poke(pid: u32, line: &str) -> Told {
	poke_at(&endpoint(pid), line)
}

#[cfg(unix)]
fn poke_at(path: &std::path::Path, line: &str) -> Told {
	use std::io::ErrorKind;
	match std::os::unix::net::UnixStream::connect(path) {
		Ok(mut stream) => {
			let _ = stream.write_all(format!("{line}\n").as_bytes());
			Told::Sent
		}
		Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
			Told::Gone
		}
		Err(_) => Told::Missed,
	}
}

#[cfg(windows)]
fn poke_at(path: &std::path::Path, line: &str) -> Told {
	match open_pipe(path) {
		Ok(mut pipe) => {
			let _ = pipe.write_all(format!("{line}\n").as_bytes());
			Told::Sent
		}
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Told::Gone,
		Err(_) => Told::Missed,
	}
}

#[cfg(not(any(unix, windows)))]
fn poke_at(_path: &std::path::Path, _line: &str) -> Told {
	Told::Missed
}

// Every instance can be busy for the moment between a client and the next
// instance, so a busy pipe is tried again before giving up.
#[cfg(windows)]
fn open_pipe(path: &std::path::Path) -> std::io::Result<std::fs::File> {
	use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
	let mut tries = 0;
	loop {
		match std::fs::OpenOptions::new()
			.read(true)
			.write(true)
			.open(path)
		{
			Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && tries < 25 => {
				tries += 1;
				std::thread::sleep(std::time::Duration::from_millis(20));
			}
			other => return other,
		}
	}
}

/// Client side: deliver one command to the window this shell runs inside.
#[cfg(any(unix, windows))]
pub fn send(cmd: &str) -> anyhow::Result<()> {
	use anyhow::Context as _;
	use std::io::Read;

	let sock = std::env::var(ENV_SOCK)
		.map_err(|_| anyhow::anyhow!("not inside a running SilkTerm window"))?;
	#[cfg(unix)]
	let mut stream = std::os::unix::net::UnixStream::connect(&sock).with_context(|| sock.clone())?;
	#[cfg(windows)]
	let mut stream = open_pipe(std::path::Path::new(&sock)).with_context(|| sock.clone())?;
	stream
		.write_all(cmd.as_bytes())
		.and_then(|()| stream.write_all(b"\n"))?;
	#[cfg(unix)]
	let _ = stream.shutdown(std::net::Shutdown::Write);
	// The server reads one line, so the pipe needs no half close.
	let mut reply = String::new();
	stream.read_to_string(&mut reply)?;
	let reply = reply.trim();
	match reply.strip_prefix("err ") {
		Some(e) => Err(anyhow::anyhow!("{e}")),
		None if reply == "ok" => Ok(()),
		None => Err(anyhow::anyhow!("unexpected reply: {reply}")),
	}
}

#[cfg(not(any(unix, windows)))]
pub fn send(_cmd: &str) -> anyhow::Result<()> {
	anyhow::bail!("control commands aren't supported on this platform yet")
}

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: EjpedZh
	#[test]
	fn parse_commands() {
		let parse = |line| parse(line, Some("k"));
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

	// A save is heard only from a window on the same settings file.
	// Test ID: EsaCcC4
	#[test]
	fn a_save_on_another_file_is_not_heard() {
		assert!(matches!(
			parse("saved\tabc", Some("abc")),
			Ok(UserEvent::PeerSaved)
		));
		assert!(parse("saved\tabd", Some("abc")).is_err());
		assert!(parse("saved", Some("abc")).is_err());
		assert!(parse("saved\tabc", None).is_err());
	}

	// What a save sends reaches a listening window, and a socket nobody listens
	// on any more reads as gone. Driven in a child copy of this test binary
	// (`poke_child` below) started inside the folder, which binds and pokes by
	// bare name: a full path under the run folder can pass the limit on a Unix
	// socket path, 104 bytes on a Mac and 107 on Linux.
	// Test ID: EsaCcC5
	#[cfg(unix)]
	#[test]
	fn a_poke_reaches_a_listener_and_finds_the_dead() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_ctl_poke_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let out = std::process::Command::new(std::env::current_exe().unwrap())
			.args(["--exact", "ctl::tests::poke_child", "--nocapture"])
			.current_dir(&dir)
			.env("SILK_CTL_POKE", "1")
			.output()
			.unwrap();
		assert!(
			out.status.success(),
			"{}{}",
			String::from_utf8_lossy(&out.stdout),
			String::from_utf8_lossy(&out.stderr)
		);
		// a filter that matched nothing passes too
		assert!(dir.join("done").exists(), "the child never ran");
		assert!(dir.join("b.sock").exists(), "a crash leaves the file");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EsaxrEv
	#[cfg(unix)]
	#[test]
	fn poke_child() {
		use std::io::Read;
		if std::env::var_os("SILK_CTL_POKE").is_none() {
			return; // only does anything when the test above starts it
		}
		let live = std::path::Path::new("a.sock");
		let listener = std::os::unix::net::UnixListener::bind(live).unwrap();
		let heard = std::thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			let mut line = String::new();
			stream.read_to_string(&mut line).unwrap();
			line
		});
		assert_eq!(poke_at(live, "saved\tabc"), Told::Sent);
		assert_eq!(heard.join().unwrap(), "saved\tabc\n");
		let dead = std::path::Path::new("b.sock");
		drop(std::os::unix::net::UnixListener::bind(dead).unwrap());
		assert_eq!(poke_at(dead, "saved\tabc"), Told::Gone);
		assert_eq!(
			poke_at(std::path::Path::new("c.sock"), "saved\tabc"),
			Told::Gone
		);
		std::fs::write("done", "").unwrap();
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
			// SAFETY: raise takes no pointer; it signals this process.
			Some("sigterm") => unsafe {
				libc::raise(libc::SIGTERM);
			},
			// SAFETY: as above.
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
