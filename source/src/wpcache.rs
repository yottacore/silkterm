// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Prepared wallpapers kept on disk as JPEG, so a launch, a return to a picture
//! in rotation, a resize or a wake from the idle release reads a small file
//! rather than blurring the original again. Every SilkTerm process on the box
//! shares the folder: an entry is written to a temp file and renamed into
//! place, and one that does not read back whole is removed and made again.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::autotheme::{SPREAD, Summary};

// Part of every key. Bump it when `wallpaper::prepare_keeping` changes what it
// makes, or old copies keep being shown until they age out.
const FORMAT: u64 = 1;
const MAGIC: &[u8; 8] = b"silkwpc\x01";
const EXT: &str = "wpc";

/// Past this the least recently used copies go. A copy at 2560x1440 is 230 to
/// 450 KiB.
pub const LIMIT: u64 = 256 << 20;

// The encoder's 4:4:4. At the default blur a copy is within 4 levels of the
// prepared pixels, under half a level on average; 100 saves little, since the
// color conversion alone costs about half a level.
const QUALITY: u8 = 95;

// How far a kept copy's pixel count may be from the size asked for.
const NEAR: f64 = 0.05;

// A temp file this old was left by a process that died mid-write.
const ABANDONED: Duration = Duration::from_mins(10);

/// Everything but the size that changes the stored pixels, as bytes. Only
/// `wallpaper::kept_key` knows what that is, so it adds the fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key(Vec<u8>);

impl Default for Key {
	fn default() -> Self {
		let mut key = Key(Vec::with_capacity(256));
		key.push_number(FORMAT);
		key
	}
}

impl Key {
	pub fn push_bytes(&mut self, field: &[u8]) {
		self.push_number(field.len() as u64);
		self.0.extend_from_slice(field);
	}

	pub fn push_number(&mut self, number: u64) {
		self.0.extend_from_slice(&number.to_le_bytes());
	}

	/// Bytes too long to keep whole, such as an image built into the program.
	pub fn push_hash(&mut self, bytes: &[u8]) {
		self.push_number(bytes.len() as u64);
		self.push_number(fnv(bytes));
	}

	pub fn push_float(&mut self, number: f32) {
		self.push_number(u64::from(number.to_bits()));
	}

	/// A file as it is now: where it is, its length, when it last changed and,
	/// on Unix, which file it is, so a picture swapped in under the same name
	/// is told apart. None when it cannot be read.
	pub fn push_file(&mut self, path: &Path) -> Option<()> {
		let meta = std::fs::metadata(path)
			.ok()
			.filter(std::fs::Metadata::is_file)?;
		let changed = meta
			.modified()
			.ok()?
			.duration_since(SystemTime::UNIX_EPOCH)
			.ok()?;
		self.push_bytes(
			std::path::absolute(path)
				.ok()?
				.as_os_str()
				.as_encoded_bytes(),
		);
		self.push_number(meta.len());
		self.push_number(changed.as_secs());
		self.push_number(u64::from(changed.subsec_nanos()));
		#[cfg(unix)]
		{
			use std::os::unix::fs::MetadataExt;
			self.push_number(meta.dev());
			self.push_number(meta.ino());
		}
		Some(())
	}

	fn prefix(&self) -> String {
		format!("{:016x}-", fnv(&self.0))
	}

	fn name(&self, (w, h): (u32, u32)) -> String {
		format!("{}{w}x{h}.{EXT}", self.prefix())
	}
}

// FNV-1a. Stable across builds, which std's hasher does not promise; the whole
// key is checked on read, so a collision costs a miss and nothing more.
fn fnv(bytes: &[u8]) -> u64 {
	bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
		(hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
	})
}

#[derive(Debug)]
pub struct Kept {
	pub rgba: image::RgbaImage,
	/// The summary of the pixels as prepared, not as read back, so derived
	/// colors come out the same with or without the copy. Its opacity is the
	/// one it was stored with.
	pub summary: Summary,
}

/// The nearest kept copy for `key` within 5% of `want`'s pixel count. A copy
/// that does not read back whole is removed and the next one tried.
pub fn find(dir: &Path, key: &Key, want: (u32, u32)) -> Option<Kept> {
	let prefix = key.prefix();
	let wanted = pixels(want);
	let mut near: Vec<((u32, u32), f64)> = std::fs::read_dir(dir)
		.ok()?
		.flatten()
		.filter_map(|entry| {
			let name = entry.file_name();
			let size = parse_size(name.to_str()?.strip_prefix(&prefix)?)?;
			let off = (pixels(size) - wanted).abs() / wanted.max(1.0);
			(off <= NEAR).then_some((size, off))
		})
		.collect();
	// nearest first, and the larger of two as near, since shrinking looks better
	near.sort_by(|(a, a_off), (b, b_off)| {
		a_off
			.total_cmp(b_off)
			.then_with(|| pixels(*b).total_cmp(&pixels(*a)))
	});
	for (size, _) in near {
		let path = dir.join(key.name(size));
		if let Some(kept) = read(&path, key, size) {
			// last used, for the prune
			let _ = std::fs::File::options()
				.append(true)
				.open(&path)
				.and_then(|file| file.set_modified(SystemTime::now()));
			return Some(kept);
		}
		let _ = std::fs::remove_file(&path);
	}
	None
}

