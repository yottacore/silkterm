#!/usr/bin/env bash

#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.'
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'
#  shellcheck disable=2329  ## 'This function is never invoked.' cleanup() runs from the trap.
#  shellcheck disable=2012  ## 'Use find instead of ls.' The wayland socket names are known-safe.
#  shellcheck disable=1091  ## 'Not following.' bench-common.bash is beside this script.

##	- Purpose:
##		Repeatable rig for the README terminal shootout. Brings up a private headless
##		Wayland compositor on the real GPU, launches one terminal as its only client,
##		fits every terminal to the same grid, and runs termbench.py inside it. A terminal
##		that draws only on X11 gets a private X server instead (x11Terms).
##
##		The rig matters more than it looks. Measured 20260730: software GL halves
##		SilkTerm (45 vs 88 MB/s on ascii) and VirtualGL still costs ~14%, while
##		CPU-rendered terminals do not move at all. A table built from mixed rigs can
##		therefore rank the wrong terminal first. Every published row comes from one rig.
##	- Syntax:
##		termbench-run.bash --term KEY [options]
##		   --term KEY      terminal to measure (--list for the known keys)
##		   --reps N        runs per scene (default 6; --reps is the only safe way to
##		                   shorten a run - see the note below)
##		   --grid CxR      grid every terminal is fitted to (default 160x42)
##		   --label TEXT    row name for the README table (default: autodetected)
##		   --scene NAME    one width class only (ascii|latin|cjk|emoji|mixed)
##		   --display :N    private X server for an X11-only terminal (default :98)
##		   --no-save       measure without recording or touching README.md
##		   --keep          leave the compositor up afterwards
##		   --list          list the known terminal keys and exit
##
##		Shortening a run: use --reps, never --scale. Fewer repetitions of the same
##		payloads leaves the measured rate directly comparable and only widens the
##		confidence interval. Shrinking the payload does not: Hyper reads 32 MB/s at
##		--scale 0.05 but ~3 MB/s at full size, a 10x difference. Watch the CV% column;
##		a run that got stepped on by other desktop activity shows up there.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Setup
##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

set -Eeuo pipefail

declare -r _here="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
declare -r _repo="$(cd "${_here}/../.." && pwd)"
declare -r _work="$(mktemp -d -t termbench-XXXXXX)"

source "${_here}/bench-common.bash"                                ## fEcho, fKillPids

declare -i _swayPid=0 _termPid=0 _xvfbPid=0
declare -i _keepRig=0

cleanup(){
	local -i rc=$?
	## The launched pid is the private session bus, with the terminal under it.
	if ((_termPid)); then
		local -a tree=()
		mapfile -t tree < <(fCollectTree ${_termPid})
		fKillPids "${tree[@]}"
	fi
	if ((_swayPid)) && ((!_keepRig)); then kill ${_swayPid} 2>/dev/null || true; fi
	if ((_xvfbPid)) && ((!_keepRig)); then kill ${_xvfbPid} 2>/dev/null || true; fi
	if ((!_keepRig)); then rm -rf "${_work}" 2>/dev/null || true; fi
	exit ${rc}
}
trap cleanup EXIT INT TERM


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Terminal recipes
##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

##	Every entry launches its terminal with SCENE as the shell/command, unstyled and
##	on a throwaway account (fPrivateAccount), so nothing personal reaches a published
##	run and nothing under the measuring account's home changes. Keys marked awkward
##	need a hook the terminal does not offer directly - see showdown-readme.md.
list_terms(){
	fEcho_Clean "  silkterm    this tree's release build, as shipped"
	fEcho_Clean "  silkplain   same binary, every optional effect off"
	fEcho_Clean "  alacritty   the VT core SilkTerm builds on, as its own terminal"
	fEcho_Clean "  kitty       needs terms/bin/kitty       (see showdown-readme.md)"
	fEcho_Clean "  wezterm     needs the AppImage extracted (see showdown-readme.md)"
	fEcho_Clean "  xfce4 gnome terminator                  (distro packages)"
	fEcho_Clean "  xterm       X11 only, so it runs on a private X server (see showdown-readme.md)"
	fEcho_Clean "  tabby       needs the AppImage extracted (see showdown-readme.md)"
	fEcho_Clean "  (no hyper: it never answers the barrier, so it cannot be timed)"
}

##	Alacritty reads the user's own config unless pointed elsewhere, and defaults TERM to
##	an entry that need not be installed. Neither matters to the measurement, so both are
##	pinned to something inert.
write_alacritty_config(){
	cat > "${_work}/alacritty.toml" <<-'EOF'
		[env]
		TERM = "xterm-256color"
	EOF
}

