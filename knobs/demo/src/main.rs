// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! A settings dialog with nothing behind it, for trying out knobs.
//!
//! knobs-demo [--spec FILE] [--dir DIR]
//!
//! --spec rereads FILE whenever it changes; without it the compiled-in
//! demo.shcl is used. --dir is where config.shcl and state.shcl are written,
//! after every change.

use eframe::egui;
use knobs::{Control, Env, Kind, Model, Source, Spec, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

const SPEC: &str = include_str!("../demo.shcl");
const INDENT: f32 = 18.0;

// What the desktop would answer, set by hand in the side panel.
struct Desk {
	font: String,
	size: f64,
	wallpaper: bool,
	wp_fg: [u8; 3],
	wp_cursor: [u8; 3],
}

impl Env for Desk {
	fn rule(&self, name: &str) -> Option<Value> {
		match name {
			"system_font_family" if !self.font.is_empty() => Some(Value::Text(self.font.clone())),
			"system_font_size" if self.size > 0.0 => Some(Value::Float(self.size)),
			"wallpaper_fg" if self.wallpaper => Some(Value::Text(hex(self.wp_fg))),
			"wallpaper_cursor" if self.wallpaper => Some(Value::Text(hex(self.wp_cursor))),
			_ => None,
		}
	}
}

fn hex(c: [u8; 3]) -> String {
	format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn unhex(t: &str) -> [u8; 3] {
	let byte = |i: usize| {
		t.get(i..i + 2)
			.and_then(|b| u8::from_str_radix(b, 16).ok())
			.unwrap_or(0)
	};
	[byte(1), byte(3), byte(5)]
}

struct Demo {
	model: Model,
	desk: Desk,
	dir: PathBuf,
	spec_file: Option<PathBuf>,
	spec_seen: Option<SystemTime>,
	spec_checked: Instant,
	spec_errors: Vec<String>,
	// top-level tab, and the one picked under each tab that has some
	tab: usize,
	sub: BTreeMap<usize, usize>,
	log: Vec<String>,
	// the change at the top of the log, so a drag's steps make one entry:
	// what was changed, the dump and dropdowns from before its first step,
	// how many log lines it took, and when
	last: Option<(String, String, Vec<String>, usize, Instant)>,
	files: (String, String),
}

impl Demo {
	fn new(spec: Spec, spec_file: Option<PathBuf>, dir: PathBuf) -> Demo {
		let desk = Desk {
			font: "DejaVu Sans Mono".into(),
			size: 11.0,
			wallpaper: true,
			wp_fg: [0xee, 0xee, 0xe0],
			wp_cursor: [0xff, 0x88, 0x00],
		};
		let config = std::fs::read_to_string(dir.join("config.shcl")).ok();
		let state = std::fs::read_to_string(dir.join("state.shcl")).unwrap_or_default();
		let mut log = Vec::new();
		let model = if let Some(config) = config {
			log.push(format!("read {}", dir.display()));
			Model::load(spec, &config, &state)
		} else {
			log.push("first run: Custom filled from what is in force".into());
			first_run(spec, &desk)
		};
		let spec_seen = spec_file
			.as_ref()
			.and_then(|f| f.metadata().ok()?.modified().ok());
		let mut demo = Demo {
			model,
			desk,
			dir,
			spec_file,
			spec_seen,
			spec_checked: Instant::now(),
			spec_errors: Vec::new(),
			tab: 0,
			sub: BTreeMap::new(),
			log,
			last: None,
			files: Default::default(),
		};
		demo.save();
		demo
	}

	fn save(&mut self) {
		self.files = self.model.save(&self.desk);
		let _ = std::fs::create_dir_all(&self.dir);
		for (name, text) in [
			("config.shcl", &self.files.0),
			("state.shcl", &self.files.1),
		] {
			if let Err(e) = std::fs::write(self.dir.join(name), text) {
				self.log.insert(0, format!("can't write {name}: {e}"));
			}
		}
	}

	// Run a change, then log every setting whose value or source moved.
	fn change(&mut self, what: &str, f: impl FnOnce(&mut Model, &Desk)) {
		let key = what.split(" = ").next().unwrap_or(what).to_string();
		let (before, star) = match self.last.take() {
			Some((k, before, star, n, at))
				if k == key && at.elapsed() < Duration::from_millis(600) =>
			{
				self.log.drain(..n.min(self.log.len()));
				(before, star)
			}
			_ => (self.model.dump(&self.desk), self.choosers()),
		};
		f(&mut self.model, &self.desk);
		let after = self.model.dump(&self.desk);
		let mut lines = vec![format!("> {what}")];
		for (b, a) in before.lines().zip(after.lines()) {
			if a != b {
				lines.push(format!("  {a}"));
			}
		}
		for (b, a) in star.iter().zip(self.choosers()) {
			if *b != a {
				lines.push(format!("  dropdown shows {a}"));
			}
		}
		self.last = Some((key, before, star, lines.len(), Instant::now()));
		for l in lines.into_iter().rev() {
			self.log.insert(0, l);
		}
		self.log.truncate(400);
		self.save();
	}

	fn choosers(&self) -> Vec<String> {
		self.model
			.spec
			.groups
			.iter()
			.map(|g| self.model.shown_choice(&g.chooser, &self.desk))
			.collect()
	}

	// Reread --spec when it changes. The values come back from the files.
	fn reload_spec(&mut self) {
		if self.spec_checked.elapsed() < Duration::from_millis(400) {
			return;
		}
		self.spec_checked = Instant::now();
		let Some(file) = &self.spec_file else {
			return;
		};
		let seen = file.metadata().ok().and_then(|m| m.modified().ok());
		if seen == self.spec_seen {
			return;
		}
		self.spec_seen = seen;
		let text = std::fs::read_to_string(file).unwrap_or_default();
		match Spec::parse(&text) {
			Ok(spec) => {
				self.spec_errors.clear();
				self.model = Model::load(spec, &self.files.0, &self.files.1);
				self.tab = self.tab.min(self.model.spec.tabs.len().saturating_sub(1));
				self.sub.clear();
				self.log.insert(0, "> spec reread".into());
				self.save();
			}
			Err(e) => self.spec_errors = e,
		}
	}

	fn tab_rows(&mut self, ui: &mut egui::Ui) -> usize {
		let spec = &self.model.spec;
		let tops = spec.tabs_under(None);
		if !tops.contains(&self.tab) {
			self.tab = tops.first().copied().unwrap_or(0);
		}
		ui.horizontal(|ui| {
			for t in &tops {
				if ui
					.selectable_label(self.tab == *t, &spec.tabs[*t].label)
					.clicked()
				{
					self.tab = *t;
				}
			}
		});
		// one more row for each level that has tabs under it
		let mut at = self.tab;
		loop {
			let kids = spec.tabs_under(Some(at));
			if kids.is_empty() {
				return at;
			}
			let pick = self.sub.entry(at).or_insert(kids[0]);
			ui.horizontal(|ui| {
				ui.add_space(16.0);
				for k in &kids {
					if ui
						.selectable_label(*pick == *k, &spec.tabs[*k].label)
						.clicked()
					{
						*pick = *k;
					}
				}
			});
			at = *pick;
		}
	}

	fn rows(&mut self, ui: &mut egui::Ui, tab: usize) {
		let ids: Vec<String> = self
			.model
			.spec
			.settings
			.iter()
			.filter(|s| s.tab == tab && s.control != Control::None)
			.map(|s| s.id.clone())
			.collect();
		// widest label on any tab, so the controls stay put between tabs
		let font = egui::TextStyle::Body.resolve(ui.style());
		let label_w = self
			.model
			.spec
			.settings
			.iter()
			.filter(|s| !matches!(s.control, Control::None | Control::Heading))
			.map(|s| {
				let w = ui
					.painter()
					.layout_no_wrap(s.label.clone(), font.clone(), egui::Color32::WHITE)
					.size()
					.x;
				w + f32::from(s.indent) * INDENT
			})
			.fold(0.0, f32::max);
		ui.spacing_mut().slider_width = 220.0;
		egui::Grid::new(("rows", tab))
			.num_columns(3)
			.spacing([12.0, 8.0])
			.show(ui, |ui| {
				for id in ids {
					self.row(ui, &id, label_w);
					ui.end_row();
				}
			});
	}

	fn row(&mut self, ui: &mut egui::Ui, id: &str, label_w: f32) {
		let Some(s) = self.model.spec.get(id).cloned() else {
			return;
		};
		if s.control == Control::Heading {
			ui.vertical(|ui| {
				ui.add_space(4.0);
				ui.strong(&s.label);
			});
			return;
		}
		let tip = self.model.tip(id, &self.desk);
		let v = self.model.value(id, &self.desk);
		ui.horizontal(|ui| {
			ui.set_min_width(label_w);
			ui.add_space(f32::from(s.indent) * INDENT);
			ui.label(&s.label).on_hover_text(&tip);
		});
		let mut new: Option<Value> = None;
		ui.vertical(|ui| {
			let r = match s.control {
				Control::Checkbox => {
					let mut b = v.as_bool();
					let r = ui.checkbox(&mut b, "");
					if r.changed() {
						new = Some(Value::Bool(b));
					}
					r
				}
				Control::Slider => Self::slider(ui, &s, &v, &mut new),
				Control::Number => {
					let mut x = v.as_f64();
					let mut d = egui::DragValue::new(&mut x).range(s.min..=s.max).speed(0.1);
					if s.kind == Kind::Int {
						d = d.fixed_decimals(0).speed(1.0);
					}
					let r = ui.add(d);
					if r.changed() {
						new = Some(Value::Float(x));
					}
					r
				}
				Control::Dropdown => {
					let group = self.model.spec.chooser_of(id).map(|g| g.id.clone());
					let current = match &group {
						Some(g) => self.model.chosen(g, &self.desk),
						None => v.as_text().to_string(),
					};
					let shown = self.model.shown_choice(id, &self.desk);
					egui::ComboBox::from_id_salt(id)
						.selected_text(shown)
						.width(180.0)
						.show_ui(ui, |ui| {
							for (key, label) in &s.options {
								if ui.selectable_label(*key == current, label).clicked() {
									new = Some(Value::Text(key.clone()));
								}
							}
						})
						.response
				}
				Control::Text | Control::File => {
					let mut t = v.as_text().to_string();
					let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(220.0));
					if r.changed() {
						new = Some(Value::Text(t));
					}
					r
				}
				Control::Color => {
					let mut c = unhex(v.as_text());
					let r = ui.color_edit_button_srgb(&mut c);
					if r.changed() {
						new = Some(Value::Text(hex(c)));
					}
					r
				}
				Control::Heading | Control::None => return,
			};
			r.on_hover_text(&tip);
		});
		let can = self.model.can_reset(id, &self.desk);
		let reset_tip = match self.model.source(id, &self.desk) {
			Source::Changed { preset, noun, .. } => format!("Back to the {preset} {noun}'s value"),
			Source::Automatic { .. } => "Forget the value set by hand".into(),
			_ => format!("Back to default, {}", s.default.show()),
		};
		let reset = ui
			.add_enabled(can, egui::Button::new("\u{21ba}").small())
			.on_hover_text(reset_tip);
		if reset.clicked() {
			self.change(&format!("reset {id}"), |m, d| m.reset(id, d));
		}
		if let Some(nv) = new {
			self.change(&format!("{id} = {}", nv.show()), |m, d| m.set(id, &nv, d));
		}
	}

	fn slider(
		ui: &mut egui::Ui,
		s: &knobs::Setting,
		v: &Value,
		new: &mut Option<Value>,
	) -> egui::Response {
		let mut t = s.to_t(v.as_f64());
		let r = ui
			.horizontal(|ui| {
				let r = ui.add(egui::Slider::new(&mut t, 0.0..=1.0).show_value(false));
				// one width for every slider's number, so the reset arrows line up
				ui.add_sized(
					[44.0, r.rect.height()],
					egui::Label::new(egui::RichText::new(v.show()).monospace()),
				);
				r
			})
			.inner;
		if r.changed() {
			let t = s.snap_t(t, 0.015);
			*new = Some(Value::Float(s.from_t(t)));
		}
		if !s.detents.is_empty() {
			// ticks under the rail, labeled where the spec gives a label
			let (rect, _) =
				ui.allocate_exact_size(egui::vec2(r.rect.width(), 14.0), egui::Sense::hover());
			let pad = r.rect.height() / 2.0;
			let painter = ui.painter();
			let color = ui.visuals().weak_text_color();
			// a label that would run into the one before it is left off
			let mut free_from = f32::MIN;
			for d in &s.detents {
				let x = r.rect.left() + pad + s.to_t(d.value) as f32 * (r.rect.width() - 2.0 * pad);
				painter.line_segment(
					[egui::pos2(x, rect.top()), egui::pos2(x, rect.top() + 3.0)],
					egui::Stroke::new(1.0, color),
				);
				if d.label.is_empty() {
					continue;
				}
				let g = painter.layout_no_wrap(
					d.label.clone(),
					egui::FontId::proportional(10.0),
					color,
				);
				let left = x - g.size().x / 2.0;
				if left < free_from {
					continue;
				}
				free_from = left + g.size().x + 4.0;
				painter.galley(egui::pos2(left, rect.top() + 3.0), g, color);
			}
		}
		r
	}

	fn side(&mut self, ui: &mut egui::Ui) {
		ui.heading("Desktop");
		ui.label("What the program would find. Empty or 0 means nothing.");
		egui::Grid::new("desk").num_columns(2).show(ui, |ui| {
			let mut dirty = false;
			ui.label("System font");
			dirty |= ui.text_edit_singleline(&mut self.desk.font).changed();
			ui.end_row();
			ui.label("System font size");
			dirty |= ui
				.add(egui::DragValue::new(&mut self.desk.size).range(0.0..=72.0))
				.changed();
			ui.end_row();
			ui.label("Wallpaper shown");
			dirty |= ui.checkbox(&mut self.desk.wallpaper, "").changed();
			ui.end_row();
			ui.label("Its text color");
			dirty |= ui.color_edit_button_srgb(&mut self.desk.wp_fg).changed();
			ui.end_row();
			ui.label("Its cursor color");
			dirty |= ui
				.color_edit_button_srgb(&mut self.desk.wp_cursor)
				.changed();
			ui.end_row();
			if dirty {
				self.save();
			}
		});
		ui.separator();
		ui.heading("State");
		ui.label("What the program worked out by itself.");
		let state: Vec<knobs::Setting> = self
			.model
			.spec
			.settings
			.iter()
			.filter(|s| s.control == Control::None)
			.cloned()
			.collect();
		egui::Grid::new("state").num_columns(2).show(ui, |ui| {
			for s in state {
				ui.label(&s.label);
				let v = self.model.value(&s.id, &self.desk);
				let mut new = None;
				match s.kind {
					Kind::Choice => {
						egui::ComboBox::from_id_salt(&s.id)
							.selected_text(v.as_text())
							.show_ui(ui, |ui| {
								for (key, label) in &s.options {
									if ui.selectable_label(v.as_text() == key, label).clicked() {
										new = Some(Value::Text(key.clone()));
									}
								}
							});
					}
					Kind::Int | Kind::Float => {
						let mut x = v.as_f64();
						if ui.add(egui::DragValue::new(&mut x).speed(1.0)).changed() {
							new = Some(Value::Float(x));
						}
					}
					Kind::Bool => {
						let mut b = v.as_bool();
						if ui.checkbox(&mut b, "").changed() {
							new = Some(Value::Bool(b));
						}
					}
					Kind::Text | Kind::Color => {
						let mut t = v.as_text().to_string();
						if ui.text_edit_singleline(&mut t).changed() {
							new = Some(Value::Text(t));
						}
					}
				}
				if let Some(nv) = new {
					let id = s.id.clone();
					self.change(&format!("state {id} = {}", nv.show()), |m, d| {
						m.set(&id, &nv, d);
					});
				}
				ui.end_row();
			}
		});
		ui.separator();
		ui.horizontal(|ui| {
			if ui
				.button("Start fresh")
				.on_hover_text("Forget every value, as on a first run")
				.clicked()
			{
				let spec = self.model.spec.clone();
				self.change("start fresh", |m, d| *m = first_run(spec, d));
			}
		});
		ui.label(format!("Files in {}", self.dir.display()));
		egui::ScrollArea::vertical()
			.id_salt("files")
			.max_height(260.0)
			.show(ui, |ui| {
				ui.strong("config.shcl");
				ui.monospace(&self.files.0);
				ui.strong("state.shcl");
				ui.monospace(&self.files.1);
			});
		ui.separator();
		ui.strong("What each change did");
		egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
			for l in &self.log {
				ui.monospace(l);
			}
		});
	}
}

