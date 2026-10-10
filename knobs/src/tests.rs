// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

use super::*;

const DEMO: &str = include_str!("../demo/demo.shcl");

// A desktop that names a font and a size, and a wallpaper to pick colors from.
struct Desk {
	font: Option<&'static str>,
	wallpaper: bool,
}

impl Env for Desk {
	fn rule(&self, name: &str) -> Option<Value> {
		match name {
			"system_font_family" => self.font.map(|f| Value::Text(f.into())),
			"system_font_size" => self.font.map(|_| Value::Float(11.0)),
			"wallpaper_fg" => self.wallpaper.then(|| Value::Text("#eeeeee".into())),
			"wallpaper_cursor" => self.wallpaper.then(|| Value::Text("#ff8800".into())),
			_ => None,
		}
	}
}

const DESK: Desk = Desk {
	font: Some("Desk Mono"),
	wallpaper: true,
};

fn demo() -> Model {
	Model::new(Spec::parse(DEMO).unwrap_or_else(|e| panic!("demo spec: {e:#?}")))
}

fn int(v: i64) -> Value {
	Value::Int(v)
}

fn text(t: &str) -> Value {
	Value::Text(t.into())
}

// Test ID: EsFc30B
#[test]
fn the_demo_spec_parses_and_works_out_indents() {
	let m = demo();
	assert_eq!(m.spec.at("strength").indent, 1, "under Text scrim");
	assert_eq!(m.spec.at("profile").indent, 1, "under Choose automatically");
	assert_eq!(m.spec.at("scrim").indent, 0, "a group is not a parent");
	let profile = m.spec.at("profile");
	let keys: Vec<&str> = profile.options.iter().map(|(k, _)| k.as_str()).collect();
	assert_eq!(
		keys,
		["max_silk", "high", "low", "standard", "remote", CUSTOM]
	);
	let look = m
		.spec
		.tabs
		.iter()
		.position(|t| t.path == "Look")
		.unwrap_or(99);
	assert_eq!(
		m.spec.tabs_under(Some(look)).len(),
		2,
		"Text and Colors sit under Look"
	);
}

// Test ID: EsFc30C
#[test]
fn a_bad_spec_names_every_mistake() {
	let bad = "tabs: \"A\"\nsettings:\n\tx:\n\t\tlabel: X\n\t\ttab: A\n\t\tcontrol: slider\n\t\tpath: a.x\n\t\tgate: nothing\n\ty:\n\t\tlabel: Y\n\t\ttab: B\n\t\tcontrol: checkbox\n\t\tpath: a.x\n";
	let errs = Spec::parse(bad).err().unwrap_or_default();
	let all = errs.join("\n");
	assert!(all.contains("x: gate nothing"), "{all}");
	assert!(all.contains("x: a slider needs a range"), "{all}");
	assert!(all.contains("y: no tab"), "{all}");
}

// Test ID: EsFc30D
#[test]
fn a_switch_keeps_the_value_set_by_hand_for_the_round_trip() {
	let mut m = demo();
	assert_eq!(
		m.value("family", &DESK),
		text("Desk Mono"),
		"automatic to start"
	);
	m.set("family", &text("Fira Code"), &DESK);
	assert!(
		!m.value("use_system_family", &DESK).as_bool(),
		"typing in it turned the switch off"
	);
	assert_eq!(m.value("family", &DESK), text("Fira Code"));
	m.set("use_system_family", &Value::Bool(true), &DESK);
	assert_eq!(m.value("family", &DESK), text("Desk Mono"));
	assert_eq!(
		m.source("family", &DESK),
		Source::Automatic {
			kept: Some(text("Fira Code"))
		}
	);
	m.set("use_system_family", &Value::Bool(false), &DESK);
	assert_eq!(
		m.value("family", &DESK),
		text("Fira Code"),
		"and back it comes"
	);
}

// Test ID: EsFc30E
#[test]
fn switching_off_with_nothing_kept_keeps_what_shows() {
	let mut m = demo();
	m.set("use_system_family", &Value::Bool(false), &DESK);
	assert_eq!(
		m.value("family", &DESK),
		text("Desk Mono"),
		"nothing on screen moved"
	);
}

