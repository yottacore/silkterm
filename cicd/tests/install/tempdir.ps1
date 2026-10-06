#!/usr/bin/env pwsh

<#
.SYNOPSIS
	Check that install.ps1 makes its download folder safely.
.DESCRIPTION
	install.ps1 makes its download folder under the shared temp folder. A name
	someone else can make first, or a step that takes over a folder already
	there, hands the download to whoever made it. This runs the installer's own
	lines for that step, lifted from the file: a new name each time, and a path
	that already exists is refused.
.PARAMETER Installer
	Path to install.ps1. Default: the one in this repo.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

##	Test ID: EqBoTW4

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

[CmdletBinding()]
param(
	[string]$Installer = (Join-Path $PSScriptRoot '../../../install.ps1')

)

Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use
$failures = 0
function fCheck([string]$What, [bool]$Ok) {
	if ($Ok) { Write-Output "  ok   $What" } else { Write-Output "  FAIL $What"; $script:failures++ }
}

$text = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $Installer).Path)
##	From the name through the step that makes the folder and its failure path.
$m = [regex]::Match($text, '(?ms)^\s*\$tmpDir = .*?^\s*catch \{[^\r\n]*')
if (-not $m.Success) { Write-Output "  FAIL no temp folder step found in $Installer"; fTestDir_End 1; exit 1 }
$statements = [System.Management.Automation.Language.Parser]::ParseInput($m.Value, [ref]$null, [ref]$null).EndBlock.Statements
$pick = [scriptblock]::Create($statements[0].Extent.Text)
$make = [scriptblock]::Create((($statements | Select-Object -Skip 1) | ForEach-Object { $_.Extent.Text }) -join "`n")

function fFail { throw 'refused' }
$exeName = 'silkterm'
$made = @()
try {
	. $pick; . $make
	$first = $tmpDir
	$made += $first
	fCheck 'the folder is made under the temp folder' ((Test-Path -LiteralPath $first -PathType Container) -and
		$first.StartsWith([System.IO.Path]::GetTempPath()))
	. $pick
	fCheck 'and gets a new name each time' ($tmpDir -ne $first)

	##	Somebody else got there first. The step has to stop rather than use it.
	$tmpDir = $first
	$refused = $false
	try { . $make } catch { $refused = $true }
	fCheck 'a folder that is already there is refused' $refused
} finally {
	foreach ($dir in $made) { Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue }
}

if ($failures) { Write-Output "$failures failed"; fTestDir_End 1; exit 1 }
Write-Output 'all passed'
fTestDir_End 0
exit 0

##	History:
##		- 20260917 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
