// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Thin wrapper over copypasta for the regular CLIPBOARD plus, on Linux, the
//! PRIMARY selection (middle-click paste). The contexts are held for the
//! app's lifetime: X11 selection ownership lives in the context, so dropping it
//! would drop the copied text. Everything is best-effort - a missing/foreign
//! display just yields no clipboard rather than an error.
//!
//! A Wayland window gets the Wayland clipboard on winit's own connection, and
//! no X11 context at all, so a Wayland session never opens an X connection for
//! it. Only an X11 window takes the X11 contexts, under Xwayland too.
//!
//! Wayland takes a selection only from a client that has the keyboard, so there
//! a program's store made while the window is unfocused is held until the
//! window has it back (2026101607585700).

use copypasta::ClipboardProvider;
use raw_window_handle::RawDisplayHandle;

#[cfg(target_os = "linux")]
use copypasta::x11_clipboard::{Primary, X11ClipboardContext};

/// The desktop's selections. `clipboard` is CLIPBOARD, beside PRIMARY.
#[allow(clippy::struct_field_names)]
pub struct Clipboard {
	clipboard: Option<Box<dyn ClipboardProvider>>,
	#[cfg(target_os = "linux")]
	primary: Option<Box<dyn ClipboardProvider>>,
	backend: Backend,
	// the window's, which on Wayland it gets some time after it opens
	has_keyboard: bool,
	// a program's last store, waiting for the keyboard (Wayland only)
	held_clipboard: Option<String>,
	#[cfg(target_os = "linux")]
	held_primary: Option<String>,
}

// Which clipboard the the window's display gave us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
	Wayland,
	/// X11 on Linux, or the platform's one clipboard elsewhere.
	Native,
}

impl std::fmt::Debug for Clipboard {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Clipboard")
			.field("backend", &self.backend)
			.finish_non_exhaustive()
	}
}

impl Clipboard {
	/// `display` is the window's. On Wayland it has to stay connected for as
	/// long as this lives, so call `release` before the event loop goes.
	pub fn new(display: Option<RawDisplayHandle>) -> Self {
		#[cfg(target_os = "linux")]
		if let Some(RawDisplayHandle::Wayland(handle)) = display {
			// SAFETY: a live wl_display from winit, which outlives this until
			// `release` from the app's `exiting`.
			let (primary, clipboard) = unsafe {
				copypasta::wayland_clipboard::create_clipboards_from_external(
					handle.display.as_ptr(),
				)
			};
			return Self {
				clipboard: Some(Box::new(clipboard)),
				primary: Some(Box::new(primary)),
				backend: Backend::Wayland,
				has_keyboard: false,
				held_clipboard: None,
				held_primary: None,
			};
		}
		let _ = display;
		Self {
			clipboard: copypasta::ClipboardContext::new()
				.ok()
				.map(|ctx| Box::new(ctx) as Box<dyn ClipboardProvider>),
			#[cfg(target_os = "linux")]
			primary: X11ClipboardContext::<Primary>::new()
				.ok()
				.map(|ctx| Box::new(ctx) as Box<dyn ClipboardProvider>),
			backend: Backend::Native,
			has_keyboard: true,
			held_clipboard: None,
			#[cfg(target_os = "linux")]
			held_primary: None,
		}
	}

	/// Let go of the Wayland contexts while winit's connection is still up.
	/// Their worker thread reads that connection, which the event loop may close
	/// before the app is dropped. X11 contexts own their connection and stay.
	pub fn release(&mut self) {
		if self.backend == Backend::Wayland {
			self.clipboard = None;
			#[cfg(target_os = "linux")]
			{
				self.primary = None;
			}
		}
	}

	/// Copy text to the primary selection (falls back to the clipboard where
	/// there's no primary, i.e. non-Linux).
	pub fn set_primary(&mut self, text: String) {
		#[cfg(target_os = "linux")]
		{
			// a newer copy beats a held one
			self.held_primary = None;
		}
		#[cfg(target_os = "linux")]
		if let Some(ctx) = self.primary.as_mut() {
			let len = text.len();
			trace("primary", len, ctx.set_contents(text));
			return;
		}
		self.set_clipboard(text);
	}

	/// Read the primary selection (middle-click paste source).
	pub fn get_primary(&mut self) -> Option<String> {
		#[cfg(target_os = "linux")]
		if let Some(ctx) = self.primary.as_mut() {
			return ctx.get_contents().ok().filter(|s| !s.is_empty());
		}
		self.get_clipboard()
	}

	pub fn set_clipboard(&mut self, text: String) {
		self.held_clipboard = None;
		if let Some(ctx) = self.clipboard.as_mut() {
			let len = text.len();
			trace("clipboard", len, ctx.set_contents(text));
		}
	}

	/// A program's store to the clipboard.
	pub fn program_set_clipboard(&mut self, text: String) {
		if self.must_hold() {
			trace_held("clipboard", text.len());
			self.held_clipboard = Some(text);
			return;
		}
		self.set_clipboard(text);
	}

	/// A program's store to the primary selection. Linux only, since elsewhere
	/// `set_primary` would fall back to the clipboard, which it never asked for.
	#[cfg(target_os = "linux")]
	pub fn program_set_primary(&mut self, text: String) {
		if self.must_hold() {
			trace_held("primary", text.len());
			self.held_primary = Some(text);
			return;
		}
		self.set_primary(text);
	}

