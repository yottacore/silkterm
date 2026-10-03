#!/usr/bin/env bash

##	- Purpose:
##		The startup gates record the newest run log and flamegraph as seen. A look
##		taken while a run was still writing them recorded the part written so far,
##		and every later look said SEEN. This runs cicd.bash's own logging block
##		and checks the gates only ever see a finished file.
##	- Test ID: EqBfc5Y
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
cicd="$(cd "${meDir}/../.." && pwd)"
lintGate="${cicd}/utility/lint-report.bash"
flameGate="${cicd}/utility/flame-report.py"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-gates.XXXXXX")"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

## A run: cicd.bash's logging block lifted out as it stands, then two warnings
## with a pause between them that the test controls.
fMakeRun(){  ## fMakeRun <dir> <exit code>
	local -r dir="${1}" rc="${2}"
	mkdir -p "${dir}"
	{
		echo 'set -Eeuo pipefail'
		echo "root='${dir}'; LINT_LOG_DIR=lint; stamp=20260101-000000; _letterbox='****'"
		echo 'gfs_rotate(){ :; }; fTestDir_End(){ :; }'
		sed -n '/^fFinishLog(){/,/^fi$/p' "${cicd}/cicd.bash"
		echo 'echo "warning: unused variable one"'
		echo "touch '${dir}/ready'"
		echo "while [[ ! -e '${dir}/go' ]]; do sleep 0.05; done"
		echo 'echo "warning: unused variable two"'
		echo "exit ${rc}"
	} >"${dir}/run.bash"
}
fWait(){ for _ in {1..100}; do [[ -e "${1}" ]] && return 0; sleep 0.05; done; return 1; }

for rc in 0 1; do
	dir="${work}/run${rc}"
	fMakeRun "${dir}" "${rc}"
	bash "${dir}/run.bash" >/dev/null 2>&1 &
	pid=$!
	fWait "${dir}/ready" || { echo "  FAIL the lifted run never started"; failures=$((failures + 1)); kill "${pid}"; continue; }
	sleep 0.2
	mid="$("${lintGate}" --check --dir "${dir}/lint" 2>&1 || true)"
	fCheck "exit ${rc}: a look during the run records nothing" bash -c '[[ "$1" != NEW* && "$1" != CLEAN* ]]' _ "${mid}"
	touch "${dir}/go"
	wait "${pid}" || true
	after="$("${lintGate}" --check --dir "${dir}/lint" 2>&1 || true)"
	fCheck "exit ${rc}: the look after it reports the whole run" grep -q '^NEW.*2 warning' <<<"${after}"
	fCheck "exit ${rc}: and no part file is left" bash -c '! compgen -G "$1/lint/*.part" >/dev/null' _ "${dir}"
done

## The flamegraph: a graph still being written sits under a name the gate skips.
fCheck "the profiler writes to the part name" grep -q 'SILK_PROFILE_OUT="${part}"' "${cicd}/cicd.bash"
fdir="${work}/flame"; mkdir -p "${fdir}"
printf '<svg total_samples="10"' >"${fdir}/flame_20260101-000000_frequent.svg.part"
out="$(python3 "${flameGate}" --check --dir "${fdir}" 2>&1 || true)"
fCheck "the flame gate does not read a part file" bash -c '[[ "$1" != NEW* ]]' _ "${out}"
fCheck "and records nothing" test ! -e "${fdir}/.flame-seen"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260917 JC: Created.
