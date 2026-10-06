#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes.'

##	- Purpose:
##		Make a Windows box's test session usable again after it locked itself, and
##		put it back on the console. A locked session hands back black pictures and
##		swallows typing, so the graphical scenarios skip until this is run.
##	- How it works, and why it is two steps:
##		Nothing can unlock a session from outside; it has to be authenticated
##		again. Connecting to it once over RDP IS that logon, so the session comes
##		back unlocked - but it is then a remote session, which the app treats
##		differently and which is not the real adapter. Moving it back to the
##		console is the second step, and that is what the 'console' job does.
##	- Syntax:
##		win-unlock.bash [--host <name>] [--as <account>] [--display <:n>]
##	- Exit: 0 if the box came back usable or was already, 1 if not.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
winRemote="${meDir}/win-remote.bash"

origArgs=("${@}")
only=""; account="wintest"; display="${WINUNLOCK_DISPLAY:-:98}"
while (($#)); do case "${1}" in
	--host)    only="${2:-}"; shift 2 ;;
	--as)      account="${2:-}"; shift 2 ;;
	--display) display="${2:-}"; shift 2 ;;
	-h|--help) grep -E '^##' "${0}" | sed 's/^##\t\?//'; exit 0 ;;
	*) echo "unknown option: ${1}" >&2; exit 2 ;;
esac; done

##	:99 is somebody's own session on this box. Refuse it rather than start a
##	server on top of it.
[[ "${display}" == ":99" ]] && { echo "win-unlock: ${display} is in use by a real session" >&2; exit 2; }
command -v sdl-freerdp >/dev/null || { echo "win-unlock: no sdl-freerdp, skipped"; exit 0; }
##	The RDP logon and the console move both change the box's sessions, so hold it for both.
[[ -n "${WINRIG_HELD:-}" ]] || exec "${winRemote}" ${only:+--host "${only}"} --optional hold "${0}" "${origArgs[@]}"

declare -a rows=()
mapfile -t rows < <("${winRemote}" ${only:+--host "${only}"} hosts 2>/dev/null | awk '$2 == "up" { print $1" "$3 }')
((${#rows[@]})) || { echo "win-unlock: nothing reachable, skipped"; exit 0; }

startedX=0
if ! DISPLAY="${display}" xdpyinfo >/dev/null 2>&1; then
	Xvfb "${display}" -screen 0 1280x720x24 -nolisten tcp >/dev/null 2>&1 &
	startedX=$!
	for _ in $(seq 1 40); do DISPLAY="${display}" xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done
fi
##	Kill by pid, never by pattern - a pattern here also matches this script.
fCleanup() { [[ -n "${rdpPid:-}" ]] && kill "${rdpPid}" 2>/dev/null; ((startedX)) && kill "${startedX}" 2>/dev/null; true; }
trap fCleanup EXIT

bad=0
for row in "${rows[@]}"; do
	read -r name addr <<<"${row}"
	echo "== ${name} (${addr})"
	##	The password is empty on purpose: this is a throwaway test account, and the
	##	box only allows it because blank-password logon is not restricted there.
	DISPLAY="${display}" sdl-freerdp "/v:${addr}" "/u:${account}" /p: /cert:ignore /w:1280 /h:720 \
		>/dev/null 2>&1 &
	rdpPid=$!
	sleep 20
	kill "${rdpPid}" 2>/dev/null || true
	unset rdpPid
	"${winRemote}" --host "${name}" --optional job console "${account}" || bad=1
done
((bad == 0))

##	Script history:
##		- 20260908: Created.
##		- 20260910: holds the box across the RDP logon and the console move.
