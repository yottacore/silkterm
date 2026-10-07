#!/usr/bin/env pwsh

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

<#
.SYNOPSIS
	Drive n8runterm.ps1 on Windows in a sandboxed profile and check its Start
	menu entry, and what a launch with nothing to do asks Windows for.
.DESCRIPTION
	The launcher adopts an entry that already runs its wrapper, wherever it was
	filed, and writes its own only when there is none. A launch used to find it
	by opening every shortcut in both Start menus, and asked WMI for its parent
	and read every process's path as well. After the first launch, one that has
	nothing to change does none of those.

	The launcher runs through a script that counts calls to Get-CimInstance, to
	a Get-ChildItem for shortcuts and to a whole Get-Process list, then hands
	each to the real command.

	Windows only; elsewhere it says so and passes.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of script.
#>

##	Test ID: EryVaSD

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $IsWindows) { Write-Host "  skip Start menu entry (Windows only)"; exit 0 }

. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use

$Launcher = Join-Path (Split-Path $PSScriptRoot -Parent | Split-Path -Parent | Split-Path -Parent) "utility/n8runterm.ps1"
if (-not (Test-Path -LiteralPath $Launcher)) { throw "no launcher at $Launcher" }
$Pwsh = (Get-Process -Id $PID).Path

$script:Failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" }
	else     { Write-Host "  FAIL $What"; $script:Failures++ }
}

## A sandbox profile with one build and the wrapper a shortcut should run.
## ProgramData moves too, since the all users' Start menu is searched.
function fSandbox {
	$root = Join-Path ([System.IO.Path]::GetTempPath()) "silkterm-startmenu-test-$PID-$(Get-Random)"
	$src  = Join-Path $root "Dropbox\0-0\common\exec\app\mswin"
	$wrap = Join-Path $root "Dropbox\0-0\common\exec\util\mswin\cli\by-self\cmd"
	New-Item -ItemType Directory -Path $src, $wrap -Force | Out-Null
	Set-Content -LiteralPath (Join-Path $src "silkterm.exe") -Value "dogfood build" -NoNewline
	Set-Content -LiteralPath (Join-Path $src "silkterm.exe.tag") -Value "gnulwi" -NoNewline
	Set-Content -LiteralPath (Join-Path $wrap "runterm.cmd") -Value "@echo off"
	return $root
}

function fWrapper { param([string]$Root) return (Join-Path $Root "Dropbox\0-0\common\exec\util\mswin\cli\by-self\cmd\runterm.cmd") }
function fUserMenu { param([string]$Root) return (Join-Path $Root "AppData\Roaming\Microsoft\Windows\Start Menu") }
function fAllMenu { param([string]$Root) return (Join-Path $Root "ProgramData\Microsoft\Windows\Start Menu") }
function fDefaultEntry { param([string]$Root) return (Join-Path (fUserMenu $Root) "Programs\SilkTerm (dogfood).lnk") }

function fUseHome { param([string]$Root)
	$env:HOME = $Root
	$env:USERPROFILE = $Root
	$env:LOCALAPPDATA = Join-Path $Root "AppData\Local"
	$env:APPDATA = Join-Path $Root "AppData\Roaming"
	$env:ProgramData = Join-Path $Root "ProgramData"
}

## One launch, as a shortcut's wrapper would make it but with nothing opened.
## Answers what it asked for: { Cim; Walks; ProcessLists }.
function fLaunch { param([string]$Root)
	fUseHome $Root
	$tally = Join-Path $Root "tally.txt"
	Remove-Item -LiteralPath $tally -ErrorAction SilentlyContinue
	$counting = Join-Path $Root "counting.ps1"
	Set-Content -LiteralPath $counting -Value @(
		'param([string]$Launcher)'
		'function Get-CimInstance { Add-Content -LiteralPath $env:SILK_TALLY -Value cim; CimCmdlets\Get-CimInstance @args }'
		'function Get-ChildItem { if ($args -contains "*.lnk") { Add-Content -LiteralPath $env:SILK_TALLY -Value walk }; Microsoft.PowerShell.Management\Get-ChildItem @args }'
		'function Get-Process { if ($args -notcontains "-Id") { Add-Content -LiteralPath $env:SILK_TALLY -Value processes }; Microsoft.PowerShell.Management\Get-Process @args }'
		'& $Launcher --install-only --no-admin'
	)
	$env:SILK_TALLY = $tally
	try { & $Pwsh -NoProfile -File $counting -Launcher $Launcher 2>&1 | Out-Null } finally { Remove-Item -Path Env:SILK_TALLY }
	$said = @(Get-Content -LiteralPath $tally -ErrorAction SilentlyContinue)
	return [pscustomobject]@{
		Cim          = @($said | Where-Object { $_ -eq "cim" }).Count
		Walks        = @($said | Where-Object { $_ -eq "walk" }).Count
		ProcessLists = @($said | Where-Object { $_ -eq "processes" }).Count
	}
}

