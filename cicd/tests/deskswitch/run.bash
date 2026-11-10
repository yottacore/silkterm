#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display under xfwm4 with 2
##		desktops, unfocused behind another window, and lets it give its device
##		back. Then switches to the other desktop and back, which tells the
##		window it lost focus it never had, shows it again, and asks it for a
##		repaint, since X11 drops a window's pixels while it is out of view.
##			- It is still let go after the switch.
##			- It took the device back only to repaint, then let it go again.
##			- It shows the same picture as before the switch, not a black one.
##			- Focusing it brings it back.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  window manager, xdotool, xprop, xlogo, or ImageMagick).
##	- Test ID: df4e3
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
for tool in Xvfb xdotool xprop xlogo xfwm4 import compare; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/deskswitch.XXXXXX")"
appPid=""; logoPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}" "${bin}"; fi
	if [[ -n "${logoPid}" ]]; then kill "${logoPid}" 2>/dev/null || true; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	if [[ "${rc}" == 0 ]]; then fTestDir_Rm "${work}" || true; fi
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

## Only the process launched here, and only while it is still that binary.
fStopOurs(){
	local -r pid="${1}" want="$(realpath -e "${2}" 2>/dev/null || true)"
	local exe; exe="$(realpath -e "/proc/${pid}/exe" 2>/dev/null || true)"
	if [[ -z "${want}" || "${exe}" != "${want}" ]]; then return 0; fi
	kill "${pid}" 2>/dev/null || true
	local _
	for _ in {1..20}; do kill -0 "${pid}" 2>/dev/null || return 0; sleep 0.1; done
	kill -9 "${pid}" 2>/dev/null || true
}

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start --wm >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
fX(){ DISPLAY="${display}" XAUTHORITY="${auth}" "${@}"; }
if ! fX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; then echo "  skip: no window manager on ${display}"; exit 3; fi
fX xdotool set_num_desktops 2 2>/dev/null || true
sleep 0.5

## Flat paper, so the picture before and after can be compared, and a solid
## color that a black window can't pass for.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'colors:' $'\tbackground: "#3050a0"' \
	'scroll:' $'\tminimap:' $'\t\tenabled: false' >"${work}/config.shcl"
DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u TMUX -u TMUX_PANE -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET \
	LIBGL_ALWAYS_SOFTWARE=1 HOME="${work}" XDG_RUNTIME_DIR="${work}" SILK_IDLE_SECS=8 SILK_IDLEDBG=1 \
	"${bin}" --config "${work}/config.shcl" --shell /bin/sh --pixel-width 600 --pixel-height 400 >/dev/null 2>"${work}/said" &
appPid=$!

win=""
for _ in {1..40}; do
	win="$(fX xdotool search --all --pid "${appPid}" --name 'SilkTerm' 2>/dev/null | head -n 1 || true)"
	[[ -n "${win}" ]] && break; sleep 0.25
done
fTitle(){ fX xprop -id "${win}" _NET_WM_NAME 2>/dev/null || true; }
fReleased(){ [[ "$(fTitle)" == *"(resource conservation mode)"* ]]; }
fWaitReleased(){ local _; for _ in $(seq 1 "${1}"); do fReleased && return 0; sleep 0.5; done; return 1; }
## The whole screen, the window, and what is around it.
fShot(){ fX import -window root "${work}/${1}.png" 2>/dev/null; }

unfocused=0; releasedFirst=0; stillReleased=0; sameShot=0; repainted=0; backOnFocus=0
if [[ -n "${win}" ]]; then
	fX xdotool windowmove "${win}" 40 40 2>/dev/null || true
	## another window, focused, over part of it, and the pointer away from both.
	## xlogo sets no _NET_WM_PID, so it is found by its class.
	DISPLAY="${display}" XAUTHORITY="${auth}" xlogo -geometry 300x200+400+250 >/dev/null 2>&1 &
	logoPid=$!
	fX xdotool mousemove 1000 700 2>/dev/null || true
	## until SilkTerm isn't the active window, since a map that is slow under
	## load left it focused, and it never let go
	logo=""; active=""; unfocused=0
	for _ in {1..20}; do
		sleep 0.5
		[[ -n "${logo}" ]] || logo="$(fX xdotool search --classname "^xlogo$" 2>/dev/null | tail -n 1 || true)"
		[[ -n "${logo}" ]] || continue
		## SilkTerm first, so it hears it lost focus. Under load xlogo could map
		## first and SilkTerm never get focus at all (2026111011350600).
		fX xdotool windowactivate --sync "${win}" 2>/dev/null || true
		fX xdotool windowactivate "${logo}" 2>/dev/null || true
		active="$(fX xdotool getactivewindow 2>/dev/null || true)"
		if [[ -n "${active}" && "${active}" != "${win}" ]]; then unfocused=1; break; fi
	done
	if fWaitReleased 40; then
		releasedFirst=1
		sleep 1; fShot before
		fX xdotool set_desktop 1 2>/dev/null || true; sleep 2
		fX xdotool set_desktop 0 2>/dev/null || true; sleep 3
		fReleased && stillReleased=1
		fShot after
		## a black window would differ over its whole area, 240,000 pixels
		differ="$(compare -metric AE -fuzz 3% "${work}/before.png" "${work}/after.png" null: 2>&1 || true)"
		echo "${differ}" >"${work}/differ"
		[[ "${differ%%.*}" =~ ^[0-9]+$ ]] && ((${differ%%.*} < 2000)) && sameShot=1
		grep -q 'redraw request, for one frame' "${work}/said" && (($(grep -c 'device released' "${work}/said") >= 2)) && repainted=1
		fX xdotool windowactivate "${win}" 2>/dev/null || true
		for _ in {1..10}; do fReleased || { backOnFocus=1; break; }; sleep 0.5; done
	fi
fi
fStopOurs "${appPid}" "${bin}"; appPid=""

fCheck "the window came up" test -n "${win}"
fCheck "another window has focus" test "${unfocused:-0}" = 1
fCheck "an unfocused, partly covered window lets its device go" test "${releasedFirst}" = 1
fCheck "it is still let go after a switch to the other desktop and back" test "${stillReleased}" = 1
fCheck "it took the device back only to repaint, then let it go again" test "${repainted}" = 1
fCheck "it shows the same picture as before the switch ($(cat "${work}/differ" 2>/dev/null || echo '?') pixels differ)" test "${sameShot}" = 1
fCheck "focusing it brings it back" test "${backOnFocus}" = 1

if ((failures)); then
	echo "  SilkTerm ${win:-none}, xlogo ${logo:-none}, active ${active:-none}"
	echo "  said:"; sed 's/^/      /' "${work}/said" 2>/dev/null || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261110 JC: Created.
