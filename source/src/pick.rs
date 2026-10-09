// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The color picker box: the color model behind it and where every piece of it
//! sits. The dialog owns the input and the drawing (`settings_ui/picker.rs`);
//! this is the part that can be worked out and tested without a window.
//!
//! Units are DIP throughout, like the rest of the dialog.

use crate::pane::Rect;

/// Hue 0..1 (wrapping), saturation and brightness 0..1. This is what the box
/// holds, not the bytes: a drag down into black or left into gray leaves the
/// hue with nothing to be read back from, and deriving it per frame would send
/// the marker home the moment the color reached an edge.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hsv {
	pub h: f32,
	pub s: f32,
	pub v: f32,
}

/// The fully lit color at this hue, 0..1 per channel, in sRGB space. The square's
/// shader computes the same thing, so the two agree on what a hue looks like.
pub fn hue_rgb(h: f32) -> [f32; 3] {
	let k = h.rem_euclid(1.0) * 6.0;
	[
		((k - 3.0).abs() - 1.0).clamp(0.0, 1.0),
		(2.0 - (k - 2.0).abs()).clamp(0.0, 1.0),
		(2.0 - (k - 4.0).abs()).clamp(0.0, 1.0),
	]
}

pub fn to_rgb(color: Hsv) -> [u8; 3] {
	let hue = hue_rgb(color.h);
	let (s, v) = (color.s.clamp(0.0, 1.0), color.v.clamp(0.0, 1.0));
	let chan = |x: f32| ((1.0 - s + s * x) * v * 255.0).round().clamp(0.0, 255.0) as u8;
	[chan(hue[0]), chan(hue[1]), chan(hue[2])]
}

/// `keep` supplies what the bytes cannot say. A gray has no hue and black has no
/// saturation either, so typing 0 into Brightness would otherwise reset the two
/// sliders the user had just set.
pub fn from_rgb(c: [u8; 3], keep: Hsv) -> Hsv {
	let [r, g, b] = c.map(|v| f32::from(v) / 255.0);
	let max = r.max(g).max(b);
	let span = max - r.min(g).min(b);
	let h = if span <= 0.0 {
		keep.h
	} else if max == r {
		((g - b) / span).rem_euclid(6.0) / 6.0
	} else if max == g {
		((b - r) / span + 2.0) / 6.0
	} else {
		((r - g) / span + 4.0) / 6.0
	};
	Hsv {
		h,
		s: if max <= 0.0 { keep.s } else { span / max },
		v: max,
	}
}

/// The six value boxes down the right side, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
	Red,
	Green,
	Blue,
	Brightness,
	Saturation,
	Hex,
}

impl Field {
	pub const ALL: [Field; 6] = [
		Field::Red,
		Field::Green,
		Field::Blue,
		Field::Brightness,
		Field::Saturation,
		Field::Hex,
	];
	pub fn label(self) -> &'static str {
		match self {
			Field::Red => "Red %",
			Field::Green => "Green %",
			Field::Blue => "Blue %",
			Field::Brightness => "Brightness %",
			Field::Saturation => "Saturation %",
			Field::Hex => "Hex",
		}
	}
	fn channel(self) -> Option<usize> {
		match self {
			Field::Red => Some(0),
			Field::Green => Some(1),
			Field::Blue => Some(2),
			Field::Brightness | Field::Saturation | Field::Hex => None,
		}
	}
	/// What the box shows while nobody is typing in it.
	pub fn text(self, color: Hsv) -> String {
		let rgb = to_rgb(color);
		match self {
			Field::Hex => crate::config::format_hex(rgb),
			Field::Brightness => whole(color.v * 100.0),
			Field::Saturation => whole(color.s * 100.0),
			Field::Red | Field::Green | Field::Blue => {
				whole(f32::from(rgb[self.channel().unwrap_or(0)]) / 255.0 * 100.0)
			}
		}
	}
	/// A typed buffer read back into the model. None where it says nothing yet -
	/// a half-typed hex, or an empty box.
	pub fn apply(self, color: Hsv, buf: &str) -> Option<Hsv> {
		match self {
			Field::Hex => crate::config::parse_hex(buf).map(|rgb| from_rgb(rgb, color)),
			Field::Brightness => pct(buf).map(|p| Hsv { v: p, ..color }),
			Field::Saturation => pct(buf).map(|p| Hsv { s: p, ..color }),
			Field::Red | Field::Green | Field::Blue => {
				let p = pct(buf)?;
				let mut rgb = to_rgb(color);
				rgb[self.channel()?] = (p * 255.0).round().clamp(0.0, 255.0) as u8;
				Some(from_rgb(rgb, color))
			}
		}
	}
	/// One arrow press, the same hundredth-of-range step every number box in the
	/// dialog takes (a tenth with Shift).
	pub fn step(self, color: Hsv, dir: i32, shift: bool) -> Hsv {
		let by = if shift { 0.1 } else { 0.01 } * dir as f32;
		match self {
			Field::Brightness => Hsv {
				v: (color.v + by).clamp(0.0, 1.0),
				..color
			},
			Field::Saturation => Hsv {
				s: (color.s + by).clamp(0.0, 1.0),
				..color
			},
			Field::Hex => color,
			Field::Red | Field::Green | Field::Blue => {
				let Some(ch) = self.channel() else {
					return color;
				};
				let mut rgb = to_rgb(color);
				let now = f32::from(rgb[ch]) / 255.0;
				rgb[ch] = ((now + by).clamp(0.0, 1.0) * 255.0).round() as u8;
				from_rgb(rgb, color)
			}
		}
	}
	/// Characters the box takes. Hex is the Color row's own rule; the rest are
	/// whole percents.
	pub fn accepts(self, ch: char, len: usize, at_start: bool) -> bool {
		match self {
			Field::Hex => {
				(ch == '#' || ch.is_ascii_hexdigit()) && len < 7 && (ch != '#' || at_start)
			}
			Field::Red | Field::Green | Field::Blue | Field::Brightness | Field::Saturation => {
				ch.is_ascii_digit() && len < 3
			}
		}
	}
}

