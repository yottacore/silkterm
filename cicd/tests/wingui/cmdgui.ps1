##	Copyright (C) 2026 Jim Collier
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A tab stops naming a program started from cmd once it ends.
.DESCRIPTION
	cmd doesn't wait for a program with no console, so its prompt is back
	while the program still runs, and nothing is printed when it ends. The
	tab kept the program's name until the next key. A small program built
	here stands in for one, asleep for 3 seconds with no window. PATH goes
	without a conpty.dll, so even an older build gets the console host
	Windows has.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EsgiFgO


if (-not (fSessionUsable)) { fSkip "console session is locked - nothing can be typed" }

Add-Type -Namespace SilkCmdGui -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
'@

function fTitle([IntPtr]$H) {
	$sb = New-Object System.Text.StringBuilder 512
	[void][SilkCmdGui.Win]::GetWindowTextW($h, $sb, 512)
	$sb.ToString()
}

$dir = Join-Path $OutDir "cmdgui"
Remove-Item $dir -Recurse -ErrorAction SilentlyContinue
[void](New-Item -ItemType Directory -Path $dir -Force)
##	csc ships with the .NET Framework that every Windows 10 and 11 has.
$csc = Join-Path $env:WINDIR "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path $csc)) { fSkip "no csc.exe to build the stand-in with" }
$src = Join-Path $dir "nap.cs"
$nap = Join-Path $dir "nap.exe"
Set-Content -Path $src -Encoding ascii -Value "class Nap { static void Main() { System.Threading.Thread.Sleep(3000); } }"
& $csc /nologo /target:winexe "/out:$nap" $src | Out-Null
if (-not (fCheck "the stand-in built" (Test-Path $nap))) { return }

$cfg = Join-Path $dir "config.shcl"
fFreshConfig $cfg @("cursor:", "`tblink: true", "window:", "`tremember_per_monitor: false")
$inbox = ($env:PATH -split ';' | Where-Object { $_ -and -not (Test-Path (Join-Path $_ "conpty.dll")) }) -join ';'
$p = fStartSilk -Exe $Exe -SilkArgs @("--config=`"$cfg`"", "--shell", "cmd.exe") -EnvVars @{ PATH = $inbox }
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }
$work = fWorkArea
[void][SilkCmdGui.Win]::MoveWindow($h, $work.x, $work.y, 1000, 560, $true)
Start-Sleep -Seconds 4
[void](fFocus $h)
fSendChars "`"$nap`""
fPress "enter"
Start-Sleep -Milliseconds 1500
$during = fTitle $h
$naps = @(Get-Process -Name nap -ErrorAction SilentlyContinue)
fNote "while it runs: '$during', $($naps.Count) running"
[void](fCheck "the tab names it while it runs" ($during -match '\[nap\]'))
foreach ($n in $naps) { [void]$n.WaitForExit(10000) }
[void](fCheck "it ended" (@(Get-Process -Name nap -ErrorAction SilentlyContinue).Count -eq 0))
Start-Sleep -Seconds 2
$after = fTitle $h
[void](fShot $h "cmdgui-after")
fNote "after: '$after'"
[void](fCheck "and stops once it ends, with nothing typed" ($after -notmatch '\[nap\]'))
##	A key makes the shell print, which was all it ever waited for.
[void](fFocus $h)
fSendChars " "
Start-Sleep -Seconds 1
$keyed = fTitle $h
fNote "after a key: '$keyed'"
[void](fCheck "a key clears it" ($keyed -notmatch '\[nap\]'))
fStop $p

##	History:
##		- 20261014 JC: Created.
