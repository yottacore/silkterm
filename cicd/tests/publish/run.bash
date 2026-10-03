#!/usr/bin/env bash

##	- Purpose:
##		The publish script commits and pushes, so nothing the caller's environment
##		holds may be executed inside it. It used to eval one of its own variables.
##	- Test ID: EpHV4IK
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
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT
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
	local -r dir="${work}/${1}/proj"; shift
	rc=0; out="$(cd "${dir}" && bash "${publish}" -q "${@}" 2>&1)" || rc=$?
}
fUpstream(){  ## fUpstream <name> <line1>: another clone pushes a change to line one
	local -r dir="${work}/${1}"
	git clone -q "${dir}/remote.git" "${dir}/other" 2>/dev/null
	git -C "${dir}/other" config user.name t; git -C "${dir}/other" config user.email t@t
	printf '%s\ntwo\nthree\nfour\n' "${2}" >"${dir}/other/file.txt"
	git -C "${dir}/other" commit -qam upstream; git -C "${dir}/other" push -q origin main 2>/dev/null
}
fLastMsg(){ git -C "${work}/${1}/remote.git" log -1 --format=%B main; }

## The exclude list reaches rar through the publisher's own reader, one argument
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

## The shipped list and the reader still agree.
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
source <(sed -n '/^auto_msg=/,/^}/p' "${root}/cicd.bash")
fCheck "the plan names the automatic message" grep -qF '(will prompt for message; blank = \"${auto_msg}\")' "${root}/cicd.bash"
fCheck "the prompt does too" grep -qF 'Publish commit message (blank = \"${auto_msg}\"' "${root}/cicd.bash"
fCheck "and its answer goes through the same choice" grep -qF 'publish_msg="$(fPublishMessage "" "" "$m")"' "${root}/cicd.bash"
fCheck "a blank answer takes the automatic message" test "$(fPublishMessage "" "" "")" = "Silk CI/CD 20260101-000000"
fCheck "a typed answer is used as typed" test "$(fPublishMessage "" "" "typed")" = "typed"
fCheck "--message still wins" test "$(fPublishMessage "cli" "cfg" "")" = "cli"
fRepo blank
echo more >>"${work}/blank/proj/file.txt"
rc=0; out="$(cd "${work}/blank/proj" && GIT_BACKUP_AND_PUBLISH_QUIET=1 GIT_AUTO_MESSAGE="$(fPublishMessage "" "" "")" bash "${publish}" --quiet 2>&1)" || rc=$?
fCheck "and that is the message committed" test "$(fLastMsg blank)" = "Silk CI/CD 20260101-000000"

## The commit message is the one question. A second "Proceed? [y/N]" after it
## was removed; its answer would be read as the next thing on stdin.
prompt="$(sed -n '/^if ((! assume_yes)); then$/,/^fi$/p' "${root}/cicd.bash")"
fAsk(){  ## fAsk <stdin>: the prompt block, then whatever it left unread
	# shellcheck disable=SC2034  ## read by the lifted block
	(assume_yes=0; publish_msg=""; GIT_PUBLISH=(x); fEcho_ResetBlankCounter(){ :; }
		eval "${prompt}"; echo "msg=${publish_msg}"; read -r rest || true; echo "rest=${rest:-}") <<<"${1}"
}
fCheck "the prompt block is found in cicd.bash" test -n "${prompt}"
fCheck "a typed message is the only line read" test "$(fAsk $'typed\nn')" = $'msg=typed\nrest=n'
fCheck "a blank line takes the automatic message" test "$(fAsk $'\nn')" = $'msg=Silk CI/CD 20260101-000000\nrest=n'

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260908 JC: Created.
##		- 20260917 JC: A failed pull, a message as given, and the blank prompt answer.
##		- 20260917 JC: The excludes go through the publisher's own reader to a stub rar.
##		- 20260922 JC: The release packages stay in the backup.
##		- 20260926 JC: The commit message prompt asks nothing after it.
