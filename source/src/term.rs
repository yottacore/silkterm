// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{ClipboardType, Config, Osc52, Term};
use alacritty_terminal::tty;
use winit::event_loop::EventLoopProxy;

pub type PaneId = u64;

#[derive(Debug, Clone)]
pub enum UserEvent {
	// new output in this pane's terminal (render only what changed)
	Wakeup(PaneId),
	Title(PaneId, String),
	// a program asked to set the clipboard or the primary selection (OSC 52)
	ClipboardStore(PaneId, ClipboardType, String),
	// terminal replies (cursor position report, device attributes, ...) that
	// must be written back to the PTY
	PtyWrite(PaneId, Vec<u8>),
	// the shell's own process ended, with the status it ended on (--keep-open
	// shows this). Always followed by Exit for the same pane.
	ChildExit(PaneId, String),
	Exit(PaneId),
	// terminal bell (BEL): drives a brief visual flash (text brightens, fades back)
	Bell,
	// control socket (ctl.rs): change the background image live (None = clear).
	// ctl is Unix-only, so these are never constructed on non-unix.
	#[cfg_attr(not(unix), allow(dead_code))]
	SetWallpaper(Option<std::path::PathBuf>),
	// control socket: re-read config.shcl and apply it (same as Menu > Reload)
	#[cfg_attr(not(unix), allow(dead_code))]
	ReloadSettings,
	// wallpaper worker (wallpaper.rs): decoded pixels, ready to upload. Boxed -
	// it carries a whole image, and every other variant is small.
	WallpaperReady(Box<crate::wallpaper::Loaded>),
	// shell scan (shells.rs): the stored shell list with whatever the scan found
	// folded in.
	ShellsReady(Vec<crate::shells::Found>),
	// VT watcher thread (app.rs spawn_vt_watch): the active console changed.
	// Linux GL path only; never constructed elsewhere.
	#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
	VtSwitched,
	// a pick from the macOS menu bar (macmenu.rs); never constructed elsewhere
	#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
	Menu(crate::app::MenuAction),
}

// One line to roll the folding back, should a platform ever need every notice
// delivered separately.
const COALESCE_WAKEUPS: bool = true;

// One outstanding "there is new output" notice per pane.
//
// The engine finishes a read cycle roughly every 900 bytes under a flood, and
// each cycle used to become its own window event: measured on 32 MiB of output,
// about 20,000 of them, costing 2.5 SECONDS of main-thread CPU inside the OS
// message pump alone - more than the parsing and the drawing put together, for
// a message that says nothing but "look again".
//
// Nothing is lost by folding them. The notice carries no payload: whenever the
// window gets round to one it reads the grid as it stands, so a queue of twenty
// identical notices produced twenty identical reads. `handled` is cleared BEFORE
// the window acts on it, so a cycle that arrives mid-handling posts a fresh notice
// rather than being dropped.
#[derive(Debug, Default)]
pub struct WakeGate {
	pending: std::sync::atomic::AtomicBool,
}

impl WakeGate {
	// True when this notice has to be posted (nothing outstanding).
	pub fn post(&self) -> bool {
		!COALESCE_WAKEUPS || !self.pending.swap(true, std::sync::atomic::Ordering::AcqRel)
	}

	pub fn handled(&self) {
		self.pending
			.store(false, std::sync::atomic::Ordering::Release);
	}
}

// bridges alacritty's PTY thread back to the winit loop
#[derive(Debug, Clone)]
pub struct EventProxy {
	id: PaneId,
	proxy: EventLoopProxy<UserEvent>,
	wake: Arc<WakeGate>,
	// the pane's grid and cell size, for a program that asks (CSI 14 t and
	// friends). Kept here because the answer is written on the PTY thread.
	size: Arc<std::sync::Mutex<WindowSize>>,
}

impl EventProxy {
	pub fn new(id: PaneId, proxy: EventLoopProxy<UserEvent>, size: WindowSize) -> Self {
		Self {
			id,
			proxy,
			wake: Arc::new(WakeGate::default()),
			size: Arc::new(std::sync::Mutex::new(size)),
		}
	}

	fn note_size(&self, size: WindowSize) {
		*crate::locks::lock(&self.size) = size;
	}
}

// A bigger OSC 52 store is dropped whole. A clipped one would paste something the
// program never sent, and the parser sets no limit of its own.
const CLIPBOARD_STORE_MAX: usize = 1 << 20;

// The bytes a query event owes the program, or None where there is no answer.
// The event carries its own formatter; all this supplies is the value.
fn query_reply(event: &Event, size: WindowSize) -> Option<Vec<u8>> {
	match event {
		Event::ColorRequest(index, format) => {
			requested_color(*index).map(|rgb| format(rgb).into_bytes())
		}
		Event::TextAreaSizeRequest(format) => Some(format(size).into_bytes()),
		_ => None,
	}
}

// The color a program is asking about, for OSC 4 / 10 / 11 / 12. Indices below
// 256 are the palette; above that are the crate's named slots, of which we can
// answer the three that matter. Answers from the settings rather than from the
// term's own table, which this thread cannot reach - a program that set the
// color itself with OSC therefore reads back the theme's, which is wrong only
// in the rare case where it set one and then asked.
fn requested_color(index: usize) -> Option<alacritty_terminal::vte::ansi::Rgb> {
	use alacritty_terminal::vte::ansi::NamedColor;
	let s = crate::config::settings();
	let [r, g, b] = match u8::try_from(index) {
		Ok(i) => crate::palette::default_indexed(i, &s),
		Err(_) => match index {
			i if i == NamedColor::Foreground as usize => s.fg,
			i if i == NamedColor::Background as usize => s.bg,
			i if i == NamedColor::Cursor as usize => s.cursor,
			_ => return None,
		},
	};
	Some(alacritty_terminal::vte::ansi::Rgb { r, g, b })
}

fn engine_config() -> Config {
	engine_config_for(&crate::config::settings())
}

fn engine_config_for(s: &crate::config::Settings) -> Config {
	Config {
		scrolling_history: s.scrollback,
		semantic_escape_chars: s.word_separators.clone(),
		// stores only: a read would answer the program with somebody else's text
		osc52: Osc52::OnlyCopy,
		..Config::default()
	}
}

// How a shell's exit reads on screen. Platform Display spellings vary
// ("exit status: 1", "exit code: 1"), so say it ourselves.
fn status_text(status: std::process::ExitStatus) -> String {
	if let Some(code) = status.code() {
		return code.to_string();
	}
	#[cfg(unix)]
	{
		use std::os::unix::process::ExitStatusExt;
		if let Some(sig) = status.signal() {
			return format!("signal {sig}");
		}
	}
	"unknown".into()
}

// The one text a program can hand the window at any size. The engine fork keeps
// 2 KiB of a title as well, and this holds the same line here, so an engine
// update cannot quietly take it away.
const TITLE_MAX_BYTES: usize = 2048;

fn capped_title(mut title: String) -> String {
	if title.len() > TITLE_MAX_BYTES {
		let mut end = TITLE_MAX_BYTES;
		while !title.is_char_boundary(end) {
			end -= 1;
		}
		title.truncate(end);
		title.shrink_to_fit();
	}
	title
}

impl EventListener for EventProxy {
	fn send_event(&self, event: Event) {
		let _ = match event {
			Event::Wakeup if !self.wake.post() => Ok(()), // one notice is enough
			Event::Wakeup => self.proxy.send_event(UserEvent::Wakeup(self.id)),
			Event::Title(t) => self
				.proxy
				.send_event(UserEvent::Title(self.id, capped_title(t))),
			// Empty, not the app name: that is how "the program set no title" is
			// told apart from one it happened to set to our own name.
			Event::ResetTitle => self
				.proxy
				.send_event(UserEvent::Title(self.id, String::new())),
			Event::ChildExit(status) => self
				.proxy
				.send_event(UserEvent::ChildExit(self.id, status_text(status))),
			Event::Exit => self.proxy.send_event(UserEvent::Exit(self.id)),
			Event::PtyWrite(text) => self
				.proxy
				.send_event(UserEvent::PtyWrite(self.id, text.into_bytes())),
			Event::Bell => self.proxy.send_event(UserEvent::Bell),
			// Which pane may set the clipboard is the window's call, since only it
			// knows which one is in use.
			Event::ClipboardStore(kind, text) if text.len() <= CLIPBOARD_STORE_MAX => self
				.proxy
				.send_event(UserEvent::ClipboardStore(self.id, kind, text)),
			// Replies the terminal owes the program. Dropping these left anything
			// asking for the background color or the text area size waiting out
			// its timeout on every start and then guessing.
			ref query @ (Event::ColorRequest(..) | Event::TextAreaSizeRequest(..)) => {
				let size = *crate::locks::lock(&self.size);
				match query_reply(query, size) {
					Some(bytes) => self.proxy.send_event(UserEvent::PtyWrite(self.id, bytes)),
					None => Ok(()),
				}
			}
			// MouseCursorDirty and any other events: nothing to forward
			_ => Ok(()),
		};
	}
}

// The grid a pane asks for, held to what the engine can take. The engine
// documents two columns as its least, since a wide character needs both, but
// does not enforce it: one column panics on a CJK character or an emoji, and
// shrinking wide text to one column reflows without end.
fn grid_dims(cols: usize, lines: usize) -> TermDimensions {
	TermDimensions {
		columns: cols.max(alacritty_terminal::term::MIN_COLUMNS),
		screen_lines: lines.max(1),
	}
}

// size descriptor handed to the crate; history is set separately via Config
#[derive(Debug, Clone, Copy)]
pub struct TermDimensions {
	pub columns: usize,
	pub screen_lines: usize,
}

impl Dimensions for TermDimensions {
	fn total_lines(&self) -> usize {
		self.screen_lines
	}
	fn screen_lines(&self) -> usize {
		self.screen_lines
	}
	fn columns(&self) -> usize {
		self.columns
	}
}

// What a pane's shell is doing, as its tab reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Task {
	/// A command is in the foreground right now.
	Running(String),
	/// Back at the prompt; this is the last command that ran.
	Last(String),
	/// This shell has never run anything.
	#[default]
	Idle,
}

