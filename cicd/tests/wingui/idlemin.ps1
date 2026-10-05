##	"Minutes when minimized" on a real Windows desktop, at its shipped 1 minute,
##	with the other two waits left at theirs. Each arm, Transparency on and then
##	off: a minimized window keeps its device for half a minute, lets it go
##	within a minute and a quarter, and shows what it did once restored.
##	Not in the pipeline: about three minutes, close to the harness limit.
##	Test ID: ErqPAvG

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

##	The text area only. The title says how the device is doing.
function fBody($bmp) {
	if (-not $bmp) { return $null }
	$top = 80; $edge = 12
	if ($bmp.Width -le 2 * $edge -or $bmp.Height -le $top + $edge) { return $bmp }
	$area = New-Object System.Drawing.Rectangle $edge, $top, ($bmp.Width - 2 * $edge), ($bmp.Height - $top - $edge)
	$bmp.Clone($area, $bmp.PixelFormat)
}

##	The log so far. The file is still open for writing, so it is read shared.
function fReleased($path) {
	try {
		$fs = [IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
		$text = (New-Object IO.StreamReader $fs).ReadToEnd()
		$fs.Close()
	} catch { return $false }
	$text -match 'device released'
}

foreach ($arm in @("seethru", "opaque")) {
	$cfg = Join-Path $RunDir "idlemin-$arm.shcl"
	fFreshConfig $cfg @("window:", "`tidle_release: true", "transparency:", "`tenabled: $(if ($arm -eq 'seethru') { 'true' } else { 'false' })")
	New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null
	$err = Join-Path $script:shotDir "idlemin-$arm.txt"
	[Environment]::SetEnvironmentVariable("SILK_IDLEDBG", "1")
	$p = Start-Process $Exe -ArgumentList @("--config=$cfg", "--columns", "100", "--rows", "30", "--shell=cmd.exe") -RedirectStandardError $err -PassThru
	[Environment]::SetEnvironmentVariable("SILK_IDLEDBG", $null)
	fTrack $p
	$h = fWaitWindow $p 40
	if (-not (fCheck "$arm a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; continue }
	Start-Sleep -Seconds 2
	[void](fFocus $h)
	fSend "cls"
	fPress "enter"
	Start-Sleep -Milliseconds 700
	fSend "for /L %i in (1,1,40) do @echo idlemin-$arm-%i"
	fPress "enter"
	Start-Sleep -Seconds 2
	$ref = fBody (fShot $h "idlemin-$arm-ref")

	[void][Silk.Win]::ShowWindow($h, 6)   ## SW_MINIMIZE
	Start-Sleep -Seconds 30
	[void](fCheck "$arm kept its device for half a minute" (-not (fReleased $err)))
	$released = $false
	for ($i = 0; $i -lt 45 -and -not $released; $i++) {
		Start-Sleep -Seconds 1
		$released = fReleased $err
	}
	fNote "released about $(30 + $i) s after the minimize"
	[void](fCheck "$arm let it go within a minute and a quarter" $released)

	[void](fFocus $h)
	$moved = 1.0
	for ($i = 0; $i -lt 6 -and $moved -ge 0.02; $i++) {
		Start-Sleep -Seconds 1
		$moved = fDiff $ref (fBody (fShot $h "idlemin-$arm-back")) 6
	}
	fNote "restored, moved $moved after $i s"
	[void](fCheck "$arm restored shows what it did" ($moved -lt 0.02))

	fStop $p
	Start-Sleep -Milliseconds 500
	foreach ($line in (Get-Content $err -ErrorAction SilentlyContinue | Select-Object -Last 60)) { fNote "dbg $line" }
}

##	History:
##		- 20261005 JC: Created.
