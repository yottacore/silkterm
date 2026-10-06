#!/usr/bin/env bash

#  shellcheck disable=1091  ## 'source is valid here, but shellcheck doesn't know the path to it.'
#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes, use double quotes for that.' I know, and I often want an explicit '$'.
#  shellcheck disable=2034  ## 'variable appears unused.' Complains about valid use of variable indirection (e.g. later use of local -n var=$1)
#  shellcheck disable=2046  ## 'Quote to prevent word-splitting.' (OK for integers.)
#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2119  ## 'Use foo "$@" if function's $1 should mean script's $1.' Confusing and inapplicable.
#  shellcheck disable=2120  ## 'Foo references arguments, but none are ever passed.' Valid function argument overloading.
#  shellcheck disable=2128  ## 'Expanding an array without an index only gives the element in the index 0.' False hits on associative arrays.
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.' Cumbersome and unnecessary. For integers it's sometimes required to even come into existence for counters.
#  shellcheck disable=2162  ## 'read without -r will mangle backslashes.'
#  shellcheck disable=2178  ## 'Variable was used as an array but is now assigned a string.' False hits on associative arrays with e.g. 'local -n assocArray=$1'.
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'
#  shellcheck disable=2317  ## 'Can't reach.' (I.e. an 'exit' is used for debugging - and makes an unusable visual mess.)
## shellcheck disable=2002  ## 'Useless use of cat.'
## shellcheck disable=2004  ## '$/${} is unnecessary on arithmetic variables.' Inappropriate complaining?
## shellcheck disable=2053  ## 'Quote the right-hand sid of = in [[ ]] to prevent glob matching.' Disable for Yoda Notation.
## shellcheck disable=2143  ## 'Use grep -q instead of echo | grep'

##	Purpose:
##		Run / screenshot a GUI app on a private Xvfb display, never touching the
##		visible :0 session. No desktop environment is involved, so this sidesteps the
##		"XFCE won't run twice per user" limit - Xvfb is a bare in-memory framebuffer,
##		not a session. Optional standalone xfwm4 (--wm) only if an app needs a WM.
##	Syntax:
##		gui-headless.bash start [--wm]      ## bring up Xvfb (+ optional xfwm4)
##		gui-headless.bash launch <cmd...>   ## run cmd in the background on it
##		gui-headless.bash shot <out.png>    ## capture the whole virtual screen
##		gui-headless.bash status
##		gui-headless.bash stop              ## kill only what we started
##	Notes:
##		Display/size is overridable via CICD_HEADLESS_DISPLAY / CICD_HEADLESS_SIZE
##		(legacy RPD_* names still honored as fallbacks).
##		start --wm waits up to CICD_HEADLESS_WM_WAIT seconds (10) for the WM.
##		A server belongs to the process that ran `start` (the calling script).
##		While that process lives, only it may stop the server, and another run's
##		`start` on the same number is refused rather than shared. Once it has
##		exited, as after a start by hand, anyone may stop the server or take it
##		over. A number some other X server holds is refused outright.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -euo pipefail

display="${CICD_HEADLESS_DISPLAY:-${RPD_HEADLESS_DISPLAY:-:99}}"
size="${CICD_HEADLESS_SIZE:-${RPD_HEADLESS_SIZE:-1920x1080x24}}"
num="${display#:}"
## ${USER} is unset in a cron or ssh context, and `set -u` then reports this
## as "headless display failed to start".
runDir="/tmp/cicd-gui-headless-${USER:-$(id -un)}"
## The name is predictable, so somebody else can get there first. Refuse anything
## we do not own, and anything that is a link to somewhere else - the pid files
## under here are read and acted on, and the auth cookie is a key to the display.
mkdir -p "${runDir}"
if [[ -L "${runDir}" || ! -d "${runDir}" || ! -O "${runDir}" ]]; then
	echo "${runDir} is not ours; refusing to use it" >&2
	exit 1
fi
chmod 700 "${runDir}"

