##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	"Minutes otherwise" on a window left behind another one, with a strip of
	it still in view, at 1 minute.
.DESCRIPTION
	See _idlebehind.ps1. Not in the pipeline: about 3 and a half minutes,
	close to the harness limit.
.NOTES
	History: At bottom of file.
#>

##	Test ID: dfskh
##	Box: gpu


$idleArms = @("opaque", "seethru")
. "$PSScriptRoot\_idlebehind.ps1"

##	History:
##		- 20261125 JC: Created.
##		- 20261125 JC: Body moved to _idlebehind.ps1, shared with idlecover.ps1.
