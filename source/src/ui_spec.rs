// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The Settings dialog's declarations, read from `settings_ui.shcl` (compiled
//! in). The file owns what the dialog IS - rows, order, sections, tabs, the
//! config path behind each row, when a row grays out, and the geometry.
//! The `settings_ui` module owns what it DOES.
//!
//! The document is constant, so it is parsed once and handed out as `'static`.
//! Anything wrong with it is a build-time mistake rather than a user's, and
//! `the_declarations_are_complete_and_well_formed` fails on all of it -
//! including the one class no parser can see, a row that is simply absent.

use std::sync::OnceLock;

// Every setting the dialog can address. Declared here so exhaustive matches
// still fail to compile when one is added or removed; the file is held against
// `Key::ALL` by test instead, which is the only way to catch an omission.
macro_rules! keys {
	($($name:ident),* $(,)?) => {
		#[derive(Clone, Copy, PartialEq, Eq, Debug)]
		pub enum Key {
			None, // headings, and a row that carries two settings rather than one
			$($name),*
		}
		impl Key {
			// The roll call the completeness test reads the document against,
			// and the spelling it names a missing one by.
			#[cfg(test)]
			pub const ALL: &'static [Key] = &[$(Key::$name),*];
			#[cfg(test)]
			pub fn name(self) -> &'static str {
				match self {
					Key::None => "none",
					$(Key::$name => stringify!($name)),*
				}
			}
			fn parse(text: &str) -> Option<Key> {
				if text.eq_ignore_ascii_case("none") {
					return Some(Key::None);
				}
				$(if text.eq_ignore_ascii_case(stringify!($name)) {
					return Some(Key::$name);
				})*
				None
			}
		}
	};
}

#[rustfmt::skip]
keys![
	PerfAuto, PerfProfile, PerfCheckHardware, PerfCheckNext,
	Transparency, Opacity, BackdropBlur,
	BgEnabled, BgRotate, BgOpacity, BgBlur, BgFit, BgHonorXmp, BgHonorXmpLook, BgImage,
	BgContrastMask, BgContrastSize, BgContrastStrength, BgContrastAuto,
	TextScrim, ScrimRadius, ScrimSoftness, ScrimStrength, ScrimFunction, ScrimRamp,
	Outline, MinContrast, CursorScrim, CursorOutline,
	CursorBlink, CursorHeight, CursorWidth, CursorAnimation, CursorResume,
	SystemFont, SystemFontSize, FontFamily, FontSize, LineHeight,
	Columns, Rows, RememberSize, RememberPerMonitor, RememberMaximized, Margin, TabRegularWidth, TabMaxWidth,
	NewTabNextToCurrent, IdleRelease, IdleHiddenMin, IdleMin, SoftwareRendering,
	TabShowsTitle, TabShowsShell, TabShowsProgram, TabShowsDirectory, TitleShowsTab,
	Shells, StartupDirectory, ShellIntegration, BashPrompt, CopyOnSelect, Hyperlinks, LinkOpenCommand,
	SmoothScroll, ScrollEaseIn, ScrollRampUp, SingleScreenTau, ScrollRampDown,
	ScrollEaseOut, WheelLines,
	Scrollbar, ScrollbarThickness, ScrollbarAutoHide, Minimap, MinimapWidth,
	ColScrollbarThumb, ColScrollbarTrough, ColBg, ColFromWallpaper, ColFg, ColCursor,
	ColHighlight, ColFocus, ColGutter,
	ColMenuBg, ColMenuFg, ColDialogBg, ColDialogFg,
	Theme, ThemeMode, ThemeActions,
	OpenBatch, OpenPowerShell, OpenVbScript, OpenFolder,
	HotkeyNewWindow, HotkeySettings, HotkeyQuit, HotkeyCopy, HotkeyPaste,
	HotkeyFontBigger, HotkeyFontSmaller, HotkeyFontReset, HotkeyFullscreen, HotkeyContextMenu,
	HotkeyNewTab, HotkeyCloseTab, HotkeyPrevTab, HotkeyNextTab, HotkeyMoveTabBack, HotkeyMoveTabForward,
	HotkeySplitRight, HotkeySplitDown, HotkeyClosePane,
	HotkeyFocusLeft, HotkeyFocusRight, HotkeyFocusUp, HotkeyFocusDown,
];

