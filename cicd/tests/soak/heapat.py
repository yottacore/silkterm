#!/usr/bin/env python3

##	Purpose:
##		The soak test's reader: the live heap in each quiet window of a run,
##		from heaptrack's data. Prints "<round> <bytes>" per window, the lowest
##		live heap seen in it, so a frame drawn mid-window doesn't count, or "-"
##		for a window with no sample.
##		heaptrack 1.5's lines: "a <size> <trace>" adds an allocation kind,
##		"+ <kind>" and "- <kind>" allocate and free one, "c <ms>" is the clock,
##		all in hex.
##	Syntax: zstd -dc heap.zst | heapat.py <marks>
##		marks: one "<round> <from ms> <to ms>" line per window, in heaptrack's clock.
##	Exit: 0 read, 1 no heaptrack data, 2 bad arguments.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import sys
from pathlib import Path


def main(argv: list[str]) -> int:
	if len(argv) != 2:
		print("usage: zstd -dc heaptrack.zst | heapat.py <marks>", file=sys.stderr)
		return 2
	with Path(argv[1]).open(encoding="utf-8") as fh:
		marks = [
			(int(r), int(a), int(b))
			for r, a, b in (ln.split() for ln in fh if ln.strip())
		]
	sizes: list[int] = []
	low: dict[int, int] = {}
	live = 0
	seen = 0
	for line in sys.stdin:
		kind = line[:1]
		if kind == "+":
			live += sizes[int(line[2:], 16)]
		elif kind == "-":
			live -= sizes[int(line[2:], 16)]
		elif kind == "a":
			sizes.append(int(line.split()[1], 16))
		elif kind == "c":
			now = int(line[2:], 16)
			seen += 1
			for rnd, start, end in marks:
				if start <= now <= end and live < low.get(rnd, live + 1):
					low[rnd] = live
	if not seen:
		print("no clock lines: not heaptrack data, or an empty run", file=sys.stderr)
		return 1
	for rnd, _, _ in marks:
		print(rnd, low.get(rnd, "-"))
	return 0


if __name__ == "__main__":
	sys.exit(main(sys.argv))

##	History:
##		- 20261101: Created.
