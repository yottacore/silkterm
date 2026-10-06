##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	The performance ladder rating itself on real hardware.
.DESCRIPTION
	It has only ever run on a software adapter, where it answers Standard, so
	what a discrete or integrated GPU actually rates has never been seen.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EpJ5h1E


if (-not (fSessionUsable)) { fSkip "console session is locked - the banner cannot be grabbed" }

$cfg = Join-Path $OutDir "perf-config.shcl"
Remove-Item $cfg -ErrorAction SilentlyContinue

$adapter = (Get-CimInstance Win32_VideoController | Select-Object -First 1)
fNote "adapter $($adapter.Name) at $($adapter.CurrentRefreshRate)Hz"
fNote "remote session: $([Silk.Win]::GetSystemMetrics(0x1000))"

$p = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg", "--columns", "110", "--rows", "32") -EnvVars @{ SILK_BENCH = "1" }
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
##	The banner stays up four seconds at least, so the shot is taken two seconds
##	after the window came up, however long focus took. A focus that fails burns
##	nearly four seconds on its own, and a shot after that misses the banner.
$up = Get-Date
$got = fFocus $h
fNote "focus $(if ($got) { 'taken' } else { 'refused' }) in $([int]((Get-Date) - $up).TotalMilliseconds)ms"
$left = 2000 - ((Get-Date) - $up).TotalMilliseconds
if ($left -gt 0) { Start-Sleep -Milliseconds $left }
$during = fShot $h "perf-banner"
Start-Sleep -Seconds 14
$after = fShot $h "perf-settled"
$moved = fDiff $during $after
fNote "banner frame differs from settled by $moved"
[void](fCheck "the banner went up and came down again" ($moved -gt 0.01))

##	Nothing is written until the run answers, so the file appearing with a rating in
##	it IS the result - there is no separate 'did it finish' to ask.
$rated = @{ value = $null }
for ($i = 0; $i -lt 60; $i++) {
	$rated = fSetting $cfg "performance.rated_hardware"
	if ($rated.source -eq "set" -and $rated.value) { break }
	Start-Sleep -Milliseconds 500
}
$chosen = fSetting $cfg "performance.profile"
$auto   = fSetting $cfg "performance.automatic"

[void](fCheck "a rating was written" ($rated.source -eq "set" -and $rated.value))
fNote "rated hardware: $($rated.value)"
fNote "PROFILE CHOSEN: $($chosen.value)   (from the $($chosen.source) line; automatic $($auto.value))"
[void](fCheck "the profile is one of the known rungs" ($chosen.value -in @("max", "high", "low", "standard")))

##	A second launch must accept the stored rating rather than measuring again.
fStop $p
Start-Sleep -Seconds 2
$stamp = (Get-Item $cfg).LastWriteTime
$p2 = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg", "--columns", "110", "--rows", "32") -EnvVars @{}
$h2 = fWaitWindow $p2 40
[void](fCheck "it comes up again on the stored rating" ($h2 -ne [IntPtr]::Zero))
##	A re-rating that finds the same hardware writes the same id, so the id alone
##	passes while the banner shows at every launch. Measured against the first
##	launch's own banner, so no fixed threshold.
Start-Sleep -Seconds 2
$early2 = fShot $h2 "perf-second-early"
Start-Sleep -Seconds 8
$settled2 = fShot $h2 "perf-second-settled"
$moved2 = fDiff $early2 $settled2
fNote "second launch early frame differs from settled by $moved2"
[void](fCheck "the second launch showed no banner" ($moved2 -lt ($moved / 3)))
[void](fCheck "the second launch did not re-rate" ((fSetting $cfg "performance.rated_hardware").value -eq $rated.value))
fNote "config rewritten on second launch: $((Get-Item $cfg).LastWriteTime -ne $stamp)"
fStop $p2

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, named arguments.
