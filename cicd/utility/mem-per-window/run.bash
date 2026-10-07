#!/usr/bin/env bash
# What one window holds on the graphics card and in memory, on the real GPU
# without touching the desktop. Starts its own headless sway on the NVIDIA
# render node and opens one window in it, either as an X11 client through
# sway's Xwayland (the GL path every X11 desktop takes) or as a native Wayland
# client (Vulkan, with the allocator's own per-texture report).
#
#	run.bash <silkterm-binary> gl|vulkan <W>x<H> [options]
#		--no-halo          scrim halo off (the outline still uses the scrim pass)
#		--no-scrim         halo and outline off, so the scrim pass has nothing to draw
#		--no-wallpaper
#		--wallpaper FILE   instead of the built-in one
#		--blur SIGMA       the wallpaper's blur, in its own pixels (whole numbers)
#		--fill             fill the scrollback (12000 lines) before sampling
#		--no-minimap
#		--settings         open Settings once and sample with it open and closed (gl only)
#		--opens N          open and close Settings N times, printing how long each
#		                   took to draw (gl only; implies --settings)
#		--flood SECS       scroll output for SECS, then print the frame counts and
#		                   render times (SILK_PERF) instead of sampling memory
#		--settle N         seconds before the first sample (default 15)
#
# Prints the process's graphics memory as the driver bills it (nvidia-smi), its
# unique resident footprint less the driver's libraries (sizebench-classify.py),
# its anonymous resident memory, and the window's own SILK_MEMDBG lines. Use an
# optimized binary. Test files go in the run's test_silkterm_<stamp> folder
# (cicd/tests/_testdir.bash).

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -uo pipefail

scriptDir="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"; readonly scriptDir
repoDir="$(cd "${scriptDir}/../../.." && pwd)"; readonly repoDir
# shellcheck source=cicd/tests/_testdir.bash
source "${repoDir}/cicd/tests/_testdir.bash"

swayPid="" appPid="" swayRun="" xDisplay="" rc=0

fMain(){
	local -r binary="${1:?binary}" mode="${2:?gl|vulkan}" size="${3:?WxH}"
	shift 3
	local scrim=true outline=1.0 wallpaper="" noWallpaper=false scene=idle minimap=true settings=false blur=""
	local -i settle=15 opens=1 flood=0
	while (($#)); do
		case "${1}" in
			--no-halo)      scrim=false ;;
			--no-scrim)     scrim=false; outline=0.0 ;;
			--no-wallpaper) noWallpaper=true ;;
			--wallpaper)    wallpaper="$(readlink -f "${2:?file}")"; shift ;;
			--blur)         [[ "${2:-}" =~ ^[0-9]+$ ]] || { echo "--blur takes a whole number" >&2; return 2; }; blur="${2}"; shift ;;
			--fill)         scene=fill ;;
			--no-minimap)   minimap=false ;;
			--settings)     settings=true ;;
			--opens)        fCount "${1}" "${2:-}" || return 2; settings=true; opens="${2}"; shift ;;
			--flood)        fCount "${1}" "${2:-}" || return 2; flood="${2}"; scene=flood; shift ;;
			--settle)       fCount "${1}" "${2:-}" || return 2; settle="${2}"; shift ;;
			*) echo "unknown option: ${1}" >&2; return 2 ;;
		esac
		shift
	done
	[[ "${mode}" == gl || "${mode}" == vulkan ]] || { echo "mode is gl or vulkan" >&2; return 2; }
	[[ "${size}" =~ ^([0-9]+)x([0-9]+)$ ]] || { echo "size is WxH" >&2; return 2; }
	local -r width="${BASH_REMATCH[1]}" height="${BASH_REMATCH[2]}"
	command -v nvidia-smi sway swaymsg >/dev/null || { echo "needs nvidia-smi and sway" >&2; return 1; }
	fGpuHasRoom || return 1
	fTestDir_Make || return 1
	trap 'rc=$?; fStop; fTestDir_End "${rc}"' EXIT
	local -r work="${SILKTERM_TEST_DIR}/mem-per-window"
	mkdir -p "${work}/cfg/silkterm" "${work}/data"
	fWriteConfig "${work}/cfg/silkterm/config.shcl" "${scrim}" "${outline}" "${noWallpaper}" "${wallpaper}" "${minimap}" "${blur}"
	fWriteScene "${work}/scene.sh" "${scene}" "${flood}"
	fStartSway "${work}" "$((width + 40))x$((height + 120))" || return 1

	# its own cache folder, or the box's kept wallpapers are read and written
	local -a env=(env -u WAYLAND_DISPLAY -u DISPLAY XDG_CONFIG_HOME="${work}/cfg" XDG_CACHE_HOME="${work}/cache" XDG_DATA_HOME="${work}/data" XDG_RUNTIME_DIR="${swayRun}" SILK_MEMDBG=1 SILK_DLGDBG=1)
	((flood == 0)) || env+=(SILK_PERF=1)
	if [[ "${mode}" == gl ]]; then env+=(DISPLAY="${xDisplay}"); else env+=(WAYLAND_DISPLAY=wayland-1); fi
	"${env[@]}" "${binary}" --pixel-width "${width}" --pixel-height "${height}" --shell "/bin/dash ${work}/scene.sh" 2>"${work}/stderr.log" &
	appPid=$!
	if ((flood > 0)); then
		# the scene exits after the flood, and the window with it
		wait "${appPid}"
		appPid=""
		grep '^\[perf\]' "${work}/stderr.log"
		return 0
	fi
	sleep "${settle}"
	fSample "${work}" settled
	if [[ "${settings}" == true ]]; then
		[[ "${mode}" == gl ]] || { echo "--settings needs gl mode" >&2; return 2; }
		local -i open
		for ((open = 1; open <= opens; open++)); do fSettingsOnce "${work}" || return 1; done
		grep '^\[dlg\] Settings drawn' "${work}/stderr.log"
	fi
}

