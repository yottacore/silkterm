#!/usr/bin/env python3

##	- Purpose:
##		The Python copy of the safe removal guard, saferm.py, with the cases
##		saferm.bash's test (dehwp) gives the bash one, and _testdir.rm(), which
##		the Python tests remove their own folders with. Run by dehwp.
##	- Test ID: depj1
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import contextlib
import io
import os
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _testdir  # noqa: E402
import saferm  # noqa: E402  ## on the path _testdir set

folder = _testdir.use()
REPO = Path(__file__).resolve().parents[3]

failures = 0
def check(what: str, ok: object, detail: str = "") -> None:
	global failures
	if ok:
		print(f"  ok   {what}")
	else:
		print(f"  FAIL {what}{f': {detail}' if detail else ''}")
		failures += 1

def said(call: Callable[[], bool]) -> tuple[bool, str]:
	err = io.StringIO()
	with contextlib.redirect_stderr(err):
		answer = call()
	return answer, err.getvalue().strip()

def refused(call: Callable[[], bool], why: str = "") -> bool:
	answer, text = said(call)
	return answer is False and text.startswith(f"safe-rm: refused, {why}")

def mk(*paths: Path) -> None:
	for p in paths:
		p.mkdir(parents=True, exist_ok=True)

work = Path(os.path.realpath(tempfile.mkdtemp(prefix="silk-saferm-py.")))
tmp = work / "tmp"

## Under a tmp folder.
mk(tmp / "a/b", tmp / "keep", tmp / "c")
(tmp / "a/b/f").write_text("x")
(tmp / "keep/f").write_text("k")
answer, text = said(lambda: saferm.rm(tmp / "a"))
check("a folder under a tmp folder goes", answer and not (tmp / "a").exists())
check("and nothing is printed", text == "", text)
mk(tmp / "s")
check("a trailing slash is the same folder", saferm.rm(f"{tmp}/s/") and not (tmp / "s").exists())
(tmp / "lnk").symlink_to(tmp / "keep")
check("a link is refused", refused(lambda: saferm.rm(tmp / "lnk"), "a link"))
check("and it and what it points at stay", (tmp / "lnk").is_symlink() and (tmp / "keep/f").is_file())
check("a path through a link is refused", refused(lambda: saferm.rm(tmp / "lnk/f"), "goes through a link"))
check("and what it reaches stays", (tmp / "keep/f").is_file())
check("a path with .. is refused", refused(lambda: saferm.rm(f"{tmp}/c/../keep"), "bad path"))
check("a path with // is refused", refused(lambda: saferm.rm(f"{tmp}//keep"), "bad path"))
check("a path with a newline is refused", refused(lambda: saferm.rm(f"{tmp}/keep\n"), "bad path"))
check("an empty path is refused", refused(lambda: saferm.rm(""), "bad path"))
check("a relative path is refused", refused(lambda: saferm.rm("tmp/x"), "bad path"))
check("a path that isn't there is refused", refused(lambda: saferm.rm(tmp / "nope"), "not there"))
check("the tmp folder itself is refused", refused(lambda: saferm.rm("/tmp"), "not under a tmp folder"))
check("so is the root", refused(lambda: saferm.rm("/"), "not under a tmp folder"))
check("and the tmp folders stay", (tmp / "c").is_dir() and (tmp / "keep/f").is_file())
## A link inside the tree goes as a link, and what it points at stays.
mk(tmp / "tree/d")
(tmp / "tree/d/out").symlink_to(tmp / "keep")
check("a tree with a link in it goes", saferm.rm(tmp / "tree") and not (tmp / "tree").exists())
check("and the link wasn't followed", (tmp / "keep/f").is_file())
## A folder on another filesystem is left in place, as rm --one-file-system does.
mk(tmp / "mnt/x")
try:
	saferm._walk(str(tmp / "mnt"), os.lstat(tmp / "mnt").st_dev + 1)
	crossed = True
except OSError:
	crossed = False
check("the walk won't cross into another filesystem", not crossed and (tmp / "mnt/x").is_dir())

## Outside any tmp folder, under the target dir, whose real path has none.
target = Path(os.path.realpath(os.environ.get("CARGO_TARGET_DIR") or REPO / "target"))
out: Path | None = None
if "/tmp/" in f"{target}/":
	print(f"  skip the cases outside a tmp folder: the target dir is under one ({target})")
else:
	target.mkdir(parents=True, exist_ok=True)
	o = out = Path(tempfile.mkdtemp(prefix="silk-saferm-py.", dir=target))
	mk(o / "tmp/r1", o / "keep/sub", o / "keepnot/x")
	(o / "keep/f").write_text("k")
	check("a folder outside a tmp folder is refused", refused(lambda: saferm.rm(o / "keep"), "not under a tmp folder"))
	check("a folder in a tmp folder there goes", saferm.rm(o / "tmp/r1") and not (o / "tmp/r1").exists())
	check("the tmp folder itself is refused", refused(lambda: saferm.rm(o / "tmp"), "not under a tmp folder"))
	## Under one named parent.
	check("under the named parent goes", saferm.rm_under(o / "keep", o / "keep/sub") and not (o / "keep/sub").exists())
	check("the parent itself is refused", refused(lambda: saferm.rm_under(o / "keep", o / "keep"), "not under"))
	check("a name that only starts like it is refused", refused(lambda: saferm.rm_under(o / "keep", o / "keepnot/x"), "not under"))
	check("no parent is refused", refused(lambda: saferm.rm_under("", o / "keep"), "no parent"))
	check("a relative parent is refused", refused(lambda: saferm.rm_under("keep", o / "keep"), "no parent"))
	check("the root as parent is refused", refused(lambda: saferm.rm_under("/", o / "keep"), "no parent"))
	check("and all of them stay", (o / "keep/f").is_file() and (o / "keepnot/x").is_dir())

