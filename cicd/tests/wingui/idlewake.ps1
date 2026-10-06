##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Free resources when idle, with a short idle time, on a window left alone.
.DESCRIPTION
	Each arm, Transparency on and then off: the window is let go while minimized
	and restored without focus, then left in view unfocused past the idle time,
	then let go minimized again and restored with focus. What it shows after
	each is held against what it showed before. A restore can catch the window
	with no size yet, which a busy GPU makes likelier. idlewake-load.txt in the
	run's folder starts gpu-stress.exe (run.bash WINGUI_EXTRA) with its
	arguments for each arm. Not in the pipeline: about three minutes, and the
	load is sent by hand.
.NOTES
	History: At bottom of file.
#>

##	Test ID: Erkt9mD


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

Add-Type -Namespace SilkIdle -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
'@
##	Restored without focus, and raised over the scenario's own console window,
##	which would otherwise cover it in the shots.
function fShowNoFocus([IntPtr]$H) {
	[void][Silk.Win]::ShowWindow($h, 4)   ## SW_SHOWNOACTIVATE
	$flags = 0x0001 -bor 0x0002 -bor 0x0010   ## no size, no move, no activate
	[void][SilkIdle.Win]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, $flags)
	[void][SilkIdle.Win]::SetWindowPos($h, [IntPtr](-2), 0, 0, 0, 0, $flags)
}

$script:t0 = Get-Date
function fElapsed { "{0,6:N1}s" -f ((Get-Date) - $script:t0).TotalSeconds }
function fStep([string]$Text) { fNote "$(fElapsed) $text" }

##	The text area only. The title says how the device is doing, and the frame
##	and the menu bar's boxes change with focus.
function fBody([System.Drawing.Bitmap]$Bmp) {
	if (-not $bmp) { return $null }
	$top = 80; $edge = 12
	if ($bmp.Width -le 2 * $edge -or $bmp.Height -le $top + $edge) { return $bmp }
	$area = New-Object System.Drawing.Rectangle $edge, $top, ($bmp.Width - 2 * $edge), ($bmp.Height - $top - $edge)
	$bmp.Clone($area, $bmp.PixelFormat)
}

function fRef([string]$Name) {
	$script:ref = fBody (fShot $h "idle-$arm-$name")
	fStep "shot ${name}: ink $(fInk $script:ref 6)"
}

##	How much of the picture moved since the reference. A parked cursor is all
##	that should differ.
function fGrab([string]$Name) {
	$bmp = fBody (fShot $h "idle-$arm-$name")
	$moved = fDiff -A $script:ref -B $bmp -Step 6
	fStep "shot ${name}: ink $(fInk $bmp 6), vs ref $moved"
	$moved
}
$same = 0.02

##	A device taken back decodes the wallpaper again, so a restore is given a
##	few seconds to settle. Text that went missing never does.
function fSettled([string]$Name) {
	$moved = 1.0
	for ($i = 0; $i -lt 6 -and $moved -ge $same; $i++) {
		Start-Sleep -Seconds 1
		$moved = fGrab $name
	}
	$moved
}

$script:typed = 0
function fType([string]$Label) {
	$script:typed++
	$proof = Join-Path $OutDir "typed-$arm-$($script:typed).txt"
	[void](fFocus $h)
	fSend "cls"
	fPress "enter"
	Start-Sleep -Milliseconds 700
	fSend "for /L %i in (1,1,40) do @echo $label-$($script:typed)-%i"
	fPress "enter"
	fSend "echo $label > $proof"
	fPress "enter"
	Start-Sleep -Seconds 2
	Test-Path $proof
}

$stress = Join-Path $RunDir "gpu-stress.exe"
$loadFile = Join-Path $RunDir "idlewake-load.txt"
$loadArgs = if ((Test-Path $stress) -and (Test-Path $loadFile)) { (Get-Content $loadFile -Raw).Trim() } else { "" }

foreach ($arm in @("seethru", "opaque")) {
	$load = $null
	if ($loadArgs) {
		$stop = Join-Path $RunDir "gpu-stress-$arm.stop"
		$loadLog = Join-Path $script:shotDir "gpu-stress-$arm.txt"
		New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null
		$load = Start-Process $stress -ArgumentList (($loadArgs -split ' ') + @("--secs", "150", "--stop-file", $stop)) -WindowStyle Hidden -RedirectStandardOutput $loadLog -PassThru
		fTrack $load
		Start-Sleep -Seconds 4
		fStep "load started ($loadArgs): $((Get-Content $loadLog -ErrorAction SilentlyContinue | Select-Object -Last 1))"
	}
	$cfgDir = Join-Path $RunDir "cfg-$arm"
	New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
	$cfg = Join-Path $cfgDir "config.shcl"
	fFreshConfig $cfg @("window:", "`tidle_release: true", "transparency:", "`tenabled: $(if ($arm -eq 'seethru') { 'true' } else { 'false' })")
	New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null
	$err = Join-Path $script:shotDir "idledbg-$arm.txt"
	$envVars = @{ SILK_IDLEDBG = "1"; SILK_IDLE_SECS = "3"; XDG_CONFIG_HOME = $cfgDir }
	foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $envVars[$k]) }
	$p = Start-Process $Exe -ArgumentList @("--config=$cfg", "--columns", "100", "--rows", "30", "--shell=cmd.exe") -RedirectStandardError $err -PassThru
	foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $null) }
	fTrack $p
	$h = fWaitWindow $p 40
	if (-not (fCheck "$arm a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; continue }
	Start-Sleep -Seconds 2
	[void](fCheck "$arm typing reached the shell" (fType "base"))
	fRef "ref"

	##	Let go while minimized, restored without focus.
	foreach ($round in 1..3) {
		[void][Silk.Win]::ShowWindow($h, 6)   ## SW_MINIMIZE
		Start-Sleep -Seconds 6
		fStep "minimized 6 s: $([SilkIdle.Win]::IsIconic($h))"
		fShowNoFocus $h
		[void](fCheck "$arm restored without focus, round ${round}, shows what it did" ((fSettled "nofocus-$round") -lt $same))
	}

	##	In view and unfocused, past the idle time.
	Start-Sleep -Seconds 4
	[void](fGrab "inview-4")
	Start-Sleep -Seconds 11
	[void](fCheck "$arm left in view keeps its picture" ((fGrab "inview-15") -lt $same))

	##	Let go while minimized, restored with focus.
	[void](fType "two")
	fRef "ref2"
	[void][Silk.Win]::ShowWindow($h, 6)
	Start-Sleep -Seconds 6
	[void](fFocus $h)
	[void](fCheck "$arm restored with focus shows what it did" ((fSettled "focus") -lt $same))

	fStop $p
	if ($load) {
		Set-Content -Path $stop -Value "stop"
		for ($i = 0; $i -lt 40 -and -not $load.HasExited; $i++) { Start-Sleep -Milliseconds 250 }
		fStop $load
	}
	Start-Sleep -Milliseconds 500
	foreach ($line in (Get-Content $err -ErrorAction SilentlyContinue | Select-Object -Last 250)) { fNote "dbg $line" }
}

##	History:
##		- 20261004 JC: Created.
##		- 20261006 JC: Help block, typed parameters.
