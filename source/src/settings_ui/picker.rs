// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

impl SettingsDialog {
	// the color picker box

	// Every measurement the box is built from. The floors come from the
	// declarations; what each one grows with is the interface line height, so a
	// big desktop font gets a bigger box rather than a cramped one.
	fn pick_metrics(&self) -> pick::Metrics {
		let l = lay();
		pick::Metrics {
			pad: l.pad,
			gap: l.pick_gap,
			line_h: self.line_h,
			field_h: self.field_h(),
			btn_w: self.btn_w,
			btn_h: self.btn_h(),
			btn_gap: l.button_gap,
			strip_w: l.pick_strip,
			label_w: l.pick_label_width.max(self.line_h * 5.0),
			field_w: l.pick_field_width.max(self.line_h * 3.0),
			min_side: l.pick_min_square,
		}
	}

	fn pick_geom(&self) -> pick::Geom {
		pick::geom(self.rect, &self.pick_metrics())
	}

	// A chip opened. The box holds the color as HSV from here on, and the row
	// behind it follows every change - so the swatch, and the window under the
	// dialog, show what is being chosen while it is chosen.
	fn pick_open(&mut self, i: usize) {
		self.commit_edit();
		self.open = None;
		let start = self.get_col(self.specs[i].key);
		self.pick = Some(Picker {
			row: i,
			start,
			hsv: pick::from_rgb(
				start,
				pick::Hsv {
					h: 0.0,
					s: 0.0,
					v: 0.0,
				},
			),
			focus: pick::Focus::Square,
			drag: None,
		});
	}

	// Cancel puts back what the row held when the box opened. There is nothing
	// else to undo: the box writes through, so the row is the only place the
	// change ever reached.
	fn pick_cancel(&mut self) {
		if let Some(picker) = self.pick.take() {
			let key = self.specs[picker.row].key;
			self.set_col(key, picker.start);
		}
		self.pick_drop_edit();
	}

	fn pick_accept(&mut self) {
		self.pick = None;
		self.pick_drop_edit();
	}

	fn pick_drop_edit(&mut self) {
		self.emenu = None;
		if self
			.edit
			.as_ref()
			.is_some_and(|e| pick_field_of(e.row).is_some())
		{
			self.edit = None;
		}
	}

	// The one place the box's color changes. The row follows it, and an open
	// value box is refreshed and reselected the way a slider's number field is -
	// so stepping it with the arrows keeps working and a commit sees the number.
	fn pick_set(&mut self, hsv: pick::Hsv) {
		let Some(picker) = self.pick.as_mut() else {
			return;
		};
		picker.hsv = hsv;
		let (row, rgb) = (picker.row, pick::to_rgb(hsv));
		let key = self.specs[row].key;
		self.set_col(key, rgb);
	}

	// After the arrows moved a value box's number: rewrite what it shows and
	// reselect it, so stepping keeps working and a commit sees the new number.
	// Typing does NOT come through here - it owns the buffer.
	fn pick_refresh_field(&mut self) {
		let hsv = self.pick.as_ref().map(|p| p.hsv);
		let open = self.edit.as_ref().and_then(|e| pick_field_of(e.row));
		if let (Some(hsv), Some(field), Some(edit)) = (hsv, open, self.edit.as_mut()) {
			let buf = field.text(hsv);
			edit.cur = buf.len();
			edit.sel = (!buf.is_empty()).then_some(0);
			edit.buf = buf;
			edit.view_to = 0.0;
		}
	}

	// Focus arriving on a value box opens it with the value selected, the same
	// as anywhere else in the dialog; leaving one closes it.
	fn pick_focus_to(&mut self, to: pick::Focus) {
		self.commit_edit();
		self.pick_drop_edit();
		let Some(picker) = self.pick.as_mut() else {
			return;
		};
		picker.focus = to;
		let hsv = picker.hsv;
		if let pick::Focus::Field(f) = to {
			let mut edit = EditState::new(pick_field_row(f), f.text(hsv));
			edit.sel = (edit.cur > 0).then_some(0);
			self.edit = Some(edit);
		}
	}

