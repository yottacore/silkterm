##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Body of the idlebehind and idlecover scenarios.
.DESCRIPTION
	A window left behind another one on a real Windows desktop: partly in
	view it waits "Minutes otherwise", and all covered it waits "Minutes when
	hidden". $idleArms names the arms to run. Each arm, Transparency off and
	then on: the window is typed in, then
	another program's window covers it and takes the focus, with the pointer
	resting over that one. The waits are read from the file.
	- Partly in view: the other window leaves a strip on the left. "Minutes
	  when hidden" is 30, so only "Minutes otherwise", at 1, can let it go.
	  With Transparency off it has to let its device go within a minute and a
	  quarter, and come back on a click. With Transparency on it has to let go
	  of all but its device and swapchain in the same time, since it would go
	  black without them (see the releasing resources design doc). The strip
	  in view must still show the same picture a few seconds later, and a
	  click must bring the rest back on the kept device. Neither may ever be
	  taken as covered.
	- All covered: the other window covers all of it. "Minutes otherwise" is
	  30, so only "Minutes when hidden", at 1, can let it go, and it has to,
	  with either Transparency setting, within a minute and a quarter. Then
	  the other window is moved off it without the focus changing. The window
	  must be taken as shown again, and with Transparency on it has to take
	  its device back without a click. Then it must be taken as hidden on
	  another virtual desktop, and as shown once back.
.NOTES
	History: At bottom of file.
#>

if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

Add-Type -Namespace IdleCover -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);'

