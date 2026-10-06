#!/usr/bin/env python3

##	Purpose:
##		The spacing every .md in the repository keeps and no markdown linter
##		checks: a blank line between each pair of top-level bullets, and a blank
##		line before and after each heading. Sub-bullets may stay tight, and so
##		may a table of contents. README.md and style-guide.md had run bullets
##		together for months.
##		Also no banner rule, such as `// ---- name ----`, in any .rs git tracks.
##		The style guide allows none, and fifteen had divided three files.
##		And a comment on anything declared `pub` outside the tests is a `///`
##		doc, and a file's opening comment is `//!`. Most were plain `//`.
##	Syntax:
##		run.py [--root DIR] [FILE ...]
##		  --root DIR  repository root (default: three levels above this script)
##		  With no FILE, every .md git tracks, then every .rs for banner rules
##		  and doc comments.
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
##	An item or a field declared `pub`, at any visibility. A `pub use` or a
##	`pub mod x;` has nothing to document here.
PUB = re.compile(
	r'^\s*pub(\([^)]*\))?\s+(((const|async|unsafe|extern\s+"[^"]*")\s+)*(fn|struct|enum|trait|type|const|static|mod|union)\b'
	r"|r?#?[a-z_][a-z0-9_]*\s*:)"
)
MOD_DECL = re.compile(r"^\s*pub(\([^)]*\))?\s+mod\s+\w+\s*;")
CFG_TEST = re.compile(r"^\s*#\[cfg\((all\()?test\b")
LICENSE = ("// SPDX-License-Identifier:", "// Copyright")
##	build.rs pulls this one in with include!, where an inner doc cannot go.
NO_INNER_DOC = {"buildnum.rs"}


def problems(lines: list[str]) -> list[tuple[int, str]]:
	##	(line, problem) pairs, 1-based. Fenced code and TOC blocks are passed over
	##	whole, and a fence counts as text for the lines around it.
	out = []
	fence: str | None = None
	toc = False
	in_item = False
	for i, line in enumerate(lines):
		stripped = line.strip()
		if fence is None and not toc and stripped.startswith(("```", "~~~")):
			fence = stripped[:3]
			in_item = False
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
			if in_item and prev.strip():
				out.append((i + 1, "top-level bullet with no blank line before it"))
			in_item = True
		elif not stripped:
			in_item = False
		elif not line[0].isspace():
			in_item = False
		if HEADING.match(line):
			##	The TOC extension's marker sits on the line above its heading.
			above = i - 1 if prev.strip() == TOC_IGNORE else i
			if above and lines[above - 1].strip():
				out.append((i + 1, "heading with no blank line before it"))
			if i + 1 < len(lines) and lines[i + 1].strip():
				out.append((i + 1, "heading with no blank line after it"))
	return out


def banners(lines: list[str]) -> list[tuple[int, str]]:
	return [(i + 1, "banner rule in a comment") for i, line in enumerate(lines) if BANNER.match(line)]


def attribute_top(lines: list[str], j: int) -> int | None:
	##	Line j ends an attribute: the line it starts on, else None.
	stripped = lines[j].strip()
	if stripped.startswith("#["):
		return j
	if not stripped.endswith("]"):
		return None
	for k in range(j - 1, max(j - 30, -1), -1):
		above = lines[k].strip()
		if above.startswith("#["):
			return k
		if not above or above.startswith("//"):
			return None
	return None


def plain_docs(lines: list[str], name: str) -> list[tuple[int, str]]:
	out = []
	i = 0
	while i < len(lines) and lines[i].startswith(LICENSE):
		i += 1
	while i < len(lines) and not lines[i].strip():
		i += 1
	j = i
	while j < len(lines) and lines[j].startswith("//") and not lines[j].startswith(("///", "//!")):
		j += 1
	##	The file's own comment runs into a blank line or the file's first `use`,
	##	where one that runs into an item is that item's.
	after = lines[j].lstrip() if j < len(lines) else ""
	if j > i and name not in NO_INNER_DOC and (not after or after.startswith(("use ", "#![", "mod "))):
		out.append((i + 1, "opening comment is not //!"))
	skip_to = None
	test_attr = False
	for n, line in enumerate(lines):
		if skip_to is not None:
			if line.startswith(skip_to):
				skip_to = None
			continue
		stripped = line.strip()
		if CFG_TEST.match(line):
			test_attr = True
			continue
		if test_attr:
			if not stripped or stripped.startswith(("#[", "//")):
				continue
			##	Everything under a test attribute is the test's, body and all. A
			##	line that opens nothing, such as a field, is all there is.
			test_attr = False
			if stripped.endswith(("{", "(", "<")):
				skip_to = line[: len(line) - len(line.lstrip())] + "}"
			continue
		if not PUB.match(line) or MOD_DECL.match(line):
			continue
		k = n - 1
		while k >= 0:
			above = lines[k].strip()
			if above.startswith("//"):
				if not above.startswith("///"):
					out.append((k + 1, "comment on a pub item is not ///"))
				k -= 1
				continue
			top = attribute_top(lines, k)
			if top is None:
				break
			k = top - 1
	return out


def main() -> int:
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
	for path in sources:
		for n, what in plain_docs(path.read_text(encoding="utf-8", errors="replace").split("\n"), path.name):
			print(f"{path}:{n}: {what}")
			bad += 1
	if bad:
		print(f"{bad} plain comment(s) where a doc comment goes")
		return 1
	print(f"OK: {len(sources)} .rs files use doc comments on pub items")
	return 0


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20260926: Created.
##		- 20261004: Banner rules in .rs files.
##		- 20261005: Doc comments on pub items and file openings.
