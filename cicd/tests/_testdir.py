##	- Purpose:
##		The run's test folder, <temp>/test_silkterm_YYYYmmDD-HHMMSSNN, for the
##		Python tests. Same contract as _testdir.bash: SILKTERM_TEST_DIR set and not
##		empty is the folder, made if missing, and never removed; otherwise a fresh
##		0700 one is made and exported, so every test the caller starts shares it.
##		A script that made one calls end(<exit status>) right before it exits,
##		which removes the folder on 0 and keeps it otherwise.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import os
import shutil
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

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
	base = Path(tempfile.gettempdir())
	for _ in range(100):
		now = datetime.now()
		folder = base / f"test_silkterm_{now:%Y%m%d-%H%M%S}{now.microsecond // 10000:02d}"
		## Never exist_ok here: it would adopt a folder or symlink planted at a guessable name.
		try:
			folder.mkdir(mode=0o700)
		except FileExistsError:
			time.sleep(0.01)
			continue
		token = f"{os.getpid()}-{time.time_ns()}"
		(folder / _MARKER).write_text(f"{token}\n")
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


def end(status: int) -> None:
	## Removes the folder make() made, when status is 0 and the folder still has
	## this run's mark. Never raises, so it never changes how a script exits.
	global _owned
	if _owned is None:
		return
	folder, token = _owned
	_owned = None
	if status != 0:
		print(f"test files kept in {folder}", file=sys.stderr)
		return
	reason = _not_ours(folder, token)
	if reason:
		print(f"test run folder: left {folder} in place: {reason}", file=sys.stderr)
		return
	## rmtree removes a link inside as a link and never follows it.
	try:
		shutil.rmtree(folder)
	except OSError as e:
		print(f"test run folder: could not remove {folder}: {e}", file=sys.stderr)


def use() -> Path:
	## make(), then this process's tempfile calls and every program it starts
	## write there. tempfile caches its answer, so TMPDIR alone would not move it.
	folder = make()
	os.environ["TMPDIR"] = str(folder)
	tempfile.tempdir = str(folder)
	return folder


##	History:
##		- 20260930 JC: Created.
##		- 20261002 JC: A run removes the folder it made when it passes.
