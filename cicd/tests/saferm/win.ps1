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

##	Test ID: dehyk

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot '../../utility/include/saferm.ps1')

$failures = 0
function fCheck { param([string]$What, [bool]$Ok)
	if ($Ok) { Write-Host "  ok   $What" } else { Write-Host "  FAIL $What"; $script:failures++ }
}

##	What the guard said on stderr, and what it returned.
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

##	HOST_TEMP itself and a whole project folder in it, here under a tmp folder,
##	where the tmp rule alone would take them. Real folders, since a path that
##	isn't there gets refused anyway.
$ht = fDir (Join-Path $tmp 'ht')
foreach ($d in 'silkterm', 'other', 'loose', 'sock') { $null = fDir (Join-Path $ht "$d${sep}r1") }
function fHt([string]$Name) { Join-Path $ht $Name }
$whole = 'a whole project temp folder'
$saved = $script:SafeRmHost, $script:SafeRmOwn, $script:SafeRmHostSet
$script:SafeRmHost = $ht; $script:SafeRmOwn = fHt 'silkterm'; $script:SafeRmHostSet = $true
fCheck 'HOST_TEMP itself is refused'      (fRefused { fSafeRm $ht } $whole)
fCheck 'so is this project''s folder in it' (fRefused { fSafeRm (fHt 'silkterm') } $whole)
fCheck 'and another project''s'           (fRefused { fSafeRm (fHt 'other') } $whole)
fCheck 'with a trailing separator too'    (fRefused { fSafeRm "$(fHt 'other')$sep" } $whole)
if ($win) { fCheck 'and in another case' (fRefused { fSafeRm (fHt 'OTHER') } $whole) }
fCheck 'a run folder in another project''s folder goes' (fTry { fSafeRm (fHt "other${sep}r1") }).Ok
fCheck 'and one in this project''s'       (fTry { fSafeRm (fHt "silkterm${sep}r1") }).Ok
fCheck 'and only those' ((Test-Path -LiteralPath (fHt 'other')) -and (Test-Path -LiteralPath (fHt 'silkterm')) -and -not (Test-Path -LiteralPath (fHt "other${sep}r1")))
fCheck 'the one-parent form won''t take HOST_TEMP' (fRefused { fSafeRm_Under $tmp $ht } $whole)
fCheck 'or this project''s folder'        (fRefused { fSafeRm_Under $ht (fHt 'silkterm') } $whole)
fCheck 'but takes a folder made right in it, as the test scripts make theirs' (fTry { fSafeRm_Under $ht (fHt 'loose') }).Ok
$script:SafeRmHostSet = $false
fCheck 'with no HOST_TEMP, the fallback itself is refused' (fRefused { fSafeRm $ht } $whole)
fCheck 'and this project''s folder in it' (fRefused { fSafeRm (fHt 'silkterm') } $whole)
fCheck 'while a folder right in it goes'  (fTry { fSafeRm (fHt 'sock') }).Ok
##	And as the guard reads it at load, from the environment.
$envWas = $env:HOST_TEMP
try {
	$env:HOST_TEMP = "$ht$sep"
	. (Join-Path $PSScriptRoot '../../utility/include/saferm.ps1')
	fCheck 'HOST_TEMP from the environment counts' (fRefused { fSafeRm (fHt 'other') } $whole)
} finally { $env:HOST_TEMP = $envWas }
$script:SafeRmHost, $script:SafeRmOwn, $script:SafeRmHostSet = $saved
fCheck 'and HOST_TEMP''s folders are still there' ((Test-Path -LiteralPath (fHt 'other')) -and (Test-Path -LiteralPath (fHt 'silkterm')) -and -not (Test-Path -LiteralPath (fHt 'sock')))

