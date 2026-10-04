// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The macOS menu bar. Its layout is plain data built from the same entry lists
// the in-window menus draw, so it is checked on every platform; only `native`
// talks to AppKit.

use winit::keyboard::NamedKey;

use crate::app::{Entry, MenuAction, mac_entries, menu_hotkey, without_rows};
use crate::keys::{Bindings, Chord, KeyName, us_shifted};

pub const APP_NAME: &str = "SilkTerm";

/// App and Window menu rows `AppKit` carries out by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemItem {
	Services,
	Hide,
	HideOthers,
	ShowAll,
	Minimize,
	Zoom,
	BringAllToFront,
}

/// The title of the menu `AppKit` keeps the window list in.
pub const WINDOW_MENU: &str = "Window";

#[derive(Debug, Clone, PartialEq)]
pub enum BarItem {
	Action {
		label: String,
		action: MenuAction,
		check: Option<bool>,
		key: Option<Chord>,
	},
	System {
		label: String,
		item: SystemItem,
		key: Option<Chord>,
	},
	Submenu {
		label: String,
		items: Vec<BarItem>,
	},
	Separator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarMenu {
	pub title: String,
	pub items: Vec<BarItem>,
}

// These three live in the app menu on a Mac, so they leave the menus they sit
// in elsewhere.
fn in_app_menu(action: MenuAction) -> bool {
	matches!(
		action,
		MenuAction::About | MenuAction::Settings | MenuAction::Quit
	)
}

// Command plus a key, for the rows `AppKit` carries out itself.
pub fn command(key: char) -> Chord {
	Chord {
		key: KeyName::Char(key),
		ctrl: false,
		alt: false,
		shift: false,
		command: true,
	}
}

// The row's chord, the first one its hotkey answers to. Only a chord with
// Command goes on a row: `AppKit` takes a row's chord before the window sees
// the press, and one without Command would take keys the shell is owed.
fn key_for(keys: &Bindings, action: MenuAction) -> Option<Chord> {
	menu_hotkey(action)
		.and_then(|hotkey| keys.shown(hotkey))
		.filter(|chord| chord.command)
}

fn action_row(keys: &Bindings, label: String, action: MenuAction) -> BarItem {
	BarItem::Action {
		label,
		action,
		check: None,
		key: key_for(keys, action),
	}
}

fn system(label: &str, item: SystemItem, key: Option<Chord>) -> BarItem {
	BarItem::System {
		label: label.into(),
		item,
		key,
	}
}

fn app_menu(keys: &Bindings) -> BarMenu {
	BarMenu {
		title: APP_NAME.into(),
		items: vec![
			action_row(keys, format!("About {APP_NAME}"), MenuAction::About),
			BarItem::Separator,
			action_row(keys, "Settings\u{2026}".into(), MenuAction::Settings),
			BarItem::Separator,
			system("Services", SystemItem::Services, None),
			BarItem::Separator,
			system(
				&format!("Hide {APP_NAME}"),
				SystemItem::Hide,
				Some(command('h')),
			),
			system(
				"Hide others",
				SystemItem::HideOthers,
				Some(Chord {
					alt: true,
					..command('h')
				}),
			),
			system("Show all", SystemItem::ShowAll, None),
			BarItem::Separator,
			action_row(keys, format!("Quit {APP_NAME}"), MenuAction::Quit),
		],
	}
}

// Apple's Window menu, with SilkTerm's own tabs where a Mac app has its window
// tabs. `AppKit` adds the list of open windows below these, and on newer
// releases its own tiling rows. Each window is a process of its own, so the
// list and Bring all to front reach only this one's.
fn window_menu(keys: &Bindings) -> BarMenu {
	BarMenu {
		title: WINDOW_MENU.into(),
		items: vec![
			system("Minimize", SystemItem::Minimize, Some(command('m'))),
			system("Zoom", SystemItem::Zoom, None),
			BarItem::Separator,
			action_row(keys, "Show previous tab".into(), MenuAction::PrevTab),
			action_row(keys, "Show next tab".into(), MenuAction::NextTab),
			BarItem::Separator,
			system("Bring all to front", SystemItem::BringAllToFront, None),
		],
	}
}

/// What a row hands `AppKit` for its chord: the character the press is matched
/// against, and the keys held. A Shift chord on a key whose shifted form is
/// another character is named by that character with no Shift held, the form
/// `AppKit` matches. None for a key `AppKit` has no character for.
pub fn key_equivalent(chord: Chord) -> Option<(String, Chord)> {
	// AppKit's private-use characters for the keys that type nothing
	let function = |code: u32| char::from_u32(code).map(String::from);
	let text = match chord.key {
		KeyName::Char(key) => match us_shifted(key) {
			Some(shifted) if chord.shift => {
				return Some((
					shifted.into(),
					Chord {
						shift: false,
						..chord
					},
				));
			}
			_ => key.to_string(),
		},
		KeyName::Plus => "+".into(),
		KeyName::Minus => "-".into(),
		KeyName::Named(named) => match named {
			NamedKey::ArrowUp => function(0xf700)?,
			NamedKey::ArrowDown => function(0xf701)?,
			NamedKey::ArrowLeft => function(0xf702)?,
			NamedKey::ArrowRight => function(0xf703)?,
			NamedKey::F1 => function(0xf704)?,
			NamedKey::F2 => function(0xf705)?,
			NamedKey::F3 => function(0xf706)?,
			NamedKey::F4 => function(0xf707)?,
			NamedKey::F5 => function(0xf708)?,
			NamedKey::F6 => function(0xf709)?,
			NamedKey::F7 => function(0xf70a)?,
			NamedKey::F8 => function(0xf70b)?,
			NamedKey::F9 => function(0xf70c)?,
			NamedKey::F10 => function(0xf70d)?,
			NamedKey::F11 => function(0xf70e)?,
			NamedKey::F12 => function(0xf70f)?,
			NamedKey::Insert => function(0xf727)?,
			NamedKey::Delete => function(0xf728)?,
			NamedKey::Home => function(0xf729)?,
			NamedKey::End => function(0xf72b)?,
			NamedKey::PageUp => function(0xf72c)?,
			NamedKey::PageDown => function(0xf72d)?,
			NamedKey::Backspace => "\u{8}".into(),
			NamedKey::Tab => "\t".into(),
			NamedKey::Enter => "\r".into(),
			NamedKey::Escape => "\u{1b}".into(),
			NamedKey::Space => " ".into(),
			_ => return None,
		},
	};
	Some((text, chord))
}

// The rows come in without the shortcut `with_shortcuts` puts in a label, since
// the menu bar draws the Command chord beside the row itself. A label is used
// as it is: a shell's name can end in parentheses of its own.
fn bar_items(keys: &Bindings, entries: Vec<Entry>) -> Vec<BarItem> {
	entries
		.into_iter()
		.map(|entry| match entry {
			Entry::Item {
				label,
				action,
				check,
				..
			} => BarItem::Action {
				label,
				action,
				check,
				key: key_for(keys, action),
			},
			Entry::Sub { label, items, .. } => BarItem::Submenu {
				label,
				items: bar_items(keys, items),
			},
			Entry::Sep => BarItem::Separator,
		})
		.collect()
}

/// The whole menu bar: the app menu, then each in-window menu by its title, less
/// the rows the app menu took and the in-window bar's own toggle, then Window.
/// File gains New window, which elsewhere is a key alone. A menu left with
/// nothing in it is dropped. Each row's chord is the one `keys` shows for it.
pub fn layout(window_menus: Vec<(&str, Vec<Entry>)>, keys: &Bindings) -> Vec<BarMenu> {
	let mut menus = vec![app_menu(keys)];
	for (title, entries) in window_menus {
		let mut items = bar_items(keys, without_rows(mac_entries(entries), in_app_menu));
		if title == "File" {
			let mut first = vec![action_row(keys, "New window".into(), MenuAction::NewWindow)];
			if !items.is_empty() {
				first.push(BarItem::Separator);
			}
			items.splice(0..0, first);
		}
		if !items.is_empty() {
			menus.push(BarMenu {
				title: title.into(),
				items,
			});
		}
	}
	menus.push(window_menu(keys));
	menus
}

#[cfg(target_os = "macos")]
pub use native::refresh;

#[cfg(target_os = "macos")]
mod native {
	use std::cell::RefCell;

