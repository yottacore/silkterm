// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// The wallpaper pipeline, off the winit thread.
//
// Everything here touches the filesystem or spends real CPU: scanning the
// rotation folder, reading the shuffle history, decoding the image, blurring and
// contrast-flattening it, reading its XMP tags. Any of those paths can be a
// mounted share that answers slowly or not at all, and the blur alone costs
// hundreds of milliseconds on a large image - so none of it may sit between
// launch and the first frame. The window paints with no wallpaper and picks one
// up when the result arrives (UserEvent::WallpaperReady).
//
// Only the GPU upload stays on the winit thread; it needs the device, and it is
// a plain texture write.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use winit::event_loop::EventLoopProxy;

use crate::config::{self, Fit, Settings};
use crate::term::UserEvent;

// A built-in wallpaper baked into the binary, shown when the user has none
// configured (wallpaper_fallback_builtin). ~100KB - negligible next to the binary.
const DEFAULT_BACKGROUND: &[u8] = include_bytes!("../assets/default-background.jpg");

// How many recently-shown images the shuffle holds back at most.
const WP_AVOID_MAX: usize = 32;

// What the worker was asked to do. `settings` is a snapshot: the worker must
// never read the live store, since it outlives the settings it was started with.
#[derive(Debug)]
pub struct Request {
	pub seq: u64,
	// The newest request's seq, shared with the window. A worker whose own seq
	// is no longer the newest has been superseded, and stops at its next stage
	// rather than blurring a photo nobody will see.
	pub newest: Arc<AtomicU64>,
	pub settings: Arc<Settings>,
	// also scan the rotation folder and pick from it (startup and each rotation
	// step); false just loads whatever `settings.wallpaper` names.
	pub scan: bool,
	// The image showing now. Order-mode rotation advances from it (by name, so a
	// re-scan that moved things around still ends in the right place), and a
	// non-scanning request keeps it when the settings name none - otherwise
	// re-reading the config while rotating would blank the wallpaper until the
	// next tick, since a rotated pick is live-only and never written to the file.
	pub current: Option<PathBuf>,
	// A bare `--wallpaper` or `--wallpaper-file` asked for no picture, and gets
	// none. Without this the built-in stood in, or a rotation folder left the
	// window bare, so one flag meant two things depending on a folder.
	pub cleared: bool,
}

impl Request {
	fn stale(&self) -> bool {
		self.newest.load(Ordering::Relaxed) != self.seq
	}
}

// Rotation pacing, kept apart from the window so it can be driven by a clock.
//
// A tick that finds a request still working sends nothing. Sending would only
// retire the one in flight, and once preparing an image took longer than the
// interval every request was retired before it arrived: the picture never
// changed, and each abandoned thread went on blurring a photo to the end. The
// tick is remembered and served when the result arrives - by the result itself
// when it was a rotation, or by sending one then when it was not.
#[derive(Debug, Default)]
pub struct Pacing {
	inflight: Option<u64>,
	owed: bool,
}

impl Pacing {
	pub fn sent(&mut self, seq: u64) {
		self.inflight = Some(seq);
	}

	// A tick: whether to send a rotation request now.
	pub fn tick(&mut self) -> bool {
		if self.inflight.is_some() {
			self.owed = true;
			return false;
		}
		true
	}

	// The newest request answered. True means a tick fired while it was working
	// and the answer was no rotation, so one is due now.
	pub fn arrived(&mut self, scanned: bool) -> bool {
		self.inflight = None;
		if scanned {
			self.owed = false;
			return false;
		}
		std::mem::take(&mut self.owed)
	}
}

// Image pixels ready for upload, with the layout the file's own tags asked for.
#[derive(Debug, Clone)]
pub struct Prepared {
	pub rgba: image::RgbaImage,
	pub opacity: f32,
	pub fit: Fit,
	pub anchor: [f32; 2],
	// What this picture is worth to a derived text color (autotheme.rs). Summed
	// here because this is where the finished pixels are, and it is six numbers
	// rather than a copy of them.
	pub summary: crate::autotheme::Summary,
}

