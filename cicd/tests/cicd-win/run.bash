#!/usr/bin/env bash

##	- Purpose:
##		Pieces of cicd-win.ps1 that need no Windows box, run here: it keeps only
##		the newest run logs, which it once never pruned, and it maps the build
##		box's paths out of release builds and fails on one left behind. The map
##		is a TOML file, so a folder name holding a quote or a backslash has to
##		come through escaped. The stash it takes before a pull says whether it
##		stashed anything, and nothing else. It takes cicd.bash's runs, and hands
##		each to the Linux half. A rule it writes to a pipe is UTF-8, not code page
##		437, where the dot is the bell byte.
##	- Test ID: Er2UgYE
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

if ! command -v pwsh >/dev/null 2>&1; then
	echo "  skip cicd-win.ps1 pieces (no pwsh)"
	exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-cicdwin.XXXXXX")"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

rc=0
out="$(pwsh -NoProfile -NonInteractive -File "${meDir}/_lift.ps1" -Work "${work}" 2>&1)" || rc=$?
grep -v '^[{]' <<<"${out}" || true
fCheck "the lifted pieces all passed" test "${rc}" -eq 0

fMapReads(){ python3 - "$(grep '^[{]' <<<"${out}" | tail -1)" <<'PY'
import json, sys, tomllib
got = json.loads(sys.argv[1])
with open(got["cfg"], "rb") as f:
	flags = tomllib.load(f)["target"]["cfg(all())"]["rustflags"]
sys.exit(0 if flags == got["want"] else 1)
PY
}
fCheck "the path map reads back as the paths it was given" fMapReads

## -NoRemote reaches the Linux half, which is the one that goes to the other boxes.
ps1="${meDir}/../../cicd-win.ps1"
fCheck "-NoRemote is a parameter" bash -c 'pwsh -NoProfile -NonInteractive -Command "(Get-Command -Name \"$1\").Parameters.ContainsKey(\"NoRemote\")" | grep -qx True' _ "${ps1}"
fCheck "and goes to the Linux half as --no-remote" grep -qE '^[[:space:]]*if \(\$NoRemote\)[[:space:]]+\{ \$cicdArgs\.Add\("--no-remote"\) \}'$'\r''?$' "${ps1}"

## cicd.bash's runs, in -PascalCase. What each hands on is checked in _lift.ps1.
## The path goes in the command text, since -Command runs any argument after it.
fHasParams(){
	local out
	out="$(pwsh -NoProfile -NonInteractive -Command "\$p = (Get-Command -Name '${ps1}').Parameters
@('BacklogRunQuick', 'Gate', 'BacklogRunFull', 'MergeGate', 'Ci', 'Long', 'NoContainer', 'KeepTemp', 'List', 'Areas') | Where-Object { -not \$p.ContainsKey(\$_) }
if (-not \$p['Long'].Aliases.Contains('Slow')) { 'Slow' }
'all there'" 2>&1)"
	[[ "${out}" == 'all there' ]]
}
fCheck "the standard runs are parameters, with -Slow as another name for -Long" fHasParams

## Over ssh pwsh writes in code page 437, where the rule's dot is the bell byte.
## A fresh pwsh starts in 437 here, takes the switch, then prints a step's rule.
fRuleOut(){ cat > "${work}/rule.ps1" <<'PS'
param([string]$Pipeline)
[Console]::OutputEncoding = [System.Text.Encoding]::GetEncoding(437)
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Pipeline, [ref]$null, [ref]$null)
foreach ($name in 'fEcho_Clean', 'fUtf8Output') {
	. ([scriptblock]::Create($ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true).Extent.Text))
}
. ([scriptblock]::Create($ast.Find({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$script:Letterbox' }, $true).Extent.Text))
$script:WasLastEchoBlank = $false
$was = fUtf8Output
fEcho_Clean $script:Letterbox
[Console]::Error.WriteLine("was=$(if ($was) { $was.CodePage })")
PS
	pwsh -NoProfile -NonInteractive -File "${work}/rule.ps1" "${ps1}" > "${work}/rule.out" 2> "${work}/rule.err" || true
}
fRuleOut
rule="$(printf '•%.0s' {1..79})"
fCheck "a rule written to a pipe is 79 dots in UTF-8, with no bell byte" bash -c '[[ "$(cat "$1")" == "$2" ]] && ! LC_ALL=C grep -q $'"'"'\a'"'"' "$1"' _ "${work}/rule.out" "${rule}"
fCheck "and the switch hands back 437 to put back at the end" grep -qx 'was=437' "${work}/rule.err"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
##		- 20261006 JC: The stash before a pull.
##		- 20261025: -NoRemote goes to the Linux half.
##		- 20261028: The standard runs.
##		- 20261028: A rule written to a pipe is UTF-8 dots, not bell bytes.
