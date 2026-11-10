#!/usr/bin/env bash

##	- Purpose:
##		The scroll harness's exit code, on its own so it can be tested without a
##		display. Sourced by run.bash; sourcing defines the function and nothing else.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

## fScrollVerdict <pass> <fail> <miss> <strict>
## Echoes one line saying why, and returns 0 for a good run.
##
## Nothing passing is a failure whether or not --strict was asked for. The caller
## has already answered the environment questions by the time it gets here, so
## zero measured scenes means the run tested nothing - which once read as OK for
## a whole release.
fScrollVerdict(){
	local -ri pass="${1}" fail="${2}" miss="${3}" strict="${4}"
	if ((fail)); then
		echo "FAILED: ${fail} scroll regression(s) measured"; return 1
	fi
	if ((miss)) && ((strict)); then
		echo "FAILED: ${miss} scenario(s) skipped under --strict"; return 1
	fi
	if ((pass == 0)); then
		echo "FAILED: no scenario ran (${miss} skipped) - the harness measured nothing"; return 1
	fi
	echo "OK: no scroll regressions"; return 0
}

## fTraceFrames <trace>
## Echoes how many frames a trace has, 0 for none or no file. `grep -c`
## prints 0 and exits 1 on no match, so `|| echo 0` after it printed 0 twice.
fTraceFrames(){
	local n; n="$(grep -c SCROLLDBG "${1}" 2>/dev/null || true)"
	echo "${n:-0}"
}

##	History:
##		- 20260908 JC: Split out of run.bash so the exit code has a test.
##		- 20260917 JC: Trace frame count.
