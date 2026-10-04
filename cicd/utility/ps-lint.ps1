#!/usr/bin/env pwsh

##	- Purpose:
##		Lint the repo's PowerShell scripts at warning level, with the rules in
##		cicd/PSScriptAnalyzerSettings.psd1, then check that indentation is tabs.
##		One line per finding. cicd.bash gates on it, and cicd-win.ps1 runs it as
##		advice.
##	- Syntax: ps-lint.ps1 [path ...]   (default: every tracked *.ps1)
##	- Exit: 0 clean, 1 findings, 2 PSScriptAnalyzer is not installed.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param([Parameter(ValueFromRemainingArguments)][string[]]$Paths)

if (-not (Get-Module -ListAvailable PSScriptAnalyzer)) { Write-Host 'PSScriptAnalyzer is not installed'; exit 2 }
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not $Paths) { $Paths = @(& git -C $root ls-files '*.ps1' | ForEach-Object { Join-Path $root $_ }) }
$settings = Join-Path $root 'cicd/PSScriptAnalyzerSettings.psd1'
$found = @($Paths | ForEach-Object { Invoke-ScriptAnalyzer -Path $_ -Settings $settings })
foreach ($f in $found) { Write-Host ('{0}:{1}: {2}: {3}' -f $f.ScriptPath, $f.Line, $f.RuleName, $f.Message) }

##	Tabs to indent, spaces only after them to line up. PSUseConsistentIndentation
##	can't tell the two apart (see the settings file). A here-string's body is
##	text, not code, so it is skipped.
$spaced = 0
foreach ($p in $Paths) {
	$tokens = $null; $errs = $null
	[void][System.Management.Automation.Language.Parser]::ParseFile($p, [ref]$tokens, [ref]$errs)
	$inText = [System.Collections.Generic.HashSet[int]]::new()
	foreach ($t in $tokens | Where-Object { $_.Kind -in 'HereStringExpandable', 'HereStringLiteral' }) {
		for ($n = $t.Extent.StartLineNumber + 1; $n -le $t.Extent.EndLineNumber; $n++) { [void]$inText.Add($n) }
	}
	$lineNo = 0
	foreach ($line in [System.IO.File]::ReadAllLines($p)) {
		$lineNo++
		if (-not $inText.Contains($lineNo) -and $line -match '^\t* +\t|^ +\S') {
			Write-Host ('{0}:{1}: Indentation: indented with spaces, not tabs' -f $p, $lineNo)
			$spaced++
		}
	}
}
if ($found.Count -or $spaced) { exit 1 }
exit 0

##	History:
##		- 20260925 JC: Created.
##		- 20261004 JC: Tab indentation check.
