#!/usr/bin/env bash

##	- Purpose:
##		Per-monitor window size on native Wayland, where a window is told
##		neither where it is nor that it moved. Runs the real program on
##		headless sway with 3 outputs, each with its own size kept:
##			- HEADLESS-1, 1920x1080 at 100%.
##			- HEADLESS-2, 2560x1600 at 200%, right of it.
##			- HEADLESS-3, 1280x1024 at 100%, under HEADLESS-1, kept with a
##			  bigger font.
##		The window opens on HEADLESS-2 and must end at its size. Then:
##			- Moved to HEADLESS-1, where the scale change is the sign.
##			- Dragged by its title bar onto HEADLESS-3. It keeps its size while
##			  held, and takes that one's once the pointer comes back over it.
##			- Moved to HEADLESS-1, with the pointer back over it after.
##			- Dragged onto HEADLESS-2, held astride the edge on the way.
##			- Moved to HEADLESS-1, then dragged onto HEADLESS-2 and dropped
##			  with a sliver still on HEADLESS-1.
##		Each size must hold once taken, and nothing the window did may save
##		a size.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  python3 or software Vulkan).
##	- Test ID: Esgxleh
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
lvp="/usr/share/vulkan/icd.d/lvp_icd.json"

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
for tool in sway swaymsg python3; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -f "${lvp}" ]]; then echo "  skip: no software Vulkan (${lvp})"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/monwl.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkmon.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; swayPid=""; ptrPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${ptrPid}" ]]; then exec 7>&- || true; kill "${ptrPid}" 2>/dev/null || true; wait "${ptrPid}" 2>/dev/null || true; fi
	if [[ -n "${swayPid}" ]]; then kill "${swayPid}" 2>/dev/null || true; wait "${swayPid}" 2>/dev/null || true; fi
	if [[ -n "${shortRun}" && "${shortRun}" == /tmp/silkmon.* ]]; then rm -rf "${shortRun}"; fi
	if ((rc == 0)); then rm -rf "${work}"; fi
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

## Only the process launched here, and only while it is still this binary: the
## repo path says silkterm, and a dogfood copy may be running.
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

## The layout is 3200x2104, which the pointer's absolute moves are scaled to.
cat >"${work}/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1920x1080 position 0 0 scale 1
output HEADLESS-2 resolution 2560x1600 position 1920 0 scale 2
output HEADLESS-3 resolution 1280x1024 position 0 1080 scale 1
workspace 1 output HEADLESS-1
workspace 2 output HEADLESS-2
workspace 3 output HEADLESS-3
xwayland disable
default_floating_border normal
for_window [all] floating enable
EOF
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SWAYSOCK XDG_RUNTIME_DIR="${runDir}" \
	WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=3 WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
	sway -c "${work}/sway.conf" >"${work}/sway.log" 2>&1 &
swayPid=$!
sock=""
for _ in {1..50}; do
	sock="$(find "${runDir}" -maxdepth 1 -name 'sway-ipc.*.sock' -print -quit 2>/dev/null || true)"
	[[ -n "${sock}" && -S "${runDir}/wayland-1" ]] && break
	sleep 0.1
done
if [[ -z "${sock}" ]]; then echo "  skip: sway did not start"; exit 3; fi
wl=(env -u DISPLAY XDG_RUNTIME_DIR="${runDir}" WAYLAND_DISPLAY=wayland-1 SWAYSOCK="${sock}")

## A pointer of its own, since the seat has no devices.
mkfifo "${work}/ptr.in"
"${wl[@]}" python3 -I "${meDir}/_vpointer.py" 3200 2104 <"${work}/ptr.in" >"${work}/ptr.out" 2>"${work}/ptr.err" &
ptrPid=$!
exec 7>"${work}/ptr.in"
for _ in {1..50}; do grep -q '^ready$' "${work}/ptr.out" 2>/dev/null && break; sleep 0.1; done
if ! grep -q '^ready$' "${work}/ptr.out"; then echo "  skip: no virtual pointer: $(cat "${work}/ptr.err")"; exit 3; fi
fPtr(){  ## fPtr <command...>, back once the compositor has it
	local -i before; before="$(grep -c '^ok$' "${work}/ptr.out" || true)"
	echo "${*}" >&7
	local _
	for _ in {1..50}; do (($(grep -c '^ok$' "${work}/ptr.out" || true) > before)) && return 0; sleep 0.02; done
	echo "  the pointer did not answer: ${*}"; return 1
}
## Off every window, in a corner of HEADLESS-2.
fPtr move 3100 700
"${wl[@]}" swaymsg "focus output HEADLESS-2" >/dev/null

