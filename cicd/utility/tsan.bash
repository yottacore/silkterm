#!/usr/bin/env bash

##	- Purpose:
##		- The test suite under ThreadSanitizer, as TSAN_CMD in config.bash has it,
##		  on the dated nightly that only the container image installs. cicd.bash
##		  runs it there, on every full run, after the ASan run and also the leak
##		  checks.
##		- The sanitizer canary goes first, under the same settings and
##		  suppressions. Its data race has to be reported by name, or the suite run
##		  would prove nothing.
##		- TSan reports as it goes and fails the test binary at exit, so a race
##		  fails the whole binary. The report names the stacks; run it again with
##		  --test-threads=1 and a test name to find the test.
##	- Syntax: cicd/utility/tsan.bash
##	- Exit: 0 clean; 1 a report, the canary got through, or no nightly here.
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

((${#TSAN_CMD[@]})) || { fEcho_Clean "TSan run disabled (TSAN_CMD is empty)"; exit 0; }
[[ -n "${SANITIZER_TOOLCHAIN}" ]] || fDie "no nightly named in rust-toolchain.toml"
rustup toolchain list 2>/dev/null | grep -q "^${SANITIZER_TOOLCHAIN}" \
	|| fDie "${SANITIZER_TOOLCHAIN} isn't installed here; cicd.bash runs this in the container image"
fTestDir_Make || fDie "could not make the test run folder"
fJobs_LowPriority; fJobs hard

symbolizer="$(command -v llvm-symbolizer || true)"
[[ -n "${symbolizer}" ]] || symbolizer="$(compgen -G '/usr/lib/llvm-*/bin/llvm-symbolizer' | sort -V | tail -n 1 || true)"
## Set whole, so nothing inherited turns reports off. Every report, not just the
## first, so one run lists them all.
export TSAN_OPTIONS="halt_on_error=0:second_deadlock_stack=1:suppressions=${root}/${TSAN_SUPPRESSIONS}${symbolizer:+:external_symbolizer_path=${symbolizer}}"

## fTsan_Reports <output file>: each report, under the test binary it came from,
## at most 400 lines. The cap is awk's own, as in asan.bash.
fTsan_Reports(){
	awk '
		function out(line) { if (++lines > 400) exit; print line }
		/^ +Running / { binary = $0; next }
		/^(WARNING|FATAL|ERROR): ThreadSanitizer/ { out(binary); inReport = 1 }
		inReport { out($0) }
		/^SUMMARY: ThreadSanitizer/ { inReport = 0; out("") }
	' "${1}"
}

canaryOut="${SILKTERM_TEST_DIR}/tsan_canary.txt"
fEcho_Clean "TSan canary (a planted race) ..."
rc=0; "${TSAN_CMD[@]}" --manifest-path cicd/tests/sanitizer-canary/Cargo.toml >"${canaryOut}" 2>&1 || rc=$?
if ((rc == 0)) || ! grep -q 'WARNING: ThreadSanitizer: data race' "${canaryOut}" || ! grep -q 'races_on_purpose' "${canaryOut}"; then
	tail -n 80 "${canaryOut}"
	fDie "the canary's race was not reported by name, so a race in the suite could pass too (output in ${canaryOut})"
fi
fEcho "OK: TSan canary caught (EtaER2K)"

suiteOut="${SILKTERM_TEST_DIR}/tsan_suite.txt"
fEcho_Clean "test suite under TSan (${SANITIZER_TOOLCHAIN}) ..."
start="${SECONDS}"
rc=0; "${TSAN_CMD[@]}" >"${suiteOut}" 2>&1 || rc=$?
counts="$(awk '/^test result: /{ for (i = 1; i <= NF; i++) { if ($(i) ~ /^(passed|failed|ignored);?$/) { n[$(i)] += $(i - 1) } } }
	END { printf "%d passed, %d failed, %d ignored", n["passed;"], n["failed;"], n["ignored;"] }' "${suiteOut}")"
reports="$(fTsan_Reports "${suiteOut}")"
if [[ -n "${reports}" ]]; then
	echo "${reports}"
	fDie "a TSan report (above; ${counts}). Rerun the binary with --test-threads=1 and a test name. Output in ${suiteOut}"
fi
if ((rc)); then
	tail -n 200 "${suiteOut}"
	fDie "the suite under TSan failed (${counts}), output in ${suiteOut}"
fi
fEcho "OK: no TSan reports (${counts}, $((SECONDS - start))s)"


##	History:
##		- 20261023 JC: Created.
##		- 20261026 JC: Jobs and priority from include/jobs.bash.