// What a scan found. Absent when the request didn't scan, or when the folder
// turned out to hold nothing.
#[derive(Debug, Clone)]
pub struct Rotation {
	pub count: usize,
	pub current: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Loaded {
	pub seq: u64,
	pub image: Option<Prepared>,
	pub rotation: Option<Rotation>,
	pub scanned: bool,
}

// Run one request on its own thread and post the result back to the event loop.
//
// A thread per request rather than one long-lived worker, deliberately: a
// request that hangs on a dead mount blocks its own thread forever, and a shared
// worker would leave every later request queued behind it. The stale result is
// harmless when it finally arrives - the sequence stamp retires it. A worker
// that has been superseded while doing real work gives up between stages.
pub fn spawn(proxy: &EventLoopProxy<UserEvent>, request: Request) {
	let proxy = proxy.clone();
	let spawned = std::thread::Builder::new()
		.name("wallpaper".into())
		.spawn(move || {
			let loaded = run(&request);
			let _ = proxy.send_event(UserEvent::WallpaperReady(Box::new(loaded)));
		});
	if let Err(e) = spawned {
		eprintln!(
			"{}: could not start wallpaper loader: {e}",
			config::APP_NAME
		);
	}
}

fn run(request: &Request) -> Loaded {
	let settings = &request.settings;
	let mut rotation = None;
	let mut path = settings.wallpaper.clone();
	if request.scan {
		let showing = request
			.current
			.as_ref()
			.and_then(|path| path.file_name())
			.map(|name| name.to_string_lossy().into_owned());
		rotation = rotate(settings, showing.as_deref());
		if let Some(picked) = &rotation {
			path = Some(picked.current.clone());
		}
	} else if path.is_none() && settings.rotation_folder().is_some() {
		path.clone_from(&request.current);
	}
	// A configured folder that supplies images owns the wallpaper, so the
	// built-in must not stand in for it. On a scan we know what the folder
	// actually holds; otherwise fall back to "is one configured at all".
	let folder_active = if request.scan {
		rotation.is_some()
	} else {
		settings.rotation_folder().is_some()
	};
	let image = (settings.wallpaper_enabled && !request.cleared)
		.then(|| {
			prepare(settings, path.as_deref(), folder_active, &|| {
				request.stale()
			})
		})
		.flatten();
	Loaded {
		seq: request.seq,
		image,
		rotation,
		scanned: request.scan,
	}
}

// Scan the rotation folder, pick the next image, and record the pick.
fn rotate(settings: &Settings, current: Option<&str>) -> Option<Rotation> {
	let dir = settings.rotation_folder()?;
	let images = list_folder_images(dir);
	if images.is_empty() {
		// Silent when the folder was auto-detected - the user never asked for
		// rotation, so an absent or empty dir is not a mistake to report.
		if !settings.wallpaper_folder_auto {
			eprintln!(
				"{}: wallpaper_folder {} has no images",
				config::APP_NAME,
				dir.display()
			);
		}
		return None;
	}
	let mut recent = load_history();
	let showing = current.and_then(|name| index_of(&images, name));
	let index = if settings.wallpaper_rotate_random {
		let held: Vec<usize> = recent
			.iter()
			.filter_map(|name| index_of(&images, name))
			.collect();
		shuffle_pick(images.len(), &held, time_entropy())
	} else {
		next_wallpaper_index(images.len(), showing.unwrap_or(0))
	};
	let current = images[index].clone();
	if let Some(name) = current
		.file_name()
		.map(|n| n.to_string_lossy().into_owned())
	{
		recent.retain(|seen| *seen != name);
		recent.insert(0, name);
		recent.truncate(WP_AVOID_MAX);
		write_history(&recent);
	}
	Some(Rotation {
		count: images.len(),
		current,
	})
}

fn index_of(images: &[PathBuf], name: &str) -> Option<usize> {
	images
		.iter()
		.position(|path| path.file_name().is_some_and(|f| f == name))
}

// The longest edge a wallpaper is kept at. Past a 4K display's own width there
// is nothing left to see, and every common GPU takes a texture this size.
const MAX_EDGE: u32 = 4096;

// The size to scale down to, or None when it already fits. Proportions kept, and
// never scaled up.
fn fit_within(w: u32, h: u32, max: u32) -> Option<(u32, u32)> {
	if w == 0 || h == 0 || (w <= max && h <= max) {
		return None;
	}
	let scale = f64::from(max) / f64::from(w.max(h));
	Some((
		((f64::from(w) * scale).round() as u32).max(1),
		((f64::from(h) * scale).round() as u32).max(1),
	))
}

// Decode the wallpaper and apply everything that is fixed at load time (blur,
// contrast mask, the image's own layout tags). `folder_active` suppresses the
// built-in stand-in where no path was given at all, since rotation is about to
// supply one; a path that fails to open still falls back to it. `stale` is asked
// before each stage, and answers None once the request has been superseded.
fn prepare(
	settings: &Settings,
	path: Option<&Path>,
	folder_active: bool,
	stale: &dyn Fn() -> bool,
) -> Option<Prepared> {
	if stale() {
		return None;
	}
	let mut source = None;
	let mut img = match path {
		Some(path) => match image::open(path) {
			Ok(loaded) => {
				source = Some(path);
				cut_to_rgba(loaded)
			}
			Err(e) => {
				eprintln!(
					"{}: background image {}: {e}",
					config::APP_NAME,
					path.display()
				);
				// A file that won't open supplies nothing, folder or not.
				builtin(settings)?
			}
		},
		// No image or rotation folder configured: fall back to the embedded default
		// so a fresh install still looks the part. Opt out with wallpaper_fallback_builtin.
		None => (!folder_active).then(|| builtin(settings)).flatten()?,
	};
	// The image's own tags: layout, and the two look values. Read straight from
	// the file the pixels came from - the embedded default wallpaper has no
	// path, and keeps the configured values.
	let tags = source
		.filter(|_| settings.wallpaper_honor_xmp || settings.wallpaper_honor_xmp_look)
		.map_or_else(crate::xmp::Tags::default, crate::xmp::read);
	let (mut opacity, mut blur) = (settings.wallpaper_opacity, settings.wallpaper_blur);
	if settings.wallpaper_honor_xmp_look {
		opacity = tags.opacity.unwrap_or(opacity);
		blur = tags.blur.unwrap_or(blur);
	}
	// Blur and contrast-flatten, done in LINEAR light (decode sRGB -> process in
	// f32 -> re-encode) so transitions are gamma-correct; an sRGB-space blur
	// darkens edges. The f32 intermediate also avoids 8-bit banding inside the
	// blur (final banding is handled by the high-precision offscreen + the blit's
	// dither).
	// The blur asserts on a sigma that is not a normal float, and a panic here
	// takes every shell down with it. 1e-40 is inside the config's range.
	let blur = if blur.is_normal() { blur } else { 0.0 };
	if blur > 0.0 || settings.wallpaper_contrast_mask {
		// The float copy is sixteen bytes a pixel and the blur is the slow part,
		// so this is where a superseded request costs the most to carry on.
		if stale() {
			return None;
		}
		let (w, h) = img.dimensions();
		let mut linear: image::ImageBuffer<image::Rgba<f32>, Vec<f32>> =
			image::ImageBuffer::new(w, h);
		for (dst, src) in linear.pixels_mut().zip(img.pixels()) {
			*dst = image::Rgba([
				config::to_linear(src[0]),
				config::to_linear(src[1]),
				config::to_linear(src[2]),
				f32::from(src[3]) / 255.0,
			]);
		}
		if blur > 0.0 {
			linear = image::imageops::blur(&linear, blur);
		}
		if stale() {
			return None;
		}
		if settings.wallpaper_contrast_mask {
			crate::contrast::apply(
				&mut linear,
				settings.wallpaper_contrast_mask_size,
				settings.wallpaper_contrast_mask_strength,
				settings.wallpaper_contrast_mask_auto,
			);
		}
		for (dst, src) in img.pixels_mut().zip(linear.pixels()) {
			*dst = image::Rgba([
				config::from_linear_u8(src[0]),
				config::from_linear_u8(src[1]),
				config::from_linear_u8(src[2]),
				(src[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
			]);
		}
	}
	if stale() {
		return None;
	}
	// A photo isn't squashed by a default that suits gradients.
	let mut fit = settings.wallpaper_default_fit;
	let mut anchor = [0.5, 0.5];
	if settings.wallpaper_honor_xmp {
		if let Some(tagged) = tags.fit {
			fit = tagged;
		}
		if let Some(tagged) = tags.anchor {
			anchor = tagged;
		}
	}
	let summary = crate::autotheme::summarize(&img, opacity);
	Some(Prepared {
		rgba: img,
		opacity,
		fit,
		anchor,
		summary,
	})
}

fn builtin(settings: &Settings) -> Option<image::RgbaImage> {
	settings
		.wallpaper_fallback_builtin
		.then(|| image::load_from_memory(DEFAULT_BACKGROUND).ok())
		.flatten()
		.map(cut_to_rgba)
}

// A wallpaper is only ever drawn at window size, and the linear intermediate in
// `prepare` is sixteen bytes a pixel - so an ordinary large photo wanted
// gigabytes, and an image wider than the GPU's texture limit aborted the upload.
// The cut comes before the RGBA copy and uses no float buffer, so a small file
// with huge dimensions costs its decode (512 MiB at most, the image crate's own
// limit) and nothing at full size after that. Blurring gets cheaper too, and its
// radius is relative to what is on screen.
fn cut_to_rgba(img: image::DynamicImage) -> image::RgbaImage {
	match fit_within(img.width(), img.height(), MAX_EDGE) {
		Some((w, h)) => img.thumbnail_exact(w, h).into_rgba8(),
		None => img.into_rgba8(),
	}
}

// Every image in a rotation folder, in filename order.
fn list_folder_images(dir: &Path) -> Vec<PathBuf> {
	let Ok(entries) = std::fs::read_dir(dir) else {
		return Vec::new();
	};
	let mut images: Vec<PathBuf> = entries
		.flatten()
		.map(|e| e.path())
		.filter(|p| p.is_file() && config::is_image_file(p))
		.collect();
	images.sort();
	images
}

// The recently-shown list, newest first. Stored as filenames, not indices, so
// adding or removing images doesn't shift what "recent" means.
fn load_history() -> Vec<String> {
	let Some(path) = config::wallpaper_history_path() else {
		return Vec::new();
	};
	let Ok(text) = std::fs::read_to_string(path) else {
		return Vec::new();
	};
	text.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(String::from)
		.take(WP_AVOID_MAX)
		.collect()
}

fn write_history(recent: &[String]) {
	let Some(path) = config::wallpaper_history_path().filter(|p| config::may_write(p)) else {
		return;
	};
	let mut text = recent.join("\n");
	text.push('\n');
	let _ = std::fs::write(path, text);
}

// Cheap non-crypto entropy for random rotation, from the wall clock. Not used
// for anything security-sensitive - just to vary which image comes up next.
fn time_entropy() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_nanos() as u64)
}

// Next image index in filename order, wrapping.
fn next_wallpaper_index(len: usize, current: usize) -> usize {
	if len < 2 {
		return 0;
	}
	(current + 1) % len
}

// Pick the next image the way a music player shuffles: at random, but never one
// of the last few shown. Straight uniform draws repeat often enough that people
// read them as broken, so holding back roughly half the folder (capped) buys the
// feel of randomness while staying a shuffle rather than a fixed cycle.
// `recent` is newest-first; entries past the hold-back window are ignored.
fn shuffle_pick(len: usize, recent: &[usize], entropy: u64) -> usize {
	if len < 2 {
		return 0;
	}
	let hold = (len / 2).clamp(1, WP_AVOID_MAX).min(len - 1);
	let avoid = &recent[..recent.len().min(hold)];
	// hold <= len-1, so at least one index always survives
	let candidates: Vec<usize> = (0..len).filter(|i| !avoid.contains(i)).collect();
	candidates[(entropy % candidates.len() as u64) as usize]
}

#[cfg(test)]
mod tests {
	use super::{
		Pacing, Prepared, Request, WP_AVOID_MAX, list_folder_images, next_wallpaper_index, prepare,
		run, shuffle_pick,
	};
	use crate::config::{Fit, Settings};
	use std::sync::Arc;
	use std::sync::atomic::AtomicU64;

