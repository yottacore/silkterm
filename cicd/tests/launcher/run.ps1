#!/usr/bin/env pwsh

##	Purpose:
##		- Drive n8runterm.ps1 in a sandboxed HOME and check what it does to files
##		  it did not create. install.bash puts a release build at the same path
##		  the launcher wants for its symlink, and the launcher used to delete it.
##		- Also: that every argument reaches the terminal exactly as given, and
##		  that a build already held is recognised without reading it again.
##		- Nothing here touches the real home directory or the real pool.
##	Test ID: EpHRcSG
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Launcher = Join-Path (Split-Path $PSScriptRoot -Parent | Split-Path -Parent | Split-Path -Parent) "utility/n8runterm.ps1"
if (-not (Test-Path -LiteralPath $Launcher)) { throw "no launcher at $Launcher" }

$script:Failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" }
	else     { Write-Host "  FAIL $What"; $script:Failures++ }
}

## A sandbox home with a source dir holding one "build".
function fSandbox {
	$root = Join-Path ([System.IO.Path]::GetTempPath()) "silkterm-launcher-test-$PID-$(Get-Random)"
	$src  = Join-Path $root "synced/0-0/common/exec/app/linux"
	New-Item -ItemType Directory -Path $src -Force | Out-Null
	Set-Content -LiteralPath (Join-Path $src "silkterm") -Value "dogfood build" -NoNewline
	Set-Content -LiteralPath (Join-Path $src "silkterm.tag") -Value "gnulli" -NoNewline
	if (-not $IsWindows) { chmod +x (Join-Path $src "silkterm") }
	return $root
}

function fRun { param([string]$Home_)
	$env:HOME = $Home_
	$env:USERPROFILE = $Home_
	& pwsh -NoProfile -File $Launcher --install-only 2>&1 | Out-Null
}

## Same, but hands back what the launcher said, and takes extra arguments.
function fRunSaying { param([string]$Home_, [string[]]$Extra = @())
	$env:HOME = $Home_
	$env:USERPROFILE = $Home_
	return (& pwsh -NoProfile -File $Launcher @Extra 2>&1 | Out-String)
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

$realHome = $env:HOME
try {
	Write-Host "a release build already installed at the symlink path"
	$root = fSandbox
	$bin = Join-Path $root ".local/bin"
	New-Item -ItemType Directory -Path $bin -Force | Out-Null
	$installed = Join-Path $bin "silkterm"
	Set-Content -LiteralPath $installed -Value "a release build somebody installed" -NoNewline
	fRun $root
	fCheck "the installed build is still there" (Test-Path -LiteralPath $installed)
	fCheck "and is still itself" ((Get-Content -LiteralPath $installed -Raw) -eq "a release build somebody installed")
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	Write-Host "nothing installed at the symlink path"
	$root = fSandbox
	fRun $root
	$link = Join-Path $root ".local/bin/silkterm"
	fCheck "the launcher put its own silkterm there" (Test-Path -LiteralPath $link)
	Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue

	## Start-Process joins an argument list with spaces and escapes nothing, so a
	## quote used to cut an argument in two, an empty one vanished, and a trailing
	## backslash took the closing quote with it and joined the next argument.
	Write-Host "arguments reach the terminal as given"
	$root = fSandbox
	fRecordingBuild (Join-Path $root "synced/0-0/common/exec/app/linux/silkterm")
	$want = @('--shell', 'bash -c "echo hi"', '', 'C:\my dir\', 'plain')
	$null = fRunSaying $root $want
	$argvFile = Join-Path $root "argv.txt"
	for ($i = 0; $i -lt 25 -and -not (Test-Path -LiteralPath $argvFile); $i++) { Start-Sleep -Milliseconds 200 }
	$got = @()
	if (Test-Path -LiteralPath $argvFile) {
		## The launcher's own --title comes first; everything after it is ours.
		$got = @(Get-Content -LiteralPath $argvFile)
		if ($got.Count -ge 1 -and $got[0] -like '--title=*') { $got = @($got[1..($got.Count - 1)]) }
	}
	fCheck "every argument arrives unchanged" (($got -join "`u{241F}") -eq ($want -join "`u{241F}"))
	if (($got -join "`u{241F}") -ne ($want -join "`u{241F}")) {
		Write-Host "    wanted: $($want | ForEach-Object { "[$_]" })"
		Write-Host "    got   : $($got  | ForEach-Object { "[$_]" })"
	}
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
} finally {
	$env:HOME = $realHome
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) failed"; exit 1 }
Write-Host "all passed"

##	History:
##		- 2026-09-08: Created.
