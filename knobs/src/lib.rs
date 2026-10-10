// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Settings declared in shcl, and what a change to one does to the others.
//!
//! Three parts, top to bottom: the spec (parsed once from a compiled-in file),
//! the stored values with the rules for reading and changing them, and the
//! files they are saved to. Nothing here draws anything. `demo/demo.shcl`
//! documents the spec format.

#![allow(clippy::must_use_candidate)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
	Bool(bool),
	Int(i64),
	Float(f64),
	Text(String),
}

impl Value {
	pub fn as_bool(&self) -> bool {
		matches!(self, Value::Bool(true))
	}
	pub fn as_f64(&self) -> f64 {
		match self {
			Value::Int(i) => *i as f64,
			Value::Float(f) => *f,
			_ => 0.0,
		}
	}
	pub fn as_text(&self) -> &str {
		match self {
			Value::Text(s) => s,
			_ => "",
		}
	}
	/// The value as a person would read it in a tip.
	pub fn show(&self) -> String {
		match self {
			Value::Bool(true) => "on".into(),
			Value::Bool(false) => "off".into(),
			Value::Int(i) => i.to_string(),
			Value::Float(f) => {
				let s = format!("{f:.2}");
				s.trim_end_matches('0').trim_end_matches('.').to_string()
			}
			Value::Text(s) if s.is_empty() => "(empty)".into(),
			Value::Text(s) => s.clone(),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
	Heading,
	Checkbox,
	Slider,
	Number,
	Dropdown,
	Text,
	Color,
	File,
	None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Bool,
	Int,
	Float,
	Text,
	Color,
	Choice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
	Linear,
	Log,
	Exp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Store {
	Config,
	State,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
	/// A stored state value, by setting id.
	State(String),
	/// Answered by the program at run time, see [`Env`].
	Named(String),
}

#[derive(Clone, Debug)]
pub struct Detent {
	pub value: f64,
	pub label: String,
}

#[derive(Clone, Debug)]
pub struct Setting {
	pub id: String,
	pub label: String,
	/// Index into [`Spec::tabs`]. Unused for `Control::None`.
	pub tab: usize,
	pub control: Control,
	pub kind: Kind,
	pub path: String,
	pub store: Store,
	pub default: Value,
	pub min: f64,
	pub max: f64,
	pub scale: Scale,
	pub detents: Vec<Detent>,
	/// Key and label. A group's chooser gets its presets here, then Custom.
	pub options: Vec<(String, String)>,
	pub tip: String,
	pub indent: u8,
	pub gate: Option<String>,
	pub auto: Option<String>,
	pub rule: Option<Rule>,
	pub group: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Tab {
	/// "Look/Text" for Text under Look.
	pub path: String,
	pub label: String,
	pub parent: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Preset {
	pub key: String,
	pub label: String,
	pub temporary: bool,
	pub values: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub struct Group {
	pub id: String,
	pub chooser: String,
	pub noun: String,
	pub custom: Option<String>,
	pub presets: Vec<Preset>,
	pub members: Vec<String>,
}

/// The key a group's own set of values is stored under.
pub const CUSTOM: &str = "custom";

impl Group {
	pub fn preset(&self, key: &str) -> Option<&Preset> {
		self.presets.iter().find(|p| p.key == key)
	}
}

#[derive(Clone, Debug, Default)]
pub struct Spec {
	pub tabs: Vec<Tab>,
	pub settings: Vec<Setting>,
	pub groups: Vec<Group>,
}

// Block names a preset uses for itself, so no setting may have them as an id.
const PRESET_WORDS: &[&str] = &["label", "temporary"];

impl Spec {
	pub fn get(&self, id: &str) -> Option<&Setting> {
		self.settings.iter().find(|s| s.id == id)
	}
	fn at(&self, id: &str) -> &Setting {
		// ids come from the spec itself, checked at parse time
		self.get(id).unwrap_or_else(|| panic!("no setting {id}"))
	}
	pub fn group(&self, id: &str) -> Option<&Group> {
		self.groups.iter().find(|g| g.id == id)
	}
	/// The group a dropdown picks for, if it is a chooser.
	pub fn chooser_of(&self, id: &str) -> Option<&Group> {
		self.groups.iter().find(|g| g.chooser == id)
	}
	/// The settings a checkbox makes automatic.
	pub fn under_switch(&self, id: &str) -> Vec<&str> {
		self.settings
			.iter()
			.filter(|s| s.auto.as_deref() == Some(id))
			.map(|s| s.id.as_str())
			.collect()
	}
	pub fn is_switch(&self, id: &str) -> bool {
		self.settings.iter().any(|s| s.auto.as_deref() == Some(id))
	}
	/// Tabs directly under `parent`, or the top row for `None`.
	pub fn tabs_under(&self, parent: Option<usize>) -> Vec<usize> {
		(0..self.tabs.len())
			.filter(|&i| self.tabs[i].parent == parent)
			.collect()
	}

	/// Parse a spec.
	///
	/// # Errors
	///
	/// Every mistake in it, not only the first.
	pub fn parse(text: &str) -> Result<Spec, Vec<String>> {
		let doc = shcl::Document::parse(text);
		let mut errs: Vec<String> = doc
			.diagnostics()
			.iter()
			.filter(|d| d.severity == shcl::Severity::Error)
			.map(|d| format!("line {}: {}", d.line, d.message))
			.collect();
		let mut spec = Spec::default();

		for path in doc.get_string_array("tabs").unwrap_or_default() {
			let (parent, label) = match path.rsplit_once('/') {
				Some((up, label)) => {
					let Some(p) = spec.tabs.iter().position(|t| t.path == up) else {
						errs.push(format!("tab {path}: {up} must come first"));
						continue;
					};
					(Some(p), label.to_string())
				}
				None => (None, path.clone()),
			};
			spec.tabs.push(Tab {
				path,
				label,
				parent,
			});
		}

		for id in doc.children("settings") {
			match read_setting(&doc, &id, &spec.tabs) {
				Ok(s) => spec.settings.push(s),
				Err(e) => errs.push(format!("{id}: {e}")),
			}
		}
		for id in doc.children("groups") {
			match read_group(&doc, &id, &spec.settings) {
				Ok(g) => spec.groups.push(g),
				Err(e) => errs.extend(e.into_iter().map(|e| format!("group {id}: {e}"))),
			}
		}
		// a chooser lists its presets
		for g in &spec.groups {
			let mut options: Vec<(String, String)> = g
				.presets
				.iter()
				.map(|p| (p.key.clone(), p.label.clone()))
				.collect();
			if let Some(label) = &g.custom {
				options.push((CUSTOM.into(), label.clone()));
			}
			if let Some(c) = spec.settings.iter_mut().find(|s| s.id == g.chooser) {
				c.options = options;
			}
		}
		errs.extend(spec.check_links());
		if errs.is_empty() {
			spec.set_indents();
			Ok(spec)
		} else {
			Err(errs)
		}
	}

	fn check_links(&self) -> Vec<String> {
		let mut errs = Vec::new();
		let mut paths: BTreeMap<(&str, Store), &str> = BTreeMap::new();
		for s in &self.settings {
			let id = &s.id;
			if PRESET_WORDS.contains(&id.as_str()) {
				errs.push(format!("{id}: that id is taken by presets"));
			}
			if s.control != Control::Heading {
				if s.path.is_empty() {
					errs.push(format!("{id}: no path"));
				} else if s.path == "kept" || s.path.starts_with("kept.") {
					errs.push(format!("{id}: kept is where set-aside values go"));
				} else if let Some(other) = paths.insert((s.path.as_str(), s.store), id) {
					errs.push(format!("{id}: path {} is {other}'s too", s.path));
				}
				if let Err(e) = self.valid(s, &s.default) {
					errs.push(format!("{id}: default {e}"));
				}
			}
			let is_bool = |other: &str| self.get(other).is_some_and(|o| o.kind == Kind::Bool);
			if let Some(g) = &s.gate {
				if !is_bool(g) {
					errs.push(format!("{id}: gate {g} is not a checkbox"));
				}
			}
			match (&s.auto, &s.rule) {
				(Some(a), Some(rule)) => {
					if !is_bool(a) {
						errs.push(format!("{id}: auto {a} is not a checkbox"));
					} else if self.at(a).auto.is_some() {
						errs.push(format!("{id}: auto {a} is automatic itself"));
					}
					if let Rule::State(from) = rule {
						if self.get(from).is_none_or(|f| f.store != Store::State) {
							errs.push(format!(
								"{id}: rule names {from}, which is not a state value"
							));
						}
					}
				}
				(Some(_), None) => errs.push(format!("{id}: auto with no rule")),
				(None, Some(_)) => errs.push(format!("{id}: rule with no auto")),
				(None, None) => {}
			}
			if s.store == Store::State
				&& (s.auto.is_some() || s.group.is_some() || s.gate.is_some())
			{
				errs.push(format!("{id}: a state value has no relations"));
			}
			if let Some(g) = &s.group {
				if self.group(g).is_none() {
					errs.push(format!("{id}: no group {g}"));
				}
				if self.chooser_of(id).is_some() {
					errs.push(format!("{id}: a chooser can't be in a group"));
				}
			}
			if s.control == Control::Slider
				&& !(s.min < s.max && s.min.is_finite() && s.max.is_finite())
			{
				errs.push(format!("{id}: a slider needs a range"));
			}
			for d in &s.detents {
				if d.value < s.min || d.value > s.max {
					errs.push(format!("{id}: detent {} is out of range", d.value));
				}
			}
		}
		// gates can chain, but not in a loop
		for s in &self.settings {
			let mut at = s.gate.clone();
			let mut hops = 0;
			while let Some(g) = at {
				hops += 1;
				if g == s.id || hops > self.settings.len() {
					errs.push(format!("{}: its gates go round in a loop", s.id));
					break;
				}
				at = self.get(&g).and_then(|o| o.gate.clone());
			}
		}
		for g in &self.groups {
			match self.get(&g.chooser) {
				Some(c) if c.control == Control::Dropdown => {
					if g.preset(c.default.as_text()).is_none() && c.default.as_text() != CUSTOM {
						errs.push(format!("group {}: chooser default is not a preset", g.id));
					}
				}
				_ => errs.push(format!(
					"group {}: chooser {} is not a dropdown",
					g.id, g.chooser
				)),
			}
		}
		errs
	}

	// One step in for each relation above a setting, unless it says otherwise.
	fn set_indents(&mut self) {
		let depth: Vec<u8> = (0..self.settings.len())
			.map(|i| {
				let mut n = 0u8;
				let mut at = &self.settings[i];
				while let Some(up) = at.gate.as_deref().or(at.auto.as_deref()) {
					n += 1;
					match self.get(up) {
						Some(next) if n < 8 => at = next,
						_ => break,
					}
				}
				n
			})
			.collect();
		for (s, d) in self.settings.iter_mut().zip(depth) {
			if s.indent == u8::MAX {
				s.indent = d;
			}
		}
	}

	/// A value checked against what the setting takes. Ints and floats turn
	/// into each other; nothing else is converted.
	///
	/// # Errors
	///
	/// Why the value doesn't fit.
	pub fn valid(&self, s: &Setting, v: &Value) -> Result<Value, String> {
		let in_range = |x: f64| {
			if x < s.min || x > s.max {
				Err(format!(
					"{} is outside {}..{}",
					Value::Float(x).show(),
					s.min,
					s.max
				))
			} else {
				Ok(())
			}
		};
		match (s.kind, v) {
			(Kind::Bool, Value::Bool(_)) | (Kind::Text, Value::Text(_)) => Ok(v.clone()),
			(Kind::Int, Value::Int(_) | Value::Float(_)) => {
				let x = v.as_f64().round();
				in_range(x)?;
				Ok(Value::Int(x as i64))
			}
			(Kind::Float, Value::Int(_) | Value::Float(_)) => {
				in_range(v.as_f64())?;
				Ok(Value::Float(v.as_f64()))
			}
			(Kind::Color, Value::Text(t)) if is_color(t) => Ok(Value::Text(t.to_ascii_lowercase())),
			(Kind::Choice, Value::Text(t)) => {
				if s.options.iter().any(|(k, _)| k == t)
					|| (self.chooser_of(&s.id).is_some() && t == CUSTOM)
				{
					Ok(v.clone())
				} else {
					Err(format!("{t} is not one of the choices"))
				}
			}
			_ => Err(format!("{} is the wrong type", v.show())),
		}
	}
}

fn is_color(t: &str) -> bool {
	t.len() == 7 && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn read_setting(doc: &shcl::Document, id: &str, tabs: &[Tab]) -> Result<Setting, String> {
	let at = |key: &str| format!("settings.{id}.{key}");
	let text = |key: &str| doc.get_string(&at(key)).unwrap_or_default();
	let control = match text("control").as_str() {
		"heading" => Control::Heading,
		"checkbox" => Control::Checkbox,
		"slider" => Control::Slider,
		"number" => Control::Number,
		"dropdown" => Control::Dropdown,
		"text" => Control::Text,
		"color" => Control::Color,
		"file" => Control::File,
		"none" => Control::None,
		other => return Err(format!("control {other:?} is not one I know")),
	};
	let whole = doc.get_bool(&at("whole")).unwrap_or(false);
	let kind = match text("type").as_str() {
		"bool" => Kind::Bool,
		"int" => Kind::Int,
		"float" => Kind::Float,
		"text" => Kind::Text,
		"color" => Kind::Color,
		"choice" => Kind::Choice,
		"" => match control {
			Control::Checkbox => Kind::Bool,
			Control::Slider | Control::Number if whole => Kind::Int,
			Control::Slider | Control::Number => Kind::Float,
			Control::Dropdown => Kind::Choice,
			Control::Color => Kind::Color,
			Control::Heading | Control::Text | Control::File => Kind::Text,
			Control::None => return Err("a control of none needs a type".into()),
		},
		other => return Err(format!("type {other:?} is not one I know")),
	};
	let tab = if matches!(control, Control::None) {
		0
	} else {
		let name = text("tab");
		tabs.iter()
			.position(|t| t.path == name)
			.ok_or_else(|| format!("no tab {name:?}"))?
	};
	let range = numbers(doc, &at("range"));
	let (min, max) = match range.as_slice() {
		[] => (f64::NEG_INFINITY, f64::INFINITY),
		[lo, hi] => (*lo, *hi),
		_ => return Err("range wants 2 numbers".into()),
	};
	let labels = doc
		.get_string_array(&at("detent_labels"))
		.unwrap_or_default();
	let detents = numbers(doc, &at("detents"))
		.into_iter()
		.enumerate()
		.map(|(i, value)| Detent {
			value,
			label: labels.get(i).cloned().unwrap_or_default(),
		})
		.collect();
	let keys = doc.get_string_array(&at("options")).unwrap_or_default();
	let shown = doc
		.get_string_array(&at("option_labels"))
		.unwrap_or_default();
	let options = keys
		.iter()
		.enumerate()
		.map(|(i, k)| (k.clone(), shown.get(i).unwrap_or(k).clone()))
		.collect();
	let default = match kind {
		_ if control == Control::Heading => Value::Text(String::new()),
		Kind::Bool => Value::Bool(doc.get_bool(&at("default")).unwrap_or(false)),
		Kind::Int => Value::Int(doc.get_int(&at("default")).unwrap_or(0)),
		Kind::Float => Value::Float(number(doc, &at("default")).unwrap_or(0.0)),
		Kind::Text | Kind::Color | Kind::Choice => Value::Text(text("default")),
	};
	let scale = match text("scale").as_str() {
		"" | "linear" => Scale::Linear,
		"log" => Scale::Log,
		"exp" => Scale::Exp,
		other => return Err(format!("scale {other:?} is not one I know")),
	};
	let store = match text("store").as_str() {
		"" | "config" => Store::Config,
		"state" => Store::State,
		other => return Err(format!("store {other:?} is not one I know")),
	};
	let rule = doc
		.get_string(&at("rule"))
		.ok()
		.map(|r| match r.strip_prefix("state:") {
			Some(from) => Rule::State(from.to_string()),
			None => Rule::Named(r),
		});
	let opt = |key: &str| doc.get_string(&at(key)).ok();
	Ok(Setting {
		id: id.to_string(),
		label: text("label"),
		tab,
		control,
		kind,
		path: text("path"),
		store,
		default,
		min,
		max,
		scale,
		detents,
		options,
		tip: text("tip"),
		indent: doc
			.get_int(&at("indent"))
			.map_or(u8::MAX, |n| n.clamp(0, 8) as u8),
		gate: opt("gate"),
		auto: opt("auto"),
		rule,
		group: opt("group"),
	})
}

// shcl keeps ints and floats apart; a spec author shouldn't have to.
fn number(doc: &shcl::Document, path: &str) -> Option<f64> {
	doc.get_float(path)
		.ok()
		.or_else(|| doc.get_int(path).ok().map(|i| i as f64))
}

fn numbers(doc: &shcl::Document, path: &str) -> Vec<f64> {
	if let Ok(v) = doc.get_float_array(path) {
		return v;
	}
	if let Ok(v) = doc.get_int_array(path) {
		return v.into_iter().map(|i| i as f64).collect();
	}
	// a mixed list, like 0.8, 2
	doc.get_string_array(path)
		.unwrap_or_default()
		.iter()
		.filter_map(|s| s.trim().parse().ok())
		.collect()
}

fn read_group(doc: &shcl::Document, id: &str, settings: &[Setting]) -> Result<Group, Vec<String>> {
	let at = |key: &str| format!("groups.{id}.{key}");
	let mut errs = Vec::new();
	let members: Vec<String> = settings
		.iter()
		.filter(|s| s.group.as_deref() == Some(id))
		.map(|s| s.id.clone())
		.collect();
	let mut presets = Vec::new();
	for key in doc.children(&at("presets")) {
		let p = |k: &str| format!("groups.{id}.presets.{key}.{k}");
		let mut values = BTreeMap::new();
		for name in doc.children(&at(&format!("presets.{key}"))) {
			if PRESET_WORDS.contains(&name.as_str()) {
				continue;
			}
			let Some(s) = settings.iter().find(|s| s.id == name) else {
				errs.push(format!("preset {key}: no setting {name}"));
				continue;
			};
			if s.group.as_deref() != Some(id) {
				errs.push(format!("preset {key}: {name} is not in this group"));
				continue;
			}
			let path = p(&name);
			let raw = match s.kind {
				Kind::Bool => doc.get_bool(&path).map(Value::Bool).ok(),
				Kind::Int | Kind::Float => number(doc, &path).map(Value::Float),
				_ => doc.get_string(&path).map(Value::Text).ok(),
			};
			match raw.map(|v| Spec::default().valid(s, &v)) {
				Some(Ok(v)) => {
					values.insert(name, v);
				}
				Some(Err(e)) => errs.push(format!("preset {key}: {name} {e}")),
				None => errs.push(format!("preset {key}: {name} has no value I can read")),
			}
		}
		for m in &members {
			if !values.contains_key(m) {
				errs.push(format!("preset {key}: no value for {m}"));
			}
		}
		presets.push(Preset {
			label: doc.get_string(&p("label")).unwrap_or_else(|_| key.clone()),
			temporary: doc.get_bool(&p("temporary")).unwrap_or(false),
			key,
			values,
		});
	}
	if presets.is_empty() {
		errs.push("no presets".into());
	}
	if !errs.is_empty() {
		return Err(errs);
	}
	Ok(Group {
		id: id.to_string(),
		chooser: doc.get_string(&at("chooser")).unwrap_or_default(),
		noun: doc
			.get_string(&at("noun"))
			.unwrap_or_else(|_| "preset".into()),
		custom: doc.get_string(&at("custom")).ok(),
		presets,
		members,
	})
}

impl Setting {
	/// Slider position, 0..1, for a value.
	pub fn to_t(&self, v: f64) -> f64 {
		let span = self.max - self.min;
		if span <= 0.0 {
			return 0.0;
		}
		let t = match self.scale {
			Scale::Linear => (v - self.min) / span,
			Scale::Log => (1.0 + v - self.min).ln() / (1.0 + span).ln(),
			Scale::Exp => 1.0 - (1.0 + self.max - v).ln() / (1.0 + span).ln(),
		};
		t.clamp(0.0, 1.0)
	}
	/// The value at a slider position, rounded for an int.
	pub fn from_t(&self, t: f64) -> f64 {
		let t = t.clamp(0.0, 1.0);
		let span = self.max - self.min;
		let v = match self.scale {
			Scale::Linear => self.min + t * span,
			Scale::Log => self.min + (1.0 + span).powf(t) - 1.0,
			Scale::Exp => self.max - (1.0 + span).powf(1.0 - t) + 1.0,
		};
		let v = v.clamp(self.min, self.max);
		if self.kind == Kind::Int { v.round() } else { v }
	}
	/// A slider position pulled onto a detent within `near` of it.
	pub fn snap_t(&self, t: f64, near: f64) -> f64 {
		self.detents
			.iter()
			.map(|d| self.to_t(d.value))
			.filter(|dt| (dt - t).abs() <= near)
			.min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
			.unwrap_or(t)
	}
}

/// What the program answers for a named rule: the desktop's font, colors
/// picked from the wallpaper. `None` when there is nothing to follow.
pub trait Env {
	fn rule(&self, name: &str) -> Option<Value>;
}

/// No answers, for code that never reaches a named rule.
pub struct NoEnv;
impl Env for NoEnv {
	fn rule(&self, _: &str) -> Option<Value> {
		None
	}
}

/// Everything stored, by setting id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values {
	/// The person's own values. For a group member it is the Custom set.
	pub own: BTreeMap<String, Value>,
	/// Changes made while a preset is in force, the "*" ones.
	pub changes: BTreeMap<String, Value>,
	/// Group id to the preset its changes were made on.
	pub changed_on: BTreeMap<String, String>,
	pub state: BTreeMap<String, Value>,
	/// Group id to a temporary preset. Never saved.
	pub session: BTreeMap<String, String>,
}

/// Where a setting's value comes from right now, for its tip.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
	Default,
	Own,
	/// From its rule. The value set by hand, if any, is kept.
	Automatic {
		kept: Option<Value>,
	},
	Preset {
		preset: String,
		noun: String,
	},
	Changed {
		preset: String,
		noun: String,
		was: Value,
	},
	State,
}

pub struct Model {
	pub spec: Spec,
	pub values: Values,
}

impl Model {
	pub fn new(spec: Spec) -> Model {
		Model {
			spec,
			values: Values::default(),
		}
	}

	pub fn value(&self, id: &str, env: &dyn Env) -> Value {
		let s = self.spec.at(id);
		if s.store == Store::State {
			return self
				.values
				.state
				.get(id)
				.cloned()
				.unwrap_or_else(|| s.default.clone());
		}
		if let Some(v) = self.automatic(s, env) {
			return v;
		}
		self.base(s, env)
	}

	// The rule's value while the auto checkbox is on and the rule has an answer.
	fn automatic(&self, s: &Setting, env: &dyn Env) -> Option<Value> {
		let sw = s.auto.as_deref()?;
		if !self.value(sw, env).as_bool() {
			return None;
		}
		let v = match s.rule.as_ref()? {
			Rule::State(from) => self.value(from, env),
			Rule::Named(name) => env.rule(name)?,
		};
		self.spec.valid(s, &v).ok()
	}

	// The value with no rule in play: a preset's, a change to it, or the own.
	fn base(&self, s: &Setting, env: &dyn Env) -> Value {
		if let Some((key, p)) = self.preset_for(s, env) {
			if !p.temporary
				&& self.values.changed_on.get(s.group.as_deref().unwrap_or("")) == Some(&key)
			{
				if let Some(v) = self.values.changes.get(&s.id) {
					return v.clone();
				}
			}
			return p
				.values
				.get(&s.id)
				.cloned()
				.unwrap_or_else(|| s.default.clone());
		}
		self.values
			.own
			.get(&s.id)
			.cloned()
			.unwrap_or_else(|| s.default.clone())
	}

	// The preset a group member follows, when it isn't Custom.
	fn preset_for<'a>(&'a self, s: &Setting, env: &dyn Env) -> Option<(String, &'a Preset)> {
		let g = self.spec.group(s.group.as_deref()?)?;
		let key = self.chosen(&g.id, env);
		g.preset(&key).map(|p| (key, p))
	}

	/// The preset key a group is on, or [`CUSTOM`].
	pub fn chosen(&self, group: &str, env: &dyn Env) -> String {
		if let Some(t) = self.values.session.get(group) {
			return t.clone();
		}
		match self.spec.group(group) {
			Some(g) => self.value(&g.chooser, env).as_text().to_string(),
			None => String::new(),
		}
	}

	/// The settings changed on top of the group's preset, in spec order.
	pub fn changed(&self, group: &str, env: &dyn Env) -> Vec<&str> {
		let key = self.chosen(group, env);
		if self.values.changed_on.get(group) != Some(&key) {
			return Vec::new();
		}
		self.spec
			.settings
			.iter()
			.filter(|s| {
				s.group.as_deref() == Some(group) && self.values.changes.contains_key(&s.id)
			})
			.map(|s| s.id.as_str())
			.collect()
	}

	/// What a dropdown shows for its current value. A chooser shows its
	/// preset, with " *" when anything was changed on top of it.
	pub fn shown_choice(&self, id: &str, env: &dyn Env) -> String {
		let s = self.spec.at(id);
		let key = match self.spec.chooser_of(id) {
			Some(g) => self.chosen(&g.id, env),
			None => self.value(id, env).as_text().to_string(),
		};
		let label = s
			.options
			.iter()
			.find(|(k, _)| *k == key)
			.map_or(key.clone(), |(_, l)| l.clone());
		match self.spec.chooser_of(id) {
			Some(g) if !self.changed(&g.id, env).is_empty() => format!("{label} *"),
			_ => label,
		}
	}

	/// Only counts while every gate above it is on.
	pub fn counts(&self, id: &str, env: &dyn Env) -> bool {
		let mut at = self.spec.at(id).gate.as_deref();
		while let Some(g) = at {
			if !self.value(g, env).as_bool() {
				return false;
			}
			at = self.spec.at(g).gate.as_deref();
		}
		true
	}

	pub fn source(&self, id: &str, env: &dyn Env) -> Source {
		let s = self.spec.at(id);
		if s.store == Store::State {
			return Source::State;
		}
		if self.automatic(s, env).is_some() {
			let kept = self
				.values
				.own
				.get(id)
				.filter(|v| **v != s.default)
				.cloned();
			return Source::Automatic {
				kept: if s.group.is_some() { None } else { kept },
			};
		}
		if let Some((_, p)) = self.preset_for(s, env) {
			let g = self
				.spec
				.group(s.group.as_deref().unwrap_or(""))
				.map(|g| g.noun.clone())
				.unwrap_or_default();
			let was = p
				.values
				.get(id)
				.cloned()
				.unwrap_or_else(|| s.default.clone());
			if self
				.changed(s.group.as_deref().unwrap_or(""), env)
				.contains(&id)
			{
				return Source::Changed {
					preset: p.label.clone(),
					noun: g,
					was,
				};
			}
			return Source::Preset {
				preset: p.label.clone(),
				noun: g,
			};
		}
		match self.values.own.get(id) {
			Some(v) if *v != s.default => Source::Own,
			_ => Source::Default,
		}
	}

	/// The tip: the description, a blank line, then where the value comes from.
	pub fn tip(&self, id: &str, env: &dyn Env) -> String {
		let mut out = self.spec.at(id).tip.clone();
		let line = self.state_line(id, env);
		if !line.is_empty() {
			if !out.is_empty() {
				out.push_str("\n\n");
			}
			out.push_str(&line);
		}
		out
	}

	/// One line on where a setting's value comes from. Empty for a default.
	pub fn state_line(&self, id: &str, env: &dyn Env) -> String {
		let s = self.spec.at(id);
		let mut line = match self.source(id, env) {
			Source::Default | Source::State => String::new(),
			Source::Own => format!("Set by hand. Default value: {}.", s.default.show()),
			Source::Automatic { kept: None } => "Automatic.".into(),
			Source::Automatic { kept: Some(v) } => {
				format!("Automatic. Your value, {}, is kept for later.", v.show())
			}
			Source::Preset { preset, noun } => format!("From the {preset} {noun}."),
			Source::Changed { preset, noun, was } => {
				format!("Changed. The {preset} {noun}'s value is {}.", was.show())
			}
		};
		if let Some(g) = self.spec.chooser_of(id) {
			let changed: Vec<&str> = self
				.changed(&g.id, env)
				.iter()
				.map(|c| self.spec.at(c).label.as_str())
				.collect();
			if !changed.is_empty() {
				line = format!(
					"Changed here: {}. Picking another {} drops these changes.",
					changed.join(", "),
					g.noun
				);
			}
		}
		line
	}

	/// Whether the reset arrow has anything to do.
	pub fn can_reset(&self, id: &str, env: &dyn Env) -> bool {
		let s = self.spec.at(id);
		match s.store {
			Store::State => self.values.state.contains_key(id),
			Store::Config => match self.preset_for(s, env) {
				Some((_, p)) if !p.temporary => self
					.changed(s.group.as_deref().unwrap_or(""), env)
					.contains(&id),
				Some(_) => false,
				None => self.values.own.get(id).is_some_and(|v| *v != s.default),
			},
		}
	}

	/// The reset arrow: a change goes back to its preset's value, anything else
	/// to its default. It is the one way a value set by hand is thrown away.
	pub fn reset(&mut self, id: &str, env: &dyn Env) {
		let s = self.spec.at(id).clone();
		if s.store == Store::State {
			self.values.state.remove(id);
			return;
		}
		if self.preset_for(&s, env).is_some() {
			self.values.changes.remove(id);
			return;
		}
		self.values.own.remove(id);
		self.picking_again(id, env);
	}

	// Turning on a chooser's auto checkbox is a pick like any other, so the
	// "*" changes go.
	fn picking_again(&mut self, id: &str, env: &dyn Env) {
		let group = self
			.spec
			.groups
			.iter()
			.find(|g| self.spec.at(&g.chooser).auto.as_deref() == Some(id))
			.map(|g| g.id.clone());
		if let Some(g) = group {
			if self.value(id, env).as_bool() {
				self.drop_changes(&g);
			}
		}
	}

	/// A change made on screen, with everything it does to the settings around it.
	pub fn set(&mut self, id: &str, v: &Value, env: &dyn Env) {
		let s = self.spec.at(id).clone();
		let Ok(v) = self.spec.valid(&s, v) else {
			return;
		};
		if s.store == Store::State {
			self.values.state.insert(s.id, v);
			return;
		}
		if let Some(gr) = self.spec.chooser_of(id) {
			// picking drops the "*" changes, and a temporary pick stores nothing
			let g = gr.id.clone();
			if gr.preset(v.as_text()).is_some_and(|p| p.temporary) {
				self.values.session.insert(g, v.as_text().to_string());
				return;
			}
			self.values.session.remove(&g);
			self.drop_changes(&g);
		}
		if let Some(gate) = &s.gate {
			if !self.value(gate, env).as_bool() {
				self.set(gate, &Value::Bool(true), env);
			}
		}
		if let Some(sw) = s.auto.clone() {
			if self.value(&sw, env).as_bool() {
				// the others under the checkbox keep what they show
				let others: Vec<(String, Value)> = self
					.spec
					.under_switch(&sw)
					.into_iter()
					.filter(|m| *m != id)
					.map(|m| (m.to_string(), self.value(m, env)))
					.collect();
				self.write(&sw, Value::Bool(false), env);
				for (m, shown) in others {
					self.write(&m, shown, env);
				}
			}
		}
		if self.spec.is_switch(id) && !v.as_bool() && self.value(id, env).as_bool() {
			// one with nothing of its own to fall back on keeps what it shows
			let keep: Vec<(String, Value)> = self
				.spec
				.under_switch(id)
				.into_iter()
				.filter(|m| self.spec.at(m).group.is_none() && !self.values.own.contains_key(*m))
				.map(|m| (m.to_string(), self.value(m, env)))
				.collect();
			self.write(id, v, env);
			for (m, shown) in keep {
				self.values.own.insert(m, shown);
			}
			return;
		}
		self.write(id, v, env);
		self.picking_again(id, env);
	}

	// Store a value where its setting keeps one right now: as a change while
	// a preset is in force, or as its own.
	fn write(&mut self, id: &str, v: Value, env: &dyn Env) {
		let s = self.spec.at(id).clone();
		if let Some(g) = s.group.clone() {
			// a change ends a temporary preset, which would hide it
			self.values.session.remove(&g);
			if let Some((key, p)) = self.preset_for(&s, env) {
				let same = p.values.get(id) == Some(&v);
				self.pin_chooser(&g, env);
				if self.values.changed_on.get(&g) != Some(&key) {
					self.drop_changes(&g);
					self.values.changed_on.insert(g, key);
				}
				if same {
					self.values.changes.remove(id);
				} else {
					self.values.changes.insert(s.id, v);
				}
				return;
			}
		}
		self.values.own.insert(s.id, v);
	}

	// A change to a preset's row stops the machine choosing, and keeps the
	// preset it had chosen, since a fresh pick would undo the change.
	fn pin_chooser(&mut self, group: &str, env: &dyn Env) {
		let Some(c) = self.spec.group(group).map(|g| g.chooser.clone()) else {
			return;
		};
		let Some(sw) = self.spec.at(&c).auto.clone() else {
			return;
		};
		if self.value(&sw, env).as_bool() {
			let shown = self.value(&c, env);
			self.values.own.insert(c, shown);
			self.values.own.insert(sw, Value::Bool(false));
		}
	}

	fn drop_changes(&mut self, group: &str) {
		let members: Vec<String> = self
			.spec
			.group(group)
			.map(|g| g.members.clone())
			.unwrap_or_default();
		for m in members {
			self.values.changes.remove(&m);
		}
		self.values.changed_on.remove(group);
	}

	/// Fill a group's own set from what is showing, if it has none yet. Run
	/// once on a first launch, so Custom starts as the preset in force.
	pub fn fill_custom(&mut self, group: &str, env: &dyn Env) {
		let Some(members) = self.spec.group(group).map(|g| g.members.clone()) else {
			return;
		};
		if members.iter().any(|m| self.values.own.contains_key(m)) {
			return;
		}
		// the preset's own values, not what a rule shows over them
		for m in members {
			let s = self.spec.at(&m).clone();
			let v = self.base(&s, env);
			self.values.own.insert(m, v);
		}
	}

	/// The config and state files. Own values go at their paths, except one
	/// under a checkbox that is on, which goes under `kept.set_aside`. Changes
	/// go under `kept.changes`, with the preset they were made on. A group
	/// member's own value is its Custom one, so it always stays put.
	pub fn save(&self, env: &dyn Env) -> (String, String) {
		let mut config = shcl::Document::new();
		let mut state = shcl::Document::new();
		// kept goes last, after everything a person would look for
		let mut kept: Vec<(String, &Value)> = Vec::new();
		for s in &self.spec.settings {
			if s.store == Store::State {
				if let Some(v) = self.values.state.get(&s.id) {
					put(&mut state, &s.path, v);
				}
				continue;
			}
			let Some(v) = self.values.own.get(&s.id) else {
				continue;
			};
			let aside = s.group.is_none()
				&& s.auto
					.as_deref()
					.is_some_and(|sw| self.value(sw, env).as_bool());
			if aside {
				kept.push((format!("kept.set_aside.{}", s.path), v));
			} else {
				put(&mut config, &s.path, v);
			}
		}
		for (path, v) in kept {
			put(&mut config, &path, v);
		}
		for g in &self.spec.groups {
			let changed = self.changed(&g.id, env);
			if changed.is_empty() {
				continue;
			}
			let base = format!("kept.changes.{}", g.id);
			let _ = config.set_string(&format!("{base}.preset"), &self.chosen(&g.id, env));
			for id in changed {
				let s = self.spec.at(id);
				put(
					&mut config,
					&format!("{base}.values.{}", s.path),
					&self.values.changes[id],
				);
			}
		}
		if config.exists("kept") {
			let _ = config.set_comment(
				"kept",
				"Kept by the program, so values can come back later.",
			);
		}
		(config.to_canonical(), state.to_canonical())
	}

	/// Read the files back. A value that fails its setting's checks is dropped,
	/// as is a change made on a preset that is no longer the one picked.
	pub fn load(spec: Spec, config: &str, state: &str) -> Model {
		let config = shcl::Document::parse(config);
		let state = shcl::Document::parse(state);
		let mut m = Model::new(spec);
		let mut own = BTreeMap::new();
		let mut st = BTreeMap::new();
		for s in &m.spec.settings {
			if s.control == Control::Heading {
				continue;
			}
			let (doc, paths) = match s.store {
				Store::State => (&state, vec![s.path.clone()]),
				Store::Config => (
					&config,
					vec![s.path.clone(), format!("kept.set_aside.{}", s.path)],
				),
			};
			let found = paths
				.iter()
				.find_map(|p| take(doc, p, s).and_then(|v| m.spec.valid(s, &v).ok()));
			if let Some(v) = found {
				if s.store == Store::State {
					st.insert(s.id.clone(), v);
				} else {
					own.insert(s.id.clone(), v);
				}
			}
		}
		m.values.own = own;
		m.values.state = st;
		for g in m.spec.groups.clone() {
			let base = format!("kept.changes.{}", g.id);
			let Ok(on) = config.get_string(&format!("{base}.preset")) else {
				continue;
			};
			if m.chosen(&g.id, &NoEnv) != on || g.preset(&on).is_none_or(|p| p.temporary) {
				continue;
			}
			for id in &g.members {
				let s = m.spec.at(id);
				let p = format!("{base}.values.{}", s.path);
				if let Some(v) = take(&config, &p, s).and_then(|v| m.spec.valid(s, &v).ok()) {
					m.values.changes.insert(id.clone(), v);
				}
			}
			m.values.changed_on.insert(g.id.clone(), on);
		}
		m
	}

	/// A line per setting with its value and where it came from, for logs.
	pub fn dump(&self, env: &dyn Env) -> String {
		let mut out = String::new();
		for s in &self.spec.settings {
			if s.control == Control::Heading {
				continue;
			}
			let _ = writeln!(
				out,
				"{} = {}  {}",
				s.id,
				self.value(&s.id, env).show(),
				self.state_line(&s.id, env)
			);
		}
		out
	}
}

fn put(doc: &mut shcl::Document, path: &str, v: &Value) {
	match v {
		Value::Bool(b) => doc.set_bool(path, *b),
		Value::Int(i) => doc.set_int(path, *i),
		Value::Float(f) => doc.set_float(path, *f),
		Value::Text(t) => doc.set_string(path, t),
	};
}

fn take(doc: &shcl::Document, path: &str, s: &Setting) -> Option<Value> {
	if !doc.exists(path) {
		return None;
	}
	match s.kind {
		Kind::Bool => doc.get_bool(path).ok().map(Value::Bool),
		Kind::Int | Kind::Float => number(doc, path).map(Value::Float),
		_ => doc.get_string(path).ok().map(Value::Text),
	}
}

#[cfg(test)]
mod tests;
