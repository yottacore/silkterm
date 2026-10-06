#!/usr/bin/env pwsh

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

<#
.SYNOPSIS
	Launch the newest SilkTerm dogfood build, passing through any arguments.
.DESCRIPTION
	One implementation for Linux, Windows and macOS; the 'runterm' wrappers
	beside it just call this with pwsh.

	One source per platform: the synced app dir that cicd installs into. A
	build made on any box arrives there over Dropbox, so there is no network
	path to wait on and nothing to probe.

	Copies go in a versions folder next to a '<program>' symlink pointing at
	the newest, so a plain 'silkterm' on PATH (and a .desktop Icon=) always
	reaches the current build without being rewritten.

	Copies are named '<prefix>_<YYYYMMDD-HHMMSS>_<tag>_<role>', where the stamp
	is the build's own mtime and the tag says what the binary is. Copies of one
	build do not agree on mtime (cicd dates its copy and Dropbox restamps what
	it syncs), so what keeps a build to one copy is the byte comparison, not
	the stamp.

	The folder is GFS-rotated every run: newest and oldest always, then the
	last few, then a widening time spread (day, week, month, year). It keeps
	at most 10 and at least 5, and stops at 1 GB in between. A copy that is
	running is never deleted.

	Windows runs the whole launcher elevated (self-elevates via UAC), so the
	copy, the symlink and the launched terminal all get admin rights - a
	filtered token has no SeCreateSymbolicLinkPrivilege and cannot make the
	symlink at all. '--no-admin' opts out.

	Reports a failure in a dialog when launched from a shortcut (or with
	'--gui'), since a click's console just flashes shut. '--admin',
	'--no-admin' and '--gui' are consumed here; everything else forwards.

	With no build held and no source reachable, falls back to the first
	installed terminal from a per-platform list.

	Edit fMain() to launch a different terminal instead.
.EXAMPLE
	pwsh n8runterm.ps1 [--no-admin] [--gui] [--install-only] [terminal options]
.NOTES
	History: At bottom of script.
#>


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Configuration

## Which OS we are on. $IsWindows/$IsLinux/$IsMacOS are PowerShell 7 automatics.
$Platform = if ($IsWindows) { "windows" } elseif ($IsMacOS) { "macos" } else { "linux" }

$ProgramName   = "silkterm"
$DogfoodPrefix = "slktrmdf"
$StampFormat   = "yyyyMMdd-HHmmss"
$ExeSuffix     = if ($Platform -eq "windows") { ".exe" } else { "" }
$ExeName       = "$ProgramName$ExeSuffix"
$IconName      = "$ProgramName.png"

## Home, spelled the way each platform spells it. $env:HOME is set on Linux and
## macOS; Windows has USERPROFILE.
$HomeDir = if ($env:HOME) { $env:HOME } else { $env:USERPROFILE }

## Where a build comes from: the synced app dir cicd installs into. One entry per
## platform today, kept as a list so a second location is a one-line change. First
## one that exists wins.
## First one that exists wins. 'synced' is a junction to Dropbox, and on Windows
## it reads as an EMPTY directory - Dropbox's filter driver does not populate
## through it - so the real spelling has to be listed as well.
$SourceDirs = switch ($Platform) {
	"windows" { @(
		(Join-Path $HomeDir "synced\0-0\common\exec\app\mswin")
		(Join-Path $HomeDir "Dropbox\0-0\common\exec\app\mswin")
	) }
	"macos"   { @(
		(Join-Path $HomeDir "synced/0-0/common/exec/app/macos")
		(Join-Path $HomeDir "Dropbox/0-0/common/exec/app/macos")
	) }
	default   { @(
		(Join-Path $HomeDir "synced/0-0/common/exec/app/linux")
		(Join-Path $HomeDir "Dropbox/0-0/common/exec/app/linux")
	) }
}

## Where copies live and what the symlink is called. Deliberately NOT under the
## synced tree - a dogfood build churns every pipeline run and has no business
## riding a sync.
$InstallRoot = switch ($Platform) {
	"windows" { Join-Path $env:LOCALAPPDATA "Programs" }
	"macos"   { Join-Path $HomeDir "Applications" }
	default   { Join-Path $HomeDir ".local/bin" }
}
$VersionsDir = Join-Path $InstallRoot "${ProgramName}_versions"
$LatestLink  = Join-Path $InstallRoot $ExeName
$IconPath    = Join-Path $InstallRoot $IconName

## Retention: at most $KeepMax copies, at least $KeepMin, and stop at $KeepBytes
## once past the minimum. $KeepRecent of the most recent are held ahead of the
## time-spread picks, so a bad build always has a couple of predecessors beside it.
$KeepMax    = 10
$KeepMin    = 5
$KeepRecent = 2
$KeepBytes  = 1GB

## How many of each period tier may be taken. Without a cap the daily picks eat the
## whole budget on a box that builds every day, and the older end of the spread -
## the point of keeping any of this - never gets a slot. These sum to the budget,
## so which tiers actually survive is decided by the order they are asked in.
$PeriodKeep = @{ day = 3; week = 2; month = 2; year = 1 }

## Per-run decision log beside the versions folder, so a console that closes can't
## take the copy and prune reasons with it.
$RunLog        = Join-Path $InstallRoot "runterm.log"
$RunLogMaxSize = 256KB

