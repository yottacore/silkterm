#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes, use double quotes for that.' I know, and I often want an explicit '$'.
#  shellcheck disable=2046  ## 'Quote to prevent word-splitting.' (OK for integers.)
#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.' Cumbersome and unnecessary.
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'

##	Purpose:
##		- Run the cross-built Windows silkterm.exe on this Linux box under host wine, so
##		  the Windows build can be looked at (rendering, fonts, chrome, dialogs, DPI,
##		  transparency) without a Windows machine.
##		- No docker. The Windows build links only system DLLs, so unlike the sister
##		  nemo-anywhere launcher there is no GTK runtime to stage out of a container -
##		  only fonts and a private wineprefix, both handled here.
##		- Everything goes in cicd/artifacts/win-run/ (gitignored): the wineprefix, a
##		  copy of the exe, its own config.shcl, and the run log. The real ~/.wine and
##		  ~/.config/silkterm are never touched, so a Windows-side config rewrite cannot
##		  disturb the Linux build's settings.
##		- The exe is re-copied every run, so a fresh cross-build is picked up
##		  automatically. --restage rebuilds the wineprefix itself (after a wine upgrade).
##		- Each run replaces the last: a previous instance of THIS staged copy is killed
##		  first, matched on its exact staged path - so a silkterm started from anywhere
##		  else (notably the Linux dogfood build) is left alone. wineserver is shared with
##		  any other wine app and is never touched.
##	What works, and what does not (measured against wine 10.0):
##		- Works: the window, the GPU path (wgpu picks up the real card through
##		  winevulkan), fonts, menu/tab chrome, dialogs, wallpaper, scrim.
##		- Does not work: the shell. Wine's ConPTY is half implemented -
##		  CreatePseudoConsole succeeds and the initial size is honored (conhost starts
##		  headless at the right grid), but a child spawned with
##		  PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE does not actually attach to it, so no
##		  child output ever reaches the grid. Expect a live, correctly drawn, empty
##		  terminal. Good for looking at the Windows build, not for driving it.
##		- Because of that, the default shell is a keep-alive rather than a real one:
##		  cmd.exe finds no usable console, exits immediately, and takes the window with
##		  it (the terminal quits when its last pane's command ends). A child that simply
##		  stays running holds the window open so there is something to look at. Pass
##		  --shell cmd.exe to get the real thing back once wine can carry the I/O.
##		- ResizePseudoConsole is a stub returning E_NOTIMPL, and the terminal backend
##		  asserts the result is S_OK - so the app used to die on its first resize. The
##		  small conpty.dll built here (fBuildConptyShim) forwards create/close to
##		  kernel32 and answers resize with S_OK, which is what keeps it alive. It needs
##		  mingw; without mingw the app still starts but dies on the first resize.
##		- Microsoft's own conpty.dll + OpenConsole.exe (the ConPTY nuget) were tried and
##		  fail earlier still - CreatePseudoConsole returns 0xd000000d - so there is no
##		  better implementation to drop in, and dropping one in would be worse: the
##		  backend prefers any loadable conpty.dll over kernel32 and does not fall back
##		  when it fails.
##		- A newer wine does not help either. Measured on 11.14: the pty pipe still gets
##		  zero bytes. Only ResizePseudoConsole changes - it fakes success from 11.2 on,
##		  which just makes the shim redundant. To try another wine put it first on PATH
##		  (wine/wineboot are called unqualified) and pass --restage for its own prefix.
##		- Only the x86_64 build runs; wine on x86_64 cannot execute the ARM64 exe.
##		- Wine's menu builder is turned off for every wine call here. Left on, it
##		  registers file types and menu entries on the desktop that point into the
##		  staged prefix, and they go dead when the prefix is rebuilt.
##	Syntax:
##		run-windows-build-via-wine.bash [OPTIONS] [-- ARGS...]
##		  --restage        Rebuild the wineprefix from scratch, then run.
##		  --attach         Run in the foreground with output on this terminal.
##		  --exe PATH       Use a specific Windows .exe instead of the newest built one.
##		  --shell CMD      Command the terminal spawns (default: a keep-alive; see above).
##		  --display :N     X display to open on (default: current DISPLAY, else :0).
##		  --help           Show this block.
##		  -- ARGS...       Everything after -- is passed through to silkterm.exe.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Configuration

