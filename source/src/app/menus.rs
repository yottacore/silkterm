// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The menus: every menu's rows, the popup that shows them, and what a pick
//! does.

#[cfg(any(test, target_os = "macos"))]
use super::MENU_BAR;
use super::{
	CloseScope, MENU_TIP_GAP, MENU_TIP_MAX_W, MENU_TIP_PAD, State, close_scope, open_link,
	save_live, shell_argv,
};
use crate::config;
use crate::input::Hotkey;
use crate::pane::{CopyKind, Dir, Rect};
use crate::settings_ui::EditCmd;
use crate::shells::ShellEntry;
use crate::term::{PaneId, UserEvent};
use winit::event_loop::EventLoopProxy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuAction {
	OpenLink,
	CopyLink,
	Copy,
	Paste,
	PasteSelection,
	// a row of a tab rename's own right-click menu
	Edit(EditCmd),
	ToggleReadOnly,
	ToggleCopySelect,
	ToggleCopyOutput,
	NewTab,
	// a row on the macOS menu bar only; elsewhere it is Ctrl+Shift+N alone
	#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
	NewWindow,
	// the macOS Window menu's tab rows; elsewhere these are keys alone
	#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
	PrevTab,
	#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
	NextTab,
	// New tab running the shell at this index in the stored list (config
	// `shells.*`; see the Tabs menu's "New tab with shell").
	NewTabShell(usize),
	CloseTab,
	SplitVertical,
	SplitHorizontal,
	// Split running the shell at this index in the stored list, like NewTabShell.
	SplitShell(Dir, usize),
	Close,
	FontBigger,
	FontSmaller,
	FontReset,
	ToggleFullscreen,
	ToggleFrame,
	ToggleMenuBar,
	ToggleSingleTab,
	ToggleMinimap,
	ToggleBare,
	ToggleRemote,
	NextWallpaper,
	ReloadConfig,
	Settings,
	About,
	Quit,
}

impl MenuAction {
	/// The flyover for a row that needs one. Most do not: "Copy" and "New tab"
	/// say what they do, and a tip on every row would be noise the reader has to
	/// learn to ignore. Empty means no tip.
	pub(crate) fn help(self) -> &'static str {
		match self {
			MenuAction::PasteSelection => {
				"Paste what was last highlighted with the mouse, without it having been copied first."
			}
			MenuAction::ToggleCopySelect => {
				"Send highlighted text straight to the clipboard, with no copy step. Per pane."
			}
			MenuAction::ToggleCopyOutput => {
				"Copy what a command printed, once the pane settles back at the prompt. Per pane."
			}
			MenuAction::ToggleReadOnly => {
				"Ignore anything typed at this pane, so a long job cannot be interrupted by accident."
			}
			MenuAction::ToggleFrame => {
				"The title bar and border. Turning it off takes the window manager's own buttons with it."
			}
			MenuAction::ToggleMenuBar => {
				"The menu bar. Right-clicking a pane still reaches the same items with it off."
			}
			MenuAction::ToggleSingleTab => {
				"The tab strip. Off keeps it hidden until there is a second tab."
			}
			MenuAction::ToggleMinimap => {
				"Show a miniature of the whole scroll buffer beside the text. It takes the room it uses."
			}
			MenuAction::ToggleBare => {
				"Drop the title bar, menu bar and tab strip together. Choosing it again puts back whatever was on."
			}
			MenuAction::ToggleRemote => {
				"Run as a plain terminal while the screen is somewhere else, since no effect survives the trip. Set for you when a remote session is noticed, and forgotten at the next launch."
			}
			MenuAction::NextWallpaper => {
				"Show the next picture from the wallpaper folder now. The rotation timer starts over from it."
			}
			MenuAction::ReloadConfig => {
				"Re-read the config file. Anything edited by hand since launch takes effect now."
			}
			MenuAction::NewTabShell(_) => "Open a tab running this shell instead of the usual one.",
			MenuAction::SplitShell(..) => "Split this pane and run this shell in the new half.",
			MenuAction::CopyLink => {
				"Put the link's address on the clipboard rather than opening it."
			}
			_ => "",
		}
	}
}

/// One row of a menu: an action item (optionally a checkmark toggle) or a group
/// separator. Separators render as a faint horizontal line, never hover/click.
/// `accel` is the byte offset of the item's accelerator letter in the label
/// (underlined; typing it picks the item); None = no accelerator - accelerators
/// must be unique per menu, so low-priority items (and ones that already have a
/// hotkey) go without.
#[derive(Debug, Clone)]
pub(crate) enum Entry {
	Item {
		label: String,
		action: MenuAction,
		check: Option<bool>,
		accel: Option<usize>,
	},
	// A row that opens a menu of its own to the right instead of doing
	// something. It carries its own items, so the popup can be built the moment
	// the pointer reaches the row.
	Sub {
		label: String,
		accel: Option<usize>,
		items: Vec<Entry>,
	},
	Sep,
}

/// The text a row draws, if it draws any - a separator does not. Item and Sub
/// rows are laid out and measured identically, so everything that walks a menu
/// asks here rather than matching the two arms itself.
pub(super) fn entry_label(entry: &Entry) -> Option<&str> {
	match entry {
		Entry::Item { label, .. } | Entry::Sub { label, .. } => Some(label),
		Entry::Sep => None,
	}
}

/// The first accelerator letter two rows of one menu both claim, if any.
///
/// Typing a letter picks the FIRST row carrying it, so a duplicate does not read
/// as a duplicate - it silently makes the LATER row unreachable from the
/// keyboard, which is why this is asserted where a menu is built rather than
/// left to be noticed.
pub(super) fn accel_clash(entries: &[Entry]) -> Option<char> {
	let mut seen: Vec<char> = Vec::new();
	for entry in entries {
		let Some((label, pos)) = entry_accel(entry) else {
			continue;
		};
		let Some(ch) = label[pos..].chars().next().map(|c| c.to_ascii_lowercase()) else {
			continue;
		};
		if seen.contains(&ch) {
			return Some(ch);
		}
		seen.push(ch);
	}
	None
}

/// The label and the byte offset of its accelerator letter, for a row that has one.
pub(super) fn entry_accel(entry: &Entry) -> Option<(&str, usize)> {
	match entry {
		Entry::Item {
			label,
			accel: Some(pos),
			..
		}
		| Entry::Sub {
			label,
			accel: Some(pos),
			..
		} => Some((label, *pos)),
		_ => None,
	}
}

// Byte offset of the accelerator letter: exact-case match first (so 'S' can
// pick "Selection" in "Paste Selection"), else case-insensitive.
fn accel_at(label: &str, ch: char) -> Option<usize> {
	label
		.find(ch)
		.or_else(|| label.to_ascii_lowercase().find(ch.to_ascii_lowercase()))
}

fn entry_item(label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: None,
		accel: None,
	}
}
pub(super) fn entry_item_accel(ch: char, label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: None,
		accel: accel_at(label, ch),
	}
}
// `ch` is optional because accelerators have to be unique WITHIN a menu, and a
// row that appears in two of them cannot always spell it the same way.
fn entry_sub(ch: Option<char>, label: &str, items: Vec<Entry>) -> Entry {
	Entry::Sub {
		label: label.into(),
		accel: ch.and_then(|ch| accel_at(label, ch)),
		items,
	}
}
fn entry_check(on: bool, label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: Some(on),
		accel: None,
	}
}
fn entry_check_accel(ch: char, on: bool, label: &str, action: MenuAction) -> Entry {
	Entry::Item {
		label: label.into(),
		action,
		check: Some(on),
		accel: accel_at(label, ch),
	}
}

// A "... with shell" row, or nothing at all while there is no shell to put
// under it - an empty flyout is worse than no row. The stored list supplies the
// titles and the order; only the active entries are offered, and the action
// carries the index into the WHOLE list so a disabled entry between two active
// ones cannot shift what a click runs.
fn shell_submenu(
	shells: &[ShellEntry],
	accel: Option<char>,
	label: &str,
	action: fn(usize) -> MenuAction,
) -> Vec<Entry> {
	let items: Vec<Entry> = shells
		.iter()
		.enumerate()
		.filter(|(_, shell)| shell.active)
		.map(|(i, shell)| entry_item(&shell.title, action(i)))
		.collect();
	if items.is_empty() {
		Vec::new()
	} else {
		vec![entry_sub(accel, label, items)]
	}
}

// What the View menu needs to know to draw its checkmarks. Every field reads
// the same way: true means the thing is on, and its row is checked.
#[derive(Clone, Copy, Hash)]
struct ViewState {
	read_only: bool,
	fullscreen: bool,
	window_frame: bool,
	menu_bar: bool,
	tab_strip: bool,
	minimap: bool,
	bare: bool,
	remote: bool,
	// a rotation folder with something to move on to; the row is left out
	// otherwise, like the link rows on the right-click menu
	next_wallpaper: bool,
}

