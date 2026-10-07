// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! BC1 for the wallpaper: half a byte a texel in graphics memory, against 4
//! plain. Each 4x4 block keeps 2 colors at 5:6:5 and picks one of 4 points on
//! the line between them per texel. Opaque pictures only, so the 3 color mode
//! with its transparent texel is never used. Encoded on the sRGB bytes, since
//! the texture is sRGB and the sampler decodes after it unpacks a block.

use std::sync::OnceLock;

/// Bytes per 4x4 block.
pub const BLOCK_BYTES: usize = 8;

/// The texture's size for a picture this big. A BC texture is whole blocks,
/// so the picture sits in its top left corner and the rest repeats its edge.
pub fn padded((w, h): (u32, u32)) -> (u32, u32) {
	(w.next_multiple_of(4), h.next_multiple_of(4))
}

/// How many bytes `encode` makes for a picture this big.
pub fn len_for((w, h): (u32, u32)) -> usize {
	w.div_ceil(4) as usize * h.div_ceil(4) as usize * BLOCK_BYTES
}

type Texel = [i32; 3];

/// Blocks in rows, left to right. The alpha is not read. About 0.1 s of
/// CPU at 2560x1440, so it is shared out over a few threads.
pub fn encode(rgba: &image::RgbaImage) -> Vec<u8> {
	let (w, h) = rgba.dimensions();
	let mut out = vec![0; len_for((w, h))];
	if out.is_empty() {
		return out;
	}
	let row_bytes = w.div_ceil(4) as usize * BLOCK_BYTES;
	let rows = h.div_ceil(4) as usize;
	let threads = std::thread::available_parallelism()
		.map_or(1, std::num::NonZero::get)
		.min(4);
	let band = rows.div_ceil(threads) * row_bytes;
	// a band whose thread would not start is done here after the rest
	let mut missed = Vec::new();
	std::thread::scope(|scope| {
		for (n, blocks) in out.chunks_mut(band).enumerate() {
			let first = n * band / row_bytes;
			let job = move || encode_rows(rgba, first, blocks);
			if std::thread::Builder::new()
				.name("bc1".into())
				.spawn_scoped(scope, job)
				.is_err()
			{
				missed.push(n);
			}
		}
	});
	for n in missed {
		if let Some(blocks) = out.chunks_mut(band).nth(n) {
			encode_rows(rgba, n * band / row_bytes, blocks);
		}
	}
	out
}

// Block rows from `first` on, as many as `out` holds. Texels past the
// picture repeat its edge, the way the sampler's clamp reads it.
fn encode_rows(rgba: &image::RgbaImage, first: usize, out: &mut [u8]) {
	let (w, h) = rgba.dimensions();
	let tables = tables();
	let across = w.div_ceil(4) as usize;
	let mut block = [[0; 3]; 16];
	for (n, bytes) in out.chunks_exact_mut(BLOCK_BYTES).enumerate() {
		let (bx, by) = ((n % across) as u32, (first + n / across) as u32);
		for (i, texel) in block.iter_mut().enumerate() {
			let x = (bx * 4 + i as u32 % 4).min(w - 1);
			let y = (by * 4 + i as u32 / 4).min(h - 1);
			let px = rgba.get_pixel(x, y);
			*texel = [i32::from(px[0]), i32::from(px[1]), i32::from(px[2])];
		}
		bytes.copy_from_slice(&encode_block(&block, tables));
	}
}

/// Unpack `encode`'s blocks for a picture `size` big, as a GPU without BC
/// support would need them. Opaque.
pub fn decode(blocks: &[u8], (w, h): (u32, u32)) -> Option<image::RgbaImage> {
	if blocks.len() != len_for((w, h)) {
		return None;
	}
	let mut out = image::RgbaImage::new(w, h);
	let across = w.div_ceil(4) as usize;
	for (n, block) in blocks.chunks_exact(BLOCK_BYTES).enumerate() {
		let c0 = u16::from_le_bytes([block[0], block[1]]);
		let c1 = u16::from_le_bytes([block[2], block[3]]);
		let picks = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
		let colors = palette(c0, c1);
		let (bx, by) = ((n % across) as u32 * 4, (n / across) as u32 * 4);
		for i in 0..16u32 {
			let (x, y) = (bx + i % 4, by + i / 4);
			if x < w && y < h {
				let c = colors[(picks >> (2 * i) & 3) as usize];
				out.put_pixel(x, y, image::Rgba([c[0] as u8, c[1] as u8, c[2] as u8, 255]));
			}
		}
	}
	Some(out)
}

