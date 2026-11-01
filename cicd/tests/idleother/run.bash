#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display, on screen and
##		unfocused, with "Minutes otherwise" at its least (1) and "Minutes when
##		hidden" at an hour. Nothing prints and nothing is typed, so the window
##		lets its device go after the one minute, and the title says so.
##			- Not before most of the minute is up.
##			- By a little past it.
##		The wait is the setting's own, read from the file, so no idle override
##		is set: SILK_IDLE_SECS would stand in for both waits and hide a setting
##		that never reaches the release.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  dash, xdotool or xprop).
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
dash="$(command -v dash)"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/idleother.XXXXXX")"
appPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
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

home="${work}/home"
mkdir -p "${home}"
## No blink, wallpaper or minimap, so nothing but the idle release changes the
## window, and no rating run to put the release off.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' \
	'window:' $'\tremember_per_monitor: false' $'\tidle_release: true' $'\tidle_release_hidden_min: 60' $'\tidle_release_min: 1' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' >"${work}/config.shcl"
printf '%s\n' 'while :; do sleep 300; done' >"${work}/scene.sh"

DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u TMUX -u TMUX_PANE -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET -u SILK_IDLE_SECS \
	LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" SILK_IDLEDBG=1 \
	"${bin}" --config "${work}/config.shcl" --pixel-width 700 --pixel-height 400 --shell "${dash} ${work}/scene.sh" >/dev/null 2>"${work}/said.txt" &
appPid=$!
launched=${SECONDS}

## The title once a half second, with the seconds since launch, until the note
## shows or the wait is well past.
win=""
releasedAt=""
giveUp=$((launched + 100))
while ((SECONDS < giveUp)); do
	if [[ -z "${win}" ]]; then
		win="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdotool search --pid "${appPid}" --name 'SilkTerm' 2>/dev/null | head -n 1 || true)"
	fi
	if [[ -n "${win}" ]]; then
		title="$(DISPLAY="${display}" XAUTHORITY="${auth}" xprop -id "${win}" _NET_WM_NAME 2>/dev/null || true)"
		echo "$((SECONDS - launched)) ${title#*= }" >>"${work}/titles.txt"
		if [[ "${title}" == *"(resource conservation mode)"* ]]; then releasedAt=$((SECONDS - launched)); break; fi
	fi
	sleep 0.5
done
fStopOurs "${appPid}"; appPid=""

fCheck "the window came up" test -n "${win}"
fCheck "an unfocused window on screen lets its device go after Minutes otherwise" test -n "${releasedAt}"
fNotEarly(){ [[ -n "${releasedAt}" ]] && ((releasedAt >= 45)); }
fCheck "and not well before the minute (${releasedAt:-never} s)" fNotEarly

if ((failures)); then
	echo "  titles:"; uniq -f 1 "${work}/titles.txt" 2>/dev/null | sed 's/^/      /' || true
	echo "  said:"; sed 's/^/      /' "${work}/said.txt" 2>/dev/null || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261101 JC: Created.