// The View menu, apart from the window it is asking about - so the labels, the
// order and the accelerators can be held to the style guide by test.
fn view_menu_items(on: ViewState) -> Vec<Entry> {
	let mut items = vec![
		entry_item_accel('I', "Increase font size", MenuAction::FontBigger),
		entry_item_accel('D', "Decrease font size", MenuAction::FontSmaller),
		entry_item_accel('e', "Reset font size", MenuAction::FontReset),
		Entry::Sep,
		entry_check_accel('R', on.read_only, "Read-only", MenuAction::ToggleReadOnly),
		Entry::Sep,
		entry_check_accel(
			'F',
			on.fullscreen,
			"Fullscreen",
			MenuAction::ToggleFullscreen,
		),
		// every toggle below names the thing itself and is checked while it is
		// showing, so the checkmarks all read one way down the column
		entry_check_accel(
			'W',
			on.window_frame,
			"Window frame",
			MenuAction::ToggleFrame,
		),
		entry_check_accel('M', on.menu_bar, "Menu bar", MenuAction::ToggleMenuBar),
		entry_check_accel('T', on.tab_strip, "Tab strip", MenuAction::ToggleSingleTab),
		// 'M' and 'i' are both spoken for on this menu (Menu bar, Increase font
		// size), so the accelerator falls to the n
		entry_check_accel('n', on.minimap, "Minimap", MenuAction::ToggleMinimap),
		entry_check_accel('B', on.bare, "Bare window", MenuAction::ToggleBare),
		Entry::Sep,
		entry_check_accel(
			'p',
			on.remote,
			"Temporary remote display mode",
			MenuAction::ToggleRemote,
		),
	];
	if on.next_wallpaper {
		items.extend([Entry::Sep, next_wallpaper_row()]);
	}
	items
}

// 'N' is New tab on the right-click menu and the n in Minimap on View, so both
// menus take the x
fn next_wallpaper_row() -> Entry {
	entry_item_accel('x', "Next wallpaper", MenuAction::NextWallpaper)
}

// The three shell rows: a new tab, and a split either way.
fn new_tab_shells(shells: &[ShellEntry], accel: Option<char>) -> Vec<Entry> {
	shell_submenu(shells, accel, "New tab with shell", MenuAction::NewTabShell)
}
fn split_shells(shells: &[ShellEntry]) -> Vec<Entry> {
	let mut rows = shell_submenu(shells, None, "Split vertical with shell", |i| {
		MenuAction::SplitShell(Dir::Vertical, i)
	});
	rows.extend(shell_submenu(
		shells,
		None,
		"Split horizontal with shell",
		|i| MenuAction::SplitShell(Dir::Horizontal, i),
	));
	rows
}

// The menu-bar dropdowns other than View, and the right-click menu, each apart
// from the window it opens in, so the labels and accelerators can be held to
// the style guide by test the way View's are.
//
// No tab or pane action goes on File; each has a menu of its own.
fn file_menu_items() -> Vec<Entry> {
	vec![
		entry_item_accel('R', "Reload config", MenuAction::ReloadConfig),
		entry_item_accel('S', "Settings\u{2026}", MenuAction::Settings),
		Entry::Sep,
		entry_item_accel('Q', "Quit", MenuAction::Quit),
	]
}

fn edit_menu_items(copy_select: bool, copy_output: bool) -> Vec<Entry> {
	vec![
		entry_item_accel('C', "Copy", MenuAction::Copy),
		entry_item_accel('P', "Paste", MenuAction::Paste),
		entry_item_accel('S', "Paste Selection", MenuAction::PasteSelection),
		Entry::Sep,
		entry_check(copy_select, "Copy on select", MenuAction::ToggleCopySelect),
		entry_check(copy_output, "Copy on output", MenuAction::ToggleCopyOutput),
	]
}

fn tabs_menu_items(shells: &[ShellEntry]) -> Vec<Entry> {
	let mut items = vec![entry_item_accel('N', "New tab", MenuAction::NewTab)];
	items.extend(new_tab_shells(shells, Some('S')));
	items.extend([
		Entry::Sep,
		entry_item_accel('C', "Close tab", MenuAction::CloseTab),
	]);
	items
}

fn panes_menu_items(shells: &[ShellEntry]) -> Vec<Entry> {
	let mut items = vec![
		entry_item_accel('V', "Split vertical", MenuAction::SplitVertical),
		entry_item_accel('H', "Split horizontal", MenuAction::SplitHorizontal),
	];
	items.extend(split_shells(shells));
	items.extend([
		Entry::Sep,
		entry_item_accel('C', "Close pane", MenuAction::Close),
	]);
	items
}

fn help_menu_items() -> Vec<Entry> {
	vec![entry_item_accel('A', "About\u{2026}", MenuAction::About)]
}

// Menu-bar dropdown `idx`, in MENU_BAR order.
fn bar_menu(
	idx: usize,
	view: ViewState,
	copy_select: bool,
	copy_output: bool,
	shells: &[ShellEntry],
) -> Vec<Entry> {
	match idx {
		0 => file_menu_items(),
		1 => edit_menu_items(copy_select, copy_output),
		2 => view_menu_items(view),
		3 => tabs_menu_items(shells),
		4 => panes_menu_items(shells),
		_ => help_menu_items(),
	}
}

// Every dropdown with its title, which is what the macOS menu bar is built from.
#[cfg(any(test, target_os = "macos"))]
fn window_menus(
	view: ViewState,
	copy_select: bool,
	copy_output: bool,
	shells: &[ShellEntry],
) -> Vec<(&'static str, Vec<Entry>)> {
	MENU_BAR
		.iter()
		.enumerate()
		.map(|(idx, title)| {
			(
				*title,
				bar_menu(idx, view, copy_select, copy_output, shells),
			)
		})
		.collect()
}

/// Each row that does what a hotkey does shows the chord that hotkey answers to
/// first, from the bindings in force, so a rebinding shows up here too. The
/// menus are built without them, and every place one opens comes through here.
pub(crate) fn with_shortcuts(
	entries: Vec<Entry>,
	keys: &crate::keys::Bindings,
	mac: bool,
) -> Vec<Entry> {
	entries
		.into_iter()
		.map(|entry| match entry {
			Entry::Item {
				label,
				action,
				check,
				accel,
			} => {
				let label = match menu_hotkey(action).and_then(|hotkey| keys.shown(hotkey)) {
					Some(chord) => format!("{label} ({})", chord.spoken(mac)),
					None => label,
				};
				Entry::Item {
					label,
					action,
					check,
					accel,
				}
			}
			Entry::Sub {
				label,
				accel,
				items,
			} => Entry::Sub {
				label,
				accel,
				items: with_shortcuts(items, keys, mac),
			},
			Entry::Sep => Entry::Sep,
		})
		.collect()
}

/// The hotkey a menu row does the same thing as, if there is one.
pub(crate) fn menu_hotkey(action: MenuAction) -> Option<Hotkey> {
	match action {
		MenuAction::Copy => Some(Hotkey::Copy),
		MenuAction::Paste => Some(Hotkey::Paste),
		MenuAction::NewTab => Some(Hotkey::NewTab),
		MenuAction::NewWindow => Some(Hotkey::NewWindow),
		MenuAction::PrevTab => Some(Hotkey::PrevTab),
		MenuAction::NextTab => Some(Hotkey::NextTab),
		MenuAction::CloseTab => Some(Hotkey::CloseTab),
		MenuAction::FontBigger => Some(Hotkey::Zoom(1)),
		MenuAction::FontSmaller => Some(Hotkey::Zoom(-1)),
		MenuAction::FontReset => Some(Hotkey::ZoomReset),
		MenuAction::ToggleFullscreen => Some(Hotkey::Fullscreen),
		MenuAction::Settings => Some(Hotkey::Settings),
		MenuAction::Quit => Some(Hotkey::Quit),
		MenuAction::SplitVertical => Some(Hotkey::SplitRight),
		MenuAction::SplitHorizontal => Some(Hotkey::SplitDown),
		MenuAction::Close => Some(Hotkey::ClosePane),
		MenuAction::OpenLink
		| MenuAction::CopyLink
		| MenuAction::PasteSelection
		| MenuAction::Edit(_)
		| MenuAction::ToggleReadOnly
		| MenuAction::ToggleCopySelect
		| MenuAction::ToggleCopyOutput
		| MenuAction::NewTabShell(_)
		| MenuAction::SplitShell(..)
		| MenuAction::ToggleFrame
		| MenuAction::ToggleMenuBar
		| MenuAction::ToggleSingleTab
		| MenuAction::ToggleMinimap
		| MenuAction::ToggleBare
		| MenuAction::ToggleRemote
		| MenuAction::NextWallpaper
		| MenuAction::ReloadConfig
		| MenuAction::About => None,
	}
}

/// A menu less the rows `drop` picks, inside submenus too, with no separator
/// left at either end or doubled up where a row went.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn without_rows(entries: Vec<Entry>, drop: fn(MenuAction) -> bool) -> Vec<Entry> {
	let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
	for entry in entries {
		match entry {
			Entry::Item { action, .. } if drop(action) => {}
			Entry::Sub {
				label,
				accel,
				items,
			} => out.push(Entry::Sub {
				label,
				accel,
				items: without_rows(items, drop),
			}),
			Entry::Sep if out.last().is_none_or(|last| matches!(last, Entry::Sep)) => {}
			entry => out.push(entry),
		}
	}
	if matches!(out.last(), Some(Entry::Sep)) {
		out.pop();
	}
	out
}

/// A menu as a Mac shows it: no Menu bar row, since the system menu bar is the
/// only one there.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn mac_entries(entries: Vec<Entry>) -> Vec<Entry> {
	without_rows(entries, |action| action == MenuAction::ToggleMenuBar)
}

// Every row turned on, and two shells, so each menu shows all it can.
#[cfg(test)]
pub(crate) fn sample_window_menus() -> Vec<(&'static str, Vec<Entry>)> {
	sample_window_menus_copying(true, true)
}