declare -r  appId="silkterm"
declare -r  repoRoot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
declare -r  stageDir="${repoRoot}/cicd/artifacts/win-run"
declare -r  winePrefix="${stageDir}/prefix"
declare -r  stagedExe="${stageDir}/app/${appId}.exe"
declare -r  logFile="${stageDir}/wine-run.log"
declare -r  mingwCc="x86_64-w64-mingw32-gcc"

## CARGO_TARGET_DIR moves the build, and a relative one is taken from the
## repository, where cargo runs.
declare     targetDir="${CARGO_TARGET_DIR:-target}"
[[ "${targetDir}" == /* ]] || targetDir="${repoRoot}/${targetDir}"
readonly targetDir

## Where a freshly cross-built exe shows up, newest wins.
declare -ar exeCandidates=(
	"${targetDir}/x86_64-pc-windows-gnu/release/${appId}.exe"
	"${repoRoot}/cicd/artifacts/release/${appId}-"*"-windows-x86_64.exe"
)

## Host font trees to expose to the Windows side. The Windows font backend scans
## C:\windows\Fonts recursively, so one symlink per tree is enough.
declare -ar hostFontDirs=(
	"/usr/share/fonts"
	"/usr/local/share/fonts"
	"${HOME}/.local/share/fonts"
	"${HOME}/.fonts"
)

## Keep-alive stand-in for a shell: a child that just stays running, so the window
## stays up. A loopback ping is the one long-lived thing every wineprefix ships.
declare -r  keepAliveShell="ping -n 100000 127.0.0.1"

## Runtime options, overridable by flags.
declare  optRestage=0
declare  optAttach=0
declare  optExe=""
declare  optShell="${SILK_WINE_SHELL:-${keepAliveShell}}"
declare  optDisplay="${DISPLAY:-:0}"
declare -a passThrough=()


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Helpers

fEcho(){ echo "[ ${*} ]"; }
fDie(){ echo -e "\nError in $(basename "${BASH_SOURCE[0]}"): ${*}\n" >&2; exit 1; }

## Unix path -> the wine drive Z: form. Z: is mapped to the unix root by default.
fToWin(){ echo "Z:${1//\//\\}"; }


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Argument handling

fParseArgs(){
	while (( $# )); do
		case "${1}" in
			--restage)       optRestage=1 ;;
			--attach)        optAttach=1 ;;
			--exe)           optExe="${2:-}";     shift ;;
			--shell)         optShell="${2:-}";   shift ;;
			--display)       optDisplay="${2:-}"; shift ;;
			--help|-h)       fShowHelp; exit 0 ;;
			--)              shift; passThrough=("${@}"); return 0 ;;
			*)               fDie "unknown option '${1}' (try --help)" ;;
		esac
		shift
	done
}

fShowHelp(){
	sed -n '/^##	Purpose:/,/^##	History:/p' "${BASH_SOURCE[0]}" | sed 's/^##\t\?//'
}


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Staging

## Newest Windows exe from the candidate list (an explicit --exe wins outright).
fResolveExe(){
	if [[ -n "${optExe}" ]]; then
		[[ -f "${optExe}" ]] || fDie "--exe '${optExe}' does not exist"
		echo "${optExe}"
		return 0
	fi
	local newest="" cand
	for cand in "${exeCandidates[@]}"; do
		[[ -f "${cand}" ]] || continue                       # skips the no-match glob
		[[ -z "${newest}" || "${cand}" -nt "${newest}" ]] && newest="${cand}"
	done
	[[ -n "${newest}" ]] || fDie "no Windows build found - run:\n\tPATH=\"\${HOME}/.cargo/bin:\${PATH}\" cargo build --release --target x86_64-pc-windows-gnu"
	echo "${newest}"
}

## Private wineprefix, so the real ~/.wine keeps its own state and app list.
fStagePrefix(){
	if (( optRestage )) && [[ -d "${winePrefix}" ]]; then
		fEcho "Restaging: removing the old wineprefix"
		rm -rf "${winePrefix}"
	fi
	[[ -d "${winePrefix}/drive_c" ]] && return 0
	fEcho "Creating a private wineprefix (first run, takes a few seconds)"
	mkdir -p "${winePrefix}"
	WINEPREFIX="${winePrefix}" WINEDEBUG=-all wineboot -i >>"${logFile}" 2>&1 \
		|| fDie "wineboot failed - see ${logFile}"
}

## A fresh wineprefix ships no fonts at all, and the text stack aborts with
## 'no default font found' when it can see none. Symlinking the host trees in is
## enough, and keeps the staged copy small.
fLinkFonts(){
	local -r fontDir="${winePrefix}/drive_c/windows/Fonts"
	mkdir -p "${fontDir}"
	local dir linkName
	for dir in "${hostFontDirs[@]}"; do
		[[ -d "${dir}" ]] || continue
		linkName="host-$(echo "${dir}" | tr -c 'a-zA-Z0-9' '-' | sed 's/^-*//; s/-*$//')"
		ln -sfn "${dir}" "${fontDir}/${linkName}"
	done
}

## conpty.dll shim - see the Purpose block. The backend loads any conpty.dll next
## to the exe in preference to kernel32, which is the hook used here: create and
## close go straight back to kernel32 (both work under wine), and resize answers
## S_OK instead of wine's E_NOTIMPL, which the backend would assert on.
## Consequence: a resize is silently not forwarded to the child, which costs
## nothing here because the child never attaches to the pseudoconsole anyway.
fBuildConptyShim(){
	local -r shimSrc="${stageDir}/app/conpty-shim.c"
	local -r shimDll="${stageDir}/app/conpty.dll"

	if ! command -v "${mingwCc}" >/dev/null 2>&1; then
		fEcho "WARNING: ${mingwCc} not found - skipping the conpty shim"
		fEcho "WARNING: the app will start but die on its first window resize"
		rm -f "${shimDll}"
		return 0
	fi

	## Written to a scratch file first and only moved into place when it differs, so
	## an unchanged source keeps its mtime and the dll below is not rebuilt every run.
	local -r shimNew="${shimSrc}.new"
	cat > "${shimNew}" <<-'EOF'
		/* See run-windows-build-via-wine.bash (fBuildConptyShim) for why this exists. */
		#include <windows.h>

		typedef void *HPCON_T;   /* HPCON needs a Win10 header level; it is just a handle */

		typedef HRESULT (WINAPI *createFn)(COORD, HANDLE, HANDLE, DWORD, HPCON_T *);
		typedef void    (WINAPI *closeFn)(HPCON_T);

		static createFn realCreate;
		static closeFn  realClose;

		static void loadReal(void) {
			if (realCreate) return;
			HMODULE k32 = GetModuleHandleW(L"kernel32.dll");
			realCreate = (createFn)(void (*)(void))GetProcAddress(k32, "CreatePseudoConsole");
			realClose  = (closeFn)(void (*)(void))GetProcAddress(k32, "ClosePseudoConsole");
		}

		__declspec(dllexport) HRESULT WINAPI CreatePseudoConsole(COORD size, HANDLE in, HANDLE out, DWORD flags, HPCON_T *pcon) {
			loadReal();
			if (!realCreate) return E_NOTIMPL;
			return realCreate(size, in, out, flags, pcon);
		}

		/* Wine returns E_NOTIMPL here, which the terminal backend asserts on. */
		__declspec(dllexport) HRESULT WINAPI ResizePseudoConsole(HPCON_T pcon, COORD size) {
			(void)pcon; (void)size;
			return S_OK;
		}

		__declspec(dllexport) void WINAPI ClosePseudoConsole(HPCON_T pcon) {
			loadReal();
			if (realClose) realClose(pcon);
		}

		BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
			(void)h; (void)reason; (void)reserved;
			return TRUE;
		}
	EOF

	if cmp -s "${shimNew}" "${shimSrc}"; then rm -f "${shimNew}"; else mv -f "${shimNew}" "${shimSrc}"; fi

	[[ -f "${shimDll}" && "${shimDll}" -nt "${shimSrc}" ]] && return 0
	fEcho "Building the conpty shim"
	"${mingwCc}" -O2 -shared -o "${shimDll}" "${shimSrc}" >>"${logFile}" 2>&1 \
		|| fDie "failed to build the conpty shim - see ${logFile}"
}


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Launch

