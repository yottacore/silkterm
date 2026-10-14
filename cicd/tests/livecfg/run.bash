#!/usr/bin/env bash

##	- Purpose:
##		Three windows on the private display, two on one settings file and one
##		on another. A save in the first has to show in the second at once, and
##		never in the third.
##			- View > Minimap in window A saves. A's minimap comes on, B keeps
##			  its own, C's does not change, and nobody picks a new wallpaper.
##			- A new wallpaper folder written by hand, then Reload config in A.
##			  A and B both pick from the new folder; C picks nothing.
##			- B killed without warning. The next save in A finds it gone and
##			  takes its marker away.
##			- A window D opened after a save starts with what was saved.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  xdotool, xwd or ImageMagick).
##	- Test ID: EsaF4Qe
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
for tool in Xvfb xdotool xwd convert compare; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/livecfg.XXXXXX")"
declare -A pids=() windows=()
startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	local name
	for name in "${!pids[@]}"; do fStopOurs "${pids[${name}]}"; done
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

## Two folders of pictures that look nothing alike, so a pick can be told by name.
fPictures(){  ## fPictures <dir> <prefix> <color...>
	local -r dir="${1}" prefix="${2}"; shift 2
	mkdir -p "${dir}"
	local color n=0
	for color in "${@}"; do
		n=$((n + 1))
		convert -size "$((200 + n * 37))x$((150 + n * 23))" "gradient:${color}-black" "${dir}/${prefix}${n}.png"
	done
}
fPictures "${work}/pics1" one red green blue yellow cyan magenta orange purple
fPictures "${work}/pics2" two white gray pink brown navy olive teal maroon

## A blink-free cursor and a fixed size, so only the setting under test
## moves pixels.
fConfig(){  ## fConfig <file>
	mkdir -p "$(dirname "${1}")"
	printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'cursor:' $'\tblink: false' \
		'window:' $'\tremember_per_monitor: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' \
		'wallpaper:' $'\trotate:' $'\t\tfolder: "'"${work}/pics1"'"' $'\t\trandom: true' >"${1}"
}
shared="${work}/shared/config.shcl"
other="${work}/other/config.shcl"
fConfig "${shared}"
fConfig "${other}"
mkdir -p "${work}/home" "${work}/run"

fLaunch(){  ## fLaunch <name> <config>
	DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SILKTERM_SOCKET LIBGL_ALWAYS_SOFTWARE=1 \
		HOME="${work}/home" XDG_CONFIG_HOME="${work}/home/.config" XDG_CACHE_HOME="${work}/home/.cache" XDG_DATA_HOME="${work}/home/.local/share" XDG_RUNTIME_DIR="${work}/run" \
		"${bin}" --config "${2}" --shell "/bin/sh -c 'seq 1 400; sleep 300'" >/dev/null 2>"${work}/${1}.said.txt" &
	pids[${1}]=$!
	windows[${1}]="$(fX timeout 30 xdotool search --sync --onlyvisible --pid "${pids[${1}]}" 2>/dev/null | head -n 1 || true)"
}

## The right quarter of a window, where the minimap column goes, or the left
## half, where only the wallpaper changes.
fShot(){  ## fShot <name> <out> [West]
	local -r side="${3:-East}"
	local size="25%x100%"
	if [[ "${side}" == West ]]; then size="50%x100%"; fi
	fX xwd -silent -id "${windows[${1}]}" 2>/dev/null | convert xwd:- -gravity "${side}" -crop "${size}+0+0" +repage "${2}" 2>/dev/null
}
fSame(){  ## fSame <a.png> <b.png>
	local diff
	diff="$(compare -metric AE "${1}" "${2}" null: 2>&1 || true)"
	[[ "${diff%% *}" == 0 ]]
}
fDiffers(){ ! fSame "${@}"; }
## A picture is prepared on a worker after the first frame, and the text's
## halo follows it, so a window is only read once two shots a second apart
## agree.
fSettled(){  ## fSettled <name> <out> [West]
	local -r tmp="${2%.png}.prev.png"
	local _
	fShot "${1}" "${tmp}" "${3:-East}"
	for _ in {1..40}; do
		sleep 1
		fShot "${1}" "${2}" "${3:-East}"
		if fSame "${tmp}" "${2}"; then return 0; fi
		cp "${2}" "${tmp}"
	done
	return 1
}
## The minimap's own line, under its heading in the scroll section.
fMinimap(){  ## fMinimap <file> <true|false>
	awk -v want="${2}" '/^\tminimap:/ {p=1; next} p && /^\t\tenabled:/ {found = ($2 == want)} p && /^\t[^\t]/ {p=0} END {exit !found}' "${1}"
}
fNoMinimap(){ ! fMinimap "${1}" true; }
fWaitMinimap(){  ## fWaitMinimap <file> <true|false>
	local _
	for _ in {1..50}; do
		if fMinimap "${1}" "${2}"; then return 0; fi
		sleep 0.2
	done
	return 1
}

fMarkers(){  ## fMarkers <name> -> the meeting folder holding this window's marker
	local dir
	for dir in "${work}/run"/silkterm-*/; do
		if [[ -e "${dir}${pids[${1}]}" ]]; then echo "${dir}"; return 0; fi
	done
	return 1
}