// The same, with the focused pane's two copy modes as given.
#[cfg(test)]
pub(crate) fn sample_window_menus_copying(
	copy_select: bool,
	copy_output: bool,
) -> Vec<(&'static str, Vec<Entry>)> {
	sample_window_menus_with(copy_select, copy_output, &["bash", "zsh"])
}

// Every row turned on, with a shell for each title given.
#[cfg(test)]
pub(crate) fn sample_window_menus_with(
	copy_select: bool,
	copy_output: bool,
	titles: &[&str],
) -> Vec<(&'static str, Vec<Entry>)> {
	let shell = |title: &str| ShellEntry {
		slug: title.into(),
		title: title.into(),
		command: title.into(),
		active: true,
		comment: String::new(),
		last_seen: String::new(),
	};
	let view = ViewState {
		read_only: true,
		fullscreen: true,
		window_frame: true,
		menu_bar: true,
		tab_strip: true,
		minimap: true,
		bare: true,
		remote: true,
		next_wallpaper: true,
	};
	let shells: Vec<ShellEntry> = titles.iter().map(|title| shell(title)).collect();
	window_menus(view, copy_select, copy_output, &shells)
}

// What the right-click menu needs to know about the pane and window it opens
// over. The checkmark fields read as ViewState's do.
#[derive(Clone, Copy)]
struct CtxState {
	// a link under the click; its two rows are left out otherwise
	link: bool,
	read_only: bool,
	copy_select: bool,
	copy_output: bool,
	menu_bar: bool,
	next_wallpaper: bool,
}

fn context_menu_items(on: CtxState, shells: &[ShellEntry]) -> Vec<Entry> {
	let mut entries = Vec::new();
	// A link under the click gets its two items at the top, and only then -
	// they'd be dead weight on every other right-click.
	if on.link {
		entries.extend([
			entry_item_accel('O', "Open link", MenuAction::OpenLink),
			entry_item_accel('L', "Copy link", MenuAction::CopyLink),
			Entry::Sep,
		]);
	}
	// no accelerator on the shell rows: this menu already spends every letter
	// their labels offer - 'S' on "Paste Selection", 'H' on "Split
	// horizontal", 'N' on "New tab" - and a duplicate would make the older
	// item unreachable, since the first match wins
	entries.extend([
		entry_item_accel('C', "Copy", MenuAction::Copy),
		entry_item_accel('P', "Paste", MenuAction::Paste),
		entry_item_accel('S', "Paste Selection", MenuAction::PasteSelection),
		Entry::Sep,
		entry_check(
			on.copy_select,
			"Copy on select",
			MenuAction::ToggleCopySelect,
		),
		entry_check(
			on.copy_output,
			"Copy on output",
			MenuAction::ToggleCopyOutput,
		),
		entry_check_accel('R', on.read_only, "Read-only", MenuAction::ToggleReadOnly),
		Entry::Sep,
		entry_item_accel('N', "New tab", MenuAction::NewTab),
	]);
	entries.extend(new_tab_shells(shells, None));
	entries.extend([
		Entry::Sep,
		entry_item_accel('V', "Split vertical", MenuAction::SplitVertical),
		entry_item_accel('H', "Split horizontal", MenuAction::SplitHorizontal),
	]);
	entries.extend(split_shells(shells));
	entries.extend([
		Entry::Sep,
		entry_item("Close pane", MenuAction::Close),
		// The one window-chrome row worth repeating here: with the bar hidden
		// this menu is the only way back to it. The rest live on View.
		Entry::Sep,
		entry_check_accel('M', on.menu_bar, "Menu bar", MenuAction::ToggleMenuBar),
	]);
	if on.next_wallpaper {
		entries.push(next_wallpaper_row());
	}
	entries.extend([
		Entry::Sep,
		entry_item("Reload config", MenuAction::ReloadConfig),
		entry_item("Settings\u{2026}", MenuAction::Settings),
	]);
	entries
}

// A popup's own DIP measurements at one scale factor: the padding above the
// first item and below the last, and the height of a separator row. Resolved
// once when the menu is built (see `popup`) so the draw and both hit tests read
// the same numbers without carrying a TextCtx into the geometry.
fn menu_metrics(scale: f32) -> (f32, f32) {
	(
		config::dip(config::MENU_ITEM_PAD_Y, scale),
		config::dip(config::MENU_SEP_H, scale),
	)
}

/// right-click context menu / menu-bar dropdown over a pane
#[derive(Debug)]
pub(super) struct ContextMenu {
	pub(super) x: f32,
	pub(super) y: f32,
	pub(super) w: f32,
	pub(super) item_h: f32,
	// this popup's `menu_metrics`, in physical px
	pad_y: f32,
	pub(super) sep_h: f32,
	pub(super) target: PaneId,
	pub(super) entries: Vec<Entry>,
	pub(super) hover: Option<usize>, // index into entries; never a separator
	// The submenu standing open off one of these rows, if any. It is placed
	// clear of this popup's right edge, so "the pointer is in the submenu" and
	// "the pointer is on a parent row" can never both be true.
	sub: Option<Box<ContextMenu>>,
}

impl ContextMenu {
	pub(super) fn height(&self) -> f32 {
		let rows: f32 = self.entries.iter().map(|entry| self.entry_h(entry)).sum();
		rows + self.pad_y * 2.0
	}
	fn entry_h(&self, entry: &Entry) -> f32 {
		match entry {
			Entry::Sep => self.sep_h,
			_ => self.item_h,
		}
	}
	/// This popup and every submenu standing open off it, outermost first.
	pub(super) fn chain(&self) -> Vec<&ContextMenu> {
		let mut out = vec![self];
		let mut at = self;
		while let Some(sub) = &at.sub {
			out.push(sub);
			at = sub;
		}
		out
	}
	// The popup the keyboard and the pointer are on: the innermost open one.
	fn inner_mut(&mut self) -> &mut ContextMenu {
		// Matching on `&mut self.sub` would keep `self` borrowed into the None
		// arm, so the Some arm borrows again.
		match self.sub {
			#[allow(clippy::expect_used, reason = "matched Some just above")]
			Some(_) => self.sub.as_mut().expect("just matched").inner_mut(),
			None => self,
		}
	}
	/// Anywhere on this popup or a submenu of it.
	pub(super) fn hit_any(&self, mx: f32, my: f32) -> bool {
		self.chain().iter().any(|popup| popup.hit(mx, my))
	}
	pub(super) fn row_top(&self, i: usize) -> f32 {
		self.y
			+ self.pad_y
			+ self.entries[..i]
				.iter()
				.map(|entry| self.entry_h(entry))
				.sum::<f32>()
	}
	// Anywhere on the popup, separators and padding included - a click that falls
	// on the menu belongs to the menu, whatever chrome it happens to cover.
	fn hit(&self, mx: f32, my: f32) -> bool {
		mx >= self.x && mx < self.x + self.w && my >= self.y && my < self.y + self.height()
	}
	fn item_at(&self, mx: f32, my: f32) -> Option<usize> {
		if mx < self.x || mx >= self.x + self.w {
			return None;
		}
		let mut y = self.y + self.pad_y;
		for (i, entry) in self.entries.iter().enumerate() {
			let h = self.entry_h(entry);
			if my >= y && my < y + h {
				return (!matches!(entry, Entry::Sep)).then_some(i);
			}
			y += h;
		}
		None
	}
	/// Next selectable item from `from` in direction `dir` (+1 down / -1 up),
	/// wrapping and skipping separators. None only if there are no items.
	pub(super) fn step(&self, from: Option<usize>, dir: i32) -> Option<usize> {
		let n = self.entries.len() as i32;
		if n == 0 {
			return None;
		}
		let mut i = from.map_or(if dir > 0 { -1 } else { 0 }, |i| i as i32);
		for _ in 0..n {
			i = (i + dir).rem_euclid(n);
			if !matches!(self.entries[i as usize], Entry::Sep) {
				return Some(i as usize);
			}
		}
		None
	}
}

impl State {
	/// The menu row a tip would describe: the innermost open popup that the
	/// pointer is actually on (a parent keeps its highlight on the row its
	/// submenu hangs off, so the deepest hovered one is the right answer), and
	/// only when that row has something to say.
	pub(super) fn menu_tip_target(&self) -> Option<(usize, usize)> {
		let root = self.menu.as_ref()?;
		let (depth, menu, row) = root
			.chain()
			.iter()
			.enumerate()
			.filter_map(|(depth, menu)| menu.hover.map(|row| (depth, *menu, row)))
			.next_back()?;
		let help = match menu.entries.get(row)? {
			Entry::Item { action, .. } => action.help(),
			_ => "",
		};
		(!help.is_empty()).then_some((depth, row))
	}

