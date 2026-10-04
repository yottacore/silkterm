#!/usr/bin/env bash

#  shellcheck shell=bash
#  shellcheck disable=2034  ## _letterbox is used by the scripts that source this.
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.'
#  shellcheck disable=2086  ## Integer pids need no quoting.

##	- Purpose:
##		Shared helpers for the two shootout rigs (termbench-run.bash, sizebench-run.bash).
##		Sourced, never run. The wrapper above them is Python and carries its own copy of
##		the same output style.
##
##		Output matches the house style used by cicd.bash: fEcho prints a bracketed status
##		line, fEcho_Clean prints plain and collapses repeat blanks, so the blank-line
##		rhythm does the visual grouping.
##

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

##	Guard against being sourced twice by a wrapper that also sources a rig.
[[ -n "${_benchCommonLoaded:-}" ]] && return 0
declare -r _benchCommonLoaded=1

declare -r _letterbox="$(printf '%.0s-' {1..78})"

declare -i _wasLastEchoBlank=0
fEcho_Clean(){ if [[ -n "${1:-}" ]]; then echo -e "$*"; _wasLastEchoBlank=0; elif [[ $_wasLastEchoBlank -eq 0 ]] && echo; then _wasLastEchoBlank=1; fi; }
fEcho(){ if [[ -n "$*" ]]; then fEcho_Clean "[ $* ]"; else fEcho_Clean ""; fi; }
fSection(){ fEcho_Clean; fEcho_Clean "${_letterbox}"; fEcho "$*"; }
fDie(){ { fEcho_Clean; fEcho "FAILED: $*"; } >&2; exit 1; }

##	Kill only pids this script started, and only by pid. A pattern kill matches the
##	harness's own command line and any copy already open and in use; that has taken out a
##	session mid-run before now.
fKillPids(){
	local -i pid=0
	for pid in "$@"; do ((pid > 0)) && kill ${pid} 2>/dev/null || true; done
	sleep 1
	for pid in "$@"; do ((pid > 0)) && kill -9 ${pid} 2>/dev/null || true; done
	return 0
}