# Checked as text first, since a -i variable evaluates whatever it is given.
fCount(){
	[[ "${2}" =~ ^[1-9][0-9]*$ ]] || { echo "${1} takes a whole number above 0" >&2; return 1; }
}

# Graphics memory is shared with the desktop and whatever else is running. A
# nearly full card would make the window fail rather than measure it.
fGpuHasRoom(){
	local used total
	IFS=', ' read -r used total < <(nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader,nounits | head -1)
	echo "card: ${used} of ${total} MiB in use before the run"
	if ((used * 10 > total * 8)); then echo "the card is over 80% full; try later" >&2; return 1; fi
}

fWriteConfig(){
	local -r file="${1}" scrim="${2}" outline="${3}" noWallpaper="${4}" wallpaper="${5}" minimap="${6}" blur="${7}"
	{
		printf 'performance:\n\tautomatic: false\n\tprofile: custom\n\tcheck_hardware: false\n'
		printf 'transparency:\n\tenabled: false\n'
		printf 'window:\n\tidle_release: false\n'
		if [[ "${noWallpaper}" == true ]]; then
			printf 'wallpaper:\n\tenabled: false\n'
		elif [[ -n "${wallpaper}" || -n "${blur}" ]]; then
			printf 'wallpaper:\n\tenabled: true\n'
			if [[ -n "${wallpaper}" ]]; then printf '\timage: "%s"\n' "${wallpaper}"; fi
			if [[ -n "${blur}" ]]; then printf '\tblur: %s\n\thonor_xmp_look: false\n' "${blur}"; fi
		fi
		printf 'text:\n\tscrim:\n\t\tenabled: %s\n\toutline: %s\n' "${scrim}" "${outline}"
		printf 'scroll:\n\tminimap:\n\t\tenabled: %s\n' "${minimap}"
	} >"${file}"
}

fWriteScene(){
	local -r file="${1}" scene="${2}" seconds="${3}"
	if [[ "${scene}" == flood ]]; then
		cat >"${file}" <<EOF
sleep 3
end=\$((\$(date +%s) + ${seconds}))
while [ \$(date +%s) -lt \$end ]; do
	seq -f '%06g the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ' 1 2000
	sleep 0.05
done
EOF
	elif [[ "${scene}" == fill ]]; then
		cat >"${file}" <<'EOF'
i=0
while [ $i -lt 12000 ]; do
	printf '%06d the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz the quick brown fox jumps over the lazy dog\n' $i
	i=$((i + 1))
done
while :; do sleep 3600; done
EOF
	else
		echo 'while :; do sleep 3600; done' >"${file}"
	fi
}

