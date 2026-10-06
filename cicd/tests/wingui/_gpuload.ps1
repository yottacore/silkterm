##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Body of the gpuload scenarios.
.DESCRIPTION
	A window is watched while another program keeps the GPU busy and nearly
	full: typed into, minimized and restored, moved off to another virtual
	desktop and back, and with $idleRelease let go idle so its device is
	dropped, then woken. Then the load ends and the window is left alone, to see
	whether it paints with no input. Needs gpu-stress.exe in the run's folder
	(run.bash WINGUI_EXTRA), and a build-tag.txt there naming the build, which
	goes into every shot's name. The checks only prove the steps ran. Whether it
	painted is read off the shots.
.NOTES
	History: At bottom of file.
#>


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

$stress = Join-Path $RunDir "gpu-stress.exe"
if (-not (Test-Path $stress)) { fSkip "no gpu-stress.exe in the run folder" }
$tagFile = Join-Path $RunDir "build-tag.txt"
$build = if (Test-Path $tagFile) { (Get-Content $tagFile -Raw).Trim() } else { "build" }
$arm = "$build-" + $(if ($idleRelease) { "idleon" } else { "idleoff" })
##	gpuload-args.txt in the run's folder replaces the load's arguments.
$argFile = Join-Path $RunDir "gpuload-args.txt"
$loadArgs = if (Test-Path $argFile) { (Get-Content $argFile -Raw).Trim() } else { "--vram-mb 4600 --touch --busy 1 --slice-ms 40" }
$script:vks.left = 0x25
$script:vks.right = 0x27

Add-Type -Namespace SilkGpu -Name Win -MemberDefinition @'
[DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int n);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
'@
function fCloaked([IntPtr]$H) { $v = 0; [void][SilkGpu.Win]::DwmGetWindowAttribute($h, 14, [ref]$v, 4); $v -ne 0 }
##	Restored without focus, and raised over the scenario's own console window,
##	which would otherwise cover it in the shots.
function fShowNoFocus([IntPtr]$H) {
	[void][Silk.Win]::ShowWindow($h, 4)   ## SW_SHOWNOACTIVATE
	$flags = 0x0001 -bor 0x0002 -bor 0x0010   ## no size, no move, no activate
	[void][SilkGpu.Win]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, $flags)
	[void][SilkGpu.Win]::SetWindowPos($h, [IntPtr](-2), 0, 0, 0, 0, $flags)
}

$script:t0 = Get-Date
function fElapsed { "{0,6:N1}s" -f ((Get-Date) - $script:t0).TotalSeconds }
function fStep([string]$Text) { fNote "$(fElapsed) $text" }

function fSmi {
	$smi = "C:\Windows\System32\nvidia-smi.exe"
	if (-not (Test-Path $smi)) { return "no nvidia-smi" }
	(& $smi --query-gpu=memory.used,memory.total,utilization.gpu --format=csv,noheader) -join " "
}

##	A shot of the window's place on screen, with how much of it moved since the
##	one before and since the last painted reference.
$script:prev = $null
$script:ref = $null
function fGrab([string]$Name) {
	$bmp = fShot $h "gl-$arm-$name"
	$moved = fDiff $script:prev $bmp
	$vsRef = fDiff $script:ref $bmp
	fStep "shot ${name}: ink $(fInk $bmp), moved $moved, vs painted ref $vsRef"
	$script:prev = $bmp
	$bmp
}

##	cls and then a screenful of numbered lines, so a painted window changes a
##	lot and a blank one not at all. The shell writes a file as well, which says
##	the keys got through whatever the picture shows.
$script:typed = 0
function fType([string]$Label) {
	$script:typed++
	$proof = Join-Path $OutDir "typed-$arm-$($script:typed).txt"
	[void](fFocus $h)
	fSend "cls"
	fPress "enter"
	Start-Sleep -Milliseconds 700
	fSend "for /L %i in (1,1,60) do @echo $label-$($script:typed)-%i"
	fPress "enter"
	fSend "echo $label > $proof"
	fPress "enter"
	Start-Sleep -Seconds 3
	$got = Test-Path $proof
	fStep "typed $label, the shell ran it: $got"
	$got
}

$cfgDir = Join-Path $RunDir "cfg-$arm"
New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
$cfg = Join-Path $cfgDir "config.shcl"
##	gpuload-config.txt in the run's folder adds its lines to the config, with
##	tabs written as \t.
$extraCfg = Join-Path $RunDir "gpuload-config.txt"
$more = if (Test-Path $extraCfg) { @(Get-Content $extraCfg | ForEach-Object { $_.Replace('\t', "`t") }) } else { @() }
fFreshConfig $cfg (@("window:", "`tidle_release: $(if ($idleRelease) { 'true' } else { 'false' })") + $more)
$err = Join-Path $script:shotDir "idledbg-$arm.txt"
New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null