	fn pick_focus_move(&mut self, forward: bool) {
		let Some(picker) = self.pick.as_ref() else {
			return;
		};
		let stops = Picker::stops();
		let at = stops.iter().position(|&s| s == picker.focus).unwrap_or(0);
		let step = if forward { 1 } else { stops.len() - 1 };
		self.pick_focus_to(stops[(at + step) % stops.len()]);
	}

	// Arrow keys. The square and the strip take them as direct adjustment, a
	// value box steps its number, and the two buttons pass them on as focus moves.
	// dir is +1 for Right/Down.
	fn pick_arrow(&mut self, dir: i32, vertical: bool) {
		let Some(picker) = self.pick.as_ref() else {
			return;
		};
		let (focus, hsv) = (picker.focus, picker.hsv);
		let by = if self.shift { 0.1 } else { 0.01 } * dir as f32;
		match focus {
			pick::Focus::Square if vertical => self.pick_set(pick::Hsv {
				v: (hsv.v - by).clamp(0.0, 1.0),
				..hsv
			}),
			pick::Focus::Square => self.pick_set(pick::Hsv {
				s: (hsv.s + by).clamp(0.0, 1.0),
				..hsv
			}),
			pick::Focus::Hue if vertical => self.pick_set(pick::Hsv {
				h: (hsv.h + by).rem_euclid(1.0),
				..hsv
			}),
			pick::Focus::Field(f) if vertical => {
				let stepped = f.step(hsv, -dir, self.shift);
				self.pick_set(stepped);
				self.pick_refresh_field();
			}
			_ => self.pick_focus_move(dir > 0),
		}
	}

	// Space or Enter on whatever the keyboard is on. The square and the strip
	// have nothing to activate, so they stay where they are.
	fn pick_activate(&mut self) {
		let Some(picker) = self.pick.as_ref() else {
			return;
		};
		match picker.focus {
			pick::Focus::Cancel => self.pick_cancel(),
			pick::Focus::Ok => self.pick_accept(),
			pick::Focus::Field(_) => self.char_input(' '),
			_ => {}
		}
	}

	// Every click while the box is up belongs to it: its own controls act, and
	// anything outside is swallowed rather than reaching the panel behind.
	fn pick_mouse_down(&mut self, x: f32, y: f32, measure: &mut impl FnMut(&str) -> f32) {
		if self.emenu.is_some() {
			return;
		}
		let g = self.pick_geom();
		for (part, r) in [(pick::Focus::Cancel, g.cancel), (pick::Focus::Ok, g.ok)] {
			if r.contains(x, y) {
				self.pick_focus_to(part);
				self.pick_activate();
				return;
			}
		}
		for f in pick::Field::ALL {
			let box_r = g.field(f);
			if box_r.contains(x, y) {
				self.pick_focus_to(pick::Focus::Field(f));
				self.field_click(pick_field_row(f), None, box_r, x, measure);
				return;
			}
		}
		let hsv = self.pick.as_ref().map(|p| p.hsv);
		let Some(hsv) = hsv else { return };
		if g.square.contains(x, y) {
			self.pick_focus_to(pick::Focus::Square);
			if let Some(picker) = self.pick.as_mut() {
				picker.drag = Some(pick::Grab::Square);
			}
			self.pick_set(g.pick_square(x, y, hsv));
		} else if g.strip.contains(x, y) {
			self.pick_focus_to(pick::Focus::Hue);
			if let Some(picker) = self.pick.as_mut() {
				picker.drag = Some(pick::Grab::Hue);
			}
			self.pick_set(g.pick_hue(y, hsv));
		}
	}

	// A drag that strays off the square or the strip keeps adjusting, clamped to
	// the edge - the same rule a slider drag follows.
	fn pick_drag_to(&mut self, x: f32, y: f32) {
		let Some(picker) = self.pick.as_ref() else {
			return;
		};
		let (Some(grab), hsv) = (picker.drag, picker.hsv) else {
			return;
		};
		let g = self.pick_geom();
		let hsv = match grab {
			pick::Grab::Square => g.pick_square(x, y, hsv),
			pick::Grab::Hue => g.pick_hue(y, hsv),
		};
		self.pick_set(hsv);
	}

