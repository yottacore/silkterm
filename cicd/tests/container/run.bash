#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted code reads them, and shellcheck cannot see into an eval.
#  shellcheck disable=2154  ## 'referenced but not assigned.' The lifted code assigns them, inside an eval.

##	- Purpose:
##		--container. The image tag follows the recipe and the pins the recipe takes,
##		the recipe names only pinned versions and checks every download, the stage
##		options reach the run inside while the host options stay out, the binaries
##		that run made are picked up here, and a stand-in docker sees the engine
##		start the container the right way and stop when it fails.
##	- Test ID: EsJg9EF
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
engine="${root}/cicd/cicd.bash"
recipe="${root}/cicd/container/Dockerfile"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fLift(){ sed -n "/${1}/,/${2}/p" "${engine}"; }
fNot(){ ! "${@}"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-container.XXXXXX")"
fEnd(){ local -r rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

fEcho(){ echo "    ${*}"; }
fEcho_Clean(){ echo "    ${*}"; }
fDie(){ echo "DIE: ${*}"; exit 1; }

## The tag: the recipe and the pins the recipe takes, nothing else.
eval "$(fLift '^fContainerArgs(){' '^}')"
eval "$(fLift '^fContainerImage(){' '^}')"
stand="${work}/stand"; mkdir -p "${stand}"
printf '[toolchain]\nchannel = "1.96.0"\ntargets = [\n\t"x86_64-pc-windows-gnu",\n\t"aarch64-unknown-linux-gnu",\n]\n' >"${stand}/rust-toolchain.toml"
printf 'FROM x\nARG VER_rust\nARG RUST_TARGETS\nARG VER_cargo_deny\nARG VER_zig\n' >"${stand}/Dockerfile"
EXE_NAME=silkterm
TOOL_PINS=("cargo-deny|0.19.9|cargo deny --version" "zig|0.16.0|zig version" "makensis|3.12|makensis -VERSION")
fWith(){ (root="${stand}"; containerRecipe="${stand}/Dockerfile"; "${@}"); }
args="$(fWith fContainerArgs)"
fCheck "the toolchain is a build arg" grep -qx 'VER_rust=1.96.0' <<<"${args}"
fCheck "and so are its targets, on one line" grep -qx 'RUST_TARGETS=x86_64-pc-windows-gnu aarch64-unknown-linux-gnu' <<<"${args}"
fCheck "a pin the recipe takes is one, dashes as underscores" grep -qx 'VER_cargo_deny=0.19.9' <<<"${args}"
fCheck "a pin it does not take is not" fNot grep -q makensis <<<"${args}"
tag1="$(fWith fContainerImage)"
fCheck "the image is named for the program" bash -c '[[ "$1" == silkterm-cicd:???????????? ]]' _ "${tag1}"
TOOL_PINS[1]="zig|0.17.0|zig version"
fCheck "a pin the recipe takes changes the tag" test "$(fWith fContainerImage)" != "${tag1}"
TOOL_PINS[1]="zig|0.16.0|zig version"; TOOL_PINS[2]="makensis|3.13|makensis -VERSION"
fCheck "one it does not take leaves it" test "$(fWith fContainerImage)" = "${tag1}"
TOOL_PINS[2]="makensis|3.12|makensis -VERSION"
echo '# a comment' >>"${stand}/Dockerfile"
fCheck "an edit to the recipe changes the tag" test "$(fWith fContainerImage)" != "${tag1}"
SANITIZER_TOOLCHAIN=nightly-2026-01-01
fCheck "the sanitizer nightly is a build arg" grep -qx 'RUST_NIGHTLY=nightly-2026-01-01' <<<"$(fWith fContainerArgs)"
tagNightly="$(fWith fContainerImage)"
SANITIZER_TOOLCHAIN=nightly-2026-01-02
fCheck "and a nightly bump changes the tag" test "$(fWith fContainerImage)" != "${tagNightly}"
unset SANITIZER_TOOLCHAIN

## The real recipe: every version it takes is pinned, every download is checked.
fRecipeArgsPinned(){
	local arg pinned bad=0
	pinned="$(awk -F'|' 'NF == 4 && $1 !~ /^#/ { gsub(/-/, "_", $1); print $1 }' "${root}/cicd/tool-pins.txt")"
	while read -r arg; do
		[[ "${arg}" == VER_rust ]] && continue
		grep -qx "${arg#VER_}" <<<"${pinned}" || { echo "    ${arg} names no pin in tool-pins.txt"; bad=1; }
	done < <(grep -oE '^ARG VER_[A-Za-z_]+' "${recipe}" | cut -d' ' -f2)
	return "${bad}"
}
fRecipeArgsHanded(){
	local arg real bad=0
	real="$(source "${root}/cicd/config.bash" >/dev/null 2>&1; containerRecipe="${recipe}"; fContainerArgs)"
	while read -r arg; do
		grep -q "^${arg}=." <<<"${real}" || { echo "    the engine hands the recipe no ${arg}"; bad=1; }
	done < <(grep -oE '^ARG (VER_[A-Za-z_]+|RUST_TARGETS|RUST_NIGHTLY)' "${recipe}" | cut -d' ' -f2)
	return "${bad}"
}
fDownloadsChecked(){
	local file bad=0
	while read -r file; do
		grep -F "${file}" "${recipe}" | grep -q 'sha256sum -c' || { echo "    ${file} is downloaded and never checked"; bad=1; }
	done < <(grep -oE 'curl -fsSLo [A-Za-z0-9._-]+' "${recipe}" | awk '{ print $3 }')
	return "${bad}"
}
fCheck "every version the recipe takes is a pin" fRecipeArgsPinned
fCheck "and the engine hands it each one, with the toolchain, its targets and the nightly" fRecipeArgsHanded
fCheck "the nightly is a dated one, from rust-toolchain.toml" bash -c 'source "$1" && [[ "${SANITIZER_TOOLCHAIN}" =~ ^nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]' _ "${root}/cicd/config.bash"
fCheck "and the recipe installs it with the std source" grep -qE 'rustup toolchain install "\$\{RUST_NIGHTLY\}" .*-c rust-src' "${recipe}"
fCheck "every download in the recipe is checked" fDownloadsChecked
fCheck "the base image is pinned by digest" grep -qE '^FROM debian:[a-z-]+@sha256:[0-9a-f]{64} AS base$' "${recipe}"
fCheck "the image marks itself, so a run inside cannot start another" grep -qE '^[[:space:]]*SILK_CICD_IN_CONTAINER=1' "${recipe}"

## The options: the stage options go in with the run, the host options stay here.
parse="$(fLift '^assumeYes=0; ' '^if ((noContainer)); then container=0; fi$')"
fParse(){  ## fParse <args...>: prints what the parser leaves for the run inside
	BUILD_CROSS=1; PROFILE_ENABLE=1; PACKAGE_ENABLE=1; FUZZ_SECS=60
	FMT_CMD=(cargo fmt); DOGFOOD_DESTS=(x); GIT_PUBLISH=(x); DEMO_ENABLE=0; PRIVATE_RUNNER=x
	useContainer="${USE_CONTAINER:-0}"
	set -- "${@}"
	eval "${parse}"
	echo "container=${container} inner=${innerArgs[*]:-}"
}
fCheck "--container is an option, and the stage options go in with it" \
	test "$(fParse --container --quick --no-arm --no-fuzz -y --no-sync --no-dogfood --no-publish --no-private -m msg --demo --no-demo)" = "container=1 inner=--quick --no-arm --no-fuzz"
fCheck "the host options stay out" test "$(fParse -y --no-sync --no-dogfood --no-publish --no-private --demo -m x)" = "container=0 inner="
fCheck "and so does --container itself" test "$(fParse --container)" = "container=1 inner="
fCheck "--no-asan is a stage option" test "$(fParse --container --no-asan)" = "container=1 inner=--no-asan"
fCheck "--no-tsan is too" test "$(fParse --container --no-tsan)" = "container=1 inner=--no-tsan"
fCheck "with docker here it is the default" test "$(USE_CONTAINER=1 fParse -y)" = "container=1 inner="
fCheck "--no-container turns it off" test "$(USE_CONTAINER=1 fParse --no-container -y)" = "container=0 inner="
fCheck "and wins over --container, wherever it sits" test "$(USE_CONTAINER=1 fParse --no-container --container)" = "container=0 inner="

## What the run inside made: stage 5's list, from the target dir the two share.
eval "$(fLift '^fContainerArtifacts(){' '^}')"
tdir="${work}/target"; mkdir -p "${tdir}/release" "${tdir}/x86_64-pc-windows-gnu/release" "${tdir}/aarch64-unknown-linux-gnu/release"
RELEASE_NATIVE_BIN="${tdir}/release/silkterm"; RELEASE_NATIVE_OSARCH=linux-x86_64
CROSS_TARGETS=(
	"Windows x86_64 (mingw)|windows-x86_64|${tdir}/x86_64-pc-windows-gnu/release/silkterm.exe|cargo build"
	"Linux ARM64 (zig)|linux-arm64|${tdir}/aarch64-unknown-linux-gnu/release/silkterm|cargo zigbuild"
)
BUILD_CROSS=1; VERSION_MANIFEST=Cargo.toml; RELEASE_ARTIFACT_DIR=cicd/artifacts/release
printf 'version = "1.2.3-beta4"\n' >"${stand}/Cargo.toml"
fArts(){ (root="${stand}"; fContainerArtifacts && printf '%s\n' "${builtArts[@]}" && echo "ver=${ver}" && echo "artDir=${artDir}"); }
rc=0; out="$(fArts 2>&1)" || rc=$?
fCheck "with no native binary the run here stops" test "${rc}" -ne 0
fCheck "and says so" grep -q '^DIE: the container run left no native release binary' <<<"${out}"
touch "${RELEASE_NATIVE_BIN}" "${tdir}/x86_64-pc-windows-gnu/release/silkterm.exe"
rc=0; out="$(fArts 2>&1)" || rc=$?
fCheck "a cross binary missing stops it too, by name" grep -q '^DIE: the container run left no Linux ARM64 (zig) binary' <<<"${out}"
touch "${tdir}/aarch64-unknown-linux-gnu/release/silkterm"
out="$(fArts)"
fCheck "with every binary there the list is stage 5's" test "$(head -3 <<<"${out}" | paste -sd' ')" = \
	"linux-x86_64|${tdir}/release/silkterm windows-x86_64|${tdir}/x86_64-pc-windows-gnu/release/silkterm.exe linux-arm64|${tdir}/aarch64-unknown-linux-gnu/release/silkterm"
fCheck "with the version from the manifest" grep -qx 'ver=1.2.3-beta4' <<<"${out}"
fCheck "and the artifact folder under the root" grep -qx "artDir=${stand}/cicd/artifacts/release" <<<"${out}"
BUILD_CROSS=0
fCheck "--no-cross wants only the native one" test "$(fArts | head -1)" = "linux-x86_64|${tdir}/release/silkterm"

## The engine against a stand-in docker, from a copy of the pipeline in its own
## repository, so nothing here touches this tree's logs.
repo="${work}/repo"; mkdir -p "${repo}"
tar -C "${root}" --exclude=cicd/artifacts -cf - cicd rust-toolchain.toml source/Cargo.toml | tar -xf - -C "${repo}"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
git init -q -b main "${repo}"
git -C "${repo}" config user.name t; git -C "${repo}" config user.email t@t
git -C "${repo}" add -A; git -C "${repo}" commit -qm first
fake="${work}/bin"; mkdir -p "${fake}"
cat >"${fake}/docker" <<'EOF'
#!/usr/bin/env bash
## A stand-in: records every call on one line, says whether the image is there, and
## on 'run' makes the binaries a --quick run builds, where the engine inside would
## have: the native one and the Windows x86_64 one. It lists the tags, untagged
## images, volumes, the run's containers and stopped ones it is given, what the
## tidy container reports, and sizes that shrink once asked twice.
printf '%s\n' "${*//$'\n'/ }" >>"${FAKE_LOG}"
case "${1}" in
	image)
		case "${2}" in
			inspect) exit "${FAKE_IMAGE_MISSING:-0}" ;;
			ls) if [[ " ${*} " == *" -q "* ]]; then printf '%s\n' ${FAKE_UNTAGGED:-}; else printf '%s\n' ${FAKE_TAGS:-}; fi ;;
		esac
		exit 0 ;;
	volume)
		case "${2}" in
			inspect) exit "${FAKE_VOLUME_MISSING:-0}" ;;
			ls) printf '%s\n' ${FAKE_VOLUMES:-} ;;
			rm) exit "${FAKE_VOLUME_RM_RC:-0}" ;;
		esac
		exit 0 ;;
	ps)
		if [[ "${*}" == *"label=dev.run="* && "${*}" == *"{{.Names}}"* ]]; then printf '%s\n' ${FAKE_RUN_NAMES:-}
		elif [[ "${*}" == *"label=dev.run="* ]]; then printf '%s\n' ${FAKE_RUN_IDS:-}
		else printf '%s\n' ${FAKE_STOPPED:-}; fi
		exit 0 ;;
	builder) echo "Total reclaimed space: ${FAKE_PRUNED:-0B}"; exit 0 ;;
	system)
		if [[ -e "${FAKE_LOG}.df" ]]; then printf 'Images|900MB\nBuild Cache|2GB\n'; else : >"${FAKE_LOG}.df"; printf 'Images|1GB\nBuild Cache|7.5GB\n'; fi
		exit 0 ;;
	run)
		if [[ "${*}" == *" --name silkterm-"*"-tidy "* ]]; then printf '%s\n' ${FAKE_TIDY:-}; exit 0; fi
		sleep "${FAKE_RUN_SLEEP:-0}"
		target=""; for a in "${@}"; do [[ "${a}" == CARGO_TARGET_DIR=* ]] && target="${a#*=}"; done
		mkdir -p "${target}/release" "${target}/x86_64-pc-windows-gnu/release"
		touch "${target}/release/silkterm" "${target}/x86_64-pc-windows-gnu/release/silkterm.exe"
		exit "${FAKE_RUN_RC:-0}" ;;