#[derive(Debug)]
pub enum Kind {
	Slider {
		min: f32,
		max: f32,
		int: bool,
	},
	Color,
	Text,   // free-text field (path / font family; empty = default)
	Toggle, // checkbox (e.g. use system font)
	// two labeled checkboxes on one row sharing the row label + revert (e.g.
	// Cursor: Scrim / Outline); each checkbox is a separate focus stop
	Dual {
		keys: [Key; 2],
		labels: [&'static str; 2],
	},
	Radio(&'static [&'static str]), // pick one of N mutually-exclusive options
	Dropdown(&'static [&'static str]), // one-of-N via a collapsed box + popup list
	// a row of push-buttons that act on the row above rather than holding a value
	// (Save / Save as / Rename / Delete); each is its own focus stop
	Buttons(&'static [&'static str]),
	// The shells grid: one line per stored shell (name, command, last seen,
	// active, and the buttons that move or remove it), plus an Add button. It is
	// one row here and many on screen, so its height follows the list's length
	// rather than the row metrics - see `SettingsDialog::row_h_for`.
	ShellList,
	// One hotkey's chords, set by pressing the new ones. The hotkey is the one
	// the row's `setting:` binds under `keys:`.
	Hotkey(crate::input::Hotkey),
	Header(&'static str), // a section heading, no control
}

#[derive(Debug)]
pub struct Spec {
	pub label: &'static str,
	pub key: Key,
	pub kind: Kind,
	pub tab: usize,
	/// Flyover text for a control whose purpose is not obvious from its label.
	/// Empty means the row explains itself and gets no tip.
	pub help: &'static str,
	/// Sub-group depth. Only the LABEL moves; the controls stay in their column,
	/// so a run of indented labels reads as belonging to the row above it. A
	/// sub-group is therefore not declared anywhere - it is the leader's own
	/// depth plus everything deeper that follows.
	pub indent: u8,
	/// Drawn on the same line as the row above rather than under it. A line of
	/// N rows splits the control column into N even parts; the first row keeps
	/// the label column and its label has to name the line, unless every part
	/// names itself. This row's own label, if it has one, comes before its box,
	/// the way the first row's does.
	pub beside: bool,
	/// Flyover for the revert arrow, where it does something other than put the
	/// shipped default back.
	pub revert_help: &'static str,
	/// Only in the Windows build. Test builds keep it everywhere, so its layout
	/// and behavior are tested on every platform.
	pub windows: bool,
	/// Left out of the macOS build, for a row that can never work there. Test
	/// builds keep it, as with `windows`.
	pub not_macos: bool,
	/// Draws a warning mark after the label, for a row that may not work
	/// everywhere. Empty means no mark. `pick_warnings` puts the text on the
	/// end of `help`, since the row's one tip answers over the mark too.
	pub warning: &'static str,
	/// The mark's text in the Windows build, in place of `warning`. Moved
	/// over by `pick_warnings`, so the dialog only ever reads `warning`.
	pub windows_warning: &'static str,
}

/// One setting a control has to wait on, resolved from the file's gate lines.
/// `numeric` is decided here rather than at every check: a slider is satisfied
/// while it sits above zero, everything else while it is switched on.
#[derive(Debug)]
pub struct Need {
	pub key: Key,
	pub invert: bool,
	pub numeric: bool,
}

#[derive(Debug)]
pub struct Layout {
	pub width: f32,
	pub pad: f32,
	pub tabs_gap: f32,
	pub buttons_gap: f32,
	pub row_height: f32,
	pub row_pad: f32,
	pub header_height: f32,
	pub header_pad: f32,
	pub header_gap: f32,
	pub subgroup_gap: f32,
	pub indent: f32,
	pub label_width: f32,
	pub label_gap: f32,
	pub revert_width: f32,
	pub slider_width: f32,
	pub swatch: f32,
	pub hex_width: f32,
	pub value_width: f32,
	pub radio_box: f32,
	pub radio_pitch: f32,
	pub dual_pitch: f32,
	pub dropdown_width: f32,
	pub dropdown_pair_width: f32,
	pub dropdown_item_pad: f32,
	pub dropdown_item_min: f32,
	pub base_line_height: f32,
	pub field_height: f32,
	pub field_pad: f32,
	pub field_pad_v: f32,
	pub caret_pad: f32,
	pub view_ahead: f32,
	pub edit_menu_width: f32,
	pub button_height: f32,
	pub button_pad: f32,
	pub button_width: f32,
	pub button_gap: f32,
	pub shell_name_width: f32,
	pub shell_command_width: f32,
	pub shell_seen_width: f32,
	pub shell_active_width: f32,
	pub shell_col_gap: f32,
	pub shell_grip: f32,
	pub shell_button: f32,
	pub shell_head_gap: f32,
	pub shell_add_gap: f32,
	pub tab_pad: f32,
	pub tab_height: f32,
	pub tab_pad_v: f32,
	pub tab_top: f32,
	pub tab_gap: f32,
	pub scrollbar_width: f32,
	pub scrollbar_inset: f32,
	pub scrollbar_thumb_min: f32,
	pub pick_gap: f32,
	pub pick_strip: f32,
	pub pick_label_width: f32,
	pub pick_field_width: f32,
	pub pick_min_square: f32,
	pub pick_marker: f32,
}