fHistory(){ cat "${1}/.wallpaper-history" 2>/dev/null || true; }


echo "three windows"
fLaunch a "${shared}"
fLaunch b "${shared}"
fLaunch c "${other}"
for name in a b c; do fCheck "${name}: a window came up" test -n "${windows[${name}]}"; done
## first frames, the launch scans, and the markers
sleep 5
dirA="$(fMarkers a || true)"; dirB="$(fMarkers b || true)"; dirC="$(fMarkers c || true)"
fCheck "a and b meet in one folder" test -n "${dirA}" -a "${dirA}" == "${dirB}"
fCheck "c meets in another" test -n "${dirC}" -a "${dirC}" != "${dirA}"

echo "a save in a"
for name in a b c; do fCheck "${name}: settled" fSettled "${name}" "${work}/${name}0.png"; done
histShared="$(fHistory "${work}/shared")"; histOther="$(fHistory "${work}/other")"
fX xdotool windowactivate --sync "${windows[a]}" 2>/dev/null || fX xdotool windowfocus --sync "${windows[a]}" 2>/dev/null || true
fX xdotool key --clearmodifiers alt+v 2>/dev/null || true
sleep 0.5
fX xdotool key --clearmodifiers n 2>/dev/null || true
fCheck "the file has the minimap on" fWaitMinimap "${shared}" true
for name in a b c; do fSettled "${name}" "${work}/${name}1.png" || true; done
## a is the control: the strip shows the minimap at all
fCheck "a shows it" fDiffers "${work}/a0.png" "${work}/a1.png"
## A view switch is for new windows now, and the others keep theirs (2026101313381123).
# fCheck "b shows it" fDiffers "${work}/b0.png" "${work}/b1.png"
fCheck "b keeps its own" fSame "${work}/b0.png" "${work}/b1.png"
fCheck "c does not" fSame "${work}/c0.png" "${work}/c1.png"
fCheck "c's file is untouched" fNoMinimap "${other}"
fCheck "nobody picked a new wallpaper" test "$(fHistory "${work}/shared")" == "${histShared}"

echo "a new folder, read again in a"
for name in b c; do fSettled "${name}" "${work}/${name}2.png" West || true; done
sed -i "s|${work}/pics1|${work}/pics2|" "${shared}"
SILKTERM_SOCKET="${work}/run/silkterm-ctl-${pids[a]}.sock" "${bin}" --reload-settings >/dev/null 2>&1 || true
sleep 1
for name in b c; do fSettled "${name}" "${work}/${name}3.png" West || true; done
## each window picks on its own, and two picks can land on one name
fCheck "the new folder was picked from" test "$(fHistory "${work}/shared" | grep -c '^two' || true)" -ge 1
fCheck "b shows a picture from it" fDiffers "${work}/b2.png" "${work}/b3.png"
fCheck "c does not" fSame "${work}/c2.png" "${work}/c3.png"
fCheck "c picked nothing" test "$(fHistory "${work}/other")" == "${histOther}"

echo "b killed"
kill -9 "${pids[b]}" 2>/dev/null || true
wait "${pids[b]}" 2>/dev/null || true
deadB="${pids[b]}"; unset 'pids[b]'
fCheck "b's marker is left behind" test -e "${dirA}${deadB}"
fX xdotool windowactivate --sync "${windows[a]}" 2>/dev/null || true
fX xdotool key --clearmodifiers alt+v 2>/dev/null || true
sleep 0.5
fX xdotool key --clearmodifiers n 2>/dev/null || true
fCheck "the file has the minimap off" fWaitMinimap "${shared}" false
sleep 1
fCheck "the next save took it away" test ! -e "${dirA}${deadB}"
fCheck "a's marker stays" test -e "${dirA}${pids[a]}"
fCheck "a still runs" kill -0 "${pids[a]}"
fCheck "c still runs" kill -0 "${pids[c]}"

echo "a window opened after a save"
fX xdotool windowactivate --sync "${windows[a]}" 2>/dev/null || true
fX xdotool key --clearmodifiers alt+v 2>/dev/null || true
sleep 0.5
fX xdotool key --clearmodifiers n 2>/dev/null || true
fCheck "the file has the minimap on" fWaitMinimap "${shared}" true
fLaunch d "${shared}"
fCheck "d: a window came up" test -n "${windows[d]}"
sleep 5
## View > Minimap flips what d shows, so the file says off only if d came up with it on
fX xdotool windowactivate --sync "${windows[d]}" 2>/dev/null || fX xdotool windowfocus --sync "${windows[d]}" 2>/dev/null || true
fX xdotool key --clearmodifiers alt+v 2>/dev/null || true
sleep 0.5
fX xdotool key --clearmodifiers n 2>/dev/null || true
fCheck "d started with the saved minimap" fWaitMinimap "${shared}" false

if ((failures)); then
	for said in "${work}"/*.said.txt; do echo "  ${said##*/}:"; tail -n 12 "${said}" | sed 's/^/    /'; done
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261013 JC: Created.
##		- 20261013 JC: B keeps its own minimap; D opens with the saved one.
