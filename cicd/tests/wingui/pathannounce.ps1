##	The installer's PATH change has to reach a console opened from the Start menu.
##	What the shell starts gets the shell's own copy of the environment, and a
##	registry write on its own left that copy as it was until the next sign-in.
##
##	The check reads a marker variable rather than PATH. Windows leaves the user
##	PATH out of every new environment once the machine one is long enough, and
##	on b29w it is, so PATH could not show the difference there. The marker is
##	written straight to the registry the way the PATH is, so it shows whether
##	the shell rebuilt its copy after the install.
##	Test ID: EqH4iss

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

if (-not (fSessionUsable)) { fSkip "console session is locked - the Run box cannot be typed into" }

##	install.ps1's own functions, lifted as they stand. run.bash sends the file.
$src = Join-Path $PSScriptRoot "install.ps1"
$ast = [System.Management.Automation.Language.Parser]::ParseFile($src, [ref]$null, [ref]$null)
foreach ($name in 'fAddToWindowsPath', 'fPathKey', 'fAnnounceEnvironment', 'fInnerMessage') {
	$fn = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
	if ($fn) { . ([scriptblock]::Create($fn.Extent.Text)) }
}
$appName = 'SilkTerm'; $exeName = 'silkterm'

$dir = Join-Path $env:TEMP ("silkpath-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$marker = "SILKPATH_" + (Split-Path $dir -Leaf).Substring(9)

##	Start a probe from the Run box, which is the shell's, and bring back what it saw.
function fShellSees($n) {
	$seen = Join-Path $dir "seen$n.txt"
	$probe = Join-Path $dir "probe$n.cmd"
	Set-Content -Path $probe -Value "@set $marker> `"$seen`" 2>&1" -Encoding ASCII
	fPress "win+r"; Start-Sleep -Milliseconds 1500
	fSend $probe; fPress "enter"
	for ($i = 0; $i -lt 40 -and -not (Test-Path $seen); $i++) { Start-Sleep -Milliseconds 250 }
	if (Test-Path $seen) { Get-Content $seen -Raw } else { $null }
}

$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
$had = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
$kind = if ($null -ne $had) { $key.GetValueKind('Path') } else { $null }
try {
	$key.SetValue($marker, 'here', [Microsoft.Win32.RegistryValueKind]::String)
	$before = fShellSees 1
	[void](fCheck "the Run box started the probe" ($null -ne $before))
	[void](fCheck "a registry write alone does not reach the shell" ($before -notlike "*$marker=here*"))
	fAddToWindowsPath $dir 'User'
	Start-Sleep -Seconds 1
	$after = fShellSees 2
	[void](fCheck "after the install, what the shell starts has the environment as the registry holds it" ($after -like "*$marker=here*"))
	$now = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
	[void](fCheck "the folder is in the user PATH" ("$now".Split(';') -contains $dir))
	if ($null -ne $had) {
		[void](fCheck "the PATH keeps its value kind" ($key.GetValueKind('Path') -eq $kind))
		[void](fCheck "the PATH keeps what it had, variables unexpanded" ($now.StartsWith($had.TrimEnd(';'))))
	}
} finally {
	if ($null -ne $had) { $key.SetValue('Path', $had, $kind) } else { $key.DeleteValue('Path', $false) }
	$key.DeleteValue($marker, $false)
	$key.Close()
	if (Get-Command fAnnounceEnvironment -ErrorAction SilentlyContinue) { fAnnounceEnvironment }
	Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue
}

##	History:
##		- 20260918 JC: Created.
