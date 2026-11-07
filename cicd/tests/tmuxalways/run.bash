#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display with "Always run in
##		new tmux session" on, and a stand-in tmux first on the PATH that logs
##		what it is asked and runs the shell it is handed. The plain launch's
##		pane starts a session, a new tab opened by its hotkey starts another,
##		and a layout the command line asks for starts none.
##			- Only splits used to get one. A new tab and a launch ran the plain
##			  shell, unless the tab was opened with "New tab in tmux".
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  dash or xdotool).
##	- Test ID: df02n
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
for tool in Xvfb xdotool dash; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi
dash="$(command -v dash)"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/tmuxalways.XXXXXX")"
appPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
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

## Anything still running from this run's folder, by the folder in its command
## line. Builtins only, so no helper of ours turns up in the list.
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
	if ! "${headless}" start >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi

home="${work}/home"
mkdir -p "${home}" "${work}/bin"
## Closing ends the session rather than asking, so nothing waits on a dialog.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' \
	'shell:' $'\ttmux:' $'\t\talways_new_session: true' $'\t\ton_close: end' >"${work}/config.shcl"

## The stand-in tmux writes each call's first word to the log, says it has no
## session by any name, and runs the shell a new session is handed.
cat >"${work}/bin/tmux" <<EOF
#!${dash}
echo "\$1" >>"${work}/tmux.log"
case "\$1" in
	new-session) while [ \$# -gt 0 ] && [ "\$1" != -- ]; do shift; done; shift; exec "\$@" ;;
	has-session) exit 1 ;;
esac
exit 0
EOF
chmod +x "${work}/bin/tmux"
## The pane's shell counts its starts, so a pane that ran is told apart from
## one that only got a session.
cat >"${work}/scene.sh" <<'EOF'
w=$1
echo shell >>"$w/shells.log"
sleep 300
EOF

fLaunch(){  ## fLaunch <args...>: the program on the private display, logs reset
	rm -f "${work}/tmux.log" "${work}/shells.log"
	DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u TMUX -u TMUX_PANE -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET \
		LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" PATH="${work}/bin:${PATH}" \
		"${bin}" --config "${work}/config.shcl" --pixel-width 900 --pixel-height 500 "${@}" >/dev/null 2>>"${work}/said.txt" &
	appPid=$!
}
fX(){ DISPLAY="${display}" XAUTHORITY="${auth}" "${@}"; }
fCount(){ grep -cx "${2}" "${work}/${1}" 2>/dev/null || echo 0; }
fWaitFor(){  ## fWaitFor <log> <word> <count>: up to 30 s for that many lines
	local _
	for _ in {1..300}; do [[ "$(fCount "${1}" "${2}")" -ge "${3}" ]] && return 0; sleep 0.1; done
	return 1
}
fWindow(){
	local _ win=""
	for _ in {1..300}; do
		win="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdotool search --all --pid "${appPid}" --name 'SilkTerm' 2>/dev/null | head -n 1 || true)"
		[[ -n "${win}" ]] && { echo "${win}"; return 0; }
		sleep 0.1
	done
	return 1
}

## A: a plain launch starts its pane in a session, and so does a new tab.
fLaunch --shell "${dash} ${work}/scene.sh ${work}"
fCheck "a plain launch starts a tmux session" fWaitFor tmux.log new-session 1
fCheck "and the shell runs inside it" fWaitFor shells.log shell 1
win="$(fWindow || true)"
fCheck "the window came up" test -n "${win}"
if [[ -n "${win}" ]]; then
	## past the shell scan, folded in 3 s after the window shows
	sleep 6
	fX xdotool windowactivate --sync "${win}" 2>/dev/null || fX xdotool windowfocus --sync "${win}" 2>/dev/null || true
	fX xdotool key --clearmodifiers ctrl+shift+t 2>/dev/null || true
fi
fCheck "a new tab by its hotkey starts another" fWaitFor tmux.log new-session 2
fCheck "and a second shell runs" fWaitFor shells.log shell 2
fStopOurs "${appPid}"; appPid=""
cp "${work}/tmux.log" "${work}/tmux-a.log" 2>/dev/null || true

## B: a layout from the command line stays as written, with no session.
fLaunch --shell "${dash} ${work}/scene.sh ${work}" --new-tab
fCheck "a command-line layout's shells run" fWaitFor shells.log shell 2
sleep 1
fCheck "and none in a session" test "$(fCount tmux.log new-session)" -eq 0
fStopOurs "${appPid}"; appPid=""

if ((failures)); then
	echo "  tmux was asked:"; cat "${work}/tmux-a.log" "${work}/tmux.log" 2>/dev/null | sed 's/^/      /' || true
	echo "  the program said:"; tail -n 20 "${work}/said.txt" 2>/dev/null | sed 's/^/      /' || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261107 JC: Created.
