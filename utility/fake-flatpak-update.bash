#!/usr/bin/env bash
# Fake a long `flatpak update` table, to check that a redraw of a table taller than the window doesn't ease.
#
#   utility/fake-flatpak-update.bash [-r ROWS] [-t TICKS] [-d DELAY] [-f FILL]
#
#   -r ROWS   Table rows. Default: window height + 20, so it never fits.
#   -t TICKS  Redraws. Default: ROWS.
#   -d DELAY  Seconds between redraws. Default: 0.2.
#   -f FILL   Lines printed first, so the scrollback is full. Default: 0. Try 5000.
#
# Each tick homes the cursor, clears, and prints the whole table again. The rows
# that don't fit are joined with no line breaks, so they wrap and scroll up into
# history, same as flatpak. Mirrors flatpak_tick in source/src/pane.rs.
#
# Pass: the view stays at the bottom, or moves a row or so, on each redraw.
# Fail: every redraw eases about a screenful.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT
set -euo pipefail

fUsage() { sed -n '2,/^$/{s/^# \{0,1\}//;s/^#$//;p}' "${BASH_SOURCE[0]}" >&2; }

lines="$(tput lines 2>/dev/null || echo 24)"
rows=$((lines + 20))
ticks=""
delay="0.2"
fill=0

while getopts ':r:t:d:f:h' opt; do
	case "${opt}" in
		r) rows="${OPTARG}" ;;
		t) ticks="${OPTARG}" ;;
		d) delay="${OPTARG}" ;;
		f) fill="${OPTARG}" ;;
		h) fUsage; exit 0 ;;
		*) fUsage; exit 1 ;;
	esac
done
ticks="${ticks:-${rows}}"

for n in "${rows}" "${ticks}" "${fill}"; do
	[[ "${n}" =~ ^[0-9]+$ ]] || { echo "error: '${n}' is not a number" >&2; exit 1; }
done

fit=$((lines - 3))
if ((rows <= fit)); then
	echo "error: ${rows} rows fit in ${lines} lines; use more rows or a shorter window" >&2
	exit 1
fi

if ((fill > 0)); then seq 1 "${fill}" | sed 's/^/fill /'; fi

# One row. Marks: done, in progress, waiting.
fRow() {
	local i="${1}" t="${2}" mark=" "
	if ((i < t)); then mark="x"; elif ((i == t)); then mark="|"; fi
	printf '%2d. [%s] org.silk.App%02d  stable  u' $((i + 1)) "${mark}" "${i}"
}

printf '\033[?25l'
trap 'printf "\033[?25h\n"' EXIT

for ((t = 0; t < ticks; t++)); do
	out=$'\033[1;1H\033[0J'
	for ((i = 0; i < rows - fit; i++)); do out+="$(fRow "${i}" "${t}")"; done
	for ((i = rows - fit; i < rows; i++)); do out+=$'\r\n'"$(fRow "${i}" "${t}")"; done
	out+=$'\r\n\r\n'"Updating $((t + 1))/${rows}"
	printf '%s' "${out}"
	sleep "${delay}"
done