	/// The window got or lost the keyboard. Getting it sets what a program
	/// stored meanwhile.
	pub fn focus_changed(&mut self, focused: bool) {
		self.has_keyboard = focused;
		if !focused {
			return;
		}
		if let Some(text) = self.held_clipboard.take() {
			self.set_clipboard(text);
		}
		#[cfg(target_os = "linux")]
		if let Some(text) = self.held_primary.take() {
			self.set_primary(text);
		}
	}

	// X11, macOS and Windows take a selection from a window without focus.
	fn must_hold(&self) -> bool {
		self.backend == Backend::Wayland && !self.has_keyboard
	}

	pub fn get_clipboard(&mut self) -> Option<String> {
		self.clipboard
			.as_mut()?
			.get_contents()
			.ok()
			.filter(|s| !s.is_empty())
	}
}

// SILK_KEYDBG: one line per store, with the result the provider reports. On
// X11 a store can be accepted and still lose to a clipboard manager that takes
// the selection straight back, so what happened at THIS end is worth having in
// the trace before blaming the key that asked for it. On Wayland a store is
// always accepted here, and the clipboard's worker drops one made while no
// SilkTerm window has the keyboard, so a program's waits for focus instead.
fn trace(which: &str, len: usize, result: Result<(), Box<dyn std::error::Error + Send + Sync>>) {
	if !crate::app::env_flag(crate::app::EnvFlag::KeyDbg) {
		return;
	}
	match result {
		Ok(()) => eprintln!("[clip] {which} <- {len} bytes"),
		Err(err) => eprintln!("[clip] {which} FAILED: {err}"),
	}
}

fn trace_held(which: &str, len: usize) {
	if crate::app::env_flag(crate::app::EnvFlag::KeyDbg) {
		eprintln!("[clip] {which} held until focus <- {len} bytes");
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::{Arc, Mutex};

	// What a provider was handed, in order.
	#[derive(Default, Clone)]
	struct Tape(Arc<Mutex<Vec<String>>>);

	impl Tape {
		fn took(&self) -> Vec<String> {
			std::mem::take(&mut *self.0.lock().unwrap())
		}
	}

	impl ClipboardProvider for Tape {
		fn get_contents(&mut self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
			Ok(String::new())
		}
		fn set_contents(
			&mut self,
			text: String,
		) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
			self.0.lock().unwrap().push(text);
			Ok(())
		}
	}

	fn taped(backend: Backend) -> (Clipboard, Tape, Tape) {
		let (clip, primary) = (Tape::default(), Tape::default());
		let board = Clipboard {
			clipboard: Some(Box::new(clip.clone())),
			#[cfg(target_os = "linux")]
			primary: Some(Box::new(primary.clone())),
			has_keyboard: backend == Backend::Native,
			backend,
			held_clipboard: None,
			#[cfg(target_os = "linux")]
			held_primary: None,
		};
		(board, clip, primary)
	}

	// Test ID: dev22
	#[test]
	fn a_program_store_without_focus_on_wayland_waits_for_focus() {
		let (mut board, clip, _) = taped(Backend::Wayland);
		board.program_set_clipboard("first".into());
		board.focus_changed(true);
		board.focus_changed(false);
		assert_eq!(clip.took(), ["first"], "held from before the first focus");
		board.program_set_clipboard("second".into());
		board.program_set_clipboard("third".into());
		assert!(clip.took().is_empty());
		board.focus_changed(true);
		assert_eq!(clip.took(), ["third"]);
		board.focus_changed(false);
		board.focus_changed(true);
		assert!(clip.took().is_empty(), "set once, not at every focus");
		board.program_set_clipboard("focused".into());
		assert_eq!(clip.took(), ["focused"]);
	}

	// Test ID: dev23
	#[cfg(target_os = "linux")]
	#[test]
	fn the_primary_waits_for_focus_on_wayland_too() {
		let (mut board, clip, primary) = taped(Backend::Wayland);
		board.program_set_primary("held".into());
		assert!(primary.took().is_empty());
		board.focus_changed(true);
		assert_eq!(primary.took(), ["held"]);
		assert!(clip.took().is_empty());
	}

	// Test ID: dev24
	#[test]
	fn a_newer_copy_beats_a_held_one() {
		let (mut board, clip, primary) = taped(Backend::Wayland);
		board.program_set_clipboard("held".into());
		#[cfg(target_os = "linux")]
		board.program_set_primary("held".into());
		// a copy made in a dialog, which has the keyboard
		board.set_clipboard("typed".into());
		#[cfg(target_os = "linux")]
		board.set_primary("selected".into());
		board.focus_changed(true);
		assert_eq!(clip.took(), ["typed"]);
		#[cfg(target_os = "linux")]
		assert_eq!(primary.took(), ["selected"]);
		let _ = primary;
	}

	// Test ID: dev25
	#[test]
	fn x11_and_the_rest_set_a_program_store_at_once() {
		let (mut board, clip, primary) = taped(Backend::Native);
		board.program_set_clipboard("now".into());
		assert_eq!(clip.took(), ["now"]);
		#[cfg(target_os = "linux")]
		{
			board.program_set_primary("now".into());
			assert_eq!(primary.took(), ["now"]);
		}
		let _ = primary;
		board.focus_changed(true);
		assert!(clip.took().is_empty());
	}
}
