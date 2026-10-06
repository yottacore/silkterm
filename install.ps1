#!/usr/bin/env pwsh

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

<#
.SYNOPSIS
	One-liner installer for a single-binary GitHub release.
.DESCRIPTION
	Detects the OS and CPU, works out which release asset that is, verifies its
	sha256 against the release's checksums file, and installs it. Idempotent:
	states its plan, asks before touching anything, and does nothing when the
	installed binary is already current.

	Reusable: everything project-specific lives in the settings block below.

	Runs on Windows PowerShell 5.1 and on PowerShell 7+ (pwsh) on any platform
	it supports - Windows, Linux and macOS. The OS, the CPU architecture and the
	asset name are all detected.
.PARAMETER Release
	stable (default): newest full release. dev: newest release, pre-releases
	included.
.PARAMETER Target
	user (default): just for you, no elevation needed. system: for everyone
	(needs admin / root).
.PARAMETER Yes
	Skip the confirmation prompt.
.PARAMETER Help
	Print the usage text.
.PARAMETER Version
	Print this installer's version and exit.
.EXAMPLE
	irm https://raw.githubusercontent.com/yottacore/silkterm/main/install.ps1 | iex
.EXAMPLE
	& ([scriptblock]::Create((irm 'https://raw.githubusercontent.com/yottacore/silkterm/main/install.ps1'))) -Release dev

	To pass options.
.NOTES
	History: At bottom of file.
#>

[CmdletBinding()]
param(
	[ValidateSet('stable', 'dev')] [string]$Release = 'stable',
	[ValidateSet('user', 'system')] [string]$Target = 'user',
	[switch]$Yes,
	[switch]$Help,
	[switch]$Version
)


##	•••••••••••••••••••  Per-project settings - edit only these  ••••••••••••••••••

$installerVersion = '1.3.0'
$ownerRepo        = 'yottacore/silkterm'
$appName          = 'SilkTerm'
$exeName          = 'silkterm'
$appComment       = 'Smooth-scrolling GPU terminal with split panes'

##	Release asset names. {exe} {version} {os} {arch} {ext} are substituted; {ext}
##	is ".exe" on Windows and empty elsewhere. {os} is windows/linux/macos, {arch}
##	is x86_64/arm64 - match whatever the release actually publishes.
$assetPattern     = '{exe}-{version}-{os}-{arch}{ext}'
$sumsPattern      = '{exe}-{version}-sha256sums.txt'

##	Windows Start Menu shortcut, and Linux freedesktop launcher. 0 for a non-GUI
##	program (the binary is still installed and still put on PATH).
$menuEntry        = 1
$desktopGenericName = 'Terminal'
$desktopIcon      = 'utilities-terminal'
$desktopCategories = 'System;TerminalEmulator;'
$desktopKeywords  = 'terminal;shell;prompt;command;'

##	••••••••••••••••••••••••  End per-project settings  ••••••••••••••••••••••••••

$apiBase = "https://api.github.com/repos/$ownerRepo"
$dlBase  = "https://github.com/$ownerRepo/releases/download"
$rawBase = "https://raw.githubusercontent.com/$ownerRepo/main"

##	`exit` is only safe when this really IS its own process. The advertised
##	one-liner runs the downloaded text inside the USER'S shell, where an `exit`
##	closes their window instead of ending the install - so failures travel as an
##	exception and only a genuine script file turns that into an exit code.
##	Asked by type: a script block's ScriptInfo has no Path at all, and reading
##	one fails when the caller's shell has StrictMode on.
$runningAsScriptFile = $MyInvocation.MyCommand -is [System.Management.Automation.ExternalScriptInfo]

##	5.0 and older lack Get-FileHash, so there is no verifying a download there.
if ($PSVersionTable.PSVersion.Major -lt 5) {
	Write-Host "Error: this installer needs PowerShell 5.1 or newer (you have $($PSVersionTable.PSVersion))." -ForegroundColor Red
	Write-Host '  Install PowerShell 7: https://aka.ms/powershell'
	Write-Host ''
	if ($runningAsScriptFile) { exit 1 }
	return
}

##	Nothing here sets StrictMode or a preference variable. Run by `irm ... | iex`
##	this text executes in the user's OWN shell, so a change at this level would
##	outlive the install and quietly alter their session. The entry point at the
##	bottom sets all of it inside a script block, which scopes it to the run and
##	needs no restoring - assigning a preference from a child scope only makes a
##	local copy of it anyway, so "save it and put it back" does not work here.


##	Output helpers

##	fFail <message> [hint ...] - one error line, then any hints, then abort.
function fFail {
	param([string]$Message, [string[]]$Hints = @())
	Write-Host ''
	Write-Host "Error: $Message" -ForegroundColor Red
	foreach ($hint in $Hints) { Write-Host "  $hint" }
	Write-Host ''
	##	Carries no message worth printing - fFail has already said everything.
	throw (New-Object System.OperationCanceledException 'installer-abort')
}

