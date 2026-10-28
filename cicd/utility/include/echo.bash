#!/usr/bin/env bash

#  shellcheck disable=2034  ## _letterbox is read by the scripts that source this.

##	Purpose:
##		- Output helpers for the pipeline scripts, so cicd.bash and release.bash
##		  print and fail the same way. Source this.
##		- fEcho "msg" -> "[ msg ]" status line; fEcho_Clean "msg" -> plain line, and
##		  a bare call collapses repeated blanks. fSection draws the leading-blank +
##		  rule letterbox before a major stage header; fStep_Start and fStep_End
##		  wrap a counted step in its Starting and PASS, SKIP or FAIL lines; fDie
##		  prints a fatal line to stderr, closes an open step as FAIL, and exits.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

declare -i _wasLastEchoBlank=0
fEcho_ResetBlankCounter(){ _wasLastEchoBlank=0; }
fEcho_Clean(){ if [[ -n "${1:-}" ]]; then echo -e "${*}"; _wasLastEchoBlank=0; elif ((_wasLastEchoBlank == 0)) && echo; then _wasLastEchoBlank=1; fi; }
fEcho(){       if [[ -n "${*}"   ]]; then fEcho_Clean "[ ${*} ]"; else fEcho_Clean ""; fi; }
fEcho_Force(){ fEcho_ResetBlankCounter; fEcho "${*}"; }
_letterbox="•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••"
fSection(){ fEcho_Clean; fEcho_Clean "${_letterbox}"; fEcho "${*}"; }

## A step: fStep_Start <number> <name> opens it, fStep_End <PASS|SKIP|FAIL> [note]
## closes it, and fDie closes an open one as FAIL. The caller sets _stepTotal.
declare -i _stepNum=0 _stepTotal=0
_stepName=""
fStamp(){ date +%Y%m%d-%H%M%S.%2N; }
fStep_Line(){ fEcho "$(fStamp): Step ${_stepNum} of ${_stepTotal}; ${*}"; }
fStep_Start(){
	[[ -z "${_stepName}" ]] || fStep_End PASS
	_stepNum="${1}"; _stepName="${2}"
	fEcho_Clean; fEcho_Clean "${_letterbox}"; fStep_Line "Starting ${_stepName}"
}
fStep_End(){
	[[ -n "${_stepName}" ]] || return 0
	fStep_Line "${1:-PASS} ${_stepName}${2:+ (${2})}"; _stepName=""
	fEcho_Clean
}
fDie(){ { fEcho_Force "FAILED: ${*}"; fStep_End FAIL; fEcho_Clean; } >&2; exit 1; }

##	History:
##		- 20261028: Steps with a stamped Starting and closing line, and the rule
##		            is 79 wide.
##		- 20261006 JC: Moved out of cicd.bash, so release.bash can share it.
