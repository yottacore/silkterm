// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

// A small deterministic fuzzer, shared by the targets that live beside the code
// they hammer (search for `mod fuzz` inside a module's tests).
//
// Not coverage-guided, and that is a choice rather than a limitation: every
// untrusted surface here is grammar-shaped - escape sequences, config lines,
// URLs - and a generator that knows the grammar reaches deep states in a few
// hundred cases where bit flips need millions. Bit flips still run, over the
// generated cases and over the saved corpus, because they find the edges a
// well-formed generator never emits.
//
// Every case is a pure function of one u64 seed, so a failure needs nothing
// saved to reproduce: SILK_FUZZ_SEED=<n> cargo test <name> runs that case alone.

use std::path::PathBuf;
use std::time::{Duration, Instant};

// xorshift64*. Small, fast, and good enough to pick between branches; nothing
// here needs a real distribution.
#[derive(Debug)]
pub struct Rng(u64);

impl Rng {
	pub fn new(seed: u64) -> Self {
		// 0 is a fixed point of xorshift, and seed 0 is the first case we run.
		Self(seed ^ 0x9e37_79b9_7f4a_7c15)
	}

	pub fn next_u64(&mut self) -> u64 {
		self.0 ^= self.0 >> 12;
		self.0 ^= self.0 << 25;
		self.0 ^= self.0 >> 27;
		self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
	}

	pub fn below(&mut self, n: usize) -> usize {
		if n == 0 {
			0
		} else {
			(self.next_u64() % n as u64) as usize
		}
	}

	// 1 in n.
	pub fn chance(&mut self, n: usize) -> bool {
		self.below(n.max(1)) == 0
	}

	pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
		&xs[self.below(xs.len())]
	}

	pub fn byte(&mut self) -> u8 {
		self.next_u64() as u8
	}
}

// The seeds every target runs whatever the clock says. A count taken from the
// budget alone moves with how busy the box is, and config-rating, which checks
// a whole launch per case, sat right on the old floor of 10 and went red on a
// loaded box while covering exactly as much as it does on an idle one. A fixed
// minimum is the same coverage either way, and a target that gets slow shows up
// as a slow test rather than a flaky one.
const MIN_CASES: u64 = 10;

// How long one target gets. A plain `cargo test` wants to stay quick, so the
// default is a fraction of a second; the pipeline sets SILK_FUZZ_SECS for a real
// soak. Seeds run in order from 0, so a longer budget only ever adds cases -
// what a short run covered, a long one covers too.
fn budget() -> Duration {
	match std::env::var("SILK_FUZZ_SECS")
		.ok()
		.and_then(|v| v.parse::<f64>().ok())
	{
		Some(secs) if secs > 0.0 => Duration::from_secs_f64(secs.min(3600.0)),
		_ => Duration::from_millis(250),
	}
}

fn one_seed() -> Option<u64> {
	std::env::var("SILK_FUZZ_SEED")
		.ok()
		.and_then(|v| v.parse().ok())
}

// Run `case` over seeds: MIN_CASES of them at least, then on while the budget
// holds out. A panic is caught so the
// report names the seed that caused it, which is the whole reproduction recipe.
pub fn soak(name: &str, mut case: impl FnMut(u64)) {
	if let Some(seed) = one_seed() {
		case(seed);
		return;
	}
	let deadline = Instant::now() + budget();
	let mut seed = 0u64;
	while seed < MIN_CASES || Instant::now() < deadline {
		let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| case(seed)));
		assert!(
			outcome.is_ok(),
			"fuzz target '{name}' failed on seed {seed} - reproduce with \
			 SILK_FUZZ_SEED={seed} cargo test {name}"
		);
		seed += 1;
	}
	// Visible with --nocapture, which is how the pipeline runs the soak: a target
	// that has quietly become too slow to cover anything shows up as a small
	// number rather than as a pass.
	println!("fuzz {name}: {seed} cases");
}

fn corpus_dir(target: &str) -> PathBuf {
	PathBuf::from(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../cicd/tests/fuzz-corpus"
	))
	.join(target)
}

// Saved cases for one target: anything a fuzz run once broke, kept so it is
// replayed on every run afterwards. An empty or missing directory is normal.
pub fn corpus(target: &str) -> Vec<Vec<u8>> {
	let Ok(entries) = std::fs::read_dir(corpus_dir(target)) else {
		return Vec::new();
	};
	let mut out: Vec<_> = entries
		.flatten()
		.filter(|e| e.path().is_file())
		.filter_map(|e| std::fs::read(e.path()).ok())
		.collect();
	out.sort();
	out
}

