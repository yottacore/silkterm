#!/usr/bin/env pwsh

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

<#
.SYNOPSIS
	Run utility/runterm.cmd with a PATH that has lost PowerShell 7, and check
	it still reaches the launcher.
.DESCRIPTION
	Explorer rebuilds its environment when a program announces a change to it,
	and that rebuild cuts PATH at 4095 characters. On a box with a long machine
	PATH the cut falls before PowerShell 7's folder, and everything started from
	the taskbar or the Start menu from then on gets the short one. The wrapper
	looked for pwsh on PATH only, so a click flashed a minimized window and did
	nothing, while a console opened earlier still worked.

	The wrapper runs a stand-in launcher beside it, which records what it was
	given. Windows only; elsewhere it says so and passes.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of script.
#>

##	Test ID: Es2WeWF

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $IsWindows) { Write-Host "  skip runterm.cmd (Windows only)"; exit 0 }

. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use

$Wrapper = Join-Path (Split-Path $PSScriptRoot -Parent | Split-Path -Parent | Split-Path -Parent) "utility/runterm.cmd"
if (-not (Test-Path -LiteralPath $Wrapper)) { throw "no wrapper at $Wrapper" }

$script:Failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" }
	else     { Write-Host "  FAIL $What"; $script:Failures++ }
}

## The wrapper tries the launcher beside itself first, so a copy of it next to a
## stand-in never reaches a real one.
$dir = Join-Path ([System.IO.Path]::GetTempPath()) "silkterm-wrapper-test-$PID-$(Get-Random)"
New-Item -ItemType Directory -Path $dir -Force | Out-Null
Copy-Item -LiteralPath $Wrapper -Destination (Join-Path $dir "runterm.cmd")
Set-Content -LiteralPath (Join-Path $dir "n8runterm.ps1") -Value @(
	'"edition=$($PSVersionTable.PSEdition)", "home=$PSHOME" | Set-Content -LiteralPath $env:SILK_WRAP_OUT'
	'foreach ($a in $args) { "arg=$a" | Add-Content -LiteralPath $env:SILK_WRAP_OUT }'
)
$said = Join-Path $dir "said.txt"

## Runs the wrapper as a shortcut would, with PATH set to $PathValue and any other
## variables in $Env, and answers { Code; Output; Said }.
function fRunWrapper { param([string]$PathValue, [hashtable]$Env = @{})
	Remove-Item -LiteralPath $said -ErrorAction SilentlyContinue
	$psi = New-Object System.Diagnostics.ProcessStartInfo
	$psi.FileName = $env:ComSpec
	$psi.Arguments = "/d /c `"`"$(Join-Path $dir 'runterm.cmd')`" --install-only `"two words`"`""
	$psi.UseShellExecute = $false
	$psi.RedirectStandardOutput = $true
	$psi.RedirectStandardError = $true
	$psi.Environment["PATH"] = $PathValue
	$psi.Environment["SILK_WRAP_OUT"] = $said
	foreach ($name in $Env.Keys) { $psi.Environment[$name] = $Env[$name] }
	$proc = [System.Diagnostics.Process]::Start($psi)
	$out = $proc.StandardOutput.ReadToEnd() + $proc.StandardError.ReadToEnd()
	if (-not $proc.WaitForExit(60000)) { $proc.Kill(); throw "the wrapper did not finish in 60 s" }
	return [pscustomobject]@{
		Code   = $proc.ExitCode
		Output = $out.Trim()
		Said   = @(Get-Content -LiteralPath $said -ErrorAction SilentlyContinue)
	}
}

function fHome { param($Run) return @($Run.Said | Where-Object { $_ -like "home=*" }) -join "" }

function fReached { param($Run)
	return ($Run.Code -eq 0 -and $Run.Said -contains "edition=Core" -and
		$Run.Said -contains "arg=--install-only" -and $Run.Said -contains "arg=two words")
}

try {
	Write-Host "PATH as given"
	$run = fRunWrapper $env:PATH
	fCheck "reaches the launcher under PowerShell 7, arguments intact ($($run.Code))" (fReached $run)
	$intactHome = fHome $run

	## What a cut PATH looks like from the wrapper: no folder on it holds pwsh.exe.
	Write-Host "PATH without PowerShell 7"
	$short = @($env:PATH -split ';' | Where-Object { $_ -and -not (Test-Path -LiteralPath (Join-Path $_ "pwsh.exe")) }) -join ';'
	fCheck "no folder left on it holds pwsh.exe" (-not ($short -split ';' | Where-Object { $_ -and (Test-Path -LiteralPath (Join-Path $_ "pwsh.exe")) }))
	$run = fRunWrapper $short
	fCheck "still reaches the launcher under PowerShell 7 ($($run.Code))" (fReached $run)
	if (-not (fReached $run)) { Write-Host "       wrapper said: $($run.Output)" }
	## A box can have the Store's PowerShell beside the installer's; the one a
	## whole PATH finds first is the one to run.
	fCheck "the same PowerShell 7 as with PATH whole ($(fHome $run))" ((fHome $run) -eq $intactHome)

	## The installer's own record, read out of 'reg query' text. Only where it made
	## one; a zip or Store install has none.
	$appPath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\pwsh.exe'
	if (Test-Path -LiteralPath $appPath) {
		Write-Host "found only through App Paths"
		$nowhere = Join-Path $dir "nowhere"
		$run = fRunWrapper $short @{ ProgramFiles = $nowhere; LOCALAPPDATA = $nowhere }
		fCheck "reaches the launcher under PowerShell 7 ($($run.Code))" (fReached $run)
		if (-not (fReached $run)) { Write-Host "       wrapper said: $($run.Output)" }
	} else {
		Write-Host "  skip App Paths (pwsh did not register one here)"
	}
} finally {
	Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
}

if ($script:Failures -gt 0) { Write-Host "$($script:Failures) failed"; fTestDir_End 1; exit 1 }
Write-Host "all passed"
fTestDir_End 0

##	History:
##		- 2026-10-07: Created.
