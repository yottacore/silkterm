#!/usr/bin/env python3

##	Purpose:
##		Test IDs. An ID is the time a test was written, as milliseconds since
##		2000-01-01 00:00 UTC, in base 62 (0-9, A-Z, a-z). A Rust test carries it
##		on a "// Test ID:" line just above its attributes; a test script carries
##		it on a "Test ID:" line in its header comment.
##	Syntax:
##		test-id.py [N]            print N new IDs for now (default 1)
##		test-id.py --at WHEN [N]  the same for a past time, "YYYY-MM-DD HH:MM[:SS]" local
##		test-id.py --decode ID    the time an ID stands for
##		test-id.py --check        every test has a well-formed ID and no two share one
##		test-id.py --annotate     copy cargo test output, each result line as status, ID, name
##	Exit: 0 fine, 1 --check found a problem, 2 bad arguments.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import io
import re
import sys
from collections.abc import Iterator
from datetime import datetime, timezone
from pathlib import Path
from typing import cast

DIGITS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
EPOCH = datetime(2000, 1, 1, tzinfo=timezone.utc)
ROOT = Path(__file__).resolve().parents[2]

ID_RE = re.compile(r"^\s*(?://|#+)\s*(?:-\s*)?Test ID:\s*(\S*)\s*$")
TEST_ATTR_RE = re.compile(r"^\s*#\[test\]\s*$")
ATTR_RE = re.compile(r"^\s*#\[.*\]\s*$")
MOD_RE = re.compile(r"^(\t*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{")
FN_RE = re.compile(r"\bfn\s+(\w+)")
RESULT_RE = re.compile(r"^test (\S+) \.\.\. (.*)$")
SCRIPT_EXTS = {".bash", ".py", ".ps1"}

## Scripts under cicd/tests that are not tests themselves: drivers, fixtures
## and pieces the tests call. A name starting with "_" is one too.
NOT_TESTS = {
	"install/stubrun.ps1",
	"scroll/analyze.py",
	"scroll/verdict.bash",
	"wingui/run.bash",
}
NOT_TEST_DIRS = {"fuzz-corpus", "scenes", "target", "__pycache__"}


def encode(ms: int) -> str:
	out = ""
	while True:
		ms, r = divmod(ms, 62)
		out = DIGITS[r] + out
		if not ms:
			return out


def decode(tid: str) -> int:
	ms = 0
	for c in tid:
		ms = ms * 62 + DIGITS.index(c)
	return ms


def to_ms(when: datetime) -> int:
	return int(round((when - EPOCH).total_seconds() * 1000))


def from_ms(ms: int) -> datetime:
	return datetime.fromtimestamp(EPOCH.timestamp() + ms / 1000, timezone.utc).astimezone()


def rust_files() -> Iterator[Path]:
	for top in ("source", "knobs", "cicd/tests"):
		for path in sorted((ROOT / top).rglob("*.rs")):
			if not NOT_TEST_DIRS & set(path.relative_to(ROOT).parts):
				yield path


def script_tests() -> Iterator[Path]:
	tests = ROOT / "cicd" / "tests"
	for path in sorted(tests.rglob("*")):
		rel = path.relative_to(tests)
		if (path.is_file() and path.suffix in SCRIPT_EXTS and not path.name.startswith("_")
				and rel.as_posix() not in NOT_TESTS and not NOT_TEST_DIRS & set(rel.parts)):
			yield path


def rust_tests() -> Iterator[tuple[str, str | None, str]]:
	##	Every Rust test as (where, id or None, name as cargo prints it). A
	##	test's ID sits above its whole attribute block, so a #[cfg] ahead of
	##	#[test] does not hide it. The module path comes from the indent of each
	##	"mod x {", which rustfmt keeps exact, and a file in a module's own folder
	##	under src/ starts with that folder's path.
	for path in rust_files():
		lines = path.read_text(encoding="utf-8").splitlines()
		parts = path.relative_to(ROOT).parts
		folders = list(parts[len(parts) - parts[::-1].index("src"):-1]) if "src" in parts else []
		base: list[str] = folders + ([] if path.stem == "main" else [path.stem])
		mods: list[tuple[str, str]] = []
		for n, line in enumerate(lines):
			m = MOD_RE.match(line)
			if m:
				mods.append((m.group(1), m.group(2)))
				continue
			if mods and line.startswith(mods[-1][0] + "}"):
				mods.pop()
				continue
			if not TEST_ATTR_RE.match(line):
				continue
			top = n
			while top > 0 and ATTR_RE.match(lines[top - 1]):
				top -= 1
			m = ID_RE.match(lines[top - 1]) if top else None
			fn = next((f.group(1) for f in map(FN_RE.search, lines[n + 1:n + 20]) if f), "?")
			name = "::".join(base + [mod for _, mod in mods] + [fn])
			yield f"{path.relative_to(ROOT)}:{n + 1}", m.group(1) if m else None, name


