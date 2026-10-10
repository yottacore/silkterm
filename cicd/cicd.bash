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
##	   3. regression tests + lints + fuzz soak (clippy gating, cargo-deny advisory,
##	      scroll harness)
##	   4. profiler (flamegraph SVG; non-gating artifact - see failure policy)
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
##	   --no-fuzz           skip the fuzz soak (the short one in the test run still runs)
##	   --no-profile        skip the profiler stage
##	   --no-dogfood        skip the dogfood install
##	   --no-publish        skip the git backup + publish stage
##	   --no-sync           skip the remote sync check (stage 0)
##	   --demo              re-record the demo video (off by default)
##	   --quick             skip the slow stages (cross-builds + packages + profiling)
##	   --gate              merge gate only: fmt --check + clippy + tests, then exit
##	                       (fast local stand-in for hosted CI; the pre-push hook runs it)
##	   --container         stages 1-6 in the pinned image from cicd/container/Dockerfile,
##	                       with its own target dir (target/container); sync, the private
##	                       runner, dogfood, the demo and publish stay here. With --gate,
##	                       the gate runs there.
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
## --container builds in its own target dir, and config.bash derives every build path
## from CARGO_TARGET_DIR, so it is set before the config loads. The run inside gets
## the same value, so the two sides of the mount agree on where everything is.
case " ${*} " in *" --container "*) export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${root}/target/container}" ;; esac
source "${here}/config.bash"
source "${here}/utility/include/gfs-rotate.bash"                  ## gfs_rotate() for the profiler artifacts
source "${here}/utility/include/remote-git.bash"                  ## fRemoteGit / fRemoteGh, as the folder's own account
##  shellcheck source=cicd/utility/include/echo.bash
source "${here}/utility/include/echo.bash"                        ## fEcho, fEcho_Clean, fSection, fDie
##  shellcheck source=cicd/utility/built-from.bash
source "${here}/utility/built-from.bash"                          ## the artifacts' provenance note
declare -p FMT_CMD &>/dev/null || FMT_CMD=()                      ## tolerate a config without the fmt stage

## Cap compile/test parallelism to at most half the cores so a pipeline run
## doesn't peg every CPU and leaves the machine usable. cargo's jobserver bounds
## total rustc + codegen parallelism to CARGO_BUILD_JOBS (covers build, test,
## clippy, and the zigbuild cross-builds); RUST_TEST_THREADS bounds the test run.
## A project can override CICD_MAX_JOBS in config.bash.
cores="$(nproc 2>/dev/null || echo 2)"
: "${CICD_MAX_JOBS:=$(( cores / 2 ))}"
(( CICD_MAX_JOBS >= 1 )) || CICD_MAX_JOBS=1
export CARGO_BUILD_JOBS="${CICD_MAX_JOBS}"
export RUST_TEST_THREADS="${CICD_MAX_JOBS}"

cd "${root}"
stamp="$(date +%Y%m%d-%H%M%S)"