/// Flyover text for the footer buttons, which are chrome rather than settings and
/// so have no row of their own to carry it.
#[derive(Debug)]
pub struct Help {
	pub cancel: &'static str,
	pub apply: &'static str,
	pub ok: &'static str,
}

#[derive(Debug)]
pub struct Icons {
	pub dropdown_arrow: &'static str,
	pub dropdown_check: &'static str,
	pub revert: &'static str,
}

#[derive(Debug)]
pub struct Ui {
	pub tabs: Vec<&'static str>,
	pub layout: Layout,
	pub icons: Icons,
	pub help: Help,
	pub specs: Vec<Spec>,
	// config path per addressable setting, in row order (parallel to `keys`)
	settings: Vec<(Key, &'static [&'static str])>,
	gates: Vec<(Key, Vec<Need>)>,
}

impl Ui {
	/// Config path(s) behind a setting, for revert's comment-out. Empty for a
	/// heading, or for a row that carries no setting of its own.
	pub fn settings_of(&self, key: Key) -> &'static [&'static str] {
		self.settings
			.iter()
			.find(|(k, _)| *k == key)
			.map_or(&[][..], |(_, paths)| *paths)
	}
	pub fn needs_of(&self, key: Key) -> &[Need] {
		self.gates
			.iter()
			.find(|(k, _)| *k == key)
			.map_or(&[][..], |(_, needs)| needs)
	}
}

const SOURCE: &str = include_str!("settings_ui.shcl");

pub fn ui() -> &'static Ui {
	static CELL: OnceLock<Ui> = OnceLock::new();
	CELL.get_or_init(|| match parse(SOURCE) {
		Ok(mut ui) => {
			keep_platform(
				&mut ui.specs,
				cfg!(any(windows, test)),
				cfg!(all(target_os = "macos", not(test))),
			);
			// by the real platform, test or not: a mark that is wrong for the
			// box it shows on is worse than none
			pick_warnings(&mut ui.specs, cfg!(windows));
			ui
		}
		// Unreachable in a tested build: the document is compiled in, so it
		// cannot vary at runtime and the test below reads the same bytes.
		Err(problems) => panic!("settings_ui.shcl: {}", problems.join("; ")),
	})
}

fn keep_platform(specs: &mut Vec<Spec>, windows: bool, macos: bool) {
	specs.retain(|spec| (windows || !spec.windows) && !(macos && spec.not_macos));
}

// A row has one tip, shown over its label, its mark and its controls alike, so
// the mark's text goes on the end of it (2026100812334388).
fn pick_warnings(specs: &mut [Spec], windows: bool) {
	for spec in specs {
		if windows && !spec.windows_warning.is_empty() {
			spec.warning = spec.windows_warning;
		}
		spec.windows_warning = "";
		spec.help = match (spec.help, spec.warning) {
			(help, "") => help,
			("", warning) => warning,
			(help, warning) => keep(format!("{help} {warning}")),
		};
	}
}

// A parsed string lives as long as the process; there is exactly one document
// and it is read once, so leaking it is cheaper than threading a lifetime
// through every signature in the dialog.
fn keep(text: String) -> &'static str {
	String::leak(text)
}
fn keep_all(items: Vec<String>) -> &'static [&'static str] {
	Vec::leak(items.into_iter().map(keep).collect::<Vec<_>>())
}

