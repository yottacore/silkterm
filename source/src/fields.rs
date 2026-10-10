// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The settings the Settings dialog edits, as knobs sees them. Each one has a
//! field in `Settings`, read and written here in the file's units, and the
//! spec knobs works from is built out of `settings_ui.shcl` and this table. See
//! `project/design_docs/20261008-180516_automatic_settings.md`.

use crate::config::{self, Choice, Settings};
use crate::ui_spec::{Key, ui};
use knobs::Value;
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
pub struct Field {
	pub key: Key,
	get: fn(&Settings) -> Value,
	// false when the value is not one the field takes
	put: fn(&mut Settings, &Value) -> bool,
}

// A float the way the file writes it, so 0.95 comes back as 0.95 and a
// preset's 41.0 equals a slider that landed on 41.
fn float(x: f32) -> Value {
	Value::Float((f64::from(x) * 1000.0).round() / 1000.0)
}

fn number(v: &Value) -> Option<f32> {
	match v {
		Value::Int(i) => Some(*i as f32),
		Value::Float(f) if f.is_finite() => Some(*f as f32),
		_ => None,
	}
}

fn hex(rgb: [u8; 3]) -> Value {
	Value::Text(config::format_hex(rgb))
}

macro_rules! flag {
	($key:ident, $field:ident) => {
		Field {
			key: Key::$key,
			get: |s| Value::Bool(s.$field),
			put: |s, v| match v {
				Value::Bool(b) => {
					s.$field = *b;
					true
				}
				_ => false,
			},
		}
	};
}

macro_rules! num {
	($key:ident, $field:ident, $range:expr) => {
		Field {
			key: Key::$key,
			get: |s| float(s.$field),
			put: |s, v| {
				let (lo, hi) = $range;
				number(v).map(|x| s.$field = x.clamp(lo, hi)).is_some()
			},
		}
	};
}

macro_rules! count {
	($key:ident, $field:ident, $range:expr) => {
		Field {
			key: Key::$key,
			get: |s| Value::Int(s.$field as i64),
			put: |s, v| {
				let (lo, hi) = $range;
				number(v)
					.map(|x| s.$field = (x.round().max(0.0) as usize).clamp(lo, hi))
					.is_some()
			},
		}
	};
}

macro_rules! word {
	($key:ident, $field:ident) => {
		Field {
			key: Key::$key,
			get: |s| Value::Text(s.$field.key().to_string()),
			put: |s, v| Choice::parse(v.as_text()).map(|c| s.$field = c).is_some(),
		}
	};
}

macro_rules! color {
	($key:ident, $field:ident) => {
		Field {
			key: Key::$key,
			get: |s| hex(s.$field),
			put: |s, v| {
				config::parse_hex(v.as_text())
					.map(|c| s.$field = c)
					.is_some()
			},
		}
	};
}

macro_rules! line {
	($key:ident, $field:ident) => {
		Field {
			key: Key::$key,
			get: |s| Value::Text(s.$field.clone()),
			put: |s, v| match v {
				Value::Text(t) => {
					s.$field.clone_from(t);
					true
				}
				_ => false,
			},
		}
	};
}

use config::limits;

