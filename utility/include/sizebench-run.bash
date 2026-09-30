#!/usr/bin/env bash

#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.'
#  shellcheck disable=2329  ## 'This function is never invoked.' fCleanup() runs from the trap.
#  shellcheck disable=1091  ## 'Not following.' bench-common.bash is beside this script.

##	- Purpose:
##		Measure one terminal's install size and resident memory for the README shootout
##		table. Sizes it to the same grid as every other row on a private display, lets it
##		settle, then hands the whole process tree to sizebench-classify.py.
##
##		Reproduce a published row before trusting a new one. This rig reproduced
##		SilkTerm's memory within 1.3% and its excluded-driver figure within 0.4% of the
##		numbers already in the table; anything measured a different way is not comparable
##		with them. Window size is the trap - the same binary reads 38 MiB heavier at its
##		default geometry than at the table's 100x30 grid.
##	- Syntax:
##		sizebench-run.bash --term KEY [options]
##		   --term KEY      terminal to measure (--list for the known keys)
##		   --grid CxR      grid every terminal is fitted to (default 100x30)
##		   --settle N      seconds to let it finish starting (default 22)
##		   --verbose       itemize what was billed to the terminal and to the driver
##		   --keep          leave the working directory behind
##

set -Eeuo pipefail
shopt -s inherit_errexit

declare -r scriptDir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
declare -r repoDir="$(cd -- "${scriptDir}/../.." && pwd -P)"
declare -r termsDir="${repoDir}/cicd/artifacts/sizebench/terms"

declare -r display=":98"
declare -r screen="1920x1080x24"
declare -i settleSecs=22

declare _work=""
declare -i _xvfbPid=0 _rootPid=0
declare -ai _termPids=()
declare _winId=""

source "${scriptDir}/bench-common.bash"                            ## fEcho, fKillPids, fCollectTree

#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##  Teardown
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

fCleanup() {
	## The launched tree as well as the measured one: a private session bus sits above
	## the terminal and is not part of its figure.
	local -a launched=()
	if [[ -n "${_work}" ]]; then mapfile -t launched < <(fOwnedPids "${_work}/home" "${_rootPid}"); fi
	fKillPids "${_termPids[@]:-0}" "${launched[@]:-0}"
	((_xvfbPid > 0)) && kill "${_xvfbPid}" 2>/dev/null || true
	if [[ -n "${_work}" && -z "${optKeep:-}" && "${_work}" == "${TMPDIR:-/tmp}"/sizebench.* ]]; then
		rm -rf "${_work}" || true
	fi
	return 0
}
trap fCleanup EXIT

#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##  The terminals
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

## Each entry: key|binary|how it is told its grid. The keep-alive shell matters - a
## terminal whose child exits takes the window with it before anything can be measured.
fTermBinary() {
	local -r key="$1"
	local path=""
	case "${key}" in
		silkterm|silkplain) path="$(fTargetDir "${repoDir}")/release/silkterm" ;;
		alacritty)          path="$(command -v alacritty || true)"
		                    [[ -z "${path}" ]] && path="${termsDir}/usr/bin/alacritty" ;;
		xterm)              path="$(command -v xterm || true)" ;;
		kitty)              path="$(command -v kitty || true)" ;;
		xfce4)              path="$(command -v xfce4-terminal || true)" ;;
		terminator)         path="$(command -v terminator || true)" ;;
		## The server does the work; the gnome-terminal command only asks it for a window.
		gnome)              path="$(sed -n 's/^Exec=//p' /usr/share/dbus-1/services/org.gnome.Terminal.service 2>/dev/null || true)" ;;
		## The launcher hands over to wezterm-gui, which is the published file size.
		wezterm)            path="$(fFindTerm "${repoDir}" wezterm || true)"
		                    [[ -n "${path}" ]] && path="$(dirname "$(readlink -f "${path}")")/wezterm-gui" ;;
		tabby|hyper)        path="$(fFindTerm "${repoDir}" "${key}" || true)" ;;
		*)                  fDie "unknown terminal key '${key}' (try --list)" ;;
	esac
	[[ -x "${path}" ]] || fDie "no binary for '${key}' (looked at '${path:-nothing}')"
	printf '%s' "${path}"
}