fn whole(v: f32) -> String {
	format!("{}", v.round().clamp(0.0, 100.0) as i32)
}
fn pct(buf: &str) -> Option<f32> {
	let v = buf.trim().parse::<f32>().ok()?;
	Some((v / 100.0).clamp(0.0, 1.0))
}

/// Where the keyboard is inside the box, in Tab order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
	Square,
	Hue,
	Field(Field),
	Cancel,
	Ok,
}

/// What the pointer took hold of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grab {
	Square,
	Hue,
}

#[derive(Debug)]
pub struct Picker {
	/// The Color row the box is editing.
	pub row: usize,
	/// What that row held when it opened, so Cancel has something to put back.
	pub start: [u8; 3],
	/// And whether that was automatic, which Cancel puts back too.
	pub start_automatic: bool,
	pub hsv: Hsv,
	pub focus: Focus,
	pub drag: Option<Grab>,
}

impl Picker {
	pub fn stops() -> Vec<Focus> {
		let mut stops = vec![Focus::Square, Focus::Hue];
		stops.extend(Field::ALL.map(Focus::Field));
		stops.push(Focus::Cancel);
		stops.push(Focus::Ok);
		stops
	}
	pub fn rgb(&self) -> [u8; 3] {
		to_rgb(self.hsv)
	}
}

/// The metrics the box is built from. All of them grow with the interface font,
/// so a large desktop font gets a proportionally larger box.
#[derive(Debug)]
pub struct Metrics {
	pub pad: f32,
	pub gap: f32,
	pub line_h: f32,
	pub field_h: f32,
	pub btn_w: f32,
	pub btn_h: f32,
	pub btn_gap: f32,
	pub strip_w: f32,
	pub label_w: f32,
	pub field_w: f32,
	pub min_side: f32,
}

#[derive(Debug)]
pub struct Geom {
	pub outer: Rect,
	pub title: Rect,
	pub square: Rect,
	pub strip: Rect,
	pub labels_x: f32,
	pub fields: [Rect; 6],
	pub cancel: Rect,
	pub ok: Rect,
}

