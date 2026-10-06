#!/usr/bin/env bash

##	- Purpose: Cut a release locally from main. No hosted CI: the tag, the
##	  artifacts, and the optional GitHub Release upload all happen on this box.
##	- Flow (run AFTER merging dev into main --no-ff):
##	   1. verify: on main, clean tree, version bumped, README badge matches
##	   2. run the full pipeline if the release artifacts are missing/stale
##	   3. tag the merge: v<version>, where <version> comes from source/Cargo.toml
##	      alone (the build stamps from it too, so they can never disagree)
##	   4. --push: push main + the tag
##	   5. --publish: also attach cicd/artifacts/release/* to a GitHub Release
##	      as plain uploads (gh CLI; no Actions)
##	- Syntax:
##	  cicd/utility/release.bash [--push] [--publish] [-y]
##	  With no flags it tags only and prints the remaining steps.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/../.." && pwd)"
cd "${root}"
source "${here}/../config.bash"
source "${here}/include/remote-git.bash"
##  shellcheck source=cicd/utility/include/echo.bash
source "${here}/include/echo.bash"

doPush=0; doPublish=0; assumeYes=0
while (($#)); do case "$1" in
	--push)    doPush=1; shift ;;
	--publish) doPush=1; doPublish=1; shift ;;
	-y|--yes)  assumeYes=1; shift ;;
	-h|--help) sed -n '/^##	- Purpose:/,/^##	Copyright/p' "${BASH_SOURCE[0]}" | sed '$d; s/^##	\{0,1\}//'; exit 0 ;;
	*) fEcho_Clean "unknown option: ${1} (try --help)" >&2; exit 2 ;;
esac; done

## 1. Preconditions: releases only cut from a clean main, with the version
## already bumped on dev (so no commit ever goes directly onto main here).
branch="$(git rev-parse --abbrev-ref HEAD)"
[[ "$branch" == "main" ]] || fDie "not on main (on ${branch}); merge dev --no-ff into main first"
git diff --quiet && git diff --cached --quiet || fDie "working tree not clean"

ver="$(sed -n 's/^version *= *"\(.*\)".*/\1/p' "${VERSION_MANIFEST}" | head -1)"
[[ -n "$ver" ]] || fDie "no version in ${VERSION_MANIFEST}"
tag="v${ver}"
git rev-parse -q --verify "refs/tags/${tag}" >/dev/null && fDie "tag ${tag} already exists - bump the version on dev first"

## The README release badge is static; it must be bumped on dev with the version
## (shields.io escapes '-' as '--'), never patched here on main.
badgeVer="${ver//-/--}"
grep -q "Release-${badgeVer}-" README.md || fDie "README release badge does not say ${ver} - update it on dev before the release merge"

## 2. Release artifacts must exist and carry this version (full cicd run makes them).
artDir="${RELEASE_ARTIFACT_DIR}"
sums="${artDir}/${EXE_NAME}-${ver}-sha256sums.txt"
[[ -s "$sums" ]] || fDie "no ${sums} - run cicd/cicd.bash (full, not --quick) first"
( cd "${artDir}" && sha256sum -c "${EXE_NAME}-${ver}-sha256sums.txt" >/dev/null ) || fDie "artifact checksums do not verify"
## The sums only say the artifacts match each other. This says they match the
## source being tagged - without it a pipeline run, more commits, then a merge
## leaves a stale artifact directory that verifies cleanly and publishes the old
## binaries under the new tag. It also says the set is whole, since a --quick or
## --no-cross run leaves the native binary in there on its own and reads exactly
## like a full one from here.
##  shellcheck source=cicd/utility/built-from.bash
source "$(dirname "${BASH_SOURCE[0]}")/built-from.bash"
why="$(fCheckBuiltFrom "${artDir}")" || fDie "${why}"

## The build number comes out of the artifact itself. Every target in a pipeline
## run shares one (cicd pins SILK_BUILD_MINUTES), so the native binary's answer is
## also the Windows and ARM ones. Asking the artifact rather than the clock is what
## stops the notes naming a build nobody can download.
native="${artDir}/${EXE_NAME}-${ver}-linux-$(uname -m)"
buildId=""
if [[ -x "$native" ]]; then
	## || true: pipefail makes an artifact that won't run (wrong arch, missing lib)
	## fail the assignment, and set -e would take the whole release down with it.
	buildId="$("$native" --version 2>/dev/null | sed -n 's/.* build \([^ ]*\)$/\1/p' || true)"