## Wayland gives no size in mm, so the keys are resolution and scale. No
## minimap, so the shell's columns are the kept ones.
cfgHome="${work}/home/.config"
mkdir -p "${cfgHome}/silkterm"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' \
	'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' \
	'window:' $'\tremember_size: true' $'\tremember_per_monitor: true' \
	$'\tremembered_columns: 70' $'\tremembered_rows: 22' $'\tmonitors:' \
	$'\t\t1920x1080_100pct:' $'\t\t\tcolumns: 110' $'\t\t\trows: 20' $'\t\t\tfont_zoom: 0' \
	$'\t\t2560x1600_200pct:' $'\t\t\tcolumns: 50' $'\t\t\trows: 30' $'\t\t\tfont_zoom: 0' \
	$'\t\t1280x1024_100pct:' $'\t\t\tcolumns: 60' $'\t\t\trows: 35' $'\t\t\tfont_zoom: 4' \
	>"${cfgHome}/silkterm/config.shcl"

## The shell writes its own "rows columns" every tenth of a second.
grid="${work}/grid.txt"; : >"${grid}"
"${wl[@]}" LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES="${lvp}" VK_LAYER_MESSAGE_ID_FILTER=VUID-vkCmdCopyBufferToImage-pRegions-00173 \
	HOME="${work}/home" XDG_CONFIG_HOME="${cfgHome}" XDG_DATA_HOME="${work}/home/.local/share" XDG_CACHE_HOME="${work}/home/.cache" \
	XDG_STATE_HOME="${work}/home/.local/state" \
	"${bin}" --shell "/bin/dash -c 'while :; do stty size >>${grid}; sleep 0.1; done'" >/dev/null 2>>"${work}/said.txt" &
appPid=$!

fGrid(){ tail -n 1 "${grid}" 2>/dev/null || true; }
## "<output> <x> <y> <width> <height> <title bar height>" of the window, from
## sway, which leaves the title bar above y out of the rest.
fWin(){
	"${wl[@]}" swaymsg -t get_tree | python3 -I -c '
import json, sys
def walk(node, output):
	if node.get("type") == "output":
		output = node["name"]
	for child in node.get("nodes", []) + node.get("floating_nodes", []):
		if child.get("pid") == int(sys.argv[1]):
			r = child["rect"]
			print(output, r["x"], r["y"], r["width"], r["height"], child["deco_rect"]["height"])
		walk(child, output)
walk(json.load(sys.stdin), None)' "${appPid}"
}
fOutput(){ local w; w="$(fWin)"; echo "${w%% *}"; }
## Waits up to <tenths> for the shell to see <rows columns>.
fWaitGrid(){  ## fWaitGrid <want> <tenths>
	local i
	for ((i = 0; i < ${2}; i++)); do [[ "$(fGrid)" == "${1}" ]] && return 0; sleep 0.1; done
	return 1
}
## Nothing but <rows columns> for <seconds>: no swap back and forth.
fHolds(){  ## fHolds <want> <seconds>
	local -i from; from="$(wc -l <"${grid}")"
	sleep "${2}"
	local seen; seen="$(tail -n +"$((from + 1))" "${grid}" | sort -u | paste -sd ',' || true)"
	[[ "${seen}" == "${1}" ]] || { echo "    seen: ${seen}"; return 1; }
}
## Off the window, then onto its middle.
fPointerBack(){
	local x y w h
	read -r _ x y w h _ <<<"$(fWin)"
	fPtr move 3100 700
	sleep 0.3
	fPtr move "$((x + w / 2))" "$((y + h / 2))"
}
fStep(){  ## fStep <what> <output> <want>
	fCheck "${1}: on ${2}" test "$(fOutput)" == "${2}"
	fCheck "${1}: takes ${3/ /x} (rows x columns)" fWaitGrid "${3}" 50
	fCheck "${1}: and keeps it" fHolds "${3}" 3
	echo "    window: $(fWin), shell: $(fGrid)"
}

for _ in {1..100}; do [[ -n "$(fWin)" ]] && break; sleep 0.1; done
if [[ -z "$(fWin)" ]]; then echo "  FAIL the window never showed"; exit 1; fi
fStep "opened on the second output" HEADLESS-2 "30 50"
echo "    first sizes: $(uniq "${grid}" | head -n 4 | paste -sd ',')"
opened="$(wc -l <"${grid}")"

