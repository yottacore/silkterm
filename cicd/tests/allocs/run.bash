#!/usr/bin/env bash

##	- Purpose:
##		Counts the heap allocations the window thread makes per frame, with
##		the program at rest and during a drag select, and fails past a limit.
##		The window has two tabs, a split pane and the halo on, so the tab
##		strip, both panes and the scrim are all in every frame.
##			- At rest: the cursor pulse redraws and nothing else changes.
##			- Drag: the pointer held down and moved across a pane's text.
##	- Syntax: run.bash [--bin PATH] [--report]   (default: the debug build)
##		--report prints the figures and checks no limit.
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no debug binary,
##	  display, window manager or xdotool).
##	- Test ID: ErzCQC2
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
headless="${root}/cicd/utility/gui-headless.bash"

## Mean allocations per frame. At rest that is the frame itself plus the
## events and loop passes before it, a timer wake or two. In the drag the
## frame is held to its limit on its own, and what came before it is held per
## pointer move instead: a slow box folds more moves into each frame, so a
## per-frame count there rises with the load, not the code. The count is the
## whole window thread's, so the graphics driver's share is in it: about 130
## of them on llvmpipe, the software GL. Measured 20261006: at rest 189 before
## the frame lists were kept, 167 after; drag 382 and 340. Between frames in
## a drag, about 2.5 per move (20261010). A driver update can move the floor,
## so read the figures with --report before moving a limit.
idleLimit="${ALLOCS_IDLE_LIMIT:-178}"
dragLimit="${ALLOCS_DRAG_LIMIT:-360}"
moveLimit="${ALLOCS_MOVE_LIMIT:-6}"

bin=""; report=0
while (($#)); do case "${1}" in
	--bin) bin="${2-}"; shift 2 ;;
	--report) report=1; shift ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	- History:/p' "${BASH_SOURCE[0]}" | sed 's/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
## Only a debug build counts; a release build reads 0 everywhere.
if [[ -z "${bin}" ]]; then
	targetDir="${CARGO_TARGET_DIR:-target}"
	[[ "${targetDir}" == /* ]] || targetDir="${root}/${targetDir}"
	bin="${targetDir}/debug/silkterm"
fi
if [[ ! -x "${bin}" ]]; then echo "  skip: no debug SilkTerm binary (build it first, or pass --bin)"; exit 3; fi
echo "  binary: ${bin}"
for tool in Xvfb xdotool xprop; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/allocs.XXXXXX")"
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

## Focus drives the cursor pulse, and xfwm4 gives it.
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

said="${work}/said.txt"
mkdir -p "${work}/home"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' $'\tidle_release: false' \
	'wallpaper:' $'\tenabled: false' $'\tfallback_builtin: false' 'text:' $'\tscrim:' $'\t\tenabled: true' >"${work}/config.shcl"
## Rows with colored backgrounds, so every frame has cell quads to copy.
cat >"${work}/fill.sh" <<'FILL'
i=0
while [ "${i}" -lt 400 ]; do
	printf '\033[4%dm row %d \033[0m plain text after it\n' $((i % 8)) "${i}"
	i=$((i + 1))
done
exec sleep 600
FILL
shellCmd="/bin/sh ${work}/fill.sh"
DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SESSION_MANAGER LIBGL_ALWAYS_SOFTWARE=1 \
	SILK_ALLOCS=1 \
	HOME="${work}/home" XDG_CONFIG_HOME="${work}/home/.config" XDG_DATA_HOME="${work}/home/.local/share" \
	XDG_CACHE_HOME="${work}/home/.cache" XDG_RUNTIME_DIR="${work}/home" \
	"${bin}" --config "${work}/config.shcl" --pixel-width 900 --pixel-height 600 \
	--shell "${shellCmd}" --new-pane --right --shell "${shellCmd}" --new-tab --shell "${shellCmd}" \
	>/dev/null 2>"${said}" &
appPid=$!
window="$(fX timeout 30 xdotool search --sync --onlyvisible --pid "${appPid}" 2>/dev/null | head -n 1 || true)"
fCheck "a window came up" test -n "${window}"
if [[ -z "${window}" ]]; then echo "${failures} failed"; exit 1; fi

## Over the lines after line $1 up to line $2: frames, the mean frame, the
## mean of frame plus between, and the between total.
fMean(){
	sed -n "$((${1} + 1)),${2}p" "${said}" | awk '
		/^\[allocs\] frame [0-9]+ between [0-9]+$/ { n++; f += $3; b += $5; t += $3 + $5 }
		END { if (n) printf "%d %.0f %.0f %d\n", n, f / n, t / n, b; else print "0 0 0 0" }'
}
fLines(){ wc -l <"${said}"; }

## The pointer rests over the left pane's text for the whole run, so hover
## settles before anything is counted.
fX xdotool windowactivate --sync "${window}" 2>/dev/null || true
fX xdotool mousemove --window "${window}" 120 200
sleep 6
from="$(fLines)"; sleep 4; to="$(fLines)"
read -r idleFrames idleFrame idleTotal _ < <(fMean "${from}" "${to}")
echo "    at rest: ${idleFrames} frames, ${idleFrame} allocations drawing each, ${idleTotal} with what came before"

moves=80
from="$(fLines)"
fX xdotool mousedown 1
for step in $(seq 0 $((moves - 1))); do
	fX xdotool mousemove --window "${window}" $((60 + step * 4)) $((140 + step * 4))
	sleep 0.03
done
fX xdotool mouseup 1
to="$(fLines)"
read -r dragFrames dragFrame dragTotal dragBetween < <(fMean "${from}" "${to}")
dragPerMove="$(awk -v b="${dragBetween}" -v m="${moves}" 'BEGIN { printf "%.1f", b / m }')"
echo "    drag: ${dragFrames} frames, ${dragFrame} allocations drawing each, ${dragTotal} with what came before, ${dragPerMove} per pointer move between frames"

if ((idleFrames > 0 && idleTotal == 0)); then echo "  skip: this build counts no allocations (a release build?)"; exit 3; fi
if ((!report)); then
	fCheck "frames counted at rest" test "${idleFrames}" -gt 5
	fCheck "frames counted in the drag" test "${dragFrames}" -gt 5
	fCheck "at rest: ${idleTotal} per frame, limit ${idleLimit}" test "${idleTotal}" -le "${idleLimit}"
	fCheck "drag: ${dragFrame} per frame, limit ${dragLimit}" test "${dragFrame}" -le "${dragLimit}"
	fCheck "drag: ${dragPerMove} per move between frames, limit ${moveLimit}" \
		awk -v a="${dragPerMove}" -v l="${moveLimit}" 'BEGIN { exit !(a <= l) }'
fi

if ((failures)); then
	grep -v '^\[allocs\]' "${said}" | sed 's/^/    /' | tail -n 20 || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261006 JC: Created.
##		- 20261010 JC: The drag holds its frame and its pointer moves apart.
