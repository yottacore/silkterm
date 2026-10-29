#!/usr/bin/env bash

#  shellcheck disable=1091  ## 'source is valid here, but shellcheck doesn't know the path to it.'
#  shellcheck disable=2001  ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes, use double quotes for that.' I know, and I often want an explicit '$'.
#  shellcheck disable=2034  ## 'variable appears unused.' Complains about valid use of variable indirection (e.g. later use of local -n var=$1)
#  shellcheck disable=2046  ## 'Quote to prevent word-splitting.' (OK for integers.)
#  shellcheck disable=2086  ## 'Double quote to prevent globbing and word splitting.' (OK for integers.)
#  shellcheck disable=2119  ## 'Use foo "$@" if function's $1 should mean script's $1.' Confusing and inapplicable.
#  shellcheck disable=2120  ## 'Foo references arguments, but none are ever passed.' Valid function argument overloading.
#  shellcheck disable=2128  ## 'Expanding an array without an index only gives the element in the index 0.' False hits on associative arrays.
#  shellcheck disable=2155  ## 'Declare and assign separately to avoid masking return values.' Cumbersome and unnecessary. For integers it's sometimes required to even come into existence for counters.
#  shellcheck disable=2162  ## 'read without -r will mangle backslashes.'
#  shellcheck disable=2178  ## 'Variable was used as an array but is now assigned a string.' False hits on associative arrays with e.g. 'local -n assocArray=$1'.
#  shellcheck disable=2181  ## 'Check exit code directly, not indirectly with $?.'
#  shellcheck disable=2317  ## 'Can't reach.' (I.e. an 'exit' is used for debugging - and makes an unusable visual mess.)
## shellcheck disable=2002  ## 'Useless use of cat.'
## shellcheck disable=2004  ## '$/${} is unnecessary on arithmetic variables.' Inappropriate complaining?
## shellcheck disable=2053  ## 'Quote the right-hand sid of = in [[ ]] to prevent glob matching.' Disable for Yoda Notation.
## shellcheck disable=2143  ## 'Use grep -q instead of echo | grep'

##	- Purpose: Local CI/CD pipeline. Generic engine, per-project settings live in config.bash.
##	- Stages (fail-fast, any error aborts before the next stage):
##	   0. remote sync (fetch; fast-forward if safely behind; abort if diverged)
##	   1. format (cargo fmt)
##	   2. debug build (this is what the tests + profiler run against)
##	   3. regression tests + lints + fuzz soak (clippy for Linux, Windows and macOS
##	      and the advisory scan gating, the rest of cargo-deny not; the tests again
##	      under ASan and LSan, in the container image; the leak checks, which
##	      are the heap-growth test under dhat; under --long, the tests again
##	      under TSan, in the image too; scroll harness)
##	   4. profiler (flamegraph SVG; non-gating artifact - see failure policy), on
##	      the card through a headless sway, and always on this box
##	   5. release build (native + cross targets; optimized, for packaging + dogfood),
##	      then a check that every Windows binary carries its icon and version block
##	   6. packages (.deb/.rpm per Linux arch; NSIS installer .exe per Windows arch),
##	      then the private runner (macOS on a Mac, the Microsoft Store bundle)
##	   7. dogfood (install each build to the synced app dir for its platform)
##	   8. backup + publish to git (runs from repo root)
##	- Syntax:
##	  cicd/cicd.bash [options]
##	  Options:
##	   -y, --yes           run unattended (no confirm prompt)
##	   -q, --quiet         quiet + unattended (implies -y); the publish step runs quiet too
##	   -m, --message MSG   publish hands-off with this commit message (no editor)
##	       --msg MSG       alias for --message
##	   --no-fmt            skip the formatter (cargo fmt) stage
##	   --no-cross          skip cross-target release builds
##	   --no-arm            skip the ARM64 release builds + packages (x86_64 only)
##	   --no-windows        skip the Windows cross targets (Linux artifacts only) -
##	                       what a Windows box's own pipeline delegates here
##	   --no-package        skip the packages stage (.deb/.rpm/installer)
##	   --no-private        skip the private runner (macOS, Microsoft Store)
##	   --no-remote         skip every other box: the Windows scenarios, the ARM64
##	                       smoke run and the private runner's Mac and Microsoft
##	                       Store jobs
##	   --no-fuzz           skip the fuzz soak (the short one in the test run still runs)
##	   --no-asan           skip the test suite under ASan and LSan
##	   --no-tsan           skip the test suite under TSan, under --long
##	   --no-leakcheck      skip the slow leak checks (the heap-growth test under dhat, CodeQL)
##	   --leakcheck         run the leak checks, under --quick too; not with
##	                       --backlog-run-quick
##	   --codeql            add CodeQL to the leak checks (off by default; slow)
##	   --long              everything, the steps that take hours too: the test
##	                       suite under TSan and CodeQL. Not with --quick or
##	                       --backlog-run-quick
##	   --slow              another name for --long
##	   --no-profile        skip the profiler stage
##	   --no-dogfood        skip the dogfood install
##	   --no-publish        skip the git backup + publish stage
##	   --no-sync           skip the remote sync check (stage 0)
##	   --no-demo           skip the demo video re-record
##	   --demo              re-record the demo video, under --quick too
##	   --quick             only the quick lints and tests, and the x86_64 release builds;
##	                       skips the ARM64 builds, packages, profiling, the fuzz soak,
##	                       the ASan and TSan runs, the slow window tests and anything
##	                       that needs another box
##	   --backlog-run-quick [AREA...]
##	                       the fast checks only, then exit: every area, or the ones
##	                       named. Areas: fmt (cargo fmt --check), clippy (Linux),
##	                       tests (cargo test), scripts (shellcheck, the bash style
##	                       check, the PowerShell and Python lints), docs (markdown
##	                       spacing, tables of contents, tables, test IDs)
##	   --gate              another name for --backlog-run-quick
##	   --backlog-run-full  every check of the default run, the other boxes too, with
##	                       the format checked, not rewritten; no sync, dogfood, demo
##	                       or publish, and no prompt
##	   --merge-gate        the same checks, before a merge to main
##	   --ci                another name for --merge-gate
##	   --fmt-check         check the format instead of rewriting it
##	   --container         stages 1-6 in the pinned image from cicd/container/Dockerfile,
##	                       with its own target dir (target/container); sync, the private
##	                       runner, dogfood, the demo and publish stay here, and so do
##	                       the steps that need the card: the profiler and the render
##	                       node test, run here once the container is done. With
##	                       --backlog-run-quick, that runs there. The default whenever
##	                       docker is here.
##	   --no-container      stages 1-6 on this box instead; the default with no docker
##	   --keep-temp         keep the run's temp folders and containers when it ends,
##	                       and print where they are
##	   --list              print the steps and test IDs the other options would run,
##	                       and run nothing
##	  With no options every stage runs, in about an hour; --long adds the rest.
## - Reuse: copy the cicd/ directory into another project and edit config.bash.

##	History: At bottom of script.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail

## Find the repo root and load project config.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/.." && pwd)"   # the git repo root (cicd/..)
export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:${PATH}"       ## rustup toolchain (cross targets, edition 2024) + zig must beat system rust.
## Stages 1-6 run in the image whenever docker is here, and --no-container runs them
## on this box. The container build uses its own target dir, and config.bash derives
## every build path from CARGO_TARGET_DIR, so it is set before the config loads. The
## run inside gets the same value, so the two sides of the mount agree on where
## everything is.
useContainer=0
case " ${*} " in
	*" --no-container "*) ;;
	*" --container "*) useContainer=1 ;;
	*) [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]] || ! command -v "${SILK_DOCKER:-docker}" >/dev/null 2>&1 || useContainer=1 ;;
esac
((! useContainer)) || export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${root}/target/container}"
source "${here}/config.bash"
source "${here}/utility/include/gfs-rotate.bash"                  ## gfs_rotate() for the profiler artifacts
source "${here}/utility/include/remote-git.bash"                  ## fRemoteGit / fRemoteGh, as the folder's own account
##  shellcheck source=cicd/utility/include/saferm.bash
source "${here}/utility/include/saferm.bash"                      ## fSafeRm, fSafeRm_Under: every recursive removal
##  shellcheck source=cicd/utility/include/sweep.bash
source "${here}/utility/include/sweep.bash"                       ## what earlier runs left, once it is stale
##  shellcheck source=cicd/utility/include/echo.bash
source "${here}/utility/include/echo.bash"                        ## fEcho, fEcho_Clean, fSection, fDie
##  shellcheck source=cicd/utility/built-from.bash
source "${here}/utility/built-from.bash"                          ## the artifacts' provenance note
declare -p FMT_CMD &>/dev/null || FMT_CMD=()                      ## tolerate a config without the fmt stage
declare -p FMT_CHECK_CMD &>/dev/null || FMT_CHECK_CMD=()
## Every cross target the config has, before --no-arm or --no-windows drop some,
## since a run that skips one still keeps its build dir for the next.
allCrossTargets=(); declare -p CROSS_TARGETS &>/dev/null && allCrossTargets=("${CROSS_TARGETS[@]}")

##  shellcheck source=cicd/utility/include/jobs.bash
source "${here}/utility/include/jobs.bash"                        ## fJobs, fJobs_LowPriority

## Half the CPUs, or a quarter when the box is busy or the step is a hard one,
## worked out again before each step. cargo's jobserver bounds rustc and codegen
## to CARGO_BUILD_JOBS, which covers build, test, clippy and the zigbuild cross
## builds; RUST_TEST_THREADS bounds the test run. A project can cap
## CICD_MAX_JOBS in config.bash.
fJobs_LowPriority
fJobs

cd "${root}"
stamp="$(date +%Y%m%d-%H%M%S)"