// Bytes that have broken parsers before: boundaries, sign flips, and the C0/C1
// controls a text generator is least likely to place next to each other.
#[rustfmt::skip]
const NASTY: [u8; 16] = [
	0x00, 0x07, 0x08, 0x0a, 0x0d, 0x1b, 0x7f, 0x80,
	0x90, 0x9b, 0x9c, 0xc0, 0xed, 0xf5, 0xfe, 0xff,
];

// Chew on one case. Deliberately crude - the point is to reach shapes the
// generators cannot produce, not to be a good mutator.
pub fn mutate(rng: &mut Rng, parent: &[u8]) -> Vec<u8> {
	let mut out = parent.to_vec();
	for _ in 0..=rng.below(6) {
		if out.is_empty() {
			out.push(rng.byte());
			continue;
		}
		let at = rng.below(out.len());
		match rng.below(6) {
			0 => out[at] = rng.byte(),
			1 => out[at] = *rng.pick(&NASTY),
			2 => out[at] ^= 1 << rng.below(8),
			3 => {
				out.remove(at);
			}
			4 => out.insert(at, rng.byte()),
			// Duplicate a run: how a parser meets its own state twice over.
			_ => {
				let len = 1 + rng.below((out.len() - at).min(64));
				let run = out[at..at + len].to_vec();
				out.splice(at..at, run);
			}
		}
		out.truncate(1 << 16);
	}
	out
}

// One case's input: either freshly generated, or a saved case chewed on. With
// no corpus every case is generated, which is the normal state.
pub fn input(
	rng: &mut Rng,
	corpus: &[Vec<u8>],
	generate: impl FnOnce(&mut Rng) -> Vec<u8>,
) -> Vec<u8> {
	if corpus.is_empty() || rng.chance(2) {
		return generate(rng);
	}
	let parent = corpus[rng.below(corpus.len())].clone();
	mutate(rng, &parent)
}

// Text with the awkward parts of Unicode in it, for anything that takes a string
// rather than bytes. Length is short on purpose: most parser bugs sit within a
// few characters of a state change, and a long case only makes a failure harder
// to read.
pub fn text(rng: &mut Rng) -> String {
	#[rustfmt::skip]
	const PIECES: [&str; 22] = [
		"", " ", "\t", "\n", "\r\n", "a", "Z", "0", "-", ".", "/", "\\", ":",
		"..", "~", "%20", "\u{7f}", "\u{200b}", "\u{301}", "\u{fffd}",
		"\u{1f600}", "中",
	];
	let mut out = String::new();
	for _ in 0..rng.below(24) {
		out.push_str(rng.pick(&PIECES));
	}
	out
}

// A stream of the kind a program on the other end of a pty sends: printable
// runs with escape sequences threaded through them. Weighted toward the
// sequences that do something rather than the ones a parser discards, and
// deliberately willing to emit malformed ones - an unterminated OSC, a CSI with
// forty parameters, a truncated UTF-8 character.
pub fn vt_stream(rng: &mut Rng) -> Vec<u8> {
	let mut out = Vec::new();
	for _ in 0..=rng.below(40) {
		match rng.below(12) {
			0..=3 => out.extend_from_slice(text(rng).as_bytes()),
			4 => out.push(*rng.pick(&C0)),
			5 | 6 => csi(rng, &mut out),
			7 | 8 => osc(rng, &mut out),
			9 => {
				out.push(0x1b);
				out.push(*rng.pick(&ESC_FINALS));
			}
			10 => string_sequence(rng, &mut out),
			// Malformed UTF-8: truncated, overlong, a surrogate, past U+10FFFF.
			_ => out.extend_from_slice(rng.pick(&BAD_UTF8)),
		}
	}
	out
}

const C0: [u8; 10] = [0x00, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f];
const ESC_FINALS: [u8; 12] = [
	b'7', b'8', b'c', b'D', b'E', b'H', b'M', b'=', b'>', b'(', b')', b'#',
];

#[rustfmt::skip]
const BAD_UTF8: [&[u8]; 6] = [
	&[0xc3], &[0xe4, 0xb8], &[0xc0, 0xaf], &[0xed, 0xa0, 0x80],
	&[0xf5, 0x80, 0x80, 0x80], &[0x80],
];

// The finals that move the cursor, edit lines, change modes or ask a question -
// the ones with state behind them.
#[rustfmt::skip]
const CSI_FINALS: [u8; 24] = [
	b'@', b'A', b'B', b'C', b'D', b'G', b'H', b'J', b'K', b'L', b'M', b'P',
	b'S', b'T', b'X', b'c', b'd', b'h', b'l', b'm', b'n', b'q', b'r', b't',
];

