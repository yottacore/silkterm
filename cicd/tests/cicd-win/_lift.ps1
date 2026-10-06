#!/usr/bin/env pwsh

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Run pieces of cicd-win.ps1 here, lifted out of the file rather than retyped.
.DESCRIPTION
	The pieces: the run log rotation, the path map it hands the release builds,
	the check for a local path left in a built file, and the installer tests,
	which must leave the pipeline's temp folder as they found it.

	Prints one ok or FAIL line per check, and the path map's file and the values
	it should hold as JSON on the last line, for a TOML reader to check.
.PARAMETER Work
	Scratch folder.
.PARAMETER Pipeline
	Path to cicd-win.ps1. Default: the one in this repo.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

[CmdletBinding()]
param(
	[Parameter(Mandatory)][string]$Work,
	[string]$Pipeline = (Join-Path $PSScriptRoot '../../cicd-win.ps1')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $Pipeline).Path, [ref]$null, [ref]$null)
foreach ($name in 'fRotateLogs', 'fTargetDir', 'fRemapConfig', 'fHasLocalPaths', 'fExec', 'fInstallerTests') {
	$fn = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
	if (-not $fn) { throw "$name not found in $Pipeline" }
	. ([scriptblock]::Create($fn.Extent.Text))
}

$failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" } else { Write-Host "  FAIL $What"; $script:failures++ }
}

##	Run logs: 35 from earlier runs, named by time, among files that are not run
##	logs. The newest 29 stay, so with the new one there are 30.
$logs = Join-Path $Work 'logs'
New-Item -ItemType Directory -Path $logs -Force | Out-Null
$start = [datetime]'2026-09-01'
$names = foreach ($i in 0..34) { 'run_{0:yyyyMMdd-HHmmss}.log' -f $start.AddHours($i * 7) }
foreach ($n in $names) { Set-Content -LiteralPath (Join-Path $logs $n) -Value $n }
foreach ($n in 'notes.txt', 'other.log', 'run_keep.txt') { Set-Content -LiteralPath (Join-Path $logs $n) -Value $n }
fRotateLogs $logs 30
$left = @(Get-ChildItem -LiteralPath $logs -Filter 'run_*.log' -File | ForEach-Object Name | Sort-Object)
$want = @($names | Sort-Object | Select-Object -Last 29)
fCheck 'the newest 29 run logs are kept' ($left.Count -eq 29 -and ($left -join ',') -eq ($want -join ','))
fCheck 'and nothing else in the folder is touched' (@('notes.txt', 'other.log', 'run_keep.txt' | Where-Object { Test-Path -LiteralPath (Join-Path $logs $_) }).Count -eq 3)
fRotateLogs $logs 30
fCheck 'a second pass removes nothing more' (@(Get-ChildItem -LiteralPath $logs -Filter 'run_*.log' -File).Count -eq 29)
$few = Join-Path $Work 'few'
New-Item -ItemType Directory -Path $few -Force | Out-Null
foreach ($n in $names[0..4]) { Set-Content -LiteralPath (Join-Path $few $n) -Value $n }
fRotateLogs $few 30
fCheck 'a folder with fewer keeps them all' (@(Get-ChildItem -LiteralPath $few -File).Count -eq 5)

##	A built file still naming the profile folder or the checkout, with either
##	slash and in any case. The paths are only text here.
$env:USERPROFILE = 'C:\Users\somebody'
$Root = 'C:\src\silkterm'
function fHas { param([string]$Text)
	$f = Join-Path $Work 'built.bin'
	[System.IO.File]::WriteAllBytes($f, [byte[]](@(0, 1, 2) + [System.Text.Encoding]::UTF8.GetBytes($Text) + @(0)))
	return (fHasLocalPaths $f)
}
fCheck 'the profile folder is found' (fHas 'panicked at C:\Users\somebody\.cargo\registry\src\x.rs')
fCheck 'with forward slashes too' (fHas 'C:/Users/somebody/.cargo/registry/src/x.rs')
fCheck 'the checkout is found, in another case' (fHas 'c:\SRC\silkterm\source\main.rs')
fCheck 'a mapped path is not' (-not (fHas '/cargo/registry/src/x.rs and /silkterm/source/main.rs'))
fCheck 'nor is a longer name that starts the same' (-not (fHas 'C:\Users\somebodyelse\x.rs'))

