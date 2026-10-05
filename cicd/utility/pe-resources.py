#!/usr/bin/env python3

##	- Purpose:
##		Say what is in a Windows binary's resource directory, so a build that
##		quietly stopped embedding its icon or its version block gets caught. A
##		missing resource is not a link error and shows up nowhere else.
##	- Syntax:
##		pe-resources.py [--require icon,version] <file.exe> ...
##	- Exit: 0 all required resources present, 1 something is missing, 2 bad usage.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

import struct
import sys
from pathlib import Path

TYPES = {1: "cursor", 2: "bitmap", 3: "icon", 4: "menu", 5: "dialog", 6: "string",
         9: "accelerator", 10: "rcdata", 12: "group_cursor", 14: "group_icon",
         16: "version", 24: "manifest"}
WANT = {"icon": 14, "version": 16}


def sections(data: bytes) -> list[tuple[str, int, int, int, int]]:
	pe = struct.unpack_from("<I", data, 0x3c)[0]
	if data[:2] != b"MZ" or data[pe:pe + 4] != b"PE\0\0":
		raise ValueError("not a PE file")
	count, opt_size = struct.unpack_from("<H", data, pe + 6)[0], struct.unpack_from("<H", data, pe + 20)[0]
	first = pe + 24 + opt_size
	out = []
	for i in range(count):
		name, vsize, vaddr, rsize, raddr = struct.unpack_from("<8sIIII", data, first + i * 40)
		out.append((name.rstrip(b"\0").decode("latin-1"), vaddr, vsize, raddr, rsize))
	return out


##	Only the top level of the tree is read. What is being asked is whether a type
##	is there at all, and the leaves say nothing more about that.
def resource_types(path: str) -> set[int]:
	data = Path(path).read_bytes()
	rsrc = next((s for s in sections(data) if s[0] == ".rsrc"), None)
	if rsrc is None:
		return set()
	_, vaddr, _, raddr, _ = rsrc
	named, ided = struct.unpack_from("<HH", data, raddr + 12)
	found: set[int] = set()
	for i in range(named + ided):
		entry = raddr + 16 + i * 8
		ident, _ = struct.unpack_from("<II", data, entry)
		if not ident & 0x8000_0000:
			found.add(ident)
	return found


def main(argv: list[str]) -> int:
	require = ["icon", "version"]
	files = []
	i = 0
	while i < len(argv):
		if argv[i] == "--require":
			i += 1
			require = [w for w in argv[i].split(",") if w]
		else:
			files.append(argv[i])
		i += 1
	if not files or any(w not in WANT for w in require):
		print("usage: pe-resources.py [--require icon,version] <file.exe> ...", file=sys.stderr)
		return 2

	bad = False
	for path in files:
		try:
			found = resource_types(path)
		except (OSError, ValueError, struct.error) as err:
			print(f"{path}: unreadable ({err})")
			bad = True
			continue
		have = sorted(TYPES.get(t, str(t)) for t in found)
		missing = [w for w in require if WANT[w] not in found]
		print(f"{path}: {', '.join(have) or 'no resources'}")
		if missing:
			print(f"{path}: MISSING {', '.join(missing)}")
			bad = True
	return 1 if bad else 0


if __name__ == "__main__":
	sys.exit(main(sys.argv[1:]))

##	History:
##		- 20260910 JC: Created.
