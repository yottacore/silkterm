##	Copyright (C) 2026 Jim Collier
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

<#
.SYNOPSIS
	The one way a PowerShell pipeline script removes a folder tree, per Safe
	removal in the CICD rules: saferm.bash's guard, with the same rules.
.DESCRIPTION
	Dot-source this. It defines functions and runs nothing.

	fSafeRm <path>: only somewhere under a folder named tmp, never that folder
	itself, or under this project's own folder in HOST_TEMP, since the Windows
	temp folder isn't named tmp. HOST_TEMP falls back to TEMP on Windows, and
	TMPDIR or /tmp elsewhere.

	fSafeRm_Under <parent> <path>: only somewhere under the one parent named, for
	a folder outside a temp root, such as the release artifacts.

	Both take one absolute path and refuse an empty one, "..", a doubled
	separator, a newline, a link, a path through a link, or one that isn't
	there. Inside the tree a link or junction is removed, never followed. On
	Windows a mount point is one too, so the walk stays on one filesystem there.
	They return $true when the removal went, and otherwise $false, with one line
	on stderr.

	Sets no StrictMode, since dot-sourcing would set it for the caller.
.EXAMPLE
	. (Join-Path $Root 'cicd\utility\include\saferm.ps1'); $null = fSafeRm_Under $parent $dir
.NOTES
	History: At bottom of file.
#>

function fSafeRm_OnWindows { if (Test-Path variable:IsWindows) { $IsWindows } else { $true } }

$script:SafeRmOwn = $(
	$tempRoot = if ($env:HOST_TEMP) { $env:HOST_TEMP } elseif (fSafeRm_OnWindows) { $env:TEMP } elseif ($env:TMPDIR) { $env:TMPDIR } else { '/tmp' }
	if ($tempRoot) { [System.IO.Path]::Combine([System.IO.Path]::GetFullPath($tempRoot), 'silkterm') }
)

function fSafeRm { fSafeRm_Do '' $args }

function fSafeRm_Under {
	if (-not $args.Count -or -not "$($args[0])" -or -not [System.IO.Path]::IsPathFullyQualified("$($args[0])")) {
		[Console]::Error.WriteLine("safe-rm: refused, no parent: `"$(if ($args.Count) { $args[0] })`"")
		return $false
	}
	fSafeRm_Do "$($args[0])".TrimEnd('\', '/') @($args | Select-Object -Skip 1)
}

function fSafeRm_Refuse([string]$Why) { [Console]::Error.WriteLine("safe-rm: refused, $Why"); return $false }

function fSafeRm_Do([string]$Parent, [object[]]$Paths) {
	$win = fSafeRm_OnWindows
	$p = if ($Paths.Count -eq 1) { "$($Paths[0])" } else { '' }
	if ($Paths.Count -ne 1 -or -not $p -or -not [System.IO.Path]::IsPathFullyQualified($p) -or $p.Contains('..') -or $p -match '[\\/]{2}' -or $p -match "[`r`n]") {
		return (fSafeRm_Refuse "bad path: `"$p`"")
	}
	$p = $p.TrimEnd('\', '/')
	$sep = '[\\/]'
	$case = if ($win) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
	if ($Parent) {
		if (-not ($p.Length -gt $Parent.Length + 1 -and $p.StartsWith($Parent, $case) -and $p[$Parent.Length] -match $sep)) { return (fSafeRm_Refuse "not under ${Parent}: $p") }
	} else {
		$own = $script:SafeRmOwn
		$inTmp = if ($win) { $p -match "${sep}tmp${sep}[^\\/]" } else { $p -cmatch '/tmp/[^/]' }
		$inOwn = $own -and $p.Length -gt $own.Length + 1 -and $p.StartsWith($own, $case) -and $p[$own.Length] -match $sep
		if (-not ($inTmp -or $inOwn)) { return (fSafeRm_Refuse "not under a tmp folder: $p") }
	}
	$item = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
	if ($item -and ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { return (fSafeRm_Refuse "a link: $p") }
	if (-not $item) { return (fSafeRm_Refuse "not there: $p") }
	for ($up = [System.IO.Path]::GetDirectoryName($p); $up; $up = [System.IO.Path]::GetDirectoryName($up)) {
		$info = [System.IO.DirectoryInfo]::new($up)
		if ($info.Exists -and ($info.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) { return (fSafeRm_Refuse "goes through a link: $p") }
	}
	try { fSafeRm_Walk $item }
	catch { [Console]::Error.WriteLine("safe-rm: could not remove ${p}: $($_.Exception.Message)"); return $false }
	return $true
}

##	Its own walk: Remove-Item -Recurse in Windows PowerShell 5.1 follows
##	junctions, and a .NET recursive delete stops at a read-only file.
function fSafeRm_Walk([System.IO.FileSystemInfo]$Entry) {
	$readOnly = [System.IO.FileAttributes]::ReadOnly
	$isFolder = [bool]($Entry.Attributes -band [System.IO.FileAttributes]::Directory)
	if ($Entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
		if ($isFolder -and (fSafeRm_OnWindows)) { [System.IO.Directory]::Delete($Entry.FullName, $false) }
		else { [System.IO.File]::Delete($Entry.FullName) }
		return
	}
	$Entry.Attributes = $Entry.Attributes -band -bnot $readOnly
	if ($isFolder) { foreach ($child in ([System.IO.DirectoryInfo]$Entry.FullName).GetFileSystemInfos()) { fSafeRm_Walk $child } }
	$Entry.Delete()
}

##	History:
##		- 20261028: Created.
