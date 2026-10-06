##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Release build of the clone on a Windows box.
.DESCRIPTION
	Prints the exit code, the time taken and the binary's size.
.NOTES
	Run by win-remote.bash job, which writes _env.ps1 beside it.
	History: At bottom of file.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"
. "$PSScriptRoot\_env.ps1"

$src = Join-Path $RepoDir "source"
Push-Location $src
$sw = [Diagnostics.Stopwatch]::StartNew()
cargo build --release 2>&1 | ForEach-Object { $_.ToString() }
$code = $LASTEXITCODE
$sw.Stop()
Pop-Location
"build exit=$code in $([math]::Round($sw.Elapsed.TotalSeconds))s"
$exe = Join-Path $RepoDir "target\release\silkterm.exe"
if (Test-Path $exe) { "binary = $exe  " + (Get-Item $exe).Length + " bytes" }
exit $code

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
