##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Put a user's session on the console, so graphical scenarios run against the
	real adapter rather than a remote one.
.DESCRIPTION
	This does NOT unlock anything. A session that locked itself has to be
	authenticated again, and the way to do that from elsewhere is to connect to it
	once over RDP - reconnecting is a logon, and a logon unlocks. Moving it back
	here afterwards is what drops the remote flag the app reads.
.PARAMETER User
	The account whose session moves. Default: the unprivileged test account.
.NOTES
	History: At bottom of file.
#>

[CmdletBinding()]
param([string] $User = "wintest")

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"
$dir = "C:\ProgramData\silkrig"
New-Item -ItemType Directory -Force -Path $dir | Out-Null

##	quser's columns shift when a session has no name, so read the user off the
##	front and the id off the run of numbers, rather than by position.
function fSessions {
	foreach ($l in (quser 2>$null | Select-Object -Skip 1)) {
		if ($l -match '^\s*>?(\S+)\s+.*?(\d+)\s+(Active|Disc)\b') {
			[pscustomobject]@{ User = $Matches[1]; Id = [int]$Matches[2]; State = $Matches[3] }
		}
	}
}

"before:"; fSessions | ForEach-Object { "  $($_.User) id=$($_.Id) $($_.State)" }
$want = fSessions | Where-Object { $_.User -eq $User } | Select-Object -First 1
if (-not $want) { "no session for ${User} - log in once (RDP is enough) and run this again"; exit 1 }

##	tscon wants SeTcbPrivilege, which an ssh session does not always carry, so it
##	goes through a one-shot SYSTEM task. The task must point at a FILE: schtasks
##	given nested quotes runs the command inline instead of registering it.
$script = Join-Path $dir "tscon.ps1"
@("tscon $($want.Id) /dest:console", "Start-Sleep -Seconds 2", "'done' | Set-Content '$dir\tscon.done'") |
	Set-Content -Path $script -Encoding UTF8
Remove-Item "$dir\tscon.done" -ErrorAction SilentlyContinue

$act = New-ScheduledTaskAction -Execute (Get-Command pwsh).Source `
	-Argument "-NoProfile -ExecutionPolicy Bypass -File `"$script`""
$pri = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
try {
	Register-ScheduledTask -TaskName 'silkrig-tscon' -Action $act -Principal $pri -Force | Out-Null
	Start-ScheduledTask -TaskName 'silkrig-tscon'
	for ($i = 0; $i -lt 40; $i++) { if (Test-Path "$dir\tscon.done") { break }; Start-Sleep -Milliseconds 500 }
} finally {
	Unregister-ScheduledTask -TaskName 'silkrig-tscon' -Confirm:$false -ErrorAction SilentlyContinue
}

Start-Sleep -Seconds 3
"after:"; fSessions | ForEach-Object { "  $($_.User) id=$($_.Id) $($_.State)" }

##	A lock screen process says nothing either way - LockApp stays around, suspended,
##	both after an unlock and while locked with the display asleep. The session's own
##	flags are the answer: offset 16 of WTSSessionInfoEx, 0 locked, 1 unlocked.
Add-Type -Namespace Con -Name Wts -MemberDefinition @"
[DllImport("wtsapi32.dll")] public static extern bool WTSQuerySessionInformationW(IntPtr srv, int id, int cls, out IntPtr buf, out int bytes);
[DllImport("wtsapi32.dll")] public static extern void WTSFreeMemory(IntPtr p);
"@
$buf = [IntPtr]::Zero; $n = 0
if ([Con.Wts]::WTSQuerySessionInformationW([IntPtr]::Zero, $want.Id, 25, [ref]$buf, [ref]$n)) {
	$flags = [Runtime.InteropServices.Marshal]::ReadInt32($buf, 16)
	[Con.Wts]::WTSFreeMemory($buf)
	if ($flags -eq 0) { "session $($want.Id) is still locked; connect once over RDP, or scenarios will skip"; exit 1 }
	if ($flags -eq 1) { "session $($want.Id) is unlocked" }
}

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