	// The picker box. Drawn in the overlay pass, over a dimmed panel, the same
	// way the name box is.
	fn pick_overlay(
		&self,
		colors: &Dlg,
		measure: &mut impl FnMut(&str) -> f32,
	) -> (Vec<RectInstance>, Vec<TextItem>) {
		let mut rects = Vec::new();
		let mut texts = Vec::new();
		let Some(picker) = &self.pick else {
			return (rects, texts);
		};
		let mk = |text: String, x: f32, y: f32| TextItem::plain(text, x, y, colors.text);
		let row_text_y = |y: f32, h: f32| y + (h - self.line_h) / 2.0;
		// a disc: a rounded quad whose radius is its own half-width
		let disc = |x: f32, y: f32, d: f32, color: [u8; 3]| RectInstance {
			pos: [x - d / 2.0, y - d / 2.0],
			size: [d, d],
			color: config::srgb_f32(color),
			params: [QuadMode::Rounded.code(), d / 2.0],
		};
		rects.push(RectInstance {
			pos: [self.rect.x, self.rect.y],
			size: [self.rect.w, self.rect.h],
			color: [0.0, 0.0, 0.0, 0.45],
			..Default::default()
		});
		let g = self.pick_geom();
		rects.push(quad(
			g.outer.x,
			g.outer.y,
			g.outer.w,
			g.outer.h,
			colors.panel_bg,
		));
		border(&mut rects, g.outer, 1.0, colors.panel_border);
		texts.push(TextItem {
			clip: Some(g.title),
			..mk(
				self.specs[picker.row].label.to_string(),
				g.title.x,
				g.title.y,
			)
		});

		// The square mixes toward the hue in sRGB, so its `color` is the hue in
		// sRGB rather than the linear every other quad carries (see RectInstance).
		let hue = pick::hue_rgb(picker.hsv.h);
		rects.push(RectInstance {
			pos: [g.square.x, g.square.y],
			size: [g.square.w, g.square.h],
			color: [hue[0], hue[1], hue[2], 1.0],
			params: [QuadMode::PickSquare.code(), 0.0],
		});
		rects.push(RectInstance {
			pos: [g.strip.x, g.strip.y],
			size: [g.strip.w, g.strip.h],
			color: [0.0, 0.0, 0.0, 1.0],
			params: [QuadMode::HueStrip.code(), 0.0],
		});
		for (r, on) in [
			(g.square, picker.focus == pick::Focus::Square),
			(g.strip, picker.focus == pick::Focus::Hue),
		] {
			if on {
				border(&mut rects, r, 2.0, colors.focus_out);
			} else {
				border(&mut rects, r, 1.0, colors.panel_border);
			}
		}
		let rgb = picker.rgb();
		let ink = pick::ink_on(rgb);
		let (mx, my) = g.marker(picker.hsv);
		let d = lay().pick_marker;
		rects.push(disc(mx, my, d, ink));
		rects.push(disc(mx, my, d - 4.0, rgb));
		// The strip is full-chroma at every point, so its marker is plain black
		// and white rather than a theme color that could land on its own hue.
		let hy = g.hue_y(picker.hsv);
		rects.push(quad(
			g.strip.x - 2.0,
			hy - 2.5,
			g.strip.w + 4.0,
			5.0,
			[0, 0, 0],
		));
		rects.push(quad(
			g.strip.x - 1.0,
			hy - 1.5,
			g.strip.w + 2.0,
			3.0,
			[255, 255, 255],
		));

		for f in pick::Field::ALL {
			let box_r = g.field(f);
			texts.push(TextItem {
				clip: Some(Rect {
					x: g.labels_x,
					w: box_r.x - g.labels_x - 6.0,
					..box_r
				}),
				..mk(
					f.label().to_string(),
					g.labels_x,
					row_text_y(box_r.y, box_r.h),
				)
			});
			rects.push(quad(box_r.x, box_r.y, box_r.w, box_r.h, colors.field_bg));
			let open = picker.focus == pick::Focus::Field(f);
			border(
				&mut rects,
				box_r,
				1.0,
				if open {
					colors.focus_out
				} else {
					colors.panel_border
				},
			);
			let (txt, view) = match &self.edit {
				Some(edit) if pick_field_of(edit.row) == Some(f) => (edit.buf.clone(), edit.view),
				_ => (f.text(picker.hsv), 0.0),
			};
			if open {
				self.caret_quad(colors, &mut rects, box_r, measure);
			}
			texts.push(TextItem {
				clip: Some(box_r),
				..mk(
					txt,
					box_r.x + lay().field_pad - view,
					row_text_y(box_r.y, box_r.h),
				)
			});
		}

		for (part, r, caption) in [
			(pick::Focus::Cancel, g.cancel, "Cancel"),
			(pick::Focus::Ok, g.ok, "OK"),
		] {
			rects.push(quad(r.x, r.y, r.w, r.h, colors.btn_bg));
			let ring = picker.focus == part;
			let outline = if ring {
				colors.focus_out
			} else if part == pick::Focus::Ok {
				colors.btn_hl
			} else {
				colors.panel_border
			};
			border(&mut rects, r, if ring { 2.0 } else { 1.0 }, outline);
			let lx = r.x + (r.w - measure(caption)).max(0.0) / 2.0;
			texts.push(mk(caption.into(), lx, row_text_y(r.y, r.h)));
		}
		(rects, texts)
	}
}

