// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The macOS menu bar. Its layout is plain data built from the same entry lists
// the in-window menus draw, so it is checked on every platform; only `native`
// talks to AppKit.

use crate::app::{Entry, MenuAction};

pub const APP_NAME: &str = "SilkTerm";

/// A menu key equivalent. Command is always held; `option` adds Option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEquivalent {
	pub key: &'static str,
	pub option: bool,
}

const fn command(key: &'static str) -> KeyEquivalent {
	KeyEquivalent { key, option: false }
}

/// App menu rows `AppKit` carries out by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemItem {
	Services,
	Hide,
	HideOthers,
	ShowAll,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BarItem {
	Action {
		label: String,
		action: MenuAction,
		check: Option<bool>,
		key: Option<KeyEquivalent>,
	},
	System {
		label: String,
		item: SystemItem,
		key: Option<KeyEquivalent>,
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

fn app_menu() -> BarMenu {
	let action = |label: String, action, key| BarItem::Action {
		label,
		action,
		check: None,
		key,
	};
	let system = |label: &str, item, key| BarItem::System {
		label: label.into(),
		item,
		key,
	};
	BarMenu {
		title: APP_NAME.into(),
		items: vec![
			action(format!("About {APP_NAME}"), MenuAction::About, None),
			BarItem::Separator,
			action(
				"Settings\u{2026}".into(),
				MenuAction::Settings,
				Some(command(",")),
			),
			BarItem::Separator,
			system("Services", SystemItem::Services, None),
			BarItem::Separator,
			system(
				&format!("Hide {APP_NAME}"),
				SystemItem::Hide,
				Some(command("h")),
			),
			system(
				"Hide others",
				SystemItem::HideOthers,
				Some(KeyEquivalent {
					key: "h",
					option: true,
				}),
			),
			system("Show all", SystemItem::ShowAll, None),
			BarItem::Separator,
			action(
				format!("Quit {APP_NAME}"),
				MenuAction::Quit,
				Some(command("q")),
			),
		],
	}
}

fn bar_items(entries: &[Entry]) -> Vec<BarItem> {
	let items = entries
		.iter()
		.filter_map(|entry| match entry {
			Entry::Item { action, .. } if in_app_menu(*action) => None,
			Entry::Item {
				label,
				action,
				check,
				..
			} => Some(BarItem::Action {
				label: label.clone(),
				action: *action,
				check: *check,
				key: None,
			}),
			Entry::Sub { label, items, .. } => Some(BarItem::Submenu {
				label: label.clone(),
				items: bar_items(items),
			}),
			Entry::Sep => Some(BarItem::Separator),
		})
		.collect();
	tidy(items)
}

// A row that moved to the app menu can leave a separator with nothing on one
// side of it.
fn tidy(items: Vec<BarItem>) -> Vec<BarItem> {
	let mut out: Vec<BarItem> = Vec::with_capacity(items.len());
	for item in items {
		let sep = item == BarItem::Separator;
		if sep && out.last().is_none_or(|last| *last == BarItem::Separator) {
			continue;
		}
		out.push(item);
	}
	if out.last() == Some(&BarItem::Separator) {
		out.pop();
	}
	out
}

/// The whole menu bar: the app menu, then each in-window menu by its title, less
/// the rows the app menu took. A menu left with nothing in it is dropped.
pub fn layout(window_menus: &[(&str, Vec<Entry>)]) -> Vec<BarMenu> {
	let mut menus = vec![app_menu()];
	for (title, entries) in window_menus {
		let items = bar_items(entries);
		if !items.is_empty() {
			menus.push(BarMenu {
				title: (*title).into(),
				items,
			});
		}
	}
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

	use super::{BarItem, BarMenu, KeyEquivalent, SystemItem};
	use crate::app::MenuAction;
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
				// no target: these go up the responder chain to NSApplication
				BarItem::System { label, item, key } => {
					let action = match item {
						SystemItem::Services => None,
						SystemItem::Hide => Some(sel!(hide:)),
						SystemItem::HideOthers => Some(sel!(hideOtherApplications:)),
						SystemItem::ShowAll => Some(sel!(unhideAllApplications:)),
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
		key: Option<KeyEquivalent>,
	) -> Retained<NSMenuItem> {
		let key_text = NSString::from_str(key.map_or("", |k| k.key));
		// SAFETY: every selector passed here is one AppKit or MenuTarget answers.
		let row = unsafe {
			NSMenuItem::initWithTitle_action_keyEquivalent(
				NSMenuItem::alloc(mtm),
				&NSString::from_str(label),
				action,
				&key_text,
			)
		};
		if key.is_some_and(|k| k.option) {
			row.setKeyEquivalentModifierMask(
				NSEventModifierFlags::Command | NSEventModifierFlags::Option,
			);
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
		let menus = layout(&window_menus());
		let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
		// Help held only About, which moved to the app menu
		assert_eq!(
			titles,
			["SilkTerm", "File", "Edit", "View", "Tabs", "Panes"]
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

	// Command+, is Settings, and only the standard app menu rows take a key, so
	// the bar adds no other chord.
	// Test ID: ErUnDRE
	#[test]
	fn command_comma_is_settings_and_the_bar_binds_nothing_else() {
		let menus = layout(&window_menus());
		let (title, settings) = find(&menus, MenuAction::Settings).expect("Settings");
		assert_eq!(title, "SilkTerm");
		assert!(matches!(
			settings,
			BarItem::Action {
				key: Some(KeyEquivalent {
					key: ",",
					option: false
				}),
				..
			}
		));
		let mut keys = Vec::new();
		for menu in &menus {
			for item in &menu.items {
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
					} => keys.push((label.as_str(), key.key, key.option)),
					_ => {}
				}
			}
		}
		assert_eq!(
			keys,
			[
				("Settings\u{2026}", ",", false),
				("Hide SilkTerm", "h", false),
				("Hide others", "h", true),
				("Quit SilkTerm", "q", false),
			]
		);
	}

	// The in-window bar starts hidden on a Mac, which is only fair while the
	// menu bar reaches every one of its rows.
	// Test ID: ErUnDUL
	#[test]
	fn the_mac_menu_bar_reaches_every_row_of_the_window_menus() {
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
		assert!(want.iter().any(|a| matches!(a, MenuAction::NewTabShell(_))));
		let menus = layout(&window);
		let mut have = Vec::new();
		for menu in &menus {
			actions(&menu.items, &mut have);
		}
		for action in &want {
			assert!(have.contains(action), "{action:?} is not on the menu bar");
		}
		// and nothing twice, so the app menu's three left their old homes
		for (i, action) in have.iter().enumerate() {
			assert!(!have[i + 1..].contains(action), "{action:?} twice");
		}
		// the check marks come across
		let (_, row) = find(&menus, MenuAction::ToggleMinimap).expect("Minimap");
		assert!(matches!(row, BarItem::Action { check: Some(_), .. }));
	}
}