	use objc2::rc::Retained;
	use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
	use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
	use objc2_app_kit::{
		NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu,
		NSMenuItem,
	};
	use objc2_foundation::NSString;
	use winit::event_loop::EventLoopProxy;

	use super::{BarItem, BarMenu, SystemItem, WINDOW_MENU, key_equivalent};
	use crate::app::MenuAction;
	use crate::keys::Chord;
	use crate::term::UserEvent;

	struct Ivars {
		proxy: EventLoopProxy<UserEvent>,
		// indexed by each row's tag
		actions: RefCell<Vec<MenuAction>>,
	}

	define_class!(
		// SAFETY: NSObject has no subclassing requirements, and MenuTarget does
		// not implement Drop.
		#[unsafe(super(NSObject))]
		#[thread_kind = MainThreadOnly]
		#[name = "SilkTermMenuTarget"]
		#[ivars = Ivars]
		struct MenuTarget;

		impl MenuTarget {
			// Hand the pick to the event loop, which runs it the way a click in
			// the in-window menu runs it.
			#[unsafe(method(pick:))]
			fn pick(&self, sender: Option<&NSMenuItem>) {
				let Some(sender) = sender else {
					return;
				};
				let action = usize::try_from(sender.tag())
					.ok()
					.and_then(|i| self.ivars().actions.borrow().get(i).copied());
				if let Some(action) = action {
					let _ = self.ivars().proxy.send_event(UserEvent::Menu(action));
				}
			}
		}

		unsafe impl NSObjectProtocol for MenuTarget {}
	);

