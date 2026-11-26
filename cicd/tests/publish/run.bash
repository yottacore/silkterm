#!/usr/bin/env bash

##	- Purpose:
##		The publish script commits and pushes, so nothing from the caller's
##		environment may run inside it. It used to eval one of its own variables.
##	- Test ID: dcbdv
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../.." && pwd)"
publish="${root}/utility/n8git_backup-and-publish"
config="${root}/config.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

fCheck "nothing in the publish script is eval'd" \
	bash -c '! grep -Eq "(^|[^[:alnum:]_])eval[[:space:]]" "$1"' _ "${publish}"

## Runs against a real repository from here on: a bare remote and a clone of it,
## with git's own config kept out so nothing on this box decides the result.
work="$(mktemp -d "${TMPDIR:-/tmp}/silk-publish.XXXXXX")"
trap 'rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset GIT_CONFIG_COUNT GIT_DIR GIT_WORK_TREE GIT_AUTO_MESSAGE GIT_BACKUP_AND_PUBLISH_MESSAGE
export GIT_BACKUP_AND_PUBLISH_NOBACKUP=1 GIT_EDITOR=false

fRepo(){  ## fRepo <name>: a remote, a clone of it at <name>/proj with one commit
	local -r dir="${work}/${1}"
	mkdir -p "${dir}"
	git init -q --bare -b main "${dir}/remote.git"
	git clone -q "${dir}/remote.git" "${dir}/proj" 2>/dev/null
	git -C "${dir}/proj" config user.name t; git -C "${dir}/proj" config user.email t@t
	printf 'one\ntwo\nthree\nfour\n' >"${dir}/proj/file.txt"
	git -C "${dir}/proj" add file.txt; git -C "${dir}/proj" commit -qm first; git -C "${dir}/proj" push -q origin main 2>/dev/null
}
fPublish(){  ## fPublish <name> <args...>: run the publisher in that clone; sets rc and out
	## The stub rar below is on PATH: the publisher wants one there even with the
	## backup off, and a box without rar (the container) is still a fair test.
	local -r dir="${work}/${1}/proj"; shift
	rc=0; out="$(cd "${dir}" && PATH="${stubs}:${PATH}" bash "${publish}" -q "${@}" 2>&1)" || rc=$?
}
fUpstream(){  ## fUpstream <name> <line1>: another clone pushes a change to line one
	local -r dir="${work}/${1}"
	git clone -q "${dir}/remote.git" "${dir}/other" 2>/dev/null
	git -C "${dir}/other" config user.name t; git -C "${dir}/other" config user.email t@t
	printf '%s\ntwo\nthree\nfour\n' "${2}" >"${dir}/other/file.txt"
	git -C "${dir}/other" commit -qam upstream; git -C "${dir}/other" push -q origin main 2>/dev/null
}
fLastMsg(){ git -C "${work}/${1}/remote.git" log -1 --format=%B main; }