## Fallback terminals, in preference order, for when nothing is held and no source
## answers. Ours keeps the tagged title; the rest are launched plainly, since
## SilkTerm's own options would not parse for them.
$FallbackTerminals = switch ($Platform) {
	"windows" { @(
		@{ Exe = "silkterm.exe"; Silk = $true  }
		@{ Exe = "wt.exe";       Silk = $false }
		@{ Exe = "PyCmd.exe";    Silk = $false }
		@{ Exe = "cmd.exe";      Silk = $false }
	) }
	"macos"   { @(
		@{ Exe = "silkterm";  Silk = $true  }
		@{ Exe = "alacritty"; Silk = $false }
		@{ Exe = "kitty";     Silk = $false }
	) }
	default   { @(
		@{ Exe = "silkterm";        Silk = $true  }
		@{ Exe = "terminator";      Silk = $false }
		@{ Exe = "xfce4-terminal";  Silk = $false }
		@{ Exe = "gnome-terminal";  Silk = $false }
		@{ Exe = "konsole";         Silk = $false }
		@{ Exe = "alacritty";       Silk = $false }
		@{ Exe = "kitty";           Silk = $false }
		@{ Exe = "xterm";           Silk = $false }
	) }
}

## The wrapper a desktop entry should run, best first. A shortcut must start the
## launcher, not the terminal directly, or it pins whichever build it was written
## against and never sees another one.
$WrapperCandidates = switch ($Platform) {
	"windows" { @(
		(Join-Path $HomeDir "synced\0-0\common\exec\util\mswin\cli\by-self\cmd\runterm.cmd")
		(Join-Path $HomeDir "Dropbox\0-0\common\exec\util\mswin\cli\by-self\cmd\runterm.cmd")
		"C:\opt\0-0\common\exec\synced\util\mswin\cli\by-self\cmd\runterm.cmd"
		"C:\0-0\common\exec\synced\util\mswin\cli\by-self\cmd\runterm.cmd"
	) }
	"macos"   { @(
		(Join-Path $HomeDir "synced/0-0/common/exec/util/macos/bash/runterm")
		(Join-Path $HomeDir "Dropbox/0-0/common/exec/util/macos/bash/runterm")
	) }
	default   { @(
		"/usr/local/bin/x9/sh/runterm"
		"/opt/0-0/common/exec/synced_local-copies/util/linux/bash/runterm"
		(Join-Path $HomeDir "synced/0-0/common/exec/util/linux/bash/runterm")
	) }
}

## Set from the entry point's flags; see there.
$RunAsAdmin = $false


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Functions

## Entry point: what this launcher runs. Edit this to launch a different terminal.
function fMain {
	param([string[]]$PassArgs, [switch]$InstallOnly)

	fEnsureDir $InstallRoot
	fEnsureDir $VersionsDir
	fTrimLog
	fLog ("=== run: PS {0}, {1}, host '{2}', user {3} ===" -f `
		$PSVersionTable.PSVersion, $Platform, [Environment]::MachineName, [Environment]::UserName)

	if ($Platform -eq "windows") { fSelfHealMotw }

	fCopyIfNewer
	fRotate
	fUpdateIcon

	$newest = fNewestHeld
	if ($newest) {
		fUpdateLatestLink -Target $newest.File.FullName
		fUpdateShortcut
		## The launchers return the Process so a test harness can stop that exact
		## instance by PID; nothing here wants it printed.
		if (-not $InstallOnly) { fLaunchSilkTerm -Exe $newest.File.FullName -PassArgs $PassArgs | Out-Null }
		return
	}

	fWarn "no dogfood build held and no source reachable; trying fallbacks"
	if (-not $InstallOnly) { fLaunchFallbackTerminal -PassArgs $PassArgs | Out-Null }
}


## The first source dir that exists, or $null.
function fSourceDir {
	foreach ($dir in $SourceDirs) {
		if (Test-Path -LiteralPath $dir) { return $dir }
	}
	return $null
}


## Copy the source build in as '<prefix>_<stamp>_<tag>' when it is newer than the
## newest copy held. No-op when the source is missing or we are already current.
function fCopyIfNewer {

	$dir = fSourceDir
	if (-not $dir) {
		fNote "no source dir on this box ($($SourceDirs -join ', '))"
		return
	}

	$src  = Join-Path $dir $ExeName
	$item = Get-Item -LiteralPath $src -ErrorAction SilentlyContinue
	if (-not $item) {
		fNote "no build in $dir"
		return
	}

	$stamp  = $item.LastWriteTime.ToString($StampFormat)
	$tag    = fBuildTag -SourceDir $dir
	$newest = fNewestHeld

	## The held stamp comes out of a file name, so it carries whole seconds only.
	## Compare at that precision: a source mtime with a fraction (cp -p from cargo's
	## output has one) otherwise always reads as newer, and every launch falls
	## through to hashing the whole binary to find the copy it already holds.
	$srcWhole = $item.LastWriteTime.AddTicks(-($item.LastWriteTime.Ticks % [timespan]::TicksPerSecond))
	if ($newest -and $newest.Stamp -ge $srcWhole) {
		fNote "already current (held $($newest.Stamp.ToString($StampFormat)), source $stamp)"
		return
	}

	## A build we already hold keeps looking new, because no two copies of it agree
	## on mtime. Settle it on the bytes, then take the source's stamp so the cheap
	## test above answers it next run without reading the whole binary.
	$twin = fHeldMatching -SrcPath $src
	if ($twin) {
		[void](fRenameHeld -Version $twin -Stamp $stamp -Tag $twin.Tag -Role $twin.Role `
			-Why "same build as $($twin.Name)")
		return
	}

	## Copy to a temp name and rename it into place. An abandoned copy otherwise
	## leaves a half-written binary that later reads as a good build and gets
	## launched; '.partial' matches neither the selection nor the prune name spec.
	$dst = Join-Path $VersionsDir "${DogfoodPrefix}_${stamp}_${tag}_newest$ExeSuffix"
	$tmp = "$dst.partial"
	Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue

	try {
		Copy-Item -LiteralPath $src -Destination $tmp -Force -ErrorAction Stop
		if ($Platform -ne "windows") { chmod +x $tmp 2>$null }
		Move-Item -LiteralPath $tmp -Destination $dst -Force -ErrorAction Stop
		fNote "copied -> $(Split-Path $dst -Leaf)"
	} catch {
		Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
		fWarn -Gui "couldn't copy the build ($($_.Exception.Message))"
	}
}