esac
exit 0
EOF
chmod +x "${fake}/docker"
export SILK_DOCKER="${fake}/docker"
fRun(){  ## fRun <log name> <options...>: the engine from the copy; sets rc and out, and the stand-in's log is ${work}/<log name>
	local -r log="${work}/${1}"; shift
	rm -f "${log}" "${log}.df"; export FAKE_LOG="${log}"
	## Unset: inside the container both are set, and this is a run from outside.
	rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR bash cicd/cicd.bash "${@}" 2>&1)" || rc=$?
}
fRun full.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick --no-fuzz
fCheck "a run with --container passes with the stand-in" test "${rc}" -eq 0
runLine="$(grep '^run ' "${work}/full.log" || true)"
fCheck "it started one container" test "$(grep -c '^run ' "${work}/full.log")" -eq 1
fCheck "dropping to this user once it has a passwd entry" bash -c '[[ "$1" == *"/etc/passwd"* && "$1" == *"exec setpriv --reuid="* && "$1" == *" _ $(id -u) $(id -g) "* ]]' _ "${runLine}"
fCheck "with the tree at its own path" bash -c '[[ "$1" == *" -v $2:$2 "* && "$1" == *" -w $2 "* ]]' _ "${runLine}" "${repo}"
fCheck "the cache volume" bash -c '[[ "$1" == *" -v silkterm-cicd-cache:/cache "* ]]' _ "${runLine}"
## Commented out 20261019: /tmp is now a new folder under target/container/tmp each
## run, so a killed run's locks and sockets are never the next one's. Checked below.
# fCheck "its /tmp under the target dir" bash -c '[[ "$1" == *" -v $2/target/container/tmp:/tmp "* ]]' _ "${runLine}" "${repo}"
fCheck "its /tmp a new folder under the target dir" bash -c '[[ "$1" =~ \ -v\ $2/target/container/tmp/run_[0-9]{8}-[0-9]{8}:/tmp\  ]]' _ "${runLine}" "${repo}"
fCheck "which goes once the run passes" test -z "$(find "${repo}/target/container/tmp" -mindepth 1 -maxdepth 1 2>/dev/null)"
fCheck "and is written down, for a run that does not get that far" grep -qE "^${repo}/target/container/tmp/run_[0-9]{8}-[0-9]{8}"$'\t'"[0-9]+-[0-9]+$" "${repo}/target/container/.cicd-container-tmp"
fCheck "the container is named for the run and labeled as the pipeline's" bash -c '[[ "$1" =~ \ --name\ silkterm-([0-9]{8}-[0-9-]+)-engine\ --label\ dev\.project=silkterm\ --label\ dev\.run=([0-9-]+)\  && "${BASH_REMATCH[1]}" == "${BASH_REMATCH[2]}" ]]' _ "${runLine}"
fCheck "with --rm" bash -c '[[ "$1" == "run --rm --init "* ]]' _ "${runLine}"
fCheck "zig's cache on a volume of its own" bash -c '[[ "$1" == *" -v silkterm-zig-cache:/zig-cache "* && "$1" == *" -e ZIG_GLOBAL_CACHE_DIR=/zig-cache "* ]]' _ "${runLine}"
fCheck "stamped with the zig that filled it" bash -c '[[ "$1" == *"zig version >\"/zig-cache/\${3}\""* && "$1" == *" _ $(id -u) $(id -g) .cicd-zig-version "* ]]' _ "${runLine}"
fCheck "the target dir for both sides" bash -c '[[ "$1" == *" CARGO_TARGET_DIR=$2/target/container "* ]]' _ "${runLine}" "${repo}"
fCheck "the build number pinned here" bash -c '[[ "$1" == *" SILK_BUILD_MINUTES="[0-9]* ]]' _ "${runLine}"
fCheck "the engine inside gets the stage options and the host stages off" \
	bash -c '[[ "$1" == *" $2/cicd/cicd.bash -y --no-sync --no-dogfood --no-publish --no-private --no-demo --quick --no-fuzz" ]]' _ "${runLine}" "${repo}"