	// The blur and the contrast mask are the slow half and neither is under test
	// here; skipping them keeps these fast.
	fn flat_settings() -> Settings {
		Settings {
			wallpaper_blur: 0.0,
			wallpaper_contrast_mask: false,
			..Settings::default()
		}
	}

	// Nothing sat between the file and a linear f32 buffer at sixteen bytes a
	// pixel, so an ordinary large photo wanted gigabytes and an image past the
	// GPU's texture limit aborted the upload.
	// Test ID: EpHe9lo
	#[test]
	fn a_large_wallpaper_is_cut_down_before_any_of_the_work() {
		use super::{MAX_EDGE, fit_within};
		// a 60 megapixel camera file, and what it used to cost as f32 rgba
		let (w, h) = (9504, 6336);
		// and the blur holds a second copy of it
		assert!(
			u64::from(w) * u64::from(h) * 16 > 900 << 20,
			"most of a gigabyte, twice"
		);
		let (nw, nh) = fit_within(w, h, MAX_EDGE).expect("cut down");
		assert_eq!(nw, MAX_EDGE);
		assert!(u64::from(nw) * u64::from(nh) * 16 < 200 << 20, "{nw}x{nh}");
		// proportions kept
		assert!((f64::from(nw) / f64::from(nh) - f64::from(w) / f64::from(h)).abs() < 0.001);

		// a tall image is measured on its own long edge
		assert_eq!(fit_within(1000, 8000, MAX_EDGE), Some((512, 4096)));
		// and anything that already fits is left exactly as it is
		assert_eq!(fit_within(3840, 2160, MAX_EDGE), None);
		assert_eq!(fit_within(MAX_EDGE, MAX_EDGE, MAX_EDGE), None);
		assert_eq!(fit_within(0, 0, MAX_EDGE), None);
	}

