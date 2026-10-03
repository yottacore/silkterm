#!/usr/bin/env pwsh

##	Purpose:
##		- Drive n8runterm.ps1 in a sandboxed HOME and check what it does to files
##		  it did not create. install.bash puts a release build at the same path
##		  the launcher wants for its symlink, and the launcher used to delete it.
##		- Also: that every argument reaches the terminal exactly as given, and
##		  that a build already held is recognised without reading it again.
##		- And the pool: what is copied in and what is declined, what the rotation
##		  keeps, the leftover of a cut-off copy, the Dropbox spelling of the
##		  source, the window title, and the fallback when there is no build.
##		- Nothing here touches the real home directory or the real pool.
##	Test ID: EpHRcSG
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use

$Launcher = Join-Path (Split-Path $PSScriptRoot -Parent | Split-Path -Parent | Split-Path -Parent) "utility/n8runterm.ps1"
if (-not (Test-Path -LiteralPath $Launcher)) { throw "no launcher at $Launcher" }

## By full path, since the fallback case narrows PATH to a single stub.
$Pwsh = (Get-Process -Id $PID).Path

$script:Failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" }
	else     { Write-Host "  FAIL $What"; $script:Failures++ }
}

## A sandbox home with nothing in it.
function fBareSandbox {
	$root = Join-Path ([System.IO.Path]::GetTempPath()) "silkterm-launcher-test-$PID-$(Get-Random)"
	New-Item -ItemType Directory -Path $root -Force | Out-Null
	return $root
}

## A sandbox home with a source dir holding one "build".
function fSandbox {
	$root = fBareSandbox
	$src  = Join-Path $root "synced/0-0/common/exec/app/linux"
	New-Item -ItemType Directory -Path $src -Force | Out-Null
	Set-Content -LiteralPath (Join-Path $src "silkterm") -Value "dogfood build" -NoNewline
	Set-Content -LiteralPath (Join-Path $src "silkterm.tag") -Value "gnulli" -NoNewline
	if (-not $IsWindows) { chmod +x (Join-Path $src "silkterm") }
	return $root
}

## On Windows the launcher keeps its pool under LOCALAPPDATA and its Start menu
## entry under APPDATA, so both move into the sandbox along with home.
function fUseHome { param([string]$Home_)
	$env:HOME = $Home_
	$env:USERPROFILE = $Home_
	$env:LOCALAPPDATA = Join-Path $Home_ "AppData/Local"
	$env:APPDATA = Join-Path $Home_ "AppData/Roaming"
}

## Where the launcher puts the pool, the symlink and its log.
function fBin { param([string]$Home_)
	if ($IsWindows) { return (Join-Path $Home_ "AppData/Local/Programs") }
	return (Join-Path $Home_ ".local/bin")
}

function fRun { param([string]$Home_)
	fUseHome $Home_
	& $Pwsh -NoProfile -File $Launcher --install-only 2>&1 | Out-Null
}

## Same, but hands back what the launcher said, and takes extra arguments.
function fRunSaying { param([string]$Home_, [string[]]$Extra = @())
	fUseHome $Home_
	return (& $Pwsh -NoProfile -File $Launcher @Extra 2>&1 | Out-String)
}

function fPool { param([string]$Home_) return (Join-Path (fBin $Home_) "silkterm_versions") }
function fRunLog { param([string]$Home_) return [string](Get-Content -LiteralPath (Join-Path (fBin $Home_) "runterm.log") -Raw -ErrorAction SilentlyContinue) }
function fStamp { param([datetime]$When) return $When.ToString("yyyyMMdd-HHmmss") }

## Plant a held copy. Without -Text it is $Bytes of nothing, sparse, so a big one
## costs no disk.
function fHold { param([string]$Home_, [datetime]$When, [long]$Bytes = 16, [string]$Text = "")
	$dir = fPool $Home_
	New-Item -ItemType Directory -Path $dir -Force | Out-Null
	$path = Join-Path $dir "slktrmdf_$(fStamp $When)_gnulli_frequent"
	if ($Text) {
		Set-Content -LiteralPath $path -Value $Text -NoNewline
	} else {
		$fs = [System.IO.File]::Create($path)
		try { $fs.SetLength($Bytes) } finally { $fs.Dispose() }
	}
	return $path
}