/// Every setting knobs keeps, in no order that matters.
#[rustfmt::skip]
pub static FIELDS: &[Field] = &[
	flag!(PerfAuto, performance_automatic),
	Field {
		key: Key::PerfProfile,
		get: |s| Value::Text(s.performance_profile.key().to_string()),
		put: |s, v| {
			s.performance_profile = crate::profile::Profile::parse(v.as_text());
			s.performance_profile.key().eq_ignore_ascii_case(v.as_text().trim())
		},
	},
	flag!(PerfCheckHardware, performance_check_hardware),
	flag!(PerfCheckNext, performance_check_next_run),
	flag!(Transparency, transparent_background),
	num!(Opacity, opacity, (0.0, 1.0)),
	flag!(BackdropBlur, transparent_background_blur),
	flag!(BgEnabled, wallpaper_enabled),
	line!(BgImage, wallpaper_raw),
	line!(BgFolder, wallpaper_folder_raw),
	flag!(BgRotate, wallpaper_rotate_enabled),
	num!(BgOpacity, wallpaper_opacity, (0.0, 1.0)),
	num!(BgBlur, wallpaper_blur, (0.0, 100.0)),
	Field {
		key: Key::BgFit,
		get: |s| Value::Text(match s.wallpaper_default_fit {
			config::Fit::Zoom => "zoom",
			config::Fit::Stretch => "stretch",
		}.to_string()),
		put: |s, v| {
			s.wallpaper_default_fit = if v.as_text().trim().eq_ignore_ascii_case("zoom") {
				config::Fit::Zoom
			} else {
				config::Fit::Stretch
			};
			matches!(v, Value::Text(_))
		},
	},
	flag!(BgHonorXmp, wallpaper_honor_xmp),
	flag!(BgHonorXmpLook, wallpaper_honor_xmp_look),
	flag!(BgContrastMask, wallpaper_contrast_mask),
	num!(BgContrastSize, wallpaper_contrast_mask_size, (0.0, 1.0)),
	num!(BgContrastStrength, wallpaper_contrast_mask_strength, (0.0, 1.0)),
	num!(BgContrastAuto, wallpaper_contrast_mask_auto, (0.0, 1.0)),
	flag!(TextScrim, text_scrim),
	num!(ScrimRadius, text_scrim_radius, (0.0, 50.0)),
	num!(ScrimSoftness, text_scrim_softness, (0.0, 1.0)),
	num!(ScrimStrength, text_scrim_strength, (0.0, 100.0)),
	word!(ScrimFunction, text_scrim_function),
	word!(ScrimRamp, text_scrim_ramp),
	num!(Outline, text_outline, (0.0, 8.0)),
	num!(MinContrast, text_min_contrast, (0.0, 0.6)),
	flag!(CursorScrim, cursor_scrim),
	flag!(CursorOutline, cursor_outline),
	flag!(CursorBlinking, cursor_blink),
	num!(CursorBlinkRate, cursor_blink_rate_s, limits::BLINK_S),
	num!(CursorHeight, cursor_size_height, (1.0, 100.0)),
	num!(CursorWidth, cursor_size_width, (1.0, 100.0)),
	word!(CursorAnimation, cursor_animation),
	num!(CursorResume, cursor_animation_resume_s, (0.05, 3600.0)),
	flag!(SystemFont, use_system_font),
	flag!(SystemFontSize, use_system_font_size),
	line!(FontFamily, font_family),
	num!(FontSize, font_size, limits::FONT_SIZE),
	num!(LineHeight, line_height_scale, limits::LINE_HEIGHT),
	flag!(RememberSize, remember_size),
	count!(Columns, columns, limits::GRID),
	count!(Rows, rows, limits::GRID),
	flag!(RememberPerMonitor, remember_per_monitor),
	flag!(RememberMaximized, remember_maximized),
	num!(Margin, margin, limits::MARGIN),
	num!(TabRegularWidth, tab_regular_pct, (2.0, 100.0)),
	num!(TabMaxWidth, tab_max_pct, (2.0, 100.0)),
	flag!(NewTabNextToCurrent, new_tab_beside),
	flag!(IdleRelease, idle_release),
	count!(IdleHiddenMin, idle_release_hidden_min, limits::IDLE_MIN),
	count!(IdleMin, idle_release_min, limits::IDLE_MIN),
	flag!(SoftwareRendering, software_rendering),
	flag!(TabShowsTitle, tab_shows_title),
	flag!(TabShowsShell, tab_shows_shell),
	flag!(TabShowsProgram, tab_shows_program),
	flag!(TabShowsDirectory, tab_shows_directory),
	flag!(TitleShowsTab, title_shows_tab),
	line!(StartupDirectory, startup_directory),
	flag!(ShellIntegration, shell_integration),
	flag!(BashPrompt, bash_prompt),
	flag!(CopyOnSelect, copy_on_select),
	flag!(Hyperlinks, hyperlinks),
	line!(LinkOpenCommand, hyperlink_open_command),
	flag!(SmoothScroll, scroll_smooth),
	num!(ScrollEaseIn, scroll_ease_in_ms, limits::EASE_MS),
	num!(ScrollRampUp, scroll_ramp_up_ms, limits::EASE_MS),
	num!(SingleScreenTau, scroll_single_screen_tau_ms, limits::EASE_MS),
	num!(ScrollRampDown, scroll_ramp_down_ms, limits::EASE_MS),
	num!(ScrollEaseOut, scroll_ease_out_ms, limits::EASE_MS),
	flag!(SmoothApps, smooth_scroll_apps),
	num!(WheelLines, wheel_lines, limits::WHEEL_LINES),
	flag!(Scrollbar, scrollbar),
	num!(ScrollbarThickness, scrollbar_thickness, (4.0, 64.0)),
	flag!(ScrollbarAutoHide, scrollbar_auto_hide),
	flag!(Minimap, minimap),
	num!(MinimapWidth, minimap_width, (24.0, 400.0)),
	color!(ColBg, bg),
	flag!(ColFromWallpaper, colors_from_wallpaper),
	color!(ColFg, fg),
	color!(ColCursor, cursor),
	color!(ColHighlight, highlight),
	color!(ColFocus, focus),
	color!(ColGutter, gutter),
	color!(ColMenuBg, menu_bg),
	color!(ColMenuFg, menu_fg),
	color!(ColDialogBg, dialog_bg),
	color!(ColDialogFg, dialog_fg),
	color!(ColScrollbarThumb, scrollbar_thumb),
	color!(ColScrollbarTrough, scrollbar_trough),
	line!(Theme, theme),
	word!(ThemeMode, theme_mode),
];

