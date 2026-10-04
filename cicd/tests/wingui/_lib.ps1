##	Helpers for a scenario running inside the console session. Dot-sourced by
##	_run.ps1, which has already checked that there is a desktop to draw on.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace Silk -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
[DllImport("user32.dll")] public static extern IntPtr OpenInputDesktop(uint f, bool i, uint a);
[DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr h);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool GetUserObjectInformation(IntPtr h, int i, System.Text.StringBuilder p, int n, out uint need);
[DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
[DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern IntPtr GetFocus();
[DllImport("user32.dll")] public static extern bool SystemParametersInfoW(uint a, uint b, out RECT r, uint c);
[DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
[DllImport("wtsapi32.dll")] public static extern bool WTSQuerySessionInformationW(IntPtr srv, int id, int cls, out IntPtr buf, out int bytes);
[DllImport("wtsapi32.dll")] public static extern void WTSFreeMemory(IntPtr p);
public struct RECT { public int Left, Top, Right, Bottom; }
'@

##	MainWindowHandle answers with whatever the process registered first, which for
##	this app is a 16x16 helper window - so the real one has to be picked out by
##	size from everything the process owns.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class SilkEnum {
	delegate bool EnumProc(IntPtr h, IntPtr p);
	[DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
	[DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
	[DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
	[DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
	struct R { public int L, T, Rt, B; }
	public static IntPtr[] All(uint want) {
		var found = new List<IntPtr>();
		EnumWindows((h, _) => {
			uint pid; GetWindowThreadProcessId(h, out pid);
			if (pid == want && IsWindowVisible(h)) found.Add(h);
			return true;
		}, IntPtr.Zero);
		return found.ToArray();
	}
	public static IntPtr Largest(uint want) {
		IntPtr best = IntPtr.Zero; long area = 0;
		EnumWindows((h, _) => {
			uint pid; GetWindowThreadProcessId(h, out pid);
			if (pid != want || !IsWindowVisible(h)) return true;
			R r; if (!GetWindowRect(h, out r)) return true;
			long a = (long)(r.Rt - r.L) * (r.B - r.T);
			if (a > area) { area = a; best = h; }
			return true;
		}, IntPtr.Zero);
		return best;
	}
}
'@

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SilkKeys {
	[StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
	[StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
	[StructLayout(LayoutKind.Explicit)] struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
	[StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public UNION u; }
	[DllImport("user32.dll", SetLastError=true)] static extern uint SendInput(uint n, INPUT[] p, int cb);
	[DllImport("user32.dll")] public static extern short VkKeyScanW(char c);
	const uint KEYUP = 0x0002, UNICODE = 0x0004;

	static void Send(INPUT[] a) { SendInput((uint)a.Length, a, Marshal.SizeOf(typeof(INPUT))); }
	static INPUT Vk(ushort vk, bool up) {
		var i = new INPUT(); i.type = 1;
		i.u.ki.vk = vk; i.u.ki.flags = up ? KEYUP : 0;
		return i;
	}
	static INPUT Uni(char c, bool up) {
		var i = new INPUT(); i.type = 1;
		i.u.ki.scan = c; i.u.ki.flags = UNICODE | (up ? KEYUP : 0);
		return i;
	}
	public static void Tap(ushort vk) { Send(new[] { Vk(vk, false), Vk(vk, true) }); }
	public static void Chord(ushort[] mods, ushort vk) {
		var a = new INPUT[mods.Length * 2 + 2];
		int n = 0;
		foreach (var m in mods) a[n++] = Vk(m, false);
		a[n++] = Vk(vk, false); a[n++] = Vk(vk, true);
		for (int j = mods.Length - 1; j >= 0; j--) a[n++] = Vk(mods[j], true);
		Send(a);
	}
	// Unicode injection arrives as VK_PACKET, which not every window reads. Typing
	// through the layout produces the same messages a keyboard does.
	public static void Text(string s) {
		foreach (char c in s) {
			short m = VkKeyScanW(c);
			if (m == -1) { Send(new[] { Uni(c, false), Uni(c, true) }); continue; }
			ushort vk = (ushort)(m & 0xFF);
			int st = (m >> 8) & 0xFF;
			var a = new System.Collections.Generic.List<INPUT>();
			if ((st & 1) != 0) a.Add(Vk(0x10, false));
			if ((st & 2) != 0) a.Add(Vk(0x11, false));
			if ((st & 4) != 0) a.Add(Vk(0x12, false));
			a.Add(Vk(vk, false)); a.Add(Vk(vk, true));
			if ((st & 4) != 0) a.Add(Vk(0x12, true));
			if ((st & 2) != 0) a.Add(Vk(0x11, true));
			if ((st & 1) != 0) a.Add(Vk(0x10, true));
			Send(a.ToArray());
		}
	}
	public static void TextUnicode(string s) {
		foreach (char c in s) Send(new[] { Uni(c, false), Uni(c, true) });
	}
}
'@

$script:checks   = @()
$script:failures = 0
$script:shotDir  = $null

function fCheck($what, $ok) {
	if ($ok) { $script:checks += "  ok   $what" }
	else     { $script:checks += "  FAIL $what"; $script:failures++ }
	$ok
}

function fNote($text) { $script:checks += "  note $text" }

##	Whether anyone could actually see or type into this desktop. A locked session
##	still runs windows and still answers PrintWindow, but screen grabs come back
##	black and injected input goes to the lock screen - so a scenario that needs
##	either must stop rather than quietly measure nothing.
##	Windows keeps the answer itself, so ask for it first. Reading it off the lock
##	screen went wrong: once the display sleeps, a locked session has nothing in
##	front and LockApp sits suspended, which is also how it lingers after an unlock.
##	The other two tests stay for a build that does not report the state. The
##	desktop name catches the old style of lock, which hands input to Winlogon.
function fSessionUsable {
	if ((fLockState) -eq "locked") { return $false }
	$fg = [Silk.Win]::GetForegroundWindow()
	if ($fg -ne [IntPtr]::Zero) {
		$owner = 0
		[void][Silk.Win]::GetWindowThreadProcessId($fg, [ref]$owner)
		$name = (Get-Process -Id $owner -ErrorAction SilentlyContinue).ProcessName
		if ($name -in @("LockApp", "LogonUI")) { return $false }
	}
	$d = [Silk.Win]::OpenInputDesktop(0, $false, 0x0100)
	if ($d -eq [IntPtr]::Zero) { return $false }
	$sb = New-Object System.Text.StringBuilder 256
	$n = 0
	$got = [Silk.Win]::GetUserObjectInformation($d, 2, $sb, 256, [ref]$n)
	[void][Silk.Win]::CloseDesktop($d)
	$got -and $sb.ToString() -eq "Default"
}

##	WTSSessionInfoEx for this session. The level-1 record starts at offset 8, and
##	SessionFlags is its third field: 0 locked, 1 unlocked.
function fLockState {
	$id = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
	$buf = [IntPtr]::Zero; $n = 0
	if (-not [Silk.Win]::WTSQuerySessionInformationW([IntPtr]::Zero, $id, 25, [ref]$buf, [ref]$n)) { return "unknown" }
	$level = [Runtime.InteropServices.Marshal]::ReadInt32($buf, 0)
	$flags = [Runtime.InteropServices.Marshal]::ReadInt32($buf, 16)
	[Silk.Win]::WTSFreeMemory($buf)
	if ($level -ne 1) { return "unknown" }
	switch ($flags) { 0 { "locked" } 1 { "unlocked" } default { "unknown" } }
}

##	A config to start from. Rating the hardware swallows input for several seconds
##	after launch, so anything that types has to switch it off or wait it out - and
##	waiting it out makes the scenario slow and its timing a guess. Only the ladder
##	scenario wants the rating.
function fFreshConfig($path, $extra = @()) {
	Remove-Item $path -ErrorAction SilentlyContinue
	$body = @("performance:", "`tautomatic: false", "`tprofile: `"custom`"") + $extra
	Set-Content -Path $path -Value $body -Encoding UTF8
}

function fStartSilk($exe, $silkArgs, $envVars) {
	foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $envVars[$k]) }
	$p = Start-Process $exe -ArgumentList $silkArgs -PassThru
	foreach ($k in $envVars.Keys) { [Environment]::SetEnvironmentVariable($k, $null) }
	fTrack $p
	$p
}

##	What _stop.ps1 may end, and nothing else.
function fTrack($p) {
	if ($script:startedList) { Add-Content -Path $script:startedList -Value "$($p.Id) $($p.StartTime.ToUniversalTime().Ticks)" }
}

function fWaitWindow($p, $seconds = 30) {
	for ($i = 0; $i -lt ($seconds * 4); $i++) {
		if ($p.HasExited) { return [IntPtr]::Zero }
		$h = [SilkEnum]::Largest([uint32]$p.Id)
		if ($h -ne [IntPtr]::Zero) {
			$r = fRect $h
			if ($r.w -gt 200 -and $r.h -gt 100) { return $h }
		}
		Start-Sleep -Milliseconds 250
	}
	[IntPtr]::Zero
}

##	A window the process owns that is not the one already known - the dialog is a
##	second window in the same process, so it cannot be found by pid alone.
function fWaitOther($p, $known, $seconds = 20) {
	for ($i = 0; $i -lt ($seconds * 4); $i++) {
		foreach ($h in [SilkEnum]::All([uint32]$p.Id)) {
			if ($h -eq $known) { continue }
			$r = fRect $h
			if ($r.w -gt 200 -and $r.h -gt 200) { return $h }
		}
		Start-Sleep -Milliseconds 250
	}
	[IntPtr]::Zero
}

##	Windows only hands the foreground to a process that owns it already or that
##	received the last input. On a session just reconnected to the console nobody
##	owns either, so a bare SetForegroundWindow is refused - press a harmless key
##	first, and borrow the current owner's input queue.
function fFocus($h) {
	$escaped = @()
	for ($try = 0; $try -lt 4; $try++) {
		##	Search and the Start menu take the foreground and hold it, and no amount
		##	of asking gets it back while they are open. Escape closes them. A Search
		##	host left over from the logon can ignore Escape and sit there for hours,
		##	so one that is still in front on the next try is ended instead. All of
		##	these come back on demand, so nothing is lost.
		$fg = [Silk.Win]::GetForegroundWindow()
		if ($fg -ne [IntPtr]::Zero -and -not (fForegroundIsOurs $h)) {
			$owner = 0
			[void][Silk.Win]::GetWindowThreadProcessId($fg, [ref]$owner)
			$name = (Get-Process -Id $owner -ErrorAction SilentlyContinue).ProcessName
			if ($name -in @("SearchHost", "StartMenuExperienceHost", "ShellExperienceHost", "TextInputHost")) {
				if ($owner -in $escaped) {
					Stop-Process -Id $owner -Force -ErrorAction SilentlyContinue
					fNote "ended $name $owner, which kept the foreground after Escape"
					Start-Sleep -Milliseconds 1500
				} else {
					[SilkKeys]::Tap([uint16]0x1B)        ## escape
					$escaped += $owner
					Start-Sleep -Milliseconds 500
				}
			}
		}
		[void][Silk.Win]::ShowWindow($h, 9)          ## SW_RESTORE
		[SilkKeys]::Tap([uint16]0x12)                ## a bare Alt: claims last input
		$mine = [Silk.Win]::GetCurrentThreadId()
		$theirs = [Silk.Win]::GetWindowThreadProcessId([Silk.Win]::GetForegroundWindow(), [IntPtr]::Zero)
		if ($theirs -ne 0 -and $theirs -ne $mine) { [void][Silk.Win]::AttachThreadInput($mine, $theirs, $true) }
		[void][Silk.Win]::BringWindowToTop($h)
		[void][Silk.Win]::SetForegroundWindow($h)
		if ($theirs -ne 0 -and $theirs -ne $mine) { [void][Silk.Win]::AttachThreadInput($mine, $theirs, $false) }
		Start-Sleep -Milliseconds 400
		##	Any window of ours in front counts. The app owns more than one, and the
		##	handle Windows reports is not always the one we went looking for.
		if (fForegroundIsOurs $h) { return $true }
	}
	$false
}

function fForegroundIsOurs($h) {
	$fg = [Silk.Win]::GetForegroundWindow()
	if ($fg -eq $h) { return $true }
	if ($fg -eq [IntPtr]::Zero) { return $false }
	$want = 0; $got = 0
	[void][Silk.Win]::GetWindowThreadProcessId($h, [ref]$want)
	[void][Silk.Win]::GetWindowThreadProcessId($fg, [ref]$got)
	$want -ne 0 -and $want -eq $got
}

##	What actually has the foreground, for when it is not us.
function fForeground {
	$fg = [Silk.Win]::GetForegroundWindow()
	if ($fg -eq [IntPtr]::Zero) { return "nothing has the foreground" }
	$pid2 = 0
	[void][Silk.Win]::GetWindowThreadProcessId($fg, [ref]$pid2)
	$sb = New-Object System.Text.StringBuilder 256
	[void][Silk.Win]::GetWindowTextW($fg, $sb, 256)
	$name = (Get-Process -Id $pid2 -ErrorAction SilentlyContinue).ProcessName
	"hwnd $fg pid $pid2 ($name) '$($sb.ToString())'"
}

##	Clicking moves the real pointer, because there is only one. Fine on a machine
##	nobody is sitting at, which is the only kind this runs on.
function fClick($x, $y, $double = $false) {
	[void][Silk.Win]::SetCursorPos([int]$x, [int]$y)
	Start-Sleep -Milliseconds 120
	foreach ($n in 1..$(if ($double) { 2 } else { 1 })) {
		[Silk.Win]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
		[Silk.Win]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
		Start-Sleep -Milliseconds 60
	}
	Start-Sleep -Milliseconds 350
}

$script:vks = @{
	ctrl = 0x11; shift = 0x10; alt = 0x12
	tab = 0x09; escape = 0x1B; enter = 0x0D; space = 0x20
	f4 = 0x73; f11 = 0x7A; win = 0x5B
}

##	Literal text, typed through the keyboard layout the way a keyboard does.
function fSend($text) { [SilkKeys]::Text($text); Start-Sleep -Milliseconds 250 }

##	The other way a character reaches a window: handed over whole instead of
##	typed. The touch keyboard, text expanders and some accessibility tools all
##	send the characters their layout has no key for this way.
function fSendChars($text) { [SilkKeys]::TextUnicode($text); Start-Sleep -Milliseconds 400 }

##	One chord, spelled "ctrl+shift+t" or "alt+f" or "escape". A single character is
##	looked up through the keyboard layout so a comma is a comma wherever it lives.
function fPress($combo) {
	$parts = $combo.ToLower() -split '\+'
	$key = $parts[-1]
	$mods = @()
	foreach ($m in $parts[0..([math]::Max(0, $parts.Count - 2))]) {
		if ($m -ne $key -and $script:vks.ContainsKey($m)) { $mods += [uint16]$script:vks[$m] }
	}
	if ($parts.Count -eq 1) { $mods = @() }
	$vk = if ($script:vks.ContainsKey($key)) { [uint16]$script:vks[$key] }
	      else { [uint16]([SilkKeys]::VkKeyScanW([char]$key) -band 0xFF) }
	if ($mods.Count) { [SilkKeys]::Chord([uint16[]]$mods, $vk) } else { [SilkKeys]::Tap($vk) }
	Start-Sleep -Milliseconds 300
}

function fRect($h) {
	$r = New-Object Silk.Win+RECT
	[void][Silk.Win]::GetWindowRect($h, [ref]$r)
	@{ x = $r.Left; y = $r.Top; w = $r.Right - $r.Left; h = $r.Bottom - $r.Top }
}

##	A screen grab is the faithful picture - it is what the compositor put up, and
##	it is the only one that sees a window drawn without a redirection bitmap (the
##	transparent path). PrintWindow is the fallback for a session nobody can see.
function fShot($h, $name) {
	$r = fRect $h
	if ($r.w -le 0 -or $r.h -le 0) { return $null }
	$bmp = New-Object System.Drawing.Bitmap $r.w, $r.h
	$g = [System.Drawing.Graphics]::FromImage($bmp)
	$how = "screen"
	if (fSessionUsable) {
		$g.CopyFromScreen($r.x, $r.y, 0, 0, (New-Object System.Drawing.Size $r.w, $r.h))
	} else {
		$dc = $g.GetHdc()
		[void][Silk.Win]::PrintWindow($h, $dc, 2)
		$g.ReleaseHdc($dc)
		$how = "printwindow"
	}
	##	Two boxes pull into one directory, so the name has to say which box.
	if ($script:shotDir) {
		New-Item -ItemType Directory -Force -Path $script:shotDir | Out-Null
		$bmp.Save((Join-Path $script:shotDir "$env:COMPUTERNAME-$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
	}
	$bmp | Add-Member -NotePropertyName How -NotePropertyValue $how -PassThru
}

##	Fraction of sampled pixels carrying any light. A window that came up but never
##	drew reads near zero, which is the difference between a real capture and the
##	black rectangle a locked session hands back.
function fInk($bmp, $step = 4) {
	if (-not $bmp) { return 0.0 }
	$lit = 0; $seen = 0
	for ($y = 0; $y -lt $bmp.Height; $y += $step) {
		for ($x = 0; $x -lt $bmp.Width; $x += $step) {
			$seen++
			$px = $bmp.GetPixel($x, $y)
			if ($px.R + $px.G + $px.B -gt 40) { $lit++ }
		}
	}
	if ($seen -eq 0) { 0.0 } else { [math]::Round($lit / $seen, 4) }
}

##	How much of the picture moved. Ink saturates on a wallpaper, so 'did anything
##	happen' has to be asked as a difference rather than a brightness.
function fDiff($a, $b, $step = 3) {
	if (-not $a -or -not $b) { return 1.0 }
	if ($a.Width -ne $b.Width -or $a.Height -ne $b.Height) { return 1.0 }
	$moved = 0; $seen = 0
	for ($y = 0; $y -lt $a.Height; $y += $step) {
		for ($x = 0; $x -lt $a.Width; $x += $step) {
			$seen++
			$p1 = $a.GetPixel($x, $y); $p2 = $b.GetPixel($x, $y)
			if ([math]::Abs($p1.R - $p2.R) + [math]::Abs($p1.G - $p2.G) + [math]::Abs($p1.B - $p2.B) -gt 24) { $moved++ }
		}
	}
	if ($seen -eq 0) { 0.0 } else { [math]::Round($moved / $seen, 4) }
}

##	One value out of a written config, and whether the file actually says it. A
##	setting left at its shipped default stays commented in the template, so a
##	scenario that cannot tell those apart reads an empty answer and calls it a bug.
function fSetting($path, $dotted) {
	if (-not (Test-Path $path)) { return @{ value = $null; source = "no file" } }
	$block, $leaf = $dotted -split '\.', 2
	$in = $false
	$fallback = $null
	foreach ($line in (Get-Content $path)) {
		if ($line -match '^[^\s#]') { $in = ($line -match "^${block}\s*:"); continue }
		if (-not $in) { continue }
		if ($line -match "^\s+${leaf}\s*:\s*(.*?)\s*$") { return @{ value = $Matches[1].Trim('"'); source = "set" } }
		if ($line -match "^\s+#\s*${leaf}\s*:\s*(.*?)\s*(##.*)?$") { $fallback = $Matches[1].Trim().Trim('"') }
	}
	if ($null -ne $fallback) { @{ value = $fallback; source = "default" } } else { @{ value = $null; source = "absent" } }
}

##	The usable part of the screen: what is left once the taskbar has had its share.
##	A window may be smaller than the display and still not fit.
function fWorkArea {
	$r = New-Object Silk.Win+RECT
	if (-not [Silk.Win]::SystemParametersInfoW(0x0030, 0, [ref]$r, 0)) { return $null }
	@{ x = $r.Left; y = $r.Top; w = $r.Right - $r.Left; h = $r.Bottom - $r.Top }
}

function fStop($p) {
	if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
}

##	History:
##		- 20260908 JC: Created.
