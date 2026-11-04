#!/usr/bin/env python3

##	- Purpose:
##		Moves a window on DISPLAY the way a window manager does: the move, then
##		a synthetic ConfigureNotify with where it is on the root (ICCCM 4.1.5).
##		With no window manager the X server sends only the real one, and winit
##		takes that for no move at all. A piece of herdback/run.bash.
##	- Syntax: _wmmove.py <window id> <x> <y>
##	- Exit: 0 moved, 2 bad arguments or no python-xlib.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import sys

try:
	from Xlib import X, display
	from Xlib.protocol import event
except ImportError:
	sys.exit(2)

if len(sys.argv) != 4:
	sys.exit(2)
disp = display.Display()
win = disp.create_resource_object("window", int(sys.argv[1], 0))
x, y = int(sys.argv[2]), int(sys.argv[3])
win.configure(x=x, y=y)
geo = win.get_geometry()
win.send_event(
	event.ConfigureNotify(
		window=win,
		event=win,
		above_sibling=X.NONE,
		x=x,
		y=y,
		width=geo.width,
		height=geo.height,
		border_width=geo.border_width,
		override=0,
	),
	event_mask=X.StructureNotifyMask,
)
disp.sync()
