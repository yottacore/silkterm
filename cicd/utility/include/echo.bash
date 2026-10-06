#!/usr/bin/env bash

#  shellcheck disable=2034  ## _letterbox is read by the scripts that source this.

##	Purpose:
##		- Output helpers for the pipeline scripts, so cicd.bash and release.bash
##		  print and fail the same way. Source this.
##		- fEcho "msg" -> "[ msg ]" status line; fEcho_Clean "msg" -> plain line, and
##		  a bare call collapses repeated blanks. fSection draws the leading-blank +
##		  rule letterbox before a major stage header; fDie prints a fatal line to
##		  stderr and exits.
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
_letterbox="••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••"
fSection(){ fEcho_Clean; fEcho_Clean "${_letterbox}"; fEcho "${*}"; }
fDie(){ { fEcho_Force "FAILED: ${*}"; echo; } >&2; exit 1; }

##	History:
##		- 20261006 JC: Moved out of cicd.bash, so release.bash can share it.