## The demo is on by default, so a run inside left to itself records it there too.
fCheck "the demo among them, though nothing here asked for it off" bash -c '[[ "$1" == *"/cicd/cicd.bash "*" --no-demo"* ]]' _ "${runLine}"
fCheck "and nothing else of this box" test "$(grep -o ' -v ' <<<"${runLine}" | wc -l)" -eq 4
## Names stamped inside, such as test folders and flamegraphs, sort with the ones
## made here.
(export TZ=Pacific/Chatham; fRun tz.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick --no-fuzz)
fCheck "this box's time zone goes in" grep -q -- ' -e TZ=Pacific/Chatham ' "${work}/tz.log"
fCheck "the image was there, so nothing was built" fNot grep -q '^build ' "${work}/full.log"
fCheck "the run here went on to the end" grep -q 'CI/CD: done' <<<"${out}"
fCheck "with the container's stages marked done" grep -q 'OK: stages 1-6 in the container' <<<"${out}"
fCheck "and the plan named the image" grep -q '^Container .*: stages 1-6 in silkterm-cicd:' <<<"${out}"
## The steps that need the card run here once the container is done: the render
## node test, then the profiler, which --quick turns off.
fAfter(){ awk -v a="${1}" -v b="${2}" 'index($0, a) && !x {x = NR} index($0, b) && x && !y {y = NR} END {exit !(x && y)}' <<<"${out}"; }
fCheck "the render node test runs here after the container" fAfter 'OK: stages 1-6 in the container' 'software rendering without shared pixmaps'
fCheck "and the profiler after it" fAfter 'software rendering without shared pixmaps' '4/8  Profiler (here)'
fCheck "off under --quick" grep -q 'profiler disabled' <<<"${out}"
## Inside, both only say where they run, and the profiler builds nothing.
eval "$(fLift '^fRenderNodeTest(){' '^}')"
eval "$(fLift '^fRunProfiler(){' '^}')"
inside="$(fRunTest_MaySkip(){ echo "ran ${1}"; }; fRetryBuild(){ echo "built ${*}"; }
	export SILK_CICD_IN_CONTAINER=1; PROFILE_ENABLE=1; fRunProfiler; fRenderNodeTest 2>&1)" || true