## Held copies, newest first, and the stamp in each name.
function fHeld { param([string]$Home_)
	return @(Get-ChildItem -LiteralPath (fPool $Home_) -File -ErrorAction SilentlyContinue |
		Where-Object { $_.Name -match '^slktrmdf_\d{8}-\d{6}_[a-z0-9]+_[a-z]+$' } |
		Sort-Object Name -Descending)
}
function fHeldStamps { param([string]$Home_) return @(fHeld $Home_ | ForEach-Object { ($_.Name -split '_')[1] }) }

function fLinkTarget { param([string]$Home_)
	$link = Get-Item -LiteralPath (Join-Path (fBin $Home_) "silkterm") -Force -ErrorAction SilentlyContinue
	if ($link) { return $link.LinkTarget }
	return $null
}

## Terminals start detached, so give a file they write a moment to appear.
function fWaitFor { param([string]$Path)
	for ($i = 0; $i -lt 25 -and -not (Test-Path -LiteralPath $Path); $i++) { Start-Sleep -Milliseconds 200 }
	return (Test-Path -LiteralPath $Path)
}

## A build that records the arguments it was handed, one per line.
function fRecordingBuild { param([string]$Path)
	if ($IsWindows) {
		Set-Content -LiteralPath $Path -Value @(
			'@echo off'
			'break > "%HOME%\argv.txt"'
			':loop'
			'if "%~1"=="" goto done'
			'echo %~1>> "%HOME%\argv.txt"'
			'shift'
			'goto loop'
			':done'
		)
	} else {
		Set-Content -LiteralPath $Path -Value @(
			'#!/bin/sh'
			': > "$HOME/argv.txt"'
			'for a in "$@"; do printf "%s\n" "$a" >> "$HOME/argv.txt"; done'
		)
		chmod +x $Path
	}
}

