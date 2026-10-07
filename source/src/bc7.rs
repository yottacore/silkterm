// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! BC7 for the wallpaper: a byte a texel in graphics memory, twice BC1 and a
//! quarter of plain. Mostly mode 6: each 4x4 block keeps 2 colors at 7 bits
//! plus a shared low bit and picks one of 16 points between them per texel,
//! where BC1 has 4 points between 5:6:5 ends, so a slow gradient no longer
//! steps at the block edges. Opaque only, so both alpha ends are 255, the low
//! bit is 1 and every color end is odd. That leaves 0 out of reach, and a
//! black sky came back a level up all over, so a block that touches 0 also
//! tries mode 5: 7 bit ends that reach 0 and 255, an alpha of its own, and 4
//! points. The other modes split a block in 2 or 3 for sharp edges, which a
//! wallpaper has few of, at several times the code and the encode time.
//! Unlike BC1, every decoder agrees to the bit.

use crate::bc1::Texel;

/// Bytes per 4x4 block.
pub const BLOCK_BYTES: usize = 16;

// Where each point sits between the two ends, in 64ths. Fixed by the format.
const SIXTEEN: [i32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];
const FOUR: [i32; 4] = [0, 21, 43, 64];

// Mode 6's point nearest each 64th.
const NEAREST: [u8; 65] = {
	let mut table = [0u8; 65];
	let mut t = 0;
	while t < 65 {
		let mut best = 0;
		let mut k = 1;
		while k < 16 {
			if (SIXTEEN[k] - t).abs() < (SIXTEEN[best] - t).abs() {
				best = k;
			}
			k += 1;
		}
		table[t as usize] = best as u8;
		t += 1;
	}
	table
};

/// How many bytes `encode` makes for a picture this big.
pub fn len_for(size: (u32, u32)) -> usize {
	crate::bc1::blocks_in(size) * BLOCK_BYTES
}

/// Blocks in rows, left to right, the same walk as BC1. The alpha is not
/// read.
pub fn encode(rgba: &image::RgbaImage) -> Vec<u8> {
	crate::bc1::encode_blocks(rgba, BLOCK_BYTES, &|block, out| {
		out.copy_from_slice(&encode_block(block));
	})
}

/// Unpack `encode`'s blocks for a picture `size` big. None for the wrong
/// length, or a block in a mode this never writes.
pub fn decode(blocks: &[u8], (w, h): (u32, u32)) -> Option<image::RgbaImage> {
	if blocks.len() != len_for((w, h)) {
		return None;
	}
	let mut out = image::RgbaImage::new(w, h);
	let across = w.div_ceil(4) as usize;
	for (n, block) in blocks.chunks_exact(BLOCK_BYTES).enumerate() {
		let bits = u128::from_le_bytes(block.try_into().ok()?);
		let field = |at: u32, len: u32| ((bits >> at) & ((1 << len) - 1)) as i32;
		// the mode is the number of zero bits before the first one; mode 5's
		// next 2 bits turn a channel round, which this never does
		let texels: [[i32; 4]; 16] = if bits & 0x7f == 0x40 {
			let low = [field(63, 1), field(64, 1)];
			let ends = [0, 1]
				.map(|e| [0, 1, 2, 3].map(|c| field(7 + 14 * c + 7 * e, 7) << 1 | low[e as usize]));
			std::array::from_fn(|i| {
				let pick = if i == 0 {
					field(65, 3)
				} else {
					field(64 + 4 * i as u32, 4)
				};
				[0, 1, 2, 3].map(|c| mix(ends[0][c], ends[1][c], SIXTEEN[pick as usize]))
			})
		} else if bits & 0xff == 0x20 {
			let ends =
				[0, 1].map(|e| [0, 1, 2].map(|c| Mode::Five.expand(field(8 + 14 * c + 7 * e, 7))));
			let alpha = [field(50, 8), field(58, 8)];
			std::array::from_fn(|i| {
				let at = |from: u32| {
					if i == 0 {
						field(from, 1)
					} else {
						field(from - 1 + 2 * i as u32, 2)
					}
				};
				let (pick, alpha_pick) = (FOUR[at(66) as usize], FOUR[at(97) as usize]);
				let [r, g, b] = [0, 1, 2].map(|c| mix(ends[0][c], ends[1][c], pick));
				[r, g, b, mix(alpha[0], alpha[1], alpha_pick)]
			})
		} else {
			return None;
		};
		let (bx, by) = ((n % across) as u32 * 4, (n / across) as u32 * 4);
		for (i, texel) in (0u32..).zip(texels) {
			let (x, y) = (bx + i % 4, by + i / 4);
			if x < w && y < h {
				out.put_pixel(x, y, image::Rgba(texel.map(|v| v as u8)));
			}
		}
	}
	Some(out)
}