pub fn field(key: Key) -> Option<&'static Field> {
	FIELDS.iter().find(|f| f.key == key)
}

/// The value a setting field has in `settings`, in the file's units.
pub fn get(settings: &Settings, key: Key) -> Option<Value> {
	field(key).map(|f| (f.get)(settings))
}

/// A value set by hand, or held for the run, rather than the default.
pub fn by_hand(settings: &Settings, key: Key) -> bool {
	let values = &settings.model.values;
	values.own.contains_key(key.name()) || values.held.contains_key(key.name())
}

/// The theme colors, in the order `theme::PALETTE_KEYS` lists them.
pub const THEME_KEYS: [Key; 10] = [
	Key::ColBg,
	Key::ColFg,
	Key::ColCursor,
	Key::ColHighlight,
	Key::ColFocus,
	Key::ColMenuBg,
	Key::ColMenuFg,
	Key::ColDialogBg,
	Key::ColDialogFg,
	Key::ColGutter,
];

/// The knobs id of the profile and the theme.
pub const PROFILE: &str = "profile";
pub const THEME: &str = "theme";

/// The spec, built once from `settings_ui.shcl` and the table.
pub fn spec() -> &'static Arc<knobs::Spec> {
	static SPEC: OnceLock<Arc<knobs::Spec>> = OnceLock::new();
	// Unreachable in a tested build, as with ui_spec: both halves are compiled in.
	SPEC.get_or_init(|| match build() {
		Ok(spec) => Arc::new(spec),
		Err(problems) => panic!("settings spec: {}", problems.join("; ")),
	})
}

/// A model with nothing stored, for a fresh `Settings`.
pub fn model() -> knobs::Model {
	knobs::Model::new(Arc::clone(spec()))
}

// What a setting is called in a tip that lists it, such as the changed rows.
fn label_of(key: Key) -> &'static str {
	let ui = ui();
	for spec in &ui.specs {
		match spec.kind {
			crate::ui_spec::Kind::Dual { keys, labels } => {
				if let Some(i) = keys.iter().position(|k| *k == key) {
					return labels[i];
				}
			}
			_ if spec.key == key => return spec.label,
			_ => {}
		}
	}
	ui.hidden
		.iter()
		.find(|(k, _)| *k == key)
		.map_or("", |(_, label)| label)
}

