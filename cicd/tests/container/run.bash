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
	done < <(grep -oE '^ARG (VER_[A-Za-z_]+|RUST_TARGETS)' "${recipe}" | cut -d' ' -f2)
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
fCheck "and the engine hands it each one, with the toolchain and its targets" fRecipeArgsHanded
fCheck "every download in the recipe is checked" fDownloadsChecked
fCheck "the base image is pinned by digest" grep -qE '^FROM debian:[a-z-]+@sha256:[0-9a-f]{64} AS base$' "${recipe}"
fCheck "the image marks itself, so a run inside cannot start another" grep -qE '^[[:space:]]*SILK_CICD_IN_CONTAINER=1' "${recipe}"

## The options: the stage options go in with the run, the host options stay here.
parse="$(fLift '^assumeYes=0; ' '^esac; done$')"
fParse(){  ## fParse <args...>: prints what the parser leaves for the run inside
	BUILD_CROSS=1; PROFILE_ENABLE=1; PACKAGE_ENABLE=1; FUZZ_SECS=60
	FMT_CMD=(cargo fmt); DOGFOOD_DESTS=(x); GIT_PUBLISH=(x); DEMO_ENABLE=0; PRIVATE_RUNNER=x
	set -- "${@}"
	eval "${parse}"
	echo "container=${container} inner=${innerArgs[*]:-}"
}
fCheck "--container is an option, and the stage options go in with it" \
	test "$(fParse --container --quick --no-arm --no-fuzz -y --no-sync --no-dogfood --no-publish --no-private -m msg --demo)" = "container=1 inner=--quick --no-arm --no-fuzz"
fCheck "the host options stay out" test "$(fParse -y --no-sync --no-dogfood --no-publish --no-private --demo -m x)" = "container=0 inner="
fCheck "and so does --container itself" test "$(fParse --container)" = "container=1 inner="

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
## A stand-in: records every call, says whether the image is there, and on 'run'
## makes the native binary where the engine inside would have.
printf '%s\n' "${*}" >>"${FAKE_LOG}"
case "${1}" in
	image) if [[ "${2}" == inspect ]]; then exit "${FAKE_IMAGE_MISSING:-0}"; fi; exit 0 ;;
	run)
		target=""; for a in "${@}"; do [[ "${a}" == CARGO_TARGET_DIR=* ]] && target="${a#*=}"; done
		mkdir -p "${target}/release" && touch "${target}/release/silkterm"
		exit "${FAKE_RUN_RC:-0}" ;;
esac
exit 0
EOF
chmod +x "${fake}/docker"
export SILK_DOCKER="${fake}/docker"
fRun(){  ## fRun <log name> <options...>: the engine from the copy; sets rc and out, and the stand-in's log is ${work}/<log name>
	local -r log="${work}/${1}"; shift
	rm -f "${log}"; export FAKE_LOG="${log}"
	rc=0; out="$(cd "${repo}" && env -u SILK_CICD_IN_CONTAINER bash cicd/cicd.bash "${@}" 2>&1)" || rc=$?
}
fRun full.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick --no-fuzz
fCheck "a run with --container passes with the stand-in" test "${rc}" -eq 0
runLine="$(grep '^run ' "${work}/full.log" || true)"
fCheck "it started one container" test "$(grep -c '^run ' "${work}/full.log")" -eq 1
fCheck "as this user" bash -c '[[ "$1" == *" --user $(id -u):$(id -g) "* ]]' _ "${runLine}"
fCheck "with the tree at its own path" bash -c '[[ "$1" == *" -v $2:$2 "* && "$1" == *" -w $2 "* ]]' _ "${runLine}" "${repo}"
fCheck "the cache volume" bash -c '[[ "$1" == *" -v silkterm-cicd-cache:/cache "* ]]' _ "${runLine}"
fCheck "its /tmp under the target dir" bash -c '[[ "$1" == *" -v $2/target/container/tmp:/tmp "* ]]' _ "${runLine}" "${repo}"
fCheck "the target dir for both sides" bash -c '[[ "$1" == *" CARGO_TARGET_DIR=$2/target/container "* ]]' _ "${runLine}" "${repo}"
fCheck "the build number pinned here" bash -c '[[ "$1" == *" SILK_BUILD_MINUTES="[0-9]* ]]' _ "${runLine}"
fCheck "the engine inside gets the stage options and the host stages off" \
	bash -c '[[ "$1" == *" $2/cicd/cicd.bash -y --no-sync --no-dogfood --no-publish --no-private --quick --no-fuzz" ]]' _ "${runLine}" "${repo}"