pub struct TermInstance {
	pub term: Arc<FairMutex<Term<EventProxy>>>,
	// same gate the engine's thread posts through, so the window can re-arm it
	notifier: EventProxy,
	pub cols: usize,
	pub lines: usize,
	sender: EventLoopSender,
	io: Option<std::thread::JoinHandle<()>>,
	// where the shell last SAID it is (OSC 7 / OSC 9;9, see cwd.rs) - the only
	// answer for a shell whose location the OS cannot see, PowerShell above all
	reported_cwd: crate::cwd::Reported,
	// for tab titles: the PTY master fd, which names the foreground process group.
	#[cfg(unix)]
	master_fd: std::os::unix::io::RawFd,
	#[cfg(unix)]
	shell_pid: u32,
	// The last command this shell ran, so an idle tab can still say what it was
	// doing. Both platforms answer "what is running" differently and both keep
	// this the same way.
	last_program: Option<String>,
	// throttles the per-frame task probe (see task())
	task_cache: Option<(std::time::Instant, Task)>,
	// windows: the shell's pid and the time it started, for the child-process
	// probe that stands in for a foreground process group (see at_shell_prompt),
	// plus that probe's answer for as long as it holds (see note_activity).
	// Either 0 means "unknown"; the inner Option is the running command's name.
	#[cfg(windows)]
	shell_pid: u32,
	#[cfg(windows)]
	shell_started: u64,
	#[cfg(windows)]
	child_probe: std::cell::RefCell<Option<Option<String>>>,
	#[allow(clippy::type_complexity)]
	cwd_cache: std::cell::RefCell<Option<(std::time::Instant, Option<std::path::PathBuf>)>>,
}

impl std::fmt::Debug for TermInstance {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TermInstance")
			.field("cols", &self.cols)
			.field("lines", &self.lines)
			.finish_non_exhaustive()
	}
}

impl TermInstance {
	// The window has taken delivery of an output notice, so the next read cycle
	// posts a fresh one (see WakeGate).
	pub fn wake_handled(&self) {
		self.notifier.wake.handled();
	}

	// command is owned by the spawned terminal conceptually; the by-value
	// constructor input threads through split_at/spawn_pane as a move
	#[allow(clippy::needless_pass_by_value)]
	pub fn spawn(
		id: PaneId,
		cols: usize,
		lines: usize,
		cell_w: u16,
		cell_h: u16,
		proxy: EventLoopProxy<UserEvent>,
		command: Option<Vec<String>>,
		cwd: Option<std::path::PathBuf>,
	) -> anyhow::Result<Self> {
		let dims = grid_dims(cols, lines);
		let (cols, lines) = (dims.columns, dims.screen_lines);

		let config = engine_config();
		let event_proxy = EventProxy::new(
			id,
			proxy,
			WindowSize {
				num_cols: cols as u16,
				num_lines: lines as u16,
				cell_width: cell_w,
				cell_height: cell_h,
			},
		);
		let mut engine = Term::new(config, &dims, event_proxy.clone());
		// the rows a region scroll pushes off, for the slide's reveal strip
		engine.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
		let term = Arc::new(FairMutex::new(engine));

		let win = WindowSize {
			num_cols: cols as u16,
			num_lines: lines as u16,
			cell_width: cell_w,
			cell_height: cell_h,
		};

		let pty = tty::new(&pane_options(command.as_deref(), cwd), win, id)?;
		// Capture the master fd + shell pid before the event loop takes the pty;
		// they drive the tab title (foreground program). The fd stays valid for
		// the pane's life (the loop owns the pty until close).
		#[cfg(unix)]
		let master_fd = {
			use std::os::unix::io::AsRawFd;
			pty.file().as_raw_fd()
		};
		#[cfg(unix)]
		let shell_pid = pty.child().id();
		// Windows has no master fd; the ConPTY child watcher carries the shell pid.
		#[cfg(windows)]
		let shell_pid = pty
			.child_watcher()
			.pid()
			.map_or(0, std::num::NonZeroU32::get);
		let notifier = event_proxy.clone();
		// the tap goes between the PTY and the parser; the bytes are untouched
		#[cfg(unix)]
		let reported_cwd = crate::cwd::Reported::for_tty(master_fd);
		#[cfg(not(unix))]
		let reported_cwd = crate::cwd::Reported::default();
		let pty = crate::cwd::TappedPty::new(pty, reported_cwd.clone());
		let event_loop = EventLoop::new(term.clone(), event_proxy, pty, false, false)?;
		let sender = event_loop.channel();
		let handle = event_loop.spawn();
		// wrap the join handle so we don't carry its tuple return type around
		let io = std::thread::spawn(move || {
			let _ = handle.join();
		});

		Ok(Self {
			term,
			notifier,
			cols,
			lines,
			sender,
			io: Some(io),
			reported_cwd,
			#[cfg(unix)]
			master_fd,
			#[cfg(unix)]
			shell_pid,
			last_program: None,
			task_cache: None,
			#[cfg(windows)]
			shell_pid,
			#[cfg(windows)]
			shell_started: process_start_time(shell_pid).unwrap_or(0),
			#[cfg(windows)]
			child_probe: std::cell::RefCell::new(None),
			cwd_cache: std::cell::RefCell::new(None),
		})
	}

	// What this pane's shell is doing, for the tab to say. "Running" is the
	// program the shell has in the foreground right now; "Last" is what it ran
	// most recently, which is what an idle tab reports instead. A shell that has
	// never run anything is Idle, and the tab shows its directory instead.
	//
	// Both platforms answer this, by quite different means - a foreground process
	// group on unix, a live child process on Windows (see at_shell_prompt) - and
	// both are throttled the same way: render asks per tab per frame, and paying
	// for a probe on every idle blink frame added up.
	pub fn task(&mut self) -> Task {
		const PROBE_IVL: std::time::Duration = std::time::Duration::from_millis(250);
		let now = std::time::Instant::now();
		if let Some((at, task)) = &self.task_cache {
			if now.duration_since(*at) < PROBE_IVL {
				return task.clone();
			}
		}
		let task = match self.running_program() {
			Some(program) => {
				self.last_program = Some(program.clone());
				Task::Running(program)
			}
			None => match &self.last_program {
				Some(last) => Task::Last(last.clone()),
				None => Task::Idle,
			},
		};
		self.task_cache = Some((now, task.clone()));
		task
	}

	// The foreground process group is the shell's own while it sits at its prompt,
	// and a command's while one runs - so a pgid that is neither answers None.
	#[cfg(unix)]
	fn running_program(&mut self) -> Option<String> {
		// SAFETY: takes no pointer, so a closed or reused fd can only answer wrong.
		let pgid = unsafe { libc::tcgetpgrp(self.master_fd) };
		if pgid <= 0 || pgid as u32 == self.shell_pid {
			return None;
		}
		proc_comm(pgid as u32)
	}

	#[cfg(windows)]
	fn running_program(&mut self) -> Option<String> {
		self.command_child()
	}

	#[cfg(not(any(unix, windows)))]
	#[allow(clippy::unused_self)]
	fn running_program(&mut self) -> Option<String> {
		None
	}

	// The shell's current directory, for a new tab/split to start in.
	//
	// What the shell SAID wins over what the OS can see, and that order is the
	// point: a shell reporting its directory is answering the question directly,
	// while the OS can only see where the process itself sits - which for
	// PowerShell is the launch directory forever. A report that no longer names
	// a directory (a stale one, or a path on the far side of an ssh) is dropped
	// rather than trusted, and the OS answer stands instead. So is a report from
	// a program that has since exited, on unix, where that can be told.
	pub fn cwd(&self) -> Option<std::path::PathBuf> {
		// Throttled the way `task()` beside it is, and for the same reason: the
		// tab strip asks once per tab per frame, and both halves of the answer
		// touch the filesystem - a stat, plus a /proc read and another stat. On a
		// mount that has stopped answering, each of those stalls the render.
		const PROBE_IVL: std::time::Duration = std::time::Duration::from_millis(250);
		let now = std::time::Instant::now();
		if let Some((at, dir)) = self.cwd_cache.borrow().as_ref() {
			if now.duration_since(*at) < PROBE_IVL {
				return dir.clone();
			}
		}
		let dir = self
			.reported_cwd
			.live()
			.filter(|dir| dir.is_dir())
			.or_else(|| self.os_cwd());
		*self.cwd_cache.borrow_mut() = Some((now, dir.clone()));
		dir
	}

	// Where the OS says the shell process itself is (see process_cwd).
	#[cfg(unix)]
	fn os_cwd(&self) -> Option<std::path::PathBuf> {
		process_cwd(self.shell_pid)
	}

	// Windows keeps a process's current directory in its own address space
	// rather than anywhere the OS will hand out, so the answer is read from the
	// shell's PEB (see peb_cwd). What that CAN'T see is a shell that keeps its
	// own idea of "where I am" and never tells the OS: measured on this box,
	// PowerShell 7 and Windows PowerShell 5.1 both leave the process directory
	// at the launch directory across a `Set-Location`, so a PowerShell pane
	// reports where it started. cmd.exe, Git Bash and MSYS2 all call
	// SetCurrentDirectory and read back correctly.
	#[cfg(windows)]
	fn os_cwd(&self) -> Option<std::path::PathBuf> {
		peb_cwd(self.shell_pid)
	}

	// Neither /proc nor a PEB: only what a shell reports can answer here.
	#[cfg(not(any(unix, windows)))]
	#[allow(clippy::unused_self)]
	fn os_cwd(&self) -> Option<std::path::PathBuf> {
		None
	}

	// Is the shell itself (not a spawned command) the terminal's foreground
	// process? Drives copy-output's command start/end detection. On unix that is
	// the foreground process group: the fg pgid equals the shell's while at the
	// prompt, and a command's while it runs. Windows answers the same question a
	// different way, below.
	#[cfg(unix)]
	pub fn at_shell_prompt(&self) -> bool {
		// SAFETY: takes no pointer, so a closed or reused fd can only answer wrong.
		let pgid = unsafe { libc::tcgetpgrp(self.master_fd) };
		pgid <= 0 || pgid as u32 == self.shell_pid
	}
	// A Windows console has no foreground process group, so the stand-in is "does
	// the shell have a live child?" - measured on this box: a command the shell
	// launches is its DIRECT child and is gone again by the time the prompt
	// returns, while the console host (conhost/OpenConsole) hangs off OUR process,
	// never the shell's. A shell builtin spawns nothing, which reads as "at the
	// prompt" throughout - right, since its output ends when the prompt returns.
	// A background job (PowerShell's Start-Job) does read as a command still
	// running; Windows offers nothing that tells one from a foreground command.
	// The only way to ask is a walk of the whole process table, and that is not
	// cheap - 6.4ms per scan measured here, release and debug alike, across 237
	// processes - so the answer is cached until the terminal next stirs (see
	// note_activity) instead of being taken per event-loop pass. Callers only
	// ask once output has gone quiet, so in practice this is one scan per
	// command rather than one per frame.
	#[cfg(windows)]
	pub fn at_shell_prompt(&self) -> bool {
		self.command_child().is_none()
	}