## What the source build IS, in the shared tag convention
## '<toolchain: gnu|msvc><built on: l|m|b|w><target: l|m|b|w><arch: i|a>'. cicd
## writes it beside the binary, because a cross-build says nothing about the box
## that reads it. Without the sidecar, describe what we can see.
function fBuildTag {
	param([Parameter(Mandatory)][string]$SourceDir)

	$sidecar = Join-Path $SourceDir "$ExeName.tag"
	$tag     = (Get-Content -LiteralPath $sidecar -TotalCount 1 -ErrorAction SilentlyContinue)
	if ($tag) {
		$tag = $tag.Trim()
		if ($tag -match '^[a-z0-9]+$') { return $tag }
	}

	$target = switch ($Platform) { "windows" { "w" } "macos" { "m" } default { "l" } }
	$arch   = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq "Arm64") { "a" } else { "i" }
	return "gnux$target$arch"
}


## Every copy held, as objects { File, Name, Stamp, Tag, Role }. Only names matching
## our own spec, so a neighbour that merely shares the dir is never touched.
function fHeldVersions {
	$rx = "^$([regex]::Escape($DogfoodPrefix))_(?<stamp>\d{8}-\d{6})_(?<tag>[a-z0-9]+)" +
	      "(?:_(?<role>[a-z]+))?$([regex]::Escape($ExeSuffix))$"

	Get-ChildItem -LiteralPath $VersionsDir -File -ErrorAction SilentlyContinue |
		ForEach-Object {
			if ($_.Name -match $rx) {
				[pscustomobject]@{
					File  = $_
					Name  = $_.Name
					Stamp = [datetime]::ParseExact($Matches.stamp, $StampFormat,
					            [System.Globalization.CultureInfo]::InvariantCulture)
					Tag   = $Matches.tag
					Role  = if ($Matches.ContainsKey("role")) { $Matches.role } else { "" }
				}
			}
		}
}


## The newest copy held, or $null.
function fNewestHeld {
	fHeldVersions | Sort-Object Stamp -Descending | Select-Object -First 1
}


## The copy holding byte-for-byte the same build as $SrcPath, or $null. Size is the
## cheap discriminator - two builds almost never match on it - so the hash only runs
## when one does.
function fHeldMatching {
	param([Parameter(Mandatory)][string]$SrcPath)

	$src = Get-Item -LiteralPath $SrcPath -ErrorAction SilentlyContinue
	if (-not $src) { return $null }

	$sameSize = @(fHeldVersions | Where-Object { $_.File.Length -eq $src.Length })
	if (-not $sameSize) { return $null }

	$srcHash = (Get-FileHash -LiteralPath $SrcPath -Algorithm SHA256 -ErrorAction SilentlyContinue).Hash
	if (-not $srcHash) { return $null }

	foreach ($cand in $sameSize) {
		$hash = (Get-FileHash -LiteralPath $cand.File.FullName -Algorithm SHA256 -ErrorAction SilentlyContinue).Hash
		if ($hash -eq $srcHash) { return $cand }
	}
	return $null
}


## Rename a held copy to a new stamp/tag/role. A running image can refuse it, which
## costs nothing - the name is bookkeeping and the next run tries again.
function fRenameHeld {
	param(
		[Parameter(Mandatory)]$Version,
		[Parameter(Mandatory)][string]$Stamp,
		[Parameter(Mandatory)][string]$Tag,
		[string]$Role = "",
		[string]$Why  = ""
	)

	$leaf = "${DogfoodPrefix}_${Stamp}_${Tag}" + $(if ($Role) { "_$Role" } else { "" }) + $ExeSuffix
	if ($leaf -eq $Version.Name) { return $Version.File.FullName }

	$dst = Join-Path $VersionsDir $leaf
	if (Test-Path -LiteralPath $dst) { return $dst }

	try {
		Move-Item -LiteralPath $Version.File.FullName -Destination $dst -ErrorAction Stop
		if ($Why) { fNote "$Why - renamed to $leaf" }
		return $dst
	} catch {
		if ($Why) { fNote "$Why, but the rename was refused" }
		return $Version.File.FullName
	}
}