	/// The menu tip's box and its wrapped lines, once the pointer has rested on a
	/// row that has a tip. It stands beside the popup rather than under the row,
	/// so the rows being chosen between stay readable.
	pub(super) fn menu_tip_layout(&mut self) -> Option<(Rect, Vec<(f32, f32, String)>)> {
		let (depth, row) = self.menu_tip.ripe()?;
		let (anchor, help) = {
			let menu = *self.menu.as_ref()?.chain().get(depth)?;
			let help = match menu.entries.get(row)? {
				Entry::Item { action, .. } => action.help(),
				_ => "",
			};
			let anchor = Rect {
				x: menu.x,
				y: menu.row_top(row),
				w: menu.w,
				h: menu.item_h,
			};
			(anchor, help)
		};
		if help.is_empty() {
			return None;
		}
		let attrs = crate::text::ui_attrs();
		let pad = self.text.dip(MENU_TIP_PAD);
		let line_h = self.text.ui_line_h;
		let budget = self.text.dip(MENU_TIP_MAX_W);
		let lines = crate::tip::wrap(help, budget, |line| self.text.measure_ui_text(line, &attrs));
		let text_w = lines.iter().fold(0.0f32, |widest, line| {
			widest.max(self.text.measure_ui_text(line, &attrs))
		});
		let w = text_w + 2.0 * pad;
		let h = line_h * lines.len() as f32 + 2.0 * pad;
		let win = (self.surface_px.0 as f32, self.surface_px.1 as f32);
		let (x, y) = crate::tip::beside(anchor, (w, h), win, self.text.dip(MENU_TIP_GAP), pad);
		let placed = lines
			.into_iter()
			.enumerate()
			.map(|(i, line)| (x + pad, y + pad + line_h * i as f32, line))
			.collect();
		Some((Rect { x, y, w, h }, placed))
	}

	pub(super) fn open_menu(&mut self, target: PaneId, mx: f32, my: f32) {
		let p = self.tabs.cur().panes.get(&target);
		let read_only = p.is_some_and(|p| p.read_only);
		let copy_select = p.is_some_and(|p| p.copy_select);
		let copy_output = p.is_some_and(|p| p.copy_output);
		self.menu_link = self.link_at_pointer().map(|(_, link)| link.url);
		let on = CtxState {
			link: self.menu_link.is_some(),
			read_only,
			copy_select,
			copy_output,
			menu_bar: self.menu_bar,
			next_wallpaper: self.can_rotate(),
		};
		let settings = config::settings();
		let entries = with_shortcuts(
			context_menu_items(on, &settings.shells),
			&settings.keys,
			cfg!(target_os = "macos"),
		);
		#[cfg(target_os = "macos")]
		let entries = mac_entries(entries);
		self.bar_open = None;
		self.popup(target, entries, mx, my);
	}

	/// Build and place a dropdown/context popup, clamped on-screen.
	pub(super) fn popup(&mut self, target: PaneId, entries: Vec<Entry>, mx: f32, my: f32) {
		self.menu = Some(self.build_popup(target, entries, mx, my));
	}

	// Lay one popup out at (mx, my), clamped on-screen. Width is the widest
	// (proportional) label plus the checkmark gutter, the padding, and - only
	// where a row opens a submenu - the column its arrow sits in. Shared by the
	// menu bar, the right-click menu and the submenus, so all of them size and
	// clamp alike.
	fn build_popup(
		&mut self,
		target: PaneId,
		entries: Vec<Entry>,
		mx: f32,
		my: f32,
	) -> ContextMenu {
		debug_assert!(
			accel_clash(&entries).is_none(),
			"two rows of one menu claim the accelerator {:?}",
			accel_clash(&entries)
		);
		let attrs = crate::text::ui_attrs();
		let mut max_label_w: f32 = 0.0;
		let mut any_sub = false;
		for entry in &entries {
			if let Some(label) = entry_label(entry) {
				max_label_w = max_label_w.max(self.text.measure_ui_text(label, &attrs));
			}
			any_sub |= matches!(entry, Entry::Sub { .. });
		}
		let arrow_col = if any_sub {
			self.text.dip(config::MENU_SUB_ARROW)
		} else {
			0.0
		};
		let w = self.text.dip(config::MENU_GUTTER)
			+ max_label_w
			+ arrow_col
			+ self.text.dip(config::MENU_PAD_X) * 2.0;
		let item_h = self.text.ui_line_h;
		let (pad_y, sep_h) = menu_metrics(self.text.scale);
		let menu = ContextMenu {
			x: mx,
			y: my,
			w,
			item_h,
			pad_y,
			sep_h,
			target,
			entries,
			hover: None,
			sub: None,
		};
		let sw = self.surface_px.0 as f32;
		let sh = self.surface_px.1 as f32;
		let x = mx.min((sw - w).max(0.0));
		let y = my.min((sh - menu.height()).max(0.0));
		ContextMenu { x, y, ..menu }
	}

	// Open the submenu on row `row` of the open popup, or close whatever was
	// standing open if that row does not have one.
	//
	// It goes to the RIGHT of the parent, never overlapping it, with its first
	// row lined up on the parent row - which is what lets the pointer rule stay
	// as simple as it is: moving right off the row leaves the parent entirely,
	// so nothing else can claim the hover on the way in. It flips to the left
	// only when there is no room on the right.
	fn open_submenu(&mut self, row: usize) {
		let Some(menu) = self.menu.as_ref() else {
			return;
		};
		let Some(Entry::Sub { items, .. }) = menu.entries.get(row) else {
			if let Some(menu) = self.menu.as_mut() {
				menu.sub = None;
			}
			return;
		};
		let items = items.clone();
		let (px, pw, top, pad_y) = (menu.x, menu.w, menu.row_top(row), menu.pad_y);
		// measured against a provisional build, since the width is what decides
		// which side it goes on
		let mut popup = self.build_popup(menu.target, items, px + pw, top - pad_y);
		if px + pw + popup.w > self.surface_px.0 as f32 {
			popup = self.build_popup(
				popup.target,
				popup.entries,
				(px - popup.w).max(0.0),
				top - pad_y,
			);
		}
		if let Some(menu) = self.menu.as_mut() {
			menu.sub = Some(Box::new(popup));
		}
	}

	/// Point the open menu at (x, y). The innermost popup under the pointer takes
	/// the highlight, and moving onto (or off) a submenu row opens (or closes) its
	/// popup. Returns whether anything moved.
	pub(super) fn menu_hover(&mut self, x: f32, y: f32) -> bool {
		let Some(menu) = self.menu.as_mut() else {
			return false;
		};
		// A submenu takes the pointer first - it overlaps no parent row, so being
		// inside it is unambiguous, and the parent keeps its highlight on the row
		// the submenu belongs to.
		if let Some(sub) = menu.sub.as_mut() {
			if sub.hit(x, y) {
				let hovered = sub.item_at(x, y);
				let moved = hovered != sub.hover;
				sub.hover = hovered;
				return moved;
			}
		}
		let hovered = menu.item_at(x, y);
		if hovered == menu.hover {
			return false;
		}
		menu.hover = hovered;
		let row = hovered.filter(|&i| matches!(menu.entries[i], Entry::Sub { .. }));
		match row {
			Some(row) => self.open_submenu(row),
			None => {
				if let Some(menu) = self.menu.as_mut() {
					menu.sub = None;
				}
			}
		}
		true
	}

	/// Act on a click at (x, y) with a menu open: an item fires and closes the
	/// whole stack, a submenu row opens its popup and leaves everything standing,
	/// and anything else dismisses.
	pub(super) fn menu_click(&mut self, x: f32, y: f32, proxy: &EventLoopProxy<UserEvent>) {
		let Some(menu) = self.menu.as_ref() else {
			return;
		};
		let target = menu.target;
		let chain = menu.chain();
		// innermost first: a submenu is drawn over whatever it covers
		let found = chain
			.iter()
			.enumerate()
			.rev()
			.find_map(|(depth, popup)| popup.item_at(x, y).map(|row| (depth, row)));
		let Some((depth, row)) = found else {
			self.menu = None;
			self.bar_open = None;
			return;
		};
		let entry = chain[depth].entries[row].clone();
		match entry {
			// only the root popup carries submenus, so a deeper one cannot open
			Entry::Sub { .. } => {
				if depth == 0 {
					self.open_submenu(row);
				}
			}
			Entry::Item { action, .. } => {
				self.menu = None;
				self.bar_open = None;
				self.apply_menu(action, target, proxy);
			}
			Entry::Sep => {}
		}
	}

	/// Fire row `row` of the innermost open popup, the way Enter and an
	/// accelerator letter do. A submenu row opens and takes the highlight to its
	/// first item instead of acting.
	pub(super) fn menu_activate(&mut self, row: usize, proxy: &EventLoopProxy<UserEvent>) {
		let Some(menu) = self.menu.as_ref() else {
			return;
		};
		let target = menu.target;
		let chain = menu.chain();
		let depth = chain.len() - 1;
		let Some(entry) = chain[depth].entries.get(row).cloned() else {
			return;
		};
		match entry {
			Entry::Sub { .. } if depth == 0 => {
				self.open_submenu(row);
				if let Some(sub) = self.menu.as_mut().and_then(|menu| menu.sub.as_mut()) {
					sub.hover = sub.step(None, 1);
				}
			}
			Entry::Item { action, .. } => {
				self.menu = None;
				self.bar_open = None;
				self.apply_menu(action, target, proxy);
			}
			_ => {}
		}
	}

	/// The popup the keyboard is on: the innermost one standing open.
	pub(super) fn menu_inner(&mut self) -> Option<&mut ContextMenu> {
		self.menu.as_mut().map(ContextMenu::inner_mut)
	}

	/// The highlighted row of the open popup when it is one that opens a submenu
	/// and has not opened it yet - i.e. what Right arrow would enter.
	pub(super) fn submenu_row(&self) -> Option<usize> {
		let menu = self.menu.as_ref()?;
		if menu.sub.is_some() {
			return None;
		}
		menu.hover
			.filter(|&row| matches!(menu.entries[row], Entry::Sub { .. }))
	}

	/// Close the open submenu; returns false when there was none, so the caller
	/// can fall through to whatever it does otherwise.
	pub(super) fn close_submenu(&mut self) -> bool {
		let Some(menu) = self.menu.as_mut() else {
			return false;
		};
		if menu.sub.is_none() {
			return false;
		}
		menu.sub = None;
		true
	}

