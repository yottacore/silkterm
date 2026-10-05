#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display with the built-in
##		wallpaper, and reads what it holds through SILK_MEMDBG. The picture is
##		held at the size the window draws it at, not the image's own size, and
##		a resize prepares it again once the resizing stops.
##			- At launch, at the window's size.
##			- After a resize, at the new size.
##			- Grown past the image, at the image's own size.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display or
##	  xdotool).
##	- Test ID: Err031q
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
headless="${root}/cicd/utility/gui-headless.bash"

## The built-in picture's size (source/assets/default-background.jpg).
readonly imageW=1920 imageH=993

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
for tool in Xvfb xdotool; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/wpresize.XXXXXX")"
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

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
fX(){ DISPLAY="${display}" XAUTHORITY="${auth}" "${@}"; }

said="${work}/said.txt"
mkdir -p "${work}/home"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' $'\tidle_release: false' \
	'wallpaper:' $'\tenabled: true' $'\tfallback_builtin: true' $'\trotate:' $'\t\tenabled: false' >"${work}/config.shcl"
DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET LIBGL_ALWAYS_SOFTWARE=1 SILK_MEMDBG=1 \
	HOME="${work}/home" XDG_CONFIG_HOME="${work}/home/.config" XDG_DATA_HOME="${work}/home/.local/share" XDG_RUNTIME_DIR="${work}/home" \
	"${bin}" --config "${work}/config.shcl" --pixel-width 800 --pixel-height 500 --shell "/bin/dash -c 'sleep 120'" >/dev/null 2>"${said}" &
appPid=$!
window="$(fX timeout 30 xdotool search --sync --onlyvisible --pid "${appPid}" 2>/dev/null | head -n 1 || true)"

## What the window should hold at its newest reported size, by the same rule
## as Sizing::held: the larger of the two scales, never past the image.
fWant(){
	local size
	size="$(grep '^memdbg window: ' "${said}" | tail -n 1 | awk '{print $3}')"
	[[ -n "${size}" ]] || return 1
	awk -v size="${size}" -v iw="${imageW}" -v ih="${imageH}" 'BEGIN {
		split(size, d, "x"); s = d[1] / iw; if (d[2] / ih > s) s = d[2] / ih
		if (s >= 1) { printf "%dx%d\n", iw, ih; exit }
		printf "%dx%d\n", int(iw * s + 0.5), int(ih * s + 0.5)
	}'
}
fHeld(){ grep '^memdbg wallpaper: ' "${said}" | tail -n 1 | awk '{print $3}'; }

## SILK_MEMDBG looks every 2 s, and the resize wait is half a second.
fHolds(){  ## fHolds <expected WxH> - waits for the newest line to say so
	local _ want
	for _ in {1..40}; do
		want="$(fWant || true)"
		if [[ -n "${want}" && "$(fHeld)" == "${want}" && "${want}" == "${1}" ]]; then return 0; fi
		sleep 0.25
	done
	echo "    window $(grep '^memdbg window: ' "${said}" | tail -n 1 | awk '{print $3}'), held $(fHeld), wanted ${1}"
	return 1
}

fCheck "a window came up" test -n "${window}"
fCheck "at launch: held at the window's size" fHolds 967x500
fX xdotool windowsize "${window}" 1200 700 || true
fCheck "after a resize: held at the new size" fHolds 1353x700
## a drag: several sizes in quick succession, then still
for size in "1300 800" "1500 900" "1700 1000"; do
	# shellcheck disable=SC2086  ## two words on purpose
	fX xdotool windowsize "${window}" ${size} || true
	sleep 0.1
done
fCheck "grown past the image: held whole" fHolds 1920x993

if ((failures)); then
	sed 's/^/    /' "${said}" | grep -v '^    memdbg pane' | tail -n 30
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261005 JC: Created.
