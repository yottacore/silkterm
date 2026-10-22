#!/usr/bin/env bash

##	- Purpose:
##		Build, lint and test on a Mac, using nothing specific to a git host. A hosted
##		runner calls this, and so can anyone on a Mac by hand. It runs every step
##		and reports them all, so one failure doesn't hide the next.
##			- What the Mac is: architecture, macOS version, and the graphics
##			  the machine reports.
##			- cargo build, clippy with -D warnings, and the unit suite, all on the
##			  toolchain rust-toolchain.toml pins.
##			- The graphics adapter the app finds, and which tests skipped for want
##			  of a device. A VM with no GPU skips the window tests.
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

## Apple's /bin/bash is 3.2, and the prompt tests expand ${PS1@P}, which needs
## 4.4. A developer's Mac has Homebrew's bash first on PATH, so this does the same.
fBashOk(){
	local v
	## The candidate bash expands this, not this one.
	# shellcheck disable=SC2016
	v="$("${1}" -c 'echo $((BASH_VERSINFO[0] * 10 + BASH_VERSINFO[1]))' 2>/dev/null)" || return 1
	[[ "${v}" =~ ^[0-9]+$ ]] && ((v >= 44))
}

fBash(){
	local dir
	if fBashOk bash; then bash --version | head -n 1; return 0; fi
	if ! fBashOk /opt/homebrew/bin/bash && ! fBashOk /usr/local/bin/bash; then
		command -v brew >/dev/null 2>&1 || { echo "no bash 4.4 or newer, and no Homebrew"; return 1; }
		HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install bash || return 1
	fi
	for dir in /opt/homebrew/bin /usr/local/bin; do
		if fBashOk "${dir}/bash"; then export PATH="${dir}:${PATH}"; bash --version | head -n 1; return 0; fi
	done
	return 1
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
	if ((rc)); then fWhyFailed; fi
	return "${rc}"
}

## --nocapture leaves no failure section at the end. The panic text is in the
## log, between the other tests' output, so it's pulled out first. Then the
## failed ones run again alone. A test that passes the second time is a flaky
## one, so that run keeps its output too.
fWhyFailed(){
	local name
	local names=()
	while IFS= read -r name; do names+=("${name}"); done < <(sed -n 's/^test \(.*\) \.\.\. FAILED$/\1/p' "${logDir}/test.log")
	if ((${#names[@]} == 0)); then return 0; fi
	echo "--- failed tests, as they failed"
	awk '/^thread .* panicked at /{n=12} n&&/^(note: |test |$)/{n=0} n{print; n--}' "${logDir}/test.log" | head -n 120
	echo "--- failed tests, run again alone"
	cargo test --locked -- --test-threads=1 --nocapture "${names[@]}" 2>&1 | grep -vE '^\s*(Compiling|Finished|Running)' | head -n 200
}

## What --about reports is the adapter the app would draw with. No Renderer line
## means none was found, which is an answer and not a failure.
fAdapter(){
	local said
	said="$(cargo run --quiet --locked -- --about 2>&1)" || { echo "${said}"; return 1; }
	echo "${said}" | grep -E 'Renderer:|Backend:|Acceleration:' || echo "no adapter"
}

fSkips(){
	local count
	count="$(grep -c 'skipped:' "${logDir}/test.log" 2>/dev/null || true)"
	echo "tests that skipped: ${count:-0}"
	grep 'skipped:' "${logDir}/test.log" | sort | uniq -c | sort -rn | head -n 20
	return 0
}

## A timing test prints what it measured as "measured: ...", pass or fail, so
## each run shows how close to its limit this machine sits.
fMeasured(){
	grep -o 'measured:.*' "${logDir}/test.log" 2>/dev/null | head -n 20
	return 0
}

fStep "machine" fMachine
fStep "bash" fBash
fStep "toolchain" fToolchain
fStep "build" fBuild
fStep "clippy" fClippy
fStep "test" fTest
fStep "adapter" fAdapter
fStep "skips" fSkips
fStep "measured" fMeasured

if [[ -n "${failed}" ]]; then echo "FAILED:${failed}"; exit 1; fi
echo "all passed"

##	History:
##		- 20261021 JC: Created.
##		- 20261022 JC: Failure text from the first run, and what timing tests measured.
