##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Three windows, two on one settings file and one on another. A save in the
	first has to show in the second at once, and never in the third.
.DESCRIPTION
	The same checks as cicd/tests/livecfg on Linux, over named pipes:
	- View > Minimap in window A saves. A's minimap comes on, B keeps its
	  own, C's does not change, and nobody picks a new wallpaper.
	- A new wallpaper folder written by hand, then --reload-settings sent to A.
	  A and B both pick from the new folder; C picks nothing.
	- B ended without warning, the way Task Manager does it. The next save in
	  A finds it gone and takes its marker away.
	- A window D opened after a save starts with what was saved.
	- --wallpaper typed into a shell inside window E changes E's picture
	  and no other.
	A locked session takes no keys and draws no frames, so there each save is
	a hand edit read again in A, and what only the screen or a wallpaper pick
	would show goes unchecked. Every notice still goes over the pipes, which
	is the part only Windows has.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EsbxCqL


$typing = fSessionUsable
if (-not $typing) { fNote "the session is locked, so saves are hand edits read again in a" }

Add-Type -Namespace SilkLive -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
'@

$dir = Join-Path $OutDir "livecfg"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
[void](New-Item -ItemType Directory -Path $dir -Force)

##	Two folders of pictures that look nothing alike, so a pick can be told by name.
function fPictures([string]$Folder, [string]$Prefix, [string[]]$Colors) {
	[void](New-Item -ItemType Directory -Path $folder -Force)
	$n = 0
	foreach ($c in $colors) {
		$n++
		$bmp = New-Object System.Drawing.Bitmap (200 + $n * 37), (150 + $n * 23)
		$g = [System.Drawing.Graphics]::FromImage($bmp)
		$rect = New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height
		$brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, ([System.Drawing.Color]::FromName($c)), ([System.Drawing.Color]::Black), 90.0
		$g.FillRectangle($brush, $rect)
		$bmp.Save((Join-Path $folder "$prefix$n.png"), [System.Drawing.Imaging.ImageFormat]::Png)
		$brush.Dispose(); $g.Dispose(); $bmp.Dispose()
	}
}
$pics1 = Join-Path $dir "pics1"
$pics2 = Join-Path $dir "pics2"
fPictures -Folder $pics1 -Prefix "one" -Colors @("Red", "Green", "Blue", "Yellow", "Cyan", "Magenta", "Orange", "Purple")
fPictures -Folder $pics2 -Prefix "two" -Colors @("White", "Gray", "Pink", "Brown", "Navy", "Olive", "Teal", "Maroon")