	impl MenuTarget {
		fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<UserEvent>) -> Retained<Self> {
			let this = Self::alloc(mtm).set_ivars(Ivars {
				proxy,
				actions: RefCell::new(Vec::new()),
			});
			// SAFETY: NSObject's init, on an object just allocated.
			unsafe { msg_send![super(this), init] }
		}
	}

	struct Installed {
		key: u64,
		// A menu item does not retain its target, so this keeps it alive.
		target: Retained<MenuTarget>,
	}

	thread_local! {
		static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
	}

	/// Put the menu bar up, or rebuild it when `key` says what it shows has
	/// changed. Replaces the default menu winit installs.
	pub fn refresh(
		key: u64,
		build: impl FnOnce() -> Vec<BarMenu>,
		proxy: &EventLoopProxy<UserEvent>,
	) {
		let Some(mtm) = MainThreadMarker::new() else {
			return;
		};
		INSTALLED.with_borrow_mut(|installed| {
			if installed.as_ref().is_some_and(|i| i.key == key) {
				return;
			}
			let target = match installed.take() {
				Some(old) => old.target,
				None => MenuTarget::new(mtm, proxy.clone()),
			};
			let app = NSApplication::sharedApplication(mtm);
			let mut actions = Vec::new();
			let bar = NSMenu::new(mtm);
			for menu in build() {
				let top = NSMenuItem::new(mtm);
				let sub = submenu(mtm, &app, &menu.title, &menu.items, &target, &mut actions);
				top.setSubmenu(Some(&sub));
				// AppKit keeps the open windows listed here
				if menu.title == WINDOW_MENU {
					app.setWindowsMenu(Some(&sub));
				}
				bar.addItem(&top);
			}
			*target.ivars().actions.borrow_mut() = actions;
			app.setMainMenu(Some(&bar));
			*installed = Some(Installed { key, target });
		});
	}

