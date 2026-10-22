#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted code reads them, and shellcheck cannot see into an eval.
#  shellcheck disable=2154  ## 'referenced but not assigned.' The lifted code assigns them, inside an eval.

##	- Purpose:
##		Pieces of cicd.bash that decide what a run does, lifted out of the file as
##		they stand and run on their own: the build retry, the dogfood tag, the
##		option parser, the running-copy check, the build number and the host line.
##	- Test ID: Er2UgY7
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
# shellcheck source=cicd/tests/_forks.bash
source "${meDir}/../_forks.bash"
cicd="$(cd "${meDir}/../.." && pwd)"
engine="${cicd}/cicd.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fLift(){ sed -n "/${1}/,/${2}/p" "${engine}"; }
fNot(){ ! "${@}"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-engine.XXXXXX")"
declare -a started=()
fEnd(){ local -r rc=$?; local p; for p in "${started[@]}"; do kill "${p}" 2>/dev/null || true; done; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

fEcho(){ echo "    ${*}"; }
fDie(){ echo "DIE: ${*}"; exit 1; }

## The build retry. rustc has crashed inside LLVM twice in a row, so the one
## rebuild the profiler stage first had was not enough.
eval "$(fLift '^fRetryBuild(){' '^}')"
counter="${work}/tries"
fFlaky(){ local n; n=$(( $(cat "${counter}") + 1 )); echo "${n}" >"${counter}"; ((n > ${1})); }
fRetry(){  ## fRetry <failures before success> [BUILD_ATTEMPTS]; sets rc, out, tries
	echo 0 >"${counter}"
	rc=0; out="$(if [[ -n "${2:-}" ]]; then export BUILD_ATTEMPTS="${2}"; else unset BUILD_ATTEMPTS; fi; fRetryBuild test fFlaky "${1}")" || rc=$?
	tries="$(cat "${counter}")"
}
fRetry 2 3
fCheck "two crashes in a row are retried through" test "${rc}" -eq 0 -a "${tries}" -eq 3
fRetry 2
fCheck "and so they are with BUILD_ATTEMPTS unset" test "${rc}" -eq 0 -a "${tries}" -eq 3
fRetry 99 3
fCheck "a build that always fails stops after three attempts" test "${rc}" -ne 0 -a "${tries}" -eq 3
fCheck "and the run dies saying so" grep -q '^DIE: test build failed 3x' <<<"${out}"
fRetry 99
fCheck "unset, a build that always fails stops after five" test "${rc}" -ne 0 -a "${tries}" -eq 5
fRetry 0 3
fCheck "a build that works runs once" test "${rc}" -eq 0 -a "${tries}" -eq 1
## 3 still aborted about 1 full run in 12; 5 is about 1 in 50.
fCheck "the shipped config allows at least five attempts" \
	bash -c 'source "$1" && ((BUILD_ATTEMPTS >= 5))' _ "${cicd}/config.bash"
fCheck "the profiler build goes through the retry" grep -q '^[[:space:]]*fRetryBuild profiler ' "${engine}"
fCheck "the native release build does" grep -q '^[[:space:]]*fRetryBuild "native release" ' "${engine}"
fCheck "and each cross build does" grep -q '^[[:space:]]*fRetryBuild "${localLabel}" ' "${engine}"

## The dogfood tag: toolchain, built on, target, arch. A cross build is tagged
## for where it will run.
eval "$(fLift '^fBuildTag(){' '^}')"
hostName="Linux"
uname(){ echo "${hostName}"; }
fTag(){ test "$(fBuildTag "${1}")" = "${2}"; }
fCheck "linux-x86_64 built here is gnulli" fTag linux-x86_64 gnulli
fCheck "windows-x86_64 built here is gnulwi" fTag windows-x86_64 gnulwi
fCheck "windows-arm64 built here is gnulwa" fTag windows-arm64 gnulwa
fCheck "linux-arm64 built here is gnulla" fTag linux-arm64 gnulla
fCheck "an unknown target gets no tag" fTag solaris-sparc ""
fCheck "and so does an unknown arch" fTag linux-riscv64 ""
hostName="MINGW64_NT-10.0-26100"
fCheck "windows-x86_64 built on Windows is gnuwwi" fTag windows-x86_64 gnuwwi
hostName="Plan9"
fCheck "an unknown host gets no tag" fTag linux-x86_64 ""
unset -f uname

## --quick turns off every slow stage, and is an option the parser knows. It
## keeps the x86_64 cross build and drops the ARM64 ones.
parse="$(fLift '^assumeYes=0; ' '^esac; done$')"
## The lines after the loop that refuse an option pair and settle the leak
## checks, lifted on their own.
refusal="$(grep -F 'if ((gate && leakAsked && leakcheck)); then' "${engine}" || true)"
fCheck "the parser has a line refusing --leakcheck with --gate" test -n "${refusal}"
leakOff="$(grep -F 'if ((quick && ! leakAsked)) || ((gate)); then leakcheck=0; fi' "${engine}" || true)"
fCheck "and one turning the leak checks off under --quick and --gate" test -n "${leakOff}"
codeqlGate="$(grep -F 'if ((gate && codeql)); then' "${engine}" || true)"
fCheck "and one refusing --codeql with --gate" test -n "${codeqlGate}"
parse+=$'\n'"${refusal}"$'\n'"${codeqlGate}"$'\n'"${leakOff}"
fParse(){  ## fParse <args...>: prints the settings the parser leaves, or exits as it does
	BUILD_CROSS=1; PROFILE_ENABLE=1; PACKAGE_ENABLE=1; FUZZ_SECS=60
	FMT_CMD=(cargo fmt); DOGFOOD_DESTS=(x); GIT_PUBLISH=(x); DEMO_ENABLE=1
	set -- "${@}"
	eval "${parse}"
	echo "quick=${quick} cross=${BUILD_CROSS} arm=$((! noArm)) demo=${DEMO_ENABLE} gate=${gate} profile=${PROFILE_ENABLE} package=${PACKAGE_ENABLE} fuzz=${FUZZ_SECS} yes=${assumeYes} sync=${sync} leak=${leakcheck} inner=${innerArgs[*]}"
}
rc=0; out="$(fParse --quick 2>&1)" || rc=$?
fCheck "--quick is accepted" test "${rc}" -eq 0
fCheck "and turns off the ARM64 builds, profiling, packages and fuzzing" \
	test "${out}" = "quick=1 cross=1 arm=0 demo=1 gate=0 profile=0 package=0 fuzz=0 yes=0 sync=1 leak=0 inner=--quick"
fCheck "with no options every stage is on, the leak checks too" \
	test "$(fParse)" = "quick=0 cross=1 arm=1 demo=1 gate=0 profile=1 package=1 fuzz=60 yes=0 sync=1 leak=1 inner="
fCheck "--leakcheck adds the leak checks, and goes into the container with the run" \
	test "$(fParse --leakcheck)" = "quick=0 cross=1 arm=1 demo=1 gate=0 profile=1 package=1 fuzz=60 yes=0 sync=1 leak=1 inner=--leakcheck"
fCheck "and stays on under --quick" bash -c '[[ "$1" == "quick=1 "*" leak=1 "* ]]' _ "$(fParse --quick --leakcheck)"
fCheck "whichever comes first" bash -c '[[ "$1" == "quick=1 "*" leak=1 inner=--leakcheck --quick" ]]' _ "$(fParse --leakcheck --quick)"
fCheck "--no-leakcheck drops them from a full run, and goes into the container" \
	test "$(fParse --no-leakcheck)" = "quick=0 cross=1 arm=1 demo=1 gate=0 profile=1 package=1 fuzz=60 yes=0 sync=1 leak=0 inner=--no-leakcheck"
fCheck "and from a quiet one" bash -c '[[ "$1" == *" yes=1 "*" leak=0 "* ]]' _ "$(fParse -q --no-leakcheck)"
fCheck "the last of --leakcheck and --no-leakcheck wins" \
	bash -c '[[ "$1" == *" leak=0 "* && "$2" == *" leak=1 "* ]]' _ "$(fParse --leakcheck --no-leakcheck)" "$(fParse --quick --no-leakcheck --leakcheck)"
rc=0; out="$(fParse --gate 2>&1)" || rc=$?
fCheck "the gate leaves them out with no flag" bash -c '[[ "$1" == 0 && "$2" == *" gate=1 "*" leak=0 "* ]]' _ "${rc}" "${out}"
rc=0; out="$(fParse --gate --no-leakcheck 2>&1)" || rc=$?
fCheck "and takes --no-leakcheck" test "${rc}" -eq 0
rc=0; out="$(fParse --gate --leakcheck 2>&1)" || rc=$?
fCheck "--leakcheck with --gate stops the run" test "${rc}" -eq 2
fCheck "and says why" grep -q 'leaves it out' <<<"${out}"
fCheck "the leak checks run in stage 3, after the ASan run" \
	bash -c 'grep -A1 "^[[:space:]]*fRunAsan$" "$1" | grep -q "^[[:space:]]*fRunLeakcheck$"' _ "${engine}"
fCheck "the config names the heap-growth test, with its feature and one thread" \
	bash -c 'source "$1" && [[ " ${LEAKCHECK_DHAT_CMD[*]} " == *" --features dhat-heap "*" heap_growth:: "*" --test-threads=1 "* ]]' _ "${cicd}/config.bash"
## CodeQL is in the image only, so a run on this box takes it there, or warns
## without docker. The run inside the container does it in place.
## Off unless --codeql asks, until 2026102211462005 settles its cost.
fCodeqlWhere(){
	local container="${1}" leakcheck="${2}" codeql="${3}" docker="${4}" SILK_CICD_IN_CONTAINER="${5:-}" CODEQL_SUITE=x
	eval "$(fLift '^codeqlInImage=0' '^fi$')"
	echo "${codeqlInImage}|${codeqlSkipped}"
}
fCheck "a full run here takes CodeQL to the image when --codeql asks" test "$(fCodeqlWhere 0 1 1 bash)" = "1|"
fCheck "and says so with no docker, rather than stopping" bash -c '[[ "$1" == "0|no docker here"* ]]' _ "$(fCodeqlWhere 0 1 1 no-such-docker-here)"
fCheck "but not with the leak checks off" test "$(fCodeqlWhere 0 0 1 no-such-docker-here)" = "0|"
fCheck "nor without --codeql, docker or not" \
	bash -c '[[ "$1" == "0|" && "$2" == "0|" ]]' _ "$(fCodeqlWhere 0 1 0 bash)" "$(fCodeqlWhere 0 1 0 no-such-docker-here)"
fCheck "nor under --container or inside it, which run it in place" \
	bash -c '[[ "$1" == "0|" && "$2" == "0|" ]]' _ "$(fCodeqlWhere 1 1 1 bash)" "$(fCodeqlWhere 0 1 1 bash 1)"
fParseCodeql(){ fParse "${@}" >/dev/null; eval "${parse}"; echo "codeql=${codeql} leak=${leakcheck} inner=${innerArgs[*]}"; }
fCheck "a full run with no options leaves CodeQL off" bash -c '[[ "$1" == "codeql=0 leak=1 inner=" ]]' _ "$(fParseCodeql)"
fCheck "--codeql adds it, and goes into the container with the run" \
	bash -c '[[ "$1" == "codeql=1 leak=1 inner=--codeql" ]]' _ "$(fParseCodeql --codeql)"
rc=0; out="$(fParse --gate --codeql 2>&1)" || rc=$?
fCheck "--codeql with --gate stops the run" test "${rc}" -eq 2
fCodeqlPlan(){ local codeql="${1}" codeqlSkipped="${2:-}"; eval "$(fLift '^	if ((! codeql)); then codeqlLine=' '^	else codeqlLine="CodeQL"; fi$')"; echo "${codeqlLine}"; }
fCheck "the plan says how to turn it on" test "$(fCodeqlPlan 0)" = "CodeQL (off; --codeql adds it)"
fCheck "and names it when on" bash -c '[[ "$1" == CodeQL && "$2" == "CodeQL (skipped, no docker"* ]]' _ "$(fCodeqlPlan 1)" "$(fCodeqlPlan 1 'no docker here')"
fCheck "the step itself stays off without --codeql" \
	bash -c 'sed -n "/^fRunCodeql(){/,/^}/p" "$1" | grep -qF "if ((! codeql)); then"' _ "${engine}"
fCheck "the dhat test hands on to CodeQL, with the report folder" \
	bash -c 'sed -n "/^fRunLeakcheck(){/,/^}/p" "$1" | grep -q "^[[:space:]]*fRunCodeql \"\${outDir}\"$"' _ "${engine}"
fCheck "the config names a Rust suite, and the suppressions file is there" \
	bash -c 'source "$1" && [[ "${CODEQL_SUITE}" == codeql/rust-queries:* && -f "$2/../${CODEQL_SUPPRESSIONS}" ]]' _ "${cicd}/config.bash" "${cicd}"
fCheck "--no-demo turns the demo off" bash -c '[[ "$1" == *" demo=0 "* ]]' _ "$(fParse --no-demo)"
fCheck "--gate is the merge gate" bash -c '[[ "$1" == *" gate=1 "* ]]' _ "$(fParse --gate)"
rc=0; out="$(fParse -y --no-such-thing 2>&1)" || rc=$?
fCheck "an unknown option still stops the run" test "${rc}" -eq 2

## The dogfood copy is skipped while that file is running.
eval "$(fLift '^fInUse(){' '^}')"
cp "$(command -v sleep)" "${work}/busy"
cp "$(command -v sleep)" "${work}/idle"
ln -s "${work}/busy" "${work}/link"
"${work}/busy" 60 &
started+=("$!")
for _ in {1..50}; do [[ "$(readlink "/proc/${started[0]}/exe" 2>/dev/null)" == "${work}/busy" ]] && break; sleep 0.05; done
fCheck "a copy that is running is in use" fInUse "${work}/busy"
fCheck "and so it is through a link" fInUse "${work}/link"
fCheck "an idle copy is not" fNot fInUse "${work}/idle"
fCheck "nor is a file that is not there" fNot fInUse "${work}/none"
ln "${work}/busy" "${work}/hard"
fCheck "nor a hard link to the running copy under another name" fNot fInUse "${work}/hard"
cp "$(command -v sleep)" "${work}/swap"
"${work}/swap" 60 &
started+=("$!")
for _ in {1..50}; do [[ "$(readlink "/proc/${started[1]}/exe" 2>/dev/null)" == "${work}/swap" ]] && break; sleep 0.05; done
rm -f "${work}/swap"; cp "$(command -v sleep)" "${work}/swap"
fCheck "nor a new file put where a running one was deleted" fNot fInUse "${work}/swap"

## The same check over a stand-in /proc of 2000 processes. One fork per process
## was about 4,500 a full run.
fake="${work}/proc"
python3 - "${fake}" "${work}" <<'PY'
import os, sys
fake, work = sys.argv[1], sys.argv[2]
for pid in range(1, 2001):
	os.makedirs(f"{fake}/{pid}")
	if pid % 7 == 0:
		continue  # a kernel thread has no exe
	target = f"{work}/idlehard" if pid == 500 else f"{work}/busy" if pid == 1999 else f"{work}/gone (deleted)" if pid % 5 == 0 else f"{work}/other"
	os.symlink(target, f"{fake}/{pid}/exe")
PY
cp "$(command -v sleep)" "${work}/other"; ln "${work}/idle" "${work}/idlehard"
eval "$(fLift '^fInUse(){' '^}' | sed "s/^fInUse(){/fInUseFake(){/; s#/proc/#${fake}/#")"
fFewForks(){
	local n limit
	fForkCount n "$(declare -f fInUseFake)" "fInUseFake $(printf '%q' "${work}/idle")"
	limit=$((forkCountExact ? 10 : 100)); echo "    ${n} forks, limit ${limit}"; ((n <= limit))
}
fCheck "the stand-in list finds a running copy" fInUseFake "${work}/busy"
fCheck "and not an idle one" fNot fInUseFake "${work}/idle"
fCheck "nor a file whose hard link one process there runs" fNot fInUseFake "${work}/idle"
fCheck "but that link is in use by its own name" fInUseFake "${work}/idlehard"
fCheck "and 2000 processes cost a few forks, not one each" fFewForks

## The build number: a clean tree takes its commit's time, a dirty one the clock,
## and a value handed down is kept.
eval "$(fLift '^fPinBuildMinutes(){' '^}')"
repo="${work}/repo"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
git init -q -b main "${repo}"
git -C "${repo}" config user.name t; git -C "${repo}" config user.email t@t
echo one >"${repo}/file.txt"
git -C "${repo}" add file.txt
GIT_COMMITTER_DATE="2026-01-02T03:04:05Z" git -C "${repo}" commit -qm first
commitMinutes=$(( ($(date -d 2026-01-02T03:04:05Z +%s) - 946684800) / 60 ))
fMinutes(){ (cd "${1}" && unset SILK_BUILD_MINUTES && fPinBuildMinutes && echo "${SILK_BUILD_MINUTES}"); }
fNearNow(){ local -r now=$(( ($(date +%s) - 946684800) / 60 )); (( ${1} >= now - 1 && ${1} <= now )); }
fCheck "a clean tree takes its commit's time" test "$(fMinutes "${repo}")" = "${commitMinutes}"
echo stray >"${repo}/untracked.txt"
fCheck "an untracked file leaves it clean" test "$(fMinutes "${repo}")" = "${commitMinutes}"
echo two >>"${repo}/file.txt"
fCheck "a dirty tree takes the clock" fNearNow "$(fMinutes "${repo}")"
mkdir -p "${work}/plain"
fCheck "and so does a folder that is not a repository" fNearNow "$(fMinutes "${work}/plain")"
fCheck "a value handed down is kept" test "$(cd "${repo}" && SILK_BUILD_MINUTES=12345 && fPinBuildMinutes && echo "${SILK_BUILD_MINUTES}")" = 12345

## The plan's host line, for each host the pipeline runs on.
eval "$(fLift '^fHostDescribe(){' '^}')"
fHost(){ test "$(env -u CICD_LINUX_HALF -u WSL_DISTRO_NAME "${@:3}" bash -c "$(declare -f fHostDescribe); "'fHostDescribe "$@"' _ "${1}" x86_64 "${2}" "Debian GNU/Linux 13 (trixie)")" = "${want}"; }
want="Linux (Debian GNU/Linux 13 (trixie)), x86_64"
fCheck "plain Linux names the distribution and arch" fHost Linux "Linux version 6.12.101+deb13-amd64 (debian-kernel@lists.debian.org)"
wsl2="Linux version 5.15.167.4-microsoft-standard-WSL2 (root@f9c826d3017f)"
want="WSL2 (Ubuntu) on Windows, run on its own"
fCheck "WSL2 on its own says so" fHost Linux "${wsl2}" WSL_DISTRO_NAME=Ubuntu
want="WSL2 (Ubuntu) on Windows, the Linux half of a cicd-win.ps1 run"
fCheck "WSL2 driven by cicd-win.ps1 says so" fHost Linux "${wsl2}" WSL_DISTRO_NAME=Ubuntu CICD_LINUX_HALF=1
want="WSL on Windows, run on its own"
fCheck "WSL1 is not taken for WSL2" fHost Linux "Linux version 4.4.0-19041-Microsoft (Microsoft@Microsoft.com)"
want="Windows (MSYS bash) - cicd-win.ps1 is the pipeline for this box"
fCheck "an MSYS shell is pointed at the Windows pipeline" fHost MINGW64_NT-10.0-26100 ""
want="FreeBSD, x86_64"
fCheck "anything else is named as it is" fHost FreeBSD ""
eval "$(fLift '^fHostLine(){' '^}')"
fCheck "and this box gets a line" test -n "$(fHostLine)"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261022: Where CodeQL runs, and --codeql, off by default.
##		- 20261022: --no-leakcheck, and the leak checks on with no options.
##		- 20260926 JC: Created.
