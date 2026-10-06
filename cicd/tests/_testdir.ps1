##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	The run's test folder, <temp>/test_silkterm_YYYYmmDD-HHMMSSNN, for the
	PowerShell tests and cicd-win.ps1.
.DESCRIPTION
	Dot-sourced; it defines fTestDir_Make, fTestDir_Use, fTestDir_Keep,
	fTestDir_End and fTestDir_Remove and runs nothing. Same contract as
	_testdir.bash: SILKTERM_TEST_DIR set and not empty is the folder, made if
	missing, and never removed; otherwise a fresh one is made, 0700 off Windows,
	and exported, so every test the caller starts shares it. A script that made
	one calls fTestDir_End <exit code> before every exit, which removes the
	folder on 0 and keeps it otherwise.

	Sets no StrictMode, since dot-sourcing would set it for the caller. Every
	caller sets Latest itself.
.EXAMPLE
	. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use
.NOTES
	History: At bottom of file.
#>

##	Windows PowerShell 5.1 has no $IsWindows, and only runs on Windows.
function fTestDir_OnWindows { if (Test-Path variable:IsWindows) { $IsWindows } else { $true } }

##	Always $script:, the nearest script's own scope. A test that cicd-win.ps1 runs
##	through & gets one of its own, so it never sees the pipeline's record. Kept
##	out of the environment, so a child never thinks the folder is its own.
$script:TestDirOwned = $null
$script:TestDirToken = $null

function fTestDir_Make {
	##	Sets $env:SILKTERM_TEST_DIR. Throws, naming the path, when it cannot.
	if ($env:SILKTERM_TEST_DIR) {
		$null = New-Item -ItemType Directory -Force -Path $env:SILKTERM_TEST_DIR
		return
	}
	$base = [System.IO.Path]::GetTempPath()
	$dir = ''
	for ($try = 1; $try -le 100; $try++) {
		$dir = Join-Path $base ('test_silkterm_' + (Get-Date).ToString('yyyyMMdd-HHmmssff'))
		##	Never -Force: it would adopt a folder or symlink planted at a guessable name.
		if (-not (Test-Path -LiteralPath $dir)) {
			$made = $true
			try { $null = New-Item -ItemType Directory -Path $dir -ErrorAction Stop } catch [System.IO.IOException] { $made = $false }
			if ($made) {
				if (-not (fTestDir_OnWindows)) {
					chmod 700 $dir
					if ($LASTEXITCODE) { throw "test run folder: cannot make '$dir' private" }
				}
				$token = "$PID-$([DateTime]::UtcNow.Ticks)"
				[System.IO.File]::WriteAllText((Join-Path $dir '.test_silkterm_owner'), "$token`n")
				$script:TestDirOwned = $dir
				$script:TestDirToken = $token
				$env:SILKTERM_TEST_DIR = $dir
				return
			}
		}
		Start-Sleep -Milliseconds 10
	}
	throw "test run folder: '$dir' and every name before it were taken"
}

function fTestDir_Use {
	##	fTestDir_Make, then points the temp folder at it, so GetTempPath() and
	##	every program the script starts write there.
	fTestDir_Make
	if (fTestDir_OnWindows) {
		$env:TEMP = $env:SILKTERM_TEST_DIR
		$env:TMP = $env:SILKTERM_TEST_DIR
	} else {
		$env:TMPDIR = $env:SILKTERM_TEST_DIR
	}
}

function fTestDir_Keep {
	##	Runs $Run, then puts back the temp folder that fTestDir_Use points
	##	elsewhere. For a caller that runs tests in its own process and has more
	##	to do after them, since the environment is the whole process's.
	param([Parameter(Mandatory)][scriptblock]$Run)
	$saved = @{ TEMP = $env:TEMP; TMP = $env:TMP; TMPDIR = $env:TMPDIR }
	try { & $Run }
	finally {
		foreach ($name in $saved.Keys) {
			if ($null -eq $saved[$name]) { Remove-Item "env:$name" -ErrorAction SilentlyContinue }
			else { Set-Item "env:$name" $saved[$name] }
		}
	}
}

function fTestDir_Remove {
	##	Removes $Dir when it is a run folder with this run's mark. Returns $true
	##	when removed; otherwise one warning with the reason, and $false.
	[CmdletBinding()]
	param(
		[Parameter(Mandatory)][string]$Dir,
		[Parameter(Mandatory)][string]$Token
	)
	$reason = fTestDir_NotOurs $Dir $Token
	if ($reason) { Write-Warning "test run folder: left $Dir in place: $reason"; return $false }
	##	Windows will not remove a folder in use as the current one.
	$here = (Get-Location).ProviderPath
	if ($here -eq $Dir -or $here.StartsWith($Dir + [System.IO.Path]::DirectorySeparatorChar)) { Set-Location -LiteralPath (Split-Path $Dir) }
	try { fTestDir_Delete ([System.IO.DirectoryInfo]$Dir) }
	catch { Write-Warning "test run folder: could not remove ${Dir}: $($_.Exception.Message)"; return $false }
	return $true
}

function fTestDir_NotOurs([string]$Dir, [string]$Token) {
	##	Why $Dir is not this run's folder, or nothing when it is.
	if ((Split-Path -Leaf $Dir) -notmatch '^test_silkterm_\d{8}-\d{8}$') { return 'not named like a run folder' }
	$info = [System.IO.DirectoryInfo]$Dir
	if (-not $info.Exists) { return 'not a folder' }
	if ($info.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { return 'it is a link' }
	$marker = [System.IO.FileInfo](Join-Path $Dir '.test_silkterm_owner')
	if (-not $marker.Exists -or ($marker.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { return 'no owner mark' }
	$line = @([System.IO.File]::ReadAllLines($marker.FullName)) | Select-Object -First 1
	if ($null -eq $line -or $line.Trim() -ne $Token) { return "another run's owner mark" }
	return ''
}

function fTestDir_Delete([System.IO.DirectoryInfo]$Folder) {
	##	Its own walk, since Remove-Item -Recurse in Windows PowerShell 5.1 follows
	##	junctions, and a .NET recursive delete stops at a read-only file.
	$readOnly = [System.IO.FileAttributes]::ReadOnly
	$Folder.Attributes = $Folder.Attributes -band -bnot $readOnly
	foreach ($entry in $Folder.GetFileSystemInfos()) {
		$isFolder = [bool]($entry.Attributes -band [System.IO.FileAttributes]::Directory)
		if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
			if ($isFolder -and (fTestDir_OnWindows)) { [System.IO.Directory]::Delete($entry.FullName, $false) }
			else { [System.IO.File]::Delete($entry.FullName) }
		} elseif ($isFolder) {
			fTestDir_Delete $entry
		} else {
			$entry.Attributes = $entry.Attributes -band -bnot $readOnly
			$entry.Delete()
		}
	}
	$Folder.Delete()
}

function fTestDir_End {
	##	Removes the folder fTestDir_Make made, when $Status is 0. Never throws, so
	##	it never changes how a script exits.
	[CmdletBinding()]
	param([int]$Status = 0)
	$dir = $script:TestDirOwned
	if (-not $dir) { return }
	$script:TestDirOwned = $null
	if ($Status -ne 0) { Write-Warning "test files kept in $dir"; return }
	$null = fTestDir_Remove -Dir $dir -Token $script:TestDirToken
}

##	History:
##		- 20260930 JC: Created.
##		- 20261002 JC: A run removes the folder it made when it passes.
