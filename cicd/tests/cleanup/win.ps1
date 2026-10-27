#!/usr/bin/env pwsh

##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Check what cicd-win.ps1's sweep of old test run folders removes and keeps.
.DESCRIPTION
	Folders written down in a ledger, oldest first, past the newest few still
	there, go. One with another run's mark, one swapped for a link, and one
	nobody wrote down stay. A folder handed down is never written down.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

##	Test ID: EtAiwnM

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$helper = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../_testdir.ps1')).Path
. $helper; fTestDir_Use
$failures = 0
function fCheck([string]$What, [bool]$Ok) {
	if ($Ok) { Write-Output "  ok   $What" } else { Write-Output "  FAIL $What"; $script:failures++ }
}

$work = Join-Path $env:SILKTERM_TEST_DIR "silk-cleanup-$PID"
$null = New-Item -ItemType Directory -Path $work
$base = Join-Path $work 'base'
$null = New-Item -ItemType Directory -Path $base
$ledger = Join-Path $work 'ledger'
$link = if (fTestDir_OnWindows) { 'Junction' } else { 'SymbolicLink' }

function fDir([int]$N) { Join-Path $base ('test_silkterm_2026101{0}-00000000' -f $N) }
foreach ($n in 1..6) {
	$null = New-Item -ItemType Directory -Path (fDir $n)
	Set-Content -LiteralPath (Join-Path (fDir $n) '.test_silkterm_owner') -Value "tok-$n"
	Set-Content -LiteralPath (Join-Path (fDir $n) 'out.txt') -Value 'x'
	[System.IO.File]::AppendAllText($ledger, "$(fDir $n)`ttok-$n`n")
}
Set-Content -LiteralPath (Join-Path (fDir 2) '.test_silkterm_owner') -Value 'tok-other'
Remove-Item -LiteralPath (fDir 3) -Recurse -Force
$elsewhere = Join-Path $work 'four'
Move-Item -LiteralPath (fDir 4) -Destination $elsewhere
$null = New-Item -ItemType $link -Path (fDir 4) -Target $elsewhere
$stray = Join-Path $base 'test_silkterm_20261099-00000000'
$null = New-Item -ItemType Directory -Path $stray
Set-Content -LiteralPath (Join-Path $stray '.test_silkterm_owner') -Value 'tok-99'

$gone = fTestDir_Sweep -Ledger $ledger -Keep 2 -WarningVariable said 3>$null
fCheck 'the newest two still there are kept' ((Test-Path (Join-Path (fDir 6) 'out.txt')) -and (Test-Path (Join-Path (fDir 5) 'out.txt')))
fCheck 'an older one with its own mark goes' (-not (Test-Path -LiteralPath (fDir 1)))
fCheck 'and the sweep counts it' ($gone -eq 1)
fCheck "one with another run's mark stays" (Test-Path (Join-Path (fDir 2) 'out.txt'))
fCheck 'and the sweep says why' ([bool]($said | Where-Object { "$_" -like "*another run's owner mark*" }))
fCheck 'a link put in its place stays, and what it points to' ((Test-Path -LiteralPath (fDir 4)) -and (Test-Path (Join-Path $elsewhere 'out.txt')))
fCheck 'a folder nobody wrote down stays' (Test-Path -LiteralPath $stray)
$left = @([System.IO.File]::ReadAllLines($ledger) | ForEach-Object { ($_ -split "`t")[1] })
fCheck 'the ledger keeps only what it kept' (($left -join ' ') -eq 'tok-5 tok-6')

##	In a child, since this process made its own folder when run alone.
$given = Join-Path $work 'given'
$handed = Join-Path $work 'handed'
$child = Join-Path $work 'child.ps1'
Set-Content -LiteralPath $child -Value @"
`$env:SILKTERM_TEST_DIR = '$given'
. '$helper'
fTestDir_Make
fTestDir_Record -Ledger '$handed'
fTestDir_End 0
"@
& (Get-Process -Id $PID).Path -NoProfile -File $child
fCheck 'a folder handed down is never written down' (-not (Test-Path -LiteralPath $handed))
fCheck 'and is never removed' (Test-Path -LiteralPath $given -PathType Container)

##	A child with no folder handed down makes one, writes it down, and removes it on a pass.
$own = Join-Path $work 'own'
Set-Content -LiteralPath $child -Value @"
`$env:SILKTERM_TEST_DIR = `$null
. '$helper'
fTestDir_Make
fTestDir_Record -Ledger '$own'
fTestDir_End 0
"@
& (Get-Process -Id $PID).Path -NoProfile -File $child
$line = @([System.IO.File]::ReadAllLines($own))
fCheck 'a folder a run made is written down with its token' ($line.Count -eq 1 -and $line[0] -match '[\\/]test_silkterm_\d{8}-\d{8}\t\d+-\d+$')

Remove-Item -LiteralPath (fDir 4) -Force
fTestDir_End ([int]($failures -ne 0))
if ($failures) { Write-Output "$failures failed"; exit 1 }
Write-Output 'all passed'

##	History:
##		- 20261019 JC: Created.
##		- 20261026 JC: The handed-down check runs in a child, so it passes alone.
