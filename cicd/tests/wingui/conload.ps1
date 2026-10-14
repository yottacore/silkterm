##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	No DLL that Windows lacks is taken from PATH or the working folder.
.DESCRIPTION
	conpty.dll isn't part of Windows, so a load by bare name went on to the
	working folder and PATH, and took whatever was there. WezTerm's copy on
	PATH replaced the console host Windows has. A harmless system DLL copied
	under that name goes in a folder put first on PATH, then in the working
	folder, and the window must load from neither. Any folder already on PATH
	with a conpty.dll in it counts as well. A plant that fails to load is let
	go at once, so dxcompiler.dll can't be checked this way. The unit test
	in dllsearch.rs covers any load by name.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EshQpKD


$dir = Join-Path $OutDir "conload"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
$onPath = Join-Path $dir "onpath"
$here = Join-Path $dir "here"
$elsewhere = Join-Path $dir "elsewhere"
foreach ($d in @($onPath, $here, $elsewhere)) { [void](New-Item -ItemType Directory -Path $d -Force) }

$stand = Join-Path $env:SystemRoot "System32\version.dll"
foreach ($d in @($onPath, $here)) { Copy-Item $stand (Join-Path $d "conpty.dll") }

$others = @($env:PATH -split ';' | Where-Object { $_ -and (Test-Path (Join-Path $_ "conpty.dll")) })
foreach ($o in $others) { fNote "already on PATH: $o" }
$banned = @($onPath, $here) + $others | ForEach-Object { [IO.Path]::GetFullPath($_).TrimEnd('\') }

function fRun([string]$Label, [string]$WorkDir, [string]$PathVar) {
	$cfg = Join-Path $dir "$Label.shcl"
	fFreshConfig $cfg @("cursor:", "`tblink: false")
	$p = fStartSilk -Exe $Exe -SilkArgs @("--config=`"$cfg`"", "--shell", "cmd.exe") -EnvVars @{ PATH = $pathVar } -WorkDir $workDir
	$h = fWaitWindow $p 40
	if (-not (fCheck "$($Label): a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
	##	the pane's console host is made once the window has its size
	Start-Sleep -Seconds 4
	$p.Refresh()
	$mods = @($p.Modules | ForEach-Object { $_.FileName })
	fStop $p
	if (-not (fCheck "$($Label): its modules could be read" ($mods.Count -gt 0))) { return }
	$got = @($mods | Where-Object { (Split-Path $_ -Leaf) -eq "conpty.dll" })
	fNote "$($Label): conpty.dll from $(if ($got) { $got -join ', ' } else { 'nowhere, so the one in Windows' })"
	$bad = @($mods | Where-Object { $banned -contains (Split-Path ([IO.Path]::GetFullPath($_)) -Parent).TrimEnd('\') })
	[void](fCheck "$($Label): nothing loaded from PATH or the working folder" ($bad.Count -eq 0))
}

##	the box as it stands, which is how WezTerm's copy got in
fRun -Label "box" -WorkDir $elsewhere -PathVar $env:PATH
fRun -Label "onpath" -WorkDir $elsewhere -PathVar "$onPath;$env:PATH"
fRun -Label "here" -WorkDir $here -PathVar $env:PATH

##	History:
##		- 20261014 JC: Created.
