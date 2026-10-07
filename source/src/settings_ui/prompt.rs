// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// A small box over the panel: name a new theme, rename one, or confirm a delete
// - of a theme, or of a shell. It is drawn in the overlay pass and takes every
// click and key while it is up, so the panel behind it can be left exactly as it
// was.
// Where the keyboard is inside the box. A confirmation has no field, so `Field`
// is unreachable there and the focus walk starts at Cancel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PromptFocus {
	Field,
	Cancel,
	Ok,
}

// What OK will do. The box itself is the same either way; only this says who
// asked for it and what to carry out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PromptJob {
	Theme(ThemeBtn),
	DropShell(usize), // index into `edited.shells`
	Notice,           // says something and asks nothing; OK alone
}

#[derive(Debug)]
struct Prompt {
	job: PromptJob,
	title: String,
	focus: PromptFocus,
	warn: Option<String>, // why OK is refusing (a blank or taken name)
}

impl Prompt {
	// Derived rather than stored: only a confirmation asks nothing, so a separate
	// flag could only ever disagree with the button that opened the box.
	fn has_field(&self) -> bool {
		matches!(
			self.job,
			PromptJob::Theme(ThemeBtn::SaveAs | ThemeBtn::Rename)
		)
	}
	// A notice has nothing to cancel, so it has no Cancel.
	fn parts(&self) -> &'static [PromptFocus] {
		if self.has_field() {
			&[PromptFocus::Field, PromptFocus::Cancel, PromptFocus::Ok]
		} else if self.job == PromptJob::Notice {
			&[PromptFocus::Ok]
		} else {
			&[PromptFocus::Cancel, PromptFocus::Ok]
		}
	}
}

impl SettingsDialog {
	// OK in the prompt box. A name that will not do keeps the box open and says why.
	fn prompt_accept(&mut self) {
		let Some(prompt) = self.prompt.as_ref() else {
			return;
		};
		let job = prompt.job;
		let typed = prompt
			.has_field()
			.then(|| self.edit.as_ref().map_or(String::new(), |e| e.buf.clone()));
		match (job, typed) {
			(PromptJob::Theme(which), Some(name)) => {
				if let Some(warn) = self.name_problem(which, &name) {
					if let Some(prompt) = self.prompt.as_mut() {
						prompt.warn = Some(warn);
					}
					return;
				}
				if which == ThemeBtn::Rename {
					self.rename_theme(&name);
				} else {
					self.save_theme_as(&name);
				}
			}
			(PromptJob::Theme(_), None) => self.delete_theme(),
			(PromptJob::DropShell(at), _) => self.shell_remove(at),
			(PromptJob::Notice, _) => {}
		}
		self.prompt_close();
	}

	fn prompt_close(&mut self) {
		self.prompt = None;
		self.emenu = None;
		if self.edit.as_ref().is_some_and(|e| e.row == PROMPT_ROW) {
			self.edit = None;
		}
	}

	// the prompt box

	// Centered over the panel, sized to what it holds. Two buttons, right-aligned,
	// the same way the dialog's own footer reads.
	fn prompt_rect(&self) -> Rect {
		let Some(prompt) = &self.prompt else {
			return Rect {
				x: 0.0,
				y: 0.0,
				w: 0.0,
				h: 0.0,
			};
		};
		let w = (self.rect.w - lay().pad * 6.0).max(240.0);
		let row = self.line_h + lay().row_pad;
		let mut h = lay().pad + row + lay().pad;
		if prompt.has_field() {
			h += row + lay().row_pad;
		}
		if prompt.warn.is_some() {
			h += row;
		}
		h += self.btn_h();
		Rect {
			x: self.rect.x + (self.rect.w - w) / 2.0,
			y: self.rect.y + (self.rect.h - h) / 2.0,
			w,
			h,
		}
	}

	fn prompt_field_rect(&self) -> Option<Rect> {
		let prompt = self.prompt.as_ref()?;
		if !prompt.has_field() {
			return None;
		}
		let r = self.prompt_rect();
		Some(Rect {
			x: r.x + lay().pad,
			y: r.y + lay().pad + self.line_h + lay().row_pad,
			w: r.w - lay().pad * 2.0,
			h: self.field_h(),
		})
	}

	fn prompt_btn_rect(&self, part: PromptFocus) -> Rect {
		let r = self.prompt_rect();
		let h = self.btn_h();
		let x_ok = r.x + r.w - lay().pad - self.btn_w;
		Rect {
			x: if part == PromptFocus::Ok {
				x_ok
			} else {
				x_ok - lay().button_gap - self.btn_w
			},
			y: r.y + r.h - lay().pad - h,
			w: self.btn_w,
			h,
		}
	}