## A shortcut to $Target at $Path.
function fShortcut { param([string]$Path, [string]$Target, [string]$Icon = "")
	New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
	$link = (New-Object -ComObject WScript.Shell).CreateShortcut($Path)
	$link.TargetPath = $Target
	if ($Icon) { $link.IconLocation = $Icon }
	$link.Save()
}

function fEntry { param([string]$Path) return (New-Object -ComObject WScript.Shell).CreateShortcut($Path) }

## Every shortcut in both menus that runs the wrapper.
function fEntriesRunning { param([string]$Root)
	$shell = New-Object -ComObject WScript.Shell
	$wrapper = fWrapper $Root
	return @(foreach ($menu in (fUserMenu $Root), (fAllMenu $Root)) {
		if (-not (Test-Path -LiteralPath $menu)) { continue }
		Get-ChildItem -LiteralPath $menu -Recurse -Filter *.lnk |
			Where-Object { $shell.CreateShortcut($_.FullName).TargetPath -eq $wrapper } |
			ForEach-Object { $_.FullName }
	})
}

$saved = @{}
foreach ($name in "HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "ProgramData") {
	$saved[$name] = [Environment]::GetEnvironmentVariable($name)
}
try {
	Write-Host "no entry anywhere"
	$root = fSandbox
	$first = fLaunch $root
	$entry = fDefaultEntry $root
	fCheck "the first launch searches the Start menus ($($first.Walks))" ($first.Walks -ge 1)
	fCheck "and writes its own entry" ((Test-Path -LiteralPath $entry) -and (fEntry $entry).TargetPath -eq (fWrapper $root))
	$link = fEntry $entry
	fCheck "which names no file in the versions folder" ($link.TargetPath -notmatch 'silkterm_versions' -and
		$link.IconLocation -notmatch 'silkterm_versions' -and $link.IconLocation -like '*\silkterm.exe,0')
	$next = fLaunch $root
	fCheck "the next launch asks WMI nothing ($($next.Cim))" ($next.Cim -eq 0)
	fCheck "searches no Start menu ($($next.Walks))" ($next.Walks -eq 0)
	fCheck "and reads no process list ($($next.ProcessLists))" ($next.ProcessLists -eq 0)
	fCheck "one entry runs the wrapper" (@(fEntriesRunning $root).Count -eq 1)
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## Filed by hand under all users, so a search opens the whole user menu first.
	Write-Host "an entry filed by hand in a folder"
	$root = fSandbox
	$other = Join-Path (fUserMenu $root) "Programs\Clutter\0.lnk"
	fShortcut -Path $other -Target (Join-Path $env:SystemRoot "System32\notepad.exe")
	foreach ($i in 1..300) { Copy-Item -LiteralPath $other -Destination (Join-Path (Split-Path -Parent $other) "$i.lnk") }
	$filed = Join-Path (fAllMenu $root) "Programs\Terminals\Silk.lnk"
	fShortcut -Path $filed -Target (fWrapper $root) -Icon "C:\Windows\System32\shell32.dll,3"
	$first = fLaunch $root
	fCheck "the first launch finds it" ($first.Walks -ge 1)
	fCheck "and refreshes it in place" ((fEntry $filed).IconLocation -like '*\silkterm.exe,0')
	fCheck "with no second entry beside it" (-not (Test-Path -LiteralPath (fDefaultEntry $root)))
	foreach ($n in 1..2) {
		$next = fLaunch $root
		fCheck "launch $($n + 1) searches no Start menu ($($next.Walks))" ($next.Walks -eq 0)
	}

	Write-Host "the entry moved to another folder"
	$moved = Join-Path (fAllMenu $root) "Programs\Shells\Silk.lnk"
	New-Item -ItemType Directory -Path (Split-Path -Parent $moved) -Force | Out-Null
	Move-Item -LiteralPath $filed -Destination $moved
	$first = fLaunch $root
	fCheck "the next launch searches again ($($first.Walks))" ($first.Walks -ge 1)
	fCheck "and still writes no entry of its own" (-not (Test-Path -LiteralPath (fDefaultEntry $root)))
	$next = fLaunch $root
	fCheck "then stops searching ($($next.Walks))" ($next.Walks -eq 0)
	fCheck "one entry runs the wrapper" (@(fEntriesRunning $root).Count -eq 1)
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
} finally {
	foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) failed"; fTestDir_End 1; exit 1 }
Write-Host "all passed"
fTestDir_End 0

##	History:
##		- 2026-10-06: Created.