#[allow(clippy::too_many_lines)] // one straight-line read of one document
fn parse(text: &str) -> Result<Ui, Vec<String>> {
	let doc = shcl::Document::parse(text);
	let mut problems: Vec<String> = doc
		.diagnostics()
		.iter()
		.filter(|d| d.severity == shcl::Severity::Error)
		.map(|d| format!("line {}: {}", d.line, d.message))
		.collect();

	let float = |path: &str, problems: &mut Vec<String>| -> f32 {
		match doc.get_float(path) {
			Ok(v) => v as f32,
			Err(status) => {
				problems.push(format!("{path}: {status:?}"));
				0.0
			}
		}
	};
	let layout = Layout {
		width: float("layout.width", &mut problems),
		pad: float("layout.pad", &mut problems),
		tabs_gap: float("layout.tabs_gap", &mut problems),
		buttons_gap: float("layout.buttons_gap", &mut problems),
		row_height: float("layout.row_height", &mut problems),
		row_pad: float("layout.row_pad", &mut problems),
		header_height: float("layout.header_height", &mut problems),
		header_pad: float("layout.header_pad", &mut problems),
		header_gap: float("layout.header_gap", &mut problems),
		subgroup_gap: float("layout.subgroup_gap", &mut problems),
		indent: float("layout.indent", &mut problems),
		label_width: float("layout.label_width", &mut problems),
		label_gap: float("layout.label_gap", &mut problems),
		revert_width: float("layout.revert_width", &mut problems),
		slider_width: float("layout.slider_width", &mut problems),
		swatch: float("layout.swatch", &mut problems),
		hex_width: float("layout.hex_width", &mut problems),
		value_width: float("layout.value_width", &mut problems),
		radio_box: float("layout.radio_box", &mut problems),
		radio_pitch: float("layout.radio_pitch", &mut problems),
		dual_pitch: float("layout.dual_pitch", &mut problems),
		dropdown_width: float("layout.dropdown_width", &mut problems),
		dropdown_pair_width: float("layout.dropdown_pair_width", &mut problems),
		dropdown_item_pad: float("layout.dropdown_item_pad", &mut problems),
		dropdown_item_min: float("layout.dropdown_item_min", &mut problems),
		base_line_height: float("layout.base_line_height", &mut problems),
		field_height: float("layout.field_height", &mut problems),
		field_pad: float("layout.field_pad", &mut problems),
		field_pad_v: float("layout.field_pad_v", &mut problems),
		caret_pad: float("layout.caret_pad", &mut problems),
		view_ahead: float("layout.view_ahead", &mut problems),
		edit_menu_width: float("layout.edit_menu_width", &mut problems),
		button_height: float("layout.button_height", &mut problems),
		button_pad: float("layout.button_pad", &mut problems),
		button_width: float("layout.button_width", &mut problems),
		button_gap: float("layout.button_gap", &mut problems),
		shell_name_width: float("layout.shell_name_width", &mut problems),
		shell_command_width: float("layout.shell_command_width", &mut problems),
		shell_seen_width: float("layout.shell_seen_width", &mut problems),
		shell_active_width: float("layout.shell_active_width", &mut problems),
		shell_col_gap: float("layout.shell_col_gap", &mut problems),
		shell_grip: float("layout.shell_grip", &mut problems),
		shell_button: float("layout.shell_button", &mut problems),
		shell_head_gap: float("layout.shell_head_gap", &mut problems),
		shell_add_gap: float("layout.shell_add_gap", &mut problems),
		tab_pad: float("layout.tab_pad", &mut problems),
		tab_height: float("layout.tab_height", &mut problems),
		tab_pad_v: float("layout.tab_pad_v", &mut problems),
		tab_top: float("layout.tab_top", &mut problems),
		tab_gap: float("layout.tab_gap", &mut problems),
		scrollbar_width: float("layout.scrollbar_width", &mut problems),
		scrollbar_inset: float("layout.scrollbar_inset", &mut problems),
		scrollbar_thumb_min: float("layout.scrollbar_thumb_min", &mut problems),
		pick_gap: float("layout.pick_gap", &mut problems),
		pick_strip: float("layout.pick_strip", &mut problems),
		pick_label_width: float("layout.pick_label_width", &mut problems),
		pick_field_width: float("layout.pick_field_width", &mut problems),
		pick_min_square: float("layout.pick_min_square", &mut problems),
		pick_marker: float("layout.pick_marker", &mut problems),
	};
	let glyph = |path: &str, problems: &mut Vec<String>| -> &'static str {
		match doc.get_string(path) {
			Ok(v) => keep(v),
			Err(status) => {
				problems.push(format!("{path}: {status:?}"));
				"?"
			}
		}
	};
	let help = Help {
		cancel: glyph("help.cancel", &mut problems),
		apply: glyph("help.apply", &mut problems),
		ok: glyph("help.ok", &mut problems),
	};
	let icons = Icons {
		dropdown_arrow: glyph("icons.dropdown_arrow", &mut problems),
		dropdown_check: glyph("icons.dropdown_check", &mut problems),
		revert: glyph("icons.revert", &mut problems),
	};

	let tabs: Vec<&'static str> = doc
		.get_string_array("tabs")
		.map(keep_all)
		.unwrap_or_default()
		.to_vec();
	if tabs.is_empty() {
		problems.push("tabs: no tab titles".into());
	}

	let mut specs: Vec<Spec> = Vec::new();
	let mut settings: Vec<(Key, &'static [&'static str])> = Vec::new();
	let mut tab = 0usize;
	// a Windows-only heading takes its rows with it, or they would show under
	// the group above on every other platform
	let mut group_windows = false;
	for name in doc.children("rows") {
		let at = |field: &str| format!("rows.{name}.{field}");
		let label = doc.get_string(&at("label")).unwrap_or_default();
		let kind_text = doc.get_string(&at("kind")).unwrap_or_default();
		// A heading names its section, not a setting; every other row's name IS
		// its setting unless the row says otherwise.
		let key = if kind_text == "heading" {
			Key::None
		} else {
			let key_text = doc.get_string(&at("key")).unwrap_or_else(|_| name.clone());
			let Some(key) = Key::parse(&key_text) else {
				problems.push(format!("rows.{name}: no setting named {key_text}"));
				continue;
			};
			key
		};
		let paths = doc.get_string_array(&at("setting")).unwrap_or_default();
		let options = doc
			.get_string_array(&at("options"))
			.map_or(&[][..], keep_all);
		let kind = match kind_text.as_str() {
			"heading" => {
				match doc
					.get_string(&at("tab"))
					.ok()
					.and_then(|t| tabs.iter().position(|title| *title == t))
				{
					Some(index) => tab = index,
					None => problems.push(format!("rows.{name}: not one of the tabs")),
				}
				Kind::Header(keep(label.clone()))
			}
			"toggle" => Kind::Toggle,
			"shells" => Kind::ShellList,
			"color" => Kind::Color,
			"text" => Kind::Text,
			"hotkey" => {
				let Some(hotkey) = paths.first().and_then(|path| crate::keys::hotkey_at(path))
				else {
					problems.push(format!("rows.{name}: a hotkey row needs a keys. setting"));
					continue;
				};
				Kind::Hotkey(hotkey)
			}
			"radio" | "dropdown" => {
				// A dropdown whose list is only known at run time (the themes) has
				// no options here; the code fills it, so an empty list is allowed.
				if options.len() < 2 && !(kind_text == "dropdown" && options.is_empty()) {
					problems.push(format!("rows.{name}: {kind_text} needs options"));
				}
				if kind_text == "radio" {
					Kind::Radio(options)
				} else {
					Kind::Dropdown(options)
				}
			}
			"buttons" => {
				if options.is_empty() {
					problems.push(format!("rows.{name}: buttons needs captions"));
				}
				Kind::Buttons(options)
			}
			"slider" => {
				let range = doc.get_float_array(&at("range")).unwrap_or_default();
				if range.len() != 2 || range[0] >= range[1] {
					problems.push(format!("rows.{name}: range must be low, high"));
					continue;
				}
				Kind::Slider {
					min: range[0] as f32,
					max: range[1] as f32,
					int: doc.get_bool(&at("whole")).unwrap_or(false),
				}
			}
			"pair" => {
				let parts: Vec<Key> = doc
					.get_string_array(&at("parts"))
					.unwrap_or_default()
					.iter()
					.filter_map(|p| Key::parse(p))
					.collect();
				if parts.len() != 2 || options.len() != 2 || paths.len() != 2 {
					problems.push(format!(
						"rows.{name}: pair needs 2 parts, options, settings"
					));
					continue;
				}
				for (part, path) in parts.iter().zip(paths.iter()) {
					settings.push((*part, keep_all(vec![path.clone()])));
				}
				Kind::Dual {
					keys: [parts[0], parts[1]],
					labels: [options[0], options[1]],
				}
			}
			other => {
				problems.push(format!("rows.{name}: unknown kind {other}"));
				continue;
			}
		};
		// a pair row already filed its two parts above; neither a buttons row nor
		// the shells grid holds a value of its own
		if key != Key::None
			&& !matches!(kind, Kind::Dual { .. } | Kind::Buttons(_) | Kind::ShellList)
		{
			// A text box may stand for more than one setting, as "File or folder"
			// does, so a revert comments out each. Anything else holds one value.
			match (paths.len(), &kind) {
				(1, _) | (2.., Kind::Text) => settings.push((key, keep_all(paths))),
				_ => problems.push(format!("rows.{name}: needs exactly one setting path")),
			}
		}
		let beside = doc.get_bool(&at("beside")).unwrap_or(false);
		// A shared line needs an ordinary row above it to share, or another row
		// already beside one, and every part has to be something that fits on one
		// line. A row whose height is not a constant (the grid) or that holds no
		// value (a heading) is not.
		let one_line = |kind: &Kind| {
			!matches!(
				kind,
				Kind::Header(_) | Kind::ShellList | Kind::Buttons(_) | Kind::Radio(_)
			)
		};
		if beside {
			if !specs
				.last()
				.is_some_and(|prev| prev.tab == tab && one_line(&prev.kind))
			{
				problems.push(format!("rows.{name}: beside needs a plain row above it"));
			}
			if !one_line(&kind) {
				problems.push(format!("rows.{name}: this kind cannot share a line"));
			}
			// its label, if any, takes room in front of a checkbox; any other
			// control fills its part and would be drawn over by it
			if !label.is_empty() && !matches!(kind, Kind::Toggle) {
				problems.push(format!(
					"rows.{name}: only a toggle beside another may carry a label"
				));
			}
		}
		let warning = doc.get_string(&at("warning")).unwrap_or_default();
		let windows_warning = doc.get_string(&at("windows_warning")).unwrap_or_default();
		// the mark sits after the label in the label column, which a heading and
		// half a line do not have
		if !(warning.is_empty() && windows_warning.is_empty())
			&& (label.is_empty() || beside || matches!(kind, Kind::Header(_)))
		{
			problems.push(format!("rows.{name}: a warning needs a label of its own"));
		}
		let windows = doc.get_bool(&at("windows")).unwrap_or(false);
		if matches!(kind, Kind::Header(_)) {
			group_windows = windows;
		} else if group_windows && !windows {
			problems.push(format!(
				"rows.{name}: a Windows-only group's rows are Windows-only"
			));
		}
		specs.push(Spec {
			label: keep(label),
			key,
			kind,
			tab,
			help: doc.get_string(&at("help")).map_or("", keep),
			indent: doc.get_int(&at("indent")).unwrap_or(0).clamp(0, 4) as u8,
			beside,
			revert_help: doc.get_string(&at("revert_help")).map_or("", keep),
			windows,
			not_macos: doc.get_bool(&at("not_macos")).unwrap_or(false),
			warning: keep(warning),
			windows_warning: keep(windows_warning),
		});
	}

	let numeric = |key: Key| {
		specs
			.iter()
			.any(|spec| spec.key == key && matches!(spec.kind, Kind::Slider { .. }))
	};
	let mut gates: Vec<(Key, Vec<Need>)> = Vec::new();
	for name in doc.children("gates") {
		let Some(key) = Key::parse(&name) else {
			problems.push(format!("gates.{name}: no setting by that name"));
			continue;
		};
		let mut needs = Vec::new();
		for entry in doc
			.get_string_array(&format!("gates.{name}"))
			.unwrap_or_default()
		{
			let (invert, target) = match entry.strip_prefix('!') {
				Some(rest) => (true, rest.trim().to_string()),
				None => (false, entry),
			};
			match Key::parse(&target) {
				Some(k) => needs.push(Need {
					key: k,
					invert,
					numeric: numeric(k),
				}),
				None => problems.push(format!("gates.{name}: no setting named {target}")),
			}
		}
		gates.push((key, needs));
	}

	if problems.is_empty() {
		Ok(Ui {
			tabs,
			layout,
			icons,
			help,
			specs,
			settings,
			gates,
		})
	} else {
		Err(problems)
	}
}