	// Tab / arrows walk field -> Cancel -> OK, wrapping; a confirmation has no field.
	fn prompt_focus_move(&mut self, forward: bool) {
		let Some(prompt) = self.prompt.as_mut() else {
			return;
		};
		let stops = prompt.parts();
		let cur = stops.iter().position(|&s| s == prompt.focus).unwrap_or(0);
		let step = if forward { 1 } else { stops.len() - 1 };
		prompt.focus = stops[(cur + step) % stops.len()];
	}

	// Every click while the box is up belongs to it: its own controls act, and
	// anything outside is swallowed rather than reaching the panel behind.
	fn prompt_mouse_down(&mut self, x: f32, y: f32, measure: &mut impl FnMut(&str) -> f32) {
		if self.emenu.is_some() {
			return;
		}
		let parts = self.prompt.as_ref().map_or(&[][..], Prompt::parts);
		for &part in parts.iter().filter(|&&p| p != PromptFocus::Field) {
			if !self.prompt_btn_rect(part).contains(x, y) {
				continue;
			}
			if let Some(prompt) = self.prompt.as_mut() {
				prompt.focus = part;
			}
			match part {
				PromptFocus::Ok => self.prompt_accept(),
				_ => self.prompt_close(),
			}
			return;
		}
		if let Some(field) = self.prompt_field_rect() {
			if field.contains(x, y) {
				if let Some(prompt) = self.prompt.as_mut() {
					prompt.focus = PromptFocus::Field;
				}
				self.field_click(PROMPT_ROW, None, field, x, measure);
			}
		}
	}

