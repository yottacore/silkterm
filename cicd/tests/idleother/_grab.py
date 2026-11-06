#!/usr/bin/env python3

##	- Purpose:
##		Grabs the pointer on DISPLAY for a moment and lets it go, as a menu, a
##		window drag or a screen locker in another program does. The X server
##		then tells the window under the pointer that the pointer left and
##		entered again, though it never moved. A piece of idleother/run.bash.
##	- Syntax: _grab.py [seconds held, default 0.5]
##	- Exit: 0 grabbed and let go, 1 the grab was refused, 2 bad arguments or no
##	  python-xlib.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import sys
import time

try:
	from Xlib import X, display
except ImportError:
	sys.exit(2)

if len(sys.argv) > 2:
	sys.exit(2)
try:
	held = float(sys.argv[1]) if len(sys.argv) == 2 else 0.5
except ValueError:
	sys.exit(2)
disp = display.Display()
root = disp.screen().root
got = root.grab_pointer(
	False,
	X.ButtonPressMask,
	X.GrabModeAsync,
	X.GrabModeAsync,
	X.NONE,
	X.NONE,
	X.CurrentTime,
)
disp.sync()
if got != X.GrabSuccess:
	sys.exit(1)
time.sleep(held)
disp.ungrab_pointer(X.CurrentTime)
disp.sync()


##	History:
##		- 20261106: Created.
