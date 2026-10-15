#!/usr/bin/env bash

##	- Purpose:
##		The clipboard and the primary selection on native Wayland. Runs the
##		real program on headless sway, once with no DISPLAY and once with
##		DISPLAY naming a stand-in X server that only counts who calls. Each
##		time:
##			- A program in the pane sets both with OSC 52, and a Wayland
##			  reader outside the window gets both.
##			- Text set outside goes in with Ctrl+Shift+V, and the primary
##			  with a middle click.
##			- A double-click on a word puts it in the primary.
##			- The window closes cleanly when its shell exits.
##		With DISPLAY set, the window must never call the X server.
##	- Syntax: run.bash [--bin PATH]   (default: the debug build, then release)
##	- Exit: 0 passed, 1 a check failed, 3 nothing ran (no binary, sway,
##	  wtype, python3 or software Vulkan).
##	- Test ID: EsnbWa2
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
lvp="/usr/share/vulkan/icd.d/lvp_icd.json"

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
for tool in sway swaymsg wtype python3 base64; do
	if ! command -v "${tool}" >/dev/null 2>&1; then echo "  skip: no ${tool}"; exit 3; fi
done
if [[ ! -f "${lvp}" ]]; then echo "  skip: no software Vulkan (${lvp})"; exit 3; fi

# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${SILKTERM_TEST_DIR}/wlclip.XXXXXX")"
## A socket path has to fit in 108 bytes.
runDir="${work}/run"
shortRun=""
if ((${#runDir} > 70)); then shortRun="$(mktemp -d /tmp/silkclip.XXXXXX)"; runDir="${shortRun}"; else mkdir -m 700 "${runDir}"; fi
appPid=""; swayPid=""; ptrPid=""; kbdPid=""; xPid=""; setPid=""
fCleanup(){
	local -r rc="${1}"
	if [[ -n "${appPid}" ]]; then fStopOurs "${appPid}"; fi
	local pid
	for pid in "${setPid}" "${kbdPid}" "${xPid}"; do
		if [[ -n "${pid}" ]]; then kill "${pid}" 2>/dev/null || true; wait "${pid}" 2>/dev/null || true; fi
	done
	if [[ -n "${ptrPid}" ]]; then exec 7>&- || true; kill "${ptrPid}" 2>/dev/null || true; wait "${ptrPid}" 2>/dev/null || true; fi
	if [[ -n "${swayPid}" ]]; then kill "${swayPid}" 2>/dev/null || true; wait "${swayPid}" 2>/dev/null || true; fi
	if [[ -n "${shortRun}" && "${shortRun}" == /tmp/silkclip.* ]]; then rm -rf "${shortRun}"; fi
	if ((rc == 0)); then rm -rf "${work}"; fi
	fTestDir_End "${rc}"
}
trap 'fCleanup $?' EXIT

## Only the process launched here, and only while it is still this binary: the
## repo path says silkterm, and a dogfood copy may be running.
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

## No Xwayland: nothing here may lean on one.
cat >"${work}/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1280x800 position 0 0 scale 1
xwayland disable
EOF
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SWAYSOCK XDG_RUNTIME_DIR="${runDir}" \
	WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
	sway -c "${work}/sway.conf" >"${work}/sway.log" 2>&1 &
swayPid=$!
sock=""
for _ in {1..50}; do
	sock="$(find "${runDir}" -maxdepth 1 -name 'sway-ipc.*.sock' -print -quit 2>/dev/null || true)"
	[[ -n "${sock}" && -S "${runDir}/wayland-1" ]] && break
	sleep 0.1
done
if [[ -z "${sock}" ]]; then echo "  skip: sway did not start"; exit 3; fi
wl=(env -u DISPLAY XDG_RUNTIME_DIR="${runDir}" WAYLAND_DISPLAY=wayland-1 SWAYSOCK="${sock}")
fClip(){ "${wl[@]}" python3 -I "${meDir}/_wlclip.py" "${@}"; }
probe=0; fClip get clipboard >/dev/null 2>"${work}/clip.err" || probe=$?
if ((probe != 0 && probe != 4)); then echo "  skip: no data control: $(cat "${work}/clip.err")"; exit 3; fi

## The seat has no devices. A keyboard held for the whole run, since a
## selection belongs to the client with keyboard focus, and a pointer.
"${wl[@]}" wtype -s 600000 &
kbdPid=$!
mkfifo "${work}/ptr.in"
"${wl[@]}" python3 -I "${meDir}/../monwl/_vpointer.py" 1280 800 <"${work}/ptr.in" >"${work}/ptr.out" 2>"${work}/ptr.err" &
ptrPid=$!
exec 7>"${work}/ptr.in"
for _ in {1..50}; do grep -q '^ready$' "${work}/ptr.out" 2>/dev/null && break; sleep 0.1; done
if ! grep -q '^ready$' "${work}/ptr.out"; then echo "  skip: no virtual pointer: $(cat "${work}/ptr.err")"; exit 3; fi
fPtr(){  ## fPtr <command...>, back once the compositor has it
	local -i before; before="$(grep -c '^ok$' "${work}/ptr.out" || true)"
	echo "${*}" >&7
	local _
	for _ in {1..50}; do (($(grep -c '^ok$' "${work}/ptr.out" || true) > before)) && return 0; sleep 0.02; done
	echo "  the pointer did not answer: ${*}"; return 1
}

## A stand-in X server on an unused display: it takes each call, writes a line,
## and hangs up, so a client gives up at once.
xNum=""
mkfifo "${work}/x.ready"
python3 -I -c '
import os, socket, sys
ready = open(sys.argv[1], "w")
for num in range(4200, 4300):
	if os.path.exists(f"/tmp/.X11-unix/X{num}"):
		continue
	server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
	try:
		server.bind(f"\0/tmp/.X11-unix/X{num}")
	except OSError:
		continue
	server.listen(8)
	break
else:
	ready.close()
	sys.exit(1)
ready.write(f"{num}\n")
ready.close()
while True:
	conn, _ = server.accept()
	conn.close()
	with open(sys.argv[2], "a") as log:
		log.write("call\n")
' "${work}/x.ready" "${work}/x.calls" &
xPid=$!
read -r xNum <"${work}/x.ready" || true
if [[ -z "${xNum}" ]]; then echo "  skip: no free display number for the stand-in X server"; exit 3; fi

cfgHome="${work}/home/.config"
mkdir -p "${cfgHome}/silkterm"
printf '%s\n' 'performance:' $'\tautomatic: false' $'\tprofile: "custom"' 'wallpaper:' $'\tenabled: false' \
	>"${cfgHome}/silkterm/config.shcl"

## Sets both selections when told, reads 2 pasted lines, fills the screen with
## one long word, and exits when told.
cat >"${work}/scene.sh" <<'EOF'
dir="$1"
: >"$dir/up"
while [ ! -e "$dir/go" ]; do sleep 0.1; done
printf '\033]52;c;%s\a' "$(printf 'osc-clipboard' | base64)"
printf '\033]52;p;%s\a' "$(printf 'osc-primary' | base64)"
IFS= read -r line; printf '%s\n' "$line" >"$dir/paste1"
IFS= read -r line; printf '%s\n' "$line" >"$dir/paste2"
set -- $(stty size)
word="$(head -c "$(($2 - 1))" </dev/zero | tr '\0' q)"
printf '\033[H\033[2J'
i=1; while [ "$i" -lt "$1" ]; do printf '%s\n' "$word"; i=$((i + 1)); done
: >"$dir/filled"
while [ ! -e "$dir/quit" ]; do sleep 0.1; done
EOF

fWaitFile(){ local _; for _ in $(seq "${2}"); do [[ -e "${1}" ]] && return 0; sleep 0.1; done; return 1; }  ## fWaitFile <path> <tenths>
fHasClip(){  ## fHasClip <clipboard|primary> <want>, within 5 s
	local _ got
	for _ in {1..25}; do
		got="$(fClip get "${1}" 2>/dev/null || true)"
		[[ "${got}" == "${2}" ]] && return 0
		sleep 0.2
	done
	echo "    ${1}: ${got:-nothing}"; return 1
}
fSaid(){ [[ "$(cat "${1}" 2>/dev/null || true)" == "${2}" ]] || { echo "    got: $(cat "${1}" 2>/dev/null || echo nothing)"; return 1; }; }
fQrun(){  ## the primary is a run of q, within 5 s
	local _ got
	for _ in {1..25}; do
		got="$(fClip get primary 2>/dev/null || true)"
		[[ "${got}" =~ ^q{20,}$ ]] && return 0
		sleep 0.2
	done
	echo "    primary: ${got:0:40}"; return 1
}
fKeys(){ "${wl[@]}" wtype -s 300 "${@}"; }

fPass(){  ## fPass <folder> <name> [DISPLAY]
	local -r name="${2}" display="${3:-}"
	local -r dir="${work}/${1}"
	mkdir -p "${dir}"
	local -a appEnv=("${wl[@]}")
	[[ -n "${display}" ]] && appEnv+=(DISPLAY="${display}")
	: >"${work}/x.calls"
	## Mesa's device select layer asks an X server which card is the default
	## when Wayland can't say. Its own call, not the program's, and there is
	## one card here.
	"${appEnv[@]}" NODEVICE_SELECT=1 LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES="${lvp}" \
		VK_LAYER_MESSAGE_ID_FILTER=VUID-vkCmdCopyBufferToImage-pRegions-00173 \
		HOME="${work}/home" XDG_CONFIG_HOME="${cfgHome}" XDG_DATA_HOME="${work}/home/.local/share" \
		XDG_CACHE_HOME="${work}/home/.cache" XDG_STATE_HOME="${work}/home/.local/state" \
		"${bin}" --shell "/bin/dash ${work}/scene.sh ${dir}" >/dev/null 2>"${dir}/app.err" &
	appPid=$!
	if ! fWaitFile "${dir}/up" 200; then echo "  FAIL ${name}: the window never started its shell"; failures=$((failures + 1)); return; fi
	## The keyboard has to reach the window before it may set a selection.
	sleep 1.5
	: >"${dir}/go"
	fCheck "${name}: OSC 52 sets the clipboard" fHasClip clipboard osc-clipboard
	fCheck "${name}: OSC 52 sets the primary" fHasClip primary osc-primary

	"${wl[@]}" python3 -I "${meDir}/_wlclip.py" set clipboard "outside-clipboard" >"${dir}/set.out" 2>&1 &
	setPid=$!
	fWaitFile "${dir}/set.out" 30 || true; sleep 0.3
	fKeys -M ctrl -M shift -k v -m shift -m ctrl -k Return
	fWaitFile "${dir}/paste1" 30 || true
	fCheck "${name}: Ctrl+Shift+V pastes the clipboard" fSaid "${dir}/paste1" "outside-clipboard"
	kill "${setPid}" 2>/dev/null || true; wait "${setPid}" 2>/dev/null || true

	"${wl[@]}" python3 -I "${meDir}/_wlclip.py" set primary "outside-primary" >"${dir}/set.out" 2>&1 &
	setPid=$!
	sleep 0.5
	fPtr move 640 400
	fPtr middle
	sleep 0.3
	fKeys -k Return
	fWaitFile "${dir}/paste2" 30 || true
	fCheck "${name}: a middle click pastes the primary" fSaid "${dir}/paste2" "outside-primary"
	kill "${setPid}" 2>/dev/null || true; wait "${setPid}" 2>/dev/null || true; setPid=""

	fWaitFile "${dir}/filled" 30 || true
	sleep 0.5
	fPtr down; fPtr up; fPtr down; fPtr up
	fCheck "${name}: a double-click puts the word in the primary" fQrun

	: >"${dir}/quit"
	local rc=0 _
	for _ in {1..50}; do kill -0 "${appPid}" 2>/dev/null || break; sleep 0.1; done
	if kill -0 "${appPid}" 2>/dev/null; then
		fCheck "${name}: closes when its shell exits" false
		fStopOurs "${appPid}"
	else
		wait "${appPid}" || rc=$?
		fCheck "${name}: closes cleanly when its shell exits (status ${rc})" test "${rc}" == 0
	fi
	appPid=""
	if [[ -n "${display}" ]]; then
		local -i calls; calls="$(grep -c . "${work}/x.calls" || true)"
		fCheck "${name}: never calls the X server (${calls} calls)" test "${calls}" == 0
	fi
}

fPass nodisplay "no DISPLAY"
fPass display "DISPLAY set" ":${xNum}"

if ((failures)); then echo "  ${failures} check(s) failed; files kept in ${work}"; exit 1; fi
exit 0

##	History:
##		- 20261015 JC: Created.