	// The name of the command the shell is running, if any - the one scan answers
	// both questions, so a tab title costs nothing on top of copy-output's.
	#[cfg(windows)]
	fn command_child(&self) -> Option<String> {
		if self.shell_pid == 0 {
			return None; // no pid to probe: report "at prompt" (the feature stays inert)
		}
		if let Some(answer) = self.child_probe.borrow().as_ref() {
			return answer.clone();
		}
		let answer = command_child_name(self.shell_pid, self.shell_started);
		*self.child_probe.borrow_mut() = Some(answer.clone());
		answer
	}

	// Windows: the cached at-prompt answer holds only until the terminal next
	// stirs. Both halves matter - anything typed can start a command, and a
	// command that ends always brings the prompt back with it, so its own last
	// act is PTY output. Anywhere else this is nothing.
	#[cfg(windows)]
	pub fn note_activity(&self) {
		*self.child_probe.borrow_mut() = None;
	}

	#[cfg(not(windows))]
	#[allow(clippy::unused_self)]
	pub fn note_activity(&self) {}

	pub fn write<B: Into<Vec<u8>>>(&self, bytes: B) {
		self.note_activity();
		let _ = self.sender.send(Msg::Input(bytes.into().into()));
	}

	// Put text on screen without a PTY behind it. The engine's thread owns the
	// parser and dies with the shell, so anything added afterwards (the
	// --keep-open exit line) needs a parser of our own.
	pub fn feed(&self, bytes: &[u8]) {
		let mut parser = alacritty_terminal::vte::ansi::Processor::<
			alacritty_terminal::vte::ansi::StdSyncHandler,
		>::default();
		parser.advance(&mut *self.term.lock(), bytes);
	}

	pub fn resize(&mut self, cols: usize, lines: usize, cell_w: u16, cell_h: u16) {
		let dims = grid_dims(cols, lines);
		let (cols, lines) = (dims.columns, dims.screen_lines);
		if cols == self.cols && lines == self.lines {
			return;
		}
		self.cols = cols;
		self.lines = lines;
		self.term.lock_unfair().resize(dims);
		let win = WindowSize {
			num_cols: cols as u16,
			num_lines: lines as u16,
			cell_width: cell_w,
			cell_height: cell_h,
		};
		self.notifier.note_size(win);
		let _ = self.sender.send(Msg::Resize(win));
	}
}

impl Drop for TermInstance {
	fn drop(&mut self) {
		let _ = self.sender.send(Msg::Shutdown);
		if let Some(io) = self.io.take() {
			join_for(io, SHUTDOWN_WAIT);
		}
	}
}

// How long closing a pane waits for its reader thread (join_for).
const SHUTDOWN_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

// Wait for a pane's reader thread to end, but not for good. Its last act is
// waiting on the shell after the hang-up signal, and a shell that ignores it
// (a `nohup` job, a trap) ends only when it chooses. This runs on the
// window's thread, so the whole window froze until then. Past the wait the
// handle is dropped and the thread finishes on its own. True if it ended.
fn join_for(io: std::thread::JoinHandle<()>, wait: std::time::Duration) -> bool {
	let deadline = std::time::Instant::now() + wait;
	while !io.is_finished() {
		if std::time::Instant::now() >= deadline {
			return false;
		}
		std::thread::sleep(std::time::Duration::from_millis(1));
	}
	let _ = io.join();
	true
}

// Variables the launching shell keeps for ITSELF, which must not ride along
// into a different shell.
//
// A terminal hands its child whatever environment it was launched with, and for
// anything the user exported that is exactly right. A shell's own private
// bookkeeping is not: pwsh 7 PREPENDS its own module directories to
// PSModulePath in its process, so a Windows PowerShell 5.1 pane opened anywhere
// below one resolves PSReadLine to pwsh's copy instead of its own, and cannot
// load it - the 5.1 copy is signed as a Windows OS component and is exempt from
// a Restricted execution policy while the pwsh 7 copy is not, so 5.1 starts
// with "Cannot load PSReadline module." and no line editing. Measured on this
// box; it reproduces in a bare cmd.exe launched from pwsh, with no terminal
// involved at all. PSExecutionPolicyPreference is the same shape - pwsh's
// -ExecutionPolicy sets it and EVERY descendant inherits it, so a pane can run
// under a policy nobody chose for it.
//
// pwsh runs on Linux and macOS too and mutates the same variable there, and a
// side-by-side install (a distro pwsh beside a preview, or a Homebrew one beside
// the .pkg) is the same collision as 5.1-below-7 - so the list is not
// platform-split. OLDPWD earns its place on every platform for a different
// reason: it is the launching shell's own `cd -` target, and a pane opens
// somewhere else entirely, so inheriting it points `cd -` at a directory the
// user was never in.
//
// What is deliberately NOT here: VIRTUAL_ENV and CONDA_*, which a user activates
// and then WANTS a pane to keep (and which cannot be dropped honestly anyway -
// the matching PATH edits would stay, leaving a half-activated environment);
// SHLVL, which is a real nesting count every shell agrees on rather than one
// shell's private state.
//
// A name may only join this list if a desktop session NEVER sets it - see
// session_env below, whose unix arm cannot tell the difference.
const SHELL_PRIVATE_ENV: &[&str] = &["PSModulePath", "PSExecutionPolicyPreference", "OLDPWD"];

// Put the shell-private variables back to what a freshly launched process would
// see, so a pane's shell starts the way it would from the desktop. Everything
// else is left exactly as inherited - discarding the whole environment would
// throw away the user's own exports, which is the one thing inheriting from a
// shell is for.
//
// Called ONCE from main, before any thread exists: an environment write is
// process-global and unsound beside a reader. Doing it to our own environment
// rather than per spawn is what makes it cover every path at once - the first
// pane, a split, a new tab, a new window, the shell scan and the PowerShell the
// profile installer starts.
pub fn sanitize_shell_env() {
	// No answer means leave the environment alone. A pane that starts with a
	// stale variable beats one that starts with none.
	let Some(session) = session_env() else {
		return;
	};
	let inherited: std::collections::HashMap<String, String> = std::env::vars().collect();
	for (name, value) in env_fixups(SHELL_PRIVATE_ENV, &session, &inherited) {
		// SAFETY: single-threaded here - this runs at the top of main, before the
		// event loop, any worker thread or any PTY exists.
		unsafe {
			match value {
				Some(want) => std::env::set_var(&name, want),
				None => std::env::remove_var(&name),
			}
		}
	}
}

// What has to change for each named variable to read the way a freshly launched
// process would see it: Some(value) to set, None to drop (the session never set
// it, so neither should a pane). Pure, and the list is passed in rather than
// read off cfg!, so both platforms' answers are testable from either box - the
// same reason config_base_for takes a Layout.
#[cfg_attr(not(windows), allow(dead_code))]
fn env_fixups(
	names: &[&str],
	session: &std::collections::HashMap<String, String>,
	inherited: &std::collections::HashMap<String, String>,
) -> Vec<(String, Option<String>)> {
	// Windows environment names are case-insensitive, and a block keeps whatever
	// spelling set it first, so the two sides can disagree on case alone.
	let find = |vars: &std::collections::HashMap<String, String>, name: &str| {
		vars.iter()
			.find(|(key, _)| key.eq_ignore_ascii_case(name))
			.map(|(_, value)| value.clone())
	};
	names
		.iter()
		.filter_map(|name| {
			let want = find(session, name);
			// already what it should be (launched from the desktop, say)
			if want == find(inherited, name) {
				return None;
			}
			Some(((*name).to_string(), want))
		})
		.collect()
}

// The environment a freshly launched process would see - machine plus user,
// merged the way the desktop composes it, so it stays right on a box whose
// PowerShell lives somewhere unusual or whose variables come from domain
// policy. NULL as the token yields SYSTEM variables only, so the process token
// is required.
#[cfg(windows)]
fn session_env() -> Option<std::collections::HashMap<String, String>> {
	use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
	use windows_sys::Win32::Security::TOKEN_QUERY;
	use windows_sys::Win32::System::Environment::{
		CreateEnvironmentBlock, DestroyEnvironmentBlock,
	};
	use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

	// SAFETY: plain Win32 calls. The token and the block are each released on
	// every path out, and any failure returns None so the caller stands aside.
	unsafe {
		let mut token: HANDLE = std::ptr::null_mut();
		if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
			return None;
		}
		let mut block: *mut core::ffi::c_void = std::ptr::null_mut();
		let made = CreateEnvironmentBlock(&raw mut block, token, 0);
		CloseHandle(token);
		if made == 0 || block.is_null() {
			return None;
		}
		let raw = parse_env_block(&read_env_block(block.cast::<u16>()));
		DestroyEnvironmentBlock(block);
		// A machine variable is stored as REG_EXPAND_SZ and comes back with its
		// references intact - PSModulePath really is spelled
		// "%ProgramFiles%\\WindowsPowerShell\\Modules" here - so a shell handed one
		// raw would search a directory that does not exist. Windows expands them
		// once when it composes an environment; so does this, against the SESSION
		// block rather than our own, since ours is the thing being replaced.
		let vars = raw
			.iter()
			.map(|(name, value)| (name.clone(), expand_refs(value, &raw)))
			.collect();
		Some(vars)
	}
}

// Unix has no equivalent of CreateEnvironmentBlock - nothing will say what a
// freshly launched program would see, because the answer is composed by PAM, the
// session manager and the login shell between them and is never recorded
// anywhere afterwards. But every name on the list above is one a desktop session
// does not set, so the answer for those IS the empty set, and each of them is
// dropped rather than reset. That is the whole reason the list may only ever
// carry variables a session never sets: a name that a login profile legitimately
// exports would be dropped here rather than restored, and nothing on this
// platform could tell the two apart.
#[cfg(not(windows))]
// The Option is the Windows arm's failure, kept so both arms read the same.
#[allow(clippy::unnecessary_wraps)]
fn session_env() -> Option<std::collections::HashMap<String, String>> {
	Some(std::collections::HashMap::new())
}

// Copy an environment block out of the OS's memory so the parsing above it can
// be an ordinary function over a slice. Safety: `block` must be a live block
// that ends in two NULs, as CreateEnvironmentBlock makes.
#[cfg(windows)]
unsafe fn read_env_block(block: *const u16) -> Vec<u16> {
	let mut len = 0;
	// two NULs in a row close the block
	// SAFETY: the walk stops at the two NULs, so no read passes the block's end.
	while unsafe { *block.add(len) } != 0 || unsafe { *block.add(len + 1) } != 0 {
		len += 1;
	}
	// SAFETY: the first `len + 1` units were all just read.
	unsafe { std::slice::from_raw_parts(block, len + 1) }.to_vec()
}