## GFS rotation. Selects what to keep, deletes the rest, then names each survivor
## for the role it is filling so a directory listing reads as the retention plan.
function fRotate {

	$all = @(fHeldVersions | Sort-Object Stamp -Descending)
	if ($all.Count -eq 0) { return }

	$now     = Get-Date
	$running = @(fRunningExePaths)

	## For each period, the newest copy in each period key. A copy is only eligible
	## for that role once its period has ended, so today's builds stay "frequent"
	## and compete on recency instead.
	$units     = @("day", "week", "month", "year")
	$periodTop = @{}
	foreach ($unit in $units) {
		$top = @{}
		foreach ($version in $all) {
			$key = fPeriodKey -When $version.Stamp -Unit $unit
			if (-not $top.ContainsKey($key)) { $top[$key] = $version.Name }
		}
		$periodTop[$unit] = $top
	}

	## Selection order, best claim first. Duplicates are dropped as it goes.
	$order = New-Object System.Collections.Generic.List[object]
	$order.Add($all[0])
	$order.Add($all[-1])
	$order.AddRange(@($all | Select-Object -Skip 1 -First $KeepRecent))
	foreach ($unit in $units) {
		$nowKey = fPeriodKey -When $now -Unit $unit
		$taken  = 0
		foreach ($version in $all) {
			if ($taken -ge $PeriodKeep[$unit]) { break }
			$key = fPeriodKey -When $version.Stamp -Unit $unit
			if ($key -eq $nowKey) { continue }
			if ($periodTop[$unit][$key] -eq $version.Name) { $order.Add($version); $taken++ }
		}
	}
	$order.AddRange($all)

	$keep  = New-Object System.Collections.Generic.List[object]
	$seen  = @{}
	$bytes = 0L
	foreach ($version in $order) {
		if ($seen.ContainsKey($version.Name)) { continue }
		if ($keep.Count -ge $KeepMax) { break }
		if ($keep.Count -ge $KeepMin -and ($bytes + $version.File.Length) -gt $KeepBytes) { break }
		$seen[$version.Name] = $true
		$bytes += $version.File.Length
		$keep.Add($version)
	}

	## Anything not selected goes, unless it is running - a window open on a build
	## must not have its binary deleted out from under it.
	$deleted = 0
	foreach ($version in $all) {
		if ($seen.ContainsKey($version.Name)) { continue }
		if ($running -contains $version.File.FullName) {
			fNote "kept (running): $($version.Name)"
			$seen[$version.Name] = $true
			$keep.Add($version)
			continue
		}
		try {
			Remove-Item -LiteralPath $version.File.FullName -Force -ErrorAction Stop
			$deleted++
		} catch {
			fNote "kept (locked): $($version.Name)"
			$seen[$version.Name] = $true
			$keep.Add($version)
		}
	}
	if ($deleted) { fNote "rotation deleted $deleted copy/copies (holding $($keep.Count))" }

	## Leftovers from a copy that was interrupted (see fCopyIfNewer).
	Get-ChildItem -LiteralPath $VersionsDir -File -Filter "*.partial" -ErrorAction SilentlyContinue |
		ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }

	## Name each survivor for the role it fills. Bigger periods win, so the last
	## build of an ended year reads 'yearly' rather than 'daily'.
	$kept   = @($keep | Sort-Object Stamp -Descending)
	$newest = $kept[0].Name
	$oldest = $kept[-1].Name
	foreach ($version in $kept) {
		$role = "frequent"
		foreach ($unit in $units) {
			$key = fPeriodKey -When $version.Stamp -Unit $unit
			if ($key -eq (fPeriodKey -When $now -Unit $unit)) { continue }
			if ($periodTop[$unit][$key] -eq $version.Name) { $role = fRoleName $unit }
		}
		if ($version.Name -eq $oldest) { $role = "oldest" }
		if ($version.Name -eq $newest) { $role = "newest" }
		[void](fRenameHeld -Version $version -Stamp $version.Stamp.ToString($StampFormat) `
			-Tag $version.Tag -Role $role)
	}
}


## The bucket a build falls in for one period unit. Two builds share a key when
## they fall in the same hour/day/week/month/year.
function fPeriodKey {
	param(
		[Parameter(Mandatory)][datetime]$When,
		[Parameter(Mandatory)][string]$Unit
	)
	switch ($Unit) {
		"day"   { return $When.ToString("yyyyMMdd") }
		"week"  { return ("{0:d4}-W{1:d2}" -f [System.Globalization.ISOWeek]::GetYear($When),
		                                       [System.Globalization.ISOWeek]::GetWeekOfYear($When)) }
		"month" { return $When.ToString("yyyyMM") }
		"year"  { return $When.ToString("yyyy") }
	}
	return $When.ToString($StampFormat)
}


## What a period unit is called in a file name.
function fRoleName {
	param([Parameter(Mandatory)][string]$Unit)
	switch ($Unit) {
		"day"   { return "daily" }
		"week"  { return "weekly" }
		"month" { return "monthly" }
		"year"  { return "yearly" }
	}
	return "frequent"
}


## Full image paths of every running process we can see. A copy that is running
## must not be deleted out from under its window.
function fRunningExePaths {
	Get-Process -ErrorAction SilentlyContinue |
		ForEach-Object { try { $_.Path } catch { $null } } |
		Where-Object { $_ }
}


## Point the '<program>' symlink at the newest copy, so a plain name on PATH and a
## .desktop Icon= both follow the current build without being rewritten. Windows
## needs a privilege for this that a filtered token lacks, which is most of why the
## launcher elevates; a copy stands in when the link is refused.
## Is this file one of ours? A copy the symlink fallback made is byte-identical
## to a build in the pool; anything else got here another way.
function fIsPoolCopy {
	param([Parameter(Mandatory)][System.IO.FileSystemInfo]$Item)

	$same = @(Get-ChildItem -LiteralPath $VersionsDir -File -ErrorAction SilentlyContinue |
		Where-Object { $_.Length -eq $Item.Length })
	if ($same.Count -eq 0) { return $false }
	$hash = (Get-FileHash -LiteralPath $Item.FullName -Algorithm SHA256).Hash
	foreach ($f in $same) {
		if ((Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash -eq $hash) { return $true }
	}
	return $false
}

function fUpdateLatestLink {
	param([Parameter(Mandatory)][string]$Target)

	$existing = Get-Item -LiteralPath $LatestLink -Force -ErrorAction SilentlyContinue
	if ($existing -and $existing.LinkTarget -eq $Target) { return }

	## A real file here is not necessarily ours: install.bash puts a release build
	## at this exact path, and this used to delete it with no prompt and no backup.
	## Only a symlink, or a copy of one of our own pool builds, may be replaced.
	if ($existing -and -not $existing.LinkTarget -and -not (fIsPoolCopy $existing)) {
		fWarn "$LatestLink was installed by something else - left it alone"
		return
	}

	try {
		if ($existing) { Remove-Item -LiteralPath $LatestLink -Force -ErrorAction Stop }
		New-Item -ItemType SymbolicLink -Path $LatestLink -Target $Target -ErrorAction Stop | Out-Null
		fNote "symlink -> $(Split-Path $Target -Leaf)"
	} catch {
		try {
			Copy-Item -LiteralPath $Target -Destination $LatestLink -Force -ErrorAction Stop
			fNote "symlink refused; copied $(Split-Path $Target -Leaf) in its place"
		} catch {
			fWarn "couldn't update $LatestLink ($($_.Exception.Message))"
		}
	}
}


## Keep the icon beside the symlink current, so a .desktop entry pointing at it
## follows whatever the newest build ships. Nothing to do if the source has none.
function fUpdateIcon {
	$dir = fSourceDir
	if (-not $dir) { return }

	$src = Join-Path $dir $IconName
	if (-not (Test-Path -LiteralPath $src)) { return }

	$have = Get-Item -LiteralPath $IconPath -ErrorAction SilentlyContinue
	$want = Get-Item -LiteralPath $src
	if ($have -and $have.Length -eq $want.Length -and $have.LastWriteTime -ge $want.LastWriteTime) { return }

	try {
		Copy-Item -LiteralPath $src -Destination $IconPath -Force -ErrorAction Stop
		fNote "refreshed icon"
	} catch {
		fWarn "couldn't refresh the icon ($($_.Exception.Message))"
	}
}


## The wrapper script a shortcut should run, or $null.
function fWrapperPath {
	foreach ($cand in $WrapperCandidates) {
		if (Test-Path -LiteralPath $cand) { return $cand }
	}
	return $null
}


## Keep the menu entry pointing at the wrapper and at the icon beside the symlink,
## so it follows the current build without being rewritten by hand. Written only
## when it would change, so a run costs one read.
function fUpdateShortcut {
	$wrapper = fWrapperPath
	if (-not $wrapper) {
		fNote "no runterm wrapper installed; menu entry left alone"
		return
	}
	if     ($Platform -eq "windows") { fWriteStartMenuLink -Wrapper $wrapper }
	elseif ($Platform -eq "macos")   { fWriteMacApp        -Wrapper $wrapper }
	else                             { fWriteDesktopEntry  -Wrapper $wrapper }
}


## macOS has no menu files. An app in ~/Applications is what Spotlight, Launchpad
## and the Dock find. Its program is a script that has the launcher copy and
## rotate, then runs the build in its own process through a link inside the
## bundle. So the terminal IS the app: one Dock icon, which can be kept there.
## Started by the launcher instead, it would be a second program with no bundle,
## and the Dock would show the app quitting and a nameless one appearing.
function fWriteMacApp {
	param([Parameter(Mandatory)][string]$Wrapper)

	$app      = Join-Path $InstallRoot "SilkTerm (dogfood).app"
	$contents = Join-Path $app "Contents"
	$script   = Join-Path $contents "MacOS/runterm"
	$link     = Join-Path $contents "MacOS/silkterm"
	$plist    = Join-Path $contents "Info.plist"
	$icns     = Join-Path $contents "Resources/silkterm.icns"

	## Started from the Finder, it gets launchd's bare PATH, which has neither
	## Homebrew nor ~/.local/bin, so the wrapper would not find pwsh. With no build
	## held, the wrapper runs as usual and falls back to another terminal.
	$q = { param([string]$s) "'" + ($s -replace "'", "'\''") + "'" }
	$wantScript = @'
#!/bin/bash
export PATH="$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH"
here="$(cd "$(dirname "$0")" && pwd)"
@WRAPPER@ --install-only >/dev/null 2>&1
held="$(basename "$(readlink @LATEST@)")"
if [ -x "$here/silkterm" ] && [[ "$held" =~ ^@PREFIX@_([0-9]{8}-[0-9]{6})_([a-z0-9]+) ]]; then
	exec "$here/silkterm" "--title=SilkTerm [dogfood ${BASH_REMATCH[2]} ${BASH_REMATCH[1]}]" "$@"
fi
exec @WRAPPER@ "$@"
'@.TrimEnd().Replace("@WRAPPER@", (& $q $Wrapper)).Replace("@LATEST@", (& $q $LatestLink)).Replace("@PREFIX@", $DogfoodPrefix)
	$wantPlist = @(
		'<?xml version="1.0" encoding="UTF-8"?>'
		'<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
		'<plist version="1.0">'
		'<dict>'
		"`t<key>CFBundleExecutable</key><string>runterm</string>"
		"`t<key>CFBundleIconFile</key><string>silkterm</string>"
		"`t<key>CFBundleIdentifier</key><string>com.yottacore.silkterm.dogfood</string>"
		"`t<key>CFBundleName</key><string>SilkTerm (dogfood)</string>"
		"`t<key>CFBundlePackageType</key><string>APPL</string>"
		"`t<key>NSHighResolutionCapable</key><true/>"
		'</dict>'
		'</plist>'
	) -join "`n"

	$haveScript = (Get-Content -LiteralPath $script -Raw -ErrorAction SilentlyContinue)
	$havePlist  = (Get-Content -LiteralPath $plist -Raw -ErrorAction SilentlyContinue)
	$haveLink   = (Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue)
	$iconDue    = (Test-Path -LiteralPath $IconPath) -and -not (Test-Path -LiteralPath $icns)
	if ($haveScript -and $haveScript.TrimEnd() -eq $wantScript -and
	    $havePlist -and $havePlist.TrimEnd() -eq $wantPlist -and
	    $haveLink -and $haveLink.LinkTarget -eq $LatestLink -and -not $iconDue) { return }

	try {
		fEnsureDir (Split-Path -Parent $script)
		fEnsureDir (Split-Path -Parent $icns)
		Set-Content -LiteralPath $script -Value $wantScript -Encoding utf8
		Set-Content -LiteralPath $plist  -Value $wantPlist  -Encoding utf8
		& chmod 755 $script
		if ($haveLink) { Remove-Item -LiteralPath $link -Force }
		New-Item -ItemType SymbolicLink -Path $link -Target $LatestLink | Out-Null
		## The logo is wider than it is tall, and an icon has to be square. The
		## padding sips adds is transparent.
		if (Test-Path -LiteralPath $IconPath) {
			$square = Join-Path ([System.IO.Path]::GetTempPath()) "silkterm-icon-$PID.png"
			& sips -p 512 512 $IconPath --out $square *> $null
			& sips -s format icns $square --out $icns *> $null
			Remove-Item -LiteralPath $square -ErrorAction SilentlyContinue
		}
		fNote "refreshed $app"
	} catch {
		fWarn "couldn't write the app bundle ($($_.Exception.Message))"
	}
}