## The exclude list is passed to rar through the publisher's own reader, one argument
## per line. A value that would have run a command under the old eval is only a
## pattern. The stub rar writes down what it was given and archives nothing.
stubs="${work}/stubs"
mkdir -p "${stubs}"
cat >"${stubs}/rar" <<EOF
#!/bin/sh
printf '%s\n' "\$@" >"${work}/rar-args"
EOF
chmod +x "${stubs}/rar"
fRarArgs(){  ## fRarArgs <name> <excludes>: publish with a backup; the args rar got are in rar-args
	rm -f "${work}/rar-args"
	fRepo "${1}"
	echo more >>"${work}/${1}/proj/file.txt"
	rc=0; out="$(cd "${work}/${1}/proj" && PATH="${stubs}:${PATH}" GIT_BACKUP_AND_PUBLISH_NOBACKUP=0 \
		GIT_BACKUP_AND_PUBLISH_RAR_EXCLUDES="${2}" bash "${publish}" -q -m excludes 2>&1)" || rc=$?
	touch "${work}/rar-args"
}
canary="${work}/canary"
fRarArgs hostile "*/ok
\$(touch '${canary}')
\`touch '${canary}'\`"
fCheck "a publish with a backup runs rar" test "${rc}" -eq 0 -a -s "${work}/rar-args"
fCheck "every line becomes one argument" grep -qFx -- "-x\$(touch '${canary}')" "${work}/rar-args"
fCheck "and the first is the pattern it was given" grep -qFx -- "-x*/ok" "${work}/rar-args"
fCheck "nothing in the value ran" test ! -e "${canary}"

## The released list and the reader still agree.
shipped="$(bash -c 'source "$1" && printf "%s" "${GIT_BACKUP_AND_PUBLISH_RAR_EXCLUDES}"' _ "${config}")"
fRarArgs shipped "${shipped}"
fCheck "the shipped excludes reach rar" grep -qFx -- "-x*/forks" "${work}/rar-args"
fCheck "and the release packages stay in" bash -c '! grep -qE "^-x.*cicd/artifacts(/release)?/?$" "$1"' _ "${work}/rar-args"
fCheck "as plain patterns" bash -c '! grep -q "^-x.*[\"'"'"']" "$1"' _ "${work}/rar-args"

## A failed pull leaves the work where it was, tracked and untracked, with no stash.
fRepo gone
printf 'one\ntwo\nthree\nmine\n' >"${work}/gone/proj/file.txt"; echo new >"${work}/gone/proj/untracked.txt"
mv "${work}/gone/remote.git" "${work}/gone/moved.git"
fPublish gone -m "should not go"
fCheck "an unreachable remote fails the run" test "${rc}" -ne 0
fCheck "and says nothing was pushed" grep -q "Nothing was committed or pushed" <<<"${out}"
fCheck "the tracked change is back in the tree" grep -qx mine "${work}/gone/proj/file.txt"
fCheck "the untracked file is back" test -f "${work}/gone/proj/untracked.txt"
fCheck "and nothing is left stashed" test -z "$(git -C "${work}/gone/proj" stash list)"

## A reachable remote still stashes, pulls and pops.
fRepo fine
printf 'one\ntwo\nthree\nmine\n' >"${work}/fine/proj/file.txt"
fUpstream fine theirs
fPublish fine -m "both"
fCheck "a reachable remote publishes" test "${rc}" -eq 0
fCheck "with the pulled change and the local one" test "$(git -C "${work}/fine/remote.git" show main:file.txt)" = "theirs
two
three
mine"

## A pop that conflicts still stops and says what to do.
fRepo clash
printf 'mine\ntwo\nthree\nfour\n' >"${work}/clash/proj/file.txt"
fUpstream clash theirs
fPublish clash -m "clash"
fCheck "a conflicting pop fails the run" test "${rc}" -ne 0
fCheck "and says how to finish" grep -q "then 'git stash drop'" <<<"${out}"

## A message is committed byte for byte, and a flag inside it is only text.
msg="don't \"quote\" -v and -h"
fRepo words
echo more >>"${work}/words/proj/file.txt"
fPublish words -m "${msg}"
fCheck "a message with quotes and flags publishes" test "${rc}" -eq 0
fCheck "and is committed as given" test "$(fLastMsg words)" = "${msg}"
fRepo inline
echo more >>"${work}/inline/proj/file.txt"
fPublish inline "--msg=it's -v"
fCheck "an inline message is committed as given" test "$(fLastMsg inline)" = "it's -v"
fRepo ver
echo more >>"${work}/ver/proj/file.txt"
fPublish ver -v
fCheck "a bare -v still only shows the version" test "$(fLastMsg ver)" = "first"

## cicd.bash: a blank answer at the publish prompt takes the message its plan
## names, and the publisher commits that message.
# shellcheck disable=SC2034  ## read by the sourced function
APP_NAME=Silk stamp=20260101-000000
# shellcheck disable=SC1090
source <(sed -n '/^autoMsg=/,/^}/p' "${root}/cicd.bash")
fCheck "the plan names the automatic message" grep -qF '(will prompt for message; blank = \"${autoMsg}\")' "${root}/cicd.bash"
fCheck "the prompt does too" grep -qF 'Publish commit message (blank = \"${autoMsg}\"' "${root}/cicd.bash"
fCheck "and its answer goes through the same choice" grep -qF 'publishMsg="$(fPublishMessage "" "" "${m}")"' "${root}/cicd.bash"
fCheck "a blank answer takes the automatic message" test "$(fPublishMessage "" "" "")" = "Silk CI/CD 20260101-000000"
fCheck "a typed answer is used as typed" test "$(fPublishMessage "" "" "typed")" = "typed"
fCheck "--message still wins" test "$(fPublishMessage "cli" "cfg" "")" = "cli"
fRepo blank
echo more >>"${work}/blank/proj/file.txt"
rc=0; out="$(cd "${work}/blank/proj" && PATH="${stubs}:${PATH}" GIT_BACKUP_AND_PUBLISH_QUIET=1 GIT_AUTO_MESSAGE="$(fPublishMessage "" "" "")" bash "${publish}" --quiet 2>&1)" || rc=$?
fCheck "and that is the message committed" test "$(fLastMsg blank)" = "Silk CI/CD 20260101-000000"

## The commit message is the one question. A second "Proceed? [y/N]" after it
## was removed; its answer would be read as the next thing on stdin.
prompt="$(sed -n '/^if ((! assumeYes)); then$/,/^fi$/p' "${root}/cicd.bash")"
fAsk(){  ## fAsk <stdin>: the prompt block, then whatever it left unread
	# shellcheck disable=SC2034  ## read by the lifted block
	(assumeYes=0; publishMsg=""; GIT_PUBLISH=(x); fEcho_ResetBlankCounter(){ :; }
		eval "${prompt}"; echo "msg=${publishMsg}"; read -r rest || true; echo "rest=${rest:-}") <<<"${1}"
}
fCheck "the prompt block is found in cicd.bash" test -n "${prompt}"
fCheck "a typed message is the only line read" test "$(fAsk $'typed\nn')" = $'msg=typed\nrest=n'
fCheck "a blank line takes the automatic message" test "$(fAsk $'\nn')" = $'msg=Silk CI/CD 20260101-000000\nrest=n'

## With a path list, only the tracked files named are committed. Another edit in
## the tree, and one already staged, stay out of the commit.
fOnly(){  ## fOnly <name> <paths>: publish with that list; another edit and a staged one wait in the tree
	fRepo "${1}"
	local -r dir="${work}/${1}/proj"
	printf 'x\n' >"${dir}/other.txt"; printf 'y\n' >"${dir}/staged.txt"
	git -C "${dir}" add other.txt staged.txt; git -C "${dir}" commit -qm more; git -C "${dir}" push -q origin main 2>/dev/null
	echo fmt >>"${dir}/file.txt"; echo theirs >>"${dir}/other.txt"
	echo staged >>"${dir}/staged.txt"; git -C "${dir}" add staged.txt
	rc=0; out="$(cd "${dir}" && PATH="${stubs}:${PATH}" GIT_BACKUP_AND_PUBLISH_PATHS="${2}" bash "${publish}" -q -m only 2>&1)" || rc=$?
}
fOnly only "file.txt
not-tracked.txt"
fCheck "a path list publishes, a path git doesn't track skipped" test "${rc}" -eq 0
fCheck "with the named file" test "$(git -C "${work}/only/remote.git" show main:file.txt | tail -1)" = fmt
fCheck "and nothing else" test "$(git -C "${work}/only/remote.git" show --format= --name-only main)" = file.txt
## The stash pop puts a staged edit back unstaged, as it always has. Its text stays.
fCheck "the other edits are still in the tree, uncommitted" test "$(git -C "${work}/only/proj" diff HEAD --name-only | tr '\n' ' ')" = "other.txt staged.txt "
fOnly none ""
fCheck "an empty list publishes" test "${rc}" -eq 0
fCheck "and commits nothing" test "$(fLastMsg none)" = more
fCheck "leaving both edits" test "$(git -C "${work}/none/proj" diff HEAD --name-only | tr '\n' ' ')" = "file.txt other.txt staged.txt "

## cicd.bash hands the publisher what its ledger names, and the ledger takes only
## files a step changed that matched HEAD before it.
fCheck "cicd.bash passes the ledger to the publisher" grep -qF 'GIT_BACKUP_AND_PUBLISH_PATHS="${pubPaths}"' "${root}/cicd.bash"
fCheck "the format step adds to it" grep -qF 'fRunChanged "${FMT_PATHS[@]}" -- "${FMT_CMD[@]}"' "${root}/cicd.bash"
fCheck "and so does the demo" grep -qF 'fRunChanged "${DEMO_PATHS[@]}" -- python3' "${root}/cicd.bash"
fCheck "a run in the container gets the ledger" grep -qF -- '-v "${SILK_PUBLISH_LEDGER%/*}:${SILK_PUBLISH_LEDGER%/*}" -e SILK_PUBLISH_LEDGER' "${root}/cicd.bash"
# shellcheck disable=SC1090
source <(sed -n '/^fRunChanged(){$/,/^}$/p' "${root}/cicd.bash")
fRepo ledger
lroot="${work}/ledger/proj"
printf 'fn a() {}\n' >"${lroot}/clean.rs"; printf 'fn b() {}\n' >"${lroot}/dirty.rs"; printf 'c\n' >"${lroot}/notes.txt"
git -C "${lroot}" add clean.rs dirty.rs notes.txt; git -C "${lroot}" commit -qm rs
echo '// mine' >>"${lroot}/dirty.rs"
fFmt(){ echo '//' >>"${lroot}/clean.rs"; echo '//' >>"${lroot}/dirty.rs"; echo more >>"${lroot}/notes.txt"; }
export SILK_PUBLISH_LEDGER="${work}/ledger/changed.txt"; : >"${SILK_PUBLISH_LEDGER}"
(root="${lroot}"; fRunChanged '*.rs' -- fFmt)
fCheck "a clean file the step changed goes in the ledger" grep -qx clean.rs "${SILK_PUBLISH_LEDGER}"
fCheck "one already edited doesn't" bash -c '! grep -q dirty "$1"' _ "${SILK_PUBLISH_LEDGER}"
fCheck "nor one outside the step's paths" bash -c '! grep -q notes "$1"' _ "${SILK_PUBLISH_LEDGER}"
rc=0; (root="${lroot}"; fRunChanged '*.rs' -- false) || rc=$?
fCheck "the step's failure comes back" test "${rc}" -eq 1
unset SILK_PUBLISH_LEDGER
rc=0; (root="${lroot}"; fRunChanged '*.rs' -- true) || rc=$?
fCheck "with no ledger the step still runs" test "${rc}" -eq 0

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260908 JC: Created.
##		- 20260917 JC: A failed pull, a message as given, and the blank prompt answer.
##		- 20260917 JC: The excludes go through the publisher's own reader to a stub rar.
##		- 20260922 JC: The release packages stay in the backup.
##		- 20260926 JC: The commit message prompt asks nothing after it.
##		- 20261119 JC: A path list commits only the files named, and cicd.bash's
##		  ledger takes only what a step changed of files that matched HEAD.
