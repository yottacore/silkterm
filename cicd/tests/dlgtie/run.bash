#!/usr/bin/env bash

##	- Purpose:
##		Opens Settings in the real program under xfwm4 on the private display,
##		and checks it stays tied to the terminal window:
##			- Transient for it, a dialog, and modal.
##			- Off the taskbar and the pager, as the window manager holds the
##			  state, so the two windows make one entry in each.
##			- Above the terminal after a click on the terminal, a raise of it,
##			  and another program's window coming in front and going back.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  window manager, xdotool, or xprop).
##	- Test ID: der6f
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
for tool in Xvfb xfwm4 xdotool xprop; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/dlgtie.XXXXXX")"
appPid=""; otherPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if [[ -n "${otherPid}" ]]; then kill "${otherPid}" 2>/dev/null || true; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	local pid; for pid in $(fLeftovers); do kill "${pid}" 2>/dev/null || true; done
	if [[ "${rc}" == 0 ]]; then fTestDir_Rm "${work}" || true; fi
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

## Anything still running from this run's folder, by the folder in its command line.
fLeftovers(){
	local dir pid
	local -a args
	for dir in /proc/[0-9]*; do
		pid="${dir#/proc/}"
		[[ "${pid}" == "$$" || "${pid}" == "${BASHPID}" ]] && continue
		mapfile -d '' args 2>/dev/null <"${dir}/cmdline" || continue
		[[ "${args[*]}" == *"${work}/"* ]] && echo "${pid}"
	done
	return 0
}

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start --wm >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
## A function sent to the background runs in a subshell, so --bg execs, and $!
## is the program and not a bash waiting on it.
fX(){ local how=""; [[ "${1:-}" == "--bg" ]] && { how="exec"; shift; }; DISPLAY="${display}" XAUTHORITY="${auth}" ${how} "${@}"; }
if ! fX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -qF 'window id'; then
	echo "  skip: no window manager on ${display}"; exit 3
fi

home="${work}/home"
mkdir -p "${home}"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'wallpaper:' $'\tenabled: false' >"${work}/config.shcl"
printf '%s\n' 'while :; do sleep 300; done' >"${work}/scene.sh"

DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SESSION_MANAGER -u TMUX -u TMUX_PANE -u SILKTERM_SOCKET \
	LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_CONFIG_HOME="${home}/.config" XDG_DATA_HOME="${home}/.local/share" \
	XDG_CACHE_HOME="${home}/.cache" XDG_RUNTIME_DIR="${home}" \
	"${bin}" --config "${work}/config.shcl" --pixel-width 900 --pixel-height 600 --shell "/bin/sh ${work}/scene.sh" >/dev/null 2>"${work}/said.txt" &
appPid=$!

fOurs(){ fX xdotool search --onlyvisible --pid "${appPid}" 2>/dev/null | sort -n || true; }
main=""
for _ in {1..150}; do main="$(fOurs | head -n 1)"; [[ -n "${main}" ]] && break; sleep 0.2; done
if [[ -z "${main}" ]]; then echo "  FAIL a window came up"; tail -n 20 "${work}/said.txt" | sed 's/^/    /'; exit 1; fi
echo "  ok   a window came up"
sleep 2
fX xdotool windowactivate --sync "${main}" 2>/dev/null || true
fX xdotool key --clearmodifiers ctrl+comma
dlg=""
for _ in {1..150}; do dlg="$(fOurs | grep -vx "${main}" | head -n 1 || true)"; [[ -n "${dlg}" ]] && break; sleep 0.2; done
fCheck "Settings opened" test -n "${dlg}"
if [[ -z "${dlg}" ]]; then tail -n 20 "${work}/said.txt" | sed 's/^/    /'; exit 1; fi
## Past the window manager's own settle after the map.
sleep 1.5

