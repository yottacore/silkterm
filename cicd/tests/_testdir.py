##	- Purpose:
##		The run's test folder, <temp>/test_silkterm_YYYYmmDD-HHMMSSNN, for the
##		Python tests. Same contract as _testdir.bash: SILKTERM_TEST_DIR set and not
##		empty is the folder, made if missing; otherwise a fresh 0700 one is made
##		and exported, so every test the caller starts shares it.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import os
import tempfile
import time
from datetime import datetime
from pathlib import Path


def make() -> Path:
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
		os.environ["SILKTERM_TEST_DIR"] = str(folder)
		return folder
	raise FileExistsError(f"test run folder: '{folder}' and every name before it were taken")


def use() -> Path:
	## make(), then this process's tempfile calls and every program it starts
	## write there. tempfile caches its answer, so TMPDIR alone would not move it.
	folder = make()
	os.environ["TMPDIR"] = str(folder)
	tempfile.tempdir = str(folder)
	return folder


##	History:
##		- 20260930 JC: Created.
