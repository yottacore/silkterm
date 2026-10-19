#!/usr/bin/env bash

##	- Purpose:
##		- The test suite under AddressSanitizer and LeakSanitizer, as ASAN_CMD in
##		  config.bash has it, on the dated nightly that only the container image
##		  installs. cicd.bash runs it there, on every full run.
##		- A canary crate that leaks on purpose goes first, under the same settings
##		  and suppressions. If its leak isn't reported by name, the suite run would
##		  prove nothing, so that stops it.
##		- LSan reports at exit, so a leak fails a whole test binary. The report says
##		  which one; run it again with --test-threads=1 and a test name to find the
##		  test.
##	- Syntax: cicd/utility/asan.bash
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
# shellcheck source=cicd/tests/_testdir.bash
source "${root}/cicd/tests/_testdir.bash"

((${#ASAN_CMD[@]})) || { fEcho_Clean "ASan run disabled (ASAN_CMD is empty)"; exit 0; }
[[ -n "${SANITIZER_TOOLCHAIN}" ]] || fDie "no nightly named in rust-toolchain.toml"
rustup toolchain list 2>/dev/null | grep -q "^${SANITIZER_TOOLCHAIN}" \
	|| fDie "${SANITIZER_TOOLCHAIN} isn't installed here; cicd.bash runs this in the container image"
fTestDir_Make || fDie "could not make the test run folder"
## Half the cores, as cicd.bash caps them. The image run only gets CICD_MAX_JOBS off of it.
: "${CICD_MAX_JOBS:=$(( $(nproc 2>/dev/null || echo 2) / 2 ))}"
((CICD_MAX_JOBS >= 1)) || CICD_MAX_JOBS=1
export CARGO_BUILD_JOBS="${CICD_MAX_JOBS}" RUST_TEST_THREADS="${CICD_MAX_JOBS}"

## Frames have to be named, or a suppression by function name matches nothing and
## a report reads as bare addresses. Debian ships it versioned only.
symbolizer="$(command -v llvm-symbolizer || true)"
[[ -n "${symbolizer}" ]] || symbolizer="$(compgen -G '/usr/lib/llvm-*/bin/llvm-symbolizer' | sort -V | tail -n 1 || true)"
## Set whole, so nothing inherited can turn the leak check off. Rust's own
## allocation frames take 15 or so, so the default 30 often stops short of ours.
export ASAN_OPTIONS="detect_leaks=1:malloc_context_size=64${symbolizer:+:external_symbolizer_path=${symbolizer}}"
export LSAN_OPTIONS="suppressions=${root}/${LSAN_SUPPRESSIONS}:print_suppressions=0"

## fAsan_Reports <output file>: each sanitizer report, under the test binary it
## came from, at most 400 lines. The cap is awk's own, since a head after it
## would end the pipe early and pipefail with it.
fAsan_Reports(){
	awk '
		function out(line) { if (++lines > 400) exit; print line }
		/^ +Running / { binary = $0; next }
		/^==[0-9]+==ERROR: / { out(binary); inReport = 1 }
		inReport { out($0) }
		/^SUMMARY: (Address|Leak)Sanitizer/ { inReport = 0; out("") }
	' "${1}"
}

canaryOut="${SILKTERM_TEST_DIR}/asan_canary.txt"
fEcho_Clean "ASan canary (a planted leak) ..."
rc=0; "${ASAN_CMD[@]}" --manifest-path cicd/tests/sanitizer-canary/Cargo.toml >"${canaryOut}" 2>&1 || rc=$?
if ((rc == 0)) || ! grep -q 'ERROR: LeakSanitizer: detected memory leaks' "${canaryOut}" || ! grep -q 'leaks_on_purpose' "${canaryOut}"; then
	tail -n 80 "${canaryOut}"
	fDie "the canary's leak was not reported by name, so a leak in the suite could pass too (output in ${canaryOut})"
fi
fEcho "OK: ASan canary caught (EtAocf6)"

suiteOut="${SILKTERM_TEST_DIR}/asan_suite.txt"
fEcho_Clean "test suite under ASan and LSan (${SANITIZER_TOOLCHAIN}) ..."
start="${SECONDS}"
rc=0; "${ASAN_CMD[@]}" >"${suiteOut}" 2>&1 || rc=$?
counts="$(awk '/^test result: /{ for (i = 1; i <= NF; i++) { if ($(i) ~ /^(passed|failed|ignored);?$/) { n[$(i)] += $(i - 1) } } }
	END { printf "%d passed, %d failed, %d ignored", n["passed;"], n["failed;"], n["ignored;"] }' "${suiteOut}")"
if ((rc)); then
	reports="$(fAsan_Reports "${suiteOut}")"
	if [[ -n "${reports}" ]]; then
		echo "${reports}"
		fDie "a sanitizer report (above; ${counts}). LSan fails the whole binary, so rerun it with --test-threads=1 and a test name. Output in ${suiteOut}"
	fi
	tail -n 200 "${suiteOut}"
	fDie "the suite under ASan failed (${counts}), output in ${suiteOut}"
fi
fEcho "OK: no ASan or LSan reports (${counts}, $((SECONDS - start))s)"


##	History:
##		- 20261019 JC: Created.
