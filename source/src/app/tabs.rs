// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The tab strip as drawn: which tab it starts at, and per tab shown, how wide
// it is and what it says. Tabs are no longer one width apiece, so a position on
// the bar is a running total rather than a multiplication (see tabtitle).
#[derive(Default)]
struct TabLayout {
	key: (u32, usize, usize, usize, u32, u64),
	first: usize,
	widths: Vec<f32>,
	labels: Vec<String>,
}

impl TabLayout {
	fn shown(&self) -> usize {
		self.widths.len()
	}

	fn x(&self, i: usize) -> Option<f32> {
		(i >= self.first && i < self.first + self.shown())
			.then(|| crate::tabtitle::slot_x(&self.widths, i - self.first))
	}

	fn w(&self, i: usize) -> Option<f32> {
		self.widths.get(i.checked_sub(self.first)?).copied()
	}

	fn at_x(&self, x: f32) -> Option<usize> {
		crate::tabtitle::slot_at_x(&self.widths, x).map(|slot| self.first + slot)
	}
}

// What one tab's label is made from. It is kept beside the forms it gave, so a
// frame can tell whether they still stand by comparing rather than building.
#[derive(Clone, Debug, Default, PartialEq)]
struct LabelFacts {
	// the text being typed, while this tab is the one being renamed
	edit: Option<String>,
	title_override: Option<String>,
	// the focused pane's own title, and the program it was started with
	said: String,
	launched: Option<String>,
	command: Option<Vec<String>>,
	task: crate::term::Task,
	cwd: Option<PathBuf>,
}

// One tab's label forms, longest first, and the room the longest and the
// shortest of them want.
struct TabLabel {
	facts: LabelFacts,
	forms: Vec<String>,
	demand: crate::tabtitle::Demand,
}

// Every tab's label, kept between frames. A frame still reads each tab's facts,
// since a shell's task and folder are only known by asking, but forms are built
// and measured again only for a tab whose facts moved, and for all of them when
// the settings or the font did. Entries go by position, so a moved tab is
// rebuilt the first time its slot reads differently.
#[derive(Default)]
struct TabLabels {
	tabs: Vec<Option<TabLabel>>,
	// What every entry was built against. Any settings change is a new snapshot,
	// and any font, zoom or scale change a new text context.
	settings: Option<Arc<config::Settings>>,
	text: u64,
	// moves whenever an entry does, so the layout knows to measure again
	revision: u64,
	// forms built so far
	builds: usize,
}

impl TabLabels {
	fn fit(&mut self, count: usize) {
		if self.tabs.len() != count {
			self.tabs.resize_with(count, || None);
			self.revision += 1;
		}
	}

	// Bring tab `index` up to date with `facts`, building its forms only if they
	// moved since last time.
	fn keep(
		&mut self,
		index: usize,
		facts: LabelFacts,
		settings: &Arc<config::Settings>,
		text: &mut TextCtx,
	) {
		let same_settings = self
			.settings
			.as_ref()
			.is_some_and(|seen| Arc::ptr_eq(seen, settings));
		if !same_settings || self.text != text.generation {
			self.settings = Some(Arc::clone(settings));
			self.text = text.generation;
			self.tabs.iter_mut().for_each(|tab| *tab = None);
			self.revision += 1;
		}
		let Some(slot) = self.tabs.get_mut(index) else {
			return;
		};
		if slot.as_ref().is_some_and(|tab| tab.facts == facts) {
			return;
		}
		let forms = label_forms_from(&facts, settings);
		// what a tab spends on itself rather than on its label
		let chrome =
			2.0 * config::dip(TAB_TITLE_PAD, text.scale) + config::dip(TAB_CLOSE_W, text.scale);
		let attrs = crate::text::ui_attrs();
		let mut width_of =
			|form: Option<&String>| form.map_or(0.0, |s| text.measure_ui_text(s, &attrs));
		let demand = crate::tabtitle::Demand {
			natural: width_of(forms.first()) + chrome,
			floor: width_of(forms.last()) + chrome,
		};
		*slot = Some(TabLabel {
			facts,
			forms,
			demand,
		});
		self.builds += 1;
		self.revision += 1;
	}

	fn forms(&self, index: usize) -> &[String] {
		self.tabs
			.get(index)
			.and_then(Option::as_ref)
			.map_or(&[], |tab| tab.forms.as_slice())
	}
}

