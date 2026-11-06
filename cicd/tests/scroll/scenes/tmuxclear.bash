#!/bin/dash

## A clear inside tmux, the way tmux 3.6b draws it on the outer terminal with its
## status line on. tmux never passes the clear on. It keeps the outer terminal on
## the alt screen and scrolls away the pane's rows under the prompt's line with a
## region and CSI S, since the status line keeps it from erasing to the bottom.
## A burst inside the pane scrolls the pane away whole too, then draws it, so the
## bursts first show that output still slides. The clears sit between a region
## scroll of 3 and one of 5, which the trace shows as sh. Each clear goes over a
## screen filled in place, so nothing but the clear can scroll.
## A clear while a big slide is still running slides, by design
## (2026101611321631), and a fill drawn in the same frame as a clear reads as a
## burst. So after each fill, clear and mark the scene waits for the trace to
## show a frame since, at rest. A fixed pause did that only on a quiet box. A
## frame advances the ease by at most 0.1 s, so at a frame a second the bursts
## were still sliding under the clears. A wait that runs out leaves a note, and
## the analyzer calls the run a skip.
## POSIX sh (dash).

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

settle="${SILK_SCENE_SETTLE:-13}"   ## seconds to idle past the GL pipeline warmup
pause="${SILK_SCENE_PAUSE:-0.8}"    ## seconds between steps, past any slide on a quiet box
restWait="${SILK_SCENE_REST_WAIT:-15}"  ## seconds a step waits for a frame at rest
trace="${SILK_SCENE_TRACE:-}"
note="${SILK_SCENE_NOTE:-}"

fFrames(){
	n=$(grep -c 'SCROLLDBG' "${trace}" 2>/dev/null)
	echo "${n:-0}"
}

## $1 frames traced before the step: wait for a later one at rest
fRest(){
	[ -n "${trace}" ] || return 0
	deadline=$(($(date +%s) + restWait))
	until [ "$(fFrames)" -gt "${1}" ] \
		&& grep 'SCROLLDBG' "${trace}" 2>/dev/null | tail -n 1 | grep -qE 'app_off=-?0\.0000 '; do
		if [ "$(date +%s)" -ge "${deadline}" ]; then
			[ -z "${note}" ] || echo "no rest within ${restWait}s" >"${note}"
			return 0
		fi
		sleep 0.05
	done
}

## after a step, until a frame has shown it and the slide is at rest
fPause(){
	shown=$(fFrames)
	sleep "${pause}"
	fRest "${shown}"
}

## the pane drawn in place, row by row
fFill(){
	i=1
	while [ "${i}" -le "${pane}" ]; do
		printf '\033[%d;1H%s line %d\033[K' "${i}" "${1}" "${i}"
		i=$((i + 1))
	done
	printf '\033[%d;1H' "${pane}"
	fPause
}

## a burst as tmux passes it on: the pane scrolled away whole, then drawn
fBurst(){
	printf '\033[1;%dr\033[1;1H\033[%dS' "${pane}" "${pane}"
	i=1
	while [ "${i}" -lt "${pane}" ]; do
		printf '%s burst %d\033[K\r\n' "${1}" "${i}"
		i=$((i + 1))
	done
	printf '%s burst %d\033[K\033[1;%dr\033[%d;1H' "${1}" "${pane}" "${rows}" "${pane}"
	sleep "${pause}"
}

## Enter on the pane's last row: the pane scrolls a line
fEnter(){
	printf '\033[1;%dr\033[%d;1H\n\033[%d;3H%s\r\n\033[K\033[1;%dr\033[%d;1H' \
		"${pane}" "${pane}" "$((pane - 1))" "${1}" "${rows}" "${pane}"
	sleep "${pause}"
}

## tmux's clear: the rows under the first one scrolled away, that one erased
fWipe(){
	printf '\033[1;%dr\033[1;1H\033[2;%dr\033[%dS\033[1;1H\033[K' "${pane}" "${pane}" "$((pane - 1))"
}

## ble.sh's two-row prompt, drawn at the top with insert- and delete-line
fBle(){
	printf '\033[1;1H\033[2M\033[1;1H\033[2L\033[1;1H\033[0;36m12:00:00\033(B\033[m user@host:~/repo [ main ]\r\n-> $ '
}

## $1 lines: a region scroll the trace shows as sh=$1
fMark(){
	fFill mark
	printf '\033[1;%dr\033[%dS\033[1;%dr\033[%d;1H' "${pane}" "${1}" "${rows}" "${pane}"
	fPause
}

sleep "${settle}"
## the window has its final size only once it has settled
sz=$(stty size 2>/dev/null) || sz=""
rows=${sz% *}
case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
[ "${rows}" -ge 10 ] || rows=30
pane=$((rows - 1))

printf '\033[?1049h\033[H\033[2J\033[%d;1H\033[30m\033[42m[0] 0:bash*\033(B\033[m\033[1;%dr\033[1;1H' "${rows}" "${rows}"
for n in 1 2 3 4; do
	fBurst "${n}"
done
fMark 3

fFill one
fEnter clear
fWipe
printf '\033[1;%dr\033[1;1H$ ' "${rows}"
fPause

## Ctrl+L
fFill two
fWipe
printf '$ \033[1;%dr\033[1;3H' "${rows}"
fPause

## under ble.sh, in one write and with a frame between tmux's clear and ble.sh
fFill three
fEnter clear
fWipe
printf '\033[1;%dr\033[1;1H\033[K\033[1;%dr\033[2;1H\033[1L' "${rows}" "${pane}"
fBle
printf '\033[1;%dr\033[2;6H' "${rows}"
fPause
fFill four
fWipe
sleep 0.2
printf '\033[1;%dr' "${pane}"
fBle
printf '\033[1;%dr\033[2;6H' "${rows}"
fPause

## a reset: tmux scrolls the whole pane away and redraws every row
fFill five
fEnter reset
printf '\033[1;%dr\033[%dS\033[1;1H\033[1;%dr\033[1;1H$ ' "${pane}" "${pane}" "${rows}"
i=1
while [ "${i}" -lt "${pane}" ]; do
	printf '\033[K\r\n'
	i=$((i + 1))
done
printf '\033[K\033[1;3H'
fPause

fMark 5
while :; do sleep 1; done

##	History:
##		- 20261016 JC: Created.
##		- 20261106: Each step waits for a frame at rest, not a fixed pause.
