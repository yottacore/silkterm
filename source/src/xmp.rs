// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Per-image tags, read out of the wallpaper file's own XMP packet:
//! `wallpaper:Fit` (stretch|zoom) and `wallpaper:Anchor` ("50%, 50%") for layout,
//! `wallpaper:Opacity` ("10%") and `wallpaper:Blur` (sigma in px, "10") for how
//! it wants to show. An image that knows it must not be distorted, or that it is
//! too busy to sit at the usual strength, can say so instead of the one global
//! setting deciding for the whole collection.
//!
//! No XML parser and no new crate: the packet is reached through the container's
//! own segment/chunk table, then the properties are picked out of the text.
//! Anything unrecognised yields None and the caller keeps its own default - a
//! wallpaper must still load when its metadata is missing or malformed.

use crate::config::Fit;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

// Namespace prefix as written into the files. The URI is
// https://github.com/jim-collier/xmp/wallpaper/1.0/ - deliberately named for
// what the tags describe, not for this program, so other tools can write them
// too. Matched on the prefix alone, which is what a writer emits and what a
// re-serialize preserves.
const NS: &str = "wallpaper:";

// Both writers put XMP in the header (before SOS / before IDAT), so these only
// bound a malformed or hostile file - never a real one.
const MAX_SCAN: u64 = 1 << 20;
const MAX_PACKET: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tags {
	pub fit: Option<Fit>,
	/// x, y in 0.0..=1.0; 0 is left/top, 1 is right/bottom. Zoom only.
	pub anchor: Option<[f32; 2]>,
	/// Same units as the two settings they replace: visibility 0..1, blur sigma
	/// in px. A tagged image takes these over the sliders.
	pub opacity: Option<f32>,
	pub blur: Option<f32>,
}

pub fn read(path: &Path) -> Tags {
	packet(path).map_or_else(Tags::default, |xmp| Tags {
		fit: property(&xmp, "Fit").as_deref().and_then(parse_fit),
		anchor: property(&xmp, "Anchor").as_deref().and_then(parse_anchor),
		opacity: property(&xmp, "Opacity").as_deref().and_then(parse_percent),
		blur: property(&xmp, "Blur").as_deref().and_then(parse_blur),
	})
}

fn packet(path: &Path) -> Option<String> {
	let mut file = File::open(path).ok()?;
	let mut magic = [0u8; 8];
	file.read_exact(&mut magic).ok()?;
	file.seek(SeekFrom::Start(0)).ok()?;
	let raw = if magic == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
		png(&mut file)?
	} else if magic[0] == 0xff && magic[1] == 0xd8 {
		jpeg(&mut file)?
	} else {
		return None;
	};
	String::from_utf8(raw).ok()
}

// PNG: walk the chunk table for the uncompressed iTXt that carries XMP. A
// zTXt/compressed one is skipped rather than inflated - nothing writes XMP that
// way, and carrying an inflate for it would not pay for itself.
fn png(file: &mut File) -> Option<Vec<u8>> {
	const KEYWORD: &[u8] = b"XML:com.adobe.xmp\0";
	file.seek(SeekFrom::Start(8)).ok()?;
	loop {
		let mut header = [0u8; 8];
		file.read_exact(&mut header).ok()?;
		let len = u64::from(u32::from_be_bytes([
			header[0], header[1], header[2], header[3],
		]));
		let kind = &header[4..8];
		// pixel data has started; any XMP would have come first
		if kind == b"IDAT" || kind == b"IEND" {
			return None;
		}
		if kind == b"iTXt" && len <= MAX_PACKET {
			let mut data = vec![0u8; usize::try_from(len).ok()?];
			file.read_exact(&mut data).ok()?;
			// keyword\0 + compression flag + method + lang\0 + translated\0 + text
			if let Some(rest) = data.strip_prefix(KEYWORD)
				&& rest.first() == Some(&0)
			{
				let mut fields = rest.get(2..)?.splitn(3, |b| *b == 0);
				fields.next()?; // language
				fields.next()?; // translated keyword
				return fields.next().map(<[u8]>::to_vec);
			}
		} else {
			file.seek(SeekFrom::Current(i64::try_from(len).ok()?))
				.ok()?;
		}
		file.seek(SeekFrom::Current(4)).ok()?; // crc
		if file.stream_position().ok()? > MAX_SCAN {
			return None;
		}
	}
}

