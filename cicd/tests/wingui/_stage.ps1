##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Makes the run's folder on the box and prints it, as RUNFOR, RUNDIR and
	RUNTOKEN lines.
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

function fConsoleUser {
	Add-Type -Namespace Con -Name W -MemberDefinition @"
[DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
"@
	$consoleId = [Con.W]::WTSGetActiveConsoleSessionId()
	$who = $env:USERNAME
	foreach ($l in (quser 2>$null | Select-Object -Skip 1)) {
		if ($l -match '^\s*>?(\S+)\s+.*?(\d+)\s+(Active|Disc)\b' -and [int]$Matches[2] -eq $consoleId) { $who = $Matches[1] }
	}
	$who
}

function fStage {
	$ErrorActionPreference = "Stop"
	$who = fConsoleUser
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
