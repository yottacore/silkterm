// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The search box over the rows, and the list of matches that takes the page's
//! place while the box holds text. Picking one goes to it.
//!
//! A match is a tab or a row, found by its label first, then by one of the
//! row's `aliases:`, then loosely: the letters in order, or a word one or two
//! typos off.

use super::{
	Dlg, Focus, Kind, Marks, SEARCH_ROW, SettingsDialog, TextItem, border, clip_rect, lay,
	leaf_node, path_to, quad, tab_nodes, ui,
};
use crate::config;
use crate::gfx::RectInstance;
use crate::pane::Rect;
use crate::ui_spec::Spec;

/// How much bigger the matched part of a result is drawn.
const MARK_SCALE: f32 = 1.15;
/// A loose match needs this many letters, or two would match half the dialog.
const LOOSE_MIN: usize = 3;
/// How long a row picked from the list stays marked, and how long of that it
/// holds before it fades, in seconds.
const FLASH_S: f32 = 1.6;
const FLASH_HOLD_S: f32 = 0.5;
const FLASH_ALPHA: f32 = 0.38;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Tier {
	Label,
	Alias,
	Loose,
}

/// What a result goes to. A tab is listed by its own title; a heading that
/// only repeats it is not listed again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
	Tab(usize), // node
	Row(usize), // spec
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Hit {
	pub(super) to: Target,
	pub(super) tier: Tier,
	/// The alias that found it, which the result shows after the label.
	pub(super) alias: Option<&'static str>,
	/// Byte ranges matched, in the alias when there is one, else the label.
	pub(super) marks: Vec<(usize, usize)>,
}

#[derive(Debug, Default)]
pub(super) struct Search {
	pub(super) query: String,
	pub(super) hits: Vec<Hit>,
	pub(super) sel: usize,
	// where the page was scrolled to before the list took its place
	kept_scroll: Option<f32>,
}

// Text lowercased for matching, with the span in the original each of its
// bytes came from, so a match found in one is marked in the other.
struct Folded {
	text: String,
	from: Vec<(usize, usize)>,
}

fn fold(text: &str) -> Folded {
	let mut out = String::with_capacity(text.len());
	let mut from = Vec::with_capacity(text.len());
	for (at, c) in text.char_indices() {
		let end = at + c.len_utf8();
		for lower in c.to_lowercase() {
			let before = out.len();
			out.push(lower);
			from.extend(std::iter::repeat_n((at, end), out.len() - before));
		}
	}
	Folded { text: out, from }
}

impl Folded {
	fn back(&self, a: usize, b: usize) -> (usize, usize) {
		(self.from[a].0, self.from[b - 1].1)
	}
	fn word_start(&self, at: usize) -> bool {
		!self.text[..at]
			.chars()
			.next_back()
			.is_some_and(char::is_alphanumeric)
	}
}

// Where `needle` is in `hay`, and how good a place: the whole of it, its start,
// a word's start, anywhere. A word's start wins over an earlier mid-word find.
fn literal(hay: &Folded, needle: &str) -> Option<(u8, (usize, usize))> {
	let mut best: Option<(u8, usize)> = None;
	for (at, _) in hay.text.match_indices(needle) {
		let how = match at {
			0 if needle.len() == hay.text.len() => 0,
			0 => 1,
			_ if hay.word_start(at) => 2,
			_ => 3,
		};
		if best.is_none_or(|(was, _)| how < was) {
			best = Some((how, at));
		}
	}
	best.map(|(how, at)| (how, hay.back(at, at + needle.len())))
}

// The query's letters in order from the start of a word, not too far apart.
// Fewer, longer runs rank higher, then a tighter span.
fn in_order(hay: &Folded, query: &str) -> Option<((usize, usize), Vec<(usize, usize)>)> {
	let want: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).collect();
	if want.len() < LOOSE_MIN {
		return None;
	}
	let chars: Vec<(usize, char)> = hay.text.char_indices().collect();
	let mut best: Option<((usize, usize), Vec<usize>)> = None;
	let starts = (0..chars.len()).filter(|&s| chars[s].1 == want[0] && hay.word_start(chars[s].0));
	for start in starts {
		let mut picked = vec![start];
		let mut j = start + 1;
		for &c in &want[1..] {
			while j < chars.len() && chars[j].1 != c {
				j += 1;
			}
			if j == chars.len() {
				break;
			}
			picked.push(j);
			j += 1;
		}
		if picked.len() < want.len() {
			continue;
		}
		let span = picked[picked.len() - 1] - start + 1;
		if span > want.len() * 2 + 2 {
			continue;
		}
		let runs = 1 + picked.windows(2).filter(|w| w[1] != w[0] + 1).count();
		if best.as_ref().is_none_or(|(key, _)| (runs, span) < *key) {
			best = Some(((runs, span), picked));
		}
	}
	let (key, picked) = best?;
	let mut marks: Vec<(usize, usize)> = Vec::new();
	for &k in &picked {
		let (at, c) = chars[k];
		let (a, b) = hay.back(at, at + c.len_utf8());
		match marks.last_mut() {
			Some(last) if last.1 == a => last.1 = b,
			_ => marks.push((a, b)),
		}
	}
	Some((key, marks))
}

