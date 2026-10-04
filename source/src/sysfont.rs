// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Best-effort detection of the OS's default monospace/fixed-pitch font (family
//! and point size). Each platform uses its native mechanism; everything is
//! optional and falls back gracefully. Detected once and cached, since it's read
//! a couple of times at startup (config size + font-family resolution).

use std::sync::OnceLock;

#[derive(Debug, Default, Clone)]
pub struct Monospace {
	pub family: Option<String>, // e.g. "Monaspace Argon" (style/size stripped)
	pub size_pt: Option<f32>,   // points
}

/// Cached system monospace description.
pub fn monospace() -> &'static Monospace {
	static M: OnceLock<Monospace> = OnceLock::new();
	M.get_or_init(platform::monospace)
}

/// How a platform's font points relate to logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointUnit {
	/// A Mac lays out in points, so a Mac point already is the logical pixel,
	/// and the backing scale factor takes it the rest of the way.
	MacLogical,
	/// 1/72 inch, against the 96-DPI reference logical pixels are measured on.
	Typographic,
}

pub fn px_from_pt_for(pt: f32, unit: PointUnit) -> f32 {
	match unit {
		PointUnit::MacLogical => pt,
		PointUnit::Typographic => pt * 96.0 / 72.0,
	}
}

/// A size the OS reported in points, in logical pixels on this platform.
pub fn px_from_pt(pt: f32) -> f32 {
	let unit = if cfg!(target_os = "macos") {
		PointUnit::MacLogical
	} else {
		PointUnit::Typographic
	};
	px_from_pt_for(pt, unit)
}

#[derive(Debug, Default, Clone)]
pub struct UiFont {
	pub family: Option<String>, // desktop interface font family, e.g. "GentiumAlt"
	pub size_pt: Option<f32>,   // points
	pub bold: bool,             // desktop asks for a bold UI face
	pub italic: bool,
}

/// Cached desktop *interface* (UI/chrome) font: whatever the user picked for
/// menus and dialogs - serif or not. This is the first choice for chrome text;
/// `sans_serif()` below is only the fallback when no desktop setting is readable.
pub fn interface() -> &'static UiFont {
	static U: OnceLock<UiFont> = OnceLock::new();
	U.get_or_init(platform::interface)
}

/// Cached OS sans-serif (proportional UI) family, best-effort. Fallback for
/// chrome when no desktop interface font is readable - the generic
/// `Family::SansSerif` is unreliable (fontdb defaults it to "Arial" and falls
/// through to a serif when that's absent).
pub fn sans_serif() -> Option<&'static str> {
	static S: OnceLock<Option<String>> = OnceLock::new();
	S.get_or_init(sans_serif_detect).as_deref()
}

#[cfg(target_os = "linux")]
fn sans_serif_detect() -> Option<String> {
	// fontconfig's `sans-serif` alias resolves to a real sans face regardless of
	// the user's (possibly serif) document font.
	let out = std::process::Command::new("fc-match")
		.args(["--format=%{family}", "sans-serif"])
		.output()
		.ok()?;
	out.status.success().then_some(())?;
	let family_list = String::from_utf8(out.stdout).ok()?;
	let family = family_list.trim().split(',').next().unwrap_or("").trim(); // may be a list
	(!family.is_empty()).then(|| family.to_string())
}

#[cfg(not(target_os = "linux"))]
fn sans_serif_detect() -> Option<String> {
	None // other platforms fall back to the curated list in text::resolve_ui_family
}

#[cfg(target_os = "linux")]
mod platform {
	use super::Monospace;
	use std::process::Command;

	pub fn monospace() -> Monospace {
		monospace_from(
			&desktop(),
			|| gsettings_desc("monospace-font-name"),
			|| xfconf_desc("/Gtk/MonospaceFontName"),
		)
	}

	fn monospace_from(
		desktop: &str,
		gnome: impl FnOnce() -> Option<String>,
		xfce: impl FnOnce() -> Option<String>,
	) -> Monospace {
		if let Some(parsed) = desktop_font(desktop, gnome, xfce) {
			return Monospace {
				family: parsed.family,
				size_pt: parsed.size_pt,
			};
		}
		// No desktop setting: fontconfig gives a size, no specific family.
		Monospace {
			family: None,
			size_pt: fontconfig_size(),
		}
	}