## Replace the previous run of THIS staged copy. Matched on the staged path, which
## no other build shares, so a terminal started from anywhere else - notably the
## Linux dogfood builds - can never match. Wine reports the app under its Windows
## path (Z:\...\app\silkterm.exe) while the launcher stub keeps the unix one, so
## every separator becomes a regex '.' and the one pattern catches both spellings.
## pkill returns 1 when nothing matched, which is fine. wineserver is shared with
## any other wine app and is deliberately left alone.
fStagedExePattern(){ printf '%s' "${stagedExe}" | sed 's/[][\\.^$*+?(){}|]/\\&/g; s#/#.#g'; }

## The keep-alive child outlives the app: it never attached to the pseudoconsole, so
## nothing tears it down when its parent goes, and one would leak per run. Matched on
## the whole command line, anchored - so it can only ever match the exact keep-alive
## started here, never some other ping the user is running.
fKillOrphanedKeepAlive(){
	local -r exact="^$(printf '%s' "${keepAliveShell}" | sed 's/[][\\.^$*+?(){}|]/\\&/g')$"
	pkill -f "${exact}" 2>/dev/null || true
}

fKillPrevious(){
	pkill -f "$(fStagedExePattern)" 2>/dev/null || true
	fKillOrphanedKeepAlive
}

