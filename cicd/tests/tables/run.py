#!/usr/bin/env python3

##	Purpose:
##		Check every markdown table is written the way the docs write them: a leading
##		pipe and no trailing one, cells padded so the columns line up, and a rule row
##		that spells out each column's alignment. mdtable.py does the rendering, so the
##		README's generated table and the hand-written ones cannot drift apart.
##	Syntax:
##		run.py [--fix] [--root DIR] [FILE ...]
##		  --fix       rewrite a table that differs instead of reporting it
##		  --root DIR  repository root (default: three levels above this script)
##	Exit: 0 all tables canonical, 1 one or more differ.
##	Test ID: EqqceDw

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import difflib
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve()
sys.path.insert(0, str(HERE.parents[3] / "utility" / "include"))
import mdtable  # noqa: E402

RULE = re.compile(r"^\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?$")


## (first line, last line + 1) of each table outside a code fence. A table is a
## header line starting with a pipe, then a rule row, then its rows.
def tables(lines):
	fence = None
	i = 0
	while i < len(lines):
		stripped = lines[i].strip()
		if stripped.startswith("~~~") or stripped.startswith("```"):
			mark = stripped[:3]
			if fence is None:
				fence = mark
			elif fence == mark:
				fence = None
		elif fence is None and stripped.startswith("|") and i + 1 < len(lines) and RULE.match(lines[i + 1].strip()):
			end = i + 2
			while end < len(lines) and lines[end].strip().startswith("|"):
				end += 1
			yield i, end
			i = end
			continue
		i += 1


def rebuild(block):
	indent = block[0][:len(block[0]) - len(block[0].lstrip())]
	grid = [mdtable.split_row(line) for line in block]
	cols = max(len(r) for r in grid)
	head = grid[0] + [""] * (cols - len(grid[0]))
	return [indent + line for line in mdtable.render(head, grid[1], grid[2:])]


def check(path, fix):
	lines = path.read_text(encoding="utf-8").split("\n")
	out = list(lines)
	ok = True
	## Last first, so a rewrite never moves a table not yet looked at.
	for start, end in reversed(list(tables(lines))):
		have = lines[start:end]
		want = rebuild(have)
		if have == want:
			continue
		ok = False
		out[start:end] = want
		if not fix:
			print(f"table not canonical: {path}:{start + 1}")
			for line in difflib.unified_diff(have, want, "on disk", "rebuilt", lineterm="", n=0):
				print(f"    {line}")
	if not ok and fix:
		path.write_text("\n".join(out), encoding="utf-8")
		print(f"fixed: {path}")
		return True
	return ok


def main():
	ap = argparse.ArgumentParser()
	ap.add_argument("--fix", action="store_true")
	ap.add_argument("--root", default=str(HERE.parents[3]))
	ap.add_argument("files", nargs="*")
	args = ap.parse_args()

	root = Path(args.root)
	if args.files:
		targets = [Path(f) for f in args.files]
	else:
		targets = sorted(p for p in root.rglob("*.md") if "/target/" not in str(p) and "/forks/" not in str(p))

	ok = True
	checked = 0
	for path in targets:
		text = path.read_text(encoding="utf-8", errors="replace")
		count = sum(1 for _ in tables(text.split("\n")))
		if not count:
			continue
		checked += count
		ok = check(path, args.fix) and ok
	if ok:
		print(f"OK: {checked} tables canonical")
	return 0 if ok else 1


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20260924: Created.