	// What the menu-bar dropdowns show: the View checkmarks, and the focused
	// pane's two copy modes.
	fn bar_state(&self) -> (ViewState, bool, bool) {
		let p = self.tabs.cur().panes.get(&self.tabs.cur().focused);
		let settings = config::settings();
		let view = ViewState {
			read_only: p.is_some_and(|p| p.read_only),
			fullscreen: self.window.fullscreen().is_some(),
			window_frame: self.decorated,
			menu_bar: self.menu_bar,
			tab_strip: !settings.hide_single_tab,
			minimap: settings.minimap,
			bare: self.bare,
			remote: settings.remote_override,
			next_wallpaper: self.can_rotate(),
		};
		(
			view,
			p.is_some_and(|p| p.copy_select),
			p.is_some_and(|p| p.copy_output),
		)
	}

	// The dropdown entries for top-level menu-bar entry `idx` (File/Edit/...).
	fn bar_menu_items(&self, idx: usize) -> Vec<Entry> {
		let (view, copy_select, copy_output) = self.bar_state();
		let settings = config::settings();
		with_shortcuts(
			bar_menu(idx, view, copy_select, copy_output, &settings.shells),
			&settings.keys,
			cfg!(target_os = "macos"),
		)
	}

	#[cfg(target_os = "macos")]
	pub(super) fn bar_menus(&self) -> Vec<(&'static str, Vec<Entry>)> {
		let (view, copy_select, copy_output) = self.bar_state();
		window_menus(view, copy_select, copy_output, &config::settings().shells)
	}

	/// Changes whenever anything the menus show does, so the macOS menu bar is
	/// rebuilt only then.
	#[cfg(target_os = "macos")]
	pub(super) fn bar_menus_key(&self) -> u64 {
		use std::hash::{Hash, Hasher};
		let mut hasher = std::collections::hash_map::DefaultHasher::new();
		self.bar_state().hash(&mut hasher);
		let settings = config::settings();
		for shell in &settings.shells {
			(shell.active, &shell.title).hash(&mut hasher);
		}
		// in table order, so each hotkey's chords keep one place in the hash
		for (_, chords) in settings.keys.in_force() {
			chords.hash(&mut hasher);
		}
		hasher.finish()
	}

	/// Open the dropdown for top-level menu `idx`, anchored under its title.
	pub(super) fn open_bar_menu(&mut self, idx: usize) {
		let items = self.bar_menu_items(idx);
		let x = self.menubar_layout().get(idx).map_or(0.0, |&(x, _)| x);
		let target = self.tabs.cur().focused;
		let bar_h = self.menu_bar_h();
		self.popup(target, items, x, bar_h);
		self.bar_open = Some(idx);
	}

	// Request the About window. App opens it (window creation needs the event
	// loop); the old in-surface overlay path is no longer used.
	fn open_about(&mut self) {
		self.pending_about = true;
		self.menu = None;
		self.bar_open = None;
	}

	pub(super) fn apply_menu(
		&mut self,
		action: MenuAction,
		target: PaneId,
		proxy: &EventLoopProxy<UserEvent>,
	) {
		if self.menu_reaches_tab_edit(action) {
			return;
		}
		let area = self.area();
		match action {
			// the URL was captured when the menu opened - the output under it may
			// have scrolled away since
			MenuAction::OpenLink => {
				if let Some(url) = self.menu_link.clone() {
					open_link(&url);
				}
			}
			MenuAction::CopyLink => {
				if let Some(url) = self.menu_link.clone() {
					self.clipboard.set_clipboard(url);
				}
			}
			MenuAction::Copy => {
				if let Some(text) = self
					.tabs
					.cur()
					.panes
					.get(&target)
					.and_then(crate::pane::Pane::selection_text)
				{
					self.clipboard.set_clipboard(text);
				}
			}
			MenuAction::Paste => {
				if let Some(text) = self.clipboard.get_clipboard() {
					if let Some(p) = self.tabs.cur_mut().panes.get_mut(&target) {
						p.paste(&text);
					}
				}
			}
			MenuAction::PasteSelection => {
				if let Some(text) = self.clipboard.get_primary() {
					if let Some(p) = self.tabs.cur_mut().panes.get_mut(&target) {
						p.paste(&text);
					}
				}
			}
			// the rename it was for has already ended
			MenuAction::Edit(_) => {}
			MenuAction::ToggleCopySelect => self.toggle_copy(target, CopyKind::Select),
			MenuAction::ToggleCopyOutput => self.toggle_copy(target, CopyKind::Output),
			MenuAction::ToggleReadOnly => {
				if let Some(p) = self.tabs.cur_mut().panes.get_mut(&target) {
					p.read_only = !p.read_only;
				}
			}
			MenuAction::SplitVertical => {
				self.tabs
					.cur_mut()
					.split(&mut self.text, proxy, target, Dir::Vertical, area);
			}
			MenuAction::SplitHorizontal => {
				self.tabs
					.cur_mut()
					.split(&mut self.text, proxy, target, Dir::Horizontal, area);
			}
			MenuAction::SplitShell(dir, index) => {
				if let Some(cmd) = shell_argv(index) {
					self.tabs
						.cur_mut()
						.split_with(&mut self.text, proxy, target, dir, cmd, area);
				}
			}
			MenuAction::Close => {
				let scope = {
					let cur = self.tabs.cur();
					close_scope(
						cur.panes.contains_key(&target),
						cur.panes.len(),
						self.tabs.len(),
					)
				};
				match scope {
					CloseScope::Pane => {
						self.tabs.cur_mut().close(&mut self.text, target, area);
					}
					// last pane in this tab -> the tab; last pane of the last tab
					// -> the window
					CloseScope::Tab => self.close_tab(),
					CloseScope::Window => self.quit = true,
					CloseScope::Nothing => {}
				}
			}
			MenuAction::NewTab => self.new_tab(proxy),
			MenuAction::NewWindow => self.new_window(),
			MenuAction::PrevTab => self.step_tab(false),
			MenuAction::NextTab => self.step_tab(true),
			MenuAction::NewTabShell(index) => self.new_tab_with(proxy, shell_argv(index)),
			MenuAction::CloseTab => self.close_tab(),
			MenuAction::FontBigger => self.font_zoom(1),
			MenuAction::FontSmaller => self.font_zoom(-1),
			MenuAction::FontReset => self.font_zoom_reset(),
			MenuAction::ToggleFullscreen => self.toggle_fullscreen(),
			MenuAction::ToggleFrame => {
				self.decorated = !self.decorated;
				self.window.set_decorations(self.decorated);
			}
			// a Mac has no in-window bar to bring back (`menu_bar_at_launch`)
			MenuAction::ToggleMenuBar => {
				self.menu_bar = !self.menu_bar && !cfg!(target_os = "macos");
				self.relayout_all();
			}
			MenuAction::ToggleBare => self.toggle_bare(),
			MenuAction::ToggleRemote => self.toggle_remote(),
			MenuAction::ToggleMinimap => {
				save_live(|live| {
					live.minimap = !live.minimap;
					true
				});
				self.relayout_all();
			}
			MenuAction::ToggleSingleTab => {
				save_live(|live| {
					live.hide_single_tab = !live.hide_single_tab;
					true
				});
				self.relayout_all();
			}
			MenuAction::NextWallpaper => self.advance_wallpaper(),
			MenuAction::ReloadConfig => self.reload_config(),
			MenuAction::Settings => self.open_settings(),
			MenuAction::About => self.open_about(),
			MenuAction::Quit => self.quit = true,
		}
		self.update_title();
	}
}

#[cfg(test)]
mod tests {
	use super::super::MENU_BAR;
	use super::super::rotation::rotation_live;
	use super::super::tests::shell;
	use super::{
		ContextMenu, CtxState, Entry, MenuAction, ViewState, accel_at, accel_clash,
		context_menu_items, edit_menu_items, entry_accel, entry_check_accel, entry_item_accel,
		entry_label, entry_sub, file_menu_items, help_menu_items, menu_metrics, panes_menu_items,
		split_shells, tabs_menu_items, view_menu_items, with_shortcuts,
	};
	use crate::config;
	use crate::pane::Dir;
	use crate::shells::ShellEntry;
	fn test_menu(x: f32, w: f32, entries: Vec<Entry>) -> ContextMenu {
		ContextMenu {
			x,
			y: 0.0,
			w,
			item_h: 20.0,
			pad_y: 6.0,
			sep_h: 9.0,
			target: 0,
			entries,
			hover: None,
			sub: None,
		}
	}

	fn test_item(label: &str) -> Entry {
		Entry::Item {
			label: label.into(),
			action: MenuAction::Copy,
			check: None,
			accel: None,
		}
	}

	// A letter picks the first row carrying it, so a menu that spends one twice
	// does not read as ambiguous - it quietly makes the later row unreachable.
	// Every menu is checked for this where it is built.
	// Test ID: EnMAGUK
	#[test]
	fn one_menu_never_spends_an_accelerator_twice() {
		let rows = vec![
			entry_item_accel('C', "Copy", MenuAction::Copy),
			entry_sub(Some('S'), "New tab with shell", vec![]),
			entry_check_accel('W', false, "Window frame", MenuAction::ToggleFrame),
		];
		assert_eq!(accel_clash(&rows), None);
		// 'w' again, which is what the View menu would have done had the submenu
		// row spelled its accelerator the way the Tabs menu's does
		let mut clashing = rows;
		clashing.insert(1, entry_sub(Some('w'), "New tab with shell", vec![]));
		assert_eq!(accel_clash(&clashing), Some('w'));
	}