	// The desktop interface (UI) font. GNOME/MATE/Cinnamon expose it through
	// gsettings; Xfce through xfconf. Either may be a serif - that's the point:
	// chrome follows whatever the user picked, not a sans assumption.
	pub fn interface() -> super::UiFont {
		interface_from(
			&desktop(),
			|| gsettings_desc("font-name"),
			|| xfconf_desc("/Gtk/FontName"),
		)
	}

	fn interface_from(
		desktop: &str,
		gnome: impl FnOnce() -> Option<String>,
		xfce: impl FnOnce() -> Option<String>,
	) -> super::UiFont {
		desktop_font(desktop, gnome, xfce).unwrap_or_default()
	}

	// The desktop's own store answers first. gsettings answers wherever GNOME's
	// schemas are installed, an Xfce box included, and a key nobody set comes
	// back as the schema default - so on Xfce it may only fill in for a missing
	// xfconf answer, or the chrome follows Cantarell 11 whatever was picked.
	fn desktop_font(
		desktop: &str,
		gnome: impl FnOnce() -> Option<String>,
		xfce: impl FnOnce() -> Option<String>,
	) -> Option<super::UiFont> {
		let desc = if is_xfce(desktop) {
			xfce().or_else(gnome)
		} else {
			gnome().or_else(xfce)
		};
		desc.map(|desc| parse_pango(&desc))
			.filter(|parsed| parsed.family.is_some() || parsed.size_pt.is_some())
	}

	// XDG_CURRENT_DESKTOP is a colon list ("ubuntu:GNOME", "XFCE"); an older
	// session may set only DESKTOP_SESSION ("xfce").
	fn desktop() -> String {
		std::env::var("XDG_CURRENT_DESKTOP")
			.ok()
			.filter(|d| !d.is_empty())
			.or_else(|| std::env::var("DESKTOP_SESSION").ok())
			.unwrap_or_default()
	}

	fn is_xfce(desktop: &str) -> bool {
		desktop
			.split(':')
			.any(|part| part.eq_ignore_ascii_case("xfce"))
	}

	fn gsettings_desc(key: &str) -> Option<String> {
		let out = Command::new("gsettings")
			.args(["get", "org.gnome.desktop.interface", key])
			.output()
			.ok()?;
		out.status.success().then_some(())?;
		String::from_utf8(out.stdout).ok()
	}

	fn xfconf_desc(property: &str) -> Option<String> {
		let out = Command::new("xfconf-query")
			.args(["-c", "xsettings", "-p", property])
			.output()
			.ok()?;
		out.status.success().then_some(())?;
		String::from_utf8(out.stdout).ok()
	}

	fn fontconfig_size() -> Option<f32> {
		let out = Command::new("fc-match")
			.args(["--format=%{size}", "monospace"])
			.output()
			.ok()?;
		out.status.success().then_some(())?;
		String::from_utf8(out.stdout).ok()?.trim().parse().ok()
	}

	// Parse a Pango font description "Family Style... Size", e.g.
	// "GentiumAlt Bold 13" -> family "GentiumAlt", size 13, bold. The style
	// words are captured (bold/italic), not just stripped, so chrome can honor
	// the desktop's weight/slant.
	fn parse_pango(desc: &str) -> super::UiFont {
		let desc = desc.trim().trim_matches(['\'', '"']);
		let mut tokens: Vec<&str> = desc.split_whitespace().collect();

		let mut size_pt = None;
		if let Some(last) = tokens.last() {
			if let Ok(size) = last.parse::<f32>() {
				size_pt = Some(size);
				tokens.pop();
			}
		}
		// Peel trailing weight/style/stretch words so only the family remains.
		let (mut bold, mut italic) = (false, false);
		while let Some(&last) = tokens.last() {
			if !is_style_word(last) {
				break;
			}
			tokens.pop();
			let word = last.to_ascii_lowercase();
			match word.as_str() {
				"bold" | "semibold" | "semi-bold" | "demibold" | "demi-bold" | "extrabold"
				| "extra-bold" | "ultrabold" | "ultra-bold" | "black" | "heavy" => bold = true,
				"italic" | "oblique" => italic = true,
				_ => {}
			}
		}
		let family = (!tokens.is_empty()).then(|| tokens.join(" "));
		super::UiFont {
			family,
			size_pt,
			bold,
			italic,
		}
	}