##	What the terminals below run as their shell: it reports the grid until the go file
##	appears, then becomes the same idle sleep the other rows keep open.
fWriteHold() {
	cat > "${_work}/hold.sh" <<-EOF
		while [ ! -f '${_work}/go' ]; do
			stty size > '${_work}/size.tmp' 2>/dev/null && mv '${_work}/size.tmp' '${_work}/size'
			sleep 0.2
		done
		exec sleep 1000000
	EOF
}

##	Hyper reads its settings from here whenever XDG_CONFIG_HOME is set, and rewrites the
##	file at launch, so it is written new every run.
fWriteHyperConfig() {
	local -r home="$1"
	mkdir -p "${home}/.config/hyper"
	cat > "${home}/.config/hyper/.hyper.js" <<-EOF
		module.exports = {
			config: {
				updateChannel: 'stable',
				disableAutoUpdates: true,
				shell: '/bin/dash',
				shellArgs: ['${_work}/hold.sh'],
			},
			plugins: [],
			localPlugins: [],
		};
	EOF
}

fLaunch() {
	local -r key="$1" bin="$2" cols="$3" rows="$4"
	local -r keepAlive="/bin/dash -c 'exec sleep 1000000'"

	export DISPLAY="${display}"
	unset WAYLAND_DISPLAY
	export XDG_CONFIG_HOME="${_work}/xdg"
	mkdir -p "${XDG_CONFIG_HOME}"

	case "${key}" in
		silkplain)
			mkdir -p "${XDG_CONFIG_HOME}/silkterm"
			cp "${scriptDir}/termbench-plain.shcl" \
			   "${XDG_CONFIG_HOME}/silkterm/config.shcl"
			"${bin}" --columns "${cols}" --rows "${rows}" --shell "${keepAlive}" \
				>"${_work}/term.log" 2>&1 &
			;;
		silkterm)
			## Shipped settings with the profile pinned. On a fresh folder SilkTerm rates
			## this software display, picks Low, and the row is measured with the text
			## scrim and the cursor animation off.
			mkdir -p "${XDG_CONFIG_HOME}/silkterm"
			cp "${scriptDir}/termbench-candy.shcl" \
			   "${XDG_CONFIG_HOME}/silkterm/config.shcl"
			"${bin}" --columns "${cols}" --rows "${rows}" --shell "${keepAlive}" \
				>"${_work}/term.log" 2>&1 &
			;;
		alacritty)
			"${bin}" -o "window.dimensions.columns=${cols}" \
			         -o "window.dimensions.lines=${rows}" \
			         -e /bin/dash -c 'exec sleep 1000000' \
				>"${_work}/term.log" 2>&1 &
			;;
		xterm)
			## X toolkit option, one dash - '--geometry' is not accepted.
			"${bin}" -geometry "${cols}x${rows}" -e /bin/dash -c 'exec sleep 1000000' \
				>"${_work}/term.log" 2>&1 &
			;;
		kitty)
			"${bin}" -o "initial_window_width=${cols}c" -o "initial_window_height=${rows}c" \
				/bin/dash -c 'exec sleep 1000000' >"${_work}/term.log" 2>&1 &
			;;
		xfce4|terminator)
			"${bin}" --geometry "${cols}x${rows}" -e "/bin/dash -c 'exec sleep 1000000'" \
				>"${_work}/term.log" 2>&1 &
			;;
		## These four start on a throwaway account and session bus, as on the speed rig.
		## GNOME Terminal would otherwise hand the window to a server already running on
		## the desktop's bus, and the Electron two write their settings under HOME.
		gnome|wezterm|tabby|hyper)
			local -r home="${_work}/home"
			fPrivateAccount "${home}"
			fWriteHold
			#  shellcheck disable=2154  ## Both arrays are filled by fPrivateAccount in bench-common.bash.
			local -a run=(env "${_privateEnv[@]}" GDK_BACKEND=x11 "${_privateBus[@]}")
			case "${key}" in
				gnome)
					run+=(gnome-terminal --wait --geometry="${cols}x${rows}" -- /bin/dash "${_work}/hold.sh") ;;
				wezterm)
					run+=("${bin}" -n --config "initial_cols=${cols}" --config "initial_rows=${rows}" \
						start --always-new-process -- /bin/dash "${_work}/hold.sh") ;;
				tabby)
					## Same hook as the speed rig: no SHELL, no profile, so the login shell.
					mkdir -p "${home}/.config/tabby"
					printf 'enableWelcomeTab: false\n' > "${home}/.config/tabby/config.yaml"
					printf 'exec /bin/dash %s\n' "${_work}/hold.sh" > "${home}/.bashrc"
					cp "${home}/.bashrc" "${home}/.bash_profile"
					run+=("${bin}") ;;
				hyper)
					fWriteHyperConfig "${home}"
					run+=("${bin}") ;;
			esac
			"${run[@]}" >"${_work}/term.log" 2>&1 &
			;;
	esac
	printf '%s' "$!"
}

