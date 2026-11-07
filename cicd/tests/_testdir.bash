#!/usr/bin/env bash

##	- Purpose:
##		The run's test folder, HOST_TEMP/silkterm/YYYYmmDD-HHMMSS_NNN with a
##		run.pid, as cicd.bash makes its own, that every file a test writes goes
##		under. Without HOST_TEMP it's under TMPDIR. Sourced; it defines fTestDir_Make,
##		fTestDir_Use, fTestDir_Owned, fTestDir_Rm, fTestDir_OnlyMarks and
##		fTestDir_End, plus the safe removal guard's fSafeRm and fSafeRm_Under, and
##		runs nothing. The Rust tests
##		(source/src/testdir.rs), _testdir.py and _testdir.ps1 keep the same contract:
##			- SILKTERM_TEST_DIR set and not empty: that is the folder, made if missing,
##			  and never removed.
##			- Otherwise a fresh one is made, 0700, and exported as SILKTERM_TEST_DIR
##			  so every test the caller starts shares it. The process that made it
##			  removes it when it exits with 0, or 3 for a skip, and keeps it
##			  otherwise, for the pipeline's stale sweep to take once its run.pid
##			  is gone. A kept one holding nothing but its owner mark and run.pid
##			  goes anyway, since there's nothing in it to look at.
##		fTestDir_Make sets an EXIT trap that calls fTestDir_End. A caller that sets
##		its own EXIT trap afterward replaces it, so that trap takes $? first and
##		calls fTestDir_End "${rc}" last:
##			trap 'rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"' EXIT
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

__testDirMarker=".test_silkterm_owner"
__testDirHere="${BASH_SOURCE[0]%/*}"; [[ "${__testDirHere}" != "${BASH_SOURCE[0]}" ]] || __testDirHere="."
##  shellcheck source=cicd/utility/include/saferm.bash
source "${__testDirHere}/../utility/include/saferm.bash"

fTestDir_Make(){
	##	Sets and exports SILKTERM_TEST_DIR. Returns 1, with the reason on stderr, when it cannot.
	if [[ -n "${SILKTERM_TEST_DIR:-}" ]]; then
		## Tested first, since some tests start others with a PATH that has no mkdir.
		if [[ ! -d "${SILKTERM_TEST_DIR}" ]] && ! mkdir -p -- "${SILKTERM_TEST_DIR}"; then echo "test run folder: cannot make '${SILKTERM_TEST_DIR}'" >&2; return 1; fi
		export SILKTERM_TEST_DIR
		return 0
	fi
	local base="${HOST_TEMP:-${TMPDIR:-/tmp}}"; base="${base%/}/silkterm"
	local dir="" err="" stamp=""
	local -i n
	if ! err="$(mkdir -p -- "${base}" 2>&1)"; then echo "test run folder: ${err}" >&2; return 1; fi
	stamp="$(date +%Y%m%d-%H%M%S)"
	for ((n = 1; n <= 999; n++)); do
		printf -v dir '%s/%s_%03d' "${base}" "${stamp}" "${n}"
		## Never mkdir -p: it would adopt a folder or symlink planted at a guessable name.
		if err="$(mkdir -m 700 -- "${dir}" 2>&1)"; then
			export SILKTERM_TEST_DIR="${dir}"
			## Kept out of the environment, so a child never thinks the folder is its own.
			__testDirOwned="${dir}"
			__testDirToken="${BASHPID}-$(date +%s%N)"
			echo "${__testDirToken}" >"${dir}/${__testDirMarker}"
			echo "$$" >"${dir}/run.pid"
			trap 'fTestDir_End $?' EXIT
			return 0
		fi
		if [[ ! -e "${dir}" && ! -L "${dir}" ]]; then echo "test run folder: ${err}" >&2; return 1; fi
	done
	echo "test run folder: '${dir}' and every name before it were taken" >&2
	return 1
}

