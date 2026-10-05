#!/usr/bin/env python3

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

"""Write one terminal's size and memory cells into the README shootout table.

The speed columns are owned by utility/termbench.py, which refreshes only its own and
leaves everything else exactly as written. This owns the other two, keyed the same way -
by terminal name - so the two writers never touch the same cell.

Only ever updates a row that already exists. Adding a row is a judgment call about where
it belongs in the ordering, and the speed tool makes that call.
"""

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).absolute().parent))
import mdtable  # noqa: E402

BEGIN, END = "<!-- termbench:begin -->", "<!-- termbench:end -->"


def norm(cell: str) -> str:
	"""Letters and digits only, so 'XFCE4 Terminal' matches 'xfce4-terminal'."""
	cell = re.sub(r"<sup>.*?</sup>", "", cell)
	cell = re.sub(r"\$\\textcolor\{[^}]*\}\{(?:\\textbf\{)?([^}]*)\}+\$", r"\1", cell)
	return re.sub(r"[^a-z0-9]", "", cell.lower())


def update(readme: Path, terminal: str, file_deps: float, mem: float) -> tuple[str | None, str | None]:
	#	Spelled out, because the default is the locale codec: on Windows that is cp1252,
	#	which cannot read the table's own characters and fails the run before it starts.
	text = readme.read_text(encoding="utf-8")
	if BEGIN not in text or END not in text:
		return None, "table markers not found"

	head, rest = text.split(BEGIN, 1)
	table, tail = rest.split(END, 1)
	lines = table.split("\n")

	rows = [i for i, line in enumerate(lines) if line.strip().startswith("|")]
	if len(rows) < 2:
		return None, "no table rows"

	header = mdtable.split_row(lines[rows[0]])
	deps_col: int | None = None
	mem_col: int | None = None
	for i, cell in enumerate(header):
		key = norm(cell)
		# Headers carry footnote markers and a '(MiB)' tail, so match on the stem.
		if key.startswith("filedeps"):
			deps_col = i
		elif key.startswith("mem"):
			mem_col = i
	if deps_col is None or mem_col is None:
		return None, f"could not find both columns in: {header}"

	target = norm(terminal)
	for idx in rows[2:]:
		cells = mdtable.split_row(lines[idx])
		if len(cells) != len(header):
			continue
		# Column 1 is the terminal name; column 0 is the platform.
		if norm(cells[1]) != target:
			continue
		bold = cells[deps_col].startswith("**")
		wrap = (lambda v: f"**{v}**") if bold else (lambda v: v)
		cells[deps_col] = wrap(f"{file_deps:.1f}")
		cells[mem_col] = wrap(f"{mem:.1f}")
		#	The whole table is laid out again, since one wider cell moves every
		#	column after it.
		grid = [mdtable.split_row(lines[i]) for i in rows]
		grid[rows.index(idx)] = cells
		lines[rows[0]:rows[-1] + 1] = mdtable.render(grid[0], grid[1], grid[2:])
		return head + BEGIN + "\n".join(lines) + END + tail, None

	return None, f"no row named '{terminal}' in the table"


def main() -> int:
	ap = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
	ap.add_argument("--readme", default="README.md", type=Path)
	ap.add_argument("--terminal", required=True, help="row name, as it appears in the table")
	ap.add_argument("--file-deps", required=True, type=float)
	ap.add_argument("--mem", required=True, type=float)
	ap.add_argument("--dry-run", action="store_true")
	args = ap.parse_args()

	out, err = update(args.readme, args.terminal, args.file_deps, args.mem)
	if out is None:
		print(f"showdown-readme: {err}", file=sys.stderr)
		return 1
	if args.dry_run:
		print(f"would set {args.terminal}: File+deps {args.file_deps:.1f}, Mem {args.mem:.1f}")
		return 0
	args.readme.write_text(out, encoding="utf-8")
	print(f"README: {args.terminal} -> File+deps {args.file_deps:.1f}, Mem {args.mem:.1f}")
	return 0


if __name__ == "__main__":
	sys.exit(main())

##	History:
##		- 20260730 JC: Created.
