#!/usr/bin/env pwsh

##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Check the freedesktop launcher install.ps1 writes on Linux.
.DESCRIPTION
	Exec= is read twice - the desktop-entry string rules, then the Exec quoting
	rules - so a path holding a space, a quote, a '$' or a '%' needs escaping,
	not just quotes. The entry block is lifted out of install.ps1 rather than
	retyped, so a change to the line it writes is a change to what is checked.
.PARAMETER Installer
	Path to install.ps1. Default: the one in this repo.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

##	Test ID: EqAwQq8

[CmdletBinding()]
param(
	[string]$Installer = (Join-Path $PSScriptRoot '../../../install.ps1')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use

$text = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $Installer).Path)

##	The installer's own escaping function and its own entry block.
$m = [regex]::Match($text, "(?ms)^function fDesktopExec \{\r?\n.*?^\}\r?$")
if (-not $m.Success) { throw "fDesktopExec not found in $Installer" }
. ([scriptblock]::Create($m.Value))

$m = [regex]::Match($text, "(?ms)@\(\s*\r?\n\s*'\[Desktop Entry\]'.*?\r?\n\s*\)\s*\|\s*Set-Content")
if (-not $m.Success) { throw "the desktop entry block was not found in $Installer" }
$block = $m.Value -replace '(?s)\s*\|\s*Set-Content$', ''

$failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" } else { Write-Host "  FAIL $What"; $script:failures++ }
}

##	The settings the block reads, as install.ps1 sets them.
$appName              = 'SilkTerm'
$desktopGenericName   = 'Terminal'
$appComment           = 'Smooth-scrolling GPU terminal with split panes'
$desktopIcon          = 'utilities-terminal'
$desktopCategories    = 'System;TerminalEmulator;'
$desktopKeywords      = 'terminal;shell;prompt;command;'

##	The same case list install.bash is checked against.
$bad = 0
foreach ($line in [System.IO.File]::ReadAllLines((Join-Path $PSScriptRoot 'desktop-exec-cases.txt'))) {
	if ($line -eq '' -or $line.StartsWith('#')) { continue }
	$path, $want = $line -split "`t", 2
	$got = fDesktopExec $path
	if ($got -ne $want) { Write-Host "    ${path}: wanted $want, got $got"; $bad++ }
}
fCheck "install.ps1 escapes Exec the way both rule sets read it" ($bad -eq 0)

$work = Join-Path ([System.IO.Path]::GetTempPath()) ('silk-desktop-' + [System.IO.Path]::GetRandomFileName())
$null = New-Item -ItemType Directory -Path $work
try {
	##	A launchable path with the characters GLib does handle. '%' is left out:
	##	GLib refuses an entry carrying the '%%' the spec asks for, so the case
	##	list above is as far as that one can be checked.
	$dir = Join-Path $work 'home dir/a$b "q"/.local/bin'
	$null = New-Item -ItemType Directory -Path $dir -Force
	$destFile = Join-Path $dir 'silkterm'
	Set-Content -LiteralPath $destFile -Value "#!/bin/sh`nprintf ran > `"$work/ran.txt`"`n" -NoNewline
	chmod +x $destFile

	$entry = Join-Path $work 'silkterm.desktop'
	(& ([scriptblock]::Create($block))) | Set-Content -LiteralPath $entry

	if (Get-Command desktop-file-validate -ErrorAction SilentlyContinue) {
		& desktop-file-validate $entry
		fCheck "the entry validates" ($LASTEXITCODE -eq 0)
	} else { Write-Host '  skip desktop-file-validate (not installed)' }

	if (Get-Command gio -ErrorAction SilentlyContinue) {
		& env -u DISPLAY gio launch $entry *> $null
		for ($i = 0; $i -lt 10 -and -not (Test-Path -LiteralPath "$work/ran.txt"); $i++) { Start-Sleep -Milliseconds 200 }
		fCheck "and it starts the program it names" (Test-Path -LiteralPath "$work/ran.txt")
	} else { Write-Host '  skip gio launch (not installed)' }
} finally {
	Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures -gt 0) { fTestDir_End 1; exit 1 }
fTestDir_End 0
exit 0

##	History:
##		- 20260917 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
