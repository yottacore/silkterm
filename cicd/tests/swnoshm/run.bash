#!/usr/bin/env bash

##	- Purpose:
##		Software rendering on an X server that cannot make shared pixmaps, as
##		NVIDIA's cannot. Mesa's software Vulkan used them anyway there, and
##		winit died of the X errors that followed. The server here is headless
##		sway's Xwayland behind _noshm_proxy.py, which answers as such a server.
##			- Launched with software rendering on, the window draws and
##			  Settings opens, with no X error.
##			- Launched on the card and then switched to software, the window
##			  takes its new device with no X error.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  Xwayland with DRI3, render node, python-xlib, xdotool, xwd or ImageMagick).
##	- Test ID: ErxxHzT
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
for tool in sway Xwayland xdotool xdpyinfo xwininfo xwd convert ss python3; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if ! python3 -c 'import Xlib' 2>/dev/null; then echo "  skip: no python-xlib"; exit 3; fi
renderNode=""
for node in /dev/dri/renderD*; do
	if [[ -r "${node}" && -w "${node}" ]]; then renderNode="${node}"; break; fi
done
## Xwayland gets DRI3 only from a compositor drawing on a GPU.
if [[ -z "${renderNode}" ]]; then echo "  skip: no render node"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/swnoshm.XXXXXX")"
appPid=""; swayPid=""; proxyPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${proxyPid}" ]]; then kill "${proxyPid}" 2>/dev/null || true; wait "${proxyPid}" 2>/dev/null || true; fi
	if [[ -n "${swayPid}" ]]; then kill "${swayPid}" 2>/dev/null || true; wait "${swayPid}" 2>/dev/null || true; fi
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

## sway takes whichever X display number is free, so it is read from the
## socket sway itself listens on.
mkdir -p "${work}/sway"
printf 'output HEADLESS-1 resolution 1280x800 position 0 0\nxwayland force\ndefault_border none\n' >"${work}/sway.conf"
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET XDG_RUNTIME_DIR="${work}/sway" WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 \
	WLR_RENDERER=gles2 WLR_RENDER_DRM_DEVICE="${renderNode}" WLR_LIBINPUT_NO_DEVICES=1 \
	sway -c "${work}/sway.conf" >"${work}/sway.log" 2>&1 &
swayPid=$!
realDisplay=""
for _ in {1..100}; do
	sockets="$(ss -xlpH 2>/dev/null || true)"
	realDisplay="$(awk -v p="pid=${swayPid}," 'index($0, p) && $5 ~ /^\/tmp\/\.X11-unix\/X[0-9]+$/ {sub(/.*X/, ":", $5); print $5; exit}' <<<"${sockets}")"
	if [[ -n "${realDisplay}" ]] && DISPLAY="${realDisplay}" xdpyinfo >/dev/null 2>&1; then break; fi
	realDisplay=""
	sleep 0.1
done
if [[ -z "${realDisplay}" ]]; then echo "  skip: sway's Xwayland did not come up"; exit 3; fi
extensions="$(DISPLAY="${realDisplay}" xdpyinfo -queryExtensions 2>/dev/null || true)"
if ! grep -q 'DRI3' <<<"${extensions}"; then echo "  skip: Xwayland has no DRI3"; exit 3; fi

## The first number nothing answers on and nothing has a socket for.
display=""
for n in {160..199}; do
	if [[ -e "/tmp/.X11-unix/X${n}" ]] || DISPLAY=":${n}" xdpyinfo >/dev/null 2>&1; then continue; fi
	display=":${n}"; break
done
if [[ -z "${display}" ]]; then echo "  skip: no free display number"; exit 3; fi
python3 "${meDir}/_noshm_proxy.py" "${realDisplay}" "${display#:}" 2>"${work}/proxy.log" &
proxyPid=$!
for _ in {1..50}; do [[ -S "/tmp/.X11-unix/X${display#:}" ]] && break; sleep 0.1; done
fX(){ DISPLAY="${display}" "${@}"; }
if ! fX xdpyinfo >/dev/null 2>&1; then echo "  FAIL the stand-in server did not answer"; exit 1; fi
echo "  server: ${realDisplay} answered as ${display}"