// JPEG: walk the marker segments for the APP1 whose payload carries the XMP
// signature. Only the main packet - an extended (>64KB) one holds overflow
// properties, never these two.
fn jpeg(file: &mut File) -> Option<Vec<u8>> {
	const SIG: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
	file.seek(SeekFrom::Start(2)).ok()?;
	loop {
		let mut byte = [0u8; 1];
		file.read_exact(&mut byte).ok()?;
		if byte[0] != 0xff {
			return None;
		}
		// fill bytes are legal before a marker
		while byte[0] == 0xff {
			file.read_exact(&mut byte).ok()?;
		}
		match byte[0] {
			// standalone markers: no length, no payload
			0x01 | 0xd0..=0xd8 => continue,
			// entropy-coded data starts here, and the header is over
			0xd9 | 0xda => return None,
			_ => {}
		}
		let mut len_bytes = [0u8; 2];
		file.read_exact(&mut len_bytes).ok()?;
		let body = u64::from(u16::from_be_bytes(len_bytes)).checked_sub(2)?;
		if byte[0] == 0xe1 && body <= MAX_PACKET {
			let mut data = vec![0u8; usize::try_from(body).ok()?];
			file.read_exact(&mut data).ok()?;
			if let Some(rest) = data.strip_prefix(SIG) {
				return Some(rest.to_vec());
			}
		} else {
			file.seek(SeekFrom::Current(i64::try_from(body).ok()?))
				.ok()?;
		}
		if file.stream_position().ok()? > MAX_SCAN {
			return None;
		}
	}
}

// Both spellings a writer may use: an element, or an attribute on the enclosing
// rdf:Description (the "shorthand" form a re-serialize can switch to).
fn property(xmp: &str, name: &str) -> Option<String> {
	let open = format!("<{NS}{name}>");
	if let Some(start) = xmp.find(&open) {
		let rest = &xmp[start + open.len()..];
		if let Some(end) = rest.find(&format!("</{NS}{name}>")) {
			return Some(rest[..end].trim().to_string());
		}
	}
	let attr = format!("{NS}{name}=");
	let rest = &xmp[xmp.find(&attr)? + attr.len()..];
	let quote = rest.chars().next()?;
	if quote != '"' && quote != '\'' {
		return None;
	}
	let rest = &rest[1..];
	Some(rest[..rest.find(quote)?].trim().to_string())
}

fn parse_fit(value: &str) -> Option<Fit> {
	match value.trim().to_ascii_lowercase().as_str() {
		"zoom" => Some(Fit::Zoom),
		"stretch" => Some(Fit::Stretch),
		_ => None,
	}
}

// "<h>%, <v>%" - the trailing % is optional so a bare "50, 50" still reads.
fn parse_anchor(value: &str) -> Option<[f32; 2]> {
	let mut parts = value.split(',');
	let x = parse_percent(parts.next()?)?;
	let y = parse_percent(parts.next()?)?;
	parts.next().is_none().then_some([x, y])
}

// "<n>%" as a 0..1 fraction; the sign is optional and so is the unit.
fn parse_percent(value: &str) -> Option<f32> {
	number(value, "%").map(|v| (v / 100.0).clamp(0.0, 1.0))
}

// Sigma in pixels, "px" optional. Same ceiling as the config's own range: past
// that a value is a typo, and a runaway sigma would stall the decode.
fn parse_blur(value: &str) -> Option<f32> {
	// a subnormal sigma is no blur, and the blur itself refuses one
	number(value, "px").map(|v| {
		if v.is_normal() {
			v.clamp(0.0, 100.0)
		} else {
			0.0
		}
	})
}