def find_ids() -> list[tuple[str, str | None]]:
	##	Every test as (where, id or None).
	found = [(where, tid) for where, tid, _ in rust_tests()]
	for path in script_tests():
		head = path.read_text(encoding="utf-8").splitlines()[:60]
		ids = [m.group(1) for m in map(ID_RE.match, head) if m]
		found.append((str(path.relative_to(ROOT)), ids[0] if ids else None))
	return found


def annotate() -> int:
	##	A filter on cargo test's stdout, so every other line goes through as is.
	##	It reads to the end, so cargo never sees a closed pipe.
	ids = {name: tid for _, tid, name in rust_tests()}
	cast(io.TextIOWrapper, sys.stdin).reconfigure(errors="surrogateescape")
	cast(io.TextIOWrapper, sys.stdout).reconfigure(errors="surrogateescape")
	missing = 0
	for line in sys.stdin:
		m = RESULT_RE.match(line.rstrip("\n"))
		if not m:
			print(line, end="", flush=True)
			continue
		name, status = m.groups()
		tid = ids.get(name)
		missing += tid is None
		status, _, why = status.partition(", ")
		print(f"{status:<7} {tid or '-------':<7}  {name}{'  (' + why + ')' if why else ''}", flush=True)
	if missing:
		print(f"WARNING: {missing} test(s) above have no ID found for them", flush=True)
	return 0


def check() -> int:
	now = to_ms(datetime.now(timezone.utc)) + 86_400_000
	first = to_ms(datetime(2026, 1, 1, tzinfo=timezone.utc))
	seen: dict[str, str] = {}
	bad = 0
	for where, tid in find_ids():
		if tid is None:
			print(f"no test ID: {where}")
		elif not re.fullmatch(r"[0-9A-Za-z]+", tid) or not first <= decode(tid) <= now:
			print(f"test ID {tid} is not a plausible time: {where}")
		elif tid in seen:
			print(f"test ID {tid} is also at {seen[tid]}: {where}")
		else:
			seen[tid] = where
			continue
		bad += 1
	if bad:
		print(f"{bad} test ID problem(s); make new IDs with cicd/utility/test-id.py")
		return 1
	print(f"{len(seen)} test IDs, all unique")
	return 0


def main() -> int:
	ap = argparse.ArgumentParser(description="Make, read or check test IDs.")
	ap.add_argument("count", nargs="?", type=int, default=1)
	ap.add_argument("--at", metavar="WHEN")
	ap.add_argument("--decode", metavar="ID")
	ap.add_argument("--check", action="store_true")
	ap.add_argument("--annotate", action="store_true")
	args = ap.parse_args()

	if args.check:
		return check()
	if args.annotate:
		return annotate()
	if args.decode:
		if not re.fullmatch(r"[0-9A-Za-z]+", args.decode):
			ap.error(f"not base 62: {args.decode}")
		print(from_ms(decode(args.decode)).isoformat(timespec="milliseconds"))
		return 0

	if args.at:
		try:
			when = datetime.fromisoformat(args.at).astimezone()
		except ValueError:
			ap.error(f"cannot read the time: {args.at}")
	else:
		when = datetime.now(timezone.utc)
	##	Several tests made at once get one millisecond each, past any already taken.
	taken = {tid for _, tid in find_ids() if tid}
	ms = to_ms(when)
	for _ in range(args.count):
		while encode(ms) in taken:
			ms += 1
		taken.add(encode(ms))
		print(encode(ms))
	return 0


if __name__ == "__main__":
	sys.exit(main())


##	History:
##		- 20260926: Created.
##		- 20260927: --annotate, for the test lines cicd prints.
##		- 20261005: PEP 8 names and type hints.
##		- 20261006: Module paths take the folder for a file under src/<module>/.