fLaunch(){  ## fLaunch <case> <software true|false>
	local -r dir="${work}/${1}"
	mkdir -p "${dir}/home" "${dir}/run"
	printf '%s\n' 'performance:' $'\tautomatic: false' 'window:' $'\tremember_per_monitor: false' $'\tsoftware_rendering: '"${2}" >"${dir}/config.shcl"
	DISPLAY="${display}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u MESA_VK_WSI_DEBUG SILK_IDLEDBG=1 \
		HOME="${dir}/home" XDG_CONFIG_HOME="${dir}/home/.config" XDG_DATA_HOME="${dir}/home/.local/share" XDG_RUNTIME_DIR="${dir}/run" \
		"${bin}" --config "${dir}/config.shcl" --shell "/bin/sh -c 'sleep 120'" >/dev/null 2>"${dir}/said.txt" &
	appPid=$!
	window="$(fX timeout 30 xdotool search --sync --onlyvisible --pid "${appPid}" 2>/dev/null | head -n 1 || true)"
	sleep 5
}

fAlive(){ kill -0 "${appPid}" 2>/dev/null; }
fNoXError(){ ! grep -Eq 'X11 error|panicked' "${work}/${1}/said.txt"; }
fSaid(){ grep -q "${2}" "${work}/${1}/said.txt"; }
## A window nothing drew on is one flat color.
fDrew(){
	local spread
	spread="$(fX xwd -silent -id "${window}" 2>/dev/null | convert xwd:- -format '%[fx:standard_deviation]' info: 2>/dev/null || echo 0)"
	awk -v s="${spread}" 'BEGIN { exit !(s > 0.01) }'
}
fSettingsShown(){
	local _ id
	for _ in {1..40}; do
		for id in $(fX xdotool search --onlyvisible --pid "${appPid}" 2>/dev/null || true); do
			if grep -q '"Settings"' <<<"$(fX xwininfo -id "${id}" 2>/dev/null || true)"; then return 0; fi
		done
		sleep 0.25
	done
	return 1
}
fDone(){ fStopOurs "${appPid}"; appPid=""; }

echo "launched in software"
fLaunch launch true
fCheck "launch: a window came up" test -n "${window}"
fCheck "launch: on the software renderer" fSaid launch 'Cpu]'
fCheck "launch: still running" fAlive
fCheck "launch: the window drew" fDrew
fX xdotool windowfocus --sync "${window}" 2>/dev/null || true
fX xdotool key --clearmodifiers ctrl+comma 2>/dev/null || true
fCheck "launch: Settings opened" fSettingsShown
sleep 2
fCheck "launch: still running with Settings up" fAlive
fCheck "launch: no X error" fNoXError launch
fDone

echo "switched to software"
fLaunch swap false
fCheck "swap: a window came up" test -n "${window}"
if fSaid swap 'Cpu]'; then
	echo "  skip swap: the card made no device here"
else
	sed -i 's/software_rendering: false/software_rendering: true/' "${work}/swap/config.shcl"
	SILKTERM_SOCKET="${work}/swap/run/silkterm-ctl-${appPid}.sock" "${bin}" --reload-settings >/dev/null 2>&1 || true
	## The swap waits for the loop's next pass, and a pointer move is one.
	sleep 2
	fX xdotool mousemove --window "${window}" 40 40 2>/dev/null || true
	sleep 4
	fCheck "swap: the setting was seen" fSaid swap 'software rendering setting changed'
	fCheck "swap: rebuilt in software" fSaid swap 'device rebuilt.*Software'
	fCheck "swap: still running" fAlive
	fCheck "swap: no X error" fNoXError swap
fi
fDone

if ((failures)); then
	for said in "${work}"/*/said.txt; do
		echo "  ${said#"${work}/"}:"
		grep -v VALIDATION "${said}" | sed 's/^/    /' | tail -n 12
	done
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261006 JC: Created.