## HOST_TEMP itself and a whole project folder in it, under a tmp folder, where
## the tmp rule alone would take them. Real folders, since a path that isn't
## there gets refused anyway.
ht = tmp / "ht"
mk(*(ht / d / "r1" for d in ("silkterm", "other", "loose", "sock")))
(tmp / "htl").symlink_to(ht)
saved = saferm.host, saferm.own, saferm.host_set
saferm.host, saferm.own, saferm.host_set = str(ht), str(ht / "silkterm"), True
whole = "a whole project temp folder"
check("HOST_TEMP itself is refused", refused(lambda: saferm.rm(ht), whole))
check("so is this project's folder in it", refused(lambda: saferm.rm(ht / "silkterm"), whole))
check("and another project's", refused(lambda: saferm.rm(ht / "other"), whole))
check("with a trailing slash too", refused(lambda: saferm.rm(f"{ht}/other/"), whole))
check("and all of them stay", (ht / "silkterm/r1").is_dir() and (ht / "other/r1").is_dir())
check("a run folder in another project's folder goes", saferm.rm(ht / "other/r1"))
check("and one in this project's", saferm.rm(ht / "silkterm/r1"))
check("the one-parent form won't take HOST_TEMP", refused(lambda: saferm.rm_under(tmp, ht), whole))
check("or this project's folder", refused(lambda: saferm.rm_under(ht, ht / "silkterm"), whole))
check("but takes a folder made right in it, as the test run folders are", saferm.rm_under(ht, ht / "loose"))
saferm.host_set = False
check("with no HOST_TEMP, the fallback itself is refused", refused(lambda: saferm.rm(ht), whole))
check("and this project's folder in it", refused(lambda: saferm.rm(ht / "silkterm"), whole))
check("while a folder right in it goes, as the socket folders in /tmp do", saferm.rm(ht / "sock"))
saferm.host, saferm.own, saferm.host_set = saved
check("and HOST_TEMP's folders are still there",
	(ht / "silkterm").is_dir() and (ht / "other").is_dir() and not (ht / "loose").exists() and not (ht / "sock").exists())
## As the guard reads it at import, from the environment, through a link.
env = {**os.environ, "HOST_TEMP": f"{tmp}/htl/"}
probe = "import sys, saferm; sys.exit(0 if not saferm.rm(sys.argv[1]) else 1)"
got = subprocess.run([sys.executable, "-I", "-c", f"import sys; sys.path.insert(0, {str(Path(saferm.__file__).parent)!r}); {probe}", str(ht / "other")],
	env=env, capture_output=True, text=True, check=False)
check("HOST_TEMP from the environment counts, resolved through a link", got.returncode == 0 and whole in got.stderr, got.stderr.strip())
check("and the folder stays", (ht / "other").is_dir())

## A test's own folders, through _testdir.rm(), under the run's test folder
## only. A stand-in test folder, since this one's work folder is in the real one.
td = work / "td"
mk(td / "run/w1/sub", td / "other")
(td / "other/f").write_text("k")
(td / "via").symlink_to(td / "run")
def td_rm(test_dir: Path, p: Path) -> bool:
	given = os.environ["SILKTERM_TEST_DIR"]
	os.environ["SILKTERM_TEST_DIR"] = str(test_dir)
	try:
		return _testdir.rm(p)
	finally:
		os.environ["SILKTERM_TEST_DIR"] = given
check("a folder in the test folder goes", td_rm(td / "run", td / "run/w1") and not (td / "run/w1").exists() and (td / "run").is_dir())
mk(td / "run/w2")
check("so does one named by a test folder behind a link", td_rm(td / "via", td / "via/w2") and not (td / "run/w2").exists())
check("a path that isn't there is left alone", td_rm(td / "run", td / "run/nope"))
check("the test folder itself is refused", refused(lambda: td_rm(td / "run", td / "run"), "not under"))
check("one outside it is refused", refused(lambda: td_rm(td / "run", td / "other"), "not under"))
(td / "run/lnk").symlink_to(td / "other")
check("a link in it is refused", refused(lambda: td_rm(td / "run", td / "run/lnk"), "a link"))
check("and what it points at stays", (td / "other/f").is_file())
answer, text = said(lambda: td_rm(work / "gone", td / "other"))
check("with no test folder it's refused", answer is False and "no folder" in text, text)
check("and still stays", (td / "other/f").is_file())

if out:
	saferm.rm_under(target, out)
_testdir.rm(work)
print(f"{failures} failed" if failures else "all passed")
_testdir.end(1 if failures else 0)
sys.exit(1 if failures else 0)

##	History:
##		- 20261031 JC: Created.