##	The one-parent form, here and in install.ps1's own copy. $Who names the copy.
function fOneParent { param([string]$Who)
	$base = fDir (Join-Path $work ('one-' + [System.IO.Path]::GetRandomFileName()))
	$parent = fDir (Join-Path $base 'parent')
	$kid = fDir (Join-Path $parent "kid${sep}deeper")
	$sib = fDir (Join-Path $base 'parent-other')
	fFile (Join-Path $sib 'keep.txt')
	$inLink = Join-Path $parent 'link'
	fLink $inLink $victim
	fCheck "${Who}the parent itself is refused"     (fRefused { fSafeRm_Under $parent $parent } 'not under')
	fCheck "${Who}a name that only starts the same is refused" (fRefused { fSafeRm_Under $parent $sib } 'not under')
	fCheck "${Who}no parent is refused"             (fRefused { fSafeRm_Under } 'no parent')
	fCheck "${Who}a relative parent is refused"     (fRefused { fSafeRm_Under 'parent' $kid } 'no parent')
	fCheck "${Who}no path is refused"               (fRefused { fSafeRm_Under $parent } 'bad path')
	fCheck "${Who}two paths are refused"            (fRefused { $three = @($parent, $kid, $kid); fSafeRm_Under @three } 'bad path')
	fCheck "${Who}a relative path is refused"       (fRefused { fSafeRm_Under $parent 'kid' } 'bad path')
	fCheck "${Who}a path with .. is refused"        (fRefused { fSafeRm_Under $parent "$parent${sep}kid${sep}..${sep}..${sep}parent-other" } 'bad path')
	fCheck "${Who}a doubled separator is refused"   (fRefused { fSafeRm_Under $parent "$parent${sep}${sep}kid" } 'bad path')
	fCheck "${Who}a newline is refused"             (fRefused { fSafeRm_Under $parent "$parent${sep}kid`nx" } 'bad path')
	fCheck "${Who}a path that is not there is refused" (fRefused { fSafeRm_Under $parent (Join-Path $parent 'missing') } 'not there')
	fCheck "${Who}a link is refused"                (fRefused { fSafeRm_Under $parent $inLink } 'a link')
	fCheck "${Who}a path through a link is refused there too" (fRefused { fSafeRm_Under $parent (Join-Path $inLink 'keep.txt') } 'goes through a link')
	if ($win) { [System.IO.Directory]::Delete($inLink, $false) } else { [System.IO.File]::Delete($inLink) }
	##	A tree with a read-only file, a hidden one, and a link out to the victim.
	$tree = fDir (Join-Path $parent 'tree')
	fFile (Join-Path (fDir (Join-Path $tree "a${sep}b")) 'f.txt')
	$ro = Join-Path $tree 'ro.txt'; fFile $ro
	(Get-Item -LiteralPath $ro).Attributes = 'ReadOnly'
	$hid = Join-Path $tree '.hidden'; fFile $hid
	if ($win) { (Get-Item -LiteralPath $hid -Force).Attributes = 'Hidden' }
	fLink (Join-Path $tree "a${sep}out") $victim
	$r = fTry { fSafeRm_Under $parent $tree }
	fCheck "${Who}a tree under the parent goes, read-only and hidden files too" ($r.Ok -and $r.Said -eq '' -and -not (Test-Path -LiteralPath $tree))
	fCheck "${Who}and the link in it went without being followed" ((Test-Path -LiteralPath (Join-Path $victim 'keep.txt')) -and @(Get-ChildItem -LiteralPath $victim).Count -eq 1)
	$r = fTry { fSafeRm_Under $parent "$(Split-Path $kid)$sep" }
	fCheck "${Who}a folder under the parent goes, a trailing separator too" ($r.Ok -and -not (Test-Path -LiteralPath (Split-Path $kid)) -and (Test-Path -LiteralPath $parent))
	fCheck "${Who}and its sibling stays"            (Test-Path -LiteralPath (Join-Path $sib 'keep.txt'))
	return $base
}
$base = fOneParent 'one parent: '
$behind = Join-Path $tmp 'behind'
fLink $behind $base
$gone = fDir (Join-Path $base "parent${sep}gone")
fCheck 'a parent behind a link is refused' (fRefused { fSafeRm_Under (Join-Path $behind 'parent') (Join-Path $behind "parent${sep}gone") } 'goes through a link')

##	install.ps1 runs alone, by irm | iex too, so it has a copy, on 5.1 as
##	well. The temp folder the system gives it can be behind a link, as macOS's
##	/var does, so that copy looks for links only below the parent.
$installer = Join-Path $PSScriptRoot '../../../install.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($installer, [ref]$null, [ref]$null)
$def = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'fSafeRm_Under' }, $false)
fCheck 'install.ps1 has its own copy' ($null -ne $def)
if ($def) {
	& {
		$onWindows = $win
		. ([scriptblock]::Create($def.Extent.Text))
		$null = fOneParent 'install.ps1: '
		$r = fTry { fSafeRm_Under (Join-Path $behind 'parent') (Join-Path $behind "parent${sep}gone") }
		fCheck 'install.ps1: but takes it' ($r.Ok -and -not (Test-Path -LiteralPath $gone))
	}
}
if ($win) { [System.IO.Directory]::Delete($behind, $false) } else { [System.IO.File]::Delete($behind) }

##	The link goes first, so the cleanup can't reach the victim through it.
if ($win) { [System.IO.Directory]::Delete($link, $false) } else { [System.IO.File]::Delete($link) }
$done = fSafeRm $work
if (-not $done -or (Test-Path -LiteralPath $work)) { Write-Warning "left $work" }

if ($failures -gt 0) { Write-Host "$failures failed"; exit 1 }
Write-Host 'all passed'
exit 0

##	History:
##		- 20261028: Created.
##		- 20261031: install.ps1's own copy takes the one-parent cases too.
##		- 20261031: HOST_TEMP itself and a whole project folder in it.