## A .desktop entry that runs the wrapper rather than the terminal - a shortcut
## naming a build pins that build forever. Terminal=false, so the wrapper's own
## console never appears; it exits as soon as the terminal is up.
function fWriteDesktopEntry {
	param([Parameter(Mandatory)][string]$Wrapper)

	$dir  = Join-Path $HomeDir ".local/share/applications"
	$path = Join-Path $dir "silkterm-dogfood.desktop"
	$want = @(
		"[Desktop Entry]"
		"Type=Application"
		"Name=SilkTerm (dogfood)"
		"GenericName=Terminal"
		"Comment=Smooth-scrolling GPU terminal with split panes"
		## Exec= is read twice: the desktop-entry string rules first, then the Exec
		## quoting rules on top, so a backslash ends up as four and a literal '%'
		## has to be doubled or it reads as a field code.
		('Exec="' + (($Wrapper -replace '([\\"`$])', '\$1' -replace '\\', '\\' -replace '%', '%%')) + '"')
		"Icon=$IconPath"
		"Terminal=false"
		"StartupNotify=false"
		"StartupWMClass=silkterm"
		"Categories=System;TerminalEmulator;"
		"Keywords=terminal;shell;prompt;command;"
	) -join "`n"

	$have = (Get-Content -LiteralPath $path -Raw -ErrorAction SilentlyContinue)
	if ($have -and $have.TrimEnd() -eq $want) { return }

	try {
		fEnsureDir $dir
		Set-Content -LiteralPath $path -Value $want -Encoding utf8
		fNote "refreshed $path"
	} catch {
		fWarn "couldn't write the desktop entry ($($_.Exception.Message))"
	}
}