// Test ID: EsFc30F
#[test]
fn with_nothing_to_follow_the_value_set_by_hand_is_used() {
	let mut m = demo();
	m.set("family", &text("Fira Code"), &DESK);
	m.set("use_system_family", &Value::Bool(true), &DESK);
	let bare = Desk {
		font: None,
		wallpaper: false,
	};
	assert_eq!(m.value("family", &bare), text("Fira Code"));
}

// Test ID: EsFc30G
#[test]
fn editing_one_member_turns_its_switch_off_and_the_others_keep_what_they_show() {
	let mut m = demo();
	m.set("last_columns", &int(100), &DESK);
	m.set("last_rows", &int(30), &DESK);
	m.values.own.insert("rows".into(), int(40)); // an old hand value, unused while on
	m.set("columns", &int(110), &DESK);
	assert!(!m.value("remember_size", &DESK).as_bool());
	assert_eq!(m.value("columns", &DESK), int(110));
	assert_eq!(
		m.value("rows", &DESK),
		int(30),
		"rows kept the size it was showing"
	);
}

// Test ID: EsFc30H
#[test]
fn changing_a_row_under_a_switch_that_is_off_turns_it_on() {
	let mut m = demo();
	m.set("idle_release", &Value::Bool(false), &DESK);
	m.set("idle_min", &int(60), &DESK);
	assert!(m.value("idle_release", &DESK).as_bool());
	assert!(m.counts("idle_min", &DESK));
}

// Test ID: EsFc30I
#[test]
fn a_row_changed_on_a_preset_stars_it_stops_the_machine_and_spares_custom() {
	let mut m = demo();
	m.set("tested_profile", &text("low"), &DESK);
	m.fill_custom("perf", &DESK);
	let custom_strength = m.values.own.get("strength").cloned();
	assert_eq!(m.shown_choice("profile", &DESK), "Low");
	// Low has the scrim off, so the change turns it on too
	m.set("strength", &int(80), &DESK);
	assert_eq!(m.shown_choice("profile", &DESK), "Low *");
	assert!(m.value("scrim", &DESK).as_bool());
	assert_eq!(m.changed("perf", &DESK), ["scrim", "strength"]);
	assert!(
		!m.value("choose_auto", &DESK).as_bool(),
		"the machine no longer picks"
	);
	m.set("tested_profile", &text("max_silk"), &DESK);
	assert_eq!(
		m.shown_choice("profile", &DESK),
		"Low *",
		"and a new test doesn't undo it"
	);
	assert_eq!(
		m.values.own.get("strength").cloned(),
		custom_strength,
		"Custom untouched"
	);
}

// Test ID: EsFc30J
#[test]
fn resetting_every_changed_row_takes_the_star_away() {
	let mut m = demo();
	m.set("strength", &int(80), &DESK);
	assert_eq!(m.shown_choice("profile", &DESK), "High *");
	assert!(m.can_reset("strength", &DESK));
	m.reset("strength", &DESK);
	assert_eq!(m.shown_choice("profile", &DESK), "High");
	assert_eq!(m.value("strength", &DESK), int(50));
}

// Test ID: EsFc30K
#[test]
fn picking_another_profile_drops_the_changes_and_custom_comes_back() {
	let mut m = demo();
	m.fill_custom("perf", &DESK);
	m.set("profile", &text(CUSTOM), &DESK);
	m.set("blur", &int(20), &DESK);
	assert_eq!(
		m.shown_choice("profile", &DESK),
		"Custom",
		"Custom never stars"
	);
	m.set("profile", &text("high"), &DESK);
	m.set("strength", &int(90), &DESK);
	m.set("profile", &text("low"), &DESK);
	assert_eq!(m.shown_choice("profile", &DESK), "Low");
	m.set("profile", &text("high"), &DESK);
	assert_eq!(
		m.value("strength", &DESK),
		int(50),
		"the change was dropped"
	);
	m.set("profile", &text(CUSTOM), &DESK);
	assert_eq!(
		m.value("blur", &DESK),
		Value::Float(20.0),
		"Custom kept its own"
	);
}

// Test ID: EsFc30L
#[test]
fn choose_automatically_is_the_profile_s_own_switch() {
	let mut m = demo();
	m.set("tested_profile", &text("max_silk"), &DESK);
	assert_eq!(m.chosen("perf", &DESK), "max_silk");
	m.set("profile", &text("low"), &DESK);
	assert!(
		!m.value("choose_auto", &DESK).as_bool(),
		"a hand pick turns it off"
	);
	m.set("choose_auto", &Value::Bool(true), &DESK);
	assert_eq!(m.chosen("perf", &DESK), "max_silk");
	m.set("choose_auto", &Value::Bool(false), &DESK);
	assert_eq!(m.chosen("perf", &DESK), "low", "the hand pick comes back");
}