// Everything a tab could say, longest form first (see tabtitle). A `--title`
// override is the whole answer; otherwise it is the shell's FRIENDLY name -
// what the Shells list calls it, which is the name the user themselves gave
// it - plus whatever that shell has to report: the command it is running,
// the last one it ran, or, having run nothing at all, where it is.
fn label_forms_from(facts: &LabelFacts, settings: &config::Settings) -> Vec<String> {
	// A rename in progress is what the tab says, so what is typed is what is
	// seen - and it is one form, never shortened, or the caret would sit off
	// the end of an abbreviated label.
	if let Some(text) = &facts.edit {
		return vec![text.clone()];
	}
	if let Some(title) = &facts.title_override {
		return vec![title.clone()];
	}
	let command_line = tab_command_line(facts.command.as_deref());
	let friendly = crate::shells::friendly(&command_line, &settings.shells);
	let cwd = facts
		.cwd
		.as_ref()
		.map(|dir| dir.to_string_lossy().into_owned());
	let home = config::home_dir().map(|dir| dir.to_string_lossy().into_owned());
	let task = match &facts.task {
		crate::term::Task::Running(program) => Some(crate::tabtitle::Task::Running(program)),
		crate::term::Task::Last(program) => Some(crate::tabtitle::Task::Last(program)),
		crate::term::Task::Idle => None,
	};
	crate::tabtitle::label_forms(
		&friendly,
		crate::tabtitle::program_title(config::rights(), &facts.said, facts.launched.as_deref()),
		task,
		cwd.as_deref(),
		home.as_deref(),
		crate::tabtitle::Style::native(),
		crate::tabtitle::Parts {
			title: settings.tab_shows_title,
			shell: settings.tab_shows_shell,
			program: settings.tab_shows_program,
			directory: settings.tab_shows_directory,
		},
	)
}

// Tab strip: each tab owns its own pane split-tree. Detach/dock to other
// windows is deferred (needs multi-window support).
struct Tabs {
	list: Vec<PaneManager>,
	active: usize,
}

impl Tabs {
	fn cur(&self) -> &PaneManager {
		&self.list[self.active]
	}
	fn cur_mut(&mut self) -> &mut PaneManager {
		&mut self.list[self.active]
	}
	fn len(&self) -> usize {
		self.list.len()
	}
	// PaneIds are globally unique; the pane may live in any tab, not just the
	// active one (background-tab shells reply to ESC[6n etc. too)
	fn find_pane(&self, id: PaneId) -> Option<&Pane> {
		self.list.iter().find_map(|pm| pm.panes.get(&id))
	}
	fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
		self.list.iter_mut().find_map(|pm| pm.panes.get_mut(&id))
	}
	fn next(&mut self) {
		self.active = tab_step(self.active, self.list.len(), true);
	}
	fn prev(&mut self) {
		self.active = tab_step(self.active, self.list.len(), false);
	}
	fn move_active(&mut self, fwd: bool) {
		self.active = move_tab(&mut self.list, self.active, fwd);
	}
}

// The tab beside `i` of `n`, wrapping at both ends.
fn tab_step(i: usize, n: usize, forward: bool) -> usize {
	if forward {
		(i + 1) % n
	} else {
		(i + n - 1) % n
	}
}

// The active tab once the one at `index` has gone and `len` are left. The same
// tab where it can be, else the one that took its place, else the last. A tab
// opened beside the one it came from goes back to that one (`opener`, where it
// sits now), since that is where its maker was.
fn active_after_close(active: usize, index: usize, len: usize, opener: Option<usize>) -> usize {
	if active == index
		&& let Some(back) = opener
	{
		return back;
	}
	let active = if active > index { active - 1 } else { active };
	active.min(len.saturating_sub(1))
}

// Put a new tab just right of the active one, or at the end, and answer where
// it went. It becomes the active tab either way.
fn insert_tab<T>(list: &mut Vec<T>, active: usize, tab: T, beside: bool) -> usize {
	let at = if beside {
		(active + 1).min(list.len())
	} else {
		list.len()
	};
	list.insert(at, tab);
	at
}

// Swap the tab at `i` with its neighbour and answer where it went, so the
// active tab follows. Past either end it trades places with the far one.
fn move_tab<T>(list: &mut [T], i: usize, forward: bool) -> usize {
	if list.len() < 2 {
		return i;
	}
	let j = tab_step(i, list.len(), forward);
	list.swap(i, j);
	j
}

// Top and height of a tab button inside the bar: inset at the top, and the bar's
// bottom hairline left showing under it. The draw and the text centering read
// the one rule.
fn tab_button_v(bar_y: f32, tab_h: f32, scale: f32) -> (f32, f32) {
	let top = config::dip(TAB_TOP_PAD, scale);
	let rule = config::dip(CHROME_HAIRLINE, scale);
	(bar_y + top, tab_h - top - rule)
}

// The close-"x" button box within a tab: a square with equal top/right/bottom
// margins (the extra room falls to the left, separating it from the title).
// Shared by the rect draw, the glyph placement, and the click hit-test so they
// can't drift apart.
fn tab_close_box(tab_x: f32, tab_w: f32, bar_y: f32, tab_h: f32, scale: f32) -> Rect {
	let m = config::dip(TAB_CLOSE_M, scale);
	let side = (tab_h - 2.0 * m).max(config::dip(8.0, scale));
	Rect {
		x: tab_x + tab_w - m - side,
		y: bar_y + m,
		w: side,
		h: side,
	}
}
// The box a tab being renamed types into: a real text field inside the tab
// button, stopping short of the close column and as tall as a line of text plus
// its own padding, or the button itself where that is shorter.
fn tab_edit_box(tab_x: f32, tab_w: f32, bar_y: f32, tab_h: f32, line_h: f32, scale: f32) -> Rect {
	let inset = config::dip(TAB_EDIT_INSET, scale);
	let (btn_y, btn_h) = tab_button_v(bar_y, tab_h, scale);
	let h = (line_h + 2.0 * config::dip(TAB_EDIT_PAD, scale)).min(btn_h - 2.0 * inset);
	let x = tab_x + inset;
	let right = tab_x + tab_w - config::dip(TAB_CLOSE_W, scale) - inset;
	Rect {
		x,
		y: btn_y + (btn_h - h) / 2.0,
		w: (right - x).max(config::dip(8.0, scale)),
		h,
	}
}