## Parse options.
assumeYes=0; quiet=0; quick=0; gate=0; noArm=0; noWindows=0; sync=1; cliMessage=""; container=0; innerArgs=()
while (($#)); do case "${1}" in
	-y|--yes)                 assumeYes=1; shift ;;
	-q|--quiet)               quiet=1; assumeYes=1; shift ;;   ## quiet + unattended; publish runs quiet too
	--gate)                   gate=1; shift ;;                  ## merge gate only, then exit
	--container)              container=1; shift ;;             ## stages 1-6 in the image; innerArgs collects what goes in with them
	--no-fmt)                 FMT_CMD=(); innerArgs+=("${1}"); shift ;;
	--no-cross)               BUILD_CROSS=0; innerArgs+=("${1}"); shift ;;
	--no-arm)                 noArm=1; innerArgs+=("${1}"); shift ;;                ## drop ARM64 builds + packages
	--no-windows)             noWindows=1; innerArgs+=("${1}"); shift ;;            ## drop the Windows cross targets
	--no-package)             PACKAGE_ENABLE=0; innerArgs+=("${1}"); shift ;;
	--no-private)             PRIVATE_RUNNER=""; shift ;;
	--no-profile)             PROFILE_ENABLE=0; innerArgs+=("${1}"); shift ;;
	--no-dogfood)             DOGFOOD_DESTS=(); shift ;;
	--no-publish)             GIT_PUBLISH=(); shift ;;
	--no-sync)                sync=0; shift ;;
	--demo)                   DEMO_ENABLE=1; shift ;;
	--quick)                  quick=1; BUILD_CROSS=0; PROFILE_ENABLE=0; PACKAGE_ENABLE=0; FUZZ_SECS=0; innerArgs+=("${1}"); shift ;;   ## skip the slow stages
	--no-fuzz)                FUZZ_SECS=0; innerArgs+=("${1}"); shift ;;
	--message=*|--msg=*|-m=*) cliMessage="${1#*=}"; shift ;;
	-m|--message|--msg)       cliMessage="${2-}"; shift; (($#)) && shift ;;
	-h|--help)                sed -n '/^##	- Purpose:/,/^##	History:/p' "${BASH_SOURCE[0]}" | sed '$d; s/^##	\{0,1\}//'; exit 0 ;;
	*) echo "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done

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
	fEcho_Clean "${label} ..."
	local rc=0 out; out="$(fTestOut "${script}")"
	"${root}/${script}" >"${out}" || rc=$?
	case "${rc}" in
		0) rm -f "${out}"; fEcho "OK: ${label} ($(fTestId "${script}"))" ;;
		3) rm -f "${out}"; fEcho "WARNING: ${skipped}" ;;
		*) fTestOut_Fail "${out}" "${label} test failed ($(fTestId "${script}"))" ;;
	esac
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
containerRecipe="${here}/container/Dockerfile"
## The build args: the toolchain and its targets, then VER_<pin> for each pin the
## recipe takes, so a pin the image does not install changes nothing.
fContainerArgs(){
	local pin name ver
	printf 'VER_rust=%s\n' "$(sed -n 's/^channel *= *"\(.*\)".*/\1/p' "${root}/rust-toolchain.toml")"
	printf 'RUST_TARGETS=%s\n' "$(sed -n '/^targets *= *\[/,/\]/p' "${root}/rust-toolchain.toml" | grep -oE '"[^"]+"' | tr -d '"' | paste -sd' ')"
	for pin in "${TOOL_PINS[@]}"; do
		name="${pin%%|*}"; name="${name//-/_}"; ver="${pin#*|}"; ver="${ver%%|*}"
		if grep -qxF "ARG VER_${name}" "${containerRecipe}"; then printf 'VER_%s=%s\n' "${name}" "${ver}"; fi
	done
}
fContainerImage(){ printf '%s-cicd:%s' "${EXE_NAME}" "$({ cat "${containerRecipe}"; fContainerArgs; } | sha256sum | cut -c1-12)"; }
## fContainerBuild <image>: builds it when it is not there, then drops the older
## images of the same name, since a new tag means the old recipe is gone.
fContainerBuild(){
	local -r image="${1}"
	local arg old
	local -a args=()
	if "${docker}" image inspect "${image}" >/dev/null 2>&1; then fEcho_Clean "image ${image}"; return 0; fi
	fEcho_Clean "building ${image} (the recipe or a pin changed; this takes a while)"
	while IFS= read -r arg; do args+=(--build-arg "${arg}"); done < <(fContainerArgs)
	"${docker}" build -t "${image}" "${args[@]}" --build-arg "JOBS=${CICD_MAX_JOBS}" "${here}/container" || fDie "image build failed"
	while IFS= read -r old; do
		[[ -n "${old}" && "${old}" != "${image}" ]] || continue
		if "${docker}" image rm "${old}" >/dev/null 2>&1; then fEcho_Clean "removed ${old}"; fi
	done < <("${docker}" image ls "${image%%:*}" --format '{{.Repository}}:{{.Tag}}' 2>/dev/null || true)
}
## fContainerRun <image> <cicd.bash options...>: the engine in the container, against
## this tree. Its /tmp is a folder under the target dir, so a failed test's files are
## still there afterward, and the X sockets inside never meet this box's.
fContainerRun(){
	local -r image="${1}"; shift
	local gitCommon
	local -a mounts=(-v "${root}:${root}" -v "${EXE_NAME}-cicd-cache:/cache" -v "${containerTarget}/tmp:/tmp")
	mkdir -p "${containerTarget}/tmp"
	[[ "${containerTarget}" == "${root}/"* ]] || mounts+=(-v "${containerTarget}:${containerTarget}")
	gitCommon="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
	[[ -z "${gitCommon}" || "${gitCommon}" == "${root}/"* ]] || mounts+=(-v "${gitCommon}:${gitCommon}")
	"${docker}" run --rm --init --user "$(id -u):$(id -g)" --shm-size 1g "${mounts[@]}" -w "${root}" \
		-e "USER=${USER:-$(id -un)}" -e "CARGO_TARGET_DIR=${containerTarget}" -e "SILK_BUILD_MINUTES=${SILK_BUILD_MINUTES:-}" \
		-e "CICD_MAX_JOBS=${CICD_MAX_JOBS}" -e XDG_RUNTIME_DIR=/tmp/xdg-runtime \
		"${image}" bash -c 'mkdir -m 700 -p "${XDG_RUNTIME_DIR}" && exec bash "${@}"' _ "${root}/cicd/cicd.bash" "${@}"
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
containerImage=""; containerTarget=""
if ((container)); then
	[[ -z "${SILK_CICD_IN_CONTAINER:-}" ]] || fDie "already in the container; --container would start another"
	command -v "${docker}" >/dev/null 2>&1 || fDie "--container needs docker, or SILK_DOCKER naming a stand-in"
	[[ -f "${containerRecipe}" ]] || fDie "missing ${containerRecipe}"
	containerImage="$(fContainerImage)"
	containerTarget="$(mkdir -p "${TARGET_DIR}" && cd "${TARGET_DIR}" && pwd)"
	## The engine inside takes this run's stage options and none of the stages that
	## need this box: sync, the private runner, dogfood, the demo and publish.
	innerArgs=(-y --no-sync --no-dogfood --no-publish --no-private "${innerArgs[@]}")
	((quiet)) && innerArgs[0]=-q
fi

## One folder for every file the tests write, made before the gate or any test
## stage so they all share it. TMPDIR stays put, so builds and packaging keep the
## system temp dir.
# shellcheck source=cicd/tests/_testdir.bash
source "${root}/cicd/tests/_testdir.bash"
fTestDir_Make || fDie "could not make the test run folder"

## Gate mode: the local merge gate (what a bare-bones hosted CI would run).
## fmt --check + clippy -D warnings + tests, fail-fast, nothing mutated, no
## artifacts/log-tee/publish. Wired as the pre-push hook for main, so nothing
## reaches the release branch unverified even outside a full run.
if ((gate)); then
	if ((container)); then
		fSection "Gate in ${containerImage}"
		fContainerBuild "${containerImage}"
		fContainerRun "${containerImage}" --gate || fDie "the gate in the container failed (above)"
		exit 0
	fi
	fSection "Gate 1/3  Format check"
	if declare -p FMT_CHECK_CMD &>/dev/null && ((${#FMT_CHECK_CMD[@]})); then
		"${FMT_CHECK_CMD[@]}" || fDie "format check failed (run: ${FMT_CMD[*]:-cargo fmt})"
		fEcho "OK: formatting clean"
	else
		fEcho_Clean "format check skipped (no FMT_CHECK_CMD)"
	fi
	fSection "Gate 2/3  Lints"
	if [[ -n "${LINT_CMD+x}" ]] && ((${#LINT_CMD[@]})) && "${LINT_PROBE[@]}" >/dev/null 2>&1; then
		"${LINT_CMD[@]}"
		fEcho "OK: lints clean"
	else
		fEcho_Clean "lints skipped (clippy unavailable)"
	fi
	fSection "Gate 3/3  Tests"
	fTestLines "${TEST_CMD[@]}"
	fEcho "OK: tests passed"
	fSection "${APP_NAME} gate: PASSED."
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
fEcho_Clean "Remote sync .........: $( ((sync)) && echo 'fetch + fast-forward check' || echo '(skipped)')"
fEcho_Clean "Format ..............: ${FMT_CMD[*]:-(skipped)}"
fEcho_Clean "Debug build .........: ${DEBUG_BUILD_CMD[*]}"
fEcho_Clean "Tests ...............: ${TEST_CMD[*]}"
if ((${FUZZ_SECS:-0} > 0)) && ((${#FUZZ_CMD[@]})); then
	fEcho_Clean "Fuzz ................: ${FUZZ_CMD[*]} at ${FUZZ_SECS}s per target"
else
	fEcho_Clean "Fuzz ................: (skipped)"
fi
if ((PROFILE_ENABLE)); then
	fEcho_Clean "Profiler ............: ${PROFILE_SECS}s run -> flamegraph SVG (on headless ${RPD_HEADLESS_DISPLAY:-:98})"
	fEcho_Clean "  output dir ........: ${profileDir}"
	fEcho_Clean "  workload ..........: python3 ${PROFILE_WORKLOAD_SCRIPT} ${PROFILE_WORKLOAD_ARGS}"
else
	fEcho_Clean "Profiler ............: (disabled)"
fi
fEcho_Clean "Release (native) ....: ${RELEASE_NATIVE_CMD[*]} -> ${RELEASE_NATIVE_BIN}"
if ((BUILD_CROSS)) && ((${#CROSS_TARGETS[@]})); then
	fEcho_Clean "Release (cross) .....:$( ((noArm)) && echo ' (x86_64 only, --no-arm)')$( ((noWindows)) && echo ' (Linux only, --no-windows)')"
	for t in "${CROSS_TARGETS[@]}"; do fEcho_Clean "    - ${t%%|*}"; done
else
	fEcho_Clean "Release (cross) .....: (skipped)"
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
if ((${#GIT_PUBLISH[@]} == 0)); then
	fEcho_Clean "Publish (last) ......: (disabled)"
elif [[ -n "${publishMsg}" ]]; then
	fEcho_Clean "Publish (last) ......: ${GIT_PUBLISH[*]} (hands-off: \"${publishMsg}\")"
else
	fEcho_Clean "Publish (last) ......: ${GIT_PUBLISH[*]} (will prompt for message; blank = \"${autoMsg}\")"
fi
fEcho_Clean
fEcho_Clean "Fail-fast: any error aborts before the next stage."
fEcho_Clean

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
	## fTestDir_End before fFinishLog, so its line reaches the log.
	trap 'rc=$?; fTestDir_End "${rc}"; fFinishLog; exit $rc' EXIT
fi

## Stage 0: remote sync. Make sure the local branch can be safely refreshed from
## its upstream BEFORE spending the build: what stage 8 pushes should be what got
## built and tested here, not an untested post-build merge. Behind-only is safe
## (fast-forward, stash-wrapped for a dirty tree); diverged aborts now rather
## than at publish. Offline just warns - a local build shouldn't need the net.
fSection "0/8  Remote sync"
if ((! sync)); then
	fEcho_Clean "remote sync skipped"
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

## The source everything from here on reads. Taken before the first build, because
## the note has to name what the binaries hold: another session committing in this
## tree during a build used to leave the note naming the new commit, clean, with the
## native binary holding the source from before it and the cross binaries the source
## after. Stage 0 is past, so its fast-forward is not mistaken for that.
builtFromState="$(fSourceState)"

## Pin the build number for the whole run. build.rs would otherwise read the clock
## per target, so the four cross builds of one release would report four different
## builds, minutes apart, and the release notes could not name one of them. A clean
## tree takes its commit's time, so a rebuild of a release commit gets the same
## number. A dirty tree is a different binary, so it keeps the clock. After stage 0,
## since a fast-forward moves the commit. A value cicd-win hands down is kept.
fPinBuildMinutes(){
	[[ -z "${SILK_BUILD_MINUTES:-}" ]] || return 0
	local buildSecs
	buildSecs="$(date +%s)"
	if [[ -z "$(git status --porcelain --untracked-files=no 2>/dev/null || echo dirty)" ]]; then
		buildSecs="$(git log -1 --format=%ct 2>/dev/null || date +%s)"
	fi
	export SILK_BUILD_MINUTES=$(( (buildSecs - 946684800) / 60 ))
}
fPinBuildMinutes

## The fuzz corpus is read-only data: every file in it is replayed on each run
## and chewed on by the mutator. A run that writes one through changes what
## later runs replay, and nothing else would say so - a seed just quietly stops
## being the case it was saved as. Hashed before and after, so a fixture edited
## by hand before the run is not mistaken for one the run made.
fCorpusHashes(){ find "${root}/cicd/tests/fuzz-corpus" -type f -exec sha256sum {} + 2>/dev/null | sort; }

## Stage 4's profiler (non-gating artifact; failures classified below).
fRunProfiler(){
	((PROFILE_ENABLE)) || { fEcho_Clean "profiler disabled"; return 0; }

	## Mundane/environmental reasons -> skip with a warning (not the app's fault),
	## unless PROFILE_STRICT. Genuine run failures below still abort. The app runs
	## on a private Xvfb (gui-headless.bash), so no visible DISPLAY is needed - only
	## Xvfb + python3 + the workload.
	local skip=""
	command -v python3 >/dev/null 2>&1 || skip="python3 not found"
	[[ -z "${skip}" ]] && [[ ! -f "${absScript}" ]] && skip="workload missing: ${absScript}"
	[[ -z "${skip}" ]] && ! command -v Xvfb >/dev/null 2>&1 && skip="Xvfb not found (headless display unavailable)"
	if [[ -n "${skip}" ]]; then
		((PROFILE_STRICT)) && fDie "profiler: ${skip}"
		fEcho "WARNING: profiler skipped: ${skip}"; return 0
	fi

	## From here a failure is the app's fault and aborts, bar the retry fRetryBuild owns.
	fEcho_Clean "building ${PROFILE_BIN} (cargo --profile ${PROFILE_PROFILE} --features ${PROFILE_FEATURE})"
	fRetryBuild profiler cargo build --profile "${PROFILE_PROFILE}" --features "${PROFILE_FEATURE}"
	mkdir -p "${profileDir}"

	## Bring up a private in-memory display so the profiler window never touches the
	## user's visible session (renders via software GL / llvmpipe on Xvfb).
	local headless="${here}/utility/gui-headless.bash"
	## Not :99 - rapid-photo-downloader-pro uses that display for its own testing.
	export CICD_HEADLESS_DISPLAY="${CICD_HEADLESS_DISPLAY:-${RPD_HEADLESS_DISPLAY:-:98}}"
	local hdisp="${CICD_HEADLESS_DISPLAY}"
	if ! "${headless}" start >/dev/null 2>&1; then
		((PROFILE_STRICT)) && fDie "profiler: headless display failed to start"
		fEcho "WARNING: profiler skipped: headless display failed to start"; return 0
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
	"${headless}" stop >/dev/null 2>&1 || true
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
## Graphical scenarios on the Windows boxes. Neither box is build hardware, so an
## unreachable or locked one is reported and stepped over; a scenario that actually
## ran and failed aborts. Skipped under --quick, and inside the container, which has
## no way to the boxes; the run outside takes it up once the container is done.
fWinGuiScenarios(){
	if ((quick)) || [[ -z "${WINGUI_HARNESS+x}" ]] || ((${#WINGUI_HARNESS[@]} == 0)) || [[ ! -x "${root}/${WINGUI_HARNESS[0]}" ]]; then return 0; fi
	if [[ -n "${SILK_CICD_IN_CONTAINER:-}" ]]; then fEcho_Clean "windows gui scenarios run outside the container"; return 0; fi
	fEcho_Clean "windows gui scenarios ..."
	if "${root}/${WINGUI_HARNESS[0]}" "${WINGUI_HARNESS[@]:1}"; then
		fEcho "OK: windows gui scenarios"
	else
		fDie "a windows gui scenario failed"
	fi
}

## Stages 1 to 6: format, debug build, tests and lints, profiler, release builds and
## packages. One function, so --container can run them in the image instead, with this
## box picking up at the private runner.
fBuildTestRelease(){
	## Stage 1: format.
	fSection "1/8  Format"
	if ((${#FMT_CMD[@]} == 0)); then
		fEcho_Clean "format skipped"
	else
		"${FMT_CMD[@]}"
		fEcho "OK: formatted (${FMT_CMD[*]})"
	fi

	## Stage 2: debug build.
	fSection "2/8  Debug build"
	"${DEBUG_BUILD_CMD[@]}"
	fEcho "OK: debug build"

	## Stage 3: regression tests.
	fSection "3/8  Regression tests"
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
	fContentScrub
	## The fuzz soak. Same targets the test run just went through, given a real
	## budget each. Gating: a case that breaks an invariant reports the seed that
	## reproduces it.
	if [[ -n "${FUZZ_CMD+x}" ]] && ((${#FUZZ_CMD[@]})) && ((${FUZZ_SECS:-0} > 0)); then
		fEcho "Fuzzing, ${FUZZ_SECS}s per target ..."
		fTestLines env "SILK_FUZZ_SECS=${FUZZ_SECS}" "${FUZZ_CMD[@]}" || fDie "fuzz found something"
		fEcho "OK: fuzz clean"
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
	## Old config files converted by the program just built: in place where shcl
	## can migrate them, written new where it cannot, the old file kept either way.
	## Exit 3 means there was no binary to run, which is a skip and never an OK.
	fRunTest_MaySkip cicd/tests/config-convert/run.bash "config conversion" "config conversion skipped, no binary to run"
	## The window shows at the size it keeps, maximized and fullscreen too, with
	## no jump after. Exit 3 is a skip: no binary, display, window manager or python-xlib.
	fRunTest_MaySkip cicd/tests/startsize/run.bash "window size at launch"
	## A window put back on a monitor that went dark and came back takes that
	## monitor's size again. Exit 3 is a skip: no binary, sway, Xwayland or python3.
	fRunTest_MaySkip cicd/tests/monwake/run.bash "window size after a monitor wakes"
	## A save after the config was deleted writes it again, keeping what it had.
	## Exit 3 is a skip: no binary, display or xdotool.
	fRunTest_MaySkip cicd/tests/delcfg/run.bash "save after the config was deleted"
	## The wallpaper is held at the size it is drawn at, and again after a resize.
	## Exit 3 is a skip: no binary, display or xdotool.
	fRunTest_MaySkip cicd/tests/wpresize/run.bash "wallpaper held at window size"
	## A window waking from the idle release shows a small copy of its wallpaper
	## until the real one is prepared again. Exit 3 is a skip: no binary, display,
	## window manager or tools.
	fRunTest_MaySkip cicd/tests/wakepic/run.bash "wallpaper kept through an idle wake"
	## A prepared wallpaper is kept on disk and the next launch reads it back.
	## Exit 3 is a skip: no binary, display or xdotool.
	fRunTest_MaySkip cicd/tests/wpkept/run.bash "prepared wallpaper kept on disk"
	## Heap allocations per frame at rest and in a drag select, against a limit.
	## Exit 3 is a skip: no debug binary, display, window manager or xdotool.
	fRunTest_MaySkip cicd/tests/allocs/run.bash "allocations per frame"
	## Software rendering on an X server with no shared pixmaps, at launch and
	## switched on later. Exit 3 is a skip: no binary, sway, Xwayland with DRI3,
	## render node or tools.
	fRunTest_MaySkip cicd/tests/swnoshm/run.bash "software rendering without shared pixmaps"
	## This script's own steps: the build retry, the dogfood tag, the options, the
	## running-copy check, the build number and the host line.
	fRunTest cicd/tests/engine/run.bash "pipeline steps" "pipeline step test failed"
	## --container: the image tag, the recipe's pins, what goes in with the run and
	## what the run here picks up after it.
	fRunTest cicd/tests/container/run.bash "container run" "container run test failed"
	## A failed test's output, which once went nowhere and left only its name.
	fRunTest cicd/tests/testout/run.bash "failed test output"
	## Stage 0, which has to stop a diverged tree before anything is built.
	fRunTest cicd/tests/sync/run.bash "remote sync"
	## The rotation that prunes run logs and flamegraphs.
	fRunTest cicd/tests/rotate/run.bash "log rotation"
	## One tool pin list for both pipelines, and the docs quoting it.
	fRunTest cicd/tests/pins/run.bash "tool pins" "tool pin test failed"
	## The PowerShell lint this stage gates on has to fail on a finding.
	fRunTest cicd/tests/pslint/run.bash "PowerShell lint"
	## The Python lint, the same way.
	fRunTest cicd/tests/pylint/run.bash "Python lint"
	## The Windows runner, which steps over a box that is off.
	fRunTest cicd/tests/win-remote/run.bash "windows runner"
	## The parts of the Windows pipeline that run anywhere.
	fRunTest cicd/tests/cicd-win/run.bash "windows pipeline pieces" "windows pipeline test failed"
	## Every table of contents, which no markdown linter regenerates. design.md had
	## been missing eight of its headings.
	fRunTest cicd/tests/toc/run.py "tables of contents" "a table of contents is out of date - run cicd/tests/toc/run.py --fix"
	## Every markdown table, laid out as the README's generated one is. Hand-written
	## ones had drifted to trailing pipes and ragged columns.
	fRunTest cicd/tests/tables/run.py "markdown tables" "a markdown table is not canonical - run cicd/tests/tables/run.py --fix"
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
	## The Windows scenario harness, which once tested whatever the box last built
	## and stopped every SilkTerm on a shared box.
	fRunTest cicd/tests/wingui/harness-test.bash "windows scenario harness"
	## The wine launcher, which once left dead file types on the desktop.
	fRunTest cicd/tests/wine/run.bash "wine launcher"
	## The wallpaper gallery and contact sheet are rendered, so they go stale in
	## silence when the pack changes. Nine removed images sat in both for a month.
	if [[ -f "${root}/cicd/utility/wallpaper-gallery.bash" ]]; then
		fEcho_Clean "wallpaper gallery ..."
		bash "${root}/cicd/utility/wallpaper-gallery.bash" --check || fDie "the wallpaper gallery does not match the pack"
		fEcho "OK: wallpaper gallery"
	fi
	fWinGuiScenarios
	## Headless scroll regression harness (slow; skipped under --quick). A measured
	## regression aborts here. Exit 3 means it could not run at all (no Xvfb, cage or
	## binary), which is a skip for that arm and never an OK.
	if ((! quick)) && [[ -n "${SCROLL_HARNESS+x}" ]] && ((${#SCROLL_HARNESS[@]})); then
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
	elif ((quick)); then
		fEcho_Clean "scroll harness skipped (--quick)"
	fi
	## Dogfood launcher: it shares a path with the release installer, so what it does
	## to a file it did not create is worth a gate. Runs in a sandboxed HOME.
	if [[ -n "${LAUNCHER_HARNESS+x}" ]] && ((${#LAUNCHER_HARNESS[@]})); then
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
	if [[ -n "${corpusBefore}" ]] && [[ "$(fCorpusHashes)" != "${corpusBefore}" ]]; then
		fDie "a test run wrote through the fuzz corpus - see 'git status cicd/tests/fuzz-corpus'"
	fi
	fEcho "OK: tests passed"

	fSection "4/8  Profiler"
	fRunProfiler

	## Stage 5: release builds.
	fSection "5/8  Release build (native)"
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
	fEcho "OK: native release: ${RELEASE_NATIVE_BIN} ($(du -h "${RELEASE_NATIVE_BIN}" | cut -f1))"
	builtArts=("${RELEASE_NATIVE_OSARCH:-native}|${RELEASE_NATIVE_BIN}")
	if ((BUILD_CROSS)) && ((${#CROSS_TARGETS[@]})); then
		for t in "${CROSS_TARGETS[@]}"; do
			localLabel="${t%%|*}"; rest="${t#*|}"; osarch="${rest%%|*}"; rest="${rest#*|}"; art="${rest%%|*}"; cmd="${rest#*|}"
			fSection "5/8  Release build: ${localLabel}"
			fRetryBuild "${localLabel}" eval "${cmd} --config $(printf '%q' "${remapCfg}")"
			[[ -f "${art}" ]] || fDie "missing artifact for ${localLabel}: ${art}"
			fEcho "OK: ${localLabel}: ${art} ($(du -h "${art}" | cut -f1))"
			builtArts+=("${osarch}|${art}")
		done
	fi

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
		rm -rf "${artDir}"; mkdir -p "${artDir}"
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

	fSection "6/8  Packages"
	if ((quick)); then
		fEcho_Clean "packages skipped (--quick)"
	else
		fBuildPackages
	fi
}

## Stages 1 to 6 on this box, or in the container with this box taking over after.
if ((container)); then
	fSection "Container"
	fContainerBuild "${containerImage}"
	fEcho_Clean "runs cicd.bash ${innerArgs[*]}"
	fContainerRun "${containerImage}" "${innerArgs[@]}" || fDie "the run in the container failed (above)"
	fEcho "OK: stages 1-6 in the container"
	fContainerArtifacts
	## What needs this box's own trees and keys.
	fContentScrub
	fWinGuiScenarios
else
	fBuildTestRelease
fi

## The private runner builds on boxes that are often off or busy, so it skips
## those itself and says so. Only a job that ran and failed stops the run.
if ((quick)); then
	fEcho_Clean "private runner skipped (--quick)"
elif [[ -n "${PRIVATE_RUNNER:-}" && -x "${PRIVATE_RUNNER}" ]]; then
	fEcho_Clean "private runner (macOS, Microsoft Store) ..."
	privStart="$(mktemp)"
	"${PRIVATE_RUNNER}" --public "${root}" || fDie "a private runner job failed"
	fEcho "OK: private runner"
	## The Mac build joins this run's set only if the runner made it just now. One
	## left from an earlier run of the same version must not be dogfooded again.
	macBin="${PRIVATE_RUNNER%/cicd/*}/dist/${ver}/macos/${EXE_NAME}-${ver}-macos-universal"
	if [[ -f "${macBin}" && "${macBin}" -nt "${privStart}" ]]; then builtArts+=("macos-universal|${macBin}"); fi
	rm -f "${privStart}"
else
	fEcho_Clean "private runner not checked out; skipped"
fi

## Stage 7: dogfood. Install each build under a fixed name in the synced app dir
## for the platform it targets; the 'runterm' launcher on each box takes it from
## there and keeps its own rotated versions folder.
fSection "7/8  Dogfood (install release builds to the synced app dirs)"
dfDid=0

for xd in "${DOGFOOD_DESTS[@]:-}"; do
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
	dfDid=1
done

if ((! dfDid)); then fEcho_Clean "dogfood disabled"; fi

## Re-record the demo video (off by default, skipped under --quick, never
## aborts). The video GFS-rotates into
## ../private/demo-video/; the README highlight gif goes in assets/demo.gif.
demoHook="${root}/cicd/utility/demo-video/demo-video.py"
if ((! ${DEMO_ENABLE:-0})); then
	fEcho_Clean "demo video disabled"
elif ((quick)); then
	fEcho_Clean "demo video skipped (--quick)"
elif [[ -f "${demoHook}" ]]; then
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
fi

## Stage 8: backup + publish.
fSection "8/8  Backup + publish"
## Always run the publisher quiet: cicd already gave the initial prompt, so skip
## its redundant continue-prompt. The message was settled before stage 0.
pubFlags=(--quiet)
if ((${#GIT_PUBLISH[@]} == 0)); then
	fEcho_Clean "publish disabled"
else
	## The publisher commits with -m, so no editor opens. GIT_EDITOR is there in
	## case some other git step ever wants one, so it cannot stall the run.
	fEcho_Clean "hands-off publish (commit message: \"${publishMsg}\")"
	GIT_BACKUP_AND_PUBLISH_QUIET=1 GIT_AUTO_MESSAGE="${publishMsg}" \
		GIT_EDITOR="${here}/utility/git-auto-msg.bash" "${GIT_PUBLISH[@]}" "${pubFlags[@]}"
	fEcho "OK: published"
fi

fSection "${APP_NAME} CI/CD: done."
fEcho_Clean


##	History:
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
