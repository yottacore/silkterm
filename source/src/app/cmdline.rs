// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The tabs and panes a command line asks for, and what a reload or a new
//! window keeps from it.

use crate::config;
use crate::pane::{Dir, PaneManager, Rect};
use crate::term::{PaneId, UserEvent};
use crate::text::TextCtx;
use std::collections::HashMap;
use std::path::PathBuf;
use winit::event_loop::EventLoopProxy;

// A pane's shell, most specific first: its own --shell, the pane it splits, its
// tab's, the window's, then the default. The first pane of a tab has no split
// source, and a window with no tabs given has only the last 2.
fn pane_shell(explicit: Option<&[String]>, split_source: Option<&[String]>, tab: Option<&[String]>, window: Option<&[String]>, default: impl FnOnce() -> Option<Vec<String>>) -> Option<Vec<String>> {
	explicit.or(split_source).or(tab).or(window).map(<[String]>::to_vec).or_else(default)
}

// What a pane inherits from a place on the command line: its shell, unless
// that is a file `--open` runs, which only the pane it was given to runs.
fn handed_on(style: &crate::cli::Style) -> Option<&[String]> {
	style.shell.as_deref().filter(|_| !style.opened_file)
}

// The shell of a launch with no tab or pane flags, and whether it is a file
// `--open` runs.
fn plain_shell(cli: &crate::cli::Cli, default: impl FnOnce() -> Option<Vec<String>>) -> (Option<Vec<String>>, bool) {
	let window = &cli.win.style;
	let shell = pane_shell(None, None, None, window.shell.as_deref(), default);
	(shell, window.opened_file)
}

// A tab's first pane's shell, and whether it is a file `--open` runs. The
// parser gives the first pane's options to the tab, so both count as its own.
fn main_shell(tab: &crate::cli::TabSpec, window: &crate::cli::Style, default: impl FnOnce() -> Option<Vec<String>>) -> (Option<Vec<String>>, bool) {
	let own = &tab.panes[0].style;
	let shell = pane_shell(own.shell.as_deref(), None, tab.style.shell.as_deref(), handed_on(window), default);
	let opened = own.opened_file || (own.shell.is_none() && tab.style.opened_file);
	(shell, opened)
}

// A new pane's direction: its own, else the one the pane it splits was given,
// which passes down the chain. None leaves it to `default_dir`.
fn pane_split_dir(explicit: Option<crate::cli::Dir4>, split_source: Option<crate::cli::Dir4>) -> Option<crate::cli::Dir4> {
	explicit.or(split_source)
}