## Parse options.
assumeYes=0; quiet=0; quick=0; long=0; noRemote=0; skipLog=""; gate=0; gateAreas=(); fullRun=0; fullRunName=""; fmtCheck=0; leakcheck=1; leakAsked=0; codeql=0; demoAsked=0; noArm=0; noWindows=0; sync=1; cliMessage=""; container="${useContainer:-0}"; noContainer=0; keepTemp=0; listing=0; innerArgs=()
while (($#)); do case "${1}" in
	-y|--yes)                 assumeYes=1; shift ;;
	-q|--quiet)               quiet=1; assumeYes=1; shift ;;   ## quiet + unattended; publish runs quiet too
	--backlog-run-quick)      gate=1; shift; while (($#)) && [[ "${1}" != -* ]]; do gateAreas+=("${1}"); shift; done ;;   ## the fast checks only, then exit
	--gate)                   echo "--gate is another name for --backlog-run-quick" >&2; set -- --backlog-run-quick "${@:2}" ;;
	--backlog-run-full|--merge-gate) fullRun=1; fullRunName="${1}"; shift ;;   ## every check of the default run, and nothing that writes or prompts
	--ci)                     echo "--ci is another name for --merge-gate" >&2; set -- --merge-gate "${@:2}" ;;
	--fmt-check)              fmtCheck=1; innerArgs+=("${1}"); shift ;;
	--container)              container=1; shift ;;             ## stages 1-6 in the image; innerArgs collects what goes in with them
	--no-container)           noContainer=1; shift ;;           ## stages 1-6 on this box
	--keep-temp)              keepTemp=1; shift ;;
	--list)                   listing=1; shift ;;               ## what would run, then exit
	--no-fmt)                 FMT_CMD=(); innerArgs+=("${1}"); shift ;;
	--no-cross)               BUILD_CROSS=0; innerArgs+=("${1}"); shift ;;
	--no-arm)                 noArm=1; innerArgs+=("${1}"); shift ;;                ## drop ARM64 builds + packages
	--no-windows)             noWindows=1; innerArgs+=("${1}"); shift ;;            ## drop the Windows cross targets
	--no-package)             PACKAGE_ENABLE=0; innerArgs+=("${1}"); shift ;;
	--no-private)             PRIVATE_RUNNER=""; shift ;;
	--no-remote)              noRemote=1; shift ;;              ## no other box; the container never reaches one anyway
	--no-profile)             PROFILE_ENABLE=0; innerArgs+=("${1}"); shift ;;
	--no-dogfood)             DOGFOOD_DESTS=(); shift ;;
	--no-publish)             GIT_PUBLISH=(); shift ;;
	--no-sync)                sync=0; shift ;;
	--demo)                   DEMO_ENABLE=1; demoAsked=1; shift ;;
	--no-demo)                DEMO_ENABLE=0; shift ;;
	--quick)                  quick=1; noArm=1; PROFILE_ENABLE=0; PACKAGE_ENABLE=0; FUZZ_SECS=0; innerArgs+=("${1}"); shift ;;   ## skip the slow stages
	--long|--slow)            long=1; innerArgs+=(--long); shift ;;   ## the steps that take hours too
	--no-fuzz)                FUZZ_SECS=0; innerArgs+=("${1}"); shift ;;
	--no-asan)                ASAN_CMD=(); innerArgs+=("${1}"); shift ;;
	--no-tsan)                TSAN_CMD=(); innerArgs+=("${1}"); shift ;;
	--leakcheck)              leakcheck=1; leakAsked=1; innerArgs+=("${1}"); shift ;;   ## the slow leak checks, under --quick too
	--no-leakcheck)           leakcheck=0; innerArgs+=("${1}"); shift ;;
	--codeql)                 codeql=1; innerArgs+=("${1}"); shift ;;                ## CodeQL in the leak checks; off until 2026102211462005
	--message=*|--msg=*|-m=*) cliMessage="${1#*=}"; shift ;;
	-m|--message|--msg)       cliMessage="${2-}"; shift; (($#)) && shift ;;
	-h|--help)                sed -n '/^##	- Purpose:/,/^##	History:/p' "${BASH_SOURCE[0]}" | sed '$d; s/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done
if ((noContainer)); then container=0; fi
## The backlog runs and the merge gate never sync, dogfood, publish, rewrite a
## tracked file or prompt. The demo rewrites the README gif, so it stays off too.
gateAreasAll="fmt clippy tests scripts docs"
if ((gate && fullRun)); then echo "--backlog-run-quick and ${fullRunName} are different runs; pick one" >&2; exit 2; fi
if ((fullRun && quick)); then echo "${fullRunName} runs every check of the default run, so not --quick" >&2; exit 2; fi
if ((long && quick)); then echo "--long runs everything, so not --quick" >&2; exit 2; fi
if ((long && gate)); then echo "--backlog-run-quick runs the fast checks only, so not --long" >&2; exit 2; fi
## TSan and CodeQL take about half an hour each, so they wait for --long, which
## keeps the default run to about an hour.
if ((long)); then codeql=1; fi
for area in "${gateAreas[@]}"; do case " ${gateAreasAll} " in *" ${area} "*) ;; *) echo "unknown area: ${area} (areas: ${gateAreasAll})" >&2; exit 2 ;; esac; done
if ((gate && ${#gateAreas[@]} == 0)); then read -r -a gateAreas <<< "${gateAreasAll}"; fi
if ((fullRun)); then assumeYes=1; sync=0; DOGFOOD_DESTS=(); GIT_PUBLISH=(); DEMO_ENABLE=0; fmtCheck=1; innerArgs+=(--fmt-check); fi
if ((fmtCheck && ${#FMT_CMD[@]})); then FMT_CMD=("${FMT_CHECK_CMD[@]}"); fi
if ((gate && leakAsked && leakcheck)); then echo "--leakcheck adds to a full run; the backlog quick run leaves it out" >&2; exit 2; fi
if ((gate && codeql)); then echo "--codeql is one of the leak checks, which the backlog quick run leaves out" >&2; exit 2; fi
if ((quick && ! leakAsked)) || ((gate)); then leakcheck=0; fi   ## on by default, like the demo

## --no-arm: drop the ARM64 cross targets so the run (and its packages) stay
## x86_64-only. Native x86_64 is untouched; the Windows/Linux x86_64 crosses stay.
if ((noArm)) && declare -p CROSS_TARGETS &>/dev/null; then
	kept=()
	for t in "${CROSS_TARGETS[@]}"; do case "${t}" in *arm64*|*aarch64*) ;; *) kept+=("${t}") ;; esac; done
	CROSS_TARGETS=("${kept[@]}")
fi

## --no-windows: drop the Windows cross targets. For a Windows box driving this
## through WSL, which has already built its own Windows binaries natively - and
## natively is the only way to get the msvc one at all.
if ((noWindows)) && declare -p CROSS_TARGETS &>/dev/null; then
	kept=()
	for t in "${CROSS_TARGETS[@]}"; do case "${t}" in *windows*) ;; *) kept+=("${t}") ;; esac; done
	CROSS_TARGETS=("${kept[@]}")
fi
declare -p PACKAGE_ENABLE &>/dev/null || PACKAGE_ENABLE=0   ## tolerate a config predating the packages stage

## Publish commit message: -m wins, then config, then what is typed at the
## prompt below. A blank answer, or --yes, takes the automatic one. The publisher
## runs quiet and never opens an editor, so the plan and the prompt name the
## message a blank answer commits. They used to promise an editor.
autoMsg="${APP_NAME} CI/CD ${stamp}"
## fPublishMessage <cli> <config> <answer>
fPublishMessage(){
	if   [[ -n "${1}" ]]; then echo "${1}"
	elif [[ -n "${2}" ]]; then echo "${2}"
	elif [[ -n "${3}" ]]; then echo "${3}"
	else echo "${autoMsg}"
	fi
}
publishMsg=""
if [[ -n "${cliMessage}" || -n "${PUBLISH_AUTO_MESSAGE:-}" ]] || ((assumeYes)); then
	publishMsg="$(fPublishMessage "${cliMessage}" "${PUBLISH_AUTO_MESSAGE:-}" "")"
fi

## A test script's ID, from the "Test ID:" line in its header.
fTestId(){ sed -n '/Test ID:/{s/.*Test ID:[[:space:]]*//;s/[[:space:]].*//;p;q;}' "${root}/${1}" 2>/dev/null || true; }
## Where a test script's stdout goes while it runs. The test run folder outlives
## a failed run, so a failed test's file is still there afterward.
fTestOut(){ local name="${1//\//_}"; echo "${SILKTERM_TEST_DIR:-${TMPDIR:-/tmp}}/output_${name%.*}.txt"; }
## fTestOut_Fail <out> <failure>: the end of a failed test's output into the log,
## then the failure line, naming the file, aborts the run.
fTestOut_Fail(){
	local -r out="${1}" failure="${2}"
	local -i lines=0
	[[ -f "${out}" ]] && lines="$(wc -l <"${out}")"
	if ((lines > 200)); then fEcho_Clean "last 200 of ${lines} lines of output:"; tail -n 200 "${out}"
	elif ((lines)); then cat "${out}"; fi
	fDie "${failure}, output kept in ${out}"
}
## fRunTest <script> <label> [failure]: runs a test script under the repo root,
## if it is there. OK and its test ID on a pass, with its output dropped. The
## failure text, by default "<label> test failed", aborts the run, after the
## test's output.
fRunTest(){
	local -r script="${1}" label="${2}" failure="${3:-${2} test failed}"
	[[ -x "${root}/${script}" ]] || return 0
	if ((listing)); then fListTest "${script}" "${label}"; return 0; fi
	fEcho_Clean "${label} ..."
	local out; out="$(fTestOut "${script}")"
	"${root}/${script}" >"${out}" || fTestOut_Fail "${out}" "${failure} ($(fTestId "${script}"))"
	rm -f "${out}"
	fEcho "OK: ${label} ($(fTestId "${script}"))"
}
## fRunTest_MaySkip <script> <label> [skipped]: the same, for a test whose exit 3
## means it could not run. That is a warning, by default "<label> skipped", and
## never an OK.
fRunTest_MaySkip(){
	local -r script="${1}" label="${2}" skipped="${3:-${2} skipped}"
	[[ -x "${root}/${script}" ]] || return 0
	if ((listing)); then fListTest "${script}" "${label}"; return 0; fi
	fEcho_Clean "${label} ..."
	local rc=0 out; out="$(fTestOut "${script}")"
	"${root}/${script}" >"${out}" || rc=$?
	case "${rc}" in
		0) rm -f "${out}"; fEcho "OK: ${label} ($(fTestId "${script}"))" ;;
		3) rm -f "${out}"; fEcho "WARNING: ${skipped}" ;;
		*) fTestOut_Fail "${out}" "${label} test failed ($(fTestId "${script}"))" ;;
	esac
}
## fListTest <script> <label>: a test's line under --list.
fListTest(){ local id; id="$(fTestId "${1}")"; printf '  %-5s  %s\n' "${id:------}" "${2}"; }
## --list: the Rust tests cargo test runs, as ID and name.
fListRustTests(){
	local found=""
	local -a lines=()
	if [[ -x "${root}/cicd/utility/test-id.py" ]] && command -v python3 >/dev/null 2>&1; then found="$("${root}/cicd/utility/test-id.py" --list)"; fi
	[[ -z "${found}" ]] || mapfile -t lines <<<"${found}"
	fEcho_Clean "Rust tests (${TEST_CMD[*]}, ${#lines[@]}):"
	((! ${#lines[@]})) || printf '  %s\n' "${lines[@]}"
}
## Runs cargo test with each result line as status, test ID and name. Every other
## line goes through untouched, and the command's own exit status is kept.
fTestLines(){
	if [[ -x "${root}/cicd/utility/test-id.py" ]] && command -v python3 >/dev/null 2>&1; then
		"${@}" | "${root}/cicd/utility/test-id.py" --annotate
	else
		"${@}"
	fi
}
## True when a process here is running the file at $1. Reads /proc, since fuser is
## not on every distro. A Windows build in the synced dir is never run from there.
## -ef is a builtin stat, so only a process running that very file costs a fork.
## The readlink after it keeps a hard link elsewhere, or a process in another
## mount namespace, from counting.
fInUse(){ local want exe; want="$(readlink -f "${1}" 2>/dev/null)" || return 1; [[ -n "${want}" ]] || return 1
	for exe in /proc/[0-9]*/exe; do
		if [[ "${exe}" -ef "${want}" && "$(readlink "${exe}" 2>/dev/null)" == "${want}" ]]; then return 0; fi
	done; return 1
}
## Tag for a build copy: '<toolchain: gnu|msvc><built on: l|m|b|w><target: l|m|b|w><arch: i|a>'.
## Built-on is this host; the target and arch come from the os-arch label the build
## was made under, so a cross-build is tagged for where it will RUN. Prints nothing
## for anything unrecognised - no tag beats a wrong one.
fBuildTag(){
	local -r osarch="${1:-}"
	local here target arch
	case "$(uname -s)" in
		Linux)                here=l ;;
		Darwin)               here=m ;;
		*BSD|DragonFly)       here=b ;;
		MINGW*|MSYS*|CYGWIN*) here=w ;;
		*)                    return 0 ;;
	esac
	case "${osarch%%-*}" in
		linux)   target=l ;;
		macos)   target=m ;;
		windows) target=w ;;
		*)       return 0 ;;
	esac
	case "${osarch#*-}" in
		x86_64)  arch=i ;;
		arm64)   arch=a ;;
		*)       return 0 ;;
	esac
	printf 'gnu%s%s%s' "${here}" "${target}" "${arch}"
}
## First writable dir out of a '|'-separated candidate list, or nothing. Reports
## only - the preflight calls it too, and a plan that is never confirmed must not
## have left directories behind.
fDogfoodDest(){
	local dir
	local -a dirs=()
	IFS='|' read -r -a dirs <<< "${1:-}"
	for dir in "${dirs[@]}"; do
		if [[ -d "${dir}" && -w "${dir}" ]]; then
			printf '%s' "${dir}"
			return 0
		fi
	done
	return 0
}
## Where this run is happening, for the plan header: the skips differ per host, so
## say which one it is up front. WSL is told from its kernel string; the Windows
## pipeline sets CICD_LINUX_HALF when it hands the Linux stages over.
fHostLine(){
	local kernel="" os=""
	[[ -r /proc/version ]] && kernel="$(</proc/version)"
	[[ -r /etc/os-release ]] && os="$(. /etc/os-release 2>/dev/null; printf '%s' "${PRETTY_NAME:-${NAME:-}}")"
	fHostDescribe "$(uname -s)" "$(uname -m)" "${kernel}" "${os}"
}
## fHostDescribe <uname -s> <uname -m> <kernel version string> <distribution>
fHostDescribe(){
	local -r sys="${1}" arch="${2}" kernel="${3}" os="${4}" distro="${WSL_DISTRO_NAME:-}"
	case "${sys}" in
		MINGW*|MSYS*|CYGWIN*)
			printf 'Windows (MSYS bash) - cicd-win.ps1 is the pipeline for this box' ;;
		Linux)
			if [[ "${kernel,,}" == *microsoft* ]]; then
				local gen="WSL"; [[ "${kernel}" == *WSL2* ]] && gen="WSL2"
				if [[ -n "${CICD_LINUX_HALF:-}" ]]; then
					printf '%s%s on Windows, the Linux half of a cicd-win.ps1 run' "${gen}" "${distro:+ (${distro})}"
				else
					printf '%s%s on Windows, run on its own' "${gen}" "${distro:+ (${distro})}"
				fi
			else
				printf 'Linux%s, %s' "${os:+ (${os})}" "${arch}"
			fi ;;
		*) printf '%s, %s' "${sys}" "${arch}" ;;
	esac
}
## Run a build command, retrying it a few times before calling it a failure. Every
## profile that reaches here uses fat LTO, and rustc has repeatedly died part way
## through one inside LLVM - a different pass and a different signal each time
## (SIGILL, SIGSEGV, SIGBUS) - then compiled the identical source clean on the next
## try. It has crashed twice in a row, so one retry is not enough. Stage 2 has
## already compiled everything bar the feature-gated profiler hooks, so a genuine
## error surfaces in seconds here and the extra tries cost nothing on that path.
fRetryBuild(){
	local -r what="${1}"; shift
	local -i tries="${BUILD_ATTEMPTS:-5}"
	((tries >= 1)) || tries=1
	local -i n=0 rc=0
	while ((n < tries)); do
		n+=1
		rc=0
		"${@}" || rc=$?
		((rc)) || return 0
		if ((n < tries)); then
			fEcho "WARNING: ${what} build failed (attempt ${n} of ${tries}) - retrying, since a compiler crash here has been a toolchain flake"
		fi
	done
	fDie "${what} build failed ${tries}x - a real error, or the fat-LTO crash is no longer occasional"
}
## The artifact files this configuration is meant to produce, whatever this run
## actually did. --quick, --no-cross and friends leave part of the set behind and a
## package step that cannot find its tool only warns, so the note has to say what a
## whole set looks like rather than list what happened to be there.
fReleaseExpects(){
	local -a want=("${EXE_NAME}-${ver}-${RELEASE_NATIVE_OSARCH}")
	local -a osarchs=("${RELEASE_NATIVE_OSARCH}")
	local t rest osarch art
	if ((${#CROSS_TARGETS[@]})); then
		for t in "${CROSS_TARGETS[@]}"; do
			rest="${t#*|}"; osarch="${rest%%|*}"; rest="${rest#*|}"; art="${rest%%|*}"
			if [[ "${art}" == *.exe ]]; then
				want+=("${EXE_NAME}-${ver}-${osarch}.exe")
			else
				want+=("${EXE_NAME}-${ver}-${osarch}")
			fi
			osarchs+=("${osarch}")
		done
	fi
	if ((PACKAGE_ENABLE)); then
		for osarch in "${osarchs[@]}"; do
			case "${osarch}" in
				linux-*)   want+=("${EXE_NAME}-${ver}-${osarch}.deb" "${EXE_NAME}-${ver}-${osarch}.rpm") ;;
				windows-*) want+=("${EXE_NAME}-${ver}-${osarch}-setup.exe") ;;
			esac
		done
	fi
	printf '%s\n' "${want[@]}"
}

## (Re)write the sha256sums file over every artifact in the release dir except the
## sums file itself. Run after stage 5 (binaries) and again after stage 6 (packages),
## so the checksums cover the packages too. Uses the script-scope artDir/ver/sums.
fWriteSums(){
	[[ -n "${artDir:-}" && -d "${artDir:-/nonexist}" ]] || return 0
	( cd "${artDir}"
	  ## the signature covers the sums file, so it can never be inside it
	  files=(); for x in "${EXE_NAME}-${ver}-"*; do [[ "${x}" == "${sums}" || "${x}" == *.sig || ! -f "${x}" ]] && continue; files+=("${x}"); done
	  ((${#files[@]})) && sha256sum "${files[@]}" > "${sums}" )
	local -a expects=()
	mapfile -t expects < <(fReleaseExpects)
	fWriteBuiltFrom "${artDir}" "${builtFromState:-}" "${expects[@]}"
}
trap 'rc=$?; printf "\n[ CICD ABORTED (exit %s) at line %s: %s ]\n\n" "$rc" "$LINENO" "$BASH_COMMAND" >&2; exit $rc' ERR

## --container: the image is built from cicd/container/Dockerfile with the versions
## in tool-pins.txt and rust-toolchain.toml as build args, and its tag hashes the
## recipe plus those args, so a change to any of them is a new image. The run inside
## sees this tree at its own path, the target dir CARGO_TARGET_DIR named before the
## config loaded, and a named volume for its home and the crate cache.
docker="${SILK_DOCKER:-docker}"
## Every container, image and volume the pipeline makes carries the project and
## the run, so a cleanup takes only its own from a shared docker. runName is the
## run folder's name, set once that folder is made (fRunName).
projectLabel="dev.project=${EXE_NAME}"
runName=""; dockerUsed=0; dockerDfBefore=""
cacheVolume="${EXE_NAME}-cicd-cache"; zigVolume="${EXE_NAME}-zig-cache"
zigStamp=".cicd-zig-version"
fLabels(){ printf '%s\n' --label "${projectLabel}" --label "dev.run=${runName:?}"; }
containerRecipe="${here}/container/Dockerfile"
## The build args: the toolchain and its targets, then VER_<pin> for each pin the
## recipe takes, so a pin the image does not install changes nothing.
fContainerArgs(){
	local pin name ver
	printf 'VER_rust=%s\n' "$(sed -n 's/^channel *= *"\(.*\)".*/\1/p' "${root}/rust-toolchain.toml")"
	printf 'RUST_TARGETS=%s\n' "$(sed -n '/^targets *= *\[/,/\]/p' "${root}/rust-toolchain.toml" | grep -oE '"[^"]+"' | tr -d '"' | paste -sd' ')"
	printf 'RUST_NIGHTLY=%s\n' "${SANITIZER_TOOLCHAIN:-}"
	for pin in "${TOOL_PINS[@]}"; do
		name="${pin%%|*}"; name="${name//-/_}"; ver="${pin#*|}"; ver="${ver%%|*}"
		if grep -qxF "ARG VER_${name}" "${containerRecipe}"; then printf 'VER_%s=%s\n' "${name}" "${ver}"; fi
	done
}
fContainerImage(){ printf '%s-cicd:%s' "${EXE_NAME}" "$({ cat "${containerRecipe}"; fContainerArgs; } | sha256sum | cut -c1-12)"; }
## fRunName: the run folder's name, for the labels and container names. The test
## run folder is the run's folder for now; one handed in from outside can be
## shared, so then the stamp and pid stand in.
fRunName(){
	local owned=""
	owned="$(fTestDir_Owned)"; owned="${owned%%$'\t'*}"
	if [[ -n "${owned}" ]]; then runName="${owned##*/}"; runName="${runName#test_"${EXE_NAME}"_}"
	else runName="${stamp}-$$"; fi
}
## fDockerStart: notes what docker holds, once, for the end line.
fDockerStart(){
	((! dockerUsed)) || return 0
	dockerUsed=1
	dockerDfBefore="$("${docker}" system df --format '{{.Type}}|{{.Size}}' 2>/dev/null || true)"
}
## fContainerBuild <image>: builds it when it is not there.
fContainerBuild(){
	local -r image="${1}"
	local arg
	local -a args=() labels=()
	fDockerStart
	if "${docker}" image inspect "${image}" >/dev/null 2>&1; then fEcho_Clean "image ${image}"; return 0; fi
	fEcho_Clean "building ${image} (the recipe or a pin changed; this takes a while)"
	while IFS= read -r arg; do args+=(--build-arg "${arg}"); done < <(fContainerArgs)
	mapfile -t labels < <(fLabels)
	"${docker}" build -t "${image}" "${labels[@]}" "${args[@]}" --build-arg "JOBS=${CICD_MAX_JOBS}" "${here}/container" || fDie "image build failed"
}
## fContainerVolume <name>: made with the labels when it is not there, since one
## docker run makes on the fly has none.
fContainerVolume(){
	local -a labels=()
	"${docker}" volume inspect "${1}" >/dev/null 2>&1 && return 0
	mapfile -t labels < <(fLabels)
	"${docker}" volume create "${labels[@]}" "${1}" >/dev/null || echo "WARNING: could not make the volume ${1}" >&2
}
## A container of ours that stopped without going, as one does when the docker
## daemon restarts under --rm. A running one is left, since it can be another
## run's.
fContainerStopped(){
	local id
	local -i gone=0
	while IFS= read -r id; do
		[[ -n "${id}" ]] || continue
		if "${docker}" rm "${id}" >/dev/null 2>&1; then gone+=1; fi
	done < <("${docker}" ps -aq --filter "label=${projectLabel}" --filter status=exited --filter status=created --filter status=dead 2>/dev/null || true)
	fSweep_Count "stopped containers" "${gone}"
}
## fContainerTags <image>: of the pipeline's images, the one this run used and the
## newest before it stay, and older ones go. Plain image rm, so one a container
## still uses stays, and goes on a later run.
fContainerTags(){
	local -r image="${1}"
	local line old
	local -i held=0 gone=0
	while IFS= read -r line; do
		old="${line#*|}"
		[[ -n "${old}" && "${old}" != "${image}" && "${old}" != *"<none>"* ]] || continue
		if ((held < 1)); then held+=1; continue; fi
		if "${docker}" image rm "${old}" >/dev/null 2>&1; then gone+=1; fi
	done < <("${docker}" image ls "${image%%:*}" --filter "label=${projectLabel}" --format '{{.CreatedAt}}|{{.Repository}}:{{.Tag}}' 2>/dev/null | sort -r || true)
	((! gone)) || fEcho_Clean "cleanup: removed ${gone} older container images"
}
## fContainerVolumes: a cache volume of ours over the cap goes whole, and so does
## the Zig one once another Zig version filled it. docker refuses a volume that a
## container still holds, such as another run's, so that one stays.
fContainerVolumes(){
	local name size stamped zigNow why seen=""
	local -r capBytes=$(( ${CLEANUP_CACHE_CAP_MB:-2048} * 1048576 )) tidy="${EXE_NAME}-${runName}-tidy"
	local -a ours=() mounts=() labels=()
	mapfile -t ours < <("${docker}" volume ls -q --filter "label=${projectLabel}" 2>/dev/null | grep -xF -e "${cacheVolume}" -e "${zigVolume}" || true)
	((${#ours[@]})) || return 0
	for name in "${ours[@]}"; do mounts+=(-v "${name}:/v/${name}:ro"); done
	mapfile -t labels < <(fLabels)
	## No --rm: it can return before the container is gone, which still holds the
	## volumes. A plain rm waits.
	seen="$("${docker}" run "${labels[@]}" --name "${tidy}" "${mounts[@]}" "${containerImage}" bash -c '
		for d in /v/*; do
			s=""; [[ -f "${d}/${1}" ]] && read -r s <"${d}/${1}"
			printf "%s|%s|%s|%s\n" "${d##*/}" "$(du -sb "${d}" | cut -f1)" "${s}" "$(zig version)"
		done' _ "${zigStamp}" 2>/dev/null || true)"
	"${docker}" rm "${tidy}" >/dev/null 2>&1 || true
	while IFS='|' read -r name size stamped zigNow; do
		why=""
		[[ "${size}" =~ ^[0-9]+$ ]] || continue
		if [[ "${name}" == "${zigVolume}" && -n "${stamped}" && "${stamped}" != "${zigNow}" ]]; then why="filled by zig ${stamped}"
		elif ((size > capBytes)); then why="over ${CLEANUP_CACHE_CAP_MB:-2048} MB"; fi
		[[ -n "${why}" ]] || continue
		if "${docker}" volume rm "${name}" >/dev/null 2>&1; then fEcho_Clean "cleanup: removed the volume ${name}, ${why}"
		else fEcho_Clean "cleanup: left the volume ${name} (${why}), still in use"; fi
	done <<<"${seen}"
}
## fContainerEnd <status>: at the end of a run, failed or stopped too. The run's
## containers go by its label, unless --keep-temp. Then untagged images of ours,
## older tags once this run has passed, the cache volume rule, and build cache
## over a week old, every project's, so the next build stays warm. All of it is
## done before a word is printed, since Ctrl+C takes the log tee down too, and a
## write to it then ends the run on the spot.
fContainerEnd(){
	local said=""
	((dockerUsed)) && [[ -n "${runName}" && -z "${SILK_CICD_IN_CONTAINER:-}" ]] || return 0
	dockerUsed=0
	said="$(fContainerEnd_Work "${1:-0}" 2>&1)" || true
	[[ -z "${said}" ]] || printf '%s\n' "${said}"
}
fContainerEnd_Work(){
	local -ri rc="${1:-0}"
	local id freed="" before after line kind
	local -a ids=() names=()
	local -A was=()
	mapfile -t ids < <("${docker}" ps -aq --filter "label=dev.run=${runName}" --filter "label=${projectLabel}" 2>/dev/null | grep . || true)
	if ((keepTemp)); then
		mapfile -t names < <("${docker}" ps -a --filter "label=dev.run=${runName}" --filter "label=${projectLabel}" --format '{{.Names}}' 2>/dev/null | grep . || true)
		((! ${#names[@]})) || fEcho_Clean "kept the run's containers (--keep-temp): ${names[*]}"
	elif ((${#ids[@]})); then
		"${docker}" stop -t 10 "${ids[@]}" >/dev/null 2>&1 || true
		"${docker}" rm "${ids[@]}" >/dev/null 2>&1 || true
		fEcho_Clean "cleanup: stopped and removed ${#ids[@]} of the run's containers"
	fi
	while IFS= read -r id; do
		[[ -z "${id}" ]] || "${docker}" image rm "${id}" >/dev/null 2>&1 || true
	done < <("${docker}" image ls -q --filter dangling=true --filter "label=${projectLabel}" 2>/dev/null || true)
	if ((rc == 0)) && [[ -n "${containerImage}" ]]; then fContainerTags "${containerImage}"; fi
	if ((! keepTemp)) && [[ -n "${containerImage}" ]]; then fContainerVolumes; fi
	freed="$("${docker}" builder prune -f --filter until=168h 2>/dev/null | sed -n 's/^Total reclaimed space: *//p' || true)"
	fEcho_Clean "docker build cache over a week old pruned${freed:+, ${freed} freed}"
	after="$("${docker}" system df --format '{{.Type}}|{{.Size}}' 2>/dev/null || true)"
	while IFS='|' read -r kind line; do [[ -z "${kind}" ]] || was[${kind}]="${line}"; done <<<"${dockerDfBefore}"
	line=""
	while IFS='|' read -r kind before; do
		[[ -n "${kind}" ]] || continue
		line+="${line:+, }${kind} ${was[${kind}]:-?} -> ${before}"
	done <<<"${after}"
	[[ -z "${line}" ]] || fEcho_Clean "docker: ${line}"
}
## fContainerRun <image> <script> <options...>: a pipeline script in the container,
## the engine or the ASan run, against this tree. Its /tmp is a new folder under
## the target dir each run, so a failed test's files are still there afterward,
## the X sockets inside never meet this box's, and no lock or socket a killed run
## left is there for the next. A run that passes removes it; a later run removes
## the ones failed runs kept. The container starts as root only long enough to
## give this uid a passwd entry, since the terminal looks its user up (a bare
## --user has none), and the X socket folder, then drops to it. The run gets a
## session bus of its own, which xfwm4 needs, and this box's time zone, so a name
## stamped inside sorts with the ones made here.
fContainerRun(){
	local -r image="${1}" script="${2}"; shift 2
	local gitCommon zone="${TZ:-}" runTmp="" runToken="" why="" step="${script##*/}"
	local -i rc=0
	local -a labels=() rmArg=(--rm) keepArgs=()
	step="${step%.bash}"; [[ "${1:-}" != --backlog-run-quick ]] || step=gate; [[ "${step}" != cicd ]] || step=engine
	## --keep-temp: a test folder handed in is never removed, so the run inside
	## keeps its own too.
	((! keepTemp)) || { rmArg=(); keepArgs=(-e "SILKTERM_TEST_DIR=/tmp/test_${EXE_NAME}_${step}"); }
	fDockerStart
	mapfile -t labels < <(fLabels)
	fContainerVolume "${cacheVolume}"; fContainerVolume "${zigVolume}"
	fSweep_MakeDir "${containerTarget}/tmp" run runTmp runToken || return 1
	fSweep_Record "${containerTmpLedger}" "${runTmp}" "${runToken}" || true
	## Zig's global cache on a volume of its own, stamped with the Zig that filled it.
	local -a mounts=(-v "${root}:${root}" -v "${cacheVolume}:/cache" -v "${zigVolume}:/zig-cache" -v "${runTmp}:/tmp") zoneArgs=()
	if [[ -z "${zone}" ]]; then
		zone="$(realpath -e /etc/localtime 2>/dev/null || true)"
		if [[ "${zone}" == */zoneinfo/* ]]; then zone="${zone#*/zoneinfo/}"; else zone=""; fi
	fi
	if [[ -n "${zone}" ]]; then zoneArgs=(-e "TZ=${zone}"); fi
	[[ "${containerTarget}" == "${root}/"* ]] || mounts+=(-v "${containerTarget}:${containerTarget}")
	gitCommon="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
	[[ -z "${gitCommon}" || "${gitCommon}" == "${root}/"* ]] || mounts+=(-v "${gitCommon}:${gitCommon}")
	"${docker}" run "${rmArg[@]}" --init --shm-size 1g --cpus "${CICD_MAX_JOBS}" --cpu-shares 256 --name "${EXE_NAME}-${runName}-${step}" "${labels[@]}" "${mounts[@]}" -w "${root}" \
		-e "USER=${USER:-$(id -un)}" -e "CARGO_TARGET_DIR=${containerTarget}" -e "SILK_BUILD_MINUTES=${SILK_BUILD_MINUTES:-}" -e "SILK_RELEASE=${SILK_RELEASE:-0}" \
		-e "CICD_MAX_JOBS=${CICD_MAX_JOBS}" -e "CICD_CORES=${CICD_CORES}" -e SILK_CICD_STEP_AT -e SILK_CICD_STEP_TOTAL -e XDG_RUNTIME_DIR=/tmp/xdg-runtime -e ZIG_GLOBAL_CACHE_DIR=/zig-cache "${zoneArgs[@]}" "${keepArgs[@]}" \
		"${image}" bash -c 'echo "${USER}:x:${1}:${2}:${USER}:${HOME}:/bin/bash" >>/etc/passwd && echo "${USER}:x:${2}:" >>/etc/group \
			&& mkdir -m 700 -p "${XDG_RUNTIME_DIR}" && chown "${1}:${2}" "${XDG_RUNTIME_DIR}" && mkdir -m 1777 -p /tmp/.X11-unix \
			&& { [[ -s "/zig-cache/${3}" ]] || zig version >"/zig-cache/${3}" || true; } && chown "${1}:${2}" /zig-cache \
			&& exec setpriv --reuid="${1}" --regid="${2}" --init-groups -- dbus-run-session -- bash "${@:4}"' _ "$(id -u)" "$(id -g)" "${zigStamp}" "${script}" "${@}" \
		|| rc=$?
	if ((rc)); then
		echo "the container's /tmp is kept in ${runTmp}" >&2
	elif ((keepTemp)); then
		echo "kept the container's /tmp (--keep-temp): ${runTmp}" >&2
	else
		why="$(fSweep_NotOurs "${runTmp}" "${runToken}")"
		if [[ -z "${why}" ]]; then fSafeRm "${runTmp}" || true
		else echo "cleanup: left ${runTmp} in place: ${why}" >&2; fi
	fi
	return "${rc}"
}
## What the run in the container made, named as stage 5 names them, for the stages
## here. Every binary this configuration builds has to be there: the run inside made
## them all, or it failed and this was never reached.
fContainerArtifacts(){
	local t rest osarch art
	[[ -f "${RELEASE_NATIVE_BIN}" ]] || fDie "the container run left no native release binary: ${RELEASE_NATIVE_BIN}"
	builtArts=("${RELEASE_NATIVE_OSARCH:-native}|${RELEASE_NATIVE_BIN}")
	if ((BUILD_CROSS)) && ((${#CROSS_TARGETS[@]})); then
		for t in "${CROSS_TARGETS[@]}"; do
			rest="${t#*|}"; osarch="${rest%%|*}"; rest="${rest#*|}"; art="${rest%%|*}"
			[[ -f "${art}" ]] || fDie "the container run left no ${t%%|*} binary: ${art}"
			builtArts+=("${osarch}|${art}")
		done
	fi
	ver="$(sed -n 's/^version *= *"\(.*\)".*/\1/p' "${root}/${VERSION_MANIFEST}" | head -1)"
	artDir="${root}/${RELEASE_ARTIFACT_DIR:-cicd/artifacts/release}"
}
containerImage=""; containerTarget=""; containerTmpLedger=""
if ((container)); then
	[[ -z "${SILK_CICD_IN_CONTAINER:-}" ]] || fDie "already in the container; --container would start another"
	command -v "${docker}" >/dev/null 2>&1 || fDie "--container needs docker, or SILK_DOCKER naming a stand-in"
elif ((! noContainer)) && [[ -z "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	fEcho_Clean "no docker here, so stages 1-6 run on this box with whatever it has installed"
fi
## The ASan run needs the nightly, which only the image has, so a full run on this
## box does that one step in the image. Without docker it's skipped with a warning,
## like the other steps missing a tool, so a plain run needs no extra flags.
asanInImage=0; asanSkipped=""
if ((! container && ! gate && ! quick)) && ((${#ASAN_CMD[@]})) && [[ -z "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	if command -v "${docker}" >/dev/null 2>&1; then asanInImage=1
	else asanSkipped="no docker here for the image with the nightly"; fi
fi
## The TSan run the same way, on the same nightly, under --long.
tsanInImage=0; tsanSkipped=""
if ((! container && ! gate && ! quick && long)) && ((${#TSAN_CMD[@]})) && [[ -z "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	if command -v "${docker}" >/dev/null 2>&1; then tsanInImage=1
	else tsanSkipped="no docker here for the image with the nightly"; fi
fi
## CodeQL, one of the leak checks, is in the image only too. Off unless --codeql
## or --long asks, since a run takes over half an hour and 19 GB here
## (2026102211462005).
codeqlInImage=0; codeqlSkipped=""
if ((! container && leakcheck && codeql)) && [[ -n "${CODEQL_SUITE:-}" && -z "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	if command -v "${docker}" >/dev/null 2>&1; then codeqlInImage=1
	else codeqlSkipped="no docker here for the image with CodeQL"; fi
fi
if ((container || asanInImage || tsanInImage || codeqlInImage)); then
	[[ -f "${containerRecipe}" ]] || fDie "missing ${containerRecipe}"
	containerImage="$(fContainerImage)"
	## Its real path, since a target dir linked to another disk would dangle inside.
	containerTarget="$(mkdir -p "${TARGET_DIR}" && cd -P "${TARGET_DIR}" && pwd -P)"
	containerTmpLedger="${containerTarget}/.cicd-container-tmp"
fi
if ((container)); then
	## The engine inside takes this run's stage options and none of the stages that
	## need this box: sync, the private runner, dogfood, the demo and publish.
	innerArgs=(-y --no-sync --no-dogfood --no-publish --no-private --no-demo "${innerArgs[@]}")
	((quiet)) && innerArgs[0]=-q
fi

## One folder for every file the tests write, made before the gate, or once a full
## run is past its prompt, so every test shares it. Not before the prompt: a run
## stopped there would keep an empty one. TMPDIR stays put, so builds and packaging
## keep the system temp dir.
# shellcheck source=cicd/tests/_testdir.bash
source "${root}/cicd/tests/_testdir.bash"

## The script lints and the docs checks, in stage 3 and in --backlog-run-quick.
fLintScripts(){
	## First-party shell scripts, at warning level.
	if command -v shellcheck >/dev/null 2>&1; then
		mapfile -t shellFiles < <(git -C "${root}" ls-files '*.bash' '*.sh' cicd/utility/n8git_backup-and-publish utility/git-hooks/pre-commit utility/git-hooks/pre-push utility/runterm)
		(cd "${root}" && shellcheck -S warning "${shellFiles[@]}") || fDie "shellcheck found problems"
		fEcho "OK: shell scripts clean"
		styleOut="$("${root}/cicd/utility/bash-style.bash" 2>&1)" || { fEcho_Clean "${styleOut}"; fDie "shell scripts drift from the house style (cicd/utility/bash-style.bash)"; }
		fEcho "OK: shell scripts in house style"
	else
		fEcho "WARNING: shellcheck not installed; shell scripts not linted"
	fi
	## First-party PowerShell scripts, at warning level too.
	if command -v pwsh >/dev/null 2>&1; then
		psRc=0
		pwsh -NoProfile -NonInteractive -File "${root}/cicd/utility/ps-lint.ps1" || psRc=$?
		case "${psRc}" in
			0) fEcho "OK: PowerShell scripts clean" ;;
			2) fEcho "WARNING: PSScriptAnalyzer not installed; PowerShell scripts not linted" ;;
			*) fDie "PSScriptAnalyzer found problems" ;;
		esac
	else
		fEcho "WARNING: pwsh not installed; PowerShell scripts not linted"
	fi
	## First-party Python scripts, with the rules in ruff.toml and mypy.ini.
	pyRc=0
	python3 "${root}/cicd/utility/py-lint.py" || pyRc=$?
	case "${pyRc}" in
		0) fEcho "OK: Python scripts clean" ;;
		2) fEcho "WARNING: ruff not installed; Python scripts not linted" ;;
		*) fDie "the Python lint found problems" ;;
	esac
}
fCheckDocs(){
	## Every table of contents, which no markdown linter regenerates. design.md had
	## been missing eight of its headings.
	fRunTest cicd/tests/toc/run.py "tables of contents" "a table of contents is out of date - run cicd/tests/toc/run.py --fix"
	## Every markdown table, laid out as comparison.md's generated one is. Hand-written
	## ones had drifted to trailing pipes and ragged columns.
	fRunTest cicd/tests/tables/run.py "markdown tables" "a markdown table is not canonical - run cicd/tests/tables/run.py --fix"
	if ((listing)); then [[ ! -x "${root}/cicd/tests/docs/run.py" ]] || fListTest cicd/tests/docs/run.py "markdown spacing, banner rules, doc comments"; return 0; fi
	## Blank lines between top-level bullets and around headings, which no markdown
	## linter here checks. The README and style guide had drifted. Also banner rules
	## in .rs comments, and plain comments on pub items.
	if [[ -x "${root}/cicd/tests/docs/run.py" ]]; then
		fEcho_Clean "markdown spacing, banner rules, doc comments ..."
		docsOut="$("${root}/cicd/tests/docs/run.py" 2>&1)" || { echo "${docsOut}"; fDie "a markdown file is missing a blank line, or a .rs file has a banner rule or a plain comment on a pub item - see above ($(fTestId cicd/tests/docs/run.py))"; }
		fEcho "OK: markdown spacing, banner rules, doc comments ($(fTestId cicd/tests/docs/run.py))"
	fi
	## Every test carries an ID, and no two share one.
	if [[ -x "${root}/cicd/utility/test-id.py" ]]; then
		fEcho_Clean "test IDs ..."
		testIds="$("${root}/cicd/utility/test-id.py" --check 2>&1)" || { echo "${testIds}"; fDie "a test has no ID or shares one - make IDs with cicd/utility/test-id.py"; }
		fEcho "OK: test IDs"
	fi
}

## The advisory scan's own tests, run after the scan.
fAdvisoryTests(){
	fRunTest_MaySkip cicd/tests/advscope/run.bash "advisory scan scope"
	fRunTest_MaySkip cicd/tests/yankfresh/run.bash "advisory scan sees new yanks"
}
## Software rendering on an X server with no shared pixmaps, at launch and switched
## on later. It needs a compositor drawing on the card, so it runs on this box, and
## a container run takes it up here once the container is done. Exit 3 is a skip:
## no binary, sway, Xwayland with DRI3, render node or tools.
fRenderNodeTest(){
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then fEcho_Clean "the render node test runs outside the container"; return 0; fi
	fRunTest_MaySkip cicd/tests/swnoshm/run.bash "software rendering without shared pixmaps"
}
## Graphical scenarios on the Windows boxes. Neither box is build hardware, so an
## unreachable or locked one is reported and stepped over; a scenario that actually
## ran and failed aborts. Skipped under --quick and --no-remote, and inside the
## container, which has no way to the boxes; the run outside takes it up once the
## container is done.
fWinGuiScenarios(){
	if ((quick)) || [[ -z "${WINGUI_HARNESS+x}" ]] || ((${#WINGUI_HARNESS[@]} == 0)) || [[ ! -x "${root}/${WINGUI_HARNESS[0]}" ]]; then return 0; fi
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then fEcho_Clean "windows gui scenarios run outside the container"; return 0; fi
	if ((noRemote)); then fEcho_Clean "windows gui scenarios skipped (--no-remote)"; return 0; fi
	if ((listing)); then fListTest "${WINGUI_HARNESS[0]}" "windows gui scenarios"; return 0; fi
	fEcho_Clean "windows gui scenarios ..."
	local -ri before="$(fSkipCount)"
	if SILK_CICD_SKIPS="${skipLog}" "${root}/${WINGUI_HARNESS[0]}" "${WINGUI_HARNESS[@]:1}"; then
		fStepDone "windows gui scenarios" "${before}"
	else
		fDie "a windows gui scenario failed"
	fi
}
## The Linux ARM64 build's --version on the arm64 VM, held, woken and put back
## through the VM state helper. Its first line has to match the native build's
## name, version and build number. A box that isn't there is a skip, with its SKIP
## line in this run's list. Skipped under --quick, --no-remote and --no-arm, and
## inside the container, which has no way to the boxes; the run outside takes it up.
fArmSmoke(){
	local pair art="" expect="" rc=0
	if ((quick || listing)); then return 0; fi
	for pair in "${builtArts[@]}"; do [[ "${pair%%|*}" != linux-arm64 ]] || art="${pair#*|}"; done
	[[ -n "${art}" ]] || return 0
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then fEcho_Clean "arm64 smoke run runs outside the container"; return 0; fi
	if ((noRemote)); then fEcho_Clean "arm64 smoke run skipped (--no-remote)"; return 0; fi
	fEcho_Clean "arm64 smoke run ..."
	expect="$(env -u DISPLAY -u WAYLAND_DISPLAY "${RELEASE_NATIVE_BIN}" --version </dev/null | head -1 | cut -d' ' -f1-4)" || true
	[[ "${expect}" == *" build "* ]] || fDie "the native build's --version gave no build line: '${expect}'"
	local -ri before="$(fSkipCount)"
	SILK_CICD_SKIPS="${skipLog}" "${here}/utility/arm-smoke.bash" "${art}" "${expect}" </dev/null || rc=$?
	case "${rc}" in
		0|3) fStepDone "arm64 smoke run" "${before}" ;;
		*) fDie "the Linux ARM64 build's --version failed on the arm64 box" ;;
	esac
}
## Stage 3's test scripts and harnesses. --list goes through it too, with each
## test printing its line rather than running.
fScriptTests(){
	## A release may only publish what was built from the source being tagged.
	fRunTest cicd/tests/release/run.bash "release provenance"
	## The release notes link every download by name, and only those uploaded.
	fRunTest cicd/tests/release-notes/run.bash "release notes table" "release notes test failed"
	## Installer and rig hygiene: no secret on a command line, no plain-http
	## redirect, no adopting somebody else's directory in a shared temp folder.
	fRunTest cicd/tests/install/run.bash "installer hygiene"
	## The packaging step and the Windows pipeline both look for binaries stage 5
	## built, and CARGO_TARGET_DIR decides where those are.
	fRunTest cicd/tests/packaging/run.bash "packaging paths" "packaging path test failed"
	## Renaming the project has to leave a tree that still builds. Skipped under
	## --quick: it clones the repository.
	((quick)) || fRunTest cicd/tests/rename/run.bash "project rename"
	## Every file a test writes goes under the run folder. Skipped under --quick: it
	## runs the Rust tests again.
	((quick)) || fRunTest cicd/tests/testdir/run.bash "test run folder"
	## The git hooks act on a commit or a push, where a mistake is awkward to undo.
	fRunTest cicd/tests/hooks/run.bash "git hooks" "git hook test failed"
	## The publish script commits and pushes, so nothing may reach a shell inside it.
	fRunTest cicd/tests/publish/run.bash "publish script safety"
	## The harness's own exit code, which once printed OK after running no scenes.
	fRunTest cicd/tests/scroll/verdict-test.bash "scroll harness verdict"
	## The demo recorder's own window manager session, which once wrote over the
	## desktop's settings and outlived the recording.
	fRunTest cicd/tests/demo/run.py "demo recorder session"
	## The showdown table writers, which once took quick, scaled and wrong-grid runs.
	fRunTest cicd/tests/showdown/run.py "showdown table writers" "showdown table test failed"
	## Every measured row of that table has a rig entry that can take it again.
	fRunTest cicd/tests/showdown/rigs.py "showdown rig entries" "showdown rig entry test failed"
	## The startup gates, which once marked a run as seen while it was being written.
	fRunTest cicd/tests/gates/run.bash "startup gates" "startup gate test failed"
	## The test ID tool makes IDs in the new form and the check refuses any other.
	fRunTest cicd/tests/testid/run.py "test IDs" "test ID test failed"
	## Old config files converted by the program just built: in place where shcl
	## can migrate them, written new where it cannot, the old file kept either way.
	## Exit 3 means there was no binary to run, which is a skip and never an OK.
	fRunTest_MaySkip cicd/tests/config-convert/run.bash "config conversion" "config conversion skipped, no binary to run"
	## --quick skips the window tests below that take 30 s to 2 min each. The
	## rest take 25 s or less.
	## The window shows at the size it keeps, maximized and fullscreen too, with
	## no jump after. Exit 3 is a skip: no binary, display, window manager or python-xlib.
	fRunTest_MaySkip cicd/tests/startsize/run.bash "window size at launch"
	## A window put back on a monitor that went dark and came back takes that
	## monitor's size again. Exit 3 is a skip: no binary, sway, Xwayland or python3.
	fRunTest_MaySkip cicd/tests/monwake/run.bash "window size after a monitor wakes"
	## On native Wayland a window takes each output's own size once moved or
	## dropped there, a sliver left on another output too. Exit 3 is a skip: no
	## binary, sway, python3 or software Vulkan.
	((quick)) || fRunTest_MaySkip cicd/tests/monwl/run.bash "window size per Wayland output"
	## On native Wayland the clipboard and the primary are Wayland's, both ways,
	## and nothing calls an X server. Exit 3 is a skip: no binary, sway, wtype,
	## python3 or software Vulkan.
	fRunTest_MaySkip cicd/tests/wlclip/run.bash "clipboard on Wayland"
	## The same on random layouts, scales and steps, judged against a model of
	## what the window should do. A failure prints its seed and the line that runs
	## it again. Exit 3 is a skip: no binary, sway, wtype, xdotool or software Vulkan.
	if ((! quick)) && [[ "${DPIRAND_SECS:-0}" != 0 ]]; then
		SILK_DPIRAND_SECS="${DPIRAND_SECS}" fRunTest_MaySkip cicd/tests/dpirand/run.py "window size per monitor, random cases"
	fi
	## A save after the config was deleted writes it again, keeping what it had.
	## Exit 3 is a skip: no binary, display or xdotool.
	fRunTest_MaySkip cicd/tests/delcfg/run.bash "save after the config was deleted"
	## The wallpaper is held at the size it is drawn at, and again after a resize.
	## Exit 3 is a skip: no binary, display or xdotool.
	((quick)) || fRunTest_MaySkip cicd/tests/wpresize/run.bash "wallpaper held at window size"
	## A window waking from the idle release shows a small copy of its wallpaper
	## until the real one is prepared again. Exit 3 is a skip: no binary, display,
	## window manager or tools.
	fRunTest_MaySkip cicd/tests/wakepic/run.bash "wallpaper kept through an idle wake"
	## A prepared wallpaper is kept on disk and the next launch reads it back.
	## Exit 3 is a skip: no binary, display or xdotool.
	((quick)) || fRunTest_MaySkip cicd/tests/wpkept/run.bash "prepared wallpaper kept on disk"
	## A save in one window shows in another on the same settings file, and in
	## no window on another file. Exit 3 is a skip: no binary, display, xdotool,
	## xwd or ImageMagick.
	((quick)) || fRunTest_MaySkip cicd/tests/livecfg/run.bash "settings shared between windows"
	## The minimap column comes and goes with what runs, when it starts and not
	## at the next redraw. Exit 3 is a skip: no binary, display, dash or xdotool.
	((quick)) || fRunTest_MaySkip cicd/tests/mapfollow/run.bash "minimap column follows what runs"
	## A tab names a command that prints nothing, with the cursor not blinking.
	## Exit 3 is a skip: no binary, display, dash, xdotool or xprop.
	fRunTest_MaySkip cicd/tests/quiettab/run.bash "tab names a quiet command"
	## Heap allocations per frame at rest and in a drag select, against a limit.
	## Exit 3 is a skip: no debug binary, display, window manager or xdotool.
	fRunTest_MaySkip cicd/tests/allocs/run.bash "allocations per frame"
	fRenderNodeTest
	## This script's own steps: the build retry, the dogfood tag, the options, the
	## running-copy check, the build number and the host line.
	fRunTest cicd/tests/engine/run.bash "pipeline steps" "pipeline step test failed"
	## --container: the image tag, the recipe's pins, what goes in with the run and
	## what the run here picks up after it.
	fRunTest cicd/tests/container/run.bash "container run" "container run test failed"
	## What a run removes of what earlier runs left, and what it never touches.
	fRunTest cicd/tests/cleanup/run.bash "stale run cleanup"
	## A failed test's output, which once went nowhere and left only its name.
	fRunTest cicd/tests/testout/run.bash "failed test output"
	## Stage 0, which has to stop a diverged tree before anything is built.
	fRunTest cicd/tests/sync/run.bash "remote sync"
	## The rotation that prunes run logs and flamegraphs.
	fRunTest cicd/tests/rotate/run.bash "log rotation"
	## The guard every recursive removal goes through.
	fRunTest cicd/tests/saferm/run.bash "safe removal"
	## One tool pin list for both pipelines, and the docs quoting it.
	fRunTest cicd/tests/pins/run.bash "tool pins" "tool pin test failed"
	fRunTest cicd/tests/jobs/run.bash "step jobs and priority" "step jobs test failed"
	## The PowerShell lint this stage gates on has to fail on a finding.
	fRunTest cicd/tests/pslint/run.bash "PowerShell lint"
	## The Python lint, the same way.
	fRunTest cicd/tests/pylint/run.bash "Python lint"
	## The Windows runner, which steps over a box that is off.
	fRunTest cicd/tests/win-remote/run.bash "windows runner"
	## The arm64 smoke run, which wakes the VM and puts it back.
	fRunTest cicd/tests/armsmoke/run.bash "arm64 smoke runner"
	## The parts of the Windows pipeline that run anywhere.
	fRunTest cicd/tests/cicd-win/run.bash "windows pipeline pieces" "windows pipeline test failed"
	fCheckDocs
	## The Windows scenario harness, which once tested whatever the box last built
	## and stopped every SilkTerm on a shared box.
	fRunTest cicd/tests/wingui/harness-test.bash "windows scenario harness"
	## The wine launcher, which once left dead file types on the desktop.
	fRunTest cicd/tests/wine/run.bash "wine launcher"
	## The wallpaper gallery and contact sheet are rendered, so they go stale in
	## silence when the pack changes. Nine removed images sat in both for a month.
	if [[ -f "${root}/cicd/utility/wallpaper-gallery.bash" ]] && ((listing)); then
		fListTest cicd/utility/wallpaper-gallery.bash "wallpaper gallery"
	elif [[ -f "${root}/cicd/utility/wallpaper-gallery.bash" ]]; then
		fEcho_Clean "wallpaper gallery ..."
		bash "${root}/cicd/utility/wallpaper-gallery.bash" --check || fDie "the wallpaper gallery does not match the pack"
		fEcho "OK: wallpaper gallery"
	fi
	fWinGuiScenarios
	## Headless scroll regression harness (slow; skipped under --quick). A measured
	## regression aborts here. Exit 3 means it could not run at all (no Xvfb, cage or
	## binary), which is a skip for that arm and never an OK.
	if ((! quick)) && [[ -n "${SCROLL_HARNESS+x}" ]] && ((${#SCROLL_HARNESS[@]})) && ((listing)); then
		fListTest "${SCROLL_HARNESS[0]}" "scroll harness (X11)"
		[[ "${SCROLL_HARNESS_WAYLAND:-0}" != 1 ]] || fListTest "${SCROLL_HARNESS[0]}" "scroll harness (Wayland)"
	elif ((! quick)) && [[ -n "${SCROLL_HARNESS+x}" ]] && ((${#SCROLL_HARNESS[@]})); then
		fEcho_Clean "scroll regression harness (headless, X11) ..."
		scrollRc=0; "${root}/${SCROLL_HARNESS[0]}" "${SCROLL_HARNESS[@]:1}" || scrollRc=$?
		case "${scrollRc}" in
			0) fEcho "OK: scroll harness (X11) ($(fTestId "${SCROLL_HARNESS[0]}"))" ;;
			3) fEcho "WARNING: scroll harness (X11) skipped, nothing was measured" ;;
			*) fDie "scroll regression harness reported a regression (X11)" ;;
		esac
		if [[ "${SCROLL_HARNESS_WAYLAND:-0}" == 1 ]]; then
			fEcho_Clean "scroll regression harness (headless, Wayland) ..."
			scrollRc=0; "${root}/${SCROLL_HARNESS[0]}" "${SCROLL_HARNESS[@]:1}" --wayland || scrollRc=$?
			case "${scrollRc}" in
				0) fEcho "OK: scroll harness (Wayland) ($(fTestId "${SCROLL_HARNESS[0]}"))" ;;
				3) fEcho "WARNING: scroll harness (Wayland) skipped, nothing was measured" ;;
				*) fDie "scroll regression harness reported a regression (Wayland)" ;;
			esac
		fi
	elif ((quick && ! listing)); then
		fEcho_Clean "scroll harness skipped (--quick)"
	fi
	## Dogfood launcher: it shares a path with the release installer, so what it does
	## to a file it did not create is worth a gate. Runs in a sandboxed HOME.
	if [[ -n "${LAUNCHER_HARNESS+x}" ]] && ((${#LAUNCHER_HARNESS[@]})) && ((listing)); then
		fListTest "${LAUNCHER_HARNESS[0]}" "launcher harness"
	elif [[ -n "${LAUNCHER_HARNESS+x}" ]] && ((${#LAUNCHER_HARNESS[@]})); then
		if command -v pwsh >/dev/null 2>&1; then
			fEcho_Clean "dogfood launcher harness ..."
			if pwsh -NoProfile -File "${root}/${LAUNCHER_HARNESS[0]}" "${LAUNCHER_HARNESS[@]:1}"; then
				fEcho "OK: launcher harness ($(fTestId "${LAUNCHER_HARNESS[0]}"))"
			else
				fDie "dogfood launcher harness failed"
			fi
		else
			fEcho "WARNING: launcher harness skipped: pwsh not found"
		fi
	fi
}

## fRunEnd <status>: the EXIT trap's cleanup. --keep-temp keeps the test run
## folder, pass or fail, and says where it is.
fRunEnd(){
	((! ${1})) || fStep_End FAIL >&2
	fContainerEnd "${1}"
	if ((keepTemp)) && [[ -n "${SILKTERM_TEST_DIR:-}" ]]; then
		__testDirOwned=""
		echo "kept the run's temp folder (--keep-temp): ${SILKTERM_TEST_DIR}" >&2
	else
		fTestDir_End "${1}"
	fi
}

## A box or VM that isn't there is a skip, never a failure. A step that reaches
## another box hands its scripts this run's skip list as SILK_CICD_SKIPS, and each
## one adds a "SKIP <what>, <why>" line for what it couldn't run. Only those steps
## get it, so a test of one of those scripts can't add to the real list. skipLog
## is made with the test run folder.
fSkipCount(){ if [[ -s "${skipLog}" ]]; then grep -c '^SKIP ' "${skipLog}" || true; else echo 0; fi ;}

## The run's steps, in the order they run. stepsOn is what this run takes and
## counts, stepsOff "<name>|<flag or reason>" what it leaves out, for the plan. The
## run in the container takes its part only, numbered from where the run outside
## put it (SILK_CICD_STEP_AT, of SILK_CICD_STEP_TOTAL).
stepsOn=(); stepsOff=()
fStepIf(){ if [[ -z "${2:-}" ]]; then stepsOn+=("${1}"); else stepsOff+=("${1}|${2}"); fi; }
## fStepOff_Why <flag>: the flag, or the full run that turned it off.
fStepOff_Why(){ if ((fullRun)); then echo "${fullRunName}"; else echo "${1}"; fi; }
fPlanSteps(){
	local t why onHost=1
	stepsOn=(); stepsOff=()
	[[ -z "${SILK_CICD_IN_CONTAINER:-}" ]] || onHost=0
	if ((onHost)); then why=""; ((sync)) || why="$(fStepOff_Why --no-sync)"; fStepIf "Remote sync" "${why}"; fi
	((! container)) || fStepIf "Container image"
	why=""; ((${#FMT_CMD[@]})) || why="--no-fmt"; fStepIf "Format" "${why}"
	fStepIf "Debug build"
	fStepIf "Regression tests"
	if ((onHost && ! container)); then why=""; ((PROFILE_ENABLE)) || why="$( ((quick)) && echo --quick || echo --no-profile)"; fStepIf "Profiler" "${why}"; fi
	fStepIf "Release build: native"
	for t in "${allCrossTargets[@]}"; do
		if ! ((BUILD_CROSS)); then why="--no-cross"
		elif [[ " ${CROSS_TARGETS[*]:-} " == *" ${t} "* ]]; then why=""
		elif [[ "${t}" == *windows* ]] && ((noWindows)); then why="--no-windows"
		else why="$( ((quick)) && echo --quick || echo --no-arm)"; fi
		fStepIf "Release build: ${t%%|*}" "${why}"
	done
	fStepIf "Release artifacts"
	why=""; ((PACKAGE_ENABLE)) || why="$( ((quick)) && echo --quick || echo --no-package)"; fStepIf "Packages" "${why}"
	if ((! onHost)); then fStepNumbers; return 0; fi
	if ((container)); then
		fStepIf "Checks on this box"
		why=""; ((PROFILE_ENABLE)) || why="$( ((quick)) && echo --quick || echo --no-profile)"; fStepIf "Profiler" "${why}"
	fi
	if ((quick)); then why="--quick"; elif ((noRemote)); then why="--no-remote"
	elif [[ -n "${PRIVATE_RUNNER:-}" && -x "${PRIVATE_RUNNER}" ]]; then why=""
	else why="--no-private, or not checked out"; fi
	fStepIf "Private runner" "${why}"
	why=""; ((${#DOGFOOD_DESTS[@]})) || why="$(fStepOff_Why --no-dogfood)"; fStepIf "Dogfood" "${why}"
	if ! ((${DEMO_ENABLE:-0})); then why="$(fStepOff_Why --no-demo)"
	elif ((quick && ! demoAsked)); then why="--quick; --demo adds it"
	elif [[ ! -f "${root}/cicd/utility/demo-video/demo-video.py" ]]; then why="no recorder in this tree"
	else why=""; fi
	fStepIf "Demo video" "${why}"
	why=""; ((${#GIT_PUBLISH[@]})) || why="$(fStepOff_Why --no-publish)"; fStepIf "Backup + publish" "${why}"
	fStepNumbers
}
## What the run outside handed in, checked, since it ends up in arithmetic.
## Kept from the tests this run starts, which number steps of their own.
export -n SILK_CICD_STEP_AT SILK_CICD_STEP_TOTAL
stepBase=0
fStepNumbers(){
	stepBase=0; _stepTotal="${#stepsOn[@]}"
	if [[ "${SILK_CICD_STEP_AT:-}" =~ ^[0-9]{1,3}$ && "${SILK_CICD_STEP_TOTAL:-}" =~ ^[0-9]{1,3}$ ]]; then
		stepBase="${SILK_CICD_STEP_AT}"; _stepTotal="${SILK_CICD_STEP_TOTAL}"
	fi
}
## fStepAt <name>: its number in this run, or nothing when the run doesn't take it.
fStepAt(){ local -i i; for i in "${!stepsOn[@]}"; do [[ "${stepsOn[i]}" != "${1}" ]] || { echo $((stepBase + i + 1)); return 0; }; done; return 1; }
## fStepOpen <name>: the step's Starting line, or false when it's off.
fStepOpen(){
	local at; at="$(fStepAt "${1}")" || return 1
	__stepSkips="$(fSkipCount)"
	fStep_Start "${at}" "${1}"
}
## fStepClose: its closing line, SKIP when it left something out for want of a box.
fStepClose(){
	local -i n=$(( $(fSkipCount) - ${__stepSkips:-0} ))
	if ((n > 0)); then fStep_End SKIP "${n} not run, listed at the end"; else fStep_End PASS; fi
}
## The plan's lines for the steps.
fPlanSteps_Show(){
	local t
	fEcho_Clean "Steps ...............: ${#stepsOn[@]}$( ((${#stepsOff[@]})) && echo ", and these off:")"
	for t in "${stepsOff[@]}"; do fEcho_Clean "    - ${t%%|*} (${t#*|})"; done
	if ((listing)); then for t in "${!stepsOn[@]}"; do fEcho_Clean "  Step $((t + 1)) of ${#stepsOn[@]}  ${stepsOn[t]}"; done; fi
}

## --backlog-run-quick: the fast checks to run before a commit,
## every area or the ones named, fail-fast, nothing mutated, no artifacts,
## log tee or publish. Each step reads the tree as it is.
declare -A gateTitles=([fmt]="Format check" [clippy]="Lints" [tests]="Tests" [scripts]="Script lints" [docs]="Docs checks")
## Its steps are the areas named, and the rest are off.
if ((gate)); then
	for area in ${gateAreasAll}; do
		if [[ " ${gateAreas[*]} " == *" ${area} "* ]]; then continue; fi
		stepsOff+=("${gateTitles[${area}]}|not named")
	done
	for area in "${gateAreas[@]}"; do stepsOn+=("${gateTitles[${area}]}"); done
	fStepNumbers
fi
if ((gate && listing)); then
	fEcho_Clean "${APP_NAME} backlog quick run (${gateAreas[*]})$( ((container)) && echo ", in ${containerImage}"):"
	fPlanSteps_Show
	for area in "${gateAreas[@]}"; do case "${area}" in
		tests) fListRustTests ;;
		docs)  fEcho_Clean "Docs checks:"; fCheckDocs ;;
	esac; done
	exit 0
fi
if ((gate)); then
	fTestDir_Make || fDie "could not make the test run folder"
	fRunName
	trap 'rc=$?; fRunEnd "${rc}"' EXIT
	if ((container)); then
		fSection "Backlog quick run in ${containerImage}"
		fContainerBuild "${containerImage}"
		fContainerRun "${containerImage}" "${root}/cicd/cicd.bash" --backlog-run-quick "${gateAreas[@]}" || fDie "the backlog quick run in the container failed (above)"
		exit 0
	fi
	fEcho_Clean
	fEcho_Clean "${APP_NAME} backlog quick run"
	fPlanSteps_Show
	for area in "${gateAreas[@]}"; do
		fStepOpen "${gateTitles[${area}]}"
		fJobs
		case "${area}" in
			fmt)
				if ((${#FMT_CHECK_CMD[@]})); then
					"${FMT_CHECK_CMD[@]}" || fDie "format check failed (run: ${FMT_CMD[*]:-cargo fmt})"
					fEcho "OK: formatting clean"
				else
					fEcho_Clean "format check skipped (no FMT_CHECK_CMD)"
				fi ;;
			clippy)
				if [[ -n "${LINT_CMD+x}" ]] && ((${#LINT_CMD[@]})) && "${LINT_PROBE[@]}" >/dev/null 2>&1; then
					"${LINT_CMD[@]}"
					fEcho "OK: lints clean"
				else
					fEcho "WARNING: lints skipped (clippy unavailable)"
				fi ;;
			tests)
				fTestLines "${TEST_CMD[@]}"
				fEcho "OK: tests passed" ;;
			scripts)
				fLintScripts ;;
			docs)
				fCheckDocs ;;
		esac
		fStepClose
	done
	fSection "${APP_NAME} backlog quick run (${gateAreas[*]}): PASSED."
	fEcho_Clean
	exit 0
fi

## Warn (non-gating) when a pinned helper tool has drifted from TOOL_PINS, so a
## box update can't silently change pipeline results.
if declare -p TOOL_PINS &>/dev/null; then
	for pin in "${TOOL_PINS[@]}"; do
		pinName="${pin%%|*}"; pinRest="${pin#*|}"; pinVer="${pinRest%%|*}"; pinCmd="${pinRest#*|}"
		have="$(${pinCmd} 2>/dev/null | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)*' | head -1)" || have=""
		if [[ -z "${have}" ]]; then
			fEcho "WARNING: ${pinName} not found (pinned ${pinVer})"
		elif [[ "${have}" != "${pinVer}" ]]; then
			fEcho "WARNING: ${pinName} is ${have}, pinned ${pinVer} (cargo install ${pinName} --version ${pinVer} --locked, or update the pin)"
		fi
	done
fi

## Preflight: show the plan with resolved paths, then confirm.
absScript="${root}/${PROFILE_WORKLOAD_SCRIPT}"
profileDir="$(cd "${root}" && mkdir -p "${PROFILE_OUT_DIR}" 2>/dev/null; cd "${PROFILE_OUT_DIR}" 2>/dev/null && pwd || echo "${root}/${PROFILE_OUT_DIR}")"

fEcho_Clean
fEcho_Clean "${APP_NAME} local CI/CD"
fEcho_Clean
fEcho_Clean "Host ................: $(fHostLine)"
if ((container)); then fEcho_Clean "Container ...........: stages 1-6 in ${containerImage}, target dir ${containerTarget}; the rest here"; fi
fEcho_Clean "Repo root ...........: ${root}"
if ((fullRun)); then fEcho_Clean "Run .................: ${fullRunName}, every check of the default run$( ((long)) && echo ' and --long'); no sync, dogfood, demo, publish or prompt"; fi
fEcho_Clean "Remote sync .........: $( ((sync)) && echo 'fetch + fast-forward check' || echo '(skipped)')"
fEcho_Clean "Format ..............: ${FMT_CMD[*]:-(skipped)}"
fEcho_Clean "Debug build .........: ${DEBUG_BUILD_CMD[*]}"
fEcho_Clean "Tests ...............: ${TEST_CMD[*]}"
if ((${FUZZ_SECS:-0} > 0)) && ((${#FUZZ_CMD[@]})); then
	fEcho_Clean "Fuzz ................: ${FUZZ_CMD[*]} at ${FUZZ_SECS}s per target"
else
	fEcho_Clean "Fuzz ................: (skipped)"
fi
if ((${#ASAN_CMD[@]} == 0)); then
	fEcho_Clean "ASan + LSan .........: (skipped)"
elif ((quick)); then
	fEcho_Clean "ASan + LSan .........: (skipped --quick)"
elif [[ -n "${asanSkipped}" ]]; then
	fEcho_Clean "ASan + LSan .........: (skipped, ${asanSkipped})"
else
	fEcho_Clean "ASan + LSan .........: the tests again on ${SANITIZER_TOOLCHAIN}, in ${containerImage:-this container}"
fi
if ((leakcheck)); then
	if ((! codeql)); then codeqlLine="CodeQL (off; --codeql or --long adds it)"
	elif [[ -n "${codeqlSkipped}" ]]; then codeqlLine="CodeQL (skipped, ${codeqlSkipped})"
	else codeqlLine="CodeQL"; fi
	fEcho_Clean "Leak checks .........: heap growth under dhat, ${codeqlLine}, reports to ${LEAKCHECK_OUT_DIR}"
elif ((quick && ! leakAsked)); then
	fEcho_Clean "Leak checks .........: (skipped --quick; --leakcheck adds them)"
else
	fEcho_Clean "Leak checks .........: (skipped)"
fi
if ((${#TSAN_CMD[@]} == 0)); then
	fEcho_Clean "TSan ................: (skipped)"
elif ((quick)); then
	fEcho_Clean "TSan ................: (skipped --quick)"
elif ((! long)); then
	fEcho_Clean "TSan ................: (off; --long adds it)"
elif [[ -n "${tsanSkipped}" ]]; then
	fEcho_Clean "TSan ................: (skipped, ${tsanSkipped})"
else
	fEcho_Clean "TSan ................: the tests again on ${SANITIZER_TOOLCHAIN}, in ${containerImage:-this container}"
fi
if [[ -z "${WINGUI_HARNESS+x}" ]] || ((${#WINGUI_HARNESS[@]} == 0)); then
	fEcho_Clean "Windows scenarios ...: (disabled)"
elif ((quick)); then
	fEcho_Clean "Windows scenarios ...: (skipped --quick)"
elif ((noRemote)); then
	fEcho_Clean "Windows scenarios ...: (skipped --no-remote)"
elif [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	fEcho_Clean "Windows scenarios ...: (run outside the container)"
else
	fEcho_Clean "Windows scenarios ...: $(( ${#WINGUI_HARNESS[@]} > 1 ? ${#WINGUI_HARNESS[@]} - 1 : 1 )) on the Windows boxes, each on the first one free$( ((container)) && echo ', here after the container')"
fi
if ((PROFILE_ENABLE)) && [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	fEcho_Clean "Profiler ............: (runs outside the container, on the card)"
elif ((PROFILE_ENABLE)); then
	fEcho_Clean "Profiler ............: ${PROFILE_SECS}s run -> flamegraph SVG (on the card through a headless sway, or software GL on headless ${RPD_HEADLESS_DISPLAY:-:98})$( ((container)) && echo ', here after the container')"
	fEcho_Clean "  output dir ........: ${profileDir}"
	fEcho_Clean "  workload ..........: python3 ${PROFILE_WORKLOAD_SCRIPT} ${PROFILE_WORKLOAD_ARGS}"
else
	fEcho_Clean "Profiler ............: (disabled)"
fi
fEcho_Clean "Release (native) ....: ${RELEASE_NATIVE_CMD[*]} -> ${RELEASE_NATIVE_BIN}"
if ((BUILD_CROSS)) && ((${#CROSS_TARGETS[@]})); then
	fEcho_Clean "Release (cross) .....:$( ((noArm)) && echo ' (x86_64 only)')$( ((noWindows)) && echo ' (Linux only, --no-windows)')"
	for t in "${CROSS_TARGETS[@]}"; do fEcho_Clean "    - ${t%%|*}"; done
else
	fEcho_Clean "Release (cross) .....: (skipped)"
fi
## --quick has no ARM64 build either.
if ! ((BUILD_CROSS)) || [[ " ${CROSS_TARGETS[*]:-} " != *"|linux-arm64|"* ]]; then
	fEcho_Clean "ARM64 smoke run .....: (no Linux ARM64 build)"
elif ((noRemote)); then
	fEcho_Clean "ARM64 smoke run .....: (skipped --no-remote)"
elif [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	fEcho_Clean "ARM64 smoke run .....: (run outside the container)"
else
	fEcho_Clean "ARM64 smoke run .....: --version on the arm64 box, woken and put back if it's a VM$( ((container)) && echo ', here after the container')"
fi
if ((PACKAGE_ENABLE)) && ((! quick)); then
	## Name only what stage 5 will actually leave behind. The installer wraps a
	## Windows binary, so under --no-windows (or --no-cross) there is none to wrap.
	pkgKinds=".deb/.rpm (Linux)"
	if ((BUILD_CROSS)) && [[ " ${CROSS_TARGETS[*]:-} " == *windows* ]]; then
		pkgKinds="${pkgKinds} + NSIS installer .exe (Windows)"
	fi
	fEcho_Clean "Packages ............: ${pkgKinds}, per built arch"
	fEcho_Clean "  deferred ..........: BSD - no cross toolchain on this box"
else
	fEcho_Clean "Packages ............: $( ((quick)) && echo '(skipped --quick)' || echo '(disabled)')"
fi
if ((quick)); then
	fEcho_Clean "Private runner ......: (skipped --quick)"
elif ((noRemote)); then
	fEcho_Clean "Private runner ......: (skipped --no-remote)"
elif [[ -n "${PRIVATE_RUNNER:-}" && -x "${PRIVATE_RUNNER}" ]]; then
	fEcho_Clean "Private runner ......: ${PRIVATE_RUNNER} (macOS, Microsoft Store)"
else
	fEcho_Clean "Private runner ......: (not checked out)"
fi
if ((${#DOGFOOD_DESTS[@]})); then
	fEcho_Clean "Dogfood .............: install to the synced app dir per target"
	for xd in "${DOGFOOD_DESTS[@]}"; do
		xosarch="${xd%%|*}"; xrest="${xd#*|}"; xname="${xrest%%|*}"
		xdest="$(fDogfoodDest "${xrest#*|}")"
		if [[ -n "${xdest}" ]]; then fEcho_Clean "    - ${xosarch} -> ${xdest}/${xname}"
		else fEcho_Clean "    - ${xosarch} -> <none of: ${xrest#*|} writable - will skip>"; fi
	done
else
	fEcho_Clean "Dogfood .............: (disabled)"
fi
if ((! ${DEMO_ENABLE:-0})); then
	fEcho_Clean "Demo video ..........: (disabled)"
elif ((quick && ! demoAsked)); then
	fEcho_Clean "Demo video ..........: (skipped --quick)"
else
	fEcho_Clean "Demo video ..........: re-record, README gif to assets/demo.gif"
fi
if declare -p CLEANUP_BUILD_DIRS &>/dev/null; then
	fEcho_Clean "Cleanup .............: test folders$( [[ -n "${containerTmpLedger}" ]] && echo ' and container /tmp') past the newest ${CLEANUP_KEEP_RUNS:-5}, stale build dirs$( [[ -n "${containerTmpLedger}" ]] && echo ', stopped containers and old images')"
fi
if ((${#GIT_PUBLISH[@]} == 0)); then
	fEcho_Clean "Publish (last) ......: (disabled)"
elif [[ -n "${publishMsg}" ]]; then
	fEcho_Clean "Publish (last) ......: ${GIT_PUBLISH[*]} (hands-off: \"${publishMsg}\")"
else
	fEcho_Clean "Publish (last) ......: ${GIT_PUBLISH[*]} (will prompt for message; blank = \"${autoMsg}\")"
fi
fPlanSteps
fPlanSteps_Show
fEcho_Clean
fEcho_Clean "Fail-fast: any error aborts before the next step."
fEcho_Clean
if ((listing)); then
	((! ${#TEST_CMD[@]})) || fListRustTests
	if ((${FUZZ_SECS:-0} > 0)) && ((${#FUZZ_CMD[@]})); then fEcho_Clean "Fuzz soak: the ${FUZZ_CMD[*]} tests above again, ${FUZZ_SECS}s each"; fi
	fEcho_Clean "Test scripts and harnesses:"
	if [[ -n "${ADVISORY_CMD+x}" ]] && ((${#ADVISORY_CMD[@]})); then fAdvisoryTests; fi
	fScriptTests
	exit 0
fi

if ((! assumeYes)); then
	## Capture the commit message up front so the run can finish unattended. This
	## is the natural place to bail on the common (publish) path - Ctrl+C here
	## aborts; there is no separate "Proceed? [y/N]" (removed to cut friction).
	if ((${#GIT_PUBLISH[@]})) && [[ -z "${publishMsg}" ]]; then
		read -r -p "Publish commit message (blank = \"${autoMsg}\"; Ctrl+C aborts): " m
		fEcho_ResetBlankCounter
		publishMsg="$(fPublishMessage "" "" "${m}")"
	fi
fi

fTestDir_Make || fDie "could not make the test run folder"
fRunName
trap 'rc=$?; fRunEnd "${rc}"' EXIT
skipLog="${SILKTERM_TEST_DIR}/skips.txt"; : >"${skipLog}"

## Tee the rest of the run (all stages) to a gitignored log so warnings from any
## stage can be reviewed after the fact. Rotate the prior (closed) logs first.
## The awk pass normalizes section spacing on the way through: exactly one blank
## line before every letterbox rule. The blank-collapse counter can't do this -
## raw tool output (cargo, git, rar) never touches it, so a section's leading
## blank gets swallowed or doubled depending on what a tool printed last. Skip
## the insert on the stream's first line: the preflight already ends with a
## blank on the tty, which this pipe never sees.
##
## The log is written under a .part name and renamed on the way out, once tee has
## finished, so the startup gate never marks a log as seen while it is still
## being written. A failed run's log is renamed too. The wait is bounded, since a
## background process a stage left behind can hold the pipe open.
##
## Inside the container the run outside already has all of it in its own log.
fFinishLog(){
	exec 1>&3 2>&4
	local i; for i in {1..50}; do kill -0 "${lintTee}" 2>/dev/null || break; sleep 0.1; done
	mv -f "${lintLog}.part" "${lintLog}" 2>/dev/null || true
}
if [[ -n "${LINT_LOG_DIR:-}" && -z "${SILK_CICD_IN_CONTAINER:-}" ]] && mkdir -p "${root}/${LINT_LOG_DIR}" 2>/dev/null; then
	gfs_rotate "${root}/${LINT_LOG_DIR}" run log >/dev/null 2>&1 || true
	lintLog="${root}/${LINT_LOG_DIR}/run_${stamp}.log"
	exec 3>&1 4>&2
	exec > >(awk -v rule="${_letterbox:?}" '
		$0 == "" { blanks++; next }
		{
			if (index($0, rule) == 1) { if (NR > 1) print "" }
			else { for (; blanks > 0; blanks--) print "" }
			blanks = 0; print; fflush()
		}
		END { for (; blanks > 0; blanks--) print "" }
	' | tee "${lintLog}.part") 2>&1
	lintTee=$!
	## fRunEnd before fFinishLog, so their lines reach the log.
	trap 'rc=$?; fRunEnd "${rc}"; fFinishLog; exit $rc' EXIT
fi

## What earlier runs left: the test run folders failed runs kept, past the newest
## few, then this run's own goes on the list. Inside the container /tmp is new
## each run, and the run outside sees to it. The build dirs are swept where the
## builds run, at the start of fBuildTestRelease.
if [[ -z "${SILK_CICD_IN_CONTAINER:-}" ]]; then
	testLedger="${TARGET_DIR}/.cicd-test-folders"
	fSweep_Ledger "${testLedger}" "${CLEANUP_KEEP_RUNS:-5}" "old test run folders"
	ownedLine="$(fTestDir_Owned)"
	if [[ -n "${ownedLine}" ]]; then fSweep_Record "${testLedger}" "${ownedLine%%$'\t'*}" "${ownedLine#*$'\t'}" || true; fi
	if [[ -n "${containerTmpLedger}" ]]; then
		fSweep_Ledger "${containerTmpLedger}" "${CLEANUP_KEEP_RUNS:-5}" "old container /tmp folders"
		fContainerStopped
	fi
fi

## Stage 0: remote sync. Make sure the local branch can be safely refreshed from
## its upstream BEFORE spending the build: what stage 8 pushes should be what got
## built and tested here, not an untested post-build merge. Behind-only is safe
## (fast-forward, stash-wrapped for a dirty tree); diverged aborts now rather
## than at publish. Offline just warns - a local build shouldn't need the net.
fJobs
if ! fStepOpen "Remote sync"; then
	:
elif ! git rev-parse --abbrev-ref '@{u}' >/dev/null 2>&1; then
	fEcho_Clean "no upstream for $(git rev-parse --abbrev-ref HEAD); nothing to sync"
elif ! fRemoteGit fetch --quiet 2>/dev/null; then
	fEcho "WARNING: git fetch failed (offline?); continuing with the local tree"
else
	ahead="$(git rev-list --count '@{u}..HEAD')"
	behind="$(git rev-list --count 'HEAD..@{u}')"
	if ((behind == 0)); then
		if ((ahead)); then fEcho "OK: up to date with upstream (${ahead} ahead)"
		else fEcho "OK: up to date with upstream"; fi
	elif ((ahead == 0)); then
		## Behind only: a fast-forward can't lose anything. Same stash dance as
		## the publisher so a dirty tree can't block the pull.
		dirty=0
		git diff --quiet          || dirty=1
		git diff --cached --quiet || dirty=1
		[[ -n "$(git ls-files --others --exclude-standard)" ]] && dirty=1
		didStash=0
		if ((dirty)); then
			stashesBefore="$(git stash list | wc -l)"
			fEcho_Clean "git stash push --include-untracked ..."
			git stash push --include-untracked -m "auto-stash"
			stashesAfter="$(git stash list | wc -l)"
			((stashesAfter > stashesBefore)) && didStash=1
		fi
		fEcho_Clean "git pull --ff-only ..."
		fRemoteGit pull --ff-only
		if ((didStash)); then
			fEcho_Clean "git stash pop ..."
			git stash pop
		fi
		fEcho "OK: fast-forwarded ${behind} commit(s) from upstream"
	else
		fDie "diverged from upstream (${ahead} ahead, ${behind} behind) - reconcile first, or rerun with --no-sync"
	fi
fi
fStepClose

## The source everything from here on reads. Taken before the first build, because
## the note has to name what the binaries hold: another session committing in this
## tree during a build used to leave the note naming the new commit, clean, with the
## native binary holding the source from before it and the cross binaries the source
## after. Stage 0 is past, so its fast-forward is not mistaken for that.
builtFromState="$(fSourceState)"

## Pin the build number for the whole run. build.rs would otherwise read the clock
## per target, so the four cross builds of one release would report four different
## builds, minutes apart, and the release notes could not name one of them. It is
## the commit's time, dirty tree or not, so a rebuild of a commit gets the same
## number, per the CICD rules. The clock only stands in where there is no commit.
## After stage 0, since a fast-forward moves the commit. A value cicd-win hands
## down is kept.
fPinBuildMinutes(){
	[[ -z "${SILK_BUILD_MINUTES:-}" ]] || return 0
	local buildSecs
	buildSecs="$(git log -1 --format=%ct 2>/dev/null || true)"
	[[ "${buildSecs}" =~ ^[0-9]+$ ]] || buildSecs="$(date +%s)"
	export SILK_BUILD_MINUTES=$(( (buildSecs - 946684800) / 60 ))
}
fPinBuildMinutes

## Only a run on a clean main can be tagged (release.bash refuses anything else), so
## only its binaries drop the date that a non-release build shows after its number.
## A value handed down is kept.
fMarkRelease(){
	[[ -z "${SILK_RELEASE:-}" ]] || return 0
	if [[ "$(git rev-parse --abbrev-ref HEAD 2>/dev/null)" == "main" && -z "$(git status --porcelain --untracked-files=no 2>/dev/null || echo dirty)" ]]; then
		export SILK_RELEASE=1
	else
		export SILK_RELEASE=0
	fi
}
fMarkRelease

## The fuzz corpus is read-only data: every file in it is replayed on each run
## and chewed on by the mutator. A run that writes one through changes what
## later runs replay, and nothing else would say so - a seed just quietly stops
## being the case it was saved as. Hashed before and after, so a fixture edited
## by hand before the run is not mistaken for one the run made.
fCorpusHashes(){ find "${root}/cicd/tests/fuzz-corpus" -type f -exec sha256sum {} + 2>/dev/null | sort; }

## A headless sway drawing on the card's render node, with its own Xwayland, so an
## app on it renders on the card and nothing reaches the desktop. On a private Xvfb
## the app draws with software GL, here as in the image, so a profile there sampled
## llvmpipe's drawing as well as the app.
## fCardDisplay_Start <folder>: sets cardDisplay and cardSway, or sets cardWhy to
## why it could not and returns 1. The folder gets sway's config and log. Its
## sockets go in a folder of their own under XDG_RUNTIME_DIR, since a test folder
## under a long temp path is past the socket length limit.
cardDisplay=""; cardSway=""; cardWhy=""; cardRuntime=""
fCardDisplay_Start(){
	local -r dir="${1}"
	local tool node="" sockets
	for tool in sway Xwayland ss xdpyinfo; do
		command -v "${tool}" >/dev/null 2>&1 || { cardWhy="no ${tool}"; return 1; }
	done
	for node in /dev/dri/renderD*; do
		if [[ -r "${node}" && -w "${node}" ]]; then break; fi
		node=""
	done
	[[ -n "${node}" ]] || { cardWhy="no render node"; return 1; }
	cardRuntime=""
	if [[ -d "${XDG_RUNTIME_DIR:-}" && -w "${XDG_RUNTIME_DIR:-}" ]]; then
		cardRuntime="$(mktemp -d "${XDG_RUNTIME_DIR}/silk-sway.XXXXXX")" || cardRuntime=""
	fi
	[[ -n "${cardRuntime}" ]] || { cardWhy="no XDG_RUNTIME_DIR for its sockets"; return 1; }
	## A Unix socket path is at most 107 bytes.
	((${#cardRuntime} + 16 <= 107)) || { fCardDisplay_Stop; cardWhy="XDG_RUNTIME_DIR is too long for a socket"; return 1; }
	printf 'output HEADLESS-1 resolution 1920x1080 position 0 0\nxwayland force\ndefault_border none\n' >"${dir}/sway.conf"
	env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET XDG_RUNTIME_DIR="${cardRuntime}" WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 \
		WLR_RENDERER=gles2 WLR_RENDER_DRM_DEVICE="${node}" WLR_LIBINPUT_NO_DEVICES=1 \
		sway -c "${dir}/sway.conf" >"${dir}/sway.log" 2>&1 &
	cardSway=$!
	## sway's Xwayland takes whichever X display number is free, so it is read from
	## the socket sway itself listens on.
	cardDisplay=""
	for _ in {1..100}; do
		sockets="$(ss -xlpH 2>/dev/null || true)"
		cardDisplay="$(awk -v p="pid=${cardSway}," 'index($0, p) && $5 ~ /^\/tmp\/\.X11-unix\/X[0-9]+$/ {sub(/.*X/, ":", $5); print $5; exit}' <<<"${sockets}")"
		if [[ -n "${cardDisplay}" ]] && DISPLAY="${cardDisplay}" xdpyinfo >/dev/null 2>&1; then return 0; fi
		cardDisplay=""
		kill -0 "${cardSway}" 2>/dev/null || break
		sleep 0.1
	done
	fCardDisplay_Stop
	cardWhy="sway's Xwayland did not come up (${dir}/sway.log)"
	return 1
}
## Only the sway started above, which takes its Xwayland with it, then the sockets
## it left, by name, and their folder if that leaves it empty.
fCardDisplay_Stop(){
	if [[ -n "${cardSway}" ]]; then
		kill "${cardSway}" 2>/dev/null || true
		wait "${cardSway}" 2>/dev/null || true
	fi
	if [[ -n "${cardRuntime}" && -d "${cardRuntime}" ]]; then
		rm -f -- "${cardRuntime}"/wayland-* "${cardRuntime}"/sway-ipc.* 2>/dev/null || true
		rmdir -- "${cardRuntime}" 2>/dev/null || true
	fi
	cardSway=""; cardDisplay=""; cardRuntime=""
}

## Stage 4's profiler (non-gating artifact; failures classified below). It runs on
## this box, since the card is what it is for, so a container run calls it here
## once the container is done.
fRunProfiler(){
	((PROFILE_ENABLE)) || { fEcho_Clean "profiler disabled"; return 0; }
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then fEcho_Clean "the profiler runs outside the container, on the card"; return 0; fi

	## Mundane/environmental reasons -> skip with a warning (not the app's fault),
	## unless PROFILE_STRICT. Genuine run failures below still abort. The app runs
	## on a headless display, never the visible one: the card's through sway, or a
	## private Xvfb (gui-headless.bash) where that can't start.
	local skip=""
	command -v python3 >/dev/null 2>&1 || skip="python3 not found"
	[[ -z "${skip}" ]] && [[ ! -f "${absScript}" ]] && skip="workload missing: ${absScript}"
	if [[ -n "${skip}" ]]; then
		((PROFILE_STRICT)) && fDie "profiler: ${skip}"
		fEcho "WARNING: profiler skipped: ${skip}"; return 0
	fi

	## From here a failure is the app's fault and aborts, bar the retry fRetryBuild owns.
	fEcho_Clean "building ${PROFILE_BIN} (cargo --profile ${PROFILE_PROFILE} --features ${PROFILE_FEATURE})"
	fRetryBuild profiler cargo build --profile "${PROFILE_PROFILE}" --features "${PROFILE_FEATURE}"
	mkdir -p "${profileDir}"

	local headless="${here}/utility/gui-headless.bash" hdisp="" why="" swayDir=""
	if [[ -n "${SILKTERM_TEST_DIR:-}" ]] && swayDir="$(mktemp -d "${SILKTERM_TEST_DIR}/profiler.XXXXXX")"; then
		if fCardDisplay_Start "${swayDir}"; then hdisp="${cardDisplay}"; else why="${cardWhy}"; fi
	else
		why="no test run folder for sway"
	fi
	if [[ -n "${hdisp}" ]]; then
		fEcho_Clean "on the card, through a headless sway's Xwayland at ${hdisp}"
	else
		((PROFILE_STRICT)) && fDie "profiler: not on the card: ${why}"
		fEcho "WARNING: profiler on software GL, not the card: ${why}"
		if ! command -v Xvfb >/dev/null 2>&1; then
			fEcho "WARNING: profiler skipped: Xvfb not found (headless display unavailable)"; return 0
		fi
		## Not :99 - rapid-photo-downloader-pro uses that display for its own testing.
		export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-${RPD_HEADLESS_DISPLAY:-:98}}"
		hdisp="${CICD_HEADLESS_DISPLAY}"
		if ! "${headless}" start >/dev/null 2>&1; then
			((PROFILE_STRICT)) && fDie "profiler: headless display failed to start"
			fEcho "WARNING: profiler skipped: headless display failed to start"; return 0
		fi
	fi

	## Born canonical (role "frequent"); the rotation retags the newest as "latest".
	## The app writes the graph as it exits, so it goes under a .part name the
	## startup gate skips, and is renamed once whole.
	local out="${profileDir}/flame_${stamp}_frequent.svg"
	local part="${out}.part"
	fEcho_Clean "running app ${PROFILE_SECS}s under sampler on headless ${hdisp} ..."
	local prc=0
	## -u WAYLAND_DISPLAY: winit prefers Wayland wherever it sees one, so on a
	## Wayland session (WSLg included) DISPLAY alone leaves the window on the
	## real desktop and the profiler samples nothing.
	env -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE \
	SILK_PROFILE_OUT="${part}" SILK_PROFILE_SECS="${PROFILE_SECS}" DISPLAY="${hdisp}" \
		"${PROFILE_BIN}" --shell "python3 ${absScript} ${PROFILE_WORKLOAD_ARGS}" || prc=$?
	if [[ -n "${cardSway}" ]]; then fCardDisplay_Stop; else "${headless}" stop >/dev/null 2>&1 || true; fi
	((prc == 0)) || { rm -f "${part}"; fDie "profiler run failed (non-zero exit - app problem)"; }
	[[ -s "${part}" ]] || { rm -f "${part}"; fDie "profiler produced no SVG (app problem): ${out}"; }
	mv -f "${part}" "${out}"
	gfs_rotate "${profileDir}" flame svg
	## Rotation renamed this run's file (newest) to the "latest" role.
	local latest="${profileDir}/flame_${stamp}_latest.svg"
	[[ -e "${latest}" ]] || latest="${out}"
	fEcho "OK: flamegraph: ${latest}"
	fEcho_Clean "open: ${latest}  (in a browser)"

	## Hot-spot summary into the log (non-fatal, no marker - the marker is for the
	## per-session --check gate, not the pipeline).
	local report="${here}/utility/flame-report.py"
	if [[ -f "${report}" ]]; then
		fEcho_Clean ""
		python3 "${report}" --dir "${profileDir}" 2>/dev/null || fEcho_Clean "hot spots: (report unavailable)"
	fi
}

## True when a built file still names this box's home or checkout.
fHasLocalPaths(){ grep -a -q -F -e "${HOME}/" -e "${root}/" "${1}"; }

## Stage 6's packages. Build distributables from the stage-5 binaries (never rebuilt).
## Linux -> .deb + .rpm per built arch (cargo-deb / cargo-generate-rpm, metadata in
## source/Cargo.toml); Windows -> one self-contained NSIS installer .exe per arch
## (upgrades in place). macOS comes from the private runner; BSD is deferred.
## Skipped under --quick; a missing tool warns (non-gating) rather than aborting.
fBuildPackages(){
	((PACKAGE_ENABLE)) || { fEcho_Clean "packages disabled"; return 0; }
	[[ -n "${artDir:-}" ]] || { fEcho "WARNING: packages skipped (no RELEASE_ARTIFACT_DIR)"; return 0; }
	local pair osarch bin srcexe triple out nsi rc made=0
	local rpmver="${ver//-/\~}"   ## RPM versions forbid '-' (it splits version-release); 1.0.0-beta1 -> 1.0.0~beta1
	for pair in "${builtArts[@]}"; do
		osarch="${pair%%|*}"; bin="${pair#*|}"
		case "${osarch}" in
			linux-x86_64) triple="" ;;
			linux-arm64)  triple="aarch64-unknown-linux-gnu" ;;
			windows-*)    triple="" ;;   ## handled below
			*) continue ;;
		esac

		## Linux: .deb then .rpm. Both package the existing binary (no rebuild).
		if [[ "${osarch}" == linux-* ]]; then
			if command -v cargo-deb >/dev/null 2>&1; then
				local -a da=(deb --no-build --no-strip --manifest-path source/Cargo.toml
					--output "${artDir}/${EXE_NAME}-${ver}-${osarch}.deb")
				[[ -n "${triple}" ]] && da+=(--target "${triple}")
				if cargo "${da[@]}" >/dev/null; then fEcho "OK: .deb (${osarch})"; made=$((made+1))
				else fEcho "WARNING: .deb build failed (${osarch})"; fi
			else fEcho "WARNING: cargo-deb missing; .deb skipped (${osarch})"; fi

			if command -v cargo-generate-rpm >/dev/null 2>&1; then
				## -p is the crate DIR (source/), assets resolve from CWD (repo root),
				## so target/release/silkterm is found; -s overrides the RPM-illegal version.
				local -a ra=(generate-rpm -p source -s "version = \"${rpmver}\""
					--output "${artDir}/${EXE_NAME}-${ver}-${osarch}.rpm")
				[[ -n "${triple}" ]] && ra+=(--target "${triple}" --arch aarch64)
				if cargo "${ra[@]}" >/dev/null; then fEcho "OK: .rpm (${osarch})"; made=$((made+1))
				else fEcho "WARNING: .rpm build failed (${osarch})"; fi
			else fEcho "WARNING: cargo-generate-rpm missing; .rpm skipped (${osarch})"; fi
		fi

		## Windows: one self-contained NSIS installer .exe per arch.
		if [[ "${osarch}" == windows-* ]]; then
			if command -v makensis >/dev/null 2>&1 && [[ -f "${root}/${NSIS_TEMPLATE}" ]]; then
				out="${artDir}/${EXE_NAME}-${ver}-${osarch}-setup.exe"
				nsi="$(mktemp --suffix=.nsi)"
				## An absolute CARGO_TARGET_DIR already gives an absolute path, and
				## prefixing the repo root then names a file that was never there.
				srcexe="${bin}"
				[[ "${srcexe}" = /* ]] || srcexe="${root}/${srcexe}"
				## Four numbers for the version block: the release, less any pre-release tag.
				vernum="${ver%%[-+]*}.0"
				sed -e "s|@VERSION@|${ver}|g" -e "s|@ARCH@|${osarch}|g" \
					-e "s|@SRCEXE@|${srcexe}|g" -e "s|@OUTFILE@|${out}|g" \
					-e "s|@ICON@|${root}/source/assets/icon.ico|g" -e "s|@VERNUM@|${vernum}|g" \
					"${root}/${NSIS_TEMPLATE}" > "${nsi}"
				rc=0; makensis -INPUTCHARSET UTF8 -V2 "${nsi}" >/dev/null || rc=$?
				rm -f "${nsi}"
				if ((rc == 0)) && [[ -f "${out}" ]]; then fEcho "OK: installer (${osarch})"; made=$((made+1))
				else fEcho "WARNING: NSIS installer failed (${osarch})"; fi
			else fEcho "WARNING: makensis/template missing; installer skipped (${osarch})"; fi
		fi
	done
	fWriteSums
	fEcho "OK: ${made} package(s) -> ${RELEASE_ARTIFACT_DIR}/ (macOS/BSD deferred)"
}

## Private content scrub, when this machine has the private tree. A clone without
## it builds as before, and so does the container, which never sees that tree.
fContentScrub(){
	[[ -x "${root}/../private/hooks/scrub.bash" ]] || return 0
	"${root}/../private/hooks/scrub.bash" "${root}" || fDie "content scrub failed"
	fEcho "OK: content scrub"
}
## fStepDone <name> <skips before>: the step's own line, SKIP when it left something out.
fStepDone(){
	local -r name="${1}" before="${2}"
	local -ri skipped=$(( $(fSkipCount) - before ))
	if ((skipped)); then fEcho "SKIP: ${name}, ${skipped} not run (SKIP lines above)"
	else fEcho "OK: ${name}"; fi
}
## The run's last section, so a skip doesn't scroll away with the stage it was in.
fSkipSummary(){
	[[ -s "${skipLog}" ]] || return 0
	fEcho_Clean "Not run, since a box wasn't there:"
	grep '^SKIP ' "${skipLog}" || true
	fEcho_Clean
}

## Stage 3's test suite under ASan and LSan (cicd/utility/asan.bash). In the
## container it runs as is. On this box it runs in the image, which has the
## nightly, against this tree and target dir.
fRunAsan(){
	if ((${#ASAN_CMD[@]} == 0)); then fEcho_Clean "ASan run skipped"; return 0; fi
	if ((quick)); then fEcho_Clean "ASan run skipped (--quick)"; return 0; fi
	if [[ -n "${asanSkipped}" ]]; then fEcho "WARNING: ASan run skipped: ${asanSkipped}"; return 0; fi
	fJobs hard
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then
		"${here}/utility/asan.bash" || fDie "the test suite under ASan and LSan failed (above)"
		return 0
	fi
	fContainerBuild "${containerImage}"
	fContainerRun "${containerImage}" "${root}/cicd/utility/asan.bash" || fDie "the test suite under ASan and LSan failed (above)"
}

## Stage 3's test suite under TSan (cicd/utility/tsan.bash), after the leak
## checks, in the image like the ASan run. About 28 minutes, so only under --long.
fRunTsan(){
	if ((${#TSAN_CMD[@]} == 0)); then fEcho_Clean "TSan run skipped"; return 0; fi
	if ((quick)); then fEcho_Clean "TSan run skipped (--quick)"; return 0; fi
	if ((! long)); then fEcho_Clean "TSan run off (--long adds it)"; return 0; fi
	if [[ -n "${tsanSkipped}" ]]; then fEcho "WARNING: TSan run skipped: ${tsanSkipped}"; return 0; fi
	fJobs hard
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then
		"${here}/utility/tsan.bash" || fDie "the test suite under TSan failed (above)"
		return 0
	fi
	fContainerBuild "${containerImage}"
	fContainerRun "${containerImage}" "${root}/cicd/utility/tsan.bash" || fDie "the test suite under TSan failed (above)"
}

## The leak checks, after the ASan run. On by default; --no-leakcheck drops them
## and --quick does unless --leakcheck asks. Each report goes to LEAKCHECK_OUT_DIR,
## rotated like the flamegraphs. A test filter that matched nothing would pass, so
## a run that wrote no report fails too.
fRunLeakcheck(){
	local outDir report
	local -i rc=0
	if ((! leakcheck)); then fEcho_Clean "leak checks skipped$( ((quick && ! leakAsked)) && echo ' (--quick; --leakcheck adds them)')"; return 0; fi
	outDir="${root}/${LEAKCHECK_OUT_DIR}"
	mkdir -p "${outDir}" || fDie "could not make ${outDir}"
	report="${outDir}/heap_${stamp}.txt"
	fJobs hard
	fEcho_Clean "heap growth under dhat ..."
	SILK_HEAP_REPORT="${report}" fTestLines "${LEAKCHECK_DHAT_CMD[@]}" || rc=$?
	if [[ -s "${report}" ]]; then sed 's/^/    /' "${report}"; fi
	((rc == 0)) || fDie "the heap grew past its warm-up, or the test failed (report and dhat profile in ${outDir})"
	[[ -s "${report}" ]] || fDie "the heap-growth test wrote no report, so it didn't run"
	gfs_rotate "${outDir}" heap txt >/dev/null 2>&1 || true
	gfs_rotate "${outDir}" heap dhat.json >/dev/null 2>&1 || true
	fEcho "OK: no heap growth past the warm-up"
	fRunCodeql "${outDir}"
}
## fRunCodeql <out dir>: CodeQL over our crates and the vendored ones a release
## binary links (cicd/utility/codeql.bash), in the image like the ASan run.
fRunCodeql(){
	local -r outDir="${1}" why="CodeQL found something new, or couldn't run (above)"
	if ((! codeql)); then fEcho_Clean "CodeQL off (--codeql or --long adds it)"; return 0; fi
	if [[ -z "${CODEQL_SUITE:-}" ]]; then fEcho_Clean "CodeQL skipped"; return 0; fi
	if [[ -n "${codeqlSkipped}" ]]; then fEcho "WARNING: CodeQL skipped: ${codeqlSkipped}"; return 0; fi
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then
		"${here}/utility/codeql.bash" "${outDir}" "${stamp}" || fDie "${why}"
	else
		fContainerBuild "${containerImage}"
		fContainerRun "${containerImage}" "${root}/cicd/utility/codeql.bash" "${outDir}" "${stamp}" || fDie "${why}"
	fi
	gfs_rotate "${outDir}" codeql sarif >/dev/null 2>&1 || true
}

## The build dirs this config builds into: CLEANUP_BUILD_DIRS under the target
## dir, and each cross target's release dir.
fBuildDirs(){
	local dir t art
	for dir in "${CLEANUP_BUILD_DIRS[@]}"; do printf '%s/%s\n' "${TARGET_DIR}" "${dir}"; done
	for t in "${allCrossTargets[@]}"; do
		art="${t#*|}"; art="${art#*|}"; art="${art%%|*}"
		printf '%s\n' "${art%/*}"
	done
}
## Stale build output in the target dir, swept where the builds run, so a container
## run sweeps its own target dir with its own toolchain.
fSweepBuilds(){
	local key="" ver="" dir
	local -a dirs=()
	declare -p CLEANUP_BUILD_DIRS &>/dev/null || return 0
	local -a nightlyDirs=()
	mapfile -t dirs < <(fBuildDirs)
	## The nightly's build dirs are named for it, so a bump leaves the old one
	## unnamed, and a change of the stable toolchain has nothing to do with them.
	for dir in "${CLEANUP_NIGHTLY_DIRS[@]}"; do nightlyDirs+=("${TARGET_DIR}/${dir}"); done
	if ver="$(rustc -vV 2>/dev/null)"; then key="$(sha256sum <<<"${ver}" | cut -c1-16)"; fi
	fSweep_Toolchain "${TARGET_DIR}" "${key}" "${dirs[@]}"
	fSweep_OldBuildDirs "${TARGET_DIR}" "${dirs[@]}" "${nightlyDirs[@]}"
	fSweep_Incremental "${CLEANUP_INCREMENTAL_DAYS:-7}" "${dirs[@]}" "${nightlyDirs[@]}"
}

## Stages 1 to 6: format, debug build, tests and lints, profiler, release builds and
## packages. One function, so --container can run them in the image instead, with this
## box taking up the steps that need the card, then the private runner.
fBuildTestRelease(){
	fSweepBuilds
	if fStepOpen "Format"; then
		fJobs
		"${FMT_CMD[@]}" || fDie "format $( ((fmtCheck)) && echo 'check ')failed (${FMT_CMD[*]})"
		fEcho "OK: $( ((fmtCheck)) && echo 'formatting clean' || echo 'formatted') (${FMT_CMD[*]})"
		fStepClose
	fi

	fStepOpen "Debug build"
	fJobs
	"${DEBUG_BUILD_CMD[@]}"
	fEcho "OK: debug build"
	fStepClose

	fStepOpen "Regression tests"
	fJobs
	corpusBefore=""
	if [[ -d "${root}/cicd/tests/fuzz-corpus" ]]; then corpusBefore="$(fCorpusHashes)"; fi
	fTestLines "${TEST_CMD[@]}"
	if [[ -n "${LINT_CMD+x}" ]] && ((${#LINT_CMD[@]})); then
		if "${LINT_PROBE[@]}" >/dev/null 2>&1; then
			"${LINT_CMD[@]}"
			fEcho "OK: lints clean"
		else
			fEcho "WARNING: lints skipped: ${LINT_PROBE[*]} failed (component not installed?)"
		fi
	fi
	if [[ -n "${XLINT_CMD+x}" ]] && ((${#XLINT_CMD[@]})) && "${LINT_PROBE[@]}" >/dev/null 2>&1; then
		"${XLINT_CMD[@]}" || fDie "windows lints failed"
		fEcho "OK: windows lints clean"
	fi
	if [[ -n "${MACLINT_CMD+x}" ]] && ((${#MACLINT_CMD[@]})) && "${LINT_PROBE[@]}" >/dev/null 2>&1; then
		"${MACLINT_CMD[@]}" || fDie "macOS lints failed"
		fEcho "OK: macOS lints clean"
	fi
	fLintScripts
	fContentScrub
	## The fuzz soak. Same targets the test run just went through, given a real
	## budget each. Gating: a case that breaks an invariant reports the seed that
	## reproduces it.
	if [[ -n "${FUZZ_CMD+x}" ]] && ((${#FUZZ_CMD[@]})) && ((${FUZZ_SECS:-0} > 0)); then
		fEcho "Fuzzing, ${FUZZ_SECS}s per target ..."
		fJobs hard
		fTestLines env "SILK_FUZZ_SECS=${FUZZ_SECS}" "${FUZZ_CMD[@]}" || fDie "fuzz found something"
		fEcho "OK: fuzz clean"
		fJobs
	fi
	if [[ -n "${ADVISORY_CMD+x}" ]] && ((${#ADVISORY_CMD[@]})); then
		"${DENY_PROBE[@]}" >/dev/null 2>&1 || fDie "advisory scan can't run: ${DENY_PROBE[*]} failed (see cicd/tool-pins.txt)"
		if [[ -n "${INDEX_REFRESH_CMD+x}" ]] && ((${#INDEX_REFRESH_CMD[@]})); then
			"${INDEX_REFRESH_CMD[@]}" || fDie "can't refresh the crates.io index, so the advisory scan can't see a yanked crate (above)"
		fi
		"${ADVISORY_CMD[@]}" || fDie "known advisories against the dependencies (above)"
		fEcho "OK: no known advisories"
		fAdvisoryTests
	fi
	if [[ -n "${DENY_CMD+x}" ]] && ((${#DENY_CMD[@]})); then
		if "${DENY_PROBE[@]}" >/dev/null 2>&1; then
			## Advisory-only for now: report license/advisory/duplicate findings
			## without failing the pipeline (tighten to gating once tuned).
			"${DENY_CMD[@]}" || fEcho "WARNING: cargo-deny reported findings (non-gating)"
		else
			fEcho "WARNING: deps check skipped: ${DENY_PROBE[*]} failed (cargo install cargo-deny)"
		fi
	fi
	fRunAsan
	fRunLeakcheck
	fRunTsan
	fJobs
	fScriptTests
	if [[ -n "${corpusBefore}" ]] && [[ "$(fCorpusHashes)" != "${corpusBefore}" ]]; then
		fDie "a test run wrote through the fuzz corpus - see 'git status cicd/tests/fuzz-corpus'"
	fi
	fEcho "OK: tests passed"
	fStepClose

	if fStepOpen "Profiler"; then fJobs; fRunProfiler; fStepClose; fi

	fStepOpen "Release build: native"
	fJobs
	## Panic locations and generated bindings carry the build box's absolute paths, which
	## put the home folder and account name into every published binary and made builds
	## differ between boxes. A cfg(all()) entry is joined with the per-target flags in
	## .cargo/config.toml, where RUSTFLAGS would replace them. Later entries win, so the
	## target dir comes after the root it usually sits in.
	mkdir -p "${TARGET_DIR}"
	remapCfg="$(cd "${TARGET_DIR}" && pwd)/remap-paths.toml"
	remapTarget="$(cd "${TARGET_DIR}" && pwd)"
	printf "[target.'cfg(all())']\nrustflags = ['--remap-path-prefix=%s=/cargo', '--remap-path-prefix=%s=/silkterm', '--remap-path-prefix=%s=/target']\n" \
		"${CARGO_HOME:-${HOME}/.cargo}" "${root}" "${remapTarget}" > "${remapCfg}"
	fRetryBuild "native release" "${RELEASE_NATIVE_CMD[@]}" --config "${remapCfg}"
	[[ -f "${RELEASE_NATIVE_BIN}" ]] || fDie "native release binary missing: ${RELEASE_NATIVE_BIN}"
	fEcho "OK: native release: ${RELEASE_NATIVE_BIN} ($(du -h --apparent-size "${RELEASE_NATIVE_BIN}" | cut -f1))"
	builtArts=("${RELEASE_NATIVE_OSARCH:-native}|${RELEASE_NATIVE_BIN}")
	fStepClose
	if ((BUILD_CROSS)) && ((${#CROSS_TARGETS[@]})); then
		for t in "${CROSS_TARGETS[@]}"; do
			localLabel="${t%%|*}"; rest="${t#*|}"; osarch="${rest%%|*}"; rest="${rest#*|}"; art="${rest%%|*}"; cmd="${rest#*|}"
			fStepOpen "Release build: ${localLabel}"
			fJobs
			fRetryBuild "${localLabel}" eval "${cmd} --config $(printf '%q' "${remapCfg}")"
			[[ -f "${art}" ]] || fDie "missing artifact for ${localLabel}: ${art}"
			fEcho "OK: ${localLabel}: ${art} ($(du -h --apparent-size "${art}" | cut -f1))"
			builtArts+=("${osarch}|${art}")
			fStepClose
		done
	fi

	fStepOpen "Release artifacts"
	for pair in "${builtArts[@]}"; do
		if fHasLocalPaths "${pair#*|}"; then fDie "${pair#*|} still holds a local path (${HOME} or ${root})"; fi
	done
	fEcho "OK: no local paths in ${#builtArts[@]} binary(s)"

	## A Windows binary with no icon and no version block links fine and reports
	## nothing, so it has to be looked for. The aarch64 exe shipped that way for a
	## while: embed-resource found no compiler for the arch and answered "not
	## attempted", which reads as success.
	resCheck="${here}/utility/pe-resources.py"
	if [[ -f "${resCheck}" ]]; then
		winArts=()
		for pair in "${builtArts[@]}"; do
			[[ "${pair#*|}" == *.exe ]] && winArts+=("${pair#*|}")
		done
		if ((${#winArts[@]})); then
			python3 "${resCheck}" "${winArts[@]}" || fDie "a windows binary is missing its icon or version info"
			fEcho "OK: windows resources present in ${#winArts[@]} binary(s)"
		fi
	fi

	## Collect the built binaries under versioned names + a sha256 checksums file,
	## ready to attach to a release as plain uploads. Version = Cargo.toml alone.
	if [[ -n "${RELEASE_ARTIFACT_DIR:-}" ]]; then
		ver="$(sed -n 's/^version *= *"\(.*\)".*/\1/p' "${root}/${VERSION_MANIFEST}" | head -1)"
		[[ -n "${ver}" ]] || fDie "no version found in ${VERSION_MANIFEST}"
		artDir="${root}/${RELEASE_ARTIFACT_DIR}"
		## Only ever under the repo's own artifacts folder, whatever the config says.
		if [[ -e "${artDir}" || -L "${artDir}" ]]; then
			fSafeRm_Under "$(realpath -e -- "${root}")/cicd/artifacts" "$(realpath -m -- "${root}")/${RELEASE_ARTIFACT_DIR%/}" || fDie "could not clear ${artDir}"
		fi
		mkdir -p "${artDir}"
		sums="${EXE_NAME}-${ver}-sha256sums.txt"
		for pair in "${builtArts[@]}"; do
			osarch="${pair%%|*}"; src="${pair#*|}"
			ext=""; [[ "${src}" == *.exe ]] && ext=".exe"
			cp -f "${src}" "${artDir}/${EXE_NAME}-${ver}-${osarch}${ext}"
		done
		fWriteSums
		fEcho "OK: ${#builtArts[@]} release artifact(s) + ${sums} -> ${RELEASE_ARTIFACT_DIR}/"
		((BUILD_CROSS)) || fEcho_Clean "note: cross targets skipped - artifact set is partial (native only)"
	fi
	fArmSmoke
	fStepClose

	if fStepOpen "Packages"; then fJobs; fBuildPackages; fStepClose; fi
}

## Stages 1 to 6 on this box, or in the container with this box taking over after.
if ((container)); then
	fStepOpen "Container image"
	fJobs
	fContainerBuild "${containerImage}"
	fEcho "OK: ${containerImage}"
	fEcho_Clean "runs cicd.bash ${innerArgs[*]}"
	fStepClose
	## The steps inside count on from here, toward this run's total.
	SILK_CICD_STEP_AT="$(fStepAt "Container image")" SILK_CICD_STEP_TOTAL="${_stepTotal}" \
		fContainerRun "${containerImage}" "${root}/cicd/cicd.bash" "${innerArgs[@]}" || fDie "the run in the container failed (above)"
	fEcho "OK: stages 1-6 in the container"
	## What needs this box's own trees and keys, or the card.
	fStepOpen "Checks on this box"
	fContainerArtifacts
	fContentScrub
	fWinGuiScenarios
	fArmSmoke
	fRenderNodeTest
	fStepClose
	if fStepOpen "Profiler"; then fJobs; fRunProfiler; fStepClose; fi
else
	fBuildTestRelease
fi

## The private runner builds on boxes that are often off or busy, so it skips
## those itself and says so. Only a job that ran and failed stops the run.
if fStepOpen "Private runner"; then
	fEcho_Clean "private runner (macOS, Microsoft Store) ..."
	privStart="$(mktemp)"
	privSkips="$(fSkipCount)"
	SILK_CICD_SKIPS="${skipLog}" "${PRIVATE_RUNNER}" --public "${root}" || fDie "a private runner job failed"
	fStepDone "private runner" "${privSkips}"
	## The Mac build joins this run's set only if the runner made it just now. One
	## left from an earlier run of the same version must not be dogfooded again.
	macBin="${PRIVATE_RUNNER%/cicd/*}/dist/${ver}/macos/${EXE_NAME}-${ver}-macos-universal"
	if [[ -f "${macBin}" && "${macBin}" -nt "${privStart}" ]]; then builtArts+=("macos-universal|${macBin}"); fi
	rm -f "${privStart}"
	fStepClose
fi

## Stage 7: dogfood. Install each build under a fixed name in the synced app dir
## for the platform it targets; the 'runterm' launcher on each box takes it from
## there and keeps its own rotated versions folder.
if fStepOpen "Dogfood"; then
	fJobs
	for xd in "${DOGFOOD_DESTS[@]}"; do
		xosarch="${xd%%|*}"; xrest="${xd#*|}"; xname="${xrest%%|*}"; xdirs="${xrest#*|}"

		xsrc=""
		for pair in "${builtArts[@]}"; do
			[[ "${pair%%|*}" == "${xosarch}" ]] && { xsrc="${pair#*|}"; break; }
		done
		if [[ -z "${xsrc}" ]]; then
			fEcho_Clean "no ${xosarch} build this run; dogfood skipped"
			continue
		fi

		## Make the first candidate when none is there yet, so a fresh box needs no
		## setup step of its own.
		xdest="$(fDogfoodDest "${xdirs}")"
		if [[ -z "${xdest}" ]]; then
			mkdir -p "${xdirs%%|*}" 2>/dev/null || true
			xdest="$(fDogfoodDest "${xdirs}")"
		fi
		if [[ -z "${xdest}" ]]; then
			fEcho "WARNING: no dogfood dest writable for ${xosarch} (${xdirs//|/, }); skipping"
			continue
		fi

		if [[ -e "${xdest}/${xname}" ]] && fInUse "${xdest}/${xname}"; then
			fEcho "WARNING: ${xdest}/${xname} is running; dogfood copy skipped"
			continue
		fi

		## -p: the launcher dates a build by its mtime, so the copy has to keep it.
		cp -pf "${xsrc}" "${xdest}/${xname}"
		chmod +x "${xdest}/${xname}"

		## A cross-build says nothing about the box that later reads it, so the tag
		## rides along in a sidecar rather than being guessed at the far end.
		xtag="${DOGFOOD_TAG-$(fBuildTag "${xosarch}")}"
		if [[ -n "${xtag}" ]]; then printf '%s\n' "${xtag}" > "${xdest}/${xname}.tag"
		else rm -f "${xdest}/${xname}.tag"; fi

		if [[ -n "${DOGFOOD_ICON:-}" && -f "${root}/${DOGFOOD_ICON}" ]]; then
			cp -f "${root}/${DOGFOOD_ICON}" "${xdest}/${EXE_NAME}.png"
		fi

		fEcho "OK: installed (${xosarch}${xtag:+, ${xtag}}) -> ${xdest}/${xname}"
	done
	fStepClose
fi

## Re-record the demo video (on by default, skipped under --quick unless --demo
## asks for it, never aborts). The video GFS-rotates into
## ../private/demo-video/; the README highlight gif goes in assets/demo.gif.
demoHook="${root}/cicd/utility/demo-video/demo-video.py"
if fStepOpen "Demo video"; then
	fEcho_Clean "recording demo video ..."
	## Absolute: the recorder runs the app from a scratch home of its own, so a
	## relative path would resolve against the wrong directory.
	silkBin="${RELEASE_NATIVE_BIN}"
	[[ "${silkBin}" = /* ]] || silkBin="${root}/${silkBin}"
	if SILK_BIN="${silkBin}" python3 "${demoHook}"; then
		fEcho "OK: demo video"
	else
		fEcho "WARNING: demo video hook failed (non-fatal)"
	fi
	fStepClose
fi

## Always run the publisher quiet: cicd already gave the initial prompt, so skip
## its redundant continue-prompt. The message was settled before stage 0.
pubFlags=(--quiet)
if fStepOpen "Backup + publish"; then
	## The publisher commits with -m, so no editor opens. GIT_EDITOR is there in
	## case some other git step ever wants one, so it cannot stall the run.
	fEcho_Clean "hands-off publish (commit message: \"${publishMsg}\")"
	GIT_BACKUP_AND_PUBLISH_QUIET=1 GIT_AUTO_MESSAGE="${publishMsg}" \
		GIT_EDITOR="${here}/utility/git-auto-msg.bash" "${GIT_PUBLISH[@]}" "${pubFlags[@]}"
	fEcho "OK: published"
	fStepClose
fi

## Zig's caches in the tree, where the builds ran. zig cc keeps to its global
## cache, which the container has on a volume and this box shares, so most runs
## find none.
if ((! container)) && command -v zig >/dev/null 2>&1; then
	zigNow="$(zig version 2>/dev/null || true)"
	while IFS= read -r -d '' zigDir; do fSweep_ZigCache "${zigDir}" "${zigNow}" "${CLEANUP_CACHE_CAP_MB:-2048}" || true
	done < <(find "${root}" -xdev \( -name .git -o -name target -o -path "${root}/forks" -o -path "${root}/cicd/artifacts" \) -prune -o -type d \( -name .zig-cache -o -name zig-cache \) -print0 -prune 2>/dev/null)
fi
fEcho_Clean
fEcho_Clean "$(fSweep_Summary)"
fSkipSummary

fSection "${APP_NAME} CI/CD: done."
fEcho_Clean


##	History:
##		- 2026-10-28: --list takes test IDs in the new 5-character form.
##		- 2026-10-28: Steps: a 79-wide rule, then a stamped Starting line numbered of
##		              the steps the run takes, and a PASS, SKIP or FAIL line. A step
##		              turned off isn't counted, and the plan says what turned it off.
##		- 2026-10-28: --long, or --slow, for the steps that take hours. TSan moved
##		              there from the default run, and CodeQL comes on with it.
##		- 2026-10-28: The Linux ARM64 build's --version runs on the arm64 VM.
##		- 2026-10-28: Recursive removals go through the guard in saferm.bash, and the
##		              release artifacts folder only under cicd/artifacts.
##		- 2026-10-26: --list prints the steps and test IDs a run would take. --keep-temp
##		              keeps the temp folders too, the container's /tmp included.
##		- 2026-10-26: --backlog-run-quick with its areas, --backlog-run-full,
##		              --merge-gate and --fmt-check. --gate and --ci are other
##		              names for the first and the merge gate.
##		- 2026-10-25: --no-remote skips the Windows scenarios and the private
##		              runner. What a box that wasn't there left out is a SKIP in
##		              its step's line, and listed again at the end of the run.
##		- 2026-10-25: The profiler and the render node test run on this box, after
##		              the container, and the profiler draws on the card through a
##		              headless sway, with a private Xvfb as the fallback.
##		- 2026-10-23: The test suite again under TSan, after the leak checks, in
##		              the image like the ASan run. --no-tsan skips it.
##		- 2026-10-23: The run in the container gets --no-demo, so the demo is
##		              recorded only here, and only when this run asks for it.
##		- 2026-10-22: The leak checks run by default. --no-leakcheck drops them,
##		              and --leakcheck adds them back under --quick.
##		- 2026-10-26: Jobs per step, a quarter when busy or for the hard steps, and
##		  the whole run at low priority, the container capped too. The build
##		  number is the commit's time on a dirty tree as well.
##		- 2026-10-22: CodeQL joins the leak checks, in the image, when --codeql asks.
##		- 2026-10-19: Every full run has the test suite again under ASan and LSan,
##		              in the container image, which has the nightly it needs.
##		- 2026-10-19: A full run removes what earlier runs left once it is stale:
##		              kept test folders and container /tmp past the newest few,
##		              build dirs from an older toolchain or no longer built,
##		              unused incremental caches, and its stopped containers and
##		              old images. The test folder is made after the prompt.
##		- 2026-10-18: --quick keeps the x86_64 cross build and skips the slow window
##		              tests. The demo is on by default, and macOS is linted too.
##		- 2026-10-13: The run in the container takes this box's time zone.
##		- 2026-10-10: --container runs stages 1-6 in a pinned image.
##		- 2026-10-08: A failed test script's output goes to the log, and stays in
##		              the test run folder.
##		- 2026-09-17: The run log and the flamegraph are written under a .part name
##		              and renamed once whole, so the startup gates cannot mark one as
##		              seen part way through.
##		- 2026-09-17: A blank answer at the publish prompt commits the automatic
##		              message, and the plan and the prompt name it. They promised an
##		              editor that the quiet publisher never opens.
##		- 2026-08-24: Name and date the rotating dogfood copy from the build, not
##		              from when the run started. The two were ~8 min apart, which
##		              the launchers read as a newer build.
##		- 2026-06-05 JC: Created.
##		- 2026-07-22 JC: Stage 0 remote sync - fetch, fast-forward if safely behind, abort if diverged.
##		- 2026-07-22 JC: Normalize section spacing (exactly one blank before each rule).