fn expand5(v: i32) -> i32 {
	(v << 3) | (v >> 2)
}

fn expand6(v: i32) -> i32 {
	(v << 2) | (v >> 4)
}

fn unpack(c: u16) -> Texel {
	let c = i32::from(c);
	[expand5(c >> 11), expand6((c >> 5) & 63), expand5(c & 31)]
}

fn pack([r, g, b]: [i32; 3]) -> u16 {
	((r.clamp(0, 31) << 11) | (g.clamp(0, 63) << 5) | b.clamp(0, 31)) as u16
}

// What a decoder makes of two endpoints. Hardware rounds the thirds a little
// differently from one maker to the next, a level at most.
fn palette(c0: u16, c1: u16) -> [Texel; 4] {
	let (p0, p1) = (unpack(c0), unpack(c1));
	let mix = |a: i32, b: i32, n: i32| {
		if c0 > c1 {
			((3 - n) * a + n * b + 1) / 3
		} else {
			i32::midpoint(a, b)
		}
	};
	let mut colors = [p0, p1, [0; 3], [0; 3]];
	for c in 0..3 {
		colors[2][c] = mix(p0[c], p1[c], 1);
		colors[3][c] = if c0 > c1 { mix(p0[c], p1[c], 2) } else { 0 };
	}
	colors
}

// Each texel's nearest of the four colors, and the squared error in all.
// With equal endpoints the block is in the 3 color mode, where the last one
// is transparent, so every texel takes the first.
fn fit(block: &[Texel; 16], c0: u16, c1: u16) -> (u32, u32) {
	let colors = palette(c0, c1);
	let dist = |t: &Texel, k: usize| {
		let (r, g, b) = (
			t[0] - colors[k][0],
			t[1] - colors[k][1],
			t[2] - colors[k][2],
		);
		(r * r + g * g + b * b) as u32
	};
	if c0 <= c1 {
		return (0, block.iter().map(|t| dist(t, 0)).sum());
	}
	let mut picks = 0u32;
	let mut total = 0u32;
	for (i, texel) in block.iter().enumerate() {
		let mut best = (dist(texel, 0), 0);
		for k in 1..4 {
			let err = dist(texel, k);
			if err < best.0 {
				best = (err, k as u32);
			}
		}
		total += best.0;
		picks |= best.1 << (2 * i);
	}
	(picks, total)
}

#[derive(Clone, Copy)]
struct Encoded {
	c0: u16,
	c1: u16,
	picks: u32,
	err: u32,
}

impl Encoded {
	fn of(block: &[Texel; 16], ends: [[i32; 3]; 2]) -> Encoded {
		let (mut c0, mut c1) = (pack(ends[0]), pack(ends[1]));
		if c0 < c1 {
			std::mem::swap(&mut c0, &mut c1);
		}
		let (picks, err) = fit(block, c0, c1);
		Encoded { c0, c1, picks, err }
	}

	fn bytes(self) -> [u8; 8] {
		let mut out = [0; 8];
		out[..2].copy_from_slice(&self.c0.to_le_bytes());
		out[2..4].copy_from_slice(&self.c1.to_le_bytes());
		out[4..].copy_from_slice(&self.picks.to_le_bytes());
		out
	}
}

// Per channel, the endpoint pair whose first third lands nearest each byte
// value, for 5 and 6 bits. A flat block is then within a level of its color,
// where rounding the color itself to 5 bits is up to 4 off: on a slow
// gradient that was the difference between smooth and stepped.
struct Tables {
	five: [[u8; 2]; 256],
	six: [[u8; 2]; 256],
}

