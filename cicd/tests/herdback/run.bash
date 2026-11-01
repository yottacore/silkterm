#!/usr/bin/env bash

##	- Purpose:
##		A window xfwm4 herded off a monitor that went dark goes back where it
##		was when the monitor comes back, as every other window does. Runs the
##		real program under xfwm4, on an Xwayland of its own that shows the two
##		outputs of a headless sway as two monitors: a landscape one and a
##		portrait one. The window sits on the portrait one. That output is
##		turned off, so xfwm4 moves the window onto the landscape one, and on
##		again, so xfwm4 puts it back. It must end where it started, at the
##		size it started at.
##			- xfwm4 keeps where to put a window back only until the window
##			  resizes itself. Taking the landscape monitor's size there left it
##			  where it had been herded to (2026102807384201).
##			- Turned on again 4 s after, past the window's look, and 1 s after,
##			  inside it.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  Xwayland, xfwm4 or xdotool).
##	- Test ID: deqx1
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
for tool in sway swaymsg Xwayland xfwm4 xdotool xprop; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/herdback.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkherd.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; swayPid=""; xPid=""; wmPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	## xfwm4 goes with its X server
	local pid
	for pid in "${wmPid}" "${xPid}" "${swayPid}"; do
		if [[ -n "${pid}" ]]; then kill "${pid}" 2>/dev/null || true; wait "${pid}" 2>/dev/null || true; fi
	done
	if [[ -n "${shortRun}" && "${shortRun}" == /tmp/silkherd.* ]]; then fSafeRm "${shortRun}" || true; fi
	if ((rc == 0)); then fTestDir_Rm "${work}" || true; fi
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

## sway's own Xwayland would make sway the window manager, so it is off, and
## the Xwayland started below is xfwm4's.
cat >"${work}/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1920x1080 position 1200 400
output HEADLESS-2 resolution 1200x1920 position 0 0
xwayland disable
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
fSway(){ env -u DISPLAY XDG_RUNTIME_DIR="${runDir}" SWAYSOCK="${sock}" swaymsg "${@}" >/dev/null; }

exec {numFd}<> <(:)
env -u DISPLAY XDG_RUNTIME_DIR="${runDir}" WAYLAND_DISPLAY=wayland-1 \
	Xwayland -rootless -noreset -displayfd "${numFd}" >"${work}/xwayland.log" 2>&1 &
xPid=$!
xDisplay=""
read -r -t 10 -u "${numFd}" xDisplay || true
exec {numFd}<&-
if [[ -z "${xDisplay}" ]]; then echo "  skip: Xwayland did not start"; exit 3; fi
xDisplay=":${xDisplay}"
fX(){ env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET DISPLAY="${xDisplay}" "${@}"; }

## xfwm4 asks xfconf for its settings over the session bus, and does without one.
wm=(xfwm4 --compositor=off)
if command -v dbus-run-session >/dev/null 2>&1; then wm=(dbus-run-session -- "${wm[@]}"); fi
fX HOME="${work}/home" XDG_CONFIG_HOME="${work}/home/.config" "${wm[@]}" >"${work}/xfwm4.log" 2>&1 &
wmPid=$!
for _ in {1..50}; do
	[[ "$(fX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)" == *"window id"* ]] && break
	sleep 0.1
done
if [[ "$(fX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)" != *"window id"* ]]; then echo "  skip: xfwm4 did not start"; exit 3; fi

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

## "<x>,<y> <width>x<height>" of the window.
fWhere(){
	local -r id="${1}"
	local X Y WIDTH HEIGHT
	eval "$(fX xdotool getwindowgeometry --shell "${id}" 2>/dev/null | grep -E '^(X|Y|WIDTH|HEIGHT)=[0-9-]+$' || true)"
	[[ -n "${X-}" ]] && echo "${X},${Y} ${WIDTH}x${HEIGHT}"
}

## A window of its own for each dance, so one that ends wrong cannot spoil the next.
fDance(){  ## fDance <seconds the portrait output stays off>
	local -r off="${1}"
	local id="" start="" herded final
	fX XDG_RUNTIME_DIR="${runDir}" LIBGL_ALWAYS_SOFTWARE=1 \
		HOME="${work}/home" XDG_CONFIG_HOME="${cfgHome}" XDG_DATA_HOME="${work}/home/.local/share" XDG_CACHE_HOME="${work}/home/.cache" \
		XDG_STATE_HOME="${work}/home/.local/state" \
		"${bin}" --shell "/bin/dash -c 'sleep 600'" >/dev/null 2>>"${work}/said.txt" &
	appPid=$!
	for _ in {1..100}; do
		id="$(fX xdotool search --pid "${appPid}" --name '^SilkTerm' 2>/dev/null | head -n 1 || true)"
		[[ -n "${id}" ]] && break
		sleep 0.1
	done
	if [[ -z "${id}" ]]; then echo "  FAIL ${off} s off: the window never showed"; failures=$((failures + 1)); return 0; fi
	## a move before the window has settled from its launch can be undone by it
	for _ in {1..10}; do
		sleep 1
		fX xdotool windowmove "${id}" 100 100
		sleep 2
		start="$(fWhere "${id}")"
		[[ "${start%%,*}" == 1?? ]] && break
	done
	fCheck "${off} s off: the window starts on the portrait output" test "${start%%,*}" -lt 1200
	fSway "output HEADLESS-2 disable"
	sleep "${off}"
	herded="$(fWhere "${id}")"
	fSway "output HEADLESS-2 enable"
	sleep 4
	final="$(fWhere "${id}")"
	echo "  start: ${start}, herded: ${herded}, back: ${final}"
	fCheck "${off} s off: the window was herded" test -n "${herded}" -a "${herded%% *}" != "${start%% *}"
	fCheck "${off} s off: back where it was, at its size" test -n "${start}" -a "${final}" == "${start}"
	fStopOurs "${appPid}"; appPid=""
}
fDance 4
fDance 1

if ((failures)); then echo "${failures} failed; kept ${work}"; exit 1; fi
echo "all passed"

##	History:
##		- 20261101 JC: Created.
