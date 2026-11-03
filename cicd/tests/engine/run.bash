#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted code reads them, and shellcheck cannot see into an eval.
#  shellcheck disable=2154  ## 'referenced but not assigned.' The lifted code assigns them, inside an eval.

##	- Purpose:
##		Pieces of cicd.bash that decide what a run does, lifted out of the file as
##		they stand and run on their own: the build retry, the dogfood tag, the
##		option parser, the running-copy check, the build number and the host line.
##	- Test ID: dd5nn
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
fEnd(){ local -r rc=$?; local p; for p in "${started[@]}"; do kill "${p}" 2>/dev/null || true; done; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"; }
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
leakOff="$(grep -F 'if ((gate)); then leakcheck=0; fi' "${engine}" || true)"
fCheck "and one turning the leak checks off under --gate" test -n "${leakOff}"
codeqlGate="$(grep -F 'if ((gate && codeql)); then' "${engine}" || true)"
fCheck "and one refusing --codeql with --gate" test -n "${codeqlGate}"
## The backlog runs and the merge gate, settled after the loop.
runs="$(fLift '^gateAreasAll=' '^if ((fmtCheck ')"
fCheck "the parser settles the backlog runs and the merge gate after the loop" test -n "${runs}"
parse+=$'\n'"${refusal}"$'\n'"${codeqlGate}"$'\n'"${leakOff}"$'\n'"${runs}"
fParse(){  ## fParse <args...>: prints the settings the parser leaves, or exits as it does
	BUILD_CROSS=1; PROFILE_ENABLE=1; PACKAGE_ENABLE=1; FUZZ_SECS=60
	FMT_CMD=(cargo fmt); FMT_CHECK_CMD=(cargo fmt --check); DOGFOOD_DESTS=(x); GIT_PUBLISH=(x); DEMO_ENABLE=1
	set -- "${@}"
	eval "${parse}"
	echo "quick=${quick} cross=${BUILD_CROSS} arm=$((! noArm)) demo=${DEMO_ENABLE} gate=${gate} profile=${PROFILE_ENABLE} package=${PACKAGE_ENABLE} fuzz=${FUZZ_SECS} yes=${assumeYes} sync=${sync} leak=${leakcheck} inner=${innerArgs[*]}"
}
rc=0; out="$(fParse --quick 2>&1)" || rc=$?
fCheck "--quick is accepted" test "${rc}" -eq 0
fCheck "and turns off the ARM64 builds, profiling, packages and fuzzing, but not the fast leak checks" \
	test "${out}" = "quick=1 cross=1 arm=0 demo=1 gate=0 profile=0 package=0 fuzz=0 yes=0 sync=1 leak=1 inner=--quick"
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
## Off unless --codeql, --leakcheck or --long asks, since it takes over half an hour.
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
fCheck "--leakcheck adds it, being every leak check" \
	bash -c '[[ "$1" == "codeql=1 leak=1 inner=--leakcheck" ]]' _ "$(fParseCodeql --leakcheck)"
fCheck "under --quick too, which alone leaves it off" \
	bash -c '[[ "$1" == "codeql=1 leak=1 inner=--quick --leakcheck" && "$2" == "codeql=0 leak=1 inner=--quick" ]]' _ "$(fParseCodeql --quick --leakcheck)" "$(fParseCodeql --quick)"
fCheck "but not once --no-leakcheck comes after it" \
	bash -c '[[ "$1" == "codeql=0 leak=0 "* && "$2" == "codeql=1 leak=1 "* ]]' _ "$(fParseCodeql --leakcheck --no-leakcheck)" "$(fParseCodeql --no-leakcheck --leakcheck)"
rc=0; out="$(fParse --gate --codeql 2>&1)" || rc=$?
fCheck "--codeql with --gate stops the run" test "${rc}" -eq 2
fCodeqlPlan(){ local codeql="${1}" codeqlSkipped="${2:-}"; eval "$(fLift '^	if ((! codeql)); then codeqlLine=' '^	else codeqlLine="CodeQL"; fi$')"; echo "${codeqlLine}"; }
fCheck "the plan says how to turn it on" test "$(fCodeqlPlan 0)" = "CodeQL (off; --leakcheck or --long adds it)"
fCheck "and names it when on" bash -c '[[ "$1" == CodeQL && "$2" == "CodeQL (skipped, no docker"* ]]' _ "$(fCodeqlPlan 1)" "$(fCodeqlPlan 1 'no docker here')"
fCheck "the step itself stays off without --codeql" \
	bash -c 'sed -n "/^fRunCodeql(){/,/^}/p" "$1" | grep -qF "if ((! codeql)); then"' _ "${engine}"
fCheck "the dhat test hands on to CodeQL, with the report folder" \
	bash -c 'sed -n "/^fRunLeakcheck(){/,/^}/p" "$1" | grep -q "^[[:space:]]*fRunCodeql \"\${outDir}\"$"' _ "${engine}"
fCheck "the config names a Rust suite, and the suppressions file is there" \
	bash -c 'source "$1" && [[ "${CODEQL_SUITE}" == codeql/rust-queries:* && -f "$2/../${CODEQL_SUPPRESSIONS}" ]]' _ "${cicd}/config.bash" "${cicd}"
## syn and the like reach the crate list only through proc macros, which run at
## build time, and brought about 200 results with them (2026102211462005).
fCheck "the release crates leave out a proc macro's own dependencies" \
	grep -qF 'cargo tree --offline --locked -e normal,no-proc-macro ' "${cicd}/utility/codeql.bash"
## The SARIF filter, lifted from codeql.bash, on two results with the same rule
## and path: one from a CFF dict's UidBase, one that adds a real password.
sed -n "/<<'EOF' || rc=\$?$/,/^EOF$/p" "${cicd}/utility/codeql.bash" | sed '1d;$d' >"${work}/cqfilter.py"
printf 'serde-1.0.0\n' >"${work}/cqcrates.txt"
cat >"${work}/cqraw.sarif" <<'EOF'
{"runs": [{"results": [
 {"ruleId": "rust/cleartext-logging", "message": {"text": "This operation writes [...::UidBase(...)](1) to a log file."}, "locations": [{"physicalLocation": {"artifactLocation": {"uri": "source/src/a.rs"}, "region": {"startLine": 3}}}]},
 {"ruleId": "rust/cleartext-logging", "message": {"text": "This operation writes [...::UidBase(...)](1) to a log file. This operation writes [password](2) to a log file."}, "locations": [{"physicalLocation": {"artifactLocation": {"uri": "source/src/a.rs"}, "region": {"startLine": 9}}}]},
 {"ruleId": "rust/unused-variable", "message": {"text": "Variable 'x' is not used."}, "locations": [{"physicalLocation": {"artifactLocation": {"uri": "vendor/serde-1.0.0/src/de.rs"}, "region": {"startLine": 5}}}]},
 {"ruleId": "rust/unused-variable", "message": {"text": "Variable 'y' is not used."}, "locations": [{"physicalLocation": {"artifactLocation": {"uri": "vendor/syn-2.0.0/src/lib.rs"}, "region": {"startLine": 7}}}]}
]}]}
EOF
printf '# a note\nrust/cleartext-logging source/* 1111 \\.\\.\\.::(UidBase|Uuid)\\(\\.\\.\\.\\)\nrust/unused-variable vendor/serde-*/* 2222\n' >"${work}/cqsupp.txt"
fCqFilter(){ python3 "${work}/cqfilter.py" "${work}/cqraw.sarif" "${work}/cqout.sarif" "${work}/cqcrates.txt" "${work}/cqsupp.txt"; }
rc=0; out="$(fCqFilter)" || rc=$?
fCheck "a suppression naming a flow's sources lets that flow through" grep -qF 'known, 1111: rust/cleartext-logging source/src/a.rs:3:' <<<"${out}"
fCheck "but not one that also starts from something else, and that fails the step" \
	bash -c 'grep -qF "NEW: rust/cleartext-logging source/src/a.rs:9:" <<<"$1" && (($2 == 1))' _ "${out}" "${rc}"
fCheck "a suppression with no sources still goes by rule and path" grep -qF 'known, 2222: rust/unused-variable vendor/serde-1.0.0/src/de.rs:5:' <<<"${out}"
fCheck "and a crate outside the release binary is dropped" fNot grep -qF 'syn-2.0.0' <<<"${out}"
## The soak test is a leak check, about 2.5 minutes, so --quick leaves it out
## unless --leakcheck asks. It needs heaptrack at the image's pin, so a run on
## this box takes it there, or warns without docker.
soakOn="$(grep -F 'soak=0; if ((leakcheck && ' "${engine}" || true)"
fCheck "the parser has a line settling the soak test" test -n "${soakOn}"
fParseSoak(){ fParse "${@}" >/dev/null; eval "${parse}"; LEAKCHECK_SOAK="${soakScript-cicd/tests/soak/run.bash}"; eval "${soakOn}"; echo "soak=${soak} leak=${leakcheck}"; }
fCheck "a full run with no options runs the soak test" test "$(fParseSoak)" = "soak=1 leak=1"
fCheck "--quick leaves it out, and --leakcheck puts it back, in either order" \
	bash -c '[[ "$1" == "soak=0 leak=1" && "$2" == "soak=1 leak=1" && "$3" == "soak=1 leak=1" ]]' _ "$(fParseSoak --quick)" "$(fParseSoak --quick --leakcheck)" "$(fParseSoak --leakcheck --quick)"
fCheck "--no-leakcheck drops it, from a full run and after --leakcheck" \
	bash -c '[[ "$1" == "soak=0 leak=0" && "$2" == "soak=0 leak=0" ]]' _ "$(fParseSoak --no-leakcheck)" "$(fParseSoak --quick --leakcheck --no-leakcheck)"
fCheck "--gate leaves it out" test "$(fParseSoak --gate)" = "soak=0 leak=0"
fCheck "and an empty LEAKCHECK_SOAK turns it off" test "$(soakScript="" fParseSoak)" = "soak=0 leak=1"
fSoakWhere(){
	local container="${1}" soak="${2}" docker="${3}" SILK_CICD_IN_CONTAINER="${4:-}"
	eval "$(fLift '^soakInImage=0' '^fi$')"
	echo "${soakInImage}|${soakSkipped}"
}
fCheck "a full run here takes the soak test to the image" test "$(fSoakWhere 0 1 bash)" = "1|"
fCheck "and says so with no docker, rather than stopping" bash -c '[[ "$1" == "0|no docker here"* ]]' _ "$(fSoakWhere 0 1 no-such-docker-here)"
fCheck "but not when it's off" test "$(fSoakWhere 0 0 bash)" = "0|"
fCheck "nor from a run already in the container" \
	bash -c '[[ "$1" == "0|" && "$2" == "0|" ]]' _ "$(fSoakWhere 1 1 bash)" "$(fSoakWhere 0 1 bash 1)"
fCheck "and a run on this box builds the image for it" grep -qF 'if ((container || asanInImage || tsanInImage || miriInImage || codeqlInImage || soakInImage)); then' "${engine}"
fSoakPlan(){ local soak="${1}" soakSkipped="${2:-}" LEAKCHECK_SOAK="${3-cicd/tests/soak/run.bash}"; eval "$(fLift '^	if \[\[ -z "\${LEAKCHECK_SOAK:-}" \]\]; then soakLine=' '^	else soakLine=')"; echo "${soakLine}"; }
fCheck "the plan names the soak test when on" test "$(fSoakPlan 1)" = "soak test under heaptrack"
fCheck "and says how to turn it on under --quick" test "$(fSoakPlan 0)" = "soak test (off under --quick; --leakcheck adds it)"
fCheck "and why it's skipped" bash -c '[[ "$1" == "soak test (skipped, no docker"* && "$2" == "soak test (skipped)" ]]' _ "$(fSoakPlan 1 'no docker here')" "$(fSoakPlan 0 '' '')"
fCheck "the plan's leak checks line carries it" grep -qF 'Leak checks .........: heap growth under dhat, ${soakLine}, ${codeqlLine}' "${engine}"
fCheck "--list names it when it would run" grep -qF 'if ((soak)); then fListTest "${LEAKCHECK_SOAK}" ' "${engine}"
fCheck "the dhat test hands on to the soak test before CodeQL, with the report folder" \
	bash -c 'sed -n "/^fRunLeakcheck(){/,/^}/p" "$1" | grep -A1 "^[[:space:]]*fRunSoak \"\${outDir}\"$" | grep -q "^[[:space:]]*fRunCodeql "' _ "${engine}"
## The step lifted on its own, with the script and the container run stood in.
fRunSoakAs(){  ## fRunSoakAs <soak> <exit> [in container]: what the step printed, and DIE or rc
	local soak="${1}" soakSkipped="" stamp=20261101-120000 containerImage=img SILK_CICD_IN_CONTAINER="${3:-}"
	local root="${work}/soakroot" LEAKCHECK_SOAK=soak.bash
	mkdir -p "${root}/out"; printf '#!/bin/sh\necho "ran $*"\nexit %s\n' "${2}" >"${root}/soak.bash"; chmod +x "${root}/soak.bash"
	fJobs(){ :; }; fEcho_Clean(){ echo "    ${*}"; }; fContainerBuild(){ :; }
	fContainerRun(){ echo "in ${1}:"; local -r s="${2}"; shift 2; "${s}" "${@}"; }
	## A stand-in keeps the real name only for a real command, so the copy calls a spy.
	fRotateSpy(){ echo "rotate ${2} ${3}" >>"${work}/soak-rotated"; }; : >"${work}/soak-rotated"
	eval "$(fLift '^fRunSoak(){' '^}' | sed 's/\bgfs_rotate /fRotateSpy /')"
	( fRunSoak "${root}/out" ) || echo "rc=$?"
	sed 's/^/    /' "${work}/soak-rotated"
}
out="$(fRunSoakAs 1 0 1)"
fCheck "in the container the step runs the script with the folder and the run's stamp" grep -qxF "ran --out ${work}/soakroot/out --stamp 20261101-120000" <<<"${out}"
fCheck "and rotates the summary and the data" bash -c '[[ "$1" == *"rotate soak txt"*"rotate soak zst"* ]]' _ "${out}"
out="$(fRunSoakAs 1 0)"
fCheck "on this box it goes through the image" grep -q '^in img:' <<<"${out}"
out="$(fRunSoakAs 1 1 1)"
fCheck "a failed soak test stops the run, saying where the data is" bash -c '[[ "$1" == *"DIE: the soak test failed"*"soakroot/out"* && "$1" != *rotate* ]]' _ "${out}"
out="$(fRunSoakAs 1 3 1)"
fCheck "and so does one that couldn't run, since the image has every tool" grep -qF 'DIE: the soak test could not run' <<<"${out}"
out="$(fRunSoakAs 0 0 1)"
fCheck "off, it runs nothing" bash -c '[[ "$1" != *ran* && "$1" == *"soak test off under --quick"* ]]' _ "${out}"
fCheck "the config names the soak test, which has a test ID" \
	bash -c 'source "$1" && [[ -x "$2/../${LEAKCHECK_SOAK}" ]] && grep -q "^##	- Test ID: der46$" "$2/../${LEAKCHECK_SOAK}"' _ "${cicd}/config.bash" "${cicd}"
## --long, or --slow: the steps that take hours. TSan and CodeQL wait for it, so
## the default run stays near an hour.
fParseLong(){ fParse "${@}" >/dev/null; eval "${parse}"; echo "long=${long} codeql=${codeql} leak=${leakcheck} inner=${innerArgs[*]}"; }
fCheck "a run with no options leaves --long off" test "$(fParseLong)" = "long=0 codeql=0 leak=1 inner="
fCheck "--long turns on CodeQL, and goes into the container" test "$(fParseLong --long)" = "long=1 codeql=1 leak=1 inner=--long"
fCheck "--slow is the same" test "$(fParseLong --slow)" = "long=1 codeql=1 leak=1 inner=--long"
fCheck "--no-leakcheck still drops the leak checks" bash -c '[[ "$1" == *" leak=0 "* ]]' _ "$(fParseLong --long --no-leakcheck)"
fCheck "--backlog-run-full takes --long" \
	bash -c '[[ "$1" == "long=1 codeql=1 leak=1 inner=--long --fmt-check" ]]' _ "$(fParseLong --backlog-run-full --long)"
for pair in "--long --quick" "--quick --long" "--slow --backlog-run-quick" "--backlog-run-quick --long"; do
	read -r -a args <<<"${pair}"
	rc=0; out="$(fParse "${args[@]}" 2>&1)" || rc=$?
	fCheck "${pair} stops the run" bash -c '[[ "$1" == 2 && "$2" == *"--long"* ]]' _ "${rc}" "${out}"
done
fTsanWhere(){
	local container="${1}" long="${2}" docker="${3}" SILK_CICD_IN_CONTAINER="${4:-}" gate=0 quick=0 TSAN_CMD=(x)
	eval "$(fLift '^tsanInImage=0' '^fi$')"
	echo "${tsanInImage}|${tsanSkipped}"
}
fCheck "a run here takes TSan to the image under --long" test "$(fTsanWhere 0 1 bash)" = "1|"
fCheck "and says so with no docker" bash -c '[[ "$1" == "0|no docker here"* ]]' _ "$(fTsanWhere 0 1 no-such-docker-here)"
fCheck "but not without --long, docker or not" \
	bash -c '[[ "$1" == "0|" && "$2" == "0|" ]]' _ "$(fTsanWhere 0 0 bash)" "$(fTsanWhere 0 0 no-such-docker-here)"
fTsanStep(){
	local long="${1}" quick=0 tsanSkipped="no docker here" TSAN_CMD=(x)
	fEcho_Clean(){ echo "${*}"; }; fJobs(){ :; }
	eval "$(fLift '^fRunTsan(){' '^}')"
	fRunTsan
}
fCheck "the TSan step is off without --long" test "$(fTsanStep 0)" = "TSan run off (--long adds it)"
fCheck "and runs with it" bash -c '[[ "$1" == *"TSan run skipped: no docker here" ]]' _ "$(fTsanStep 1)"
fTsanPlan(){
	local long="${1}" quick=0 tsanSkipped="" TSAN_CMD=(x) SANITIZER_TOOLCHAIN=nightly containerImage=img
	fEcho_Clean(){ echo "${*}"; }
	eval "$(fLift 'TSAN_CMD\[@\]} == 0)); then$' '^fi$')"
}
fCheck "the plan says how to turn TSan on" test "$(fTsanPlan 0)" = "TSan ................: (off; --long adds it)"
fCheck "and names it when on" test "$(fTsanPlan 1)" = "TSan ................: the tests again on nightly, in img"
## Miri: hours even for the fast part, so only --leakcheck or --long asks for it,
## and a later --no-leakcheck drops it with the rest (2026102320591323).
fParseMiri(){ MIRI_CMD=(x); fParse "${@}" >/dev/null; eval "${parse}"; echo "miri=${miri} leak=${leakcheck} cmd=${#MIRI_CMD[@]} inner=${innerArgs[*]}"; }
fCheck "a run with no options leaves Miri off" test "$(fParseMiri)" = "miri=0 leak=1 cmd=1 inner="
fCheck "and so does --quick" bash -c '[[ "$1" == "miri=0 "* ]]' _ "$(fParseMiri --quick)"
fCheck "--leakcheck turns it on, under --quick too" \
	bash -c '[[ "$1" == "miri=1 leak=1 cmd=1 inner=--leakcheck" && "$2" == "miri=1 "* ]]' _ "$(fParseMiri --leakcheck)" "$(fParseMiri --quick --leakcheck)"
fCheck "--long and --slow do" bash -c '[[ "$1" == "miri=1 "* && "$2" == "miri=1 "* ]]' _ "$(fParseMiri --long)" "$(fParseMiri --slow)"
fCheck "--backlog-run-full leaves it off, and takes --long" \
	bash -c '[[ "$1" == "miri=0 "* && "$2" == "miri=1 "* ]]' _ "$(fParseMiri --backlog-run-full)" "$(fParseMiri --backlog-run-full --long)"
fCheck "the last of --leakcheck and --no-leakcheck wins for it too" \
	bash -c '[[ "$1" == "miri=0 leak=0 "* && "$2" == "miri=1 leak=1 "* && "$3" == "miri=0 leak=0 "* ]]' _ \
	"$(fParseMiri --leakcheck --no-leakcheck)" "$(fParseMiri --no-leakcheck --leakcheck)" "$(fParseMiri --long --no-leakcheck)"
fCheck "--no-miri empties its command, and goes into the container" \
	test "$(fParseMiri --long --no-miri)" = "miri=1 leak=1 cmd=0 inner=--long --no-miri"
fMiriWhere(){
	local container="${1}" miri="${2}" docker="${3}" SILK_CICD_IN_CONTAINER="${4:-}" MIRI_CMD=(x)
	eval "$(fLift '^miriInImage=0' '^fi$')"
	echo "${miriInImage}|${miriSkipped}"
}
fCheck "a run here takes Miri to the image when it's on" test "$(fMiriWhere 0 1 bash)" = "1|"
fCheck "and says so with no docker" bash -c '[[ "$1" == "0|no docker here"* ]]' _ "$(fMiriWhere 0 1 no-such-docker-here)"
fCheck "but not when it's off, docker or not" \
	bash -c '[[ "$1" == "0|" && "$2" == "0|" ]]' _ "$(fMiriWhere 0 0 bash)" "$(fMiriWhere 0 0 no-such-docker-here)"
fCheck "nor from a run already in the container" \
	bash -c '[[ "$1" == "0|" && "$2" == "0|" ]]' _ "$(fMiriWhere 1 1 bash)" "$(fMiriWhere 0 1 bash 1)"
fMiriStep(){
	local miri="${1}" leakcheck="${2:-1}" miriSkipped="no docker here" MIRI_CMD=(x)
	fEcho_Clean(){ echo "${*}"; }; fJobs(){ :; }
	eval "$(fLift '^fRunMiri(){' '^}')"
	fRunMiri
}
fCheck "the Miri step is off without --leakcheck or --long" test "$(fMiriStep 0)" = "Miri run off (--leakcheck or --long adds it)"
fCheck "and says why under --no-leakcheck" test "$(fMiriStep 0 0)" = "Miri run skipped (--no-leakcheck)"
fCheck "and runs when on" bash -c '[[ "$1" == *"Miri run skipped: no docker here" ]]' _ "$(fMiriStep 1)"
fMiriPlan(){
	local miri="${1}" leakcheck="${2:-1}" miriSkipped="${3:-}" MIRI_CMD=(x) SANITIZER_TOOLCHAIN=nightly containerImage=img
	fEcho_Clean(){ echo "${*}"; }
	eval "$(fLift 'MIRI_CMD\[@\]} == 0)); then$' '^fi$')"
}
fCheck "the plan says how to turn Miri on" test "$(fMiriPlan 0)" = "Miri ................: (off; --leakcheck or --long adds it)"
fCheck "and that --no-leakcheck took it out" test "$(fMiriPlan 0 0)" = "Miri ................: (skipped --no-leakcheck)"
fCheck "and names it when on, as very slow" test "$(fMiriPlan 1)" = "Miri ................: the tests again on nightly, in img; very slow, hours"
fCheck "and why it's skipped" test "$(fMiriPlan 1 1 'no docker here')" = "Miri ................: (skipped, no docker here)"
fMiriFirst(){ local miri="${1}" miriSkipped="${2:-}" MIRI_CMD=(x); fEcho_Clean(){ echo "${*}"; }; eval "$(grep '^if ((miri && ${#MIRI_CMD\[@\]})) && \[\[ -z "${miriSkipped}" \]\]; then fEcho_Clean "Miri is on' "${engine}")"; }
fCheck "a run with Miri on says it's very slow before it starts" \
	bash -c '[[ "$1" == "Miri is on: very slow"*"Ctrl+C"* && -z "$2" && -z "$3" ]]' _ "$(fMiriFirst 1)" "$(fMiriFirst 0)" "$(fMiriFirst 1 'no docker here')"
fCheck "before the fail-fast line, which a run without -y prompts after" \
	bash -c 'grep -A1 "^if ((miri && .*Miri is on" "$1" | grep -q "^fEcho_Clean \"Fail-fast"' _ "${engine}"
fCheck "the Miri run follows the TSan run in stage 3" \
	bash -c 'grep -A1 "^[[:space:]]*fRunTsan$" "$1" | grep -q "^[[:space:]]*fRunMiri$"' _ "${engine}"
## fMiriConfig [CARGO_TARGET_DIR]: Miri's sysroot path is absolute either way.
## The container hands in an absolute target dir, a run here none.
fMiriConfig(){
	local -a targetEnv=(-u CARGO_TARGET_DIR); [[ -z "${1:-}" ]] || targetEnv=("CARGO_TARGET_DIR=${1}")
	env "${targetEnv[@]}" bash -c 'source "$1" && sys="${TARGET_DIR}/${MIRI_DIR}/miri/sysroot" && { [[ "${sys}" == /* ]] || sys="$2/${sys}"; } && [[ " ${MIRI_CMD[*]} " == *" MIRIFLAGS=-Zmiri-disable-isolation cargo +${SANITIZER_TOOLCHAIN} miri test "* && "${MIRI_DIR}" == "miri-${SANITIZER_TOOLCHAIN#nightly-}" && " ${MIRI_ENV[*]} " == *" CARGO_TARGET_DIR=${TARGET_DIR}/${MIRI_DIR} MIRI_SYSROOT=${sys} "* ]]' _ "${cicd}/config.bash" "$(cd "${cicd}/.." && pwd)"
}
fCheck "the config runs Miri on the nightly with isolation off, in a dir named for it" fMiriConfig
fCheck "and with an absolute target dir, as in the container" fMiriConfig /no/such/target
fCheck "and the cleanup and the backup know that dir" \
	bash -c 'source "$1" && [[ " ${CLEANUP_NIGHTLY_DIRS[*]} " == *" ${MIRI_DIR}/miri "* && "${MIRI_ENV[*]}" == *"/${MIRI_DIR}/miri/sysroot"* ]] && grep -qxF "*/target/miri-*" <<<"${GIT_BACKUP_AND_PUBLISH_RAR_EXCLUDES}"' _ "${cicd}/config.bash"
fCheck "--list says what the Miri step runs" grep -qF 'if ((miri && ${#MIRI_CMD[@]})); then fEcho_Clean "Miri: the canary' "${engine}"
fCheck "--no-demo turns the demo off" bash -c '[[ "$1" == *" demo=0 "* ]]' _ "$(fParse --no-demo)"
fCheck "--gate is the backlog quick run" bash -c '[[ "$1" == *" gate=1 "* ]]' _ "$(fParse --gate 2>/dev/null)"

## --backlog-run-quick, --backlog-run-full and --merge-gate. None of them syncs,
## dogfoods, publishes, rewrites a tracked file or prompts.
fParseRun(){  ## fParseRun <args...>: what the 3 runs settle, or exits as the parser does
	BUILD_CROSS=1; PROFILE_ENABLE=1; PACKAGE_ENABLE=1; FUZZ_SECS=60
	FMT_CMD=(cargo fmt); FMT_CHECK_CMD=(cargo fmt --check); DOGFOOD_DESTS=(x); GIT_PUBLISH=(x); DEMO_ENABLE=1
	eval "${parse}"
	echo "gate=${gate} areas=${gateAreas[*]} full=${fullRun} yes=${assumeYes} sync=${sync} fmt=${FMT_CMD[*]} dogfood=${#DOGFOOD_DESTS[@]} publish=${#GIT_PUBLISH[@]} demo=${DEMO_ENABLE} leak=${leakcheck} inner=${innerArgs[*]}"
}
fCheck "--backlog-run-quick runs every area with none named" \
	test "$(fParseRun --backlog-run-quick)" = "gate=1 areas=fmt clippy tests scripts docs full=0 yes=0 sync=1 fmt=cargo fmt dogfood=1 publish=1 demo=1 leak=0 inner="
fCheck "or only the ones named, in order, up to the next option" \
	bash -c '[[ "$1" == "gate=1 areas=docs fmt full=0 "*" inner=--no-fuzz" ]]' _ "$(fParseRun --backlog-run-quick docs fmt --no-fuzz)"
rc=0; out="$(fParseRun --backlog-run-quick fmt parser 2>&1)" || rc=$?
fCheck "an unknown area stops the run, naming the areas" bash -c '[[ "$1" == 2 && "$2" == *"unknown area: parser (areas: fmt clippy tests scripts docs)"* ]]' _ "${rc}" "${out}"
rc=0; out="$(fParseRun --gate docs 2>&1)" || rc=$?
fCheck "--gate takes areas too" bash -c '[[ "$1" == 0 && "$2" == *"gate=1 areas=docs full=0 "* ]]' _ "${rc}" "${out}"
fCheck "and says in one line what it is another name for" test "$(grep -c -- '--gate is another name for --backlog-run-quick' <<<"${out}")" -eq 1
for run in --backlog-run-full --merge-gate; do
	fCheck "${run} turns off sync, dogfood, the demo and publish, checks the format, and asks nothing" \
		test "$(fParseRun "${run}")" = "gate=0 areas= full=1 yes=1 sync=0 fmt=cargo fmt --check dogfood=0 publish=0 demo=0 leak=1 inner=--fmt-check"
	fCheck "${run} keeps them off when asked for after it" \
		bash -c '[[ "$1" == *" sync=0 fmt=cargo fmt --check dogfood=0 publish=0 demo=0 "* ]]' _ "$(fParseRun "${run}" --demo -m hello)"
	rc=0; out="$(fParseRun "${run}" --quick 2>&1)" || rc=$?
	fCheck "${run} with --quick stops the run" bash -c '[[ "$1" == 2 && "$2" == *"every check of the default run"* ]]' _ "${rc}" "${out}"
	rc=0; out="$(fParseRun "${run}" --backlog-run-quick 2>&1)" || rc=$?
	fCheck "${run} with --backlog-run-quick stops the run" bash -c '[[ "$1" == 2 && "$2" == *"different runs"* ]]' _ "${rc}" "${out}"
done
rc=0; out="$(fParseRun --ci 2>&1)" || rc=$?
fCheck "--ci is the merge gate" bash -c '[[ "$1" == 0 && "$2" == *" full=1 yes=1 sync=0 "* ]]' _ "${rc}" "${out}"
fCheck "and says so in one line" test "$(grep -c -- '--ci is another name for --merge-gate' <<<"${out}")" -eq 1
fCheck "--fmt-check checks rather than rewrites, and goes into the container" \
	bash -c '[[ "$1" == *" fmt=cargo fmt --check "*" inner=--fmt-check" ]]' _ "$(fParseRun --fmt-check)"
fCheck "--no-fmt still wins over it" bash -c '[[ "$1" == *" fmt= dogfood="* ]]' _ "$(fParseRun --no-fmt --fmt-check)"
fCheck "the plan names the run" grep -qF 'if ((fullRun)); then fEcho_Clean "Run .................: ${fullRunName}' "${engine}"
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

## The build number: the commit's time, dirty tree or not, the clock only where
## there is no commit, and a value handed down is kept.
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
## Was "a dirty tree takes the clock" until 2026-10-26; the CICD rules take it from the commit.
fCheck "a dirty tree takes its commit's time too" test "$(fMinutes "${repo}")" = "${commitMinutes}"
mkdir -p "${work}/plain"
fCheck "and so does a folder that is not a repository" fNearNow "$(fMinutes "${work}/plain")"
fCheck "a value handed down is kept" test "$(cd "${repo}" && SILK_BUILD_MINUTES=12345 && fPinBuildMinutes && echo "${SILK_BUILD_MINUTES}")" = 12345

## Only a clean main is marked a release build, which is what drops the date
## from the build number.
eval "$(fLift '^fMarkRelease(){' '^}')"
fRelease(){ (cd "${1}" && unset SILK_RELEASE && fMarkRelease && echo "${SILK_RELEASE}"); }
fCheck "a dirty main is not a release build" test "$(fRelease "${repo}")" = 0
git -C "${repo}" commit -qam second
fCheck "a clean main is" test "$(fRelease "${repo}")" = 1
git -C "${repo}" checkout -qb side
fCheck "a clean side branch is not" test "$(fRelease "${repo}")" = 0
git -C "${repo}" checkout -q main
fCheck "nor is a folder that is not a repository" test "$(fRelease "${work}/plain")" = 0
fCheck "a value handed down is kept" test "$(cd "${repo}" && SILK_RELEASE=0 && fMarkRelease && echo "${SILK_RELEASE}")" = 0

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

## --no-remote leaves out every other box, and a box that wasn't there is a SKIP
## in its step's line and again at the end.
fLiftF(){  ## fLiftF <first line, exactly>: from that line to the first top-level fi
	local n; n="$(grep -m1 -nxF -- "${1}" "${engine}" | cut -d: -f1)"
	[[ -n "${n}" ]] && sed -n "${n},\$ { p; /^fi\$/q }" "${engine}"
}
fParseRemote(){ fParse "${@}" >/dev/null; eval "${parse}"; echo "noremote=${noRemote} inner=${innerArgs[*]}"; }
fCheck "a run with no options reaches the other boxes" test "$(fParseRemote)" = "noremote=0 inner="
fCheck "--no-remote leaves them out, and isn't handed to the container" test "$(fParseRemote --no-remote)" = "noremote=1 inner="
fCheck "--help names it" bash -c '"$1" --help | grep -q -- "--no-remote "' _ "${engine}"
wingPlan="$(fLiftF 'if [[ -z "${WINGUI_HARNESS+x}" ]] || ((${#WINGUI_HARNESS[@]} == 0)); then')"
fCheck "the plan has a Windows scenarios line" test -n "${wingPlan}"
fWingPlan(){ local quick="${1}" noRemote="${2}" container=0 SILK_CICD_IN_CONTAINER=""; WINGUI_HARNESS=(run.bash a b c); fEcho_Clean(){ echo "${*}"; }; eval "${wingPlan}"; }
fCheck "which counts the scenarios" test "$(fWingPlan 0 0)" = "Windows scenarios ...: 3 on the Windows boxes, each on the first one free"
fCheck "and says --no-remote left them out" test "$(fWingPlan 0 1)" = "Windows scenarios ...: (skipped --no-remote)"
fCheck "or --quick" test "$(fWingPlan 1 1)" = "Windows scenarios ...: (skipped --quick)"
privPlan="$(fLiftF 'if ((quick)); then')"
fCheck "the private runner's plan line says --no-remote left it out" grep -qF '"Private runner ......: (skipped --no-remote)"' <<<"${privPlan}"
eval "$(grep '^fSkipCount(){' "${engine}")"
eval "$(fLift '^fStepDone(){' '^}')"
eval "$(fLift '^fSkipSummary(){' '^}')"
eval "$(fLift '^fWinGuiScenarios(){' '^}')"
listing=0
fEcho_Clean(){ echo "${*}"; }
root="${work}/rootw"; mkdir -p "${root}/cicd/tests/wingui" "${work}/priv/cicd"
## A harness and a private runner that each skip what STUB_SKIPS names and note
## that they ran.
cat >"${root}/cicd/tests/wingui/run.bash" <<'STUB'
#!/usr/bin/env bash
echo ran >>"${STUB_RAN}"
for s in ${STUB_SKIPS:-}; do echo "SKIP wingui: ${s}, no box answered"; [[ -z "${SILK_CICD_SKIPS:-}" ]] || echo "SKIP wingui: ${s}, no box answered" >>"${SILK_CICD_SKIPS}"; done
exit "${STUB_EXIT:-0}"
STUB
cp "${root}/cicd/tests/wingui/run.bash" "${work}/priv/cicd/run.bash"
sed -i 's/SKIP wingui: /SKIP private: /g' "${work}/priv/cicd/run.bash"
chmod +x "${root}/cicd/tests/wingui/run.bash" "${work}/priv/cicd/run.bash"
WINGUI_HARNESS=(cicd/tests/wingui/run.bash a b); export STUB_RAN="${work}/ran"
fWing(){  ## fWing <noRemote> <skips>: sets out; skipLog is a fresh list
	local quick=0 noRemote="${1}" SILK_CICD_IN_CONTAINER=""
	skipLog="${work}/skips.txt"; : >"${skipLog}"; : >"${STUB_RAN}"
	out="$(STUB_SKIPS="${2}" fWinGuiScenarios 2>&1)" || true
}
fWing 0 ""
fCheck "with every box there, the scenarios' line is OK" test "$(tail -1 <<<"${out}")" = "    OK: windows gui scenarios"
fWing 0 "b"
fCheck "with one left out, it's a SKIP saying how many" test "$(tail -1 <<<"${out}")" = "    SKIP: windows gui scenarios, 1 not run (SKIP lines above)"
fCheck "and the harness's own line is on the run's list" test "$(cat "${skipLog}")" = "SKIP wingui: b, no box answered"
fWing 1 "b"
fCheck "--no-remote leaves the scenarios out, and says so" test "${out}" = "windows gui scenarios skipped (--no-remote)"
fCheck "without starting the harness" test ! -s "${STUB_RAN}"
fCheck "nor putting anything on the list" test ! -s "${skipLog}"
privStep="$(fLiftF '## The private runner builds on boxes that are often off or busy, so it skips')"
fCheck "the private runner's step can be lifted" test -n "${privStep}"
## Whether the step is on is the plan's call, checked with the plan below.
fPriv(){  ## fPriv <noRemote> <skips>: sets out
	local quick=0 noRemote="${1}" PRIVATE_RUNNER="${work}/priv/cicd/run.bash" ver=0 EXE_NAME=x builtArts=()
	fStepOpen(){ ((! noRemote)); }; fStepClose(){ :; }
	: >"${STUB_RAN}"
	out="$(STUB_SKIPS="${2}" eval "${privStep}" 2>&1)" || true
	unset -f fStepOpen fStepClose
}
skipLog="${work}/skips.txt"; : >"${skipLog}"
fPriv 0 ""
fCheck "with both boxes there, the private runner's line is OK" test "$(tail -1 <<<"${out}")" = "    OK: private runner"
fPriv 0 "mac store"
fCheck "a job it skipped makes it a SKIP" test "$(tail -1 <<<"${out}")" = "    SKIP: private runner, 2 not run (SKIP lines above)"
fPriv 1 "mac"
fCheck "--no-remote leaves it out" test -z "${out}"
fCheck "without starting it" test ! -s "${STUB_RAN}"
out="$(fSkipSummary)"
fCheck "the summary at the end lists every SKIP line, in order" \
	test "${out}" = $'Not run, since a box wasn\'t there:\nSKIP private: mac, no box answered\nSKIP private: store, no box answered'
skipLog="${work}/skips.txt"; : >"${skipLog}"
fCheck "and is left out when nothing was skipped" test -z "$(fSkipSummary)"
skipLog=""
fCheck "or when the run has no list" test -z "$(fSkipSummary)" -a "$(fSkipCount)" = 0
## Changed 20261101: read before the end removes the run's folder, said after
## its cleanup line (2026110112424508).
fCheck "and the run says it last, after the cleanup line" \
	bash -c 'grep -B1 -A1 -x "fRunEnd 0" "$1" | paste -sd"|" | grep -qF "skipsSaid=\"\$(fSkipSummary)\"|fRunEnd 0|[[ -z \"\${skipsSaid}\" ]]"' _ "${engine}"

## The steps: which a run takes and counts, which it turns off and why, and the
## lines each one opens and closes with.
for f in fStepIf fStepOff_Why fPlanSteps fStepNumbers fStepAt fStepOpen fStepClose fPlanSteps_Show; do
	eval "$(fLift "^${f}(){" '^}')"
done
fCheck "the step helpers can be lifted" declare -F fPlanSteps fStepOpen fStepClose fPlanSteps_Show
fPlan(){  ## fPlan [var=value]...: the plan's steps for the defaults, changed by those
	local sync=1 container=0 quick=0 noRemote=0 noWindows=0 fullRun=0 fullRunName="" demoAsked=0 BUILD_CROSS=1 PROFILE_ENABLE=1 PACKAGE_ENABLE=1 DEMO_ENABLE=1
	local PRIVATE_RUNNER="${work}/priv/cicd/run.bash" SILK_CICD_IN_CONTAINER="" root="${work}/rootp"
	local -a FMT_CMD=(cargo fmt) DOGFOOD_DESTS=(x) GIT_PUBLISH=(x) allCrossTargets=("lw|a" "la|linux-arm64") CROSS_TARGETS=("lw|a" "la|linux-arm64")
	mkdir -p "${root}/cicd/utility/demo-video"; : >"${root}/cicd/utility/demo-video/demo-video.py"
	local kv; for kv in "${@}"; do eval "${kv}"; done
	fPlanSteps
	printf 'on:%s\n' "${stepsOn[@]}"; printf 'off:%s\n' "${stepsOff[@]}"; echo "base=${stepBase} total=${_stepTotal}"
}
plan="$(fPlan)"
fCheck "a default run takes every step, in order" test "$(grep '^on:' <<<"${plan}" | paste -sd, -)" = "on:Remote sync,on:Format,on:Debug build,on:Regression tests,on:Profiler,on:Release build: native,on:Release build: lw,on:Release build: la,on:Release artifacts,on:Packages,on:Private runner,on:Dogfood,on:Demo video,on:Backup + publish"
fCheck "and counts them all" grep -qx 'base=0 total=14' <<<"${plan}"
plan="$(fPlan quick=1 PROFILE_ENABLE=0 PACKAGE_ENABLE=0 'CROSS_TARGETS=("lw|a")')"
fCheck "--quick turns steps off and says so" bash -c 'for w in "Profiler|--quick" "Packages|--quick" "Private runner|--quick" "Release build: la|--quick" "Demo video|--quick; --demo adds it"; do grep -qxF "off:${w}" <<<"$1" || exit 1; done' _ "${plan}"
fCheck "and leaves them out of the count" grep -qx 'base=0 total=9' <<<"${plan}"
plan="$(fPlan fullRun=1 fullRunName=--merge-gate sync=0 DOGFOOD_DESTS=\(\) GIT_PUBLISH=\(\) DEMO_ENABLE=0 noRemote=1)"
fCheck "a full run names itself as what turned a step off" bash -c 'for w in "Remote sync" Dogfood "Demo video" "Backup + publish"; do grep -qxF "off:${w}|--merge-gate" <<<"$1" || exit 1; done' _ "${plan}"
fCheck "and --no-remote the private runner" grep -qxF 'off:Private runner|--no-remote' <<<"${plan}"
plan="$(fPlan container=1)"
fCheck "with --container, the image is a step and the card's steps follow the container's" test "$(grep '^on:' <<<"${plan}" | sed -n '2p;3p;11p;12p' | paste -sd, -)" = "on:Container image,on:Format,on:Checks on this box,on:Profiler"
plan="$(export SILK_CICD_STEP_AT=2 SILK_CICD_STEP_TOTAL=16; fPlan SILK_CICD_IN_CONTAINER=1; fStepAt Format; fStepAt Packages; fStepAt Dogfood || echo none)"
fCheck "inside, only its part, numbered on from where the run outside put it" \
	test "$(grep -v '^on:\|^off:' <<<"${plan}" | paste -sd, -)" = "base=2 total=16,3,10,none"
plan="$(export SILK_CICD_STEP_AT='x[$(touch '"${work}"'/pwned)]' SILK_CICD_STEP_TOTAL=16; fPlan SILK_CICD_IN_CONTAINER=1)"
fCheck "a number handed in that isn't one is ignored, and runs nothing" bash -c '[[ "$1" == *"base=0 total=8"* && ! -e "$2/pwned" ]]' _ "${plan}" "${work}"
out="$(stepsOn=(A B); stepsOff=("C|--no-c"); listing=1; fPlanSteps_Show)"
fCheck "the plan counts the steps and lists the ones off with why" test "${out}" = $'Steps ...............: 2, and these off:\n    - C (--no-c)\n  Step 1 of 2  A\n  Step 2 of 2  B'
## The lines themselves, from the shared helpers.
fSteps(){ (
	# shellcheck source=cicd/utility/include/echo.bash
	source "${cicd}/utility/include/echo.bash"
	stepsOn=(Format "Debug build"); stepBase=0; _stepTotal=2; skipLog="${work}/stepskips.txt"; : >"${skipLog}"
	"${@}"
) 2>&1; }
stamp='[0-9]{8}-[0-9]{6}\.[0-9]{2}'
out="$(fSteps eval 'fStepOpen "Debug build"; echo built; fStepClose; fStepOpen Nope || echo off')"
fCheck "a step opens after a blank and a 79-wide rule" bash -c '[[ "$(sed -n 2p <<<"$1")" == "$2" && -z "$(sed -n 1p <<<"$1")" && ${#2} -eq 79 ]]' _ "${out}" "$(printf '•%.0s' {1..79})"
fCheck "with its stamp, number and total" bash -c '[[ "$(sed -n 3p <<<"$1")" =~ ^\[\ $2:\ Step\ 2\ of\ 2\;\ Starting\ Debug\ build\ \]$ ]]' _ "${out}" "${stamp}"
fCheck "and closes with PASS, then a blank" bash -c '[[ "$(sed -n 5p <<<"$1")" =~ ^\[\ $2:\ Step\ 2\ of\ 2\;\ PASS\ Debug\ build\ \]$ && -z "$(sed -n 6p <<<"$1")" ]]' _ "${out}" "${stamp}"
fCheck "a step that's off prints nothing" test "$(tail -1 <<<"${out}")" = "off"
out="$(fSteps eval 'fStepOpen Format; echo "SKIP x, no box" >>"${skipLog}"; fStepClose')"
fCheck "one that left something out for want of a box closes with SKIP" bash -c '[[ "$(tail -1 <<<"$1")" =~ ^\[\ $2:\ Step\ 1\ of\ 2\;\ SKIP\ Format\ \(1\ not\ run,\ listed\ at\ the\ end\)\ \]$ ]]' _ "${out}" "${stamp}"
rc=0; out="$(fSteps eval 'fStepOpen Format; fDie "it broke"; echo after')" || rc=$?
fCheck "fDie in a step says why, then closes it with FAIL" bash -c '[[ "$3" == 1 && "$(grep -v "^$" <<<"$1" | tail -2 | head -1)" == "[ FAILED: it broke ]" && "$(grep -v "^$" <<<"$1" | tail -1)" =~ ^\[\ $2:\ Step\ 1\ of\ 2\;\ FAIL\ Format\ \]$ && "$1" != *after* ]]' _ "${out}" "${stamp}" "${rc}"
eval "$(fLift '^fRunEnd(){' '^}')"
out="$(fSteps eval 'fContainerEnd(){ :; }; fTestDir_End(){ :; }; fRunDir_End(){ :; }; fSweep_Summary(){ :; }; keepTemp=0; runEnded=0; fStepOpen Format; fRunEnd 2')"
fCheck "a step a plain error ended closes with FAIL too" bash -c '[[ "$(grep -v "^$" <<<"$1" | tail -1)" =~ ^\[\ $2:\ Step\ 1\ of\ 2\;\ FAIL\ Format\ \]$ ]]' _ "${out}" "${stamp}"
out="$(fSteps eval 'fContainerEnd(){ :; }; fTestDir_End(){ :; }; fRunDir_End(){ :; }; fSweep_Summary(){ :; }; keepTemp=0; runEnded=0; fRunEnd 0')"
fCheck "and a run that passed adds no line" test -z "${out}"

## The ARM64 smoke run: its own line, SKIP when its box wasn't there.
eval "$(fLift '^fArmSmoke(){' '^}')"
here="${work}/armhere"; mkdir -p "${here}/utility"
cat >"${here}/utility/arm-smoke.bash" <<'STUB'
#!/usr/bin/env bash
echo "ran ${2}" >>"${STUB_RAN}"
if [[ -n "${STUB_SKIP:-}" ]]; then echo "SKIP arm64 smoke: box unreachable" >>"${SILK_CICD_SKIPS}"; exit 3; fi
exit "${STUB_EXIT:-0}"
STUB
printf '#!/usr/bin/env bash\necho "SilkTerm v9.9.9 build abcde (2026-10-28)"\necho "Copyright"\n' >"${work}/native"
chmod +x "${here}/utility/arm-smoke.bash" "${work}/native"
fArm(){  ## fArm <noRemote> <arts>: sets out; skipLog is a fresh list
	local quick=0 listing=0 noRemote="${1}" SILK_CICD_IN_CONTAINER="" RELEASE_NATIVE_BIN="${work}/native"
	local -a builtArts=(); read -ra builtArts <<<"${2}"
	skipLog="${work}/skips.txt"; : >"${skipLog}"; : >"${STUB_RAN}"
	out="$(fArmSmoke 2>&1)" || true
}
fArm 0 "linux-x86_64|a linux-arm64|b"
fCheck "the ARM64 build gets a smoke run" test "$(cat "${STUB_RAN}")" = "ran SilkTerm v9.9.9 build abcde"
fCheck "and its line is OK" test "$(tail -1 <<<"${out}")" = "    OK: arm64 smoke run"
STUB_SKIP=1 fArm 0 "linux-arm64|b"
fCheck "with its box not there, the line is a SKIP" test "$(tail -1 <<<"${out}")" = "    SKIP: arm64 smoke run, 1 not run (SKIP lines above)"
STUB_EXIT=1 fArm 0 "linux-arm64|b"
fCheck "a smoke run that failed stops the run" test "$(tail -1 <<<"${out}")" = "DIE: the Linux ARM64 build's --version failed on the arm64 box"
fArm 1 "linux-arm64|b"
fCheck "--no-remote leaves it out, and says so" test "${out}" = "arm64 smoke run skipped (--no-remote)"
fCheck "without starting it" test ! -s "${STUB_RAN}"
fArm 0 "linux-x86_64|a"
fCheck "with no ARM64 build there's nothing to run" test -z "${out}" -a ! -s "${STUB_RAN}"
armPlan="$(n="$(grep -m1 -nF '"ARM64 smoke run .....: (no Linux ARM64 build)"' "${engine}" | cut -d: -f1)"; sed -n "$((n - 1)),\$ { p; /^fi\$/q }" "${engine}")"
fArmPlan(){ local quick=0 noRemote="${1}" container=0 BUILD_CROSS=1 SILK_CICD_IN_CONTAINER=""; local -a CROSS_TARGETS=(); read -ra CROSS_TARGETS <<<"${2}"; eval "${armPlan}"; }
fCheck "the plan has an ARM64 smoke run line" test "$(fArmPlan 0 "x|linux-arm64|y")" = "ARM64 smoke run .....: --version on the arm64 box, woken and put back if it's a VM"
fCheck "which says --no-remote left it out" test "$(fArmPlan 1 "x|linux-arm64|y")" = "ARM64 smoke run .....: (skipped --no-remote)"
fCheck "or that there's no ARM64 build" test "$(fArmPlan 0 "x|linux-x86_64|y")" = "ARM64 smoke run .....: (no Linux ARM64 build)"

## The release stage's check for this box's paths. Every published binary once
## held the home folder and account name, from panic locations.
eval "$(grep '^fHasLocalPaths(){' "${engine}")"
fPaths(){ local HOME="${work}/home/someone" root="${work}/src/silkterm"; fHasLocalPaths "${1}"; }
printf 'x\0panicked at %s/src/silkterm/source/src/main.rs\0' "${work}" >"${work}/bin-root"
printf 'x\0%s/home/someone/.cargo/registry/src/a.rs\0' "${work}" >"${work}/bin-home"
printf 'x\0/silkterm/source/src/main.rs\0/cargo/registry/src/a.rs\0' >"${work}/bin-clean"
fCheck "a binary naming the checkout holds a local path" fPaths "${work}/bin-root"
fCheck "and so does one naming the home folder" fPaths "${work}/bin-home"
fCheck "one with the paths mapped away does not" fNot fPaths "${work}/bin-clean"
fCheck "the release stage checks every built binary" grep -qF 'if fHasLocalPaths "${pair#*|}"; then fDie ' "${engine}"

## The fuzz corpus is hashed either side of the tests. A run once rewrote a seed
## in place, and nothing said so.
eval "$(grep '^fCorpusHashes(){' "${engine}")"
mkdir -p "${work}/corpus-root/cicd/tests/fuzz-corpus/config"
printf 'window:\n\tcolumns: 100\n' >"${work}/corpus-root/cicd/tests/fuzz-corpus/config/seed.shcl"
corpusBefore="$(root="${work}/corpus-root" fCorpusHashes)"
fCheck "an untouched corpus hashes the same twice" test "$(root="${work}/corpus-root" fCorpusHashes)" = "${corpusBefore}"
printf '## a banner a launch refreshed\n' >>"${work}/corpus-root/cicd/tests/fuzz-corpus/config/seed.shcl"
fCheck "a seed written through hashes differently" test "$(root="${work}/corpus-root" fCorpusHashes)" != "${corpusBefore}"
fCheck "and the test stage fails on a difference" grep -qF '[[ "$(fCorpusHashes)" != "${corpusBefore}" ]]; then' "${engine}"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261102: Miri's config check with an absolute target dir too.
##		- 20261102: Miri: when it runs, where, its plan, first and --list lines, the
##		            step, and its config.
##		- 20261101: The soak test: when it runs, where, its plan and --list lines,
##		            and the step.
##		- 20261101: The heap-growth test under --quick, and --leakcheck adding CodeQL.
##		- 20261028: The release stage's check for local paths in a binary, and the
##		            fuzz corpus hashes either side of the tests.
##		- 20261028: The steps a run takes, the ones it turns off and why, and each
##		            step's opening and closing lines.
##		- 20261028: --long and --slow, with TSan and CodeQL waiting for them.
##		- 20261028: The ARM64 smoke run's step and plan line.
##		- 20261026: --backlog-run-quick, --backlog-run-full, --merge-gate and their other names.
##		- 20261025: --no-remote, and the SKIP lines in a step's line and at the end.
##		- 20261022: Where CodeQL runs, and --codeql, off by default.
##		- 20261022: --no-leakcheck, and the leak checks on with no options.
##		- 20260926 JC: Created.
