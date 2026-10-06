#!/bin/dash

## A burst of plain output that is still easing when a full-screen app takes over.
## Models `git commit` opening nano right after a long push: the alt screen has no
## scrollback, so the view must be at rest the moment it swaps in. The gap is
## what puts the ease mid-flight - without it the burst and the swap arrive in one
## read cycle and nothing eases at all. The nano-shaped paint that follows is held
## still, so every alt-screen frame should carry a zero fraction.
## POSIX sh (dash).

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

settle="${SILK_SCENE_SETTLE:-13}"   ## seconds to idle past the GL pipeline warmup
gap="${SILK_SCENE_GAP:-0.25}"       ## seconds between the burst and the alt screen

sleep "${settle}"
seq 1 400 | sed 's/^/output line /'
sleep "${gap}"

printf '\033[?1049h\033[2J'                       ## enter alt screen, clear
trap 'printf "\033[?1049l"' EXIT INT TERM         ## restore on the way out

sz=$(stty size 2>/dev/null) || sz=""
rows=${sz% *}
case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
[ "${rows}" -ge 10 ] || rows=30

printf '\033[1;1H\033[7m  GNU nano                       New Buffer                   \033[0m\033[K'
r=2
while [ "${r}" -le $((rows - 3)) ]; do
	printf '\033[%d;1H  text line %d\033[K' "${r}" "${r}"
	r=$((r + 1))
done
printf '\033[%d;1H\033[7m^G\033[0m Help    \033[7m^O\033[0m Write Out\033[K' $((rows - 1))
printf '\033[%d;1H\033[7m^X\033[0m Exit    \033[7m^R\033[0m Read File\033[K' "${rows}"
printf '\033[2;1H'

while :; do sleep 1; done

##	History:
##		- 20260827 JC: Created.