"${wl[@]}" swaymsg "[pid=${appPid}] move container to output HEADLESS-1" >/dev/null
fStep "moved to the 100% output" HEADLESS-1 "20 110"
winW1="$(read -r _ _ _ w _ <<<"$(fWin)"; echo "${w}")"

## A drag is the pointer down on the title bar, which the window follows.
ptrX=0; ptrY=0
fGlide(){  ## fGlide <x> <y>, in 10 steps
	local -r fromX="${ptrX}" fromY="${ptrY}"
	local i
	for ((i = 1; i <= 10; i++)); do
		fPtr move "$((fromX + (${1} - fromX) * i / 10))" "$((fromY + (${2} - fromY) * i / 10))"
		sleep 0.05
	done
	ptrX="${1}"; ptrY="${2}"
}
## Takes the title bar, and says where the pointer is on the window.
fGrab(){
	local x y w deco
	read -r _ x y w _ deco <<<"$(fWin)"
	grabDx=$((w / 2)); grabDy=$((-deco / 2))
	ptrX=$((x + grabDx)); ptrY=$((y + grabDy))
	fPtr move "${ptrX}" "${ptrY}"
	fPtr down
}
fDragTo(){ fGlide "$((${1} + grabDx))" "$((${2} + grabDy))"; }  ## fDragTo <left> <top>
fDrop(){ fPtr up; sleep 1.5; echo "    dropped, pointer on the title bar: $(fWin), shell: $(fGrid)"; }

## Down onto HEADLESS-3. Same scale, so only the pointer coming back over the
## window says it moved.
fGrab
fDragTo 40 1140
sleep 1.5
fCheck "held over HEADLESS-3: keeps its size" test "$(fGrid)" == "20 110"
fDrop
fPtr move "$((40 + grabDx))" 1200
fStep "dragged to the 1280x1024 output" HEADLESS-3 "35 60"
## The kept font zoom too: wider cells for fewer columns.
fWider(){ local w; read -r _ _ _ w _ <<<"$(fWin)"; ((w * 110 > winW1 * 60)); }
fCheck "dragged to the 1280x1024 output: at its own font size" fWider

"${wl[@]}" swaymsg "[pid=${appPid}] move container to output HEADLESS-1" >/dev/null
sleep 1.5
echo "    moved, pointer elsewhere: $(fWin), shell: $(fGrid)"
fPointerBack
fStep "moved back to the 100% output" HEADLESS-1 "20 110"

## Across onto HEADLESS-2, held a while astride the edge on the way.
fGrab
fDragTo 1300 100
sleep 2
echo "    held astride: $(fWin), shell: $(fGrid)"
fCheck "held astride the 100% and 200% outputs: keeps its size" test "$(fGrid)" == "20 110"
fDragTo 1960 20
sleep 1.5
fDrop
fPointerBack
fStep "dragged to the 200% output" HEADLESS-2 "30 50"

"${wl[@]}" swaymsg "[pid=${appPid}] move container to output HEADLESS-1" >/dev/null
fStep "moved to the 100% output again" HEADLESS-1 "20 110"

## Dropped with a sliver left on HEADLESS-1. It is HEADLESS-2's to size.
fGrab
fDragTo 1880 20
fDrop
fPointerBack
fStep "dropped mostly on the 200% output" HEADLESS-2 "30 50"

## Every output here has a size kept, so the last size anywhere, which a
## monitor with none gets, is never right once the window has opened.
fNoFallback(){
	local -i n
	n="$(tail -n +"$((opened + 1))" "${grid}" | grep -c '^22 70$' || true)"
	((n == 0)) || { echo "    at 22x70 for ${n} samples, a tenth of a second each"; return 1; }
}
fCheck "never at the last size anywhere after it opened" fNoFallback

## The launch moved the sizes from the config to the state file.
fNoSave(){
	local got
	got="$(grep -oE '^(	remembered_(columns|rows)|			(columns|rows|font_zoom)): [0-9]+' "${work}/home/.local/state/silkterm/state.shcl" | tr -d '\t' | paste -sd ' ' || true)"
	[[ "${got}" == "remembered_columns: 70 remembered_rows: 22 columns: 110 rows: 20 font_zoom: 0 columns: 50 rows: 30 font_zoom: 0 columns: 60 rows: 35 font_zoom: 4" ]] || { echo "    sizes now: ${got}"; return 1; }
}
fCheck "no size was saved" fNoSave

if ((failures)); then echo "${failures} failed; kept ${work}"; exit 1; fi
echo "all passed"

##	History:
##		- 20261014 JC: Created.