#[cfg(test)]
mod tests {
	// the color picker

	fn color_row(d: &SettingsDialog) -> usize {
		d.specs
			.iter()
			.position(|s| s.key == Key::ColBg)
			.expect("a color row")
	}

	// Open the picker the way a click does, on a dialog showing that tab.
	fn mk_picker() -> (SettingsDialog, usize) {
		let mut d = mk_dialog(4000.0);
		let i = color_row(&d);
		d.tab = d.specs[i].tab;
		let mut m = |s: &str| s.chars().count() as f32;
		let chip = d.swatch(i);
		d.mouse_down_dip(chip.x + chip.w / 2.0, chip.y + chip.h / 2.0, &mut m);
		assert!(d.pick.is_some(), "the chip opens the picker");
		(d, i)
	}

	// Test ID: EqSaSRl
	#[test]
	fn a_chip_opens_the_picker_and_cancel_puts_the_color_back() {
		let (mut d, _) = mk_picker();
		let before = d.get_col(Key::ColBg);
		let g = d.pick_geom();
		// the square's top left corner is white at any hue
		let mut m = |s: &str| s.chars().count() as f32;
		d.mouse_down_dip(g.square.x, g.square.y, &mut m);
		assert_eq!(d.get_col(Key::ColBg), [255, 255, 255], "the row follows");
		d.mouse_up_dip(g.square.x, g.square.y);
		d.key_escape();
		assert!(d.pick.is_none(), "Escape closes the box");
		assert_eq!(d.get_col(Key::ColBg), before, "Cancel puts the color back");

		// and OK keeps it
		let (mut d, _) = mk_picker();
		let g = d.pick_geom();
		d.mouse_down_dip(g.square.x, g.square.y, &mut m);
		d.mouse_up_dip(g.square.x, g.square.y);
		assert_eq!(d.key_enter(), super::Action::None, "Enter is the box's OK");
		assert!(d.pick.is_none());
		assert_eq!(d.get_col(Key::ColBg), [255, 255, 255]);
	}