	// Test ID: EjwZbJA
	#[test]
	fn wallpaper_order_wraps() {
		assert_eq!(next_wallpaper_index(3, 0), 1);
		assert_eq!(next_wallpaper_index(3, 2), 0); // wraps
		assert_eq!(next_wallpaper_index(1, 0), 0); // single image: stays put
		assert_eq!(next_wallpaper_index(0, 0), 0); // empty: safe
	}

	// Test ID: Eld70rY
	#[test]
	fn shuffle_never_repeats_a_recent_image() {
		// whatever the entropy, the pick avoids the held-back window and stays in range
		for entropy in 0..200u64 {
			for recent in [
				vec![],
				vec![0],
				vec![3, 1],
				vec![4, 2, 0], // deeper than the window: extra entries are ignored
			] {
				let next = shuffle_pick(5, &recent, entropy);
				assert!(next < 5);
				// 5 images hold back 2, so the two newest must not come back
				for held in recent.iter().take(2) {
					assert_ne!(next, *held, "shuffle repeated a recent image");
				}
			}
		}
	}

	// Test ID: Eld70rZ
	#[test]
	fn shuffle_survives_tiny_folders() {
		// two images alternate; one (or none) has nowhere else to go
		for entropy in 0..20u64 {
			assert_eq!(shuffle_pick(2, &[0], entropy), 1);
			assert_eq!(shuffle_pick(2, &[1], entropy), 0);
			assert_eq!(shuffle_pick(1, &[0], entropy), 0);
			assert_eq!(shuffle_pick(0, &[], entropy), 0);
		}
	}