##	SilkTerm with every optional effect off. Only the overrides are written; the loader
##	backfills the rest, so this cannot go stale as new settings are added.
write_plain_config(){
	cp "${_here}/termbench-plain.shcl" "${_work}/plain.shcl"
}

##	SilkTerm as shipped, with the automatic profile pinned off so a slow renderer cannot
##	turn the effects down under the row that claims them.
write_candy_config(){
	cp "${_here}/termbench-candy.shcl" "${_work}/candy.shcl"
}

##	Tabby ignores SHELL and has no profile hook that takes, so the scene goes in through
##	the login shell it starts, which is bash here. With its Welcome tab on, the window
##	opens on that tab and no shell starts at all.
write_tabby_account(){
	local -r home="$1"
	local rc=""
	mkdir -p "${home}/.config/tabby"
	printf 'enableWelcomeTab: false\n' > "${home}/.config/tabby/config.yaml"
	for rc in .bashrc .bash_profile; do printf 'exec %s\n' "${sceneCmd}" > "${home}/${rc}"; done
}

##	Start a terminal on the throwaway account, with its output in term.log.
launch(){
	#  shellcheck disable=2154  ## Both arrays are filled by fPrivateAccount in bench-common.bash.
	env "${_privateEnv[@]}" "${_privateBus[@]}" "$@" > "${_work}/term.log" 2>&1 &
	_termPid=$!
}


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	The rig
##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

##	A headless sway on the real card. Headless means no monitor and no interference with
##	whatever is on the actual desktop, while still handing the client a native Vulkan
##	context on the discrete GPU - which is the whole point over Xvfb software GL.
start_rig(){
	command -v sway >/dev/null 2>&1 || fDie "sway is not installed - see showdown-readme.md"
	printf 'default_border none\ndefault_floating_border none\ngaps inner 0\ngaps outer 0\n' > "${_work}/sway.cfg"

	local runtimeDir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
	## Identify our socket by age, not by which names are new: a compositor that was
	## killed leaves its socket behind and the next one reuses the same name, so a
	## set-difference finds nothing. Nothing is deleted - a live session may own one.
	local -i startedAt=$(( $(date +%s) - 1 ))

	env -u DISPLAY WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 \
		sway -c "${_work}/sway.cfg" > "${_work}/sway.log" 2>&1 &
	_swayPid=$!

	## The ipc socket name is derived from our own pid, so this can never latch onto
	## somebody else's compositor.
	export SWAYSOCK="${runtimeDir}/sway-ipc.$(id -u).${_swayPid}.sock"
	local -i waited=0
	while ((waited < 100)); do
		[[ -S "${SWAYSOCK}" ]] && break
		kill -0 ${_swayPid} 2>/dev/null || fDie "sway exited during startup - see ${_work}/sway.log"
		sleep 0.2; waited+=1
	done
	[[ -S "${SWAYSOCK}" ]] || fDie "sway ipc socket never appeared"

	local candidate
	unset WAYLAND_DISPLAY
	waited=0
	while ((waited < 100)); do
		for candidate in "${runtimeDir}"/wayland-*; do
			[[ -S "${candidate}" ]] || continue
			local -i mtime=$(stat -c %Y "${candidate}" 2>/dev/null || echo 0)
			if ((mtime >= startedAt)); then export WAYLAND_DISPLAY="$(basename "${candidate}")"; fi
		done
		[[ -n "${WAYLAND_DISPLAY:-}" ]] && break
		sleep 0.2; waited+=1
	done
	[[ -n "${WAYLAND_DISPLAY:-}" ]] || fDie "no wayland socket appeared - see ${_work}/sway.log"

	unset DISPLAY
	export GDK_BACKEND=wayland
	fEcho "rig: sway pid ${_swayPid}, ${WAYLAND_DISPLAY}"
	## Never end a function on a bare 'cond && action': a false condition becomes the
	## function's return value and set -e kills the script. Bit this rig once already.
	local gpu="$(/usr/bin/grep -oiE 'DRM device[^,]*|renderer:.*' "${_work}/sway.log" 2>/dev/null | head -2 | tr '\n' ' ' || true)"
	if [[ -n "${gpu}" ]]; then fEcho_Clean "      ${gpu}"; fi
	return 0
}

##	The compositor tiles its only client to the whole output, so the grid is steered by
##	the output mode instead of by each terminal's own geometry flags - which is what
##	makes one fitter work for every terminal.
resize_output(){ swaymsg output HEADLESS-1 mode "${1}x${2}" >/dev/null 2>&1 || true; }