	// The name / confirm box, drawn over everything the panel just drew. Its own
	// field caret rides the same edit state the rows use, so it blinks and eases
	// the same way.
	fn prompt_overlay(
		&self,
		colors: &Dlg,
		measure: &mut impl FnMut(&str) -> f32,
	) -> (Vec<RectInstance>, Vec<TextItem>) {
		let mut rects = Vec::new();
		let mut texts = Vec::new();
		let Some(prompt) = &self.prompt else {
			return (rects, texts);
		};
		let mk = |text: String, x: f32, y: f32| TextItem::plain(text, x, y, colors.text);
		let row_text_y = |y: f32, h: f32| y + (h - self.line_h) / 2.0;
		// dim the panel behind, so it is plain that it is not taking input
		rects.push(RectInstance {
			pos: [self.rect.x, self.rect.y],
			size: [self.rect.w, self.rect.h],
			color: [0.0, 0.0, 0.0, 0.45],
			..Default::default()
		});
		let box_r = self.prompt_rect();
		rects.push(quad(box_r.x, box_r.y, box_r.w, box_r.h, colors.panel_bg));
		border(&mut rects, box_r, 1.0, colors.panel_border);
		// a long message is cut at the box's edge rather than drawn past it
		texts.push(TextItem {
			clip: Some(box_r),
			..mk(
				prompt.title.clone(),
				box_r.x + lay().pad,
				box_r.y + lay().pad + (self.line_h + lay().row_pad - self.line_h) / 2.0,
			)
		});
		if let Some(field) = self.prompt_field_rect() {
			rects.push(quad(field.x, field.y, field.w, field.h, colors.field_bg));
			if prompt.focus == PromptFocus::Field {
				border(&mut rects, field, 1.0, colors.focus_out);
			} else {
				border(&mut rects, field, 1.0, colors.panel_border);
			}
			self.caret_quad(colors, &mut rects, field, measure);
			let view = self.edit.as_ref().map_or(0.0, |e| e.view);
			texts.push(TextItem {
				clip: Some(field),
				..mk(
					self.edit.as_ref().map_or(String::new(), |e| e.buf.clone()),
					field.x + lay().field_pad - view,
					row_text_y(field.y, field.h),
				)
			});
		}
		if let Some(warn) = &prompt.warn {
			let y = self.prompt_btn_rect(PromptFocus::Ok).y - (self.line_h + lay().row_pad);
			texts.push(TextItem {
				color: colors.btn_hl,
				clip: Some(box_r),
				..mk(warn.clone(), box_r.x + lay().pad, y)
			});
		}
		for (part, caption) in [(PromptFocus::Cancel, "Cancel"), (PromptFocus::Ok, "OK")] {
			if !prompt.parts().contains(&part) {
				continue;
			}
			let r = self.prompt_btn_rect(part);
			rects.push(quad(r.x, r.y, r.w, r.h, colors.btn_bg));
			let ring = prompt.focus == part;
			// OK is the default here too, so it keeps the highlight outline when
			// the keyboard is elsewhere
			let outline = if ring {
				colors.focus_out
			} else if part == PromptFocus::Ok {
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
	// The box is modal by gate, and the gate is a list every input path has to be
	// on. These four were missed once: the accelerators applied and closed the
	// dialog through the box, and typing edited the row sitting behind it.
	// Test ID: Em430PC
	#[test]
	fn the_prompt_swallows_every_input_path() {
		let mut m = |s: &str| s.chars().count() as f32;
		for which in [super::ThemeBtn::SaveAs, super::ThemeBtn::Delete] {
			let mut d = on_theme("Matrix");
			d.save_theme_as("Mine"); // Rename and Delete need a theme of the user's own
			d.reverted.clear();
			d.focus = Some(super::Focus::Row(
				d.specs.iter().position(|s| s.key == Key::ColFg).unwrap(),
				0,
			));
			let before = d.get_col(Key::ColFg);
			d.theme_action(which);
			assert!(d.prompt.is_some(), "{which:?} opens the box");

			for c in ['o', 'a', 'c'] {
				assert_eq!(
					d.alt_key(c),
					super::Action::None,
					"Alt+{c} must not reach OK"
				);
			}
			d.char_input('f');
			d.select_all();
			d.mouse_right(d.rect.x + 4.0, d.rect.y + d.rect.h - 4.0, true, &mut m);

			assert!(d.prompt.is_some(), "the box is still up");
			assert_eq!(d.get_col(Key::ColFg), before, "the row behind is untouched");
			assert!(
				d.edit.as_ref().is_none_or(|e| e.row == super::PROMPT_ROW),
				"no edit opened on a panel row"
			);
		}
	}

	// The prompt is not a row. Clicking its field used to put the prompt's own
	// sentinel index into the focus ring, and the next frame read the row list
	// with it.
	// Test ID: EpHNgxU
	#[test]
	fn clicking_the_prompt_field_leaves_the_focus_ring_on_a_real_row() {
		let mut m = |s: &str| s.chars().count() as f32;
		let mut d = on_theme("Matrix");
		let row = d.specs.iter().position(|s| s.key == Key::ColFg).unwrap();
		d.focus = Some(super::Focus::Row(row, 0));
		d.theme_action(super::ThemeBtn::SaveAs);
		let field = d.prompt_field_rect().expect("the box has a name field");
		d.prompt_mouse_down(field.x + 4.0, field.y + field.h / 2.0, &mut m);

		if let Some(super::Focus::Row(r, _)) = d.focus {
			assert!(r < d.specs.len(), "focus row {r} is not a row");
		}
		// the frame that used to abort
		let _ = d.rects_dip(d.line_h, &mut m);
		assert!(d.prompt.is_some(), "the box is still up");
	}

	// The prompt box takes the keyboard while it is up, and Esc leaves the theme
	// exactly as it was.
	// Test ID: Em3lZEz
	#[test]
	fn the_prompt_box_owns_the_keyboard_until_it_closes() {
		let mut d = on_theme("Matrix");
		d.set_col(Key::ColFg, [7, 7, 7]);
		d.theme_action(super::ThemeBtn::SaveAs);
		assert!(d.prompt.is_some() && d.edit.is_some());

		for c in "My Theme".chars() {
			d.char_input(c);
		}
		// Esc closes the box, not the dialog, and saves nothing
		assert_eq!(d.key_escape(), super::Action::None);
		assert!(d.prompt.is_none() && d.edit.is_none());
		assert!(d.edited.user_themes.is_empty());

		// again, this time through OK
		d.theme_action(super::ThemeBtn::SaveAs);
		for c in "My Theme".chars() {
			d.char_input(c);
		}
		assert_eq!(d.key_enter(), super::Action::None);
		assert!(d.prompt.is_none());
		assert_eq!(d.edited.theme, "My Theme");
		assert_eq!(
			crate::theme::find_user(&d.edited.user_themes, "My Theme")
				.unwrap()
				.dark
				.fg,
			[7, 7, 7]
		);
	}

	// A name OK cannot take keeps the box open and says why, instead of closing
	// and quietly doing nothing.
	// Test ID: Em3lZF0
	#[test]
	fn a_name_it_cannot_take_keeps_the_box_open() {
		let mut d = on_theme("Matrix");
		d.save_theme_as("Mine");
		d.theme_action(super::ThemeBtn::Rename);
		d.select_all();
		for c in "   ".chars() {
			d.char_input(c);
		}
		d.prompt_accept();
		assert!(d.prompt.is_some(), "still asking");
		assert!(d.prompt.as_ref().unwrap().warn.is_some(), "and saying why");
		// typing again clears the complaint
		d.char_input('x');
		assert!(d.prompt.as_ref().unwrap().warn.is_none());
	}
}