## Run something on our private display. Clearing the Wayland vars matters as much
## as setting DISPLAY: winit and GTK both prefer Wayland when they see it, so on a
## Wayland session (WSLg included) the window opens on the real desktop instead and
## nothing here can find it. SESSION_MANAGER goes too: xfwm4 joined the desktop's
## session through it, and the desktop restarted each one we stopped, onto this
## display, where it took the next run's screen.
## --bg is for a background job: it execs, so $! is the program itself. Without
## it $! was a subshell, and stop killed that and left the program running.
fOnX(){ local how=""; [[ "${1:-}" == "--bg" ]] && { how="exec"; shift; }; ${how} env -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE -u SESSION_MANAGER DISPLAY="${display}" "${@}"; }

xvfbPid="${runDir}/xvfb-${num}.pid"
wmPid="${runDir}/wm-${num}.pid"
appsPids="${runDir}/apps-${num}.pids"
auth="${runDir}/Xauthority-${num}"

ownerFile="${runDir}/owner-${num}"
xLock="/tmp/.X${num}-lock"

## A pid alone is not a process: a run killed before `stop` leaves its pid file,
## and the number can come back as anything. The start time from /proc goes with
## it, so a record names one process and no other.
fStartedAt() { local s; s="$(cat "/proc/${1}/stat" 2>/dev/null)" || return 1; s="${s##*) }"; set -- ${s}; echo "${20}"; }
fRecord() { echo "${1} $(fStartedAt "${1}")"; }
fSame() { local p t; read -r p t <<<"${1}"; [[ -n "${t}" && "$(fStartedAt "${p}" || true)" == "${t}" ]]; }
fAlive() { [[ -f "${1}" ]] && fSame "$(cat "${1}")"; }
fPidOf() { local p _; read -r p _ <"${1}"; echo "${p}"; }
## Kill a recorded process and wait for it to go. A server still shutting down
## holds the lock, and a start right after a stop found the number taken.
fEndIt() {
	local i
	fSame "${1}" || return 0
	kill "${1%% *}" 2>/dev/null || true
	for ((i = 0; i < 50; i++)); do fSame "${1}" || return 0; sleep 0.1; done
	kill -9 "${1%% *}" 2>/dev/null || true
}

## Who is asking: the script that ran this one.
me="$(fRecord "${PPID}")"
fOwnedByOther() { [[ -f "${ownerFile}" ]] && [[ "$(cat "${ownerFile}")" != "${me}" ]] && fSame "$(cat "${ownerFile}")"; }

fStart() {
	local fresh=""
	if fAlive "${xvfbPid}"; then
		if fOwnedByOther; then
			echo "Xvfb on ${display} belongs to another run (pid $(fPidOf "${ownerFile}")); not sharing it" >&2
			exit 1
		fi
		echo "${me}" > "${ownerFile}"
		echo "Xvfb already on ${display} (pid $(fPidOf "${xvfbPid}"))"
	else
		## Another X server on this number would answer xdpyinfo in our place,
		## while our own Xvfb quits at once.
		local held; held="$(tr -dc '0-9' 2>/dev/null <"${xLock}" || true)"
		if [[ -n "${held}" && -d "/proc/${held}" ]]; then
			echo "${display} is taken by another X server (pid ${held})" >&2
			exit 1
		fi
		## The cookie is a key to the display; the ambient umask left it readable.
		(umask 077; : > "${auth}")
		Xvfb "${display}" -screen 0 "${size}" -nolisten tcp -auth "${auth}" \
			>"${runDir}/xvfb-${num}.log" 2>&1 &
		local pid=$!
		fRecord "${pid}" > "${xvfbPid}"
		echo "${me}" > "${ownerFile}"
		## Up means ours is still running, holds the number, and takes connections.
		local ok=""
		for _ in $(seq 1 50); do
			kill -0 "${pid}" 2>/dev/null || break
			if [[ "$(tr -dc '0-9' 2>/dev/null <"${xLock}" || true)" == "${pid}" ]] \
				&& DISPLAY="${display}" xdpyinfo >/dev/null 2>&1; then ok=1; break; fi
			sleep 0.1
		done
		if [[ -z "${ok}" ]]; then
			kill "${pid}" 2>/dev/null || true
			rm -f "${xvfbPid}" "${ownerFile}"
			echo "Xvfb did not come up on ${display}; see ${runDir}/xvfb-${num}.log" >&2
			exit 1
		fi
		fresh=1
		echo "Started Xvfb on ${display} (pid ${pid}, ${size})"
	fi
	if [[ "${1:-}" == "--wm" ]]; then
		if ! fAlive "${wmPid}"; then
			fOnX --bg xfwm4 --compositor=off >"${runDir}/wm-${num}.log" 2>&1 &
			fRecord $! > "${wmPid}"
			echo "Started xfwm4 on ${display} (pid $(fPidOf "${wmPid}"))"
		fi
		## xfwm4 is still setting up when the fork returns, and a caller that
		## asks for the WM right away finds none (2026100512560044).
		fWmWait || {
			local why="did not come up within ${wmWaitSecs}s"
			fAlive "${wmPid}" || why="exited"
			echo "xfwm4 ${why} on ${display}; see ${runDir}/wm-${num}.log" >&2
			[[ -f "${wmPid}" ]] && fEndIt "$(cat "${wmPid}")"
			rm -f "${wmPid}"
			## A display started just now would be left behind by a caller that
			## only stops what it saw start.
			if [[ -n "${fresh}" ]]; then
				fEndIt "$(cat "${xvfbPid}")"
				rm -f "${xvfbPid}" "${ownerFile}"
			fi
			exit 1
		}
	fi
}