#[cfg(test)]
mod tests {
	use super::{Key, Kind, SOURCE, Spec, keep_platform, parse, pick_warnings, ui};

	// The one check no parser strictness can make: a setting the code knows but
	// the document never mentions is a perfectly valid document, and a setting
	// silently missing from the dialog is exactly the failure worth catching.
	// Test ID: Em2iOem
	#[test]
	fn the_declarations_are_complete_and_well_formed() {
		let ui = match parse(SOURCE) {
			Ok(ui) => ui,
			Err(problems) => panic!("settings_ui.shcl:\n  {}", problems.join("\n  ")),
		};
		let mut declared: Vec<Key> = Vec::new();
		// a buttons row and the shells grid are on the roll call but store no
		// single value, so neither has a config path
		let mut valueless: Vec<Key> = Vec::new();
		for spec in &ui.specs {
			match spec.kind {
				Kind::Dual { keys, .. } => declared.extend(keys),
				Kind::Header(_) => {}
				Kind::Buttons(_) | Kind::ShellList => {
					declared.push(spec.key);
					valueless.push(spec.key);
				}
				_ => declared.push(spec.key),
			}
		}
		let missing: Vec<&str> = Key::ALL
			.iter()
			.filter(|k| !declared.contains(k))
			.map(|k| k.name())
			.collect();
		assert!(missing.is_empty(), "no dialog row for: {missing:?}");
		for key in declared.iter().filter(|k| !valueless.contains(k)) {
			assert!(
				!ui.settings_of(*key).is_empty(),
				"{} has no config path",
				key.name()
			);
		}
		// every gate names a setting that is actually on a row
		for (key, needs) in &ui.gates {
			assert!(declared.contains(key), "gate on unlisted {}", key.name());
			for need in needs {
				assert!(
					declared.contains(&need.key),
					"gate waits on unlisted {}",
					need.key.name()
				);
			}
		}
	}