##	Terminals that draw only on X11 and are measured on an X server of their own rather
##	than through the compositor's Xwayland. xterm reads about a third slower through
##	Xwayland (18 MB/s of ASCII against 28 on an X server, 20260929), and its row came
##	from X11. It draws on the CPU, so a software X server costs it nothing.
declare -r x11Terms=" xterm "

##	A private X server on a number nobody else is using. One already there is refused
##	rather than reused, since it could be somebody else's.
start_x11(){
	command -v Xvfb >/dev/null 2>&1 || fDie "Xvfb is not installed (package xvfb)"
	if DISPLAY="${xDisplayNum}" xdpyinfo >/dev/null 2>&1 || [[ -e "/tmp/.X${xDisplayNum#:}-lock" ]]; then
		fDie "${xDisplayNum} is already in use; pick another with --display"
	fi
	Xvfb "${xDisplayNum}" -screen 0 2560x1600x24 -nolisten tcp > "${_work}/xvfb.log" 2>&1 &
	_xvfbPid=$!
	local -i waited=0
	while ((waited < 50)); do
		if DISPLAY="${xDisplayNum}" xdpyinfo >/dev/null 2>&1; then break; fi
		kill -0 ${_xvfbPid} 2>/dev/null || fDie "Xvfb exited during startup - see ${_work}/xvfb.log"
		sleep 0.2; waited+=1
	done
	## The lock names the server that took the number, which has to be ours.
	[[ "$(tr -d ' \n' < "/tmp/.X${xDisplayNum#:}-lock" 2>/dev/null || true)" == "${_xvfbPid}" ]] \
		|| fDie "${xDisplayNum} did not come up as ours"
	export DISPLAY="${xDisplayNum}" GDK_BACKEND=x11
	unset WAYLAND_DISPLAY
	fEcho "rig: Xvfb pid ${_xvfbPid}, ${DISPLAY}"
}

##	The terminal is started at the grid, and fFitGrid only confirms it.
resize_none(){ :; }

##	The X display of the compositor's own Xwayland, for WezTerm, which falls back to X11.
##	Only the compositor's children are told it, so it is asked for.
xwayland_display(){
	local -r file="${_work}/xdisplay"
	local -i waited=0
	swaymsg exec "printf '%s' \"\$DISPLAY\" > '${file}'" >/dev/null 2>&1 || true
	while ((waited < 40)); do
		if [[ -s "${file}" ]]; then cat "${file}"; return 0; fi
		sleep 0.25; waited+=1
	done
	return 1
}


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Arguments
##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

declare termKey="" label="" scene="" grid="160x42" xDisplayNum=":98"
declare -i reps=6 noSave=0

while (($#)); do
	case "$1" in
		--term)    termKey="${2:-}"; shift 2 ;;
		--reps)    reps="${2:-6}";   shift 2 ;;
		--grid)    grid="${2:-}";    shift 2 ;;
		--label)   label="${2:-}";   shift 2 ;;
		--scene)   scene="${2:-}";   shift 2 ;;
		--display) xDisplayNum="${2:-}"; shift 2 ;;
		--no-save) noSave=1;         shift ;;
		--keep)    _keepRig=1;       shift ;;
		--list)    list_terms; exit 0 ;;
		-h|--help) sed -n '/- Purpose:/,/^$/p' "${BASH_SOURCE[0]}" | sed 's/^##\t\?//'; exit 0 ;;
		*)         fDie "unknown option: $1" ;;
	esac
done

[[ -n "${termKey}" ]] || { fEcho "a terminal is required"; list_terms; exit 2; }
declare -i wantC="${grid%x*}" wantR="${grid#*x}"


##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##	Run
##•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

fSection "Terminal throughput: ${termKey}"

declare -r outFile="${_work}/out.txt"
declare -r sizeFile="${_work}/size"
declare -r goFile="${_work}/go"

declare benchArgs="--reps ${reps}"
if [[ -n "${scene}" ]]; then benchArgs+=" --scene ${scene}"; fi
if ((noSave)); then benchArgs+=" --no-save --no-readme"; fi

declare -r silkBin="$(fTargetDir "${_repo}")/release/silkterm"
case "${termKey}" in
	silkterm|silkplain) [[ -x "${silkBin}" ]] || fDie "no build at ${silkBin}" ;;
esac

declare resizeFn=resize_output
if [[ "${x11Terms}" == *" ${termKey} "* ]]; then
	start_x11
	resizeFn=resize_none
