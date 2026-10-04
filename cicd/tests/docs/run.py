#!/usr/bin/env python3

##	Purpose:
##		The spacing every .md in the repository keeps and no markdown linter
##		checks: a blank line between each pair of top-level bullets, and a blank
##		line before and after each heading. Sub-bullets may stay tight, and so
##		may a table of contents. README.md and style-guide.md had run bullets
##		together for months.
##		Also no banner rule, such as `// ---- name ----`, in any .rs git tracks.
##		The style guide allows none, and fifteen had divided three files.
##	Syntax:
##		run.py [--root DIR] [FILE ...]
##		  --root DIR  repository root (default: three levels above this script)
##		  With no FILE, every .md git tracks, then every .rs for banner rules.
##	Exit: 0 all clean, 1 one or more problems.
##	Test ID: Er2UgYD

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import re
import subprocess
import sys
from pathlib import Path

TOC_BEGIN = "<!-- TOC -->"
TOC_END = "<!-- /TOC -->"
TOC_IGNORE = "<!-- TOC ignore:true -->"
BULLET = re.compile(r"^[-*+] ")
HEADING = re.compile(r"^#{1,6} ")
##	A comment line that opens on a run of four or more of one rule character.
##	A box drawn in a comment opens on a corner, so it passes.
BANNER = re.compile(r"^\s*//[/!]?\s*([-=*#~_+•])\1{3,}")


def problems(lines):
	##	(line, problem) pairs, 1-based. Fenced code and TOC blocks are passed over
	##	whole, and a fence counts as text for the lines around it.
	out = []
	fence = None
	toc = False
	inItem = False
	for i, line in enumerate(lines):
		stripped = line.strip()
		if fence is None and not toc and stripped.startswith(("```", "~~~")):
			fence = stripped[:3]
			inItem = False
			continue
		if fence is not None:
			if stripped.startswith(fence):
				fence = None
			continue
		if stripped == TOC_BEGIN:
			toc = True
			continue
		if toc:
			toc = stripped != TOC_END
			continue
		prev = lines[i - 1] if i else ""
		if BULLET.match(line):
			if inItem and prev.strip():
				out.append((i + 1, "top-level bullet with no blank line before it"))
			inItem = True
		elif not stripped:
			inItem = False
		elif not line[0].isspace():
			inItem = False
		if HEADING.match(line):
			##	The TOC extension's marker sits on the line above its heading.
			above = i - 1 if prev.strip() == TOC_IGNORE else i
			if above and lines[above - 1].strip():
				out.append((i + 1, "heading with no blank line before it"))
			if i + 1 < len(lines) and lines[i + 1].strip():
				out.append((i + 1, "heading with no blank line after it"))
	return out


def banners(lines):
	return [(i + 1, "banner rule in a comment") for i, line in enumerate(lines) if BANNER.match(line)]


def main():
	here = Path(__file__).resolve()
	ap = argparse.ArgumentParser()
	ap.add_argument("--root", default=str(here.parents[3]))
	ap.add_argument("files", nargs="*")
	args = ap.parse_args()

	root = Path(args.root)
	if args.files:
		targets = [Path(f) for f in args.files]
	else:
		listed = subprocess.run(["git", "-C", str(root), "ls-files", "-z", "*.md"], capture_output=True, check=True).stdout
		targets = [root / p for p in listed.decode("utf-8").split("\0") if p]
	if not targets:
		print("no .md files found", file=sys.stderr)
		return 2

	bad = 0
	for path in targets:
		lines = path.read_text(encoding="utf-8", errors="replace").split("\n")
		for n, what in problems(lines):
			print(f"{path}:{n}: {what}")
			bad += 1
	if bad:
		print(f"{bad} spacing problem(s)")
		return 1
	print(f"OK: {len(targets)} .md files spaced")
	if args.files:
		return 0
	listed = subprocess.run(["git", "-C", str(root), "ls-files", "-z", "*.rs"], capture_output=True, check=True).stdout
	sources = [root / p for p in listed.decode("utf-8").split("\0") if p]
	if not sources:
		print("no .rs files found", file=sys.stderr)
		return 2
	for path in sources:
		for n, what in banners(path.read_text(encoding="utf-8", errors="replace").split("\n")):
			print(f"{path}:{n}: {what}")
			bad += 1
	if bad:
		print(f"{bad} banner rule(s)")
		return 1
	print(f"OK: {len(sources)} .rs files have no banner rules")
	return 0


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20260926: Created.
##		- 20261004: Banner rules in .rs files.