fn first_run(spec: Spec, desk: &Desk) -> Model {
	let mut m = Model::new(spec);
	let groups: Vec<String> = m.spec.groups.iter().map(|g| g.id.clone()).collect();
	for g in groups {
		m.fill_custom(&g, desk);
	}
	m
}

impl eframe::App for Demo {
	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		self.reload_spec();
		if self.spec_file.is_some() {
			ui.ctx().request_repaint_after(Duration::from_millis(500));
		}
		egui::Panel::right("side")
			.resizable(true)
			.default_size(420.0)
			.show(ui, |ui| self.side(ui));
		egui::CentralPanel::default_margins().show(ui, |ui| {
			if !self.spec_errors.is_empty() {
				let red = ui.visuals().error_fg_color;
				ui.colored_label(
					red,
					"The spec has mistakes, so the last good one is still in use:",
				);
				for e in &self.spec_errors {
					ui.colored_label(red, e);
				}
				ui.separator();
			}
			let tab = self.tab_rows(ui);
			ui.separator();
			egui::ScrollArea::vertical()
				.id_salt("page")
				.show(ui, |ui| self.rows(ui, tab));
		});
	}
}

fn main() -> eframe::Result {
	let mut spec_file = None;
	let mut dir = std::env::temp_dir().join("knobs-demo");
	let mut args = std::env::args().skip(1);
	while let Some(a) = args.next() {
		match a.as_str() {
			"--spec" => spec_file = args.next().map(PathBuf::from),
			"--dir" => dir = args.next().map_or(dir, PathBuf::from),
			_ => {
				eprintln!("knobs-demo [--spec FILE] [--dir DIR]");
				std::process::exit(2);
			}
		}
	}
	let text = spec_file.as_ref().map_or_else(
		|| SPEC.to_string(),
		|f| std::fs::read_to_string(f).unwrap_or_default(),
	);
	let spec = match Spec::parse(&text) {
		Ok(spec) => spec,
		Err(errs) => {
			for e in errs {
				eprintln!("{e}");
			}
			std::process::exit(1);
		}
	};
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_title("knobs demo")
			.with_inner_size([1180.0, 760.0]),
		..Default::default()
	};
	eframe::run_native(
		"knobs demo",
		options,
		Box::new(move |_| Ok(Box::new(Demo::new(spec, spec_file, dir)))),
	)
}