	// Test ID: Eld70ra
	#[test]
	fn shuffle_still_reaches_every_image() {
		// holding back recent picks must not strand any image permanently
		let mut seen = std::collections::HashSet::new();
		let mut recent: Vec<usize> = Vec::new();
		for entropy in 0..500u64 {
			let next = shuffle_pick(6, &recent, entropy);
			seen.insert(next);
			recent.insert(0, next);
			recent.truncate(WP_AVOID_MAX);
		}
		assert_eq!(seen.len(), 6, "some image was never picked");
	}

	// Test ID: EjwZbJB
	#[test]
	fn folder_scan_filters_and_sorts() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_wp_scan_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		for name in ["b.png", "a.JPG", "c.jpeg", "notes.txt", "c.gif", ".hidden"] {
			std::fs::write(dir.join(name), b"x").unwrap();
		}
		std::fs::create_dir_all(dir.join("d.png")).unwrap(); // a dir named like an image
		let imgs = list_folder_images(&dir);
		let names: Vec<String> = imgs
			.iter()
			.map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
			.collect();
		// only decodable image files, case-insensitive ext, sorted; the .txt, the dir
		// and the .gif (no decoder for it) all kept out
		assert_eq!(names, vec!["a.JPG", "b.png", "c.jpeg"]);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The path is no longer stat'd before the worker sees it (that check used to
	// run on the startup thread), so an unreadable one must end on the built-in
	// rather than on nothing.
	// Test ID: ElvpdEu
	#[test]
	fn an_unreadable_image_still_lands_on_the_builtin() {
		let mut s = flat_settings();
		let missing = crate::testdir::run_dir().join("silkterm_no_such_wallpaper.png");
		let _ = std::fs::remove_file(&missing);
		assert!(prepare(&s, Some(&missing), false, &|| false).is_some());
		// a rotation folder doesn't change that: the picked file supplies nothing
		assert!(prepare(&s, Some(&missing), true, &|| false).is_some());
		// ... unless the user opted out
		s.wallpaper_fallback_builtin = false;
		assert!(prepare(&s, Some(&missing), false, &|| false).is_none());
	}

	// The blur asserts on a sigma that is not a normal float, and a subnormal one
	// passed both the config's range and the tag reader, so the worker panicked
	// and took the terminal with it.
	// Test ID: Eq8KqtE
	#[test]
	fn a_subnormal_blur_is_no_blur() {
		let mut s = flat_settings();
		s.wallpaper_blur = 1e-40;
		assert!(prepare(&s, None, false, &|| false).is_some());

		// the same value from an image's own tag
		let packet = "<x:xmpmeta><rdf:RDF><rdf:Description rdf:about=''>\
			<wallpaper:Blur>1e-40</wallpaper:Blur></rdf:Description></rdf:RDF></x:xmpmeta>";
		let path =
			crate::testdir::run_dir().join(format!("silkterm_wp_blur_{}.png", std::process::id()));
		std::fs::write(&path, tagged_png(packet)).unwrap();
		s.wallpaper_blur = 0.0;
		s.wallpaper_honor_xmp_look = true;
		assert_eq!(crate::xmp::read(&path).blur, Some(0.0));
		let prepared = prepare(&s, Some(&path), false, &|| false).expect("prepared");
		assert_eq!(
			prepared.rgba.dimensions(),
			(8, 8),
			"the file, not the built-in"
		);
		let _ = std::fs::remove_file(&path);
	}

