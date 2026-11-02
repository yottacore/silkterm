#!/usr/bin/env pwsh

##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Run pieces of cicd-win.ps1 here, lifted out of the file rather than retyped.
.DESCRIPTION
	The pieces: the run log rotation, the path map it hands the release builds,
	the check for a local path left in a built file, the installer tests,
	which must leave the pipeline's temp folder as they found it, the run's
	folder, removed at the end, and the stale ones swept, the stash
	taken before a pull, which answers whether it stashed anything, and the run
	options: their other names, the pairs refused, what the Linux half is
	handed, and -List printing the test scripts rather than running them.

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
foreach ($name in 'fRotateLogs', 'fTargetDir', 'fRemapConfig', 'fHasLocalPaths', 'fExec', 'fScriptTest', 'fPython', 'fCargoTest', 'fInstallerTests', 'fScriptTests', 'fStashIfDirty', 'fRunFlags', 'fWslArgs', 'fQuickRun', 'fStamp', 'fStepLine', 'fStepIf', 'fStepOpen', 'fStepClose', 'fShowSteps', 'fPlanSteps', 'fRunDirMake', 'fRunDirEnd', 'fSweepStaleRuns', 'fOldLedger', 'fDirBytes', 'fCleanupCount', 'fCleanupNote', 'fCleanupLine') {
	$fn = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
	if (-not $fn) { throw "$name not found in $Pipeline" }
	. ([scriptblock]::Create($fn.Extent.Text))
}
foreach ($name in '$QuickAreas', '$QuickTitles', '$script:StepsOn', '$script:StepsOff', '$script:Letterbox', '$script:StepNum', '$script:StepName', '$Targets', '$script:CleanupKinds', '$script:CleanupBytes', '$script:CleanupDue') {
	$set = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq $name }, $true)
	if (-not $set) { throw "$name not found in $Pipeline" }
	. ([scriptblock]::Create($set.Extent.Text))
}
$List = $false

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
function fEcho_Clean { }
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
Remove-Item env:HOST_TEMP -ErrorAction SilentlyContinue
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
fCheck 'the pipeline made its run folder in the project''s folder in the system temp folder' ((Split-Path $run) -eq (Join-Path $sys.TrimEnd('/', '\') 'silkterm'))
fCheck 'and none of the installer tests removed it' (Test-Path -LiteralPath $run)
fTestDir_End 0
fCheck 'while the pipeline''s own fTestDir_End 0 does' (-not (Test-Path -LiteralPath $run))

##	The run's folder: HOST_TEMP\silkterm\YYYYmmDD-HHMMSS_NNN, else under the temp
##	folder, with TEMP and TMP pointed at it. It goes at the end, failed or not,
##	unless -KeepTemp. Stale ones go once no process from their run.pid is left,
##	and so do the folders an old ledger names.
. (Join-Path $PSScriptRoot '../../utility/include/saferm.ps1')
$script:notes = [System.Collections.Generic.List[string]]::new()
function fNote { param([string]$Msg); $script:notes.Add($Msg) }
$KeepTemp = $false
$envWas = @{}
foreach ($name in 'TEMP', 'TMP', 'TMPDIR', 'HOST_TEMP', 'SILKTERM_TEST_DIR') { $envWas[$name] = [Environment]::GetEnvironmentVariable($name) }
$sys2 = Join-Path $Work 'sys2'
$null = New-Item -ItemType Directory -Path $sys2
$env:TEMP = $sys2; $env:TMP = $sys2; $env:TMPDIR = $sys2
Remove-Item env:HOST_TEMP, env:SILKTERM_TEST_DIR -ErrorAction SilentlyContinue
fRunDirMake
$rd = $script:RunDir
fCheck 'the run''s folder is made in the project''s folder in the temp folder, named for the time' ((Split-Path $rd) -eq (Join-Path $sys2 'silkterm') -and (Split-Path -Leaf $rd) -match '^\d{8}-\d{6}_\d{3}$')
fCheck 'with this process''s PID in its run.pid' ("$(Get-Content -LiteralPath (Join-Path $rd 'run.pid') -TotalCount 1)" -eq "$PID")
fCheck 'and TEMP and TMP pointed at it' ($env:TEMP -eq $rd -and $env:TMP -eq $rd)
fCheck 'and it''s the tests'' folder' ($env:SILKTERM_TEST_DIR -eq $rd)
fRunDirEnd $false
fCheck 'a run that failed removes it' (-not (Test-Path -LiteralPath $rd))
fCheck 'and says -KeepTemp keeps it' ($script:notes -contains "removed the run's temp folder $rd; -KeepTemp keeps it")
fCheck 'and counts it, and its size, for the cleanup line' ($script:CleanupKinds.Contains("the run's temp folder") -and $script:CleanupBytes -gt 0)
$ht = Join-Path $Work 'ht'
$null = New-Item -ItemType Directory -Path $ht
$env:HOST_TEMP = $ht; $env:SILKTERM_TEST_DIR = Join-Path $Work 'handed-in'
$script:notes.Clear()
fRunDirMake
$rd = $script:RunDir
fCheck 'with HOST_TEMP set, it''s made there' ((Split-Path $rd) -eq (Join-Path $ht 'silkterm'))
fCheck 'and a test folder handed in stays the tests'' folder' ($env:SILKTERM_TEST_DIR -eq (Join-Path $Work 'handed-in') -and $env:TEMP -eq $rd)
fRunDirEnd $true
fCheck 'a run that passed removes it, and says nothing' (-not (Test-Path -LiteralPath $rd) -and $script:notes.Count -eq 0)
$KeepTemp = $true
fRunDirMake
$rd = $script:RunDir
fRunDirEnd $false
fCheck '-KeepTemp keeps it' (Test-Path -LiteralPath (Join-Path $rd 'run.pid'))
fCheck 'and says where' ($script:notes -contains "kept the run's temp folder (-KeepTemp): $rd")
$KeepTemp = $false
foreach ($name in $envWas.Keys) { [Environment]::SetEnvironmentVariable($name, $envWas[$name]) }

$stale = Join-Path $Work 'stale'
$dead = (Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList '-NoProfile', '-Command', 'exit 0' -PassThru -Wait).Id
function fStale([string]$At, [string]$RunPid, [int]$DaysOld) {
	$null = New-Item -ItemType Directory -Path $At -Force
	if ($RunPid) { Set-Content -LiteralPath (Join-Path $At 'run.pid') -Value $RunPid }
	Set-Content -LiteralPath (Join-Path $At 'out.txt') -Value 'x'
	(Get-Item -LiteralPath $At).LastWriteTime = (Get-Date).AddDays(-$DaysOld)
	$At
}
$s1 = fStale -At (Join-Path $stale '20261001-000000_001') -RunPid "$dead" -DaysOld 3
$s2 = fStale -At (Join-Path $stale '20261001-000000_002') -RunPid "$PID" -DaysOld 3
$s3 = fStale -At (Join-Path $stale '20261001-000000_003') -RunPid "$dead" -DaysOld 1
$s4 = fStale -At (Join-Path $stale '20261001-000000_004') -RunPid '' -DaysOld 3
$s5 = fStale -At (Join-Path $stale 'by-hand') -RunPid "$dead" -DaysOld 3
$away = fStale -At (Join-Path $Work 'away') -RunPid "$dead" -DaysOld 3
$s6 = Join-Path $stale '20261001-000000_006'
$null = New-Item -ItemType $(if (fTestDir_OnWindows) { 'Junction' } else { 'SymbolicLink' }) -Path $s6 -Target $away
$s1Bytes = fDirBytes $s1
$hid = Join-Path $Work 'hid'
$null = New-Item -ItemType Directory -Path $hid
$hidFile = Join-Path $hid '.h'
[System.IO.File]::WriteAllText($hidFile, 'xyz')
(Get-Item -LiteralPath $hidFile -Force).Attributes = [System.IO.FileAttributes]::Hidden
fCheck 'a folder''s size takes its hidden files' ((fDirBytes $hid) -eq 3)
fCheck 'but not what a link in it points to' ((fDirBytes $stale) -eq (($s1, $s2, $s3, $s4, $s5 | ForEach-Object { fDirBytes $_ } | Measure-Object -Sum).Sum))
$script:CleanupBytes = [long]0
$gone = fSweepStaleRuns $stale 2
fCheck 'a run folder over 2 days old, with no process from its run.pid left, goes' (-not (Test-Path -LiteralPath $s1))
fCheck 'and only that one' ($gone -eq 1)
fCheck 'and its size goes to what the cleanup line says it freed' ($s1Bytes -gt 0 -and $script:CleanupBytes -eq $s1Bytes)
fCheck 'one whose process is still there stays' (Test-Path -LiteralPath (Join-Path $s2 'out.txt'))
fCheck 'so does a younger one' (Test-Path -LiteralPath (Join-Path $s3 'out.txt'))
fCheck 'and one with no run.pid' (Test-Path -LiteralPath (Join-Path $s4 'out.txt'))
fCheck 'and one not named as a run folder' (Test-Path -LiteralPath (Join-Path $s5 'out.txt'))
fCheck 'and a link, and what it points to' ((Test-Path -LiteralPath $s6) -and (Test-Path -LiteralPath (Join-Path $away 'out.txt')))
fCheck 'with no folder there, nothing' ((fSweepStaleRuns (Join-Path $Work 'none') 2) -eq 0)
Remove-Item -LiteralPath $s6 -Force

$oldBase = Join-Path $Work 'old'
$oldDir = Join-Path $oldBase 'test_silkterm_20261020-00000000'
$null = New-Item -ItemType Directory -Path $oldDir
Set-Content -LiteralPath (Join-Path $oldDir '.test_silkterm_owner') -Value 'tok-1'
$oldLedger = Join-Path $Work 'old.ledger'
[System.IO.File]::WriteAllText($oldLedger, "$oldDir`ttok-1`n")
$script:CleanupBytes = [long]0
$gone = fOldLedger $oldLedger
fCheck 'an old ledger''s folders go, under its checks' ($gone -eq 1 -and -not (Test-Path -LiteralPath $oldDir))
fCheck 'and then the ledger' (-not (Test-Path -LiteralPath $oldLedger))
fCheck 'and what they held goes to what the cleanup line says it freed' ($script:CleanupBytes -gt 0)
fCheck 'with no ledger, nothing' ((fOldLedger $oldLedger) -eq 0)

##	The run's one cleanup line, as cicd.bash prints it: each kind with its count,
##	or just named, and about what it freed.
$script:notes.Clear()
$script:CleanupKinds = [ordered]@{}
$script:CleanupDue = $false
fCleanupCount 'stale run folders' 1
fCleanupLine
fCheck 'no cleanup line for a run that never made its folder' ($script:notes.Count -eq 0)
$script:CleanupDue = $true
fCleanupCount 'stale run folders' 2
fCleanupCount 'old test run folders' 0
fCleanupNote "the run's temp folder"
fCleanupNote "the run's temp folder"
$script:CleanupBytes = [long](5.2 * 1GB)
fCleanupLine
fCheck 'one cleanup line: what went, the run''s folder included, and about what it freed' ($script:notes.Count -eq 1 -and $script:notes[0] -eq "cleanup: removed 3 stale run folders, the run's temp folder; about 5.2 GB freed")
fCleanupLine
fCheck 'and only once' ($script:notes.Count -eq 1)
$script:CleanupDue = $true
$script:CleanupBytes = [long]1572864
fCleanupLine
fCheck 'in MB under a GB' ($script:notes[-1] -eq "cleanup: removed 3 stale run folders, the run's temp folder; about 2 MB freed")
$script:CleanupDue = $true
$script:CleanupBytes = [long]4096
fCleanupLine
fCheck 'and under 1 MB below that' ($script:notes[-1] -eq "cleanup: removed 3 stale run folders, the run's temp folder; under 1 MB freed")
$script:CleanupDue = $true
$script:CleanupKinds = [ordered]@{}
fCleanupLine
fCheck 'with nothing removed, it says so' ($script:notes[-1] -eq 'cleanup: nothing stale')

$text = Get-Content -LiteralPath $Pipeline -Raw
fCheck 'the pipeline makes its run folder, then sweeps the stale ones and the old ledger' ($text -match '(?m)^\s*fRunDirMake\s*\r?\n\s*\$script:CleanupDue = \$true\s*\r?\n\s*fCleanupCount ''stale run folders'' \(fSweepStaleRuns \(Split-Path -Parent \$script:RunDir\) \$StaleDays\)\s*\r?\n\s*fCleanupCount ''old test run folders'' \(fOldLedger ')
fCheck 'and says nothing of it until the end' ($text -notmatch 'fNote "cleanup: removed \$gone')
fCheck 'where the cleanup line comes before done' ($text -match '(?m)^\s*fRunDirEnd \$true\s*\r?\n\s*fCleanupLine\s*\r?\n\s*fSection "\$AppName Windows CI/CD: done\."')
fCheck 'and removes it in its finally block, on a failure too' ($text -match '(?s)\} finally \{\s*##[^\n]*\n\s*fStepClose FAIL\s*\n\s*##[^\n]*\n\s*fRunDirEnd \$script:RunOk')
fCheck 'and then says the cleanup line, on a failure too' ($text -match '(?m)^\s*fRunDirEnd \$script:RunOk\s*\r?\n\s*fCleanupLine\s*\r?\n\s*try \{ Stop-Transcript')
fCheck 'which fDie''s exit reaches' ($text -match '(?m)^function fDie .*; exit 1 \}\r?$' -and $text -notmatch 'fTestDir_End 1')
fCheck 'with 2 days for a stale one' ($text -match '(?m)^\$StaleDays = 2\r?$')

##	The stash before a pull answers with a bare bool, so git's own lines must
##	not end up in it. A clean tree stashes nothing; a dirty one is stashed with
##	its untracked files.
function fEcho_Clean { }
$repo = Join-Path $Work 'stash'
New-Item -ItemType Directory -Path $repo -Force | Out-Null
Push-Location -LiteralPath $repo
try {
	& git init --quiet
	& git config user.name 'test'
	& git config user.email 'test@example.invalid'
	& git config commit.gpgsign false
	& git config core.hooksPath (Join-Path $Work 'nohooks')
	Set-Content -LiteralPath 'a.txt' -Value 'one'
	& git add a.txt
	& git commit --quiet -m 'first'
	$clean = fStashIfDirty
	fCheck 'a clean tree is not stashed' ($clean -is [bool] -and -not $clean -and @(& git stash list).Count -eq 0)
	Set-Content -LiteralPath 'a.txt' -Value 'two'
	Set-Content -LiteralPath 'b.txt' -Value 'new'
	$dirty = fStashIfDirty
	fCheck 'a dirty tree is, untracked files too, and the answer is only true' ($dirty -is [bool] -and $dirty -and @(& git stash list).Count -eq 1 -and -not (Test-Path -LiteralPath 'b.txt'))
}
finally { Pop-Location }

##	The run options. Each case starts from none set, as a bare run does, and
##	notes the refusal, the one-line notices and what the Linux half gets.
$flagNames = 'Gate', 'Ci', 'BacklogRunQuick', 'BacklogRunFull', 'MergeGate', 'Quick', 'Long', 'Leakcheck', 'NoLeakcheck', 'NoArm', 'NoPackage', 'NoRemote', 'NoContainer', 'KeepTemp', 'List', 'NoSync', 'NoDogfood', 'NoPublish', 'Unattended'
function fRun { param([hashtable]$Set = @{})
	foreach ($n in $flagNames) { Set-Variable -Name $n -Value $false -Scope Script }
	$script:Areas = @()
	foreach ($k in $Set.Keys) { Set-Variable -Name $k -Value $Set[$k] -Scope Script }
	$saved = [Console]::Error
	$err = [System.IO.StringWriter]::new()
	[Console]::SetError($err)
	try { $why = fRunFlags } finally { [Console]::SetError($saved) }
	return [pscustomobject]@{ Why = "$why"; Said = $err.ToString().Trim(); Wsl = (fWslArgs) -join ' ' }
}
$plain = '-y --no-sync --no-fmt --no-windows --no-dogfood --no-publish'
$r = fRun
fCheck 'a bare run hands the Linux half what it always did' ($r.Why -eq '' -and $r.Wsl -eq $plain -and $r.Said -eq '')
$r = fRun @{ Gate = $true }
fCheck '-Gate says in one line that it is another name' ($r.Said -eq '-Gate is another name for -BacklogRunQuick')
fCheck 'and is the backlog quick run, for the Linux half too' ($BacklogRunQuick -and $r.Why -eq '' -and $r.Wsl -eq '--backlog-run-quick')
$r = fRun @{ BacklogRunQuick = $true; Areas = @('FMT', 'tests'); NoContainer = $true }
fCheck '-BacklogRunQuick hands its areas on, in lower case, before the other options' ($r.Why -eq '' -and $r.Wsl -eq '--backlog-run-quick fmt tests --no-container')
$r = fRun @{ Ci = $true }
fCheck '-Ci says in one line that it is another name' ($r.Said -eq '-Ci is another name for -MergeGate')
fCheck 'and is the merge gate, for the Linux half too' ($MergeGate -and $r.Why -eq '' -and $r.Wsl -eq "$plain --merge-gate")
fCheck 'which never syncs, dogfoods, publishes or prompts, and only checks the format' ($NoSync -and $NoDogfood -and $NoPublish -and $Unattended -and $FmtCheck -and $FullRunName -eq '-MergeGate')
$r = fRun @{ BacklogRunFull = $true; Long = $true }
fCheck '-BacklogRunFull -Long hands on both' ($r.Why -eq '' -and $r.Wsl -eq "$plain --backlog-run-full --long" -and $NoPublish -and $FmtCheck)
$r = fRun @{ Quick = $true }
fCheck 'a plain run neither checks the format only nor drops publish' ($r.Why -eq '' -and -not $FmtCheck -and -not $NoPublish -and $FullRunName -eq '')
$r = fRun @{ NoRemote = $true; NoContainer = $true; KeepTemp = $true; List = $true }
fCheck '-NoRemote, -NoContainer, -KeepTemp and -List each reach the Linux half' ($r.Wsl -eq "$plain --no-remote --no-container --keep-temp --list")
foreach ($case in @(
		@{ Set = @{ Long = $true; Quick = $true }; Want = '-Long runs everything, so not -Quick' }
		@{ Set = @{ Long = $true; Gate = $true }; Want = '-BacklogRunQuick runs the fast checks only, so not -Long' }
		@{ Set = @{ BacklogRunQuick = $true; MergeGate = $true }; Want = '-BacklogRunQuick and -MergeGate are different runs; pick one' }
		@{ Set = @{ BacklogRunFull = $true; Quick = $true }; Want = '-BacklogRunFull runs every check of the default run, so not -Quick' }
		@{ Set = @{ BacklogRunQuick = $true; Leakcheck = $true }; Want = '-Leakcheck adds to a full run; the backlog quick run leaves it out' }
		@{ Set = @{ Areas = @('-Quik') }; Want = 'unknown option: -quik (try -Help)' }
		@{ Set = @{ BacklogRunQuick = $true; Areas = @('lint') }; Want = 'unknown area: lint (areas: fmt clippy tests scripts docs)' })) {
	$r = fRun $case.Set
	fCheck "refused: $($case.Want)" ($r.Why -eq $case.Want)
}

##	-List goes through stage 3's own test calls, and prints each script's ID
##	rather than running it.
$Root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
$listed = [System.Collections.Generic.List[string]]::new()
function fEcho_Clean { param([string]$Msg = ''); $listed.Add($Msg) }
function fEcho { param([string]$Msg = ''); $listed.Add("[ $Msg ]") }
function fDie { param([string]$Msg); throw $Msg }
$List = $true
fScriptTests
$List = $false
$ids = @($listed | ForEach-Object { ($_.Trim() -split '\s+')[0] })
fCheck '-List names every stage 3 test script by its ID' (($ids -join ',') -eq 'dcrgs,dcrz7,dd2n3,dd2n3,dde3v,ddkk4,ddmj2,dehyk')
fCheck 'and runs none of them' (-not @($listed | Where-Object { $_ -match '^(\[ OK:|PASS |FAIL )' }))

##	A run prints one PASS or FAIL line per script, with its ID and name.
$repoRoot = $Root
$Root = Join-Path $Work 'lines'
New-Item -ItemType Directory -Path $Root -Force | Out-Null
Set-Content -LiteralPath (Join-Path $Root 'pass.ps1') -Value "##`tTest ID: zzzzz", 'exit 0'
Set-Content -LiteralPath (Join-Path $Root 'fail.ps1') -Value "##`tTest ID: zzzzz", 'exit 1'
Set-Content -LiteralPath (Join-Path $Root 'noid.ps1') -Value 'exit 0'
$listed.Clear()
fScriptTest -What 'passing one' -Script 'pass.ps1' -InProcess
fCheck 'a script that passes prints its PASS line' (($listed -join '|') -eq 'PASS zzzzz passing one')
$listed.Clear()
fScriptTest -What 'no ID' -Script 'noid.ps1' -InProcess
fCheck 'with dashes for an ID it lacks' (($listed -join '|') -eq 'PASS ----- no ID')
$listed.Clear()
$threw = ''
try { fScriptTest -What 'failing one' -Script 'fail.ps1' -InProcess } catch { $threw = "$_" }
fCheck 'one that fails prints its FAIL line, then stops the run' (($listed -join '|') -eq 'FAIL zzzzz failing one' -and $threw -eq 'failing one failed (exit 1)')
$Root = $repoRoot

##	cargo test's lines go through test-id.py, and its exit status still counts.
##	A stand-in cargo prints one result line of a real test.
if (-not $IsWindows) {
	$bin = Join-Path $Work 'bin'
	New-Item -ItemType Directory -Path $bin -Force | Out-Null
	$fake = Join-Path $bin 'cargo'
	Set-Content -LiteralPath $fake -Value '#!/bin/sh', 'echo "test app::closing::tests::dont_ask_stores_the_answer_given ... ok"', 'exit "${FAKE_CARGO_RC:-0}"'
	& chmod +x $fake
	$pathWas = $env:PATH
	$env:PATH = "$bin$([IO.Path]::PathSeparator)$pathWas"
	function fNote { }
	$got = @(fCargoTest)
	fCheck 'cargo test prints a PASS line with the test''s ID' ($got -contains 'PASS de30g app::closing::tests::dont_ask_stores_the_answer_given')
	$env:FAKE_CARGO_RC = '101'
	$threw = ''
	try { $null = fCargoTest } catch { $threw = "$_" }
	fCheck 'and a failed cargo test still stops the run' ($threw -like 'tests failed (exit 101)*')
	$env:FAKE_CARGO_RC = $null
	$env:PATH = $pathWas
}
$listed.Clear()
$AppName = 'SilkTerm'; $Wsl = $false; $List = $true; $Areas = @('fmt')
fQuickRun
$List = $false
fCheck '-BacklogRunQuick with one area lists it as the one step' ($listed -contains '  Step 1 of 1  Format check')
fCheck 'and the areas not named as off' ($listed -contains '    - Tests (not named)' -and $listed -contains '    - Linux half (WSL2) (-Wsl adds it)')

##	The steps a full run takes, and the ones it turns off with why.
function fArmSkipReason { param($Target); return $null }
function fPlan { param([hashtable]$Set)
	$vars = @{ NoSync = $false; NoFmt = $false; NoArm = $false; Quick = $false; NoPackage = $false; Wsl = $false; NoDogfood = $false; NoPublish = $false }
	foreach ($k in $Set.Keys) { $vars[$k] = $Set[$k] }
	foreach ($k in $vars.Keys) { Set-Variable -Name $k -Value $vars[$k] }
	$script:FullRunName = if ($Set.ContainsKey('FullRunName')) { $Set.FullRunName } else { '' }
	fPlanSteps
}
fPlan @{}
fCheck 'a default run takes every step but the Linux half, in order' (($script:StepsOn -join ',') -eq 'Remote sync,Format,Debug build,Tests,Release build: windows-x86_64-msvc,Release build: windows-x86_64-gnu,Release build: windows-arm64-gnullvm,Release artifacts,Packages,Dogfood,Publish')
fPlan @{ Quick = $true; Wsl = $true }
fCheck '-Quick turns off the ARM64 build and the packages, and says so' (($script:StepsOff -join ',') -eq 'Release build: windows-arm64-gnullvm (-Quick),Packages (-Quick)')
fCheck 'and leaves them out of the count' ($script:StepsOn.Count -eq 10)
fPlan @{ FullRunName = '-MergeGate'; NoSync = $true; NoDogfood = $true; NoPublish = $true }
fCheck 'a full run names itself as what turned a step off' (($script:StepsOff -join ',') -eq 'Remote sync (-MergeGate),Linux half (WSL2) (-Wsl adds it),Dogfood (-MergeGate),Publish (-MergeGate)')
$listed.Clear()
$null = fStepOpen 'Tests'; fEcho 'tested'; fStepClose
$stamp = '\d{8}-\d{6}\.\d{2}'
fCheck 'a step opens after a blank and a 79-wide rule' ($listed[0] -eq '' -and $listed[1] -eq ([string][char]0x2022) * 79)
fCheck 'with its stamp, number and total' ($listed[2] -match "^\[ ${stamp}: Step 3 of 8; Starting Tests \]$")
fCheck 'and closes with PASS' ($listed[4] -match "^\[ ${stamp}: Step 3 of 8; PASS Tests \]$")
fCheck 'a step that is off is false, and prints nothing' (-not (fStepOpen 'Dogfood') -and $listed.Count -eq 6)
$listed.Clear()
$null = fStepOpen 'Format'; fStepClose FAIL
fCheck 'one that failed closes with FAIL' ($listed[3] -match "^\[ ${stamp}: Step 1 of 8; FAIL Format \]$")

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
##		- 20261006 JC: The stash before a pull.
##		- 20261028: The run options and -List.
##		- 20261028: The steps, the ones turned off, and each step's lines.
##		- 20261028: The PASS and FAIL lines of a test script.
##		- 20261101: The run folder goes in the project's folder, and HOST_TEMP is unset for it.
##		- 20261101: The run's folder, its end, the stale sweep and the old ledger.
##		- 20261101: The one cleanup line at the end.