// One pass of %NAME% substitution, the way Windows expands a REG_EXPAND_SZ when
// it builds an environment: a name it does not know is left standing rather than
// blanked, and a lone % is a literal.
#[cfg_attr(not(windows), allow(dead_code))]
fn expand_refs(value: &str, vars: &std::collections::HashMap<String, String>) -> String {
	let mut out = String::with_capacity(value.len());
	let mut rest = value;
	while let Some(open) = rest.find('%') {
		let (before, tail) = rest.split_at(open);
		out.push_str(before);
		match tail[1..].find('%') {
			Some(len) if len > 0 => {
				let name = &tail[1..=len];
				match vars.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)) {
					Some((_, found)) => out.push_str(found),
					None => out.push_str(&tail[..=len + 1]),
				}
				rest = &tail[len + 2..];
			}
			_ => {
				out.push('%');
				rest = &tail[1..];
			}
		}
	}
	out.push_str(rest);
	out
}

// An environment block is NAME=VALUE runs separated by NUL. A name is never
// empty, so a leading '=' marks one of the hidden per-drive entries Windows
// keeps ("=C:=C:\dir") and the split starts past the first character.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_env_block(block: &[u16]) -> std::collections::HashMap<String, String> {
	block
		.split(|unit| *unit == 0)
		.filter_map(|entry| {
			let text = String::from_utf16_lossy(entry);
			let at = text
				.char_indices()
				.skip(1)
				.find(|(_, ch)| *ch == '=')
				.map(|(idx, _)| idx)?;
			Some((text[..at].to_string(), text[at + 1..].to_string()))
		})
		.collect()
}

// Where a process is now. A deleted dir reads back from /proc with a
// " (deleted)" suffix, so the answer has to still exist.
#[cfg(all(unix, not(target_os = "macos")))]
fn process_cwd(pid: u32) -> Option<std::path::PathBuf> {
	std::fs::read_link(format!("/proc/{pid}/cwd"))
		.ok()
		.filter(|dir| dir.is_dir())
}

// macOS has no /proc. bash and zsh never report where they are, so before this
// a split or new tab from one of them had no directory to inherit and opened
// where SilkTerm was started - outside the git project, so a PowerShell tab
// opened from a bash pane there showed no git status in its prompt.
#[cfg(target_os = "macos")]
fn process_cwd(pid: u32) -> Option<std::path::PathBuf> {
	use std::os::unix::ffi::OsStrExt;
	let pid = libc::c_int::try_from(pid).ok().filter(|&pid| pid > 0)?;
	let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_vnodepathinfo>()).ok()?;
	// SAFETY: all-integer C struct, so zeroed is a valid value; the call writes
	// at most `size` bytes into it and says how many it wrote.
	let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
	// SAFETY: as above.
	let wrote = unsafe {
		libc::proc_pidinfo(
			pid,
			libc::PROC_PIDVNODEPATHINFO,
			0,
			std::ptr::from_mut(&mut info).cast(),
			size,
		)
	};
	if wrote != size {
		return None;
	}
	// libc splits the path's MAXPATHLEN chars into rows; it ends at the NUL
	let path: Vec<u8> = info
		.pvi_cdir
		.vip_path
		.iter()
		.flatten()
		.map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
		.take_while(|&b| b != 0)
		.collect();
	let dir = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&path));
	(!path.is_empty() && dir.is_dir()).then_some(dir)
}

// Executable basename of a process from /proc/<pid>/comm (Linux/most Unix).
#[cfg(all(unix, not(target_os = "macos")))]
fn proc_comm(pid: u32) -> Option<String> {
	let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
	named(&comm)
}

// macOS has no /proc, so a tab there never named the program running in it,
// and the minimap's per-program switch never saw one either. The short info
// is asked for rather than `proc_name`, which refuses another user's process,
// so a `sudo` command went unnamed (measured on pid 1). Like Linux's comm,
// it is the first 16 bytes of the name.
#[cfg(target_os = "macos")]
fn proc_comm(pid: u32) -> Option<String> {
	// <sys/proc_info.h>; the libc this builds against does not have it yet
	const PROC_PIDT_SHORTBSDINFO: libc::c_int = 13;
	let pid = libc::c_int::try_from(pid).ok().filter(|&pid| pid > 0)?;
	let size = libc::c_int::try_from(std::mem::size_of::<BsdShortInfo>()).ok()?;
	let mut info = BsdShortInfo::default();
	// SAFETY: the call writes at most `size` bytes and says how many it wrote
	let wrote = unsafe {
		libc::proc_pidinfo(
			pid,
			PROC_PIDT_SHORTBSDINFO,
			0,
			std::ptr::from_mut(&mut info).cast(),
			size,
		)
	};
	if wrote != size {
		return None;
	}
	let comm: Vec<u8> = info.comm.iter().copied().take_while(|&b| b != 0).collect();
	named(&String::from_utf8_lossy(&comm))
}

// `struct proc_bsdshortinfo` from <sys/proc_info.h>, which libc does not carry.
#[cfg(target_os = "macos")]
#[derive(Default)]
#[repr(C)]
struct BsdShortInfo {
	pid: u32,
	ppid: u32,
	pgid: u32,
	status: u32,
	comm: [u8; 16],
	flags: u32,
	uid: u32,
	gid: u32,
	ruid: u32,
	rgid: u32,
	svuid: u32,
	svgid: u32,
	rfu: u32,
}

#[cfg(unix)]
fn named(comm: &str) -> Option<String> {
	let comm = program_name(comm.trim());
	(!comm.is_empty()).then(|| comm.to_string())
}

// A process that renames itself writes "name: what it is doing" - tmux's client
// reports "tmux: client", sshd reports "sshd: user@pts/0". The program is the
// part before the colon, and that is what a tab should say and what the
// minimap's list is matched against.
#[cfg(unix)]
fn program_name(comm: &str) -> &str {
	comm.split_once(':').map_or(comm, |(name, _)| name).trim()
}

// Does a process-table row belong to a command this shell launched? Windows
// recycles pids aggressively and a row keeps whatever parent id it was born
// with, so the parent id ALONE can name an unrelated long-lived process whose
// creator's pid the shell later inherited - which would read as a command that
// never finishes and would silently kill copy-output for that pane. A child
// cannot predate its parent, so the start times settle it. An unknown child
// start time (a process we may not open, so never one of ours) answers no; an
// unknown shell start time has nothing to compare against, so the parent id
// stands on its own.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_command_child(child_started: Option<u64>, shell_started: u64) -> bool {
	shell_started == 0 || child_started.is_some_and(|started| started >= shell_started)
}

// Windows: the name of `shell_pid`'s live child process, if it has one. Walks
// the process table - there is no narrower query - and stops at the first real
// child. The name is the executable's, without its extension, which is the same
// shape unix reports through /proc comm.
#[cfg(windows)]
fn command_child_name(shell_pid: u32, shell_started: u64) -> Option<String> {
	use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
	use windows_sys::Win32::System::Diagnostics::ToolHelp::{
		CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
		TH32CS_SNAPPROCESS,
	};

	// SAFETY: takes no pointer.
	let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
	if snapshot == INVALID_HANDLE_VALUE {
		return None; // can't tell: answer "no command", i.e. at the prompt
	}
	// SAFETY: a C struct of integers and a u16 array, so all zeros is valid.
	let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
	entry.dwSize = u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0);
	let mut found = None;
	// SAFETY: a live snapshot handle, and a live entry with `dwSize` set.
	let mut more = unsafe { Process32FirstW(snapshot, &raw mut entry) };
	while more != 0 {
		if entry.th32ParentProcessID == shell_pid
			&& is_command_child(process_start_time(entry.th32ProcessID), shell_started)
		{
			found = Some(exe_display_name(&entry.szExeFile));
			break;
		}
		// SAFETY: as for Process32FirstW.
		more = unsafe { Process32NextW(snapshot, &raw mut entry) };
	}
	// SAFETY: closed once, and not used after.
	unsafe { CloseHandle(snapshot) };
	found
}

// A PROCESSENTRY32W name (NUL-padded UTF-16) as the bare program name a tab
// shows: no directory, no extension. An empty or unreadable name still has to
// answer something, or a running command would read as an idle prompt.
#[cfg(windows)]
fn exe_display_name(raw: &[u16]) -> String {
	let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
	let name = String::from_utf16_lossy(&raw[..end]);
	let name = name.rsplit(['\\', '/']).next().unwrap_or(&name);
	let stem = name
		.rfind('.')
		.filter(|dot| *dot > 0)
		.map_or(name, |dot| &name[..dot]);
	if stem.is_empty() {
		"command".to_string()
	} else {
		stem.to_string()
	}
}

