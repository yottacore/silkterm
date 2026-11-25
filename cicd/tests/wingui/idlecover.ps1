##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	"Minutes when hidden" on a window all covered by another one, at 1
	minute, and the window back once the other one closes.
.DESCRIPTION
	See _idlebehind.ps1. Not in the pipeline: about 3 and a half minutes,
	close to the harness limit.
.NOTES
	History: At bottom of file.
#>

##	Test ID: dfsmp
##	Box: gpu


$idleArms = @("covered-opaque", "covered-seethru")
. "$PSScriptRoot\_idlebehind.ps1"

##	History:
##		- 20261125 JC: Created.
