#!/usr/bin/env bash

##	Purpose:
##		- How many jobs a pipeline step gets, and the low priority every run takes,
##		  per Machine use in the CICD rules. Source this.
##		- fJobs [hard]: before each step. Half the CPUs, or a quarter when the box
##		  is busy (1-minute load over half the CPUs) or the step is hard on the
##		  machine (sanitizers, leak checks, CodeQL, fuzzing). Never under 1. Sets
##		  and exports CICD_MAX_JOBS, CARGO_BUILD_JOBS and RUST_TEST_THREADS.
##		- A CICD_MAX_JOBS already set, by config.bash or by the run that started
##		  this one, is the ceiling. CICD_CORES does the same for the CPU count, so a
##		  run in a container with --cpus still counts the box's CPUs.
##		- fJobs_LowPriority: nice 19, and ionice idle where there is one, on this
##		  shell, so everything it starts after inherits them.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

__jobsCeiling=""; __jobsLast=""

fJobs_Cores(){
	if [[ "${CICD_CORES:-}" =~ ^[1-9][0-9]*$ ]]; then echo "${CICD_CORES}"; return 0; fi
	nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2
}

## The 1-minute load average, as printed: "3.27". macOS and BSD have no /proc.
fJobs_Load(){
	local line=""
	if [[ -r /proc/loadavg ]]; then read -r line _ </proc/loadavg
	else line="$(sysctl -n vm.loadavg 2>/dev/null || true)"; line="${line#\{ }"; line="${line%% *}"; fi
	[[ "${line}" =~ ^[0-9]+(\.[0-9]+)?$ ]] || line=0
	echo "${line}"
}

fJobs(){
	local hard="${1:-}" cores load whole frac why=""
	local -i jobs quarter load100
	cores="$(fJobs_Cores)"; load="$(fJobs_Load)"
	if [[ -z "${__jobsCeiling}" ]]; then
		if [[ "${CICD_MAX_JOBS:-}" =~ ^[1-9][0-9]*$ ]]; then __jobsCeiling="${CICD_MAX_JOBS}"; else __jobsCeiling=$(( cores / 2 )); fi
		((__jobsCeiling >= 1)) || __jobsCeiling=1
		export CICD_CORES="${cores}"
	fi
	whole="${load%%.*}"; frac="${load#"${whole}"}"; frac="${frac#.}00"
	load100=$(( 10#${whole} * 100 + 10#${frac:0:2} ))
	jobs="${__jobsCeiling}"; quarter=$(( cores / 4 ))
	if [[ -n "${hard}" ]]; then why="hard step"
	elif (( load100 * 2 > cores * 100 )); then why="busy, load ${load}"; fi
	if [[ -n "${why}" ]] && ((jobs > quarter)); then jobs="${quarter}"; fi
	((jobs >= 1)) || jobs=1
	export CICD_MAX_JOBS="${jobs}" CARGO_BUILD_JOBS="${jobs}" RUST_TEST_THREADS="${jobs}"
	if [[ "${__jobsLast}" != "${jobs}" ]]; then
		echo "jobs: ${jobs} of ${cores} CPUs${why:+ (${why})}"
		__jobsLast="${jobs}"
	fi
}

fJobs_LowPriority(){
	renice -n 19 -p "${$}" >/dev/null 2>&1 || true
	if command -v ionice >/dev/null 2>&1; then ionice -c 3 -p "${$}" >/dev/null 2>&1 || true; fi
}

##	History:
##		- 20261026 JC: Created, so the pipeline and its helper scripts count jobs one way.