	// A real 8x8 PNG with an iTXt XMP packet ahead of IDAT, where the reader
	// stops looking.
	fn tagged_png(packet: &str) -> Vec<u8> {
		fn crc(bytes: &[u8]) -> u32 {
			let mut c = !0u32;
			for b in bytes {
				c ^= u32::from(*b);
				for _ in 0..8 {
					c = if c & 1 == 1 {
						(c >> 1) ^ 0xedb8_8320
					} else {
						c >> 1
					};
				}
			}
			!c
		}
		let mut plain = Vec::new();
		image::GrayImage::new(8, 8)
			.write_to(
				&mut std::io::Cursor::new(&mut plain),
				image::ImageFormat::Png,
			)
			.unwrap();
		let mut body = b"iTXtXML:com.adobe.xmp\0\0\0\0\0".to_vec();
		body.extend_from_slice(packet.as_bytes());
		let mut chunk = u32::try_from(body.len() - 4)
			.unwrap()
			.to_be_bytes()
			.to_vec();
		chunk.extend_from_slice(&body);
		chunk.extend_from_slice(&crc(&body).to_be_bytes());
		// signature (8) + IHDR (25)
		let mut out = plain[..33].to_vec();
		out.extend_from_slice(&chunk);
		out.extend_from_slice(&plain[33..]);
		out
	}

