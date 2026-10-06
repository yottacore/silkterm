#!/usr/bin/env bash

##	- Purpose:
##		cicd.bash gates on ps-lint.ps1, so the lint has to fail on a finding, pass
##		a clean script, and leave alone what the settings file excludes. The same
##		for its tab check, which lets spaces line up after the tabs. Run on
##		scripts written here, never tracked, so the pipeline's own lint of the
##		repository never sees them.
##	- Test ID: Er2UgYC
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
lint="$(cd "${meDir}/../.." && pwd)/utility/ps-lint.ps1"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

if ! command -v pwsh >/dev/null 2>&1; then
	echo "  skip PowerShell lint (no pwsh)"
	exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-pslint.XXXXXX")"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

## An alias, which the settings keep at warning level.
printf 'gci\n' >"${work}/finding.ps1"
## Clean.
printf 'function Get-Thing { param([string]$Name) $Name }\nGet-Thing -Name x\n' >"${work}/clean.ps1"
## Write-Host, which the settings exclude for console scripts.
printf "Write-Host 'hello'\n" >"${work}/excluded.ps1"
## An unapproved verb, excluded since functions here are fCamelCase.
printf 'function Frob-Thing { param([string]$Name) $Name }\nFrob-Thing -Name x\n' >"${work}/verb.ps1"
## A block indented with spaces.
printf 'if ($true) {\n    Get-Date\n}\n' >"${work}/spaces.ps1"
## Tabs, then spaces to line up a continuation, and a here-string whose text starts with spaces.
printf 'if ($true) {\n\t$x = (1 -eq 1) -and\n\t     (2 -eq 2)\n\t$x\n}\n$t = @"\n    text\n"@\n$t\n' >"${work}/aligned.ps1"
## Three arguments by position, to a cmdlet and to a function of our own.
printf "Join-Path 'a' 'b' 'c'\nfunction fThree { param([string]\$One, [string]\$Two, [string]\$Three) \$One + \$Two + \$Three }\nfThree 'a' 'b' 'c'\n" >"${work}/positional.ps1"
## Two by position is fine, and a program's arguments are its own.
printf "Join-Path 'a' 'b'\nfunction fThree { param([string]\$One, [string]\$Two, [string]\$Three) \$One + \$Two + \$Three }\nfThree 'a' 'b' -Three 'c'\ngit -C 'a' log -n 1\n" >"${work}/twopos.ps1"
## A non-ASCII comment, and a BOM.
printf '# caf\xc3\xa9\nGet-Date\n' >"${work}/nonascii.ps1"
printf '\xef\xbb\xbfGet-Date\n' >"${work}/bom.ps1"
## Common parameters by their aliases.
printf "Get-Item -Path 'a' -EA SilentlyContinue\nGet-Item -Path 'b' -ea:Stop\n" >"${work}/alias.ps1"

fLint(){ rc=0; out="$(pwsh -NoProfile -NonInteractive -File "${lint}" "${@}" 2>&1)" || rc=$?; }

fLint "${work}/clean.ps1"
if ((rc == 2)); then
	echo "  skip PowerShell lint (no PSScriptAnalyzer)"
	exit 0
fi
fCheck "a clean script passes" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/finding.ps1"
fCheck "a finding fails" test "${rc}" -eq 1
fCheck "and is named with its line and rule" grep -qF "finding.ps1:1: PSAvoidUsingCmdletAliases:" <<<"${out}"
fLint "${work}/clean.ps1" "${work}/finding.ps1"
fCheck "one bad script among several still fails" test "${rc}" -eq 1
fLint "${work}/excluded.ps1"
fCheck "a rule the settings exclude is not reported" test "${rc}" -eq 0
fLint "${work}/verb.ps1"
fCheck "an unapproved verb is not reported" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/spaces.ps1"
fCheck "a block indented with spaces fails" test "${rc}" -eq 1
fCheck "and is named with its line" grep -qF "spaces.ps1:2: Indentation:" <<<"${out}"
fLint "${work}/aligned.ps1"
fCheck "spaces after tabs, and here-string text, pass" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/positional.ps1"
fCheck "three positional arguments fail" test "${rc}" -eq 1
fCheck "to a cmdlet" grep -qF "positional.ps1:1: Positional:" <<<"${out}"
fCheck "and to a function of our own" grep -qF "positional.ps1:3: Positional:" <<<"${out}"
fLint "${work}/twopos.ps1"
fCheck "two positional, or a program's arguments, pass" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/alias.ps1"
fCheck "a parameter alias fails" test "${rc}" -eq 1
fCheck "spaced or with a colon" test "$(grep -cF ": ParameterAlias:" <<<"${out}")" -eq 2
fLint "${work}/nonascii.ps1"
fCheck "a non-ASCII byte fails" test "${rc}" -eq 1
fCheck "and is named with its line" grep -qF "nonascii.ps1:1: NonAscii:" <<<"${out}"
fLint "${work}/bom.ps1"
fCheck "a byte-order mark fails" grep -qF "bom.ps1:1: NonAscii:" <<<"${out}"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
##		- 20261004 JC: Tab indentation cases; the finding no longer an unapproved verb.
##		- 20261006 JC: Positional argument and parameter alias cases.
##		- 20261006 JC: Non-ASCII and BOM cases.
