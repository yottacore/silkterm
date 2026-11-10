#!/bin/sh
# The soak test's shell: no rc files, no user config. Each line typed is noted
# in SOAK_TYPED with this shell's pid, so the test knows which pane it got to,
# and answered with a screenful, so the scrollback fills and moves.
#
# Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
# SPDX-License-Identifier: GPL-2.0-or-later
printf 'soak shell ready\n'
while IFS= read -r line; do
	[ -n "${SOAK_TYPED:-}" ] && printf '%s %s\n' "$$" "${line}" >>"${SOAK_TYPED}"
	i=0
	while [ "${i}" -lt 60 ]; do
		printf '\033[3%dm%s\033[0m line %d of the answer\n' $((i % 8)) "${line}" "${i}"
		i=$((i + 1))
	done
done

# History:
#	- 20261101: Created.