fCheck "inside the container the profiler says it runs outside" grep -qx '    the profiler runs outside the container, on the card' <<<"${inside}"
fCheck "and builds nothing" fNot grep -q '^built ' <<<"${inside}"
fCheck "the render node test the same, and does not run" bash -c 'grep -qx "    the render node test runs outside the container" <<<"$1" && ! grep -q "^ran " <<<"$1"' _ "${inside}"
inside="$(fRunTest_MaySkip(){ echo "ran ${1}"; }; unset SILK_CICD_IN_CONTAINER; fRenderNodeTest)" || true
fCheck "outside it runs" grep -qx 'ran cicd/tests/swnoshm/run.bash' <<<"${inside}"

FAKE_IMAGE_MISSING=1 fRun build.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
buildLine="$(grep '^build ' "${work}/build.log" || true)"
fCheck "with no image, one is built first" test -n "${buildLine}"
fCheck "from the recipe folder" bash -c '[[ "$1" == *" $2/cicd/container" ]]' _ "${buildLine}" "${repo}"
fCheck "tagged as the plan named it" bash -c '[[ "$1" == "build -t silkterm-cicd:"* ]]' _ "${buildLine}"
fCheck "with the toolchain and its targets" bash -c '[[ "$1" == *" --build-arg VER_rust=1.96.0 "* && "$1" == *" --build-arg RUST_TARGETS=x86_64-pc-windows-gnu aarch64-unknown-linux-gnu aarch64-pc-windows-gnullvm x86_64-apple-darwin "* ]]' _ "${buildLine}"
fCheck "the pins" bash -c '[[ "$1" == *" --build-arg VER_zig=0.16.0 "* && "$1" == *" --build-arg VER_cargo_deny="* ]]' _ "${buildLine}"
fCheck "and the job cap" bash -c '[[ "$1" == *" --build-arg JOBS="[0-9]* ]]' _ "${buildLine}"
fCheck "labeled as the pipeline's" bash -c '[[ "$1" =~ \ --label\ dev\.project=silkterm\ --label\ dev\.run=[0-9]{8}-[0-9-]+\  ]]' _ "${buildLine}"
FAKE_VOLUME_MISSING=1 fRun vol.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "a volume not there yet is made with the labels" bash -c 'grep -qE "^volume create --label dev\.project=silkterm --label dev\.run=[0-9-]+ silkterm-zig-cache$" "$1" && grep -qE "^volume create .* silkterm-cicd-cache$" "$1"' _ "${work}/vol.log"
fCheck "before the container starts" bash -c 'awk "/^volume create/ {v = NR} /^run --rm --init/ {r = NR} END {exit !(v && r && v < r)}" "$1"' _ "${work}/vol.log"

## What earlier runs and builds left in docker.
## Commented out 20261025: under the CICD reset (2026102416233805) the run ends with
## docker builder prune, older tags go only once a newer one passed and the one
## before stays, and the pipeline's label is dev.project. Checked below.
# FAKE_TAGS="silkterm-cicd:000000000000" FAKE_UNTAGGED="sha256:feedface" FAKE_STOPPED="c0ffee" FAKE_CACHE="7.5GB" \
# 	FAKE_IMAGE_MISSING=1 fRun tidy.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
# fCheck "after a build the older tag goes" grep -qx 'image rm silkterm-cicd:000000000000' "${work}/tidy.log"
# fCheck "and so does an untagged image the pipeline labeled" bash -c 'grep -qx "image ls -q --filter dangling=true --filter label=silkterm-cicd=image" "$1" && grep -qx "image rm sha256:feedface" "$1"' _ "${work}/tidy.log"
# fCheck "a stopped container the pipeline labeled goes" bash -c 'grep -qx "ps -aq --filter label=silkterm-cicd=run --filter status=exited --filter status=created --filter status=dead" "$1" && grep -qx "rm c0ffee" "$1"' _ "${work}/tidy.log"
# fCheck "the build cache is not pruned" fNot grep -qE '^(builder|buildx|system) prune' "${work}/tidy.log"
# fCheck "only reported" grep -qF 'docker build cache: 7.5GB reclaimable, shared with other projects, not pruned here' <<<"${out}"
# fCheck "and the run says what went" grep -qE '^cleanup: removed 1 stopped containers, 2 old container images; ' <<<"${out}"
# FAKE_TAGS="silkterm-cicd:000000000000" fRun tags.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
# fCheck "with the image there, the older tag still goes" grep -qx 'image rm silkterm-cicd:000000000000' "${work}/tags.log"
fRun gate.log --gate --container
fCheck "--gate --container runs the backlog quick run inside, every area named" bash -c '[[ "$(grep "^run " "$1")" == *" $2/cicd/cicd.bash --backlog-run-quick fmt clippy tests scripts docs" ]]' _ "${work}/gate.log" "${repo}"
fCheck "and says --gate is another name for it" grep -qx -- '--gate is another name for --backlog-run-quick' <<<"${out}"
fCheck "and nothing else" test "${rc}" -eq 0 -a "$(grep -c '^run ' "${work}/gate.log")" -eq 1
fCheck "with no log tee or plan" fNot grep -q 'Fail-fast' <<<"${out}"
fCheck "the gate's container is named for the step" grep -qE '^run --rm --init --shm-size 1g --cpus [0-9]+ --cpu-shares 256 --name silkterm-[0-9]{8}-[0-9-]+-gate ' "${work}/gate.log"
fCheck "and the gate ends with the same cleanup" grep -q '^builder prune -f --filter until=168h' "${work}/gate.log"
fRun quickareas.log --backlog-run-quick docs fmt --container
fCheck "--backlog-run-quick takes the areas named into the container, in order" bash -c '[[ "$(grep "^run " "$1")" == *" $2/cicd/cicd.bash --backlog-run-quick docs fmt" ]]' _ "${work}/quickareas.log" "${repo}"

