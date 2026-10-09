#!/usr/bin/env python3

##	Purpose:
##		Refuse a direct read of an auto setting's stored value. An auto setting is
##		read through `config::auto::value` and `automatic` only, so a reader can
##		never take "nothing stored" for a value. The stored value comes out
##		through `Auto::stored`, and that call may appear inside `pub mod auto` in
##		config.rs and nowhere else.
##	Syntax:
##		run.py [--root DIR]
##		  --root DIR  repository root (default: three levels above this script)
##	Exit: 0 no direct read, 1 one or more, 2 the auto module was not found.
##	Test ID: EsDxeGy

##	Copyright © 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import re
import sys
from pathlib import Path

READ = re.compile(r"\.stored\s*\(")
MODULE = re.compile(r"^pub mod auto \{\s*$")


## The lines of `pub mod auto { ... }`, 1-based and inclusive, by brace count.
## Braces inside strings or comments would throw the count off; the module has
## none, and a miscount shows up as the module's own reads failing.
def module_span(lines: list[str]) -> tuple[int, int] | None:
	for start, line in enumerate(lines, 1):
		if not MODULE.match(line):
			continue
		depth = 0
		for at in range(start - 1, len(lines)):
			depth += lines[at].count("{") - lines[at].count("}")
			if depth == 0:
				return start, at + 1
		return None
	return None


def code_part(line: str) -> str:
	return line.split("//", 1)[0]


def check(root: Path) -> int:
	src = root / "source" / "src"
	files = sorted(src.rglob("*.rs"))
	if not files:
		print(f"autoread: no .rs files under {src}", file=sys.stderr)
		return 2
	config = src / "config.rs"
	span = module_span(config.read_text(encoding="utf-8").splitlines()) if config.is_file() else None
	if span is None:
		print(f"autoread: no `pub mod auto {{` block in {config}", file=sys.stderr)
		return 2
	bad: list[str] = []
	for path in files:
		for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
			if not READ.search(code_part(line)):
				continue
			if path == config and span[0] <= number <= span[1]:
				continue
			bad.append(f"{path.relative_to(root)}:{number}: {line.strip()}")
	for line in bad:
		print(line)
	if bad:
		print(
			f"autoread: {len(bad)} direct read(s) of an auto setting's stored value. "
			"Use config::auto::value or automatic.",
			file=sys.stderr,
		)
		return 1
	return 0


def main() -> int:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[3])
	args = parser.parse_args()
	return check(args.root)


if __name__ == "__main__":
	sys.exit(main())