	// Test ID: Em2iOen
	#[test]
	fn every_tab_has_rows_and_every_row_a_tab() {
		let ui = ui();
		for (index, title) in ui.tabs.iter().enumerate() {
			assert!(
				ui.specs.iter().any(|s| s.tab == index),
				"tab {title} has no rows"
			);
		}
		assert!(ui.specs.iter().all(|s| s.tab < ui.tabs.len()));
		// the first row must be a heading, or the rows before it have no section
		assert!(matches!(ui.specs[0].kind, Kind::Header(_)));
	}

	// Sanity clamps rather than validation: every layout number is a floor that
	// content can outgrow, so the only real mistake is a negative or absurd one.
	// Test ID: Em2iOeo
	#[test]
	fn the_layout_numbers_are_sane() {
		let lay = &ui().layout;
		for (name, value) in [
			("width", lay.width),
			("pad", lay.pad),
			("row_height", lay.row_height),
			("label_width", lay.label_width),
			("slider_width", lay.slider_width),
			("dropdown_pair_width", lay.dropdown_pair_width),
			("swatch", lay.swatch),
			("button_height", lay.button_height),
			("base_line_height", lay.base_line_height),
			("scrollbar_width", lay.scrollbar_width),
		] {
			assert!(
				value > 0.0 && value < 4000.0,
				"layout.{name} is {value} DIP"
			);
		}
	}

