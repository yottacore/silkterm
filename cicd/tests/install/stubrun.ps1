#!/usr/bin/env pwsh

<#
.SYNOPSIS
	Run install.ps1 for real against a stand-in release.
.DESCRIPTION
	Runs it the way the one-liner does: its text as a script block. The two web
	cmdlets are replaced by functions of the same name, which the block finds
	first. They serve the folder in STUB_DIR, and the API answers with
	STUB_API_CODE. STUB_ONE_OBJECT=1 hands the release list over as one object,
	the way Windows PowerShell 5.1 does, and STUB_NO_YES=1 leaves out -Yes.
.PARAMETER Installer
	Path to install.ps1.
.PARAMETER Rest
	Installer options, as -Name value pairs.
.EXAMPLE
	stubrun.ps1 -Installer ../../../install.ps1 -Release dev
.NOTES
	History: At bottom of file.
#>

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidOverwritingBuiltInCmdlets', '', Justification = 'the stand-ins replace them on purpose')]
[CmdletBinding()]
param(
	[Parameter(Mandatory)][string]$Installer,
	[Parameter(ValueFromRemainingArguments)][string[]]$Rest
)

Set-StrictMode -Version Latest

function Invoke-RestMethod {
	param([string]$Uri, [hashtable]$Headers, [switch]$UseBasicParsing)
	$code = if ($env:STUB_API_CODE) { [int]$env:STUB_API_CODE } else { 200 }
	$body = [System.IO.File]::ReadAllText((Join-Path $env:STUB_DIR 'releases.json'))
	if ($code -ge 300) {
		##	What each PowerShell throws for an HTTP error, body in ErrorDetails.
		if ($PSVersionTable.PSVersion.Major -ge 6) {
			$resp = [System.Net.Http.HttpResponseMessage]::new([System.Net.HttpStatusCode]$code)
			$ex = [Microsoft.PowerShell.Commands.HttpResponseException]::new("Response status code does not indicate success: $code.", $resp)
		} else {
			$ex = New-Object System.Net.WebException "The remote server returned an error: ($code)."
		}
		$err = [System.Management.Automation.ErrorRecord]::new($ex, 'WebCmdletWebResponseException', 'InvalidOperation', $Uri)
		$err.ErrorDetails = [System.Management.Automation.ErrorDetails]::new($body)
		throw $err
	}
	$list = $body | ConvertFrom-Json
	if ($env:STUB_ONE_OBJECT) { return , $list }
	return $list
}

function Invoke-WebRequest {
	param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing)
	Copy-Item -LiteralPath (Join-Path $env:STUB_DIR ($Uri -replace '^.*/', '')) -Destination $OutFile -ErrorAction Stop
}

##	A stub that breaks has to show as a failed run, not a line of noise.
$ErrorActionPreference = 'Stop'

$options = if ($env:STUB_NO_YES) { @{} } else { @{ Yes = $true } }
if ($Rest) { for ($i = 0; $i -lt $Rest.Count; $i += 2) { $options[$Rest[$i].TrimStart('-')] = $Rest[$i + 1] } }
& ([scriptblock]::Create([System.IO.File]::ReadAllText($Installer))) @options

##	History:
##		- 20260925 JC: Created.
##		- 20260926 JC: STUB_ONE_OBJECT and STUB_NO_YES.
##		- 20261006 JC: Help block, StrictMode Latest, typed stand-in parameters.
