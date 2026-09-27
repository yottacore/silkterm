#!/usr/bin/env pwsh

##	- Purpose:
##		Run pieces of cicd-win.ps1 here, lifted out of the file rather than
##		retyped: the run log rotation, the path map it hands the release builds,
##		and the check for a local path left in a built file.
##	- Syntax: _lift.ps1 -Work <scratch dir> [-Pipeline <path to cicd-win.ps1>]
##		Prints one ok or FAIL line per check, and the path map's file and the
##		values it should hold as JSON on the last line, for a TOML reader to check.
##	- Exit: 0 when every check passed, 1 otherwise.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param(
	[Parameter(Mandatory)][string]$Work,
	[string]$Pipeline = (Join-Path $PSScriptRoot '../../cicd-win.ps1')
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $Pipeline).Path, [ref]$null, [ref]$null)
foreach ($name in 'fRotateLogs', 'fTargetDir', 'fRemapConfig', 'fHasLocalPaths') {
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
