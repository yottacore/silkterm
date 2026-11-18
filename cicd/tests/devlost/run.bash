#!/usr/bin/env bash

##	- Purpose:
##		Loses the GPU device of the real program on headless sway, native
##		Wayland, as a driver reset or a resume without video memory does,
##		through SILK_LOSE_DEVICE:
##			- With only the terminal up, it sees the loss, builds a new
##			  device, and draws again without any input.
##			- With Settings up, both get new devices, and nothing panics.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  wtype, or software Vulkan).
##	- Test ID: dfeg0
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
for tool in sway swaymsg wtype; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -f "${lvp}" ]]; then echo "  skip: no software Vulkan (${lvp})"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/devlost.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkdevlost.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; swayPid=""; kbdPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${kbdPid}" ]]; then kill "${kbdPid}" 2>/dev/null || true; wait "${kbdPid}" 2>/dev/null || true; fi
	if [[ -n "${swayPid}" ]]; then kill "${swayPid}" 2>/dev/null || true; wait "${swayPid}" 2>/dev/null || true; fi
	if [[ -n "${shortRun}" && "${shortRun}" == /tmp/silkdevlost.* ]]; then fSafeRm "${shortRun}" || true; fi
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

printf '%s\n' 'output HEADLESS-1 resolution 1280x800 position 0 0 scale 1' 'xwayland disable' >"${work}/sway.conf"
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

## The seat has no devices, so wtype adds a keyboard for the whole run.
"${wl[@]}" wtype -s 600000 &
kbdPid=$!

home="${work}/home"
mkdir -p "${home}/.config/silkterm"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'wallpaper:' $'\tenabled: false' >"${home}/.config/silkterm/config.shcl"
## Output twice a second, so the window keeps asking for frames.
# shellcheck disable=SC2016 # written for the scene's shell to expand
printf '%s\n' 'n=0; while :; do n=$((n + 1)); echo "line ${n}"; sleep 0.5; done' >"${work}/scene.sh"
lose="${work}/lose"
said="${work}/said.txt"

"${wl[@]}" NODEVICE_SELECT=1 LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES="${lvp}" \
	SILK_IDLEDBG=1 SILK_LOSE_DEVICE="${lose}" \
	HOME="${home}" XDG_CONFIG_HOME="${home}/.config" XDG_DATA_HOME="${home}/.local/share" \
	XDG_CACHE_HOME="${home}/.cache" XDG_STATE_HOME="${home}/.local/state" \
	"${bin}" --shell "/bin/sh ${work}/scene.sh" >/dev/null 2>"${said}" &
appPid=$!

fAlive(){ kill -0 "${appPid}" 2>/dev/null; }
fSaid(){ grep -cF -- "${1}" "${said}" || true; }
## fWaitSaid <text> <count>: until the text has been said that many times
fWaitSaid(){ local _; for _ in {1..100}; do (($(fSaid "${1}") >= ${2})) && return 0; fAlive || return 1; sleep 0.1; done; return 1; }

for _ in {1..150}; do grep -qF 'renderer = ' "${said}" && break; sleep 0.2; done
if ! grep -qF 'renderer = ' "${said}"; then echo "  FAIL the window came up"; tail -n 20 "${said}" | sed 's/^/    /'; exit 1; fi
echo "  ok   the window came up"
sleep 3

## The terminal alone. Before the fix every frame after the loss was refused,
## at most 2 s apart, for good.
touch "${lose}"
fCheck "the terminal sees its device is lost" fWaitSaid "the GPU device was lost" 1
fCheck "and builds a new one" fWaitSaid "device rebuilt" 1
refused="$(fSaid "frame refused")"
sleep 3
fCheck "and its frames are drawn again" test "$(fSaid "frame refused")" -eq "${refused}"
fCheck "and it's still running" fAlive

## Settings up, drawing on the dialogs' kept device. Settings draws only when
## asked, and a loss is found only by drawing, so a key asks.
"${wl[@]}" wtype -M ctrl -k comma -m ctrl
fCheck "Settings opened" fWaitSaid "kept: a dialog is up" 1
sleep 2
touch "${lose}"
sleep 0.5
"${wl[@]}" wtype -k Tab
fCheck "the terminal builds another device with Settings up" fWaitSaid "device rebuilt" 2
fCheck "Settings gets a new device too" fWaitSaid "a dialog lost its GPU device and has a new one" 1
refused="$(fSaid "frame refused")"
sleep 3
fCheck "and the terminal's frames are drawn again" test "$(fSaid "frame refused")" -eq "${refused}"
fCheck "and nothing panicked" test "$(fSaid "panicked")" -eq 0
fCheck "and it's still running" fAlive

if ((failures)); then
	echo "  said:"; grep -v 'wait restarted' "${said}" | tail -n 30 | sed 's/^/    /'
	echo "  ${failures} check(s) failed; files kept in ${work}"; exit 1
fi
echo "all passed"

##	History:
##		- 20261117 JC: Created.
