#!/usr/bin/env bash
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.' Those parts are for the inner bash.
#  shellcheck disable=2034  ## 'variable appears unused.' Both are set for the caller, one through a nameref.

##	- Purpose:
##		Counts the processes a piece of bash starts, for tests that hold a loop to
##		no fork per item. Sourced; it defines fForkCount and runs nothing.
##			fForkCount <var> <setup> <measured>
##		Both scripts run in a fresh bash, setup first and not counted. var is set to
##		the count, and forkCountExact to 1 when it came from a PID namespace of its
##		own, where every PID is ours. Where one cannot be made it is 0, and the count
##		is the kernel's total, best of five, which the rest of the box adds to; give
##		that case a looser limit. The measured script must be safe to run five times.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

fForkCount(){
	local -n forkCount_r8u="${1}"
	local -r setup="${2}" measured="${3}"
	local got="" best=999999 _
	got="$(unshare -rpf --mount-proc bash -c "${setup}"'
		read -r forkA </proc/sys/kernel/ns_last_pid
		{ '"${measured}"'
		} >/dev/null 2>&1 || true
		read -r forkB </proc/sys/kernel/ns_last_pid
		echo "$((forkB - forkA))"' 2>/dev/null || true)"
	if [[ "${got}" =~ ^[0-9]+$ ]]; then forkCountExact=1; forkCount_r8u="${got}"; return 0; fi
	forkCountExact=0
	for _ in 1 2 3 4 5; do
		got="$(bash -c "${setup}"'
			fTotal(){ local k v; while read -r k v; do if [[ "${k}" == processes ]]; then printf -v "${1}" "%s" "${v}"; return 0; fi; done </proc/stat; }
			fTotal forkA
			{ '"${measured}"'
			} >/dev/null 2>&1 || true
			fTotal forkB
			echo "$((forkB - forkA))"' 2>/dev/null || true)"
		if [[ "${got}" =~ ^[0-9]+$ ]] && ((got < best)); then best="${got}"; fi
	done
	forkCount_r8u="${best}"
}

##	History:
##		- 20261004 JC: Created.