##	A blink-free cursor, so only the setting under test moves pixels. Forward
##	slashes keep the folder clear of the string's escapes.
function fConfig([string]$Path) {
	[void](New-Item -ItemType Directory -Path (Split-Path $path) -Force)
	fFreshConfig $path @("cursor:", "`tblink: false", "window:", "`tremember_per_monitor: false",
		"scroll:", "`tminimap:", "`t`tenabled: false",
		"wallpaper:", "`trotate:", "`t`tfolder: `"$($pics1 -replace '\\', '/')`"", "`t`trandom: true")
}
$shared = Join-Path $dir "shared\config.shcl"
$other = Join-Path $dir "other\config.shcl"
fConfig $shared
fConfig $other

##	The minimap's own line, under its heading in the scroll section.
function fMinimap([string]$Path, [string]$Want) {
	$in = $false; $found = $false
	foreach ($line in (Get-Content $path)) {
		if ($line -match '^\tminimap:') { $in = $true; continue }
		if ($in -and $line -match '^\t\tenabled:\s*(\S+)') { $found = ($Matches[1] -eq $want) }
		if ($in -and $line -match '^\t[^\t]') { $in = $false }
		if ($line -match '^[^\t#]') { $in = $false }
	}
	$found
}
function fWaitMinimap([string]$Path, [string]$Want) {
	for ($i = 0; $i -lt 50; $i++) {
		if (fMinimap $path $want) { return $true }
		Start-Sleep -Milliseconds 200
	}
	$false
}

function fHistory([string]$Config) {
	$h = Join-Path (Split-Path $config) ".wallpaper-history"
	if (Test-Path $h) { (Get-Content $h -Raw) } else { "" }
}

##	The meeting folder holding this window's marker.
function fMarkers([int]$Id) {
	foreach ($d in (Get-ChildItem $env:TEMP -Directory -Filter "silkterm-*" -ErrorAction SilentlyContinue)) {
		if (Test-Path (Join-Path $d.FullName "$id")) { return $d.FullName }
	}
	$null
}

##	A picture is prepared on a worker after the first frame, and the text's
##	halo follows it, so a window is only read once two shots a second apart
##	agree.
function fSettled([IntPtr]$H, [string]$Name) {
	$prev = fShot $h "$name-prev"
	for ($i = 0; $i -lt 40; $i++) {
		Start-Sleep -Seconds 1
		$now = fShot $h $name
		if ((fDiff $prev $now) -eq 0) { return $now }
		$prev = $now
	}
	$null
}

##	The minimap line set by hand, the way a person would edit it.
function fSetMinimap([string]$Path, [string]$Value) {
	$in = $false
	$lines = @(foreach ($line in (Get-Content $path)) {
		if ($line -match '^\tminimap:') { $in = $true }
		elseif ($line -match '^[^\t#]' -or $line -match '^\t[^\t]') { $in = $false }
		elseif ($in -and $line -match '^\t\tenabled:') { $line = "`t`tenabled: $value" }
		$line
	})
	Set-Content -Path $path -Value $lines -Encoding UTF8
}

##	--reload-settings sent to a's pipe, the way a shell inside a would send it.
function fReload {
	$cli = fStartSilk -Exe $Exe -SilkArgs @("--reload-settings") -EnvVars @{ SILKTERM_SOCKET = "\\.\pipe\silkterm-ctl-$($procs.a.Id)" }
	##	Without a handle taken before it ends, the exit code reads as nothing.
	[void]$cli.Handle
	[void]$cli.WaitForExit(15000)
	if (-not $cli.HasExited -or $cli.ExitCode -ne 0) {
		$pipes = @([IO.Directory]::GetFiles("\\.\pipe\") | Where-Object { $_ -like "*silkterm*" })
		fNote "--reload-settings: exited $($cli.HasExited), code $(if ($cli.HasExited) { $cli.ExitCode }), a is $($procs.a.Id), pipes: $($pipes -join ' ')"
	}
	$cli.HasExited -and $cli.ExitCode -eq 0
}

##	A check that needs frames drawn, or a note where none are.
function fSeen([string]$What, [bool]$Ok) {
	if ($typing) { [void](fCheck $what $ok) } else { fNote "not checked while locked: $what" }
}

##	View > Minimap in a, or the same change by hand where nothing can be typed.
function fSave([string]$Value) {
	if ($typing) {
		[void](fFocus $wins.a)
		fPress "alt+v"
		Start-Sleep -Milliseconds 500
		fPress "n"
	} else {
		fSetMinimap $shared $value
		[void](fCheck "--reload-settings reached a" (fReload))
	}
}

$work = fWorkArea
$w = [int]($work.w / 3); $ht = [int]($work.h / 2)
$procs = @{}; $wins = @{}
$names = @("a", "b", "c")
foreach ($i in 0..2) {
	$name = $names[$i]
	$cfg = if ($name -eq "c") { $other } else { $shared }
	$procs[$name] = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg") -EnvVars @{}
	$wins[$name] = fWaitWindow $procs[$name] 40
	if (-not (fCheck "${name}: a window came up" ($wins[$name] -ne [IntPtr]::Zero))) {
		foreach ($p in $procs.Values) { fStop $p }
		return
	}
	##	Side by side, since a screen grab takes whatever is on top.
	[void][SilkLive.Win]::MoveWindow($wins[$name], $work.x + $i * $w, $work.y, $w, $ht, $true)
}
##	first frames, the launch scans, and the markers
Start-Sleep -Seconds 8
$dirA = fMarkers $procs.a.Id; $dirB = fMarkers $procs.b.Id; $dirC = fMarkers $procs.c.Id
[void](fCheck "a and b meet in one folder" ($dirA -and $dirA -eq $dirB))
[void](fCheck "c meets in another" ($dirC -and $dirC -ne $dirA))

##	Everything that types goes to a, so b and c keep the look of a window
##	without focus all the way through.
if ($typing) {
	if (-not (fCheck "a took the focus" (fFocus $wins.a))) { fNote (fForeground); foreach ($p in $procs.Values) { fStop $p }; return }
	Start-Sleep -Seconds 1
}

$shots0 = @{}
foreach ($name in $names) { $shots0[$name] = fSettled $wins[$name] "${name}0" }
[void](fCheck "all 3 settled" ($null -ne $shots0.a -and $null -ne $shots0.b -and $null -ne $shots0.c))
$histShared = fHistory $shared; $histOther = fHistory $other

fSave "true"
[void](fCheck "the file has the minimap on" (fWaitMinimap $shared "true"))
$shots1 = @{}
foreach ($name in $names) { $shots1[$name] = fSettled $wins[$name] "${name}1" }
$moved = @{}
foreach ($name in $names) { $moved[$name] = fDiff $shots0[$name] $shots1[$name] }
fNote "moved: a $($moved.a), b $($moved.b), c $($moved.c)"
##	a is the control: the minimap shows at all
fSeen "a shows it" ($moved.a -gt 0)
##	A view switch is for new windows now, and the others keep theirs (2026101313381123).
fSeen "b keeps its own" ($moved.b -eq 0)
fSeen "c does not" ($moved.c -eq 0)
[void](fCheck "c's file is untouched" (-not (fMinimap $other "true")))
fSeen "nobody picked a new wallpaper" ((fHistory $shared) -eq $histShared)

##	The new folder, read again in a
$shots2 = @{}
foreach ($name in @("b", "c")) { $shots2[$name] = fSettled $wins[$name] "${name}2" }
(Get-Content $shared -Raw).Replace(($pics1 -replace '\\', '/'), ($pics2 -replace '\\', '/')) | Set-Content $shared -NoNewline -Encoding UTF8
[void](fCheck "--reload-settings reached a" (fReload))
Start-Sleep -Seconds 1
$shots3 = @{}
foreach ($name in @("b", "c")) { $shots3[$name] = fSettled $wins[$name] "${name}3" }
$picks = @((fHistory $shared) -split "`r?`n" | Where-Object { $_ -match 'two\d+\.png' })
fNote "picks from the new folder: $($picks.Count)"
##	each window picks on its own, and two picks can land on one name
fSeen "the new folder was picked from" ($picks.Count -ge 1)
fSeen "b shows a picture from it" ((fDiff $shots2.b $shots3.b) -gt 0)
fSeen "c does not" ((fDiff $shots2.c $shots3.c) -eq 0)
fSeen "c picked nothing" ((fHistory $other) -eq $histOther)

##	b ended without warning
$deadB = $procs.b.Id
Stop-Process -Id $deadB -Force
[void]$procs.b.WaitForExit(10000)
[void](fCheck "b's marker is left behind" ($dirA -and (Test-Path (Join-Path $dirA "$deadB"))))
fSave "false"
[void](fCheck "the file has the minimap off" (fWaitMinimap $shared "false"))
Start-Sleep -Seconds 1
[void](fCheck "the next save took it away" ($dirA -and -not (Test-Path (Join-Path $dirA "$deadB"))))
[void](fCheck "a's marker stays" ($dirA -and (Test-Path (Join-Path $dirA "$($procs.a.Id)"))))
[void](fCheck "a still runs" (-not $procs.a.HasExited))
[void](fCheck "c still runs" (-not $procs.c.HasExited))

##	A window opened after a save. Only View > Minimap in d can show what it
##	came up with, so this needs keys.
if ($typing) {
	fSave "true"
	[void](fCheck "the file has the minimap on" (fWaitMinimap $shared "true"))
	$procs["d"] = fStartSilk -Exe $Exe -SilkArgs @("--config=$shared") -EnvVars @{}
	$wins["d"] = fWaitWindow $procs.d 40
	if (fCheck "d: a window came up" ($wins.d -ne [IntPtr]::Zero)) {
		[void][SilkLive.Win]::MoveWindow($wins.d, $work.x + $w, $work.y, $w, $ht, $true)
		Start-Sleep -Seconds 5
		##	It flips what d shows, so the file says off only if d came up with it on
		[void](fFocus $wins.d)
		fPress "alt+v"
		Start-Sleep -Milliseconds 500
		fPress "n"
		[void](fCheck "d started with the saved minimap" (fWaitMinimap $shared "false"))
	}
} else { fNote "not checked while locked: d started with the saved minimap" }

##	--wallpaper typed into a shell inside e, on a file of its own, reaches e
##	alone through the pipe name the shell got from it. cmd clears the screen
##	before each shot, so only the picture can differ.
if ($typing) {
	$third = Join-Path $dir "third\config.shcl"
	fConfig $third
	$procs["e"] = fStartSilk -Exe $Exe -SilkArgs @("--config=$third", "--shell", "cmd.exe") -EnvVars @{}
	$wins["e"] = fWaitWindow $procs.e 40
	if (fCheck "e: a window came up" ($wins.e -ne [IntPtr]::Zero)) {
		[void][SilkLive.Win]::MoveWindow($wins.e, $work.x + 2 * $w, $work.y, $w, $ht, $true)
		Start-Sleep -Seconds 5
		[void](fFocus $wins.e)
		fSendChars "cls"
		fPress "enter"
		$e0 = fSettled $wins.e "e0"; $a0 = fSettled $wins.a "a4"
		fSendChars "`"$Exe`" --wallpaper `"$(Join-Path $pics2 'two3.png')`" & cls"
		fPress "enter"
		Start-Sleep -Seconds 2
		$e1 = fSettled $wins.e "e1"; $a1 = fSettled $wins.a "a5"
		fNote "moved: e $(fDiff $e0 $e1), a $(fDiff $a0 $a1)"
		[void](fCheck "--wallpaper changed e" ((fDiff $e0 $e1) -gt 0.05))
		[void](fCheck "and not a" ((fDiff $a0 $a1) -eq 0))
	}
} else { fNote "not checked while locked: --wallpaper from a shell inside a window" }

foreach ($p in $procs.Values) { fStop $p }

##	History:
##		- 20261013 JC: Created.
##		- 20261014 JC: B keeps its own minimap; D opens with the saved one; --wallpaper from E's shell.
