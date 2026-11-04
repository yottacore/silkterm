#!/usr/bin/env bash

##	- Purpose:
##		Opens Settings in the real program on headless sway, native Wayland,
##		and checks it's tied to the terminal window:
##			- Its toplevel names the terminal's as its parent.
##			- sway reads it as a dialog, so it floats above the tiled terminal
##			  rather than taking half the screen, and holds the focus.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  wtype, python3 or software Vulkan).
##	- Test ID: detyk
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
for tool in sway swaymsg wtype python3; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -f "${lvp}" ]]; then echo "  skip: no software Vulkan (${lvp})"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/dlgwl.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkdlgwl.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; swayPid=""; kbdPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${kbdPid}" ]]; then kill "${kbdPid}" 2>/dev/null || true; wait "${kbdPid}" 2>/dev/null || true; fi
	if [[ -n "${swayPid}" ]]; then kill "${swayPid}" 2>/dev/null || true; wait "${swayPid}" 2>/dev/null || true; fi
	if [[ -n "${shortRun}" && "${shortRun}" == /tmp/silkdlgwl.* ]]; then fSafeRm "${shortRun}" || true; fi
	if ((rc == 0)); then fTestDir_Rm "${work}" || true; fi
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

cat >"${work}/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1280x800 position 0 0 scale 1
xwayland disable
EOF
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SWAYSOCK XDG_RUNTIME_DIR="${runDir}" \
	WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
	sway -c "${work}/sway.conf" >"${work}/sway.log" 2>&1 &
swayPid=$!
sock=""
for _ in {1..50}; do
	sock="$(find "${runDir}" -maxdepth 1 -name 'sway-ipc.*.sock' -print -quit 2>/dev/null || true)"
	[[ -n "${sock}" && -S "${runDir}/wayland-1" ]] && break
	sleep 0.1
done
if [[ -z "${sock}" ]]; then echo "  skip: sway did not start"; exit 3; fi
wl=(env -u DISPLAY -u TMUX -u TMUX_PANE -u SILKTERM_SOCKET XDG_RUNTIME_DIR="${runDir}" WAYLAND_DISPLAY=wayland-1 SWAYSOCK="${sock}")

## The seat has no devices, so a keyboard held for the whole run.
"${wl[@]}" wtype -s 600000 &
kbdPid=$!

home="${work}/home"
mkdir -p "${home}/.config/silkterm"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'wallpaper:' $'\tenabled: false' >"${home}/.config/silkterm/config.shcl"
printf '%s\n' 'while :; do sleep 300; done' >"${work}/scene.sh"

## The client's side of the wire, for the toplevels and the parent it gives.
"${wl[@]}" WAYLAND_DEBUG=client NODEVICE_SELECT=1 LIBGL_ALWAYS_SOFTWARE=1 \
	VK_ICD_FILENAMES="${lvp}" HOME="${home}" XDG_CONFIG_HOME="${home}/.config" XDG_DATA_HOME="${home}/.local/share" \
	XDG_CACHE_HOME="${home}/.cache" XDG_STATE_HOME="${home}/.local/state" \
	"${bin}" --shell "/bin/sh ${work}/scene.sh" >/dev/null 2>"${work}/said.txt" &
appPid=$!

## One line per window of ours: "floating|tiled focused|- title".
fWindows(){
	"${wl[@]}" swaymsg -t get_tree 2>/dev/null | python3 -I -c '
import json, sys
pid = int(sys.argv[1])
def walk(node, kind):
	if node.get("pid") == pid:
		print(kind, "focused" if node.get("focused") else "-", node.get("name") or "")
	for child in node.get("nodes", []):
		walk(child, "tiled")
	for child in node.get("floating_nodes", []):
		walk(child, "floating")
walk(json.load(sys.stdin), "tiled")
' "${appPid}" || true
}
fCount(){ fWindows | grep -c . || true; }

for _ in {1..150}; do (($(fCount) >= 1)) && break; sleep 0.2; done
if (($(fCount) < 1)); then echo "  FAIL a window came up"; grep -v '^\[' "${work}/said.txt" | tail -n 20 | sed 's/^/    /'; exit 1; fi
echo "  ok   a window came up"
sleep 2
"${wl[@]}" wtype -M ctrl -k comma -m ctrl
for _ in {1..150}; do (($(fCount) >= 2)) && break; sleep 0.2; done
fCheck "Settings opened" test "$(fCount)" -ge 2
if (($(fCount) < 2)); then fWindows | sed 's/^/    /'; grep -v '^\[' "${work}/said.txt" | tail -n 20 | sed 's/^/    /'; exit 1; fi
sleep 1

## The first toplevel made is the terminal's, the second the dialog's. Older
## libwayland writes an object as name@id, newer as name#id.
mapfile -t toplevels < <(grep -oE 'get_toplevel\(new id xdg_toplevel[@#][0-9]+\)' "${work}/said.txt" | grep -oE '[0-9]+\)$' | tr -d ')' || true)
fParent(){
	((${#toplevels[@]} >= 2)) || { echo "    toplevels made: ${#toplevels[@]}"; return 1; }
	grep -qE "xdg_toplevel[@#]${toplevels[1]}\.set_parent\(xdg_toplevel[@#]${toplevels[0]}\)" "${work}/said.txt"
}
fCheck "the dialog's toplevel names the terminal's as its parent" fParent
fCheck "sway floats the dialog" grep -qE '^floating .* Settings$' <<<"$(fWindows)"
fCheck "and the terminal stays tiled" grep -qE '^tiled ' <<<"$(fWindows)"
fCheck "the dialog has the focus" grep -qE '^floating focused Settings$' <<<"$(fWindows)"

if ((failures)); then
	echo "  windows:"; fWindows | sed 's/^/      /'
	echo "  ${failures} check(s) failed; files kept in ${work}"; exit 1
fi
echo "all passed"

##	History:
##		- 20261103 JC: Created.
