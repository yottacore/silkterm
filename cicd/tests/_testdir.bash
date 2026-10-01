#!/usr/bin/env bash

##	- Purpose:
##		The run's test folder, <temp>/test_silkterm_YYYYmmDD-HHMMSSNN, that every
##		file a test writes goes under. Sourced; it defines fTestDir_Make and
##		fTestDir_Use and runs nothing. The Rust tests (source/src/testdir.rs),
##		_testdir.py and _testdir.ps1 keep the same contract:
##			- SILKTERM_TEST_DIR set and not empty: that is the folder, made if missing.
##			- Otherwise a fresh one is made, 0700, and exported as SILKTERM_TEST_DIR
##			  so every test the caller starts shares it.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

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
			return 0
		fi
		if [[ ! -e "${dir}" && ! -L "${dir}" ]]; then echo "test run folder: ${err}" >&2; return 1; fi
		sleep 0.01
	done
	echo "test run folder: '${dir}' and every name before it were taken" >&2
	return 1
}

fTestDir_Use(){
	##	fTestDir_Make, then points TMPDIR at the folder, so the script's own mktemp
	##	calls and every program it starts write there.
	fTestDir_Make || return 1
	export TMPDIR="${SILKTERM_TEST_DIR}"
}

##	History:
##		- 20260930 JC: Created.