	// A submenu row is an ordinary row to the pointer and to the keyboard - only
	// what ACTIVATING it does is different. Treating it as a separator instead
	// (which is what the old two-arm matches did) leaves it unhoverable and
	// unreachable, i.e. an item nothing can ever pick.
	// Test ID: EnM97jE
	#[test]
	fn a_submenu_row_hit_tests_and_steps_like_an_item() {
		let menu = test_menu(
			0.0,
			200.0,
			vec![
				test_item("One"),
				entry_sub(Some('w'), "With Shell", vec![test_item("Bash")]),
				Entry::Sep,
				test_item("Two"),
			],
		);
		let mid = |row: usize| menu.row_top(row) + menu.item_h / 2.0;
		assert_eq!(menu.item_at(10.0, mid(1)), Some(1), "the row is hoverable");
		let sep_mid = menu.row_top(2) + menu.sep_h / 2.0;
		assert_eq!(
			menu.item_at(10.0, sep_mid),
			None,
			"a separator still is not"
		);
		// down from the first row reaches it, and carries on past it
		assert_eq!(menu.step(Some(0), 1), Some(1));
		assert_eq!(menu.step(Some(1), 1), Some(3), "the separator is skipped");
		assert_eq!(menu.step(Some(3), -1), Some(1));
	}

	// The submenu is placed clear of its parent's right edge on purpose: that is
	// the whole of what keeps the pointer rule simple, since "inside the submenu"
	// and "on a parent row" can then never both be true. A submenu that overlaps
	// would close itself the moment the pointer entered it.
	// Test ID: EnM97jF
	#[test]
	fn a_submenu_stands_clear_of_the_rows_it_came_from() {
		let parent = test_menu(0.0, 200.0, vec![test_item("One"), test_item("Two")]);
		let sub = test_menu(200.0, 120.0, vec![test_item("Bash"), test_item("Zsh")]);
		for row in 0..2 {
			let y = sub.row_top(row) + sub.item_h / 2.0;
			let x = sub.x + sub.w / 2.0;
			assert!(sub.item_at(x, y).is_some(), "the submenu owns its own rows");
			assert!(
				parent.item_at(x, y).is_none(),
				"and the parent claims none of them"
			);
		}
	}

	// A click inside an open submenu is a click on the menu, so the chrome that
	// stands aside for a popup (the menu bar, the tab bar) has to stand aside for
	// it too - otherwise a submenu overlapping either band loses its clicks to it.
	// Test ID: EnM97jG
	#[test]
	fn a_click_in_the_submenu_still_counts_as_a_click_on_the_menu() {
		let mut parent = test_menu(0.0, 200.0, vec![entry_sub(Some('w'), "With Shell", vec![])]);
		let sub = test_menu(200.0, 120.0, vec![test_item("Bash")]);
		let (x, y) = (sub.x + 10.0, sub.row_top(0) + 2.0);
		assert!(!parent.hit(x, y));
		assert!(!parent.hit_any(x, y), "nothing is open yet");
		parent.sub = Some(Box::new(sub));
		assert!(parent.hit_any(x, y));
		assert_eq!(parent.chain().len(), 2);
	}

	// A dropdown resolves its own padding and separator height from DIP once, at
	// the moment it is built, so the draw and the two hit tests read one set of
	// numbers. Whatever the display does to them, `item_at` and `row_top` have to
	// keep agreeing - a menu whose rows are drawn one place and clicked another
	// is the failure this guards.
	// Test ID: EnLuU53
	#[test]
	fn a_dropdown_scales_whole_and_its_rows_still_hit_test() {
		let menu_at = |scale: f32| {
			let (pad_y, sep_h) = menu_metrics(scale);
			ContextMenu {
				x: 0.0,
				y: 0.0,
				w: config::dip(200.0, scale),
				item_h: config::dip(20.0, scale),
				pad_y,
				sep_h,
				target: 0,
				entries: vec![
					Entry::Item {
						label: "One".into(),
						action: MenuAction::Copy,
						check: None,
						accel: None,
					},
					Entry::Sep,
					Entry::Item {
						label: "Two".into(),
						action: MenuAction::Paste,
						check: None,
						accel: None,
					},
				],
				hover: None,
				sub: None,
			}
		};
		// the padding and the separator row scale, which is what makes the whole
		// popup scale - height() and row_top() are built out of them
		assert_eq!(
			menu_metrics(1.0),
			(config::MENU_ITEM_PAD_Y, config::MENU_SEP_H)
		);
		assert_eq!(
			menu_metrics(2.0),
			(config::MENU_ITEM_PAD_Y * 2.0, config::MENU_SEP_H * 2.0)
		);
		let one = menu_at(1.0);
		let two = menu_at(2.0);
		assert_eq!(two.height(), one.height() * 2.0);
		assert_eq!(two.row_top(2), one.row_top(2) * 2.0);
		// every item is still picked at the row it is drawn on, at either scale
		for menu in [&one, &two] {
			for i in [0usize, 2] {
				let mid = menu.row_top(i) + menu.item_h / 2.0;
				assert_eq!(menu.item_at(menu.w / 2.0, mid), Some(i));
			}
			// the separator's own band belongs to no item
			assert_eq!(menu.item_at(menu.w / 2.0, menu.row_top(1) + 1.0), None);
			// and a click just past the last row is off the menu entirely
			assert!(!menu.hit(menu.w / 2.0, menu.height() + 1.0));
		}
	}

	// The style guide asks every View toggle to name the thing and be checked
	// while that thing is on, so a reader can take the whole column one way. A
	// "Hide ..." caption checked when the thing is GONE reads backwards next to
	// its neighbours, which is what this stops coming back.
	// Test ID: EolQiSW
	#[test]
	fn every_view_toggle_is_checked_while_its_subject_is_on() {
		let all_on = ViewState {
			read_only: true,
			fullscreen: true,
			window_frame: true,
			menu_bar: true,
			tab_strip: true,
			minimap: true,
			bare: true,
			remote: true,
			next_wallpaper: true,
		};
		for entry in view_menu_items(all_on) {
			if let Entry::Item {
				label,
				check: Some(on),
				..
			} = &entry
			{
				assert!(
					on,
					"{label} is a toggle that draws unchecked with everything on"
				);
				assert!(
					!label.starts_with("Hide "),
					"{label} names the absence of a thing rather than the thing"
				);
			}
		}
		// and each of these has a row at all, checked exactly while it is on
		let all_off = ViewState {
			read_only: false,
			fullscreen: false,
			window_frame: false,
			menu_bar: false,
			tab_strip: false,
			minimap: false,
			bare: false,
			remote: false,
			next_wallpaper: false,
		};
		let rows: [(&str, fn(&MenuAction) -> bool); 4] = [
			("Menu bar", |a| matches!(a, MenuAction::ToggleMenuBar)),
			("Window frame", |a| matches!(a, MenuAction::ToggleFrame)),
			("Fullscreen", |a| matches!(a, MenuAction::ToggleFullscreen)),
			("Read-only", |a| matches!(a, MenuAction::ToggleReadOnly)),
		];
		for (state, on) in [(all_on, true), (all_off, false)] {
			let items = view_menu_items(state);
			for (name, want) in rows {
				assert_eq!(
					check_of(&items, want),
					Some(Some(on)),
					"View's {name} row, everything {}",
					if on { "on" } else { "off" }
				);
			}
		}
	}

	// Test ID: EolQiSX
	#[test]
	fn the_view_menu_spends_no_accelerator_twice() {
		let off = ViewState {
			read_only: false,
			fullscreen: false,
			window_frame: false,
			menu_bar: false,
			tab_strip: false,
			minimap: false,
			bare: false,
			remote: false,
			next_wallpaper: true,
		};
		assert_eq!(accel_clash(&view_menu_items(off)), None);
	}

	// Test ID: EqpdApU
	#[test]
	fn next_wallpaper_shows_only_with_somewhere_to_go() {
		let state = |next_wallpaper| ViewState {
			read_only: false,
			fullscreen: false,
			window_frame: false,
			menu_bar: false,
			tab_strip: false,
			minimap: false,
			bare: false,
			remote: false,
			next_wallpaper,
		};
		let has_row = |items: Vec<Entry>| {
			items.iter().any(|entry| {
				matches!(
					entry,
					Entry::Item {
						action: MenuAction::NextWallpaper,
						..
					}
				)
			})
		};
		assert!(has_row(view_menu_items(state(true))));
		assert!(!has_row(view_menu_items(state(false))));

		assert!(rotation_live(false, 2, true));
		assert!(
			!rotation_live(true, 2, true),
			"a command-line wallpaper holds"
		);
		assert!(!rotation_live(false, 1, true), "one image has no next");
		assert!(
			!rotation_live(false, 5, false),
			"no folder, nothing to rotate"
		);
	}

	// Some(check) for the first top-level item doing what `want` picks.
	fn check_of(items: &[Entry], want: fn(&MenuAction) -> bool) -> Option<Option<bool>> {
		items.iter().find_map(|entry| match entry {
			Entry::Item { action, check, .. } if want(action) => Some(*check),
			_ => None,
		})
	}

	fn position_of(items: &[Entry], want: fn(&MenuAction) -> bool) -> Option<usize> {
		items
			.iter()
			.position(|entry| matches!(entry, Entry::Item { action, .. } if want(action)))
	}

