#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted block reads them, and shellcheck can't see into an eval.

##	- Purpose:
##		Stage 0 of cicd.bash brings the tree up to its upstream before anything is
##		built, so what gets published is what got tested. Behind only is a
##		fast-forward, dirty tree or not; diverged stops the run before the build.
##		The stage is lifted out of cicd.bash as it stands and run against a
##		scratch remote and clones.
##	- Test ID: dd5np
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
trap 'rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset GIT_DIR GIT_WORK_TREE

stage="$(sed -n '/^if ! fStepOpen "Remote sync"; then$/,/^fi$/p' "${cicd}/cicd.bash")"
fCheck "the stage is found in cicd.bash" test -n "${stage}"

## The helpers the stage calls. fRemoteGit is plain git here, so no account
## wrapper decides the result. With pullFails set, its pull fails after the
## fetch went through, and "block" also leaves an untracked file in the way of
## the stashed one, so the pop that puts the work back fails too.
fStepOpen(){ ((sync)); }
fEcho(){ echo "[ ${*} ]"; }
fEcho_Clean(){ echo "${*}"; }
fDie(){ echo "DIE: ${*}"; exit 1; }
fRemoteGit(){
	if [[ "${1}" == pull && -n "${pullFails:-}" ]]; then
		[[ "${pullFails}" != block ]] || echo theirs >untracked.txt
		return 1
	fi
	git "${@}"
}
fStashes(){ git -C "${1}" stash list | wc -l; }
## Under cicd.bash's own shell options. A command substitution, or a subshell
## on the left of ||, runs without errexit, and a failed git call would go on.
fStage(){  ## fStage <clone> [sync]: run the stage there; sets rc and out
	set +e
	(set -Eeuo pipefail; cd "${1}"; sync="${2:-1}"; eval "${stage}") >"${work}/stage.out" 2>&1
	rc=$?
	set -e
	out="$(cat "${work}/stage.out")"
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

## A failed pull puts the stash back before stopping. It used to leave the tree
## at HEAD with every edit only in the stash, and nothing said so.
fCommit "${work}/other" pulled; git -C "${work}/other" push -q origin main 2>/dev/null
before="$(git -C "${work}/mine" rev-parse HEAD)"
sed -i 's/^one$/mine/' "${work}/mine/file.txt"
echo new >"${work}/mine/untracked.txt"
pullFails=1 fStage "${work}/mine"
fCheck "a failed pull stops the run" test "${rc}" -ne 0
fCheck "and says the tree is as it was" grep -qF 'DIE: git pull --ff-only failed. The working tree is as it was.' <<<"${out}"
fCheck "with the edit back" grep -qx mine "${work}/mine/file.txt"
fCheck "and the untracked file" grep -qx new "${work}/mine/untracked.txt"
fCheck "and nothing left stashed" test "$(fStashes "${work}/mine")" -eq 0
fCheck "and the branch where it was" test "$(git -C "${work}/mine" rev-parse HEAD)" = "${before}"

## When putting it back fails too, the stash stays and the message names it.
pullFails=block fStage "${work}/mine"
fCheck "a failed pull and a failed pop stop the run" test "${rc}" -ne 0
fCheck "and name the stash" grep -qF "putting the stashed work back failed too. It is still in 'stash@{0}' (auto-stash)" <<<"${out}"
fCheck "which is kept" test "$(fStashes "${work}/mine")" -eq 1
## The pop may have put the tracked edit back before it stopped.
git -C "${work}/mine" checkout -q -- file.txt; rm -f "${work}/mine/untracked.txt"; git -C "${work}/mine" stash pop -q

## A stash an earlier run left stops the run before anything else.
git -C "${work}/mine" stash push -q --include-untracked -m auto-stash
fStage "${work}/mine"
fCheck "a leftover auto-stash stops the run" test "${rc}" -ne 0
fCheck "and says so" grep -qF "DIE: an 'auto-stash' from an earlier run is still in 'git stash list'" <<<"${out}"
fCheck "before it pulls" test "$(git -C "${work}/mine" rev-parse HEAD)" = "${before}"
fCheck "leaving the stash" test "$(fStashes "${work}/mine")" -eq 1
git -C "${work}/mine" stash pop -q
fCheck "which still holds the work" grep -qx mine "${work}/mine/file.txt"
git -C "${work}/mine" checkout -q -- file.txt; rm -f "${work}/mine/untracked.txt"

## A pop that fails after a good pull, here on a pulled file in the way of an
## untracked one, names the stash too.
echo theirs >"${work}/other/clash.txt"
git -C "${work}/other" add clash.txt; git -C "${work}/other" commit -qm clash; git -C "${work}/other" push -q origin main 2>/dev/null
echo mine >"${work}/mine/clash.txt"
fStage "${work}/mine"
fCheck "a failed pop after the pull stops the run" test "${rc}" -ne 0
fCheck "and names the stash" grep -qF "DIE: git stash pop failed after the fast-forward. The stashed work is still in 'stash@{0}' (auto-stash)" <<<"${out}"
fCheck "which is kept" test "$(fStashes "${work}/mine")" -eq 1
fCheck "with the untracked file in it" test "$(git -C "${work}/mine" show 'stash@{0}^3:clash.txt')" = mine
git -C "${work}/mine" stash drop -q

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
## The step is off then, so it prints nothing; the plan says why.
fCheck "--no-sync passes even when diverged" test "${rc}" -eq 0 -a -z "${out}"

## Offline: the fetch fails, the run goes on with the local tree.
mv "${remote}" "${work}/moved.git"
fStage "${work}/mine"
fCheck "a failed fetch only warns" test "${rc}" -eq 0 -a -n "$(grep -F 'WARNING: git fetch failed' <<<"${out}")"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
##		- 20261028: The stage is a step, which --no-sync turns off.
##		- 20261117: A failed pull puts the stash back, a failed pop names it, and a
##		  leftover auto-stash stops the run.