// Edits apart, a swap of two neighbors counting as one.
fn typos(a: &[char], b: &[char]) -> usize {
	let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
	for (i, row) in rows.iter_mut().enumerate() {
		row[0] = i;
	}
	for (j, cell) in rows[0].iter_mut().enumerate() {
		*cell = j;
	}
	for i in 1..=a.len() {
		for j in 1..=b.len() {
			let cost = usize::from(a[i - 1] != b[j - 1]);
			let mut d = (rows[i - 1][j] + 1)
				.min(rows[i][j - 1] + 1)
				.min(rows[i - 1][j - 1] + cost);
			if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
				d = d.min(rows[i - 2][j - 2] + 1);
			}
			rows[i][j] = d;
		}
	}
	rows[a.len()][b.len()]
}

// A word, a word's start or the whole text a typo or two off the query. A short
// query gets none, since every short word is one typo from many others.
fn near(hay: &Folded, query: &str) -> Option<(usize, (usize, usize))> {
	let want: Vec<char> = query.chars().collect();
	let allowed = match want.len() {
		0..=3 => return None,
		4..=7 => 1,
		_ => 2,
	};
	let chars: Vec<(usize, char)> = hay.text.char_indices().collect();
	// (first char, end char) runs to try: every word, and the whole text
	let mut spans: Vec<(usize, usize)> = vec![(0, chars.len())];
	let mut k = 0;
	while k < chars.len() {
		if !chars[k].1.is_alphanumeric() {
			k += 1;
			continue;
		}
		let start = k;
		while k < chars.len() && chars[k].1.is_alphanumeric() {
			k += 1;
		}
		spans.push((start, k));
	}
	let mut best: Option<(usize, (usize, usize))> = None;
	for (start, end) in spans {
		// and each one cut to the query's length, for a word still being typed,
		// though a short one cut that way is a typo away from too much
		let cut = if want.len() >= 6 {
			(start + want.len()).min(end)
		} else {
			end
		};
		for end in [end, cut] {
			if end <= start {
				continue;
			}
			let them: Vec<char> = chars[start..end].iter().map(|&(_, c)| c).collect();
			let d = typos(&want, &them);
			if d <= allowed && best.is_none_or(|(was, _)| d < was) {
				let a = chars[start].0;
				let b = chars[end - 1].0 + chars[end - 1].1.len_utf8();
				best = Some((d, hay.back(a, b)));
			}
		}
	}
	best
}

// One way a target can be found: its label, or one of its aliases.
struct Names {
	label: &'static str,
	aliases: &'static [&'static str],
}

// The tabs and rows a search can list, each with its place in dialog order.
fn targets(specs: &[Spec]) -> Vec<(Target, Names, usize)> {
	let nodes = tab_nodes();
	let mut out = Vec::new();
	for (node, n) in nodes.iter().enumerate() {
		// a tab sorts just ahead of its first row, and lends the aliases of the
		// heading that only repeats its title
		let first_leaf = n
			.leaf
			.unwrap_or_else(|| super::first_leaf_under(nodes, node));
		let first_row = specs
			.iter()
			.position(|s| s.tab == first_leaf)
			.unwrap_or(specs.len());
		let aliases = n
			.leaf
			.and_then(|leaf| {
				specs.iter().find(|s| {
					s.tab == leaf
						&& matches!(s.kind, Kind::Header(_))
						&& SettingsDialog::header_is_tab_title(s)
				})
			})
			.map_or(&[][..], |s| s.aliases);
		out.push((
			Target::Tab(node),
			Names {
				label: n.title,
				aliases,
			},
			first_row,
		));
	}
	for (row, spec) in specs.iter().enumerate() {
		if spec.label.is_empty() || SettingsDialog::header_is_tab_title(spec) {
			continue;
		}
		out.push((
			Target::Row(row),
			Names {
				label: spec.label,
				aliases: spec.aliases,
			},
			row,
		));
	}
	out
}