$envVars = @{ SILK_IDLEDBG = "1"; XDG_CONFIG_HOME = $cfgDir }
if ($idleRelease) { $envVars.SILK_IDLE_SECS = "5" }
foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $envVars[$k]) }
$p = Start-Process $Exe -ArgumentList @("--config=$cfg", "--columns", "100", "--rows", "30", "--shell=cmd.exe") -RedirectStandardError $err -PassThru
foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $null) }
fTrack $p
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
Start-Sleep -Seconds 3
$r = fRect $h
fStep "window $($r.w)x$($r.h) at $($r.x),$($r.y), gpu $(fSmi)"

[void](fCheck "typing reached the shell before the load" (fType "base"))
$script:ref = fGrab "base"

$stop = Join-Path $RunDir "gpu-stress-$arm.stop"
Remove-Item $stop -ErrorAction SilentlyContinue
$loadLog = Join-Path $script:shotDir "gpu-stress-$arm.txt"
$load = Start-Process $stress -ArgumentList (($loadArgs -split ' ') + @("--secs", "600", "--stop-file", $stop)) -WindowStyle Hidden -RedirectStandardOutput $loadLog -PassThru
fTrack $load
Start-Sleep -Seconds 6
fStep "load started ($loadArgs), gpu $(fSmi)"
fNote "load says: $((Get-Content $loadLog -ErrorAction SilentlyContinue | Select-Object -Last 1))"
[void](fCheck "the load is running" (-not $load.HasExited))

##	On screen, under load.
[void](fGrab "load-onscreen-idle")
[void](fCheck "typing reached the shell under load" (fType "load"))
[void](fGrab "load-onscreen-typed")

##	Minimize and restore under load.
[void][Silk.Win]::ShowWindow($h, 6)
Start-Sleep -Seconds 3
fStep "minimized: $([SilkGpu.Win]::IsIconic($h))"
[void][Silk.Win]::ShowWindow($h, 9)
[void](fFocus $h)
Start-Sleep -Seconds 2
[void](fGrab "restored")
[void](fType "restored")
[void](fGrab "restored-typed")

##	Another virtual desktop and back. Win+Ctrl+D makes one and moves to it.
fPress "ctrl+win+d"
Start-Sleep -Seconds 3
fStep "on the new desktop, window cloaked: $(fCloaked $h)"
[void](fCheck "the window left with its desktop" (fCloaked $h))
fPress "ctrl+win+left"
Start-Sleep -Seconds 3
fStep "back, window cloaked: $(fCloaked $h)"
[void](fGrab "vd-back")
##	The extra desktop goes again: over to it and close it, which lands back here.
fPress "ctrl+win+right"
Start-Sleep -Seconds 1
fPress "ctrl+win+f4"
Start-Sleep -Seconds 3
fStep "extra desktop closed, window cloaked: $(fCloaked $h)"
[void](fGrab "vd-closed")

if ($idleRelease) {
	##	Minimized past SILK_IDLE_SECS, so the device is let go under load, then
	##	woken by the restore.
	[void][Silk.Win]::ShowWindow($h, 6)
	Start-Sleep -Seconds 12
	fStep "minimized for 12 s"
	[void][Silk.Win]::ShowWindow($h, 9)
	[void](fFocus $h)
	Start-Sleep -Seconds 3
	[void](fGrab "idle-woken")
	[void](fType "woken")
	[void](fGrab "idle-woken-typed")
	##	And once more with the window left minimized, restored without focus,
	##	so nothing but the restore asks for the device.
	[void][Silk.Win]::ShowWindow($h, 6)
	Start-Sleep -Seconds 12
	fShowNoFocus $h
	Start-Sleep -Seconds 3
	fStep "restored without focus after 12 s minimized"
	[void](fGrab "idle-woken-nofocus")
}

##	The case the bug is about: the load ends and nobody touches the window.
##	It is moved off screen and back first, without focus or input, the closest
##	there is to coming back from another desktop with the load still on.
[void][Silk.Win]::ShowWindow($h, 6)
Start-Sleep -Seconds 2
fShowNoFocus $h
Start-Sleep -Seconds 3
[void](fGrab "before-end")
Set-Content -Path $stop -Value "stop"
for ($i = 0; $i -lt 40 -and -not $load.HasExited; $i++) { Start-Sleep -Milliseconds 250 }
if (-not $load.HasExited) { fStop $load; fNote "the load had to be ended" }
fStep "load ended, gpu $(fSmi)"
Start-Sleep -Seconds 3
[void](fGrab "after-end-3s")
Start-Sleep -Seconds 7
[void](fGrab "after-end-10s")
[void](fCheck "typing reached the shell after the load" (fType "after"))
[void](fGrab "after-typed")

fStop $p
Start-Sleep -Milliseconds 500
foreach ($line in (Get-Content $err -ErrorAction SilentlyContinue | Select-Object -Last 200)) { fNote "dbg $line" }

##	History:
##		- 20261003 JC: Created.
##		- 20261006 JC: Help block, typed parameters; the painted reference starts empty.
