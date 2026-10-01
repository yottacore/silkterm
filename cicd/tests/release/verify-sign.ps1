#!/usr/bin/env pwsh

##	- Purpose:
##		Run install.ps1's own signature check, not a retyped command, against a
##		throwaway key: a good signature passes, and a changed checksums file, a
##		missing signature and another key's signature are each refused.
##	- Self-contained (keys, signing and the checks all happen here), so the
##		Windows pipeline runs it as well as the Linux one. Needs ssh-keygen.
##	- Syntax: verify-sign.ps1 [-Installer <path to install.ps1>]
##	- Exit: 0 when every check passed or ssh-keygen is missing, 1 otherwise.
##	- Test ID: Eq9wAnY
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param(
	[string]$Installer = (Join-Path $PSScriptRoot '../../../install.ps1')
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../_testdir.ps1'); fTestDir_Use

$sshKeygen = Get-Command ssh-keygen -ErrorAction SilentlyContinue
if (-not $sshKeygen) { Write-Host '  skip installer signing (no ssh-keygen)'; exit 0 }

##	ssh-keygen with an Arguments string, so an empty passphrase (`-N ""`)
##	reaches it the same way on 5.1 and 7, which pass an empty argument
##	differently. Stdin is closed at once: left inherited, the Windows build
##	never returned under an ssh session.
function fKeygen {
	param([string]$Arguments)
	$psi = New-Object System.Diagnostics.ProcessStartInfo
	$psi.FileName  = $sshKeygen.Source
	$psi.Arguments = $Arguments
	$psi.UseShellExecute = $false
	$psi.CreateNoWindow  = $true
	$psi.RedirectStandardInput  = $true
	$psi.RedirectStandardOutput = $true
	$psi.RedirectStandardError  = $true
	$proc = [System.Diagnostics.Process]::Start($psi)
	$proc.StandardInput.Close()
	$null = $proc.StandardOutput.ReadToEnd()
	$null = $proc.StandardError.ReadToEnd()
	$proc.WaitForExit()
	if ($proc.ExitCode -ne 0) { throw "ssh-keygen $Arguments failed ($($proc.ExitCode))" }
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("silk-sign-" + [System.IO.Path]::GetRandomFileName())
$null = New-Item -ItemType Directory -Path $work
$rc = 1
try {
	$text = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $Installer).Path)
	##	The installer's own function, its failure path and its identity and
	##	namespace, lifted from the file so the test cannot drift from it.
	function fLift {
		param([string]$Name)
		$m = [regex]::Match($text, "(?ms)^function $Name \{\r?\n.*?^\}\r?$")
		if (-not $m.Success) { throw "function $Name not found in $Installer" }
		return $m.Value
	}
	function fSetting {
		param([string]$Name)
		$m = [regex]::Match($text, "(?m)^\`$$Name\s*=\s*'([^']*)'")
		if (-not $m.Success) { throw "setting `$$Name not found in $Installer" }
		return $m.Groups[1].Value
	}
	$identity  = fSetting 'releaseSignIdentity'
	$namespace = fSetting 'releaseSignNamespace'

	$keyDir = Join-Path $work 'key'
	$null = New-Item -ItemType Directory -Path $keyDir
	$key   = Join-Path $keyDir 'id'
	$other = Join-Path $keyDir 'other'
	fKeygen ('-q -t ed25519 -N "" -C {0} -f "{1}"' -f $identity, $key)
	fKeygen ('-q -t ed25519 -N "" -C other -f "{0}"' -f $other)
	$pubkey = ([System.IO.File]::ReadAllText("$key.pub")).Trim()

	##	What the installer sees: a download folder standing in for the release
	##	page, and a working folder holding the checksums file it fetched.
	$dl   = Join-Path $work 'dl'
	$tag  = 'v0.0.0-test'
	$inst = Join-Path $work 'inst'
	$null = New-Item -ItemType Directory -Path (Join-Path $dl $tag)
	$null = New-Item -ItemType Directory -Path $inst
	$sums = 'sums.txt'
	$good = [System.Text.Encoding]::ASCII.GetBytes("0123456789abcdef  silkterm-0.0.0-test`n")
	$signed = Join-Path $dl "$tag/$sums"
	[System.IO.File]::WriteAllBytes($signed, $good)
	fKeygen ('-Y sign -f "{0}" -n {1} "{2}"' -f $key, $namespace, $signed)
	$sig = "$signed.sig"

	$lifted = @(
		"`$releaseSignPubkey    = '$pubkey'",
		"`$releaseSignIdentity  = '$identity'",
		"`$releaseSignNamespace = '$namespace'",
		"`$ownerRepo = '$(fSetting 'ownerRepo')'",
		"`$dlBase  = '$($dl -replace "'", "''")'",
		"`$webArgs = @{}",
		"`$onWindows = `$$(if ($PSVersionTable.PSVersion.Major -ge 6) { $IsWindows } else { $true })",
		##	The installer fetches the signature over https. Here the release is a
		##	folder, so the same call copies the file, or fails when it is missing.
		'function Invoke-WebRequest { param($Uri, $OutFile, [switch]$UseBasicParsing) Copy-Item -LiteralPath $Uri -Destination $OutFile }',
		(fLift 'fVerifySignature'),
		(fLift 'fFail')
	) -join "`n"
	. ([scriptblock]::Create($lifted))

	function fVerifies {
		param([byte[]]$Message)
		[System.IO.File]::WriteAllBytes((Join-Path $inst $sums), $Message)
		try { fVerifySignature -Dir $inst -Sums $sums -Tag $tag *>$null; return $true }
		catch [System.OperationCanceledException] { return $false }
	}

	$failures = 0
	function fCheck {
		param([string]$What, [bool]$Want, [bool]$Got)
		if ($Want -eq $Got) { Write-Host "  ok   $What" }
		else { Write-Host "  FAIL $What"; $script:failures++ }
	}

	fCheck 'install.ps1 accepts a signed checksums file' $true (fVerifies $good)
	$tampered = [System.Text.Encoding]::ASCII.GetBytes("fedcba9876543210  silkterm-0.0.0-test`n")
	fCheck 'install.ps1 refuses a changed one' $false (fVerifies $tampered)

	Remove-Item -LiteralPath $sig
	fCheck 'install.ps1 refuses a release with no signature' $false (fVerifies $good)

	fKeygen ('-Y sign -f "{0}" -n {1} "{2}"' -f $other, $namespace, $signed)
	fCheck "install.ps1 refuses another key's signature" $false (fVerifies $good)

	if ($failures) { Write-Host "$failures failed" } else { $rc = 0 }
} finally {
	Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
exit $rc

##	History:
##		- 20260917 JC: Created.