function fHelp {
	Write-Host ''
	Write-Host "$appName installer $installerVersion"
	Write-Host ''
	Write-Host "Downloads the newest $appName release from GitHub, checks its sha256, and"
	Write-Host 'installs it. It prints what it is about to do and asks first, and it does'
	Write-Host 'nothing at all when the installed copy is already current.'
	Write-Host ''
	Write-Host 'Usage:'
	Write-Host "  & ([scriptblock]::Create((irm '$rawBase/install.ps1'))) [options]"
	Write-Host ''
	Write-Host 'Options:'
	Write-Host '  -Release stable|dev   stable (default): newest full release'
	Write-Host '                        dev:              newest release, pre-releases included'
	Write-Host '  -Target  user|system  user (default):   just for you, no elevation needed'
	Write-Host '                        system:           for everyone (needs admin / root)'
	Write-Host '  -Yes                  skip the confirmation prompt'
	Write-Host '  -Version              print this installer''s version and exit'
	Write-Host '  -Help                 this text'
	Write-Host ''
	Write-Host 'The operating system, the CPU architecture and the matching release asset are'
	Write-Host 'all detected - there is nothing to pass for them.'
	Write-Host ''
}

##	The message buried in an exception chain is the one worth showing; the outer
##	one is usually just "Exception calling ...".
function fInnerMessage {
	param([System.Management.Automation.ErrorRecord]$ErrorRecord)
	$ex = $ErrorRecord.Exception
	while ($ex.InnerException) { $ex = $ex.InnerException }
	return $ex.Message
}

##	Turn a filesystem failure into something actionable. Access-denied and
##	file-in-use are the two that actually happen, and they need opposite advice.
function fFileError {
	param([System.Management.Automation.ErrorRecord]$ErrorRecord, [string]$What, [string]$Path)
	$ex = $ErrorRecord.Exception
	while ($ex.InnerException) { $ex = $ex.InnerException }
	$msg = $ex.Message
	if ($ex -is [System.UnauthorizedAccessException]) {
		$hints = @("Windows/your OS refused write access to: $Path")
		if ($onWindows) {
			if ($Target -eq 'system') { $hints += 'Re-run from an elevated PowerShell (right-click -> Run as administrator).' }
			else { $hints += 'Check that the folder is not read-only, and that antivirus is not blocking it.' }
			$hints += 'Or use -Target user to install under your own profile instead.'
		} else {
			if ($Target -eq 'system') { $hints += 'Re-run under sudo, or use -Target user to install under $HOME instead.' }
			else { $hints += "Check who owns it:  ls -ld $Path" }
		}
		fFail "$What - permission denied" $hints
	}
	if ($msg -match 'being used by another process|used by another process|text file busy') {
		fFail "$What - the file is in use" @(
			"Another copy of $appName is still running and is holding $Path open.",
			"Close every $appName window (or end its task) and run this again."
		)
	}
	fFail "$What - $msg" @("Path: $Path")
}


##	Environment

##	5.1 is Windows-only and defines no $IsWindows/$IsLinux/$IsMacOS at all, so
##	those cannot simply be read - under StrictMode that is a hard error.
if ($PSVersionTable.PSVersion.Major -ge 6) {
	$onWindows = $IsWindows
	$onMac     = $IsMacOS
} else {
	$onWindows = $true
	$onMac     = $false
}