##	The log so far. The file is still open for writing, so it is read shared.
function fLog([string]$Path) {
	try {
		$fs = [IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
		$text = (New-Object IO.StreamReader $fs).ReadToEnd()
		$fs.Close()
		$text
	} catch { "" }
}

##	The strip a partly covered window leaves in view, inside its frame.
function fStrip([IntPtr]$H, [string]$Name) {
	$bmp = fShot $h $name
	if (-not $bmp -or $bmp.Width -lt 160 -or $bmp.Height -lt 120) { return $null }
	$bmp.Clone((New-Object System.Drawing.Rectangle 12, 50, 136, ($bmp.Height - 100)), [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
}

##	Another program's window, in a process of its own so it has its own
##	message loop and takes the focus the way any other program's does. Started
##	hidden, so its console stays out of the way, and the first ShowWindow of
##	the process takes that hidden too. The timer shows it a second time.
$coverScript = Join-Path $RunDir "idlebehind-cover.ps1"
Set-Content -Path $coverScript -Encoding UTF8 -Value @'
param([int]$X, [int]$Y, [int]$W, [int]$H)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -Namespace Cover -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);'
$f = New-Object System.Windows.Forms.Form
$f.Text = "idlebehind cover"
$f.StartPosition = "Manual"
$f.Bounds = New-Object System.Drawing.Rectangle $X, $Y, $W, $H
$f.BackColor = [System.Drawing.Color]::Gainsboro
$t = New-Object System.Windows.Forms.Timer
$t.Interval = 300
$t.Add_Tick({ $t.Stop(); [void][Cover.Win]::ShowWindow($f.Handle, 1); $f.Activate() })
$t.Start()
[System.Windows.Forms.Application]::Run($f)
'@

foreach ($arm in $idleArms) {
	$full = $arm -like "covered-*"
	$seethru = $arm -like "*seethru"
	$cfg = Join-Path $RunDir "idlebehind-$arm.shcl"
	fFreshConfig $cfg @("window:", "`tidle_release: true", "`tidle_release_min: $(if ($full) { 30 } else { 1 })", "`tidle_release_hidden_min: $(if ($full) { 1 } else { 30 })", "`tremember_per_monitor: false", "transparency:", "`tenabled: $(if ($seethru) { 'true' } else { 'false' })")
	New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null
	$err = Join-Path $script:shotDir "idlebehind-$arm.txt"
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
	fSend "for /L %i in (1,1,40) do @echo idlebehind-$arm-%i"
	fPress "enter"
	Start-Sleep -Seconds 2

	##	Most of it covered, a strip on the left still showing, or all of it.
	$r = fRect $h
	$at = if ($full) { @(($r.x - 20), ($r.y - 20), ($r.w + 40), ($r.h + 40)) } else { @(($r.x + 160), ($r.y - 10), ($r.w), ($r.h + 20)) }
	$cover = Start-Process powershell.exe -ArgumentList (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $coverScript) + $at) -WindowStyle Hidden -PassThru
	fTrack $cover
	$ch = [IntPtr]::Zero
	for ($i = 0; $i -lt 80 -and $ch -eq [IntPtr]::Zero; $i++) {
		Start-Sleep -Milliseconds 250
		$ch = [SilkEnum]::Largest([uint32]$cover.Id)
	}
	if (-not (fCheck "$arm the cover window came up" ($ch -ne [IntPtr]::Zero))) { fStop $p; fStop $cover; continue }
	[void](fFocus $ch)
	$cr = fRect $ch
	[void][Silk.Win]::SetCursorPos($cr.x + [int]($cr.w / 2), $cr.y + [int]($cr.h / 2))
	$t0 = Get-Date
	fNote "$arm covered, foreground ours: $(fForegroundIsOurs $h)"
	$before = $null
	if ($seethru -and -not $full) {
		Start-Sleep -Seconds 2
		$before = fStrip $h "idlebehind-$arm-before"
	}

	$released = $false
	for ($i = 0; $i -lt 80 -and -not $released; $i++) {
		Start-Sleep -Seconds 1
		$released = (fLog $err) -match 'device released'
	}
	$secs = [int]((Get-Date) - $t0).TotalSeconds
	fNote "$arm released: $released, $secs s after it was covered"
	if ($full) {
		[void](fCheck "$arm all covered is taken as covered" ((fLog $err) -match 'covered by other windows'))
		[void](fCheck "$arm all covered let its device go within a minute and a quarter" $released)
	} else {
		[void](fCheck "$arm partly in view is never taken as covered" (-not ((fLog $err) -match 'covered by other windows')))
		if ($seethru) {
			$log = fLog $err
			[void](fCheck "$arm partly in view lets go of all but its device and swapchain within a minute and a quarter" ($released -and ($log -match 'device released but for itself and its swapchain') -and -not ($log -match 'kept: on screen')))
			##	It went black within 4 seconds when the device went with the rest.
			Start-Sleep -Seconds 6
			$after = fStrip $h "idlebehind-$arm-after"
			$moved = fDiff $before $after
			fNote "$arm strip ink before $(fInk $before 3), after $(fInk $after 3), moved $moved"
			[void](fCheck "$arm partly in view still shows its last frame" ($after -and $moved -lt 0.1))
		} else {
			[void](fCheck "$arm behind another window let its device go within a minute and a quarter" $released)
		}
	}

	if ($full) {
		##	Off to the right, keeping the focus: SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE.
		$r = fRect $h
		[void][IdleCover.Win]::SetWindowPos($ch, [IntPtr]::Zero, ($r.x + $r.w + 40), $cr.y, 0, 0, 0x15)
		$shown = $false
		for ($i = 0; $i -lt 20 -and -not $shown; $i++) {
			Start-Sleep -Milliseconds 250
			$shown = (fLog $err) -match 'not covered'
		}
		[void](fCheck "$arm shown again once the other window moves off it" ($shown -and -not ((fLog $err) -match 'not covered: focused')))
		fNote "$arm moved off, foreground ours: $(fForegroundIsOurs $h)"
		if ($seethru -and $released) {
			$back = $false
			for ($i = 0; $i -lt 20 -and -not $back; $i++) {
				Start-Sleep -Milliseconds 250
				$back = (fLog $err) -match 'device rebuilt'
			}
			[void](fCheck "$arm takes its device back without a click, as one that would go black" $back)
			$released = $false
		}
		##	Another virtual desktop and back, with the focus elsewhere. Win+Ctrl+D
		##	makes one and moves to it, and switching back to it and closing it
		##	ends up back here.
		$log = fLog $err
		$hid = ([regex]::Matches($log, 'covered by other windows')).Count
		$showed = ([regex]::Matches($log, 'not covered')).Count
		fPress "ctrl+win+d"
		Start-Sleep -Seconds 3
		$away = ([regex]::Matches((fLog $err), 'covered by other windows')).Count -gt $hid
		fPress "ctrl+win+left"
		Start-Sleep -Seconds 3
		$back = ([regex]::Matches((fLog $err), 'not covered')).Count -gt $showed
		fPress "ctrl+win+right"
		Start-Sleep -Seconds 1
		fPress "ctrl+win+f4"
		Start-Sleep -Seconds 3
		[void](fCheck "$arm on another virtual desktop is taken as hidden" $away)
		[void](fCheck "$arm back from the other virtual desktop is taken as shown" $back)
	}
	fStop $cover
	Start-Sleep -Milliseconds 500
	if ($released) {
		$r = fRect $h
		fClick ($r.x + [int]($r.w / 2)) ($r.y + [int]($r.h / 2))
		$back = $false
		for ($i = 0; $i -lt 20 -and -not $back; $i++) {
			Start-Sleep -Milliseconds 250
			$back = (fLog $err) -match 'device rebuilt'
		}
		[void](fCheck "$arm a click brings it back" $back)
		if ($seethru -and -not $full) {
			[void](fCheck "$arm a click brings it back on the kept device" ((fLog $err) -match 'device rebuilt.*on the kept device'))
		}
	}

	fStop $p
	Start-Sleep -Milliseconds 500
	foreach ($line in ((fLog $err) -split "`r?`n" | Where-Object { $_ } | Select-Object -Last 40)) { fNote "dbg $line" }
}

##	History:
##		- 20261125 JC: Created, from idlebehind.ps1, with all covered arms.
##		- 20261125 JC: Transparency on, partly in view: lets go of all but its device and swapchain, and keeps its last frame up.
