@echo off
SETLOCAL

::	Purpose:
::		Runs the SilkTerm dogfood launcher, 'n8runterm.ps1', passing every
::		argument through. Lets it be started from cmd.exe, Win+R, the Start
::		menu, a shortcut and Task Scheduler, none of which can execute a .ps1
::		directly: having .PS1 in PATHEXT only makes cmd hand the file to
::		ShellExecute, and the default .ps1 association opens an editor rather
::		than running it.
::		All the logic lives in the .ps1, including the self-elevation, so this
::		runs pwsh in the current window and lets the launcher put up its own
::		UAC prompt and continue minimized from there.
::		The .ps1 lives in the crossplatform util dir rather than beside this
::		file, so the known locations are tried in order. 'synced' is a junction
::		to Dropbox that reads as an empty directory on Windows, so the real
::		spelling is tried too.
::	History: At bottom of file.

::	Copyright (c) 2026 Bubbles
::	Licensed under The MIT License (MIT). Full text at:
::		https://mit-license.org/
::	SPDX-License-Identifier: MIT

::----------------------------------------------------------------------------
:MAIN

	set "PSFILE="
	call :FIND "%~dp0n8runterm.ps1"
	call :FIND "%USERPROFILE%\synced\0-0\common\exec\util\0_crossplatform\n8runterm.ps1"
	call :FIND "%USERPROFILE%\Dropbox\0-0\common\exec\util\0_crossplatform\n8runterm.ps1"
	call :FIND "C:\opt\0-0\common\exec\synced\util\0_crossplatform\n8runterm.ps1"
	call :FIND "C:\0-0\common\exec\synced\util\0_crossplatform\n8runterm.ps1"

	::
	:: Validate
	::

	:: Script
	if defined PSFILE goto :OK005
		echo n8runterm.ps1 was not found in any of the known locations.
		goto :ERROR
	:OK005

	:: PowerShell 7. The script is pwsh-only, so do not fall back to the
	:: Windows PowerShell 5.1 that ships in the box.
	where /q pwsh.exe
	if not errorlevel 1 goto :OK010
		echo PowerShell 7 ^(pwsh.exe^) was not found on PATH.
		goto :ERROR
	:OK010

	::
	:: Execute
	::
	pwsh.exe -NoProfile -ExecutionPolicy Bypass -File "%PSFILE%" %*
	set RC=%ERRORLEVEL%

ENDLOCAL & exit /b %RC%

::----------------------------------------------------------------------------
:FIND
	if defined PSFILE goto :EOF
	if exist "%~1" set "PSFILE=%~1"
goto :EOF

::----------------------------------------------------------------------------
:ERROR
	echo [ An error occurred. ]
ENDLOCAL & exit /b 1

::	History:
::		- 20260908 JC: Look under Dropbox as well as 'synced'.
::		- 20260907 JC: Created.