// How much of a tab its title actually gets: the button less its own inset on
// both sides and the close-button column it must never run under. The draw and
// the fit read the one rule, or a title is shortened to a width it is not then
// given.
fn tab_title_w(tab_w: f32, scale: f32) -> f32 {
	let pad = config::dip(TAB_TITLE_PAD, scale);
	(tab_w - 2.0 * pad - config::dip(TAB_CLOSE_W, scale)).max(config::dip(8.0, scale))
}

// The command line behind a tab, for naming the shell it runs. Every pane
// resolves its own at spawn (see `spawn_pane`), so None here means nothing is
// switched on at all and the engine picked its own default - which we have no
// way to name, and must not GUESS at from the list: guessing is what had a pane
// running PowerShell labelled Command Prompt.
fn tab_command_line(command: Option<&[String]>) -> String {
	command.map_or_else(String::new, crate::shells::command_line)
}

// A tab's hover tip: what it runs, how it was started, where it is, and how
// long it has been open - the three of those a tab is too narrow to say, plus
// the one it never says. The lines are built on a timer rather than per frame:
// naming the shell resolves its program on the filesystem, and the clock at the
// bottom has to tick anyway.
struct TabTip {
	tab: usize,
	lines: Vec<String>,
	built: Instant,
	// the widest line, and the text context that measured it
	width: Option<(u64, f32)>,
}

impl TabTip {
	// The widest line, measured once per set of lines rather than per frame. A
	// new text context (a font or zoom change) measures again.
	fn text_w(&mut self, generation: u64, mut measure: impl FnMut(&str) -> f32) -> f32 {
		if let Some((seen, w)) = self.width {
			if seen == generation {
				return w;
			}
		}
		let w = self
			.lines
			.iter()
			.fold(0.0f32, |widest, line| widest.max(measure(line)));
		self.width = Some((generation, w));
		w
	}
}

impl State {
	// Measure the strip afresh: what each tab's label wants, what the least it
	// can be given is, which tabs that leaves on the page, and the widest label
	// form that fits the width each one ends up with.
	//
	// Kept rather than recomputed per call, because measuring every tab's label
	// on each mouse move or frame would be paid for on each one. Everything goes
	// through `tab_layout`, which rebuilds only when one of the inputs in
	// `tab_layout_key` moved; the labels' own revision is one of them, so a
	// label that changed is measured again (see TabLabels).
	fn rebuild_tab_layout(&mut self) {
		// the count may have moved since the last frame read the labels
		self.refresh_tab_labels(None);
		let total = self.surface_px.0 as f32;
		let scale = self.text.scale;
		let attrs = crate::text::ui_attrs();
		// every slot was filled just above
		let demands: Vec<crate::tabtitle::Demand> = self
			.tab_labels
			.tabs
			.iter()
			.map(|tab| tab.as_ref().map(|tab| tab.demand).unwrap_or_default())
			.collect();
		let floors: Vec<f32> = demands.iter().map(|d| d.floor).collect();
		// Bring the active tab onto the page when it CHANGES - and only then, so
		// a page the wheel moved to stays put. Driven from the change rather than
		// from each of the many places that set `tabs.active`, so no path misses
		// it. `tab_first` is otherwise only a preference, clamped on read, so
		// opening or closing a tab cannot strand it.
		if self.tab_followed != self.tabs.active {
			self.tab_followed = self.tabs.active;
			self.tab_first =
				crate::tabtitle::page_for(self.tab_first, self.tabs.active, &floors, total);
		}
		let first = crate::tabtitle::clamp_page(self.tab_first, &floors, total);
		let shown = crate::tabtitle::tabs_that_fit(total, &floors, first)
			.min(self.tabs.len().saturating_sub(first));
		let settings = config::settings();
		// The tab in front takes what the row can spare, so the strip has to know
		// which slot it is on this page - and nothing, when it is on another.
		let active_slot = self
			.tabs
			.active
			.checked_sub(first)
			.filter(|slot| *slot < shown);
		let widths = crate::tabtitle::widths(
			total,
			&demands[first..first + shown],
			settings.tab_regular_pct,
			settings.tab_max_pct,
			active_slot,
		);
		// The widest form that fits the space this tab ended up with, else the
		// shortest there is - which still names the shell, so it reads as a tab
		// even clipped.
		let labels = widths
			.iter()
			.enumerate()
			.map(|(slot, w)| {
				let title_w = tab_title_w(*w, scale);
				let tab = self.tab_labels.forms(first + slot);
				tab.iter()
					.find(|form| self.text.measure_ui_text(form, &attrs) <= title_w)
					.or_else(|| tab.last())
					.cloned()
					.unwrap_or_default()
			})
			.collect();
		self.tab_layout = TabLayout {
			key: self.tab_layout_key(),
			first,
			widths,
			labels,
		};
	}