// Test ID: EsFc30M
#[test]
fn a_temporary_preset_ignores_changes_stores_nothing_and_ends_on_an_edit() {
	let mut m = demo();
	m.set("profile", &text("low"), &DESK);
	m.set("radius", &int(9), &DESK);
	m.set("profile", &text("remote"), &DESK);
	assert_eq!(m.value("radius", &DESK), int(0));
	assert_eq!(
		m.values.own.get("profile"),
		Some(&text("low")),
		"remote is never stored"
	);
	let (config, _) = m.save(&DESK);
	assert!(!config.contains("remote"), "{config}");
	m.set("blur", &int(4), &DESK);
	assert_eq!(m.chosen("perf", &DESK), "low");
	assert_eq!(m.shown_choice("profile", &DESK), "Low *");
}

// Test ID: EsFc30N
#[test]
fn wallpaper_colors_beat_the_theme_and_an_edit_turns_them_off() {
	let mut m = demo();
	assert_eq!(m.value("fg", &DESK), text("#eeeeee"));
	let plain = Desk {
		font: None,
		wallpaper: false,
	};
	assert_eq!(
		m.value("fg", &plain),
		text("#d0d0d0"),
		"no wallpaper, so the theme's"
	);
	m.set("fg", &text("#123456"), &DESK);
	assert!(!m.value("from_wallpaper", &DESK).as_bool());
	assert_eq!(
		m.value("cursor", &DESK),
		text("#ff8800"),
		"cursor kept what it showed"
	);
	assert_eq!(m.shown_choice("theme", &DESK), "Dark *");
	// turning it back on, then off, gives the theme's colors with the changes
	m.set("from_wallpaper", &Value::Bool(true), &DESK);
	m.set("from_wallpaper", &Value::Bool(false), &DESK);
	assert_eq!(m.value("fg", &DESK), text("#123456"));
}

// Test ID: EsFc30O
#[test]
fn the_files_round_trip_and_set_aside_values_go_in_kept() {
	let mut m = demo();
	m.set("family", &text("Fira Code"), &DESK);
	m.set("use_system_family", &Value::Bool(true), &DESK);
	m.set("strength", &int(80), &DESK);
	m.set("last_columns", &int(120), &DESK);
	let (config, state) = m.save(&DESK);
	assert!(config.contains("set_aside"), "{config}");
	assert!(config.contains("changes"), "{config}");
	assert!(state.contains("120"), "{state}");
	let back = Model::load(m.spec.clone(), &config, &state);
	assert_eq!(back.values, m.values, "{config}\n{state}");
}

// Test ID: EsFc30P
#[test]
fn a_bad_value_in_the_file_is_dropped() {
	let m = demo();
	let config = "text:\n\tscrim:\n\t\tstrength: 400\nfont:\n\tfamily: \"Fira Code\"\ntheme:\n\tname: plaid\n";
	let back = Model::load(m.spec.clone(), config, "");
	assert!(!back.values.own.contains_key("strength"), "out of range");
	assert!(!back.values.own.contains_key("theme"), "not a theme");
	assert_eq!(back.values.own.get("family"), Some(&text("Fira Code")));
}

// Test ID: EsFc30Q
#[test]
fn slider_scales_round_trip_and_snap_to_detents() {
	let m = demo();
	for id in ["ease_in", "ramp_down", "blur", "idle_min", "line_height"] {
		let s = m.spec.at(id);
		if s.kind == Kind::Int {
			// a whole number won't land back on every position, but every value
			// has to come back as itself
			for v in (s.min as i64)..=(s.max as i64).min(2000) {
				let back = s.from_t(s.to_t(v as f64));
				assert_eq!(back, v as f64, "{id} at {v}");
			}
			continue;
		}
		for t in [0.0, 0.1, 0.25, 0.5, 0.9, 1.0] {
			let back = s.to_t(s.from_t(t));
			assert!((back - t).abs() <= 1e-9, "{id} at {t}: {back}");
		}
	}
	let idle = m.spec.at("idle_min");
	assert!(idle.to_t(60.0) > 0.4, "log spreads out the low end");
	let ramp = m.spec.at("ramp_down");
	assert!(ramp.to_t(50.0) < 0.2, "exp spreads out the high end");
	let hour = idle.to_t(60.0);
	assert_eq!(idle.snap_t(hour + 0.01, 0.02), hour);
	assert_eq!(idle.snap_t(hour + 0.1, 0.02), hour + 0.1);
}