##	The root of an extracted AppImage the binary sits in: the folder holding AppRun, a
##	few levels up at most. The WezTerm and Electron bundles are measured that way.
fAppDir() {
	local dir=""
	local -i level=0
	dir="$(dirname "$(readlink -f "$1")")"
	for ((level = 0; level < 3; level++)); do
		if [[ -e "${dir}/AppRun" ]]; then printf '%s' "${dir}"; return 0; fi
		dir="$(dirname "${dir}")"
	done
	return 1
}

##	The terminal's window on the private display, and its size, once it is up.
declare -i _winW=0 _winH=0
fFindWindow() {
	local -r exe="$1"
	local -i waited=0
	local main="" ids="" geometry=""
	while ((waited < 120)); do
		main="$(fOwnedRoot "${exe}" "${_work}/home" "${_rootPid}" || true)"
		if [[ -n "${main}" ]]; then
			ids="$(DISPLAY="${display}" xdotool search --onlyvisible --pid "${main}" 2>/dev/null || true)"
			_winId="${ids%%$'\n'*}"
			[[ -n "${_winId}" ]] && break
		fi
		sleep 0.5; waited+=1
	done
	[[ -n "${_winId}" ]] || fDie "no window came up for ${exe}"
	geometry="$(DISPLAY="${display}" xdotool getwindowgeometry --shell "${_winId}")"
	_winW="$(sed -n 's/^WIDTH=//p' <<< "${geometry}")"
	_winH="$(sed -n 's/^HEIGHT=//p' <<< "${geometry}")"
	return 0
}

##	The Electron two take no grid on the command line, so their window is resized until
##	the shell inside reports it.
fResizeWindow() {
	DISPLAY="${display}" xdotool windowsize "${_winId}" "$1" "$2" >/dev/null 2>&1 || true
}

#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##  Main
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

fUsage() {
	awk '/- Purpose:/{f=1} f&&!/^##/{exit} f' "${BASH_SOURCE[0]}" | sed 's/^##[[:space:]]\{0,2\}//'
}