fn tables() -> &'static Tables {
	static TABLES: OnceLock<Tables> = OnceLock::new();
	TABLES.get_or_init(|| {
		let build = |bits: u32, expand: fn(i32) -> i32| {
			let top = (1 << bits) - 1;
			let mut table = [[0u8; 2]; 256];
			for (want, slot) in table.iter_mut().enumerate() {
				let want = want as i32;
				// nearest first, then the closer pair, which every decoder's
				// rounding agrees on best
				let mut best = (i32::MAX, i32::MAX);
				for e0 in 0..=top {
					for e1 in 0..=top {
						let (a, b) = (expand(e0), expand(e1));
						let got = (2 * a + b + 1) / 3;
						let rank = ((got - want).abs(), (a - b).abs());
						if rank < best {
							best = rank;
							*slot = [e0 as u8, e1 as u8];
						}
					}
				}
			}
			table
		};
		Tables {
			five: build(5, expand5),
			six: build(6, expand6),
		}
	})
}

// One color, nearly exact, through the tables.
fn solid(block: &[Texel; 16], color: Texel, tables: &Tables) -> Encoded {
	let [r, g, b] = color.map(|v| v.clamp(0, 255) as usize);
	let ends = [
		[
			i32::from(tables.five[r][0]),
			i32::from(tables.six[g][0]),
			i32::from(tables.five[b][0]),
		],
		[
			i32::from(tables.five[r][1]),
			i32::from(tables.six[g][1]),
			i32::from(tables.five[b][1]),
		],
	];
	Encoded::of(block, ends)
}

fn quantize(color: [f32; 3]) -> [i32; 3] {
	let q = |v: f32, top: f32| (v.clamp(0.0, 255.0) * top / 255.0).round() as i32;
	[q(color[0], 31.0), q(color[1], 63.0), q(color[2], 31.0)]
}

fn encode_block(block: &[Texel; 16], tables: &Tables) -> [u8; 8] {
	if block.iter().all(|texel| *texel == block[0]) {
		return solid(block, block[0], tables).bytes();
	}
	let mut mean = [0.0f32; 3];
	for texel in block {
		for c in 0..3 {
			mean[c] += texel[c] as f32 / 16.0;
		}
	}
	let flat = solid(block, mean.map(|v| v.round() as i32), tables);
	let mut best = along_the_axis(block, mean);
	// endpoints from the colors the texels picked, by least squares
	for _ in 0..2 {
		match refit(block, best) {
			Some(next) if next.err < best.err => best = next,
			_ => break,
		}
	}
	if flat.err <= best.err {
		return flat.bytes();
	}
	best.bytes()
}

// The block's main direction of color change, through its mean. The two ends
// are where the texels reach furthest along it.
fn along_the_axis(block: &[Texel; 16], mean: [f32; 3]) -> Encoded {
	let mut cov = [0.0f32; 6];
	for texel in block {
		let d = [0, 1, 2].map(|c| texel[c] as f32 - mean[c]);
		cov[0] += d[0] * d[0];
		cov[1] += d[0] * d[1];
		cov[2] += d[0] * d[2];
		cov[3] += d[1] * d[1];
		cov[4] += d[1] * d[2];
		cov[5] += d[2] * d[2];
	}
	let rows = [
		[cov[0], cov[1], cov[2]],
		[cov[1], cov[3], cov[4]],
		[cov[2], cov[4], cov[5]],
	];
	// power iteration from the row with the most spread
	let mut axis = rows[[cov[0], cov[3], cov[5]]
		.iter()
		.enumerate()
		.max_by(|a, b| a.1.total_cmp(b.1))
		.map_or(0, |(i, _)| i)];
	for _ in 0..6 {
		let next =
			[0, 1, 2].map(|r| rows[r][0] * axis[0] + rows[r][1] * axis[1] + rows[r][2] * axis[2]);
		let norm = next.iter().map(|v| v * v).sum::<f32>().sqrt();
		if norm < 1e-6 {
			break;
		}
		axis = next.map(|v| v / norm);
	}
	let norm = axis.iter().map(|v| v * v).sum::<f32>().sqrt();
	axis = if norm < 1e-6 {
		[0.577; 3]
	} else {
		axis.map(|v| v / norm)
	};
	let (mut lo, mut hi) = (f32::MAX, f32::MIN);
	for texel in block {
		let t: f32 = (0..3).map(|c| (texel[c] as f32 - mean[c]) * axis[c]).sum();
		lo = lo.min(t);
		hi = hi.max(t);
	}
	let at = |t: f32| [0, 1, 2].map(|c| mean[c] + axis[c] * t);
	Encoded::of(block, [quantize(at(hi)), quantize(at(lo))])
}