FAKE_RUN_RC=7 fRun fail.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "a run inside that fails stops the run here" test "${rc}" -ne 0
fCheck "and says where" grep -q 'the run in the container failed' <<<"${out}"
failTmp="$(sed -n 's/^the container.s \/tmp is kept in //p' <<<"${out}")"
fCheck "keeping its /tmp, and saying where" test -n "${failTmp}" -a -f "${failTmp}/.test_silkterm_owner"

## Later runs keep the newest few kept /tmp folders, five by the config, and
## remove the rest. Six written down after the failed one, and one nobody wrote
## down.
# shellcheck source=cicd/utility/include/sweep.bash
source "${root}/cicd/utility/include/sweep.bash"
declare -a planted=()
for _ in 1 2 3 4 5 6; do
	fSweep_MakeDir "${repo}/target/container/tmp" run made token
	fSweep_Record "${repo}/target/container/.cicd-container-tmp" "${made}" "${token}"
	planted+=("${made}")
done
mkdir "${repo}/target/container/tmp/run_nobody"
fRun sweep.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "a later run removes the failed run's /tmp past the newest five" test -n "${failTmp}" -a ! -e "${failTmp}"
fCheck "and the oldest planted one" test ! -e "${planted[0]}"
fCheck "and keeps the newest five" bash -c 'for d in "${@}"; do [[ -d "${d}" ]] || exit 1; done' _ "${planted[@]:1}"
fCheck "and one nobody wrote down" test -d "${repo}/target/container/tmp/run_nobody"
fCheck "and says what went" grep -qE '^cleanup: removed 2 old container /tmp folders' <<<"${out}"
fCheck "with none of the run's containers left, says nothing of them" fNot grep -q "of the run's containers" <<<"${out}"

## At the start a stopped container of ours goes. At the end, failed or not, the
## run's containers go by its label, then untagged images of ours, then older tags
## once the run passed, keeping the one before, then the volume rule, then build
## cache over a week old, and a line comparing docker's sizes.
oldTags="2026-10-03T07:00|silkterm-cicd:000000000003 2026-10-01T07:00|silkterm-cicd:000000000001 2026-10-02T07:00|silkterm-cicd:000000000002"
FAKE_TAGS="${oldTags}" FAKE_UNTAGGED="sha256:feedface" FAKE_STOPPED="c0ffee" FAKE_RUN_IDS="beef1 beef2" FAKE_PRUNED="1.2GB" \
	FAKE_IMAGE_MISSING=1 fRun tidy.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "a stopped container the pipeline labeled goes" bash -c 'grep -qx "ps -aq --filter label=dev.project=silkterm --filter status=exited --filter status=created --filter status=dead" "$1" && grep -qx "rm c0ffee" "$1"' _ "${work}/tidy.log"
fCheck "the run's containers are stopped and removed by its label" bash -c 'grep -qE "^ps -aq --filter label=dev\.run=[0-9]{8}-[0-9-]+ --filter label=dev\.project=silkterm$" "$1" && grep -qx "stop -t 10 beef1 beef2" "$1" && grep -qx "rm beef1 beef2" "$1"' _ "${work}/tidy.log"
fCheck "and the run says so" grep -qF 'cleanup: stopped and removed 2 of the run' <<<"${out}"
fCheck "an untagged image the pipeline labeled goes" bash -c 'grep -qx "image ls -q --filter dangling=true --filter label=dev.project=silkterm" "$1" && grep -qx "image rm sha256:feedface" "$1"' _ "${work}/tidy.log"
fCheck "the older tags are only the pipeline's" grep -qE '^image ls silkterm-cicd --filter label=dev\.project=silkterm --format' "${work}/tidy.log"
fCheck "the newest one before this run's stays" fNot grep -qx 'image rm silkterm-cicd:000000000003' "${work}/tidy.log"
fCheck "and older ones go" bash -c 'grep -qx "image rm silkterm-cicd:000000000002" "$1" && grep -qx "image rm silkterm-cicd:000000000001" "$1"' _ "${work}/tidy.log"
fCheck "the build cache over a week old is pruned" grep -qx 'builder prune -f --filter until=168h' "${work}/tidy.log"
fCheck "after the run's containers went" bash -c 'awk "/^rm beef1/ {r = NR} /^builder prune/ {p = NR} END {exit !(r && p && r < p)}" "$1"' _ "${work}/tidy.log"
fCheck "saying what it freed" grep -qF 'docker build cache over a week old pruned, 1.2GB freed' <<<"${out}"
fCheck "and docker's sizes before and after" grep -qF 'docker: Images 1GB -> 900MB, Build Cache 7.5GB -> 2GB' <<<"${out}"
fCheck "nothing is removed by force, nor the whole system pruned" fNot grep -qE '^((rm|rmi|image rm|container rm|volume rm) .*(^| )(-f|--force)( |$)|system prune)' "${work}/tidy.log"
FAKE_TAGS="${oldTags}" FAKE_RUN_RC=7 fRun tagfail.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "a run that failed removes no tag" fNot grep -q '^image rm silkterm-cicd:' "${work}/tagfail.log"
fCheck "but still prunes and removes its containers" bash -c 'grep -q "^builder prune" "$1" && grep -qE "^ps -aq --filter label=dev\.run=" "$1"' _ "${work}/tagfail.log"

## The cache volumes: only the pipeline's, one over the cap goes whole, and so does
## the Zig one another Zig filled. docker keeps one still in use.
FAKE_VOLUMES="silkterm-cicd-cache silkterm-zig-cache silkterm-other" FAKE_TIDY="silkterm-cicd-cache|2147483649||0.16.0 silkterm-zig-cache|1000|0.15.1|0.16.0" \
	fRun volgo.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "the volumes are only the pipeline's" grep -qx 'volume ls -q --filter label=dev.project=silkterm' "${work}/volgo.log"
