#!/bin/dash

## Deterministic full-redraw scroll scene for the SILK_SCROLLDBG harness. Models the
## paint shape of a real full-screen app so the alt-screen slide can be measured
## without key injection - it self-scrolls on a timer. Shape ($1):
##   less   - content from the top, one static bottom status line (no top band)
##   vim    - content from the top, two static bottom rows (status + command line)
##   nano   - one static title bar on top, two static help rows at the bottom
##   muffer - two static header rows on top, one static footer row
##   tmux   - a scroll region above one status row, scrolled with real linefeeds
##   pill   - a region scrolled back with CSI T, a pill repainted over its last row
##   chrome - a transcript on the normal screen with a live block redrawn under it,
##            one new transcript line per step (muffer's shape)
##   aptbar - apt's progress bar: a region over all but the last row on the normal
##            screen, the bar redrawn on that row, with the scrollback already full
##   paste  - an input box on a half-empty normal screen growing a line a step and
##            then shrinking back, repainted, with a footer under it (muffer's
##            input box taking a paste)
##   pasteil - the same box grown with insert-line and shrunk with delete-line, so
##            the engine records each step as a region scroll
## The repaint shapes use explicit cursor positioning (CUP) and never a newline, so
## nothing scrolls the real grid - only the drawn content shifts, exactly the way
## curses/nano repaint. The tmux shape is the other kind: it sets DECSTBM and lets
## the terminal scroll, the way tmux (and less) drive it.
## POSIX sh (dash): no backticks (dash would run them and leak the temp path).

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

shape="${1:-less}"
settle="${SILK_SCENE_SETTLE:-13}"   ## seconds to idle past the GL pipeline warmup
step="${SILK_SCENE_STEP:-0.15}"     ## seconds between repaints (one line/step)

if [ "${shape}" = chrome ]; then
	i=0
	while [ "${i}" -lt 80 ]; do
		printf '  history %04d\n' "${i}"
		i=$((i + 1))
	done
	printf '+------------------+\n| working 0        |\n+------------------+\n'
	sleep "${settle}"
	n=0
	while :; do
		printf '\033[3A\r\033[J  transcript %06d the quick brown fox\n+------------------+\n| working %-8d |\n+------------------+\n' "${n}" "${n}"
		n=$((n + 1))
		sleep "${step}"
	done
fi

if [ "${shape}" = aptbar ]; then
	## Past the scrollback's depth, so it is full before the loop starts and its
	## depth no longer moves with each line.
	seq -f '  history %06.0f' 1 10200
	## the window has its final size only once it has settled
	sleep "${settle}"
	sz=$(stty size 2>/dev/null) || sz=""
	rows=${sz% *}
	case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
	[ "${rows}" -ge 10 ] || rows=30
	printf '\033[%d;1H\033[7m  progress 0  \033[0m\033[K' "${rows}"
	printf '\033[1;%dr\033[%d;1H' "$((rows - 1))" "$((rows - 1))"
	trap 'printf "\033[r"' EXIT INT TERM
	n=0
	while :; do
		printf '\n  unpacking %06d the quick brown fox\0337\033[%d;1H\033[7m  progress %-6d\033[0m\033[K\0338' "${n}" "${rows}" "${n}"
		n=$((n + 1))
		sleep "${step}"
	done
fi

if [ "${shape}" = paste ] || [ "${shape}" = pasteil ]; then
	printf '\033[2J\033[H  transcript one\n  transcript two\n  transcript three\n'
	sleep "${settle}"
	sz=$(stty size 2>/dev/null) || sz=""
	rows=${sz% *}
	case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
	[ "${rows}" -ge 16 ] || rows=30
	## transcript 1-3, border 4, input 5.., border, two footer rows, and at
	## least two blank rows left under it
	hmax=$((rows - 9))
	h=0
	grow=1
	while :; do
		if [ "${grow}" = 1 ] && [ "${h}" -ge "${hmax}" ]; then grow=0; fi
		if [ "${grow}" = 0 ] && [ "${h}" -le 1 ]; then grow=1; fi
		if [ "${shape}" = pasteil ] && [ "${h}" -gt 0 ]; then
			if [ "${grow}" = 1 ]; then
				## push the border and the footer down a row, then write the new line
				h=$((h + 1))
				printf '\033[%d;1H\033[L| > pasted line %-6d |\033[K' "$((4 + h))" "${h}"
			else
				printf '\033[%d;1H\033[M' "$((4 + h))"
				h=$((h - 1))
			fi
		else
			if [ "${grow}" = 1 ]; then h=$((h + 1)); else h=$((h - 1)); fi
			printf '\033[4;1H+----------------------+\033[K'
			r=1
			while [ "${r}" -le "${h}" ]; do
				printf '\033[%d;1H| > pasted line %-6d |\033[K' "$((4 + r))" "${r}"
				r=$((r + 1))
			done
			printf '\033[%d;1H+----------------------+\033[K' "$((5 + h))"
			printf '\033[%d;1H  ? for shortcuts\033[K' "$((6 + h))"
			printf '\033[%d;1H  footer status line\033[K\033[J' "$((7 + h))"
		fi
		sleep "${step}"
	done
