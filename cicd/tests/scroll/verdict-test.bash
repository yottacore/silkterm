#!/usr/bin/env bash

##	- Purpose:
##		The scroll harness once printed OK and exited zero after running no scenes
##		at all, and stayed that way through a release. This holds its exit code to
##		what actually happened, and the trace checks to what they are there to
##		catch. Nothing here needs a display.
##	- Test ID: dcbdt
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
source "${meDir}/verdict.bash"

failures=0
fExpect(){
	local -r what="${1}" wantRc="${2}" rc="${3}" out="${4}"
	if [[ "${rc}" == "${wantRc}" ]]; then
		echo "  ok   ${what}"
	else
		echo "  FAIL ${what}: wanted rc ${wantRc}, got ${rc} (${out})"
		failures=$((failures + 1))
	fi
}

fCase(){
	local -r what="${1}" wantRc="${2}"; shift 2
	local out rc=0
	out="$(fScrollVerdict "${@}")" || rc=$?
	fExpect "${what}" "${wantRc}" "${rc}" "${out}"
}

##                                        pass fail miss strict
fCase "four scenes passed"            0      4    0    0    0
fCase "a measured regression"         1      3    1    0    0
fCase "nothing ran at all"            1      0    0    4    0
fCase "nothing ran, nothing skipped"  1      0    0    0    0
fCase "a skip with passes, lenient"   0      3    0    1    0
fCase "a skip with passes, strict"    1      3    0    1    1

## analyze.py on made-up traces, one frame per line: sh app_off frac alt.
## Anything after the frames goes to analyze.py as it is.
fAnalyze(){
	local -r what="${1}" wantRc="${2}" mode="${3}" frames="${4}"
	shift 4
	local out rc=0
	out="$(
		n=0
		while read -r sh app frac alt; do
			printf 'SCROLLDBG f=%d pane=0 sh=%s app_off=%s slide_sh=0.0000 st=0 sb=1 frac=%s alt=%s ob=0\n' \
				"${n}" "${sh}" "${app}" "${frac}" "${alt}"
			n=$((n + 1))
		done <<<"${frames}" | python3 "${meDir}/analyze.py" --mode "${mode}" --expect-st 0 --expect-sb 1 --label test "${@}"
	)" || rc=$?
	fExpect "${what}" "${wantRc}" "${rc}" "${out}"
}

fAnalyze "a slide that eases one way" 0 slide "1 1.0 0 1
0 0.8 0 1
0 0.6 0 1
0 0.4 0 1
0 0.2 0 1
1 1.0 0 1
0 0.5 0 1"
fAnalyze "a scene that scrolled and never slid" 1 slide "1 0 0 1
0 0 0 1
0 0 0 1
1 0 0 1
0 0 0 1
1 0 0 1"
fAnalyze "a scene that never scrolled" 2 slide "0 0 0 1
0 0 0 1
0 0 0 1
0 0 0 1
0 0 0 1"
fAnalyze "an ease landed by the alt screen" 0 still "0 0 0.5 0
0 0 0.3 0
0 0 0 1
0 0 0 1"
fAnalyze "an alt screen with no ease before it" 1 still "0 0 0 1
0 0 0 1
0 0 0 1"
fAnalyze "an ease left running on the alt screen" 1 still "0 0 0.5 0
0 0 0.4 1
0 0 0 1"
fAnalyze "no alt screen at all" 2 still "0 0 0.5 0"
## altenter waits for the ease before it swaps, and says so when it gives up
noteDir="$(mktemp -d)"
echo "no ease within 10s" >"${noteDir}/gaveup.note"
fAnalyze "no ease, the scene gave up waiting" 2 still "0 0 0 1
0 0 0 1" --note "${noteDir}/gaveup.note"
fAnalyze "no ease, and no note" 1 still "0 0 0 1
0 0 0 1" --note "${noteDir}/missing.note"
fAnalyze "an ease, with a note anyway" 0 still "0 0 0.5 0
0 0 0 1" --note "${noteDir}/gaveup.note"
fAnalyze "a note does not excuse an ease left running" 1 still "0 0 0.5 0
0 0 0.4 1" --note "${noteDir}/gaveup.note"
rm -f "${noteDir}/gaveup.note"; rmdir "${noteDir}"