## Did it survive startup? A command that ends straight away takes the window with
## it, which otherwise just looks like nothing happened.
fReportOutcome(){
	sleep 10   ## GPU + font bring-up under wine takes a few seconds
	if pgrep -f "$(fStagedExePattern)" >/dev/null 2>&1; then
		fEcho "Running"
	else
		fEcho "WARNING: it exited during startup - see ${logFile}"
		fEcho "WARNING: a command that ends at once (cmd.exe here) closes the window with it"
	fi
}

fLaunch(){
	local -r configFile="${stageDir}/config/config.shcl"
	mkdir -p "$(dirname "${configFile}")"

	export WINEPREFIX="${winePrefix}"
	export WINEDEBUG="${WINEDEBUG:--all}"
	export DISPLAY="${optDisplay}"

	## The shell string is split by the terminal's own POSIX-style parser, where an
	## unquoted backslash escapes the next character - so a plain C:\windows\... path
	## would arrive with its separators eaten. Bare names resolve on the Windows PATH.
	local -a launchArgs=(
		"--config" "$(fToWin "${configFile}")"
		"--shell"  "${optShell}"
	)
	(( ${#passThrough[@]} )) && launchArgs+=("${passThrough[@]}")

	fEcho "Launching the Windows build under wine (DISPLAY=${DISPLAY}, shell=${optShell})"
	fEcho "Note: wine's ConPTY does not carry child I/O - expect an empty grid"

	if (( optAttach )); then
		wine "${stagedExe}" "${launchArgs[@]}"
		return 0
	fi

	## Detach: new session, no controlling tty, own fds - so this script returns at
	## once and the app outlives it. The log path rides an env var to keep quotes off
	## argv, matching the sister launcher.
	fEcho "Log: ${logFile}"
	WINE_LOG="${logFile}" setsid /bin/sh -c \
		'exec "$@" </dev/null >>"$WINE_LOG" 2>&1' \
		sh wine "${stagedExe}" "${launchArgs[@]}" &
	disown 2>/dev/null || true

	fReportOutcome
}


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Main

fMain(){
	fParseArgs "${@}"

	command -v wine >/dev/null 2>&1 || fDie "wine is not installed"

	## Before wineboot as well as the run, since a new prefix registers its own set.
	export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:+${WINEDLLOVERRIDES};}winemenubuilder.exe=d"

	local -r sourceExe="$(fResolveExe)"

	mkdir -p "${stageDir}/app"
	: >"${logFile}"

	fKillPrevious
	fStagePrefix
	fLinkFonts
	fBuildConptyShim

	## Re-copied every run, so a rebuilt exe is picked up without --restage.
	cp -f "${sourceExe}" "${stagedExe}"
	fEcho "Staged $(basename "${sourceExe}") ($(date -r "${sourceExe}" '+%Y-%m-%d %H:%M'))"

	fLaunch
}


#••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
# Script entry point

## Bash environment settings
 set -u  #..................: Require variable declaration.
 set -e  #..................: Exit on errors.
 set -E  #..................: Propagate ERR trap into functions and subshells.
 set   -o pipefail  #.......: Fail a pipe if any stage fails.
 shopt -s inherit_errexit  #: Propagate 'set -e' into command substitutions. (Bash >=4.4.)

## This is a launcher, not a library.
[[ "${BASH_SOURCE[0]}" == "${0}" ]] || { echo -e "\nError in $(basename "${BASH_SOURCE[0]}"): Not meant to be 'sourced'.\n" >&2; return 1; }

## Kick everything off.
fMain  "${@}"


##	History:
##		- 2026-07-25: Created.
##		- 2026-09-24: Menu builder off.