	// What the strip was measured from. A mouse move is not on the list, which
	// is the point of having one.
	fn tab_layout_key(&self) -> (u32, usize, usize, usize, u32, u64) {
		(
			self.surface_px.0,
			self.tabs.len(),
			self.tabs.active,
			self.tab_first,
			self.text.scale.to_bits(),
			self.tab_labels.revision,
		)
	}

	// Bring the kept labels up to date: every tab's, or only one. Reading a tab's
	// facts is cheap; building its forms is the part that is skipped.
	fn refresh_tab_labels(&mut self, only: Option<usize>) {
		let settings = config::settings();
		self.tab_labels.fit(self.tabs.len());
		let tabs = only.map_or(0..self.tabs.len(), |index| index..index + 1);
		for index in tabs {
			if let Some(facts) = self.label_facts(index) {
				self.tab_labels
					.keep(index, facts, &settings, &mut self.text);
			}
		}
	}

	// While the bar is hidden nothing is built or measured, but every tab's
	// shell is still asked what it runs. A shell only learns its last command
	// by being asked while that command runs, so skipping this would lose one
	// that started and finished before the bar came back.
	fn probe_tabs(&mut self) {
		for index in 0..self.tabs.len() {
			let _ = self.label_facts(index);
		}
	}

	// The strip as drawn, measured again only if one of its inputs moved.
	fn tab_layout(&mut self) -> &TabLayout {
		if self.tab_layout.key != self.tab_layout_key() {
			self.rebuild_tab_layout();
		}
		&self.tab_layout
	}

	// Where tab `i` sits on the bar and how wide it is, or None when it is on
	// another page. Drawing and both hit tests read this one answer, or a click
	// sits on a different tab than the one under the pointer.
	fn tab_box(&mut self, i: usize) -> Option<(f32, f32)> {
		let layout = self.tab_layout();
		Some((layout.x(i)?, layout.w(i)?))
	}

	// Which tab a pointer at `x` is over - the inverse of `tab_box`, and the only
	// thing the two hit tests may use.
	fn tab_at(&mut self, x: f32) -> Option<usize> {
		self.tab_layout().at_x(x)
	}

	// The close button of tab `i`, if that tab is on the page.
	fn tab_close_box_at(&mut self, i: usize, bar_y: f32, tab_h: f32) -> Option<Rect> {
		let (x, w) = self.tab_box(i)?;
		Some(tab_close_box(x, w, bar_y, tab_h, self.text.scale))
	}

	// A wheel over the tab bar turns the page. Without it a tab past the edge
	// could only be reached from the keyboard or the Tabs menu.
	fn scroll_tab_strip(&mut self, lines: f32) {
		let first = self.tab_layout().first;
		let step = if lines > 0.0 {
			first.saturating_sub(1)
		} else {
			first.saturating_add(1)
		};
		if step != self.tab_first {
			self.tab_first = step;
			self.dirty = true;
		}
	}

	// Everything tab `index` could say, built afresh (see label_forms_from).
	// The strip reads the kept copy in `tab_labels` instead.
	fn tab_label_forms(&mut self, index: usize) -> Vec<String> {
		let settings = config::settings();
		self.label_facts(index).map_or_else(
			|| vec![config::APP_NAME.to_string()],
			|facts| label_forms_from(&facts, &settings),
		)
	}

	// What tab `index`'s label is made from, or None for no such tab. A rename
	// or a title of its own is the whole label, so the shell is not asked.
	fn label_facts(&mut self, index: usize) -> Option<LabelFacts> {
		let edit = self
			.tab_edit
			.as_ref()
			.filter(|edit| edit.tab == index)
			.map(|edit| edit.text.clone());
		let pm = self.tabs.list.get_mut(index)?;
		if edit.is_some() || pm.title_override.is_some() {
			return Some(LabelFacts {
				edit,
				title_override: pm.title_override.clone(),
				..LabelFacts::default()
			});
		}
		// The focused pane's own title, plus the program the pane was started
		// with - that is what tells a console's own decoration apart from text
		// somebody chose.
		let focused_id = pm.focused;
		let (said, launched) = pm.panes.get(&focused_id).map_or_else(
			|| (String::new(), None),
			|pane| (pane.title.clone(), pane.launched().map(str::to_string)),
		);
		let (command, task, cwd) = pm.tab_facts();
		Some(LabelFacts {
			edit: None,
			title_override: None,
			said,
			launched,
			command,
			task,
			cwd,
		})
	}

	// Which tab the pointer is over, and since when. Anything the pointer is
	// already busy with - a drag, an open menu - owns it instead, so no tip
	// appears underneath one.
	fn note_tab_hover(&mut self, x: f32, y: f32) {
		let busy = self.bar_dragging.is_some()
			|| self.map_dragging.is_some()
			|| self.dragging_pane.is_some()
			|| self.tab_edit.is_some()
			|| self.menu.is_some()
			|| self.bar_open.is_some()
			|| self.tab_close_arm.is_some();
		let bar_y = self.menubar_h();
		let over = if busy || !self.tab_bar_visible() || y < bar_y || y >= bar_y + self.tab_bar_h()
		{
			None
		} else {
			self.tab_at(x)
		};
		if self.tab_hover.point_at(over) && self.tab_tip.take().is_some() {
			self.dirty = true;
		}
	}

