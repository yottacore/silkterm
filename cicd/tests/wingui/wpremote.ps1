##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A window launched on the console after an RDP logon keeps its wallpaper, and
	so does the next launch after it's turned on.
.DESCRIPTION
	A session logged on over RDP and then moved to the console keeps the
	Explorer it started with, and that Explorer still says SESSIONNAME=RDP-Tcp#N.
	Everything started from the taskbar gets that, and the Remote profile, which
	has no wallpaper, used to read it as a remote screen. Turning the wallpaper
	on, by Settings or by the View menu, lasted until the next launch or the
	next Ctrl+Shift+N window. A new tab kept it, being the same process.
	Each launch here is given the stale name, the way Explorer would give it.
.NOTES
	History: At bottom of file.
#>

##	Test ID: de5bg


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }
if ([Silk.Win]::GetSystemMetrics(0x1000) -ne 0) { fSkip "this is a real remote session, where Remote is right" }

$dir = Join-Path $OutDir "wpremote"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
[void](New-Item -ItemType Directory -Path $dir -Force)

##	A pale picture at full opacity, so a window showing it is mostly lit and one
##	without it mostly dark.
$pic = Join-Path $dir "pale.png"
$bmp = New-Object System.Drawing.Bitmap 64, 64
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(200, 190, 170))
$g.Dispose()
$bmp.Save($pic, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

function fConfig([string]$Name) {
	$sub = Join-Path $dir $name
	[void](New-Item -ItemType Directory -Path $sub -Force)
	$path = Join-Path $sub "config.shcl"
	Set-Content -Path $path -Encoding UTF8 -Value @(
		"performance:", "`tautomatic: false", "`tprofile: `"high`"",
		"window:", "`tremember_per_monitor: false",
		"wallpaper:", "`timage: `"$($pic -replace '\\', '/')`"", "`topacity: 1.0",
		"`trotate:", "`t`tenabled: false")
	$path
}

$stale = @{ SESSIONNAME = "RDP-Tcp#0" }

function fLaunch([string]$Cfg, [hashtable]$EnvVars) {
	$p = fStartSilk -Exe $Exe -SilkArgs @("--config=`"$cfg`"", "--shell", "cmd.exe") -EnvVars $envVars
	$h = fWaitWindow $p 40
	if ($h -ne [IntPtr]::Zero) {
		[void](fFocus $h)
		Start-Sleep -Seconds 5
	}
	@{ p = $p; h = $h }
}

function fShowing([IntPtr]$H, [string]$Name) {
	[void](fFocus $h)
	Start-Sleep -Seconds 2
	$ink = fInk (fShot $h $name)
	fNote "$($name): ink $ink"
	$ink -gt 0.4
}

##	The picture and the threshold are good for something, or every check
##	below means nothing.
$w = fLaunch (fConfig "control") @{}
if (-not (fCheck "the control window came up" ($w.h -ne [IntPtr]::Zero))) { fStop $w.p; return }
$ok = fShowing $w.h "control"
fStop $w.p
if (-not (fCheck "a console launch shows the wallpaper" $ok)) { return }

##	Settings, then a relaunch, a new tab and a new window
$cfg = fConfig "settings"
$w = fLaunch $cfg $stale
if (-not (fCheck "the Settings window came up" ($w.h -ne [IntPtr]::Zero))) { fStop $w.p; return }
$first = fShowing $w.h "settings-launch"
[void](fCheck "a launch given a stale RDP session name shows the wallpaper" $first)
if (-not $first) {
	fPress "ctrl+,"
	$d = fWaitOther -P $w.p -Known $w.h -Seconds 25
	if (fCheck "the dialog opened" ($d -ne [IntPtr]::Zero)) {
		Start-Sleep -Seconds 2
		[void](fFocus $d)
		fPress "ctrl+f"
		fSendChars "Wallpaper"
		fPress "enter"
		fPress "space"
		fPress "enter"
		Start-Sleep -Seconds 2
	}
	[void](fCheck "turned on in Settings, it shows" (fShowing $w.h "settings-on"))
}
fNote "performance.profile in the file: $((fSetting $cfg 'performance.profile').value)"

[void](fFocus $w.h)
fPress "ctrl+shift+t"
Start-Sleep -Seconds 3
[void](fCheck "a new tab keeps it" (fShowing $w.h "settings-tab"))

[void](fFocus $w.h)
fPress "ctrl+shift+n"
$child = $null
for ($i = 0; $i -lt 80 -and -not $child; $i++) {
	$found = Get-CimInstance Win32_Process -Filter "ParentProcessId = $($w.p.Id)" | Where-Object { $_.ExecutablePath -eq $Exe } | Select-Object -First 1
	if ($found) { $child = Get-Process -Id $found.ProcessId -ErrorAction SilentlyContinue }
	if (-not $child) { Start-Sleep -Milliseconds 250 }
}
if (fCheck "Ctrl+Shift+N started a second window" ($null -ne $child)) {
	fTrack $child
	$h2 = fWaitWindow $child 40
	if (fCheck "and it came up" ($h2 -ne [IntPtr]::Zero)) {
		Start-Sleep -Seconds 5
		[void](fCheck "the new window shows it" (fShowing $h2 "settings-newwin"))
	}
	fStop $child
}
fStop $w.p
Start-Sleep -Seconds 1

$w = fLaunch $cfg $stale
if (fCheck "the relaunch came up" ($w.h -ne [IntPtr]::Zero)) {
	[void](fCheck "after Settings, the relaunch shows it" (fShowing $w.h "settings-relaunch"))
}
fStop $w.p

##	The View menu's remote switch, then a relaunch
$cfg = fConfig "menu"
$w = fLaunch $cfg $stale
if (-not (fCheck "the menu window came up" ($w.h -ne [IntPtr]::Zero))) { fStop $w.p; return }
if (-not (fShowing $w.h "menu-launch")) {
	fPress "alt+v"
	Start-Sleep -Milliseconds 600
	fSend "p"
	Start-Sleep -Seconds 2
	[void](fCheck "with Remote turned off from the View menu, it shows" (fShowing $w.h "menu-on"))
}
fStop $w.p
Start-Sleep -Seconds 1
$w = fLaunch $cfg $stale
if (fCheck "the relaunch came up" ($w.h -ne [IntPtr]::Zero)) {
	[void](fCheck "after the menu, the relaunch shows it" (fShowing $w.h "menu-relaunch"))
}
fStop $w.p

##	History:
##		- 20261019 JC: Created.