/// Centered over the panel and sized to what it holds, the way the name box is.
/// The square is what gives on a narrow panel: it is the one piece with no text
/// in it, so shrinking it costs nothing that can be read.
pub fn geom(panel: Rect, m: &Metrics) -> Geom {
	let column = m.label_w + m.field_w;
	let fields_h = 6.0 * m.field_h + 5.0 * m.gap;
	let fixed = m.pad * 2.0 + m.gap * 2.0 + m.strip_w + column;
	let side = (panel.w - m.pad * 2.0 - fixed)
		.min(fields_h)
		.max(m.min_side);
	let w = fixed + side;
	let body_h = fields_h.max(side);
	let h = m.pad * 3.0 + m.line_h + m.gap + body_h + m.btn_h;
	let x = (panel.x + (panel.w - w) / 2.0).max(panel.x);
	let y = (panel.y + (panel.h - h) / 2.0).max(panel.y);
	let body_y = y + m.pad + m.line_h + m.gap;
	let square = Rect {
		x: x + m.pad,
		y: body_y,
		w: side,
		h: side,
	};
	let strip = Rect {
		x: square.x + side + m.gap,
		y: body_y,
		w: m.strip_w,
		h: side,
	};
	let labels_x = strip.x + m.strip_w + m.gap;
	let fields = std::array::from_fn(|k| Rect {
		x: labels_x + m.label_w,
		y: body_y + k as f32 * (m.field_h + m.gap),
		w: m.field_w,
		h: m.field_h,
	});
	let btn_y = y + h - m.pad - m.btn_h;
	let ok = Rect {
		x: x + w - m.pad - m.btn_w,
		y: btn_y,
		w: m.btn_w,
		h: m.btn_h,
	};
	Geom {
		outer: Rect { x, y, w, h },
		title: Rect {
			x: x + m.pad,
			y: y + m.pad,
			w: w - m.pad * 2.0,
			h: m.line_h,
		},
		square,
		strip,
		labels_x,
		fields,
		cancel: Rect {
			x: ok.x - m.btn_gap - m.btn_w,
			..ok
		},
		ok,
	}
}

impl Geom {
	pub fn field(&self, f: Field) -> Rect {
		self.fields[Field::ALL.iter().position(|&a| a == f).unwrap_or(0)]
	}
	/// Where the marker sits in the square, and where a press in it lands.
	pub fn marker(&self, color: Hsv) -> (f32, f32) {
		(
			self.square.x + color.s.clamp(0.0, 1.0) * self.square.w,
			self.square.y + (1.0 - color.v.clamp(0.0, 1.0)) * self.square.h,
		)
	}
	pub fn pick_square(&self, x: f32, y: f32, color: Hsv) -> Hsv {
		Hsv {
			s: ((x - self.square.x) / self.square.w.max(1.0)).clamp(0.0, 1.0),
			v: (1.0 - (y - self.square.y) / self.square.h.max(1.0)).clamp(0.0, 1.0),
			..color
		}
	}
	pub fn hue_y(&self, color: Hsv) -> f32 {
		self.strip.y + color.h.rem_euclid(1.0) * self.strip.h
	}
	pub fn pick_hue(&self, y: f32, color: Hsv) -> Hsv {
		Hsv {
			h: ((y - self.strip.y) / self.strip.h.max(1.0)).clamp(0.0, 1.0),
			..color
		}
	}
}

