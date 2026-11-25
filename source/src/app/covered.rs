// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! Whether other windows cover this one completely. Windows sends no occlusion
//! events, so a covered window counted as on screen, kept building frames, and
//! waited "Minutes otherwise" rather than "Minutes when hidden". Worked out the
//! way Chromium's occlusion tracker does it: hooks on the desktop's window
//! events say when to look again, and a walk down the z-order takes away every
//! window above this one that is known to be opaque. A window that might be
//! seen through, or that isn't sure to be where it says, never counts, since a
//! window wrongly taken as covered stops drawing while someone reads it.
//!
//! Only asked while the window is unfocused. A focused window counts as shown,
//! and its hooks are off, so nothing else on the desktop wakes it.

/// A screen rectangle, right and bottom outside it.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
	pub l: i32,
	pub t: i32,
	pub r: i32,
	pub b: i32,
}

#[cfg(any(windows, test))]
impl Rect {
	fn empty(self) -> bool {
		self.l >= self.r || self.t >= self.b
	}

	fn overlaps(self, o: Rect) -> bool {
		self.l < o.r && o.l < self.r && self.t < o.b && o.t < self.b
	}

	// What's left of self with `cut` taken out, as up to 4 pieces.
	fn minus(self, cut: Rect, out: &mut Vec<Rect>) {
		if !self.overlaps(cut) {
			out.push(self);
			return;
		}
		let (t, b) = (self.t.max(cut.t), self.b.min(cut.b));
		let pieces = [Rect { b: cut.t, ..self }, Rect { t: cut.b, ..self }, Rect { t, b, r: cut.l, ..self }, Rect { t, b, l: cut.r, ..self }];
		out.extend(pieces.into_iter().filter(|p| !p.empty()));
	}
}

// Past this many pieces the answer is "not covered". A desktop with that many
// windows cutting this one up is better treated as showing it.
#[cfg(any(windows, test))]
const MAX_PIECES: usize = 512;

/// Whether `target` is all under the rectangles in `above`. An empty target
/// isn't covered: a window without any area is the minimized check's.
#[cfg(any(windows, test))]
pub(super) fn all_under(target: Rect, above: impl IntoIterator<Item = Rect>) -> bool {
	if target.empty() {
		return false;
	}
	let mut left = vec![target];
	let mut next = Vec::new();
	for cut in above {
		next.clear();
		for piece in &left {
			piece.minus(cut, &mut next);
		}
		std::mem::swap(&mut left, &mut next);
		if left.is_empty() {
			return true;
		}
		if left.len() > MAX_PIECES {
			return false;
		}
	}
	false
}

#[cfg(windows)]
pub(super) use win::Cover;

#[cfg(not(windows))]
pub(super) use other::Cover;

#[cfg(not(windows))]
mod other {
	use crate::term::UserEvent;
	use winit::event_loop::EventLoopProxy;
	use winit::window::Window;

	/// Other desktops report occlusion themselves, where they do at all.
	#[derive(Debug)]
	pub(in crate::app) struct Cover;

	#[allow(clippy::unused_self, reason = "the same calls as the Windows one")]
	impl Cover {
		pub(in crate::app) fn new(_: &EventLoopProxy<UserEvent>) -> Self {
			Cover
		}

		pub(in crate::app) fn check(&mut self, _: &Window, _: bool) -> bool {
			false
		}

		pub(in crate::app) fn touch(&mut self) {}
	}
}