	// Every action a menu can reach, submenus included.
	fn actions_in(items: &[Entry]) -> Vec<MenuAction> {
		let mut out = Vec::new();
		for entry in items {
			match entry {
				Entry::Item { action, .. } => out.push(*action),
				Entry::Sub { items, .. } => out.extend(actions_in(items)),
				Entry::Sep => {}
			}
		}
		out
	}

	// A disabled entry sits between the two active ones, so an index that
	// counted only the offered rows would show.
	fn three_shells() -> Vec<ShellEntry> {
		vec![
			shell("PowerShell 7", true),
			shell("fish", false),
			shell("Nushell", true),
		]
	}

	// Every menu with as many rows as it can have: each toggle on, a link under
	// the pointer, a wallpaper to move on to, and shells to offer.
	fn every_menu(shells: &[ShellEntry]) -> Vec<(&'static str, Vec<Entry>)> {
		let view = ViewState {
			read_only: true,
			fullscreen: true,
			window_frame: true,
			menu_bar: true,
			tab_strip: true,
			minimap: true,
			bare: true,
			remote: true,
			next_wallpaper: true,
		};
		let ctx = CtxState {
			link: true,
			read_only: true,
			copy_select: true,
			copy_output: true,
			menu_bar: true,
			next_wallpaper: true,
		};
		let keys = crate::keys::Bindings::defaults(false);
		vec![
			("File", file_menu_items()),
			("Edit", edit_menu_items(true, true)),
			("View", view_menu_items(view)),
			("Tabs", tabs_menu_items(shells)),
			("Panes", panes_menu_items(shells)),
			("Help", help_menu_items()),
			("right-click", context_menu_items(ctx, shells)),
		]
		.into_iter()
		.map(|(name, items)| (name, with_shortcuts(items, &keys, false)))
		.collect()
	}

	// The shortcut in "Label (Ctrl+Shift+C)", if the row shows one.
	fn shortcut_of(label: &str) -> Option<&str> {
		let open = label.rfind(" (")?;
		label[open + 2..].strip_suffix(')')
	}

	// Build time only checks a menu's letters in a debug build, and only when
	// that menu is opened. This holds every one of them to it.
	// Test ID: Er2UvPk
	#[test]
	fn no_menu_spends_an_accelerator_twice() {
		for shells in [three_shells(), Vec::new()] {
			for (name, items) in every_menu(&shells) {
				assert_eq!(accel_clash(&items), None, "{name}");
			}
		}
	}

	// Keys are spelled as words, so "Ctrl+=" or "Ctrl++" cannot come back.
	// Ctrl+, keeps its comma: the style guide spells it that way too.
	// Test ID: Er2UvPl
	#[test]
	fn every_shortcut_in_a_menu_is_written_one_way() {
		for (name, items) in every_menu(&three_shells()) {
			for label in items.iter().filter_map(entry_label) {
				let Some(keys) = shortcut_of(label) else {
					continue;
				};
				assert!(!keys.contains(' '), "{name}: {label}");
				for key in keys.split('+') {
					assert!(
						key == "," || (!key.is_empty() && key.chars().all(char::is_alphanumeric)),
						"{name}: {label} spells a key as {key:?}"
					);
				}
			}
		}
		let view = &every_menu(&[])[2].1;
		let label_of = |want| {
			position_of(view, want)
				.and_then(|i| entry_label(&view[i]))
				.unwrap_or_default()
		};
		assert_eq!(
			shortcut_of(label_of(|a| matches!(a, MenuAction::FontBigger))),
			Some("Ctrl+Plus")
		);
		assert_eq!(
			shortcut_of(label_of(|a| matches!(a, MenuAction::FontSmaller))),
			Some("Ctrl+Minus")
		);
	}

	// A row that opens a prompt or a dialog ends in the one ellipsis character,
	// never three dots.
	// Test ID: Er2UvPm
	#[test]
	fn a_row_that_asks_for_more_ends_in_a_real_ellipsis() {
		use crate::ui_spec::{Key, Kind};
		let spec = crate::ui_spec::ui()
			.specs
			.iter()
			.find(|spec| spec.key == Key::ThemeActions)
			.expect("the theme buttons row");
		let Kind::Buttons(labels) = spec.kind else {
			panic!("the theme actions are buttons");
		};
		for want in ["Save as\u{2026}", "Rename\u{2026}"] {
			assert!(labels.contains(&want), "{want} in {labels:?}");
		}
		for (name, items) in every_menu(&three_shells()) {
			for label in items.iter().filter_map(entry_label) {
				assert!(!label.contains("..."), "{name}: {label}");
			}
		}
	}

	// Sentence case: past the first letter, a capital is either the row's
	// accelerator ("Paste Selection") or part of the shortcut. Shell titles in a
	// flyout are names and keep theirs.
	// Test ID: Er2UvPn
	#[test]
	fn every_menu_row_is_in_sentence_case() {
		for (name, items) in every_menu(&three_shells()) {
			for entry in &items {
				let Some(label) = entry_label(entry) else {
					continue;
				};
				let accel = entry_accel(entry).map(|(_, pos)| pos);
				let words = label.rfind(" (").map_or(label, |open| &label[..open]);
				for (at, ch) in words.char_indices().skip(1) {
					assert!(
						!ch.is_uppercase() || accel == Some(at),
						"{name}: {label} capitalizes {ch}"
					);
				}
			}
		}
	}

	// Test ID: Er2UvPo
	#[test]
	fn the_right_click_menu_rules_off_the_tab_rows_from_the_pane_rows() {
		let ctx = CtxState {
			link: false,
			read_only: false,
			copy_select: false,
			copy_output: false,
			menu_bar: true,
			next_wallpaper: false,
		};
		for shells in [three_shells(), Vec::new()] {
			let items = context_menu_items(ctx, &shells);
			let last_tab = items
				.iter()
				.rposition(|entry| {
					actions_in(std::slice::from_ref(entry))
						.iter()
						.any(|a| matches!(a, MenuAction::NewTab | MenuAction::NewTabShell(_)))
				})
				.expect("a tab row");
			let first_pane = position_of(&items, |a| matches!(a, MenuAction::SplitVertical))
				.expect("a pane row");
			assert!(last_tab < first_pane);
			assert!(
				items[last_tab..first_pane]
					.iter()
					.any(|entry| matches!(entry, Entry::Sep)),
				"no rule between the tab rows and the pane rows"
			);
		}
	}

	// Off since macOS has no in-window bar at all (20261002): `--hide-menu=false`
	// no longer brings it back there. `there_is_no_in_window_menu_bar_on_macos`
	// covers it.
	// // macOS has the system menu bar, so the in-window one starts hidden there
	// // unless --hide-menu says otherwise. Elsewhere nothing changes.
	// // Test ID: ErUnxsD
	// #[test]
	// fn the_in_window_menu_bar_starts_hidden_on_macos_only() {
	// 	use super::menu_bar_at_launch;
	// 	assert!(menu_bar_at_launch(None, false));
	// 	assert!(!menu_bar_at_launch(None, true));
	// 	for mac in [false, true] {
	// 		assert!(menu_bar_at_launch(Some(false), mac));
	// 		assert!(!menu_bar_at_launch(Some(true), mac));
	// 	}
	// }

	// The system menu bar is the only one on a Mac: `--hide-menu` is taken and
	// ignored there, and no menu row offers the in-window bar. Elsewhere nothing
	// changes.
	// Test ID: ErZrS8g
	#[test]
	fn there_is_no_in_window_menu_bar_on_macos() {
		use super::super::menubar::menu_bar_at_launch;
		use super::mac_entries;
		for hide_menu in [None, Some(false), Some(true)] {
			assert!(!menu_bar_at_launch(hide_menu, true), "{hide_menu:?}");
		}
		assert!(menu_bar_at_launch(None, false));
		assert!(menu_bar_at_launch(Some(false), false));
		assert!(!menu_bar_at_launch(Some(true), false));
		let shells = [shell("bash", true)];
		for next_wallpaper in [false, true] {
			let ctx = CtxState {
				link: true,
				read_only: false,
				copy_select: false,
				copy_output: false,
				menu_bar: false,
				next_wallpaper,
			};
			let window = context_menu_items(ctx, &shells);
			assert!(actions_in(&window).contains(&MenuAction::ToggleMenuBar));
			let mac = mac_entries(window);
			assert!(!actions_in(&mac).contains(&MenuAction::ToggleMenuBar));
			assert!(!matches!(mac.first(), Some(Entry::Sep)));
			assert!(!matches!(mac.last(), Some(Entry::Sep)));
			assert!(
				!mac.windows(2)
					.any(|pair| matches!(pair, [Entry::Sep, Entry::Sep]))
			);
		}
	}

