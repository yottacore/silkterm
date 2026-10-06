#!/usr/bin/env bash

#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.'
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'

##	- Purpose:
##		Headless scroll regression harness. Drives SilkTerm on a private Xvfb with
##		SILK_SCROLLDBG on and deterministic full-redraw scenes that model how real
##		full-screen apps repaint, then checks the per-frame trace for the behavior
##		each app is supposed to have:
##		   less / vim   - no static top band: the smooth slide engages, monotone (no bounce)
##		   nano / muffer - static title bar held still, the region under it slides
##		   tmux         - a real scroll region (DECSTBM + linefeeds) slides off the engine's count
##		   pill         - a pill repainted over a recorded region's edge is held still
##		   altenter     - a burst still easing when an alt screen takes over comes to rest
##		   chrome       - output easing under a live block redrawn in place holds the block still
##		   aptbar       - lines above a pinned status row still ease once the scrollback is full
##		   paste/pasteil - an input box growing into blank rows pops in rather than sliding down
##		Plain shell-output easing is covered by the library tests (cargo test); the
##		"jumping / re-listing / bottom-up" symptoms map to those monotonicity checks.
##		Scenes self-scroll on a timer - no key injection (unreliable here), so the
##		result is deterministic. --real also smoke-tests the actual apps (best effort).
##	- Syntax:
##		run.bash [options]
##		   --bin PATH      SilkTerm binary (default: <target dir>/debug then release)
##		   --display :N    headless display (default: $CICD_HEADLESS_DISPLAY or :98)
##		   --wayland       run under a headless Wayland compositor (cage) instead of Xvfb,
##		                   to prove the Wayland backend scrolls the same as X11
##		   --settle SECS   idle before scrolling, past GL warmup (default 13)
##		   --capture SECS  scrolling capture window per scene (default 16)
##		   --step SECS     seconds between scene repaints (default 0.15)
##		   --real          also launch real less/nano/vim.tiny (smoke, non-fatal)
##		   --keep          leave the Xvfb up and keep the trace files
##		   --strict        treat environment skips as failures
##		   --only <label>  run one deterministic scene, e.g. tmux
##		   -v, --verbose   show per-scene frame counts
##		   -h, --help
##	- Exit: 0 all pass (a skipped scene beside passes is still 0), 1 a regression
##		was measured, 3 nothing ran (no binary, python3, display or cage).
##	- Notes: uses cicd/utility/gui-headless.bash (:98, never :0). Kills only the
##		binary it launched (PID + /proc/PID/exe path checked), never by name.
##	- Test ID: EjIG8ro
##	History: At bottom of script.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail

meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"                     ## repo root (github/)
headless="${root}/cicd/utility/gui-headless.bash"

## Output helpers, same as cicd.bash: fEcho / fEcho_Clean.
declare -i _wasLastEchoBlank=0
fEcho_Clean(){ if [[ -n "${1:-}" ]]; then echo -e "${*}"; _wasLastEchoBlank=0; elif [[ ${_wasLastEchoBlank} -eq 0 ]] && echo; then _wasLastEchoBlank=1; fi; }
fEcho(){ if [[ -n "${*}" ]]; then fEcho_Clean "[ ${*} ]"; else fEcho_Clean ""; fi; }
_letterbox="••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••"
fSection(){ fEcho_Clean; fEcho_Clean "${_letterbox}"; fEcho "${*}"; }
fDie(){ { fEcho_Clean; fEcho "FAILED: ${*}"; } >&2; exit 1; }
##  shellcheck source=cicd/tests/scroll/verdict.bash
source "${meDir}/verdict.bash"
## 3 means "nothing ran" to cicd, so an abort that happens to exit 3 must not say so
trap 'rc=$?; [[ $rc -ne 0 && $rc -ne 1 ]] && printf "\n[ scroll harness ABORTED (exit %s) at line %s: %s ]\n" "$rc" "$LINENO" "$BASH_COMMAND" >&2; [[ $rc -eq 3 ]] && rc=1; exit $rc' ERR

## Options.
bin=""; display="${CICD_HEADLESS_DISPLAY:-${RPD_HEADLESS_DISPLAY:-:98}}"
settle=13; capture=16; step=0.15; doReal=0; keep=0; strict=0; verbose=0; wayland=0; only=""
while (($#)); do case "${1}" in
	--bin)      bin="${2-}"; shift 2 ;;
	--display)  display="${2-}"; shift 2 ;;
	--wayland)  wayland=1; shift ;;
	--settle)   settle="${2-}"; shift 2 ;;
	--capture)  capture="${2-}"; shift 2 ;;
	--step)     step="${2-}"; shift 2 ;;
	--real)     doReal=1; shift ;;
	--keep)     keep=1; shift ;;
	--strict)   strict=1; shift ;;
	--only)     only="${2-}"; shift 2 ;;
	-v|--verbose) verbose=1; shift ;;
	-h|--help)  sed -n '/^##	- Purpose:/,/^##	History:/p' "${BASH_SOURCE[0]}" | sed '$d; s/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
