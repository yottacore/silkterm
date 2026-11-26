#!/usr/bin/env bash

##	- Purpose:
##		Makes the program's window wider than the device's largest texture,
##		which a window manager or a drag across monitors can do. It renders in
##		software, where the limit is 16384. Before the fix the program died in
##		wgpu's error handler at the resize.
##			- On X11 with no window manager, the window gets that big. It stays
##			  up and draws up to the limit.
##			- On X11 under xfwm4, the window is held at the limit.
##			- On native Wayland, headless sway tiles it on an output wider than
##			  the limit, and it stays up on the Vulkan surface.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, Xvfb,
##	  xfwm4, xdotool, sway, software Vulkan, or free display).
##	- Test ID: dfek4
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
lvp="/usr/share/vulkan/icd.d/lvp_icd.json"
limit=16384

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
for tool in Xvfb xfwm4 xdpyinfo xdotool sway; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -f "${lvp}" ]]; then echo "  skip: no software Vulkan (${lvp})"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/oversize.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkoversize.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; xvfbPid=""; wmPid=""; swayPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${wmPid}" ]]; then kill "${wmPid}" 2>/dev/null || true; wait "${wmPid}" 2>/dev/null || true; fi
	if [[ -n "${xvfbPid}" ]]; then kill "${xvfbPid}" 2>/dev/null || true; wait "${xvfbPid}" 2>/dev/null || true; fi
	if [[ -n "${swayPid}" ]]; then kill "${swayPid}" 2>/dev/null || true; wait "${swayPid}" 2>/dev/null || true; fi
	if [[ -n "${shortRun}" && "${shortRun}" == /tmp/silkoversize.* ]]; then fSafeRm "${shortRun}" || true; fi
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
fAlive(){ [[ -n "${appPid}" ]] && kill -0 "${appPid}" 2>/dev/null; }

home="${work}/home"
mkdir -p "${home}/.config/silkterm"
## X11 draws through GL by default, and the offscreen texture there is what
## was refused. Wayland draws on Vulkan, and software rendering keeps it off
## the card, whose limit can be higher.
xConfig="${work}/x11.shcl"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' $'\tremember_size: false' \
	'wallpaper:' $'\tenabled: false' >"${xConfig}"
sed 's/^\tremember_size: false$/&\n\tsoftware_rendering: true/' "${xConfig}" >"${home}/.config/silkterm/config.shcl"
## A software frame this wide takes every core it's given.
appEnv=(LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES="${lvp}" NODEVICE_SELECT=1 LP_NUM_THREADS="${CICD_MAX_JOBS:-4}"
	HOME="${home}" XDG_CONFIG_HOME="${home}/.config" XDG_DATA_HOME="${home}/.local/share"
	XDG_CACHE_HOME="${home}/.cache" XDG_STATE_HOME="${home}/.local/state")

## A display of our own, since the shared one is too narrow for the case.
display=""
for n in {143..160}; do
	if [[ ! -e "/tmp/.X${n}-lock" && ! -e "/tmp/.X11-unix/X${n}" ]] && ! DISPLAY=":${n}" xdpyinfo >/dev/null 2>&1; then display=":${n}"; break; fi
done
if [[ -z "${display}" ]]; then echo "  skip: no free display from :143 to :160"; exit 3; fi
Xvfb "${display}" -screen 0 "$((limit + 1000))x1000x24" -nolisten tcp >"${work}/xvfb.log" 2>&1 &
xvfbPid=$!
for _ in {1..50}; do DISPLAY="${display}" xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done
if ! kill -0 "${xvfbPid}" 2>/dev/null; then echo "  skip: Xvfb did not start on ${display}"; xvfbPid=""; exit 3; fi

fLaunchX(){  ## fLaunchX <case>
	DISPLAY="${display}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SESSION_MANAGER -u TMUX -u TMUX_PANE -u SILKTERM_SOCKET "${appEnv[@]}" \
		"${bin}" --config "${xConfig}" --pixel-width 800 --pixel-height 500 --shell /bin/sh >/dev/null 2>"${work}/${1}.txt" &
	appPid=$!
	win="$(DISPLAY="${display}" timeout 60 xdotool search --sync --onlyvisible --pid "${appPid}" 2>/dev/null | head -n 1 || true)"
	[[ -n "${win}" ]] && grep -qF '[Gl / Cpu]' "${work}/${1}.txt"
}
fSizeX(){ DISPLAY="${display}" xdotool getwindowgeometry "${win}" 2>/dev/null | awk '/Geometry/ { print $2 }'; }
fWidthIs(){ [[ "$(fSizeX)" == "${1}x"* ]] || { echo "    window now $(fSizeX)"; return 1; }; }

echo "X11, no window manager"
win=""
if fLaunchX x11bare; then
	DISPLAY="${display}" xdotool windowsize "${win}" "$((limit + 500))" 600 2>/dev/null || true
	sleep 4
	fCheck "x11: the window is past the limit" fWidthIs "$((limit + 500))"
	fCheck "x11: and the program is still up" fAlive
	DISPLAY="${display}" xdotool windowsize "${win}" 1000 600 2>/dev/null || true
	sleep 2
	fCheck "x11: and once it's back under the limit" fAlive
else
	fCheck "x11: the window came up on software GL" false
fi
fStopOurs "${appPid}"; appPid=""

echo "X11, xfwm4"
mkdir -p "${work}/wmhome"
DISPLAY="${display}" env -u SESSION_MANAGER HOME="${work}/wmhome" xfwm4 --compositor=off >"${work}/wm.log" 2>&1 &
wmPid=$!
sleep 2
if fLaunchX x11wm; then
	DISPLAY="${display}" xdotool windowsize "${win}" "$((limit + 500))" 600 2>/dev/null || true
	sleep 4
	fCheck "xfwm4: the window is held at the limit" fWidthIs "${limit}"
	fCheck "xfwm4: and the program is still up" fAlive
else
	fCheck "xfwm4: the window came up on software GL" false
fi
fStopOurs "${appPid}"; appPid=""

echo "Wayland, sway"
printf '%s\n' "output HEADLESS-1 resolution $((limit + 600))x900 position 0 0 scale 1" 'xwayland disable' 'default_border none' >"${work}/sway.conf"
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
if [[ -z "${sock}" ]]; then
	echo "  skip: sway did not start"
else
	said="${work}/wayland.txt"
	env -u DISPLAY -u TMUX -u TMUX_PANE -u SILKTERM_SOCKET XDG_RUNTIME_DIR="${runDir}" WAYLAND_DISPLAY=wayland-1 "${appEnv[@]}" \
		"${bin}" --shell /bin/sh >/dev/null 2>"${said}" &
	appPid=$!
	for _ in {1..150}; do grep -qF 'renderer = ' "${said}" && break; fAlive || break; sleep 0.2; done
	fCheck "wayland: the window came up on Vulkan" grep -qF '[Vulkan / Cpu]' "${said}"
	sleep 4
	fCheck "wayland: and is still up, tiled past the limit" fAlive
fi

if ((failures)); then
	for f in "${work}"/*.txt; do echo "  ${f##*/}:"; tail -n 8 "${f}" 2>/dev/null | sed 's/^/    /' || true; done
	echo "  ${failures} check(s) failed; files kept in ${work}"; exit 1
fi
echo "all passed"

##	History:
##		- 20261117 JC: Created.