##	5.1 inherits .NET Framework's TLS default, which on older Windows is still
##	TLS 1.0 - and github.com has refused that for years. The symptom is an
##	unhelpful "underlying connection was closed", so set it before any request.
if (-not $onWindows -or $PSVersionTable.PSVersion.Major -lt 6) {
	try { [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072 } catch {}
}


##	Network + hashing

##	-UseBasicParsing keeps 5.1 off the Internet Explorer engine, which throws
##	outright when IE has never been launched on the machine. 7 dropped the
##	parameter's meaning but still accepts it, so only 5.1 needs it passed.
$webArgs = @{}
if ($PSVersionTable.PSVersion.Major -lt 6) { $webArgs['UseBasicParsing'] = $true }

##	Only the API is rate-limited per IP; release downloads are not, so they
##	stay anonymous.
function fApi {
	param([string]$Url)
	$headers = @{ 'Accept' = 'application/vnd.github+json' }
	if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $($env:GITHUB_TOKEN)" }
	return Invoke-RestMethod -Uri $Url -Headers $headers @webArgs
}

##	The release signing key, as one allowed_signers line. Empty until a key is
##	generated (see cicd/config.bash), and then the checksums file is only trusted
##	when it carries a good signature by this key - which is what turns the check
##	below from "the download was not corrupted" into "this came from the author".
$releaseSignPubkey    = ''
$releaseSignIdentity  = 'releases@silkterm'
$releaseSignNamespace = 'silkterm-release'

##	Verify the checksums file against the pinned key. Everything else is covered
##	by the checksums, so this one signature covers the whole release. ssh-keygen
##	ships with Windows 10 1803 and later.
function fVerifySignature {
	param(
		[Parameter(Mandatory)][string]$Dir,
		[Parameter(Mandatory)][string]$Sums,
		[Parameter(Mandatory)][string]$Tag
	)

	if (-not $releaseSignPubkey) {
		Write-Host 'Note: this release is not signed; the download is checked against its checksums only.'
		return
	}
	$sshKeygen = Get-Command ssh-keygen -ErrorAction SilentlyContinue
	if (-not $sshKeygen) {
		fFail 'ssh-keygen not found, and this release is signed' @(
			'Add the OpenSSH client (Settings > Apps > Optional features) and re-run.'
		)
	}
	$sigPath = Join-Path $Dir "$Sums.sig"
	try { Invoke-WebRequest -Uri "$dlBase/$Tag/$Sums.sig" -OutFile $sigPath @webArgs }
	catch {
		fFail "release $Tag carries no signature ($Sums.sig)" @(
			'This installer only accepts signed releases.',
			"Release page: https://github.com/$ownerRepo/releases/tag/$Tag"
		)
	}
	$signers = Join-Path $Dir 'allowed_signers'
	Set-Content -LiteralPath $signers -Value "$releaseSignIdentity $releaseSignPubkey" -Encoding ascii
	$sumsPath = Join-Path $Dir $Sums
	##	The message goes in on stdin and has to be the file's own bytes, and how
	##	that is done differs by platform. On Windows, Start-Process hands the file
	##	itself to the child as its stdin, which is also the one way the Windows
	##	OpenSSH reads a pipe reliably: written and closed before it has started
	##	up, a pipe never reads as ended there. Elsewhere Start-Process copies the
	##	file as text with a newline of its own on the end, so the bytes go down
	##	a pipe by hand. Every argument is quoted because the string is passed
	##	as-is, and 5.1 has no ArgumentList.
	$verifyArgs = '-Y verify -f "{0}" -I {1} -n {2} -s "{3}"' -f `
		$signers, $releaseSignIdentity, $releaseSignNamespace, $sigPath
	if ($onWindows) {
		$out = Join-Path $Dir 'verify.out'
		$err = Join-Path $Dir 'verify.err'
		$proc = Start-Process -FilePath $sshKeygen.Source -ArgumentList $verifyArgs -NoNewWindow -Wait -PassThru `
			-RedirectStandardInput $sumsPath -RedirectStandardOutput $out -RedirectStandardError $err
	} else {
		$psi = New-Object System.Diagnostics.ProcessStartInfo
		$psi.FileName  = $sshKeygen.Source
		$psi.Arguments = $verifyArgs
		$psi.UseShellExecute        = $false
		$psi.CreateNoWindow         = $true
		$psi.RedirectStandardInput  = $true
		$psi.RedirectStandardOutput = $true
		$psi.RedirectStandardError  = $true
		$proc = [System.Diagnostics.Process]::Start($psi)
		$message = [System.IO.File]::ReadAllBytes($sumsPath)
		$proc.StandardInput.BaseStream.Write($message, 0, $message.Length)
		$proc.StandardInput.Close()
		$null = $proc.StandardOutput.ReadToEnd()
		$null = $proc.StandardError.ReadToEnd()
		$proc.WaitForExit()
	}
	if ($proc.ExitCode -ne 0) {
		fFail 'the release signature does not verify - NOT installing' @(
			'The checksums file was not signed by the release key.',
			'Do not use this download; report it.'
		)
	}
	Write-Host 'Signature OK.'
}

##	fFieldCmp <a> <b> - -1, 0 or 1 for one dotted field. Numbers compare as
##	numbers and sort below words. Two words with the same letters and a trailing
##	number compare by that number, so beta10 is above beta3.
function fFieldCmp {
	param([string]$A, [string]$B)
	$aNum = $A -match '^[0-9]+$'
	$bNum = $B -match '^[0-9]+$'
	if ($aNum -and $bNum) { return [Math]::Sign(([decimal]$A).CompareTo([decimal]$B)) }
	if ($aNum) { return -1 }
	if ($bNum) { return 1 }
	if ($A -match '^([^0-9]*)([0-9]+)$') { $aStem = $Matches[1]; $aTail = $Matches[2] } else { $aStem = $null; $aTail = $null }
	if ($B -match '^([^0-9]*)([0-9]+)$') { $bStem = $Matches[1]; $bTail = $Matches[2] } else { $bStem = $null; $bTail = $null }
	if ($null -ne $aTail -and $null -ne $bTail -and $aStem -ceq $bStem) { return fFieldCmp $aTail $bTail }
	return [Math]::Sign([string]::CompareOrdinal($A, $B))
}

