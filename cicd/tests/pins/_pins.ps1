#!/usr/bin/env pwsh

##	- Purpose:
##		The tool pins cicd-win.ps1 reads, one "name|version" line each. Its own
##		fCheckToolPins is lifted out of the file and run with nothing on PATH, so
##		every pin it reads is reported as a missing tool.
##	- Syntax: _pins.ps1 -Pins <tool-pins.txt> [-Pipeline <path to cicd-win.ps1>]
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param(
	[Parameter(Mandatory)][string]$Pins,
	[string]$Pipeline = (Join-Path $PSScriptRoot '../../cicd-win.ps1')
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $Pipeline).Path, [ref]$null, [ref]$null)
$fn = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'fCheckToolPins' }, $true)
if (-not $fn) { throw "fCheckToolPins not found in $Pipeline" }
. ([scriptblock]::Create($fn.Extent.Text))

$script:warnings = [System.Collections.Generic.List[string]]::new()
function fWarn { param([string]$Msg) $script:warnings.Add($Msg) }
function fFindMakensis { return $null }
$ToolPinsFile = $Pins

$empty = Join-Path ([System.IO.Path]::GetTempPath()) "silk-pins-$PID"
New-Item -ItemType Directory -Path $empty -Force | Out-Null
$savedPath = $env:PATH
try {
	$env:PATH = $empty
	fCheckToolPins
} finally {
	$env:PATH = $savedPath
	Remove-Item -LiteralPath $empty -Force
}
foreach ($w in $script:warnings) {
	$m = [regex]::Match($w, '^(\S+) not found \(pinned (.+)\)$')
	if (-not $m.Success) { throw "unexpected warning: $w" }
	"$($m.Groups[1].Value)|$($m.Groups[2].Value)"
}

##	History:
##		- 20260926 JC: Created.
