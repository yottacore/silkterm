// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! A leak and a data race the sanitizer runs have to catch, so a setting or a
//! suppression that hides every report can't pass the suite unnoticed. Each run
//! looks for its own; the other test passes quietly under it.

#[cfg(test)]
mod tests {
	// Test ID: de5f0
	#[test]
	#[allow(clippy::mem_forget, reason = "the leak is the point")]
	fn leaks_on_purpose() {
		let lost = std::hint::black_box(vec![0x5a_u8; 4096]);
		std::mem::forget(lost);
	}

	// Test ID: debjk
	#[test]
	fn races_on_purpose() {
		static mut SHARED: u64 = 0;
		// SAFETY: it isn't. Two unsynchronized writes are the race TSan must report.
		let writer = std::thread::spawn(|| unsafe { (&raw mut SHARED).write_volatile(1) });
		// SAFETY: as above.
		unsafe { (&raw mut SHARED).write_volatile(2) };
		writer.join().unwrap();
	}
}
