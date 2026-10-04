##	Runs one scenario inside the console session and writes a verdict file. Started
##	by an interactive scheduled task, because a process arriving over ssh is in
##	session 0 and has no desktop at all - which is what made every earlier attempt
##	at this look like a permissions problem.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param(
	[Parameter(Mandatory)] [string] $Scenario,
	[Parameter(Mandatory)] [string] $Exe,
	[Parameter(Mandatory)] [string] $OutDir,
	[Parameter(Mandatory)] [string] $RunDir
)

$ErrorActionPreference = "Stop"
##	What the scenario and the app under test write goes in the run's folder too.
$env:SILKTERM_TEST_DIR = $RunDir
$env:TEMP = $RunDir
$env:TMP = $RunDir
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$result = Join-Path $OutDir "result.txt"
$script:startedList = Join-Path $OutDir "started.txt"
$verdict = "fail"
$reason = ""

function fSkip($why) { $script:verdict = "skip"; $script:reason = $why; throw [OperationCanceledException]::new($why) }

try {
	. "$PSScriptRoot\_lib.ps1"
	[void][Silk.Win]::SetProcessDpiAwarenessContext([IntPtr](-4))   ## per-monitor v2
	$script:shotDir = Join-Path $OutDir "shots"

	fNote ("session " + [System.Diagnostics.Process]::GetCurrentProcess().SessionId + " as " + (whoami))
	fNote ("desktop usable: " + (fSessionUsable) + "  session " + (fLockState))
	fNote ("screen " + [Silk.Win]::GetSystemMetrics(0) + "x" + [Silk.Win]::GetSystemMetrics(1) +
	       "  remote-session " + [Silk.Win]::GetSystemMetrics(0x1000))

	if (-not (Test-Path $Exe)) { fSkip "no built binary at $Exe - run the build job first" }

	$script = Join-Path $PSScriptRoot "$Scenario.ps1"
	if (-not (Test-Path $script)) { throw "no such scenario: $Scenario" }
	. $script

	##	A scenario that asserted nothing must not read as a pass. The scroll harness
	##	printed OK for a while after it quietly stopped running any scene, and this
	##	is the same shape of hole.
	$asserted = @($script:checks | Where-Object { $_ -match '^\s+(ok|FAIL) ' }).Count
	if ($asserted -eq 0) { $reason = "the scenario made no checks"; $verdict = "fail" }
	else { $verdict = if ($script:failures -eq 0) { "pass" } else { "fail" } }
}
catch [OperationCanceledException] { }
catch {
	$reason = $_.Exception.Message
	$script:checks += "  FAIL scenario threw: $reason"
	$verdict = "fail"
}
finally {
	& "$PSScriptRoot\_stop.ps1" -List $script:startedList
	@("SCENARIO $Scenario", "VERDICT $verdict $reason") + $script:checks | Set-Content -Path $result -Encoding UTF8
}

##	History:
##		- 20260908 JC: Created.