fn pixels((w, h): (u32, u32)) -> f64 {
	f64::from(w) * f64::from(h)
}

fn parse_size(rest: &str) -> Option<(u32, u32)> {
	let (w, h) = rest.strip_suffix(EXT)?.strip_suffix('.')?.split_once('x')?;
	Some((w.parse().ok()?, h.parse().ok()?))
}

// Layout, all little-endian: magic, key length and key, width and height, the
// summary's numbers, JPEG length, the JPEG's hash and the JPEG. A file of any
// other length was cut short or written over, and the hash catches the rest,
// since the decoder reads a spoiled JPEG without complaint.
fn read(path: &Path, key: &Key, size: (u32, u32)) -> Option<Kept> {
	let bytes = std::fs::read(path).ok()?;
	let mut at = Reader(&bytes);
	if at.take(MAGIC.len())? != MAGIC {
		return None;
	}
	let key_len = at.number()? as usize;
	if at.take(key_len)? != key.0 {
		return None;
	}
	if (at.number()?, at.number()?) != size {
		return None;
	}
	let summary = read_summary(&mut at)?;
	let jpeg_len = at.number()? as usize;
	let hash = u64::from_le_bytes(at.take(8)?.try_into().ok()?);
	let jpeg = at.take(jpeg_len)?;
	if !at.0.is_empty() || fnv(jpeg) != hash {
		return None;
	}
	let rgba = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
		.ok()?
		.into_rgba8();
	(rgba.dimensions() == size).then_some(Kept { rgba, summary })
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
	fn take(&mut self, len: usize) -> Option<&'a [u8]> {
		if len > self.0.len() {
			return None;
		}
		let (head, rest) = self.0.split_at(len);
		self.0 = rest;
		Some(head)
	}

	fn number(&mut self) -> Option<u32> {
		Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
	}

	fn float(&mut self) -> Option<f32> {
		self.number().map(f32::from_bits)
	}
}

fn summary_numbers(summary: &Summary) -> Vec<f32> {
	let mut numbers = vec![summary.luma_hi, summary.luma_lo, summary.luma_mean];
	numbers.extend(summary.spread.iter().flatten());
	numbers.extend([summary.alpha, summary.hue, summary.chroma, summary.opacity]);
	numbers
}

fn read_summary(at: &mut Reader) -> Option<Summary> {
	let (luma_hi, luma_lo, luma_mean) = (at.float()?, at.float()?, at.float()?);
	let mut spread = [[0.0; 3]; SPREAD];
	for value in spread.iter_mut().flatten() {
		*value = at.float()?;
	}
	Some(Summary {
		luma_hi,
		luma_lo,
		luma_mean,
		spread,
		alpha: at.float()?,
		hue: at.float()?,
		chroma: at.float()?,
		opacity: at.float()?,
	})
}

/// Keep `rgba` for `key`, then prune the folder back under `limit`. A picture
/// with any transparency is not kept, since a JPEG has no alpha.
pub fn store(
	dir: &Path,
	key: &Key,
	rgba: &image::RgbaImage,
	summary: &Summary,
	limit: u64,
) -> std::io::Result<bool> {
	if rgba.pixels().any(|px| px[3] != u8::MAX) || !crate::config::may_write(dir) {
		return Ok(false);
	}
	let mut jpeg = Vec::new();
	image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, QUALITY)
		.encode_image(rgba)
		.map_err(std::io::Error::other)?;
	let numbers = summary_numbers(summary);
	let mut bytes = Vec::with_capacity(jpeg.len() + key.0.len() + numbers.len() * 4 + 32);
	bytes.extend_from_slice(MAGIC);
	push_u32(&mut bytes, key.0.len())?;
	bytes.extend_from_slice(&key.0);
	bytes.extend_from_slice(&rgba.width().to_le_bytes());
	bytes.extend_from_slice(&rgba.height().to_le_bytes());
	for number in numbers {
		bytes.extend_from_slice(&number.to_bits().to_le_bytes());
	}
	push_u32(&mut bytes, jpeg.len())?;
	bytes.extend_from_slice(&fnv(&jpeg).to_le_bytes());
	bytes.extend_from_slice(&jpeg);

	std::fs::create_dir_all(dir)?;
	let name = key.name(rgba.dimensions());
	let nanos = SystemTime::now()
		.duration_since(SystemTime::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	let temp = dir.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()));
	let written = std::fs::File::create_new(&temp)
		.and_then(|mut file| file.write_all(&bytes))
		.and_then(|()| std::fs::rename(&temp, dir.join(&name)));
	if let Err(e) = written {
		let _ = std::fs::remove_file(&temp);
		return Err(e);
	}
	prune(dir, limit, SystemTime::now());
	Ok(true)
}