	// Test ID: Em2iOep
	#[test]
	fn a_bad_document_is_reported_rather_than_half_read() {
		let bad = "tabs: \"Only\"\nrows:\n\tNotASetting:\n\t\tlabel: x\n\t\tkind: toggle\n";
		let Err(problems) = parse(bad) else {
			panic!("a row naming no setting must be reported")
		};
		assert!(
			problems.iter().any(|p| p.contains("notasetting")),
			"{problems:?}"
		);
	}

	// The macOS build leaves out software rendering and nothing else.
	// Test ID: ErycRzO
	#[test]
	fn only_software_rendering_leaves_the_macos_build() {
		let Ok(mut ui) = parse(SOURCE) else {
			panic!("settings_ui.shcl does not parse")
		};
		let all = ui.specs.len();
		keep_platform(&mut ui.specs, false, false);
		let not_windows = ui.specs.len();
		assert!(ui.specs.iter().any(|s| s.key == Key::SoftwareRendering));
		keep_platform(&mut ui.specs, false, true);
		assert_eq!(ui.specs.len(), not_windows - 1);
		assert!(!ui.specs.iter().any(|s| s.key == Key::SoftwareRendering));
		let Ok(mut ui) = parse(SOURCE) else {
			unreachable!()
		};
		keep_platform(&mut ui.specs, true, false);
		assert_eq!(ui.specs.len(), all, "Windows keeps every row");
	}

	// Everywhere but Windows the file-type group is gone, heading and all, and
	// nothing else goes with it.
	// Test ID: ErNFx0g
	#[test]
	fn the_windows_rows_leave_every_other_build() {
		let Ok(mut ui) = parse(SOURCE) else {
			panic!("settings_ui.shcl does not parse")
		};
		let all = ui.specs.len();
		let windows = ui.specs.iter().filter(|s| s.windows).count();
		assert!(windows >= 5, "the heading and four rows");
		assert!(
			ui.specs
				.iter()
				.any(|s| s.windows && matches!(s.kind, Kind::Header(_)))
		);
		keep_platform(&mut ui.specs, false, false);
		assert_eq!(ui.specs.len(), all - windows);
		for key in [
			Key::OpenBatch,
			Key::OpenPowerShell,
			Key::OpenVbScript,
			Key::OpenFolder,
		] {
			assert!(ui.specs.iter().all(|s| s.key != key), "{}", key.name());
		}
	}

	// A warning mark sits after a label in the label column, so a row with no
	// label there cannot carry one.
	// Test ID: EreHnyt
	#[test]
	fn a_warning_needs_a_label_of_its_own() {
		let head =
			"tabs: \"Only\"\nrows:\n\tHead:\n\t\tkind: heading\n\t\tlabel: Head\n\t\ttab: Only\n";
		let margin = "\tMargin:\n\t\tlabel: x\n\t\tkind: toggle\n\t\tsetting: margin\n";
		let fine = format!("{head}{margin}\t\twarning: \"Careful.\"\n");
		let Err(problems) = parse(&fine) else {
			panic!("a fragment with no layout block parses clean")
		};
		assert!(
			!problems.iter().any(|p| p.contains("warning")),
			"a labelled row may warn: {problems:?}"
		);
		for bad in [
			format!("{head}\t\twarning: \"Careful.\"\n{margin}"),
			format!(
				"{head}{margin}\tRows:\n\t\tkind: toggle\n\t\tbeside: true\n\t\tsetting: rows\n\t\twarning: \"Careful.\"\n"
			),
		] {
			let Err(problems) = parse(&bad) else {
				panic!("a warning with no label column must be reported: {bad}")
			};
			assert!(
				problems.iter().any(|p| p.contains("warning")),
				"{problems:?}"
			);
		}
	}