	fn is_style_word(word: &str) -> bool {
		const STYLES: &[&str] = &[
			"thin",
			"hairline",
			"extralight",
			"extra-light",
			"ultralight",
			"ultra-light",
			"light",
			"semilight",
			"semi-light",
			"demilight",
			"demi-light",
			"book",
			"regular",
			"normal",
			"medium",
			"semibold",
			"semi-bold",
			"demibold",
			"demi-bold",
			"bold",
			"extrabold",
			"extra-bold",
			"ultrabold",
			"ultra-bold",
			"black",
			"heavy",
			"italic",
			"oblique",
			"condensed",
			"semicondensed",
			"semi-condensed",
			"expanded",
			"semiexpanded",
			"semi-expanded",
			"roman",
		];
		STYLES.iter().any(|style| style.eq_ignore_ascii_case(word))
	}

	#[cfg(test)]
	mod tests {
		use super::{interface_from, is_xfce, monospace_from};

		// gsettings answers on an Xfce box too, with GNOME's defaults for keys
		// nobody set, so the desktop decides which store is asked first.
		// Test ID: Eq9PYAK
		#[test]
		fn the_desktop_decides_which_font_store_answers_first() {
			let gnome = || Some("'Cantarell 11'".to_string());
			let xfce = || Some("GentiumAlt Bold 13".to_string());
			let ui = interface_from("XFCE", gnome, xfce);
			assert_eq!(
				(ui.family.as_deref(), ui.size_pt, ui.bold),
				(Some("GentiumAlt"), Some(13.0), true)
			);
			let ui = interface_from("ubuntu:GNOME", gnome, xfce);
			assert_eq!(
				(ui.family.as_deref(), ui.size_pt),
				(Some("Cantarell"), Some(11.0))
			);

			let gnome = || Some("'Monospace 11'".to_string());
			let xfce = || Some("Monaspace Argon 13".to_string());
			let mono = monospace_from("XFCE", gnome, xfce);
			assert_eq!(
				(mono.family.as_deref(), mono.size_pt),
				(Some("Monaspace Argon"), Some(13.0))
			);
			let mono = monospace_from("GNOME", gnome, xfce);
			assert_eq!(
				(mono.family.as_deref(), mono.size_pt),
				(Some("Monospace"), Some(11.0))
			);

			// The other store still fills in when the desktop's own has nothing.
			let ui = interface_from("XFCE", || Some("'Cantarell 11'".to_string()), || None);
			assert_eq!(ui.family.as_deref(), Some("Cantarell"));
			let mono = monospace_from("GNOME", || None, || Some("Monaspace Argon 13".to_string()));
			assert_eq!(mono.family.as_deref(), Some("Monaspace Argon"));

			assert!(is_xfce("xfce") && is_xfce("XFCE:GNOME"));
			assert!(!is_xfce("X-Cinnamon") && !is_xfce(""));
		}
	}
}

#[cfg(target_os = "macos")]
mod platform {
	use super::Monospace;
	use objc2_app_kit::NSFont;
	use std::process::Command;

	pub fn monospace() -> Monospace {
		Monospace {
			family: family(),
			size_pt: fixed_pitch_size(),
		}
	}

	fn defaults_global(key: &str) -> Option<String> {
		let out = Command::new("defaults")
			.args(["read", "-g", key])
			.output()
			.ok()?;
		out.status.success().then_some(())?;
		Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
	}

	// AppKit answers with its default when nobody has set NSFixedPitchFontSize,
	// which the defaults key alone does not.
	fn fixed_pitch_size() -> Option<f32> {
		objc2::rc::autoreleasepool(|_| {
			let font = NSFont::userFixedPitchFontOfSize(0.0)?;
			let size = font.pointSize() as f32;
			(size > 0.0).then_some(size)
		})
	}

	// Only a family somebody picked. AppKit would always name one (Menlo), and
	// following that would drop the font_family stack for everyone.
	fn family() -> Option<String> {
		// Stored as a PostScript name, e.g. "Menlo-Regular"; take the family part.
		let postscript_name = defaults_global("NSFixedPitchFont")?;
		let family = postscript_name
			.split('-')
			.next()
			.unwrap_or(&postscript_name)
			.trim();
		(!family.is_empty()).then(|| family.to_string())
	}

	// macOS has no user-set UI font, and the actual one (San Francisco) hides
	// behind a private name fontdb can't query. Report AppKit's system size and
	// let the family fall back (curated list has Helvetica Neue).
	pub fn interface() -> super::UiFont {
		let size = NSFont::systemFontSize() as f32;
		super::UiFont {
			family: None,
			size_pt: Some(if size > 0.0 { size } else { 13.0 }),
			bold: false,
			italic: false,
		}
	}
}

