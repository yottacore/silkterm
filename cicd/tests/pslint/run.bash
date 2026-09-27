#!/usr/bin/env bash

##	- Purpose:
##		cicd.bash gates on ps-lint.ps1, so the lint has to fail on a finding, pass
##		a clean script, and leave alone what the settings file excludes. Run on
##		scripts written here, never tracked, so the pipeline's own lint of the
##		repository never sees them.
##	- Test ID: Er2UgYC
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
lint="$(cd "${meDir}/../.." && pwd)/utility/ps-lint.ps1"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

if ! command -v pwsh >/dev/null 2>&1; then
	echo "  skip PowerShell lint (no pwsh)"
	exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-pslint.XXXXXX")"
trap 'rm -rf "${work}"' EXIT

## A function with an unapproved verb, which the settings keep at warning level.
printf 'function Frob-Thing { param([string]$Name) $Name }\nFrob-Thing -Name x\n' >"${work}/finding.ps1"
## Clean.
printf 'function Get-Thing { param([string]$Name) $Name }\nGet-Thing -Name x\n' >"${work}/clean.ps1"
## Write-Host, which the settings exclude for console scripts.
printf "Write-Host 'hello'\n" >"${work}/excluded.ps1"

fLint(){ rc=0; out="$(pwsh -NoProfile -NonInteractive -File "${lint}" "$@" 2>&1)" || rc=$?; }

fLint "${work}/clean.ps1"
if ((rc == 2)); then
	echo "  skip PowerShell lint (no PSScriptAnalyzer)"
	exit 0
fi
fCheck "a clean script passes" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/finding.ps1"
fCheck "a finding fails" test "${rc}" -eq 1
fCheck "and is named with its line and rule" grep -qF "finding.ps1:1: PSUseApprovedVerbs:" <<<"${out}"
fLint "${work}/clean.ps1" "${work}/finding.ps1"
fCheck "one bad script among several still fails" test "${rc}" -eq 1
fLint "${work}/excluded.ps1"
fCheck "a rule the settings exclude is not reported" test "${rc}" -eq 0

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
