#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display, on screen and
##		unfocused, with "Minutes otherwise" at its least (1) and "Minutes when
##		hidden" at an hour. Nothing prints and nothing is typed, so the window
##		lets its device go after the one minute, and the title says so.
##			- Not before most of the minute is up.
##			- By a little past it.
##		A second window beside it runs a program that prints every 10 s in a
##		tab in the back, with a quiet tab in front, and has to let go the same.
##		The pointer rests over the quiet window, and another program grabs it
##		and lets it go every few seconds, as a menu or a window drag does. The
##		window is told the pointer entered each time, which is nobody there.
##			- So it still lets go after the minute.
##			- A grab after that leaves it let go.
##			- Moving the pointer over it brings it back.
##		The wait is the setting's own, read from the file, so no idle override
##		is set: SILK_IDLE_SECS would stand in for both waits and hide a setting
##		that never reaches the release.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  dash, xdotool, xprop or python-xlib).
##	- Test ID: deqy2
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
headless="${root}/cicd/utility/gui-headless.bash"

bin=""
while (($#)); do case "${1}" in
	--bin) bin="${2-}"; shift 2 ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	- History:/p' "${BASH_SOURCE[0]}" | sed 's/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
if [[ -z "${bin}" ]]; then
	targetDir="${CARGO_TARGET_DIR:-target}"
	[[ "${targetDir}" == /* ]] || targetDir="${root}/${targetDir}"
	for candidate in "${targetDir}/debug/silkterm" "${targetDir}/release/silkterm"; do
		if [[ -x "${candidate}" ]]; then bin="${candidate}"; break; fi
	done
fi
if [[ -z "${bin}" || ! -x "${bin}" ]]; then echo "  skip: no SilkTerm binary (build it first, or pass --bin)"; exit 3; fi
echo "  binary: ${bin}"
for tool in Xvfb xdotool xprop dash; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi
if ! python3 -I -c 'import Xlib' 2>/dev/null; then echo "  skip: no python-xlib"; exit 3; fi
dash="$(command -v dash)"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/idleother.XXXXXX")"
appPid=""; backPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${backPid}" ]]; then fStopOurs "${backPid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	local pid; for pid in $(fLeftovers); do kill "${pid}" 2>/dev/null || true; done
	if [[ "${rc}" == 0 ]]; then fTestDir_Rm "${work}" || true; fi
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

## Only the process launched here, and only while it is still this binary.
fStopOurs(){
	local -r pid="${1}"
	local want exe
	want="$(realpath -e "${bin}" 2>/dev/null || true)"
	exe="$(realpath -e "/proc/${pid}/exe" 2>/dev/null || true)"
	if [[ -z "${want}" || "${exe}" != "${want}" ]]; then return 0; fi
	kill "${pid}" 2>/dev/null || true
	local _
	for _ in {1..20}; do kill -0 "${pid}" 2>/dev/null || return 0; sleep 0.1; done
	kill -9 "${pid}" 2>/dev/null || true
}

## Anything still running from this run's folder, by the folder in its command
## line. Builtins only, so no helper of ours turns up in the list.
fLeftovers(){
	local dir pid
	local -a args
	for dir in /proc/[0-9]*; do
		pid="${dir#/proc/}"
		[[ "${pid}" == "$$" || "${pid}" == "${BASHPID}" ]] && continue
		mapfile -d '' args 2>/dev/null <"${dir}/cmdline" || continue
		[[ "${args[*]}" == *"${work}/"* ]] && echo "${pid}"
	done
	return 0
}

## No window manager on purpose: winit reports the window unfocused at once,
## and nothing ever focuses it, minimizes it or covers it.
export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi

## No blink, wallpaper or minimap, so nothing but the idle release changes the
## window, and no rating run to put the release off.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' \
	'window:' $'\tremember_per_monitor: false' $'\tidle_release: true' $'\tidle_release_hidden_min: 60' $'\tidle_release_min: 1' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' >"${work}/config.shcl"
printf '%s\n' 'while :; do sleep 300; done' >"${work}/scene.sh"
printf '%s\n' 'while :; do sleep 10; echo tick; done' >"${work}/ticks.sh"

## One window, with a home and a config of its own, so two never race to
## convert or seed the same file. Its title lines go to `<name>.titles`, its
## stderr to `<name>.said`. The arguments after the name are its layout.
fLaunch(){
	local -r name="${1}"; shift
	local -r home="${work}/${name}"
	mkdir -p "${home}"
	cp "${work}/config.shcl" "${home}/config.shcl"
	DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u TMUX -u TMUX_PANE -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET -u SILK_IDLE_SECS \
		LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" SILK_IDLEDBG=1 \
		"${bin}" --config "${home}/config.shcl" --pixel-width 700 --pixel-height 400 "${@}" >/dev/null 2>"${work}/${name}.said" &
}
fLaunch quiet --shell "${dash} ${work}/scene.sh"
appPid=$!
## The window opens on its first tab, so that one is quiet and the tab made
## after it prints. A --shell before --new-tab is the window's, for every tab.
fLaunch back --shell "${dash} ${work}/scene.sh" --new-tab --shell "${dash} ${work}/ticks.sh"
backPid=$!
launched=${SECONDS}

## Each title once a half second, with the seconds since launch, until both
## notes show or the wait is well past.
declare -A win=([quiet]="" [back]="") releasedAt=([quiet]="" [back]="")
declare -A pidOf=([quiet]="${appPid}" [back]="${backPid}")
## another program's pointer grab, held a moment and let go (_grab.py)
fGrab(){ DISPLAY="${display}" XAUTHORITY="${auth}" python3 -I "${meDir}/_grab.py" 0.2 >/dev/null 2>&1; }
grabs=0; nextGrab=$((launched + 5))
giveUp=$((launched + 100))
while ((SECONDS < giveUp)); do
	for name in quiet back; do
		[[ -n "${releasedAt[${name}]}" ]] && continue
		if [[ -z "${win[${name}]}" ]]; then
			win[${name}]="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdotool search --all --pid "${pidOf[${name}]}" --name 'SilkTerm' 2>/dev/null | head -n 1 || true)"
			## side by side, since with no window manager both open at the
			## same corner and one would cover the other
			if [[ "${name}" == back && -n "${win[${name}]}" ]]; then DISPLAY="${display}" XAUTHORITY="${auth}" xdotool windowmove "${win[${name}]}" 0 500 2>/dev/null || true; fi
			if [[ "${name}" == quiet && -n "${win[${name}]}" ]]; then DISPLAY="${display}" XAUTHORITY="${auth}" xdotool mousemove --window "${win[${name}]}" 100 100 2>/dev/null || true; fi
		fi
		[[ -n "${win[${name}]}" ]] || continue
		title="$(DISPLAY="${display}" XAUTHORITY="${auth}" xprop -id "${win[${name}]}" _NET_WM_NAME 2>/dev/null || true)"
		echo "$((SECONDS - launched)) ${title#*= }" >>"${work}/${name}.titles"
		if [[ "${title}" == *"(resource conservation mode)"* ]]; then releasedAt[${name}]=$((SECONDS - launched)); fi
	done
	[[ -n "${releasedAt[quiet]}" && -n "${releasedAt[back]}" ]] && break
	if [[ -n "${win[quiet]}" && -z "${releasedAt[quiet]}" ]] && ((SECONDS >= nextGrab)); then
		fGrab && grabs=$((grabs + 1))
		nextGrab=$((SECONDS + 8))
	fi
	sleep 0.5
done

## The quiet window's title, until it does or doesn't say it is let go, or 5 s.
fQuietTitle(){ DISPLAY="${display}" XAUTHORITY="${auth}" xprop -id "${win[quiet]}" _NET_WM_NAME 2>/dev/null || true; }
fStaysReleased(){ local _; for _ in {1..6}; do [[ "$(fQuietTitle)" == *"(resource conservation mode)"* ]] || return 1; sleep 0.5; done; }
fComesBack(){ local _; for _ in {1..10}; do [[ "$(fQuietTitle)" == *"(resource conservation mode)"* ]] || return 0; sleep 0.5; done; return 1; }
grabbedAfter=0; staysAfterGrab=1; backOnMove=0
if [[ -n "${releasedAt[quiet]}" ]]; then
	fGrab && grabbedAfter=1
	fStaysReleased || staysAfterGrab=0
	DISPLAY="${display}" XAUTHORITY="${auth}" xdotool mousemove --window "${win[quiet]}" 150 150 2>/dev/null || true
	fComesBack && backOnMove=1
fi
fStopOurs "${appPid}"; appPid=""
fStopOurs "${backPid}"; backPid=""

fNotEarly(){ [[ -n "${releasedAt[${1}]}" ]] && ((releasedAt[${1}] >= 45)); }
fCheck "the window came up" test -n "${win[quiet]}"
fCheck "an unfocused window on screen lets its device go after Minutes otherwise" test -n "${releasedAt[quiet]}"
fCheck "and not well before the minute (${releasedAt[quiet]:-never} s)" fNotEarly quiet
fCheck "the pointer over it was grabbed and let go during the wait (${grabs} times)" test "${grabs}" -ge 4
fCheck "a pointer grab after the release leaves it let go" test "${grabbedAfter}" = 1 -a "${staysAfterGrab}" = 1
fCheck "moving the pointer over it brings it back" test "${backOnMove}" = 1
fCheck "the window with a printing tab in the back came up" test -n "${win[back]}"
## xdotool matches any one condition unless told --all, and two windows read
## as one then
fCheck "each window is its own" test "${win[quiet]}" != "${win[back]}"
fCheck "output from a tab in the back does not keep the device" test -n "${releasedAt[back]}"
fCheck "and not well before the minute (${releasedAt[back]:-never} s)" fNotEarly back

if ((failures)); then
	for name in quiet back; do
		echo "  ${name} titles:"; uniq -f 1 "${work}/${name}.titles" 2>/dev/null | sed 's/^/      /' || true
		echo "  ${name} said:"; sed 's/^/      /' "${work}/${name}.said" 2>/dev/null || true
	done
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261101 JC: Created.
##		- 20261102 JC: A second window, printing in a tab in the back.
##		- 20261102 JC: Each title read from its own window, and the printing
##		  tab really in the back.
##		- 20261106 JC: The pointer over the quiet window, grabbed and let go by
##		  another program during the wait and after the release.
