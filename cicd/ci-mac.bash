#!/usr/bin/env bash

##	- Purpose:
##		Build, lint and test on a Mac, with nothing from any git host. A hosted
##		runner calls this, and so can anyone on a Mac by hand. It runs every step
##		and reports them all, so one failure doesn't hide the next.
##			- What the Mac is: architecture, macOS version, and the graphics
##			  the machine reports.
##			- cargo build, clippy with -D warnings, and the unit suite, all on the
##			  toolchain rust-toolchain.toml pins.
##			- Which tests skipped for want of a graphics device. A VM with no GPU
##			  skips the window tests, and the skip lines are the evidence.
##	- Syntax: ci-mac.bash
##	- Exit: 0 every step passed, 1 a step failed, 3 not a Mac.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -uo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}" || exit 1

if [[ "$(uname -s)" != "Darwin" ]]; then echo "skip: not a Mac"; exit 3; fi

logDir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/silkterm-ci-mac.$$"
mkdir -p "${logDir}"

failed=""
fStep(){
	local -r name="${1}"; shift
	local -r started="${SECONDS}"
	echo "=== ${name}"
	if "${@}"; then echo "=== ok: ${name} ($((SECONDS - started)) s)"
	else echo "=== FAIL: ${name} ($((SECONDS - started)) s)"; failed="${failed} ${name}"; fi
}

fMachine(){
	uname -m
	sw_vers
	sysctl -n machdep.cpu.brand_string hw.ncpu hw.memsize 2>/dev/null || true
	## Not every VM answers this, and an empty answer is data too.
	system_profiler SPDisplaysDataType 2>&1 | head -n 30 || true
	return 0
}

## rustup 1.28 stopped installing the pinned toolchain by itself. The file also
## lists the cross targets the Linux box lints, which a Mac run doesn't need, so
## the channel is read from it and installed bare.
fToolchain(){
	local channel
	channel="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' rust-toolchain.toml | head -n 1)"
	if [[ -z "${channel}" ]]; then echo "no channel in rust-toolchain.toml"; return 1; fi
	rustup toolchain install "${channel}" --profile minimal --component clippy --no-self-update || return 1
	export RUSTUP_TOOLCHAIN="${channel}"
	rustc --version && cargo --version && cargo clippy --version
}

fBuild(){ cargo build --locked; }
fClippy(){ cargo clippy --workspace --all-targets --locked -- -D warnings; }

## --nocapture so a test that skips can say why. The log is long, so only the
## result lines and the skips are printed.
fTest(){
	local rc=0
	cargo test --locked -- --nocapture > "${logDir}/test.log" 2>&1 || rc=${?}
	grep -E '^test result:|^error|FAILED' "${logDir}/test.log" | head -n 60
	return "${rc}"
}

fSkips(){
	local count
	count="$(grep -c 'skipped:' "${logDir}/test.log" 2>/dev/null || true)"
	echo "tests that skipped: ${count:-0}"
	grep 'skipped:' "${logDir}/test.log" | sort | uniq -c | sort -rn | head -n 20
	return 0
}

fStep "machine" fMachine
fStep "toolchain" fToolchain
fStep "build" fBuild
fStep "clippy" fClippy
fStep "test" fTest
fStep "skips" fSkips

if [[ -n "${failed}" ]]; then echo "FAILED:${failed}"; exit 1; fi
echo "all passed"

##	History:
##		- 20261021 JC: Created.
