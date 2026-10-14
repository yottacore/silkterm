#!/usr/bin/env bash

##	- Purpose:
##		Launches the real program on the private display with the minimap on,
##		running a scene that starts programs which draw their own screen, and
##		reads the pane's width from inside it. The column comes and goes with
##		what runs, at the time it should.
##			- A program that goes to the alt screen and then sits still, its
##			  cursor hidden, loses the column while it sits there. It used to
##			  wait for some later redraw, and then took it away from whatever
##			  came after.
##			- A listed program keeps the column from start to exit. tmux used to
##			  lose it for a moment as it started.
##			- A program asking for focus reports on the main screen loses it
##			  while it runs, and the shell gets it back.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, display,
##	  dash or xdotool).
##	- Test ID: EsdlZTW
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
for tool in Xvfb xdotool dash stty; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -x "${headless}" ]]; then echo "  skip: no gui-headless.bash"; exit 3; fi
dash="$(command -v dash)"

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/mapfollow.XXXXXX")"
appPid=""; startedDisplay=0
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	if ((startedDisplay)); then "${headless}" stop >/dev/null 2>&1 || true; fi
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
## No blink and no wallpaper, so a frame comes only from what the scene does.
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'window:' $'\tremember_per_monitor: false' \
	'cursor:' $'\tblink: false' 'wallpaper:' $'\tenabled: false' 'scroll:' $'\tminimap:' $'\t\tenabled: true' $'\t\ttui_process_whitelist: "less tmux screen"' >"${work}/config.shcl"

## The programs are dash under other names, since the name is what the column
## goes by. The scene runs them under job control, as a shell at a prompt
## would, so the terminal can tell what runs.
for name in tmux chat; do ln -s "${dash}" "${work}/bin/${name}"; done
## Each line of sizes.txt is the step the scene was on and the pane's rows and
## columns, written when they change.
cat >"${work}/scene.sh" <<'EOF'
w=$1
echo start >"$w/step"
poll() { last=; while :; do s=$(stty size </dev/tty); if [ "$s" != "$last" ]; then echo "$(cat "$w/step") $s" >>"$w/sizes.txt"; last=$s; fi; sleep 0.02; done; }
poll &
step() { echo "$1" >"$w/step"; }
sleep 3
step quiet
printf '\033[?25l\033[?1049h\033[Ha dialog'
sleep 3
printf '\033[?1049l\033[?25h'
step shell; sleep 2
set -m
step tmux; printf .
"$w/bin/tmux" -c 'printf "\033[?1049h\033[Hpanes"; sleep 3; printf "\033[?1049l"'
step back; sleep 2
step chat; printf .
"$w/bin/chat" -c 'printf "\033[?1004h> "; sleep 3; printf "\033[?1004l"'
step end; sleep 2
step done
sleep 300
EOF

DISPLAY="${display}" XAUTHORITY="${auth}" env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u XDG_CONFIG_HOME -u XDG_CACHE_HOME -u XDG_DATA_HOME -u SILKTERM_SOCKET \
	LIBGL_ALWAYS_SOFTWARE=1 HOME="${home}" XDG_RUNTIME_DIR="${home}" \
	"${bin}" --config "${work}/config.shcl" --pixel-width 1000 --pixel-height 600 --shell "${dash} ${work}/scene.sh ${work}" >/dev/null 2>"${work}/said.txt" &
appPid=$!

giveUp=$((SECONDS + 60))
while ((SECONDS < giveUp)) && [[ "$(cat "${work}/step" 2>/dev/null || true)" != "done" ]]; do sleep 0.5; done
fStopOurs "${appPid}"; appPid=""
## A line with no size is the poll losing the terminal as the window closed.
sizes="${work}/sized.txt"
awk 'NF == 3' "${work}/sizes.txt" >"${sizes}" 2>/dev/null || true
fCheck "the scene ran to the end" grep -qx "done" "${work}/step"

## The width the shell had at the start, with the column showing.
narrow="$(grep '^start ' "${sizes}" 2>/dev/null | tail -n 1 | awk '{print $3}')"
fWide(){ awk -v step="${1}" -v narrow="${narrow:-0}" '$1 == step && $3 > narrow {found = 1} END {exit !found}' "${sizes}"; }
fNarrow(){ ! fWide "${1}"; }
fNone(){ ! grep -q "^${1} " "${sizes}"; }
fEndsNarrow(){ [[ "$(tail -n 1 "${sizes}" | awk '{print $3}')" == "${narrow}" ]]; }
fCheck "the column shows at the start" test -n "${narrow}"
fCheck "a quiet program on the alt screen takes it" fWide quiet
fCheck "and the shell after it has it" fNarrow shell
fCheck "a listed program keeps it as it starts" fNone tmux
fCheck "and as it exits" fNone back
fCheck "a program asking for focus reports takes it" fWide chat
fCheck "and the shell gets it back" fEndsNarrow

if ((failures)); then
	echo "  sizes:"; sed 's/^/      /' "${sizes}" 2>/dev/null || true
	echo "${failures} failed"; exit 1
fi
echo "all passed"

##	History:
##		- 20261013 JC: Created.