## The Windows equivalent. The icon comes from the symlink itself, which is a real
## Windows binary and carries its own - so it tracks the build with no .ico to keep
## in step.
function fWriteStartMenuLink {
	param([Parameter(Mandatory)][string]$Wrapper)

	$dir  = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
	$path = Join-Path $dir "SilkTerm (dogfood).lnk"

	try {
		fEnsureDir $dir
		$shell = New-Object -ComObject WScript.Shell

		## Adopt an entry that already points at the wrapper, wherever it was filed -
		## these menus get organised by hand, and writing our own name beside one the
		## user already put in a folder of terminals just leaves two of everything.
		## Quick Launch is deliberately not searched: a taskbar pin is not a menu entry.
		foreach ($root in (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu"),
		                  (Join-Path $env:ProgramData "Microsoft\Windows\Start Menu")) {
			if (-not (Test-Path -LiteralPath $root)) { continue }
			$hit = Get-ChildItem -LiteralPath $root -Recurse -Force -Filter *.lnk -ErrorAction SilentlyContinue |
				Where-Object { $shell.CreateShortcut($_.FullName).TargetPath -eq $Wrapper } |
				Select-Object -First 1
			if ($hit) { $path = $hit.FullName; break }
		}

		$link = $shell.CreateShortcut($path)
		if ($link.TargetPath -eq $Wrapper -and $link.IconLocation -eq "$LatestLink,0") { return }
		$link.TargetPath       = $Wrapper
		$link.IconLocation     = "$LatestLink,0"
		$link.WorkingDirectory = $HomeDir
		$link.Description      = "SilkTerm (dogfood)"
		$link.WindowStyle      = 7          # minimized: the wrapper's console, not the terminal
		$link.Save()
		fNote "refreshed $path"
	} catch {
		fWarn "couldn't write the start menu shortcut ($($_.Exception.Message))"
	}
}


## Launch SilkTerm, prepending a title tagged with the build so a dogfood window is
## identifiable. Passed args come last so a caller can still override it.
function fLaunchSilkTerm {
	param(
		[Parameter(Mandatory)][string]$Exe,
		[string[]]$PassArgs
	)

	$leaf  = [System.IO.Path]::GetFileNameWithoutExtension($Exe)
	$title = "SilkTerm"
	if ($leaf -match "^$([regex]::Escape($DogfoodPrefix))_(?<stamp>\d{8}-\d{6})_(?<tag>[a-z0-9]+)") {
		$title = "SilkTerm [dogfood $($Matches.tag) $($Matches.stamp)]"
	}

	## Picking a wallpaper here is disabled: the terminal rotates its own now, and
	## one named on the command line pins it for the session, hiding exactly what
	## we want to see.
	$all = @("--title=$title")
	if ($PassArgs) { $all += $PassArgs }

	return fStartTerminal -Exe $Exe -ArgList $all
}


## Fall back to whatever terminal is installed, in $FallbackTerminals order. Ours
## keeps the tagged title; the rest get no arguments, since SilkTerm's options
## would not parse for them.
function fLaunchFallbackTerminal {
	param([string[]]$PassArgs)

	foreach ($cand in $FallbackTerminals) {
		$path = fFindOnPath $cand.Exe
		if (-not $path) { continue }
		fNote "falling back to $($cand.Exe): $path"
		if ($cand.Silk) { return fLaunchSilkTerm -Exe $path -PassArgs $PassArgs }
		return fStartTerminal -Exe $path -ArgList @()
	}

	fFail ("no terminal available (no build, no source, and none of " +
		(($FallbackTerminals | ForEach-Object { $_.Exe }) -join ", ") + " installed)")
}


## Resolve an executable's full path, or $null. -CommandType Application keeps it
## to real programs, never a shell function or alias of the same name.
function fFindOnPath {
	param([Parameter(Mandatory)][string]$Exe)
	$cmd = Get-Command $Exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
	if ($cmd) { return $cmd.Source }
	return $null
}


## Join arguments into one command line the way CommandLineToArgvW reads it back,
## which is also how .NET splits a command line on Linux and macOS. Start-Process
## joins a list with spaces and escapes nothing, so wrapping a spaced argument in
## quotes is not enough: an argument holding a quote gets cut in two, an empty one
## vanishes, and a trailing backslash eats the closing quote.
function fJoinArgs {
	param([string[]]$ArgList)

	$out = foreach ($a in $ArgList) {
		if ($a -ne '' -and $a -notmatch '[\s"]') { $a; continue }
		$q = '"'
		$slashes = 0
		foreach ($ch in $a.ToCharArray()) {
			if ($ch -eq '\') { $slashes++; continue }
			## A quote needs every backslash before it doubled, and itself escaped.
			if ($ch -eq '"') { $q += ('\' * ($slashes * 2 + 1)) + '"' }
			else             { $q += ('\' * $slashes) + $ch }
			$slashes = 0
		}
		## Same for the closing quote we are about to add.
		$q + ('\' * ($slashes * 2)) + '"'
	}
	return ($out -join ' ')
}


## Launch a terminal in its own process, elevated when $RunAsAdmin. Returns the
## Process so a caller can stop this exact instance by PID - matching on a name or
## a pattern risks hitting a copy started somewhere else.
function fStartTerminal {
	param(
		[Parameter(Mandatory)][string]$Exe,
		[string[]]$ArgList
	)

	$sp = @{ FilePath = $Exe; PassThru = $true }
	if ($ArgList -and $ArgList.Count) { $sp.ArgumentList = (fJoinArgs $ArgList) }
	if ($RunAsAdmin) { $sp.Verb = "RunAs" }

	try {
		$proc = Start-Process @sp
	} catch {
		fFail "launch failed for $Exe ($($_.Exception.Message))"
	}

	$how = if ($RunAsAdmin) { " (as admin)" } else { "" }
	fNote "launched$how pid $($proc.Id): $([System.IO.Path]::GetFileName($Exe))"
	return $proc
}


function fEnsureDir {
	param([Parameter(Mandatory)][string]$Path)
	if (-not (Test-Path -LiteralPath $Path)) {
		New-Item -ItemType Directory -Path $Path -Force | Out-Null
	}
}


## Informational note to the host and the run log.
function fNote { param([string]$Msg); fLog $Msg; Write-Host "runterm: $Msg" }

## Non-fatal note. Pass -Gui to also surface it in the end-of-run dialog (the
## shortcut case, where the console flashes shut); reserved for real problems.
function fWarn {
	param([string]$Msg, [switch]$Gui)
	fLog "WARN: $Msg"
	Write-Warning "runterm: $Msg"
	if ($Gui) { $script:RunWarnings += $Msg }
}

## Fatal error, then stop. Pops a dialog first when GUI feedback is on, so a
## shortcut click shows why instead of a blank flash.
function fFail {
	param([string]$Msg)
	fLog "FAIL: $Msg"
	if ($script:GuiFeedback) { fGuiShow -Msg $Msg -Icon Error -Title "SilkTerm dogfood - failed" }
	Write-Error "runterm: $Msg"
	exit 1
}


## Append a timestamped line to the run log. Best-effort: logging must never be the
## thing that stops a launch.
function fLog {
	param([string]$Msg)
	try {
		Add-Content -LiteralPath $RunLog -Encoding utf8 -Value `
			("{0}  {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $Msg)
	} catch { }
}


## Keep the run log from growing without bound.
function fTrimLog {
	try {
		if ((Test-Path -LiteralPath $RunLog) -and (Get-Item -LiteralPath $RunLog).Length -gt $RunLogMaxSize) {
			$tail = Get-Content -LiteralPath $RunLog -Tail 500
			Set-Content -LiteralPath $RunLog -Value $tail -Encoding utf8
		}
	} catch { }
}


## Remove any mark-of-the-web this script picked up from the sync layer. An
## unsigned script carrying one is refused under RemoteSigned, which silently kills
## a shortcut click - the body never runs, so nothing copies and nothing logs. This
## only helps the NEXT run; the current one already cleared the policy to be here.
function fSelfHealMotw {
	try {
		$zone = Get-Content -LiteralPath $PSCommandPath -Stream Zone.Identifier -ErrorAction SilentlyContinue
		if ($zone) {
			Unblock-File -LiteralPath $PSCommandPath -ErrorAction Stop
			fNote "cleared mark-of-the-web on this script"
		}
	} catch {
		fWarn "couldn't clear mark-of-the-web on this script ($($_.Exception.Message))"
	}
}


## True when this process is running elevated.
function fIsElevated {
	if ($Platform -ne "windows") { return $true }
	$id = [System.Security.Principal.WindowsIdentity]::GetCurrent()
	return (New-Object System.Security.Principal.WindowsPrincipal($id)).IsInRole(
		[System.Security.Principal.WindowsBuiltInRole]::Administrator)
}


## True when we were double-clicked rather than started from a shell - Explorer is
## the parent of a shortcut click, a terminal is the parent of a command-line run.
## Used to auto-enable GUI feedback so a flash-and-close click can still report.
function fLaunchedFromShortcut {
	if ($Platform -ne "windows") { return $false }
	try {
		$parentId = (Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction Stop).ParentProcessId
		$parent   = (Get-Process -Id $parentId -ErrorAction Stop).ProcessName
		return ($parent -ieq "explorer")
	} catch { return $false }
}


## Show a modal message box. Never throws - feedback must not be the thing that
## breaks a launch; a no-op where WinForms cannot load.
function fGuiShow {
	param(
		[Parameter(Mandatory)][string]$Msg,
		[ValidateSet("Error", "Warning", "Information")][string]$Icon = "Information",
		[string]$Title = "SilkTerm dogfood"
	)
	try {
		Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
		[System.Windows.Forms.MessageBox]::Show(
			$Msg, $Title,
			[System.Windows.Forms.MessageBoxButtons]::OK,
			[System.Windows.Forms.MessageBoxIcon]::$Icon) | Out-Null
	} catch { }
}


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Script entry point

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

## Problems worth surfacing at the end, shown in a dialog when launched from a
## shortcut. Must exist before any fWarn -Gui / fFail can run.
$script:RunWarnings = @()

## Consume our own flags; forward everything else to the terminal.
##   --no-admin  run without elevating (Windows only; elsewhere there is nothing
##               to elevate and the flag is accepted and ignored).
##   --gui       force the failure dialog on; auto-on for a shortcut click.
##   --install-only  copy, rotate and refresh the menu entry, then stop without
##               opening a terminal. For an unattended run, and for any session
##               with no desktop to put a window on.
$wantAdmin   = $true
$forceGui    = $false
$installOnly = $false
$passArgs    = New-Object System.Collections.Generic.List[string]
foreach ($arg in $args) {
	switch -Regex ($arg) {
		'^--admin$'        { $wantAdmin   = $true;  continue }
		'^--no-admin$'     { $wantAdmin   = $false; continue }
		'^--gui$'          { $forceGui    = $true;  continue }
		'^--install-only$' { $installOnly = $true;  continue }
		default        { $passArgs.Add($arg) }
	}
}

$script:GuiFeedback = $forceGui -or (fLaunchedFromShortcut)

## Self-elevate: relaunch the whole script elevated and hand off, so the copy, the
## symlink and the launched terminal all run high-integrity. Minimized, because the
## relaunch has no console of its own to reuse. The relaunch carries '--gui' - its
## parent is the UAC broker, not Explorer, so it cannot re-detect the shortcut. A
## declined consent does NOT abort: we fall through and run unelevated so there is
## still a terminal, with a dialog saying it may be stale.
if ($Platform -eq "windows" -and $wantAdmin -and -not (fIsElevated)) {
	$self = (Get-Process -Id $PID).Path
	$fwd  = fJoinArgs (@("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $PSCommandPath) + $args + "--gui")
	try {
		Start-Process -FilePath $self -Verb RunAs -WindowStyle Minimized -ArgumentList $fwd -ErrorAction Stop | Out-Null
		exit 0
	} catch {
		fWarn "elevation declined; running without admin (a newer build may not copy)"
		if ($script:GuiFeedback) {
			fGuiShow -Icon Warning -Title "SilkTerm dogfood - not elevated" -Msg (
				"Administrator access was declined.`n`nRunning without it - a newer " +
				"build may not copy in, so an older one could launch.")
		}
	}
}

if ($Platform -eq "windows" -and $wantAdmin) { $RunAsAdmin = $true }

fMain -PassArgs $passArgs -InstallOnly:$installOnly

if ($script:GuiFeedback -and $script:RunWarnings.Count) {
	fGuiShow -Icon Warning -Title "SilkTerm dogfood" -Msg (
		"Launched, but with issues:`n`n - " + ($script:RunWarnings -join "`n - "))
}


##	History:
##		- 2026-10-06: Help block; no array growth in loops.
##		- 2026-10-01: An app in ~/Applications is the menu entry on a Mac. It runs
##		  the build in its own process, so the Dock sees one program.
##		- 2026-09-08: Refresh a menu entry that already points at the wrapper
##		  wherever it was filed, rather than always adding one of our own.
##		- 2026-09-08: Name the Dropbox spelling beside 'synced' for both the build
##		  source and the wrapper - on Windows the junction reads empty. Added
##		  '--install-only'.
##		- 2026-09-08: Runs directly on Linux and macOS - shebang first, no BOM,
##		  LF endings. Stopped a held-build rename printing its path.
##		- 2026-09-07: One cross-platform implementation, replacing the Windows-only
##		  script and the separate bash launcher. Reads only the synced app dir, so
##		  the network source and its bounded waits are gone. Copies GFS-rotate in a
##		  versions folder behind a '<program>' symlink, which also carries the icon
##		  a .desktop entry points at.
##		- 2026-09-01: Elevate by default; '--no-admin' opts out. A filtered token
##		  has no SeCreateSymbolicLinkPrivilege, so an unelevated shell can't make a
##		  symlink at all.
##		- 2026-08-24: Tell builds apart by their bytes, not their mtime - copies of
##		  one build disagreed on it, so the same binary kept getting copied in again
##		  under a second tag. A match keeps its own tag and takes the newer stamp.
##		- 2026-08-23: Added the synced dogfood dir as a fourth source.
##		- 2026-08-06: Bound the wait on a network source instead of sitting through
##		  the SMB timeout when b23 is off or the link drops.
##		- 2026-08-02: Stop picking a wallpaper; the terminal rotates its own.
##		- 2026-08-01: Retag copies '<toolchain><built on><target><arch>'.
##		- 2026-07-22: Resolve the local clone root from a per-host candidate list.
##		- 2026-07-19: '--admin' self-elevates the whole launcher; report failures in
##		  a dialog for the shortcut case.
##		- 2026-07-17: Strip a synced-on mark-of-the-web at startup.
##		- 2026-07-17: Log every run's decisions beside the copies.
##		- 2026-07-15: Age-prune, tagged sources, fallback terminals.
##		- 2026-07-14: Created (Windows port of the bash n8runterm).
