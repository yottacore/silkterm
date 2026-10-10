#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display under xfwm4 and checks
##		that the window first shows at the size it keeps. A jump after the
##		window shows is what this catches.
##			- A remembered size, with a size kept for this monitor too.
##			- A window last closed maximized, with Remember maximized on. It must
##			  show maximized, not at its restored size and then maximized.
##			- --fullscreen, which must show fullscreen the same way.
##		Each window is watched for a while after it shows, past the wait for a
##		move to another monitor to settle. A launch nobody resized must save no
##		size either: a stale resize event once saved the default window's grid.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  window manager or python-xlib).
##	- Test ID: Erkahb9
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
for tool in Xvfb xfwm4 xdpyinfo xprop python3; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if ! python3 -c 'import Xlib' 2>/dev/null; then echo "  skip: no python-xlib"; exit 3; fi
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/startsize.XXXXXX")"
appPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	rm -rf "${work}"
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

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start --wm >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
## With no window manager nothing maximizes and nothing is placed, so a pass
## would test nothing.
wmCheck="$(DISPLAY="${display}" XAUTHORITY="${auth}" xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)"
if [[ "${wmCheck}" != *"window id"* ]]; then echo "  skip: no window manager on ${display}"; exit 3; fi
screenInfo="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdpyinfo)"
screen="$(awk '/dimensions:/ { print $2; exit }' <<<"${screenInfo}")"

fFixture(){  ## fFixture <case> <line>...
	local -r dir="${work}/${1}"; shift
	mkdir -p "${dir}/home"
	printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' "${@}" >"${dir}/config.shcl"
}

## Prints the watcher's lines: one per change of size, map state or maximized.
fWatch(){  ## fWatch <case> [option]...
	local -r dir="${work}/${1}"; shift
	DISPLAY="${display}" XAUTHORITY="${auth}" LIBGL_ALWAYS_SOFTWARE=1 \
		HOME="${dir}/home" XDG_CONFIG_HOME="${dir}/home/.config" XDG_DATA_HOME="${dir}/home/.local/share" XDG_RUNTIME_DIR="${dir}/home" \
		env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET "${bin}" --config "${dir}/config.shcl" --shell "/bin/dash -c 'sleep 60'" "${@}" >/dev/null 2>"${dir}/said.txt" &
	appPid=$!
	python3 "${meDir}/_watch.py" "${display}" "${appPid}" 2.5 >"${dir}/seen.txt" || true
	fStopOurs "${appPid}"; appPid=""
	sed 's/^/    /' "${dir}/seen.txt"
}

## The remembered sizes are still the ones the fixture wrote. The launch moves
## them from the config to the state file beside it.
fSavedNoSize(){  ## fSavedNoSize <case> <expected sizes>
	local -r tab=$'\t'
	local got
	got="$(grep -oE "^(${tab}remembered_(columns|rows)|${tab}${tab}${tab}(columns|rows)): [0-9]+" "${work}/${1}/state.shcl" | tr -d '\t' | paste -sd ' ' || true)"
	[[ "${got}" == "${2}" ]] || { echo "    sizes now: ${got}"; return 1; }
}

## The state the window first showed in, and every one after it, must match.
fShownOnce(){  ## fShownOnce <case>
	local -r seen="${work}/${1}/seen.txt"
	local shown
	shown="$(awk '$3 == "shown" { print $2, $4 }' "${seen}" | sort -u | wc -l)"
	grep -q ' shown ' "${seen}" && [[ "${shown}" == 1 ]] && ! awk 'f && $3 == "hidden" { bad = 1 } $3 == "shown" { f = 1 } END { exit !bad }' "${seen}"
}

fShowedAs(){  ## fShowedAs <case> <fullscreen|maximized|->
	[[ "$(awk '$3 == "shown" { print $4; exit }' "${work}/${1}/seen.txt")" == "${2}" ]]
}

echo "remembered size, kept for this monitor (${screen})"
fFixture kept 'window:' $'\tremembered_columns: 100' $'\tremembered_rows: 30' $'\tmonitors:' $'\t\t'"${screen}_100pct:" $'\t\t\tcolumns: 90' $'\t\t\trows: 28' $'\t\t\tfont_zoom: 0'
fWatch kept
fCheck "kept: shows once, at one size" fShownOnce kept
fCheck "kept: and saves no size" fSavedNoSize kept "remembered_columns: 100 remembered_rows: 30 columns: 90 rows: 28"

## Three launches: a request that misses the map can still win the race to
## the window manager now and then, and one lucky launch would pass.
echo "last closed maximized"
for try in 1 2 3; do
	fFixture "maximized${try}" 'window:' $'\tremember_maximized: true' $'\tremembered_maximized: true' $'\tremembered_columns: 100' $'\tremembered_rows: 30'
	fWatch "maximized${try}"
	fCheck "maximized ${try}: shows once, at one size" fShownOnce "maximized${try}"
	fCheck "maximized ${try}: and is maximized when it shows" fShowedAs "maximized${try}" maximized
done

echo "--fullscreen"
fFixture fullscreen 'window:' $'\tremembered_columns: 100' $'\tremembered_rows: 30'
fWatch fullscreen --fullscreen
fCheck "fullscreen: shows once, at one size" fShownOnce fullscreen
fCheck "fullscreen: and is fullscreen when it shows" fShowedAs fullscreen fullscreen

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261004 JC: Created.
##		- 20261005 JC: A launch saves no size (2026100514211602).
##		- 20261009 JC: The sizes are read from the state file.
