#!/usr/bin/env pwsh

<#
.SYNOPSIS
	Lint the repo's PowerShell scripts.
.DESCRIPTION
	Runs PSScriptAnalyzer at warning level with the rules in
	cicd/PSScriptAnalyzerSettings.psd1, then checks what it has no rule for:
	indentation is tabs, no PowerShell command takes three or more arguments by
	position, and no common parameter goes by its alias (-EA and the like).
	One line per finding. cicd.bash gates on it, and cicd-win.ps1 runs it as
	advice.
.PARAMETER Paths
	Scripts to lint. Default: every tracked *.ps1.
.EXAMPLE
	pwsh -File cicd/utility/ps-lint.ps1 install.ps1
.NOTES
	Exit: 0 clean, 1 findings, 2 PSScriptAnalyzer is not installed.
	History: At bottom of file.
#>

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments)][string[]]$Paths)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Module -ListAvailable -Name PSScriptAnalyzer)) { Write-Host 'PSScriptAnalyzer is not installed'; exit 2 }
$root = (Resolve-Path -Path (Join-Path -Path $PSScriptRoot -ChildPath '../..')).Path
if (-not $Paths) { $Paths = @(& git -C $root ls-files '*.ps1' | ForEach-Object { Join-Path -Path $root -ChildPath $_ }) }
$settings = Join-Path -Path $root -ChildPath 'cicd/PSScriptAnalyzerSettings.psd1'
$found = @($Paths | ForEach-Object { Invoke-ScriptAnalyzer -Path $_ -Settings $settings })
foreach ($f in $found) { Write-Host ('{0}:{1}: {2}: {3}' -f $f.ScriptPath, $f.Line, $f.RuleName, $f.Message) }
$other = 0

$parsed = foreach ($p in $Paths) {
	$tokens = $null; $errs = $null
	$ast = [System.Management.Automation.Language.Parser]::ParseFile($p, [ref]$tokens, [ref]$errs)
	[PSCustomObject]@{ Path = $p; Ast = $ast; Tokens = $tokens }
}

##	Tabs to indent, spaces only after them to line up. PSUseConsistentIndentation
##	can't tell the two apart (see the settings file). A here-string's body is
##	text, not code, so it is skipped.
foreach ($one in $parsed) {
	$inText = [System.Collections.Generic.HashSet[int]]::new()
	foreach ($t in $one.Tokens | Where-Object { $_.Kind -in 'HereStringExpandable', 'HereStringLiteral' }) {
		for ($n = $t.Extent.StartLineNumber + 1; $n -le $t.Extent.EndLineNumber; $n++) { [void]$inText.Add($n) }
	}
	$lineNo = 0
	foreach ($line in [System.IO.File]::ReadAllLines($one.Path)) {
		$lineNo++
		if (-not $inText.Contains($lineNo) -and $line -match '^\t* +\t|^ +\S') {
			Write-Host ('{0}:{1}: Indentation: indented with spaces, not tabs' -f $one.Path, $lineNo)
			$other++
		}
	}
}

##	PSAvoidUsingPositionalParameters is only Information, knows only the
##	functions in its own file, and nothing in PSScriptAnalyzer looks at parameter
##	aliases. Both checks here take a command as PowerShell's when one of the
##	linted scripts defines it, or when it is Verb-Noun and no program on PATH
##	has that name. A program's arguments are its own business.
$ours = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
foreach ($one in $parsed) {
	$one.Ast.FindAll({ param($a) $a -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true) | ForEach-Object { [void]$ours.Add($_.Name) }
}
$isPs = @{}
function fIsPowerShellCommand([string]$Name) {
	if ($ours.Contains($Name)) { return $true }
	if (-not $isPs.ContainsKey($Name)) {
		$isPs[$Name] = $Name -match '^[A-Za-z]+-[A-Za-z]+$' -and -not (Get-Command -Name $Name -CommandType Application -ErrorAction Ignore)
	}
	return $isPs[$Name]
}
$aliasOf = @{
	ea = 'ErrorAction'; ev = 'ErrorVariable'; wa = 'WarningAction'; wv = 'WarningVariable'
	infa = 'InformationAction'; iv = 'InformationVariable'; ov = 'OutVariable'; ob = 'OutBuffer'
	pv = 'PipelineVariable'; vb = 'Verbose'; db = 'Debug'
}
foreach ($one in $parsed) {
	$calls = $one.Ast.FindAll({ param($a) $a -is [System.Management.Automation.Language.CommandAst] }, $true)
	foreach ($call in $calls) {
		$name = $call.GetCommandName()
		if (-not $name -or -not (fIsPowerShellCommand $name)) { continue }
		$items = $call.CommandElements
		$byPosition = 0
		for ($i = 1; $i -lt $items.Count; $i++) {
			$item = $items[$i]
			if ($item -is [System.Management.Automation.Language.CommandParameterAst]) {
				if ($aliasOf.ContainsKey($item.ParameterName)) {
					Write-Host ('{0}:{1}: ParameterAlias: -{2} is an alias; write -{3}' -f $one.Path, $item.Extent.StartLineNumber, $item.ParameterName, $aliasOf[$item.ParameterName])
					$other++
				}
				continue
			}
			##	Same reading as PSScriptAnalyzer's: a value right after a bare -Name
			##	is that parameter's, so a switch followed by a value is missed.
			$after = $items[$i - 1]
			$taken = $after -is [System.Management.Automation.Language.CommandParameterAst] -and $null -eq $after.Argument
			$splat = $item -is [System.Management.Automation.Language.VariableExpressionAst] -and $item.Splatted
			if (-not $taken -and -not $splat) { $byPosition++ }
		}
		if ($byPosition -ge 3) {
			Write-Host ('{0}:{1}: Positional: {2} takes {3} arguments by position; name them' -f $one.Path, $call.Extent.StartLineNumber, $name, $byPosition)
			$other++
		}
	}
}

if ($found.Count -or $other) { exit 1 }
exit 0

##	History:
##		- 20260925 JC: Created.
##		- 20261004 JC: Tab indentation check.
##		- 20261006 JC: Positional argument and parameter alias checks. Help block.
