##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

<#
.SYNOPSIS
	A settings file with a line that cannot be read cannot be saved either, and
	on Windows the only word of it went to a console a release build does not
	have.
.DESCRIPTION
	Now a standard message box says so, a few seconds after launch, when the
	shell scan's save is refused.
.NOTES
	History: At bottom of file.
#>

##	Test ID: EqH8w4O


if (-not (fSessionUsable)) { fSkip "console session is locked - the box cannot be grabbed" }

Add-Type -Namespace SilkBox -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
'@
function fText([IntPtr]$H) { $sb = New-Object System.Text.StringBuilder 2048; [void][SilkBox.Win]::GetWindowTextW($h, $sb, 2048); $sb.ToString() }
function fClass([IntPtr]$H) { $sb = New-Object System.Text.StringBuilder 256; [void][SilkBox.Win]::GetClassNameW($h, $sb, 256); $sb.ToString() }

$cfg = Join-Path $OutDir "savenotice-config.shcl"
##	The margin line steps back to a depth nothing uses, so the reader cannot place
##	it. shcl writes such a line back as it was, and saves go through beside it,
##	unless the save falls back to the canonical form. A shells block written twice
##	makes the shell scan's save do that.
fFreshConfig $cfg @("window:", "`t`topacity: 1.0", "`tmargin: 4", "shells:", "`tcmd:", "`t`tcommand: cmd.exe", "shells:", "`tpwsh:", "`t`tcommand: pwsh.exe")

$p = fStartSilk -Exe $Exe -SilkArgs @("--config=$cfg", "--columns", "100", "--rows", "30") -EnvVars @{}
$h = fWaitWindow $p 40
if (-not (fCheck "a window came up" ($h -ne [IntPtr]::Zero))) { fStop $p; return }

$box = [IntPtr]::Zero
for ($i = 0; $i -lt 120 -and $box -eq [IntPtr]::Zero; $i++) {
	foreach ($w in [SilkEnum]::All([uint32]$p.Id)) { if ((fClass $w) -eq '#32770') { $box = $w } }
	if ($box -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 250 }
}
if (fCheck "a message box came up for the refused save" ($box -ne [IntPtr]::Zero)) {
	$title = fText $box
	$body = fText ([SilkBox.Win]::GetDlgItem($box, 0xFFFF))
	fNote "box says: $title / $($body -replace '\s+', ' ')"
	$onDisk = @(Get-Content $cfg)
	$at = 1 + [array]::FindIndex([string[]]$onDisk, [Predicate[string]] { param($l) $l -eq "`tmargin: 4" })
	fNote "the file has $($onDisk.Count) lines, the bad one at $at"
	[void](fCheck "it is titled as the notice" ($title -eq 'Settings not saved'))
	##	The launch has filled in the missing settings by then, so the bad line has moved down.
	[void](fCheck "it names the file and the line as the file has it" ($body.Contains($cfg) -and $body -like "*Line $at cannot be read*"))
	[void](fShot $box "notice")
	[void](fCheck "the settings file was left as it was" (@(Get-Content $cfg) -contains "`tmargin: 4"))
}
fStop $p

##	History:
##		- 20260918 JC: Created.
##		- 20261006 JC: Help block, typed parameters, named arguments.