## frame counts, for a trace with frames, one with none and no trace at all
fCount(){
	local -r what="${1}" want="${2}" file="${3}"
	local got
	got="$(fTraceFrames "${file}")"
	if [[ "${got}" == "${want}" ]]; then
		echo "  ok   ${what}"
	else
		echo "  FAIL ${what}: wanted ${want}, got '${got//$'\n'/\\n}'"
		failures=$((failures + 1))
	fi
}
countDir="$(mktemp -d)"
printf 'SCROLLDBG f=0\nnoise\nSCROLLDBG f=1\n' >"${countDir}/two"
printf 'noise\n' >"${countDir}/none"
fCount "a trace with two frames" 2 "${countDir}/two"
fCount "a trace with no frames"  0 "${countDir}/none"
fCount "no trace file"           0 "${countDir}/missing"
rm -f "${countDir}/two" "${countDir}/none"; rmdir "${countDir}"

## run.bash where something it needs is missing: 3, so cicd says skipped, not OK
fakeBin="$(mktemp -d)"
trap 'rc=$?; rm -f "${fakeBin}/dirname" "${fakeBin}/python3"; rmdir "${fakeBin}" 2>/dev/null || true; fTestDir_End "${rc}"' EXIT
ln -s "$(command -v dirname)" "${fakeBin}/dirname"
ln -s "$(command -v python3)" "${fakeBin}/python3"
fRun(){
	local -r what="${1}" wantRc="${2}"; shift 2
	local out rc=0
	out="$(PATH="${fakeBin}" "${BASH}" "${meDir}/run.bash" "${@}" 2>&1)" || rc=$?
	fExpect "${what}" "${wantRc}" "${rc}" "$(tail -n 1 <<<"${out}")"
}
fRun "no binary"                 3 --bin /nonexistent/silkterm
fRun "no binary, strict"         1 --bin /nonexistent/silkterm --strict
fRun "no Xvfb"                   3 --bin /bin/true
fRun "no cage"                   3 --bin /bin/true --wayland

## Each scene has its own ID and its own line, also when the run can't start.
## The IDs come from the scenes table, so --list is checked against it.
fLines(){
	local -r what="${1}" want="${2}"; shift 2
	local got
	## the result lines only, and all of --list
	got="$(PATH="${fakeBin}" "${BASH}" "${meDir}/run.bash" "${@}" 2>/dev/null | grep -E '^(PASS |FAIL |SKIP |  [0-9a-z]{5}  )')" || true
	if [[ "${got}" == "${want}" ]]; then
		echo "  ok   ${what}"
	else
		echo "  FAIL ${what}: wanted"$'\n'"${want}"$'\n'"  got"$'\n'"${got}"
		failures=$((failures + 1))
	fi
}
mapfile -t sceneRows < <(sed -n '/^scenes=(/,/^)/{s/^\t"\([^"]*\)"$/\1/p}' "${meDir}/run.bash")
listWant="" skipWant="" failWant=""
for row in "${sceneRows[@]}"; do
	read -r -a f <<<"${row}"
	listWant+="$(printf '  %-5s  scroll scene %s (X11)' "${f[0]}" "${f[1]}")"$'\n'
	skipWant+="SKIP ${f[0]} scroll scene ${f[1]} (X11), no SilkTerm binary (build it first, or pass --bin)"$'\n'
	failWant+="FAIL ${f[0]} scroll scene ${f[1]} (X11), no SilkTerm binary (build it first, or pass --bin)"$'\n'
done
fExpect "the scenes table has 14 rows" 14 "${#sceneRows[@]}" "${#sceneRows[@]} rows"
ids="$(printf '%s\n' "${sceneRows[@]}" | cut -d' ' -f1 | sort | uniq -d)"
fExpect "and no two scenes share an ID" "" "${ids}" "shared: ${ids}"
fLines "--list gives each scene's ID and name"  "${listWant%$'\n'}" --list
fLines "--list --only names the one scene"      "  d9j4x  scroll scene tmux (Wayland)" --list --only tmux --wayland
fLines "no binary: a SKIP line for every scene" "${skipWant%$'\n'}" --bin /nonexistent/silkterm
fLines "and under --strict a FAIL line"         "${failWant%$'\n'}" --bin /nonexistent/silkterm --strict
fLines "no cage: the reason on the one scene asked for" \
	"SKIP d9j4x scroll scene tmux (Wayland), cage not found (no headless Wayland compositor)" --bin /bin/true --wayland --only tmux

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260908 JC: Created.
##		- 20260915 JC: Trace checks and the skips before any scene runs.
##		- 20260917 JC: Frame counts.
##		- 20261013 JC: altenter's note.
##		- 20261028: Each scene's ID and its line, and --list.