// Test ID: EsFdZzC
#[test]
fn custom_starts_as_the_preset_not_as_what_a_rule_shows_over_it() {
	let mut m = demo();
	m.fill_custom("theme", &DESK);
	assert_eq!(
		m.values.own.get("fg"),
		Some(&text("#d0d0d0")),
		"Dark's, not the wallpaper's"
	);
	let (config, _) = m.save(&DESK);
	assert!(
		!config.contains("set_aside"),
		"a Custom value is not set aside:\n{config}"
	);
}

// Test ID: EsFdZzD
#[test]
fn the_kept_block_comes_last() {
	let mut m = demo();
	m.set("family", &text("Fira Code"), &DESK);
	m.set("use_system_family", &Value::Bool(true), &DESK);
	m.set("bg", &text("#000000"), &DESK);
	let (config, _) = m.save(&DESK);
	let kept = config.find("kept:").unwrap_or(0);
	assert!(
		kept > 0
			&& config[kept..]
				.lines()
				.skip(1)
				.all(|l| l.is_empty() || l.starts_with('\t')),
		"{config}"
	);
}

// Test ID: EsFdq3f
#[test]
fn turning_choose_automatically_back_on_drops_the_changes() {
	let mut m = demo();
	m.set("strength", &int(80), &DESK);
	assert_eq!(m.shown_choice("profile", &DESK), "High *");
	m.set("choose_auto", &Value::Bool(true), &DESK);
	assert_eq!(m.shown_choice("profile", &DESK), "High");
	m.set("strength", &int(80), &DESK);
	m.reset("choose_auto", &DESK);
	assert_eq!(
		m.shown_choice("profile", &DESK),
		"High",
		"the reset arrow too"
	);
}

// A spec a program builds in code: a theme group with presets of its own, and
// an opener whose default is the desktop's.
struct Shop {
	dark: bool,
}

impl Env for Shop {
	fn rule(&self, name: &str) -> Option<Value> {
		(name == "desktop_opener").then(|| text("xdg-open"))
	}
	fn preset(&self, group: &str, key: &str) -> Option<Preset> {
		if group != "theme" || !["silk", "amber"].contains(&key) {
			return None;
		}
		let bg = match (key, self.dark) {
			("silk", true) => "#101014",
			("silk", false) => "#f4f4f0",
			_ => "#201000",
		};
		Some(Preset {
			key: key.into(),
			label: if key == "silk" { "SilkTerm" } else { "Amber" }.into(),
			temporary: false,
			values: [("bg".to_string(), text(bg))].into(),
		})
	}
}

fn plain(id: &str, kind: Kind, path: &str, default: Value) -> Setting {
	Setting {
		id: id.into(),
		label: id.into(),
		tab: 0,
		control: match kind {
			Kind::Bool => Control::Checkbox,
			Kind::Choice => Control::Dropdown,
			Kind::Color => Control::Color,
			_ => Control::Text,
		},
		kind,
		path: path.into(),
		store: Store::Config,
		default,
		min: f64::NEG_INFINITY,
		max: f64::INFINITY,
		scale: Scale::Linear,
		detents: Vec::new(),
		options: Vec::new(),
		tip: String::new(),
		indent: u8::MAX,
		gate: None,
		auto: None,
		rule: None,
		group: None,
	}
}

fn shop() -> Model {
	let mut opener = plain("opener", Kind::Text, "links.opener", text(""));
	opener.rule = Some(Rule::Named("desktop_opener".into()));
	let mut bg = plain("bg", Kind::Color, "colors.bg", text("#000000"));
	bg.group = Some("theme".into());
	let spec = Spec {
		tabs: vec![Tab {
			path: "Look".into(),
			label: "Look".into(),
			parent: None,
		}],
		settings: vec![
			plain("theme", Kind::Choice, "theme", text("silk")),
			bg,
			opener,
		],
		groups: vec![Group {
			id: "theme".into(),
			chooser: "theme".into(),
			noun: "theme".into(),
			custom: Some("Custom".into()),
			presets: Vec::new(),
			from_program: true,
			members: Vec::new(),
		}],
	};
	Model::new(spec.finish().unwrap_or_else(|e| panic!("{e:#?}")))
}