fCheck "read by a labeled container of the run's, read only" bash -c 'grep -qE "^run --label dev\.project=silkterm --label dev\.run=[0-9-]+ --name silkterm-[0-9-]+-tidy -v silkterm-cicd-cache:/v/silkterm-cicd-cache:ro -v silkterm-zig-cache:/v/silkterm-zig-cache:ro silkterm-cicd:" "$1"' _ "${work}/volgo.log"
fCheck "and removed before any volume is" bash -c 'awk "/^rm silkterm-[0-9-]+-tidy$/ {t = NR} /^volume rm/ && !v {v = NR} END {exit !(t && v && t < v)}" "$1"' _ "${work}/volgo.log"
fCheck "one over 2 GB goes" grep -qx 'volume rm silkterm-cicd-cache' "${work}/volgo.log"
fCheck "the zig one another zig filled goes" grep -qx 'volume rm silkterm-zig-cache' "${work}/volgo.log"
fCheck "and the run says why" bash -c 'grep -qF "removed the volume silkterm-cicd-cache, over 2048 MB" <<<"$1" && grep -qF "removed the volume silkterm-zig-cache, filled by zig 0.15.1" <<<"$1"' _ "${out}"
fCheck "a volume that is not one of the caches is left" fNot grep -q 'silkterm-other' "$(grep -v '^volume ls' "${work}/volgo.log" >"${work}/volgo.rest"; echo "${work}/volgo.rest")"
FAKE_VOLUMES="silkterm-cicd-cache silkterm-zig-cache" FAKE_TIDY="silkterm-cicd-cache|2147483648||0.16.0 silkterm-zig-cache|1000|0.16.0|0.16.0" \
	fRun volstay.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "at the cap, and stamped by this zig, both stay" fNot grep -q '^volume rm' "${work}/volstay.log"
FAKE_VOLUMES="silkterm-cicd-cache" FAKE_TIDY="silkterm-cicd-cache|9999999999||0.16.0" FAKE_VOLUME_RM_RC=1 \
	fRun volheld.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "one still in use is left, and the run says so" grep -qF 'left the volume silkterm-cicd-cache (over 2048 MB), still in use' <<<"${out}"

## --keep-temp keeps the run's containers, without --rm, and names them.
FAKE_RUN_IDS="beef1" FAKE_RUN_NAMES="silkterm-x-engine" FAKE_VOLUMES="silkterm-cicd-cache" FAKE_TIDY="silkterm-cicd-cache|9999999999||0.16.0" \
	fRun keep.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick --keep-temp
fCheck "--keep-temp starts the container without --rm" bash -c '[[ "$(grep "^run .*-engine " "$1")" == "run --init "* ]]' _ "${work}/keep.log"
fCheck "and stops nothing at the end" fNot grep -qE '^(stop|rm) beef1' "${work}/keep.log"
fCheck "nor touches the volumes the kept ones hold" fNot grep -qE '^volume rm' "${work}/keep.log"
fCheck "and names them" grep -qF "kept the run's containers (--keep-temp): silkterm-x-engine" <<<"${out}"
fCheck "--keep-temp stays out of the run inside" fNot grep -qE '^run .*cicd\.bash .*--keep-temp' "${work}/keep.log"
keepTmp="$(sed -n "s/^kept the container's \/tmp (--keep-temp): //p" <<<"${out}")"
fCheck "it keeps the container's /tmp too, though the run passed, and says where" test -n "${keepTmp}" -a -d "${keepTmp}"
fCheck "and the run inside keeps its test folder in it" grep -qE '^run .* -e SILKTERM_TEST_DIR=/tmp/test_silkterm_engine ' "${work}/keep.log"
fCheck "and this run says where its own is" grep -qF "kept the run's temp folder (--keep-temp): ${SILKTERM_TEST_DIR}" <<<"${out}"

## --list prints what the other options would run, and runs none of it.
fRun list.log --merge-gate --list --container
fCheck "--list with --merge-gate prints the plan" bash -c '[[ "$1" == 0 && "$2" == *"Run .................: --merge-gate"* ]]' _ "${rc}" "${out}"
fCheck "and the test IDs" bash -c '[[ "$1" == *"Rust tests (cargo test, "* && "$1" == *"  Er2UgY7  pipeline steps"* && "$1" == *"  EsJg9EF  container run"* ]]' _ "${out}"
fCheck "and starts and builds nothing" fNot grep -sqE '^(run|build) ' "${work}/list.log"
fCheck "nor goes past the plan" bash -c '[[ "$1" != *"0/8  Remote sync"* && "$1" != *"CI/CD: done"* ]]' _ "${out}"
fRun listquick.log --list --quick --container
fCheck "--quick leaves out what it skips" bash -c '[[ "$1" == *"  EqA7Wki  git hooks"* && "$1" != *"project rename"* ]]' _ "${out}"
fRun listgate.log --backlog-run-quick docs fmt --list --container
fCheck "--list with --backlog-run-quick prints its steps in order" bash -c '[[ "$1" == *"  Quick 1/2  Docs checks"*"  Quick 2/2  Format check"* ]]' _ "${out}"
fCheck "and the docs checks' IDs" bash -c '[[ "$1" == *"  EqRbSy0  tables of contents"* && "$1" == *"  Er2UgYD  markdown spacing"* ]]' _ "${out}"
fCheck "and starts nothing" bash -c '[[ "$1" == 0 ]] && ! grep -sqE "^(run|build) " "$2"' _ "${rc}" "${work}/listgate.log"

## Ctrl+C mid-run, to the whole process group as a terminal sends it, and the end
## still runs. A background job starts with SIGINT ignored, so it is put back.
rm -f "${work}/int.log" "${work}/int.log.df"
( cd "${repo}" && exec env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR FAKE_LOG="${work}/int.log" FAKE_RUN_IDS="beef9" FAKE_RUN_SLEEP=30 \
	setsid python3 -c 'import os, signal, sys; signal.signal(signal.SIGINT, signal.SIG_DFL); os.execvp(sys.argv[1], sys.argv[1:])' \
	bash cicd/cicd.bash --gate --container >"${work}/int.out" 2>&1 ) &
intPid=$!
for _ in $(seq 1 100); do grep -q '^run --rm' "${work}/int.log" 2>/dev/null && break; sleep 0.1; done
sleep 0.5; kill -INT -- "-${intPid}" 2>/dev/null || true; wait "${intPid}" 2>/dev/null || true
fCheck "on Ctrl+C the run's containers still go" bash -c 'grep -qx "stop -t 10 beef9" "$1" && grep -q "^builder prune" "$1"' _ "${work}/int.log"
## The same in a full run, whose output goes through the log tee, which Ctrl+C
## takes down too, so the end must not write before it is done.
rm -f "${work}/intfull.log" "${work}/intfull.log.df"
( cd "${repo}" && exec env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR FAKE_LOG="${work}/intfull.log" FAKE_RUN_IDS="beef8" FAKE_RUN_SLEEP=30 \
	FAKE_VOLUMES="silkterm-cicd-cache" FAKE_TIDY="silkterm-cicd-cache|9999999999||0.16.0" \
	setsid python3 -c 'import os, signal, sys; signal.signal(signal.SIGINT, signal.SIG_DFL); os.execvp(sys.argv[1], sys.argv[1:])' \
	bash cicd/cicd.bash --container -y --no-sync --no-dogfood --no-publish --no-private --quick >"${work}/intfull.out" 2>&1 ) &
