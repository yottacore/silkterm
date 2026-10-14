#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display with cursor blink off,
##		and runs quiet programs from a scene shell. Each prints nothing while it
##		runs, so nothing else asks for a frame. The window title follows the
##		tab, and it has to name each one while it runs.
##			- Each starts just after a frame was drawn, some at once and some a
##			  little later. The tab kept what it saw then for a short while, and
##			  with no later frame it never said the program's name.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  dash, xdotool or xprop).
##	- Test ID: EshV2SF
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
headless="${root}/cicd/utility/gui-headless.bash"

bin=""
while (($#)); do case "${1}" in
	--bin) bin="${2-}"; shift 2 ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	- History:/p' "${BASH_SOURCE[0]}" | sed 's/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
if [[ -z "${bin}" ]]; then
	targetDir="${CARGO_TARGET_DIR:-target}"
	[[ "${targetDir}" == /* ]] || targetDir="${root}/${targetDir}"
	for candidate in "${targetDir}/debug/silkterm" "${targetDir}/release/silkterm"; do
		if [[ -x "${candidate}" ]]; then bin="${candidate}"; break; fi
	done
fi
if [[ -z "${bin}" || ! -x "${bin}" ]]; then echo "  skip: no SilkTerm binary (build it first, or pass --bin)"; exit 3; fi
echo "  binary: ${bin}"
for tool in Xvfb xdotool xprop dash; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi
dash="$(command -v dash)"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/quiettab.XXXXXX")"
appPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	local pid; for pid in $(fLeftovers); do kill "${pid}" 2>/dev/null || true; done
	if [[ "${rc}" == 0 ]]; then rm -rf "${work}"; fi
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

## Only the process launched here, and only while it is still this binary.
fStopOurs(){
	local -r pid="${1}"
	local want exe
	want="$(realpath -e "${bin}" 2>/dev/null || true)"
	exe="$(realpath -e "/proc/${pid}/exe" 2>/dev/null || true)"
	if [[ -z "${want}" || "${exe}" != "${want}" ]]; then return 0; fi
	kill "${pid}" 2>/dev/null || true
	local _
	for _ in {1..20}; do kill -0 "${pid}" 2>/dev/null || return 0; sleep 0.1; done
	kill -9 "${pid}" 2>/dev/null || true
}

## Anything still running from this run's folder, by the folder in its command
## line. Builtins only, so no helper of ours turns up in the list.
fLeftovers(){
	local dir pid
	local -a args
	for dir in /proc/[0-9]*; do
		pid="${dir#/proc/}"
		[[ "${pid}" == "$$" || "${pid}" == "${BASHPID}" ]] && continue
		mapfile -d '' args 2>/dev/null <"${dir}/cmdline" || continue
		[[ "${args[*]}" == *"${work}/"* ]] && echo "${pid}"
	done
	return 0
}

export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi

home="${work}/home"
mkdir -p "${home}" "${work}/bin"
## No blink, no wallpaper and no minimap, so a frame comes only from what the
## scene prints.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' >"${work}/config.shcl"

## dash under other names, since the name is what the tab goes by. Each run
## has its own, so what ran last can't pass for what runs now.
delays=(0.05 0.1 0.2 0.4 0.8)
for i in "${!delays[@]}"; do ln -s "${dash}" "${work}/bin/hush${i}"; done
cat >"${work}/scene.sh" <<'EOF'
w=$1; shift
step() { echo "$1" >"$w/step"; }
sleep 3
set -m
i=0
for delay in "$@"; do
	step "gap$i"
	## a frame, drawn just before the program starts, with the shell in front
	printf .
	set +m; sleep "$delay"; set -m
	step "hush$i"
	"$w/bin/hush$i" -c 'sleep 2.5'
	i=$((i + 1))
done
step done
sleep 300
EOF

DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u TMUX -u TMUX_PANE -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET \
	LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" \
	"${bin}" --config "${work}/config.shcl" --pixel-width 900 --pixel-height 500 --shell "${dash} ${work}/scene.sh ${work} ${delays[*]}" >/dev/null 2>"${work}/said.txt" &
appPid=$!

## The window title, with the step the scene was on when it was read.
win=""
giveUp=$((SECONDS + 90))
while ((SECONDS < giveUp)); do
	step="$(cat "${work}/step" 2>/dev/null || true)"
	[[ "${step}" == "done" ]] && break
	if [[ -z "${win}" ]]; then
		win="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdotool search --pid "${appPid}" --name 'SilkTerm' 2>/dev/null | head -n 1 || true)"
	fi
	if [[ -n "${win}" && -n "${step}" ]]; then
		title="$(DISPLAY="${display}" XAUTHORITY="${auth}" xprop -id "${win}" _NET_WM_NAME 2>/dev/null || true)"
		echo "${step} ${title#*= }" >>"${work}/titles.txt"
	fi
	sleep 0.1
done
fStopOurs "${appPid}"; appPid=""

fCheck "the scene ran to the end" grep -qx "done" "${work}/step"
fNamed(){ grep -q "^${1} .*${1}" "${work}/titles.txt" 2>/dev/null; }
for i in "${!delays[@]}"; do
	fCheck "a quiet program started ${delays[${i}]} s after a frame gets named" fNamed "hush${i}"
done

if ((failures)); then
	echo "  titles:"; uniq "${work}/titles.txt" 2>/dev/null | sed 's/^/      /' || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261014 JC: Created.
