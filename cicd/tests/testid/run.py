#!/usr/bin/env python3

##	Purpose:
##		The test ID tool, run on a made-up tree: IDs in whole minutes in
##		Crockford base 32, the next free minute for tests made together, and a
##		check that refuses a missing, malformed, old base 62, future or shared ID.
##		Test result lines as PASS, FAIL or SKIP, ID and name.
##	Syntax: run.py
##	Exit: 0 all passed, 1 a check failed.
##	Test ID: dejjp

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _testdir  # noqa: E402
_testdir.use()

REPO = Path(__file__).resolve().parents[3]
TOOL = REPO / "cicd" / "utility" / "test-id.py"
ENV = {**os.environ, "TZ": "UTC"}

failures = 0
def check(what: str, ok: object, detail: str = "") -> None:
	global failures
	if ok:
		print(f"  ok   {what}")
	else:
		failures += 1
		print(f"  FAIL {what}{': ' + detail if detail else ''}")


def run(root: Path, *args: str, stdin: str = "") -> tuple[int, str]:
	p = subprocess.run([sys.executable, str(root / "cicd" / "utility" / "test-id.py"), *args],
		input=stdin, capture_output=True, text=True, env=ENV, check=False)
	return p.returncode, p.stdout + p.stderr


def rust(ids: list[str | None]) -> str:
	out = "#[cfg(test)]\nmod tests {\n"
	for n, tid in enumerate(ids):
		out += (f"\t// Test ID: {tid}\n" if tid else "") + f"\t#[test]\n\tfn case_{n}() {{}}\n"
	return out + "}\n"


def tree(ids: list[str | None], script_id: str = "decp2") -> Path:
	root = Path(tempfile.mkdtemp(dir=scratch))
	(root / "cicd" / "utility").mkdir(parents=True)
	shutil.copy(TOOL, root / "cicd" / "utility" / "test-id.py")
	(root / "source" / "src").mkdir(parents=True)
	(root / "source" / "src" / "main.rs").write_text(rust(ids))
	(root / "cicd" / "tests" / "one").mkdir(parents=True)
	(root / "cicd" / "tests" / "one" / "run.bash").write_text(f"#!/bin/bash\n##	Test ID: {script_id}\n")
	return root


scratch = Path(tempfile.mkdtemp())
empty = tree([])

rc, out = run(empty, "--decode", "decp8")
check("decp8 is 2026-10-24 22:00 UTC", rc == 0 and out.strip() == "2026-10-24T22:00+00:00", out.strip())
rc, out = run(empty, "--at", "2026-10-24 22:00:59", "3")
check("tests made in one minute take that minute and the next free ones", out.split() == ["decp8", "decp9", "decpa"], out.strip())
rc, out = run(tree(["decp8", "decpa"]), "--at", "2026-10-24 22:00", "2")
check("a minute already taken is skipped", out.split() == ["decp9", "decpb"], out.strip())
rc, out = run(empty, "--decode", "Er2UgY7")
check("an old base 62 ID is not read as one", rc == 2, out.strip())

rc, out = run(tree(["decp8", "decp9"]), "--check")
check("a tree with every ID given and unique passes", rc == 0 and "3 test IDs, all unique" in out, out.strip())
cases: list[tuple[str, list[str | None], str, str]] = [
	("a missing ID", ["decp8", None], "decp2", "no test ID: source/src/main.rs"),
	("a shared ID", ["decp8", "decp8"], "decp2", "test ID decp8 is also at"),
	("a script sharing a Rust test's ID", ["decp8"], "decp8", "test ID decp8 is also at"),
	("an old base 62 ID", ["Er2UgY7"], "decp2", "test ID Er2UgY7 is not a plausible time"),
	("a letter Crockford leaves out", ["decpl"], "decp2", "test ID decpl is not a plausible time"),
	("upper case", ["DECP8"], "decp2", "test ID DECP8 is not a plausible time"),
	("an ID from before 2026", ["d0000"], "decp2", "test ID d0000 is not a plausible time"),
	("an ID more than a day ahead", ["zzzzz"], "decp2", "test ID zzzzz is not a plausible time"),
]
for what, ids, script, says in cases:
	rc, out = run(tree(ids, script), "--check")
	check(f"the check refuses {what}", rc == 1 and says in out, out.strip())

results = "test tests::case_0 ... ok\ntest tests::gone ... FAILED\ntest tests::case_1 ... ignored, needs b29w\ntest tests::case_0 ... bench\nrunning 2 tests\n"
rc, out = run(tree(["decp8", "decp9"]), "--annotate", stdin=results)
lines = out.splitlines()
check("a passing test is a PASS line with its ID and name", lines[:1] == ["PASS decp8 tests::case_0"], repr(lines[:1]))
check("a failed one is FAIL, with dashes as wide as an ID when it has none", lines[1:2] == ["FAIL ----- tests::gone"], repr(lines[1:2]))
check("an ignored one is SKIP, with its why", lines[2:3] == ["SKIP decp9 tests::case_1, needs b29w"], repr(lines[2:3]))
check("a status it doesn't know goes through as is", lines[3:4] == ["test tests::case_0 ... bench"], repr(lines[3:4]))
check("and every other line goes through", "running 2 tests" in lines, out)
check("and the missing ID is counted", lines[-1:] == ["WARNING: 1 test(s) above have no ID found for them"], repr(lines[-1:]))
p = subprocess.run([sys.executable, str(tree(["decp8"]) / "cicd" / "utility" / "test-id.py"), "--annotate"],
	input=b"\xff\xfe not text\r\ntest tests::case_0 ... ok\r\n", capture_output=True, env=ENV, check=False)
check("a line that isn't UTF-8 goes through byte for byte", p.stdout.startswith(b"\xff\xfe not text\r\n"), repr(p.stdout[:40]))
check("and a CRLF result line still gets its ID", b"PASS decp8 tests::case_0\n" in p.stdout, repr(p.stdout))
cmd = "import sys; print('test tests::case_0 ... ok'); print('to stderr', file=sys.stderr); sys.exit(7)"
rc, out = run(tree(["decp8"]), "--run", sys.executable, "-c", cmd)
check("--run annotates a command's output", "PASS decp8 tests::case_0" in out.splitlines(), out.strip())
check("and exits with its status", rc == 7, str(rc))
rc, out = run(tree(["decp8"]), "--run", sys.executable, "-c", "pass")
check("which is 0 when it passed", rc == 0, f"{rc}: {out.strip()}")
rc, out = run(tree(["decp8"]), "--run", str(scratch / "no-such-command"))
check("a command that isn't there fails", rc != 0 and "cannot run" in out, f"{rc}: {out.strip()}")
rc, out = run(tree(["decp8"]), "--list")
check("the list gives ID and name", out.splitlines() == ["decp8  tests::case_0"], out.strip())

real = subprocess.run([sys.executable, str(TOOL), "--check"], capture_output=True, text=True, check=False)
check("every test in this tree has an ID in the new form, and no two share one", real.returncode == 0, real.stdout.strip()[-300:])

shutil.rmtree(scratch, ignore_errors=True)
_testdir.end(1 if failures else 0)
if failures:
	print(f"{failures} failed")
	sys.exit(1)
print("all passed")

##	History:
##		- 20261028: Created.
##		- 20261028: PASS, FAIL and SKIP lines, and --run.