fi
[[ -n "$buildId" ]] || fEcho_Clean "note: could not read a build number from ${native##*/}; notes will omit it"

## Sign the checksum file. Everything else is covered by it, so one signature
## covers the whole release. Done before the tag so a signing failure costs
## nothing.
sig="${sums}.sig"
rm -f "${sig}"
if [[ -n "${RELEASE_SIGN_KEY:-}" ]]; then
	[[ -r "${RELEASE_SIGN_KEY}" ]] || fDie "cannot read the signing key ${RELEASE_SIGN_KEY}"
	command -v ssh-keygen >/dev/null 2>&1 || fDie "ssh-keygen not found, and the release is set up to be signed"
	ssh-keygen -Y sign -f "${RELEASE_SIGN_KEY}" -n "${RELEASE_SIGN_NAMESPACE}" "${sums}" >/dev/null \
		|| fDie "signing ${sums##*/} failed"
	[[ -s "${sig}" ]] || fDie "signing produced no ${sig##*/}"
	fEcho_Clean "signed ${sums##*/}"
else
	fEcho_Clean "note: no signing key set (RELEASE_SIGN_KEY) - this release will be unsigned"
fi

## The notes link each download by name, so they are built from the same list
## that gets uploaded, before the tag so a problem here costs nothing.
assets=("${artDir}/${EXE_NAME}-${ver}-"*)
notes=""; slug=""
if ((doPublish)); then
	##  shellcheck source=cicd/utility/release-notes.bash
	source "${here}/release-notes.bash"
	slug="$(fReleaseNotes_RepoSlug "$(git remote get-url origin)")" || fDie "no GitHub owner/repo for the download links"
	notes="$(fReleaseNotes "${slug}" "${tag}" "${EXE_NAME}" "${ver}" "${buildId}" "${assets[@]}")"
fi

fEcho_Clean
fEcho_Clean "Release ${tag} from $(git rev-parse --short HEAD) on main${buildId:+, build ${buildId}}"
fEcho_Clean "Artifacts:"; ls -1 "${artDir}/${EXE_NAME}-${ver}-"* | sed 's/^/  /'
fEcho_Clean "Push: ${doPush}  Publish (gh): ${doPublish}"
if ((! assumeYes)); then read -r -p "Proceed? [y/N] " a; [[ "$a" == [yY]* ]] || exit 1; fi

## 3. Tag the merge.
git tag -a "${tag}" -m "${tag}"
fEcho_Clean "tagged ${tag}"

## 4/5. Push and publish.
if ((doPush)); then
	fRemoteGit push origin main
	fRemoteGit push origin "${tag}"
	fEcho_Clean "pushed main + ${tag}"
else
	fEcho_Clean "next: git push origin main && git push origin ${tag}"
fi
## A semver version carrying a pre-release suffix (the '-' in 1.0.0-beta2) is
## marked as one on the release page too. That is not cosmetic: both installers
## go by that mark, and a stable install takes a pre-release only when no full
## release exists. Publishing a beta unmarked makes it the stable answer.
prerelease=()
if [[ "$ver" == *-* ]]; then prerelease=(--prerelease); fi

if ((doPublish)); then
	command -v gh >/dev/null 2>&1 || fDie "gh CLI not found"
	fRemoteGh release create "${tag}" --repo "${slug}" --title "${APP_NAME} ${ver}" --notes "${notes}" \
		"${prerelease[@]}" "${assets[@]}"
	fEcho_Clean "GitHub Release ${tag} created with artifacts${prerelease:+ (pre-release)}"
elif ((doPush)); then
	fEcho_Clean "next (optional): gh release create ${tag} ${prerelease[*]} ${artDir}/${EXE_NAME}-${ver}-*"
fi

##	History:
##		- 20260711: Created.
##		- 20261006: Output through the shared fEcho and fDie helpers.
