##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A window under GPU load with Free resources when idle on and a short idle
	time, so the device is let go and built again under load.
.DESCRIPTION
	See _gpuload.ps1. Not in the pipeline: it needs the load program sent along.
.NOTES
	History: At bottom of file.
#>

##	Test ID: ErgpmyB


$idleRelease = $true
. "$PSScriptRoot\_gpuload.ps1"

##	History:
##		- 20261003 JC: Created.
##		- 20261006 JC: Help block.
