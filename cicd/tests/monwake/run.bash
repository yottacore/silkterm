#!/usr/bin/env bash

##	- Purpose:
##		A monitor going dark and coming back, as two monitors do waking from
##		power save. Runs the real program on headless sway with two outputs,
##		a landscape one and a portrait one, through its Xwayland. The window
##		sits on the portrait output at the size kept for it. The portrait
##		output is turned off, so sway moves the window onto the landscape one,
##		where it takes that one's size. The portrait output is turned on again
##		and sway puts the window back. It must end at the portrait size.
##			- Turned on just after the window took the landscape size, and
##			  again 0.8 s after. Both are inside the grace after the window's
##			  own resize, where a move back was dropped (2026100907341816).
##			- Neither the dance nor the window's own resizes saves a size.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  Xwayland or python3).
##	- Test ID: EsDZaHX
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"

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
for tool in sway swaymsg Xwayland python3 ss; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/monwake.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkmon.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; swayPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
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

cat >"${work}/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1920x1080 position 1200 400
output HEADLESS-2 resolution 1200x1920 position 0 0
workspace 1 output HEADLESS-1
workspace 2 output HEADLESS-2
xwayland enable
default_border none
for_window [all] floating enable
EOF
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SWAYSOCK XDG_RUNTIME_DIR="${runDir}" \
	WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=2 WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
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
## sway listens for X clients before Xwayland is running, and starts it on the first.
xDisplay=""
for _ in {1..50}; do
	xDisplay="$(ss -xlpH 2>/dev/null | awk -v p="pid=${swayPid}," 'index($0, p) && $5 ~ /^\/tmp\/\.X11-unix\/X[0-9]+$/ { sub(/.*X/, ":", $5); print $5; exit }' || true)"
	[[ -n "${xDisplay}" ]] && break
	sleep 0.1
done
if [[ -z "${xDisplay}" ]]; then echo "  skip: no Xwayland display from sway"; exit 3; fi
"${wl[@]}" swaymsg "focus output HEADLESS-2" >/dev/null

## Xwayland's outputs report no size in mm, so the keys are resolution and scale.
cfgHome="${work}/home/.config"
mkdir -p "${cfgHome}/silkterm"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' \
	'wallpaper:' $'\tenabled: false' \
	'window:' $'\tremember_size: true' $'\tremember_per_monitor: true' \
	$'\tremembered_columns: 70' $'\tremembered_rows: 22' $'\tmonitors:' \
	$'\t\t1920x1080_100pct:' $'\t\t\tcolumns: 110' $'\t\t\trows: 20' $'\t\t\tfont_zoom: 0' \
	$'\t\t1200x1920_100pct:' $'\t\t\tcolumns: 50' $'\t\t\trows: 45' $'\t\t\tfont_zoom: 0' \
	>"${cfgHome}/silkterm/config.shcl"
## "<output> <width>x<height>" of the window, from sway.
fWhere(){
	"${wl[@]}" swaymsg -t get_tree | python3 -I -c '
import json, sys
def walk(node, output):
	if node.get("type") == "output":
		output = node["name"]
	for child in node.get("nodes", []) + node.get("floating_nodes", []):
		if child.get("pid") == int(sys.argv[1]):
			print(output, "%dx%d" % (child["rect"]["width"], child["rect"]["height"]))
		walk(child, output)
walk(json.load(sys.stdin), None)' "${appPid}"
}
## Waits up to <tenths> for the window to be somewhere else than <where>.
fLeaves(){  ## fLeaves <where> <tenths>
	local i
	for ((i = 0; i < ${2}; i++)); do [[ "$(fWhere)" != "${1}" ]] && return 0; sleep 0.1; done
	return 1
}

## A window of its own for each dance, so one that ends wrong cannot spoil the next.
fDance(){  ## fDance <seconds to wait after the landscape size is taken>
	local -r wait="${1}"
	local portrait herded landscape final
	env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET DISPLAY="${xDisplay}" XDG_RUNTIME_DIR="${runDir}" LIBGL_ALWAYS_SOFTWARE=1 \
		HOME="${work}/home" XDG_CONFIG_HOME="${cfgHome}" XDG_DATA_HOME="${work}/home/.local/share" XDG_CACHE_HOME="${work}/home/.cache" \
		XDG_STATE_HOME="${work}/home/.local/state" \
		"${bin}" --shell "/bin/dash -c 'sleep 600'" >/dev/null 2>>"${work}/said.txt" &
	appPid=$!
	for _ in {1..100}; do [[ -n "$(fWhere)" ]] && break; sleep 0.1; done
	if [[ -z "$(fWhere)" ]]; then echo "  FAIL +${wait} s: the window never showed"; failures=$((failures + 1)); return 0; fi
	"${wl[@]}" swaymsg "[pid=${appPid}] move position 100 100" >/dev/null
	sleep 3
	portrait="$(fWhere)"
	fCheck "+${wait} s: the window starts on the portrait output" test "${portrait%% *}" == HEADLESS-2
	"${wl[@]}" swaymsg "output HEADLESS-2 disable" >/dev/null
	fLeaves "${portrait}" 30 || true
	herded="$(fWhere)"
	## the window's monitor check comes 0.75 s after the move, then its resize
	fLeaves "${herded}" 40 || true
	landscape="$(fWhere)"
	sleep "${wait}"
	"${wl[@]}" swaymsg "output HEADLESS-2 enable" >/dev/null
	sleep 4
	final="$(fWhere)"
	echo "  portrait: ${portrait}, herded: ${herded}, sized there: ${landscape}, back: ${final}"
	fCheck "+${wait} s: herded onto the landscape output as it was" test "${herded}" == "HEADLESS-1 ${portrait#* }"
	fCheck "+${wait} s: took the landscape size there" test "${landscape%% *}" == HEADLESS-1 -a "${landscape#* }" != "${portrait#* }"
	fCheck "+${wait} s: back on the portrait output at its size" test "${final}" == "${portrait}"
	fStopOurs "${appPid}"; appPid=""
}
fDance 0
fDance 0.8

## The launch moved the sizes from the config to the state file.
fNoSave(){
	local got
	got="$(grep -oE '^(	remembered_(columns|rows)|			(columns|rows)): [0-9]+' "${work}/home/.local/state/silkterm/state.shcl" | tr -d '\t' | paste -sd ' ' || true)"
	[[ "${got}" == "remembered_columns: 70 remembered_rows: 22 columns: 110 rows: 20 columns: 50 rows: 45" ]] || { echo "    sizes now: ${got}"; return 1; }
}
fCheck "no size was saved" fNoSave

if ((failures)); then echo "${failures} failed; kept ${work}"; exit 1; fi
echo "all passed"

##	History:
##		- 20261009 JC: Created.
##		- 20261009 JC: The sizes are read from the state file.