intPid=$!
for _ in $(seq 1 200); do grep -q '^run --rm' "${work}/intfull.log" 2>/dev/null && break; sleep 0.1; done
sleep 0.5; kill -INT -- "-${intPid}" 2>/dev/null || true; wait "${intPid}" 2>/dev/null || true
fCheck "and in a full run, through the log tee, all of the end runs" \
	bash -c 'grep -qx "stop -t 10 beef8" "$1" && grep -qx "volume rm silkterm-cicd-cache" "$1" && grep -q "^builder prune" "$1" && grep -q "^system df" <(tail -1 "$1")' _ "${work}/intfull.log"

rc=0; out="$(cd "${repo}" && SILK_CICD_IN_CONTAINER=1 FAKE_LOG="${work}/nested.log" bash cicd/cicd.bash --container -y 2>&1)" || rc=$?
fCheck "inside the container, --container is refused" test "${rc}" -ne 0
fCheck "by name" grep -q 'already in the container' <<<"${out}"
fCheck "before docker is asked anything" test ! -e "${work}/nested.log"

rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER SILK_DOCKER="${work}/no-such-docker" bash cicd/cicd.bash --container -y 2>&1)" || rc=$?
fCheck "with no docker the run stops at once" test "${rc}" -ne 0
fCheck "and says what it needs" grep -q -- '--container needs docker' <<<"${out}"

## The run inside skips what has to run here, and the run here takes it up.
fCheck "the Windows scenarios step aside inside the container" grep -q 'windows gui scenarios run outside the container' "${engine}"
fCheck "and the run here calls them after the container" bash -c 'sed -n "/^if ((container)); then$/,/^fi$/p" "$1" | grep -q "^[[:space:]]fWinGuiScenarios$"' _ "${engine}"
fCheck "the content scrub too" bash -c 'sed -n "/^if ((container)); then$/,/^fi$/p" "$1" | grep -q "^[[:space:]]fContentScrub$"' _ "${engine}"
fCheck "the run inside writes no second log" grep -q 'LINT_LOG_DIR:-}" && -z "${SILK_CICD_IN_CONTAINER:-}"' "${engine}"

## The ASan run: in the image from a run on this box, as is inside the container.
# shellcheck source=cicd/utility/include/jobs.bash
source "${root}/cicd/utility/include/jobs.bash"
eval "$(fLift '^fRunAsan(){' '^}')"
asanStand="${work}/asan"; mkdir -p "${asanStand}/utility"
printf '#!/bin/sh\necho "asan ran here"\nexit "${FAKE_RC:-0}"\n' >"${asanStand}/utility/asan.bash"; chmod +x "${asanStand}/utility/asan.bash"
fContainerBuild(){ echo "build ${1}"; }
fContainerRun(){ echo "run ${*}"; return "${FAKE_RC:-0}"; }
fAsan(){ (ASAN_CMD=(cargo test); quick=0; asanSkipped=""; containerImage=img:1; root=/r; here="${asanStand}"; unset SILK_CICD_IN_CONTAINER FAKE_RC; "${@}"; fRunAsan) 2>&1; }
fNothing(){ :; }
fReport(){ export FAKE_RC=1; }
fInside(){ export SILK_CICD_IN_CONTAINER=1; }
out="$(fAsan fNothing)"
fCheck "a run here does the ASan step in the image" bash -c '[[ "$1" == *"build img:1"* && "$1" == *"run img:1 /r/cicd/utility/asan.bash"* ]]' _ "${out}"
rc=0; out="$(fAsan fReport)" || rc=$?
fCheck "a report there stops the run" test "${rc}" -ne 0
fCheck "and says so" grep -q '^DIE: the test suite under ASan and LSan failed' <<<"${out}"
out="$(fAsan fInside)"
fCheck "inside the container it runs the script as is" bash -c '[[ "$1" == *"asan ran here"* && "$1" != *"run img"* ]]' _ "${out}"
fQuick(){ quick=1; }
fCheck "--quick skips it" grep -q 'ASan run skipped (--quick)' <<<"$(fAsan fQuick)"
fNoCmd(){ ASAN_CMD=(); }
fCheck "--no-asan does" grep -q 'ASan run skipped$' <<<"$(fAsan fNoCmd)"
fNoDocker(){ asanSkipped="no docker"; }
fCheck "and with no docker it warns" grep -qF 'WARNING: ASan run skipped: no docker' <<<"$(fAsan fNoDocker)"
fAsanJobs(){ (__jobsCeiling=""; export CICD_CORES=8 CICD_MAX_JOBS=4; fContainerRun(){ echo "jobs=${CICD_MAX_JOBS}"; }; fAsan fNothing); }
fCheck "the ASan step goes in with a quarter of the CPUs" grep -qx 'jobs=2' <<<"$(fAsanJobs)"

## The TSan run goes the same way, under --long.
eval "$(fLift '^fRunTsan(){' '^}')"
printf '#!/bin/sh\necho "tsan ran here"\nexit "${FAKE_RC:-0}"\n' >"${asanStand}/utility/tsan.bash"; chmod +x "${asanStand}/utility/tsan.bash"
fTsan(){ (TSAN_CMD=(cargo test); quick=0; long=1; tsanSkipped=""; containerImage=img:1; root=/r; here="${asanStand}"; unset SILK_CICD_IN_CONTAINER FAKE_RC; "${@}"; fRunTsan) 2>&1; }
out="$(fTsan fNothing)"
fCheck "a run here does the TSan step in the image" bash -c '[[ "$1" == *"build img:1"* && "$1" == *"run img:1 /r/cicd/utility/tsan.bash"* ]]' _ "${out}"
rc=0; out="$(fTsan fReport)" || rc=$?
fCheck "a TSan report there stops the run" test "${rc}" -ne 0
fCheck "and says so" grep -q '^DIE: the test suite under TSan failed' <<<"${out}"
fCheck "inside the container it runs tsan.bash as is" bash -c '[[ "$1" == *"tsan ran here"* && "$1" != *"run img"* ]]' _ "$(fTsan fInside)"
fCheck "--quick skips TSan" grep -q 'TSan run skipped (--quick)' <<<"$(fTsan fQuick)"
fNoLong(){ long=0; }
fCheck "without --long it's off" bash -c '[[ "$1" == *"TSan run off (--long adds it)"* && "$1" != *"run img"* ]]' _ "$(fTsan fNoLong)"
fNoTsan(){ TSAN_CMD=(); }
fCheck "--no-tsan does" grep -q 'TSan run skipped$' <<<"$(fTsan fNoTsan)"
fNoDockerT(){ tsanSkipped="no docker"; }
fCheck "and with no docker it warns" grep -qF 'WARNING: TSan run skipped: no docker' <<<"$(fTsan fNoDockerT)"
fCheck "the TSan run follows the leak checks in stage 3" \
	bash -c 'grep -A1 "^[[:space:]]*fRunLeakcheck$" "$1" | grep -q "^[[:space:]]*fRunTsan$"' _ "${engine}"