/// The first tabs and panes, from the parsed command line. With no tab or pane
/// flags it's one tab with one pane, which runs the window's --shell if given.
pub(super) fn build_layout(cli: &crate::cli::Cli, text: &mut TextCtx, proxy: &EventLoopProxy<UserEvent>, area: Rect) -> Vec<PaneManager> {
	use crate::cli::Size;
	// None when a shell started the program, so the pane keeps the shell's
	// directory. `config::startup_dir` has the rules.
	let start = config::startup_dir();
	// `--directory` is resolved once per place it was written, not once per pane,
	// so a path that isn't there is reported once however many panes inherit it.
	let win_dir = cli.win.style.directory.as_deref().and_then(config::cli_dir);
	// a shell that won't start exits like a command-line parse error, rather
	// than with a panic and a backtrace
	let spawn = |text: &mut TextCtx, shell: Option<Vec<String>>, dir: Option<PathBuf>, in_tmux: bool| {
		let dir = dir.or_else(|| start.clone());
		PaneManager::new(text, proxy, area, shell, dir, in_tmux).unwrap_or_else(|e| {
			eprintln!("{}: failed to start shell: {e}", config::APP_NAME);
			std::process::exit(2);
		})
	};
	// --keep-open cascades like the shell and the directory do
	let hold = |pm: &mut PaneManager, id: PaneId, keep: bool| {
		if let Some(p) = pm.panes.get_mut(&id) {
			p.keep_open = keep;
		}
	};
	let mark_opened = |pm: &mut PaneManager, id: PaneId, opened: bool| {
		if let Some(p) = pm.panes.get_mut(&id) {
			p.opened_file = opened;
		}
	};
	if !cli.hierarchical() {
		let (shell, opened) = plain_shell(cli, config::default_shell_argv);
		// a plain launch starts its pane as a new tab would, in tmux when the
		// setting is on; a layout the command line asks for stays as written
		let in_tmux = config::settings().tmux_new_session;
		let mut pm = spawn(text, shell, win_dir, in_tmux);
		let id = pm.focused;
		hold(&mut pm, id, cli.win.style.keep_open.unwrap_or(false));
		mark_opened(&mut pm, id, opened);
		return vec![pm];
	}
	let mut out = Vec::new();
	for tab in cli.tab_order(config::settings().new_tab_beside).into_iter().map(|index| &cli.tabs[index]) {
		// main pane's shell cascades pane -> tab -> window
		let (main_shell, main_opened) = main_shell(tab, &cli.win.style, config::default_shell_argv);
		// directories cascade the same way the shells do
		let tab_dir = tab.style.directory.as_deref().and_then(config::cli_dir).or_else(|| win_dir.clone());
		let main_dir = tab.panes[0].style.directory.as_deref().and_then(config::cli_dir).or_else(|| tab_dir.clone());
		let main_keep = tab.panes[0].style.keep_open.or(tab.style.keep_open).or(cli.win.style.keep_open).unwrap_or(false);
		let mut pm = spawn(text, main_shell.clone(), main_dir.clone(), false);
		let main_id = pm.focused;
		hold(&mut pm, main_id, main_keep);
		mark_opened(&mut pm, main_id, main_opened);
		let mut handles: HashMap<String, PaneId> = HashMap::new();
		handles.insert("main".into(), main_id);
		handles.insert("0".into(), main_id);
		if let Some(handle) = &tab.panes[0].id {
			handles.insert(handle.clone(), main_id);
		}
		let mut shells: HashMap<PaneId, Option<Vec<String>>> = HashMap::new();
		shells.insert(main_id, crate::pane::passed_on(main_shell.as_deref(), main_opened));
		let mut dirs: HashMap<PaneId, Option<PathBuf>> = HashMap::new();
		dirs.insert(main_id, main_dir);
		let mut keeps: HashMap<PaneId, bool> = HashMap::new();
		keeps.insert(main_id, main_keep);
		// only a direction somebody gave, or inherited from one; the tab's first
		// pane has none
		let mut split_dirs: HashMap<PaneId, crate::cli::Dir4> = HashMap::new();
		let mut prev = main_id;

		for pane_spec in &tab.panes[1..] {
			let target = pane_spec.splits.as_deref().and_then(|handle| handles.get(handle).copied()).unwrap_or(prev);
			let given_dir = pane_split_dir(pane_spec.dir, split_dirs.get(&target).copied());
			let dir4 = given_dir.unwrap_or_else(|| default_dir(&pm, target));
			let (dir, before) = match dir4 {
				crate::cli::Dir4::Down => (Dir::Horizontal, false),
				crate::cli::Dir4::Up => (Dir::Horizontal, true),
				crate::cli::Dir4::Right => (Dir::Vertical, false),
				crate::cli::Dir4::Left => (Dir::Vertical, true),
			};
			// new pane's shell: explicit -> the pane it splits -> tab -> window
			let shell = pane_shell(pane_spec.style.shell.as_deref(), shells.get(&target).and_then(Option::as_deref), handed_on(&tab.style), handed_on(&cli.win.style), config::default_shell_argv);
			let opened = pane_spec.style.opened_file;
			// and its directory: explicit -> the pane it splits -> tab -> window
			let pane_dir = pane_spec.style.directory.as_deref().and_then(config::cli_dir).or_else(|| dirs.get(&target).cloned().flatten()).or_else(|| tab_dir.clone());
			// and whether it is held open: explicit -> the pane it splits -> tab -> window
			let keep = pane_spec.style.keep_open.or_else(|| keeps.get(&target).copied()).or(tab.style.keep_open).or(cli.win.style.keep_open).unwrap_or(false);
			// no size evens out a run of same-direction splits, as a split from
			// the keyboard does
			let ratio = pane_spec.size.map(|size| match size {
				Size::Percent(pct) => pct / 100.0,
				Size::Cells(n) => {
					let rect = pm.panes.get(&target).map_or(area, |p| p.rect);
					let denom = match dir {
						Dir::Vertical => (rect.w / text.cell_w).max(1.0),
						Dir::Horizontal => (rect.h / text.cell_h).max(1.0),
					};
					n as f32 / denom
				}
			});
			if let Some(new_id) = pm.split_at(text, proxy, target, dir, before, ratio, shell.clone(), pane_dir.clone().or_else(|| start.clone()), area, false) {
				if let Some(handle) = &pane_spec.id {
					handles.insert(handle.clone(), new_id);
				}
				shells.insert(new_id, crate::pane::passed_on(shell.as_deref(), opened));
				dirs.insert(new_id, pane_dir);
				keeps.insert(new_id, keep);
				if let Some(given) = given_dir {
					split_dirs.insert(new_id, given);
				}
				hold(&mut pm, new_id, keep);
				mark_opened(&mut pm, new_id, opened);
				prev = new_id;
			}
		}
		// focus the tab's first pane, not the last split
		pm.focused = main_id;
		pm.title_override.clone_from(&tab.title);
		out.push(pm);
	}
	out
}

