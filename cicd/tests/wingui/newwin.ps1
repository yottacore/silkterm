##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A window opened with Ctrl+Shift+N opens at the size of the one it came from,
	and a launch nobody resized saves no size.
.DESCRIPTION
	On macOS a stale resize from the window's creation once saved the default
	window's grid on every launch, so the next window opened at that. A
	minimized window has no area, and that is no size to save either.
.NOTES
	History: At bottom of file.
#>

##	Test ID: ErsO6KS


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

##	A folder of its own, since the state file sits beside the config. The
##	launch moves the size there.
$dir = Join-Path $OutDir "newwin"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
[void](New-Item -ItemType Directory -Path $dir -Force)
$cfg = Join-Path $dir "config.shcl"
$state = Join-Path $dir "state.shcl"
fFreshConfig $cfg @("window:", "`tremembered_columns: 100", "`tremembered_rows: 30")
function fKept { "$((fSetting $state 'window.remembered_columns').value)x$((fSetting $state 'window.remembered_rows').value)" }

$p = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg") -EnvVars @{}
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
Start-Sleep -Seconds 3
$a = fRect $h
fNote "first window $($a.w)x$($a.h), file says $(fKept)"
[void](fCheck "the launch saved no size" ((fKept) -eq "100x30"))

[void](fFocus $h)
fPress "ctrl+shift+n"
$child = $null
for ($i = 0; $i -lt 80 -and -not $child; $i++) {
	$found = Get-CimInstance Win32_Process -Filter "ParentProcessId = $($p.Id)" | Where-Object { $_.ExecutablePath -eq $Exe } | Select-Object -First 1
	if ($found) { $child = Get-Process -Id $found.ProcessId -ErrorAction SilentlyContinue }
	if (-not $child) { Start-Sleep -Milliseconds 250 }
}
if (fCheck "Ctrl+Shift+N started a second window" ($null -ne $child)) {
	fTrack $child
	$h2 = fWaitWindow $child 40
	if (fCheck "and it came up" ($h2 -ne [IntPtr]::Zero)) {
		Start-Sleep -Seconds 3
		$b = fRect $h2
		fNote "second window $($b.w)x$($b.h), file says $(fKept)"
		[void](fCheck "it is the first one's size" ($b.w -eq $a.w -and $b.h -eq $a.h))
		[void](fCheck "and still nothing was saved" ((fKept) -eq "100x30"))
	}
	fStop $child
}

[void][Silk.Win]::ShowWindow($h, 6)          ## SW_MINIMIZE
Start-Sleep -Seconds 3
fNote "minimized, file says $(fKept)"
[void](fCheck "a minimized window saves no size" ((fKept) -eq "100x30"))
[void][Silk.Win]::ShowWindow($h, 9)          ## SW_RESTORE
Start-Sleep -Seconds 2
$back = fRect $h
[void](fCheck "and comes back at its size" ($back.w -eq $a.w -and $back.h -eq $a.h))
fStop $p

##	History:
##		- 20261005 JC: Created.
##		- 20261006 JC: Help block, named arguments.
##		- 20261009 JC: The size is read from the state file.
