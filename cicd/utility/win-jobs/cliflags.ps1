##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Print what the CLI-only flags say, from the clone's release build.
.DESCRIPTION
	--version and --about, read through redirected files, plus the bytes of the copyright line.
.NOTES
	Run by win-remote.bash job, which writes _env.ps1 beside it.
	History: At bottom of file.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"
. "$PSScriptRoot\_env.ps1"

# The CLI-only flags print through a console this build does not own, so shell
# redirection loses them: AttachConsole joins the parent's console and the output
# goes there instead of the file. An explicit redirect with no console to join is
# the only way to read them from a job. See G37.
$exe = Join-Path $RepoDir "target\release\silkterm.exe"
if (-not (Test-Path $exe)) { "no binary - run the build job first"; exit 1 }

"account = " + [Security.Principal.WindowsIdentity]::GetCurrent().Name
"config  = " + (Test-Path (Join-Path $env:APPDATA "silkterm\config.shcl"))

$o = Join-Path $env:TEMP "silk-cli-out.txt"
$e = Join-Path $env:TEMP "silk-cli-err.txt"
foreach ($flag in "--version", "--about") {
	$p = Start-Process -FilePath $exe -ArgumentList $flag -NoNewWindow -Wait -PassThru `
		-RedirectStandardOutput $o -RedirectStandardError $e
	"--- $flag (exit $($p.ExitCode)) ---"
	$out = Get-Content $o -Raw -ErrorAction SilentlyContinue
	if ($out) { $out.TrimEnd() }
	if ($flag -eq "--about" -and $out) {
		$line = ($out -split "`n" | Where-Object { $_ -match "Copyright" })
		"copyright bytes = " + (([Text.Encoding]::UTF8.GetBytes($line) | ForEach-Object { $_.ToString("x2") }) -join " ")
	}
	$err = Get-Content $e -Raw -ErrorAction SilentlyContinue
	if ($err) { "stderr: " + $err.TrimEnd() }
}
Remove-Item $o, $e -ErrorAction SilentlyContinue

##	History:
##		- 20260908 JC: Created.
##		- 20261006 JC: Help block, StrictMode Latest.
