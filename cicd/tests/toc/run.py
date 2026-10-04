#!/usr/bin/env python3

##	Purpose:
##		Rebuild the table of contents in every .md that carries one, and fail on any
##		difference. The editor's TOC extension writes these blocks; this reproduces
##		its output so a heading added by hand cannot leave the TOC behind.
##	Syntax:
##		run.py [--fix] [--root DIR] [FILE ...]
##		  --fix       write the rebuilt block instead of reporting
##		  --root DIR  repository root (default: three levels above this script)
##	Exit: 0 all TOCs current, 1 one or more differ.
##	Test ID: EqRbSy0

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import difflib
import re
import sys
from pathlib import Path

BEGIN = "<!-- TOC -->"
END = "<!-- /TOC -->"
IGNORE = "TOC ignore:true"

## Lowercase, keep letters, digits, spaces, hyphens and underscores, spaces to
## hyphens. The extension keeps underscores, which the written-down rule does not
## mention - see #api-alacritty_terminal in design.md.
def anchor(text):
	kept = [c for c in text.lower() if c.isalnum() or c in " -_"]
	return "".join(kept).replace(" ", "-")


def headings(lines):
	fence = None
	prev_ignored = False
	for line in lines:
		stripped = line.strip()
		if stripped.startswith("~~~") or stripped.startswith("```"):
			mark = stripped[:3]
			if fence is None:
				fence = mark
			elif fence == mark:
				fence = None
			continue
		if fence is not None:
			continue
		if IGNORE in line:
			prev_ignored = True
			continue
		match = re.match(r"^(#{2,6}) +(.*?)\s*$", line)
		if match:
			if not prev_ignored:
				yield len(match.group(1)), match.group(2)
			prev_ignored = False
			continue
		if stripped:
			prev_ignored = False


def toc_block(lines):
	out = [BEGIN, ""]
	for level, text in headings(lines):
		out.append("\t" * (level - 2) + f"- [{text}](#{anchor(text)})")
	out += ["", END]
	return out


## Two headings with the same text share an anchor, so one of the links goes to the
## wrong place. No markdown linter catches it.
def clashes(lines):
	seen = {}
	for _, text in headings(lines):
		seen.setdefault(anchor(text), []).append(text)
	return {a: t for a, t in seen.items() if len(t) > 1}


def check(path, fix):
	lines = path.read_text(encoding="utf-8").split("\n")
	try:
		start = lines.index(BEGIN)
		stop = lines.index(END)
	except ValueError:
		return True
	ok = True
	for a, texts in clashes(lines).items():
		print(f"anchor clash in {path}: #{a} <- {', '.join(texts)}")
		ok = False
	if not ok:
		return False
	want = toc_block(lines)
	have = lines[start:stop + 1]
	if have == want:
		return True
	if fix:
		path.write_text("\n".join(lines[:start] + want + lines[stop + 1:]), encoding="utf-8")
		print(f"fixed: {path}")
		return True
	print(f"stale TOC: {path}")
	for line in difflib.unified_diff(have, want, "on disk", "rebuilt", lineterm="", n=1):
		print(f"    {line}")
	return False


def main():
	here = Path(__file__).resolve()
	ap = argparse.ArgumentParser()
	ap.add_argument("--fix", action="store_true")
	ap.add_argument("--root", default=str(here.parents[3]))
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
		if BEGIN not in text:
			continue
		checked += 1
		ok = check(path, args.fix) and ok
	if not checked:
		print("no .md carries a TOC block", file=sys.stderr)
		return 2
	if ok:
		print(f"OK: {checked} TOC blocks current")
	return 0 if ok else 1


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20260920: Created.