#[cfg(windows)]
mod platform {
	use super::Monospace;
	use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, LOGPIXELSY, ReleaseDC};
	use windows_sys::Win32::UI::WindowsAndMessaging::{
		NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SystemParametersInfoW,
	};

	// Windows has no dedicated monospace setting; report the message-box font
	// size (the conventional system size). No reliable system *monospace* family,
	// so leave family None - the resolver then walks the user's font_family stack
	// and config::DEFAULT_FONT_STACK (never the bare Family::Monospace db lottery,
	// whose winner can lack a bold face).
	pub fn monospace() -> Monospace {
		Monospace {
			family: None,
			size_pt: message_font_pt(),
		}
	}

	// The menu font is what native chrome (menus/dialog labels) uses; family,
	// size, weight and slant all honor the user's "Menu" font setting.
	pub fn interface() -> super::UiFont {
		unsafe {
			let mut ncm: NONCLIENTMETRICSW = core::mem::zeroed();
			ncm.cbSize = core::mem::size_of::<NONCLIENTMETRICSW>() as u32;
			let ok = SystemParametersInfoW(
				SPI_GETNONCLIENTMETRICS,
				ncm.cbSize,
				core::ptr::addr_of_mut!(ncm).cast(),
				0,
			);
			if ok == 0 {
				return super::UiFont::default();
			}
			let lf = &ncm.lfMenuFont;
			let end = lf.lfFaceName.iter().position(|&c| c == 0).unwrap_or(32);
			let family = String::from_utf16(&lf.lfFaceName[..end])
				.ok()
				.map(|f| f.trim().to_string())
				.filter(|f| !f.is_empty());
			let size_pt = (lf.lfHeight != 0).then(|| {
				let dc = GetDC(core::ptr::null_mut());
				let dpi = if dc.is_null() {
					96
				} else {
					GetDeviceCaps(dc, LOGPIXELSY as i32)
				};
				if !dc.is_null() {
					ReleaseDC(core::ptr::null_mut(), dc);
				}
				let dpi = if dpi <= 0 { 96 } else { dpi };
				lf.lfHeight.unsigned_abs() as f32 * 72.0 / dpi as f32
			});
			super::UiFont {
				family,
				size_pt,
				bold: lf.lfWeight >= 600,
				italic: lf.lfItalic != 0,
			}
		}
	}

	fn message_font_pt() -> Option<f32> {
		unsafe {
			let mut ncm: NONCLIENTMETRICSW = core::mem::zeroed();
			ncm.cbSize = core::mem::size_of::<NONCLIENTMETRICSW>() as u32;
			let ok = SystemParametersInfoW(
				SPI_GETNONCLIENTMETRICS,
				ncm.cbSize,
				core::ptr::addr_of_mut!(ncm).cast(),
				0,
			);
			if ok == 0 {
				return None;
			}
			let h = ncm.lfMessageFont.lfHeight;
			if h == 0 {
				return None;
			}
			let dc = GetDC(core::ptr::null_mut());
			let dpi = if dc.is_null() {
				96
			} else {
				GetDeviceCaps(dc, LOGPIXELSY as i32)
			};
			if !dc.is_null() {
				ReleaseDC(core::ptr::null_mut(), dc);
			}
			let dpi = if dpi <= 0 { 96 } else { dpi };
			Some(h.unsigned_abs() as f32 * 72.0 / dpi as f32)
		}
	}
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
	use super::{Monospace, UiFont};
	pub fn monospace() -> Monospace {
		Monospace::default()
	}
	pub fn interface() -> UiFont {
		UiFont::default()
	}
}

#[cfg(test)]
mod tests {
	use super::{PointUnit, px_from_pt_for};

	// macOS reports 13 pt for the interface and about 11 pt for fixed pitch, and
	// both are already logical pixels there. Taking them as 1/72 inch made every
	// font on a Mac a third too big.
	// Test ID: ErUj5E3
	#[test]
	fn a_mac_point_is_already_a_logical_pixel() {
		assert_eq!(px_from_pt_for(13.0, PointUnit::MacLogical), 13.0);
		assert_eq!(px_from_pt_for(11.0, PointUnit::MacLogical), 11.0);
		assert_eq!(px_from_pt_for(9.0, PointUnit::Typographic), 12.0);
		assert_eq!(px_from_pt_for(12.0, PointUnit::Typographic), 16.0);
	}
}
