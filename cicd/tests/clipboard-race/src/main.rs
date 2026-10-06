// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Does a SelectionClear left over from an earlier hand-over wipe the text a
//! later store just put in? Steal the selection from a second connection in this
//! same process, so the hand-over and the re-store can be squeezed together, and
//! see whether a reader can still get the text.
use std::process::Command;
use std::time::Duration;
use x11_clipboard::Clipboard;

fn readable() -> Option<String> {
	let out = Command::new("sh")
		.arg("-c")
		.arg("timeout 2 xclip -selection clipboard -o")
		.output()
		.ok()?;
	out.status
		.success()
		.then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn main() {
	let ours = Clipboard::new().expect("ours");
	let thief = Clipboard::new().expect("thief");
	let sel = ours.setter.atoms.clipboard;
	let utf8 = ours.setter.atoms.utf8_string;

	let rounds: usize = std::env::args()
		.nth(1)
		.and_then(|s| s.parse().ok())
		.unwrap_or(200);
	let mut broken = 0;
	for i in 0..rounds {
		ours.store(sel, utf8, "OURS").expect("store");
		// the thief takes it, queueing a SelectionClear at our end, and we take it
		// straight back with no chance for our event thread to read the clear
		thief.store(sel, utf8, "THIEF").expect("thief store");
		ours.store(sel, utf8, format!("OURS-{i}")).expect("restore");
		std::thread::sleep(Duration::from_millis(30));
		match readable() {
			Some(text) if text == format!("OURS-{i}") => {}
			other => {
				broken += 1;
				let _ = other;
			}
		}
	}
	println!("{broken} broken out of {rounds} rounds");
}