// Numbers a parser is most likely to mishandle: the edges of every width it
// might use internally, and enough digits to overflow all of them.
#[rustfmt::skip]
const NUMBERS: [&str; 14] = [
	"", "0", "1", "2", "7", "38", "48", "255", "256", "65535", "2147483648",
	"4294967296", "18446744073709551616", "999999999999999999999999999999",
];

fn csi(rng: &mut Rng, out: &mut Vec<u8>) {
	out.extend_from_slice(if rng.chance(8) { b"\x9b" } else { b"\x1b[" });
	if rng.chance(3) {
		out.push(*rng.pick(b"?<=>"));
	}
	let params = if rng.chance(20) { 40 } else { 5 };
	for i in 0..rng.below(params) {
		if i > 0 {
			out.push(if rng.chance(4) { b':' } else { b';' });
		}
		out.extend_from_slice(rng.pick(&NUMBERS).as_bytes());
	}
	if rng.chance(6) {
		out.push(*rng.pick(b" !\"$'*"));
	}
	// Mostly a final that does something; sometimes any byte in the legal range,
	// to reach the arms nothing above names.
	if rng.chance(4) {
		out.push(0x40 + rng.byte() % 0x3f);
	} else {
		out.push(*rng.pick(&CSI_FINALS));
	}
}

// The OSC numbers that carry meaning here: the titles, the color queries, the
// two ways a shell reports where it is, hyperlinks, and the clipboard.
#[rustfmt::skip]
const OSC_NUMBERS: [&str; 18] = [
	"0", "1", "2", "4", "7", "8", "9", "10", "11", "12", "52", "104", "110",
	"111", "112", "133", "777", "99999",
];

fn osc(rng: &mut Rng, out: &mut Vec<u8>) {
	out.extend_from_slice(if rng.chance(8) { b"\x9d" } else { b"\x1b]" });
	out.extend_from_slice(rng.pick(&OSC_NUMBERS).as_bytes());
	for _ in 0..=rng.below(3) {
		out.push(b';');
		if rng.chance(6) {
			out.push(b'?');
		}
		out.extend_from_slice(text(rng).as_bytes());
	}
	// An unterminated string is the case worth reaching: the parser has to hold
	// it open without holding it forever.
	match rng.below(8) {
		0 => {}
		1 => out.push(0x9c),
		2..=3 => out.extend_from_slice(b"\x1b\\"),
		_ => out.push(0x07),
	}
}

// DCS, APC, PM and SOS: strings the terminal is meant to swallow whole.
fn string_sequence(rng: &mut Rng, out: &mut Vec<u8>) {
	out.push(0x1b);
	out.push(*rng.pick(b"P_^X"));
	out.extend_from_slice(text(rng).as_bytes());
	if !rng.chance(6) {
		out.extend_from_slice(b"\x1b\\");
	}
}

// Every sequence that asks the terminal a question, so each case exercises the
// reply path as well as the parse. Appended after the generated stream, which
// has by then left the terminal in whatever state it managed to reach.
pub const VT_PROBES: &[u8] = b"\x1b[6n\x1b[0c\x1b[>0c\x1b[5n\x1b[14t\x1b[16t\x1b[18t\
\x1b[19t\x1b[20t\x1b[21t\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07\
\x1b]52;c;?\x07";

#[cfg(test)]
mod tests {
	use super::*;

	// Test ID: EpQN0oH
	#[test]
	fn a_seed_always_gives_the_same_case() {
		let run = |seed| {
			let mut rng = Rng::new(seed);
			(0..8).map(|_| rng.next_u64()).collect::<Vec<_>>()
		};
		assert_eq!(run(0), run(0));
		assert_ne!(run(0), run(1));
	}

	// Test ID: EpQN0oI
	#[test]
	fn a_mutation_stays_within_the_size_cap() {
		let mut rng = Rng::new(7);
		let mut case = vec![b'x'; 60_000];
		for _ in 0..40 {
			case = mutate(&mut rng, &case);
			assert!(case.len() <= 1 << 16);
		}
	}

	// Test ID: EpQN0oJ
	#[test]
	fn a_missing_corpus_directory_is_empty_rather_than_an_error() {
		assert!(corpus("no-such-target").is_empty());
	}

	// The path to the corpus is built from the manifest directory, so a tree laid
	// out differently - or a platform that spells the separator the other way -
	// would find nothing and every target would still pass.
	// Test ID: EpQNtRI
	#[test]
	fn the_corpus_is_where_the_targets_look_for_it() {
		for target in [
			"vt", "config", "links", "title", "cwd", "paste", "shapes", "pairs", "xmp",
		] {
			let cases = corpus(target);
			assert!(!cases.is_empty(), "no corpus found for '{target}'");
			assert!(
				cases.iter().all(|case| !case.is_empty()),
				"an empty file in the '{target}' corpus"
			);
		}
	}
}
