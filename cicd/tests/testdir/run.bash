#!/usr/bin/env bash
#  shellcheck disable=2001       ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2016       ## 'Expressions don't expand in single quotes.' The stand-in cargo and the grep pattern are meant as written.
#  shellcheck disable=2030,2031  ## 'Modification is local to subshell.' Each helper run gets an environment of its own on purpose.

##	- Purpose:
##		Every file a test writes goes under one folder per run,
##		<temp>/test_silkterm_YYYYmmDD-HHMMSSNN. This runs the Rust suite and a set
##		of script tests with the temp dir read-only, so a write anywhere but the
##		run folder fails. It also checks that the Bash, Python and PowerShell
##		helpers make the same folder, that cicd.bash makes one and leaves TMPDIR
##		alone, and guards the sites it cannot run from here.
##	- Test ID: ErOj67l
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
tests="$(cd "${meDir}/.." && pwd)"
root="$(cd "${meDir}/../../.." && pwd)"
export PATH="${HOME}/.cargo/bin:${PATH}"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fShowTail(){ tail -n 25 "${1}" | sed 's/^/      /'; }

work="$(mktemp -d)"
declare -a locked=()
fEnd(){ local dir; for dir in "${locked[@]}"; do chmod 755 "${dir}"; rm -rf "${dir}"; done; rm -rf "${work}"; }
trap fEnd EXIT

