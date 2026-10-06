##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Proves the rig itself: a window comes up on the real adapter, it draws, it
	takes the foreground, and what is typed reaches the shell inside it.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EpJ4XDc


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

$cfg = Join-Path $OutDir "smoke-config.shcl"
fFreshConfig $cfg

$p = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg", "--columns", "100", "--rows", "30") -EnvVars @{}
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }

Start-Sleep -Seconds 3
$r = fRect $h
fNote "window $($r.w)x$($r.h) at $($r.x),$($r.y)"
$idle = fShot $h "smoke-idle"
fNote "capture via $(if ($idle) { $idle.How }), ink $(fInk $idle)"
[void](fCheck "the window is a real size" ($r.w -gt 400 -and $r.h -gt 200))
[void](fCheck "the window actually drew" ((fInk $idle) -gt 0.02))

$got = fFocus $h
fNote "foreground: $(fForeground)"
[void](fCheck "it takes the foreground" $got)

##	Have the shell write a file rather than watching for the picture to change. A
##	line of text moves a few tenths of a percent of the pixels, which is the same
##	order as a blinking cursor - so a pixel diff cannot tell typing from noise.
$proof = Join-Path $OutDir "typed.txt"
Remove-Item $proof -ErrorAction SilentlyContinue
$before = fShot $h "smoke-before"
fSend "echo silkrig-was-here > $proof"
fPress "enter"
Start-Sleep -Seconds 3
$after = fShot $h "smoke-after"
fNote "picture moved $(fDiff $before $after)"
$said = if (Test-Path $proof) { (Get-Content $proof -Raw) } else { "" }
[void](fCheck "typing reached the shell and it ran the line" ($said -match "silkrig-was-here"))

fStop $p

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, named arguments.
