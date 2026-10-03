#!/usr/bin/env pwsh

##	- Purpose:
##		A run removes the test run folder it made when it passes, and keeps it
##		when it fails. A folder it was handed, one without its mark, one swapped
##		for a link, and one not named like a run folder are left alone. A link
##		inside is removed as a link, and a read-only file inside goes too. Each
##		case runs the helper in a child shell with a temp folder of its own.
##	- Syntax: remove.ps1
##	- Exit: 0 when every check passed, 1 otherwise.
##	- Test ID: ErbiCgJ
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

Set-StrictMode -Version 2.0
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
$stampRe = '^test_silkterm_\d{8}-\d{8}$'
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
	$saved = @{ SILKTERM_TEST_DIR = $env:SILKTERM_TEST_DIR; TEMP = $env:TEMP; TMP = $env:TMP; TMPDIR = $env:TMPDIR }
	##	Windows PowerShell 5.1 turns a native command's stderr into an error, which Stop would throw.
	$ErrorActionPreference = 'Continue'
	try {
		if ($Given) { $env:SILKTERM_TEST_DIR = $Given } else { Remove-Item env:SILKTERM_TEST_DIR -ErrorAction SilentlyContinue }
		$env:TEMP = $base; $env:TMP = $base; $env:TMPDIR = $base
		$out = & $shell -NoProfile -NonInteractive -File $run 2>&1 | Out-String
		$code = $LASTEXITCODE
	} finally {
		foreach ($name in $saved.Keys) {
			if ($null -eq $saved[$name]) { Remove-Item "env:$name" -ErrorAction SilentlyContinue }
			else { Set-Item "env:$name" $saved[$name] }
		}
	}
	[pscustomobject]@{ Base = $base; Out = $out; Code = $code; Left = @(Get-ChildItem -LiteralPath $base -Force) }
}
function fOneStamped($Case) { $Case.Left.Count -eq 1 -and $Case.Left[0].Name -match $stampRe -and $Case.Left[0].PSIsContainer }
function fShow($Case) { if ($Case.Out) { $Case.Out.TrimEnd() -split "`n" | ForEach-Object { "      $_" } } }

$case = fCase 'pass' "Set-Content -LiteralPath (Join-Path `$dir 'file') -Value x`nfTestDir_End 0`nexit 0"
fCheck 'a run that passes removes its folder' ($case.Code -eq 0 -and $case.Left.Count -eq 0)
if ($case.Left.Count) { fShow $case }

$case = fCase 'fail' "Set-Content -LiteralPath (Join-Path `$dir 'file') -Value x`nfTestDir_End 1`nexit 1"
fCheck 'a run that fails keeps it' ($case.Code -eq 1 -and (fOneStamped $case) -and (Test-Path -LiteralPath (Join-Path $case.Left[0].FullName 'file')))
fCheck 'and says where' ($case.Out -match 'test files kept in')

$case = fCase 'nested' "function fQuit([int]`$Code) { fTestDir_End `$Code; exit `$Code }`nfQuit 0"
fCheck 'fTestDir_End called from a function in the script removes it too' ($case.Code -eq 0 -and $case.Left.Count -eq 0)
if ($case.Left.Count) { fShow $case }

$given = Join-Path $work 'handed'
$case = fCase 'given' "Set-Content -LiteralPath (Join-Path `$dir 'file') -Value x`nfTestDir_End 0`nexit 0" $given
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

if ($failures) { Write-Output "$failures failed"; fTestDir_End 1; exit 1 }
Write-Output 'all passed'
fTestDir_End 0
exit 0

##	History:
##		- 20261002 JC: Created.