/// Black or white, whichever can be seen on `c`. The marker sits on a field that
/// runs from white to black to full color, so it cannot be one fixed ink.
pub fn ink_on(c: [u8; 3]) -> [u8; 3] {
	if crate::palette::to_oklab(c).0 > 0.6 {
		[0, 0, 0]
	} else {
		[255, 255, 255]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn hsv(h: f32, s: f32, v: f32) -> Hsv {
		Hsv { h, s, v }
	}
	fn metrics() -> Metrics {
		Metrics {
			pad: 18.0,
			gap: 12.0,
			line_h: 19.0,
			field_h: 27.0,
			btn_w: 76.0,
			btn_h: 31.0,
			btn_gap: 10.0,
			strip_w: 22.0,
			label_w: 96.0,
			field_w: 58.0,
			min_side: 80.0,
		}
	}
	fn panel() -> Rect {
		Rect {
			x: 40.0,
			y: 30.0,
			w: 560.0,
			h: 520.0,
		}
	}

	// The box holds HSV and the row holds bytes, so every drag and every
	// keystroke crosses this both ways. A color that shifted on the way through
	// would creep every time the box opened.
	// Test ID: EqSaSRc
	#[test]
	fn a_color_survives_the_trip_through_hue_saturation_and_brightness() {
		let seed = hsv(0.25, 0.5, 0.5);
		let mut checked = 0;
		for r in (0..=255).step_by(5) {
			for g in (0..=255).step_by(5) {
				for b in (0..=255).step_by(5) {
					let want = [r as u8, g as u8, b as u8];
					assert_eq!(to_rgb(from_rgb(want, seed)), want, "{want:?}");
					checked += 1;
				}
			}
		}
		assert!(checked > 100_000, "only {checked} colors");
	}

	// A gray says nothing about hue and black says nothing about saturation
	// either. Reading them off the bytes would send both markers home the moment
	// a drag reached an edge, or Brightness was typed to 0.
	// Test ID: EqSaSRd
	#[test]
	fn a_colorless_color_keeps_the_hue_and_saturation_it_had() {
		let held = hsv(0.6, 0.8, 0.4);
		let gray = from_rgb([128, 128, 128], held);
		assert_eq!(gray.h, held.h);
		assert_eq!(gray.s, 0.0, "a gray really has no saturation");
		let black = from_rgb([0, 0, 0], held);
		assert_eq!((black.h, black.s), (held.h, held.s));
		assert_eq!(black.v, 0.0);
	}

	// Test ID: EqSaSRe
	#[test]
	fn the_hue_ramp_hits_the_six_primaries() {
		let want = [
			(0.0, [255, 0, 0]),
			(1.0 / 6.0, [255, 255, 0]),
			(2.0 / 6.0, [0, 255, 0]),
			(3.0 / 6.0, [0, 255, 255]),
			(4.0 / 6.0, [0, 0, 255]),
			(5.0 / 6.0, [255, 0, 255]),
		];
		for (h, rgb) in want {
			assert_eq!(to_rgb(hsv(h, 1.0, 1.0)), rgb, "hue {h}");
		}
		// and it wraps rather than clamping at either end
		assert_eq!(to_rgb(hsv(1.0, 1.0, 1.0)), [255, 0, 0]);
		assert_eq!(to_rgb(hsv(-0.5, 1.0, 1.0)), [0, 255, 255]);
	}

	// Nothing here runs WGSL, so the square and the strip are checked by holding
	// the shader's own text against the model the markers are placed with. The
	// two painting different colors is the failure that has no other symptom:
	// the marker would sit where the color is not.
	// Test ID: EqSaSRf
	#[test]
	fn the_shader_paints_what_the_model_says() {
		let wgsl = crate::gfx::RECT_WGSL;
		for line in [
			// the square: x is saturation, y runs bright to dark
			"let s = in.local.x / in.size.x;",
			"let v = 1.0 - in.local.y / in.size.y;",
			"rgb = to_linear(mix(vec3<f32>(1.0), in.color.rgb, s) * v);",
			// the strip: hue down its length
			"rgb = to_linear(hue_rgb(in.local.y / in.size.y));",
			// the same ramp pick::hue_rgb computes
			"clamp(abs(k - 3.0) - 1.0, 0.0, 1.0),",
			"clamp(2.0 - abs(k - 2.0), 0.0, 1.0),",
			"clamp(2.0 - abs(k - 4.0), 0.0, 1.0),",
			"let k = fract(h) * 6.0;",
		] {
			assert!(wgsl.contains(line), "the shader lost `{line}`");
		}
		// and it encodes the result, or the whole box comes out washed out
		for part in ["c / 12.92", "1.055", "2.4", "0.04045"] {
			assert!(
				wgsl.contains(part),
				"the shader lost sRGB encoding `{part}`"
			);
		}
	}

	// A press reads back as the color drawn under it, and the marker goes back
	// where the press was. Without that a click moves the color slightly.
	// Test ID: EqSaSRg
	#[test]
	fn the_square_and_the_strip_read_back_where_the_marker_is() {
		let g = geom(panel(), &metrics());
		for (sat, val) in [(0.0, 0.0), (0.5, 0.25), (1.0, 1.0), (0.37, 0.81)] {
			let c = hsv(0.3, sat, val);
			let (mx, my) = g.marker(c);
			let back = g.pick_square(mx, my, c);
			assert!(
				(back.s - sat).abs() < 0.01,
				"saturation {sat} -> {}",
				back.s
			);
			assert!(
				(back.v - val).abs() < 0.01,
				"brightness {val} -> {}",
				back.v
			);
		}
		for hue in [0.0, 0.25, 0.5, 0.999] {
			let c = hsv(hue, 1.0, 1.0);
			let back = g.pick_hue(g.hue_y(c), c);
			assert!((back.h - hue).abs() < 0.01, "hue {hue} -> {}", back.h);
		}
		// a drag off the edge sticks to the edge rather than wrapping round
		let c = hsv(0.3, 0.5, 0.5);
		let far = g.pick_square(g.square.x - 500.0, g.square.y + 5000.0, c);
		assert_eq!((far.s, far.v), (0.0, 0.0));
	}

	// Test ID: EqSaSRh
	#[test]
	fn every_piece_of_the_box_sits_inside_it() {
		for panel_w in [280.0, 400.0, 560.0, 1400.0] {
			let p = Rect {
				w: panel_w,
				..panel()
			};
			let m = metrics();
			let g = geom(p, &m);
			let inside = |r: Rect, what: &str| {
				assert!(
					r.x >= g.outer.x
						&& r.y >= g.outer.y
						&& r.x + r.w <= g.outer.x + g.outer.w + 0.01
						&& r.y + r.h <= g.outer.y + g.outer.h + 0.01,
					"{what} escapes the box at panel width {panel_w}"
				);
			};
			inside(g.title, "the title");
			inside(g.square, "the square");
			inside(g.strip, "the strip");
			inside(g.cancel, "Cancel");
			inside(g.ok, "OK");
			for (k, r) in g.fields.iter().enumerate() {
				inside(*r, &format!("value box {k}"));
			}
			assert!(g.square.w >= m.min_side, "the square shrank past its floor");
			assert!(
				g.strip.x >= g.square.x + g.square.w,
				"strip over the square"
			);
			assert!(g.labels_x >= g.strip.x + g.strip.w, "labels over the strip");
			assert!(
				g.fields[0].x >= g.labels_x + m.label_w,
				"box over its label"
			);
			assert!(g.cancel.x + g.cancel.w < g.ok.x, "the buttons overlap");
			assert!(
				g.fields[5].y + g.fields[5].h <= g.ok.y,
				"the last box runs into the buttons"
			);
		}
	}

	// Test ID: EqSaSRi
	#[test]
	fn a_typed_percent_reaches_the_channel_it_names() {
		let start = from_rgb([10, 20, 30], hsv(0.0, 0.0, 0.0));
		let red = Field::Red.apply(start, "100").expect("100 is a percent");
		assert_eq!(to_rgb(red), [255, 20, 30]);
		let green = Field::Green.apply(start, "0").expect("0 is a percent");
		assert_eq!(to_rgb(green), [10, 0, 30]);
		assert_eq!(
			to_rgb(Field::Hex.apply(start, "#a0b1c2").expect("a hex value")),
			[0xa0, 0xb1, 0xc2]
		);
		// half-typed says nothing yet, rather than jumping the color to black
		assert!(Field::Hex.apply(start, "#a0b").is_none());
		assert!(Field::Red.apply(start, "").is_none());
		// and a box shows what the model holds
		assert_eq!(Field::Blue.text(from_rgb([0, 0, 255], start)), "100");
		assert_eq!(Field::Hex.text(from_rgb([0, 0, 255], start)), "#0000ff");
		assert_eq!(Field::Saturation.text(hsv(0.5, 0.42, 1.0)), "42");
	}

	// Test ID: EqSaSRj
	#[test]
	fn a_value_box_takes_only_what_it_can_mean() {
		assert!(Field::Red.accepts('7', 2, false));
		assert!(!Field::Red.accepts('7', 3, false), "a fourth digit");
		assert!(!Field::Red.accepts('a', 0, false), "not a digit");
		assert!(Field::Hex.accepts('#', 0, true));
		assert!(!Field::Hex.accepts('#', 3, false), "# only up front");
		assert!(Field::Hex.accepts('e', 6, false));
		assert!(!Field::Hex.accepts('e', 7, false), "an eighth character");
		assert!(!Field::Hex.accepts('z', 1, false));
	}

	// Arrows step a value box by the same hundredth of its range every other
	// number box in the dialog takes.
	// Test ID: EqSaSRk
	#[test]
	fn an_arrow_steps_a_value_box_by_a_hundredth() {
		let c = hsv(0.5, 0.5, 0.5);
		assert!((Field::Brightness.step(c, 1, false).v - 0.51).abs() < 1e-5);
		assert!((Field::Brightness.step(c, 1, true).v - 0.6).abs() < 1e-5);
		assert!((Field::Saturation.step(c, -1, false).s - 0.49).abs() < 1e-5);
		let red_up = Field::Red.step(c, 1, false);
		assert_eq!(to_rgb(red_up)[0], to_rgb(c)[0] + 3, "a percent of 255");
		// the hex box has no single number to step
		assert_eq!(to_rgb(Field::Hex.step(c, 1, false)), to_rgb(c));
		// and neither end runs past itself
		assert_eq!(Field::Brightness.step(hsv(0.0, 0.0, 1.0), 1, true).v, 1.0);
		assert_eq!(Field::Saturation.step(hsv(0.0, 0.0, 1.0), -1, true).s, 0.0);
	}
}