	// A small file with huge dimensions was converted to RGBA and resized through
	// a float copy at full width before the cut, gigabytes from a few hundred KB.
	// Measured in a child copy of this test binary so no other test's memory
	// counts.
	// Test ID: Eq8KqtF
	#[cfg(target_os = "linux")]
	#[test]
	fn a_huge_image_costs_its_decode_and_no_more() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_wp_huge_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("huge.png");
		// 64 MiB decoded. Before the fix this peaked near 900 MiB.
		image::GrayImage::new(8000, 8000).save(&path).unwrap();
		let out = std::process::Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"wallpaper::tests::huge_image_child",
				"--nocapture",
			])
			.env("SILK_WP_HUGE", &path)
			.output()
			.unwrap();
		let _ = std::fs::remove_dir_all(&dir);
		let text = String::from_utf8_lossy(&out.stdout);
		assert!(
			out.status.success(),
			"{text}{}",
			String::from_utf8_lossy(&out.stderr)
		);
		let grew: u64 = text
			.lines()
			.find_map(|l| l.strip_prefix("grew_kb "))
			.and_then(|v| v.trim().parse().ok())
			.expect("child reports its growth");
		assert!(grew < 200 << 10, "peak grew {} MiB", grew >> 10);
	}

	// Test ID: Eq8KqtG
	#[cfg(target_os = "linux")]
	#[test]
	fn huge_image_child() {
		fn peak_kb() -> u64 {
			std::fs::read_to_string("/proc/self/status")
				.unwrap()
				.lines()
				.find_map(|l| l.strip_prefix("VmHWM:"))
				.and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
				.unwrap()
		}
		let Some(path) = std::env::var_os("SILK_WP_HUGE") else {
			return; // only does anything when the test above starts it
		};
		let before = peak_kb();
		let prepared = prepare(
			&flat_settings(),
			Some(std::path::Path::new(&path)),
			false,
			&|| false,
		);
		assert_eq!(prepared.expect("prepared").rgba.dimensions(), (4096, 4096));
		println!("grew_kb {}", peak_kb() - before);
	}

	// A request retired by a newer one used to blur and mask a photo to the end,
	// a gigabyte or so each for a 4K image, and a rotation faster than the
	// preparation kept several going at once. The worker asks between stages
	// now, and every stage boundary is a place it gives up.
	// Test ID: Eq9J5HU
	#[test]
	fn a_superseded_request_stops_before_its_next_stage() {
		use std::cell::Cell;
		let s = Settings {
			wallpaper_blur: 1.0,
			wallpaper_contrast_mask: true,
			..flat_settings()
		};
		let asked = Cell::new(0);
		let live = prepare(&s, None, false, &|| {
			asked.set(asked.get() + 1);
			false
		});
		assert!(live.is_some());
		let stages = asked.get();
		assert_eq!(
			stages, 4,
			"before the decode, the float copy, the mask and the layout"
		);
		for stale_from in 0..stages {
			asked.set(0);
			let gone = prepare(&s, None, false, &|| {
				let n = asked.get();
				asked.set(n + 1);
				n >= stale_from
			});
			assert!(gone.is_none(), "stale at check {stale_from}");
			assert_eq!(
				asked.get(),
				stale_from + 1,
				"stopped at the check that said so"
			);
		}
		// the stamp is what says so in the real thing
		let newest = Arc::new(AtomicU64::new(2));
		let request = Request {
			seq: 1,
			newest,
			settings: Arc::new(s),
			scan: false,
			current: None,
			cleared: false,
		};
		assert!(run(&request).image.is_none());
	}

	// The pacing rule, driven by a clock: images that take six seconds to
	// prepare on a two-second interval. Each tick used to send a request that
	// retired the one before it, so the picture never changed, and the retired
	// workers piled up. One preparation at a time now, and the picture changes
	// no later than one preparation after a tick.
	// Test ID: Eq9J5HV
	#[test]
	fn rotation_keeps_going_when_preparing_outlasts_the_interval() {
		const IVL: u32 = 2;
		const PREP: u32 = 6;
		let mut pacing = Pacing::default();
		let mut seq = 0u64;
		let mut next = Some(0u32); // the timer, as the window keeps it
		let mut working: Vec<(u64, u32, bool)> = Vec::new(); // seq, done at, scan
		let mut changes = Vec::new();
		let mut most_at_once = 0;
		for now in 0..80u32 {
			// results first, as the event loop would see them before its own poll
			let done: Vec<_> = working.iter().filter(|w| w.1 == now).copied().collect();
			working.retain(|w| w.1 != now);
			for (id, _, scan) in done {
				if id != seq {
					continue; // superseded while it was working
				}
				if pacing.arrived(scan) {
					next = Some(now);
				}
				if scan {
					changes.push(now);
					next = Some(now + IVL);
				}
			}
			// a settings change re-reads the current image without rotating, and
			// supersedes whatever was working
			if now == 21 {
				seq += 1;
				pacing.sent(seq);
				working.push((seq, now + PREP, false));
			}
			if next.is_some_and(|at| now >= at) {
				next = Some(now + IVL);
				if pacing.tick() {
					seq += 1;
					pacing.sent(seq);
					working.push((seq, now + PREP, true));
				} else {
					next = None;
				}
			}
			let live = working.iter().filter(|w| w.0 == seq).count();
			most_at_once = most_at_once.max(live);
			assert!(
				working.len() <= 2,
				"retired workers pile up at {now}: {working:?}"
			);
		}
		assert_eq!(most_at_once, 1);
		// one preparation, a rest of one interval, the next: every eight seconds,
		// and the settings change costs one preparation more before rotation
		// picks up again
		assert_eq!(changes, vec![6, 14, 33, 41, 49, 57, 65, 73]);
	}

	// An empty rotation folder reports no rotation, and the built-in fills in -
	// the folder's emptiness used to be tested during config resolve, so the
	// suppression has to key on what the scan found, not on the folder existing.
	// Test ID: ElvpdEv
	#[test]
	fn an_empty_rotation_folder_falls_back_to_the_builtin() {
		let mut s = flat_settings();
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_wp_empty_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		s.wallpaper_folder = Some(dir.clone());
		s.wallpaper_folder_auto = true; // auto-detected: nothing to report
		assert!(super::rotate(&s, None).is_none());
		assert!(prepare(&s, None, false, &|| false).is_some());
		let _ = std::fs::remove_dir_all(&dir);
	}

	// The image's own tags win over the configured default, and are read from the
	// file the pixels actually came from (which for rotation is the picked image,
	// not whatever `wallpaper` happened to name).
	// Test ID: ElvpdEw
	#[test]
	fn the_builtin_keeps_the_configured_fit() {
		let s = Settings {
			wallpaper_default_fit: Fit::Zoom,
			..flat_settings()
		};
		let Some(Prepared { fit, anchor, .. }) = prepare(&s, None, false, &|| false) else {
			panic!("built-in wallpaper failed to decode");
		};
		assert_eq!(fit, Fit::Zoom);
		assert_eq!(anchor, [0.5, 0.5]);
	}

	// Test ID: Er2UJeL
	#[test]
	fn the_master_switch_turns_the_wallpaper_off() {
		let request = |enabled| Request {
			seq: 1,
			newest: Arc::new(AtomicU64::new(1)),
			settings: Arc::new(Settings {
				wallpaper_enabled: enabled,
				..flat_settings()
			}),
			scan: false,
			current: None,
			cleared: false,
		};
		assert!(run(&request(false)).image.is_none());
		// the control: switched on, the same request shows the built-in
		assert!(run(&request(true)).image.is_some());
	}

	// Rotation off leaves the folder in the settings, so switching it back on
	// picks up where it was.
	// Test ID: Er2UJeM
	#[test]
	fn rotation_off_stops_rotating_and_keeps_the_folder() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_wp_rot_off_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		image::GrayImage::new(2, 2).save(dir.join("a.png")).unwrap();
		let on = Settings {
			wallpaper_folder: Some(dir.clone()),
			wallpaper_rotate_enabled: true,
			..flat_settings()
		};
		assert_eq!(on.rotation_folder(), Some(&dir));
		let off = Settings {
			wallpaper_rotate_enabled: false,
			..on.clone()
		};
		assert!(off.rotation_folder().is_none());
		assert!(super::rotate(&off, None).is_none());
		assert_eq!(off.wallpaper_folder, Some(dir.clone()));
		// and the master switch takes rotation with it
		let master_off = Settings {
			wallpaper_enabled: false,
			..on
		};
		assert!(master_off.rotation_folder().is_none());
		let _ = std::fs::remove_dir_all(&dir);
	}

	fn tagged_file(name: &str, tags: &str) -> std::path::PathBuf {
		let packet = format!(
			"<x:xmpmeta><rdf:RDF><rdf:Description rdf:about=''>{tags}\
			</rdf:Description></rdf:RDF></x:xmpmeta>"
		);
		let path = crate::testdir::run_dir()
			.join(format!("silkterm_wp_{name}_{}.png", std::process::id()));
		std::fs::write(&path, tagged_png(&packet)).unwrap();
		path
	}

	// Test ID: Er2UJeN
	#[test]
	fn an_images_layout_tags_win_until_honor_tags_is_off() {
		let path = tagged_file(
			"layout",
			"<wallpaper:Fit>zoom</wallpaper:Fit><wallpaper:Anchor>25%, 80%</wallpaper:Anchor>",
		);
		let mut s = Settings {
			wallpaper_default_fit: Fit::Stretch,
			wallpaper_honor_xmp: true,
			..flat_settings()
		};
		let honored = prepare(&s, Some(&path), false, &|| false).expect("prepared");
		assert_eq!(honored.fit, Fit::Zoom);
		assert_eq!(honored.anchor, [0.25, 0.8]);
		s.wallpaper_honor_xmp = false;
		let ignored = prepare(&s, Some(&path), false, &|| false).expect("prepared");
		assert_eq!(ignored.fit, Fit::Stretch);
		assert_eq!(ignored.anchor, [0.5, 0.5]);
		let _ = std::fs::remove_file(&path);
	}

	// Test ID: Er2UJeO
	#[test]
	fn an_images_look_tags_win_until_honor_look_tags_is_off() {
		let path = tagged_file("look", "<wallpaper:Opacity>40%</wallpaper:Opacity>");
		let mut s = Settings {
			wallpaper_opacity: 0.1,
			wallpaper_honor_xmp_look: true,
			..flat_settings()
		};
		let honored = prepare(&s, Some(&path), false, &|| false).expect("prepared");
		assert!((honored.opacity - 0.4).abs() < 1e-6, "{}", honored.opacity);
		s.wallpaper_honor_xmp_look = false;
		let ignored = prepare(&s, Some(&path), false, &|| false).expect("prepared");
		assert!((ignored.opacity - 0.1).abs() < 1e-6, "{}", ignored.opacity);
		let _ = std::fs::remove_file(&path);
	}

	// A blur done on the sRGB bytes darkens an edge: black beside white met at
	// about 128 there, where half the light is sRGB 188.
	// Test ID: Er2UJeP
	#[test]
	fn the_blur_mixes_in_linear_light() {
		let path =
			crate::testdir::run_dir().join(format!("silkterm_wp_edge_{}.png", std::process::id()));
		let mut edge = image::RgbaImage::new(16, 1);
		for (x, _, px) in edge.enumerate_pixels_mut() {
			let v = if x < 8 { 0 } else { 255 };
			*px = image::Rgba([v, v, v, 255]);
		}
		edge.save(&path).unwrap();
		let s = Settings {
			wallpaper_blur: 2.0,
			..flat_settings()
		};
		let out = prepare(&s, Some(&path), false, &|| false).expect("prepared");
		let _ = std::fs::remove_file(&path);
		// the two pixels either side of the edge straddle half the light
		let pair = f32::from(out.rgba.get_pixel(7, 0)[0]) + f32::from(out.rgba.get_pixel(8, 0)[0]);
		let mid = pair / 2.0;
		assert!((mid - 188.0).abs() < 12.0, "edge at {mid}");
	}

	// A bare flag means no picture, whether or not a rotation folder is there.
	// Without the folder this used to show the built-in.
	// Test ID: Eq4Yrbi
	#[test]
	fn a_cleared_wallpaper_shows_nothing_with_or_without_a_folder() {
		let dir =
			crate::testdir::run_dir().join(format!("silkterm_wp_cleared_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		for folder in [None, Some(dir.clone())] {
			let settings = Settings {
				wallpaper_enabled: true,
				wallpaper: None,
				wallpaper_folder: folder.clone(),
				wallpaper_rotate_enabled: true,
				..flat_settings()
			};
			let request = |cleared| Request {
				seq: 1,
				newest: Arc::new(AtomicU64::new(1)),
				settings: Arc::new(settings.clone()),
				scan: false,
				current: None,
				cleared,
			};
			assert!(run(&request(true)).image.is_none(), "folder {folder:?}");
			if folder.is_none() {
				// the control: the same request, not cleared, shows the built-in
				assert!(run(&request(false)).image.is_some());
			}
		}
		let _ = std::fs::remove_dir_all(&dir);
	}
}
