#!/usr/bin/env python3

##	Purpose:
##		Lint the repo's Python scripts: ruff with the rules in ruff.toml, then a
##		check that blocks are indented with tabs, which no ruff rule can ask for.
##		One line per finding. cicd.bash gates on it.
##	Syntax: py-lint.py [path ...]   (default: every tracked *.py outside forks/)
##	Exit: 0 clean, 1 findings, 2 ruff is not installed.
##	History: At bottom of script.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import shutil
import subprocess
import sys
import tokenize
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

## Indented with spaces from the start, and not reindented.
SPACE_INDENTED = {ROOT / "cicd/tests/scroll/analyze.py"}


def find_ruff() -> str | None:
	found = shutil.which("ruff")
	if found:
		return found
	local = Path.home() / ".local/bin/ruff"
	return str(local) if local.is_file() else None


def tracked_scripts() -> list[Path]:
	out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "*.py"], capture_output=True, text=True, check=True).stdout
	return [ROOT / name for name in out.splitlines() if not name.startswith("forks/")]


def skip_indent(path: Path) -> bool:
	return path in SPACE_INDENTED or (path.is_relative_to(ROOT) and path.relative_to(ROOT).parts[0] == "forks")


## Only INDENT tokens count: a continuation line or a docstring may line up
## with spaces after its tabs.
def space_indents(path: Path) -> list[int]:
	lines = []
	with path.open("rb") as stream:
		try:
			for token in tokenize.tokenize(stream.readline):
				if token.type == tokenize.INDENT and " " in token.string:
					lines.append(token.start[0])
		except (tokenize.TokenError, SyntaxError):
			pass  ## ruff reports the syntax error
	return lines


def main(args: list[str]) -> int:
	ruff = find_ruff()
	if not ruff:
		print("ruff is not installed")
		return 2
	paths = [Path(arg).resolve() for arg in args] if args else tracked_scripts()
	if not paths:
		return 0
	findings = 0
	checked = subprocess.run(
		[ruff, "check", "--config", str(ROOT / "ruff.toml"), "--force-exclude", "--no-cache", "--quiet", "--output-format", "concise", *map(str, paths)],
		capture_output=True, text=True, check=False)
	for line in checked.stdout.splitlines():
		if line.strip():
			print(line)
			findings += 1
	if checked.returncode not in (0, 1):
		print(checked.stderr.strip() or f"ruff exited {checked.returncode}")
		return 1
	for path in paths:
		if skip_indent(path):
			continue
		for line in space_indents(path):
			print(f"{path}:{line}: indentation uses spaces, not tabs")
			findings += 1
	return 1 if findings else 0


if __name__ == "__main__":
	sys.exit(main(sys.argv[1:]))

##	History:
##		- 20261004 JC: Created.
