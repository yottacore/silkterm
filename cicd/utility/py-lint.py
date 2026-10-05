#!/usr/bin/env python3

##	Purpose:
##		Lint the repo's Python scripts: ruff with the rules in ruff.toml, mypy with
##		mypy.ini, then a check that blocks are indented with tabs, which no ruff
##		rule can ask for. One line per finding. cicd.bash gates on it.
##	Syntax: py-lint.py [path ...]   (default: every tracked *.py outside forks/)
##	Exit: 0 clean, 1 findings, 2 ruff is not installed. With no mypy the types
##		go unchecked, and a note on stderr says so.
##	History: At bottom of script.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import shutil
import subprocess
import sys
import tokenize
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

## Indented with spaces from the start, and not reindented.
SPACE_INDENTED = {ROOT / "cicd/tests/scroll/analyze.py"}


def find_tool(name: str) -> str | None:
	found = shutil.which(name)
	if found:
		return found
	local = Path.home() / ".local/bin" / name
	return str(local) if local.is_file() else None


def tracked_scripts() -> list[Path]:
	out = subprocess.run(["git", "-C", str(ROOT), "ls-files", "*.py"], capture_output=True, text=True, check=True).stdout
	return [ROOT / name for name in out.splitlines() if not name.startswith("forks/")]


## What ruff.toml leaves out, so mypy and the tab check leave it out too.
def ruff_excluded() -> list[Path]:
	with (ROOT / "ruff.toml").open("rb") as stream:
		config = tomllib.load(stream)
	return [ROOT / entry for entry in config.get("extend-exclude", [])]


def is_excluded(path: Path, excluded: list[Path]) -> bool:
	return any(path == entry or path.is_relative_to(entry) for entry in excluded)


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


## mypy names a script by its file name, and refuses two files with one name in
## a run. The five test drivers are all run.py, so each takes a run of its own.
def mypy_groups(paths: list[Path]) -> list[list[Path]]:
	groups: list[list[Path]] = []
	seen: dict[str, int] = {}
	for path in paths:
		turn = seen.get(path.stem, 0)
		seen[path.stem] = turn + 1
		if turn == len(groups):
			groups.append([])
		groups[turn].append(path)
	return groups


def mypy_findings(mypy: str, paths: list[Path]) -> list[str]:
	out: list[str] = []
	for group in mypy_groups(paths):
		checked = subprocess.run(
			[mypy, "--config-file", str(ROOT / "mypy.ini"), "--no-error-summary", "--no-pretty", *map(str, group)],
			cwd=ROOT, capture_output=True, text=True, check=False)
		if checked.returncode not in (0, 1):
			out.append(checked.stderr.strip() or checked.stdout.strip() or f"mypy exited {checked.returncode}")
			continue
		## A module two groups both import is reported by each.
		errors = [line for line in checked.stdout.splitlines() if ": error:" in line]
		if checked.returncode and not errors:
			errors = [checked.stdout.strip() or f"mypy exited {checked.returncode}"]
		out += [line for line in errors if line not in out]
	return out


def main(args: list[str]) -> int:
	ruff = find_tool("ruff")
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
	excluded = ruff_excluded()
	kept = [path for path in paths if not is_excluded(path, excluded)]
	mypy = find_tool("mypy")
	if not mypy:
		print("mypy is not installed; types not checked", file=sys.stderr)
	elif kept:
		for line in mypy_findings(mypy, kept):
			print(line)
			findings += 1
	for path in kept:
		if path in SPACE_INDENTED:
			continue
		for number in space_indents(path):
			print(f"{path}:{number}: indentation uses spaces, not tabs")
			findings += 1
	return 1 if findings else 0


if __name__ == "__main__":
	sys.exit(main(sys.argv[1:]))

##	History:
##		- 20261004 JC: Created.
##		- 20261005 JC: mypy, after ruff.
