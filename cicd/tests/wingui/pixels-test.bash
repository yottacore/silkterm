#!/usr/bin/env bash

##	- Purpose:
##		The Windows scenarios' ink and diff checks, without a box. They called
##		GetPixel once a sampled pixel, which took seconds a check, and now read the
##		locked bitmap a row at a time in _lib.ps1's SilkPix. Its counts are checked
##		here against a pixel-at-a-time reference, over rows stored top first and
##		bottom first, sizes that don't divide by the step, and colors at each
##		threshold. System.Drawing doesn't run here, so the bitmap is plain memory.
##	- Test ID: dfe8t
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

if ! command -v pwsh >/dev/null 2>&1; then
	echo "  skip wingui pixel checks (no pwsh)"
	exit 0
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-pixels.XXXXXX")"
trap 'rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"' EXIT

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

## fInk and fDiff themselves, by the AST, so a comment that names GetPixel doesn't count.
fNoGetPixel(){ pwsh -NoProfile -NonInteractive -Command "
	\$ast = [System.Management.Automation.Language.Parser]::ParseFile('${meDir}/_lib.ps1', [ref]\$null, [ref]\$null)
	foreach (\$name in 'fInk', 'fDiff') {
		\$fn = \$ast.Find({ param(\$n) \$n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and \$n.Name -eq \$name }, \$true)
		if (-not \$fn -or \$fn.Body.Extent.Text -match 'GetPixel') { exit 1 }
	}" ;}
fCheck "fInk and fDiff don't read a pixel a call" fNoGetPixel

cat > "${work}/pixels.ps1" <<'PS'
param([string]$Lib)
$ErrorActionPreference = 'Stop'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Lib, [ref]$null, [ref]$null)
$add = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and $n.Extent.Text -match '^Add-Type\b' -and $n.Extent.Text -match 'class SilkPix\b' }, $true)
if (-not $add) { "FAIL no SilkPix in _lib.ps1"; exit 1 }
Invoke-Expression $add.Extent.Text

$rng = [System.Random]::new(8)
$bad = 0; $cases = 0
## B G R A, rows top first, as GetPixel would hand them out one by one.
function fRef([byte[]]$P, [int]$W, [int]$H, [int]$Step, [scriptblock]$Hit) {
	$hits = 0; $seen = 0
	for ($y = 0; $y -lt $h; $y += $step) {
		for ($x = 0; $x -lt $w; $x += $step) { $seen++; if (& $Hit (($y * $w + $x) * 4)) { $hits++ } }
	}
	[math]::Round($hits / $seen, 4)
}
## The pixels into unmanaged memory with a stride of its own, padded, and bottom
## row first when $Up. Returns the pointer for row 0 and the stride.
function fPlace([byte[]]$P, [int]$W, [int]$H, [bool]$Up) {
	$row = $w * 4; $stride = $row + 8
	$mem = [System.Runtime.InteropServices.Marshal]::AllocHGlobal($stride * $h)
	$script:mems += $mem
	for ($y = 0; $y -lt $h; $y++) {
		$at = if ($up) { $h - 1 - $y } else { $y }
		[System.Runtime.InteropServices.Marshal]::Copy($p, $y * $row, [IntPtr]::Add($mem, $at * $stride), $row)
	}
	if ($up) { @([IntPtr]::Add($mem, ($h - 1) * $stride), -$stride) } else { @($mem, $stride) }
}
$script:mems = @()
function fSame([string]$What, $Want, $Got) {
	$script:cases++
	if ($want -ne $got) { $script:bad++; "FAIL ${What}: want $want got $got" }
}

## Dim pixels, so R+G+B lands either side of the ink line, alpha anything.
function fPixels([int]$W, [int]$H, [int]$Max) {
	$p = New-Object byte[] ($w * $h * 4)
	for ($i = 0; $i -lt $p.Length; $i++) { $p[$i] = if (($i % 4) -eq 3) { $rng.Next(0, 256) } else { $rng.Next(0, $max) } }
	,$p
}
foreach ($size in @(@(1, 1), @(7, 5), @(13, 11), @(64, 37))) {
	$w, $h = $size
	$a = fPixels $w $h 28
	$b = [byte[]]$a.Clone()
	for ($i = 0; $i -lt $b.Length; $i++) { if (($i % 4) -ne 3) { $b[$i] = [byte](($b[$i] + $rng.Next(0, 13)) % 256) } else { $b[$i] = $rng.Next(0, 256) } }
	foreach ($up in $false, $true) {
		$pa = fPlace $a $w $h $up; $pb = fPlace $b $w $h (-not $up)
		foreach ($step in 1..7) {
			$inkHit = { param($i) $a[$i] + $a[$i + 1] + $a[$i + 2] -gt 40 }
			$diffHit = { param($i) [math]::Abs($a[$i] - $b[$i]) + [math]::Abs($a[$i + 1] - $b[$i + 1]) + [math]::Abs($a[$i + 2] - $b[$i + 2]) -gt 24 }
			fSame "ink ${w}x${h} up=$up step $step" (fRef $a $w $h $step $inkHit) ([SilkPix]::Ink($pa[0], $pa[1], $w, $h, $step))
			fSame "diff ${w}x${h} up=$up step $step" (fRef $a $w $h $step $diffHit) ([SilkPix]::Diff($pa[0], $pa[1], $pb[0], $pb[1], $w, $h, $step))
		}
	}
}

## The lines themselves: 40 is dark and 41 lit, alpha aside; a move of 24 is
## still and 25 moved, counted across channels.
$edge = [byte[]](40, 0, 0, 255,  14, 14, 13, 0,  0, 0, 41, 0,  0, 0, 0, 255)
$p = fPlace $edge 4 1 $false
fSame "ink counts R+G+B over 40, not alpha" 0.5 ([SilkPix]::Ink($p[0], $p[1], 4, 1, 1))
$was  = [byte[]](0, 0, 0, 255,  10, 10, 10, 255,  200, 0, 0, 0,  0, 0, 0, 0)
$now  = [byte[]](8, 8, 8, 0,    18, 18, 19, 255,  175, 0, 0, 0,  0, 0, 0, 255)
$p1 = fPlace $was 4 1 $false; $p2 = fPlace $now 4 1 $false
fSame "diff counts a move over 24, either way, not alpha" 0.5 ([SilkPix]::Diff($p1[0], $p1[1], $p2[0], $p2[1], 4, 1, 1))

foreach ($m in $script:mems) { [System.Runtime.InteropServices.Marshal]::FreeHGlobal($m) }
"cases $cases, failed $bad"
if ($bad) { exit 1 }
PS

rc=0
out="$(pwsh -NoProfile -NonInteractive -File "${work}/pixels.ps1" -Lib "${meDir}/_lib.ps1" 2>&1)" || rc=$?
printf '%s\n' "${out}" | sed 's/^/  /'
fCheck "SilkPix counts the same as one pixel at a time" test "${rc}" -eq 0

((failures == 0)) || { echo "${failures} check(s) failed"; exit 1; }
echo "all passed"

##	Script history:
##		- 20261117: Created.
