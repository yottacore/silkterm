##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A release build owns no console, so what a control command has to say goes
	to the console it was typed at, or nowhere.
.DESCRIPTION
	It went nowhere for a while.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EqH4isq


$said = Join-Path $OutDir "consolemsg.txt"
$c = Start-Process cmd.exe -ArgumentList '/k', "`"$Exe`" --reload-settings" -PassThru
fTrack $c
Start-Sleep -Seconds 3
$reader = (Get-Process -Id $PID).Path
Start-Process $reader -ArgumentList '-NoProfile', '-File', "`"$PSScriptRoot\_readcon.ps1`"", '-Of', $c.Id, '-Out', "`"$said`"" -WindowStyle Hidden -Wait
$text = if (Test-Path $said) { (Get-Content $said -Raw) -replace '\s+', ' ' } else { '' }
fNote ("console reads: " + $text.Trim())
[void](fCheck "the console could be read" ($text -ne '' -and $text -notlike '*could not attach*'))
[void](fCheck "--reload-settings says why it did nothing, on the console it was typed at" ($text -like '*SilkTerm: *'))
fStop $c

##	History:
##		- 20260918 JC: Created.
##		- 20261006 JC: Help block.