$realHome    = $env:HOME
$realProfile = $env:USERPROFILE
$realLocal   = $env:LOCALAPPDATA
$realRoaming = $env:APPDATA
$realPath    = $env:PATH
## Nothing here may open a window on the desktop, whatever the launcher falls back to.
$realDisplay = $env:DISPLAY
$realWayland = $env:WAYLAND_DISPLAY
Remove-Item Env:DISPLAY, Env:WAYLAND_DISPLAY -ErrorAction SilentlyContinue
try {
	Write-Host "a release build already installed at the symlink path"
	$root = fSandbox
	$bin = fBin $root
	New-Item -ItemType Directory -Path $bin -Force | Out-Null
	$installed = Join-Path $bin "silkterm"
	Set-Content -LiteralPath $installed -Value "a release build somebody installed" -NoNewline
	fRun $root
	## A Windows run would otherwise rotate the real pool.
	$inside = { param([string]$Path) $Path -and $Path.StartsWith($root) }
	fCheck "every folder the launcher writes to is in the sandbox" ((& $inside $env:HOME) -and (& $inside $env:USERPROFILE) -and (& $inside $env:LOCALAPPDATA) -and (& $inside $env:APPDATA))
	fCheck "the installed build is still there" (Test-Path -LiteralPath $installed)
	fCheck "and is still itself" ((Get-Content -LiteralPath $installed -Raw) -eq "a release build somebody installed")
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	Write-Host "nothing installed at the symlink path"
	$root = fSandbox
	fRun $root
	$link = Join-Path (fBin $root) "silkterm"
	fCheck "the launcher put its own silkterm there" (Test-Path -LiteralPath $link)
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## Start-Process joins an argument list with spaces and escapes nothing, so a
	## quote used to cut an argument in two, an empty one vanished, and a trailing
	## backslash took the closing quote with it and joined the next argument.
	Write-Host "arguments reach the terminal as given"
	$root = fSandbox
	$src = Join-Path $root "synced/0-0/common/exec/app/linux/silkterm"
	fRecordingBuild $src
	$stamp = fStamp (Get-Item -LiteralPath $src).LastWriteTime
	$want = @('--shell', 'bash -c "echo hi"', '', 'C:\my dir\', 'plain')
	$null = fRunSaying $root $want
	$argvFile = Join-Path $root "argv.txt"
	$got = @()
	$title = ""
	if (fWaitFor $argvFile) {
		## The launcher's own --title comes first; everything after it is ours.
		$got = @(Get-Content -LiteralPath $argvFile)
		if ($got.Count -ge 1 -and $got[0] -like '--title=*') { $title = $got[0]; $got = @($got[1..($got.Count - 1)]) }
	}
	fCheck "every argument arrives unchanged" (($got -join "`u{241F}") -eq ($want -join "`u{241F}"))
	if (($got -join "`u{241F}") -ne ($want -join "`u{241F}")) {
		Write-Host "    wanted: $($want | ForEach-Object { "[$_]" })"
		Write-Host "    got   : $($got  | ForEach-Object { "[$_]" })"
	}
	fCheck "the title names the build's tag and date ($title)" ($title -ceq "--title=SilkTerm [dogfood gnulli $stamp]")
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## A build from before cicd wrote the sidecar still runs, and the title says
	## what the launcher could see instead.
	Write-Host "a build with no tag beside it"
	$root = fSandbox
	Remove-Item -LiteralPath (Join-Path $root "synced/0-0/common/exec/app/linux/silkterm.tag")
	fRecordingBuild (Join-Path $root "synced/0-0/common/exec/app/linux/silkterm")
	$null = fRunSaying $root @()
	$argvFile = Join-Path $root "argv.txt"
	$got = @(if (fWaitFor $argvFile) { Get-Content -LiteralPath $argvFile })
	fCheck "still launches" ($got.Count -ge 1)
	fCheck "with a title naming what it could see ($got)" ($got.Count -ge 1 -and
		$got[0] -cmatch '^--title=SilkTerm \[dogfood gnux[lmw][ia] \d{8}-\d{6}\]$')
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## The held stamp is a name in whole seconds. Compared against a source mtime
	## carrying a fraction, it always read as older, so every launch hashed the
	## whole binary to rediscover the copy it was already holding.
	Write-Host "a source whose mtime has a fraction is still recognised"
	$root = fSandbox
	$src = Join-Path $root "synced/0-0/common/exec/app/linux/silkterm"
	(Get-Item -LiteralPath $src).LastWriteTime = (Get-Date).Date.AddHours(10).AddMilliseconds(500)
	$null = fRunSaying $root @('--install-only')
	$second = fRunSaying $root @('--install-only')
	fCheck "the second launch says it is already current" ($second -match 'already current')
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	Write-Host "a newer build is copied in, an older one declined, and the log says which"
	$root = fSandbox
	$src = Join-Path $root "synced/0-0/common/exec/app/linux/silkterm"
	$built = (Get-Date).Date.AddHours(9)
	(Get-Item -LiteralPath $src).LastWriteTime = $built
	$null = fHold $root $built.AddDays(-1) -Text "yesterday's build"
	fRun $root
	fCheck "the newer build is copied in beside the one held" ((fHeldStamps $root) -join ' ' -eq "$(fStamp $built) $(fStamp $built.AddDays(-1))")
	fCheck "and the log says so" ((fRunLog $root) -match "copied -> slktrmdf_$(fStamp $built)_gnulli_newest")
	Set-Content -LiteralPath $src -Value "an older build" -NoNewline
	(Get-Item -LiteralPath $src).LastWriteTime = $built.AddDays(-3)
	fRun $root
	fCheck "an older source is declined" (@(fHeldStamps $root).Count -eq 2)
	fCheck "and the log says why" ((fRunLog $root) -match "already current \(held $(fStamp $built), source $(fStamp $built.AddDays(-3))\)")
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## cicd dates its copy and Dropbox restamps what it syncs, so one build turns up
	## with more than one date.
	Write-Host "one build arriving with two dates is held once"
	$root = fSandbox
	$src = Join-Path $root "synced/0-0/common/exec/app/linux/silkterm"
	$built = (Get-Date).Date.AddHours(9)
	(Get-Item -LiteralPath $src).LastWriteTime = $built
	fRun $root
	fCheck "the copy is named for the build's date" ((fHeldStamps $root) -join ' ' -eq (fStamp $built))
	(Get-Item -LiteralPath $src).LastWriteTime = $built.AddMinutes(8)
	fRun $root
	fCheck "the same bytes with a later date are not copied again" (@(fHeldStamps $root).Count -eq 1)
	fCheck "the one copy takes the later date" ((fHeldStamps $root) -join ' ' -eq (fStamp $built.AddMinutes(8)))
	fCheck "and the log says it is the same build" ((fRunLog $root) -match 'same build as')
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## A quiet stretch used to age every copy out, and the launch fell back to
	## another terminal.
	Write-Host "a pool where every copy is years old"
	$root = fBareSandbox
	$now = Get-Date
	foreach ($years in 3..8) { $null = fHold $root $now.AddYears(-$years) }
	fRun $root
	$newest = fStamp $now.AddYears(-3)
	fCheck "the newest copy is kept" ((fHeldStamps $root) -contains $newest)
	fCheck "and the symlink points at it" ("$(fLinkTarget $root)" -like "*slktrmdf_${newest}_*")
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	Write-Host "the pool keeps a spread of builds, and never one that is running"
	$root = fBareSandbox
	$now = Get-Date
	$ages = @(0, 1, 2, 3, 5, 8, 12, 20, 35, 50, 70, 100, 150, 250, 400)
	foreach ($days in $ages) { $null = fHold $root $now.AddDays(-$days).AddMinutes(-10) }
	## A second copy from the 250-day copy's day, a second older. No period ever
	## picks it and the budget is spent long before the fill reaches it, so only
	## running can keep it.
	$runningStamp = fStamp $now.AddDays(-250).AddMinutes(-10).AddSeconds(-1)
	$runner = $null
	if (-not $IsWindows) {
		$exe = fHold $root $now.AddDays(-250).AddMinutes(-10).AddSeconds(-1)
		Copy-Item -LiteralPath (Get-Command sleep -CommandType Application | Select-Object -First 1).Source -Destination $exe -Force
		chmod +x $exe
		$runner = Start-Process -FilePath $exe -ArgumentList 120 -PassThru
	}
	try {
		fRun $root
	} finally {
		if ($runner) { Stop-Process -Id $runner.Id -Force -ErrorAction SilentlyContinue }
	}
	$stamps = @(fHeldStamps $root)
	$idle = @($stamps | Where-Object { $_ -ne $runningStamp })
	fCheck "between five and ten idle copies are kept ($($idle.Count))" ($idle.Count -ge 5 -and $idle.Count -le 10)
	fCheck "the newest is one of them" ($stamps -contains (fStamp $now.AddMinutes(-10)))
	fCheck "and so is the oldest" ($stamps -contains (fStamp $now.AddDays(-400).AddMinutes(-10)))
	if ($runner) { fCheck "the running copy is kept" ($stamps -contains $runningStamp) }
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	Write-Host "the pool stops at 1 GB once it holds five"
	$root = fBareSandbox
	$now = Get-Date
	foreach ($i in 0..9) { $null = fHold $root $now.AddDays(-3 * $i).AddMinutes(-10) -Bytes 150MB }
	fRun $root
	$held = @(fHeld $root)
	$bytes = ($held | Measure-Object -Property Length -Sum).Sum
	## Five are kept whatever they weigh; at 150 MB a sixth still fits and a seventh does not.
	fCheck "six of ten are kept ($($held.Count))" ($held.Count -eq 6)
	fCheck "within 1 GB" ($bytes -le 1GB)
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## '.partial' is what a copy is written under until it is whole.
	Write-Host "the leftover of a cut-off copy is swept, never launched"
	$root = fSandbox
	New-Item -ItemType Directory -Path (fPool $root) -Force | Out-Null
	## Dated past the source, so only the sweep can take it.
	$partial = Join-Path (fPool $root) "slktrmdf_$(fStamp (Get-Date).AddDays(1))_gnulli_newest.partial"
	Set-Content -LiteralPath $partial -Value "half a build" -NoNewline
	fRun $root
	fCheck "the leftover is gone" (-not (Test-Path -LiteralPath $partial))
	$target = fLinkTarget $root
	fCheck "and the symlink points at the whole build" ($target -and $target -notlike '*.partial' -and
		(Test-Path -LiteralPath $target) -and (Get-Content -LiteralPath $target -Raw) -eq "dogfood build")
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## On Windows 'synced' is a junction that reads as an empty folder, so the
	## build is only found under the Dropbox spelling.
	Write-Host "a build found only under the Dropbox spelling"
	$root = fBareSandbox
	New-Item -ItemType Directory -Path (Join-Path $root "synced") -Force | Out-Null
	$srcDir = Join-Path $root "Dropbox/0-0/common/exec/app/linux"
	New-Item -ItemType Directory -Path $srcDir -Force | Out-Null
	Set-Content -LiteralPath (Join-Path $srcDir "silkterm") -Value "dogfood build via Dropbox" -NoNewline
	Set-Content -LiteralPath (Join-Path $srcDir "silkterm.tag") -Value "gnulli" -NoNewline
	if (-not $IsWindows) { chmod +x (Join-Path $srcDir "silkterm") }
	fRun $root
	$held = @(fHeld $root)
	fCheck "it is copied in" ($held.Count -eq 1 -and (Get-Content -LiteralPath $held[0].FullName -Raw) -eq "dogfood build via Dropbox")
	fCheck "and the symlink points at it" ($held.Count -eq 1 -and (fLinkTarget $root) -eq $held[0].FullName)
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## PATH holds nothing but a stand-in terminal that records what it was handed.
	if (-not $IsWindows) {
		Write-Host "no build anywhere falls back to an installed terminal"
		$root = fBareSandbox
		$stubs = Join-Path $root "stubs"
		New-Item -ItemType Directory -Path $stubs -Force | Out-Null
		$stub = Join-Path $stubs "xfce4-terminal"
		Set-Content -LiteralPath $stub -Value @('#!/bin/sh', 'printf "%s\n" "$#" "$@" > "$HOME/fallback.txt"')
		chmod +x $stub
		$env:PATH = $stubs
		try { $null = fRunSaying $root @('--shell', 'bash') } finally { $env:PATH = $realPath }
		$ran = fWaitFor (Join-Path $root "fallback.txt")
		fCheck "the terminal is started" $ran
		fCheck "with none of SilkTerm's arguments" ($ran -and (Get-Content -LiteralPath (Join-Path $root "fallback.txt") -Raw).Trim() -eq "0")
		Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
	}
} finally {
	$env:HOME = $realHome
	$env:USERPROFILE = $realProfile
	$env:LOCALAPPDATA = $realLocal
	$env:APPDATA = $realRoaming
	$env:PATH = $realPath
	if ($null -ne $realDisplay) { $env:DISPLAY = $realDisplay }
	if ($null -ne $realWayland) { $env:WAYLAND_DISPLAY = $realWayland }
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) failed"; fTestDir_End 1; exit 1 }
Write-Host "all passed"
fTestDir_End 0

##	History:
##		- 2026-09-28: LOCALAPPDATA and APPDATA are sandboxed too.
##		- 2026-09-26: Pool cases: copy and decline, rotation, the byte cap, the
##		  swept leftover, the Dropbox spelling, the title and the fallback.
##		- 2026-09-08: Created.