else
	start_rig
fi

## The scene script waits on the go file, reporting its grid meanwhile, so the fitter
## can settle the size before a single byte is measured.
export REPO_DIR="${_repo}" BENCH_ARGS="${benchArgs}" LABEL="${label}" \
       OUT_FILE="${outFile}" SIZE_FILE="${sizeFile}" GO_FILE="${goFile}"
declare -r sceneCmd="/bin/dash ${_here}/termbench-scene.sh"

fPrivateAccount "${_work}/home"

## Looked up here rather than inside the launch line, where a failed lookup only ends
## the substitution and the rig goes on to launch nothing.
declare termBin=""
case "${termKey}" in
	alacritty|kitty|wezterm|tabby)
		termBin="$(fFindTerm "${_repo}" "${termKey}")" || fDie "${termKey} not found - see showdown-readme.md" ;;
esac
declare xDisplay=""
case "${termKey}" in
	wezterm)
		xDisplay="$(xwayland_display)" || fDie "the compositor has no Xwayland display" ;;
esac

case "${termKey}" in
	silkterm)
		write_candy_config
		launch "${silkBin}" --config "${_work}/candy.shcl" --shell "${sceneCmd}" ;;
	silkplain)
		write_plain_config
		launch "${silkBin}" --config "${_work}/plain.shcl" --shell "${sceneCmd}" ;;
	alacritty)
		write_alacritty_config
		launch "${termBin}" --config-file "${_work}/alacritty.toml" -e ${sceneCmd} ;;
	kitty)
		launch "${termBin}" ${sceneCmd} ;;
	wezterm)
		## 20240203 falls back to X11 under sway 1.10 whatever enable_wayland says.
		launch env DISPLAY="${xDisplay}" "${termBin}" --config enable_wayland=true start --always-new-process -- ${sceneCmd} ;;
	tabby)
		write_tabby_account "${_work}/home"
		launch "${termBin}" --ozone-platform=wayland ;;
	xfce4)
		launch xfce4-terminal --disable-server -x ${sceneCmd} ;;
	gnome)
		## gnome-terminal never resizes with the compositor output, so it is the one
		## terminal that has to be told its geometry directly.
		launch gnome-terminal --wait --geometry=${wantC}x${wantR} -- ${sceneCmd} ;;
	terminator)
		launch terminator -e "${sceneCmd}" ;;
	xterm)
		launch xterm -geometry ${wantC}x${wantR} -e ${sceneCmd} ;;
	*)
		fDie "unknown terminal key: ${termKey} (--list)" ;;
esac
fEcho "launched pid ${_termPid}"

fFitGrid ${wantC} ${wantR} "${sizeFile}" ${resizeFn} 2438 1680
touch "${goFile}"

fEcho "measuring (${reps} runs per scene)"
declare -i waited=0
while ((waited < 900)); do
	[[ -f "${outFile}.done" ]] && break
	if ! kill -0 ${_termPid} 2>/dev/null; then
		sleep 3
		[[ -f "${outFile}.done" ]] || fDie "the terminal exited before finishing - see ${_work}/term.log"
		break
	fi
	sleep 2; waited+=1
done
[[ -f "${outFile}.done" ]] || fDie "timed out waiting for the run to finish"

fEcho_Clean
cat "${outFile}"

case "${termKey}" in
	silkterm)  fRequireCandyProfile "${_work}/candy.shcl" ;;
	silkplain) fEcho "SilkTerm profile in force: $(fSilkProfile "${_work}/plain.shcl")" ;;
esac

## A run whose scenes never answered the device-attributes query timed a timeout, not
## throughput, and must not reach the table.
if /usr/bin/grep -q "sync NONE" "${outFile}" 2>/dev/null; then
	fEcho "WARNING: this terminal never answered the barrier - the figures are not comparable"
fi

if ((_keepRig)); then
	if ((_xvfbPid)); then fEcho "rig left up: DISPLAY=${DISPLAY} (work: ${_work})"
	else fEcho "rig left up: SWAYSOCK=${SWAYSOCK} WAYLAND_DISPLAY=${WAYLAND_DISPLAY} (work: ${_work})"; fi
fi
exit 0


##	History:
##		- 20260730 JC: Created, from the scratch rig used for the first shootout table.
##		- 20260918 JC: Every terminal runs on a throwaway account and session bus; the +candy row pins its profile.
##		- 20260928 JC: Tabby entry. WezTerm gets the compositor's Xwayland display.
##		- 20260929 JC: xterm runs on a private X server, as its row was taken.