	// Bring the tab tip up once the pointer has rested, and keep what it says
	// current while it is up. Returns true when the frame has to be redrawn.
	fn update_tab_tip(&mut self) -> bool {
		let limit =
			Duration::try_from_secs_f32(config::settings().tab_tip_max_s).unwrap_or_default();
		let Some(tab) = self.tab_hover.ripe_for(limit) else {
			return self.tab_hover.wake().is_none() && self.tab_tip.take().is_some();
		};
		let now = Instant::now();
		let stale = self
			.tab_tip
			.as_ref()
			.is_none_or(|tip| tip.tab != tab || now.duration_since(tip.built) >= TAB_TIP_REFRESH);
		if !stale {
			return false;
		}
		let lines = self.tab_tip_lines(tab);
		let kept = self
			.tab_tip
			.as_ref()
			.filter(|tip| tip.tab == tab && tip.lines == lines)
			.map(|tip| tip.width);
		self.tab_tip = Some(TabTip {
			tab,
			lines,
			built: now,
			width: kept.flatten(),
		});
		kept.is_none()
	}

	// When the loop next has to wake for the tip - to raise one whose pointer has
	// rested, or to re-read one that is already up (its clock ticks).
	fn tab_tip_wake(&self) -> Option<Instant> {
		match &self.tab_tip {
			Some(tip) => Some(tip.built + TAB_TIP_REFRESH),
			None => self.tab_hover.wake(),
		}
	}

	// What a tip says, as key/value pairs padded to one column (tabtitle::tip_lines).
	// The path is shown WHOLE here - the tab is where it gets shortened, and the tip
	// is the place to look when the short form was not enough. A value that carries
	// a space or a quote is quoted, so its edges are never in doubt.
	fn tab_tip_lines(&mut self, index: usize) -> Vec<String> {
		let Some(pm) = self.tabs.list.get_mut(index) else {
			return Vec::new();
		};
		let created = pm.created;
		let override_title = pm.title_override.clone();
		let focused_id = pm.focused;
		let (said, launched) = pm.panes.get(&focused_id).map_or_else(
			|| (String::new(), None),
			|pane| (pane.title.clone(), pane.launched().map(str::to_string)),
		);
		let (command, task, cwd) = pm.tab_facts();
		let settings = config::settings();
		let command_line = tab_command_line(command.as_deref());
		let quoted = crate::tabtitle::tip_value;
		let mut rows: Vec<(&str, String)> = Vec::new();
		if let Some(title) = override_title {
			rows.push(("Tab title", quoted(&title)));
		}
		if let Some(said) =
			crate::tabtitle::program_title(config::rights(), &said, launched.as_deref())
		{
			rows.push(("Program title", quoted(said)));
		}
		rows.push((
			"Shell name",
			quoted(&crate::shells::friendly(&command_line, &settings.shells)),
		));
		if !command_line.is_empty() {
			rows.push(("Shell command", quoted(&command_line)));
		}
		// Only what is running NOW. A tab already says so itself, but it says it in
		// the width it has left; the tip has the whole name.
		if let crate::term::Task::Running(program) = task {
			rows.push(("Running", quoted(&program)));
		}
		rows.push((
			"Current path",
			cwd.map_or_else(
				// not a value, so it takes no quotes - a directory called
				// "(not reported)" is not what this line is saying
				|| "(not reported)".to_string(),
				|dir| {
					quoted(
						&crate::tabtitle::path_forms(
							&dir.to_string_lossy(),
							None,
							crate::tabtitle::Style::native(),
						)
						.into_iter()
						.next()
						.unwrap_or_default(),
					)
				},
			),
		));
		// a clock reading, not a value either
		rows.push((
			"Open",
			crate::tabtitle::elapsed(created.elapsed().as_secs()),
		));
		crate::tabtitle::tip_lines(&rows)
	}

	// The tip's box, and where each of its lines sits inside it. Measured in the
	// TERMINAL font, which is the one thing in the chrome that is: the lines are a
	// key/value table padded with spaces, and spaces align nothing in a
	// proportional face. The box fits the longest line rather than guessing; it
	// hangs off its own TAB rather than off the pointer, so it does not jitter as
	// the pointer moves about inside one, and it is pushed back inside the window
	// rather than being allowed to run off the right edge.
	fn tab_tip_layout(&mut self) -> Option<(Rect, Vec<(f32, f32, String)>)> {
		let generation = self.text.generation;
		let text = &mut self.text;
		let tip = self.tab_tip.as_mut()?;
		if tip.lines.is_empty() {
			return None;
		}
		let text_w = tip.text_w(generation, |line| text.measure_mono_text(line));
		let (tab, lines) = (tip.tab, tip.lines.clone());
		let pad = self.text.dip(TAB_TIP_PAD);
		let line_h = self.text.cell_h;
		let w = text_w + 2.0 * pad;
		let h = line_h * lines.len() as f32 + 2.0 * pad;
		let win_w = self.surface_px.0 as f32;
		// A tab paged off the strip while its tip was up takes the tip with it -
		// a tip hanging off nothing would sit at the bar's left end, pointing at
		// whichever tab happened to be there.
		let x = self.tab_box(tab)?.0.min((win_w - w).max(0.0)).max(0.0);
		let y = self.menubar_h() + self.tab_bar_h() + self.text.dip(TAB_TIP_GAP);
		let placed = lines
			.into_iter()
			.enumerate()
			.map(|(i, line)| (x + pad, y + pad + line_h * i as f32, line))
			.collect();
		Some((Rect { x, y, w, h }, placed))
	}