	// A row that does what a hotkey does shows the chord that hotkey answers to,
	// from the bindings in force: the pane rows show the pane chords, a chord
	// moved in the config file shows where it went, and a hotkey turned off
	// shows nothing.
	// Test ID: EreU3se
	#[test]
	fn a_menu_row_shows_the_chord_its_hotkey_answers_to() {
		use crate::input::Hotkey;
		use crate::keys::{Bindings, Chord};
		let shells = [shell("bash", true)];
		let ctx = CtxState {
			link: false,
			read_only: false,
			copy_select: false,
			copy_output: false,
			menu_bar: true,
			next_wallpaper: false,
		};
		let label = |entries: &[Entry], want: MenuAction| {
			entries.iter().find_map(|entry| match entry {
				Entry::Item { label, action, .. } if *action == want => Some(label.clone()),
				_ => None,
			})
		};
		let pc = Bindings::defaults(false);
		for items in [
			with_shortcuts(panes_menu_items(&shells), &pc, false),
			with_shortcuts(context_menu_items(ctx, &shells), &pc, false),
		] {
			for (action, shown) in [
				(MenuAction::SplitVertical, "Split vertical (Alt+Shift+Plus)"),
				(
					MenuAction::SplitHorizontal,
					"Split horizontal (Alt+Shift+Minus)",
				),
				(MenuAction::Close, "Close pane (Alt+Shift+W)"),
			] {
				assert_eq!(label(&items, action).as_deref(), Some(shown));
			}
		}
		let mac = with_shortcuts(panes_menu_items(&shells), &Bindings::defaults(true), true);
		assert_eq!(
			label(&mac, MenuAction::SplitVertical).as_deref(),
			Some("Split vertical (Command+D)")
		);
		assert_eq!(
			label(&mac, MenuAction::SplitHorizontal).as_deref(),
			Some("Split horizontal (Shift+Command+D)")
		);
		// Close pane had no Mac chord until 20261003
		// assert_eq!(
		// 	label(&mac, MenuAction::Close).as_deref(),
		// 	Some("Close pane")
		// );
		assert_eq!(
			label(&mac, MenuAction::Close).as_deref(),
			Some("Close pane (Option+Command+W)")
		);
		let chord = |text| Chord::parse(text).expect(text);
		let (moved, _) = Bindings::with(
			false,
			&[
				(Hotkey::ClosePane, vec![chord("Ctrl+Shift+W")]),
				(Hotkey::SplitRight, Vec::new()),
			],
		);
		let tabs = with_shortcuts(tabs_menu_items(&shells), &moved, false);
		assert_eq!(
			label(&tabs, MenuAction::CloseTab).as_deref(),
			Some("Close tab (Ctrl+F4)")
		);
		let panes = with_shortcuts(panes_menu_items(&shells), &moved, false);
		assert_eq!(
			label(&panes, MenuAction::Close).as_deref(),
			Some("Close pane (Ctrl+Shift+W)")
		);
		assert_eq!(
			label(&panes, MenuAction::SplitVertical).as_deref(),
			Some("Split vertical")
		);
	}

	// The right-click menu on a Mac names the Command chord for each row that has
	// one, never a Ctrl one, and keeps its accelerator letters.
	// Test ID: ErZrSS3
	#[test]
	fn the_mac_right_click_menu_shows_command_chords() {
		use super::mac_entries;
		let shells = [shell("bash", true)];
		let ctx = CtxState {
			link: true,
			read_only: false,
			copy_select: false,
			copy_output: false,
			menu_bar: false,
			next_wallpaper: true,
		};
		let window = context_menu_items(ctx, &shells);
		let mac = mac_entries(with_shortcuts(
			window.clone(),
			&crate::keys::Bindings::defaults(true),
			true,
		));
		let label = |entries: &[Entry], want: MenuAction| {
			entries.iter().find_map(|entry| match entry {
				Entry::Item { label, action, .. } if *action == want => Some(label.clone()),
				_ => None,
			})
		};
		for (action, shown) in [
			(MenuAction::Copy, "Copy (Command+C)"),
			(MenuAction::Paste, "Paste (Command+V)"),
			(MenuAction::NewTab, "New tab (Command+T)"),
			(MenuAction::Settings, "Settings\u{2026} (Command+,)"),
			(MenuAction::PasteSelection, "Paste Selection"),
		] {
			assert_eq!(label(&mac, action).as_deref(), Some(shown));
		}
		let mut rows = 0;
		for entry in &mac {
			let Some(text) = entry_label(entry) else {
				continue;
			};
			rows += 1;
			assert!(!text.contains("Ctrl"), "{text}");
			if let Some((label, pos)) = entry_accel(entry) {
				let before = window
					.iter()
					.filter_map(entry_accel)
					.find(|(old, _)| old.split(" (").next() == label.split(" (").next());
				let (old, old_pos) = before.expect("row came across");
				assert_eq!(label[pos..].chars().next(), old[old_pos..].chars().next());
			}
		}
		assert!(rows > 10);
		assert_eq!(accel_clash(&mac), None);
	}

	// Test ID: Er2UvPp
	#[test]
	fn the_bar_reads_file_to_help_and_file_has_no_tab_or_pane_action() {
		assert_eq!(MENU_BAR, ["File", "Edit", "View", "Tabs", "Panes", "Help"]);
		for action in actions_in(&file_menu_items()) {
			assert!(!matches!(
				action,
				MenuAction::NewTab
					| MenuAction::NewTabShell(_)
					| MenuAction::CloseTab
					| MenuAction::SplitVertical
					| MenuAction::SplitHorizontal
					| MenuAction::SplitShell(..)
					| MenuAction::Close
			));
		}
	}

	// Test ID: Er2UvPq
	#[test]
	fn the_right_click_menu_has_the_pane_actions_and_their_checkmarks() {
		for on in [false, true] {
			let items = context_menu_items(
				CtxState {
					link: false,
					read_only: on,
					copy_select: false,
					copy_output: false,
					menu_bar: on,
					next_wallpaper: false,
				},
				&[],
			);
			let plain: [(&str, fn(&MenuAction) -> bool); 8] = [
				("Copy", |a| matches!(a, MenuAction::Copy)),
				("Paste", |a| matches!(a, MenuAction::Paste)),
				("Paste selection", |a| {
					matches!(a, MenuAction::PasteSelection)
				}),
				("New tab", |a| matches!(a, MenuAction::NewTab)),
				("Split vertical", |a| matches!(a, MenuAction::SplitVertical)),
				("Split horizontal", |a| {
					matches!(a, MenuAction::SplitHorizontal)
				}),
				("Reload config", |a| matches!(a, MenuAction::ReloadConfig)),
				("Settings", |a| matches!(a, MenuAction::Settings)),
			];
			for (name, want) in plain {
				assert_eq!(check_of(&items, want), Some(None), "{name}");
			}
			assert_eq!(
				check_of(&items, |a| matches!(a, MenuAction::ToggleReadOnly)),
				Some(Some(on))
			);
			assert_eq!(
				check_of(&items, |a| matches!(a, MenuAction::ToggleMenuBar)),
				Some(Some(on))
			);
			assert_eq!(accel_clash(&items), None);
		}
	}

	// Test ID: Er2UvPr
	#[test]
	fn a_new_tab_shell_row_lists_the_active_shells_or_is_not_there() {
		let ctx = CtxState {
			link: false,
			read_only: false,
			copy_select: false,
			copy_output: false,
			menu_bar: true,
			next_wallpaper: false,
		};
		let shells = three_shells();
		for (name, items) in [
			("Tabs", tabs_menu_items(&shells)),
			("right-click", context_menu_items(ctx, &shells)),
		] {
			let below = position_of(&items, |a| matches!(a, MenuAction::NewTab)).unwrap() + 1;
			let Entry::Sub { label, items, .. } = &items[below] else {
				panic!("{name}: no shell row under New tab");
			};
			assert_eq!(label, "New tab with shell");
			let offered: Vec<_> = items.iter().filter_map(entry_label).collect();
			assert_eq!(offered, ["PowerShell 7", "Nushell"], "{name}");
			let picks = actions_in(items);
			assert!(matches!(
				picks[..],
				[MenuAction::NewTabShell(0), MenuAction::NewTabShell(2)]
			));
		}
		for shells in [Vec::new(), vec![shell("fish", false)]] {
			for items in [tabs_menu_items(&shells), context_menu_items(ctx, &shells)] {
				assert!(!items.iter().any(|entry| matches!(entry, Entry::Sub { .. })));
			}
		}
	}

	// Test ID: Er2UvPs
	#[test]
	fn both_split_shell_rows_offer_the_same_shells_by_their_place_in_the_list() {
		let shells = three_shells();
		let rows = split_shells(&shells);
		assert_eq!(rows.len(), 2);
		for (row, (label, dir)) in rows.iter().zip([
			("Split vertical with shell", Dir::Vertical),
			("Split horizontal with shell", Dir::Horizontal),
		]) {
			let Entry::Sub {
				label: shown,
				items,
				..
			} = row
			else {
				panic!("{label} is not a submenu");
			};
			assert_eq!(shown, label);
			let offered: Vec<_> = items.iter().filter_map(entry_label).collect();
			assert_eq!(offered, ["PowerShell 7", "Nushell"], "{label}");
			let picks = actions_in(items);
			assert!(
				matches!(
					picks[..],
					[MenuAction::SplitShell(a, 0), MenuAction::SplitShell(b, 2)] if a == dir && b == dir
				),
				"{label}"
			);
		}
		// both rows sit under the two plain splits on the Panes menu
		let panes = panes_menu_items(&shells);
		assert!(matches!(
			panes[2..4],
			[Entry::Sub { .. }, Entry::Sub { .. }]
		));
		assert!(split_shells(&[shell("fish", false)]).is_empty());
		assert!(split_shells(&[]).is_empty());
	}

	// Test ID: EkrYObg
	#[test]
	fn accel_prefers_exact_case_then_falls_back() {
		// 'S' must pick "Selection", not the 's' in "Paste"
		assert_eq!(accel_at("Paste Selection", 'S'), Some(6));
		// no capital 'O' -> case-insensitive fallback finds "only"
		assert_eq!(accel_at("Read-only", 'O'), Some(5));
		assert_eq!(accel_at("Quit", 'x'), None);
	}
}
