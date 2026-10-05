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
run_dir="/tmp/cicd-gui-headless-${USER:-$(id -un)}"
## The name is predictable, so somebody else can get there first. Refuse anything
## we do not own, and anything that is a link to somewhere else - the pid files
## under here are read and acted on, and the auth cookie is a key to the display.
mkdir -p "$run_dir"
if [[ -L "$run_dir" || ! -d "$run_dir" || ! -O "$run_dir" ]]; then
	echo "${run_dir} is not ours; refusing to use it" >&2
	exit 1
fi
chmod 700 "$run_dir"

## Run something on our private display. Clearing the Wayland vars matters as much
## as setting DISPLAY: winit and GTK both prefer Wayland when they see it, so on a
## Wayland session (WSLg included) the window opens on the real desktop instead and
## nothing here can find it. SESSION_MANAGER goes too: xfwm4 joined the desktop's
## session through it, and the desktop restarted each one we stopped, onto this
## display, where it took the next run's screen.
## --bg is for a background job: it execs, so $! is the program itself. Without
## it $! was a subshell, and stop killed that and left the program running.
onX(){ local how=""; [[ "${1:-}" == "--bg" ]] && { how="exec"; shift; }; ${how} env -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE -u SESSION_MANAGER DISPLAY="$display" "$@"; }

xvfb_pid="$run_dir/xvfb-${num}.pid"
wm_pid="$run_dir/wm-${num}.pid"
apps_pids="$run_dir/apps-${num}.pids"
auth="$run_dir/Xauthority-${num}"

owner_file="$run_dir/owner-${num}"
x_lock="/tmp/.X${num}-lock"

## A pid alone is not a process: a run killed before `stop` leaves its pid file,
## and the number can come back as anything. The start time from /proc goes with
## it, so a record names one process and no other.
started_at() { local s; s="$(cat "/proc/$1/stat" 2>/dev/null)" || return 1; s="${s##*) }"; set -- $s; echo "${20}"; }
record() { echo "$1 $(started_at "$1")"; }
same() { local p t; read -r p t <<<"$1"; [[ -n "$t" && "$(started_at "$p" || true)" == "$t" ]]; }
alive() { [[ -f "$1" ]] && same "$(cat "$1")"; }
pid_of() { local p _; read -r p _ <"$1"; echo "$p"; }
## Kill a recorded process and wait for it to go. A server still shutting down
## holds the lock, and a start right after a stop found the number taken.
end_it() {
	local i
	same "$1" || return 0
	kill "${1%% *}" 2>/dev/null || true
	for ((i = 0; i < 50; i++)); do same "$1" || return 0; sleep 0.1; done
	kill -9 "${1%% *}" 2>/dev/null || true
}

## Who is asking: the script that ran this one.
me="$(record "$PPID")"
owned_by_other() { [[ -f "$owner_file" ]] && [[ "$(cat "$owner_file")" != "$me" ]] && same "$(cat "$owner_file")"; }

