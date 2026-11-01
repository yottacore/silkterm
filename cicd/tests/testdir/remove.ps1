#!/usr/bin/env pwsh

##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Check what the PowerShell test folder helper removes and what it keeps.
.DESCRIPTION
	A run removes the test run folder it made when it passes, and keeps it when
	it fails. A folder it was handed, one without its mark, one swapped for a
	link, and one not named like a run folder are left alone. A link inside is
	removed as a link, and a read-only file inside goes too. Each case runs the
	helper in a child shell with a temp folder of its own.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

##	Test ID: dde3v

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use
$failures = 0
function fCheck([string]$What, [bool]$Ok) {
	if ($Ok) { Write-Output "  ok   $What" } else { Write-Output "  FAIL $What"; $script:failures++ }
}

$helper = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../_testdir.ps1')).Path
##	The same shell as this one, so under Windows PowerShell 5.1 the cases run there too.
$shell = (Get-Process -Id $PID).Path
$work = Join-Path $env:SILKTERM_TEST_DIR "silk-remove-$PID"
$null = New-Item -ItemType Directory -Path $work
$stampRe = '^\d{8}-\d{6}_\d{3}$'
$link = if (fTestDir_OnWindows) { 'Junction' } else { 'SymbolicLink' }

##	fCase <name> <body> [<given folder>]: the body runs after fTestDir_Use in a
##	child shell whose temp folder is a fresh base, with SILKTERM_TEST_DIR unset
##	unless a folder is given.
function fCase([string]$Name, [string]$Body, [string]$Given = '') {
	$base = Join-Path $work $Name
	$null = New-Item -ItemType Directory -Path $base
	$file = Join-Path $work "$Name.ps1"
	Set-Content -LiteralPath $file -Value ". '$helper'`nfTestDir_Use`n`$dir = `$env:SILKTERM_TEST_DIR`n$Body"
	##	Warnings come back as plain lines on stdout, the same on both shells.
	##	Windows PowerShell 5.1 wraps a warning at the window width, which splits
	##	a long one across lines.
	$run = Join-Path $work "$Name-run.ps1"
	Set-Content -LiteralPath $run -Value "& '$file' 3>&1 | ForEach-Object { if (`$_ -is [System.Management.Automation.WarningRecord]) { [Console]::Out.WriteLine('WARNING: ' + `$_.Message) } else { `$_ } }`nexit `$LASTEXITCODE"
	$saved = @{ SILKTERM_TEST_DIR = $env:SILKTERM_TEST_DIR; TEMP = $env:TEMP; TMP = $env:TMP; TMPDIR = $env:TMPDIR; HOST_TEMP = $env:HOST_TEMP }
	##	Windows PowerShell 5.1 turns a native command's stderr into an error, which Stop would throw.
	$ErrorActionPreference = 'Continue'
	try {
		if ($Given) { $env:SILKTERM_TEST_DIR = $Given } else { Remove-Item env:SILKTERM_TEST_DIR -ErrorAction SilentlyContinue }
		$env:TEMP = $base; $env:TMP = $base; $env:TMPDIR = $base; $env:HOST_TEMP = $base
		$out = & $shell -NoProfile -NonInteractive -File $run 2>&1 | Out-String
		$code = $LASTEXITCODE
	} finally {
		foreach ($name in $saved.Keys) {
			if ($null -eq $saved[$name]) { Remove-Item "env:$name" -ErrorAction SilentlyContinue }
			else { Set-Item "env:$name" $saved[$name] }
		}
	}
	##	What's left in the project's folder. Anything else in the base is a stray.
	$project = Join-Path $base 'silkterm'
	$left = @(Get-ChildItem -LiteralPath $base -Force | Where-Object { $_.FullName -ne $project })
	if (Test-Path -LiteralPath $project) { $left += @(Get-ChildItem -LiteralPath $project -Force) }
	[pscustomobject]@{ Base = $base; Out = $out; Code = $code; Left = $left }
}
function fOneStamped([psobject]$Case) { $Case.Left.Count -eq 1 -and $Case.Left[0].Name -match $stampRe -and $Case.Left[0].PSIsContainer -and $Case.Left[0].Parent.Name -eq 'silkterm' }
function fShow([psobject]$Case) { if ($Case.Out) { $Case.Out.TrimEnd() -split "`n" | ForEach-Object { "      $_" } } }

$case = fCase 'pass' "Set-Content -LiteralPath (Join-Path `$dir 'file') -Value x`nfTestDir_End 0`nexit 0"
fCheck 'a run that passes removes its folder' ($case.Code -eq 0 -and $case.Left.Count -eq 0)
if ($case.Left.Count) { fShow $case }