##	fListCmp <a> <b> <missing> - dotted lists, field by field. <missing> is what
##	a list that runs out first counts as: -1 for a pre-release, where fewer
##	fields sort lower, or 0 for the core, where 1.0 is 1.0.0.
function fListCmp {
	param([string]$A, [string]$B, [int]$Missing)
	$aParts = @($A -split '\.')
	$bParts = @($B -split '\.')
	$count = [Math]::Max($aParts.Count, $bParts.Count)
	for ($i = 0; $i -lt $count; $i++) {
		if ($i -ge $aParts.Count) { if ($Missing -eq 0) { $x = '0' } else { return -1 } } else { $x = $aParts[$i] }
		if ($i -ge $bParts.Count) { if ($Missing -eq 0) { $y = '0' } else { return 1 } } else { $y = $bParts[$i] }
		$c = fFieldCmp $x $y
		if ($c -ne 0) { return $c }
	}
	return 0
}

##	fNewer <a> <b> - true when tag <a> is a higher version than <b>, in semver
##	order: 1.0.0-alpha.2 is below 1.0.0, where plain version sorts put it above.
function fNewer {
	param([string]$A, [string]$B)
	$aCore, $aPre = (($A -replace '^v', '') -replace '\+.*$', '') -split '-', 2
	$bCore, $bPre = (($B -replace '^v', '') -replace '\+.*$', '') -split '-', 2
	$c = fListCmp -A $aCore -B $bCore -Missing 0
	if ($c -ne 0) { return ($c -gt 0) }
	##	Same core: a release is above any of its pre-releases.
	if (-not $aPre) { return [bool]$bPre }
	if (-not $bPre) { return $false }
	return ((fListCmp -A $aPre -B $bPre -Missing -1) -gt 0)
}

##	fPickTag <releases> <stable|dev> - the highest version in an API release
##	list, skipping drafts, and pre-releases too for stable. $null if none.
function fPickTag {
	param([object[]]$Releases, [string]$Want)
	$best = $null
	foreach ($rel in @($Releases)) {
		if ($rel.draft) { continue }
		if ($Want -eq 'stable' -and $rel.prerelease) { continue }
		if ($null -eq $best -or (fNewer $rel.tag_name $best)) { $best = [string]$rel.tag_name }
	}
	return $best
}

##	Exec= is read twice: the desktop-entry string rules first, then the Exec
##	quoting rules on top. So a backslash in the path ends up as four, a quote,
##	backtick or '$' as two-plus-itself, and a literal '%' has to be doubled or it
##	reads as a field code. The caller quotes the whole value, which a space needs.
function fDesktopExec {
	param([string]$Path)
	$s = $Path -replace '([\\"`$])', '\$1'
	$s = $s -replace '\\', '\\'
	return ($s -replace '%', '%%')
}