// Test ID: EsFwIor
#[test]
fn a_spec_built_in_code_is_checked_like_a_parsed_one() {
	let m = shop();
	assert_eq!(
		m.spec.group("theme").map(|g| g.members.clone()),
		Some(vec!["bg".to_string()])
	);
	let mut broken = (*m.spec).clone();
	broken.settings[1].gate = Some("nothing".into());
	broken.groups[0].from_program = false;
	let errs = broken.finish().err().unwrap_or_default().join("\n");
	assert!(errs.contains("gate nothing"), "{errs}");
	assert!(errs.contains("no presets"), "{errs}");
}

// Test ID: EsFwIuv
#[test]
fn a_rule_with_no_switch_is_the_default_and_the_arrow_puts_it_back() {
	let mut m = shop();
	let env = Shop { dark: true };
	assert_eq!(m.value("opener", &env), text("xdg-open"));
	assert!(!m.can_reset("opener", &env));
	m.set("opener", &text("xdg-open"), &env);
	assert!(
		m.can_reset("opener", &env),
		"typed in, so it no longer follows the desktop"
	);
	assert_eq!(
		m.state_line("opener", &env),
		"Set by hand. Default value: xdg-open."
	);
	m.reset("opener", &env);
	assert_eq!(m.value("opener", &env), text("xdg-open"));
	assert!(m.values.own.is_empty());
}

// Test ID: EsFwIzk
#[test]
fn a_group_s_presets_can_come_from_the_program() {
	let mut m = shop();
	let dark = Shop { dark: true };
	let light = Shop { dark: false };
	assert_eq!(m.value("bg", &dark), text("#101014"));
	assert_eq!(
		m.value("bg", &light),
		text("#f4f4f0"),
		"the program's answer moves"
	);
	m.set("bg", &text("#123456"), &dark);
	assert_eq!(m.shown_choice("theme", &dark), "SilkTerm *");
	assert_eq!(
		m.value("bg", &light),
		text("#123456"),
		"a change holds in both"
	);
	let back = Model::load(Arc::clone(&m.spec), &m.save(&dark).0, "");
	assert_eq!(back.values, m.values);
	m.set("theme", &text("amber"), &dark);
	assert_eq!(m.shown_choice("theme", &dark), "Amber");
	m.set("theme", &text(CUSTOM), &dark);
	assert_eq!(m.shown_choice("theme", &dark), "Custom");
	assert_eq!(
		m.value("bg", &dark),
		text("#000000"),
		"nothing of its own yet"
	);
}

// Test ID: EsFwJ4B
#[test]
fn changes_under_a_temporary_preset_are_still_saved() {
	let mut m = demo();
	m.set("profile", &text("low"), &DESK);
	m.set("radius", &int(9), &DESK);
	m.temporary("perf", Some("remote"));
	assert_eq!(m.shown_choice("profile", &DESK), "Remote (temporary)");
	let (config, _) = m.save(&DESK);
	assert!(config.contains("radius: 9"), "{config}");
	m.temporary("perf", None);
	assert_eq!(m.shown_choice("profile", &DESK), "Low *");
}

// Test ID: EsFwJ8T
#[test]
fn lines_put_set_aside_values_and_changes_under_kept() {
	let mut m = demo();
	m.set("family", &text("Fira Code"), &DESK);
	m.set("use_system_family", &Value::Bool(true), &DESK);
	m.set("strength", &int(80), &DESK);
	let lines = m.lines(&DESK);
	let kept: Vec<&str> = lines.kept.iter().map(|(p, _)| p.as_str()).collect();
	assert_eq!(
		kept,
		[
			"kept.set_aside.font.family",
			"kept.changes.perf.preset",
			"kept.changes.perf.values.text.scrim.strength"
		]
	);
	assert!(
		lines
			.config
			.iter()
			.any(|(p, v)| p == "font.use_system_family" && *v == Value::Bool(true))
	);
	assert!(!lines.config.iter().any(|(p, _)| p == "font.family"));
}
