##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A tab names a command that prints nothing, with the cursor not blinking.
.DESCRIPTION
	The frame drawn as Enter goes in comes just before the command starts, and
	what it saw was kept. With no output and no blink, no later frame asked
	again, so the tab never named the command. PowerShell runs ping with its
	output thrown away, which prints nothing for a few seconds. PowerShell sets
	no title per command, where cmd does, and that title is output.
.NOTES
	History: At bottom of file.
#>

##	Test ID: ddydp


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed" }

Add-Type -Namespace SilkQuietTab -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
'@

function fTitle([IntPtr]$H) {
	$sb = New-Object System.Text.StringBuilder 512
	[void][SilkQuietTab.Win]::GetWindowTextW($h, $sb, 512)
	$sb.ToString()
}

$dir = Join-Path $OutDir "quiettab"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
[void](New-Item -ItemType Directory -Path $dir -Force)

$cfg = Join-Path $dir "config.shcl"
fFreshConfig $cfg @("cursor:", "`tblink: false", "window:", "`tremember_per_monitor: false", "wallpaper:", "`tenabled: false")
$p = fStartSilk -Exe $Exe -SilkArgs @("--config=`"$cfg`"", "--shell", "`"powershell.exe -NoLogo -NoProfile`"") -EnvVars @{}
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
$work = fWorkArea
[void][SilkQuietTab.Win]::MoveWindow($h, $work.x, $work.y, 1000, 560, $true)
Start-Sleep -Seconds 5
[void](fFocus $h)
foreach ($round in 1..2) {
	fSendChars "ping -n 6 127.0.0.1 > `$null"
	fPress "enter"
	$seen = @()
	for ($i = 0; $i -lt 20; $i++) {
		Start-Sleep -Milliseconds 200
		$seen += fTitle $h
	}
	$seen = @($seen | Select-Object -Unique)
	foreach ($t in $seen) { fNote "round $($round): '$t'" }
	[void](fCheck "round $($round): the tab names ping while it runs" (@($seen | Where-Object { $_ -match '\[ping\]' }).Count -gt 0))
	##	let it end, and the prompt come back
	Start-Sleep -Seconds 3
}
[void](fShot $h "quiettab")
fStop $p

##	History:
##		- 20261014 JC: Created.
