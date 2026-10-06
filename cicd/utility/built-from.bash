#!/usr/bin/env bash

##	- Purpose:
##		Tie a set of release artifacts to the source they were built from, so a
##		stale artifact directory cannot be published under a new tag. Sourced by
##		cicd.bash (which writes the note) and release.bash (which checks it).
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

##	The TREE, not the commit. A release is cut from main after 'dev -> main
##	--no-ff', so HEAD is a different commit from the one the pipeline built -
##	but the merge keeps the tree, and the tree is what the binaries came from.
##	The two branches have diverged in ancestry before now without diverging in
##	content, which is exactly the case a commit comparison gets wrong.

BUILT_FROM_FILE="built-from.txt"

## Untracked files named in the note before it gives up and counts the rest. A
## tree with hundreds is not a release candidate; the first few say enough.
BUILT_FROM_UNTRACKED_MAX=10

## fSourceState
## The working tree as it stands, in the note's own lines.
##
## Untracked files that are not ignored count as dirty. They build here and the
## tag does not carry them, so the tagged source may not even compile - a module
## whose 'mod' line is committed and whose file is not is the shape that got
## through. They are named, since "something is untracked" is no use on its own.
fSourceState(){
	local dirty="no"
	local top=""
	top="$(git rev-parse --show-toplevel)"
	local -a untracked=()
	## from the top, since ls-files alone answers for the current directory down
	mapfile -t untracked < <(git -C "${top}" ls-files --others --exclude-standard)
	if ! (git diff --quiet && git diff --cached --quiet); then dirty="yes"; fi
	if ((${#untracked[@]})); then dirty="yes"; fi
	echo "tree $(git rev-parse "HEAD^{tree}")"
	echo "commit $(git rev-parse HEAD)"
	echo "dirty ${dirty}"
	local p
	for p in "${untracked[@]:0:${BUILT_FROM_UNTRACKED_MAX}}"; do echo "untracked ${p}"; done
	if ((${#untracked[@]} > BUILT_FROM_UNTRACKED_MAX)); then
		echo "untracked_more $((${#untracked[@]} - BUILT_FROM_UNTRACKED_MAX))"
	fi
	return 0
}

## fStateField <state-text> <key>
## Every value under <key>, one per line, or nothing.
fStateField(){
	sed -n "s/^${2} //p" <<< "${1}"
}

## fWriteBuiltFrom <artifact-dir> [state] [expected-name...]
##
## <state> is what fSourceState answered BEFORE the first build. The note has to
## name the source the binaries hold, not whatever the tree says by the time the
## note is written: another session committing in the same tree during a build
## left the note naming the new commit with a clean flag, and the binaries holding
## the source from before it. Read now when no state is passed, which is what a
## caller that builds nothing wants.
##
## Each <expected-name> is a file the configuration says a whole set holds. The
## checksums only say the artifacts match each other, so without this a --quick
## run's native-only set reads exactly like a full one.
fWriteBuiltFrom(){
	local -r dir="${1}"
	[[ -d "${dir}" ]] || return 0
	shift
	local state=""
	if (($#)); then state="${1}"; shift; fi
	[[ -n "${state}" ]] || state="$(fSourceState)"
	local now=""
	now="$(fSourceState)"

	local moved="no"
	[[ "$(fStateField "${state}" tree)" == "$(fStateField "${now}" tree)" ]] || moved="yes"
	## Dirty at either end counts. The binaries are only known to hold the
	## committed source if nothing was uncommitted for the whole run.
	local dirty="yes"
	if [[ "$(fStateField "${state}" dirty)" == "no" && "$(fStateField "${now}" dirty)" == "no" ]]; then
		dirty="no"
	fi
	local untrackedList=""
	untrackedList="$(fStateField "${state}" untracked)"
	[[ -n "${untrackedList}" ]] || untrackedList="$(fStateField "${now}" untracked)"

	{
		echo "tree $(fStateField "${state}" tree)"
		echo "commit $(fStateField "${state}" commit)"
		echo "dirty ${dirty}"
		echo "moved ${moved}"
		if [[ -n "${untrackedList}" ]]; then
			local line
			while IFS= read -r line; do echo "untracked ${line}"; done <<< "${untrackedList}"
		fi
		local name
		for name in "${@}"; do echo "expect ${name}"; done
	} > "${dir}/${BUILT_FROM_FILE}"
	return 0
}

## fCheckBuiltFrom <artifact-dir>
## Echoes why on a mismatch and returns non-zero.
fCheckBuiltFrom(){
	local -r dir="${1}"
	local -r note="${dir}/${BUILT_FROM_FILE}"
	if [[ ! -s "${note}" ]]; then
		echo "no ${note} - the artifacts do not say what they were built from; re-run the pipeline"
		return 1
	fi
	local state=""
	state="$(cat "${note}")"
	local builtTree="" builtCommit="" dirty="" moved=""
	builtTree="$(  fStateField "${state}" tree)"
	builtCommit="$(fStateField "${state}" commit)"
	dirty="$(      fStateField "${state}" dirty)"
	moved="$(      fStateField "${state}" moved)"

	if [[ "${dirty}" == "yes" ]]; then
		local untrackedSeen="" more=""
		untrackedSeen="$(fStateField "${state}" untracked | tr '\n' ' ')"
		more="$(fStateField "${state}" untracked_more)"
		if [[ -n "${untrackedSeen}" ]]; then
			echo "the artifacts were built over files git does not track, which the tag will not carry: ${untrackedSeen}${more:+plus ${more} more; }add or ignore them and re-run the pipeline"
		else
			echo "the artifacts were built from a dirty working tree; re-run the pipeline"
		fi
		return 1
	fi
	if [[ "${moved}" == "yes" ]]; then
		echo "the source changed while the artifacts were being built, so they do not all hold ${builtCommit:0:12}; re-run the pipeline"
		return 1
	fi
	local here=""
	here="$(git rev-parse "HEAD^{tree}")"
	if [[ "${builtTree}" != "${here}" ]]; then
		echo "the artifacts were built from ${builtCommit:0:12} (tree ${builtTree:0:12}), not from what is checked out (tree ${here:0:12}); re-run the pipeline"
		return 1
	fi

	## A set missing a target reads like a whole one: --quick, --no-cross and
	## friends leave the native binary alone in there, and a package step that
	## cannot find its tool only warns. Windows then has nothing to install, and
	## the tag cannot be cut again without a new version.
	local -a missing=()
	local want=""
	while IFS= read -r want; do
		[[ -n "${want}" ]] || continue
		[[ -f "${dir}/${want}" ]] || missing+=("${want}")
	done <<< "$(fStateField "${state}" expect)"
	if ((${#missing[@]})); then
		echo "the artifact set is missing ${#missing[@]} file(s) this configuration builds: ${missing[*]}; re-run the full pipeline (not --quick)"
		return 1
	fi
	return 0
}

##	History:
##		- 20260908 JC: Created - the release could publish a stale build under a
##		  new tag with everything reporting green.
##		- 20260917 JC: The note names the source the builds read rather than the
##		  tree at write time, untracked files count as dirty, and it carries the
##		  artifact names a whole set holds.