	fn new_tab(&mut self, proxy: &EventLoopProxy<UserEvent>) {
		self.new_tab_with(proxy, None);
	}

	// `shell` is a shell picked by name from the Tabs menu; None inherits from
	// the pane that was active, as a plain new tab does. The directory is
	// inherited either way - picking a shell says nothing about where to start.
	fn new_tab_with(&mut self, proxy: &EventLoopProxy<UserEvent>, shell: Option<Vec<String>>) {
		self.commit_tab_edit();
		// area with the bar shown (we're about to have >1 tab); relayout_all fixes
		// the exact rects right after, this is just the new pane's provisional box
		let bar = self.menubar_h() + self.tab_bar_h();
		let area = Rect {
			x: 0.0,
			y: bar,
			w: self.surface_px.0 as f32,
			h: (self.surface_px.1 as f32 - bar).max(1.0),
		};
		// inherit shell + directory from the pane that was active when the tab
		// was opened; a default-shell pane carries None -> still the default
		let (cmd, cwd) = self
			.tabs
			.list
			.get(self.tabs.active)
			.map_or((None, None), PaneManager::inherit_spawn);
		let cmd = shell.or(cmd).or_else(config::default_shell_argv);
		if let Ok(pm) = PaneManager::new(&mut self.text, proxy, area, cmd, cwd) {
			let beside = config::settings().new_tab_beside;
			let made = pm.focused;
			self.tab_opener = self
				.tabs
				.list
				.get(self.tabs.active)
				.filter(|_| beside)
				.map(|from| (made, from.focused));
			self.tabs.active = insert_tab(&mut self.tabs.list, self.tabs.active, pm, beside);
			if self.tabs.active + 1 < self.tabs.len() {
				self.forget_tab_pointer();
			}
			self.relayout_all(); // existing tab(s) shrink for the now-shown bar
			self.update_title();
			self.dirty = true;
		}
	}

	// What the pointer was doing to a tab is keyed by its position, and the tabs
	// right of a new one have each moved along one. A held close button would
	// close the wrong tab on release.
	fn forget_tab_pointer(&mut self) {
		self.tab_close_arm = None;
		self.tab_dbl = None;
		self.tab_hover.point_at(None);
		self.tab_tip = None;
	}

	fn close_tab(&mut self) {
		self.close_tab_at(self.tabs.active);
	}

	// the tab to the left or right, round the end
	fn step_tab(&mut self, forward: bool) {
		if forward {
			self.tabs.next();
		} else {
			self.tabs.prev();
		}
		self.freeze_catchup();
		self.update_title();
		self.dirty = true;
	}

	// Close the tab at `idx` (not necessarily the active one - a background tab's
	// shell can exit). Keeps `active` pointing at the same tab where it can.
	fn close_tab_at(&mut self, idx: usize) {
		// A rename is keyed by position, so any change to the list ends it.
		self.cancel_tab_edit();
		if self.tabs.list.len() <= 1 {
			self.quit = true; // closing the only tab closes the window
			return;
		}
		let showed = idx == self.tabs.active;
		let opener = match self.tab_opener {
			Some((made, from)) if self.tabs.list[idx].panes.contains_key(&made) => {
				self.tab_opener = None;
				Some(from)
			}
			_ => None,
		};
		self.tabs.list.remove(idx);
		let opener = opener.and_then(|from| {
			self.tabs
				.list
				.iter()
				.position(|pm| pm.panes.contains_key(&from))
		});
		self.tabs.active = active_after_close(self.tabs.active, idx, self.tabs.len(), opener);
		if showed {
			self.freeze_catchup(); // closing the shown tab reveals a frozen one
		}
		self.relayout_all(); // if back to 1 tab, the bar hides and panes grow
		self.update_title();
		self.dirty = true;
	}
}

#[cfg(test)]
mod tests {
	// One frame's pass over the strip's labels, as the render makes it. Returns
	// how many tabs built their forms.
	fn label_frame(
		labels: &mut super::TabLabels,
		tabs: &[super::LabelFacts],
		settings: &std::sync::Arc<config::Settings>,
		text: &mut TextCtx,
	) -> usize {
		let before = labels.builds;
		labels.fit(tabs.len());
		for (index, facts) in tabs.iter().enumerate() {
			labels.keep(index, facts.clone(), settings, text);
		}
		labels.builds - before
	}

	fn label_settings(shell_title: &str) -> std::sync::Arc<config::Settings> {
		std::sync::Arc::new(config::Settings {
			tab_shows_title: true,
			tab_shows_shell: true,
			tab_shows_program: true,
			tab_shows_directory: true,
			shells: vec![ShellEntry {
				slug: "silk-test-sh".into(),
				title: shell_title.into(),
				command: "silk-test-sh".into(),
				active: true,
				comment: String::new(),
				last_seen: String::new(),
			}],
			..(*config::settings()).clone()
		})
	}

	fn label_tab(n: usize) -> super::LabelFacts {
		super::LabelFacts {
			command: Some(vec!["silk-test-sh".into()]),
			cwd: Some(format!("/srv/work/project{n}/src").into()),
			..super::LabelFacts::default()
		}
	}