fn mix(a: i32, b: i32, weight: i32) -> i32 {
	((64 - weight) * a + weight * b + 32) >> 6
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
	Five,
	Six,
}

impl Mode {
	fn weights(self) -> &'static [i32] {
		match self {
			Mode::Five => &FOUR,
			Mode::Six => &SIXTEEN,
		}
	}

	// A stored 7 bit end as the decoder reads it.
	fn expand(self, q: i32) -> i32 {
		match self {
			Mode::Five => q << 1 | q >> 6,
			Mode::Six => q << 1 | 1,
		}
	}

	// The stored end that reads back nearest `v`.
	fn end_of(self, v: f32) -> i32 {
		let guess = (v / 2.0).round() as i32;
		let off = |q: i32| (self.expand(q) as f32 - v).abs();
		[guess - 1, guess, guess + 1]
			.map(|q| q.clamp(0, 127))
			.into_iter()
			.min_by(|a, b| off(*a).total_cmp(&off(*b)))
			.unwrap_or(0)
	}

	// The point nearest `t` 64ths of the way.
	fn nearest(self, t: f32) -> usize {
		let t = t.round().clamp(0.0, 64.0);
		match self {
			Mode::Six => usize::from(NEAREST[t as usize]),
			Mode::Five => (0..4)
				.min_by_key(|&k| (FOUR[k] - t as i32).abs())
				.unwrap_or(0),
		}
	}
}

#[derive(Clone, Copy)]
struct Encoded {
	mode: Mode,
	ends: [[i32; 3]; 2],
	picks: [u8; 16],
	err: u32,
}

impl Encoded {
	// Each texel's nearest point, by where it falls along the line and then
	// the point either side, since rounding moves each point off the line by
	// up to half a level.
	fn of(mode: Mode, block: &[Texel; 16], ends: [[i32; 3]; 2]) -> Encoded {
		let weights = mode.weights();
		let top = weights.len() - 1;
		let (a, b) = (
			ends[0].map(|q| mode.expand(q)),
			ends[1].map(|q| mode.expand(q)),
		);
		let points: [[i32; 3]; 16] =
			std::array::from_fn(|k| [0, 1, 2].map(|c| mix(a[c], b[c], weights[k.min(top)])));
		let d = [0, 1, 2].map(|c| (b[c] - a[c]) as f32);
		let len = d.iter().map(|v| v * v).sum::<f32>();
		let mut picks = [0u8; 16];
		let mut err = 0;
		for (texel, pick) in block.iter().zip(&mut picks) {
			let dist = |k: usize| {
				(0..3)
					.map(|c| {
						let off = texel[c] - points[k][c];
						(off * off) as u32
					})
					.sum::<u32>()
			};
			let along = if len > 0.0 {
				(0..3).map(|c| (texel[c] - a[c]) as f32 * d[c]).sum::<f32>() / len
			} else {
				0.0
			};
			let near = mode.nearest(along * 64.0);
			let mut best = (dist(near), near);
			for k in [near.saturating_sub(1), (near + 1).min(top)] {
				let e = dist(k);
				if e < best.0 {
					best = (e, k);
				}
			}
			*pick = best.1 as u8;
			err += best.0;
		}
		Encoded {
			mode,
			ends,
			picks,
			err,
		}
	}

	// The first texel's pick is a bit short, so it must be in the lower half.
	// Swapping the ends and mirroring every pick draws the same.
	fn bytes(mut self) -> [u8; 16] {
		let top = (self.mode.weights().len() - 1) as u8;
		if self.picks[0] > top / 2 {
			self.ends.swap(0, 1);
			for pick in &mut self.picks {
				*pick = top - *pick;
			}
		}
		let mut bits: u128 = 0;
		let mut at = 0;
		let mut put = |value: u128, len: u32| {
			bits |= value << at;
			at += len;
		};
		let pick_bits = match self.mode {
			Mode::Five => {
				put(1 << 5, 6);
				put(0, 2);
				2
			}
			Mode::Six => {
				put(1 << 6, 7);
				4
			}
		};
		for c in 0..3 {
			for end in &self.ends {
				put(end[c] as u128, 7);
			}
		}
		match self.mode {
			Mode::Five => {
				put(255, 8);
				put(255, 8);
			}
			// 127 with the low bit set is 255
			Mode::Six => {
				put(127, 7);
				put(127, 7);
				put(1, 1);
				put(1, 1);
			}
		}
		put(u128::from(self.picks[0]), pick_bits - 1);
		for pick in &self.picks[1..] {
			put(u128::from(*pick), pick_bits);
		}
		// mode 5's alpha picks follow, all the first end: zeros
		bits.to_le_bytes()
	}
}

