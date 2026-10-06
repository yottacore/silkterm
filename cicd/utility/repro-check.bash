#!/usr/bin/env bash

##	- Purpose:
##		Build one commit twice, from two clones in different folders, the way
##		cicd.bash builds a release, and compare the binaries. The same commit has
##		to give the same bytes wherever it is built, or a published checksum
##		says nothing about the source it names.
##	- Syntax: repro-check.bash [--target <triple>] [--keep] [commit]
##		The commit defaults to HEAD, and the target to this box's own. A target is
##		built with the command config.bash gives it, zigbuild for the ARM ones.
##		--keep leaves both clones under target/ to look into.
##	- Exit: 0 the two are identical, 1 they differ, 2 usage or a failed build.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/../.." && pwd)"
export PATH="${HOME}/.cargo/bin:${HOME}/.local/bin:${PATH}"

triple=""; keep=0; commit="HEAD"
while (($#)); do case "${1}" in
	--target) [[ $# -ge 2 ]] || { echo "--target needs a triple" >&2; exit 2; }; triple="${2}"; shift 2 ;;
	--keep)   keep=1; shift ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	- History:/p' "${BASH_SOURCE[0]}" | sed '$d; s/^##	\{0,1\}//'; exit 0 ;;
	-*)       echo "unknown option: ${1}" >&2; exit 2 ;;
	*)        commit="${1}"; shift ;;
esac; done
commit="$(git -C "${root}" rev-parse --verify "${commit}^{commit}")" || exit 2

## The build command, from the same table cicd.bash builds releases from.
# shellcheck source=cicd/config.bash
source "${root}/cicd/config.bash"
buildCmd="${RELEASE_NATIVE_CMD[*]}"
if [[ -n "${triple}" ]]; then
	buildCmd=""
	for row in "${CROSS_TARGETS[@]}"; do
		[[ "${row##*|}" == *"--target ${triple}" ]] && buildCmd="${row##*|}"
	done
	[[ -n "${buildCmd}" ]] || { echo "config.bash builds no release for ${triple}" >&2; exit 2; }
fi

## The build number the way cicd.bash pins it for a clean tree: the commit's time.
SILK_BUILD_MINUTES=$(( ($(git -C "${root}" log -1 --format=%ct "${commit}") - 946684800) / 60 ))
export SILK_BUILD_MINUTES

mkdir -p "${root}/target"
work="$(mktemp -d "${root}/target/repro.XXXXXX")"
fCleanup(){ if ((keep)); then echo "kept: ${work}"; else rm -rf "${work}"; fi; }
trap fCleanup EXIT

## Two folders of different depth and length, so a path that leaks in shows as a
## difference rather than cancelling out.
sides=("${work}/a" "${work}/second/checkout-b")
jobs="$(( $(nproc) / 2 ))"; ((jobs > 0)) || jobs=1
export CARGO_BUILD_JOBS="${jobs}"

fBuild(){
	local dir="${1}" try
	git clone --quiet --no-hardlinks "${root}" "${dir}"
	git -C "${dir}" checkout --quiet --detach "${commit}"
	mkdir -p "${dir}/target"
	## The same path map cicd.bash's release stage writes.
	printf "[target.'cfg(all())']\nrustflags = ['--remap-path-prefix=%s=/cargo', '--remap-path-prefix=%s=/silkterm', '--remap-path-prefix=%s=/target']\n" \
		"${CARGO_HOME:-${HOME}/.cargo}" "${dir}" "${dir}/target" > "${dir}/target/remap-paths.toml"
	## A fat-LTO rustc crash here is a known transient, so one more try.
	for try in 1 2; do
		if (cd "${dir}" && CARGO_TARGET_DIR="${dir}/target" eval "${buildCmd} --config $(printf '%q' "${dir}/target/remap-paths.toml")"); then return 0; fi
		echo "build failed in ${dir} (try ${try})" >&2
	done
	return 1
}

bins=()
for dir in "${sides[@]}"; do
	echo "building ${commit:0:9} in ${dir}"
	fBuild "${dir}" || exit 2
	bin="${dir}/target/${triple:+${triple}/}release/silkterm"
	[[ -f "${bin}" ]] || bin="${bin}.exe"
	[[ -f "${bin}" ]] || { echo "no binary under ${dir}/target" >&2; exit 2; }
	bins+=("${bin}")
done

sumA="$(sha256sum "${bins[0]}" | cut -d' ' -f1)"
sumB="$(sha256sum "${bins[1]}" | cut -d' ' -f1)"
echo "${sumA}  ${bins[0]#"${work}/"}"
echo "${sumB}  ${bins[1]#"${work}/"}"
if [[ "${sumA}" == "${sumB}" ]]; then
	echo "identical"
	exit 0
fi
echo "they differ in $(cmp -l "${bins[0]}" "${bins[1]}" | wc -l || true) byte(s)"
exit 1

##	History:
##		- 20260925 JC: Created.
