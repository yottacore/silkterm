#!/usr/bin/env bash

##	- Purpose:
##		The soak test. Loops the program's main actions on a private Xvfb under
##		heaptrack, and fails when the live heap after round 2N is over the heap
##		after round N by more than a margin. Each round opens a tab, types into
##		its shell, splits it, types into the new pane, closes the pane, opens
##		and closes the Settings dialog, opens a second window, types into it and
##		closes it, then closes the tab. heaptrack sees every allocation in the
##		process, the graphics driver and the other C libraries included.
##		Every shell is soak-shell.sh, from --shell adn the test config's shell
##		list, with HOME and the XDG dirs in the run's own folder.
##	- Syntax: run.bash [--bin PATH] [--rounds N] [--out DIR] [--stamp STAMP] [--plant]
##		--bin     the program (default: the debug build, a plain one)
##		--rounds  N, so 2N rounds in all (default 6, or SOAK_ROUNDS)
##		--out     keep the heaptrack data and a summary there, as
##		          soak_<stamp>.zst and soak_<stamp>.txt
##		--stamp   the <stamp> for --out (default SOAK_STAMP, else now)
##		--plant   preload plant.c, which leaks 64 KiB per child started; the
##		          test must then fail
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  window manager, xdotool, heaptrack, zstd, python3 or gcc for --plant).
##	- Test ID: der46
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
headless="${root}/cicd/utility/gui-headless.bash"

## Growth allowed from round N to 2N. Measured 20261101 on the debug build on
## llvmpipe: the heap after a round moves between two levels about 120 KiB
## apart, and round 6 to 12 grew 4 KiB. With --plant a round starts 3
## children, and round 6 to 12 grew 1152 KiB.
marginKib="${SOAK_MARGIN_KIB:-256}"
rounds="${SOAK_ROUNDS:-6}"
bin=""; out=""; stamp="${SOAK_STAMP:-}"; plant=0
while (($#)); do case "${1}" in
	--bin) bin="${2-}"; shift 2 ;;
	--rounds) rounds="${2-}"; shift 2 ;;
	--out) out="${2-}"; shift 2 ;;
	--stamp) stamp="${2-}"; shift 2 ;;
	--plant) plant=1; shift ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	- History:/p' "${BASH_SOURCE[0]}" | sed 's/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