/// Everything that matches `query`: label matches, then alias matches, then
/// loose ones, each in the order the dialog shows them.
pub(super) fn rank(specs: &[Spec], query: &str) -> Vec<Hit> {
	let query = fold(&query.split_whitespace().collect::<Vec<_>>().join(" ")).text;
	if query.is_empty() {
		return Vec::new();
	}
	// tier, how good, how good again, tab before row, dialog order
	let mut found: Vec<((Tier, u8, usize, u8, usize), Hit)> = Vec::new();
	for (to, names, at) in targets(specs) {
		let kind = u8::from(matches!(to, Target::Row(_)));
		let hit = |tier, alias, marks| Hit {
			to,
			tier,
			alias,
			marks,
		};
		let label = fold(names.label);
		if let Some((how, mark)) = literal(&label, &query) {
			found.push((
				(Tier::Label, how, 0, kind, at),
				hit(Tier::Label, None, vec![mark]),
			));
			continue;
		}
		let aliases: Vec<(&'static str, Folded)> =
			names.aliases.iter().map(|&a| (a, fold(a))).collect();
		let by_alias = aliases
			.iter()
			.filter_map(|(alias, f)| literal(f, &query).map(|(how, mark)| (how, *alias, mark)))
			.min_by_key(|&(how, _, _)| how);
		if let Some((how, alias, mark)) = by_alias {
			found.push((
				(Tier::Alias, how, 0, kind, at),
				hit(Tier::Alias, Some(alias), vec![mark]),
			));
			continue;
		}
		// loose: the letters in order, then near misses, a label before an
		// alias each time
		let in_label = in_order(&label, &query).map(|(key, marks)| (key, None, marks));
		let in_alias = || {
			aliases
				.iter()
				.filter_map(|(alias, f)| in_order(f, &query).map(|(key, m)| (key, Some(*alias), m)))
				.min_by_key(|(key, _, _)| *key)
		};
		if let Some(((runs, span), alias, marks)) = in_label.or_else(in_alias) {
			found.push((
				(
					Tier::Loose,
					u8::from(alias.is_some()),
					runs * 100 + span,
					kind,
					at,
				),
				hit(Tier::Loose, alias, marks),
			));
			continue;
		}
		let near_label = near(&label, &query).map(|(d, mark)| (d, None, mark));
		let near_alias = || {
			aliases
				.iter()
				.filter_map(|(alias, f)| near(f, &query).map(|(d, mark)| (d, Some(*alias), mark)))
				.min_by_key(|(d, _, _)| *d)
		};
		if let Some((d, alias, mark)) = near_label.or_else(near_alias) {
			let class = 2 + u8::from(alias.is_some());
			found.push((
				(Tier::Loose, class, d, kind, at),
				hit(Tier::Loose, alias, vec![mark]),
			));
		}
	}
	found.sort_by_key(|(key, _)| *key);
	found.into_iter().map(|(_, hit)| hit).collect()
}

// The heading a row is under, where it says more than the tab's own title.
fn heading_of(specs: &[Spec], row: usize) -> Option<usize> {
	if matches!(specs[row].kind, Kind::Header(_)) {
		return None;
	}
	let tab = specs[row].tab;
	let at = (0..row)
		.rev()
		.take_while(|&i| specs[i].tab == tab)
		.find(|&i| matches!(specs[i].kind, Kind::Header(_)))?;
	(!SettingsDialog::header_is_tab_title(&specs[at])).then_some(at)
}

// The row a sub-group member sits under, for two rows that would otherwise
// read the same (the Scrollbar's Width px and the Minimap's).
fn leader_of(specs: &[Spec], row: usize) -> Option<usize> {
	let me = &specs[row];
	let twin = (0..specs.len()).any(|j| {
		j != row
			&& specs[j].tab == me.tab
			&& specs[j].label == me.label
			&& heading_of(specs, j) == heading_of(specs, row)
	});
	if !twin {
		return None;
	}
	(0..row)
		.rev()
		.take_while(|&i| specs[i].tab == me.tab && !matches!(specs[i].kind, Kind::Header(_)))
		.find(|&i| specs[i].indent < me.indent && !specs[i].label.is_empty())
}

/// A result's line: its tab and the tabs over it, its heading where that
/// isn't the tab's title, its own label, then the alias that found it. The
/// marks come back moved to where they fall in the line.
pub(super) fn crumb(specs: &[Spec], hit: &Hit) -> (String, Vec<(usize, usize)>) {
	let nodes = tab_nodes();
	let sep = format!(" {} ", ui().icons.search_sep);
	let (path, subject) = match hit.to {
		Target::Tab(node) => {
			let mut up = Vec::new();
			let mut at = nodes[node].parent;
			while let Some(p) = at {
				up.insert(0, nodes[p].title);
				at = nodes[p].parent;
			}
			(up, nodes[node].title)
		}
		Target::Row(row) => {
			let mut parts: Vec<&str> = path_to(nodes, specs[row].tab)
				.into_iter()
				.map(|n| nodes[n].title)
				.collect();
			if let Some(h) = heading_of(specs, row) {
				parts.push(specs[h].label);
			}
			if let Some(l) = leader_of(specs, row) {
				parts.push(specs[l].label);
			}
			(parts, specs[row].label)
		}
	};
	let mut line = String::new();
	for part in &path {
		line.push_str(part);
		line.push_str(&sep);
	}
	let marked_from = if let Some(alias) = hit.alias {
		line.push_str(subject);
		line.push_str(" (");
		let from = line.len();
		line.push_str(alias);
		line.push(')');
		from
	} else {
		let from = line.len();
		line.push_str(subject);
		from
	};
	let marks = hit
		.marks
		.iter()
		.map(|&(a, b)| (a + marked_from, b + marked_from))
		.collect();
	(line, marks)
}

impl SettingsDialog {
	pub(super) fn searching(&self) -> bool {
		!self.search.query.is_empty()
	}

	/// The search box's band over the rows: the box, the gap under it and the
	/// line the rows scroll under.
	pub(super) fn search_band_h_for(line_h: f32) -> f32 {
		Self::field_h_for(line_h) + lay().search_gap + 1.0
	}

	/// Window space: it sits over the rows and never scrolls with them.
	pub(super) fn search_box(&self) -> Rect {
		let h = self.field_h();
		let x = self.rows_x0() + lay().pad;
		Rect {
			x,
			y: self.rows_y0() - 1.0 - lay().search_gap - h,
			w: (self.rect.x + self.rect.w - lay().pad - x).max(0.0),
			h,
		}
	}

	pub(super) fn search_content_h(&self) -> f32 {
		self.search.hits.len().max(1) as f32 * self.line_row_h()
	}

	// Result `k`'s line in the list, which scrolls like the rows it stands for.
	fn hit_rect(&self, k: usize) -> Rect {
		let vp = self.viewport();
		let h = self.line_row_h();
		Rect {
			x: vp.x + lay().pad / 2.0,
			y: vp.y + lay().rows_pad - self.scroll + k as f32 * h,
			w: (vp.w - lay().pad * 1.5).max(0.0),
			h,
		}
	}

	fn hit_at(&self, x: f32, y: f32) -> Option<usize> {
		if !self.viewport().contains(x, y) {
			return None;
		}
		(0..self.search.hits.len()).find(|&k| self.hit_rect(k).contains(x, y))
	}

	/// Ctrl+F: the keyboard goes to the search box, with what it holds selected.
	pub fn search_focus(&mut self) {
		if self.modal() {
			return;
		}
		self.dismiss_menu();
		self.open = None;
		self.capture_end();
		self.focus = Some(Focus::Search);
		self.open_edit(SEARCH_ROW, true);
	}

	/// What the box holds changed: list again from the top.
	pub(super) fn search_set(&mut self, text: &str) {
		let was = self.searching();
		self.search.query = text.to_string();
		self.search.hits = rank(self.specs, text);
		self.search.sel = 0;
		match (was, self.searching()) {
			(false, true) => {
				self.search.kept_scroll = Some(self.scroll);
				self.scroll = 0.0;
			}
			(true, false) => self.scroll = self.search.kept_scroll.take().unwrap_or(0.0),
			_ => self.scroll = 0.0,
		}
		self.scroll = self.scroll.clamp(0.0, self.max_scroll());
	}

	/// Empty the box, and the page comes back where it was.
	pub(super) fn search_clear(&mut self) {
		if let Some(edit) = self.edit.as_mut().filter(|e| e.row == SEARCH_ROW) {
			edit.buf.clear();
			edit.cur = 0;
			edit.sel = None;
			edit.view = 0.0;
			edit.view_to = 0.0;
		}
		self.search_set("");
	}

	/// Up and Down in the box walk the list, which keeps the one picked in view.
	pub(super) fn search_step(&mut self, forward: bool) {
		let n = self.search.hits.len();
		if n == 0 {
			return;
		}
		self.search.sel = if forward {
			(self.search.sel + 1).min(n - 1)
		} else {
			self.search.sel.saturating_sub(1)
		};
		let vp = self.viewport();
		let r = self.hit_rect(self.search.sel);
		if r.y < vp.y {
			self.scroll -= vp.y - r.y;
		} else if r.y + r.h > vp.y + vp.h {
			self.scroll += r.y + r.h - (vp.y + vp.h);
		}
		self.scroll = self.scroll.clamp(0.0, self.max_scroll());
	}

	/// Go to result `k`: its tab, the row scrolled into view and marked for a
	/// moment, the keyboard on its control. The box empties on the way.
	pub(super) fn search_go(&mut self, k: usize) {
		let Some(to) = self.search.hits.get(k).map(|h| h.to) else {
			return;
		};
		self.search.kept_scroll = None;
		self.search_clear();
		if self.edit.as_ref().is_some_and(|e| e.row == SEARCH_ROW) {
			self.commit_edit();
		}
		let row = match to {
			Target::Tab(node) => {
				self.go_to_tab(node);
				self.scroll = 0.0;
				self.hscroll = 0.0;
				self.focus = self.first_focus();
				self.open_focused_field();
				return;
			}
			Target::Row(row) => row,
		};
		self.go_to_tab(leaf_node(tab_nodes(), self.specs[row].tab));
		self.hscroll = 0.0;
		self.focus = if matches!(self.specs[row].kind, Kind::Header(_)) {
			None
		} else {
			(0..self.parts_of(row))
				.find(|&p| !self.part_disabled(row, p))
				.map(|p| Focus::Row(row, p))
		};
		// a third of the way down, unless it is already in full view
		let vp = self.viewport();
		let top = self.row_y(row);
		if top < vp.y || top + self.row_screen_h(row) > vp.y + vp.h {
			self.scroll += top - (vp.y + vp.h / 3.0);
		}
		self.scroll = self.scroll.clamp(0.0, self.max_scroll());
		self.open_focused_field();
		self.flash = Some((row, 0.0));
	}

	/// A click on the box or the list. False when it was on neither, or the
	/// list isn't up, so the rows behind can take it.
	pub(super) fn search_mouse_down(
		&mut self,
		x: f32,
		y: f32,
		measure: &mut impl FnMut(&str) -> f32,
	) -> bool {
		let field = self.search_box();
		if field.contains(x, y) {
			self.open = None;
			self.focus = Some(Focus::Search);
			self.field_click(SEARCH_ROW, None, field, x, measure);
			return true;
		}
		if !self.searching() || !self.viewport().contains(x, y) {
			return false;
		}
		if let Some(k) = self.hit_at(x, y) {
			self.search_go(k);
		}
		true
	}

	/// The pointer picks the result under it, the way it does in an open list.
	pub(super) fn search_hover(&mut self, x: f32, y: f32) -> bool {
		if !self.searching() {
			return false;
		}
		match self.hit_at(x, y) {
			Some(k) if k != self.search.sel => {
				self.search.sel = k;
				true
			}
			_ => false,
		}
	}

	/// The picked row's mark fades out over a second or so. The wake the frame
	/// loop wants while it does.
	pub(super) fn flash_step(&mut self, dt: f32) -> Option<u64> {
		let (_, t) = self.flash.as_mut()?;
		*t += dt;
		if *t >= FLASH_S {
			self.flash = None;
			// one more frame, to draw it gone
			return Some(16);
		}
		Some(16)
	}

	fn flash_alpha(t: f32) -> f32 {
		if t <= FLASH_HOLD_S {
			FLASH_ALPHA
		} else {
			FLASH_ALPHA * (1.0 - (t - FLASH_HOLD_S) / (FLASH_S - FLASH_HOLD_S)).clamp(0.0, 1.0)
		}
	}

	/// The box goes in the fixed chrome, the list's pick and a row's mark in
	/// the scrolled rows. One outline on the box: the focus color takes the
	/// place of its own border.
	pub(super) fn search_rects(
		&self,
		colors: &Dlg,
		fixed: &mut Vec<RectInstance>,
		rows: &mut Vec<RectInstance>,
		measure: &mut impl FnMut(&str) -> f32,
	) {
		let field = self.search_box();
		fixed.push(quad(field.x, field.y, field.w, field.h, colors.field_bg));
		let focused = self.focus == Some(Focus::Search);
		border(
			fixed,
			field,
			1.0,
			if focused {
				colors.focus_out
			} else {
				colors.panel_border
			},
		);
		if self.edit.as_ref().is_some_and(|e| e.row == SEARCH_ROW) {
			self.caret_quad(colors, fixed, field, measure);
		}
		if self.searching() {
			if !self.search.hits.is_empty() {
				let r = clip_rect(self.hit_rect(self.search.sel), self.viewport());
				rows.push(quad(r.x, r.y, r.w, r.h, colors.tab_hl));
			}
			return;
		}
		let Some((row, t)) = self.flash else {
			return;
		};
		if self.specs[row].tab != self.tab {
			return;
		}
		let a = Self::flash_alpha(t);
		let c = config::srgb_f32(colors.handle);
		rows.push(RectInstance {
			pos: [self.content_x() + lay().pad / 2.0, self.row_y(row)],
			// clear of the scrollbar, as a result's shade is
			size: [self.layout_w() - lay().pad * 1.5, self.row_screen_h(row)],
			// premultiplied, as the quad pass blends
			color: [c[0] * a, c[1] * a, c[2] * a, a],
			..Default::default()
		});
	}

	pub(super) fn search_texts(&self, colors: &Dlg, out: &mut Vec<TextItem>) {
		let field = self.search_box();
		let text_y = |y: f32, h: f32| y + (h - self.line_h) / 2.0;
		let editing = self.edit.as_ref().filter(|e| e.row == SEARCH_ROW);
		let (typed, view) = editing.map_or((self.search.query.clone(), 0.0), |e| {
			(e.buf.clone(), e.view)
		});
		let x = field.x + lay().field_pad - view;
		if typed.is_empty() {
			let chord = if self.mac { "Command+F" } else { "Ctrl+F" };
			out.push(TextItem {
				color: colors.dim,
				clip: Some(field),
				..TextItem::plain(
					format!("Search ({chord})"),
					x,
					text_y(field.y, field.h),
					colors.dim,
				)
			});
		} else {
			out.push(TextItem {
				clip: Some(field),
				..TextItem::plain(typed, x, text_y(field.y, field.h), colors.text)
			});
		}
		if !self.searching() {
			return;
		}
		let vp = self.viewport();
		if self.search.hits.is_empty() {
			let r = self.hit_rect(0);
			out.push(TextItem {
				clip: Some(vp),
				..TextItem::plain(
					format!("No settings match \"{}\"", self.search.query.trim()),
					r.x + lay().pad / 2.0,
					text_y(r.y, r.h),
					colors.dim,
				)
			});
			return;
		}
		for (k, hit) in self.search.hits.iter().enumerate() {
			let r = self.hit_rect(k);
			if r.y + r.h < vp.y || r.y > vp.y + vp.h {
				continue;
			}
			let (line, spans) = crumb(self.specs, hit);
			out.push(TextItem {
				clip: Some(clip_rect(r, vp)),
				marks: Some(Marks {
					spans,
					color: colors.handle,
					scale: MARK_SCALE,
				}),
				..TextItem::plain(line, r.x + lay().pad / 2.0, text_y(r.y, r.h), colors.text)
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::super::tests::{chars7, mk_dialog};
	use super::super::{Focus, Kind, SEARCH_ROW, tab_nodes, ui};
	use super::{Hit, Target, Tier, crumb, rank};
	use crate::ui_spec::Key;

	fn row_of(key: Key) -> usize {
		ui().specs
			.iter()
			.position(|s| s.key == key && !matches!(s.kind, Kind::Header(_)))
			.unwrap_or_else(|| panic!("no row for {key:?}"))
	}
	fn heading(label: &str) -> usize {
		ui().specs
			.iter()
			.position(|s| matches!(s.kind, Kind::Header(l) if l == label))
			.unwrap_or_else(|| panic!("no heading {label}"))
	}
	fn tab(path: &str) -> usize {
		tab_nodes()
			.iter()
			.position(|n| n.path == path)
			.unwrap_or_else(|| panic!("no tab {path}"))
	}
	fn tiers(hits: &[Hit]) -> Vec<Tier> {
		hits.iter().map(|h| h.tier).collect()
	}

	// Label matches first, alias ones next, loose ones last, and within a tier
	// in the order the dialog shows them.
	// Test ID: de03v
	#[test]
	fn label_matches_come_before_alias_ones_and_loose_ones_last() {
		let hits = rank(&ui().specs, "opacity");
		let mut seen = tiers(&hits);
		seen.dedup();
		assert_eq!(seen, [Tier::Label, Tier::Alias], "{hits:?}");
		assert_eq!(hits[0].to, Target::Row(row_of(Key::Opacity)));
		assert!(
			hits.iter()
				.any(|h| h.to == Target::Row(row_of(Key::BgOpacity)) && h.tier == Tier::Alias),
			"Visibility % comes up by its alias"
		);
		// a typo finds it only loosely, after everything spelled right
		let hits = rank(&ui().specs, "opactiy");
		assert!(
			!hits.is_empty() && hits.iter().all(|h| h.tier == Tier::Loose),
			"{hits:?}"
		);
		assert_eq!(hits[0].to, Target::Row(row_of(Key::Opacity)));

		let hits = rank(&ui().specs, "font");
		let at = |key: Key| hits.iter().position(|h| h.to == Target::Row(row_of(key)));
		assert!(at(Key::SystemFont) < at(Key::FontFamily), "{hits:?}");
		assert_eq!(hits[at(Key::FontFamily).unwrap()].tier, Tier::Alias);
		// whatever the query, the tiers come in order and nothing is listed twice
		for q in [
			"e", "col", "colr", "scroll", "size", "trans", "width", "zoom", "tab",
		] {
			let hits = rank(&ui().specs, q);
			let seen = tiers(&hits);
			assert!(seen.is_sorted(), "{q}: {hits:?}");
			let mut to: Vec<String> = hits.iter().map(|h| format!("{:?}", h.to)).collect();
			to.sort();
			let n = to.len();
			to.dedup();
			assert_eq!(n, to.len(), "{q} lists a target twice");
		}
		let hits = rank(&ui().specs, "lnhght");
		assert_eq!(tiers(&hits).first(), Some(&Tier::Loose));
		assert_eq!(hits[0].to, Target::Row(row_of(Key::LineHeight)));
	}

	// Inside a tier, the whole label beats its start, which beats a word's
	// start, which beats the middle of a word.
	// Test ID: de03w
	#[test]
	fn a_whole_label_beats_a_word_start_beats_the_middle_of_one() {
		let hits = rank(&ui().specs, "cursor");
		let first: Vec<Target> = hits.iter().take(2).map(|h| h.to).collect();
		assert!(
			first.contains(&Target::Tab(tab("Appearance/Cursor"))),
			"{hits:?}"
		);
		assert!(
			first.contains(&Target::Row(row_of(Key::ColCursor))),
			"{hits:?}"
		);
		// "Ease-out" starts a word; nothing in front of it starts with "out"
		let hits = rank(&ui().specs, "out");
		let at = |key: Key| hits.iter().position(|h| h.to == Target::Row(row_of(key)));
		assert!(at(Key::Outline) < at(Key::ScrollEaseOut), "{hits:?}");
		// and a tab before a row the same distance off
		let hits = rank(&ui().specs, "shell");
		assert_eq!(hits[0].to, Target::Tab(tab("Shell")), "{hits:?}");
	}

	// Two letters loosely would match half the dialog, so they find only
	// what has them together.
	// Test ID: de03x
	#[test]
	fn a_short_query_matches_only_as_typed() {
		for q in ["ot", "zq"] {
			let hits = rank(&ui().specs, q);
			assert!(hits.iter().all(|h| h.tier != Tier::Loose), "{q}: {hits:?}");
		}
		assert!(rank(&ui().specs, "zqxj").is_empty());
		assert!(rank(&ui().specs, "   ").is_empty());
		// case and extra spaces don't matter
		assert_eq!(
			rank(&ui().specs, "LINE  height"),
			rank(&ui().specs, "line height")
		);
	}

	// The line a result shows: the tabs it is under, its heading where that
	// isn't the tab's title, the label, and the alias that found it, with the
	// marks over the matched text.
	// Test ID: de03y
	#[test]
	fn a_result_reads_tab_then_heading_then_setting() {
		let sep = ui().icons.search_sep;
		let specs = &ui().specs;
		let first = |q: &str, to: Target| {
			let hits = rank(specs, q);
			let hit = hits
				.into_iter()
				.find(|h| h.to == to)
				.unwrap_or_else(|| panic!("{q}"));
			let (line, marks) = crumb(specs, &hit);
			let marked: Vec<String> = marks.iter().map(|&(a, b)| line[a..b].to_string()).collect();
			(line, marked)
		};
		let (line, marked) = first("fami", Target::Row(row_of(Key::FontFamily)));
		assert_eq!(
			line,
			format!("Appearance {sep} Text {sep} Font {sep} Family")
		);
		assert_eq!(marked, ["Fami"]);
		// a heading that is the tab's title says nothing more
		let (line, _) = first("columns", Target::Row(row_of(Key::Columns)));
		assert_eq!(line, format!("Window {sep} Columns"));
		let (line, marked) = first("typeface", Target::Row(row_of(Key::FontFamily)));
		assert_eq!(
			line,
			format!("Appearance {sep} Text {sep} Font {sep} Family (typeface)")
		);
		assert_eq!(marked, ["typeface"]);
		// a tab is its own path
		let (line, marked) = first("them", Target::Tab(tab("Appearance/Themes")));
		assert_eq!(line, format!("Appearance {sep} Themes"));
		assert_eq!(marked, ["Them"]);
		let (line, _) = first("hyperlinks", Target::Row(heading("Hyperlinks")));
		assert_eq!(line, format!("Window {sep} Hyperlinks"));
		// two rows that would read the same say which sub-group they lead from
		let (a, _) = first("width px", Target::Row(row_of(Key::ScrollbarThickness)));
		let (b, _) = first("width px", Target::Row(row_of(Key::MinimapWidth)));
		assert_eq!(a, format!("Movement {sep} Scrollbar {sep} Width px"));
		assert_eq!(b, format!("Movement {sep} Minimap {sep} Width px"));
		// loose marks land on the letters that matched
		let (_, marked) = first("lnhght", Target::Row(row_of(Key::LineHeight)));
		assert_eq!(marked.concat(), "Lnhght");
	}

	// No two results read the same, or picking between them is a guess.
	// Test ID: de03z
	#[test]
	fn no_two_results_read_the_same() {
		let specs = &ui().specs;
		let mut seen = std::collections::HashMap::new();
		for hit in rank(specs, "e") {
			let (line, _) = crumb(specs, &hit);
			if let Some(was) = seen.insert(line.clone(), hit.to) {
				panic!("{line} is both {was:?} and {:?}", hit.to);
			}
		}
		assert!(seen.len() > 50, "only {} results for e", seen.len());
	}

	// Every alias is on a row the list can show, says something its label
	// doesn't, and finds that row by itself.
	// Test ID: de040
	#[test]
	fn every_alias_finds_the_row_it_is_on() {
		let specs = &ui().specs;
		let mut count = 0;
		for (row, spec) in specs.iter().enumerate() {
			if spec.aliases.is_empty() {
				continue;
			}
			// a tab-title heading lends its aliases to the tab
			let to = if super::SettingsDialog::header_is_tab_title(spec) {
				Target::Tab(super::leaf_node(tab_nodes(), spec.tab))
			} else {
				Target::Row(row)
			};
			let label = spec.label.to_lowercase();
			let mut mine = std::collections::HashSet::new();
			for alias in spec.aliases {
				count += 1;
				let lower = alias.to_lowercase();
				assert!(
					!label.contains(&lower),
					"{}: {alias} is in the label",
					spec.label
				);
				assert!(mine.insert(lower), "{}: {alias} twice", spec.label);
				let hits = rank(specs, alias);
				assert!(
					hits.iter().any(|h| h.to == to && h.tier != Tier::Loose),
					"{}: {alias} doesn't find it: {hits:?}",
					spec.label
				);
			}
		}
		assert!(count > 100, "only {count} aliases");
	}

	// Typing in the box lists matches over the page; Enter goes to the one
	// picked, on its tab, in view and with the keyboard on it, and empties
	// the box. Escape empties it first and cancels only once it's empty.
	// Test ID: de041
	#[test]
	fn enter_goes_to_the_pick_and_escape_empties_the_box_first() {
		let mut d = mk_dialog(420.0);
		let mut m = chars7;
		d.search_focus();
		assert_eq!(d.focus, Some(Focus::Search));
		for c in "minutes".chars() {
			d.char_input(c);
		}
		assert!(d.searching());
		assert!(d.search.hits.len() >= 2, "{:?}", d.search.hits);
		d.key_vertical(true);
		assert_eq!(d.search.sel, 1);
		let want = d.search.hits[1].to;
		let Target::Row(row) = want else {
			panic!("{want:?}")
		};
		assert_eq!(
			d.key_enter(),
			super::super::Action::None,
			"Enter is the pick, not OK"
		);
		assert!(!d.searching() && d.search.query.is_empty());
		assert_eq!(d.tab, d.specs[row].tab);
		assert_eq!(d.focus, Some(Focus::Row(row, 0)));
		let vp = d.viewport();
		let top = d.row_y(row);
		assert!(
			top >= vp.y && top + d.row_screen_h(row) <= vp.y + vp.h,
			"in view"
		);
		assert_eq!(d.flash.map(|(r, _)| r), Some(row));
		let _ = d.rects_dip(d.line_h, &mut m);
		// the mark fades out, and the frame loop is woken until it's gone
		assert!(d.flash_step(0.1).is_some());
		assert!(d.flash_step(5.0).is_some());
		assert!(d.flash.is_none() && d.flash_step(0.1).is_none());

		d.search_focus();
		for c in "xyzzy".chars() {
			d.char_input(c);
		}
		assert!(d.searching() && d.search.hits.is_empty());
		assert_eq!(
			d.key_enter(),
			super::super::Action::None,
			"nothing to go to"
		);
		assert_eq!(
			d.key_escape(),
			super::super::Action::None,
			"first Esc empties"
		);
		assert!(!d.searching());
		assert!(
			d.edit
				.as_ref()
				.is_some_and(|e| e.row == SEARCH_ROW && e.buf.is_empty())
		);
		assert_eq!(
			d.key_escape(),
			super::super::Action::Cancel,
			"then it cancels"
		);
	}

	// While the list is up the rows behind it take no clicks and no keys, and
	// a click on a result goes there.
	// Test ID: de042
	#[test]
	fn the_list_covers_the_rows_behind_it() {
		let mut d = mk_dialog(900.0);
		let mut m = chars7;
		let page = d.tab;
		d.search_focus();
		for c in "blink".chars() {
			d.char_input(c);
		}
		// Tab walks the box and the footer only
		for _ in 0..5 {
			d.key_tab();
			assert!(
				matches!(d.focus, Some(Focus::Search | Focus::Button(_))),
				"{:?}",
				d.focus
			);
		}
		let (_, rows) = d.rects_dip(d.line_h, &mut m);
		assert!(rows.len() <= 1, "only the pick's shade: {}", rows.len());
		let texts = d.texts_dip(d.line_h, &mut m);
		assert!(texts.iter().any(|t| t.marks.is_some()), "results drawn");
		assert!(
			!texts.iter().any(|t| t.text == "Wheel lines"),
			"no row labels"
		);
		assert_eq!(d.tab, page);
		let want = d.search.hits[0].to;
		let r = d.hit_rect(0);
		let _ = d.mouse_down_dip(r.x + 20.0, r.y + r.h / 2.0, &mut m);
		assert!(!d.searching());
		let Target::Row(row) = want else {
			panic!("{want:?}")
		};
		assert_eq!(d.tab, d.specs[row].tab);
		// a tab click empties the box and goes to that tab, as it always does
		d.search_focus();
		d.char_input('m');
		d.char_input('o');
		let node = tab("Window");
		let t = d.tab_rect(node);
		let _ = d.mouse_down_dip(t.x + 2.0, t.y + 2.0, &mut m);
		assert!(!d.searching());
		assert_eq!(d.tab, tab_nodes()[node].leaf.unwrap());
	}

	// The box is the dialog's one open edit at a row index no row has. Every
	// place that reads the rows by that index has to know it.
	// Test ID: de044
	#[test]
	fn the_search_box_is_no_rows_field() {
		let mut d = mk_dialog(900.0);
		let mut m = chars7;
		assert_eq!(super::super::shell_field_of(SEARCH_ROW), None);
		assert_eq!(super::super::pick_field_of(SEARCH_ROW), None);
		let field = d.search_box();
		let _ = d.mouse_down_dip(field.x + 5.0, field.y + field.h / 2.0, &mut m);
		let _ = d.mouse_up_dip(field.x + 5.0, field.y + field.h / 2.0);
		assert!(d.edit.as_ref().is_some_and(|e| e.row == SEARCH_ROW));
		d.insert_str("scrim\u{7}");
		assert_eq!(d.search.query, "scrim", "control characters dropped");
		d.select_all();
		d.backspace();
		assert!(!d.searching());
		for c in "size".chars() {
			d.char_input(c);
		}
		let _ = d.animate_dip(0.016, &mut m);
		let _ = d.rects_dip(d.line_h, &mut m);
		let _ = d.texts_dip(d.line_h, &mut m);
		let _ = d.hover_tip_dip(field.x + 5.0, field.y + 5.0);
		// Tab away keeps what was typed; the list stays up
		d.key_tab();
		assert!(d.searching() && d.edit.is_none());
		assert_eq!(d.edit_buf(SEARCH_ROW), "size");
	}

	// With nothing typed the box is outside the Tab walk, as the tabs are, so
	// Tab from nothing still lands on the tab's first control. From the box,
	// Tab goes on to that control and Shift+Tab back to OK.
	// Test ID: de045
	#[test]
	fn tab_from_the_empty_box_goes_on_to_the_first_control() {
		let mut d = mk_dialog(900.0);
		let first = d.first_focus();
		assert!(matches!(first, Some(Focus::Row(..))), "{first:?}");
		d.search_focus();
		d.key_tab();
		assert_eq!(d.focus, first);
		assert!(d.edit.as_ref().is_none_or(|e| e.row != SEARCH_ROW));
		d.search_focus();
		d.set_mods(false, true, false);
		d.key_tab();
		assert_eq!(d.focus, Some(Focus::Button(2)));
		d.set_mods(false, false, false);
		d.focus = None;
		d.key_tab();
		assert_eq!(d.focus, first, "Tab from nothing skips the box");
	}

	// The box takes a band over the rows on every tab, and the rows start
	// under it, so nothing on a tab moves relative to the others.
	// Test ID: de043
	#[test]
	fn the_search_box_sits_over_the_rows_and_clear_of_them() {
		let d = mk_dialog(900.0);
		let field = d.search_box();
		let vp = d.viewport();
		assert!(field.y + field.h + super::lay().search_gap <= vp.y + 0.01);
		assert!(field.x >= vp.x && field.x + field.w <= vp.x + vp.w);
		for node in 0..tab_nodes().len() {
			let t = d.tab_rect(node);
			assert!(
				t.w == 0.0 || t.x + t.w <= field.x || t.y + t.h <= field.y,
				"the box overlaps tab {node}"
			);
		}
	}
}
