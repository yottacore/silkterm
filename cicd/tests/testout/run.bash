#!/usr/bin/env bash
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' Those are the scratch scripts' own lines.

##	- Purpose:
##		A failed test script's output reaches the run log, and the failure line
##		names the file that keeps it. A passing or skipped test stays quiet. Lifts
##		fRunTest and its helpers out of cicd.bash as they stand and feeds them
##		scratch scripts that pass, fail and skip.
##	- Test ID: Es9knL6
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

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-testout.XXXXXX")"
fEnd(){ local -r rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## The fake repo root fRunTest reads scripts from, and the folder it writes
## output to, kept apart from this test's own run folder.
root="${work}/repo"
mkdir -p "${root}/t" "${work}/run"
fScript(){  ## fScript <name> <body...>
	local -r f="${root}/t/${1}"; shift
	printf '%s\n' '#!/usr/bin/env bash' '##	- Test ID: Zz00000' "${@}" >"${f}"
	chmod +x "${f}"
}
fScript pass.bash  'echo "pass-chatter"'
fScript fail.bash  'echo "fail-clue-one"' 'echo "fail-clue-two"' 'exit 1'
fScript skip.bash  'echo "skip-chatter"' 'exit 3'
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
		eval "$(sed -n '/^fTestId(){/p; /^fTestOut(){/p' "${engine}")"
		eval "$(sed -n '/^fTestOut_Fail(){/,/^}/p; /^fRunTest(){/,/^}/p; /^fRunTest_MaySkip(){/,/^}/p' "${engine}")"
		export SILKTERM_TEST_DIR="${work}/run"
		"${@}"
	) >"${log}" 2>&1 || rc=$?
}
fNamed(){ grep -oE 'output kept in [^ ]+' "${1}" | sed 's/^output kept in //'; }

fRun "${work}/fail.log" fRunTest t/fail.bash "failing one"
fCheck "a failed test stops the run" test "${rc}" -ne 0
fCheck "and its output is in the log" grep -q '^fail-clue-two$' "${work}/fail.log"
fCheck "the failure line keeps its text and test ID" grep -q 'FAILED: failing one test failed (Zz00000)' "${work}/fail.log"
kept="$(fNamed "${work}/fail.log" || true)"
fCheck "and names a file" test -n "${kept}"
fCheck "which holds the output" grep -q '^fail-clue-one$' "${kept:-/nonexistent}"
fCheck "inside the test run folder" test "${kept%/*}" = "${work}/run"

fRun "${work}/pass.log" fRunTest t/pass.bash "passing one"
fCheck "a passing test goes on" test "${rc}" -eq 0
fCheck "with its OK line" grep -qxF '[ OK: passing one (Zz00000) ]' "${work}/pass.log"
fCheck "and nothing it printed" fNot grep -q 'pass-chatter' "${work}/pass.log"
fCheck "and leaves no output file" test -z "$(find "${work}/run" -name '*pass*')"

fRun "${work}/mfail.log" fRunTest_MaySkip t/fail.bash "skippable one"
fCheck "a skippable test that fails stops the run" test "${rc}" -ne 0
fCheck "with its output in the log" grep -q '^fail-clue-one$' "${work}/mfail.log"
fCheck "and the file named" test -n "$(fNamed "${work}/mfail.log" || true)"

fRun "${work}/skip.log" fRunTest_MaySkip t/skip.bash "skipped one"
fCheck "a skip goes on" test "${rc}" -eq 0
fCheck "with a warning" grep -qxF '[ WARNING: skipped one skipped ]' "${work}/skip.log"
fCheck "and nothing it printed" fNot grep -q 'skip-chatter' "${work}/skip.log"
fCheck "and leaves no output file" test -z "$(find "${work}/run" -name '*skip*')"

fRun "${work}/long.log" fRunTest t/long.bash "long one"
fCheck "a long output shows its end" grep -q '^long-line-250$' "${work}/long.log"
fCheck "and says how much was cut" grep -q '^last 200 of 250 lines of output:$' "${work}/long.log"
fCheck "and leaves out the start" fNot grep -q '^long-line-50$' "${work}/long.log"
fCheck "the file has all of it" grep -q '^long-line-1$' "$(fNamed "${work}/long.log" || echo /nonexistent)"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261008 JC: Created.
