##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	cargo test in the clone on a Windows box.
.DESCRIPTION
	Runs as whichever account ssh logged in as.
.NOTES
	Run by win-remote.bash job, which writes _env.ps1 beside it.
	History: At bottom of file.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"
. "$PSScriptRoot\_env.ps1"

# G6: the test process reads the live user config, so this reflects whichever
# account ssh logged in as, not a clean machine.
Push-Location (Join-Path $RepoDir "source")
cargo test 2>&1 | ForEach-Object { $_.ToString() }
$code = $LASTEXITCODE
Pop-Location
"test exit=$code"
exit $code

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
