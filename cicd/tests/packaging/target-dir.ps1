#!/usr/bin/env pwsh

##	- Purpose:
##		cicd-win.ps1 looks for each release binary under the target directory.
##		It used to spell that '$Root\target', which is wrong wherever
##		CARGO_TARGET_DIR is set, and stops the Windows pipeline outright.
##		fTargetDir is lifted out of the file rather than retyped.
##	- Syntax: target-dir.ps1 [-Pipeline <path to cicd-win.ps1>]
##	- Exit: 0 when every check passed, 1 otherwise.
##	- Test ID: EqAwQqA
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param(
	[string]$Pipeline = (Join-Path $PSScriptRoot '../../cicd-win.ps1')
)

Set-StrictMode -Version 2.0
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