	fn submenu(
		mtm: MainThreadMarker,
		app: &NSApplication,
		title: &str,
		items: &[BarItem],
		target: &MenuTarget,
		actions: &mut Vec<MenuAction>,
	) -> Retained<NSMenu> {
		let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
		for item in items {
			let row = match item {
				BarItem::Separator => NSMenuItem::separatorItem(mtm),
				BarItem::Action {
					label,
					action,
					check,
					key,
				} => {
					let row = menu_item(mtm, label, Some(sel!(pick:)), *key);
					let target: &AnyObject = target;
					// SAFETY: the target answers pick:, and is kept alive in
					// INSTALLED for as long as this menu is up.
					unsafe { row.setTarget(Some(target)) };
					row.setTag(actions.len().try_into().unwrap_or(isize::MAX));
					actions.push(*action);
					let help = action.help();
					if !help.is_empty() {
						row.setToolTip(Some(&NSString::from_str(help)));
					}
					if let Some(on) = check {
						row.setState(if *on {
							NSControlStateValueOn
						} else {
							NSControlStateValueOff
						});
					}
					row
				}
				// no target: these go up the responder chain, to the key window
				// or to NSApplication
				BarItem::System { label, item, key } => {
					let action = match item {
						SystemItem::Services => None,
						SystemItem::Hide => Some(sel!(hide:)),
						SystemItem::HideOthers => Some(sel!(hideOtherApplications:)),
						SystemItem::ShowAll => Some(sel!(unhideAllApplications:)),
						SystemItem::Minimize => Some(sel!(performMiniaturize:)),
						SystemItem::Zoom => Some(sel!(performZoom:)),
						SystemItem::BringAllToFront => Some(sel!(arrangeInFront:)),
					};
					let row = menu_item(mtm, label, action, *key);
					if *item == SystemItem::Services {
						let services =
							NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(label));
						row.setSubmenu(Some(&services));
						app.setServicesMenu(Some(&services));
					}
					row
				}
				BarItem::Submenu { label, items } => {
					let row = menu_item(mtm, label, None, None);
					let sub = submenu(mtm, app, label, items, target, actions);
					row.setSubmenu(Some(&sub));
					row
				}
			};
			menu.addItem(&row);
		}
		menu
	}

	fn menu_item(
		mtm: MainThreadMarker,
		label: &str,
		action: Option<objc2::runtime::Sel>,
		key: Option<Chord>,
	) -> Retained<NSMenuItem> {
		let key = key.and_then(key_equivalent);
		let key_text = NSString::from_str(key.as_ref().map_or("", |(text, _)| text.as_str()));
		// SAFETY: every selector passed here is one AppKit or MenuTarget answers.
		let row = unsafe {
			NSMenuItem::initWithTitle_action_keyEquivalent(
				NSMenuItem::alloc(mtm),
				&NSString::from_str(label),
				action,
				&key_text,
			)
		};
		if let Some((_, key)) = key {
			let mut mask = NSEventModifierFlags::empty();
			for (held, flag) in [
				(key.command, NSEventModifierFlags::Command),
				(key.shift, NSEventModifierFlags::Shift),
				(key.alt, NSEventModifierFlags::Option),
				(key.ctrl, NSEventModifierFlags::Control),
			] {
				if held {
					mask |= flag;
				}
			}
			row.setKeyEquivalentModifierMask(mask);
		}
		row
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::app::sample_window_menus as window_menus;

	fn actions(items: &[BarItem], out: &mut Vec<MenuAction>) {
		for item in items {
			match item {
				BarItem::Action { action, .. } => out.push(*action),
				BarItem::Submenu { items, .. } => actions(items, out),
				_ => {}
			}
		}
	}

	fn find(menus: &[BarMenu], want: MenuAction) -> Option<(&str, &BarItem)> {
		menus.iter().find_map(|menu| {
			menu.items
				.iter()
				.find(|item| matches!(item, BarItem::Action { action, .. } if *action == want))
				.map(|item| (menu.title.as_str(), item))
		})
	}

	// Test ID: ErUnDN8
	#[test]
	fn the_mac_menu_bar_has_the_app_menu_first_then_the_window_menus() {
		let menus = layout(window_menus(), &Bindings::defaults(true));
		let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
		// Help held only About, which moved to the app menu
		assert_eq!(
			titles,
			[
				"SilkTerm", "File", "Edit", "View", "Tabs", "Panes", "Window"
			]
		);
		let app: Vec<String> = menus[0]
			.items
			.iter()
			.map(|item| match item {
				BarItem::Action { label, .. } | BarItem::System { label, .. } => label.clone(),
				BarItem::Submenu { label, .. } => format!("{label} >"),
				BarItem::Separator => "-".into(),
			})
			.collect();
		assert_eq!(
			app,
			[
				"About SilkTerm",
				"-",
				"Settings\u{2026}",
				"-",
				"Services",
				"-",
				"Hide SilkTerm",
				"Hide others",
				"Show all",
				"-",
				"Quit SilkTerm",
			]
		);
		for menu in &menus {
			assert_ne!(
				menu.items.first(),
				Some(&BarItem::Separator),
				"{}",
				menu.title
			);
			assert_ne!(
				menu.items.last(),
				Some(&BarItem::Separator),
				"{}",
				menu.title
			);
			assert!(
				!menu
					.items
					.windows(2)
					.any(|pair| pair.iter().all(|item| *item == BarItem::Separator)),
				"{}",
				menu.title
			);
		}
	}

	// Off since the menu rows take Apple's standard Command chords (20261002),
	// so the bar binds more than the app menu's keys.
	// `the_mac_menu_bar_shows_the_command_chords` covers it.
	// // Command+, is Settings, and only the standard app menu rows take a key, so
	// // the bar adds no other chord.
	// // Test ID: ErUnDRE
	// #[test]
	// fn command_comma_is_settings_and_the_bar_binds_nothing_else() {
	// 	let menus = layout(&window_menus());
	// 	let (title, settings) = find(&menus, MenuAction::Settings).expect("Settings");
	// 	assert_eq!(title, "SilkTerm");
	// 	assert!(matches!(
	// 		settings,
	// 		BarItem::Action {
	// 			key: Some(KeyEquivalent {
	// 				key: ",",
	// 				option: false
	// 			}),
	// 			..
	// 		}
	// 	));
	// 	let mut keys = Vec::new();
	// 	for menu in &menus {
	// 		for item in &menu.items {
	// 			match item {
	// 				BarItem::Action {
	// 					label,
	// 					key: Some(key),
	// 					..
	// 				}
	// 				| BarItem::System {
	// 					label,
	// 					key: Some(key),
	// 					..
	// 				} => keys.push((label.as_str(), key.key, key.option)),
	// 				_ => {}
	// 			}
	// 		}
	// 	}
	// 	assert_eq!(
	// 		keys,
	// 		[
	// 			("Settings\u{2026}", ",", false),
	// 			("Hide SilkTerm", "h", false),
	// 			("Hide others", "h", true),
	// 			("Quit SilkTerm", "q", false),
	// 		]
	// 	);
	// }

	// Off since a Mac has no in-window bar (20261002): View > Menu bar is gone
	// from the system menu bar, and File gains New window.
	// `the_mac_menu_bar_reaches_every_window_row_but_its_toggle` covers it.
	// // The in-window bar starts hidden on a Mac, which is only fair while the
	// // menu bar reaches every one of its rows.
	// // Test ID: ErUnDUL
	// #[test]
	// fn the_mac_menu_bar_reaches_every_row_of_the_window_menus() {
	// 	fn entry_actions(entries: &[Entry], out: &mut Vec<MenuAction>) {
	// 		for entry in entries {
	// 			match entry {
	// 				Entry::Item { action, .. } => out.push(*action),
	// 				Entry::Sub { items, .. } => entry_actions(items, out),
	// 				Entry::Sep => {}
	// 			}
	// 		}
	// 	}
	// 	let window = window_menus();
	// 	let mut want = Vec::new();
	// 	for (_, entries) in &window {
	// 		entry_actions(entries, &mut want);
	// 	}
	// 	assert!(want.iter().any(|a| matches!(a, MenuAction::NewTabShell(_))));
	// 	let menus = layout(&window);
	// 	let mut have = Vec::new();
	// 	for menu in &menus {
	// 		actions(&menu.items, &mut have);
	// 	}
	// 	for action in &want {
	// 		assert!(have.contains(action), "{action:?} is not on the menu bar");
	// 	}
	// 	// and nothing twice, so the app menu's three left their old homes
	// 	for (i, action) in have.iter().enumerate() {
	// 		assert!(!have[i + 1..].contains(action), "{action:?} twice");
	// 	}
	// 	// the check marks come across
	// 	let (_, row) = find(&menus, MenuAction::ToggleMinimap).expect("Minimap");
	// 	assert!(matches!(row, BarItem::Action { check: Some(_), .. }));
	// }

	fn keys(items: &[BarItem], out: &mut Vec<(String, Chord)>) {
		for item in items {
			match item {
				BarItem::Action {
					label,
					key: Some(key),
					..
				}
				| BarItem::System {
					label,
					key: Some(key),
					..
				} => out.push((label.clone(), *key)),
				BarItem::Submenu { items, .. } => keys(items, out),
				_ => {}
			}
		}
	}

	// A shell whose name ends in parentheses keeps them. The bar used to cut
	// anything in a trailing pair as though it were a shortcut, so "Bash (no
	// rc)" read as "Bash", twice over beside the ordinary one.
	// Test ID: ErkT4M3
	#[test]
	fn a_shell_name_in_the_mac_menu_bar_keeps_its_parentheses() {
		fn titles(items: &[BarItem], out: &mut Vec<String>) {
			for item in items {
				match item {
					BarItem::Action {
						label,
						action: MenuAction::NewTabShell(_) | MenuAction::SplitShell(..),
						..
					} => out.push(label.clone()),
					BarItem::Submenu { items, .. } => titles(items, out),
					_ => {}
				}
			}
		}
		let window = crate::app::sample_window_menus_with(true, true, &["Bash", "Bash (no rc)"]);
		let menus = layout(window, &Bindings::defaults(true));
		let mut shown = Vec::new();
		for menu in &menus {
			titles(&menu.items, &mut shown);
		}
		assert!(!shown.is_empty());
		for pair in shown.chunks(2) {
			assert_eq!(pair, ["Bash", "Bash (no rc)"], "{shown:?}");
		}
	}

	// Each row with an Apple standard shortcut shows its Command chord, drawn by
	// the menu bar beside a plain label. Every chord the key bindings answer to
	// is on a row, and no row carries a chord they do not.
	// Test ID: ErZrSlO
	#[test]
	fn the_mac_menu_bar_shows_the_command_chords() {
		fn labels(items: &[BarItem]) {
			for item in items {
				match item {
					BarItem::Action { label, .. } | BarItem::System { label, .. } => {
						assert!(!label.contains('('), "{label}");
					}
					BarItem::Submenu { items, .. } => labels(items),
					BarItem::Separator => {}
				}
			}
		}
		let menus = layout(window_menus(), &Bindings::defaults(true));
		let mut have = Vec::new();
		for menu in &menus {
			keys(&menu.items, &mut have);
		}
		let shown: Vec<(&str, String)> = have
			.iter()
			.map(|(label, key)| (label.as_str(), key.spoken(true)))
			.collect();
		assert_eq!(
			shown,
			[
				("Settings\u{2026}", "Command+,".to_string()),
				("Hide SilkTerm", "Command+H".to_string()),
				("Hide others", "Option+Command+H".to_string()),
				("Quit SilkTerm", "Command+Q".to_string()),
				("New window", "Command+N".to_string()),
				("Copy", "Command+C".to_string()),
				("Paste", "Command+V".to_string()),
				("Increase font size", "Command+Plus".to_string()),
				("Decrease font size", "Command+Minus".to_string()),
				("Reset font size", "Command+0".to_string()),
				("Fullscreen", "Control+Command+F".to_string()),
				("New tab", "Command+T".to_string()),
				("Close tab", "Command+W".to_string()),
				("Split vertical", "Command+D".to_string()),
				("Split horizontal", "Shift+Command+D".to_string()),
				("Close pane", "Option+Command+W".to_string()),
				("Minimize", "Command+M".to_string()),
				("Show previous tab", "Shift+Command+[".to_string()),
				("Show next tab", "Shift+Command+]".to_string()),
			]
		);
		// every hotkey with a row shows the chord it answers to first; the focus
		// moves, the tab carrying and the Menu key have no row
		let bound = Bindings::defaults(true);
		for (hotkey, _) in crate::keys::config_paths() {
			use crate::input::Hotkey;
			if matches!(
				hotkey,
				Hotkey::Focus(_) | Hotkey::MoveTab { .. } | Hotkey::ContextMenu
			) {
				continue;
			}
			let Some(chord) = bound.shown(hotkey) else {
				continue;
			};
			assert!(
				have.iter().any(|(_, key)| *key == chord),
				"{hotkey:?} has no row"
			);
		}
		for menu in &menus {
			labels(&menu.items);
		}
	}

	// A Mac has no in-window bar, so the system one reaches every row of the
	// in-window menus but the toggle that would bring that bar back. The copy
	// modes, shown at the right of the in-window bar elsewhere, are check rows on
	// Edit that follow the focused pane.
	// Test ID: ErZrT4e
	#[test]
	fn the_mac_menu_bar_reaches_every_window_row_but_its_toggle() {
		fn entry_actions(entries: &[Entry], out: &mut Vec<MenuAction>) {
			for entry in entries {
				match entry {
					Entry::Item { action, .. } => out.push(*action),
					Entry::Sub { items, .. } => entry_actions(items, out),
					Entry::Sep => {}
				}
			}
		}
		let window = window_menus();
		let mut want = Vec::new();
		for (_, entries) in &window {
			entry_actions(entries, &mut want);
		}
		assert!(want.contains(&MenuAction::ToggleMenuBar));
		assert!(want.iter().any(|a| matches!(a, MenuAction::NewTabShell(_))));
		let menus = layout(window, &Bindings::defaults(true));
		let mut have = Vec::new();
		for menu in &menus {
			actions(&menu.items, &mut have);
		}
		for action in &want {
			if *action == MenuAction::ToggleMenuBar {
				assert!(!have.contains(action), "the in-window bar has a way back");
			} else {
				assert!(have.contains(action), "{action:?} is not on the menu bar");
			}
		}
		for (i, action) in have.iter().enumerate() {
			assert!(!have[i + 1..].contains(action), "{action:?} twice");
		}
		let file = menus.iter().find(|m| m.title == "File").expect("File");
		assert!(matches!(
			file.items.first(),
			Some(BarItem::Action {
				action: MenuAction::NewWindow,
				..
			})
		));
		let (_, row) = find(&menus, MenuAction::ToggleMinimap).expect("Minimap");
		assert!(matches!(row, BarItem::Action { check: Some(_), .. }));
		for copy_select in [false, true] {
			for copy_output in [false, true] {
				let menus = layout(
					crate::app::sample_window_menus_copying(copy_select, copy_output),
					&Bindings::defaults(true),
				);
				for (action, on) in [
					(MenuAction::ToggleCopySelect, copy_select),
					(MenuAction::ToggleCopyOutput, copy_output),
				] {
					let (title, row) = find(&menus, action).expect("copy mode row");
					assert_eq!(title, "Edit");
					assert!(
						matches!(row, BarItem::Action { check: Some(c), .. } if *c == on),
						"{action:?}"
					);
				}
			}
		}
		for menu in &menus {
			assert_ne!(menu.items.first(), Some(&BarItem::Separator));
			assert_ne!(menu.items.last(), Some(&BarItem::Separator));
		}
	}

	// A chord moved in the config file goes with it to the menu bar row, and one
	// without Command stays off the bar, since AppKit would take the press from
	// the shell.
	// Test ID: EreU3sf
	#[test]
	fn the_mac_menu_bar_follows_the_bindings() {
		use crate::input::Hotkey;
		let chord = |text| Chord::parse(text).expect(text);
		let (keys, _) = Bindings::with(
			true,
			&[
				(Hotkey::SplitRight, vec![chord("Option+Command+R")]),
				(Hotkey::ClosePane, vec![chord("Shift+Command+W")]),
				(Hotkey::Copy, vec![chord("Control+Option+C")]),
			],
		);
		let menus = layout(window_menus(), &keys);
		let key_of = |want| match find(&menus, want) {
			Some((_, BarItem::Action { key, .. })) => key.map(|k| k.spoken(true)),
			_ => panic!("{want:?} has no row"),
		};
		assert_eq!(
			key_of(MenuAction::SplitVertical).as_deref(),
			Some("Option+Command+R")
		);
		assert_eq!(
			key_of(MenuAction::Close).as_deref(),
			Some("Shift+Command+W")
		);
		assert_eq!(key_of(MenuAction::Copy), None);
		assert_eq!(
			key_of(MenuAction::SplitHorizontal).as_deref(),
			Some("Shift+Command+D")
		);
		let defaults = layout(window_menus(), &Bindings::defaults(true));
		assert_ne!(defaults, menus);
	}

	// The Window menu has Apple's rows, with SilkTerm's tabs where a Mac app has
	// its window tabs, and comes last so the window list goes in it. A
	// Command+Shift chord on a bracket is handed over as the shifted
	// character, the form AppKit matches.
	// Test ID: ErbGPNM
	#[test]
	fn the_mac_window_menu_minimizes_zooms_and_walks_the_tabs() {
		let menus = layout(window_menus(), &Bindings::defaults(true));
		let window = menus.last().expect("menus");
		assert_eq!(window.title, WINDOW_MENU);
		let rows: Vec<String> = window
			.items
			.iter()
			.map(|item| match item {
				BarItem::Action { label, key, .. } | BarItem::System { label, key, .. } => {
					match key {
						Some(key) => format!("{label} {}", key.spoken(true)),
						None => label.clone(),
					}
				}
				BarItem::Submenu { label, .. } => format!("{label} >"),
				BarItem::Separator => "-".into(),
			})
			.collect();
		assert_eq!(
			rows,
			[
				"Minimize Command+M",
				"Zoom",
				"-",
				"Show previous tab Shift+Command+[",
				"Show next tab Shift+Command+]",
				"-",
				"Bring all to front",
			]
		);
		for (action, hotkey) in [
			(MenuAction::PrevTab, crate::input::Hotkey::PrevTab),
			(MenuAction::NextTab, crate::input::Hotkey::NextTab),
		] {
			assert_eq!(crate::app::menu_hotkey(action), Some(hotkey));
			assert_eq!(find(&menus, action).map(|(title, _)| title), Some("Window"));
		}
		let bound = Bindings::defaults(true);
		let previous = bound.shown(crate::input::Hotkey::PrevTab).expect("chord");
		let (text, handed) = key_equivalent(previous).expect("a key");
		assert_eq!((text.as_str(), handed.shift), ("{", false));
		let next = bound.shown(crate::input::Hotkey::NextTab).expect("chord");
		let (text, handed) = key_equivalent(next).expect("a key");
		assert_eq!((text.as_str(), handed.shift), ("}", false));
		for (chord, text) in [
			(command('c'), "c"),
			(
				Chord {
					shift: true,
					..command('t')
				},
				"t",
			),
			(
				Chord {
					alt: true,
					..command('h')
				},
				"h",
			),
		] {
			assert_eq!(key_equivalent(chord), Some((text.to_string(), chord)));
		}
	}
}
