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
		if let Some(ctx) = self.clipboard.as_mut() {
			let len = text.len();
			trace("clipboard", len, ctx.set_contents(text));
		}
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
// always accepted here, and one made while no SilkTerm window has the keyboard
// is dropped by the compositor.
fn trace(which: &str, len: usize, result: Result<(), Box<dyn std::error::Error + Send + Sync>>) {
	if !crate::app::env_flag(crate::app::EnvFlag::KeyDbg) {
		return;
	}
	match result {
		Ok(()) => eprintln!("[clip] {which} <- {len} bytes"),
		Err(err) => eprintln!("[clip] {which} FAILED: {err}"),
	}
}
