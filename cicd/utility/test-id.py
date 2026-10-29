#!/usr/bin/env python3

##	Purpose:
##		Test IDs. An ID is the time a test was written, as whole minutes since
##		2000-01-01 00:00 UTC, in lowercase Crockford base 32, such as "decp8"
##		for 2026-10-24 22:00 UTC. Tests made in one minute take the next free
##		minutes, and an ID never changes once given. A Rust test carries it
##		on a "// Test ID:" line just above its attributes; a test script carries
##		it on a "Test ID:" line in its header comment. A script that runs cases
##		of its own, such as the scroll harness's scenes, gives each its ID as
##		the first word of its row in a case table, listed in CASE_TABLES. Each
##		fuzz target is a Rust test of its own, so it has its own ID.
##	Syntax:
##		test-id.py [N]            print N new IDs for now (default 1)
##		test-id.py --at WHEN [N]  the same for a past time, "YYYY-MM-DD HH:MM" local
##		test-id.py --decode ID    the time an ID stands for
##		test-id.py --check        every test has a well-formed ID and no two share one
##		test-id.py --annotate     copy cargo test output, each result line as PASS, FAIL or SKIP, ID, name
##		test-id.py --run CMD...   the same over CMD's output, exiting with its status
##		test-id.py --list         every Rust test, as ID and name
##	Exit: 0 fine, 1 --check found a problem, 2 bad arguments.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import argparse
import re
import subprocess
import sys
from collections.abc import Iterator
from datetime import datetime, timezone
from pathlib import Path
from typing import BinaryIO

DIGITS = "0123456789abcdefghjkmnpqrstvwxyz"
ID_FORM = re.compile(f"[{DIGITS}]+")
NO_ID = "-----"
EPOCH = datetime(2000, 1, 1, tzinfo=timezone.utc)
ROOT = Path(__file__).resolve().parents[2]

ID_RE = re.compile(r"^\s*(?://|#+)\s*(?:-\s*)?Test ID:\s*(\S*)\s*$")
TEST_ATTR_RE = re.compile(r"^\s*#\[test\]\s*$")
ATTR_RE = re.compile(r"^\s*#\[.*\]\s*$")
MOD_RE = re.compile(r"^(\t*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{")
FN_RE = re.compile(r"\bfn\s+(\w+)")
RESULT_RE = re.compile(r"^test (\S+) \.\.\. (.*)$")
SOAK_RE = re.compile(r"\bfuzz::soak\(\s*\"([^\"]*)\"")
FN_LINE_RE = re.compile(r"^(\t*)(?:pub(?:\([^)]*\))?\s+)?fn\s")
STATUS = {"ok": "PASS", "FAILED": "FAIL", "ignored": "SKIP"}
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

## Script tests that run cases of their own, each case with its own ID: the
## script under cicd/tests, and the bash array whose rows are the cases, one
## per line as "<ID> <name> ...".
CASE_TABLES = {
	"scroll/run.bash": "scenes",
}


def encode(minutes: int) -> str:
	out = ""
	while True:
		minutes, r = divmod(minutes, 32)
		out = DIGITS[r] + out
		if not minutes:
			return out


def decode(tid: str) -> int:
	minutes = 0
	for c in tid:
		minutes = minutes * 32 + DIGITS.index(c)
	return minutes