	// A drag across the square keeps adjusting, and the strip beside it moves
	// the hue without disturbing what the square set.
	// Test ID: EqSaSRm
	#[test]
	fn a_drag_across_the_square_moves_the_color_with_it() {
		let (mut d, _) = mk_picker();
		let g = d.pick_geom();
		let mut m = |s: &str| s.chars().count() as f32;
		d.mouse_down_dip(g.square.x + 2.0, g.square.y + 2.0, &mut m);
		let started = d.get_col(Key::ColBg);
		// off the bottom right corner: full saturation, no brightness
		d.mouse_move_dip(
			g.square.x + g.square.w + 90.0,
			g.square.y + g.square.h + 90.0,
			&mut m,
		);
		assert_ne!(d.get_col(Key::ColBg), started, "the drag went nowhere");
		assert_eq!(
			d.get_col(Key::ColBg),
			[0, 0, 0],
			"clamped to the far corner"
		);
		d.mouse_up_dip(0.0, 0.0);
		// the pointer moving on after the release no longer moves the color
		let settled = d.get_col(Key::ColBg);
		d.mouse_move_dip(g.square.x + 4.0, g.square.y + 4.0, &mut m);
		assert_eq!(d.get_col(Key::ColBg), settled, "the release did not take");

		// the hue strip keeps the saturation and brightness the square set
		let (mut d, _) = mk_picker();
		let g = d.pick_geom();
		d.mouse_down_dip(g.square.x + g.square.w - 1.0, g.square.y + 1.0, &mut m);
		d.mouse_up_dip(0.0, 0.0);
		let before = d.pick.as_ref().map(|p| (p.hsv.s, p.hsv.v)).unwrap();
		d.mouse_down_dip(g.strip.x + 2.0, g.strip.y + g.strip.h * 0.75, &mut m);
		let after = d.pick.as_ref().map(|p| (p.hsv.s, p.hsv.v)).unwrap();
		assert_eq!(before, after, "the strip disturbed the square's choice");
		assert!(
			(d.pick.as_ref().unwrap().hsv.h - 0.75).abs() < 0.02,
			"three quarters down the strip"
		);
	}

	// Every way in must reach the box rather than the panel under it. The panel
	// is still there, still has a focused row, and would take the keystroke.
	// Test ID: EqSaSRn
	#[test]
	fn the_picker_swallows_every_input_path() {
		let (mut d, i) = mk_picker();
		let mut m = |s: &str| s.chars().count() as f32;
		let before = d.get_col(Key::ColBg);
		let other = d
			.specs
			.iter()
			.position(|s| matches!(s.kind, Kind::Text))
			.expect("a text row");
		d.focus = Some(super::Focus::Row(other, 0));

		for c in ['o', 'a', 'c'] {
			assert_eq!(
				d.alt_key(c),
				super::Action::None,
				"Alt+{c} must not reach OK"
			);
		}
		d.char_input('f'); // the square holds focus, so this types nowhere
		d.wheel(0.0, -400.0);
		d.mouse_down_dip(d.rect.x + 2.0, d.rect.y + d.rect.h - 2.0, &mut m);
		d.mouse_right(d.rect.x + 4.0, d.rect.y + d.rect.h - 4.0, true, &mut m);
		d.select_all();

		assert!(d.pick.is_some(), "the box is still up");
		assert_eq!(d.scroll, 0.0, "the panel scrolled under the box");
		assert_eq!(d.get_col(Key::ColBg), before, "the color moved on its own");
		assert!(d.edit.is_none(), "an edit opened on a row behind the box");
		assert_eq!(
			d.hover_tip_dip(d.rect.x + 4.0, d.rect.y + 40.0, &mut chars7),
			None
		);
		// and the row it belongs to is still the one it opened on
		assert_eq!(d.pick.as_ref().map(|p| p.row), Some(i));
	}

	// The box's six value boxes borrow row indices no row can have, at the far
	// end from the ones the shells grid borrows. Either range reading the other's
	// would edit a shell that is not there.
	// Test ID: EqSaSRo
	#[test]
	fn a_value_box_is_never_mistaken_for_a_shells_field() {
		for f in super::pick::Field::ALL {
			let row = super::pick_field_row(f);
			assert_eq!(super::pick_field_of(row), Some(f));
			assert_eq!(super::shell_field_of(row), None, "{f:?} read as a shell");
		}
		// and the grid's own rows are not value boxes
		for entry in 0..4 {
			for command in [false, true] {
				let row = super::shell_field_row(entry, command);
				assert_eq!(
					super::pick_field_of(row),
					None,
					"shell {entry} read as a value box"
				);
			}
		}
		assert_eq!(super::pick_field_of(super::PROMPT_ROW), None);
		assert_eq!(super::pick_field_of(0), None);
	}