fn build() -> Result<knobs::Spec, Vec<String>> {
	let ui = ui();
	let defaults = Settings::bare();
	let choosers: Vec<Key> = ui.groups.iter().map(|g| g.chooser).collect();
	let mut problems = Vec::new();
	let settings: Vec<knobs::Setting> = FIELDS
		.iter()
		.map(|f| {
			let rel = ui.rel(f.key);
			let default = (f.get)(&defaults);
			let chooser = choosers.contains(&f.key);
			let kind = match default {
				Value::Bool(_) => knobs::Kind::Bool,
				Value::Int(_) => knobs::Kind::Int,
				Value::Float(_) => knobs::Kind::Float,
				Value::Text(_) if chooser => knobs::Kind::Choice,
				Value::Text(_) => knobs::Kind::Text,
			};
			let path = ui.settings_of(f.key).first().copied().unwrap_or_default();
			if path.is_empty() {
				problems.push(format!("{}: no setting path", f.key.name()));
			}
			knobs::Setting {
				id: f.key.name().to_string(),
				label: label_of(f.key).to_string(),
				tab: 0,
				control: if chooser {
					knobs::Control::Dropdown
				} else if kind == knobs::Kind::Bool {
					knobs::Control::Checkbox
				} else {
					knobs::Control::Text
				},
				kind,
				path: path.to_string(),
				store: knobs::Store::Config,
				default,
				min: f64::NEG_INFINITY,
				max: f64::INFINITY,
				scale: knobs::Scale::Linear,
				detents: Vec::new(),
				options: Vec::new(),
				tip: String::new(),
				indent: 0,
				gate: rel.gate.map(|k| k.name().to_string()),
				auto: rel.auto.map(|k| k.name().to_string()),
				rule: rel.rule.map(|r| knobs::Rule::Named(r.to_string())),
				group: rel.group.map(str::to_string),
			}
		})
		.collect();
	let mut scratch = Settings::bare();
	let mut groups = Vec::new();
	for g in &ui.groups {
		let members: Vec<Key> = FIELDS
			.iter()
			.filter(|f| ui.rel(f.key).group == Some(g.id))
			.map(|f| f.key)
			.collect();
		let mut presets = Vec::new();
		for p in &g.presets {
			let mut values = std::collections::BTreeMap::new();
			for (key, raw) in &p.values {
				match normal(*key, raw, &mut scratch) {
					Some(v) if members.contains(key) => {
						values.insert(key.name().to_string(), v);
					}
					Some(_) => problems.push(format!("{}.{}: not in the group", p.key, key.name())),
					None => {
						problems.push(format!("{}.{}: not a value it takes", p.key, key.name()));
					}
				}
			}
			for key in &members {
				if !values.contains_key(key.name()) {
					problems.push(format!("{}: no value for {}", p.key, key.name()));
				}
			}
			presets.push(knobs::Preset {
				key: p.key.to_string(),
				label: p.label.to_string(),
				temporary: p.temporary,
				values,
			});
		}
		groups.push(knobs::Group {
			id: g.id.to_string(),
			chooser: g.chooser.name().to_string(),
			noun: g.noun.to_string(),
			custom: Some(g.custom.to_string()),
			presets,
			from_program: g.from_program,
			members: Vec::new(),
		});
	}
	if !problems.is_empty() {
		return Err(problems);
	}
	knobs::Spec {
		tabs: vec![knobs::Tab {
			path: "Settings".into(),
			label: "Settings".into(),
			parent: None,
		}],
		settings,
		groups,
	}
	.finish()
}

// A value as the field would write it back: clamped, its case fixed, a float
// rounded the way the file has it. None when the field doesn't take it.
fn normal(key: Key, v: &Value, scratch: &mut Settings) -> Option<Value> {
	let f = field(key)?;
	(f.put)(scratch, v).then(|| (f.get)(scratch))
}

