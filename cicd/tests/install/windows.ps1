#!/usr/bin/env pwsh

##	- Purpose:
##		install.ps1 on Windows, run for real against a stand-in release through
##		stubrun.ps1: an upgrade over a running copy, and a re-run that puts back a
##		missing Start Menu shortcut and PATH entry. It installs into a scratch
##		folder, with LOCALAPPDATA and APPDATA pointed there. The user PATH in the
##		registry is the one real thing it changes, and it is put back as it was.
##	- Syntax: windows.ps1 [-Shell pwsh|powershell] [-Installer <path to install.ps1>]
##		powershell runs the installer under Windows PowerShell 5.1.
##	- Exit: 0 when every check passed, 1 otherwise.
##	- Test ID: Eqq4nMu
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param(
	[ValidateSet('pwsh', 'powershell')][string]$Shell = 'pwsh',
	[string]$Installer = (Join-Path $PSScriptRoot '../../../install.ps1')
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) { Write-Host '  skip installer on Windows (not Windows)'; exit 0 }

$installer = (Resolve-Path -LiteralPath $Installer).Path
$stubrun = Join-Path $PSScriptRoot 'stubrun.ps1'

$failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   ${Shell}: $What" } else { Write-Host "  FAIL ${Shell}: $What"; $script:failures++ }
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ('silk-wininst-' + [System.IO.Path]::GetRandomFileName())
$rel = Join-Path $work 'release'
$null = New-Item -ItemType Directory -Path $rel

##	The stand-in program is ping, since it keeps running for as long as asked,
##	with its version appended where Windows ignores it.
function fRelease { param([string]$Ver)
	$name = "silkterm-$Ver-windows-x86_64.exe"
	$bytes = [System.IO.File]::ReadAllBytes((Join-Path $env:SystemRoot 'System32\PING.EXE')) +
		[System.Text.Encoding]::ASCII.GetBytes("`n#ver $Ver`n")
	[System.IO.File]::WriteAllBytes((Join-Path $rel $name), $bytes)
	$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $rel $name)).Hash.ToLower()
	[System.IO.File]::WriteAllText((Join-Path $rel "silkterm-$Ver-sha256sums.txt"), "$hash  $name`n")
	[System.IO.File]::WriteAllText((Join-Path $rel 'releases.json'), "[{`"tag_name`":`"v$Ver`",`"draft`":false,`"prerelease`":false}]")
}

##	One installer run. Returns what it printed, and a line starting "Error:" is
##	its failure, since run as a script block it has no exit status.
$log = Join-Path $work 'out.log'
##	pwsh's module path hides 5.1's own modules, so 5.1 gets none and uses its
##	default.
function fInstall {
	$modules = $env:PSModulePath
	if ($Shell -eq 'powershell') { $env:PSModulePath = $null }
	try { & $Shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $stubrun -Installer $installer *> $log }
	finally { $env:PSModulePath = $modules }
	return [System.IO.File]::ReadAllText($log)
}

$destDir = Join-Path $work 'local\Programs\SilkTerm'
$destFile = Join-Path $destDir 'silkterm.exe'
$lnk = Join-Path $work 'roaming\Microsoft\Windows\Start Menu\Programs\SilkTerm.lnk'
function fInstalled { param([string]$Ver)
	if (-not (Test-Path -LiteralPath $destFile)) { return $false }
	return ([System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($destFile))).Contains("#ver $Ver`n")
}
function fPathHas {
	$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
	try { return (([string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')) -split ';') -contains $destDir }
	finally { $key.Close() }
}

$saved = @{ LOCALAPPDATA = $env:LOCALAPPDATA; APPDATA = $env:APPDATA; STUB_DIR = $env:STUB_DIR; STUB_API_CODE = $env:STUB_API_CODE }
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
$hadPath = $key.GetValue('Path', $null, 'DoNotExpandEnvironmentNames')
$pathKind = if ($null -ne $hadPath) { $key.GetValueKind('Path') } else { $null }
$running = $null
try {
	$env:LOCALAPPDATA = Join-Path $work 'local'
	$env:APPDATA = Join-Path $work 'roaming'
	$env:STUB_DIR = $rel

	fRelease '9.9.8'
	$out = fInstall
	fCheck 'a first install' ((fInstalled '9.9.8') -and $out -notmatch '(?m)^Error:')
	fCheck 'makes the shortcut' (Test-Path -LiteralPath $lnk)
	fCheck 'and the PATH entry' (fPathHas)

	##	An upgrade over a running copy. A copy over it fails with the file in use.
	$running = Start-Process -FilePath $destFile -ArgumentList '-n', '60', '127.0.0.1' -WindowStyle Hidden -PassThru
	Start-Sleep -Milliseconds 500
	fRelease '9.9.9'
	$out = fInstall
	fCheck 'an upgrade goes over a running copy' ((fInstalled '9.9.9') -and $out -notmatch '(?m)^Error:')
	if ($out -match '(?m)^Error:') { Write-Host $out }
	$running.Kill(); $running.WaitForExit(); $running = $null
	fCheck 'leaving nothing staged behind' (-not (Get-ChildItem -LiteralPath $destDir -Filter '.silkterm-new-*' -Force))

	##	The copy moved aside goes at the next upgrade, once nothing holds it.
	fRelease '9.9.10'
	$null = fInstall
	fCheck 'the next upgrade removes the old copy' ((fInstalled '9.9.10') -and -not (Get-ChildItem -LiteralPath $destDir -Filter 'silkterm.exe.old-*' -Force))

	##	A re-run with the program current puts back what went missing.
	Remove-Item -LiteralPath $lnk -ErrorAction SilentlyContinue
	$key.SetValue('Path', ((([string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')) -split ';' | Where-Object { $_ -ne $destDir }) -join ';'), $key.GetValueKind('Path'))
	$before = (Get-Item -LiteralPath $destFile).LastWriteTimeUtc
	$out = fInstall
	fCheck 'a re-run puts back the shortcut' (Test-Path -LiteralPath $lnk)
	fCheck 'and the PATH entry' (fPathHas)
	fCheck 'without installing the program again' ((Get-Item -LiteralPath $destFile).LastWriteTimeUtc -eq $before)
	fCheck 'and says so' ($out -match 'Put back what was missing')
	$out = fInstall
	fCheck 'with nothing missing, a re-run does nothing' ($out -match 'Already up to date')

	##	An API failure is an error, never "no full release".
	$env:STUB_API_CODE = '500'
	$out = fInstall
	$env:STUB_API_CODE = $null
	fCheck 'a failed API call stops the install' ($out -match '(?m)^Error: could not read the release list')
	fCheck 'and is not read as no full release' ($out -notmatch 'No full release')
} finally {
	if ($running) { try { $running.Kill() } catch { $null = $_ } }
	foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
	if ($null -eq $hadPath) { $key.DeleteValue('Path', $false) } else { $key.SetValue('Path', $hadPath, $pathKind) }
	$key.Close()
	Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures -gt 0) { exit 1 }
exit 0

##	History:
##		- 20260925 JC: Created.