// Mode 6 ends for one color, as near as odd ends get: an odd value is both
// ends, and an even one sits midway between the odd values either side.
fn flat_ends(color: Texel) -> [[i32; 3]; 2] {
	let pair = |v: i32| match v.clamp(0, 255) {
		0 => (0, 0),
		v if v % 2 == 1 => ((v - 1) / 2, (v - 1) / 2),
		v => (v / 2 - 1, v / 2),
	};
	let [r, g, b] = color.map(pair);
	[[r.0, g.0, b.0], [r.1, g.1, b.1]]
}

fn encode_block(block: &[Texel; 16]) -> [u8; 16] {
	let touches_zero = block.iter().any(|texel| texel.contains(&0));
	if block.iter().all(|texel| *texel == block[0]) {
		let six = Encoded::of(Mode::Six, block, flat_ends(block[0]));
		if !touches_zero {
			return six.bytes();
		}
		let q = block[0].map(|v| Mode::Five.end_of(v as f32));
		let five = Encoded::of(Mode::Five, block, [q, q]);
		return if five.err < six.err { five } else { six }.bytes();
	}
	let mut mean = [0.0f32; 3];
	for texel in block {
		for c in 0..3 {
			mean[c] += texel[c] as f32 / 16.0;
		}
	}
	let ends = crate::bc1::axis_ends(block, mean);
	let flat = Encoded::of(Mode::Six, block, flat_ends(mean.map(|v| v.round() as i32)));
	let mut best = along(Mode::Six, block, ends);
	if flat.err <= best.err {
		best = flat;
	}
	// mode 5's 4 points only beat mode 6's 16 over a few levels, and a
	// starry sky touches 0 in most blocks, so the rest skip it
	let spread = (0..3)
		.map(|c| {
			let (lo, hi) = block.iter().fold((255, 0), |(lo, hi), texel| {
				(texel[c].min(lo), texel[c].max(hi))
			});
			hi - lo
		})
		.max()
		.unwrap_or(0);
	if touches_zero && best.err > 0 && spread <= 12 {
		let five = along(Mode::Five, block, ends);
		if five.err < best.err {
			best = five;
		}
	}
	best.bytes()
}

// The line through the block's ends, then ends from the points the texels
// picked, by least squares.
fn along(mode: Mode, block: &[Texel; 16], [hi, lo]: [[f32; 3]; 2]) -> Encoded {
	let mut best = Encoded::of(
		mode,
		block,
		[hi.map(|v| mode.end_of(v)), lo.map(|v| mode.end_of(v))],
	);
	for _ in 0..2 {
		match refit(block, best) {
			Some(next) if next.err < best.err => best = next,
			_ => break,
		}
	}
	best
}