	// Tab walks the box, a value box opens with its number selected on the way
	// past, and what is typed into it reaches the row behind.
	// Test ID: EqSaSRp
	#[test]
	fn walking_the_box_opens_each_value_box_selected() {
		let (mut d, _) = mk_picker();
		assert_eq!(d.pick.as_ref().map(|p| p.focus), Some(pick::Focus::Square));
		assert!(d.edit.is_none(), "the square is not a field");
		d.key_tab();
		assert_eq!(d.pick.as_ref().map(|p| p.focus), Some(pick::Focus::Hue));
		assert!(d.edit.is_none(), "neither is the strip");
		for f in pick::Field::ALL {
			d.key_tab();
			assert_eq!(
				d.pick.as_ref().map(|p| p.focus),
				Some(pick::Focus::Field(f)),
				"Tab stopped somewhere else"
			);
			let want = f.text(d.pick.as_ref().unwrap().hsv);
			assert_eq!(
				d.selected_text().as_deref(),
				Some(want.as_str()),
				"{f:?} did not open with its value selected"
			);
		}
		d.key_tab();
		assert_eq!(d.pick.as_ref().map(|p| p.focus), Some(pick::Focus::Cancel));
		assert!(d.edit.is_none(), "the buttons are not fields");
		d.key_tab();
		assert_eq!(d.pick.as_ref().map(|p| p.focus), Some(pick::Focus::Ok));
		d.key_tab();
		assert_eq!(
			d.pick.as_ref().map(|p| p.focus),
			Some(pick::Focus::Square),
			"the walk wraps"
		);
	}

	// Test ID: EqSaSRq
	#[test]
	fn a_typed_value_box_reaches_the_row_behind_the_box() {
		let (mut d, _) = mk_picker();
		d.pick_focus_to(pick::Focus::Field(pick::Field::Hex));
		d.select_all();
		d.insert_str("#3366cc");
		assert_eq!(d.get_col(Key::ColBg), [0x33, 0x66, 0xcc]);
		// the other boxes now read off the same color
		let hsv = d.pick.as_ref().unwrap().hsv;
		assert_eq!(pick::Field::Red.text(hsv), "20");
		assert_eq!(pick::Field::Blue.text(hsv), "80");

		// a percent box, and the hex beside it follows
		d.pick_focus_to(pick::Focus::Field(pick::Field::Red));
		d.select_all();
		d.insert_str("100");
		assert_eq!(d.get_col(Key::ColBg), [0xff, 0x66, 0xcc]);
		assert_eq!(
			pick::Field::Hex.text(d.pick.as_ref().unwrap().hsv),
			"#ff66cc"
		);
		// junk never reaches the box, let alone the row: only the digit goes in,
		// and it replaces the selection the way the first keystroke should
		d.select_all();
		d.insert_str("zz9");
		assert_eq!(d.edit.as_ref().map(|e| e.buf.as_str()), Some("9"));
		assert_eq!(d.get_col(Key::ColBg), [23, 0x66, 0xcc], "9% of 255");
	}

