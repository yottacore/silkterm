// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! A leak the sanitizer run has to catch, so a setting or a suppression that
//! hides every leak can't pass the suite unnoticed.

#[cfg(test)]
mod tests {
	// Test ID: EtAocf6
	#[test]
	#[allow(clippy::mem_forget, reason = "the leak is the point")]
	fn leaks_on_purpose() {
		let lost = std::hint::black_box(vec![0x5a_u8; 4096]);
		std::mem::forget(lost);
	}
}