[[ "${rounds}" =~ ^[1-9][0-9]*$ ]] || { echo "--rounds takes a whole number, not '${rounds}'" >&2; exit 2; }
if [[ -z "${bin}" ]]; then
	targetDir="${CARGO_TARGET_DIR:-target}"
	[[ "${targetDir}" == /* ]] || targetDir="${root}/${targetDir}"
	bin="${targetDir}/debug/silkterm"
fi
if [[ ! -x "${bin}" ]]; then echo "  skip: no SilkTerm binary at ${bin} (build it first, or pass --bin)"; exit 3; fi
echo "  binary: ${bin}"
for tool in Xvfb xdotool xprop heaptrack zstd python3; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if ((plant)) && ! command -v gcc >/dev/null 2>&1; then echo "  skip: no gcc for --plant"; exit 3; fi
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/soak.XXXXXX")"
## appPid is heaptrack's own script; the program is its child, in silkPid.
appPid=""; silkPid=""; startedDisplay=0
## Only the process launched here, and only while it is still this binary.
fStopOurs(){
	local want exe
	[[ -n "${silkPid}" ]] || return 0
	want="$(realpath -e "${bin}" 2>/dev/null || true)"
	exe="$(realpath -e "/proc/${silkPid}/exe" 2>/dev/null || true)"
	if [[ -n "${want}" && "${exe}" == "${want}" ]]; then kill "${silkPid}" 2>/dev/null || true; fi
	silkPid=""
}
fCleanup(){
	local -r rc="${1}"
	fStopOurs
	if [[ -n "${appPid}" ]]; then wait "${appPid}" 2>/dev/null || true; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	if [[ "${rc}" == 0 ]]; then fTestDir_Rm "${work}" || true; fi
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start --wm >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi
fX(){ DISPLAY="${display}" XAUTHORITY="${auth}" "${@}"; }
if ! fX xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -qF 'window id'; then
	echo "  skip: no window manager on ${display}"; exit 3
fi

preload=""
if ((plant)); then
	gcc -shared -fPIC -O1 -o "${work}/plant.so" "${meDir}/plant.c" -ldl
	preload="${work}/plant.so"
fi

said="${work}/said.txt"; typed="${work}/typed.txt"; marks="${work}/marks.txt"
mkdir -p "${work}/home"; : >"${typed}"; : >"${marks}"
shellCmd="/bin/sh ${meDir}/soak-shell.sh"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' $'\tidle_release: false' \
	'wallpaper:' $'\tenabled: false' $'\tfallback_builtin: false' \
	'shells:' $'\tsoak:' $'\t\ttitle: "soak"' $'\t\tcommand: "'"${shellCmd}"'"' $'\t\tactive: true' >"${work}/config.shcl"

## heaptrack's clock starts in the process, a little after this one. Marks are
## read half a second in from each end, so a lag under that only narrows each window.
startMs="$(date +%s%3N)"
DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SESSION_MANAGER LIBGL_ALWAYS_SOFTWARE=1 \
	SOAK_TYPED="${typed}" ${preload:+LD_PRELOAD="${preload}"} \
	HOME="${work}/home" XDG_CONFIG_HOME="${work}/home/.config" XDG_DATA_HOME="${work}/home/.local/share" \
	XDG_CACHE_HOME="${work}/home/.cache" XDG_RUNTIME_DIR="${work}/home" \
	heaptrack -o "${work}/heap" "${bin}" --config "${work}/config.shcl" --pixel-width 900 --pixel-height 600 --shell "${shellCmd}" \
	>"${work}/heaptrack.txt" 2>"${said}" &
appPid=$!
fWindows(){ fX xdotool search --onlyvisible --class SilkTerm 2>/dev/null | sort -n || true; }
fWaitCount(){
	local -r want="${1}"; local -i i
	for ((i = 0; i < 150; i++)); do (($(fWindows | wc -l) == want)) && return 0; sleep 0.2; done
	return 1
}
## fTyped <line>: the pid of the shell that got it, once one has.
fTyped(){
	local -r line="${1}"; local pid; local -i i
	for ((i = 0; i < 100; i++)); do
		pid="$(awk -v want="${line}" '{ p = $1; sub(/^[0-9]+ /, "") } $0 == want { print p; exit }' "${typed}")"
		[[ -z "${pid}" ]] || { echo "${pid}"; return 0; }
		sleep 0.2
	done
	return 1
}
fGone(){ local -i i; for ((i = 0; i < 50; i++)); do kill -0 "${1}" 2>/dev/null || return 0; sleep 0.2; done; return 1; }
fKey(){ fX xdotool windowactivate --sync "${1}" 2>/dev/null || true; fX xdotool key --clearmodifiers "${2}"; }
fType(){ fX xdotool windowactivate --sync "${1}" 2>/dev/null || true; fX xdotool type --delay 15 "${2}"; fX xdotool key Return; }
fMs(){ echo $(($(date +%s%3N) - startMs)); }

if ! fWaitCount 1; then echo "  FAIL a window came up"; tail -n 20 "${said}" | sed 's/^/    /'; exit 1; fi
main="$(fWindows | head -n 1)"
silkPid="$(fX xdotool getwindowpid "${main}" 2>/dev/null || true)"
echo "  ok   a window came up"
fType "${main}" "soak first"; firstPid="$(fTyped "soak first" || echo 0)"
## Past the shell scan, which is folded in once 3 s after the window shows.
sleep 6

## Each pane's shell is a new process, and gone once its pane is closed, so a
## hotkey that did nothing can't pass as one that worked.
lost=0
fLost(){ echo "    round ${1}: ${2}"; lost=$((lost + 1)); }
fRound(){
	local -r n="${1}"
	local w other tabPid splitPid winPid
	fKey "${main}" ctrl+shift+t; sleep 1
	fType "${main}" "round ${n} tab"
	tabPid="$(fTyped "round ${n} tab")" || fLost "${n}" "the tab's shell got nothing"
	[[ "${tabPid:-x}" != "${firstPid}" ]] || fLost "${n}" "no new tab; the first one got the typing"
	fKey "${main}" alt+shift+plus; sleep 1
	fType "${main}" "round ${n} split"
	splitPid="$(fTyped "round ${n} split")" || fLost "${n}" "the split's shell got nothing"
	[[ "${splitPid:-x}" != "${tabPid:-}" && "${splitPid:-x}" != "${firstPid}" ]] || fLost "${n}" "no split; an older pane got the typing"
	fKey "${main}" alt+shift+w
	[[ -z "${splitPid:-}" ]] || fGone "${splitPid}" || fLost "${n}" "the split's shell outlived its pane"
	fKey "${main}" ctrl+comma
	fWaitCount 2 || fLost "${n}" "no Settings dialog"
	sleep 1
	other="$(fWindows | grep -vxF "${main}" | head -n 1 || true)"
	[[ -z "${other}" ]] || fKey "${other}" Escape
	fWaitCount 1 || fLost "${n}" "the Settings dialog stayed up"
	fKey "${main}" ctrl+shift+n
	if fWaitCount 2; then
		w="$(fWindows | grep -vxF "${main}" | head -n 1)"
		sleep 1
		fType "${w}" "round ${n} window"
		winPid="$(fTyped "round ${n} window")" || fLost "${n}" "the new window's shell got nothing"
		[[ "${winPid:-x}" != "${tabPid:-}" && "${winPid:-x}" != "${firstPid}" ]] || fLost "${n}" "the new window's typing went to an older pane"
		fKey "${w}" ctrl+shift+w
		fWaitCount 1 || fLost "${n}" "the new window stayed up"
	else
		fLost "${n}" "no new window"
	fi
	fKey "${main}" ctrl+shift+w
	[[ -z "${tabPid:-}" ]] || fGone "${tabPid}" || fLost "${n}" "the tab's shell outlived its tab"
	sleep 1
	## A quiet window to read the heap in.
	local from; from="$(fMs)"; sleep 3
	echo "${n} $((from + 500)) $(($(fMs) - 500))" >>"${marks}"
}
total=$((rounds * 2))
for ((r = 1; r <= total; r++)); do kill -0 "${silkPid:-0}" 2>/dev/null || break; fRound "${r}"; done
fCheck "still running after ${total} rounds" kill -0 "${silkPid:-0}"
fCheck "every action reached its window or shell" test "${lost}" -eq 0

## A clean exit lets heaptrack finish its file.
fKey "${main}" ctrl+shift+w
for _ in {1..100}; do kill -0 "${appPid}" 2>/dev/null || break; sleep 0.2; done
fStopOurs; wait "${appPid}" 2>/dev/null || true; appPid=""
shopt -s nullglob; found=("${work}"/heap*.zst); shopt -u nullglob
data="${found[0]:-}"
if [[ -z "${data}" ]]; then fCheck "heaptrack wrote its data" false; echo "${failures} failed"; exit 1; fi

mapfile -t heap < <(zstd -dc "${data}" | python3 "${meDir}/heapat.py" "${marks}")
summary="${work}/summary.txt"
{
	echo "soak: ${total} rounds, margin ${marginKib} KiB$( ((plant)) && echo ', planted leak')"
	echo "live heap after each round, in KiB:"
	for line in "${heap[@]}"; do
		read -r rr bytes <<<"${line}"
		if [[ "${bytes}" == "-" ]]; then echo "  ${rr}  (no sample)"; else echo "  ${rr}  $((bytes / 1024))"; fi
	done
} >"${summary}"
sed 's/^/    /' "${summary}"
atN="$(awk -v r="${rounds}" '$1 == r { print $2 }' <<<"$(printf '%s\n' "${heap[@]}")")"
at2N="$(awk -v r="${total}" '$1 == r { print $2 }' <<<"$(printf '%s\n' "${heap[@]}")")"
if [[ "${atN}" =~ ^[0-9]+$ && "${at2N}" =~ ^[0-9]+$ ]]; then
	grew=$(( (at2N - atN) / 1024 ))
	echo "round ${rounds} to ${total}: ${grew} KiB" >>"${summary}"
	fCheck "heap read at round ${rounds} and ${total}" test "${atN}" -gt 0
	fCheck "round ${rounds} to ${total}: grew ${grew} KiB, margin ${marginKib}" test "${grew}" -le "${marginKib}"
else
	fCheck "heap read at round ${rounds} and ${total}" false
fi
if [[ -n "${out}" ]]; then
	mkdir -p "${out}"
	[[ -n "${stamp}" ]] || stamp="$(date +%Y%m%d-%H%M%S)"
	cp "${data}" "${out}/soak_${stamp}.zst"
	cp "${summary}" "${out}/soak_${stamp}.txt"
	echo "  kept ${out}/soak_${stamp}.zst and .txt"
fi

if ((failures)); then
	tail -n 20 "${said}" | sed 's/^/    /' || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261101: --stamp, for the pipeline step.
##		- 20261101 JC: Created.