mainHex="$(printf '0x%x' "${main}")"; dlgHex="$(printf '0x%x' "${dlg}")"
fProp(){ fX xprop -id "${1}" "${2}" 2>/dev/null || true; }
fTransient(){ [[ "$(fProp "${dlg}" WM_TRANSIENT_FOR)" == *"# ${mainHex}" ]]; }
fCheck "transient for the terminal window" fTransient
fCheck "a dialog" grep -qF '_NET_WM_WINDOW_TYPE_DIALOG' <<<"$(fProp "${dlg}" _NET_WM_WINDOW_TYPE)"
fCheck "modal" grep -qF '_NET_WM_STATE_MODAL' <<<"$(fProp "${dlg}" _NET_WM_STATE)"

## Entries the two windows make, as the window manager lists and holds them.
fEntries(){
	local -r skip="${1}"
	local w n=0
	for w in $(fX xprop -root _NET_CLIENT_LIST 2>/dev/null | sed 's/.*# //; s/,/ /g'); do
		[[ "${w}" == "${mainHex}" || "${w}" == "${dlgHex}" ]] || continue
		[[ "$(fProp "${w}" _NET_WM_STATE)" == *"${skip}"* ]] || n=$((n + 1))
	done
	echo "${n}"
}
fOne(){ [[ "$(fEntries "${1}")" == 1 ]]; }
fCheck "one taskbar entry with Settings open ($(fEntries _NET_WM_STATE_SKIP_TASKBAR))" fOne _NET_WM_STATE_SKIP_TASKBAR
fCheck "one pager entry with Settings open ($(fEntries _NET_WM_STATE_SKIP_PAGER))" fOne _NET_WM_STATE_SKIP_PAGER

## The dialog sits above the terminal in the stack, and has the focus.
fAbove(){
	local order
	order="$(fX xprop -root _NET_CLIENT_LIST_STACKING 2>/dev/null | sed 's/.*# //; s/,/ /g')"
	local -i i=0 m=-1 d=-1
	local w
	for w in ${order}; do
		[[ "${w}" == "${mainHex}" ]] && m=${i}
		[[ "${w}" == "${dlgHex}" ]] && d=${i}
		i=$((i + 1))
	done
	((m >= 0 && d > m))
}
fFocused(){ [[ "$(fX xprop -root _NET_ACTIVE_WINDOW 2>/dev/null)" == *"# ${dlgHex}"* ]]; }
fCheck "above the terminal once open" fAbove

eval "$(fX xdotool getwindowgeometry --shell "${main}")"
fClickMain(){ fX xdotool mousemove "$((X + 40))" "$((Y + HEIGHT - 40))" click 1; sleep 1; }
fClickMain
fCheck "above the terminal after a click on it" fAbove
fCheck "and keeps the focus" fFocused
fX xdotool windowraise "${main}"; sleep 1
fCheck "above the terminal after it is raised" fAbove

if command -v xterm >/dev/null 2>&1; then
	fX --bg xterm -geometry 60x20+500+200 -T dlgtie-other -e /bin/sh "${work}/scene.sh" >/dev/null 2>&1 &
	otherPid=$!
	other=""
	for _ in {1..50}; do other="$(fX xdotool search --onlyvisible --name "^dlgtie-other$" 2>/dev/null | head -n 1 || true)"; [[ -n "${other}" ]] && break; sleep 0.2; done
	if [[ -n "${other}" ]]; then
		fX xdotool windowactivate --sync "${other}" 2>/dev/null || true; sleep 1
		fClickMain
		fCheck "above the terminal after another window came in front" fAbove
		fCheck "and has the focus back" fFocused
	else
		echo "  skip another window in front: xterm did not come up"
	fi
else
	echo "  skip another window in front: no xterm"
fi

if ((failures)); then
	echo "  dialog: $(fProp "${dlg}" _NET_WM_STATE)"
	echo "  stack:  $(fX xprop -root _NET_CLIENT_LIST_STACKING 2>/dev/null)"
	echo "  said:"; tail -n 20 "${work}/said.txt" 2>/dev/null | sed 's/^/      /' || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261101 JC: Created.
##		- 20261103 JC: The other window's launch execs, so its pid is xterm.
