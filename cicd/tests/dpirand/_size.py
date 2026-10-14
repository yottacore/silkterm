#!/usr/bin/env python3

##	- Purpose:
##		The shell in a dpirand/run.py window. Writes the terminal's size, as
##		"rows columns width height" with the last two in pixels, to a file every
##		tenth of a second. A piece of dpirand/run.py, not a test of its own.
##	- Syntax: _size.py <file>

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import fcntl
import struct
import sys
import termios
import time
from pathlib import Path


def main() -> int:
	with Path(sys.argv[1]).open("a") as out:
		while True:
			rows, cols, width, height = struct.unpack("HHHH", fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 8))
			out.write(f"{rows} {cols} {width} {height}\n")
			out.flush()
			time.sleep(0.1)


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20261014 JC: Created.
