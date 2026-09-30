##	- Purpose:
##		The run's test folder, <temp>/test_silkterm_YYYYmmDD-HHMMSSNN, for the
##		PowerShell tests and cicd-win.ps1. Dot-sourced; it defines fTestDir_Make,
##		fTestDir_Use and fTestDir_Keep and runs nothing. Same contract as _testdir.bash:
##		SILKTERM_TEST_DIR set and not empty is the folder, made if missing;
##		otherwise a fresh one is made, 0700 off Windows, and exported, so every
##		test the caller starts shares it.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

##	Windows PowerShell 5.1 has no $IsWindows, and only runs on Windows.
function fTestDir_OnWindows { if (Test-Path variable:IsWindows) { $IsWindows } else { $true } }

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

##	History:
##		- 20260930 JC: Created.
