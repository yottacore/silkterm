##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Makes the run's folder on the box and prints it, as RUNFOR, RUNDIR and
	RUNTOKEN lines. Prints a NOSESSION line instead when nobody is logged on
	at the console, since there is no desktop to run a scenario on.
.DESCRIPTION
	run.bash removes the folder with the token once the run is over, so this
	never calls fTestDir_End. Sent over ssh by run.bash with _testdir.ps1 ahead
	of it. The scenario runs as whoever holds the console, and a temp folder is
	private to its account, so the folder goes in that user's temp folder, not
	the ssh account's.
.NOTES
	History: At bottom of file.
#>


##	The registry keeps TEMP unexpanded, and %USERPROFILE% here would be the ssh
##	account's own.
function fUserTemp([string] $Raw, [string] $UserProfile) {
	if (-not $Raw) { $Raw = '%USERPROFILE%\AppData\Local\Temp' }
	$Raw = $Raw.Replace('%USERPROFILE%', $UserProfile, [StringComparison]::OrdinalIgnoreCase)
	$Raw = $Raw.Replace('%LOCALAPPDATA%', "$UserProfile\AppData\Local", [StringComparison]::OrdinalIgnoreCase)
	[Environment]::ExpandEnvironmentVariables($Raw)
}

##	The user on the console session in quser's lines, or nothing.
function fConsoleUserIn([string[]] $Lines, [uint32] $ConsoleId) {
	foreach ($l in ($Lines | Select-Object -Skip 1)) {
		if ($l -match '^\s*>?(\S+)\s+.*?(\d+)\s+(Active|Disc)\b' -and [int]$Matches[2] -eq $ConsoleId) { return $Matches[1] }
	}
}

##	Nothing when nobody is logged on there, as at the logon screen of a VM just
##	started. This used to fall back to the ssh account, and a task for a user
##	with no session never runs, so the scenario waited out its 4 minutes.
function fConsoleUser {
	if (-not ('Con.W' -as [type])) {
		Add-Type -Namespace Con -Name W -MemberDefinition @"
[DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
"@
	}
	fConsoleUserIn @(quser 2>$null) ([Con.W]::WTSGetActiveConsoleSessionId())
}

function fLogsOnByItself {
	try { (Get-ItemPropertyValue 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' -Name AutoAdminLogon -ErrorAction Stop) -eq '1' }
	catch { $false }
}

##	A box that logs a user on by itself gets time to, since ssh can answer
##	before that's done. One that doesn't would only wait for nothing.
function fWaitConsoleUser([int] $Seconds = 120, [int] $Poll = 5) {
	$who = fConsoleUser
	if ($who -or -not (fLogsOnByItself)) { return $who }
	$until = [DateTime]::UtcNow.AddSeconds($Seconds)
	while (-not $who -and [DateTime]::UtcNow -lt $until) {
		Start-Sleep -Seconds $Poll
		$who = fConsoleUser
	}
	$who
}

function fStage {
	$ErrorActionPreference = "Stop"
	$who = fWaitConsoleUser
	if (-not $who) {
		if (fLogsOnByItself) { "NOSESSION nobody logged on at the console within 120 s" }
		else { "NOSESSION nobody is logged on at the console" }
		return
	}
	$sid = [Security.Principal.NTAccount]::new("$env:COMPUTERNAME\$who").Translate([Security.Principal.SecurityIdentifier]).Value
	$userProfile = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$sid").ProfileImagePath
	##	The user is logged on, so their hive is loaded.
	$key = [Microsoft.Win32.Registry]::Users.OpenSubKey("$sid\Environment")
	$raw = if ($key) { $key.GetValue('TEMP', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } else { $null }
	$env:TEMP = fUserTemp $raw $userProfile
	$env:TMP = $env:TEMP
	Remove-Item env:SILKTERM_TEST_DIR -ErrorAction SilentlyContinue
	fTestDir_Make
	##	Made by this account, written by that one. Inheritance usually gives them
	##	the folder already; this does not depend on it.
	icacls $env:SILKTERM_TEST_DIR /grant "*${sid}:(OI)(CI)M" /Q | Out-Null
	if ($LASTEXITCODE) { throw "cannot give $who the run folder $env:SILKTERM_TEST_DIR" }
	"RUNFOR $who"
	"RUNDIR $env:SILKTERM_TEST_DIR"
	"RUNTOKEN $script:TestDirToken"
}

Set-StrictMode -Version Latest
fStage

##	History:
##		- 20261002 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
##		- 20261025: NOSESSION when nobody is logged on at the console, after a
##		  wait on a box that logs on by itself.
