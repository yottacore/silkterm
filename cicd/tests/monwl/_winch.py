#!/usr/bin/env python3

##	- Purpose:
##		The shell in a monwl/run.bash window. Writes "rows columns" to a file
##		every tenth of a second, and on each SIGWINCH as well, so a size that
##		lasted only a moment is still seen. A piece of monwl/run.bash, not a
##		test of its own.
##	- Syntax: _winch.py <file> [--quiet]
##		--quiet   on SIGWINCH only

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import fcntl
import signal
import struct
import sys
import termios
import time
from pathlib import Path


def main() -> int:
	quiet = "--quiet" in sys.argv[2:]
	## Read in the handler, written by the loop: a write from a handler can land
	## inside one the loop is making.
	sizes: list[str] = []

	def told(*_: object) -> None:
		rows, cols, _w, _h = struct.unpack("HHHH", fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 8))
		sizes.append(f"{rows} {cols}\n")

	signal.signal(signal.SIGWINCH, told)
	told()
	with Path(sys.argv[1]).open("a") as out:
		while True:
			while sizes:
				out.write(sizes.pop(0))
			out.flush()
			time.sleep(0.1)
			if not quiet:
				told()


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20261014 JC: Created.