	// Arrows adjust whatever the keyboard is on, so nothing in the box is
	// reachable only by pointer.
	// Test ID: EqSaSRr
	#[test]
	fn the_arrows_work_the_square_the_strip_and_the_boxes() {
		let (mut d, _) = mk_picker();
		d.pick_set(pick::Hsv {
			h: 0.5,
			s: 0.5,
			v: 0.5,
		});
		d.key_horizontal(1); // the square holds focus: Right raises saturation
		assert!((d.pick.as_ref().unwrap().hsv.s - 0.51).abs() < 1e-4);
		d.key_vertical(false); // Up raises brightness
		assert!((d.pick.as_ref().unwrap().hsv.v - 0.51).abs() < 1e-4);

		d.pick_focus_to(pick::Focus::Hue);
		let before = d.pick.as_ref().unwrap().hsv.h;
		d.key_vertical(true); // Down walks the strip forward
		assert!((d.pick.as_ref().unwrap().hsv.h - (before + 0.01)).abs() < 1e-4);
		let sv = d.pick.as_ref().map(|p| (p.hsv.s, p.hsv.v));
		d.key_horizontal(1); // Left/Right on the strip is a focus move
		assert_eq!(
			d.pick.as_ref().map(|p| p.focus),
			Some(pick::Focus::Field(pick::Field::Red))
		);
		assert_eq!(d.pick.as_ref().map(|p| (p.hsv.s, p.hsv.v)), sv);

		// a value box steps, and its own text is rewritten and reselected
		d.pick_focus_to(pick::Focus::Field(pick::Field::Brightness));
		let shown = d.selected_text().expect("open and selected");
		d.key_vertical(false);
		assert!((d.pick.as_ref().unwrap().hsv.v - 0.52).abs() < 1e-4);
		assert_ne!(
			d.selected_text().as_deref(),
			Some(shown.as_str()),
			"the box still shows the old number"
		);
		// and Left/Right in an open box moves the caret rather than the value
		let held = d.pick.as_ref().unwrap().hsv.v;
		d.key_horizontal(-1);
		assert_eq!(d.pick.as_ref().unwrap().hsv.v, held);
	}

	// The keyboard reaches the chip, and walking onto it neither opens a field
	// nor opens the picker by itself.
	// Test ID: EqSaSRs
	#[test]
	fn the_chip_is_a_focus_stop_of_its_own() {
		let mut d = mk_dialog(4000.0);
		let i = color_row(&d);
		d.tab = d.specs[i].tab;
		assert_eq!(d.parts_of(i), 2, "the chip and the hex box");
		assert_eq!(d.focus_ctl_rect(i, 0), d.swatch(i));
		assert_eq!(d.focus_ctl_rect(i, 1), d.hexbox(i));
		d.focus = Some(super::Focus::Row(i, 0));
		d.open_focused_field();
		assert!(d.edit.is_none(), "the chip is not a field");
		assert!(d.pick.is_none(), "and walking onto it opens nothing");
		d.key_space();
		assert!(d.pick.is_some(), "Space on the chip opens the picker");
		d.pick_cancel();
		d.focus = Some(super::Focus::Row(i, 0));
		assert_eq!(
			d.key_enter(),
			super::Action::None,
			"Enter is not the OK here"
		);
		assert!(d.pick.is_some(), "Enter on the chip opens the picker");
	}

	// The box draws. It used to be possible to leave a sentinel row index where
	// the row list gets read with it.
	// Test ID: EqSaSRt
	#[test]
	fn the_box_draws_without_reading_a_row_that_is_not_there() {
		let (mut d, _) = mk_picker();
		let mut m = |s: &str| s.chars().count() as f32;
		d.pick_focus_to(pick::Focus::Field(pick::Field::Saturation));
		if let Some(super::Focus::Row(r, _)) = d.focus {
			assert!(r < d.specs.len(), "focus row {r} is not a row");
		}
		let _ = d.rects_dip(d.line_h, &mut m);
		let (quads, texts) = d.overlay_dip(&mut m);
		assert!(d.overlay_open(), "the box needs the second pass");
		// the square and the strip are the two quads only this box draws
		assert_eq!(
			quads
				.iter()
				.filter(|q| q.mode() == QuadMode::PickSquare)
				.count(),
			1,
			"one saturation/brightness square"
		);
		assert_eq!(
			quads
				.iter()
				.filter(|q| q.mode() == QuadMode::HueStrip)
				.count(),
			1,
			"one hue strip"
		);
		for f in pick::Field::ALL {
			assert!(
				texts.iter().any(|t| t.text == f.label()),
				"{f:?} has no label"
			);
		}
		assert!(texts.iter().any(|t| t.text == "Cancel"));
		assert!(texts.iter().any(|t| t.text == "OK"));
	}
}