/// A reload rereads the file, and the file never had what the command line gave
/// at launch, so that goes back on first. The session's own state goes on after,
/// which lets a wallpaper set through the socket since launch beat the one the
/// launch named.
pub(super) fn settings_after_reload(live: &config::Settings, mut from_disk: config::Settings, launch: &crate::cli::Style, wallpaper_locked: bool) -> config::Settings {
	crate::cli::fold_window_style(&mut from_disk, launch);
	config::keep_session(live, &mut from_disk, wallpaper_locked);
	from_disk
}

/// What Ctrl+Shift+N starts. The settings file comes along, made absolute since
/// the child runs somewhere else, and so does the pane's directory, flagged so
/// the child keeps it even when it is home or a root (`config::startup_dir`).
/// Passing `--config` alone leaves the file's own `command_line` in charge, as it
/// is for any launch that names only a file.
pub(super) fn new_window_command(exe: &std::path::Path, cwd: Option<&std::path::Path>, config: Option<&std::path::Path>) -> std::process::Command {
	let mut cmd = std::process::Command::new(exe);
	if let Some(file) = config {
		cmd.arg("--config").arg(std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()));
	}
	if let Some(dir) = cwd {
		cmd.current_dir(dir).env(config::ENV_DIR_HANDED_DOWN, "1");
	}
	cmd
}

// Default split direction when none is given: split along the longer axis so the
// new pane goes where there's more room.
fn default_dir(pm: &PaneManager, target: PaneId) -> crate::cli::Dir4 {
	default_dir_for(pm.panes.get(&target).map(|p| p.rect))
}
fn default_dir_for(rect: Option<Rect>) -> crate::cli::Dir4 {
	match rect {
		Some(rect) if rect.h > rect.w => crate::cli::Dir4::Down,
		_ => crate::cli::Dir4::Right,
	}
}

#[cfg(test)]
mod tests {
	use super::{default_dir_for, handed_on, main_shell, new_window_command, pane_shell, pane_split_dir, plain_shell, settings_after_reload};
	use crate::config;
	use crate::pane::Rect;
	// Test ID: dd5py
	#[test]
	fn a_split_with_no_direction_goes_along_the_longer_side() {
		use crate::cli::Dir4;
		let rect = |w, h| Some(Rect { x: 0.0, y: 0.0, w, h });
		assert_eq!(default_dir_for(rect(400.0, 900.0)), Dir4::Down);
		assert_eq!(default_dir_for(rect(900.0, 400.0)), Dir4::Right);
		assert_eq!(default_dir_for(rect(500.0, 500.0)), Dir4::Right);
		assert_eq!(default_dir_for(None), Dir4::Right);
	}

	// --new-pane=a --down --new-pane --splits=a stacks the second pane below too.
	// Test ID: dd7z7
	#[test]
	fn a_pane_splits_the_way_the_pane_it_splits_was_split() {
		use crate::cli::Dir4;
		assert_eq!(pane_split_dir(None, Some(Dir4::Down)), Some(Dir4::Down));
		assert_eq!(pane_split_dir(Some(Dir4::Left), Some(Dir4::Down)), Some(Dir4::Left));
		assert_eq!(pane_split_dir(None, None), None, "left to the longer side");
	}

	// Test ID: dd5q0
	#[test]
	fn a_pane_takes_the_most_specific_shell_it_was_given() {
		let argv = |s: &str| vec![s.to_string()];
		let (pane, source, tab, window) = (argv("pane"), argv("source"), argv("tab"), argv("window"));
		let default = || Some(argv("default"));
		let pick = |p, s, t, w| pane_shell(p, s, t, w, default).unwrap()[0].clone();
		assert_eq!(pick(Some(&pane), Some(&source), Some(&tab), Some(&window)), "pane");
		assert_eq!(pick(None, Some(&source), Some(&tab), Some(&window)), "source");
		assert_eq!(pick(None, None, Some(&tab), Some(&window)), "tab");
		assert_eq!(pick(None, None, None, Some(&window)), "window");
		assert_eq!(pick(None, None, None, None), "default");
	}