stampRe='^test_silkterm_[0-9]{8}-[0-9]{8}$'
##	fLocked <base>: an empty r/ under base, then base read-only. Each base has
##	a name over 100 characters, so a Unix socket bound by its full path under
##	the run folder goes past the 107-byte limit and fails, whatever TMPDIR is.
longName="$(printf 'l%.0s' {1..100})"
fLocked(){ mkdir "${1}/r"; chmod 555 "${1}"; locked+=("${1}"); }
fOnlyRun(){ [[ "$(ls -A "${1}")" == "r" ]]; }
fInside(){ [[ -n "${1}" && "${2}" == "${1}"/* ]]; }  ## fInside <folder> <path>
fStamped(){ [[ "$(basename "${1}")" =~ ${stampRe} ]]; }
##	fOneStamped <base>: exactly one entry, a 0700 folder named for the time.
fOneStamped(){
	local -a entries=()
	mapfile -t entries < <(ls -A "${1}")
	((${#entries[@]} == 1)) || return 1
	[[ "${entries[0]}" =~ ${stampRe} && -d "${1}/${entries[0]}" && ! -L "${1}/${entries[0]}" ]] || return 1
	[[ "$(stat -c %a "${1}/${entries[0]}")" == 700 ]]
}

##	A: the Rust suite, with the temp dir read-only. Built first with the normal
##	environment, so rustc and the linker never write there.
if ! ( cd "${root}" && cargo test --no-run ) >"${work}/build.log" 2>&1; then fShowTail "${work}/build.log"; echo "the test build failed"; exit 1; fi
baseA="$(mktemp -d "${TMPDIR}/${longName}XXX")"; fLocked "${baseA}"
rc=0; ( cd "${root}" && SILKTERM_TEST_DIR="${baseA}/r" TMPDIR="${baseA}" cargo test ) >"${work}/a.log" 2>&1 || rc=$?
fCheck "the Rust suite passes with the temp dir read-only" test "${rc}" -eq 0
((rc == 0)) || grep -E 'FAILED|panicked' "${work}/a.log" | head -n 20 | sed 's/^/      /'
fCheck "and writes nothing beside the run folder" fOnlyRun "${baseA}"

##	A2: a lone run, with nothing handed down.
mkdir "${work}/a2"
rc=0; ( cd "${root}" && env -u SILKTERM_TEST_DIR TMPDIR="${work}/a2" cargo test testdir:: ) >"${work}/a2.log" 2>&1 || rc=$?
fCheck "a lone Rust run passes the run folder's own tests" grep -q 'test result: ok. 5 passed' "${work}/a2.log"
fCheck "and makes exactly one 0700 folder in the temp dir, named for the time" fOneStamped "${work}/a2"

##	C: script tests, the same way. The rest need a display, the remote box, or
##	minutes of installer runs, and are guarded by E instead.
baseC="$(mktemp -d "${TMPDIR}/${longName}XXX")"; fLocked "${baseC}"
fScript(){  ## fScript <label> <command ...>
	local -r label="${1}"; shift
	local -r log="${work}/c-${label//\//-}.log"
	local rc=0
	( cd "${root}" && env -u DISPLAY -u WAYLAND_DISPLAY SILKTERM_TEST_DIR="${baseC}/r" TMPDIR="${baseC}" "${@}" ) >"${log}" 2>&1 || rc=$?
	fCheck "${label} passes with the temp dir read-only" test "${rc}" -eq 0
	((rc == 0)) || fShowTail "${log}"
}
for name in engine gates hooks rotate sync publish rename pins; do fScript "${name}/run.bash" bash "${tests}/${name}/run.bash"; done
fScript showdown/rigs.py python3 "${tests}/showdown/rigs.py"
fScript showdown/run.py python3 "${tests}/showdown/run.py"
if command -v pwsh >/dev/null; then
	fScript launcher/run.ps1 pwsh -NoProfile -NonInteractive -File "${tests}/launcher/run.ps1"
else
	echo "  skip launcher/run.ps1: no pwsh here"
fi
fCheck "and none of them writes beside the run folder" fOnlyRun "${baseC}"

##	D: the helpers agree. fHelper <lang> prints the run folder, then what the
##	language's own temp call answers once the helper has run.
fHelper(){
	case "${1}" in
		bash)   bash -c 'set -euo pipefail; source "${1}"; fTestDir_Use; echo "${SILKTERM_TEST_DIR}"; mktemp -d' _ "${tests}/_testdir.bash" ;;
		python) python3 -c 'import os, sys, tempfile; sys.path.insert(0, sys.argv[1]); import _testdir; _testdir.use(); print(os.environ["SILKTERM_TEST_DIR"]); print(tempfile.mkdtemp())' "${tests}" ;;
		pwsh)   pwsh -NoProfile -NonInteractive -Command ". '${tests}/_testdir.ps1'; fTestDir_Use; \$env:SILKTERM_TEST_DIR; [System.IO.Path]::GetTempPath()" ;;
	esac
}
declare -a langs=(bash python)
if command -v pwsh >/dev/null; then langs+=(pwsh); else echo "  skip the PowerShell helper: no pwsh here"; fi
mkdir "${work}/d"
for lang in "${langs[@]}"; do
	base="${work}/d/${lang}"; mkdir "${base}"
	out="$(unset SILKTERM_TEST_DIR; export TMPDIR="${base}"; fHelper "${lang}" 2>&1 || true)"
	made="$(sed -n 1p <<<"${out}")"; answer="$(sed -n 2p <<<"${out}")"
	fCheck "the ${lang} helper makes exactly one 0700 folder named for the time" fOneStamped "${base}"
	fCheck "and exports it" test "$(dirname "${made}")" == "${base}"
	fCheck "and the temp dir then points inside it" fInside "${made}" "${answer}"
	base="${work}/d/${lang}-given-base"; mkdir "${base}"
	out="$(export SILKTERM_TEST_DIR="${work}/d/given" TMPDIR="${base}"; fHelper "${lang}" 2>&1 || true)"
	made="$(sed -n 1p <<<"${out}")"; answer="$(sed -n 2p <<<"${out}")"
	fCheck "given a folder, the ${lang} helper uses it" test "${made}" == "${work}/d/given" -a -d "${work}/d/given"
	fCheck "and the temp dir points inside it" fInside "${made}" "${answer}"
	fCheck "and nothing else is made" test -z "$(ls -A "${base}")"
done

##	E: guards for the sites A and C cannot run, such as cfg(windows) tests and
##	the display-bound scripts. The list is the one the test ID check uses, plus
##	the wingui driver. The other wingui scripts run on the remote box, which
##	has an item of its own.
mapfile -t scripts < <(python3 -c '
import importlib.util, sys
spec = importlib.util.spec_from_file_location("test_id", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
for path in mod.fScriptTests():
	print(path.relative_to(mod.ROOT / "cicd" / "tests").as_posix())
' "${root}/cicd/utility/test-id.py")
fCheck "the test ID check lists the script tests" test "${#scripts[@]}" -gt 10
scripts+=(wingui/run.bash)
skipped=""
for rel in "${scripts[@]}"; do
	[[ "${rel}" == wingui/* && "${rel}" != wingui/harness-test.bash && "${rel}" != wingui/run.bash ]] && continue
	grep -qE 'mktemp|tempfile|GetTempPath|\$env:TEMP' "${tests}/${rel}" || continue
	grep -qE 'fTestDir_Use|_testdir\.use\(\)' "${tests}/${rel}" || skipped+=" ${rel}"
done
fCheck "every script test that makes scratch uses the run folder${skipped:+ (not:${skipped})}" test -z "${skipped}"
stray="$(git -C "${root}" grep -n 'temp_dir' -- 'source/src/*.rs' \
	| grep -v '^source/src/testdir\.rs:' \
	| grep -v -F 'var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from)' || true)"
fCheck "no Rust code but the run folder and the control socket asks for the temp dir" test -z "${stray}"
[[ -z "${stray}" ]] || sed 's/^/      /' <<<"${stray}"
boxSpecific="$(git -C "${root}" grep -n '/m''nt/' -- cicd source utility || true)"
fCheck "nothing box-specific is in the repo" test -z "${boxSpecific}"

##	F: cicd.bash makes the run folder before its gate, and leaves TMPDIR alone.
##	A stand-in cargo records what the test step is handed.
home="${work}/f/home"; mkdir -p "${home}/.cargo/bin" "${work}/f/base"
printf '%s\n' '#!/usr/bin/env bash' \
	'if [[ "${1:-}" == test ]]; then printf "%s\n%s\n" "${SILKTERM_TEST_DIR:-}" "${TMPDIR:-}" >"${STUB_RECORD}"; fi' \
	'exit 0' >"${home}/.cargo/bin/cargo"
chmod +x "${home}/.cargo/bin/cargo"
rc=0; env -u SILKTERM_TEST_DIR TMPDIR="${work}/f/base" HOME="${home}" STUB_RECORD="${work}/f/record" \
	bash "${root}/cicd/cicd.bash" --gate >"${work}/f.log" 2>&1 || rc=$?
fCheck "the gate runs with a stand-in cargo" test "${rc}" -eq 0
((rc == 0)) || fShowTail "${work}/f.log"
given="$(sed -n 1p "${work}/f/record" 2>/dev/null || true)"
fCheck "and hands its tests a run folder it made in the temp dir" \
	test "$(dirname "${given}")" == "${work}/f/base" -a -d "${given}"
fCheck "named for the time" fStamped "${given}"
fCheck "and leaves TMPDIR as it was" test "$(sed -n 2p "${work}/f/record" 2>/dev/null || true)" == "${work}/f/base"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260930 JC: Created.
