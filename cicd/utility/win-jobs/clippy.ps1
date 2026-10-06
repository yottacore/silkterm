##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	clippy over the clone on a Windows box, warnings as errors.
.DESCRIPTION
	Uses target\lint, the way cicd does.
.NOTES
	Run by win-remote.bash job, which writes _env.ps1 beside it.
	History: At bottom of file.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"
. "$PSScriptRoot\_env.ps1"

# Separate target dir, the way cicd does it - clippy and build otherwise
# invalidate each other's artifacts every run.
$env:CARGO_TARGET_DIR = Join-Path $RepoDir "target\lint"
Push-Location (Join-Path $RepoDir "source")
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | ForEach-Object { $_.ToString() }
$code = $LASTEXITCODE
Pop-Location
"clippy exit=$code"
exit $code

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