	// Transparency's mark adds that it keeps a window from giving its memory
	// back, in the Windows build only, since only there is a window in view
	// with Transparency on never let go. No other row's mark differs.
	// Test ID: EryD9zK
	#[test]
	fn only_the_windows_build_warns_that_transparency_keeps_memory() {
		let read = |windows: bool| {
			let Ok(mut ui) = parse(SOURCE) else {
				panic!("settings_ui.shcl does not parse")
			};
			pick_warnings(&mut ui.specs, windows);
			ui.specs
		};
		let (elsewhere, windows) = (read(false), read(true));
		let mark = |specs: &[Spec], key: Key| specs.iter().find(|s| s.key == key).unwrap().warning;
		assert!(!mark(&elsewhere, Key::Transparency).contains("memory"));
		assert!(mark(&windows, Key::Transparency).contains("memory"));
		assert!(mark(&windows, Key::Transparency).contains("compositor"));
		for (one, other) in elsewhere.iter().zip(&windows) {
			assert!(one.windows_warning.is_empty() && other.windows_warning.is_empty());
			if one.key != Key::Transparency {
				assert_eq!(one.warning, other.warning, "{}", one.key.name());
			}
		}
		// a mark for Windows alone has no label column to sit in either, on a
		// heading or half a line
		let head =
			"tabs: \"Only\"\nrows:\n\tHead:\n\t\tkind: heading\n\t\tlabel: Head\n\t\ttab: Only\n";
		let bad = format!("{head}\t\twindows_warning: \"Careful.\"\n");
		let Err(problems) = parse(&bad) else {
			panic!("a fragment with no layout block never parses clean")
		};
		assert!(
			problems.iter().any(|p| p.contains("warning")),
			"{problems:?}"
		);
	}

	// "Carry" read as "has" or "keeps" is a word to avoid in what people read,
	// and it had crept into 2 tips (2026100812334384). Every string declared
	// for the dialog, both platforms' marks included.
	// Test ID: Es9Vugt
	#[test]
	fn no_dialog_text_says_carry() {
		let Ok(ui) = parse(SOURCE) else {
			panic!("settings_ui.shcl does not parse")
		};
		let mut texts: Vec<&str> = vec![ui.help.cancel, ui.help.apply, ui.help.ok];
		texts.extend(&ui.tabs);
		for spec in &ui.specs {
			texts.extend([
				spec.label,
				spec.help,
				spec.revert_help,
				spec.warning,
				spec.windows_warning,
			]);
			match spec.kind {
				Kind::Radio(options) | Kind::Dropdown(options) | Kind::Buttons(options) => {
					texts.extend(options);
				}
				Kind::Dual { labels, .. } => texts.extend(labels),
				Kind::Header(title) => texts.push(title),
				_ => {}
			}
		}
		for text in texts {
			let words = text.split(|c: char| !c.is_ascii_alphabetic());
			assert!(
				!words
					.map(str::to_ascii_lowercase)
					.any(|w| ["carry", "carries", "carried", "carrying"].contains(&w.as_str())),
				"{text}"
			);
		}
	}

	// Test ID: ErNFx0h
	#[test]
	fn a_windows_group_cannot_contain_a_row_for_everyone() {
		let bad = "tabs: \"Only\"\nrows:\n\tHead:\n\t\tkind: heading\n\t\tlabel: Head\n\t\ttab: Only\n\t\twindows: true\n\tMargin:\n\t\tlabel: x\n\t\tkind: slider\n\t\trange: 0, 1\n\t\tsetting: margin\n";
		let Err(problems) = parse(bad) else {
			panic!("a shared row under a Windows-only heading must be reported")
		};
		assert!(
			problems
				.iter()
				.any(|p| p.contains("margin") && p.contains("Windows")),
			"{problems:?}"
		);
	}

	// Every hotkey the config file can bind has one row on the Keys tab, and a
	// row naming a path that binds nothing is refused. A hotkey added to the
	// table with no row would otherwise be settable only by hand.
	// Test ID: ErektRs
	#[test]
	fn every_hotkey_has_one_row_on_the_keys_tab() {
		let ui = ui();
		let keys_tab = ui
			.tabs
			.iter()
			.position(|t| *t == "Keys")
			.expect("a Keys tab");
		for (hotkey, path) in crate::keys::config_paths() {
			let rows: Vec<&super::Spec> = ui
				.specs
				.iter()
				.filter(|s| matches!(s.kind, Kind::Hotkey(h) if h == hotkey))
				.collect();
			assert_eq!(rows.len(), 1, "{path}");
			assert_eq!(rows[0].tab, keys_tab, "{path}");
			assert_eq!(ui.settings_of(rows[0].key), [path.as_str()]);
		}
		let rows = ui
			.specs
			.iter()
			.filter(|s| matches!(s.kind, Kind::Hotkey(_)))
			.count();
		assert_eq!(rows, crate::keys::config_paths().count());
		let bad = "tabs: \"Only\"\nrows:\n\tHead:\n\t\tkind: heading\n\t\tlabel: Head\n\t\ttab: Only\n\tHotkeyCopy:\n\t\tlabel: Copy\n\t\tkind: hotkey\n\t\tsetting: keys.bogus\n";
		let Err(problems) = parse(bad) else {
			panic!("a hotkey row binding nothing must be reported")
		};
		assert!(
			problems
				.iter()
				.any(|p| p.contains("hotkeycopy") && p.contains("keys.")),
			"{problems:?}"
		);
	}
}