/// The stored values from the config file. A value a field doesn't take is
/// dropped, the way knobs drops one that fails its checks, and text that is
/// empty is no value where a rule or a switch has one to give.
pub fn load(config: &shcl::Document) -> knobs::Model {
	let mut m = knobs::Model::load_docs(Arc::clone(spec()), config, &shcl::Document::new());
	// "none" was how a file said no blink before `cursor.blink`. It is no
	// animation, so the model never takes it in.
	let animation = spec()
		.get(Key::CursorAnimation.name())
		.map_or("cursor.animation", |s| s.path.as_str());
	if config
		.get_string(animation)
		.is_ok_and(|v| v.trim().eq_ignore_ascii_case("none"))
	{
		m.values.own.remove(Key::CursorAnimation.name());
		m.values
			.own
			.insert(Key::CursorBlinking.name().into(), Value::Bool(false));
	}
	let mut scratch = Settings::bare();
	let tidy = |values: &mut std::collections::BTreeMap<String, Value>, scratch: &mut Settings| {
		values.retain(|id, v| {
			let Some(key) = Key::parse(id) else {
				return false;
			};
			let rel = ui().rel(key);
			if (rel.rule.is_some() || rel.auto.is_some())
				&& matches!(v, Value::Text(_))
				&& v.as_text().trim().is_empty()
			{
				return false;
			}
			// the usual folder written out is what an older build wrote for none
			if key == Key::BgFolder && v.as_text().trim() == config::WALLPAPER_DIR_TOKEN {
				return false;
			}
			match normal(key, v, scratch) {
				Some(n) => {
					*v = n;
					true
				}
				None => false,
			}
		});
	};
	tidy(&mut m.values.own, &mut scratch);
	tidy(&mut m.values.changes, &mut scratch);
	// A file with no line for a font switch reads as builds before 10-09 read
	// it: a family of its own turns the face off, and the size follows the face
	// unless it has a size of its own.
	let line = |path: &str| config.get_string(path).is_ok_and(|v| !v.trim().is_empty());
	let switch = |path: &str| config.get_bool(path).is_ok();
	let own = |m: &knobs::Model, key: Key| m.values.own.get(key.name()).cloned();
	let face_on = match own(&m, Key::SystemFont) {
		Some(on) => on.as_bool(),
		None => !line("font.family"),
	};
	if !switch("font.use_system_family") && !face_on {
		m.values
			.own
			.insert(Key::SystemFont.name().into(), Value::Bool(false));
	}
	if !switch("font.use_system_size") && (!face_on || line("font.size")) {
		m.values
			.own
			.insert(Key::SystemFontSize.name().into(), Value::Bool(false));
	}
	m
}

/// What the program answers for a rule, and the themes as presets.
#[derive(Debug)]
pub struct Env<'a> {
	settings: &'a Settings,
	monitor: Option<&'a str>,
	wallpaper: Option<crate::autotheme::Derived>,
	mode: crate::theme::Mode,
	per_monitor: bool,
}

impl<'a> Env<'a> {
	/// Settings fields knobs keeps are read off the model, since `settings`
	/// may not be filled from it yet.
	pub fn new(settings: &'a Settings, monitor: Option<&'a str>) -> Env<'a> {
		let plain = |key: Key| settings.model.value(key.name(), &knobs::NoEnv);
		Env {
			settings,
			monitor,
			wallpaper: settings.wallpaper_derived,
			mode: Choice::parse(plain(Key::ThemeMode).as_text())
				.unwrap_or(crate::theme::Mode::Dark),
			per_monitor: plain(Key::RememberPerMonitor).as_bool(),
		}
	}
}

impl knobs::Env for Env<'_> {
	fn rule(&self, name: &str) -> Option<Value> {
		let s = self.settings;
		match name {
			"system_font_family" => crate::sysfont::monospace().family.clone().map(Value::Text),
			"system_font_size" => crate::sysfont::monospace()
				.size_pt
				.map(crate::sysfont::px_from_pt)
				.filter(|px| *px >= 4.0)
				.map(float),
			"remembered_columns" | "remembered_rows" => {
				let kept = config::remembered_window_at(s, self.monitor, self.per_monitor);
				let n = if name == "remembered_columns" {
					kept.columns
				} else {
					kept.rows
				};
				Some(Value::Int(n as i64))
			}
			"wallpaper_fg" => self.wallpaper.map(|d| hex(d.fg)),
			"wallpaper_cursor" => self.wallpaper.map(|d| hex(d.cursor)),
			"machine_profile" => {
				// a step only ever goes down from the tested pick
				let pick = match (s.stepped_profile, s.tested_profile) {
					(Some(step), Some(tested)) if step.index() <= tested.index() => tested,
					(Some(step), _) => step,
					(None, tested) => tested?,
				};
				Some(Value::Text(pick.key().to_string()))
			}
			"desktop_opener" => Some(Value::Text(crate::links::desktop_opener().to_string())),
			"found_picture" => Some(Value::Text(s.found_picture.clone())),
			"usual_folder" => Some(Value::Text(config::WALLPAPER_DIR_TOKEN.to_string())),
			_ => None,
		}
	}

	fn preset(&self, group: &str, key: &str) -> Option<knobs::Preset> {
		if group != THEME {
			return None;
		}
		let user = &self.settings.user_themes;
		let name = crate::theme::all_names(user)
			.into_iter()
			.find(|n| n.eq_ignore_ascii_case(key.trim()))?;
		let pal = crate::theme::resolve_in(user, &name, self.mode, config::os_dark());
		Some(knobs::Preset {
			key: key.to_string(),
			label: name,
			temporary: false,
			values: THEME_KEYS
				.iter()
				.enumerate()
				.map(|(i, k)| (k.name().to_string(), hex(pal.get(i))))
				.collect(),
		})
	}
}

