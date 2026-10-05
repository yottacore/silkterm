#!/usr/bin/env python3

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

"""Markdown tables the way the project's docs write them.

A leading pipe and no trailing one, cells padded so the columns line up, and every
column's alignment spelled out, so a plain dash row comes back as left. Shared by the
two tools that write the README's showdown table, so they cannot drift apart.
"""

from collections.abc import Iterable, Sequence


def split_row(line: str) -> list[str]:
	"""The cells of one table line, with or without the outer pipes."""
	line = line.strip()
	if line.startswith("|"):
		line = line[1:]
	if line.endswith("|"):
		line = line[:-1]
	return [c.strip() for c in line.split("|")]


def _kind(align: str) -> str:
	"""left, right or center, from one cell of the alignment row."""
	if len(align) > 1 and align.startswith(":") and align.endswith(":"):
		return "center"
	return "right" if align.endswith(":") else "left"


def _rule(kind: str, width: int) -> str:
	if kind == "center":
		return ":" + "-" * (width - 2) + ":"
	if kind == "right":
		return "-" * (width - 1) + ":"
	return ":" + "-" * (width - 1)


def render(head: Sequence[str], align: Sequence[str], data: Iterable[Sequence[str]]) -> list[str]:
	"""Lines of a table from its header, alignment row and data rows."""
	cols = len(head)
	rows = [list(head)] + [list(r) + [""] * (cols - len(r)) for r in data]
	kind = [_kind(align[i] if i < len(align) else "") for i in range(cols)]
	width = [max(3, *(len(r[i]) for r in rows)) for i in range(cols)]
	rule = [_rule(kind[i], width[i]) for i in range(cols)]
	out = []
	for cells in [rows[0], rule] + rows[1:]:
		#	Numbers sit to the right in the source as well as on the page.
		padded = [c.rjust(width[i]) if kind[i] == "right" and cells is not rows[0] else c.ljust(width[i])
		          for i, c in enumerate(cells)]
		out.append(("| " + " | ".join(padded)).rstrip())
	return out

##	History:
##		- 20260924 JC: Created.