fi

printf '\033[?1049h\033[2J'                       ## enter alt screen, clear
trap 'printf "\033[?1049l"' EXIT INT TERM         ## restore on the way out
sleep "${settle}"

if [ "${shape}" = tmux ]; then
	sz=$(stty size 2>/dev/null) || sz=""
	rows=${sz% *}
	case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
	[ "${rows}" -ge 10 ] || rows=30
	printf '\033[%d;1H\033[7m  status line (static)  \033[0m\033[K' "${rows}"
	printf '\033[1;%dr\033[%d;1H' "$((rows - 1))" "$((rows - 1))"
	n=0
	while :; do
		printf '  line %06d  the quick brown fox jumps\n' "${n}"
		n=$((n + 1))
		sleep "${step}"
	done
fi

if [ "${shape}" = pill ]; then
	sz=$(stty size 2>/dev/null) || sz=""
	rows=${sz% *}
	case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
	[ "${rows}" -ge 10 ] || rows=30
	last=$((rows - 2))
	printf '\033[1;1H\033[7m  header one  \033[0m\033[K\033[2;1H\033[7m  header two  \033[0m\033[K'
	r=3
	while [ "${r}" -le "${last}" ]; do
		printf '\033[%d;1H  line %06d  the quick brown fox jumps\033[K' "${r}" "$((1000 + r))"
		r=$((r + 1))
	done
	printf '\033[%d;1H> input line\033[K' "${rows}"
	## a real screen has settled before the first wheel notch; with nothing to
	## compare the first step against, no edge is held
	sleep 1
	n=1003
	while :; do
		n=$((n - 1))
		## one write per step, so a build cannot fall between the scroll and the pill
		printf '\033[3;%dr\033[3;1H\033[T\033[r\033[3;1H  line %06d  the quick brown fox jumps\033[K\033[%d;1H  line %06d  the quick\033[7m 1 new message \033[0m\033[K\033[%d;3H' \
			"${last}" "${n}" "${last}" "$((n + last - 3))" "${rows}"
		sleep "${step}"
	done
fi

case "${shape}" in
	nano)   top=1; bot=2 ;;
	muffer) top=2; bot=1 ;;
	vim)    top=0; bot=2 ;;
	*)      top=0; bot=1 ;;   ## less
esac

n=0
while :; do
	sz=$(stty size 2>/dev/null) || sz=""
	rows=${sz% *}
	case "${rows}" in ''|*[!0-9]*) rows=30 ;; esac
	[ "${rows}" -ge 10 ] || rows=30

	## static top band (title bar / header): constant across frames
	r=1
	while [ "${r}" -le "${top}" ]; do
		printf '\033[%d;1H\033[7m  header line %d (static)  \033[0m\033[K' "${r}" "${r}"
		r=$((r + 1))
	done

	## scrolling middle region: the value at a fixed row grows by 1 each frame, so
	## the content moves up one line per step (a clean forward translate).
	r=$((top + 1))
	midbot=$((rows - bot))
	while [ "${r}" -le "${midbot}" ]; do
		printf '\033[%d;1H  line %06d  the quick brown fox jumps\033[K' "${r}" "$((n + r))"
		r=$((r + 1))
	done

	## static bottom band (status / help): constant across frames
	r=$((rows - bot + 1))
	while [ "${r}" -le "${rows}" ]; do
		printf '\033[%d;1H\033[7m  status/help line (static)  \033[0m\033[K' "${r}"
		r=$((r + 1))
	done

	printf '\033[%d;1H' "${rows}"   ## park the cursor (harmless)
	n=$((n + 1))
	sleep "${step}"
done

##	History:
##		- 20260706 JC: Created.