/// Fill every setting field from the stored values, then what follows from
/// them. `monitor` is the one Columns and Rows are shown for.
pub fn fill(settings: &mut Settings, monitor: Option<&str>) {
	let was = (
		settings.wallpaper_raw.clone(),
		settings.wallpaper_folder_raw.clone(),
	);
	// worked out from the theme's colors, never from the last answer
	settings.wallpaper_derived = None;
	let values: Vec<Value> = {
		let env = Env::new(settings, monitor);
		FIELDS
			.iter()
			.map(|f| settings.model.value(f.key.name(), &env))
			.collect()
	};
	for (f, v) in FIELDS.iter().zip(&values) {
		let _ = (f.put)(settings, v);
	}
	// the wallpaper's colors are worked out from everything else
	if settings.colors_from_wallpaper && settings.wallpaper_enabled {
		if let Some(sum) = settings.wallpaper_summary {
			settings.wallpaper_derived = Some(crate::autotheme::derive(&sum, settings));
			let pair = {
				let env = Env::new(settings, monitor);
				[Key::ColFg, Key::ColCursor].map(|k| settings.model.value(k.name(), &env))
			};
			for (k, v) in [Key::ColFg, Key::ColCursor].into_iter().zip(&pair) {
				if let Some(f) = field(k) {
					let _ = (f.put)(settings, v);
				}
			}
		}
	}
	config::retheme(settings);
	config::rewallpaper(settings, (&was.0, &was.1));
}

/// The window's grid on `monitor`, where Columns and Rows follow the last size.
pub fn grid(settings: &Settings, monitor: Option<&str>) -> (usize, usize) {
	let env = Env::new(settings, monitor);
	let n = |key: Key| match settings.model.value(key.name(), &env) {
		Value::Int(n) => usize::try_from(n).unwrap_or(1).max(1),
		other => number(&other).map_or(1, |x| x.round().max(1.0) as usize),
	};
	(n(Key::Columns), n(Key::Rows))
}

/// Put `v` into the field for `key`, as a fill would. False when the field
/// doesn't take it.
pub fn put(settings: &mut Settings, key: Key, v: &Value) -> bool {
	field(key).is_some_and(|f| (f.put)(settings, v))
}

/// A value the program sets for itself, such as Re-test next run turning off
/// after the test. Stored as it is, with nothing else changed by it.
pub fn store(settings: &mut Settings, key: Key, v: Value) {
	if field(key).is_some() {
		settings.model.values.own.insert(key.name().to_string(), v);
		fill(settings, None);
	}
}

/// A change made on screen: what knobs does with it, then every field filled
/// again.
pub fn set(settings: &mut Settings, key: Key, v: &Value, monitor: Option<&str>) {
	if field(key).is_none() {
		return;
	}
	let mut model = settings.model.clone();
	model.set(key.name(), v, &Env::new(settings, monitor));
	settings.model = model;
	fill(settings, monitor);
}

/// The reset arrow, then every field filled again.
pub fn reset(settings: &mut Settings, key: Key, monitor: Option<&str>) {
	if field(key).is_none() {
		return;
	}
	let mut model = settings.model.clone();
	model.reset(key.name(), &Env::new(settings, monitor));
	settings.model = model;
	fill(settings, monitor);
}

pub fn can_reset(settings: &Settings, key: Key, monitor: Option<&str>) -> bool {
	field(key).is_some()
		&& settings
			.model
			.can_reset(key.name(), &Env::new(settings, monitor))
}

/// What a setting has with nothing of its own, for a tip.
pub fn default_of(settings: &Settings, key: Key, monitor: Option<&str>) -> Option<Value> {
	field(key)?;
	Some(
		settings
			.model
			.default_of(key.name(), &Env::new(settings, monitor)),
	)
}