	// Ctrl+Shift+N used to start the program with nothing but a working
	// directory, so a window opened with --config got the default file, and a
	// pane sitting in home looked like a desktop launch and took the setting.
	// Test ID: dcq7d
	#[test]
	fn a_new_window_keeps_the_settings_file_and_the_panes_directory() {
		let exe = std::path::Path::new("/opt/silkterm/silkterm");
		let home = config::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
		let args = |cmd: &std::process::Command| -> Vec<String> { cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect() };
		let handed_down = |cmd: &std::process::Command| cmd.get_envs().any(|(name, value)| name == config::ENV_DIR_HANDED_DOWN && value.is_some());

		// built from home so it is absolute on every platform; "/x/alt.shcl"
		// isn't on Windows, where it came back as C:\x\alt.shcl
		let alt = home.join("alt.shcl");
		let cmd = new_window_command(exe, Some(&home), Some(&alt));
		assert_eq!(args(&cmd), ["--config", &*alt.to_string_lossy()]);
		assert_eq!(cmd.get_current_dir(), Some(home.as_path()));
		assert!(handed_down(&cmd), "home has to be kept, not read as a launcher's");

		// a relative --config is made absolute, since the child starts elsewhere
		let cmd = new_window_command(exe, Some(&home), Some(std::path::Path::new("alt.shcl")));
		assert!(std::path::Path::new(&args(&cmd)[1]).is_absolute());

		// no --config at launch, none passed on, so the default file is used
		let cmd = new_window_command(exe, Some(&home), None);
		assert!(args(&cmd).is_empty());

		// no known directory: nothing is handed down, and the setting decides
		let cmd = new_window_command(exe, None, None);
		assert_eq!(cmd.get_current_dir(), None);
		assert!(!handed_down(&cmd));
	}

	// Reload config dropped the font and colors given on the command line, while
	// a value the command line didn't name has to come from the file.
	// Test ID: dcq7e
	#[test]
	#[cfg_attr(miri, ignore = "too slow under Miri")]
	fn a_reload_keeps_the_launch_options_over_the_file() {
		let launch = crate::cli::Style { font_size: Some(21.0), bg_color: Some([0xff, 0, 0]), wallpaper_img: Some(Some("/launch.png".into())), ..crate::cli::Style::default() };
		// the socket changed the wallpaper since launch
		let mut live = config::Settings::default();
		config::name_wallpaper(&mut live, Some("/socket.png".into()));
		let from_disk = {
			use crate::ui_spec::Key;
			use knobs::Value;
			crate::fields::owning(config::Settings::default(), &[(Key::SystemFontSize, Value::Bool(false)), (Key::FontSize, Value::Float(9.0)), (Key::Theme, Value::Text(knobs::CUSTOM.into())), (Key::ColBg, Value::Text("#000000".into())), (Key::ColFg, Value::Text("#010203".into())), (Key::ColFromWallpaper, Value::Bool(false)), (Key::BgEnabled, Value::Bool(false))])
		};
		let reloaded = settings_after_reload(&live, from_disk, &launch, true);
		assert_eq!(reloaded.font_size, 21.0);
		assert_eq!(reloaded.bg, [0xff, 0, 0]);
		assert_eq!(reloaded.fg, [1, 2, 3], "not on the command line, so the file's");
		assert_eq!(reloaded.wallpaper.as_deref(), Some(std::path::Path::new("/socket.png")));
		assert!(reloaded.wallpaper_enabled, "a named wallpaper stays switched on");
	}

	// A double-clicked script is its pane's command. A split or new tab from
	// that pane starts the default shell, rather than run the script again.
	// Test ID: dfjtj
	#[test]
	fn a_split_or_new_tab_does_not_run_an_opened_file_again() {
		use crate::pane::passed_on;
		let parse = |s: &[&str]| crate::cli::parse(s.iter().map(ToString::to_string)).unwrap();
		let never = || -> Option<Vec<String>> { panic!("every pane here was given a shell") };
		let fish = Some(vec!["fish".to_string()]);

		let c = parse(&["--keep-open", "--open", "/s/go.bat"]);
		let (shell, opened) = plain_shell(&c, never);
		assert_eq!(shell, c.win.style.shell, "the first pane runs the file");
		assert_eq!(passed_on(shell.as_deref(), opened), None);

		let c = parse(&["--shell", "fish"]);
		let (shell, opened) = plain_shell(&c, never);
		assert_eq!(passed_on(shell.as_deref(), opened), fish);

		// on a tab of its own, beside a tab that takes the window's shell
		let c = parse(&["--shell", "fish", "--new-tab", "--open", "/s/go.bat"]);
		let (shell, opened) = main_shell(&c.tabs[1], &c.win.style, never);
		assert_eq!(shell, c.tabs[1].style.shell, "the tab's pane runs the file");
		assert_eq!(passed_on(shell.as_deref(), opened), None);
		let (shell, opened) = main_shell(&c.tabs[0], &c.win.style, never);
		assert_eq!(passed_on(shell.as_deref(), opened), fish);
		assert_eq!(handed_on(&c.tabs[1].style), None, "nor to its other panes");
		assert_eq!(handed_on(&c.win.style).map(<[String]>::to_vec), fish);
	}
}