def to_minutes(when: datetime) -> int:
	return int((when - EPOCH).total_seconds() // 60)


def from_minutes(minutes: int) -> datetime:
	return datetime.fromtimestamp(EPOCH.timestamp() + minutes * 60, timezone.utc).astimezone()


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


def table_cases(path: Path, table: str) -> list[tuple[str, str]] | None:
	##	A case table's rows as (where, first word), or None when the script has
	##	no such table, so a renamed one can't leave its cases unchecked.
	lines = path.read_text(encoding="utf-8").splitlines()
	try:
		start = lines.index(f"{table}=(")
	except ValueError:
		return None
	rows: list[tuple[str, str]] = []
	for n in range(start + 1, len(lines)):
		row = lines[n].strip()
		if row == ")":
			return rows
		if row and not row.startswith("#"):
			words = row.strip("\"'").split()
			name = words[1] if len(words) > 1 else "?"
			rows.append((f"{path.relative_to(ROOT)}:{n + 1} ({name})", words[0] if words else ""))
	return None


def find_ids() -> list[tuple[str, str | None]]:
	##	Every test as (where, id or None), the cases in a case table included.
	found = [(where, tid) for where, tid, _ in rust_tests()]
	for path in script_tests():
		head = path.read_text(encoding="utf-8").splitlines()[:60]
		ids = [m.group(1) for m in map(ID_RE.match, head) if m]
		found.append((str(path.relative_to(ROOT)), ids[0] if ids else None))
		table = CASE_TABLES.get(path.relative_to(ROOT / "cicd" / "tests").as_posix())
		for where, tid in (table_cases(path, table) or []) if table else []:
			found.append((where, tid or None))
	return found


def table_problems() -> list[str]:
	##	A case table the check was told about and can't find.
	problems = []
	for rel, table in CASE_TABLES.items():
		path = ROOT / "cicd" / "tests" / rel
		if path.is_file() and table_cases(path, table) is None:
			problems.append(f"no case table {table}=( ... ) in cicd/tests/{rel}")
	return problems


def fuzz_problems() -> list[str]:
	##	Each fuzz target has to be the body of a Rust test of its own, or its
	##	result and ID would be another target's. A target is a fuzz::soak call.
	problems = []
	for path in rust_files():
		if path.name == "fuzz.rs":
			continue
		lines = path.read_text(encoding="utf-8").splitlines()
		bodies: list[tuple[int, int, str]] = []
		for n, line in enumerate(lines):
			if not TEST_ATTR_RE.match(line):
				continue
			at = next((k for k in range(n + 1, min(n + 20, len(lines))) if FN_LINE_RE.match(lines[k])), None)
			head = FN_LINE_RE.match(lines[at]) if at is not None else None
			if at is None or head is None:
				continue
			indent = head.group(1)
			end = next((k for k in range(at + 1, len(lines)) if lines[k] == indent + "}"), len(lines))
			fn = FN_RE.search(lines[at])
			bodies.append((at, end, fn.group(1) if fn else "?"))
		owner: dict[int, str] = {}
		for n, line in enumerate(lines):
			m = SOAK_RE.search(line)
			if not m:
				continue
			where = f"{path.relative_to(ROOT)}:{n + 1}"
			body = next((b for b in bodies if b[0] < n < b[1]), None)
			if body is None:
				problems.append(f"fuzz target {m.group(1)} is not in a test of its own: {where}")
			elif body[0] in owner:
				problems.append(f"fuzz target {m.group(1)} shares test {body[2]} with {owner[body[0]]}: {where}")
			else:
				owner[body[0]] = m.group(1)
	return problems


def annotate(lines: Iterator[bytes], out: BinaryIO) -> None:
	##	Each result line as "PASS|FAIL|SKIP <ID> <name>[, why]", and every other
	##	line as it came, byte for byte. It reads to the end, so cargo never sees
	##	a closed pipe.
	ids = {name: tid for _, tid, name in rust_tests()}
	missing = 0
	for raw in lines:
		m = RESULT_RE.match(raw.decode("utf-8", "replace").rstrip("\r\n"))
		status, _, why = m.group(2).partition(", ") if m else ("", "", "")
		if m is None or status not in STATUS:
			out.write(raw)
			out.flush()
			continue
		name = m.group(1)
		tid = ids.get(name)
		missing += tid is None
		out.write(f"{STATUS[status]} {tid or NO_ID} {name}{', ' + why if why else ''}\n".encode())
		out.flush()
	if missing:
		out.write(f"WARNING: {missing} test(s) above have no ID found for them\n".encode())
		out.flush()


def run_annotated(cmd: list[str]) -> int:
	##	The same over a command's stdout, keeping its exit status, for a shell
	##	with no pipefail (cicd-win.ps1). Its stderr goes straight through.
	try:
		proc = subprocess.Popen(cmd, stdout=subprocess.PIPE)
	except OSError as e:
		print(f"cannot run {cmd[0]}: {e}", file=sys.stderr)
		return 127
	assert proc.stdout is not None
	try:
		annotate(iter(proc.stdout.readline, b""), sys.stdout.buffer)
	except KeyboardInterrupt:
		proc.wait()
		raise
	return proc.wait()


def list_rust() -> int:
	for _, tid, name in rust_tests():
		print(f"{tid or NO_ID:<5}  {name}")
	return 0


def check() -> int:
	now = to_minutes(datetime.now(timezone.utc)) + 1440
	first = to_minutes(datetime(2026, 1, 1, tzinfo=timezone.utc))
	seen: dict[str, str] = {}
	bad = 0
	for where, tid in find_ids():
		if tid is None:
			print(f"no test ID: {where}")
		elif not ID_FORM.fullmatch(tid) or not first <= decode(tid) <= now:
			print(f"test ID {tid} is not a plausible time: {where}")
		elif tid in seen:
			print(f"test ID {tid} is also at {seen[tid]}: {where}")
		else:
			seen[tid] = where
			continue
		bad += 1
	for problem in table_problems() + fuzz_problems():
		print(problem)
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
	ap.add_argument("--list", action="store_true")
	ap.add_argument("--run", nargs=argparse.REMAINDER, metavar="CMD")
	args = ap.parse_args()

	if args.check:
		return check()
	if args.run is not None:
		if not args.run:
			ap.error("--run needs a command")
		return run_annotated(args.run)
	if args.annotate:
		annotate(iter(sys.stdin.buffer.readline, b""), sys.stdout.buffer)
		return 0
	if args.list:
		return list_rust()
	if args.decode:
		if not ID_FORM.fullmatch(args.decode):
			ap.error(f"not a test ID: {args.decode}")
		print(from_minutes(decode(args.decode)).isoformat(timespec="minutes"))
		return 0

	if args.at:
		try:
			when = datetime.fromisoformat(args.at).astimezone()
		except ValueError:
			ap.error(f"cannot read the time: {args.at}")
	else:
		when = datetime.now(timezone.utc)
	##	Several tests made at once get one minute each, past any already taken.
	taken = {tid for _, tid in find_ids() if tid}
	minutes = to_minutes(when)
	for _ in range(args.count):
		while encode(minutes) in taken:
			minutes += 1
		taken.add(encode(minutes))
		print(encode(minutes))
	return 0


if __name__ == "__main__":
	sys.exit(main())


##	History:
##		- 20260926: Created.
##		- 20260927: --annotate, for the test lines cicd prints.
##		- 20261005: PEP 8 names and type hints.
##		- 20261006: Module paths take the folder for a file under src/<module>/.
##		- 20261026: --list, for cicd.bash --list.
##		- 20261028: IDs in whole minutes, in Crockford base 32. The base 62
##		  millisecond IDs were converted once, from the times they stood for.
##		- 20261028: --annotate prints PASS, FAIL or SKIP lines. --run, for
##		  cicd-win.ps1.
##		- 20261028: --check takes the scroll harness's scenes, each with an
##		  ID of its own, and refuses 2 fuzz targets in one test.
