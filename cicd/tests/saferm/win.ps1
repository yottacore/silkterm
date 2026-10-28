#!/usr/bin/env pwsh

##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	The PowerShell guard every recursive removal goes through, saferm.ps1.
.DESCRIPTION
	A wrong answer here deletes something that was never ours, so this plants
	folders, links and odd paths beside each other, and checks that only the
	ones it may take go. A link inside a tree goes, and what it points at
	stays. Junctions on Windows, symbolic links elsewhere.

	Works in a folder of its own under the project's folder in the temp root,
	the one place the guard takes outside a folder named tmp.
.NOTES
	Exit: 0 when every check passed, 1 otherwise.
	History: At bottom of file.
#>

##	Test ID: Eu0ji27

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot '../../utility/include/saferm.ps1')

$failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" } else { Write-Host "  FAIL $What"; $script:failures++ }
}

##	What the guard said on stderr, and what it answered.
function fTry { param([scriptblock]$Call)
	$saved = [Console]::Error
	$err = [System.IO.StringWriter]::new()
	[Console]::SetError($err)
	try { $answer = & $Call } finally { [Console]::SetError($saved) }
	[pscustomobject]@{ Ok = ($answer -is [bool] -and $answer); Said = $err.ToString().Trim() }
}
function fRefused { param([scriptblock]$Call, [string]$Why)
	$r = fTry $Call
	return (-not $r.Ok -and $r.Said.StartsWith("safe-rm: refused, $Why"))
}

$win = fSafeRm_OnWindows
$sep = [System.IO.Path]::DirectorySeparatorChar
function fLink { param([string]$Path, [string]$To)
	$type = if ($win) { 'Junction' } else { 'SymbolicLink' }
	New-Item -ItemType $type -Path $Path -Target $To | Out-Null
}
function fFile { param([string]$Path) Set-Content -LiteralPath $Path -Value 'x' }
function fDir { param([string]$Path) New-Item -ItemType Directory -Path $Path -Force | Out-Null; return $Path }

##	<own>/<stamp>_NNN, made with a plain mkdir and counted up when taken.
$own = $script:SafeRmOwn
New-Item -ItemType Directory -Path $own -Force | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$work = $null
for ($n = 1; -not $work; $n++) {
	$try = Join-Path $own ('{0}_{1:000}' -f $stamp, $n)
	try { New-Item -ItemType Directory -Path $try | Out-Null; $work = $try } catch { if ($n -ge 999) { throw } }
}

##	Outside any tree the guard is asked to take: what a link points at.
$victim = fDir (Join-Path $work 'victim')
fFile (Join-Path $victim 'keep.txt')
$tmp = fDir (Join-Path $work 'tmp')

fCheck 'no path is refused'               (fRefused { fSafeRm } 'bad path')
fCheck 'two paths are refused'            (fRefused { fSafeRm (Join-Path $tmp 'a') (Join-Path $tmp 'b') } 'bad path')
fCheck 'an empty path is refused'         (fRefused { fSafeRm '' } 'bad path')
fCheck 'a relative path is refused'       (fRefused { fSafeRm (Join-Path 'tmp' 'x') } 'bad path')
fCheck 'a path with .. is refused'        (fRefused { fSafeRm "$tmp${sep}x${sep}..${sep}..${sep}victim" } 'bad path')
fCheck 'a doubled separator is refused'   (fRefused { fSafeRm "$tmp${sep}${sep}x" } 'bad path')
fCheck 'a newline is refused'             (fRefused { fSafeRm "$tmp${sep}x`ny" } 'bad path')
##	Refusals of a whole folder go to paths that aren't there, outside every
##	temp root, so a broken guard has nothing to take. The project folder is
##	moved there for its check, since the real one may itself sit under a tmp
##	folder, which makes it fair game.
$nowhere = if ($win) { 'C:\silk-saferm-nowhere' } else { '/silk-saferm-nowhere' }
$outside = "$nowhere${sep}x"
fCheck 'a tmp folder itself is refused'   (fRefused { fSafeRm "$nowhere${sep}tmp" } 'not under a tmp folder')
$script:SafeRmOwn = "$nowhere${sep}own${sep}silkterm"
fCheck 'and the project folder itself'    (fRefused { fSafeRm $script:SafeRmOwn } 'not under a tmp folder')
fCheck 'while a path in it gets as far as not there' (fRefused { fSafeRm "$($script:SafeRmOwn)${sep}x" } 'not there')
$script:SafeRmOwn = $own
fCheck 'a path under neither is refused'  (fRefused { fSafeRm $outside } 'not under a tmp folder')
fCheck 'a name ending in tmp is not a tmp folder' (fRefused { fSafeRm ($outside -replace 'nowhere', 'xtmp') } 'not under a tmp folder')
fCheck 'a path that is not there is refused' (fRefused { fSafeRm (Join-Path $tmp 'missing') } 'not there')

