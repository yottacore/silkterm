#!/usr/bin/env python3

##	- Purpose:
##		Watches one SilkTerm window on an X display from launch and prints its
##		size each time that, its map state or its window state changes. A piece
##		of startsize/run.bash, not a test of its own.
##	- Syntax: _watch.py <display> <pid> <seconds after it shows>
##	- Output: one line per change, "<ms> <width>x<height> <shown|hidden> <fullscreen|maximized|->".
##	- Exit: 0 it showed, 1 it never did, 2 bad arguments or no python-xlib.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import sys
import time

try:
	from Xlib import X, display, error
	from Xlib.xobject.drawable import Window
except ImportError:
	sys.exit(2)


def find_window(disp: display.Display, window: Window, pid: int, pid_atom: int) -> Window | None:
	try:
		wm_class = window.get_wm_class()
		if wm_class and wm_class[0].lower() == "silkterm":
			owner = window.get_full_property(pid_atom, X.AnyPropertyType)
			if owner and owner.value[0] == pid:
				return window
		for child in window.query_tree().children:
			found = find_window(disp, child, pid, pid_atom)
			if found:
				return found
	except error.XError:
		pass
	return None


def main() -> int:
	if len(sys.argv) != 4:
		sys.exit(2)
	disp = display.Display(sys.argv[1])
	pid, after = int(sys.argv[2]), float(sys.argv[3])
	root = disp.screen().root
	pid_atom = disp.intern_atom("_NET_WM_PID")
	state_atom = disp.intern_atom("_NET_WM_STATE")
	max_atoms = {disp.intern_atom("_NET_WM_STATE_MAXIMIZED_VERT"), disp.intern_atom("_NET_WM_STATE_MAXIMIZED_HORZ")}
	full_atom = disp.intern_atom("_NET_WM_STATE_FULLSCREEN")
	start = time.monotonic()
	## Polled, not evented: the window is reparented under the window manager's
	## frame, and "shown" is the frame's map state, not the window's own.
	window: Window | None = None
	last: tuple[int, int, bool, str] | None = None
	shown_at: float | None = None
	while True:
		now = time.monotonic()
		if shown_at is None and now - start > 60:
			return 1
		if shown_at is not None and now - shown_at > after:
			return 0
		try:
			if window is None:
				window = find_window(disp, root, pid, pid_atom)
				if window is None:
					time.sleep(0.002)
					continue
			## One sample is three requests, and the window manager can resize
			## the window between them. A shown window read with the size it had
			## while hidden looks like a jump that never happened, or hides one.
			disp.grab_server()
			try:
				size = window.get_geometry()
				shown = window.get_attributes().map_state == X.IsViewable
				state = window.get_full_property(state_atom, X.AnyPropertyType)
			finally:
				disp.ungrab_server()
				disp.flush()
			held = set(state.value) if state is not None else set()
			mode = "fullscreen" if full_atom in held else "maximized" if max_atoms <= held else "-"
		except error.XError:
			window = None
			continue
		seen = (size.width, size.height, shown, mode)
		if seen != last:
			print(f"{(now - start) * 1000:.0f} {size.width}x{size.height} {'shown' if shown else 'hidden'} {mode}", flush=True)
			last = seen
		if shown and shown_at is None:
			shown_at = now
		time.sleep(0.001)


sys.exit(main())

##	History:
##		- 20261004 JC: Created.
##		- 20261005: PEP 8 names and type hints.
