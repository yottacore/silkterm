// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
#![cfg_attr(
	all(target_os = "windows", not(debug_assertions)),
	windows_subsystem = "windows"
)]
mod app;
mod autotheme;
mod bc1;
mod bgimage;
// Compiled by build.rs, which include!s it to bake the build number in. Pulled
// into the crate only so its tests run with everything else.
#[cfg(test)]
mod buildnum;
mod cli;
mod clipboard;
mod coloremoji;
mod config;
mod contrast;
mod ctl;
mod cwd;
mod dialog;
mod fileassoc;
// Adversarial input generators, shared by the fuzz targets that sit beside the
// code they hammer. Test-only, so nothing of it reaches a shipped binary.
#[cfg(test)]
mod fuzz;
mod gfx;
mod input;
mod integration;
mod keys;
mod links;
mod locks;
#[cfg(any(test, target_os = "macos"))]
mod macmenu;
mod memdbg;
mod minimap;
mod monitor;
mod palette;
mod pane;
mod perf;
mod pick;
mod profile;
mod scrim;
mod scroll;
mod settings_ui;
mod shapes;
mod shells;
mod sysfont;
mod tabtitle;
mod term;
// One temp folder per test run, shared by every test that writes files.
// Test-only.
#[cfg(test)]
mod testdir;
mod text;
mod textedit;
mod theme;
mod tip;
mod ui_spec;
mod visibility;
mod wallpaper;
mod wpcache;
mod xmp;
use crate::app::App;
use crate::term::UserEvent;
#[cfg(feature = "profiling")]
use anyhow::Context;
use winit::event_loop::{ControlFlow, EventLoop};
// Make stdout/stderr reach the terminal we were launched from.
//
// A Windows release build is GUI-subsystem (see the attribute at the top of this
// file), so the loader gives it no console and a plain println! from a CLI-only
// flag goes NOWHERE - measured: run from a real console, the output simply never
// appears, while the same command through a pipe works, which is what makes this
// so easy to miss. Joining the parent's console fixes it.
//
// Called only on the paths that print and exit. NOT on the normal launch path: a
// terminal window that owns a console would die with the shell that started it.
// Everywhere else this is a no-op (already have one, or nothing to join).
fn open_console() {
	#[cfg(windows)]
	// SAFETY: a plain Win32 call taking a constant; failure is reported by the
	// return value, which we have nothing useful to do about.
	unsafe {
		windows_sys::Win32::System::Console::AttachConsole(
			windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
		);
	}
}
fn cli_only(cli: &cli::Cli) -> bool {
	cli.info.is_some()
}
fn control(cli: &cli::Cli) -> bool {
	cli.reload || cli.wallpaper.is_some()
}
// Every path that prints and exits needs the console, the control commands
// included - on Windows their errors went nowhere.
fn prints_and_exits(cli: &cli::Cli) -> bool {
	cli_only(cli) || control(cli)
}
fn main() -> anyhow::Result<()> {
	config::mark_launch();
	env_logger::init();
	alacritty_terminal::tty::setup_env();
	// Drop the launching shell's private variables before anything can spawn
	// a shell of its own (see term.rs SHELL_PRIVATE_ENV). Here because an
	// environment write needs the process still single-threaded.
	term::sanitize_shell_env();
	config::take_handed_down_dir();
	app::tune_heap();
	let mut cli = match cli::parse(std::env::args().skip(1)) {
		Ok(parsed) => parsed,
		Err(e) => {
			open_console();
			eprintln!("{}: {e}\nTry --help.", config::APP_NAME);
			std::process::exit(2);
		}
	};
	if prints_and_exits(&cli) {
		open_console();
	}
	// CLI-only flags: print and exit, before anything reads a config or opens a
	// window. All but --version are padded with a blank line either side so the
	// block stands clear of the prompts above and below it; --version stays flush
	// because its job is to be captured.
	if let Some(info) = cli.info {
		match info {
			cli::Info::Help => print!(
				"{}",
				cli::padded(&format!("{}\n\n{}", cli::version_line(), cli::usage()))
			),
			cli::Info::Syntax => print!("{}", cli::padded(cli::usage())),
			cli::Info::About => print!(
				"{}",
				cli::padded(&cli::about(gfx::probe_adapter_info().as_ref()))
			),
			cli::Info::Donate => print!("{}", cli::padded(&cli::donate())),
			cli::Info::Version => println!("{}", cli::version_line()),
		}
		return Ok(());
	}
	// Control commands: talk to the already-running window this shell lives in
	// (via SILKTERM_SOCKET), then exit - nothing here launches a window. Reload
	// first so --reload-settings --wallpaper x ends with x applied.
	if control(&cli) {
		let mut cmds: Vec<String> = Vec::new();
		if cli.reload {
			cmds.push("reload".into());
		}
		if let Some(img) = &cli.wallpaper {
			cmds.push(match img {
				// resolve against this shell's cwd; the window's cwd differs
				Some(p) => match std::fs::canonicalize(p) {
					Ok(abs) => format!("wallpaper\t{}", abs.display()),
					Err(e) => {
						eprintln!("{}: --wallpaper {p}: {e}", config::APP_NAME);
						std::process::exit(2);
					}
				},
				None => "wallpaper".into(),
			});
		}
		for cmd in &cmds {
			if let Err(e) = ctl::send(cmd) {
				eprintln!("{}: {e}", config::APP_NAME);
				std::process::exit(2);
			}
		}
		return Ok(());
	}
	if let Some(path) = &cli.config {
		config::set_config_override(path.clone());
	}
	// Start over from the shipped defaults: move the current config aside before
	// anything reads it, so the load below writes a fresh one. Runs after --config
	// so the two combine (reset THAT file).
	if cli.reset_config {
		match config::reset_config() {
			Some(backup) => println!(
				"{}: previous config saved as {}",
				config::APP_NAME,
				backup.display()
			),
			None => println!("{}: no config to reset", config::APP_NAME),
		}
	}
	// Launched with no layout arguments? Fall back to a config-defined command
	// line (real CLI arguments override it entirely). A bare --config still takes
	// the fallback - it picks WHICH config, so that config's command_line applies.
	if cli::only_config_args(std::env::args().skip(1)) {
		let command_line = config::settings().command_line.clone();
		if !command_line.trim().is_empty() {
			match cli::shell_split(&command_line).and_then(cli::parse) {
				Ok(parsed) => cli = parsed,
				Err(e) => eprintln!("{}: config command_line: {e}", config::APP_NAME),
			}
		}
	}
	let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
	event_loop.set_control_flow(ControlFlow::Wait);
	// An environment write, so before the first thread (see gfx.rs).
	#[cfg(target_os = "linux")]
	gfx::keep_software_off_shared_pixmaps(winit::platform::x11::EventLoopExtX11::is_x11(
		&event_loop,
	));
	let proxy = event_loop.create_proxy();
	// control socket up before any PTY spawns, so shells inherit SILKTERM_SOCKET
	let _ctl = ctl::serve(proxy.clone());
	// Read what the machine is on a worker: the profile needs it, nothing before
	// the first frame does, and a /proc read has no business on that path. After
	// the environment writes above.
	profile::probe_machine();
	let mut app = App::new(proxy, cli);
	// cicd profiler stage: SILK_PROFILE_OUT set -> sample this run and write a
	// flamegraph SVG when the app exits (App exits itself after SILK_PROFILE_SECS).
	#[cfg(feature = "profiling")]
	let profile_guard = std::env::var("SILK_PROFILE_OUT")
		.ok()
		.map(|out| {
			pprof::ProfilerGuardBuilder::default()
				.frequency(199)
				.blocklist(&["libc", "libpthread", "vdso", "libgcc"])
				.build()
				.map(|guard| (guard, out))
		})
		.transpose()
		.context("pprof: failed to start profiler")?;
	event_loop.run_app(&mut app)?;
	#[cfg(feature = "profiling")]
	if let Some((guard, out)) = profile_guard {
		let report = guard
			.report()
			.build()
			.context("pprof: failed to build report")?;
		let file = std::fs::File::create(&out).context("pprof: failed to create SVG")?;
		report
			.flamegraph(file)
			.context("pprof: failed to write flamegraph")?;
		eprintln!("{}: wrote flamegraph -> {out}", config::APP_NAME);
	}
	Ok(())
}
#[cfg(test)]
mod tests {
	use super::*;
	fn parsed(args: &[&str]) -> cli::Cli {
		cli::parse(args.iter().map(ToString::to_string)).unwrap()
	}
	// A Windows release build owns no console, so a control command that failed
	// said nothing at all.
	// Test ID: EqH4ist
	#[test]
	fn a_control_command_joins_the_console_it_was_typed_at() {
		for args in [
			&["--reload-settings"][..],
			&["--wallpaper", "x.png"],
			&["--wallpaper"],
			&["--version"],
			&["--help"],
		] {
			assert!(prints_and_exits(&parsed(args)), "{args:?}");
		}
		// a window launch must not own a console, or it dies with that shell
		for args in [&[][..], &["--rows", "30"], &["--reset-config"]] {
			assert!(!prints_and_exits(&parsed(args)), "{args:?}");
		}
	}
}
