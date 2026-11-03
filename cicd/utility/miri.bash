#!/usr/bin/env bash

##	- Purpose:
##		- The test suite under Miri, as MIRI_CMD in config.bash has it, on the
##		  dated nightly and its miri component, which only the container image
##		  installs. cicd.bash runs it there, after the TSan run, under
##		  --leakcheck or --long. Very slow: hours, even with the tests Miri takes
##		  over 60 s on ignored there.
##		- Miri's sysroot is built first, into the step's own dir under target/.
##		- The sanitizer canary goes next, each of its two tests alone, since a UB
##		  report stops the whole test binary. Miri has to report the race as a
##		  data race and the forgotten Vec as leaked memory, or the suite run would
##		  prove nothing.
##		- A report stops that test binary, and --no-fail-fast goes on to the
##		  next. Run the binary again with the test's name to find it.
##	- Syntax: cicd/utility/miri.bash [cargo test arguments]
##		Arguments go after MIRI_CMD, such as `-p knobs -- slider_step` to run a
##		few tests through the step.
##	- Exit: 0 clean; 1 a report, the canary got through, or no nightly with Miri here.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/../.." && pwd)"
cd "${root}"
# shellcheck source=cicd/config.bash
source "${root}/cicd/config.bash"
# shellcheck source=cicd/utility/include/echo.bash
source "${here}/include/echo.bash"
# shellcheck source=cicd/utility/include/jobs.bash
source "${here}/include/jobs.bash"
# shellcheck source=cicd/tests/_testdir.bash
source "${root}/cicd/tests/_testdir.bash"

((${#MIRI_CMD[@]})) || { fEcho_Clean "Miri run disabled (MIRI_CMD is empty)"; exit 0; }
[[ -n "${SANITIZER_TOOLCHAIN}" ]] || fDie "no nightly named in rust-toolchain.toml"
rustup toolchain list 2>/dev/null | grep -q "^${SANITIZER_TOOLCHAIN}" \
	|| fDie "${SANITIZER_TOOLCHAIN} isn't installed here; cicd.bash runs this in the container image"
cargo "+${SANITIZER_TOOLCHAIN}" miri --version >/dev/null 2>&1 \
	|| fDie "${SANITIZER_TOOLCHAIN} has no miri component here; cicd.bash runs this in the container image"
fTestDir_Make || fDie "could not make the test run folder"
fJobs_LowPriority; fJobs hard

## fMiri_Reports <output file>: each report, under the test binary it came from,
## at most 400 lines. A report runs to rustc's "aborting due to" line. cargo's
## own error lines, such as "test failed, to rerun", aren't reports.
fMiri_Reports(){
	awk '
		function out(line) { if (++lines > 400) exit; print line }
		/^ +Running / { binary = $0; next }
		/(^|\.\.\. )error: / && !/error: (test failed|aborting due to|[0-9]+ targets? failed)/ { if (!inReport) out(binary); inReport = 1 }
		inReport { out($0) }
		/^error: aborting due to/ { if (inReport) out(""); inReport = 0 }
	' "${1}"
}

setupOut="${SILKTERM_TEST_DIR}/miri_setup.txt"
fEcho_Clean "Miri sysroot ..."
env "${MIRI_ENV[@]}" cargo "+${SANITIZER_TOOLCHAIN}" miri setup >"${setupOut}" 2>&1 \
	|| { tail -n 80 "${setupOut}"; fDie "could not build Miri's sysroot (output in ${setupOut})"; }

## fMiri_Canary <id> <test> <what Miri has to say> <race|leak>: one canary test alone.
## It has to fail, with Miri's report and the test's name in its output.
fMiri_Canary(){
	local -r id="${1}" test="${2}" said="${3}" what="${4}" out="${SILKTERM_TEST_DIR}/miri_canary_${2}.txt"
	local -i rc=0
	"${MIRI_CMD[@]}" --manifest-path cicd/tests/sanitizer-canary/Cargo.toml -- --exact "tests::${test}" >"${out}" 2>&1 || rc=$?
	if ((rc == 0)) || ! grep -qF "${said}" "${out}" || ! grep -qF "tests::${test}" "${out}"; then
		tail -n 80 "${out}"
		fEcho_Clean "FAIL ${id} Miri canary ${what} caught"
		fDie "Miri didn't report the canary's ${what} as \"${said}\", so one in the suite could pass too (output in ${out})"
	fi
	fEcho_Clean "PASS ${id} Miri canary ${what} caught"
}
fEcho_Clean "Miri canary (a planted race, then a planted leak) ..."
fMiri_Canary debjk races_on_purpose "Undefined Behavior: Data race detected" race
fMiri_Canary de5f0 leaks_on_purpose "error: memory leaked:" leak

suiteOut="${SILKTERM_TEST_DIR}/miri_suite.txt"
fEcho_Clean "test suite under Miri (${SANITIZER_TOOLCHAIN}; very slow, hours) ..."
start="${SECONDS}"
rc=0; "${MIRI_CMD[@]}" "${@}" >"${suiteOut}" 2>&1 || rc=$?
counts="$(awk '/^test result: /{ for (i = 1; i <= NF; i++) { if ($(i) ~ /^(passed|failed|ignored);?$/) { n[$(i)] += $(i - 1) } } }
	END { printf "%d passed, %d failed, %d ignored", n["passed;"], n["failed;"], n["ignored;"] }' "${suiteOut}")"
reports="$(fMiri_Reports "${suiteOut}")"
if [[ -n "${reports}" ]]; then
	echo "${reports}"
	fDie "a Miri report (above; ${counts}). Rerun the binary with the test's name. Output in ${suiteOut}"
fi
if ((rc)); then
	tail -n 200 "${suiteOut}"
	fDie "the suite under Miri failed (${counts}), output in ${suiteOut}"
fi
[[ "${counts}" != "0 passed, "* ]] || fDie "no test ran under Miri (${counts}), output in ${suiteOut}"
fEcho "OK: no Miri reports (${counts}, $((SECONDS - start))s)"


##	History:
##		- 20261102: Created.
