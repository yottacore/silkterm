##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	Software rendering on a real Windows desktop, where the software adapter is
	WARP.
.DESCRIPTION
	Each arm, Transparency off and then on: the setting turned on, and a card
	made to refuse every device (SILK_REFUSE_CARD) with the setting off. The
	window has to come up on a CPU adapter, draw, and take typing.
.NOTES
	History: At bottom of file.
#>

##	Test ID: ErqPAbe


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed or grabbed" }

$refuse = Join-Path $RunDir "refuse-card.txt"
Set-Content -Path $refuse -Value "refuse"

$arms = @(
	@{ name = "setting-opaque"; seethru = $false; refused = $false },
	@{ name = "setting-seethru"; seethru = $true; refused = $false },
	@{ name = "refused-opaque"; seethru = $false; refused = $true },
	@{ name = "refused-seethru"; seethru = $true; refused = $true }
)
foreach ($arm in $arms) {
	$name = $arm.name
	$cfg = Join-Path $RunDir "soft-$name.shcl"
	$soft = if ($arm.refused) { "false" } else { "true" }
	$see = if ($arm.seethru) { "true" } else { "false" }
	fFreshConfig $cfg @("window:", "`tsoftware_rendering: $soft", "transparency:", "`tenabled: $see")
	New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null
	$err = Join-Path $script:shotDir "soft-$name.txt"
	$envVars = @{}
	if ($arm.refused) { $envVars.SILK_REFUSE_CARD = $refuse }
	foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $envVars[$k]) }
	$p = Start-Process $Exe -ArgumentList @("--config=$cfg", "--columns", "100", "--rows", "30", "--shell=cmd.exe") -RedirectStandardError $err -PassThru
	foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $null) }
	fTrack $p
	$h = fWaitWindow $p 40
	if (-not (fCheck "$name a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; continue }
	Start-Sleep -Seconds 3
	[void](fCheck "$name it drew" ((fInk (fShot $h "soft-$name") 6) -gt 0.02))

	$proof = Join-Path $OutDir "soft-typed-$name.txt"
	Remove-Item $proof -ErrorAction SilentlyContinue
	[void](fFocus $h)
	fSend "echo soft-$name > $proof"
	fPress "enter"
	Start-Sleep -Seconds 3
	[void](fCheck "$name typing reached the shell" (Test-Path $proof))

	fStop $p
	Start-Sleep -Milliseconds 500
	$said = @(Get-Content $err -ErrorAction SilentlyContinue)
	foreach ($line in $said) { fNote "said $line" }
	$renderer = @($said | Where-Object { $_ -match 'renderer = ' })
	[void](fCheck "$name drew on a CPU adapter" ($renderer.Count -gt 0 -and @($renderer | Where-Object { $_ -notmatch '/ Cpu\]' }).Count -eq 0))
	if ($arm.refused) {
		[void](fCheck "$name said it fell back" (($said -match 'drawing in software').Count -gt 0))
	}
}

##	History:
##		- 20261005 JC: Created.
##		- 20261006 JC: Help block; counts what a filter left as a list.