##	fPathNote <dir> <file> - how to run it when <dir> is not on PATH (Linux, macOS).
function fPathNote {
	param([string]$Dir, [string]$File)
	if (":$($env:PATH):" -like "*:$Dir`:*") { return }
	Write-Host ''
	Write-Host "Note: $Dir is not on your PATH, so '$exeName' won't be found by name yet."
	Write-Host "  Add it with:  echo 'export PATH=`"$Dir`:`$PATH`"' >> ~/.profile"
	Write-Host "  Until then, run it in full:  $File"
}


function fMain {

	Write-Host ''

	##	A local copy, because the stable -> dev fallback below rewrites it. Note
	##	nothing in here uses a $script:-qualified variable: that scope does not
	##	exist when this text is run as a script BLOCK, which is one of the three
	##	ways the one-liner reaches a user.
	$release = $Release

	##	Detect the platform. Windows reports the CPU through the environment,
	##	because [RuntimeInformation] needs .NET 4.7.1 and 5.1 predates that.
	$osToken = ''; $archToken = ''; $exeExt = ''; $osProblem = ''
	if ($onWindows) {
		$osToken = 'windows'; $exeExt = '.exe'
		$rawArch = $env:PROCESSOR_ARCHITEW6432
		if (-not $rawArch) { $rawArch = $env:PROCESSOR_ARCHITECTURE }
	} elseif ($onMac) {
		$osToken = 'macos'
		$rawArch = (& uname -m)
	} else {
		$osToken = 'linux'
		$rawArch = (& uname -m)
	}
	##	Every `break` is load-bearing: a PowerShell switch runs EVERY matching
	##	branch, so without them "arm64" matches the arm64 arm and then the
	##	32-bit-ARM arm, and the last one wins.
	switch -Regex ("$rawArch".ToLower()) {
		'^(amd64|x64|x86_64)$'  { $archToken = 'x86_64'; break }
		'^(arm64|aarch64)$'     { $archToken = 'arm64'; break }
		'^(x86|i[3-6]86)$'      { $osProblem = '32-bit x86 is not supported'; break }
		'^arm'                  { $osProblem = '32-bit ARM is not supported'; break }
		default                 { $osProblem = "unrecognized CPU architecture: $rawArch" }
	}
	if ($osProblem) {
		fFail $osProblem @(
			"No $appName build is published for this platform.",
			"Building from source: https://github.com/$ownerRepo#build-it-yourself"
		)
	}

	##	Resolve the release tag: the highest version in the release list, drafts
	##	skipped. Stable wants a full release, and takes the newest pre-release
	##	only when the list holds none, which is what makes a project with only
	##	betas installable. A failed call is an error, never "no release".
	Write-Host "Looking up the newest $release release of $appName ..."
	try {
		##	5.1 passes the array on as one object, so collect it before wrapping.
		$rels = fApi "$apiBase/releases?per_page=100"
		$rels = @($rels)
	} catch {
		$apiError = fInnerMessage $_
		$apiBody = if ($_.ErrorDetails) { $_.ErrorDetails.Message } else { '' }
		if ("$apiError $apiBody" -match 'rate limit') {
			fFail "GitHub's API rate limit is exhausted for this IP" @(
				'Wait an hour, or set $env:GITHUB_TOKEN to a personal access token and re-run.'
			)
		}
		fFail "could not read the release list from github.com/$ownerRepo" @(
			'Check your network or proxy, and that the repository still exists.',
			"Detail: $apiError"
		)
	}
	$tag = fPickTag $rels $release
	if (-not $tag -and $release -eq 'stable') {
		$tag = fPickTag $rels 'dev'
		if ($tag) {
			Write-Host 'No full release published yet; using the newest pre-release instead.'
			$release = 'dev'
		}
	}
	if (-not $tag) {
		fFail "github.com/$ownerRepo has no release published yet" @(
			"Building from source: https://github.com/$ownerRepo#build-it-yourself"
		)
	}
	$version = $tag -replace '^v', ''

	##	Work out the asset name for this platform
	$asset = $assetPattern -replace '\{exe\}', $exeName -replace '\{version\}', $version `
		-replace '\{os\}', $osToken -replace '\{arch\}', $archToken -replace '\{ext\}', $exeExt
	$sums = $sumsPattern -replace '\{exe\}', $exeName -replace '\{version\}', $version

	##	Pull the checksums first: it is small, it says which platforms this
	##	release actually carries, and its hash lets an already-current install
	##	finish without downloading the binary at all.
	##	A predictable name under a shared temp directory is somebody else's to
	##	create first, and '-Force' would then adopt it - or the symlink they put
	##	there. Random name, and no '-Force', so an existing path is an error.
	$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) `
		("$exeName-install-" + [System.IO.Path]::GetRandomFileName())
	try { New-Item -ItemType Directory -Path $tmpDir -ErrorAction Stop | Out-Null }
	catch { fFail "could not create a temporary directory ($tmpDir)" @($_.Exception.Message) }
	try {
		$sumsPath = Join-Path $tmpDir $sums
		try { Invoke-WebRequest -Uri "$dlBase/$tag/$sums" -OutFile $sumsPath @webArgs }
		catch {
			fFail "release $tag has no checksums file ($sums)" @(
				'Nothing can be verified without it, so nothing will be installed.',
				"Release page: https://github.com/$ownerRepo/releases/tag/$tag"
			)
		}

		fVerifySignature -Dir $tmpDir -Sums $sums -Tag $tag

		$wantSha = $null
		$published = New-Object System.Collections.Generic.List[string]
		foreach ($line in (Get-Content -LiteralPath $sumsPath)) {
			$parts = $line -split '\s+', 2
			if ($parts.Count -ne 2) { continue }
			$name = $parts[1].Trim().TrimStart('*')
			$published.Add($name)
			if ($name -eq $asset) { $wantSha = $parts[0].ToLower() }
		}
		if (-not $wantSha) {
			fFail "release $tag has no build for $osToken-$archToken" (@(
				"Expected asset: $asset",
				'What it does carry:'
			) + ($published | ForEach-Object { "  $_" }) + @(
				"Building from source: https://github.com/$ownerRepo#build-it-yourself"
			))
		}

		##	Destination
		$menuDir = ''
		$appDir = ''
		if ($onWindows) {
			if ($Target -eq 'user') {
				$destDir = Join-Path $env:LOCALAPPDATA "Programs\$appName"
				$menuDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
				$pathScope = 'User'
			} else {
				$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
				if (-not $admin) {
					fFail 'a system-wide install needs an elevated PowerShell' @(
						'Right-click PowerShell -> Run as administrator, and run this again,',
						'or drop -Target system to install just for you (no elevation needed).'
					)
				}
				##	32-bit PowerShell on 64-bit Windows sees Program Files (x86) here.
				$programFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
				$destDir = Join-Path $programFiles $appName
				$menuDir = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
				$pathScope = 'Machine'
			}
			$destFile = Join-Path $destDir "$exeName.exe"
		} else {
			if ($Target -eq 'user') {
				$destDir = Join-Path $HOME '.local/bin'
				$appDir = Join-Path $HOME '.local/share/applications'
			} else {
				if ((& id -u) -ne '0') {
					fFail 'a system-wide install needs root' @(
						'Re-run it under sudo, or drop -Target system to install under $HOME instead.'
					)
				}
				$destDir = '/usr/local/bin'
				$appDir = '/usr/local/share/applications'
			}
			$destFile = Join-Path $destDir $exeName
		}
		if ($menuEntry -ne 1 -or $onMac) { $menuDir = ''; $appDir = '' }

		##	Already current? Then only the pieces that went missing are left to
		##	do, and with none missing, say so and stop - no prompt, no download.
		##	A piece that exists is left as it is, since it may have been edited.
		##	A file that cannot be READ (locked by a running copy) must not throw
		##	here: fall through unresolved and let the copy fail with a message
		##	that actually says what to do about it.
		$installedSha = ''
		if (Test-Path -LiteralPath $destFile) {
			try { $installedSha = (Get-FileHash -Algorithm SHA256 -LiteralPath $destFile).Hash.ToLower() }
			catch { $installedSha = '' }
		}
		$needBinary = $installedSha -ne $wantSha
		$menuFile = if ($menuDir) { Join-Path $menuDir "$appName.lnk" } else { '' }
		$appFile = if ($appDir) { Join-Path $appDir "$exeName.desktop" } else { '' }
		$needMenu = [bool]$menuFile -and ($needBinary -or -not (Test-Path -LiteralPath $menuFile))
		$needApp = [bool]$appFile -and ($needBinary -or -not (Test-Path -LiteralPath $appFile))
		$needPath = $onWindows -and -not (fOnWindowsPath $destDir $pathScope)
		if (-not ($needBinary -or $needMenu -or $needApp -or $needPath)) {
			Write-Host ''
			Write-Host "Already up to date: $destFile is $tag. Nothing to do."
			if (-not $onWindows) { fPathNote $destDir $destFile }
			Write-Host ''
			return
		}

		##	The plan
		Write-Host ''
		Write-Host 'Plan:'
		if ($needBinary) {
			Write-Host "  Program:  $appName $tag ($release)"
			Write-Host "  Platform: $osToken-$archToken"
			Write-Host "  Download: $dlBase/$tag/$asset"
			Write-Host "  Verify:   sha256 against $sums"
			Write-Host "  Install:  $destFile"
		} else {
			Write-Host "  Program:  $destFile is already $tag"
		}
		if ($needMenu) { Write-Host "  Shortcut: $menuFile" }
		if ($needApp)  { Write-Host "  Launcher: $appFile" }
		if ($needPath) { Write-Host "  PATH:     $destDir added to the $pathScope PATH" }
		Write-Host ''
		if (-not $Yes) {
			##	Read-Host hands back an empty COLLECTION at end-of-input, and
			##	`@() -notmatch ...` is itself an empty collection - which is
			##	falsy, so an `if (... -notmatch ...)` abort branch silently does
			##	not run and the thing installs unasked. Cast to a string, then
			##	test for an explicit yes, so anything unexpected declines.
			##	[Environment]::UserInteractive is no help here: it stays True
			##	with stdin redirected from nowhere.
			##	"$(...)" flattens $null and an empty collection alike to ''; a
			##	plain [string] cast of the latter still comes back $null, and
			##	.Trim() on that throws.
			$answer = ''
			$cannotAsk = $false
			try { $answer = "$(Read-Host 'Proceed? [y/N]')".Trim().ToLowerInvariant() } catch { $cannotAsk = $true }
			if ($answer -ne 'y' -and $answer -ne 'yes') {
				if ($cannotAsk -or ($answer -eq '' -and [Console]::IsInputRedirected)) {
					fFail 'there is no terminal here to ask for confirmation' @(
						'Re-run with -Yes to install without being asked.'
					)
				}
				Write-Host 'Aborted - nothing was touched.'
				Write-Host ''
				return
			}
			Write-Host ''
		}

		if ($needBinary) {
			##	Download + verify
			Write-Host "Downloading $asset ..."
			$assetPath = Join-Path $tmpDir $asset
			try { Invoke-WebRequest -Uri "$dlBase/$tag/$asset" -OutFile $assetPath @webArgs }
			catch {
				fFail 'download failed' @(
					'The release lists this asset, so this is most likely a network problem.',
					"URL: $dlBase/$tag/$asset",
					"Detail: $(fInnerMessage $_)"
				)
			}
			$haveSha = (Get-FileHash -Algorithm SHA256 -LiteralPath $assetPath).Hash.ToLower()
			if ($haveSha -ne $wantSha) {
				fFail 'checksum mismatch - NOT installing' @(
					"expected $wantSha",
					"got      $haveSha",
					'The download was corrupted or tampered with. Try again; if it repeats, report it.'
				)
			}
			Write-Host 'Checksum OK.'

			##	Install. Land beside the target and rename into place, as
			##	install.bash does, so a running copy does not stop an upgrade.
			##	Linux refuses to write over a running program ("text file busy")
			##	but lets a rename replace it. Windows refuses both, but lets the
			##	running file itself be renamed out of the way; the old copy is
			##	removed by the next upgrade, once nothing has it open.
			Write-Host ''
			Write-Host 'Installing ...'
			try { New-Item -ItemType Directory -Force -Path $destDir | Out-Null }
			catch { fFileError -ErrorRecord $_ -What "could not create $destDir" -Path $destDir }
			$staged = Join-Path $destDir (".$exeName-new-" + [System.IO.Path]::GetRandomFileName())
			try { Copy-Item -LiteralPath $assetPath -Destination $staged }
			catch { fFileError -ErrorRecord $_ -What "could not write to $destDir" -Path $staged }
			try {
				if ($onWindows) {
					Get-ChildItem -LiteralPath $destDir -Filter "$exeName.exe.old-*" -Force -ErrorAction SilentlyContinue |
						ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }
					if (Test-Path -LiteralPath $destFile) {
						try { [System.IO.File]::Delete($destFile) }
						catch { [System.IO.File]::Move($destFile, "$destFile.old-" + [System.IO.Path]::GetRandomFileName()) }
					}
					[System.IO.File]::Move($staged, $destFile)
				} else {
					& chmod 0755 $staged
					##	By path: on Windows 'mv' is Move-Item's alias, and the lint there says so.
					$mvExe = (Get-Command -Name mv -CommandType Application -TotalCount 1).Source
					& $mvExe -f $staged $destFile
					if ($LASTEXITCODE -ne 0) { throw "mv exited $LASTEXITCODE" }
				}
			} catch {
				Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
				fFileError -ErrorRecord $_ -What "could not replace $destFile" -Path $destFile
			}
		}

		##	Start Menu shortcut (Windows)
		if ($needMenu) {
			try {
				New-Item -ItemType Directory -Force -Path $menuDir | Out-Null
				$shell = New-Object -ComObject WScript.Shell
				$lnk = $shell.CreateShortcut($menuFile)
				$lnk.TargetPath = $destFile
				$lnk.WorkingDirectory = $destDir
				$lnk.Description = $appComment
				$lnk.Save()
			} catch {
				Write-Host "Note: could not create the Start Menu shortcut ($(fInnerMessage $_)) - $appName itself installed fine."
			}
		}

		##	Freedesktop launcher (Linux)
		if ($needApp) {
			try {
				New-Item -ItemType Directory -Force -Path $appDir | Out-Null
				@(
					'[Desktop Entry]', 'Type=Application', "Name=$appName",
					"GenericName=$desktopGenericName", "Comment=$appComment",
					('Exec="' + (fDesktopExec $destFile) + '"'),
					"Icon=$desktopIcon", 'Terminal=false', "Categories=$desktopCategories",
					"Keywords=$desktopKeywords", 'StartupNotify=true'
				) | Set-Content -LiteralPath $appFile
			} catch {
				Write-Host "Note: could not write the desktop launcher to $appDir - $appName itself installed fine."
			}
		}

		##	PATH
		if ($needPath) { fAddToWindowsPath $destDir $pathScope }
		if (-not $onWindows) { fPathNote $destDir $destFile }

		Write-Host ''
		if ($needBinary) { Write-Host "Installed $appName $tag to $destFile" }
		else { Write-Host "Put back what was missing for $appName $tag" }
		Write-Host ''
	} finally {
		Remove-Item -Recurse -Force -LiteralPath $tmpDir -ErrorAction SilentlyContinue
	}
}


##	Windows PATH

##	fPathKey <scope> <writable> - the registry key holding that scope's PATH.
function fPathKey {
	param([string]$Scope, [bool]$Writable)
	if ($Scope -eq 'Machine') {
		$root = [Microsoft.Win32.Registry]::LocalMachine
		$sub = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
	} else {
		$root = [Microsoft.Win32.Registry]::CurrentUser
		$sub = 'Environment'
	}
	$key = $root.OpenSubKey($sub, $Writable)
	if (-not $key) { throw "cannot open HKEY\$sub" }
	return $key
}

##	True when the persistent PATH of that scope already names the folder. A
##	PATH that cannot be read counts as missing, so the install tries and says.
function fOnWindowsPath {
	param([string]$Dir, [string]$Scope)
	try {
		$key = fPathKey $Scope $false
		try {
			$raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
			return (($raw -split ';') -contains $Dir)
		} finally { $key.Close() }
	} catch { return $false }
}

##	Append to the persistent PATH via the registry rather than via
##	[Environment]::SetEnvironmentVariable, which rewrites a REG_EXPAND_SZ PATH as
##	a plain REG_SZ - that silently kills every %VAR% already in it. Reading and
##	writing the raw value keeps whatever kind it already was.
function fAddToWindowsPath {
	param([string]$Dir, [string]$Scope)
	try {
		$key = fPathKey $Scope $true
		try {
			$raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
			$kind = if ($raw) { $key.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
			if (($raw -split ';') -contains $Dir) { return }
			$new = if ($raw) { $raw.TrimEnd(';') + ';' + $Dir } else { $Dir }
			##	A User PATH over ~2047 chars gets truncated by parts of Windows.
			##	Losing an existing PATH is far worse than not being on it.
			if ($Scope -eq 'User' -and $new.Length -gt 2047) {
				Write-Host "Note: your user PATH is already near the length limit, so $Dir was NOT added (nothing was changed)."
				Write-Host "  Run $appName in full:  $(Join-Path $Dir "$exeName.exe")"
				return
			}
			$key.SetValue('Path', $new, $kind)
			$env:PATH = "$env:PATH;$Dir"
			Write-Host "Added $Dir to the $Scope PATH - already-open shells need a restart to see it."
		} finally { $key.Close() }
		fAnnounceEnvironment
	} catch {
		Write-Host "Note: could not update the $Scope PATH ($(fInnerMessage $_)) - $appName itself installed fine."
		Write-Host "  Run it in full:  $(Join-Path $Dir "$exeName.exe")"
	}
}

##	A registry write alone reaches nothing until the next sign-in. Explorer, which
##	starts everything from the Start menu, reloads its environment only when told,
##	and that is the message SetEnvironmentVariable and setx both send after their
##	own write.
function fAnnounceEnvironment {
	try {
		if (-not ('SilkInstall.Env' -as [type])) {
			Add-Type -Namespace SilkInstall -Name Env -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
		}
		$ignored = [UIntPtr]::Zero
		##	HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG, and 5 s for a window that is stuck
		[void][SilkInstall.Env]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$ignored)
	} catch {
		Write-Host "  A console opened from the Start menu may not find it until you sign out and in again."
	}
}


##	Script entry point. The `& { }` is what keeps StrictMode off the caller's
##	shell - it applies to this block and everything it calls, and lapses here.
##	A hashtable rather than the block's return value: it is a reference, so the
##	child scope can set it directly, and a stray line of pipeline output from
##	anything in here cannot turn the answer into an array.
$state = @{ failed = $false }
& {
	Set-StrictMode -Version Latest
	$ErrorActionPreference = 'Stop'
	##	On 5.1 the progress bar makes Invoke-WebRequest an order of magnitude
	##	slower. Both of these lapse with the block, so nothing needs restoring.
	$ProgressPreference = 'SilentlyContinue'
	try {
		if ($Help) { fHelp }
		elseif ($Version) { Write-Host ''; Write-Host "$appName installer $installerVersion"; Write-Host '' }
		else { fMain }
	} catch [System.OperationCanceledException] {
		##	A handled failure; fFail already printed what went wrong and why.
		$state.failed = $true
	} catch {
		##	Anything unforeseen still reads as a sentence, not a stack trace.
		Write-Host ''
		Write-Host "Error: $(fInnerMessage $_)" -ForegroundColor Red
		Write-Host ''
		$state.failed = $true
	}
} | Out-Null
if ($state.failed -and $runningAsScriptFile) { exit 1 }

##	History:
##		- 20260723 JC: Created.
##		- 20260806 JC: Made project-agnostic; dropped -Arch for autodetection;
##		  added -Version; runs on Windows PowerShell 5.1 as well as 7+;
##		  permission and lock failures now explain themselves.
##		- 20260807 JC: Safe to run from `irm | iex`, where the text executes in the
##		  caller's own shell: no `exit` (it would close their window),
##		  no $script: scope (absent in a script block), and StrictMode
##		  plus the preference variables scoped to the run.
##		- 20260917 JC: The signature check feeds ssh-keygen the checksums file's own
##		  bytes on Linux and macOS, where Start-Process rewrote it.
##		- 20260924 JC: Release list read correctly on 5.1, which hands an API array
##		  over as one object; a -NonInteractive run fails with the -Yes
##		  hint instead of quietly aborting; a system install from 32-bit
##		  PowerShell goes to the 64-bit Program Files.
##		- 20260925 JC: Picks the highest version from the release list and skips
##		  drafts; an API error no longer reads as "no full release";
##		  upgrades over a running copy; a re-run puts back a missing
##		  shortcut, launcher or PATH entry.
##		- 20261006 JC: Help block; StrictMode Latest; named arguments where there
##		  were three by position. Runs from a shell that has StrictMode on.
##		  mv is called by path.

