##	A character can reach a window handed over whole instead of typed, and that is
##	how the touch keyboard sends what its layout has no key for. Text expanders and
##	some accessibility tools do the same. It used to reach nothing at all here.
##	Test ID: EpPhAdU

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed" }

$cfg = Join-Path $OutDir "inj-config.shcl"
fFreshConfig $cfg

$p = fStartSilk $Exe @("--config=$cfg", "--columns", "100", "--rows", "30") @{}
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }

Start-Sleep -Seconds 3
[void](fCheck "it takes the foreground" (fFocus $h))

##	The shell writes a file rather than the picture being read, for the reason
##	smoke.ps1 gives: a line of text moves about as many pixels as the cursor does.
$proof = Join-Path $OutDir "injected.txt"
Remove-Item $proof -ErrorAction SilentlyContinue
fSendChars "echo silkrig-packet > $proof"
fPress "enter"
Start-Sleep -Seconds 3
$said = if (Test-Path $proof) { Get-Content $proof -Raw } else { "" }
[void](fShot $h "inj-after")
[void](fCheck "an injected character reaches the shell" ($said -match "silkrig-packet"))

##	The case the touch keyboard is actually for: a character the layout has no key
##	for, so nothing but an injection could have produced it. The check is loose and
##	the note carries what was really written: a shell that redirects in the console
##	code page mangles the accent on its way to the file, which says nothing about
##	what the terminal received.
$accent = Join-Path $OutDir "injected-accent.txt"
Remove-Item $accent -ErrorAction SilentlyContinue
fSendChars ([string]::Concat("echo caf", [char]0x00E9, "-", [char]0x4E2D, " > ", $accent))
fPress "enter"
Start-Sleep -Seconds 3
$got = if (Test-Path $accent) { Get-Content $accent -Raw } else { "" }
fNote "wrote: $($got.Trim())"
[void](fCheck "a character with no key on this layout reaches it too" ($got -match "caf"))

##	Real key presses were never broken, and must not be by the fix.
$keys = Join-Path $OutDir "typed-keys.txt"
Remove-Item $keys -ErrorAction SilentlyContinue
fSend "echo silkrig-keys > $keys"
fPress "enter"
Start-Sleep -Seconds 3
$typed = if (Test-Path $keys) { Get-Content $keys -Raw } else { "" }
[void](fCheck "ordinary typing still reaches the shell" ($typed -match "silkrig-keys"))

fStop $p

##	History:
##		- 20260909 JC: Created.