start() {
	local fresh=""
	if alive "$xvfb_pid"; then
		if owned_by_other; then
			echo "Xvfb on $display belongs to another run (pid $(pid_of "$owner_file")); not sharing it" >&2
			exit 1
		fi
		echo "$me" > "$owner_file"
		echo "Xvfb already on $display (pid $(pid_of "$xvfb_pid"))"
	else
		## Another X server on this number would answer xdpyinfo in our place,
		## while our own Xvfb quits at once.
		local held; held="$(tr -dc '0-9' 2>/dev/null <"$x_lock" || true)"
		if [[ -n "$held" && -d "/proc/$held" ]]; then
			echo "$display is taken by another X server (pid $held)" >&2
			exit 1
		fi
		## The cookie is a key to the display; the ambient umask left it readable.
		(umask 077; : > "$auth")
		Xvfb "$display" -screen 0 "$size" -nolisten tcp -auth "$auth" \
			>"$run_dir/xvfb-${num}.log" 2>&1 &
		local pid=$!
		record "$pid" > "$xvfb_pid"
		echo "$me" > "$owner_file"
		## Up means ours is still running, holds the number, and takes connections.
		local ok=""
		for _ in $(seq 1 50); do
			kill -0 "$pid" 2>/dev/null || break
			if [[ "$(tr -dc '0-9' 2>/dev/null <"$x_lock" || true)" == "$pid" ]] \
				&& DISPLAY="$display" xdpyinfo >/dev/null 2>&1; then ok=1; break; fi
			sleep 0.1
		done
		if [[ -z "$ok" ]]; then
			kill "$pid" 2>/dev/null || true
			rm -f "$xvfb_pid" "$owner_file"
			echo "Xvfb did not come up on $display; see $run_dir/xvfb-${num}.log" >&2
			exit 1
		fi
		fresh=1
		echo "Started Xvfb on $display (pid $pid, $size)"
	fi
	if [[ "${1:-}" == "--wm" ]]; then
		if ! alive "$wm_pid"; then
			onX --bg xfwm4 --compositor=off >"$run_dir/wm-${num}.log" 2>&1 &
			record $! > "$wm_pid"
			echo "Started xfwm4 on $display (pid $(pid_of "$wm_pid"))"
		fi
		## xfwm4 is still setting up when the fork returns, and a caller that
		## asks for the WM right away finds none (2026100512560044).
		wm_wait || {
			local why="did not come up within ${wm_wait_secs}s"
			alive "$wm_pid" || why="exited"
			echo "xfwm4 ${why} on $display; see $run_dir/wm-${num}.log" >&2
			[[ -f "$wm_pid" ]] && end_it "$(cat "$wm_pid")"
			rm -f "$wm_pid"
			## A display started just now would be left behind by a caller that
			## only stops what it saw start.
			if [[ -n "${fresh}" ]]; then
				end_it "$(cat "$xvfb_pid")"
				rm -f "$xvfb_pid" "$owner_file"
			fi
			exit 1
		}
	fi
}

## Digits only: the value goes into arithmetic, which would run a $( ) in it.
wm_wait_secs="${CICD_HEADLESS_WM_WAIT:-10}"
[[ "$wm_wait_secs" =~ ^[0-9]+$ ]] || wm_wait_secs=10
## Up per EWMH: the root names a check window, and that window names itself.
## A WM that died leaves the root property behind, so the first half alone lies.
wm_up() {
	local id self
	id="$(onX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)"
	[[ "$id" == *"window id # "* ]] || return 1
	id="${id##* }"
	self="$(onX xprop -id "$id" _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)"
	[[ "${self##* }" == "$id" ]]
}
wm_wait() {
	local i
	for ((i = 0; i < wm_wait_secs * 10; i++)); do
		alive "$wm_pid" || return 1
		wm_up && return 0
		sleep 0.1
	done
	return 1
}

launch() {
	[[ $# -gt 0 ]] || { echo "usage: launch <cmd...>" >&2; exit 2; }
	alive "$xvfb_pid" || start
	onX --bg "$@" >"$run_dir/app-${num}.log" 2>&1 &
	record $! >> "$apps_pids"
	echo "Launched on $display (pid $!); log: $run_dir/app-${num}.log"
}

shot() {
	local out="${1:-}"
	[[ -n "$out" ]] || { echo "usage: shot <out.png>" >&2; exit 2; }
	alive "$xvfb_pid" || { echo "no Xvfb on $display - run 'start' first" >&2; exit 1; }
	import -display "$display" -window root "$out"
	echo "Wrote $out"
}

stop() {
	if alive "$xvfb_pid" && owned_by_other; then
		echo "Xvfb on $display belongs to another run (pid $(pid_of "$owner_file")); leaving it" >&2
		return 1
	fi
	local line
	if [[ -f "$apps_pids" ]]; then
		while read -r line; do end_it "$line"; done < "$apps_pids"
		rm -f "$apps_pids"
	fi
	for f in "$wm_pid" "$xvfb_pid"; do
		[[ -f "$f" ]] || continue
		end_it "$(cat "$f")"
		rm -f "$f"
	done
	rm -f "$owner_file"
	echo "Stopped headless session on $display"
}

case "${1:-}" in
	start)  shift; start "${1:-}" ;;
	launch) shift; launch "$@" ;;
	shot)   shift; shot "${1:-}" ;;
	status) alive "$xvfb_pid" && echo "Xvfb up on $display (pid $(pid_of "$xvfb_pid"))" || echo "no Xvfb on $display" ;;
	stop)   stop ;;
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
