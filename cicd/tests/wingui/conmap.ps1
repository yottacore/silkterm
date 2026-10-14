##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A plain command in a cmd pane keeps the minimap column.
.DESCRIPTION
	The Windows console host asks for focus reports itself as it starts, so
	every pane looks like a program that wants them. The column rule read that
	as a full-screen program for as long as any command ran. A command reads
	its own width a few times, once with the minimap off and once with it on,
	in a window of the same size. With the column there it has fewer columns.
	Builds before the conload fix took a conpty.dll on PATH ahead of the one
	Windows has, and an older one asks for nothing, so PATH goes without one.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EsgiFLt


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed" }

Add-Type -Namespace SilkConMap -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
'@

$dir = Join-Path $OutDir "conmap"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
[void](New-Item -ItemType Directory -Path $dir -Force)

##	PowerShell only to read the width. Neither it nor ping asks for anything.
$probe = Join-Path $dir "widths.ps1"
Set-Content -Path $probe -Encoding ascii -Value @'
param([string]$Out)
foreach ($i in 1..10) {
	$w = $Host.UI.RawUI.WindowSize.Width
	Add-Content -Path $Out -Value $w -Encoding ascii
	Start-Sleep -Milliseconds 300
}
'@

$inbox = ($env:PATH -split ';' | Where-Object { $_ -and -not (Test-Path (Join-Path $_ "conpty.dll")) }) -join ';'
$work = fWorkArea
function fWidths([string]$Minimap) {
	$cfg = Join-Path $dir "map-$Minimap.shcl"
	fFreshConfig $cfg @("cursor:", "`tblink: false", "window:", "`tremember_per_monitor: false",
		"scroll:", "`tminimap:", "`t`tenabled: $Minimap")
	$out = Join-Path $dir "widths-$Minimap.txt"
	$p = fStartSilk -Exe $Exe -SilkArgs @("--config=`"$cfg`"", "--shell", "cmd.exe") -EnvVars @{ PATH = $inbox }
	$h = fWaitWindow $p 40
	if (-not (fCheck "minimap $($Minimap): a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return @() }
	[void][SilkConMap.Win]::MoveWindow($h, $work.x, $work.y, 1000, 560, $true)
	Start-Sleep -Seconds 4
	[void](fFocus $h)
	fSendChars "powershell -NoProfile -ExecutionPolicy Bypass -File `"$probe`" `"$out`""
	fPress "enter"
	$got = @()
	for ($i = 0; $i -lt 60; $i++) {
		Start-Sleep -Milliseconds 500
		if (Test-Path $out) { $got = @(Get-Content $out | Where-Object { $_ -match '^\d+$' } | ForEach-Object { [int]$_ }) }
		if ($got.Count -ge 10) { break }
	}
	[void](fShot $h "conmap-$Minimap")
	fStop $p
	fNote "minimap $($Minimap): $($got -join ' ')"
	$got
}

$off = @(fWidths "false")
$on = @(fWidths "true")
if ((fCheck "both runs read their width 10 times" ($off.Count -ge 10 -and $on.Count -ge 10))) {
	##	the first reading or two may come before the column would have gone
	$offLate = @($off | Select-Object -Last 6 | Sort-Object -Unique)
	$onLate = @($on | Select-Object -Last 6 | Sort-Object -Unique)
	[void](fCheck "the width held still with the minimap off" ($offLate.Count -eq 1))
	[void](fCheck "and with it on" ($onLate.Count -eq 1))
	[void](fCheck "the column kept its room while the command ran" ($onLate[-1] -lt $offLate[0]))
}

##	History:
##		- 20261014 JC: Created.
