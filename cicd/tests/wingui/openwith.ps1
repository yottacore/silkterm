##	A file opened the way Explorer opens one runs in SilkTerm once the per-user
##	association names it. A batch file sits in a folder with a space in its
##	name and gets an argument, and has to start beside itself. A VBScript has to
##	go through the console host. A folder's menu entry has to hand the folder
##	over as %V.
##
##	The keys are written here the way Register writes them, plus a config of
##	its own, and removed again whatever happens. Only keys this account did not
##	already have are touched.
##	Test ID: ErNJ7tr

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

if (-not (fSessionUsable)) { fSkip "console session is locked - nothing opened would be seen" }

$classes = "HKCU:\Software\Classes"
$made = @()
foreach ($k in @("batfile", "VBSFile", "Directory", "Directory\shell", "Directory\shell\SilkTerm")) {
	if (-not (Test-Path "$classes\$k")) { $made += $k }
}
if ($made -notcontains "batfile" -or $made -notcontains "VBSFile" -or $made -notcontains "Directory\shell\SilkTerm") {
	fSkip "this account has per-user entries of its own for these types"
}

$cfg = Join-Path $OutDir "openwith-config.shcl"
fFreshConfig $cfg
$dir = Join-Path $OutDir "has space"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$bat = Join-Path $dir "go.bat"
Set-Content -Path $bat -Encoding ASCII -Value @(
	'@echo off',
	'echo arg=%~1> "%~dp0bat.txt"',
	'echo cwd=%CD%>> "%~dp0bat.txt"'
)
$vbs = Join-Path $dir "go.vbs"
Set-Content -Path $vbs -Encoding ASCII -Value @(
	'Set fso = CreateObject("Scripting.FileSystemObject")',
	'Set f = fso.CreateTextFile(fso.GetParentFolderName(WScript.ScriptFullName) & "\vbs.txt", True)',
	'f.WriteLine "arg=" & WScript.Arguments(0)',
	'f.WriteLine "host=" & WScript.FullName',
	'f.Close'
)
$proofs = @{ bat = (Join-Path $dir "bat.txt"); vbs = (Join-Path $dir "vbs.txt"); folder = (Join-Path $dir "dir.txt") }
foreach ($p in $proofs.Values) { Remove-Item $p -ErrorAction SilentlyContinue }

function fSetDefault($key, $value) {
	New-Item -Path "$classes\$key" -Force | Out-Null
	Set-ItemProperty -Path "$classes\$key" -Name "(default)" -Value $value
}

try {
	$open = "`"$Exe`" --config=`"$cfg`" --keep-open --open `"%1`" %*"
	fSetDefault "batfile\shell\open\command" $open
	fSetDefault "VBSFile\shell\open\command" $open
	fSetDefault "Directory\shell\SilkTerm" "Open in SilkTerm"
	##	The real entry is only --directory. The shell here writes where it
	##	started, which is the proof that %V arrived whole.
	fSetDefault "Directory\shell\SilkTerm\command" "`"$Exe`" --config=`"$cfg`" --directory `"%V`" --keep-open --shell `"cmd.exe /c cd > dir.txt`""

	Start-Process -FilePath $bat -ArgumentList '"two words"'
	Start-Process -FilePath $vbs -ArgumentList '"vb arg"'
	Start-Process -FilePath $dir -Verb SilkTerm

	for ($i = 0; $i -lt 160; $i++) {
		if (@($proofs.Values | Where-Object { -not (Test-Path $_) }).Count -eq 0) { break }
		Start-Sleep -Milliseconds 250
	}
	Start-Sleep -Milliseconds 500

	$ours = @(Get-CimInstance Win32_Process -Filter "Name = 'silkterm.exe'" | Where-Object { $_.CommandLine -like "*$cfg*" })
	foreach ($c in $ours) {
		$p = Get-Process -Id $c.ProcessId -ErrorAction SilentlyContinue
		if ($p) { fTrack $p }
		fNote "ran: $($c.CommandLine)"
	}
	$read = { param($f) if (Test-Path $f) { (Get-Content $f -Raw).Trim() } else { "" } }
	$said = & $read $proofs.bat
	fNote "bat: $($said -replace "`r?`n", ' | ')"
	[void](fCheck "a batch file opened from the shell ran in SilkTerm" (@($ours | Where-Object { $_.CommandLine -like "*go.bat*" }).Count -eq 1))
	[void](fCheck "it got its argument whole" ($said -match "arg=two words"))
	[void](fCheck "it started in its own folder" ($said -match [regex]::Escape("cwd=$dir")))
	$said = & $read $proofs.vbs
	fNote "vbs: $($said -replace "`r?`n", ' | ')"
	[void](fCheck "a VBScript opened from the shell ran in SilkTerm" (@($ours | Where-Object { $_.CommandLine -like "*go.vbs*" }).Count -eq 1))
	[void](fCheck "it got its argument, through the console host" ($said -match "arg=vb arg" -and $said -match "cscript\.exe"))
	$said = & $read $proofs.folder
	fNote "folder: $said"
	[void](fCheck "the folder entry opened SilkTerm in that folder" ($said -eq $dir))
}
finally {
	foreach ($k in ($made | Sort-Object Length)) {
		if (Test-Path "$classes\$k") { Remove-Item "$classes\$k" -Recurse -Force }
	}
}

##	History:
##		- 20260930 JC: Created.