# Every window floats at the size it asked for, since a tiled one would be
# sized by sway instead. The X display is whichever number sway's Xwayland
# takes, so it is read from the socket sway itself listens on.
fStartSway(){
	local -r work="${1}" output="${2}"
	local node=""
	for node in /sys/class/drm/renderD*; do
		[[ "$(cat "${node}/device/vendor" 2>/dev/null)" == 0x10de ]] && break
		node=""
	done
	[[ -n "${node}" ]] || { echo "no NVIDIA render node" >&2; return 1; }
	swayRun="$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/silkmem.XXXXXX")"
	printf 'output HEADLESS-1 resolution %s\nxwayland enable\ndefault_border none\nfor_window [all] floating enable\n' "${output}" >"${work}/sway.conf"
	env -u DISPLAY -u WAYLAND_DISPLAY XDG_RUNTIME_DIR="${swayRun}" WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 \
		WLR_RENDERER=gles2 WLR_RENDER_DRM_DEVICE="/dev/dri/${node##*/}" WLR_LIBINPUT_NO_DEVICES=1 \
		sway -c "${work}/sway.conf" >"${work}/sway.log" 2>&1 &
	swayPid=$!
	local -i tries
	for ((tries = 0; tries < 50; tries++)); do
		xDisplay="$(ss -xlpH 2>/dev/null | awk -v p="pid=${swayPid}," 'index($0, p) && $5 ~ /^\/tmp\/\.X11-unix\/X[0-9]+$/ {sub(/.*X/, ":", $5); print $5; exit}')"
		[[ -S "${swayRun}/wayland-1" && -n "${xDisplay}" ]] && break
		sleep 0.1
	done
	[[ -S "${swayRun}/wayland-1" && -n "${xDisplay}" ]] || { echo "sway did not come up; see ${work}/sway.log" >&2; return 1; }
}

fSample(){
	local -r work="${1}" tag="${2}"
	echo "== ${tag}"
	nvidia-smi -q -d PIDS | awk -v p="${appPid}" '/Process ID/ {on = ($4 == p)} on && /Used GPU Memory/ {print "graphics memory (nvidia-smi):", $5, $6}'
	python3 "${repoDir}/utility/include/sizebench-classify.py" --summary "${appPid}" 2>/dev/null \
		| awk '/^RESULT/ {for (i = 2; i <= NF; i++) if ($i ~ /^mem=/) print "unique footprint less driver libraries:", substr($i, 5), "MiB"}'
	# file pages come and go with the page cache, so this one is steadier between runs
	awk '/^RssAnon:/ {printf "anonymous resident: %.1f MiB\n", $2 / 1024}' "/proc/${appPid}/status"
	# the newest line per source
	grep '^memdbg ' "${work}/stderr.log" | awk '{key = ($2 == "pane") ? $2 $3 : $2; line[key] = $0; if (!(key in seen)) {seen[key] = 1; order[++n] = key}} END {for (i = 1; i <= n; i++) print line[order[i]]}'
}

# Ctrl+, opens Settings and Escape closes it, sent through XTEST to the
# focused window, which is the only kind of input winit takes.
fSettingsOnce(){
	local -r work="${1}"
	local win dialog
	win="$(DISPLAY="${xDisplay}" xdotool search --all --pid "${appPid}" --onlyvisible --name . 2>/dev/null | head -1)"
	DISPLAY="${xDisplay}" xdotool windowactivate --sync "${win}" 2>/dev/null
	sleep 0.5
	DISPLAY="${xDisplay}" xdotool key ctrl+comma
	sleep 6
	fSample "${work}" "settings open"
	dialog="$(DISPLAY="${xDisplay}" xdotool search --all --pid "${appPid}" --onlyvisible --name '^Settings$' | head -1)"
	[[ -n "${dialog}" ]] || { echo "Settings did not open" >&2; return 1; }
	DISPLAY="${xDisplay}" xdotool windowactivate --sync "${dialog}" 2>/dev/null
	sleep 0.3
	DISPLAY="${xDisplay}" xdotool key Escape
	sleep 4
	fSample "${work}" "settings closed"
}

fStop(){
	[[ -n "${appPid}" ]] && { kill "${appPid}" 2>/dev/null; wait "${appPid}" 2>/dev/null; }
	[[ -n "${swayPid}" ]] && { kill "${swayPid}" 2>/dev/null; wait "${swayPid}" 2>/dev/null; }
	# made by this run (mktemp above) and holding only sway's sockets
	[[ -n "${swayRun}" && -d "${swayRun}" ]] && rm -rf -- "${swayRun}"
	return 0
}

fMain "${@}"

##	History:
##		- 20261004 JC: Created.
##		- 20261004 JC: --opens and --flood.
##		- 20261007 JC: --blur, and a cache folder of its own.