	// The strip used to build and measure every tab's label on every frame,
	// with nothing on screen changing.
	// Test ID: Erlb8mF
	#[test]
	fn idle_frames_build_no_tab_labels() {
		let mut text = TextCtx::new_cpu(1.0);
		let settings = label_settings("Work shell");
		let tabs: Vec<_> = (0..3).map(label_tab).collect();
		let mut labels = super::TabLabels::default();
		// four seconds at 60 frames a second
		let built: usize = (0..240)
			.map(|_| label_frame(&mut labels, &tabs, &settings, &mut text))
			.sum();
		assert!(
			built <= tabs.len(),
			"{built} label builds over 240 idle frames"
		);
	}

	// Whatever a label shows makes that tab build again, and only that tab: its
	// title, task, folder, rename or own title. The settings and the font make
	// every tab build. A move the strip missed would show a stale label until
	// something else happened.
	// Test ID: Erlb98Q
	#[test]
	fn a_tab_label_builds_again_when_what_it_shows_moves() {
		use crate::term::Task;
		let mut text = TextCtx::new_cpu(1.0);
		let settings = label_settings("Work shell");
		let mut tabs: Vec<_> = (0..3).map(label_tab).collect();
		let mut labels = super::TabLabels::default();
		assert_eq!(label_frame(&mut labels, &tabs, &settings, &mut text), 3);
		assert!(
			labels.forms(0)[0].contains("Work shell"),
			"{:?}",
			labels.forms(0)
		);
		let moves: [(&str, fn(&mut super::LabelFacts)); 7] = [
			("program title", |tab| tab.said = "notes.txt - vim".into()),
			("launched", |tab| tab.launched = Some("vim".into())),
			("task", |tab| tab.task = Task::Running("make".into())),
			("last task", |tab| tab.task = Task::Last("make".into())),
			("folder", |tab| tab.cwd = Some("/srv/elsewhere".into())),
			("own title", |tab| tab.title_override = Some("Logs".into())),
			("rename", |tab| tab.edit = Some("Lo".into())),
		];
		for (what, change) in moves {
			let revision = labels.revision;
			change(&mut tabs[1]);
			assert_eq!(
				label_frame(&mut labels, &tabs, &settings, &mut text),
				1,
				"{what}"
			);
			assert_ne!(labels.revision, revision, "{what} reaches the layout");
			assert_eq!(
				label_frame(&mut labels, &tabs, &settings, &mut text),
				0,
				"{what}, again"
			);
		}
		assert_eq!(labels.forms(1), ["Lo"]);
		// a tab moved along the strip: both slots read differently now
		tabs.swap(0, 2);
		assert_eq!(label_frame(&mut labels, &tabs, &settings, &mut text), 2);
		// closed, then opened
		let revision = labels.revision;
		tabs.pop();
		assert_eq!(label_frame(&mut labels, &tabs, &settings, &mut text), 0);
		assert_ne!(labels.revision, revision, "a closed tab reaches the layout");
		tabs.push(label_tab(7));
		assert_eq!(label_frame(&mut labels, &tabs, &settings, &mut text), 1);
		// The bar hidden: no frame reads the strip while a folder moves, and the
		// first one after it shows again has the new one.
		tabs[0].cwd = Some("/srv/later".into());
		assert_eq!(label_frame(&mut labels, &tabs, &settings, &mut text), 1);
		// the shell renamed in the list
		let renamed = label_settings("Build box");
		assert_eq!(label_frame(&mut labels, &tabs, &renamed, &mut text), 3);
		assert!(
			labels.forms(0)[0].contains("Build box"),
			"{:?}",
			labels.forms(0)
		);
		// a font or zoom change is a new text context
		let mut zoomed = TextCtx::new_cpu(1.5);
		assert_eq!(label_frame(&mut labels, &tabs, &renamed, &mut zoomed), 3);
		assert_eq!(label_frame(&mut labels, &tabs, &renamed, &mut zoomed), 0);
	}

	// A new tab goes just right of the active one and becomes active, or at
	// the end with the setting off. Every tab past it moves along one, so a
	// label kept by position has to follow its tab, not stay in the slot.
	// Test ID: ErsV4PO
	#[test]
	fn a_new_tab_goes_next_to_the_current_one() {
		use super::{active_after_close, insert_tab};
		let mut list = vec![0, 1, 2, 3];
		assert_eq!(insert_tab(&mut list, 1, 9, true), 2);
		assert_eq!(list, [0, 1, 9, 2, 3]);
		assert_eq!(insert_tab(&mut list, 4, 8, true), 5, "from the last tab");
		assert_eq!(list, [0, 1, 9, 2, 3, 8]);
		assert_eq!(insert_tab(&mut list, 0, 7, false), 6, "setting off");
		assert_eq!(list, [0, 1, 9, 2, 3, 8, 7]);
		let mut one = vec![0];
		assert_eq!(insert_tab(&mut one, 0, 1, true), 1);

		// closing: of four left, as (active, closed, opener)
		assert_eq!(active_after_close(2, 0, 4, None), 1, "an earlier tab");
		assert_eq!(active_after_close(2, 3, 4, None), 2, "a later tab");
		assert_eq!(active_after_close(2, 2, 4, None), 2, "the one in its place");
		assert_eq!(active_after_close(4, 4, 4, None), 3, "the last");
		assert_eq!(active_after_close(2, 2, 4, Some(1)), 1, "back to its maker");
		assert_eq!(active_after_close(3, 2, 4, Some(1)), 2, "not the shown one");

		let mut text = TextCtx::new_cpu(1.0);
		let settings = label_settings("Work shell");
		let mut tabs: Vec<_> = (0..4).map(label_tab).collect();
		let mut labels = super::TabLabels::default();
		label_frame(&mut labels, &tabs, &settings, &mut text);
		let revision = labels.revision;
		assert_eq!(insert_tab(&mut tabs, 1, label_tab(9), true), 2);
		assert_eq!(
			label_frame(&mut labels, &tabs, &settings, &mut text),
			3,
			"the new tab and the two it moved along"
		);
		assert_ne!(labels.revision, revision);
		for (index, tab) in tabs.iter().enumerate() {
			assert_eq!(
				labels.forms(index),
				super::label_forms_from(tab, &settings),
				"tab {index}"
			);
		}
	}

