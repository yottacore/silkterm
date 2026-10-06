#!/usr/bin/env pwsh

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Check where cicd-win.ps1 looks for each release binary.
.DESCRIPTION
	cicd-win.ps1 looks for each release binary under the target directory. It
	used to spell that '$Root\target', which is wrong wherever CARGO_TARGET_DIR
	is set, and stops the Windows pipeline outright. fTargetDir is lifted out of
	the file rather than retyped.
.PARAMETER Pipeline
	Path to cicd-win.ps1. Default: the one in this repo.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

##	Test ID: EqAwQqA

[CmdletBinding()]
param(
	[string]$Pipeline = (Join-Path $PSScriptRoot '../../cicd-win.ps1')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$text = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $Pipeline).Path)
$m = [regex]::Match($text, "(?ms)^function fTargetDir \{\r?\n.*?^\}\r?$")
if (-not $m.Success) { throw "fTargetDir not found in $Pipeline" }
. ([scriptblock]::Create($m.Value))

$failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "    ok   $What" } else { Write-Host "    FAIL $What"; $script:failures++ }
}

$Root = if ($IsWindows) { 'C:\repo' } else { '/repo' }
$saved = $env:CARGO_TARGET_DIR
try {
	$env:CARGO_TARGET_DIR = $null
	fCheck "unset means the repository's own target" ((fTargetDir) -eq (Join-Path $Root 'target'))

	$env:CARGO_TARGET_DIR = 'target/lint'
	fCheck "a relative one hangs off the repository" ((fTargetDir) -eq (Join-Path $Root 'target/lint'))

	$abs = if ($IsWindows) { 'D:\builds\silk' } else { '/builds/silk' }
	$env:CARGO_TARGET_DIR = $abs
	fCheck "an absolute one is taken as it stands" ((fTargetDir) -eq $abs)
} finally {
	$env:CARGO_TARGET_DIR = $saved
}

if ($failures -gt 0) { exit 1 }
exit 0

##	History:
##		- 20260917 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