fn push_u32(bytes: &mut Vec<u8>, len: usize) -> std::io::Result<()> {
	let len = u32::try_from(len).map_err(std::io::Error::other)?;
	bytes.extend_from_slice(&len.to_le_bytes());
	Ok(())
}

// Oldest first, by last use, until the copies fit under `limit`. Another
// process may be pruning too, so a copy already gone counts as removed.
fn prune(dir: &Path, limit: u64, now: SystemTime) {
	let Ok(entries) = std::fs::read_dir(dir) else {
		return;
	};
	let mut copies = Vec::new();
	let mut total = 0u64;
	for entry in entries.flatten() {
		let Ok(meta) = entry.metadata() else {
			continue;
		};
		let name = entry.file_name();
		let name = name.to_string_lossy();
		if !meta.is_file() {
			continue;
		}
		let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
		if name.starts_with('.') && name.ends_with(".tmp") {
			if now.duration_since(used).unwrap_or_default() > ABANDONED {
				let _ = std::fs::remove_file(entry.path());
			}
			continue;
		}
		if !name.ends_with(&format!(".{EXT}")) {
			continue;
		}
		total += meta.len();
		copies.push((used, meta.len(), entry.path()));
	}
	copies.sort_by_key(|(used, ..)| *used);
	for (_, len, path) in copies {
		if total <= limit {
			break;
		}
		match std::fs::remove_file(&path) {
			Err(e) if e.kind() != std::io::ErrorKind::NotFound => {}
			_ => total -= len,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{ABANDONED, EXT, Key, LIMIT, find, prune, read, store};
	use std::path::PathBuf;
	use std::time::{Duration, SystemTime};

	fn folder(name: &str) -> PathBuf {
		let dir = crate::testdir::run_dir().join(format!("wpcache_{name}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		dir
	}

	// A smooth picture, the kind a blurred wallpaper is.
	fn picture((w, h): (u32, u32)) -> image::RgbaImage {
		image::RgbaImage::from_fn(w, h, |x, y| {
			image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 90, 255])
		})
	}

	fn key(name: &str) -> Key {
		let mut key = Key::default();
		key.push_bytes(name.as_bytes());
		key
	}

	fn keep(dir: &std::path::Path, key: &Key, size: (u32, u32)) {
		let rgba = picture(size);
		let summary = crate::autotheme::summarize(&rgba, 0.1);
		assert!(store(dir, key, &rgba, &summary, LIMIT).unwrap(), "{size:?}");
	}

	fn kept_size(dir: &std::path::Path, key: &Key, want: (u32, u32)) -> Option<(u32, u32)> {
		find(dir, key, want).map(|kept| kept.rgba.dimensions())
	}

	// Test ID: EryHHuh
	#[test]
	fn a_kept_copy_reads_back_as_stored() {
		let dir = folder("roundtrip");
		let key = key("a");
		let rgba = picture((300, 200));
		let summary = crate::autotheme::summarize(&rgba, 0.25);
		assert!(store(&dir, &key, &rgba, &summary, LIMIT).unwrap());
		let kept = find(&dir, &key, (300, 200)).expect("kept");
		assert_eq!(
			kept.summary, summary,
			"the summary is the original's, exactly"
		);
		assert_eq!(kept.rgba.dimensions(), (300, 200));
		let worst = rgba
			.pixels()
			.zip(kept.rgba.pixels())
			.flat_map(|(a, b)| (0..4).map(move |c| a[c].abs_diff(b[c])))
			.max()
			.unwrap();
		assert!(worst <= 4, "off by {worst} levels");
		// another key never reads it, even one that names the same file
		assert!(find(&dir, &self::key("b"), (300, 200)).is_none());
		// a picture with any transparency is not kept, since JPEG has none
		let mut clear = picture((40, 40));
		clear.put_pixel(3, 3, image::Rgba([0, 0, 0, 128]));
		assert!(!store(&dir, &self::key("c"), &clear, &summary, LIMIT).unwrap());
		assert!(find(&dir, &self::key("c"), (40, 40)).is_none());
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EryHHyF
	#[test]
	fn the_nearest_copy_within_five_percent_of_the_pixels_is_used() {
		let dir = folder("near");
		let key = key("near");
		// 500,000 pixels, 4.0% and 12.4% more
		for size in [(1000, 500), (1020, 510), (1060, 530)] {
			keep(&dir, &key, size);
		}
		assert_eq!(kept_size(&dir, &key, (1000, 500)), Some((1000, 500)));
		assert_eq!(kept_size(&dir, &key, (1020, 510)), Some((1020, 510)));
		assert_eq!(kept_size(&dir, &key, (1010, 505)), Some((1000, 500)));
		// as near to 1000x500 as to 1020x510: the larger of the two
		assert_eq!(kept_size(&dir, &key, (5101, 100)), Some((1020, 510)));
		// 4.9% under the smallest
		assert_eq!(kept_size(&dir, &key, (976, 488)), Some((1000, 500)));
		// 3.7% short of it, and 8.8%
		assert_eq!(kept_size(&dir, &key, (1080, 540)), Some((1060, 530)));
		assert_eq!(kept_size(&dir, &key, (1100, 560)), None);
		assert_eq!(kept_size(&dir, &key, (900, 450)), None);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EryHI1u
	#[test]
	fn the_cache_prunes_the_least_recently_used_first() {
		let dir = folder("prune");
		let now = SystemTime::now();
		let ago = |minutes: u64| now - Duration::from_mins(minutes);
		let plant = |name: &str, bytes: usize, at: SystemTime| {
			let path = dir.join(name);
			std::fs::write(&path, vec![0u8; bytes]).unwrap();
			std::fs::File::options()
				.append(true)
				.open(&path)
				.unwrap()
				.set_modified(at)
				.unwrap();
		};
		plant(&format!("old-1x1.{EXT}"), 400, ago(50));
		plant(&format!("used-1x1.{EXT}"), 400, ago(5));
		plant(&format!("middle-1x1.{EXT}"), 400, ago(20));
		plant(&format!("new-1x1.{EXT}"), 400, ago(1));
		// a write that died long ago, one that may still be going, and a file
		// that is none of ours
		plant(
			".dead.tmp",
			10_000,
			now - ABANDONED - Duration::from_mins(1),
		);
		plant(".busy.tmp", 10_000, ago(1));
		plant("notes.txt", 10_000, ago(500));
		prune(&dir, 1000, now);
		let mut left: Vec<String> = std::fs::read_dir(&dir)
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		left.sort();
		assert_eq!(
			left,
			[
				".busy.tmp".to_string(),
				format!("new-1x1.{EXT}"),
				"notes.txt".into(),
				format!("used-1x1.{EXT}"),
			]
		);
		// a copy that is read counts as used
		let key = key("lru");
		keep(&dir, &key, (64, 64));
		let name = dir.join(format!("{}64x64.{EXT}", key.prefix()));
		std::fs::File::options()
			.append(true)
			.open(&name)
			.unwrap()
			.set_modified(ago(90))
			.unwrap();
		assert!(find(&dir, &key, (64, 64)).is_some());
		let used = std::fs::metadata(&name).unwrap().modified().unwrap();
		assert!(used > ago(1), "{used:?}");
		let _ = std::fs::remove_dir_all(&dir);
	}

	// Test ID: EryHI5c
	#[test]
	fn a_broken_copy_is_ignored_and_replaced() {
		let dir = folder("broken");
		let key = key("broken");
		let size = (200, 100);
		keep(&dir, &key, size);
		let path = dir.join(key.name(size));
		let whole = std::fs::read(&path).unwrap();
		let mut damaged: Vec<(&str, Vec<u8>)> = vec![
			("cut short", whole[..whole.len() / 2].to_vec()),
			("empty", Vec::new()),
			("one byte more", [whole.as_slice(), &[0]].concat()),
			("garbage", vec![0x5a; whole.len()]),
		];
		// the JPEG itself spoiled, with the lengths still right
		let mut spoiled = whole.clone();
		let at = spoiled.len() - 40;
		spoiled[at..].fill(0);
		let tail = spoiled.len() - 2;
		spoiled[tail] = 0xff;
		damaged.push(("bad JPEG", spoiled));
		// another key's copy under this one's name, as a hash clash would leave
		let other = self::key("other");
		keep(&dir, &other, size);
		damaged.push((
			"another key",
			std::fs::read(dir.join(other.name(size))).unwrap(),
		));
		for (what, bytes) in damaged {
			std::fs::write(&path, &bytes).unwrap();
			assert!(read(&path, &key, size).is_none(), "{what}");
			assert!(find(&dir, &key, size).is_none(), "{what}");
			assert!(!path.exists(), "{what}: left in place");
			keep(&dir, &key, size);
			assert!(find(&dir, &key, size).is_some(), "{what}: not replaced");
		}
		// a damaged copy does not stop a good one near it being used
		std::fs::write(&path, b"x").unwrap();
		keep(&dir, &key, (202, 101));
		assert_eq!(kept_size(&dir, &key, size), Some((202, 101)));
		let _ = std::fs::remove_dir_all(&dir);
	}
}
