##	Prints what is on another process's console. Run as its own process: it has
##	to let go of its own console to join that one. A pipe or a redirect would
##	give the program a handle to write to, which is the case that always worked.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

param([Parameter(Mandatory)] [int] $Of, [Parameter(Mandatory)] [string] $Out)

Add-Type -Namespace SilkCon -Name Read -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct COORD { public short X; public short Y; }
[StructLayout(LayoutKind.Sequential)] public struct INFO {
	public COORD Size; public COORD Cursor; public short Attr;
	public short L, T, R, B; public COORD Max;
}
[DllImport("kernel32.dll")] public static extern bool FreeConsole();
[DllImport("kernel32.dll")] public static extern bool AttachConsole(uint pid);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll")] public static extern bool GetConsoleScreenBufferInfo(IntPtr h, out INFO info);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern bool ReadConsoleOutputCharacterW(IntPtr h, System.Text.StringBuilder text, uint len, COORD at, out uint read);
[DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
public static string Text(uint pid) {
	FreeConsole();
	if (!AttachConsole(pid)) return "(could not attach)";
	IntPtr h = CreateFileW("CONOUT$", 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
	INFO info; GetConsoleScreenBufferInfo(h, out info);
	uint len = (uint)(info.Size.X * (info.Cursor.Y + 1));
	var text = new System.Text.StringBuilder((int)len + 1);
	uint read; ReadConsoleOutputCharacterW(h, text, len, new COORD(), out read);
	CloseHandle(h); FreeConsole();
	return text.ToString();
}
'@
[SilkCon.Read]::Text([uint32]$Of) | Set-Content -Path $Out -Encoding UTF8

##	History:
##		- 20260918 JC: Created.