## Digits only: the value goes into arithmetic, which would run a $( ) in it.
wmWaitSecs="${CICD_HEADLESS_WM_WAIT:-10}"
[[ "${wmWaitSecs}" =~ ^[0-9]+$ ]] || wmWaitSecs=10
## Up per EWMH: the root names a check window, and that window names itself.
## A WM that died leaves the root property behind, so the first half alone lies.
fWmUp() {
	local id self
	id="$(fOnX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)"
	[[ "${id}" == *"window id # "* ]] || return 1
	id="${id##* }"
	self="$(fOnX xprop -id "${id}" _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)"
	[[ "${self##* }" == "${id}" ]]
}
fWmWait() {
	local i
	for ((i = 0; i < wmWaitSecs * 10; i++)); do
		fAlive "${wmPid}" || return 1
		fWmUp && return 0
		sleep 0.1
	done
	return 1
}

fLaunch() {
	[[ $# -gt 0 ]] || { echo "usage: launch <cmd...>" >&2; exit 2; }
	fAlive "${xvfbPid}" || fStart
	fOnX --bg "${@}" >"${runDir}/app-${num}.log" 2>&1 &
	fRecord $! >> "${appsPids}"
	echo "Launched on ${display} (pid $!); log: ${runDir}/app-${num}.log"
}

fShot() {
	local out="${1:-}"
	[[ -n "${out}" ]] || { echo "usage: shot <out.png>" >&2; exit 2; }
	fAlive "${xvfbPid}" || { echo "no Xvfb on ${display} - run 'start' first" >&2; exit 1; }
	import -display "${display}" -window root "${out}"
	echo "Wrote ${out}"
}

fStop() {
	if fAlive "${xvfbPid}" && fOwnedByOther; then
		echo "Xvfb on ${display} belongs to another run (pid $(fPidOf "${ownerFile}")); leaving it" >&2
		return 1
	fi
	local line
	if [[ -f "${appsPids}" ]]; then
		while read -r line; do fEndIt "${line}"; done < "${appsPids}"
		rm -f "${appsPids}"
	fi
	for f in "${wmPid}" "${xvfbPid}"; do
		[[ -f "${f}" ]] || continue
		fEndIt "$(cat "${f}")"
		rm -f "${f}"
	done
	rm -f "${ownerFile}"
	echo "Stopped headless session on ${display}"
}

case "${1:-}" in
	start)  shift; fStart "${1:-}" ;;
	launch) shift; fLaunch "${@}" ;;
	shot)   shift; fShot "${1:-}" ;;
	status) fAlive "${xvfbPid}" && echo "Xvfb up on ${display} (pid $(fPidOf "${xvfbPid}"))" || echo "no Xvfb on ${display}" ;;
	stop)   fStop ;;
	*) echo "usage: gui-headless.bash {start [--wm]|launch <cmd...>|shot <out.png>|status|stop}" >&2; exit 2 ;;
esac


##	Script history:
##		- 20260701: Created.
##		- 20260917: A pid file carries the start time, so a reused pid is not
##		  ours; start refuses a number another server holds and reports success
##		  only for its own; a server belongs to the run that started it.
##		- 20261005: start --wm waits for the WM to answer, and fails if it
##		  does not. Background pids are the programs, not a subshell, and
##		  stop waits for them to exit.
