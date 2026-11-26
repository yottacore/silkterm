#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display, twice, on screen and
##		unfocused, with "Minutes otherwise" at its least (1) and "Minutes when
##		hidden" at an hour. Each window runs tmux on a socket of its own, with
##		a clock in the status line that tmux redraws every 5 s. Nothing is
##		typed into either.
##			- In the "clock" window the shell inside tmux is quiet, so the
##			  status line is the only thing that changes. The window lets its
##			  device go after the minute anyway, not well before it, and a tick
##			  after that leaves it let go.
##			- In the "busy" window the shell inside tmux prints a line every
##			  10 s, which still counts as use, so that window keeps its device.
##		The wait is the setting's own, read from the file, as in idleother.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  tmux, dash, xdotool, or xprop).
##	- Test ID: dfj7n
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
for tool in Xvfb xdotool xprop dash tmux; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi
dash="$(command -v dash)"
tmuxBin="$(command -v tmux)"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/idletmux.XXXXXX")"
clockPid=""; busyPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${clockPid}" ]]; then fStopOurs "${clockPid}"; fi
	if [[ -n "${busyPid}" ]]; then fStopOurs "${busyPid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
	local name; for name in clock busy; do
		if [[ -S "${work}/${name}/sock" ]]; then "${tmuxBin}" -S "${work}/${name}/sock" kill-server >/dev/null 2>&1 || true; fi
	done
	local pid; for pid in $(fLeftovers); do kill "${pid}" 2>/dev/null || true; done
	if [[ "${rc}" == 0 ]]; then fTestDir_Rm "${work}" || true; fi
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

## No window manager on purpose: winit reports the window unfocused at once,
## and nothing ever focuses it, minimizes it, or covers it.
export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-:98}"
display="${CICD_HEADLESS_DISPLAY}"
auth="/tmp/cicd-gui-headless-${USER:-$(id -un)}/Xauthority-${display#:}"
status="$("${headless}" status 2>/dev/null || true)"
if [[ "${status}" == *"no Xvfb"* ]]; then
	if ! "${headless}" start >/dev/null 2>&1; then echo "  skip: the display on ${display} did not start"; exit 3; fi
	startedDisplay=1
fi

## No blink, wallpaper, or minimap, so nothing but the idle release changes the
## window, and no rating run to put the release off.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' \
	'window:' $'\tremember_per_monitor: false' $'\tidle_release: true' $'\tidle_release_hidden_min: 60' $'\tidle_release_min: 1' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: false' >"${work}/config.shcl"
## tmux's own default redraws its clock once a minute, which a one-minute wait
## would race. A seconds clock every 5 s is the same redraw, more often.
printf '%s\n' 'set -g status-interval 5' "set -g status-right '%H:%M:%S'" >"${work}/tmux.conf"
printf '%s\n' 'while :; do sleep 300; done' >"${work}/quiet.sh"
printf '%s\n' 'while :; do sleep 10; echo tick; done' >"${work}/ticks.sh"

## One window, with a home, a config and a tmux socket of its own. Its title
## lines go to `<name>.titles`, its stderr to `<name>.said`. The script named
## runs in the tmux session.
fLaunch(){
	local -r name="${1}" inner="${2}"
	local -r home="${work}/${name}"
	mkdir -p "${home}"
	cp "${work}/config.shcl" "${home}/config.shcl"
	printf '%s\n' "exec '${tmuxBin}' -S '${home}/sock' -f '${work}/tmux.conf' new-session '${dash} ${inner}'" >"${home}/scene.sh"
	DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u TMUX -u TMUX_PANE -u TMUX_TMPDIR -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET -u SILK_IDLE_SECS \
		LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" SILK_IDLEDBG=1 TERM=xterm-256color \
		"${bin}" --config "${home}/config.shcl" --pixel-width 700 --pixel-height 400 --shell "${dash} ${home}/scene.sh" >/dev/null 2>"${work}/${name}.said" &
}
fLaunch clock "${work}/quiet.sh"
clockPid=$!
fLaunch busy "${work}/ticks.sh"
busyPid=$!
launched=${SECONDS}

declare -A win=([clock]="" [busy]="") releasedAt=([clock]="" [busy]="") seen=([clock]=0 [busy]=0)
declare -A pidOf=([clock]="${clockPid}" [busy]="${busyPid}")
fTitle(){ DISPLAY="${display}" XAUTHORITY="${auth}" xprop -id "${win[${1}]}" _NET_WM_NAME 2>/dev/null || true; }
giveUp=$((launched + 100))
while ((SECONDS < giveUp)); do
	for name in clock busy; do
		[[ -n "${releasedAt[${name}]}" ]] && continue
		if [[ -z "${win[${name}]}" ]]; then
			win[${name}]="$(DISPLAY="${display}" XAUTHORITY="${auth}" xdotool search --all --pid "${pidOf[${name}]}" --name 'SilkTerm' 2>/dev/null | head -n 1 || true)"
			## side by side, since with no window manager both open at the
			## same corner and one would cover the other
			if [[ "${name}" == busy && -n "${win[${name}]}" ]]; then DISPLAY="${display}" XAUTHORITY="${auth}" xdotool windowmove "${win[${name}]}" 0 500 2>/dev/null || true; fi
		fi
		[[ -n "${win[${name}]}" ]] || continue
		title="$(fTitle "${name}")"
		echo "$((SECONDS - launched)) ${title#*= }" >>"${work}/${name}.titles"
		## a window found before the real one maps can go away, so look again
		if [[ "${title}" != *"="* ]]; then win[${name}]=""; continue; fi
		seen[${name}]=1
		if [[ "${title}" == *"(resource conservation mode)"* ]]; then releasedAt[${name}]=$((SECONDS - launched)); fi
	done
	## the busy window is meant to hold on, so the clock's release ends the wait
	[[ -n "${releasedAt[clock]}" ]] && ((SECONDS - launched >= 75)) && break
	sleep 0.5
done

## Two ticks of the clock after the release, which have to leave it let go.
staysAfterTick=0
if [[ -n "${releasedAt[clock]}" ]]; then
	staysAfterTick=1
	for _ in {1..24}; do [[ "$(fTitle clock)" == *"(resource conservation mode)"* ]] || { staysAfterTick=0; break; }; sleep 0.5; done
fi
fStopOurs "${clockPid}"; clockPid=""
fStopOurs "${busyPid}"; busyPid=""

ticks="$(grep -c 'status line' "${work}/clock.said" || true)"
fNotEarly(){ [[ -n "${releasedAt[clock]}" ]] && ((releasedAt[clock] >= 45)); }
fCheck "both windows came up" test "${seen[clock]}${seen[busy]}" = 11
fCheck "tmux redrew its status line during the wait (said ${ticks} times)" test "${ticks}" -ge 1
fCheck "a window showing only a tmux clock lets its device go after Minutes otherwise" test -n "${releasedAt[clock]}"
fCheck "and not well before the minute (${releasedAt[clock]:-never} s)" fNotEarly
fCheck "a tick of the clock after the release leaves it let go" test "${staysAfterTick}" = 1
fCheck "a window whose shell prints inside tmux keeps its device" test -z "${releasedAt[busy]}"

if ((failures)); then
	echo "  ${failures} check(s) failed; work dir kept: ${work}"
	for name in clock busy; do
		echo "  ${name} said:"; sed 's/^/    /' "${work}/${name}.said" 2>/dev/null | tail -n 15 || true
	done
	exit 1
fi
echo "all passed"

##	History:
##		- 20261120 JC: Created.