// Windows: a process's creation time as a raw FILETIME, for is_command_child.
// None when the process is gone or can't be opened (a protected/system process,
// which is never a command our shell started).
#[cfg(windows)]
fn process_start_time(pid: u32) -> Option<u64> {
	use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
	use windows_sys::Win32::System::Threading::{
		GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
	};

	let mut created = FILETIME {
		dwLowDateTime: 0,
		dwHighDateTime: 0,
	};
	let mut ignored = [FILETIME {
		dwLowDateTime: 0,
		dwHighDateTime: 0,
	}; 3];
	// SAFETY: takes no pointer.
	let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
	if process.is_null() {
		return None;
	}
	// SAFETY: an open process handle and four live FILETIMEs to write.
	let ok = unsafe {
		GetProcessTimes(
			process,
			&raw mut created,
			&raw mut ignored[0],
			&raw mut ignored[1],
			&raw mut ignored[2],
		)
	};
	// SAFETY: closed once, and not used after.
	unsafe { CloseHandle(process) };
	(ok != 0).then(|| u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

// Windows: a process's current directory, out of its own PEB. There is no API
// that answers this for another process - GetCurrentDirectory only ever reports
// the caller's - so the walk is: ProcessBasicInformation for the PEB address,
// then two reads across it. Both offsets are undocumented but have been fixed
// since Vista, and the result is checked with is_dir() before it is believed,
// so a layout that ever did move degrades to "don't know" rather than to a
// wrong directory.
#[cfg(all(windows, target_pointer_width = "64"))]
fn peb_cwd(shell_pid: u32) -> Option<std::path::PathBuf> {
	use std::ffi::c_void;
	use std::os::windows::ffi::OsStringExt;

	use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
	use windows_sys::Win32::Foundation::CloseHandle;
	use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
	use windows_sys::Win32::System::Threading::{
		OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
	};

	// PEB -> RTL_USER_PROCESS_PARAMETERS -> CurrentDirectory.DosPath, 64-bit.
	const PROCESS_PARAMETERS: usize = 0x20;
	const CURRENT_DIRECTORY: usize = 0x38;

	if shell_pid == 0 {
		return None;
	}
	// SAFETY: every call below is a plain FFI call on values this function owns;
	// the handle is closed on every path out, and ReadProcessMemory reports a
	// failure rather than faulting when an address isn't mapped.
	unsafe {
		let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, shell_pid);
		if process.is_null() {
			return None;
		}
		let read = |at: usize, into: *mut c_void, len: usize| -> bool {
			let mut got = 0usize;
			at != 0
				&& ReadProcessMemory(process, at as *const c_void, into, len, &raw mut got) != 0
				&& got == len
		};
		let mut basic: ProcessBasicInfo = std::mem::zeroed();
		let status = NtQueryInformationProcess(
			process,
			ProcessBasicInformation,
			(&raw mut basic).cast::<c_void>(),
			u32::try_from(size_of::<ProcessBasicInfo>()).unwrap_or(0),
			std::ptr::null_mut(),
		);
		let peb = usize::try_from(basic.peb).unwrap_or(0);
		let mut params = 0usize;
		let mut dos_path = UnicodeString::default();
		let ok = status == 0
			&& read(
				peb + PROCESS_PARAMETERS,
				(&raw mut params).cast::<c_void>(),
				size_of::<usize>(),
			) && read(
			params + CURRENT_DIRECTORY,
			(&raw mut dos_path).cast::<c_void>(),
			size_of::<UnicodeString>(),
		);
		// Length is in BYTES, and a path is UTF-16 - halve it for the buffer.
		let chars = usize::from(dos_path.length) / 2;
		let mut wide = vec![0u16; chars];
		let ok = ok
			&& chars > 0
			&& read(
				dos_path.buffer as usize,
				wide.as_mut_ptr().cast::<c_void>(),
				chars * 2,
			);
		CloseHandle(process);
		if !ok {
			return None;
		}
		// It comes back with a trailing separator and may name a directory that
		// has since been removed, so it is checked the way the unix side is.
		let dir = std::path::PathBuf::from(std::ffi::OsString::from_wide(&wide));
		dir.is_dir().then_some(dir)
	}
}

// The 32-bit PEB is laid out differently and no 32-bit Windows target is built,
// so it answers "don't know" rather than reading the wrong offsets.
#[cfg(all(windows, not(target_pointer_width = "64")))]
fn peb_cwd(_shell_pid: u32) -> Option<std::path::PathBuf> {
	None
}

// The two structures the walk reads, declared here rather than taken from
// windows-sys because they describe ANOTHER process's memory: what matters is
// the 64-bit layout of the process being read, which is fixed, and declaring
// them costs less than a windows-sys feature pulled in for two fields.
#[cfg(all(windows, target_pointer_width = "64"))]
#[repr(C)]
struct ProcessBasicInfo {
	exit_status: i32,
	peb: u64, // repr(C) pads to the 8-byte alignment, as the real one does
	affinity_mask: usize,
	base_priority: i32,
	unique_pid: usize,
	parent_pid: usize,
}

// The UNICODE_STRING sitting inside RTL_USER_PROCESS_PARAMETERS.
#[cfg(all(windows, target_pointer_width = "64"))]
#[derive(Default)]
#[repr(C)]
struct UnicodeString {
	length: u16,
	capacity: u16,
	_pad: u32,
	buffer: u64,
}

// A directory a shell reported is not always usable as the next shell's
// working directory. A pane inside a WSL distribution reports a posix path,
// which Windows either rejects or - worse - resolves against the current
// drive, which is how a new tab came up in a garbled /tmp.
fn usable_cwd(dir: Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
	// Every source of a pane's directory funnels through here: the reported one,
	// the one on the command line, the one in the config. A relative path is not
	// a directory anyone chose - it resolves against wherever SilkTerm itself was
	// started, which is nowhere the person can see. A posix path on Windows is the
	// other half of the same rule; that one arrives from a WSL pane reporting
	// where it is, and it names a real directory in the wrong filesystem.
	dir.filter(|dir| dir.is_absolute())
}

fn pane_options(command: Option<&[String]>, cwd: Option<std::path::PathBuf>) -> tty::Options {
	// a CLI/menu-supplied command runs as argv[0] + args; else the default shell
	let mut opts = tty::Options::default();
	// The argv is split already, so Windows has to get it back whole: a spaced
	// program path ran a planted program in front of it, and a spaced --cd
	// reached WSL as two words. Unix never joins the words at all.
	#[cfg(windows)]
	{
		opts.escape_args = true;
	}
	let with_cd = match (command, cwd.as_deref()) {
		(Some(argv), Some(dir)) => wsl_cd(argv, dir),
		_ => None,
	};
	let argv = with_cd.as_deref().or(command);
	if let Some((prog, args)) = argv.and_then(<[String]>::split_first) {
		opts.shell = Some(tty::Shell::new(prog.clone(), args.to_vec()));
	}
	// start in an inherited directory (new tab/split follows the source pane)
	opts.working_directory = usable_cwd(cwd);
	opts.env.extend(crate::integration::pane_env(command));
	opts
}

// wsl.exe launches the shell inside the distribution, which does not inherit
// the Windows working directory. --cd is how it is told, and it takes a
// Windows path or a posix one, so whichever spelling the source pane reported
// can go straight through. Options have to come before the command, hence the
// insert rather than a push. None where there is nothing to do.
fn wsl_cd(argv: &[String], dir: &std::path::Path) -> Option<Vec<String>> {
	// split on both separators: a Windows path reaches this on any platform, and
	// Path would hand back the whole string for one on unix
	let first = argv.first()?;
	let prog = first
		.rsplit(['/', '\\'])
		.next()
		.unwrap_or(first)
		.to_ascii_lowercase();
	if prog != "wsl.exe" && prog != "wsl" {
		return None;
	}
	if argv.iter().any(|a| a.eq_ignore_ascii_case("--cd")) {
		return None;
	}
	let mut out = argv.to_vec();
	out.splice(
		1..1,
		["--cd".to_string(), dir.to_string_lossy().into_owned()],
	);
	Some(out)
}

#[cfg(test)]
mod tests {
	use super::{
		SHELL_PRIVATE_ENV, TITLE_MAX_BYTES, WakeGate, capped_title, env_fixups, expand_refs,
		grid_dims, is_command_child, join_for, parse_env_block, query_reply, requested_color,
		usable_cwd, wsl_cd,
	};
	#[cfg(unix)]
	use super::{program_name, status_text};

	// tmux, editors over ssh and muffer's auto-copy all set the clipboard with
	// OSC 52, and whether the engine passes a store on is its config's call. A
	// default that moved would quietly break copying again, or start answering
	// reads.
	// Test ID: EpyJWG0
	#[test]
	fn a_program_can_set_the_clipboard_but_never_read_it() {
		use alacritty_terminal::event::{Event, EventListener};
		use alacritty_terminal::term::Term;
		use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
		use std::sync::{Arc, Mutex};

		#[derive(Clone, Default)]
		struct Seen(Arc<Mutex<Vec<String>>>);
		impl EventListener for Seen {
			fn send_event(&self, event: Event) {
				let line = match event {
					Event::ClipboardStore(kind, text) => format!("{kind:?} {text}"),
					Event::ClipboardLoad(kind, _) => format!("load {kind:?}"),
					_ => return,
				};
				self.0.lock().expect("seen lock").push(line);
			}
		}

		let seen = Seen::default();
		let dims = super::TermDimensions {
			columns: 20,
			screen_lines: 4,
		};
		let mut term = Term::new(super::engine_config(), &dims, seen.clone());
		let mut parser = Processor::<StdSyncHandler>::default();
		// "hello" to the clipboard, then the primary selection, then a read
		parser.advance(
			&mut term,
			b"\x1b]52;c;aGVsbG8=\x07\x1b]52;p;aGVsbG8=\x07\x1b]52;c;?\x07",
		);
		assert_eq!(
			*seen.0.lock().expect("seen lock"),
			["Clipboard hello", "Selection hello"]
		);
	}

	// The configured word separators are what the engine splits words on.
	// Test ID: Er2UFeT
	#[test]
	fn the_engine_splits_words_on_the_configured_separators() {
		let s = crate::config::Settings {
			word_separators: " ,;".to_string(),
			scrollback: 1234,
			..Default::default()
		};
		let config = super::engine_config_for(&s);
		assert_eq!(config.semantic_escape_chars, " ,;");
		assert_eq!(config.scrolling_history, 1234);
	}

	// A program could set a title of any size and push it thousands of times onto
	// the title stack, and each copy was kept. A megabyte title grew the whole
	// program by about 4 GiB.
	// Test ID: EqBcj6m
	#[test]
	fn a_program_title_is_held_to_a_size() {
		use alacritty_terminal::event::{Event, EventListener};
		use alacritty_terminal::term::Term;
		use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
		use std::sync::{Arc, Mutex};

		#[derive(Clone, Default)]
		struct Titles(Arc<Mutex<Vec<(usize, usize)>>>);
		impl EventListener for Titles {
			fn send_event(&self, event: Event) {
				if let Event::Title(t) = event {
					let mut seen = self.0.lock().expect("titles lock");
					seen.push((t.len(), t.capacity()));
				}
			}
		}

		// the engine half: set, push, set something short, pop back
		let titles = Titles::default();
		let mut term = Term::new(super::engine_config(), &grid_dims(20, 4), titles.clone());
		let mut parser = Processor::<StdSyncHandler>::default();
		let big = format!("\x1b]2;{}\x07", "x".repeat(1 << 20));
		parser.advance(&mut term, big.as_bytes());
		parser.advance(&mut term, "\x1b[22t".repeat(64).as_bytes());
		parser.advance(&mut term, b"\x1b]2;short\x07\x1b[23t");
		let seen = titles.0.lock().expect("titles lock").clone();
		assert_eq!(seen.len(), 3, "{seen:?}");
		for (len, cap) in &seen {
			assert!(
				*len <= TITLE_MAX_BYTES && *cap < 2 * TITLE_MAX_BYTES,
				"{seen:?}"
			);
		}
		assert_eq!(seen[1].0, 5, "an ordinary title is left alone");
		assert!(seen[2].0 > 0, "the pushed title comes back");

		// and ours, whatever the engine does
		let held = capped_title("x".repeat(1 << 20));
		assert_eq!(held.len(), TITLE_MAX_BYTES);
		assert!(held.capacity() < 2 * TITLE_MAX_BYTES);
		let wide = capped_title(format!("x{}", "\u{4e2d}".repeat(TITLE_MAX_BYTES)));
		assert!(wide.len() > TITLE_MAX_BYTES - 3 && wide.ends_with('\u{4e2d}'));
		assert_eq!(capped_title("~/src".into()), "~/src");
	}

	// A pane narrowed below two cells asked the engine for one column, where a
	// wide character panicked and wide text already on screen reflowed until
	// memory ran out.
	// Test ID: EqBQjc0
	#[test]
	fn a_pane_too_narrow_for_a_wide_character_still_takes_one() {
		use alacritty_terminal::event::VoidListener;
		use alacritty_terminal::grid::Dimensions;
		use alacritty_terminal::term::Term;
		use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

		let mut parser = Processor::<StdSyncHandler>::default();
		let mut narrow = Term::new(super::engine_config(), &grid_dims(1, 3), VoidListener);
		parser.advance(&mut narrow, "\u{4e2d}\u{1f600}x".as_bytes());
		assert_eq!(narrow.columns(), 2);

		let mut wide = Term::new(super::engine_config(), &grid_dims(6, 3), VoidListener);
		parser.advance(&mut wide, "\u{4e2d}\u{4e2d}\u{4e2d}".as_bytes());
		wide.resize(grid_dims(1, 3));
		assert_eq!(wide.columns(), 2);
		assert_eq!(grid_dims(0, 0).screen_lines, 1);
	}

	// Closing a pane whose shell ignores the hang-up signal froze the whole
	// window until that shell ended, since the close waited on it.
	// Test ID: EqBSso5
	#[test]
	fn closing_a_pane_does_not_wait_on_a_shell_that_stays() {
		use std::time::{Duration, Instant};
		let stays = std::thread::spawn(|| std::thread::sleep(Duration::from_secs(5)));
		let start = Instant::now();
		assert!(!join_for(stays, Duration::from_millis(100)));
		assert!(
			start.elapsed() < Duration::from_secs(2),
			"waited {:?}",
			start.elapsed()
		);

		let ends = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(10)));
		assert!(
			join_for(ends, Duration::from_secs(5)),
			"a normal close is reaped"
		);
	}

	// A program that closed its terminal and kept running spun the reader thread
	// at a whole core until it exited. The engine read EIO, went round again, and
	// found the PTY still readable. The fix is in the engine fork, so an engine
	// update is what would bring this back.
	// Test ID: EqBacFs
	#[cfg(target_os = "linux")]
	#[test]
	fn a_hung_up_terminal_does_not_spin_the_reader() {
		let out = std::process::Command::new(std::env::current_exe().unwrap())
			.args(["--exact", "term::tests::hung_up_child", "--nocapture"])
			.env("SILK_HUP_CHILD", "1")
			.output()
			.unwrap();
		let text = String::from_utf8_lossy(&out.stdout);
		assert!(
			out.status.success(),
			"{text}{}",
			String::from_utf8_lossy(&out.stderr)
		);
		let cpu: u64 = text
			.lines()
			.find_map(|l| l.strip_prefix("cpu_ms "))
			.and_then(|v| v.trim().parse().ok())
			.expect("child reports its CPU time");
		// about 2000 while it spun
		assert!(cpu < 300, "the reader used {cpu} ms of CPU in 2 s");
		// the pause may not cost output from a program that opens its terminal
		// again, or hold up the end of the pane
		assert!(text.contains("reopened yes"), "{text}");
		let ended: u64 = text
			.lines()
			.find_map(|l| l.strip_prefix("ended_ms "))
			.and_then(|v| v.trim().parse().ok())
			.expect("child reports when the pane ended");
		assert!(ended < 1000, "the pane ended {ended} ms after its program");
	}

	// Test ID: EqBacFt
	#[cfg(target_os = "linux")]
	#[test]
	fn hung_up_child() {
		use alacritty_terminal::event::{VoidListener, WindowSize};
		use alacritty_terminal::event_loop::EventLoop;
		use alacritty_terminal::index::{Column, Line, Point};
		use alacritty_terminal::term::Term;
		use alacritty_terminal::tty;
		use std::time::{Duration, Instant};

		fn cpu_ms() -> u64 {
			// SAFETY: an all-integer C struct, so all zeros is valid.
			let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
			// SAFETY: writes into the live struct above.
			unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) };
			let ms = |t: libc::timeval| t.tv_sec as u64 * 1000 + t.tv_usec as u64 / 1000;
			ms(usage.ru_utime) + ms(usage.ru_stime)
		}
		if std::env::var_os("SILK_HUP_CHILD").is_none() {
			return; // only does anything when the test above starts it
		}
		let dims = grid_dims(80, 24);
		let term = std::sync::Arc::new(alacritty_terminal::sync::FairMutex::new(Term::new(
			super::engine_config(),
			&dims,
			VoidListener,
		)));
		let opts = tty::Options {
			shell: Some(tty::Shell::new(
				"/bin/sh".into(),
				vec![
					"-c".into(),
					"exec </dev/null >/dev/null 2>&1; sleep 3; echo back >/dev/tty; sleep 0.5"
						.into(),
				],
			)),
			..Default::default()
		};
		let win = WindowSize {
			num_cols: 80,
			num_lines: 24,
			cell_width: 8,
			cell_height: 16,
		};
		let pty = tty::new(&opts, win, 0).unwrap();
		let event_loop = EventLoop::new(term.clone(), VoidListener, pty, false, false).unwrap();
		let start = Instant::now();
		let handle = event_loop.spawn();
		// let the shell get as far as the sleep
		std::thread::sleep(Duration::from_millis(300));
		let before = cpu_ms();
		std::thread::sleep(Duration::from_secs(2));
		println!("cpu_ms {}", cpu_ms() - before);
		// the loop ends by itself once the program exits
		let _ = handle.join();
		println!(
			"ended_ms {}",
			start.elapsed().as_millis().saturating_sub(3500)
		);
		let text = term.lock().bounds_to_string(
			Point::new(Line(0), Column(0)),
			Point::new(Line(23), Column(79)),
		);
		println!(
			"reopened {}",
			if text.contains("back") { "yes" } else { "no" }
		);
	}

	fn argv(words: &str) -> Vec<String> {
		words.split(' ').map(str::to_string).collect()
	}

	// A program that asks what color the background is (neovim, delta, termbg on
	// every start) used to get nothing back and wait out its timeout. The three
	// named slots and the whole palette answer now; anything else does not.
	// Test ID: EpHUIAS
	#[test]
	fn a_color_query_gets_an_answer() {
		use alacritty_terminal::event::{Event, WindowSize};
		use alacritty_terminal::vte::ansi::NamedColor;
		let s = crate::config::settings();
		for (index, want) in [
			(NamedColor::Foreground as usize, s.fg),
			(NamedColor::Background as usize, s.bg),
			(NamedColor::Cursor as usize, s.cursor),
		] {
			let got = requested_color(index).expect("a named slot answers");
			assert_eq!([got.r, got.g, got.b], want);
		}
		for index in [0usize, 1, 15, 128, 255] {
			let got = requested_color(index).expect("the palette answers");
			let want = crate::palette::default_indexed(index as u8, &s);
			assert_eq!([got.r, got.g, got.b], want, "index {index}");
		}
		assert!(requested_color(9999).is_none(), "and nothing else does");

		// and the whole reply, as the listener assembles it
		let size = WindowSize {
			num_cols: 80,
			num_lines: 24,
			cell_width: 9,
			cell_height: 18,
		};
		let bg = Event::ColorRequest(
			NamedColor::Background as usize,
			std::sync::Arc::new(|rgb: alacritty_terminal::vte::ansi::Rgb| {
				format!("\x1b]11;rgb:{:04x}/{:04x}/{:04x}\x07", rgb.r, rgb.g, rgb.b)
			}),
		);
		let reply = query_reply(&bg, size).expect("a background query is answered");
		assert!(reply.starts_with(b"\x1b]11;rgb:"), "{reply:?}");

		let area = Event::TextAreaSizeRequest(std::sync::Arc::new(|w: WindowSize| {
			format!(
				"\x1b[4;{};{}t",
				w.num_lines * w.cell_height,
				w.num_cols * w.cell_width
			)
		}));
		assert_eq!(
			query_reply(&area, size).as_deref(),
			Some(b"\x1b[4;432;720t".as_slice())
		);

		assert!(query_reply(&Event::Bell, size).is_none());
	}

	// tmux renames its client, so /proc/pid/comm reads "tmux: client" and a plain
	// name comparison never matched it.
	// Test ID: EoqHGQz
	#[cfg(unix)]
	#[test]
	fn a_renamed_process_still_reports_its_program() {
		assert_eq!(program_name("tmux: client"), "tmux");
		assert_eq!(program_name("sshd: jim@pts/3"), "sshd");
		assert_eq!(program_name("less"), "less");
		assert_eq!(program_name(""), "");
	}

	// Test ID: Eolpl0a
	#[test]
	fn wsl_is_handed_the_directory_ahead_of_its_own_command() {
		let dir = std::path::Path::new("/home/jim");
		assert_eq!(
			wsl_cd(&argv("wsl.exe -d DebianWSL2"), dir).unwrap(),
			argv("wsl.exe --cd /home/jim -d DebianWSL2")
		);
		// options must precede the command, so the insert goes at the front
		assert_eq!(
			wsl_cd(&argv("wsl.exe bash -l"), dir).unwrap(),
			argv("wsl.exe --cd /home/jim bash -l")
		);
		assert_eq!(
			wsl_cd(&argv(r"C:\Windows\System32\WSL.EXE"), dir).unwrap(),
			argv(r"C:\Windows\System32\WSL.EXE --cd /home/jim")
		);
	}

	// Test ID: Eolpl0b
	#[test]
	fn nothing_else_gets_a_cd_argument() {
		let dir = std::path::Path::new("/home/jim");
		assert!(wsl_cd(&argv("pwsh.exe -NoLogo"), dir).is_none());
		assert!(wsl_cd(&argv("wslconfig.exe"), dir).is_none());
		assert!(wsl_cd(&[], dir).is_none());
		// a directory of its own already there is left alone
		assert!(wsl_cd(&argv("wsl.exe --cd ~ -d DebianWSL2"), dir).is_none());
	}

	// Test ID: EpQN0oR
	#[test]
	fn a_pane_only_ever_starts_in_an_absolute_directory() {
		let keep = |s: &str| usable_cwd(Some(std::path::PathBuf::from(s))).is_some();
		assert!(usable_cwd(None).is_none());
		if cfg!(windows) {
			assert!(!keep("/home/jim"));
			assert!(!keep("/tmp"));
			assert!(keep(r"C:\Users\jim"));
			assert!(keep(r"\\host\share"));
			// Drive-relative: which directory this is depends on per-drive state
			// nothing here can see.
			assert!(!keep("C:jim"));
		} else {
			assert!(keep("/home/jim"));
		}
		// Relative, on any platform. A shell can report one, and it would open a
		// pane in whatever sits under SilkTerm's own directory.
		assert!(!keep("relative/path"));
		assert!(!keep(""));
		assert!(!keep("."));
	}

	// The --keep-open line reads this out to the user, so it has to say the same
	// thing on every platform.
	// Test ID: EoSKhlp
	#[cfg(unix)]
	#[test]
	fn an_exit_status_reads_the_same_on_every_platform() {
		use std::os::unix::process::ExitStatusExt;
		let st = |raw| status_text(std::process::ExitStatus::from_raw(raw));
		assert_eq!(st(0), "0");
		assert_eq!(st(1 << 8), "1");
		assert_eq!(st(9), "signal 9");
	}

	// Folding the notices may never LOSE one: the window clears the gate before
	// it looks at the grid, so a read cycle that arrives mid-handling posts again.
	// Test ID: EnLddKq
	#[test]
	fn one_notice_stands_until_the_window_takes_it() {
		let gate = WakeGate::default();
		assert!(gate.post()); // nothing outstanding: this one goes
		assert!(!gate.post()); // ... and the next hundred ride on it
		assert!(!gate.post());
		gate.handled();
		assert!(gate.post());
	}

	// Test ID: EqH4isw
	#[cfg(windows)]
	#[test]
	fn a_pane_hands_its_program_and_arguments_down_whole() {
		let wsl = ["wsl.exe".to_string()];
		let opts = super::pane_options(Some(&wsl), Some(crate::testdir::run_dir().to_path_buf()));
		assert!(
			opts.escape_args,
			"a split argv has to be joined back with quotes"
		);
	}

	// The Windows freeze on a long run of output: a read that empties the pipe
	// without finding it empty leaves the pipe's waker unarmed, so the engine's
	// reader thread has to say when it fills again. The fix is in the engine
	// fork, and this is what fails if an engine update leaves it behind.
	// Test ID: EqH4isx
	#[cfg(windows)]
	#[test]
	fn a_pane_under_a_flood_keeps_saying_there_is_more() {
		use alacritty_terminal::tty::{self, EventedReadWrite};
		use polling::{Event, Events, PollMode, Poller};
		use std::io::Read;

		let flood = "for /l %i in (0,0,1) do @echo 0123456789012345678901234567890123456789";
		let args = std::iter::once("/c")
			.chain(flood.split(' '))
			.map(String::from)
			.collect();
		let opts = tty::Options {
			shell: Some(tty::Shell::new("cmd.exe".into(), args)),
			..Default::default()
		};
		let win = alacritty_terminal::event::WindowSize {
			num_cols: 80,
			num_lines: 24,
			cell_width: 8,
			cell_height: 16,
		};
		let mut pty = tty::new(&opts, win, 0).expect("spawn");
		let poller = std::sync::Arc::new(Poller::new().unwrap());
		// SAFETY: the pty stays alive, and registered, until the end of the test
		unsafe { pty.register(&poller, Event::readable(0), PollMode::Level) }.unwrap();
		// the engine's own read buffer, which is also the pipe's capacity
		let mut buf = vec![0u8; 0x10_0000];
		let mut events = Events::new();
		let mut total = 0;
		// cmd's echo loop gives about half a megabyte a second, and the engine
		// without the fix goes quiet at round 2, after its first full drain
		for round in 0..8 {
			events.clear();
			poller
				.wait(&mut events, Some(std::time::Duration::from_secs(10)))
				.unwrap();
			assert!(
				!events.is_empty(),
				"no word for 10 s after {total} bytes in {round} rounds"
			);
			total += pty.reader().read(&mut buf).unwrap_or(0);
		}
		// Ending the flood by dropping the pty can block while nobody reads
		// (ClosePseudoConsole waits on its output), so end the shell and leave
		// the rest to process exit.
		let shell = pty.child_watcher().raw_handle();
		// SAFETY: the handle is the pty's own and still open
		unsafe { windows_sys::Win32::System::Threading::TerminateProcess(shell, 0) };
		std::mem::forget(pty);
	}

	// Windows has no /proc, so a new tab or split can only inherit a directory
	// if the shell's own PEB can be read - and the point is the directory it is
	// in NOW. A process that never moves must read back where it was started
	// (that half is deterministic), and one that calls SetCurrentDirectory - as
	// cmd.exe's `cd` does - must read back the new place, not the old one.
	// Test ID: EnWyJ9U
	#[cfg(windows)]
	#[test]
	fn a_windows_shell_reports_where_it_is_now_not_where_it_started() {
		use std::io::Write;
		use std::process::{Command, Stdio};

		use super::peb_cwd;
		let real = |dir: &std::path::Path| std::fs::canonicalize(dir).expect("canonicalize");
		let started_in = crate::testdir::run_dir().to_path_buf();
		let moved_to = std::path::PathBuf::from(r"C:\Windows");

		// Each shell is held open on its own stdin pipe, and runs NOTHING. That is
		// load-bearing rather than tidy: `Child::kill` is TerminateProcess, which
		// ends one process and never its tree, so the old `cmd /c ping ...` left the
		// ping behind every time this test ran. They do not exit on their own either
		// (measured: alive for days at 0% CPU, 2 MB apiece), so they accumulate one
		// or two per `cargo test` - fifteen had piled up before anyone counted. A
		// cmd.exe waiting on a pipe is the same shell doing the same `cd` with no
		// grandchild under it to strand.
		let hold = |dir: &std::path::Path| {
			Command::new("cmd.exe")
				.arg("/k")
				.current_dir(dir)
				.stdin(Stdio::piped())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.spawn()
				.expect("spawn cmd.exe")
		};
		// one that stays put, for the plain "did we read the right one"
		let mut still = hold(&started_in);
		// and one that moves, for the half that matters - told to down its own stdin
		let mut roams = hold(&started_in);
		if let Some(pipe) = roams.stdin.as_mut() {
			let _ = writeln!(pipe, "cd /d {}", moved_to.display());
			let _ = pipe.flush();
		}

		// the move takes a moment to happen; nothing else here waits on a clock
		let mut roamed = None;
		for _ in 0..60 {
			roamed = peb_cwd(roams.id());
			if roamed
				.as_deref()
				.map(std::path::Path::to_path_buf)
				.map(|dir| real(&dir))
				== Some(real(&moved_to))
			{
				break;
			}
			std::thread::sleep(std::time::Duration::from_millis(50));
		}
		let stayed = peb_cwd(still.id());
		// killed BEFORE the assertions, so a failing assert still cleans up after
		// itself - these two wait forever otherwise, being fed by a pipe
		let _ = still.kill();
		let _ = roams.kill();
		// reaped, or the test leaves two zombies behind it
		let _ = still.wait();
		let _ = roams.wait();

		assert_eq!(
			stayed.map(|dir| real(&dir)),
			Some(real(&started_in)),
			"a process that never moved read back somewhere else"
		);
		assert_eq!(
			roamed.map(|dir| real(&dir)),
			Some(real(&moved_to)),
			"the directory the shell moved to never came back"
		);
	}

	// The unix half of the same question. bash and zsh never report where they
	// are, so a split or new tab from one inherits only what the OS can say -
	// and a Mac, with no /proc, said nothing, so the new pane opened outside the
	// project it was meant to be in.
	// Test ID: ErkhGGP
	#[cfg(unix)]
	#[test]
	fn a_unix_shell_reports_where_it_is_now_not_where_it_started() {
		use std::io::Write;
		use std::process::{Command, Stdio};

		use super::process_cwd;
		let real = |dir: &std::path::Path| std::fs::canonicalize(dir).expect("canonicalize");
		let started_in = crate::testdir::run_dir().join("cwd started");
		let moved_to = crate::testdir::run_dir().join("cwd moved");
		std::fs::create_dir_all(&started_in).expect("mkdir");
		std::fs::create_dir_all(&moved_to).expect("mkdir");

		// each shell waits on its own stdin and runs nothing but a cd
		let hold = |dir: &std::path::Path| {
			Command::new("/bin/sh")
				.current_dir(dir)
				.stdin(Stdio::piped())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.spawn()
				.expect("spawn /bin/sh")
		};
		let mut still = hold(&started_in);
		let mut roams = hold(&started_in);
		if let Some(pipe) = roams.stdin.as_mut() {
			let _ = writeln!(pipe, "cd '{}'", moved_to.display());
			let _ = pipe.flush();
		}

		let mut roamed = None;
		for _ in 0..60 {
			roamed = process_cwd(roams.id());
			if roamed.as_deref().map(real) == Some(real(&moved_to)) {
				break;
			}
			std::thread::sleep(std::time::Duration::from_millis(50));
		}
		let stayed = process_cwd(still.id());
		let _ = still.kill();
		let _ = roams.kill();
		let _ = still.wait();
		let _ = roams.wait();

		assert_eq!(
			stayed.as_deref().map(real),
			Some(real(&started_in)),
			"a process that never moved read back somewhere else"
		);
		assert_eq!(
			roamed.as_deref().map(real),
			Some(real(&moved_to)),
			"the directory the shell moved to never came back"
		);
	}

	// A tab names the program in the foreground: the terminal's foreground
	// process group, looked up by its leader. On a Mac that lookup read /proc,
	// which is not there, so no tab ever named anything.
	// Test ID: ErkjAN1
	#[cfg(unix)]
	#[test]
	fn the_foreground_program_is_named_by_its_process_group() {
		use std::os::unix::process::CommandExt;
		use std::process::{Command, Stdio};

		let mut child = Command::new("sleep")
			.arg("30")
			.process_group(0)
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn sleep");
		let pid = libc::pid_t::try_from(child.id()).expect("pid");
		// SAFETY: a plain query on a child this test owns
		let pgid = unsafe { libc::getpgid(pid) };
		let name = u32::try_from(pgid).ok().and_then(super::proc_comm);
		let _ = child.kill();
		let _ = child.wait();
		assert_eq!(pgid, pid, "the child did not lead its own group");
		assert_eq!(name.as_deref(), Some("sleep"));
		// another user's program, as under sudo: pid 1 is root's everywhere
		assert!(super::proc_comm(1).is_some(), "a root process went unnamed");
	}

	// A recycled pid is the failure this guard exists for: the row claims the
	// shell as its parent but started before the shell did, so it belongs to
	// whoever held that pid before.
	// Test ID: EnL1S0f
	#[test]
	fn a_child_that_predates_the_shell_is_not_its_command() {
		assert!(is_command_child(Some(200), 100));
		assert!(is_command_child(Some(100), 100)); // same tick: still a child
		assert!(!is_command_child(Some(50), 100));
		assert!(!is_command_child(None, 100)); // unopenable, so not ours
		assert!(is_command_child(None, 0)); // shell time unknown: parent id alone
	}

	fn vars(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
		pairs
			.iter()
			.map(|(name, value)| ((*name).to_string(), (*value).to_string()))
			.collect()
	}

	// A shell's private bookkeeping must not decide how a DIFFERENT shell starts.
	// pwsh 7 prepends its own module directories to PSModulePath in its process,
	// so a Windows PowerShell 5.1 pane opened below one finds pwsh's PSReadLine
	// ahead of its own, cannot load it, and starts with no line editing.
	// Test ID: EncA4gS
	#[test]
	fn a_shell_private_variable_is_put_back_to_the_session_value() {
		let session = vars(&[(
			"PSModulePath",
			r"C:\Program Files\WindowsPowerShell\Modules",
		)]);
		let inherited = vars(&[(
			"PSModulePath",
			r"C:\Program Files\PowerShell\7\Modules;C:\Program Files\WindowsPowerShell\Modules",
		)]);
		assert_eq!(
			env_fixups(&["PSModulePath"], &session, &inherited),
			vec![(
				"PSModulePath".to_string(),
				Some(r"C:\Program Files\WindowsPowerShell\Modules".to_string()),
			)]
		);
	}

	// pwsh's -ExecutionPolicy sets this and every descendant inherits it. The
	// session never sets it, so neither should a pane - and that means DROPPING
	// it, not handing the shell an empty one to read.
	// Test ID: EncA4gT
	#[test]
	fn a_variable_the_session_never_set_is_dropped() {
		let session = vars(&[("PATH", r"C:\bin")]);
		let inherited = vars(&[("PSExecutionPolicyPreference", "Bypass")]);
		assert_eq!(
			env_fixups(&["PSExecutionPolicyPreference"], &session, &inherited),
			vec![("PSExecutionPolicyPreference".to_string(), None)]
		);
	}

	// Launched from the desktop rather than from a shell, the inherited
	// environment already IS the session one. That is the ordinary case and it
	// must write nothing at all.
	// Test ID: EncA4gU
	#[test]
	fn an_environment_that_already_matches_needs_no_fixups() {
		let session = vars(&[("PSModulePath", "one;two")]);
		let inherited = vars(&[("PSModulePath", "one;two")]);
		assert!(env_fixups(&["PSModulePath"], &session, &inherited).is_empty());
	}

	// Everything the user exported themselves is the reason a pane inherits at
	// all, so a variable off the list is left alone however far it has drifted.
	// Test ID: EncA4gV
	#[test]
	fn only_the_named_variables_are_touched() {
		let session = vars(&[("VIRTUAL_ENV", ""), ("PSModulePath", "one")]);
		let inherited = vars(&[("VIRTUAL_ENV", "/home/me/venv"), ("PSModulePath", "one")]);
		assert!(env_fixups(&["PSModulePath"], &session, &inherited).is_empty());
	}

	// Windows environment names are case-insensitive and a block keeps whichever
	// spelling set it first, so the two sides can differ by case alone - which is
	// not a difference and must not read as one.
	// Test ID: EncA4gW
	#[test]
	fn a_name_that_differs_only_in_case_is_the_same_variable() {
		let session = vars(&[("PSModulePath", "one")]);
		let inherited = vars(&[("PSMODULEPATH", "one")]);
		assert!(env_fixups(&["PSModulePath"], &session, &inherited).is_empty());
	}

	// The block is NUL-separated NAME=VALUE closed by a second NUL, and it also
	// carries the hidden per-drive entries Windows keeps, whose NAME begins with
	// '=' - so the separator can never be the first character.
	// Test ID: EncA4gX
	#[test]
	fn an_environment_block_splits_names_from_values() {
		let mut block: Vec<u16> = Vec::new();
		for entry in [r"=C:=C:\work", "PSModulePath=one;two", r"Path=C:\bin"] {
			block.extend(entry.encode_utf16());
			block.push(0);
		}
		block.push(0);

		let found = parse_env_block(&block);
		assert_eq!(
			found.get("PSModulePath").map(String::as_str),
			Some("one;two")
		);
		assert_eq!(found.get("Path").map(String::as_str), Some(r"C:\bin"));
		assert_eq!(found.get("=C:").map(String::as_str), Some(r"C:\work"));
	}

	// PSModulePath is stored as REG_EXPAND_SZ, so the session block hands it over
	// spelled with its references intact. A shell given that raw would search a
	// directory that does not exist - and an unknown name has to survive rather
	// than collapse to nothing, which would silently shorten a search path.
	// Test ID: EncA4gY
	#[test]
	fn a_stored_reference_expands_and_an_unknown_one_survives() {
		let vars = vars(&[("ProgramFiles", r"C:\Program Files")]);
		assert_eq!(
			expand_refs(r"%ProgramFiles%\WindowsPowerShell\Modules", &vars),
			r"C:\Program Files\WindowsPowerShell\Modules"
		);
		assert_eq!(expand_refs("%NotAThing%;tail", &vars), "%NotAThing%;tail");
		assert_eq!(expand_refs("100% done", &vars), "100% done");
	}

	// The unix arm has no way to ask what a freshly launched program would see, so
	// it answers with the empty set and every listed variable is DROPPED. That is
	// only honest because nothing on the list is a variable a session sets - and it
	// is the same path Windows takes for a variable its session block lacks.
	// Test ID: EncBmYy
	#[test]
	fn an_empty_session_drops_every_private_variable() {
		let inherited = vars(&[
			("PSModulePath", "/opt/microsoft/powershell/7/Modules"),
			("PSExecutionPolicyPreference", "Bypass"),
			("OLDPWD", "/home/me/elsewhere"),
			("PATH", "/usr/bin"),
		]);
		let fixups = env_fixups(SHELL_PRIVATE_ENV, &vars(&[]), &inherited);
		assert_eq!(fixups.len(), SHELL_PRIVATE_ENV.len());
		assert!(fixups.iter().all(|(_, value)| value.is_none()));
		// and the one nobody asked about is untouched
		assert!(!fixups.iter().any(|(name, _)| name == "PATH"));
	}

	// Whatever a program prints, the terminal must not end up typing for it.
	//
	// Everything a pane shows arrives from the other end of a pty, and some of it
	// comes from further away than that: a file, a remote host, a build log. The
	// terminal answers a few of the questions such a stream can ask, and those
	// answers go back down the pty as if the user had typed them - so an answer
	// that could carry a newline, or the program's own text, would let the program
	// run a command nobody typed. That is the oldest hole in terminal emulators
	// and it is the one thing worth hammering hardest.
	mod fuzz {
		use alacritty_terminal::event::{Event, EventListener, WindowSize};
		use alacritty_terminal::grid::Dimensions;
		use alacritty_terminal::term::{Config, Term};
		use alacritty_terminal::vte::ansi::Processor;
		use std::sync::{Arc, Mutex};

		use crate::fuzz;
		use crate::term::{TermDimensions, query_reply};

		const COLS: usize = 20;
		const LINES: usize = 8;
		const HISTORY: usize = 200;
		// Planted in every title, icon name and clipboard the stream sets. Nothing
		// the generator emits can produce it by chance.
		const MARKER: &str = "kQ7marker7Qk";

		// Stands in for the real listener, and forwards exactly what it forwards:
		// the engine's own replies, plus the two queries we answer ourselves.
		// Everything else - the clipboard included - is dropped on the floor.
		#[derive(Clone, Default)]
		struct Replies(Arc<Mutex<Vec<u8>>>);

		impl EventListener for Replies {
			fn send_event(&self, event: Event) {
				let size = WindowSize {
					num_cols: COLS as u16,
					num_lines: LINES as u16,
					cell_width: 8,
					cell_height: 16,
				};
				let bytes = match event {
					Event::PtyWrite(text) => text.into_bytes(),

					ref query @ (Event::ColorRequest(..) | Event::TextAreaSizeRequest(..)) => {
						query_reply(query, size).unwrap_or_default()
					}
					_ => return,
				};
				self.0.lock().expect("reply lock").extend_from_slice(&bytes);
			}
		}

		fn drive(stream: &[u8]) {
			let replies = Replies::default();
			let config = Config {
				scrolling_history: HISTORY,
				..Config::default()
			};
			let dims = TermDimensions {
				columns: COLS,
				screen_lines: LINES,
			};
			let mut term = Term::new(config, &dims, replies.clone());
			term.set_scroll_ledger_rows(crate::scroll::SLIDE_ROWS);
			let mut parser = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();

			parser.advance(&mut term, format!("\x1b]0;{MARKER}\x07").as_bytes());
			parser.advance(&mut term, format!("\x1b]1;{MARKER}\x07").as_bytes());
			parser.advance(&mut term, format!("\x1b]52;c;{MARKER}\x07").as_bytes());
			parser.advance(&mut term, stream);
			parser.advance(&mut term, fuzz::VT_PROBES);

			let said = replies.0.lock().expect("reply lock").clone();
			let shown = String::from_utf8_lossy(&said);
			// A reply the program can steer is a command the user never typed.
			assert!(
				!shown.contains(MARKER),
				"a reply carried the program's own text: {shown:?}"
			);
			// And a reply that can carry a line ending submits itself.
			assert!(
				!said.iter().any(|&b| matches!(b, b'\n' | b'\r' | 0)),
				"a reply carried a line ending: {shown:?}"
			);
			// Nothing we answer is long. An unbounded one would be a way to flood
			// the shell's input as well as a way to hide something in it.
			assert!(said.len() < 4096, "a reply ran to {} bytes", said.len());

			let grid = term.grid();
			let cursor = grid.cursor.point;
			assert!(
				cursor.line.0 >= 0 && (cursor.line.0 as usize) < LINES,
				"cursor left the screen: {cursor:?}"
			);
			assert!(cursor.column.0 < COLS, "cursor left the screen: {cursor:?}");
			assert!(
				grid.display_offset() <= grid.history_size(),
				"the view sits past the scrollback"
			);
			// The scrollback is what a burst of output is allowed to cost. Without
			// a ceiling on it a program prints until the process is killed.
			assert!(
				grid.history_size() <= HISTORY,
				"scrollback grew to {} past its {HISTORY} limit",
				grid.history_size()
			);
		}

		// Test ID: EpQN0oS
		#[test]
		fn a_program_cannot_make_the_terminal_type() {
			let corpus = fuzz::corpus("vt");
			for case in &corpus {
				drive(case);
			}
			fuzz::soak("vt", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				let stream = fuzz::input(&mut rng, &corpus, fuzz::vt_stream);
				drive(&stream);
			});
		}
	}
}
