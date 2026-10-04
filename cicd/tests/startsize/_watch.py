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
except ImportError:
	sys.exit(2)


def fFind(disp, window, pid, pidAtom):
	try:
		wmClass = window.get_wm_class()
		if wmClass and wmClass[0].lower() == "silkterm":
			owner = window.get_full_property(pidAtom, X.AnyPropertyType)
			if owner and owner.value[0] == pid:
				return window
		for child in window.query_tree().children:
			found = fFind(disp, child, pid, pidAtom)
			if found:
				return found
	except error.XError:
		pass
	return None


def fMain():
	if len(sys.argv) != 4:
		sys.exit(2)
	disp = display.Display(sys.argv[1])
	pid, after = int(sys.argv[2]), float(sys.argv[3])
	root = disp.screen().root
	pidAtom = disp.intern_atom("_NET_WM_PID")
	stateAtom = disp.intern_atom("_NET_WM_STATE")
	maxAtoms = {disp.intern_atom("_NET_WM_STATE_MAXIMIZED_VERT"), disp.intern_atom("_NET_WM_STATE_MAXIMIZED_HORZ")}
	fullAtom = disp.intern_atom("_NET_WM_STATE_FULLSCREEN")
	start = time.monotonic()
	## Polled, not evented: the window is reparented under the window manager's
	## frame, and "shown" is the frame's map state, not the window's own.
	window, last, shownAt = None, None, None
	while True:
		now = time.monotonic()
		if shownAt is None and now - start > 60:
			return 1
		if shownAt is not None and now - shownAt > after:
			return 0
		try:
			if window is None:
				window = fFind(disp, root, pid, pidAtom)
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
				state = window.get_full_property(stateAtom, X.AnyPropertyType)
			finally:
				disp.ungrab_server()
				disp.flush()
			held = set(state.value) if state is not None else set()
			mode = "fullscreen" if fullAtom in held else "maximized" if maxAtoms <= held else "-"
		except error.XError:
			window = None
			continue
		seen = (size.width, size.height, shown, mode)
		if seen != last:
			print(f"{(now - start) * 1000:.0f} {size.width}x{size.height} {'shown' if shown else 'hidden'} {mode}", flush=True)
			last = seen
		if shown and shownAt is None:
			shownAt = now
		time.sleep(0.001)


sys.exit(fMain())