fMain() {
	local key="" grid="100x30"
	optKeep=""; optVerbose=""

	while (($# > 0)); do
		case "$1" in
			--term)    key="${2:-}"; shift 2 ;;
			--grid)    grid="${2:-}"; shift 2 ;;
			--settle)  settleSecs="${2:-22}"; shift 2 ;;
			--verbose) optVerbose=1; shift ;;
			--keep)    optKeep=1; shift ;;
			--list)    fEcho_Clean "silkterm silkplain alacritty xterm kitty xfce4 terminator gnome wezterm tabby hyper"; return 0 ;;
			-h|--help) fUsage; return 0 ;;
			*)         fDie "unknown option '$1'" ;;
		esac
	done
	[[ -n "${key}" ]] || { fUsage; fDie "--term is required"; }

	local -i cols="${grid%x*}" rows="${grid#*x}"
	## Apart from the assignment, or 'local' hides the failure and the rig goes on to
	## launch an empty command.
	local bin=""
	bin="$(fTermBinary "${key}")" || exit 1

	_work="$(mktemp -d "${TMPDIR:-/tmp}/sizebench.XXXXXX")"

	fSection "Rig"
	fEcho "terminal ${key} -> ${bin}"
	if ! DISPLAY="${display}" xdpyinfo >/dev/null 2>&1; then
		Xvfb "${display}" -screen 0 "${screen}" -nolisten tcp >"${_work}/xvfb.log" 2>&1 &
		_xvfbPid=$!
		sleep 2
		DISPLAY="${display}" xdpyinfo >/dev/null 2>&1 || fDie "could not start Xvfb on ${display}"
		fEcho "started Xvfb on ${display} (pid ${_xvfbPid})"
	else
		fEcho "reusing the display already on ${display}"
	fi

	fSection "Launch"
	local -i root=0
	root="$(fLaunch "${key}" "${bin}" "${cols}" "${rows}")"
	_rootPid=${root}
	case "${key}" in
		gnome|wezterm)
			## Told their grid on the command line, so checked rather than trusted.
			local got=""
			got="$(fReadGrid "${_work}/size")" || exit 1
			[[ "${got}" == "${rows} ${cols}" ]] || fDie "asked for ${cols}x${rows}, got '${got}' (rows cols)"
			fEcho "grid ${cols}x${rows}" ;;
		tabby|hyper)
			fEcho "pid ${root}, fitting the window to ${cols}x${rows}"
			fFindWindow "${bin}"
			fFitGrid "${cols}" "${rows}" "${_work}/size" fResizeWindow ${_winW} ${_winH} ;;
	esac
	case "${key}" in
		gnome|wezterm|tabby|hyper) touch "${_work}/go" ;;
	esac
	fEcho "pid ${root}, settling ${settleSecs}s at ${cols}x${rows}"
	sleep "${settleSecs}"
	kill -0 "${root}" 2>/dev/null || { tail -5 "${_work}/term.log" >&2; fDie "terminal exited before it could be measured"; }

	case "${key}" in
		silkterm)  fRequireCandyProfile "${_work}/xdg/silkterm/config.shcl" ;;
		silkplain) fEcho "SilkTerm profile in force: $(fSilkProfile "${_work}/xdg/silkterm/config.shcl")" ;;
	esac

	case "${key}" in
		gnome|wezterm|tabby|hyper)
			root="$(fOwnedRoot "${bin}" "${_work}/home" "${root}")" || fDie "nothing on the rig's account is running ${bin}" ;;
	esac
	mapfile -t _termPids < <(fCollectTree "${root}")
	fEcho "process tree: ${_termPids[*]}"

	fSection "Measurement"
	local -a extra=()
	[[ -n "${optVerbose}" ]] && extra+=(--verbose)
	## A self-contained bundle is billed for everything it unpacks, not its one binary.
	local payload=""
	case "${key}" in
		wezterm)     payload="$(fAppDir "${bin}" || true)" ;;
		tabby|hyper) payload="$(fAppDir "${bin}" || dirname "$(readlink -f "${bin}")")" ;;
	esac
	if [[ -n "${payload}" ]]; then extra+=(--payload "${payload}"); fi
	python3 "${scriptDir}/sizebench-classify.py" "${_termPids[@]}" --exe "${bin}" --summary ${extra[@]+"${extra[@]}"}

	fEcho_Clean ""
	return 0
}

#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••
##  Script entry point
#•••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
	fMain "$@"
fi

##
##  History:
##  - 20260730: Written, after the previous pass's scripts were lost with their scratch dir.
##  - 20260918: The +candy row pins its profile, and the rig prints the one in force.
##  - 20260928: GNOME Terminal, WezTerm, Tabby and Hyper, on a throwaway account.
##  - 20260930: Scratch goes under TMPDIR when it is set.
##