fTestDir_End(){
	##	fTestDir_End <status>: removes the folder fTestDir_Make made, when status
	##	is 0 or 3 (a skip), or it holds nothing else, and the folder still has
	##	this run's mark. Always returns 0, so it never changes how a script exits.
	local -r status="${1:-0}" dir="${__testDirOwned:-}"
	[[ -n "${dir}" ]] || return 0
	__testDirOwned=""
	if [[ "${status}" != 0 && "${status}" != 3 ]] && ! fTestDir_OnlyMarks "${dir}"; then echo "test files kept in ${dir}" >&2; return 0; fi
	local reason="" line="" err=""
	if [[ -L "${dir}" ]]; then reason="it is a link"
	elif [[ ! -d "${dir}" ]]; then reason="not a folder"
	elif [[ -L "${dir}/${__testDirMarker}" || ! -f "${dir}/${__testDirMarker}" ]]; then reason="no owner mark"
	else
		read -r line <"${dir}/${__testDirMarker}" || true
		[[ "${line}" == "${__testDirToken}" ]] || reason="another run's owner mark"
	fi
	if [[ -n "${reason}" ]]; then echo "test run folder: left ${dir} in place: ${reason}" >&2; return 0; fi
	## Under the temp folder it was made in, resolved, since that can sit behind a link.
	local up=""
	up="$(realpath -e -- "${dir%/*}" 2>/dev/null)" && err="$(fSafeRm_Under "${up}" "${up}/${dir##*/}" 2>&1)" \
		|| echo "test run folder: could not remove ${dir}${err:+: ${err}}" >&2
	return 0
}

fTestDir_OnlyMarks(){
	##	fTestDir_OnlyMarks <dir>: true when <dir> holds nothing but the owner mark
	##	and run.pid.
	##	Globs, not find, since some tests run with a PATH that has none.
	local f
	[[ -d "${1}" && ! -L "${1}" ]] || return 1
	for f in "${1}"/* "${1}"/.[!.]* "${1}"/..?*; do
		[[ -e "${f}" || -L "${f}" ]] || continue
		[[ "${f##*/}" == "${__testDirMarker}" || "${f##*/}" == run.pid ]] || return 1
	done
}

fTestDir_Rm(){
	##	fTestDir_Rm <path>: removes a folder tree inside this run's test folder,
	##	through the safe removal guard with that folder as the one parent. Leaves
	##	a path that isn't there alone, as rm -rf does. Returns 1, with the reason
	##	on stderr, when the guard refuses.
	local p="${1:-}" root=""
	[[ -e "${p}" || -L "${p}" ]] || return 0
	root="$(realpath -e -- "${SILKTERM_TEST_DIR:-}" 2>/dev/null)" || { echo "test run folder: no folder to remove ${p} from" >&2; return 1; }
	## A path built on the folder's name before it was resolved, as TMPDIR is.
	[[ "${p}" != "${SILKTERM_TEST_DIR}"/* ]] || p="${root}/${p#"${SILKTERM_TEST_DIR}"/}"
	fSafeRm_Under "${root}" "${p}"
}

fTestDir_Owned(){
	##	Prints the folder and its token, tab between, when this process made the
	##	folder; nothing for one it was handed. The pipeline writes it down, so a
	##	later run can remove it if this one fails and keeps it.
	[[ -n "${__testDirOwned:-}" ]] || return 0
	printf '%s\t%s\n' "${__testDirOwned}" "${__testDirToken}"
}

fTestDir_Use(){
	##	fTestDir_Make, then points TMPDIR at the folder, so the script's own mktemp
	##	calls and every program it starts write there. TEMP and TMP too, as the
	##	pipeline sets them, since a program may read either spelling.
	fTestDir_Make || return 1
	export TMPDIR="${SILKTERM_TEST_DIR}" TEMP="${SILKTERM_TEST_DIR}" TMP="${SILKTERM_TEST_DIR}"
}

##	History:
##		- 20260930 JC: Created.
##		- 20261002 JC: A run removes the folder it made when it passes.
##		- 20261019 JC: fTestDir_Owned, for the pipeline's record of its folders.
##		- 20261031: fTestDir_Rm, and the folder goes through the safe removal guard.
##		- 20261031: fTestDir_Use points TEMP and TMP at the folder too.
##		- 20261101: The folder is HOST_TEMP/silkterm/YYYYmmDD-HHMMSS_NNN, with a run.pid.
##		- 20261106: A skip, or a failed run that left nothing in it, removes it too.
