#!/bin/dash

## Every way a shell clears the screen, each over a full screen of text, first with
## the scrollback full and then nearly empty. A clear shows at once: nothing slides
## up. Bash's Ctrl+L sends home and erase-display with no scrollback clear, so the
## old screen goes up into history whole, and that is the form that used to ease.
## A burst first shows that output still eases. The trace prints nothing for a
## frame at rest, so the part under test sits between two trips to the alt screen.
## Both scroll down, the first a line at a time and the second 9 lines at once,
## which tells the start from the end.
## `clear` under ble.sh with a two-row prompt is in the run too.
## POSIX sh (dash).

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

settle="${SILK_SCENE_SETTLE:-13}"   ## seconds to idle past the GL pipeline warmup
pause="${SILK_SCENE_PAUSE:-0.8}"    ## seconds between clears, past any ease

## a screenful that fits under a cleared screen without scrolling
fill(){
	i=1
	while [ "${i}" -lt "${rows}" ]; do
		printf '%s line %d\n' "${1}" "${i}"
		i=$((i + 1))
	done
	sleep "${pause}"
}

## `clear` under ble.sh with a prompt too long for one row. ble.sh draws it at
## the top with insert- and delete-line, and the delete at the top row sends 2
## blank rows up into history.
ble_clear(){
	printf '\033[H\033[2J\033[3J\033(B\033[m\0337\033[0;94m[ble: EOF]\033(B\033[m\0338\033[%dC  \r\033[K' "$((cols - 2))"
	printf '\r\n\033[1L\033[2K\033[1A\033[2K\033[2M\033[2L\033[2K%s/repo$ ' "${long_prompt}"
	sleep "${pause}"
}

## $1 start or end: how far the alt screen scrolls, which the trace shows as sh
mark(){
	printf '\033[?1049h\033[2J\033[1;1H'
	i=1
	while [ "${i}" -lt "${rows}" ]; do
		printf 'mark %s %d\r\n' "${1}" "${i}"
		i=$((i + 1))
	done
	for _ in 1 2 3; do
		if [ "${1}" = start ]; then printf '\033[1;1H\033M'; else printf '\033[9T'; fi
		sleep 0.2
	done
	printf '\033[?1049l'
	sleep 0.5
}

sleep "${settle}"
## the window has its final size only once it has settled
sz=$(stty size 2>/dev/null) || sz=""
rows=${sz% *}
case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
[ "${rows}" -ge 10 ] || rows=30
cols=${sz#* }
case "${cols}" in ''|*[!0-9]*) cols=100 ;; esac
long_prompt=$(printf "%${cols}s" "" | tr ' ' p)

seq 1 10200 | sed 's/^/deep line /'
fill before
sleep 3
mark start

for n in 1 2; do
	## run 1 starts with the scrollback full; Ctrl+L leaves it that way
	printf '\033[H\033[2J'
	fill "after ctrl-l ${n}"
	clear
	fill "after clear ${n}"
	ble_clear
	tput clear
	fill "after tput ${n}"
	## the scrollback now holds one screen
	printf '\033[H\033[2J'
	fill "after ctrl-l again ${n}"
	printf '\033c'
	fill "after ris ${n}"
done

mark end
while :; do sleep 1; done

##	History:
##		- 20261013 JC: Created.
##		- 20261013 JC: ble.sh's redraw of a long prompt after a clear.