#[cfg(windows)]
mod win {
	use super::{Rect, all_under};
	use crate::term::UserEvent;
	use std::cell::{Cell, RefCell};
	use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
	use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
	use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, CreateRectRgn, DeleteObject, GetWindowRgn};
	use windows_sys::Win32::System::Threading::GetCurrentProcessId;
	use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
	use windows_sys::Win32::UI::WindowsAndMessaging::{CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_REORDER, EVENT_OBJECT_STATECHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EnumWindows, GA_ROOT, GWL_EXSTYLE, GWL_STYLE, GetAncestor, GetClientRect, GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP};
	use winit::event_loop::EventLoopProxy;
	use winit::window::Window;

	thread_local! {
		// Set by the hooks, which run on this thread inside the event loop's
		// message pump. The pass after looks again.
		static STALE: Cell<bool> = const { Cell::new(false) };
		static WAKE: RefCell<Option<EventLoopProxy<UserEvent>>> = const { RefCell::new(None) };
	}

	// The events that can change what covers a window, as Chromium's tracker
	// takes them: activation, moves and resizes by the user, minimize and
	// restore, task switching, show, hide, and close, z-order, maximize and
	// restore, and a move to another virtual desktop. Location changes come
	// apart (`Cover::follow`), since hooked for every process they come with
	// every pointer move.
	const RANGES: [(u32, u32); 4] = [(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND), (EVENT_OBJECT_DESTROY, EVENT_OBJECT_REORDER), (EVENT_OBJECT_STATECHANGE, EVENT_OBJECT_STATECHANGE), (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED)];

	unsafe extern "system" fn heard(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
		// a top-level window itself, not a caret, a cursor, or a control
		if hwnd.is_null() || object != OBJID_WINDOW || child != CHILDID_SELF as i32 {
			return;
		}
		// SAFETY: any handle may be asked about; a stale one gives null, which
		// a closed window's handle already is
		if event != EVENT_OBJECT_DESTROY && unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
			return;
		}
		if !STALE.replace(true) {
			// the pump wakes the loop for the hook as it is, and this makes sure of it
			WAKE.with_borrow(|proxy| proxy.as_ref().map(|p| p.send_event(UserEvent::Restacked)));
		}
	}

	fn hook(min: u32, max: u32, pid: u32) -> Option<HWINEVENTHOOK> {
		// SAFETY: out of context, so `heard` runs on this thread from its pump,
		// and it's unhooked on this thread too (`Cover::unhook`).
		let h = unsafe { SetWinEventHook(min, max, std::ptr::null_mut(), Some(heard), pid, 0, WINEVENT_OUTOFCONTEXT) };
		(!h.is_null()).then_some(h)
	}

	fn unhook(h: HWINEVENTHOOK) {
		// SAFETY: a hook this thread set and hasn't unhooked
		unsafe { UnhookWinEvent(h) };
	}

	#[derive(Debug)]
	pub(in crate::app) struct Cover {
		covered: bool,
		stale: bool,
		hooks: Vec<HWINEVENTHOOK>,
		// location changes, only from the processes whose windows cover this one
		follow: Vec<(u32, HWINEVENTHOOK)>,
	}

	impl Cover {
		pub(in crate::app) fn new(proxy: &EventLoopProxy<UserEvent>) -> Self {
			WAKE.with_borrow_mut(|wake| *wake = Some(proxy.clone()));
			Cover { covered: false, stale: true, hooks: Vec::new(), follow: Vec::new() }
		}

		/// Something about this window moved: look again on the next check.
		pub(in crate::app) fn touch(&mut self) {
			self.stale = true;
		}

		/// Whether the window is all covered, or on another virtual desktop.
		/// Looks again only after a hook or `touch` said something changed.
		pub(in crate::app) fn check(&mut self, window: &Window, focused: bool) -> bool {
			if focused {
				if !self.hooks.is_empty() || !self.follow.is_empty() {
					self.unhook();
				}
				if self.covered {
					super::super::idle::idledbg("not covered: focused");
				}
				self.covered = false;
				self.stale = true;
				return false;
			}
			if self.hooks.is_empty() {
				self.hooks = RANGES.iter().filter_map(|&(min, max)| hook(min, max, 0)).collect();
				self.stale = true;
			}
			if !(STALE.replace(false) | std::mem::take(&mut self.stale)) {
				return self.covered;
			}
			let (covered, pids) = hwnd_of(window).map_or((false, Vec::new()), look);
			if covered != self.covered {
				super::super::idle::idledbg(if covered { "covered by other windows" } else { "not covered" });
			}
			self.covered = covered;
			self.follow_only(&pids);
			covered
		}

		// Location changes from exactly these processes.
		fn follow_only(&mut self, pids: &[u32]) {
			self.follow.retain(|&(pid, h)| {
				let keep = pids.contains(&pid);
				if !keep {
					unhook(h);
				}
				keep
			});
			for &pid in pids {
				if !self.follow.iter().any(|&(p, _)| p == pid)
					&& let Some(h) = hook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, pid)
				{
					self.follow.push((pid, h));
				}
			}
		}

		fn unhook(&mut self) {
			for h in self.hooks.drain(..) {
				unhook(h);
			}
			for (_, h) in self.follow.drain(..) {
				unhook(h);
			}
		}
	}

	impl Drop for Cover {
		fn drop(&mut self) {
			self.unhook();
		}
	}

	fn hwnd_of(window: &Window) -> Option<HWND> {
		use raw_window_handle::{HasWindowHandle, RawWindowHandle};
		match window.window_handle().ok()?.as_raw() {
			RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
			_ => None,
		}
	}

	fn cloaked(hwnd: HWND) -> bool {
		let mut cloak: u32 = 0;
		// SAFETY: a u32 out value, its size given
		let hr = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&raw mut cloak).cast(), 4) };
		hr >= 0 && cloak != 0
	}

	fn rect(r: RECT) -> Rect {
		Rect { l: r.left, t: r.top, r: r.right, b: r.bottom }
	}

	// The client area on screen: what nobody would see if it were all covered.
	fn client_on_screen(hwnd: HWND) -> Option<Rect> {
		let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
		let mut at = POINT { x: 0, y: 0 };
		// SAFETY: the live window's own handle and plain out values
		let ok = unsafe { GetClientRect(hwnd, &raw mut r) != 0 && ClientToScreen(hwnd, &raw mut at) != 0 };
		ok.then(|| Rect { l: at.x, t: at.y, r: at.x + r.right, b: at.y + r.bottom })
	}

	// Where a window above this one draws, if it's sure to hide what's under
	// it. Visible, not minimized, not cloaked, not another of ours, not
	// layered, click-through, or drawn without a redirection bitmap, since any
	// of those may be seen through. Not a tool window or a popup, whose bounds
	// can be bigger than what shows, and not shaped by a window region. The
	// bounds are the frame as drawn, without the invisible resize border.
	fn opaque(hwnd: HWND, own_pid: u32) -> Option<(Rect, u32)> {
		// SAFETY: every call takes any window handle and fails on a stale one
		unsafe {
			if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
				return None;
			}
			let mut pid = 0;
			GetWindowThreadProcessId(hwnd, &raw mut pid);
			if pid == own_pid || pid == 0 {
				return None;
			}
			let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
			if ex & (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW) != 0 {
				return None;
			}
			if GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_POPUP != 0 {
				return None;
			}
			if cloaked(hwnd) {
				return None;
			}
			let rgn = CreateRectRgn(0, 0, 0, 0);
			let shaped = !rgn.is_null() && GetWindowRgn(hwnd, rgn) != 0;
			if !rgn.is_null() {
				DeleteObject(rgn);
			}
			if shaped {
				return None;
			}
			let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
			let hr = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS as u32, (&raw mut r).cast(), size_of::<RECT>() as u32);
			(hr >= 0).then(|| (rect(r), pid))
		}
	}

	struct Walk {
		me: HWND,
		target: Rect,
		own_pid: u32,
		above: Vec<Rect>,
		pids: Vec<u32>,
		found: bool,
	}

	unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
		// SAFETY: `look` passes its own Walk and outlives the enumeration
		let walk = unsafe { &mut *(lp as *mut Walk) };
		if hwnd == walk.me {
			walk.found = true;
			return 0;
		}
		if let Some((r, pid)) = opaque(hwnd, walk.own_pid)
			&& r.overlaps(walk.target)
		{
			walk.above.push(r);
			if !walk.pids.contains(&pid) {
				walk.pids.push(pid);
			}
		}
		1
	}

	// Covered, and the processes whose windows cover it. On another virtual
	// desktop it's hidden with nothing to follow: coming back uncloaks it.
	fn look(me: HWND) -> (bool, Vec<u32>) {
		if cloaked(me) {
			return (true, Vec::new());
		}
		let Some(target) = client_on_screen(me) else {
			return (false, Vec::new());
		};
		// SAFETY: plain call
		let own_pid = unsafe { GetCurrentProcessId() };
		let mut walk = Walk { me, target, own_pid, above: Vec::new(), pids: Vec::new(), found: false };
		// SAFETY: `each` reads the Walk only during this call. Top-level windows
		// come from the top of the z-order down, and it stops at this one.
		unsafe { EnumWindows(Some(each), &raw mut walk as LPARAM) };
		if !walk.found {
			return (false, Vec::new());
		}
		let covered = all_under(target, walk.above.iter().copied());
		(covered, if covered { walk.pids } else { Vec::new() })
	}
}