	// The tip's lines are a table in the terminal font, measured line by line.
	// That used to happen on every frame the tip was up.
	// Test ID: Erlb9TL
	#[test]
	fn the_tab_tip_is_measured_once_per_set_of_lines() {
		let mut tip = super::TabTip {
			tab: 0,
			lines: vec!["Shell name: Bash".into(), "Open:       2m".into()],
			built: Instant::now(),
			width: None,
		};
		let measured = std::cell::Cell::new(0);
		let measure = |line: &str| {
			measured.set(measured.get() + 1);
			line.len() as f32
		};
		for _frame in 0..120 {
			assert_eq!(tip.text_w(1, measure), 16.0);
		}
		assert_eq!(
			measured.get(),
			2,
			"{} lines measured over 120 frames",
			measured.get()
		);
		// a new text context measures again
		tip.text_w(2, measure);
		assert_eq!(measured.get(), 4);
	}

	// Build a popup by hand, the way the geometry tests need it - no window, no
	// text context, just the numbers `row_top`/`item_at`/`step` are made of.
	// The width a title is FITTED to and the width the buffer is SHAPED at are
	// the same number by construction - shorten a path to a width the tab does not
	// then give it and the last component is clipped anyway, which is the whole
	// thing the shortening exists to avoid.
	// Test ID: EnbYSzw
	#[test]
	fn a_title_is_fitted_to_the_width_it_is_given() {
		for scale in [1.0, 1.5, 2.0] {
			for tab_w in [60.0, 140.0, 300.0] {
				let title_w = tab_title_w(tab_w, scale);
				let close = tab_close_box(0.0, tab_w, 0.0, 30.0, scale);
				assert!(title_w > 0.0, "no room at all for a title");
				assert!(
					title_w <= tab_w,
					"a title wider than its own tab: {title_w} in {tab_w}"
				);
				// and it stops short of the close button rather than running under it
				assert!(
					title_w <= close.x || tab_w < config::dip(40.0, scale),
					"title {title_w} runs under the close box at {}",
					close.x
				);
			}
		}
	}

	// A tab may only name the shell it can SEE. A pane resolves its own command
	// at spawn, so an unresolved one means nothing was switched on and the engine
	// chose for itself - and a guess from the list is exactly what had a pane
	// running PowerShell labelled Command Prompt.
	// Test ID: EnbYSzx
	#[test]
	fn a_tab_names_only_the_shell_it_can_see() {
		assert_eq!(tab_command_line(None), "");
		// An argument holding a space survives the round trip back into one line,
		// so the name lookup splits it the same way the launch did.
		let argv = vec!["C:/Program Files/x.exe".to_string(), "a b".to_string()];
		assert_eq!(
			tab_command_line(Some(&argv)),
			"\"C:/Program Files/x.exe\" \"a b\""
		);
	}

	// Test ID: Er2UiYT
	#[test]
	fn changing_tab_wraps_at_both_ends() {
		use super::tab_step;
		assert_eq!(tab_step(0, 3, true), 1);
		assert_eq!(tab_step(2, 3, true), 0);
		assert_eq!(tab_step(0, 3, false), 2);
		assert_eq!(tab_step(1, 3, false), 0);
		assert_eq!(tab_step(0, 1, true), 0);
		assert_eq!(tab_step(0, 1, false), 0);
	}

	// Test ID: Er2UiYU
	#[test]
	fn a_moved_tab_trades_places_with_its_neighbour_and_stays_active() {
		use super::move_tab;
		let mut tabs = ['a', 'b', 'c'];
		assert_eq!(move_tab(&mut tabs, 1, true), 2);
		assert_eq!(tabs, ['a', 'c', 'b']);
		assert_eq!(move_tab(&mut tabs, 2, true), 0, "past the end");
		assert_eq!(tabs, ['b', 'c', 'a']);
		assert_eq!(move_tab(&mut tabs, 0, false), 2, "past the start");
		assert_eq!(tabs, ['a', 'c', 'b']);
		let mut one = ['a'];
		assert_eq!(move_tab(&mut one, 0, true), 0);
	}
}