## Before anything is built: a full run plans that step in the image, or skipped
## with no docker.
fPlan(){ rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR -u CICD_LINUX_HALF "${@}" bash cicd/cicd.bash --no-sync </dev/null 2>&1)" || rc=$?; }
fPlan
fCheck "a full run plans the ASan step in the image" grep -qE '^ASan \+ LSan \.+: the tests again on nightly-[0-9-]+, in silkterm-cicd:[0-9a-f]{12}$' <<<"${out}"
fCheck "and TSan off, for --long" grep -qE '^TSan \.+: \(off; --long adds it\)$' <<<"${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR -u CICD_LINUX_HALF bash cicd/cicd.bash --no-sync --long </dev/null 2>&1)" || rc=$?
fCheck "--long plans the TSan step in the image" grep -qE '^TSan \.+: the tests again on nightly-[0-9-]+, in silkterm-cicd:[0-9a-f]{12}$' <<<"${out}"
fCheck "and plans the container as the default" grep -qE '^Container \.+: stages 1-6 in silkterm-cicd:[0-9a-f]{12}' <<<"${out}"
fPlan SILK_DOCKER="${work}/no-such-docker"
fCheck "with no docker the run says it builds here" grep -qF 'no docker here, so stages 1-6 run on this box' <<<"${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR -u CICD_LINUX_HALF bash cicd/cicd.bash --no-sync --no-container </dev/null 2>&1)" || rc=$?
fCheck "--no-container plans no container, and says nothing about docker" bash -c '[[ "$1" != *"Container ."* && "$1" != *"no docker here"* ]]' _ "${out}"
fPlan SILK_DOCKER="${work}/no-such-docker"
fCheck "with no docker a full run plans it skipped and goes on" bash -c '[[ "$1" != *"DIE:"* && "$1" == *"Fail-fast"* ]] && grep -qE "^ASan \+ LSan \.+: \(skipped, no docker" <<<"$1"' _ "${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR -u CICD_LINUX_HALF SILK_DOCKER="${work}/no-such-docker" bash cicd/cicd.bash --no-sync --long </dev/null 2>&1)" || rc=$?
fCheck "and TSan too, under --long" grep -qE '^TSan \.+: \(skipped, no docker' <<<"${out}"
fPlan SILK_DOCKER="${work}/no-such-docker" CICD_LINUX_HALF=1
fCheck "and so does the Linux half of a Windows run" grep -qE '^ASan \+ LSan \.+: \(skipped, no docker' <<<"${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR SILK_DOCKER="${work}/no-such-docker" bash cicd/cicd.bash --no-sync --no-asan </dev/null 2>&1)" || rc=$?
fCheck "--no-asan needs no docker" bash -c '[[ "$1" != *"needs docker"* ]] && grep -qE "^ASan \+ LSan \.+: \(skipped\)$" <<<"$1"' _ "${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR SILK_DOCKER="${work}/no-such-docker" bash cicd/cicd.bash --no-sync --no-tsan </dev/null 2>&1)" || rc=$?
fCheck "--no-tsan plans it skipped" grep -qE '^TSan \.+: \(skipped\)$' <<<"${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR -u CICD_LINUX_HALF bash cicd/cicd.bash --no-sync --no-container --no-asan --long </dev/null 2>&1)" || rc=$?
fCheck "and a run here with only TSan still takes it into the image" grep -qE '^TSan \.+: the tests again on nightly-[0-9-]+, in silkterm-cicd:[0-9a-f]{12}$' <<<"${out}"
rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR SILK_DOCKER="${work}/no-such-docker" bash cicd/cicd.bash --no-sync --quick </dev/null 2>&1)" || rc=$?
fCheck "and nor does --quick" bash -c '[[ "$1" != *"needs docker"* ]] && grep -qE "^ASan \+ LSan \.+: \(skipped --quick\)$" <<<"$1"' _ "${out}"
fCheck "which skips TSan too" grep -qE '^TSan \.+: \(skipped --quick\)$' <<<"${out}"

## A target dir linked to another disk would dangle inside, so it goes in at its
## real path.
linked="${work}/linked"; mkdir -p "${linked}" "${work}/real-target"
tar -C "${repo}" --exclude=./target -cf - . | tar -xf - -C "${linked}"
ln -s "${work}/real-target" "${linked}/target"
realTarget="$(cd -P "${work}/real-target" && pwd -P)"
export FAKE_LOG="${work}/linked.log"
rc=0; out="$(cd "${linked}" && env -u SILK_CICD_IN_CONTAINER -u CARGO_TARGET_DIR bash cicd/cicd.bash --container -y --no-sync --no-dogfood --no-publish --no-private --quick --no-fuzz 2>&1)" || rc=$?
runLine="$(grep '^run ' "${work}/linked.log" || true)"
fCheck "a linked target dir: the run passes" test "${rc}" -eq 0
fCheck "with the target dir mounted at its real path" bash -c '[[ "$1" == *" -v $2/container:$2/container "* ]]' _ "${runLine}" "${realTarget}"
fCheck "and named by it" bash -c '[[ "$1" == *" CARGO_TARGET_DIR=$2/container "* ]]' _ "${runLine}" "${realTarget}"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261010 JC: Created.
##		- 20261013 JC: The time zone goes in.
##		- 20261019 JC: A new /tmp per run, and what earlier runs left in docker.
##		- 20261019 JC: The sanitizer nightly, the ASan step, and a linked target dir.
##		- 20261023 JC: The run inside gets --no-demo.
##		- 20261023 JC: The TSan step.
##		- 20261025 JC: The render node test and the profiler run here, after the container.
##		- 20261026: --gate is the backlog quick run, and the areas go in with it.
##		- 20261026: --keep-temp keeps the /tmp folders, and --list starts nothing.
##		- 20261028: TSan only under --long.
