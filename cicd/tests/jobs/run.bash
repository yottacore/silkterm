#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' They expand in the bash -c they go to.
#  shellcheck disable=2031  ## 'Modified in a subshell.' Each case runs in its own on purpose.
#  shellcheck disable=2329  ## 'Never invoked.' The stand-ins are called by the library they replace.

##	- Purpose:
##		How many jobs a step gets, worked out in one place (utility/include/jobs.bash):
##		half the CPUs, a quarter when the box is busy or the step is hard on it,
##		never under 1, under any cap handed down. The run goes at low priority,
##		and no helper script counts the CPUs on its own any more.
##	- Test ID: EtrCPPO
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
jobsLib="${root}/cicd/utility/include/jobs.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fNot(){ ! "${@}"; }
trap 'fTestDir_End $?' EXIT

## fJobsAt <cores> <load> <cap or ""> [hard]: the count a step gets.
fJobsAt(){
	(
		# shellcheck source=cicd/utility/include/jobs.bash
		source "${jobsLib}"
		fJobs_Load(){ echo "${fakeLoad}"; }
		export CICD_CORES="${1}"; fakeLoad="${2}"
		if [[ -n "${3}" ]]; then export CICD_MAX_JOBS="${3}"; else unset CICD_MAX_JOBS; fi
		fJobs ${4:+"${4}"} >/dev/null
		echo "${CICD_MAX_JOBS} ${CARGO_BUILD_JOBS} ${RUST_TEST_THREADS}"
	)
}
fIs(){ [[ "$(fJobsAt "${@:2}")" == "${1} ${1} ${1}" ]]; }

fCheck "an idle box gets half its CPUs"             fIs 8 16 1.00 ""
fCheck "a load of exactly half is not busy"         fIs 8 16 8.00 ""
fCheck "a busy box gets a quarter"                  fIs 4 16 8.01 ""
fCheck "a hard step gets a quarter, busy or not"    fIs 4 16 0.10 "" hard
fCheck "never under 1"                              fIs 1 2 5.5 "" hard
fCheck "one CPU still gets a job"                   fIs 1 1 0 ""
fCheck "a cap handed down is kept"                  fIs 3 16 0.5 3
fCheck "and a hard step stays under it"             fIs 2 16 0.5 2 hard
fCheck "a cap over a quarter drops to it when busy" fIs 4 16 12 6

## The ceiling is read once, so a hard step doesn't leave the next one at a quarter.
fAfterHard(){
	(
		# shellcheck source=cicd/utility/include/jobs.bash
		source "${jobsLib}"
		fJobs_Load(){ echo 0.5; }
		export CICD_CORES=16; unset CICD_MAX_JOBS
		fJobs >/dev/null; fJobs hard >/dev/null; fJobs >/dev/null
		echo "${CICD_MAX_JOBS}"
	)
}
fCheck "the step after a hard one is back at half" test "$(fAfterHard)" = 8
fCheck "it says when the count changes" bash -c 'source "$1"; fJobs_Load(){ echo 0; }; CICD_CORES=8; unset CICD_MAX_JOBS; out="$(fJobs; fJobs; fJobs hard)"; [[ "${out}" == $'"'"'jobs: 4 of 8 CPUs\njobs: 2 of 8 CPUs (hard step)'"'"' ]]' _ "${jobsLib}"
fCheck "the real load reads as a number" bash -c 'source "$1"; [[ "$(fJobs_Load)" =~ ^[0-9]+(\.[0-9]+)?$ ]]' _ "${jobsLib}"

fCheck "the run goes at nice 19" bash -c 'source "$1"; fJobs_LowPriority; [[ "$(ps -o ni= -p "$$" | tr -d " ")" == 19 ]]' _ "${jobsLib}"
if command -v ionice >/dev/null 2>&1; then
	fCheck "and idle I/O" bash -c 'source "$1"; fJobs_LowPriority; [[ "$(ionice -p "$$")" == idle ]]' _ "${jobsLib}"
fi

## One place: the helper scripts don't count CPUs themselves, and the hard ones ask for a quarter.
fCheck "no pipeline script counts the CPUs on its own" fNot grep -lE '\bnproc\b|ProcessorCount' \
	"${root}"/cicd/cicd.bash "${root}"/cicd/utility/{asan,tsan,codeql,repro-check}.bash
for script in asan tsan codeql; do
	fCheck "${script}.bash runs as a hard step" grep -qE '^fJobs_LowPriority; fJobs hard$' "${root}/cicd/utility/${script}.bash"
done
fCheck "repro-check.bash goes by the same count" grep -qE '^fJobs_LowPriority; fJobs$' "${root}/cicd/utility/repro-check.bash"
fCheck "the pipeline lowers its own priority first" grep -qx 'fJobs_LowPriority' "${root}/cicd/cicd.bash"
fCheck "a container gets the step's jobs and a low CPU share" grep -qF -- '--cpus "${CICD_MAX_JOBS}" --cpu-shares 256' "${root}/cicd/cicd.bash"
fCheck "cicd-win.ps1 sets BelowNormal at the top" grep -qF "(Get-Process -Id \$PID).PriorityClass = 'BelowNormal'" "${root}/cicd/cicd-win.ps1"
fCheck "and Idle for a hard step" grep -qF "if (\$Hard) { 'Idle' } else { 'BelowNormal' }" "${root}/cicd/cicd-win.ps1"

if ((failures)); then echo "${failures} check(s) failed"; exit 1; fi
echo "PASS EtrCPPO step jobs and priority"

##	History:
##		- 20261026 JC: Created.