## gui-headless.bash reads its display from here and defaults to :99 without it.
## cicd only exported this from the profiler stage, so a run that reached here
## without that started Xvfb on :99, ran SilkTerm on :98, and every scene skipped.
export CICD_HEADLESS_DISPLAY="${display}"

fSection "SilkTerm scroll regression (headless)"

## Resolve the binary: prefer debug (exists after the debug-build stage), then release.
if [[ -z "${bin}" ]]; then
	## cargo's output dir moves with CARGO_TARGET_DIR, so ./target is a guess.
	## Guessing wrong here costs nothing visible: the harness reports through its
	## pass count, so a missed binary reads as a clean run that tested nothing.
	tdir="${CARGO_TARGET_DIR:-target}"
	[[ "${tdir}" = /* ]] || tdir="${root}/${tdir}"
	for cand in "${tdir}/debug/silkterm" "${tdir}/release/silkterm"; do
		[[ -x "${cand}" ]] && { bin="${cand}"; break; }
	done
fi

## Environment preconditions -> skip (non-fatal) unless --strict. A skip exits 3,
## not 0, so cicd cannot print OK for a run that measured nothing.
skip=""
[[ -n "${bin}" && -x "${bin}" ]] || skip="no SilkTerm binary (build it first, or pass --bin)"
[[ -z "${skip}" ]] && ! command -v python3 >/dev/null 2>&1 && skip="python3 not found"
if ((wayland)); then
	[[ -z "${skip}" ]] && ! command -v cage >/dev/null 2>&1 && skip="cage not found (no headless Wayland compositor)"
else
	[[ -z "${skip}" ]] && ! command -v Xvfb    >/dev/null 2>&1 && skip="Xvfb not found (no headless display)"
	[[ -z "${skip}" ]] && [[ ! -x "${headless}" ]] && skip="gui-headless.bash missing: ${headless}"
fi
if [[ -n "${skip}" ]]; then
	((strict)) && fDie "scroll harness: ${skip}"
	fEcho "WARNING: skipped: ${skip}"; exit 3
fi
fEcho_Clean "binary ....: ${bin}"
if ((wayland)); then
	fEcho_Clean "engine ....: Wayland (cage, headless)   settle ${settle}s  capture ${capture}s  step ${step}s"
else
	fEcho_Clean "engine ....: X11 (${display})   settle ${settle}s  capture ${capture}s  step ${step}s"
fi

## Throwaway config + temp workspace (nothing personal leaks; cleaned on exit).
work="$(mktemp -d "${TMPDIR:-/tmp}/silk-scroll.XXXXXX")"
cfg="${work}/config.shcl"
cat >"${cfg}" <<-'SHCL'
	## Throwaway config for the scroll harness - not a user config.
	## No performance profile: the settings below are the scene, and a step
	## down on a software renderer would turn smooth scrolling off mid-run.
	performance.automatic: false
	performance.profile: custom
	scroll.smooth_apps: true
	## The scenes watch the text. A minimap column would add a strip that moves
	## on its own schedule.
	scroll.minimap.enabled: false
	transparency.enabled: false
	text.scrim.enabled: false
	wallpaper.fallback_builtin: false
	window.columns: 100
	window.rows: 34
	cursor.animation: none
SHCL

## ${USER} is unset in a cron or ssh context, which `set -u` turns into a
## "scroll regression" that is nothing of the sort.
runDir="/tmp/cicd-gui-headless-${USER:-$(id -un)}"
auth="${runDir}/Xauthority-${display#:}"

## Bring up the private display (with a WM - winit needs one on bare Xvfb to get
## the events that drive rendering). Only stop it on exit if we started it. The
## Wayland path needs no persistent display - each scene runs its own cage kiosk.
startedHeadless=0
if ! ((wayland)) && "${headless}" status 2>/dev/null | grep -q 'no Xvfb'; then
	if "${headless}" start --wm >/dev/null 2>&1; then startedHeadless=1
	else
		((strict)) && fDie "headless display failed to start"
		fEcho "WARNING: skipped: headless display failed to start"; exit 3
	fi
fi

fCleanup(){
	local -r rc=$?
	if ((keep)); then
		fEcho_Clean "kept: traces in ${work}  (display left up)"
		## The traces are inside the run folder, so it stays too.
		fTestDir_End 1
	else
		((startedHeadless)) && "${headless}" stop >/dev/null 2>&1 || true
		rm -rf "${work}" 2>/dev/null || true
		fTestDir_End "${rc}"
	fi
}
trap fCleanup EXIT

## Kill a PID only if it is still the binary we launched (PID + exe path), never by
## name - the repo path contains "silkterm" and a dogfood copy may be running.
fKillOurs(){
	local pid="${1}" want; want="$(realpath -e "${2}" 2>/dev/null || true)"
	local exe; exe="$(realpath -e "/proc/${pid}/exe" 2>/dev/null || true)"
	if [[ -n "${want}" && "${exe}" == "${want}" ]]; then
		kill "${pid}" 2>/dev/null || true
		local _; for _ in $(seq 1 20); do kill -0 "${pid}" 2>/dev/null || break; sleep 0.1; done
		kill -9 "${pid}" 2>/dev/null || true
	fi
}

## Spawn SilkTerm for one scene, backgrounded; log -> $2, trace -> $3. Sets $spawnedPid.
## X11: straight onto $display. Wayland: as the single client of a headless cage kiosk
## (software pixman compositor + software Vulkan), DISPLAY unset so winit can only pick
## the Wayland backend. cage inherits the child's stderr, so the same SILK_SCROLLDBG
## trace comes through either way (cage's own stderr lines don't match the trace regex).
fSpawnSilk(){
	local shellcmd="${1}" logf="${2}" tracef="${3}"
	if ((wayland)); then
		WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 \
		XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}" \
		SILK_SCROLLDBG=1 SILK_SCENE_SETTLE="${settle}" SILK_SCENE_STEP="${step}" \
		LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json SHELL=/bin/dash \
			cage -- /bin/dash -c 'unset DISPLAY; exec "$0" --config "$1" --shell "$2"' \
				"${bin}" "${cfg}" "${shellcmd}" >"${logf}" 2>"${tracef}" &
	else
		SILK_SCROLLDBG=1 SILK_SCENE_SETTLE="${settle}" SILK_SCENE_STEP="${step}" \
			DISPLAY="${display}" XAUTHORITY="${auth}" LIBGL_ALWAYS_SOFTWARE=1 SHELL=/bin/dash \
			"${bin}" --config "${cfg}" --shell "${shellcmd}" >"${logf}" 2>"${tracef}" &
	fi
	spawnedPid=$!
}

## Stop what fSpawnSilk started. X11: our SilkTerm PID (exe-path guarded). Wayland: the
## cage kiosk we launched (exact captured PID, unambiguously ours); killing it takes the
## SilkTerm child down with it.
fStopSilk(){
	local pid="${1}"
	if ((wayland)); then
		kill "${pid}" 2>/dev/null || true
		local _; for _ in $(seq 1 20); do kill -0 "${pid}" 2>/dev/null || break; sleep 0.1; done
		kill -9 "${pid}" 2>/dev/null || true
	else
		fKillOurs "${pid}" "${bin}"
	fi
}

pass=0; fail=0; miss=0; spawnedPid=0

## Run one deterministic scene and judge its trace. shape|mode|expect_st|[expect_sb].
fRunScene(){
	local label="${1}" shape="${2}" mode="${3}" est="${4}" esb="${5:--1}"
	[[ -z "${only}" || "${only}" == "${label}" ]] || return 0
	local trace="${work}/${label}.trace"
	## a scene with a script of its own runs that; scene.bash has no case for it
	local script="${meDir}/scenes/${shape}.bash"
	[[ -f "${script}" ]] || script="${meDir}/scenes/scene.bash"
	fSpawnSilk "/bin/dash ${script} ${shape}" "${work}/${label}.log" "${trace}"
	local pid=${spawnedPid}

	## GL warmup under llvmpipe swings widely with machine load (cicd runs this while
	## the release + cross builds may still be busy), so a fixed sleep can expire before
	## a single frame renders = a false "0 trace frames" skip. Poll until the scene is
	## actually producing frames, then stop; bounded by a generous ceiling so a truly
	## dead binary still exits. The scene self-scrolls forever, so more wall time just
	## means more frames - never a hang.
	local want=60 ceiling=$((settle + capture + 60)) frames=0 doneAt=0 lead=0 marked=0
	SECONDS=0
	while ((SECONDS < ceiling)); do
		kill -0 "${pid}" 2>/dev/null || break
		## The chrome scene prints its history and then sleeps before the loop
		## under test starts. A renderer quick off the mark traces that burst
		## easing, with no block on screen to hold, and it used to fill most of
		## the frames judged. Count only what comes after, taken just before the
		## loop can have started.
		if [[ "${mode}" == pinned ]] && ((! marked && SECONDS >= settle - 1)); then
			lead=$(fTraceFrames "${trace}"); marked=1
		fi
		if [[ "${mode}" == still ]]; then
			## A still screen builds only when something changes, so it may never
			## reach the frame count. Stop a few seconds after the swap instead,
			## which is past any leftover ease.
			((doneAt == 0)) && grep -q 'alt=1' "${trace}" 2>/dev/null && doneAt=$((SECONDS + 3))
			((doneAt && SECONDS >= doneAt)) && break
		else
			frames=$(fTraceFrames "${trace}")
			frames=$((frames - lead))
			[[ "${mode}" != pinned ]] || ((marked)) || frames=0
			((frames >= want)) && break
		fi
		sleep 0.5
	done
	fStopSilk "${pid}"
	wait "${pid}" 2>/dev/null || true

	((verbose)) && fEcho_Clean "  ${label}: $(fTraceFrames "${trace}") trace frames"
	local rc=0
	python3 "${meDir}/analyze.py" --mode "${mode}" --expect-st "${est}" --expect-sb "${esb}" --label "${label}" --skip-frames "${lead}" <"${trace}" \
		| sed 's/^/  /' || rc=$?
	case "${rc}" in
		0) pass=$((pass + 1)) ;;
		1) fail=$((fail + 1)) ;;
		*) miss=$((miss + 1)) ;;
	esac
}

fSection "Deterministic scenes"
fRunScene less   less   slide 0
fRunScene vim    vim    slide 0
fRunScene nano   nano   slide 1
fRunScene muffer muffer slide 2
## A real region scroll (tmux, less): the engine's own record drives the slide, and
## the one row outside the region is the only band.
fRunScene tmux   tmux   slide 0 1
## A pill repainted over the last row of a recorded region (muffer scrolling
## back): held with the two rows under the region.
fRunScene pill   pill   slide 2 3
## A burst still easing when the alt screen takes over (git commit opening nano):
## no scrollback behind it, so the view must be at rest - frac 0 on every frame.
fRunScene altenter altenter still -1
## muffer's shape: new transcript lines ease in above a block it redraws in
## place, which must hold still (three block rows plus the blank cursor row).
fRunScene chrome chrome pinned -1 4
## apt's progress bar with the scrollback full: the lines above the bar still ease,
## and the bar's row is held.
fRunScene aptbar aptbar pinned -1 1
## An input box growing on a half-empty screen, repainted and then with
## insert-line: the new lines pop in rather than sliding down from behind the
## rows above.
fRunScene paste   paste   popin -1
fRunScene pasteil pasteil popin -1

## Best-effort real-app smoke (never fails the suite): prove the real apps render
## under SilkTerm (enter alt-screen, no hang) - regresses e.g. the cosmic-text hang
## and the alt-screen enter/exit hard-cut. No key injection, so it does not assert
## scroll correctness - that is what the deterministic scenes above are for.
fRealSmoke(){
	local app="${1}" exe; exe="$(command -v "${app}" 2>/dev/null || true)"
	[[ -n "${exe}" ]] || { fEcho_Clean "  ${app}: not installed, skipped"; return 0; }
	local file="${work}/${app}.txt" launch="${work}/${app}.launch.bash" trace="${work}/${app}.real.trace"
	seq 1 400 | sed 's/^/line /' >"${file}"
	printf '#!/bin/dash\nexec %s %s\n' "${exe}" "${file}" >"${launch}"
	fSpawnSilk "/bin/dash ${launch}" "${work}/${app}.real.log" "${trace}"
	local pid=${spawnedPid}
	sleep "$((settle + 4))"
	local alive=0; kill -0 "${pid}" 2>/dev/null && alive=1
	fStopSilk "${pid}"
	wait "${pid}" 2>/dev/null || true
	## An idle real app only builds on dirty frames, so a handful is expected; the
	## signal is that it stayed alive (no hang) and rendered the alt screen (frames>0).
	local n; n="$(fTraceFrames "${trace}")"
	if ((alive)) && ((n >= 1)); then
		fEcho_Clean "  ${app}: OK (alive, entered alt-screen: ${n} frame(s))"
	else
		fEcho_Clean "  ${app}: INFO (alive=${alive}, ${n} frames) - not asserted"
	fi
}

if ((doReal)); then
	fSection "Real-app smoke (best effort)"
	fRealSmoke less
	fRealSmoke nano
	fRealSmoke vim.tiny
fi

fSection "Summary"
fEcho_Clean "pass ${pass}   fail ${fail}   skip ${miss}"
verdict="$(fScrollVerdict "${pass}" "${fail}" "${miss}" "${strict}")"; rc=$?
fEcho "${verdict}"
exit "${rc}"


##	History:
##		- 20260706 JC: Created.
##		- 20260718 JC: --wayland pass (cage kiosk) alongside the X11 Xvfb pass.
##		- 20260918 JC: aptbar scene, --only.
