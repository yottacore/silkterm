#!/usr/bin/env bash

##	- Purpose:
##		The run's test folder, <temp>/test_silkterm_YYYYmmDD-HHMMSSNN, that every
##		file a test writes goes under. Sourced; it defines fTestDir_Make,
##		fTestDir_Use and fTestDir_End and runs nothing. The Rust tests
##		(source/src/testdir.rs), _testdir.py and _testdir.ps1 keep the same contract:
##			- SILKTERM_TEST_DIR set and not empty: that is the folder, made if missing,
##			  and never removed.
##			- Otherwise a fresh one is made, 0700, and exported as SILKTERM_TEST_DIR
##			  so every test the caller starts shares it. The process that made it
##			  removes it when it exits with 0, and keeps it otherwise.
##		fTestDir_Make sets an EXIT trap that calls fTestDir_End. A caller that sets
##		its own EXIT trap afterward replaces it, so that trap takes $? first and
##		calls fTestDir_End "${rc}" last:
##			trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

__testDirMarker=".test_silkterm_owner"

fTestDir_Make(){
	##	Sets and exports SILKTERM_TEST_DIR. Returns 1, with the reason on stderr, when it cannot.
	if [[ -n "${SILKTERM_TEST_DIR:-}" ]]; then
		## Tested first, since some tests start others with a PATH that has no mkdir.
		if [[ ! -d "${SILKTERM_TEST_DIR}" ]] && ! mkdir -p -- "${SILKTERM_TEST_DIR}"; then echo "test run folder: cannot make '${SILKTERM_TEST_DIR}'" >&2; return 1; fi
		export SILKTERM_TEST_DIR
		return 0
	fi
	local -r base="${TMPDIR:-/tmp}"
	local dir="" err=""
	local -i tries
	for ((tries = 1; tries <= 100; tries++)); do
		dir="${base%/}/test_silkterm_$(date +%Y%m%d-%H%M%S%2N)"
		## Never mkdir -p: it would adopt a folder or symlink planted at a guessable name.
		if err="$(mkdir -m 700 -- "${dir}" 2>&1)"; then
			export SILKTERM_TEST_DIR="${dir}"
			## Kept out of the environment, so a child never thinks the folder is its own.
			__testDirOwned="${dir}"
			__testDirToken="${BASHPID}-$(date +%s%N)"
			echo "${__testDirToken}" >"${dir}/${__testDirMarker}"
			trap 'fTestDir_End $?' EXIT
			return 0
		fi
		if [[ ! -e "${dir}" && ! -L "${dir}" ]]; then echo "test run folder: ${err}" >&2; return 1; fi
		sleep 0.01
	done
	echo "test run folder: '${dir}' and every name before it were taken" >&2
	return 1
}

fTestDir_End(){
	##	fTestDir_End <status>: removes the folder fTestDir_Make made, when status
	##	is 0 and the folder still has this run's mark. Always returns 0, so it
	##	never changes how a script exits.
	local -r status="${1:-0}" dir="${__testDirOwned:-}"
	[[ -n "${dir}" ]] || return 0
	__testDirOwned=""
	if [[ "${status}" != 0 ]]; then echo "test files kept in ${dir}" >&2; return 0; fi
	local reason="" line="" err=""
	if [[ -L "${dir}" ]]; then reason="it is a link"
	elif [[ ! -d "${dir}" ]]; then reason="not a folder"
	elif [[ -L "${dir}/${__testDirMarker}" || ! -f "${dir}/${__testDirMarker}" ]]; then reason="no owner mark"
	else
		read -r line <"${dir}/${__testDirMarker}" || true
		[[ "${line}" == "${__testDirToken}" ]] || reason="another run's owner mark"
	fi
	if [[ -n "${reason}" ]]; then echo "test run folder: left ${dir} in place: ${reason}" >&2; return 0; fi
	err="$(rm -rf -- "${dir}" 2>&1)" || echo "test run folder: could not remove ${dir}: ${err}" >&2
	return 0
}

fTestDir_Use(){
	##	fTestDir_Make, then points TMPDIR at the folder, so the script's own mktemp
	##	calls and every program it starts write there.
	fTestDir_Make || return 1
	export TMPDIR="${SILKTERM_TEST_DIR}"
}

##	History:
##		- 20260930 JC: Created.
##		- 20261002 JC: A run removes the folder it made when it passes.
