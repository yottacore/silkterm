#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' The inner bash -c reads its own $1.

##	- Purpose:
##		Launches the real program on the private display, deletes its config
##		file while it runs, then makes it save. The save has to write a new
##		file that keeps what the old one had, with the save's own change in it.
##			- A window resize, which saves the window size.
##			- Ctrl+Plus, which saves the font zoom.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display or
##	  xdotool).
##	- Test ID: ErqP9yG
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
for tool in Xvfb xdotool; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/delcfg.XXXXXX")"
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

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
fX(){ DISPLAY="${display}" XAUTHORITY="${auth}" "${@}"; }

## The profile lines are something only the old file had, so finding them in
## the new one shows it was carried over and not just the template.
fLaunch(){  ## fLaunch <case>
	local -r dir="${work}/${1}"
	mkdir -p "${dir}/home"
	printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremembered_columns: 100' $'\tremembered_rows: 30' $'\tremember_per_monitor: false' >"${dir}/config.shcl"
	DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET LIBGL_ALWAYS_SOFTWARE=1 \
		HOME="${dir}/home" XDG_CONFIG_HOME="${dir}/home/.config" XDG_DATA_HOME="${dir}/home/.local/share" XDG_RUNTIME_DIR="${dir}/home" \
		"${bin}" --config "${dir}/config.shcl" --shell "/bin/dash -c 'sleep 60'" >/dev/null 2>"${dir}/said.txt" &
	appPid=$!
	window="$(fX timeout 30 xdotool search --sync --onlyvisible --pid "${appPid}" 2>/dev/null | head -n 1 || true)"
	## The first frame and the launch's own writes come after the map.
	sleep 3
}

## Waits for the file to come back, then for the write to finish.
fWaitFile(){  ## fWaitFile <case>
	local -r file="${work}/${1}/config.shcl"
	local _
	for _ in {1..50}; do
		if [[ -s "${file}" ]]; then sleep 0.5; return 0; fi
		sleep 0.2
	done
	return 1
}

fSetting(){  ## fSetting <case> <regex>
	grep -Eq "^[[:space:]]*${2}" "${work}/${1}/config.shcl"
}

## The window size and zoom go to the state file beside the config.
fState(){  ## fState <case> <regex>
	grep -Eq "^[[:space:]]*${2}" "${work}/${1}/state.shcl"
}

fDone(){
	fStopOurs "${appPid}"; appPid=""
}

echo "window resize"
fLaunch resize
fCheck "resize: a window came up" test -n "${window}"
rm -f "${work}/resize/config.shcl"
fX xdotool windowsize "${window}" 700 400 || true
fCheck "resize: the file was written again" fWaitFile resize
fCheck "resize: with the new size" bash -c '! grep -Eq "^[[:space:]]*remembered_columns: 100$" "$1" && grep -Eq "^[[:space:]]*remembered_columns: [0-9]+" "$1"' _ "${work}/resize/state.shcl"
fCheck "resize: keeping what the old file had" fSetting resize 'profile: "custom"'
fDone

echo "font zoom"
fLaunch zoom
fCheck "zoom: a window came up" test -n "${window}"
rm -f "${work}/zoom/config.shcl"
fX xdotool windowfocus --sync "${window}" || true
fX xdotool key --clearmodifiers ctrl+equal || true
fCheck "zoom: the file was written again" fWaitFile zoom
fCheck "zoom: with the new zoom" fState zoom 'remembered_font_zoom: 1$'
fCheck "zoom: keeping what the old file had" fSetting zoom 'profile: "custom"'
fDone

if ((failures)); then
	for said in "${work}"/*/said.txt; do sed 's/^/    /' "${said}"; done
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261005 JC: Created.
##		- 20261009 JC: The size and zoom are read from the state file.