$case = fCase 'fail' "Set-Content -LiteralPath (Join-Path `$dir 'file') -Value x`nfTestDir_End 1`nexit 1"
fCheck 'a run that fails keeps it, in HOST_TEMP''s project folder' ($case.Code -eq 1 -and (fOneStamped $case) -and (Test-Path -LiteralPath (Join-Path $case.Left[0].FullName 'file')))
fCheck 'with its PID in its run.pid' ((fOneStamped $case) -and (Get-Content -LiteralPath (Join-Path $case.Left[0].FullName 'run.pid') -TotalCount 1) -match '^\d+$')
fCheck 'and says where' ($case.Out -match 'test files kept in')

$case = fCase 'nested' "function fQuit([int]`$Code) { fTestDir_End `$Code; exit `$Code }`nfQuit 0"
fCheck 'fTestDir_End called from a function in the script removes it too' ($case.Code -eq 0 -and $case.Left.Count -eq 0)
if ($case.Left.Count) { fShow $case }

$given = Join-Path $work 'handed'
$case = fCase -Name 'given' -Body "Set-Content -LiteralPath (Join-Path `$dir 'file') -Value x`nfTestDir_End 0`nexit 0" -Given $given
fCheck 'a folder it was handed is never removed' ((Test-Path -LiteralPath (Join-Path $given 'file')) -and $case.Left.Count -eq 0)

$case = fCase 'marker' "Set-Content -LiteralPath (Join-Path `$dir '.test_silkterm_owner') -Value 'another run'`nfTestDir_End 0`nexit 0"
fCheck 'a folder without this run''s mark is left' (fOneStamped $case)
fCheck 'and says why' ($case.Out -match 'left .* in place')

$case = fCase 'swapped' "[System.IO.Directory]::Move(`$dir, `"`$dir-aside`")`n`$null = New-Item -ItemType $link -Path `$dir -Target `"`$dir-aside`"`nfTestDir_End 0`nexit 0"
$aside = @($case.Left | Where-Object { $_.Name -like '*-aside' })
$swapped = @($case.Left | Where-Object { $_.Name -match $stampRe })
fCheck 'a link put in place of the folder is left, and so is its target' ($swapped.Count -eq 1 -and $aside.Count -eq 1 -and (Test-Path -LiteralPath (Join-Path $aside[0].FullName '.test_silkterm_owner')))
if ($swapped.Count -ne 1 -or $aside.Count -ne 1) { fShow $case }

$outside = Join-Path $work 'outside'
$null = New-Item -ItemType Directory -Path $outside
Set-Content -LiteralPath (Join-Path $outside 'file') -Value x
$case = fCase 'inner' "`$null = New-Item -ItemType $link -Path (Join-Path `$dir 'link') -Target '$outside'`nfTestDir_End 0`nexit 0"
fCheck 'a link inside is removed as a link' ($case.Left.Count -eq 0)
fCheck 'and what it points to stays' (Test-Path -LiteralPath (Join-Path $outside 'file'))
if ($case.Left.Count) { fShow $case }

$readOnly = if (fTestDir_OnWindows) { "(Get-Item -LiteralPath `$file).Attributes = 'ReadOnly'" } else { "chmod 444 `$file" }
$case = fCase 'readonly' "`$null = New-Item -ItemType Directory -Path (Join-Path `$dir 'sub')`n`$file = Join-Path `$dir 'sub/file'`nSet-Content -LiteralPath `$file -Value x`n$readOnly`nfTestDir_End 0`nexit 0"
fCheck 'a read-only file inside goes too' ($case.Left.Count -eq 0)
if ($case.Left.Count) { fShow $case }

$odd = Join-Path $work 'not_a_run'
$null = New-Item -ItemType Directory -Path $odd
Set-Content -LiteralPath (Join-Path $odd '.test_silkterm_owner') -Value 'token'
$removed = fTestDir_Remove -Dir $odd -Token 'token' 3>$null
fCheck 'fTestDir_Remove refuses a folder not named like a run folder' (-not $removed -and (Test-Path -LiteralPath $odd))
##	Older runs' folders, which the ledgers still name.
$old = Join-Path $work 'test_silkterm_20261001-00000000'
$null = New-Item -ItemType Directory -Path $old
Set-Content -LiteralPath (Join-Path $old '.test_silkterm_owner') -Value 'token'
fCheck 'and takes one named the old way' ((fTestDir_Remove -Dir $old -Token 'token' 3>$null) -and -not (Test-Path -LiteralPath $old))

if ($failures) { Write-Output "$failures failed"; fTestDir_End 1; exit 1 }
Write-Output 'all passed'
fTestDir_End 0
exit 0

##	History:
##		- 20261002 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest, typed parameters.
##		- 20261101: The folder is HOST_TEMP\silkterm\YYYYmmDD-HHMMSS_NNN, with a run.pid.