#[cfg(test)]
mod tests {
	use super::{Rect, all_under};

	fn r(l: i32, t: i32, r: i32, b: i32) -> Rect {
		Rect { l, t, r, b }
	}

	// The idlebehind layout and its near misses.
	// Test ID: dfsm7
	#[test]
	fn a_window_counts_as_covered_only_with_none_of_it_left() {
		let me = r(100, 100, 500, 400);
		assert!(!all_under(me, []), "nothing above");
		assert!(all_under(me, [r(0, 0, 800, 600)]), "one window over all of it");
		assert!(all_under(me, [r(100, 100, 500, 400)]), "exactly its size");
		// the idlebehind case: all but a strip on the left
		assert!(!all_under(me, [r(110, 0, 800, 600)]), "a strip left showing");
		assert!(all_under(me, [r(110, 0, 800, 600), r(0, 0, 120, 600)]), "the strip covered by a second one");
		// 4 quarters, one short by a pixel
		let quarters = [r(0, 0, 300, 250), r(300, 0, 600, 250), r(0, 250, 300, 500), r(300, 250, 600, 500)];
		assert!(all_under(me, quarters));
		let short = [r(0, 0, 300, 250), r(300, 0, 600, 250), r(0, 250, 300, 500), r(301, 250, 600, 500)];
		assert!(!all_under(me, short), "a one-pixel gap");
		// a hole in the middle, framed by 4 windows
		let frame = [r(0, 0, 600, 200), r(0, 300, 600, 500), r(0, 0, 250, 500), r(350, 0, 600, 500)];
		assert!(!all_under(me, frame), "a hole in the middle");
		assert!(!all_under(r(0, 0, 0, 0), [r(0, 0, 10, 10)]), "no area is the minimized check's");
		assert!(!all_under(me, [r(600, 600, 700, 700), r(0, 0, 50, 50)]), "windows elsewhere");
	}

	// Thin slivers each leave pieces either side, and past the cap the answer
	// is shown, even though the last window would cover it all.
	// Test ID: dfsm8
	#[test]
	fn a_window_cut_into_too_many_pieces_counts_as_shown() {
		let me = r(0, 0, 2000, 2000);
		let slivers = (0..1000).map(|i| r(2 * i, 2 * i, 2 * i + 1, 2 * i + 1));
		assert!(!all_under(me, slivers.chain([r(0, 0, 2000, 2000)])));
	}
}
