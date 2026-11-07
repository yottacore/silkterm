##	- Purpose:
##		The run's test folder, HOST_TEMP/silkterm/YYYYmmDD-HHMMSS_NNN with a
##		run.pid, or under the temp dir without HOST_TEMP, for the Python tests.
##		Same contract as _testdir.bash: SILKTERM_TEST_DIR set and not
##		empty is the folder, made if missing, and never removed; otherwise a fresh
##		0700 one is made and exported, so every test the caller starts shares it.
##		A script that made one calls end(<exit status>) right before it exits,
##		which removes the folder on 0 or 3 (a skip) and keeps it otherwise, unless
##		it holds nothing but its owner mark and run.pid. rm(<path>) removes a
##		folder tree inside it, as fTestDir_Rm does.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import os
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "utility" / "include"))
import saferm  # noqa: E402

_MARKER = ".test_silkterm_owner"
## Kept out of the environment, so a child never thinks the folder is its own.
_owned: tuple[Path, str] | None = None


def make() -> Path:
	global _owned
	given = os.environ.get("SILKTERM_TEST_DIR", "")
	if given:
		folder = Path(given)
		folder.mkdir(parents=True, exist_ok=True)
		return folder
	base = Path(os.environ.get("HOST_TEMP") or tempfile.gettempdir()) / "silkterm"
	base.mkdir(parents=True, exist_ok=True)
	stamp = f"{datetime.now():%Y%m%d-%H%M%S}"
	for n in range(1, 1000):
		folder = base / f"{stamp}_{n:03d}"
		## Never exist_ok here: it would adopt a folder or symlink planted at a guessable name.
		try:
			folder.mkdir(mode=0o700)
		except FileExistsError:
			continue
		token = f"{os.getpid()}-{time.time_ns()}"
		(folder / _MARKER).write_text(f"{token}\n")
		(folder / "run.pid").write_text(f"{os.getpid()}\n")
		_owned = (folder, token)
		os.environ["SILKTERM_TEST_DIR"] = str(folder)
		return folder
	raise FileExistsError(f"test run folder: '{folder}' and every name before it were taken")


def _not_ours(folder: Path, token: str) -> str:
	if folder.is_symlink():
		return "it is a link"
	if not folder.is_dir():
		return "not a folder"
	marker = folder / _MARKER
	if marker.is_symlink() or not marker.is_file():
		return "no owner mark"
	try:
		lines = marker.read_text().splitlines()
	except OSError:
		return "owner mark unreadable"
	if not lines or lines[0].strip() != token:
		return "another run's owner mark"
	return ""


def only_marks(folder: Path) -> bool:
	## True when folder holds nothing but the owner mark and run.pid.
	try:
		return (
			folder.is_dir()
			and not folder.is_symlink()
			and all(e.name in (_MARKER, "run.pid") for e in os.scandir(folder))
		)
	except OSError:
		return False


def end(status: int) -> None:
	## Removes the folder make() made, when status is 0 or 3 (a skip), or it
	## holds nothing else, and the folder still has this run's mark. Never
	## raises, so it never changes how a script exits.
	global _owned
	if _owned is None:
		return
	folder, token = _owned
	_owned = None
	if status not in (0, 3) and not only_marks(folder):
		print(f"test files kept in {folder}", file=sys.stderr)
		return
	reason = _not_ours(folder, token)
	if reason:
		print(f"test run folder: left {folder} in place: {reason}", file=sys.stderr)
		return
	## Under the temp folder it was made in, resolved, since that can sit behind a link.
	up = os.path.realpath(folder.parent)
	if not saferm.rm_under(up, f"{up}/{folder.name}"):
		print(f"test run folder: could not remove {folder}", file=sys.stderr)


def rm(path: "str | os.PathLike[str]") -> bool:
	## A folder tree inside this run's test folder, through the guard with that
	## folder as the one parent. A path that isn't there is left alone, as
	## rmtree(ignore_errors=True) did. False, with the reason on stderr, when the
	## guard refuses.
	p = os.fspath(path)
	if not os.path.lexists(p):
		return True
	given = os.environ.get("SILKTERM_TEST_DIR", "")
	try:
		root = os.path.realpath(given, strict=True) if given else ""
	except OSError:
		root = ""
	if not root:
		print(f"test run folder: no folder to remove {p} from", file=sys.stderr)
		return False
	## A path built on the folder's name before it was resolved, as TMPDIR is.
	if p.startswith(given + "/"):
		p = root + p[len(given):]
	return saferm.rm_under(root, p)


def use() -> Path:
	## make(), then this process's tempfile calls and every program it starts
	## write there. tempfile caches its answer, so TMPDIR alone would not move it.
	## TEMP and TMP too, as the pipeline sets them.
	folder = make()
	for name in ("TMPDIR", "TEMP", "TMP"):
		os.environ[name] = str(folder)
	tempfile.tempdir = str(folder)
	return folder


##	History:
##		- 20260930 JC: Created.
##		- 20261002 JC: A run removes the folder it made when it passes.
##		- 20261031: Removals go through the safe removal guard, and rm() for a test's own folders.
##		- 20261031: use() points TEMP and TMP at the folder too.
##		- 20261101: The folder is HOST_TEMP/silkterm/YYYYmmDD-HHMMSS_NNN, with a run.pid.
##		- 20261106: A skip, or a failed run that left nothing in it, removes it too.
