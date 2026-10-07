#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display with the built-in
##		wallpaper, three times over the same home folder, and reads what it
##		says through SILK_MEMDBG. The prepared picture is kept on disk and the
##		next launch reads it back.
##			- The first launch prepares it and keeps a copy in the platform's
##			  cache folder, not beside the settings.
##			- The second uses the copy and prepares nothing.
##			- A copy cut short is not used, and is written again.
##			- With --config, copies go beside that config instead.
##			- A light blur leaves the picture held by the window, so the GPU
##			  gets it as BC1 and the copy is those blocks.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display or
##	  xdotool).
##	- Test ID: EryJg1H
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

work="$(mktemp -d "${SILKTERM_TEST_DIR}/wpkept.XXXXXX")"
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

## The settings sit where a launch with no --config looks, so the cache goes
## where the platform keeps one. XDG_CONFIG_HOME and XDG_CACHE_HOME are left
## out, since either one moves it.
home="${work}/home"
native="${home}/.config/silkterm"
kept="${home}/.cache/silkterm/wallpaper"
mkdir -p "${native}" "${work}/alt"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' $'\tidle_release: false' \
	'wallpaper:' $'\tenabled: true' $'\tfallback_builtin: true' $'\tblur: 10.0' $'\trotate:' $'\t\tenabled: false' >"${native}/config.shcl"
cp "${native}/config.shcl" "${work}/alt/config.shcl"

said=""
fLaunch(){  ## fLaunch <log name> [args...]
	said="${work}/${1}.txt"; shift
	DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME \
		LIBGL_ALWAYS_SOFTWARE=1 SILK_MEMDBG=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" \
		"${bin}" "${@}" --pixel-width 800 --pixel-height 500 --shell "/bin/dash -c 'sleep 120'" >/dev/null 2>"${said}" &
	appPid=$!
}
fStop(){ fStopOurs "${appPid}"; appPid=""; }

## The blur is unoptimized in a debug build, so give a prepare a minute.
fSays(){  ## fSays <fixed text> - waits for a line holding it
	local -ri giveUp=$((SECONDS + 60))
	while ((SECONDS < giveUp)); do
		if grep -qF -- "${1}" "${said}"; then return 0; fi
		sleep 0.25
	done
	echo "    never said: ${1}"
	return 1
}
fNeverSays(){ ! grep -qF -- "${1}" "${said}"; }
fCopies(){ find "${1}" -maxdepth 1 -name '*.wpc' 2>/dev/null | wc -l; }
fCopiesAre(){ [[ "$(fCopies "${1}")" == "${2}" ]]; }
fHeld(){ grep '^memdbg wallpaper: ' "${said}" | tail -n 1 | awk '{print $3}'; }
fHolds(){
	local -ri giveUp=$((SECONDS + 30))
	while ((SECONDS < giveUp)); do
		if [[ "$(fHeld)" == "${1}" ]]; then return 0; fi
		sleep 0.25
	done
	echo "    held $(fHeld), wanted ${1}"
	return 1
}

fLaunch first
fCheck "first launch: prepared and kept" fSays "memdbg wallpaper copy: stored 770x399"
fStop
fCheck "kept in the platform's cache folder" fCopiesAre "${kept}" 1
fCheck "nothing kept beside the settings" fCopiesAre "${native}/cache/wallpaper" 0

fLaunch second
fCheck "second launch: the copy is used" fSays "memdbg wallpaper copy: used 770x399 for 770x399"
fCheck "and drawn" fHolds 768x397
fCheck "and nothing prepared" fNeverSays "copy: stored"
fStop

copy="$(find "${kept}" -maxdepth 1 -name '*.wpc' | head -n 1)"
size="$(stat -c %s "${copy}")"
truncate -s $((size / 2)) "${copy}"
fLaunch third
fCheck "a copy cut short is prepared again" fSays "memdbg wallpaper copy: stored 770x399"
fCheck "and not used" fNeverSays "copy: used"
fStop
fCheck "and written whole" test "$(stat -c %s "${copy}" 2>/dev/null || echo 0)" == "${size}"

fLaunch alt --config "${work}/alt/config.shcl"
fCheck "--config: prepared and kept" fSays "memdbg wallpaper copy: stored 770x399"
fStop
fCheck "beside that config" fCopiesAre "${work}/alt/cache/wallpaper" 1
fCheck "and not in the platform's folder" fCopiesAre "${kept}" 1

## 1920x993 drawn in 800x500 is held at 967x500, and a sigma of 1 is too light
## to hold it any smaller.
sed -i 's/^\tblur: 10\.0$/\tblur: 1.0/' "${native}/config.shcl"
fCheck "the blur is light now" grep -q $'^\tblur: 1.0$' "${native}/config.shcl"
fLaunch light
fCheck "light blur: kept as BC1" fSays "memdbg wallpaper copy: stored 967x500 BC1"
fCheck "and drawn as BC1" fSays "wallpaper: 967x500 held of 1920x993, 0.2 MiB as BC1"
fStop
fCheck "a copy of its own" fCopiesAre "${kept}" 2
fLaunch lightAgain
fCheck "light blur again: the blocks are used" fSays "memdbg wallpaper copy: used 967x500 for 967x500"
fCheck "and drawn as BC1" fSays "wallpaper: 967x500 held of 1920x993, 0.2 MiB as BC1"
fCheck "and nothing prepared" fNeverSays "copy: stored"
fStop

if ((failures)); then
	for log in "${work}"/*.txt; do
		echo "    ${log##*/}:"
		grep '^memdbg wallpaper' "${log}" | sed 's/^/      /' | tail -n 10 || true
	done
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261006 JC: Created.
##		- 20261006 JC: The blur holds the picture at 768x397 plus a border, so
##		  the the copy is 770x399.
##		- 20261007 JC: A light blur case, kept as BC1.