$link = Join-Path $tmp 'link'
fLink $link $victim
fCheck 'a link is refused'                (fRefused { fSafeRm $link } 'a link')
fCheck 'and a path through one'           (fRefused { fSafeRm (Join-Path $link 'keep.txt') } 'goes through a link')
fCheck 'and what it points at is still there' (Test-Path -LiteralPath (Join-Path $victim 'keep.txt'))

##	A tree with a read-only file, a hidden one, and a link out to the victim.
$tree = fDir (Join-Path $tmp 'tree')
$deep = fDir (Join-Path $tree "a${sep}b")
fFile (Join-Path $deep 'f.txt')
$ro = Join-Path $tree 'ro.txt'; fFile $ro
(Get-Item -LiteralPath $ro).Attributes = 'ReadOnly'
$hid = Join-Path $tree '.hidden'; fFile $hid
if ($win) { (Get-Item -LiteralPath $hid -Force).Attributes = 'Hidden' }
fLink (Join-Path $deep 'out') $victim
$r = fTry { fSafeRm $tree }
fCheck 'a tree under a tmp folder goes, read-only and hidden files too' ($r.Ok -and $r.Said -eq '' -and -not (Test-Path -LiteralPath $tree))
fCheck 'and the link in it went without being followed' ((Test-Path -LiteralPath (Join-Path $victim 'keep.txt')) -and @(Get-ChildItem -LiteralPath $victim).Count -eq 1)
$r = fTry { fSafeRm "$(fDir (Join-Path $tmp 'slash'))$sep" }
fCheck 'a trailing separator is taken' ($r.Ok -and -not (Test-Path -LiteralPath (Join-Path $tmp 'slash')))

##	The project's folder in the temp root counts, outside a tmp folder too.
$plain = fDir (Join-Path $work 'plain')
fFile (Join-Path $plain 'f.txt')
$r = fTry { fSafeRm $plain }
fCheck 'a folder in the project folder goes' ($r.Ok -and -not (Test-Path -LiteralPath $plain))

##	The one-parent form.
$parent = fDir (Join-Path $work 'parent')
$kid = fDir (Join-Path $parent "kid${sep}deeper")
$sib = fDir (Join-Path $work 'parent-other')
fFile (Join-Path $sib 'keep.txt')
fCheck 'the parent itself is refused'     (fRefused { fSafeRm_Under $parent $parent } 'not under')
fCheck 'a name that only starts the same is refused' (fRefused { fSafeRm_Under $parent $sib } 'not under')
fCheck 'no parent is refused'             (fRefused { fSafeRm_Under } 'no parent')
fCheck 'a relative parent is refused'     (fRefused { fSafeRm_Under 'parent' $kid } 'no parent')
fCheck 'a path through a link is refused there too' (fRefused { fSafeRm_Under $tmp (Join-Path $link 'keep.txt') } 'goes through a link')
$r = fTry { fSafeRm_Under $parent (Split-Path $kid) }
fCheck 'a folder under the parent goes'   ($r.Ok -and -not (Test-Path -LiteralPath (Split-Path $kid)) -and (Test-Path -LiteralPath $parent))
fCheck 'and its sibling stays'            (Test-Path -LiteralPath (Join-Path $sib 'keep.txt'))

##	The link goes first, so the cleanup cannot reach the victim through it.
if ($win) { [System.IO.Directory]::Delete($link, $false) } else { [System.IO.File]::Delete($link) }
$done = fSafeRm $work
if (-not $done -or (Test-Path -LiteralPath $work)) { Write-Warning "left $work" }

if ($failures -gt 0) { Write-Host "$failures failed"; exit 1 }
Write-Host 'all passed'
exit 0

##	History:
##		- 20261028: Created.
