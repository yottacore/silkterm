#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted block reads them, and shellcheck cannot see into an eval.

##	- Purpose:
##		Stage 0 of cicd.bash brings the tree up to its upstream before anything is
##		built, so what gets published is what got tested. Behind only is a
##		fast-forward, dirty tree or not; diverged stops the run before the build.
##		The stage is lifted out of cicd.bash as it stands and run against a
##		scratch remote and clones.
##	- Test ID: Er2UgY8
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
cicd="$(cd "${meDir}/../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-sync.XXXXXX")"
trap 'rm -rf "${work}"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset GIT_DIR GIT_WORK_TREE

stage="$(sed -n '/^fSection "0\/8  Remote sync"$/,/^fi$/p' "${cicd}/cicd.bash")"
fCheck "the stage is found in cicd.bash" test -n "${stage}"

## The helpers the stage calls. fRemoteGit is plain git here, so no account
## wrapper decides the result.
fSection(){ :; }
fEcho(){ echo "[ $* ]"; }
fEcho_Clean(){ echo "$*"; }
fDie(){ echo "DIE: $*"; exit 1; }
fRemoteGit(){ git "$@"; }
fStage(){  ## fStage <clone> [sync]: run the stage there; sets rc and out
	rc=0
	out="$(cd "${1}" && sync="${2:-1}" && eval "${stage}" 2>&1)" || rc=$?
}
fClone(){  ## fClone <remote> <dir>
	git clone -q "${1}" "${2}" 2>/dev/null
	git -C "${2}" config user.name t; git -C "${2}" config user.email t@t
}
fCommit(){  ## fCommit <clone> <line>: commit a new last line to file.txt
	echo "${2}" >>"${1}/file.txt"
	git -C "${1}" commit -qam "${2}"
}

remote="${work}/remote.git"
git init -q --bare -b main "${remote}"
fClone "${remote}" "${work}/seed"
printf 'one\ntwo\nthree\n' >"${work}/seed/file.txt"
git -C "${work}/seed" add file.txt; git -C "${work}/seed" commit -qm first; git -C "${work}/seed" push -q origin main 2>/dev/null
fClone "${remote}" "${work}/mine"
fClone "${remote}" "${work}/other"

fStage "${work}/mine"
fCheck "up to date passes" test "${rc}" -eq 0
fCheck "and says so" grep -qF '[ OK: up to date with upstream ]' <<<"${out}"

## Behind only, with a local edit to another line and an untracked file.
fCommit "${work}/other" four; git -C "${work}/other" push -q origin main 2>/dev/null
sed -i 's/^one$/mine/' "${work}/mine/file.txt"
echo new >"${work}/mine/untracked.txt"
fStage "${work}/mine"
fCheck "behind with a dirty tree fast-forwards" test "${rc}" -eq 0 -a "$(git -C "${work}/mine" rev-parse HEAD)" = "$(git -C "${remote}" rev-parse main)"
fCheck "keeping the edit" grep -qx mine "${work}/mine/file.txt"
fCheck "and the pulled line" grep -qx four "${work}/mine/file.txt"
fCheck "and the untracked file" test -f "${work}/mine/untracked.txt"
fCheck "with nothing left stashed" test -z "$(git -C "${work}/mine" stash list)"
fCheck "and no merge commit" test "$(git -C "${work}/mine" rev-list --count HEAD)" -eq 2
git -C "${work}/mine" checkout -q -- file.txt; rm -f "${work}/mine/untracked.txt"

## Ahead only is fine; the publish stage pushes it.
fCommit "${work}/mine" five
fStage "${work}/mine"
fCheck "ahead only passes" test "${rc}" -eq 0 -a -n "$(grep -F '1 ahead' <<<"${out}")"

## Diverged stops the run and leaves the branch where it was.
git -C "${work}/other" pull -q --ff-only 2>/dev/null
fCommit "${work}/other" six; git -C "${work}/other" push -q origin main 2>/dev/null
before="$(git -C "${work}/mine" rev-parse HEAD)"
fStage "${work}/mine"
fCheck "diverged stops the run" test "${rc}" -ne 0
fCheck "and says why" grep -q '^DIE: diverged from upstream (1 ahead, 1 behind)' <<<"${out}"
fCheck "leaving the branch where it was" test "$(git -C "${work}/mine" rev-parse HEAD)" = "${before}"

## No upstream, or --no-sync: nothing to do, and no failure.
git -C "${work}/mine" checkout -q -b local-only
fStage "${work}/mine"
fCheck "a branch with no upstream passes" test "${rc}" -eq 0 -a -n "$(grep -F 'no upstream for local-only' <<<"${out}")"
git -C "${work}/mine" checkout -q main
fStage "${work}/mine" 0
fCheck "--no-sync passes even when diverged" test "${rc}" -eq 0 -a -n "$(grep -F 'remote sync skipped' <<<"${out}")"

## Offline: the fetch fails, the run goes on with the local tree.
mv "${remote}" "${work}/moved.git"
fStage "${work}/mine"
fCheck "a failed fetch only warns" test "${rc}" -eq 0 -a -n "$(grep -F 'WARNING: git fetch failed' <<<"${out}")"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