fn number(value: &str, unit: &str) -> Option<f32> {
	let value: f32 = value.trim().trim_end_matches(unit).trim().parse().ok()?;
	value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
	use super::*;

	const DOC: &str = "<rdf:Description rdf:about=''>\
		<wallpaper:Fit>stretch</wallpaper:Fit>\
		<wallpaper:Anchor>25%, 80%</wallpaper:Anchor>\
		<wallpaper:Opacity>15%</wallpaper:Opacity>\
		<wallpaper:Blur>5</wallpaper:Blur></rdf:Description>";

	// Test ID: EllRmPA
	#[test]
	fn reads_both_spellings_of_a_property() {
		assert_eq!(property(DOC, "Fit").as_deref(), Some("stretch"));
		let short = "<rdf:Description wallpaper:Fit='zoom' wallpaper:Anchor=\"0%, 100%\"/>";
		assert_eq!(property(short, "Fit").as_deref(), Some("zoom"));
		assert_eq!(property(short, "Anchor").as_deref(), Some("0%, 100%"));
		assert_eq!(property(DOC, "Nope"), None);
	}

	// Chunk/segment walking is the part that can silently stop finding tags after
	// an edit, so both containers get driven end to end. Bodies are synthetic but
	// laid out exactly as a writer emits them; CRCs are never checked.
	fn png_file(packet: &str) -> Vec<u8> {
		let mut text = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
		text.extend_from_slice(packet.as_bytes());
		let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
		for (kind, body) in [
			(b"IHDR".as_slice(), vec![0u8; 13]),
			(b"iTXt".as_slice(), text),
			(b"IDAT".as_slice(), vec![0u8; 4]),
		] {
			out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
			out.extend_from_slice(kind);
			out.extend_from_slice(&body);
			out.extend_from_slice(&[0; 4]); // crc
		}
		out
	}

	fn jpeg_file(packet: &str) -> Vec<u8> {
		let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
		app1.extend_from_slice(packet.as_bytes());
		let mut out = vec![0xff, 0xd8, 0xff, 0xe0]; // SOI, APP0
		out.extend_from_slice(&6u16.to_be_bytes());
		out.extend_from_slice(b"JFIF");
		out.extend_from_slice(&[0xff, 0xe1]);
		out.extend_from_slice(&u16::try_from(app1.len() + 2).unwrap().to_be_bytes());
		out.extend_from_slice(&app1);
		out.extend_from_slice(&[0xff, 0xda]); // SOS
		out
	}

	// Test ID: EllRmPB
	#[test]
	fn walks_a_png_and_a_jpeg_to_the_packet() {
		let dir = crate::testdir::run_dir().join(format!("silkterm_xmp_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let cases = [
			("a.png", png_file(DOC)),
			("a.jpg", jpeg_file(DOC)),
			// no packet at all: the caller keeps its own default
			("bare.png", png_file("<rdf:Description/>")),
		];
		for (name, bytes) in cases {
			let path = dir.join(name);
			std::fs::write(&path, &bytes).unwrap();
			let got = read(&path);
			if name.starts_with("bare") {
				assert_eq!(got, Tags::default(), "{name}");
			} else {
				assert_eq!(got.fit, Some(Fit::Stretch), "{name}");
				assert_eq!(got.anchor, Some([0.25, 0.8]), "{name}");
				assert_eq!(got.opacity, Some(0.15), "{name}");
				assert_eq!(got.blur, Some(5.0), "{name}");
			}
		}
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EllRmPC
	#[test]
	fn anchor_accepts_the_written_form_and_rejects_junk() {
		assert_eq!(parse_anchor("50%, 50%"), Some([0.5, 0.5]));
		assert_eq!(parse_anchor("0,100"), Some([0.0, 1.0]));
		// out of range is clamped, not dropped - the intent is still readable
		assert_eq!(parse_anchor("-20%, 300%"), Some([0.0, 1.0]));
		for junk in ["50%", "50%, 50%, 50%", "left, top", "", "%,%"] {
			assert_eq!(parse_anchor(junk), None, "{junk} should not parse");
		}
	}

	// A missing or unreadable tag must leave the caller on its own default
	// rather than forcing one - that is what keeps the global setting meaningful.
	// Test ID: EllRmPD
	#[test]
	fn unknown_values_yield_nothing_to_apply() {
		assert_eq!(parse_fit("ZOOM"), Some(Fit::Zoom));
		assert_eq!(parse_fit(" stretch "), Some(Fit::Stretch));
		assert_eq!(parse_fit("cover"), None);
		assert_eq!(
			read(Path::new("/nonexistent/wallpaper-test.png")),
			Tags::default()
		);
	}

	// Test ID: EoSDflY
	#[test]
	fn look_tags_read_in_the_settings_own_units() {
		assert_eq!(parse_percent("10%"), Some(0.1));
		assert_eq!(parse_percent(" 25 "), Some(0.25));
		assert_eq!(parse_percent("150%"), Some(1.0));
		assert_eq!(parse_percent("-10%"), Some(0.0));
		assert_eq!(parse_blur("10"), Some(10.0));
		assert_eq!(parse_blur("2.5px"), Some(2.5));
		assert_eq!(parse_blur("500"), Some(100.0));
		assert_eq!(parse_blur("1e-40"), Some(0.0));
		for junk in ["", "%", "lots", "nan", "inf"] {
			assert_eq!(parse_percent(junk), None, "{junk} should not parse");
			assert_eq!(parse_blur(junk), None, "{junk} should not parse");
		}
	}

	// A wallpaper is somebody else's file, and a rotation folder is a folder of
	// them. The chunk walk below reads lengths out of the file and uses them to
	// size a read, so a corrupt or hostile image must not be able to take the
	// wallpaper worker down with it - which in a release build takes the process.
	mod fuzz {
		use super::super::{Tags, read};
		use crate::fuzz;
		use std::fmt::Write as _;

		// PNG and JPEG openings, so a case gets past the magic check and into the
		// chunk walk rather than being turned away at the first byte.
		const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
		const JPEG: [u8; 2] = [0xff, 0xd8];

		// A packet laid out the way a writer lays one out, holding values no writer
		// would. Nothing else a generator makes gets as far as the value parsers.
		fn tagged(rng: &mut fuzz::Rng) -> Vec<u8> {
			#[rustfmt::skip]
			const VALUES: [&str; 16] = [
				"nan", "NaN%", "-nan px", "inf", "-infinity", "1e40", "1e-40", "-0",
				"", " ", "%", "10px", "25%", "stretch", "50%, nan%", "nan, 50",
			];
			let mut packet = String::from("<rdf:Description");
			let mut elements = String::new();
			for _ in 0..=rng.below(4) {
				let name = rng.pick(&["Fit", "Anchor", "Opacity", "Blur"]);
				let value = if rng.chance(4) {
					fuzz::text(rng)
				} else if rng.chance(3) {
					format!("{}, {}", rng.pick(&VALUES), rng.pick(&VALUES))
				} else {
					(*rng.pick(&VALUES)).to_string()
				};
				if rng.chance(2) {
					write!(elements, "<wallpaper:{name}>{value}</wallpaper:{name}>").unwrap();
				} else {
					write!(packet, " wallpaper:{name}=\"{value}\"").unwrap();
				}
			}
			write!(packet, ">{elements}</rdf:Description>").unwrap();
			if rng.chance(2) {
				super::png_file(&packet)
			} else {
				super::jpeg_file(&packet)
			}
		}

		fn image(rng: &mut fuzz::Rng) -> Vec<u8> {
			if rng.chance(3) {
				return tagged(rng);
			}
			let mut out = Vec::new();
			match rng.below(4) {
				0 => out.extend_from_slice(&JPEG),
				1..=2 => out.extend_from_slice(&PNG),
				_ => {}
			}
			for _ in 0..=rng.below(20) {
				match rng.below(5) {
					// A chunk header: four bytes of length, four of type. A length
					// that lies about what follows is the case worth reaching.
					0..=2 => {
						let len = *rng.pick(&[0u32, 1, 8, 0xffff, 0x7fff_ffff, 0xffff_ffff]);
						out.extend_from_slice(&len.to_be_bytes());
						out.extend_from_slice(rng.pick(&[&b"iTXt"[..], b"zTXt", b"IDAT", b"IEND"]));
					}
					3 => out.extend_from_slice(b"XML:com.adobe.xmp\0"),
					_ => out.extend_from_slice(fuzz::text(rng).as_bytes()),
				}
			}
			out
		}

		fn check(case: &[u8], path: &std::path::Path) -> Tags {
			std::fs::write(path, case).expect("write the case");
			let tags = read(path);
			// Whatever comes out has to be a number the renderer can use.
			if let Some(opacity) = tags.opacity {
				assert!(opacity.is_finite(), "opacity {opacity}");
			}
			if let Some(blur) = tags.blur {
				assert!(blur == 0.0 || blur.is_normal(), "blur {blur}");
			}
			if let Some(anchor) = tags.anchor {
				assert!(anchor.iter().all(|v| v.is_finite()), "anchor {anchor:?}");
			}
			tags
		}

		// Test ID: EpQN0oT
		#[test]
		fn a_corrupt_image_cannot_take_the_wallpaper_worker_down() {
			let path =
				crate::testdir::run_dir().join(format!("silkfuzz-xmp-{}.bin", std::process::id()));
			let corpus = fuzz::corpus("xmp");
			for case in &corpus {
				check(case, &path);
			}
			// A generator whose packets never read asserts nothing, and still passes.
			let read_some = (0..64)
				.filter(|&seed| check(&tagged(&mut fuzz::Rng::new(seed)), &path) != Tags::default())
				.count();
			assert!(read_some > 0, "no generated packet read as tags");
			fuzz::soak("xmp", |seed| {
				let mut rng = fuzz::Rng::new(seed);
				check(&fuzz::input(&mut rng, &corpus, image), &path);
			});
			let _ = std::fs::remove_file(&path);
		}
	}
}