// The endpoints that best fit the colors the texels picked: each texel is a
// known blend of the two, so it is least squares in two unknowns a channel.
fn refit(block: &[Texel; 16], from: Encoded) -> Option<Encoded> {
	// thirds of the first endpoint in each of the four colors
	const SHARE: [f32; 4] = [3.0, 0.0, 2.0, 1.0];
	if from.c0 == from.c1 {
		return None;
	}
	let (mut aa, mut ab, mut bb) = (0.0f32, 0.0f32, 0.0f32);
	let mut ax = [0.0f32; 3];
	let mut bx = [0.0f32; 3];
	for (i, texel) in block.iter().enumerate() {
		let a = SHARE[(from.picks >> (2 * i) & 3) as usize] / 3.0;
		let b = 1.0 - a;
		aa += a * a;
		ab += a * b;
		bb += b * b;
		for c in 0..3 {
			ax[c] += a * texel[c] as f32;
			bx[c] += b * texel[c] as f32;
		}
	}
	let det = aa * bb - ab * ab;
	if det.abs() < 1e-6 {
		return None;
	}
	let first = [0, 1, 2].map(|c| (ax[c] * bb - bx[c] * ab) / det);
	let second = [0, 1, 2].map(|c| (bx[c] * aa - ax[c] * ab) / det);
	Some(Encoded::of(block, [quantize(first), quantize(second)]))
}

#[cfg(test)]
mod tests {
	use super::{decode, encode, len_for, padded};

	fn worst(a: &image::RgbaImage, b: &image::RgbaImage) -> u8 {
		a.pixels()
			.zip(b.pixels())
			.flat_map(|(p, q)| (0..3).map(move |c| p[c].abs_diff(q[c])))
			.max()
			.unwrap_or(0)
	}

	// Rounding a flat color to 5:6:5 was up to 4 levels off. Through the
	// tables every byte value comes back within one.
	// Test ID: Erz0m8u
	#[test]
	fn a_flat_color_comes_back_within_a_level() {
		for value in 0..=255u8 {
			let shade = [value, 255 - value, value / 3 + 40];
			let rgba = image::RgbaImage::from_pixel(
				8,
				8,
				image::Rgba([shade[0], shade[1], shade[2], 255]),
			);
			let back = decode(&encode(&rgba), (8, 8)).unwrap();
			assert!(
				worst(&rgba, &back) <= 1,
				"{shade:?}: {}",
				worst(&rgba, &back)
			);
		}
	}

	// A slow sky gradient, where BC1 bands if anywhere: each block a level or
	// two of change, and it comes back within two.
	// Test ID: Erz0mAu
	#[test]
	fn a_slow_gradient_comes_back_within_two_levels() {
		let rgba = image::RgbaImage::from_fn(256, 96, |x, y| {
			image::Rgba([
				(90 + x / 6) as u8,
				(140 + x / 8 + y / 12) as u8,
				(200 + y / 4) as u8,
				255,
			])
		});
		let back = decode(&encode(&rgba), (256, 96)).unwrap();
		assert!(worst(&rgba, &back) <= 2, "{}", worst(&rgba, &back));
	}

	// Test ID: Erz0mCs
	#[test]
	fn sizes_round_up_to_whole_blocks() {
		assert_eq!(padded((1, 1)), (4, 4));
		assert_eq!(padded((2560, 1440)), (2560, 1440));
		assert_eq!(padded((1917, 993)), (1920, 996));
		assert_eq!(len_for((1917, 993)), 480 * 249 * 8);
		// the texels past the picture repeat its last row and column
		let rgba = image::RgbaImage::from_fn(7, 5, |x, y| {
			image::Rgba([(x * 30) as u8, (y * 40) as u8, 9, 255])
		});
		let blocks = encode(&rgba);
		assert_eq!(blocks.len(), len_for((7, 5)));
		let wide = image::RgbaImage::from_fn(8, 8, |x, y| *rgba.get_pixel(x.min(6), y.min(4)));
		assert_eq!(encode(&wide), blocks);
		assert!(decode(&blocks, (7, 5)).is_some());
		assert!(decode(&blocks, (8, 9)).is_none());
		assert!(encode(&image::RgbaImage::new(0, 0)).is_empty());
	}
}
