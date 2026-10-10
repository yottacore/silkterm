// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Settings declared in shcl, and what a change to one does to the others.
//!
//! Three parts, top to bottom: the spec (parsed once from a compiled-in file),
//! the stored values with the rules for reading and changing them, and the
//! files they are saved to. Nothing here draws anything. `demo/demo.shcl`
//! documents the spec format.

#![allow(clippy::must_use_candidate)]

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

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
	/// The presets are the program's, asked for through [`Env::preset`], and
	/// `presets` is empty.
	pub from_program: bool,
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
		match spec.finish() {
			Ok(spec) if errs.is_empty() => Ok(spec),
			Ok(_) => Err(errs),
			Err(more) => {
				errs.extend(more);
				Err(errs)
			}
		}
	}

	/// The last step of [`Spec::parse`], for a spec a program built itself:
	/// choosers get their presets as options, the links are checked, and indents
	/// are worked out. A group's `members` are filled here too.
	///
	/// # Errors
	///
	/// Every broken link.
	pub fn finish(mut self) -> Result<Spec, Vec<String>> {
		for g in &mut self.groups {
			g.members = self
				.settings
				.iter()
				.filter(|s| s.group.as_deref() == Some(g.id.as_str()))
				.map(|s| s.id.clone())
				.collect();
		}
		// a chooser lists its presets
		for g in &self.groups {
			let mut options: Vec<(String, String)> = g
				.presets
				.iter()
				.map(|p| (p.key.clone(), p.label.clone()))
				.collect();
			if let Some(label) = &g.custom {
				options.push((CUSTOM.into(), label.clone()));
			}
			if let Some(c) = self.settings.iter_mut().find(|s| s.id == g.chooser) {
				c.options = options;
			}
		}
		let errs = self.check_links();
		if errs.is_empty() {
			self.set_indents();
			Ok(self)
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
			if let Some(a) = &s.auto {
				if s.rule.is_none() {
					errs.push(format!("{id}: auto with no rule"));
				}
				if !is_bool(a) {
					errs.push(format!("{id}: auto {a} is not a checkbox"));
				} else if self.at(a).auto.is_some() {
					errs.push(format!("{id}: auto {a} is automatic itself"));
				}
			}
			if let Some(Rule::State(from)) = &s.rule {
				if self.get(from).is_none_or(|f| f.store != Store::State) {
					errs.push(format!(
						"{id}: rule names {from}, which is not a state value"
					));
				}
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
					if !g.from_program
						&& g.preset(c.default.as_text()).is_none()
						&& c.default.as_text() != CUSTOM
					{
						errs.push(format!("group {}: chooser default is not a preset", g.id));
					}
				}
				_ => errs.push(format!(
					"group {}: chooser {} is not a dropdown",
					g.id, g.chooser
				)),
			}
			if g.presets.is_empty() && !g.from_program {
				errs.push(format!("group {}: no presets", g.id));
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
			// a choice is matched without case, and stored as the spec spells it
			(Kind::Choice, Value::Text(t)) => {
				let chooser = self.chooser_of(&s.id);
				let word = t.trim();
				if let Some((k, _)) = s.options.iter().find(|(k, _)| k.eq_ignore_ascii_case(word)) {
					Ok(Value::Text(k.clone()))
				} else if chooser.is_some() && word.eq_ignore_ascii_case(CUSTOM) {
					Ok(Value::Text(CUSTOM.to_string()))
				} else if chooser.is_some_and(|g| g.from_program && !t.is_empty()) {
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
	let from_program = doc.get_bool(&at("from_program")).unwrap_or(false);
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
		from_program,
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

/// What the program answers at run time: a named rule, such as the desktop's
/// font or colors picked from the wallpaper, and the presets of a group that
/// has the program's own. `None` when there is nothing to follow.
pub trait Env {
	fn rule(&self, name: &str) -> Option<Value>;
	fn preset(&self, _group: &str, _key: &str) -> Option<Preset> {
		None
	}
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
	/// Values for this run only, over everything else, such as one given on a
	/// command line. Never saved.
	pub held: BTreeMap<String, Value>,
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
	/// Held for this run.
	Held,
}

/// What the files hold, in spec order. `kept` lines have their full path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
	pub config: Vec<(String, Value)>,
	pub kept: Vec<(String, Value)>,
	pub state: Vec<(String, Value)>,
}

#[derive(Clone, Debug)]
pub struct Model {
	pub spec: Arc<Spec>,
	pub values: Values,
}

// two models of one spec are the same when they store the same
impl PartialEq for Model {
	fn eq(&self, other: &Self) -> bool {
		self.values == other.values
	}
}

// A preset by key, from the spec or from the program. Custom is none.
fn preset_of<'a>(g: &'a Group, key: &str, env: &dyn Env) -> Option<Cow<'a, Preset>> {
	if key == CUSTOM {
		return None;
	}
	if g.from_program {
		env.preset(&g.id, key).map(Cow::Owned)
	} else {
		g.preset(key).map(Cow::Borrowed)
	}
}

impl Model {
	pub fn new(spec: impl Into<Arc<Spec>>) -> Model {
		Model {
			spec: spec.into(),
			values: Values::default(),
		}
	}

	pub fn value(&self, id: &str, env: &dyn Env) -> Value {
		let s = self.spec.at(id);
		if let Some(v) = self.values.held.get(id) {
			return v.clone();
		}
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
		self.rule_answer(s, env)
	}

	fn rule_answer(&self, s: &Setting, env: &dyn Env) -> Option<Value> {
		let v = match s.rule.as_ref()? {
			Rule::State(from) => self.value(from, env),
			Rule::Named(name) => env.rule(name)?,
		};
		self.spec.valid(s, &v).ok()
	}

	/// What a setting has with nothing of its own: its rule's answer where it
	/// has a rule and no switch, else its default.
	pub fn default_of(&self, id: &str, env: &dyn Env) -> Value {
		self.fallback(self.spec.at(id), env)
	}

	fn fallback(&self, s: &Setting, env: &dyn Env) -> Value {
		if s.auto.is_none() {
			if let Some(v) = self.rule_answer(s, env) {
				return v;
			}
		}
		s.default.clone()
	}

	// A value of its own that counts as set. Where the default comes from a
	// rule any value does, since the rule's answer can change under it.
	fn has_own(&self, s: &Setting) -> bool {
		match self.values.own.get(&s.id) {
			None => false,
			Some(_) if s.auto.is_none() && s.rule.is_some() => true,
			Some(v) => *v != s.default,
		}
	}

	// The value with no switch in play: a preset's, a change to it, or the own.
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
				.unwrap_or_else(|| self.fallback(s, env));
		}
		self.values
			.own
			.get(&s.id)
			.cloned()
			.unwrap_or_else(|| self.fallback(s, env))
	}

	// The preset a group member follows, when it isn't Custom.
	fn preset_for<'a>(&'a self, s: &Setting, env: &dyn Env) -> Option<(String, Cow<'a, Preset>)> {
		let g = self.spec.group(s.group.as_deref()?)?;
		let key = self.chosen(&g.id, env);
		preset_of(g, &key, env).map(|p| (key, p))
	}

	/// The preset key a group is on, or [`CUSTOM`]. A temporary one wins.
	pub fn chosen(&self, group: &str, env: &dyn Env) -> String {
		if let Some(t) = self.values.session.get(group) {
			return t.clone();
		}
		self.stored_choice(group, env)
	}

	/// The group's pick with no temporary preset, the one a restart comes back to.
	pub fn stored_choice(&self, group: &str, env: &dyn Env) -> String {
		match self.spec.group(group) {
			Some(g) => self.value(&g.chooser, env).as_text().to_string(),
			None => String::new(),
		}
	}

	/// Put a temporary preset in force, or take it away with `None`. It stores
	/// nothing, and the "*" changes of the pick under it wait.
	pub fn temporary(&mut self, group: &str, key: Option<&str>) {
		match key {
			Some(k) => {
				self.values.session.insert(group.to_string(), k.to_string());
			}
			None => {
				self.values.session.remove(group);
			}
		}
	}

	/// Hold a value for this run, over everything else. Nothing is stored, and a
	/// change on screen takes its place.
	pub fn hold(&mut self, id: &str, v: &Value) {
		let spec = Arc::clone(&self.spec);
		if let Some(v) = spec.get(id).and_then(|s| spec.valid(s, v).ok()) {
			self.values.held.insert(id.to_string(), v);
		}
	}

	pub fn release(&mut self, id: &str) {
		self.values.held.remove(id);
	}

	/// The settings changed on top of the group's preset, in spec order.
	pub fn changed(&self, group: &str, env: &dyn Env) -> Vec<&str> {
		self.changes_on(group, &self.chosen(group, env))
	}

	fn changes_on(&self, group: &str, key: &str) -> Vec<&str> {
		if self.values.changed_on.get(group).map(String::as_str) != Some(key) {
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
		let Some(g) = self.spec.chooser_of(id) else {
			let key = self.value(id, env).as_text().to_string();
			return s
				.options
				.iter()
				.find(|(k, _)| *k == key)
				.map_or(key.clone(), |(_, l)| l.clone());
		};
		let key = self.chosen(&g.id, env);
		let label = match preset_of(g, &key, env) {
			Some(p) => p.label.clone(),
			None if key == CUSTOM => g.custom.clone().unwrap_or(key),
			None => key,
		};
		if self.changed(&g.id, env).is_empty() {
			label
		} else {
			format!("{label} *")
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
		if self.values.held.contains_key(id) {
			return Source::Held;
		}
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
			let group = s.group.as_deref().unwrap_or("");
			let noun = self
				.spec
				.group(group)
				.map(|g| g.noun.clone())
				.unwrap_or_default();
			let was = p
				.values
				.get(id)
				.cloned()
				.unwrap_or_else(|| s.default.clone());
			if self.changed(group, env).contains(&id) {
				return Source::Changed {
					preset: p.label.clone(),
					noun,
					was,
				};
			}
			return Source::Preset {
				preset: p.label.clone(),
				noun,
			};
		}
		if self.has_own(s) {
			Source::Own
		} else {
			Source::Default
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
		self.state_line_shown(id, env, &|_, v| v.show())
	}

	/// [`Model::state_line`] with each value written by `show`, for a program
	/// that shows values in other units than the file's.
	pub fn state_line_shown(
		&self,
		id: &str,
		env: &dyn Env,
		show: &dyn Fn(&Setting, &Value) -> String,
	) -> String {
		let s = self.spec.at(id);
		let mut line = match self.source(id, env) {
			Source::Default | Source::State => String::new(),
			Source::Held => "Set for this run only.".into(),
			Source::Own => format!(
				"Set by hand. Default value: {}.",
				show(s, &self.fallback(s, env))
			),
			Source::Automatic { kept: None } => "Automatic.".into(),
			Source::Automatic { kept: Some(v) } => {
				format!("Automatic. Your value, {}, is kept for later.", show(s, &v))
			}
			Source::Preset { preset, noun } => format!("From the {preset} {noun}."),
			Source::Changed { preset, noun, was } => {
				format!("Changed. The {preset} {noun}'s value is {}.", show(s, &was))
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
		if self.values.held.contains_key(id) {
			return true;
		}
		// a temporary preset is something the chooser's arrow takes away
		if self
			.spec
			.chooser_of(id)
			.is_some_and(|g| self.values.session.contains_key(&g.id))
		{
			return true;
		}
		match s.store {
			Store::State => self.values.state.contains_key(id),
			Store::Config => match self.preset_for(s, env) {
				Some((_, p)) if !p.temporary => self
					.changed(s.group.as_deref().unwrap_or(""), env)
					.contains(&id),
				Some(_) => false,
				None => self.has_own(s),
			},
		}
	}

	/// The reset arrow: a change goes back to its preset's value, anything else
	/// to its default. It is the one way a value set by hand is thrown away.
	pub fn reset(&mut self, id: &str, env: &dyn Env) {
		let spec = Arc::clone(&self.spec);
		let s = spec.at(id);
		if self.values.held.remove(id).is_some() {
			return;
		}
		// the chooser's arrow is a pick of the default, so its "*" changes and a
		// temporary preset go too
		if let Some(g) = spec.chooser_of(id) {
			self.temporary(&g.id, None);
			self.drop_changes(&g.id);
		}
		if s.store == Store::State {
			self.values.state.remove(id);
			return;
		}
		if self.preset_for(s, env).is_some() {
			self.values.changes.remove(id);
			return;
		}
		self.values.own.remove(id);
		self.picking_again(id, env);
	}

	// Turning on a chooser's auto checkbox is a pick like any other, so the
	// "*" changes go.
	fn picking_again(&mut self, id: &str, env: &dyn Env) {
		let spec = Arc::clone(&self.spec);
		let group = spec
			.groups
			.iter()
			.find(|g| spec.at(&g.chooser).auto.as_deref() == Some(id));
		if let Some(g) = group {
			if self.value(id, env).as_bool() {
				self.drop_changes(&g.id);
			}
		}
	}

	/// A change made on screen, with everything it does to the settings around it.
	pub fn set(&mut self, id: &str, v: &Value, env: &dyn Env) {
		let spec = Arc::clone(&self.spec);
		let s = spec.at(id);
		let Ok(v) = spec.valid(s, v) else {
			return;
		};
		self.values.held.remove(id);
		if s.store == Store::State {
			self.values.state.insert(s.id.clone(), v);
			return;
		}
		if let Some(gr) = spec.chooser_of(id) {
			// picking drops the "*" changes, and a temporary pick stores nothing
			if preset_of(gr, v.as_text(), env).is_some_and(|p| p.temporary) {
				self.temporary(&gr.id, Some(v.as_text()));
				return;
			}
			self.temporary(&gr.id, None);
			self.drop_changes(&gr.id);
		}
		if let Some(gate) = &s.gate {
			if !self.value(gate, env).as_bool() {
				self.set(gate, &Value::Bool(true), env);
			}
		}
		if let Some(sw) = &s.auto {
			if self.value(sw, env).as_bool() {
				// the others under the checkbox keep what they show
				let others: Vec<(&str, Value)> = spec
					.under_switch(sw)
					.into_iter()
					.filter(|m| *m != id)
					.map(|m| (m, self.value(m, env)))
					.collect();
				self.write(sw, Value::Bool(false), env);
				for (m, shown) in others {
					self.write(m, shown, env);
				}
			}
		}
		if spec.is_switch(id) && !v.as_bool() && self.value(id, env).as_bool() {
			// one with nothing of its own to fall back on keeps what it shows
			let keep: Vec<(&str, Value)> = spec
				.under_switch(id)
				.into_iter()
				.filter(|m| spec.at(m).group.is_none() && !self.values.own.contains_key(*m))
				.map(|m| (m, self.value(m, env)))
				.collect();
			self.write(id, v, env);
			for (m, shown) in keep {
				self.values.own.insert(m.to_string(), shown);
			}
			return;
		}
		self.write(id, v, env);
		self.picking_again(id, env);
	}

	// Store a value where its setting keeps one right now: as a change while
	// a preset is in force, or as its own.
	fn write(&mut self, id: &str, v: Value, env: &dyn Env) {
		let spec = Arc::clone(&self.spec);
		let s = spec.at(id);
		if let Some(g) = &s.group {
			// a change ends a temporary preset, which would hide it
			self.temporary(g, None);
			let found = self
				.preset_for(s, env)
				.map(|(key, p)| (key, p.values.get(id) == Some(&v)));
			if let Some((key, same)) = found {
				self.pin_chooser(g, env);
				if self.values.changed_on.get(g) != Some(&key) {
					self.drop_changes(g);
					self.values.changed_on.insert(g.clone(), key);
				}
				if same {
					self.values.changes.remove(id);
				} else {
					self.values.changes.insert(s.id.clone(), v);
				}
				return;
			}
		}
		self.values.own.insert(s.id.clone(), v);
	}

	// A change to a preset's row stops the machine choosing, and keeps the
	// preset it had chosen, since a fresh pick would undo the change.
	fn pin_chooser(&mut self, group: &str, env: &dyn Env) {
		let spec = Arc::clone(&self.spec);
		let Some(c) = spec.group(group).map(|g| g.chooser.as_str()) else {
			return;
		};
		let Some(sw) = spec.at(c).auto.as_deref() else {
			return;
		};
		if self.value(sw, env).as_bool() {
			let shown = self.value(c, env);
			self.values.own.insert(c.to_string(), shown);
			self.values.own.insert(sw.to_string(), Value::Bool(false));
		}
	}

	fn drop_changes(&mut self, group: &str) {
		let spec = Arc::clone(&self.spec);
		for m in spec.group(group).map_or(&[][..], |g| g.members.as_slice()) {
			self.values.changes.remove(m);
		}
		self.values.changed_on.remove(group);
	}

	/// Fill a group's own set from what is showing, if it has none yet. Run
	/// once on a first launch, so Custom starts as the preset in force.
	pub fn fill_custom(&mut self, group: &str, env: &dyn Env) {
		let spec = Arc::clone(&self.spec);
		let Some(members) = spec.group(group).map(|g| g.members.as_slice()) else {
			return;
		};
		if members.iter().any(|m| self.values.own.contains_key(m)) {
			return;
		}
		// the preset's own values, not what a rule shows over them
		for m in members {
			let v = self.base(spec.at(m), env);
			self.values.own.insert(m.clone(), v);
		}
	}

	/// What the files hold. Own values go at their paths, except one under a
	/// checkbox that is on, which goes under `kept.set_aside`. Changes go under
	/// `kept.changes`, with the preset they were made on, and stay there while a
	/// temporary preset hides them. A group member's own value is its Custom
	/// one, so it always stays put.
	pub fn lines(&self, env: &dyn Env) -> Lines {
		let mut out = Lines::default();
		for s in &self.spec.settings {
			if s.store == Store::State {
				if let Some(v) = self.values.state.get(&s.id) {
					out.state.push((s.path.clone(), v.clone()));
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
				out.kept
					.push((format!("kept.set_aside.{}", s.path), v.clone()));
			} else {
				out.config.push((s.path.clone(), v.clone()));
			}
		}
		for g in &self.spec.groups {
			let on = self.stored_choice(&g.id, env);
			let changed = self.changes_on(&g.id, &on);
			if changed.is_empty() {
				continue;
			}
			let base = format!("kept.changes.{}", g.id);
			out.kept
				.push((format!("{base}.preset"), Value::Text(on.clone())));
			for id in changed {
				let s = self.spec.at(id);
				out.kept.push((
					format!("{base}.values.{}", s.path),
					self.values.changes[id].clone(),
				));
			}
		}
		out
	}

	/// The config and state files, from [`Model::lines`], with `kept` last.
	pub fn save(&self, env: &dyn Env) -> (String, String) {
		let lines = self.lines(env);
		let mut config = shcl::Document::new();
		let mut state = shcl::Document::new();
		for (path, v) in lines.config.iter().chain(&lines.kept) {
			put(&mut config, path, v);
		}
		for (path, v) in &lines.state {
			put(&mut state, path, v);
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
	pub fn load(spec: impl Into<Arc<Spec>>, config: &str, state: &str) -> Model {
		Model::load_docs(
			spec,
			&shcl::Document::parse(config),
			&shcl::Document::parse(state),
		)
	}

	/// [`Model::load`] from documents already parsed.
	pub fn load_docs(
		spec: impl Into<Arc<Spec>>,
		config: &shcl::Document,
		state: &shcl::Document,
	) -> Model {
		let mut m = Model::new(spec);
		let spec = Arc::clone(&m.spec);
		for s in &spec.settings {
			if s.control == Control::Heading {
				continue;
			}
			let (doc, paths) = match s.store {
				Store::State => (state, vec![s.path.clone()]),
				Store::Config => (
					config,
					vec![s.path.clone(), format!("kept.set_aside.{}", s.path)],
				),
			};
			let found = paths
				.iter()
				.find_map(|p| take(doc, p, s).and_then(|v| spec.valid(s, &v).ok()));
			// a temporary pick is never stored, so a file that names one names nothing
			let temporary = |v: &Value| {
				spec.chooser_of(&s.id)
					.and_then(|g| g.preset(v.as_text()))
					.is_some_and(|p| p.temporary)
			};
			if let Some(v) = found {
				if s.store == Store::State {
					m.values.state.insert(s.id.clone(), v);
				} else if !temporary(&v) {
					m.values.own.insert(s.id.clone(), v);
				}
			}
		}
		for g in &spec.groups {
			let base = format!("kept.changes.{}", g.id);
			let Ok(on) = config.get_string(&format!("{base}.preset")) else {
				continue;
			};
			// the program's presets can't be asked for yet, so only Custom is ruled out
			let gone = if g.from_program {
				on == CUSTOM
			} else {
				g.preset(&on).is_none_or(|p| p.temporary)
			};
			if m.stored_choice(&g.id, &NoEnv) != on || gone {
				continue;
			}
			for id in &g.members {
				let s = spec.at(id);
				let p = format!("{base}.values.{}", s.path);
				if let Some(v) = take(config, &p, s).and_then(|v| spec.valid(s, &v).ok()) {
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
