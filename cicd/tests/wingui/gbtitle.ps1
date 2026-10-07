##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A Git Bash pane's title follows a cd, with the git prompt off and on.
.DESCRIPTION
	Git's bin\bash.exe starts the real bash as its child and waits, so the
	pane's own process never moved and the title kept the folder the pane
	started in. Skipped where Git for Windows is not installed.
.NOTES
	History: At bottom of file.
#>

##	Test ID: Es2etJZ


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed" }
$gitBash = @($env:ProgramFiles, $env:ProgramW6432) | Where-Object { $_ } |
	ForEach-Object { Join-Path $_ 'Git\bin\bash.exe' } |
	Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $gitBash) { fSkip "no Git for Windows here" }

function fTitle([IntPtr]$H) {
	$sb = New-Object System.Text.StringBuilder 512
	[void][Silk.Win]::GetWindowTextW($h, $sb, 512)
	$sb.ToString()
}

$start = Join-Path $OutDir "gbtitle-start"
New-Item -ItemType Directory -Force -Path $start | Out-Null
foreach ($prompt in @("false", "true")) {
	$cfg = Join-Path $OutDir "gbtitle-$prompt.shcl"
	fFreshConfig $cfg @("shell:", "`tbash_prompt: $prompt")
	##	One string, so the quoted program path reaches --shell as the shell list writes it.
	$line = "--config=`"$cfg`" --columns 100 --rows 30 --directory `"$start`" --shell `"\`"$gitBash\`"`""
	$p = fStartSilk -Exe $Exe -SilkArgs @($line) -EnvVars @{}
	$h = fWaitWindow $p 40
	if (-not (fCheck "prompt $($prompt): a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; continue }
	Start-Sleep -Seconds 4
	$before = fTitle $h
	[void](fFocus $h)
	fSendChars "cd /c/Windows"
	fPress "enter"
	$after = ""
	for ($i = 0; $i -lt 20; $i++) {
		Start-Sleep -Milliseconds 250
		$after = fTitle $h
		if ($after -match 'Windows') { break }
	}
	fNote "prompt $($prompt): '$before' -> '$after'"
	[void](fShot $h "gbtitle-$prompt")
	[void](fCheck "prompt $($prompt): the title names the start folder" ($before -match 'gbtitle-start'))
	[void](fCheck "prompt $($prompt): and follows the cd" ($after -match 'Windows' -and $after -notmatch 'gbtitle-start'))
	[void](fCheck "prompt $($prompt): a bash at its prompt is running nothing" ($after -notmatch '\[bash\]'))
	fStop $p
}

##	History:
##		- 20261007 JC: Created.
