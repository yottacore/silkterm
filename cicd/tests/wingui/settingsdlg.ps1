##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	The Settings dialog is a second window with its own GPU context, and winit's
	parenting on Windows makes it a child rather than an owned window - so
	nothing about it follows from the main window working.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EpJAgKe


if (-not (fSessionUsable)) { fSkip "console session is locked - the dialog cannot be grabbed" }

$cfg = Join-Path $OutDir "dlg-config.shcl"
fFreshConfig $cfg

$p = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg", "--columns", "110", "--rows", "32") -EnvVars @{}
$h = fWaitWindow $p 40
if (-not (fCheck "the terminal came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
[void](fCheck "the terminal takes the foreground" (fFocus $h))
Start-Sleep -Seconds 4

fPress "ctrl+,"
$d = fWaitOther -P $p -Known $h -Seconds 25
if (-not (fCheck "ctrl+comma opens the dialog" ($d -ne [IntPtr]::Zero))) { [void](fShot $h "dlg-none"); fStop $p; return }

Start-Sleep -Seconds 2
$r = fRect $d
fNote "dialog $($r.w)x$($r.h) at $($r.x),$($r.y)"
$first = fShot $d "dlg-tab-first"
fNote "capture via $(if ($first) { $first.How }), ink $(fInk $first)"
[void](fCheck "the dialog drew something" ((fInk $first) -gt 0.05))

##	A child window is clipped to its parent. The dialog is taller than it is wide,
##	so a clipped one shows up as a short window rather than a missing one.
[void](fCheck "the dialog is not clipped to its parent" ($r.h -gt $r.w))

##	It also has to fit where the user can reach it. The buttons are along the
##	bottom, so a dialog taller than the usable screen puts OK under the taskbar.
$area = fWorkArea
if ($area) {
	fNote "work area $($area.w)x$($area.h) at $($area.x),$($area.y)"
	$fits = ($r.h -le $area.h) -and ($r.w -le $area.w) -and
	        ($r.y -ge $area.y) -and (($r.y + $r.h) -le ($area.y + $area.h))
	[void](fCheck "the dialog fits on the usable screen" $fits)
}

[void](fFocus $d)
fPress "ctrl+tab"
Start-Sleep -Seconds 1
$second = fShot $d "dlg-tab-second"
fNote "the panel changed across a tab by $(fDiff $first $second)"
[void](fCheck "ctrl+tab moves to another tab" ((fDiff $first $second) -gt 0.02))

fPress "escape"
Start-Sleep -Seconds 2
[void](fCheck "escape closes the dialog" (-not ([SilkEnum]::All([uint32]$p.Id) -contains $d)))
fStop $p

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, named arguments.
