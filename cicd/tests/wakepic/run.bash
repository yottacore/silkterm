#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display with a wallpaper and
##		the idle release down to seconds, minimizes it until it lets its device
##		go, then brings it back with the picture's file swapped for a pipe
##		nobody writes. The picture is then never prepared again, so what the
##		window shows is what it shows while a slow one is on its way.
##			- The picture before the release.
##			- After the wake, a picture close to it, not the bare background.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  window manager, xdotool, xwd or ImageMagick).
##	- Test ID: Ersij5t
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
for tool in Xvfb xdotool xprop xwd convert compare mkfifo; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/wakepic.XXXXXX")"
appPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	if [[ "${rc}" == 0 ]]; then rm -rf "${work}"; fi
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

## Minimize needs a window manager, so a display already up without one is
## no use here.
export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start --wm >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
fX(){ DISPLAY="${display}" XAUTHORITY="${auth}" "${@}"; }
if ! fX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -qF 'window id'; then
	echo "  skip: no window manager on ${display}"; exit 3
fi

## A picture bright enough, and shown strongly enough, that the bare
## background is far from it.
picture="${work}/picture.png"
convert -size 800x500 gradient:'#f0a040-#3070e0' -swirl 120 "${picture}"
said="${work}/said.txt"
mkdir -p "${work}/home"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' $'\tidle_release: true' \
	'wallpaper:' $'\tenabled: true' "	image: \"${picture}\"" $'\topacity: 0.8' $'\tblur: 4' $'\thonor_xmp_look: false' $'\trotate:' $'\t\tenabled: false' \
	'cursor:' $'\tanimation: "none"' >"${work}/config.shcl"
DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SESSION_MANAGER LIBGL_ALWAYS_SOFTWARE=1 \
	SILK_IDLE_SECS=3 SILK_IDLEDBG=1 \
	HOME="${work}/home" XDG_CONFIG_HOME="${work}/home/.config" XDG_DATA_HOME="${work}/home/.local/share" XDG_RUNTIME_DIR="${work}/home" \
	"${bin}" --config "${work}/config.shcl" --pixel-width 800 --pixel-height 500 --shell "/bin/dash -c 'sleep 300'" >/dev/null 2>"${said}" &
appPid=$!
window="$(fX timeout 30 xdotool search --sync --onlyvisible --pid "${appPid}" 2>/dev/null | head -n 1 || true)"

fSaid(){  ## fSaid <text> <seconds> - waits for a line holding it
	local -ri giveUp=$((SECONDS + ${2}))
	while ((SECONDS < giveUp)); do
		if grep -qF "${1}" "${said}"; then return 0; fi
		sleep 0.25
	done
	return 1
}
fShot(){ fX xwd -id "${window}" -silent | convert xwd:- "${1}"; }
## Mean difference over the window, 0 to 1. The stand-in is softer than the
## picture, and the bare background is far from both.
fNear(){
	local err
	err="$(compare -metric MAE "${1}" "${2}" null: 2>&1 | sed -n 's/.*(\([0-9.e+-]*\)).*/\1/p' || true)"
	echo "    off by ${err:-?} of full scale"
	[[ -n "${err}" ]] && awk -v e="${err}" 'BEGIN { exit !(e < 0.02) }'
}

fCheck "a window came up" test -n "${window}"
## The idle wait is reset by output, focus and the pointer, so the window is
## left alone with focus until the picture is up, then minimized.
for _ in {1..120}; do
	fShot "${work}/before.png" || true
	## the first frames are the bare background; the picture lifts the mean
	if [[ -s "${work}/before.png" ]] && (( $(convert "${work}/before.png" -format '%[fx:int(mean*255)]' info:) > 60 )); then break; fi
	sleep 0.5
done
fShot "${work}/before.png"
fX xdotool windowminimize "${window}" || true
fCheck "let go once minimized" fSaid "device released" 30
## the file now answers nothing, so the wake's own prepare never finishes
mv "${picture}" "${work}/picture.kept.png"
mkfifo "${picture}"
fX xdotool windowactivate "${window}" 2>/dev/null || fX xdotool windowmap "${window}" || true
fCheck "taken back on the restore" fSaid "device rebuilt" 30
sleep 3
fShot "${work}/after.png"
fCheck "the picture never came" bash -c "! grep -qF 'wallpaper prepared again' '${said}'"
fCheck "after the wake: close to the picture, not the bare background" fNear "${work}/before.png" "${work}/after.png"

if ((failures)); then
	sed 's/^/    /' "${said}" | tail -n 20
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261005 JC: Created.