fCheck "the image was there, so nothing was built" fNot grep -q '^build ' "${work}/full.log"
fCheck "the run here went on to the end" grep -q 'CI/CD: done' <<<"${out}"
fCheck "with the container's stages marked done" grep -q 'OK: stages 1-6 in the container' <<<"${out}"
fCheck "and the plan named the image" grep -q '^Container .*: stages 1-6 in silkterm-cicd:' <<<"${out}"

FAKE_IMAGE_MISSING=1 fRun build.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
buildLine="$(grep '^build ' "${work}/build.log" || true)"
fCheck "with no image, one is built first" test -n "${buildLine}"
fCheck "from the recipe folder" bash -c '[[ "$1" == *" $2/cicd/container" ]]' _ "${buildLine}" "${repo}"
fCheck "tagged as the plan named it" bash -c '[[ "$1" == "build -t silkterm-cicd:"* ]]' _ "${buildLine}"
fCheck "with the toolchain and its targets" bash -c '[[ "$1" == *" --build-arg VER_rust=1.96.0 "* && "$1" == *" --build-arg RUST_TARGETS=x86_64-pc-windows-gnu aarch64-unknown-linux-gnu aarch64-pc-windows-gnullvm "* ]]' _ "${buildLine}"
fCheck "the pins" bash -c '[[ "$1" == *" --build-arg VER_zig=0.16.0 "* && "$1" == *" --build-arg VER_cargo_deny="* ]]' _ "${buildLine}"
fCheck "and the job cap" bash -c '[[ "$1" == *" --build-arg JOBS="[0-9]* ]]' _ "${buildLine}"

fRun gate.log --gate --container
fCheck "--gate --container runs the gate inside" bash -c '[[ "$(grep "^run " "$1")" == *" $2/cicd/cicd.bash --gate" ]]' _ "${work}/gate.log" "${repo}"
fCheck "and nothing else" test "${rc}" -eq 0 -a "$(grep -c '^run ' "${work}/gate.log")" -eq 1
fCheck "with no log tee or plan" fNot grep -q 'Fail-fast' <<<"${out}"

FAKE_RUN_RC=7 fRun fail.log --container -y --no-sync --no-dogfood --no-publish --no-private --quick
fCheck "a run inside that fails stops the run here" test "${rc}" -ne 0
fCheck "and says where" grep -q 'the run in the container failed' <<<"${out}"

rc=0; out="$(cd "${repo}" && SILK_CICD_IN_CONTAINER=1 FAKE_LOG="${work}/nested.log" bash cicd/cicd.bash --container -y 2>&1)" || rc=$?
fCheck "inside the container, --container is refused" test "${rc}" -ne 0
fCheck "by name" grep -q 'already in the container' <<<"${out}"
fCheck "before docker is asked anything" test ! -e "${work}/nested.log"

rc=0; out="$(cd "${repo}" && SILK_DOCKER="${work}/no-such-docker" bash cicd/cicd.bash --container -y 2>&1)" || rc=$?
fCheck "with no docker the run stops at once" test "${rc}" -ne 0
fCheck "and says what it needs" grep -q -- '--container needs docker' <<<"${out}"

## The run inside skips what has to run here, and the run here takes it up.
fCheck "the Windows scenarios step aside inside the container" grep -q 'windows gui scenarios run outside the container' "${engine}"
fCheck "and the run here calls them after the container" bash -c 'sed -n "/^if ((container)); then$/,/^fi$/p" "$1" | grep -q "^[[:space:]]fWinGuiScenarios$"' _ "${engine}"
fCheck "the content scrub too" bash -c 'sed -n "/^if ((container)); then$/,/^fi$/p" "$1" | grep -q "^[[:space:]]fContentScrub$"' _ "${engine}"
fCheck "the run inside writes no second log" grep -q 'LINT_LOG_DIR:-}" && -z "${SILK_CICD_IN_CONTAINER:-}"' "${engine}"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261010 JC: Created.
