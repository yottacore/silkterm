// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The Settings dialog's declarations, read from `settings_ui.shcl` (compiled
//! in). The file owns what the dialog IS - rows, order, sections, tabs, the
//! config path behind each row, when a row grays out, and the geometry.
//! `settings_ui.rs` owns what it DOES.
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
	Columns, Rows, RememberSize, Margin, TabRegularWidth, TabMaxWidth,
	IdleRelease, IdleHiddenMin, IdleMin,
	TabShowsTitle, TabShowsShell, TabShowsProgram, TabShowsDirectory, TitleShowsTab,
	Shells, StartupDirectory, ShellIntegration, BashPrompt, CopyOnSelect, Hyperlinks, LinkOpenCommand,
	SmoothScroll, ScrollEaseIn, ScrollRampUp, SingleScreenTau, ScrollRampDown,
	ScrollEaseOut, WheelLines,
	Scrollbar, ScrollbarThickness, ScrollbarAutoHide, Minimap, MinimapWidth,
	ColScrollbarThumb, ColScrollbarTrough, ColBg, ColFromWallpaper, ColFg, ColCursor,
	ColHighlight, ColFocus, ColGutter,
	ColMenuBg, ColMenuFg, ColDialogBg, ColDialogFg,
	Theme, ThemeMode, ThemeActions,
];

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
	Header(&'static str), // a section heading, no control
}

pub struct Spec {
	pub label: &'static str,
	pub key: Key,
	pub kind: Kind,
	pub tab: usize,
	// Flyover text for a control whose purpose is not obvious from its label.
	// Empty means the row explains itself and gets no tip.
	pub help: &'static str,
	// Sub-group depth. Only the LABEL moves; the controls stay in their column,
	// so a run of indented labels reads as belonging to the row above it. A
	// sub-group is therefore not declared anywhere - it is the leader's own
	// depth plus everything deeper that follows.
	pub indent: u8,
	// Drawn on the same line as the row above rather than under it. The pair
	// splits the control column in half; the row above keeps the label column and
	// its label has to name both halves. This row's own label, if it has one,
	// follows its control the way a checkbox's does.
	pub beside: bool,
}

// One setting a control has to wait on, resolved from the file's gate lines.
// `numeric` is decided here rather than at every check: a slider is satisfied
// while it sits above zero, everything else while it is switched on.
pub struct Need {
	pub key: Key,
	pub invert: bool,
	pub numeric: bool,
}

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

// Flyover text for the footer buttons, which are chrome rather than settings and
// so have no row of their own to carry it.
pub struct Help {
	pub cancel: &'static str,
	pub apply: &'static str,
	pub ok: &'static str,
}

pub struct Icons {
	pub dropdown_arrow: &'static str,
	pub dropdown_check: &'static str,
	pub revert: &'static str,
}

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
	// Config path(s) behind a setting, for revert's comment-out. Empty for a
	// heading, or for a row that carries no setting of its own.
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
		Ok(ui) => ui,
		// Unreachable in a tested build: the document is compiled in, so it
		// cannot vary at runtime and the test below reads the same bytes.
		Err(problems) => panic!("settings_ui.shcl: {}", problems.join("; ")),
	})
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
		// A shared line needs an ordinary row above it to share, and both halves
		// have to be things that fit on one line. A row whose height is not a
		// constant (the grid) or that holds no value (a heading) is not.
		let one_line = |kind: &Kind| {
			!matches!(
				kind,
				Kind::Header(_) | Kind::ShellList | Kind::Buttons(_) | Kind::Radio(_)
			)
		};
		if beside {
			if !specs
				.last()
				.is_some_and(|prev| prev.tab == tab && !prev.beside && one_line(&prev.kind))
			{
				problems.push(format!("rows.{name}: beside needs a plain row above it"));
			}
			if !one_line(&kind) {
				problems.push(format!("rows.{name}: this kind cannot share a line"));
			}
			// its label, if any, is drawn where a checkbox puts one - to the right
			// of the control - so anything else would draw over its own control
			if !label.is_empty() && !matches!(kind, Kind::Toggle) {
				problems.push(format!(
					"rows.{name}: only a toggle beside another may carry a label"
				));
			}
		}
		specs.push(Spec {
			label: keep(label),
			key,
			kind,
			tab,
			help: doc.get_string(&at("help")).map_or("", keep),
			indent: doc.get_int(&at("indent")).unwrap_or(0).clamp(0, 4) as u8,
			beside,
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
	use super::{Key, Kind, SOURCE, parse, ui};

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
}
