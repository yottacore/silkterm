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
##	Exit: 0 fine, 1 --check found a problem, 2 bad arguments.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

DIGITS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
EPOCH = datetime(2000, 1, 1, tzinfo=timezone.utc)
ROOT = Path(__file__).resolve().parents[2]

ID_RE = re.compile(r"^\s*(?://|#+)\s*(?:-\s*)?Test ID:\s*(\S*)\s*$")
TEST_ATTR_RE = re.compile(r"^\s*#\[test\]\s*$")
ATTR_RE = re.compile(r"^\s*#\[.*\]\s*$")
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


def fEncode(ms):
	out = ""
	while True:
		ms, r = divmod(ms, 62)
		out = DIGITS[r] + out
		if not ms:
			return out


def fDecode(tid):
	ms = 0
	for c in tid:
		ms = ms * 62 + DIGITS.index(c)
	return ms


def fMs(when):
	return int(round((when - EPOCH).total_seconds() * 1000))


def fWhen(ms):
	return datetime.fromtimestamp(EPOCH.timestamp() + ms / 1000, timezone.utc).astimezone()


def fRustFiles():
	for top in ("source", "cicd/tests"):
		for path in sorted((ROOT / top).rglob("*.rs")):
			if not NOT_TEST_DIRS & set(path.relative_to(ROOT).parts):
				yield path


def fScriptTests():
	tests = ROOT / "cicd" / "tests"
	for path in sorted(tests.rglob("*")):
		rel = path.relative_to(tests)
		if (path.is_file() and path.suffix in SCRIPT_EXTS and not path.name.startswith("_")
				and rel.as_posix() not in NOT_TESTS and not NOT_TEST_DIRS & set(rel.parts)):
			yield path


def fFindIds():
	##	Every test as (where, id or None). A Rust test's ID sits above its whole
	##	attribute block, so a #[cfg] ahead of #[test] does not hide it.
	found = []
	for path in fRustFiles():
		lines = path.read_text(encoding="utf-8").splitlines()
		for n, line in enumerate(lines):
			if not TEST_ATTR_RE.match(line):
				continue
			top = n
			while top > 0 and ATTR_RE.match(lines[top - 1]):
				top -= 1
			m = ID_RE.match(lines[top - 1]) if top else None
			found.append((f"{path.relative_to(ROOT)}:{n + 1}", m.group(1) if m else None))
	for path in fScriptTests():
		head = path.read_text(encoding="utf-8").splitlines()[:60]
		ids = [m.group(1) for m in map(ID_RE.match, head) if m]
		found.append((str(path.relative_to(ROOT)), ids[0] if ids else None))
	return found


def fCheck():
	now = fMs(datetime.now(timezone.utc)) + 86_400_000
	first = fMs(datetime(2026, 1, 1, tzinfo=timezone.utc))
	seen, bad = {}, 0
	for where, tid in fFindIds():
		if tid is None:
			print(f"no test ID: {where}")
		elif not re.fullmatch(r"[0-9A-Za-z]+", tid) or not first <= fDecode(tid) <= now:
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


def main():
	ap = argparse.ArgumentParser(description="Make, read or check test IDs.")
	ap.add_argument("count", nargs="?", type=int, default=1)
	ap.add_argument("--at", metavar="WHEN")
	ap.add_argument("--decode", metavar="ID")
	ap.add_argument("--check", action="store_true")
	args = ap.parse_args()

	if args.check:
		return fCheck()
	if args.decode:
		if not re.fullmatch(r"[0-9A-Za-z]+", args.decode):
			ap.error(f"not base 62: {args.decode}")
		print(fWhen(fDecode(args.decode)).isoformat(timespec="milliseconds"))
		return 0

	if args.at:
		try:
			when = datetime.fromisoformat(args.at).astimezone()
		except ValueError:
			ap.error(f"cannot read the time: {args.at}")
	else:
		when = datetime.now(timezone.utc)
	##	Several tests made at once get one millisecond each, past any already taken.
	taken = {tid for _, tid in fFindIds() if tid}
	ms = fMs(when)
	for _ in range(args.count):
		while fEncode(ms) in taken:
			ms += 1
		taken.add(fEncode(ms))
		print(fEncode(ms))
	return 0


if __name__ == "__main__":
	sys.exit(main())


##	History:
##		- 20260926: Created.
