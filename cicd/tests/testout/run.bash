#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted fRunTest reads listing, and shellcheck cannot see into an eval.
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' Those are the scratch scripts' own lines.

##	- Purpose:
##		A failed test script's output reaches the run log, then its FAIL line, and
##		the failure line names the file that keeps it, when it outlives the run. A passing or skipped test
##		stays quiet but for its PASS or SKIP line, which says why it skipped. In the
##		image a skip for a missing tool fails instead. Lifts
##		fRunTest and its helpers out of cicd.bash as they stand and feeds them
##		scratch scripts that pass, fail and skip.
##	- Test ID: ddp9p
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
cicd="$(cd "${meDir}/../.." && pwd)"
engine="${cicd}/cicd.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fNot(){ ! "${@}"; }

## A run in the image sets this, and the cases below set it themselves.
unset SILK_CICD_IN_CONTAINER

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-testout.XXXXXX")"
fEnd(){ local -r rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"; }
trap fEnd EXIT

## The fake repo root fRunTest reads scripts from, and the folder it writes
## output to, kept apart from this test's own run folder.
root="${work}/repo"
mkdir -p "${root}/t" "${work}/run"
fScript(){  ## fScript <name> <body...>
	local -r f="${root}/t/${1}"; shift
	printf '%s\n' '#!/usr/bin/env bash' '##	- Test ID: zzzzz' "${@}" >"${f}"
	chmod +x "${f}"
}
fScript pass.bash  'echo "pass-chatter"'
fScript fail.bash  'echo "fail-clue-one"' 'echo "fail-clue-two"' 'exit 1'
fScript skip.bash  'echo "skip-chatter"' 'exit 3'
fScript skipwhy.bash  'echo "  skip: no frobnicator"' 'echo "skip-chatter"' 'echo "  skip: no widget either"' 'exit 3'
fScript skiptool.bash  'echo "  skip: no wtype"' 'exit 3'
fScript skipother.bash  'echo "  skip: no software Vulkan (/x/lvp.json)"' 'exit 3'
fScript long.bash  'for ((i = 1; i <= 250; i++)); do echo "long-line-${i}"; done' 'exit 1'

## fRun <log> <function> <script>: one call in a subshell, as cicd would make
## it, with stdout and stderr both into the log. Sets rc.
fRun(){
	local -r log="${1}"; shift
	rc=0
	(
		set -Eeuo pipefail
		# shellcheck source=cicd/utility/include/echo.bash
		source "${cicd}/utility/include/echo.bash"
		eval "$(sed -n '/^fTestId(){/p; /^fTestOut(){/p; /^fTestLine(){/p' "${engine}")"
		eval "$(sed -n '/^fTestOut_Fail(){/,/^}/p; /^fRunTest(){/,/^}/p; /^fRunTest_MaySkip(){/,/^}/p' "${engine}")"
		export SILKTERM_TEST_DIR="${work}/run"
		listing=0
		"${@}"
	) >"${log}" 2>&1 || rc=$?
}
fNamed(){ grep -oE 'output kept in [^ ]+' "${1}" | sed 's/^output kept in //'; }

fRun "${work}/fail.log" fRunTest t/fail.bash "failing one"
fCheck "a failed test stops the run" test "${rc}" -ne 0
fCheck "and its output is in the log" grep -q '^fail-clue-two$' "${work}/fail.log"
fCheck "the failure line keeps its text and test ID" grep -q 'FAILED: failing one test failed (zzzzz)' "${work}/fail.log"
fCheck "after a FAIL line with its ID and name" grep -qxF 'FAIL zzzzz failing one' "${work}/fail.log"
fCheck "which comes after the output" bash -c '[[ "$(grep -nxF "FAIL zzzzz failing one" "$1" | cut -d: -f1)" -gt "$(grep -nx "fail-clue-two" "$1" | cut -d: -f1)" ]]' _ "${work}/fail.log"
kept="$(fNamed "${work}/fail.log" || true)"
fCheck "and names a file" test -n "${kept}"
fCheck "which holds the output" grep -q '^fail-clue-one$' "${kept:-/nonexistent}"
fCheck "inside the test run folder" test "${kept%/*}" = "${work}/run"

##	The run's own folder goes at the end, so a failure there names no file and
##	says --keep-temp keeps it. Under --keep-temp it names the file again.
runDir="${work}/run"; keepTemp=0
fRun "${work}/gone.log" fRunTest t/fail.bash "failing one"
fCheck "a failure in the run's own folder names no file" test -z "$(fNamed "${work}/gone.log" || true)"
fCheck "and says --keep-temp keeps it" grep -qF 'FAILED: failing one test failed (zzzzz), --keep-temp keeps its output' "${work}/gone.log"
keepTemp=1
fRun "${work}/kept.log" fRunTest t/fail.bash "failing one"
fCheck "with --keep-temp it names the file" test "$(fNamed "${work}/kept.log" || true)" = "${work}/run/output_t_fail.txt"
unset runDir keepTemp

fRun "${work}/pass.log" fRunTest t/pass.bash "passing one"
fCheck "a passing test goes on" test "${rc}" -eq 0
fCheck "with its PASS line" grep -qxF 'PASS zzzzz passing one' "${work}/pass.log"
fCheck "and nothing it printed" fNot grep -q 'pass-chatter' "${work}/pass.log"
fCheck "and leaves no output file" test -z "$(find "${work}/run" -name '*pass*')"

fRun "${work}/mfail.log" fRunTest_MaySkip t/fail.bash "skippable one"
fCheck "a skippable test that fails stops the run" test "${rc}" -ne 0
fCheck "with its output in the log" grep -q '^fail-clue-one$' "${work}/mfail.log"
fCheck "and the file named" test -n "$(fNamed "${work}/mfail.log" || true)"
fCheck "and a FAIL line" grep -qxF 'FAIL zzzzz skippable one' "${work}/mfail.log"

fRun "${work}/skip.log" fRunTest_MaySkip t/skip.bash "skipped one"
fCheck "a skip goes on" test "${rc}" -eq 0
fCheck "with a SKIP line" grep -qxF 'SKIP zzzzz skipped one, could not run' "${work}/skip.log"
fCheck "and no warning" fNot grep -qi 'warning' "${work}/skip.log"
fCheck "and nothing it printed" fNot grep -q 'skip-chatter' "${work}/skip.log"
fCheck "and leaves no output file" test -z "$(find "${work}/run" -name '*skip*')"

fRun "${work}/skipwhy.log" fRunTest_MaySkip t/skipwhy.bash "skipped two"
fCheck "a skip says the last why the test gave" grep -qxF 'SKIP zzzzz skipped two, no widget either' "${work}/skipwhy.log"
fCheck "and is the only line" test "$(grep -c . "${work}/skipwhy.log")" -eq 2
fRun "${work}/skipgiven.log" fRunTest_MaySkip t/skip.bash "skipped three" "no thing to run"
fCheck "with no why from the test, the one given" grep -qxF 'SKIP zzzzz skipped three, no thing to run' "${work}/skipgiven.log"

##	In the image a skip for a missing tool is the recipe's bug, so it fails and
##	names the recipe. Any other skip there is still a skip, and so is a missing
##	tool here.
SILK_CICD_IN_CONTAINER=1 fRun "${work}/imgtool.log" fRunTest_MaySkip t/skiptool.bash "tool one"
fCheck "in the image a skip for a missing tool stops the run" test "${rc}" -ne 0
fCheck "with a FAIL line" grep -qxF 'FAIL zzzzz tool one' "${work}/imgtool.log"
fCheck "and a failure line naming the tool and the recipe" grep -qF 'FAILED: tool one: the image has no wtype; add it to cicd/container/Dockerfile (zzzzz)' "${work}/imgtool.log"
fCheck "and no SKIP line" fNot grep -q '^SKIP ' "${work}/imgtool.log"
SILK_CICD_IN_CONTAINER=1 fRun "${work}/imgother.log" fRunTest_MaySkip t/skipother.bash "other one"
fCheck "another why in the image is still a skip" test "${rc}" -eq 0
fCheck "with its SKIP line" grep -qxF 'SKIP zzzzz other one, no software Vulkan (/x/lvp.json)' "${work}/imgother.log"
SILK_CICD_IN_CONTAINER=1 fRun "${work}/imggiven.log" fRunTest_MaySkip t/skip.bash "given one" "no thing"
fCheck "and so is the why given, when the test gave none" grep -qxF 'SKIP zzzzz given one, no thing' "${work}/imggiven.log"
fRun "${work}/hosttool.log" fRunTest_MaySkip t/skiptool.bash "tool one"
fCheck "a missing tool here is a skip" test "${rc}" -eq 0
fCheck "with its SKIP line" grep -qxF 'SKIP zzzzz tool one, no wtype' "${work}/hosttool.log"

fRun "${work}/long.log" fRunTest t/long.bash "long one"
fCheck "a long output shows its end" grep -q '^long-line-250$' "${work}/long.log"
fCheck "and says how much was cut" grep -q '^last 200 of 250 lines of output:$' "${work}/long.log"
fCheck "and leaves out the start" fNot grep -q '^long-line-50$' "${work}/long.log"
fCheck "the file has all of it" grep -q '^long-line-1$' "$(fNamed "${work}/long.log" || echo /nonexistent)"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261008 JC: Created.
##		- 20261028: PASS, FAIL and SKIP lines, and the why of a skip.
##		- 20261031: The failure line names the file only when it outlives the run.
##		- 20261106: In the image a skip for a missing tool fails.
