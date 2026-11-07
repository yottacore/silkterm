##	Purpose:
##		- The one way a Python script in cicd/ removes a folder tree, per Safe
##		  removal in the CICD rules: saferm.bash's guard, with the same rules.
##		  Import it. It defines functions and runs nothing.
##		- rm(path): only somewhere under a folder named tmp, never that folder
##		  itself, or under this project's own folder in HOST_TEMP. With HOST_TEMP
##		  set, never a whole folder right under it either, where other runs and
##		  sessions keep theirs.
##		- rm_under(parent, path): only somewhere under the one parent named, for
##		  a folder outside a temp root.
##		- Neither takes HOST_TEMP itself or this project's folder in it. Where
##		  HOST_TEMP is unset, that's TMPDIR or /tmp, as in Temp files.
##		- Both take one absolute path and refuse an empty one, "..", "//", a
##		  newline, a link, a path through a link, or one that isn't there. They
##		  stay on one filesystem. True when the removal went; otherwise one line
##		  on stderr, and False.
##		- POSIX paths only. No Python script in cicd/ removes anything on Windows.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

import errno
import os
import re
import stat
import sys
from pathlib import Path

_IN_TMP = re.compile(r"/(.+/)?tmp/[^/]+(/[^/]+)*")

##	Read once, as saferm.bash does at source time. Resolved, since macOS's temp
##	folder sits behind the /var link. A test may set these.
host = os.path.realpath(os.environ.get("HOST_TEMP") or os.environ.get("TMPDIR") or "/tmp")
own = f"{host.rstrip('/')}/{os.environ.get('EXE_NAME') or 'silkterm'}"
##	Without HOST_TEMP the root is /tmp, and the short socket folders sit right in it.
host_set = bool(os.environ.get("HOST_TEMP"))


def _refuse(why: str) -> bool:
	print(f"safe-rm: refused, {why}", file=sys.stderr)
	return False


def rm(path: "str | os.PathLike[str]") -> bool:
	return _do("", path)


def rm_under(parent: "str | os.PathLike[str]", path: "str | os.PathLike[str]") -> bool:
	top = os.fspath(parent) if parent else ""
	if not top.startswith("/") or len(top) < 2:
		return _refuse(f'no parent: "{top}"')
	return _do(top.rstrip("/"), path)


def _do(parent: str, path: "str | os.PathLike[str]") -> bool:
	p = os.fspath(path) if path else ""
	if not p.startswith("/") or ".." in p or "//" in p or "\n" in p:
		return _refuse(f'bad path: "{p}"')
	p = p.removesuffix("/")
	if parent:
		if not p.startswith(parent + "/") or len(p) == len(parent) + 1:
			return _refuse(f"not under {parent}: {p}")
	elif not (_IN_TMP.fullmatch(p) or (p.startswith(own + "/") and len(p) > len(own) + 1)):
		return _refuse(f"not under a tmp folder: {p}")
	if p in (host, own) or (not parent and host_set and str(Path(p).parent) == host):
		return _refuse(f"a whole project temp folder: {p}")
	if Path(p).is_symlink():
		return _refuse(f"a link: {p}")
	try:
		real = os.path.realpath(p, strict=True)
	except OSError:
		return _refuse(f"not there: {p}")
	if real != p:
		return _refuse(f"goes through a link: {p}")
	try:
		_walk(p, os.lstat(p).st_dev)
	except OSError as e:
		print(f"safe-rm: could not remove {p}: {e}", file=sys.stderr)
		return False
	return True


##	Its own walk, since rmtree crosses into a mount. A link inside is removed, never followed.
def _walk(p: str, dev: int) -> None:
	st = os.lstat(p)
	if not stat.S_ISDIR(st.st_mode):
		Path(p).unlink()
		return
	if st.st_dev != dev:
		raise OSError(errno.EXDEV, "another filesystem, left in place", p)
	## A folder a test made read-only would stop the walk partway.
	if st.st_mode & stat.S_IRWXU != stat.S_IRWXU:
		Path(p).chmod(st.st_mode | stat.S_IRWXU)
	with os.scandir(p) as entries:
		for entry in entries:
			_walk(entry.path, dev)
	Path(p).rmdir()


##	History:
##		- 20261031: Created.
##		- 20261106: A read-only folder inside is opened first, so the tree goes whole.