// Each texel is a known blend of the two ends, so the ends that fit best are
// least squares in two unknowns a channel.
fn refit(block: &[Texel; 16], from: Encoded) -> Option<Encoded> {
	let weights = from.mode.weights();
	let (mut aa, mut ab, mut bb) = (0.0f32, 0.0f32, 0.0f32);
	let mut ax = [0.0f32; 3];
	let mut bx = [0.0f32; 3];
	for (texel, pick) in block.iter().zip(from.picks) {
		let b = weights[usize::from(pick)] as f32 / 64.0;
		let a = 1.0 - b;
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
	let end = |v: f32| from.mode.end_of(v);
	let first = [0, 1, 2].map(|c| end((ax[c] * bb - bx[c] * ab) / det));
	let second = [0, 1, 2].map(|c| end((bx[c] * aa - ax[c] * ab) / det));
	Some(Encoded::of(from.mode, block, [first, second]))
}

#[cfg(test)]
mod tests {
	use super::{decode, encode, len_for};

	fn worst(a: &image::RgbaImage, b: &image::RgbaImage) -> u8 {
		a.pixels()
			.zip(b.pixels())
			.flat_map(|(p, q)| (0..4).map(move |c| p[c].abs_diff(q[c])))
			.max()
			.unwrap_or(0)
	}

	fn mean(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
		let sum: u64 = a
			.pixels()
			.zip(b.pixels())
			.flat_map(|(p, q)| (0..3).map(move |c| u64::from(p[c].abs_diff(q[c]))))
			.sum();
		sum as f64 / (3 * a.width() * a.height()) as f64
	}

	// Every byte value comes back as it was but 0, which odd ends cannot
	// reach, and the alpha stays opaque.
	// Test ID: Es1eEgS
	#[test]
	fn a_flat_color_comes_back_exact() {
		for value in 0..=255u8 {
			let shade = [value, 255 - value, value / 3 + 40];
			let rgba = image::RgbaImage::from_pixel(
				8,
				8,
				image::Rgba([shade[0], shade[1], shade[2], 255]),
			);
			let back = decode(&encode(&rgba), (8, 8)).unwrap();
			let off = worst(&rgba, &back);
			assert!(
				off == 0 || (off == 1 && shade.contains(&0)),
				"{shade:?}: {off}"
			);
		}
	}

	// The sky gradient BC1 comes back within 2 levels of, with its steps at
	// the block edges. This is what BC7 is for: within a level, and well
	// under BC1 on average.
	// Test ID: Es1eEgT
	#[test]
	fn a_slow_gradient_comes_back_within_a_level() {
		let rgba = image::RgbaImage::from_fn(256, 96, |x, y| {
			image::Rgba([
				(90 + x / 6) as u8,
				(140 + x / 8 + y / 12) as u8,
				(200 + y / 4) as u8,
				255,
			])
		});
		let back = decode(&encode(&rgba), (256, 96)).unwrap();
		assert!(worst(&rgba, &back) <= 1, "{}", worst(&rgba, &back));
		let bc1 = crate::bc1::decode(&crate::bc1::encode(&rgba), (256, 96)).unwrap();
		let (ours, theirs) = (mean(&rgba, &back), mean(&rgba, &bc1));
		assert!(ours * 2.0 < theirs, "BC7 {ours:.3}, BC1 {theirs:.3}");
	}

	// Noise is the worst case for one line a block. Still closer than BC1.
	// Test ID: Es1eEgU
	#[test]
	fn busy_detail_is_no_worse_than_bc1() {
		let mut seed = 0x2545_f491_u32;
		let mut next = move || {
			seed ^= seed << 13;
			seed ^= seed >> 17;
			seed ^= seed << 5;
			(seed >> 24) as u8
		};
		let rgba =
			image::RgbaImage::from_fn(64, 64, |_, _| image::Rgba([next(), next(), next(), 255]));
		let back = decode(&encode(&rgba), (64, 64)).unwrap();
		let bc1 = crate::bc1::decode(&crate::bc1::encode(&rgba), (64, 64)).unwrap();
		let (ours, theirs) = (mean(&rgba, &back), mean(&rgba, &bc1));
		assert!(ours < theirs, "BC7 {ours:.3}, BC1 {theirs:.3}");
		assert!(back.pixels().all(|px| px[3] == 255));
	}

	// Mode 6's ends are odd, so black came back a level up all over a dark
	// sky. A block that touches 0 may take mode 5, where black is black.
	// Test ID: Es1gYa7
	#[test]
	fn black_stays_black() {
		let mode = |blocks: &[u8]| match blocks[0] & 0x7f {
			0x40 => 6,
			0x20 => 5,
			other => panic!("mode byte {other:#x}"),
		};
		let black = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]));
		let blocks = encode(&black);
		assert_eq!(mode(&blocks), 5);
		assert_eq!(decode(&blocks, (4, 4)).unwrap(), black);
		// black speckled with a dark gray, every pick placed where the decoder
		// reads it
		let speckled = image::RgbaImage::from_fn(4, 4, |x, y| {
			let lit = (x * 7 + y * 3) % 5 < 2;
			image::Rgba(if lit { [8, 12, 8, 255] } else { [0, 0, 0, 255] })
		});
		let blocks = encode(&speckled);
		assert_eq!(mode(&blocks), 5);
		assert_eq!(decode(&blocks, (4, 4)).unwrap(), speckled);
		// a dark gradient into black takes whichever is closer, and the same a
		// level up never tries mode 5
		for lift in [0, 1] {
			let rgba = image::RgbaImage::from_fn(4, 4, |x, y| {
				let v = (lift + x * 3 + y) as u8;
				image::Rgba([v, v / 2, v + 4, 255])
			});
			let blocks = encode(&rgba);
			if lift > 0 {
				assert_eq!(mode(&blocks), 6, "no 0, no mode 5");
			}
			let back = decode(&blocks, (4, 4)).unwrap();
			assert!(
				worst(&rgba, &back) <= 1,
				"lift {lift}: {}",
				worst(&rgba, &back)
			);
		}
	}

	// Test ID: Es1eEgV
	#[test]
	fn sizes_round_up_to_whole_blocks_and_only_mode_6_reads() {
		assert_eq!(len_for((1917, 993)), 480 * 249 * 16);
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
		// a BC1 copy is half the length, and a block of zeros is no mode at all
		assert!(decode(&crate::bc1::encode(&wide), (8, 8)).is_none());
		assert!(decode(&[0; 64], (8, 8)).is_none());
	}
}