##	The installer tests, with stand-ins that each take the run folder the way
##	the real ones do, note the temp folder they saw, and end the way the real
##	ones do. One takes the Windows branch, so TEMP and TMP move too. Afterward
##	the pipeline's own temp folder, which the release builds use, must be what
##	it was, TMP still unset. The pipeline's run folder is made here the way
##	cicd-win.ps1 makes it, and only the pipeline's own fTestDir_End removes it.
. (Join-Path $PSScriptRoot '../_testdir.ps1')
function fEcho { }
function fDie { param([string]$Msg); throw $Msg }
$Root = Join-Path $Work 'root'
$seen = Join-Path $Work 'seen.txt'
$helper = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../_testdir.ps1')).Path
$stub = { param([string]$Extra)
	"param([string]`$Shell)`n. '$helper'`n$Extra`nfTestDir_Use`nAdd-Content -LiteralPath '$seen' -Value ([System.IO.Path]::GetTempPath() + '|' + `$env:TEMP + '|' + `$env:TMP)`nfTestDir_End 0`nexit 0"
}
foreach ($rel in 'cicd/tests/release/verify-sign.ps1', 'cicd/tests/install/tempdir.ps1', 'cicd/tests/install/windows.ps1') {
	$path = Join-Path $Root $rel
	New-Item -ItemType Directory -Path (Split-Path $path) -Force | Out-Null
	$extra = if ($rel -like '*tempdir*') { 'function fTestDir_OnWindows { $true }' } else { '' }
	Set-Content -LiteralPath $path -Value (& $stub $extra)
}
$sys = Join-Path $Work 'sys'
New-Item -ItemType Directory -Path $sys -Force | Out-Null
$env:TEMP = $sys; $env:TMPDIR = $sys; Remove-Item env:TMP -ErrorAction SilentlyContinue
Remove-Item env:SILKTERM_TEST_DIR -ErrorAction SilentlyContinue
fTestDir_Make
$run = $env:SILKTERM_TEST_DIR
fInstallerTests
fCheck 'the installer tests leave TMPDIR as it was' ($env:TMPDIR -eq $sys)
fCheck 'and TEMP' ($env:TEMP -eq $sys)
fCheck 'and TMP still unset' (-not (Test-Path env:TMP))
fCheck 'so the temp folder is the system one again' ([System.IO.Path]::GetTempPath().TrimEnd('/', '\') -eq $sys)
$lines = @(Get-Content -LiteralPath $seen)
$inRun = @($lines | Where-Object { $_.Split('|')[0].StartsWith($env:SILKTERM_TEST_DIR) })
fCheck 'while each installer test had the run folder as its temp folder' ($lines.Count -eq 4 -and $inRun.Count -eq 4)
fCheck 'with TEMP and TMP there too in the one that takes the Windows branch' ($lines.Count -eq 4 -and $lines[1].EndsWith("|$($env:SILKTERM_TEST_DIR)|$($env:SILKTERM_TEST_DIR)"))
fCheck 'the pipeline made its run folder in the system temp folder' ((Split-Path $run) -eq $sys.TrimEnd('/', '\'))
fCheck 'and none of the installer tests removed it' (Test-Path -LiteralPath $run)
fTestDir_End 0
fCheck 'while the pipeline''s own fTestDir_End 0 does' (-not (Test-Path -LiteralPath $run))

##	The path map, from folders whose names hold a backslash and a quote. The
##	TOML reader on the other end decides whether they were escaped right.
$Root = "$Work/sil\k`"term"
New-Item -ItemType Directory -Path $Root -Force | Out-Null
$env:CARGO_HOME = "$Work/car\go"
$env:CARGO_TARGET_DIR = $null
$cfg = fRemapConfig
@{
	cfg  = $cfg
	want = @("--remap-path-prefix=$($env:CARGO_HOME)=/cargo", "--remap-path-prefix=$Root=/silkterm", "--remap-path-prefix=$(Join-Path $Root 'target')=/target")
} | ConvertTo-Json -Compress

if ($failures -gt 0) { exit 1 }
exit 0

##	History:
##		- 20260926 JC: Created.
##		- 20260930 JC: The installer tests leave the temp folder alone.
##		- 20261002 JC: Only the pipeline removes its run folder.
##		- 20261006 JC: Help block, StrictMode Latest.