/// Where a setting's value comes from.
pub fn source(settings: &Settings, key: Key, monitor: Option<&str>) -> Option<knobs::Source> {
	field(key)?;
	Some(
		settings
			.model
			.source(key.name(), &Env::new(settings, monitor)),
	)
}

fn group_of(chooser: Key) -> Option<&'static str> {
	ui().groups
		.iter()
		.find(|g| g.chooser == chooser)
		.map(|g| g.id)
}

/// The preset a chooser row is on, or Custom, with a temporary one in force.
pub fn chosen(settings: &Settings, chooser: Key) -> String {
	match group_of(chooser) {
		Some(group) => settings.model.chosen(group, &Env::new(settings, None)),
		None => String::new(),
	}
}

/// What a chooser's box says: the preset, with " *" after a change under it.
pub fn shown_choice(settings: &Settings, chooser: Key) -> String {
	settings
		.model
		.shown_choice(chooser.name(), &Env::new(settings, None))
}

/// The rows changed on top of the preset a chooser is on.
pub fn changed(settings: &Settings, chooser: Key) -> Vec<Key> {
	let Some(group) = group_of(chooser) else {
		return Vec::new();
	};
	settings
		.model
		.changed(group, &Env::new(settings, None))
		.into_iter()
		.filter_map(Key::parse)
		.collect()
}

/// What a chooser calls the person's own values.
pub fn custom_label(chooser: Key) -> &'static str {
	ui().groups
		.iter()
		.find(|g| g.chooser == chooser)
		.map_or("Custom", |g| g.custom)
}

/// The tip's line on where a value comes from, with each value written by
/// `show` in the row's own units. Empty for a value at its default.
pub fn state_line(
	settings: &Settings,
	key: Key,
	monitor: Option<&str>,
	show: &dyn Fn(Key, &Value) -> String,
) -> String {
	if field(key).is_none() {
		return String::new();
	}
	settings
		.model
		.state_line_shown(key.name(), &Env::new(settings, monitor), &|s, v| {
			Key::parse(&s.id).map_or_else(|| v.show(), |k| show(k, v))
		})
}

/// A preset that changed its name, such as a saved theme renamed. The chooser
/// follows it, and so do its "*" changes, where a pick would drop them.
pub fn rename_preset(settings: &mut Settings, chooser: Key, old: &str, new: &str) {
	let Some(group) = group_of(chooser) else {
		return;
	};
	let values = &mut settings.model.values;
	if values
		.own
		.get(chooser.name())
		.is_some_and(|v| v.as_text().eq_ignore_ascii_case(old))
	{
		values
			.own
			.insert(chooser.name().to_string(), Value::Text(new.to_string()));
	}
	if values
		.changed_on
		.get(group)
		.is_some_and(|on| on.eq_ignore_ascii_case(old))
	{
		values.changed_on.insert(group.to_string(), new.to_string());
	}
	fill(settings, None);
}

/// Settings fields set directly, the way a test builds them, made into what is
/// stored, as a file with those lines would load. Only a field that shows a
/// value other than the one stored is taken.
#[cfg(test)]
pub fn adopt(mut settings: Settings) -> Settings {
	let shown: Vec<(Key, Value)> = FIELDS.iter().map(|f| (f.key, (f.get)(&settings))).collect();
	fill(&mut settings, None);
	for (key, v) in shown {
		if (field(key).map(|f| (f.get)(&settings))).as_ref() != Some(&v) {
			settings.model.values.own.insert(key.name().to_string(), v);
		}
	}
	fill(&mut settings, None);
	settings
}

/// Settings with these stored as the person's own. For tests.
#[cfg(test)]
pub fn owning(mut settings: Settings, values: &[(Key, Value)]) -> Settings {
	for (key, v) in values {
		settings
			.model
			.values
			.own
			.insert(key.name().to_string(), v.clone());
	}
	fill(&mut settings, None);
	settings
}

/// Every line the settings file would have, config and kept, for tests.
#[cfg(test)]
pub fn lines_of(settings: &Settings) -> Vec<(String, Value)> {
	let lines = settings.model.lines(&Env::new(settings, None));
	lines.config.into_iter().chain(lines.kept).collect()
}