##	Where cargo put the build. CARGO_TARGET_DIR moves it, and a relative one is taken
##	from the repository, where cargo runs.
fTargetDir(){
	local -r repo="$1" dir="${CARGO_TARGET_DIR:-target}"
	if [[ "${dir}" == /* ]]; then printf '%s' "${dir}"; else printf '%s' "${repo}/${dir}"; fi
}

##	A terminal binary: PATH first, then the kept downloads, so a re-measure needs no
##	re-download.
fFindTerm(){
	local -r repo="$1" name="$2"
	local -r terms="${repo}/cicd/artifacts/sizebench/terms"
	local candidate=""
	if candidate="$(command -v "${name}" 2>/dev/null)"; then printf '%s' "${candidate}"; return 0; fi
	for candidate in "${terms}/bin/${name}" "${terms}/usr/bin/${name}"; do
		if [[ -f "${candidate}" && -x "${candidate}" ]]; then printf '%s' "${candidate}"; return 0; fi
	done
	return 1
}

##	Every process started on a throwaway account (fPrivateAccount): the launched tree, plus
##	whatever carries the account's HOME. A service the private bus starts is not the bus's
##	child, since dbus-daemon forks twice, so the tree alone misses GNOME Terminal's server.
##	HOME alone misses Electron, which writes its process title over its environment.
fOwnedPids(){
	local -r home="$1" launched="${2:-0}"
	local pid="" environ=""
	if ((launched > 0)); then fCollectTree "${launched}"; fi
	for pid in $(pgrep -u "$(id -u)" 2>/dev/null || true); do
		environ="$(tr '\0' '\n' 2>/dev/null < "/proc/${pid}/environ" || true)"
		if [[ $'\n'"${environ}"$'\n' == *$'\n'"HOME=${home}"$'\n'* ]]; then printf '%s\n' "${pid}"; fi
	done
	return 0
}

##	The topmost of those running the given executable: the terminal itself, measured from
##	there down, so neither the bus nor a client that only asked for a window is billed.
##	Usage: fOwnedRoot <executable> <throwaway home> <launched pid>
fOwnedRoot(){
	local -r exe="$1" home="$2" launched="$3"
	local want="" pid="" parent=""
	local -A mine=()
	want="$(readlink -f "${exe}")"
	for pid in $(fOwnedPids "${home}" "${launched}"); do
		if [[ "$(readlink -f "/proc/${pid}/exe" 2>/dev/null || true)" == "${want}" ]]; then mine[${pid}]=1; fi
	done
	for pid in $(printf '%s\n' "${!mine[@]}" | sort -n); do
		parent="$(ps -o ppid= -p "${pid}" 2>/dev/null || true)"
		parent="${parent//[[:space:]]/}"
		if [[ -z "${mine[${parent:-0}]:-}" ]]; then printf '%s' "${pid}"; return 0; fi
	done
	return 1
}

##	The grid a terminal reports from inside, as "rows cols", once it has settled. A report
##	can predate the resize it is meant to answer (80x24 from a window not yet tiled), so
##	a size is taken only once two reports in a row agree.
fReadGrid(){
	local -r reportFile="$1"
	local seen="" again=""
	local -i waited=0 tries=0
	while ((waited < 60)); do [[ -f "${reportFile}" ]] && break; sleep 0.25; waited+=1; done
	[[ -f "${reportFile}" ]] || fDie "the terminal never reported its grid"
	for ((tries = 0; tries < 12; tries++)); do
		seen="$(cat "${reportFile}" 2>/dev/null || true)"
		sleep 0.5
		again="$(cat "${reportFile}" 2>/dev/null || true)"
		[[ -n "${seen}" && "${seen}" == "${again}" ]] && break
	done
	printf '%s' "${again}"
}

##	Fit a terminal to a grid by resizing whatever holds it, reading back the grid the
##	terminal reports from inside. That works for any terminal without knowing its cell
##	metrics or its geometry flags.
##	Usage: fFitGrid <cols> <rows> <report file> <resize function> <start width> <start height>
fFitGrid(){
	local -i wantC=$1 wantR=$2
	local reportFile="$3" resizeFn="$4"
	local -i w=$5 h=$6 pass=0 gotC=0 gotR=0
	## A proportional step can hop over the answer and come back (43 rows, 41, 43 ...)
	## when a cell is near 20 pixels, which a new account's default font made routine.
	## When two passes in a row land close on either side, the next try is the middle
	## of them. Far apart, the proportional step is the better guess.
	## Only the pass before counts: the first report can predate the window's tiling.
	local -i prevW=0 prevH=0 prevC=0 prevR=0 nextW=0 nextH=0
	local report=""

	for ((pass = 1; pass <= 12; pass++)); do
		"${resizeFn}" ${w} ${h}
		rm -f "${reportFile}"
		report="$(fReadGrid "${reportFile}")" || exit 1
		read -r gotR gotC <<< "${report}" || true
		((gotC)) || fDie "unreadable grid report"
		fEcho_Clean "      fit pass ${pass}: ${gotC}x${gotR} at ${w}x${h}"
		if ((gotC == wantC && gotR == wantR)); then fEcho "grid ${gotC}x${gotR}"; return 0; fi
		nextW=${w}; nextH=${h}
		if ((gotC != wantC)); then
			if ((prevC && prevC - wantC <= 3 && wantC - prevC <= 3 && (prevC - wantC) * (gotC - wantC) < 0)); then nextW=$(( (prevW + w) / 2 ))
			else nextW=$(( w * wantC / gotC )); fi
		fi
		if ((gotR != wantR)); then
			if ((prevR && prevR - wantR <= 3 && wantR - prevR <= 3 && (prevR - wantR) * (gotR - wantR) < 0)); then nextH=$(( (prevH + h) / 2 ))
			else nextH=$(( h * wantR / gotR )); fi
		fi
		prevW=${w}; prevH=${h}; prevC=${gotC}; prevR=${gotR}
		w=${nextW}; h=${nextH}
	done
	fDie "could not fit ${wantC}x${wantR} (stopped at ${gotC}x${gotR})"
}

##	A launched pid plus everything under it. Diffing the system-wide process list instead
##	would sweep in whatever else the desktop started meanwhile, and a name match would find
##	copies that were already running.
fCollectTree(){
	local -i root="$1"
	local -a out=("${root}") queue=("${root}")
	local -i pid=0 kid=0
	while ((${#queue[@]} > 0)); do
		pid="${queue[0]}"; queue=("${queue[@]:1}")
		while read -r kid; do
			[[ -z "${kid}" ]] && continue
			out+=("${kid}"); queue+=("${kid}")
		done < <(pgrep -P ${pid} 2>/dev/null || true)
	done
	printf '%s\n' "${out[@]}"
}

##	A terminal under test runs as it would on a new account: a home, the XDG folders
##	and a session bus that the rig makes and removes. The measuring account's own
##	settings then cannot reach a published figure, and a terminal that writes its
##	settings on launch writes them here. GNOME Terminal and xfce4-terminal keep theirs
##	behind the session bus, which is why the bus is part of it. XDG_RUNTIME_DIR is left
##	alone, since the compositor's socket is in it.
##
##	The bus starts only the services a terminal keeps its settings in. With the whole
##	desktop's set on offer, a GTK program asks for the desktop portal, the portal asks
##	for the keyring, the keyring cannot be reached from here, and GNOME Terminal's
##	server sits in that until its own launcher gives up on it.
##	Usage: fPrivateAccount <dir>, then: env "${_privateEnv[@]}" "${_privateBus[@]}" <terminal...>
declare -a _privateEnv=() _privateBus=()
fPrivateAccount(){
	local -r home="$1"
	command -v dbus-run-session >/dev/null 2>&1 || fDie "dbus-run-session is not installed (package dbus-daemon)"
	mkdir -p "${home}/.config" "${home}/.local/share" "${home}/.local/state" "${home}/.cache" "${home}/.bus"
	local service=""
	for service in org.gnome.Terminal ca.desrt.dconf org.xfce.Xfconf; do
		if [[ -f "/usr/share/dbus-1/services/${service}.service" ]]; then
			cp "/usr/share/dbus-1/services/${service}.service" "${home}/.bus/"
		fi
	done
	cat > "${home}/.bus.conf" <<-EOF
		<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
		 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
		<busconfig>
			<type>session</type>
			<keep_umask/>
			<listen>unix:tmpdir=/tmp</listen>
			<auth>EXTERNAL</auth>
			<servicedir>${home}/.bus</servicedir>
			<policy context="default">
				<allow send_destination="*" eavesdrop="true"/>
				<allow eavesdrop="true"/>
				<allow own="*"/>
			</policy>
		</busconfig>
	EOF
	_privateBus=(dbus-run-session --config-file="${home}/.bus.conf" --)
	_privateEnv=(
		"HOME=${home}"
		"XDG_CONFIG_HOME=${home}/.config"
		"XDG_DATA_HOME=${home}/.local/share"
		"XDG_STATE_HOME=${home}/.local/state"
		"XDG_CACHE_HOME=${home}/.cache"
		"BENCH_REAL_HOME=${HOME}"
		"BENCH_REAL_XDG_DATA_HOME=${XDG_DATA_HOME:-}"
		## no accessibility bus here either, and GTK warns on every start without this
		"NO_AT_BRIDGE=1"
	)
}

##	What a SilkTerm settings file says about its performance profile, in either the
##	nested or the dotted spelling: "automatic=<value> profile=<value>".
fSilkProfile(){
	local -r file="$1"
	awk '
		function bare(v){ gsub(/["\047]/, "", v); return v }
		/^performance:/              { inside = 1; next }
		/^[^ \t#]/                   { inside = 0 }
		inside && $1 == "automatic:" { automatic = bare($2) }
		inside && $1 == "profile:"   { profile = bare($2) }
		$1 == "performance.automatic:" { automatic = bare($2) }
		$1 == "performance.profile:"   { profile = bare($2) }
		END { printf "automatic=%s profile=%s", (automatic == "" ? "?" : automatic), (profile == "" ? "?" : profile) }
	' "${file}"
}

##	The +candy row is only that if nothing turned its effects down.
fRequireCandyProfile(){
	local -r file="$1"
	local state=""
	state="$(fSilkProfile "${file}")"
	fEcho "SilkTerm profile in force: ${state}"
	if [[ "${state}" != "automatic=false profile=custom" ]]; then
		fDie "the +candy row ran with '${state}', so its effects may have been turned down"
	fi
}

##
##	History:
##		- 20260730: Factored out of the two rigs when they moved under utility/include/.
##		- 20260918: fPrivateAccount, fSilkProfile, fRequireCandyProfile.
##		- 20260928: fFindTerm, fOwnedPids, fOwnedRoot, fReadGrid and fFitGrid, from the speed rig.
##
