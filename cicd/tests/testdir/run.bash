#!/usr/bin/env bash
#  shellcheck disable=2001       ## 'See if you can use ${variable//search/replace} instead.' Complains about good uses of sed.
#  shellcheck disable=2016       ## 'Expressions don't expand in single quotes.' The stand-in cargo and the grep pattern are meant as written.
#  shellcheck disable=2030,2031  ## 'Modification is local to subshell.' Each helper run gets an environment of its own on purpose.

##	- Purpose:
##		Every file a test writes goes under one folder per run,
##		HOST_TEMP/silkterm/YYYYmmDD-HHMMSS_NNN. This runs the Rust suite and a set
##		of script tests with the temp dir read-only, so a write anywhere but the
##		run folder fails. It also checks that the Bash, Python, and PowerShell
##		helpers make the same folder, that cicd.bash makes its own in
##		HOST_TEMP/silkterm with TMPDIR pointed at it and removes it at the end,
##		and guards the sites it can't run from here. A run removes the
##		folder it made when it passes, keeps it when it fails, and never removes
##		one it was handed, one without its mark, or anything a link points to.
##	- Test ID: ddazx
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
fEnd(){ local -r rc=$?; local dir; for dir in "${locked[@]}"; do chmod 755 "${dir}"; fTestDir_Rm "${dir}" || true; done; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"; }
trap fEnd EXIT

stampRe='^[0-9]{8}-[0-9]{6}_[0-9]{3}$'
##	fLocked <base>: an empty r/ under base, then base read-only. Each base has
##	a name over 100 characters, so a Unix socket bound by its full path under
##	the run folder goes past the 107-byte limit and fails, whatever TMPDIR is.
longName="$(printf 'l%.0s' {1..100})"
fLocked(){ mkdir "${1}/r"; chmod 555 "${1}"; locked+=("${1}"); }
fOnlyRun(){ [[ "$(ls -A "${1}")" == "r" ]]; }
fInside(){ [[ -n "${1}" && "${2}" == "${1}"/* ]]; }  ## fInside <folder> <path>
fStamped(){ [[ "$(basename "${1}")" =~ ${stampRe} ]]; }
##	fOneStamped <base>: exactly one entry in base/silkterm, a 0700 folder named
##	for the time, with a counter, and a PID in its run.pid.
fOneStamped(){
	local -a entries=()
	local -r p="${1}/silkterm"
	[[ "$(ls -A "${1}")" == silkterm ]] || return 1
	mapfile -t entries < <(ls -A "${p}")
	((${#entries[@]} == 1)) || return 1
	[[ "${entries[0]}" =~ ${stampRe} && -d "${p}/${entries[0]}" && ! -L "${p}/${entries[0]}" ]] || return 1
	[[ "$(stat -c %a "${p}/${entries[0]}")" == 700 && "$(cat "${p}/${entries[0]}/run.pid")" =~ ^[0-9]+$ ]]
}

##	A: the Rust suite, with the temp dir read-only. Built first with the normal
##	environment, so rustc and the linker never write there.
if ! ( cd "${root}" && cargo test --no-run ) >"${work}/build.log" 2>&1; then fShowTail "${work}/build.log"; echo "the test build failed"; exit 1; fi
baseA="$(mktemp -d "${TMPDIR}/${longName}XXX")"; fLocked "${baseA}"
rc=0; ( cd "${root}" && SILKTERM_TEST_DIR="${baseA}/r" TMPDIR="${baseA}" TEMP="${baseA}" TMP="${baseA}" cargo test ) >"${work}/a.log" 2>&1 || rc=$?
fCheck "the Rust suite passes with the temp dir read-only" test "${rc}" -eq 0
((rc == 0)) || grep -E 'FAILED|panicked' "${work}/a.log" | head -n 20 | sed 's/^/      /'
fCheck "and writes nothing beside the run folder" fOnlyRun "${baseA}"

##	A2: a lone run, with nothing handed down, removes its folder when it passes,
##	even though one of the run folder's own tests panics on purpose.
##	HOST_TEMP is unset, so the temp dir stands in.
fEmpty(){ [[ -d "${1}" && -z "$(ls -A "${1}")" ]]; }
##	fNoneLeft <base>: nothing in base but the project's folder, empty.
fNoneLeft(){ [[ "$(ls -A "${1}")" == silkterm ]] && fEmpty "${1}/silkterm"; }
mkdir "${work}/a2"
rc=0; ( cd "${root}" && env -u SILKTERM_TEST_DIR -u HOST_TEMP TMPDIR="${work}/a2" TEMP="${work}/a2" TMP="${work}/a2" cargo test testdir:: ) >"${work}/a2.log" 2>&1 || rc=$?
fCheck "a lone Rust run passes the run folder's own tests" bash -c '(($1 == 0)) && grep -qE "test result: ok\. [0-9]+ passed; 0 failed" "$2"' _ "${rc}" "${work}/a2.log"
fCheck "and leaves nothing in the temp dir but the project's folder" fNoneLeft "${work}/a2"

##	A3: a lone run that fails keeps its folder, with the failing test's file in
##	it. Under HOST_TEMP when that's set, and nothing in the temp dir.
mkdir "${work}/a3" "${work}/a3-tmp"
rc=0; ( cd "${root}" && env -u SILKTERM_TEST_DIR SILKTERM_TEST_FAIL_ON_PURPOSE=1 HOST_TEMP="${work}/a3" TMPDIR="${work}/a3-tmp" TEMP="${work}/a3-tmp" TMP="${work}/a3-tmp" \
	cargo test testdir::tests::a_run_that_fails_keeps_its_folder ) >"${work}/a3.log" 2>&1 || rc=$?
kept="$(find "${work}/a3/silkterm" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null || true)"
fCheck "a lone Rust run that fails exits 101" test "${rc}" -eq 101
fCheck "and keeps exactly one 0700 folder in HOST_TEMP/silkterm, named for the time, with a run.pid" fOneStamped "${work}/a3"
fCheck "and nothing in the temp dir" fEmpty "${work}/a3-tmp"
fCheck "with the failing test's file still in it" test -f "${kept}/kept"
fCheck "and names it" grep -qxF "test files kept in ${kept}" "${work}/a3.log"

##	C: script tests, the same way. The rest need a display, the remote box, or
##	minutes of installer runs, and are guarded by E instead.
baseC="$(mktemp -d "${TMPDIR}/${longName}XXX")"; fLocked "${baseC}"
fScript(){  ## fScript <label> <command ...>
	local -r label="${1}"; shift
	local -r log="${work}/c-${label//\//-}.log"
	local rc=0
	( cd "${root}" && env -u DISPLAY -u WAYLAND_DISPLAY SILKTERM_TEST_DIR="${baseC}/r" TMPDIR="${baseC}" TEMP="${baseC}" TMP="${baseC}" "${@}" ) >"${log}" 2>&1 || rc=$?
	fCheck "${label} passes with the temp dir read-only" test "${rc}" -eq 0
	((rc == 0)) || fShowTail "${log}"
}
fEachScript(){  ## fEachScript <fScript or fAlone>
	local name
	for name in engine gates hooks rotate sync publish rename pins; do "${1}" "${name}/run.bash" bash "${tests}/${name}/run.bash"; done
	"${1}" showdown/rigs.py python3 "${tests}/showdown/rigs.py"
	"${1}" showdown/run.py python3 "${tests}/showdown/run.py"
	if command -v pwsh >/dev/null; then
		"${1}" launcher/run.ps1 pwsh -NoProfile -NonInteractive -File "${tests}/launcher/run.ps1"
	else
		echo "  skip launcher/run.ps1: no pwsh here"
	fi
}
fEachScript fScript
fCheck "and none of them writes beside the run folder" fOnlyRun "${baseC}"

##	C2: the same scripts alone, each with a temp dir of its own and nothing
##	handed down. Each removes the run folder it made when it passes.
fAlone(){  ## fAlone <label> <command ...>
	local -r label="${1}"; shift
	local -r base="${work}/c2/${label//\//-}" log="${work}/c2-${label//\//-}.log"
	local rc=0
	mkdir -p "${base}"
	( cd "${root}" && env -u DISPLAY -u WAYLAND_DISPLAY -u SILKTERM_TEST_DIR -u HOST_TEMP TMPDIR="${base}" TEMP="${base}" TMP="${base}" "${@}" ) >"${log}" 2>&1 || rc=$?
	fCheck "${label} alone passes and leaves nothing in the temp dir but the project's folder" bash -c '(($1 == 0)) && [[ "$(ls -A "$2")" == silkterm && -z "$(ls -A "$2/silkterm")" ]]' _ "${rc}" "${base}"
	if ((rc)) || ! fNoneLeft "${base}"; then fShowTail "${log}"; fi
}
fEachScript fAlone

##	D: the helpers agree. fHelper <lang> prints the run folder, then what the
##	language's own temp call answers once the helper has run. The Bash child
##	removes its folder when it exits, so it also prints the base's entry count,
##	the folder's mode, and whether it is a real folder, while it still runs.
fHelper(){
	case "${1}" in
		bash)   bash -c 'set -euo pipefail; source "${1}"; fTestDir_Use; echo "${SILKTERM_TEST_DIR}"; mktemp -d
			dir="${SILKTERM_TEST_DIR}"; real=""; [[ -d "${dir}" && ! -L "${dir}" ]] && real=dir
			pid=""; read -r pid <"${dir}/run.pid"; [[ "${pid}" == "$$" ]] && pid=pid
			echo "$(ls -A "$(dirname "${dir}")" | wc -l) $(stat -c %a "${dir}") ${real} ${pid}"' _ "${tests}/_testdir.bash" ;;
		python) python3 -c 'import os, sys, tempfile; sys.path.insert(0, sys.argv[1]); import _testdir; _testdir.use(); print(os.environ["SILKTERM_TEST_DIR"]); print(tempfile.mkdtemp())' "${tests}" ;;
		pwsh)   pwsh -NoProfile -NonInteractive -Command ". '${tests}/_testdir.ps1'; fTestDir_Use; \$env:SILKTERM_TEST_DIR; [System.IO.Path]::GetTempPath()" ;;
	esac
}
declare -a langs=(bash python)
if command -v pwsh >/dev/null; then langs+=(pwsh); else echo "  skip the PowerShell helper: no pwsh here"; fi
mkdir "${work}/d"
for lang in "${langs[@]}"; do
	base="${work}/d/${lang}"; mkdir "${base}"
	out="$(unset SILKTERM_TEST_DIR HOST_TEMP; export TMPDIR="${base}" TEMP="${base}" TMP="${base}"; fHelper "${lang}" 2>&1 || true)"
	made="$(sed -n 1p <<<"${out}")"; answer="$(sed -n 2p <<<"${out}")"
	if [[ "${lang}" == bash ]]; then
		fCheck "the ${lang} helper makes exactly one 0700 folder named for the time, with its PID in run.pid" \
			bash -c '[[ "$1" == "1 700 dir pid" && "$(basename "$2")" =~ $3 ]]' _ "$(sed -n 3p <<<"${out}")" "${made}" "${stampRe}"
		fCheck "and removes it when the child exits with 0" fNoneLeft "${base}"
	else
		fCheck "the ${lang} helper makes exactly one 0700 folder named for the time, with a run.pid" fOneStamped "${base}"
	fi
	fCheck "and exports it, in the project's folder in the temp dir" test "$(dirname "${made}")" == "${base}/silkterm"
	fCheck "and the temp dir then points inside it" fInside "${made}" "${answer}"
	##	HOST_TEMP set: the folder goes there, and not in the temp dir.
	base="${work}/d/${lang}-host"; mkdir "${base}" "${base}-tmp"
	out="$(unset SILKTERM_TEST_DIR; export HOST_TEMP="${base}" TMPDIR="${base}-tmp" TEMP="${base}-tmp" TMP="${base}-tmp"; fHelper "${lang}" 2>&1 || true)"
	made="$(sed -n 1p <<<"${out}")"
	fCheck "with HOST_TEMP set, the ${lang} helper makes it in HOST_TEMP/silkterm" test "$(dirname "${made}")" == "${base}/silkterm"
	fCheck "and nothing in the temp dir" fEmpty "${base}-tmp"
	base="${work}/d/${lang}-given-base"; mkdir "${base}"
	out="$(export SILKTERM_TEST_DIR="${work}/d/given" HOST_TEMP="${base}" TMPDIR="${base}" TEMP="${base}" TMP="${base}"; fHelper "${lang}" 2>&1 || true)"
	made="$(sed -n 1p <<<"${out}")"; answer="$(sed -n 2p <<<"${out}")"
	fCheck "given a folder, the ${lang} helper uses it" test "${made}" == "${work}/d/given" -a -d "${work}/d/given"
	fCheck "and the temp dir points inside it" fInside "${made}" "${answer}"
	fCheck "and nothing else is made" test -z "$(ls -A "${base}")"
done

##	E: guards for the sites A and C can't run, such as cfg(windows) tests and
##	the display-bound scripts. The list is the one the test ID check uses, plus
##	the wingui driver. The other wingui scripts run on the remote box, which
##	has an item of its own.
mapfile -t scripts < <(python3 -c '
import importlib.util, sys
spec = importlib.util.spec_from_file_location("test_id", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
for path in mod.script_tests():
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
##	A guard for what C2 can't run. The pipelines are on the list too.
unended=""
for rel in "${scripts[@]}" ../cicd.bash ../cicd-win.ps1; do
	[[ "${rel}" == wingui/* && "${rel}" != wingui/harness-test.bash && "${rel}" != wingui/run.bash ]] && continue
	grep -qE 'fTestDir_(Use|Make)|_testdir\.use\(\)' "${tests}/${rel}" || continue
	grep -qE 'fTestDir_End|_testdir\.end\(' "${tests}/${rel}" || unended+=" ${rel}"
done
fCheck "and every one that makes a run folder ends it${unended:+ (not:${unended})}" test -z "${unended}"
##	A socket bound under a run folder in a long temp dir is past the 107-byte
##	limit, so sway never comes up and the control socket never answers. A script
##	that starts sway or talks to that socket moves its runtime dir to a short
##	folder when the path is long.
unshortened=""
for rel in "${scripts[@]}"; do
	## This file names the patterns, so it matches them.
	[[ "${rel}" == wingui/* || "${rel}" == testdir/run.bash ]] && continue
	grep -qE 'sway -c|"sway", "-c"|SILKTERM_SOCKET=|--reload-settings' "${tests}/${rel}" || continue
	grep -qE '\(\(\$\{#runDir\} > 70\)\)|len\(str\(run\)\) > 70' "${tests}/${rel}" || unshortened+=" ${rel}"
done
fCheck "and every one that binds sockets there keeps them short${unshortened:+ (not:${unshortened})}" test -z "${unshortened}"
stray="$(git -C "${root}" grep -n 'temp_dir' -- 'source/src/*.rs' \
	| grep -v '^source/src/testdir\.rs:' \
	| grep -v -E '^source/src/peers\.rs:[0-9]+:[[:space:]]*\.map_or_else\(std::env::temp_dir, PathBuf::from\)$' || true)"
fCheck "no Rust code but the run folder and the control socket asks for the temp dir" test -z "${stray}"
[[ -z "${stray}" ]] || sed 's/^/      /' <<<"${stray}"
boxSpecific="$(git -C "${root}" grep -n '/m''nt/' -- cicd source utility || true)"
fCheck "nothing box-specific is in the repo" test -z "${boxSpecific}"

##	F: cicd.bash makes its run folder before its gate, in HOST_TEMP/silkterm,
##	points TMPDIR, TEMP, and TMP at it, and removes it at the end, failed or not.
##	A stand-in cargo records what the test step is handed, whether the folder
##	was there then, and its run.pid. HOST_TEMP is unset, so TMPDIR stands in.
home="${work}/f/home"; mkdir -p "${home}/.cargo/bin" "${work}/f/base"
runRe='^[0-9]{8}-[0-9]{6}_[0-9]{3}$'
printf '%s\n' '#!/usr/bin/env bash' \
	'if [[ "${1:-}" == test ]]; then' \
	'	there=""; [[ -d "${SILKTERM_TEST_DIR:-}" ]] && there=there' \
	'	pid=""; read -r pid <"${SILKTERM_TEST_DIR}/run.pid" || true' \
	'	printf "%s\n%s\n%s\n%s\n%s\n" "${SILKTERM_TEST_DIR:-}" "${TMPDIR:-}" "${there}" "${TEMP:-}|${TMP:-}" "${pid}" >"${STUB_RECORD}"' \
	'	exit "${STUB_TEST_RC:-0}"' \
	'fi' \
	'exit 0' >"${home}/.cargo/bin/cargo"
chmod +x "${home}/.cargo/bin/cargo"
fGate(){  ## fGate <name> [env...] -- [options...]: the gate, from a base of its own; sets rc and given
	local -r name="${1}"; shift
	local -a envs=()
	while [[ "${1}" != -- ]]; do envs+=("${1}"); shift; done; shift
	mkdir -p "${work}/f/${name}"
	rc=0; env -u SILKTERM_TEST_DIR -u HOST_TEMP TMPDIR="${work}/f/${name}" TEMP="${work}/f/${name}" TMP="${work}/f/${name}" HOME="${home}" STUB_RECORD="${work}/f/${name}.record" "${envs[@]}" \
		bash "${root}/cicd/cicd.bash" --gate --no-container "${@}" >"${work}/f/${name}.log" 2>&1 || rc=$?
	given="$(sed -n 1p "${work}/f/${name}.record" 2>/dev/null || true)"
}
fRec(){ sed -n "${2}p" "${work}/f/${1}.record" 2>/dev/null || true; }
fGate base --
fCheck "the gate runs with a stand-in cargo" test "${rc}" -eq 0
((rc == 0)) || fShowTail "${work}/f/base.log"
fCheck "and hands its tests a run folder it made in the project's folder in the temp dir" \
	test "$(dirname "${given}")" == "$(realpath -m "${work}/f/base")/silkterm" -a "$(fRec base 3)" == there
fCheck "named for the time, with a counter" bash -c '[[ "$(basename "$1")" =~ $2 ]]' _ "${given}" "${runRe}"
fCheck "with the run's PID in its run.pid" bash -c '[[ "$1" =~ ^[0-9]+$ ]]' _ "$(fRec base 5)"
## Commented out 20261031: the run folder is the temp folder now (2026102807494157).
# fCheck "and leaves TMPDIR as it was" test "$(sed -n 2p "${work}/f/record" 2>/dev/null || true)" == "${work}/f/base"
fCheck "and points TMPDIR, TEMP and TMP at it" test -n "${given}" -a "$(fRec base 2)" == "${given}" -a "$(fRec base 4)" == "${given}|${given}"
fCheck "and removes the folder once the gate passes" test -n "${given}" -a ! -e "${given}"

##	F2: a gate whose tests fail removes it too, and says --keep-temp keeps it.
fGate base2 STUB_TEST_RC=1 --
fCheck "a gate whose tests fail fails" test "${rc}" -ne 0
## Commented out 20261031: a failed run removes its folder now (2026102807494157).
# fCheck "and keeps the run folder" test -n "${given}" -a -d "${given}"
# fCheck "and says where" grep -qxF "test files kept in ${given}" "${work}/f2.log"
fCheck "and removes the run folder" test -n "${given}" -a ! -e "${given}"
fCheck "and says --keep-temp keeps it" grep -qxF "removed the run's temp folder ${given}; --keep-temp keeps it" "${work}/f/base2.log"
fCheck "leaving the project's folder" test -d "$(dirname "${given}")"

##	F3: --keep-temp keeps it, failed or not, and says where.
fGate base3 STUB_TEST_RC=1 -- --keep-temp
fCheck "--keep-temp keeps the run folder of a gate that failed" test "${rc}" -ne 0 -a -n "${given}" -a -f "${given}/run.pid"
fCheck "and says where" grep -qxF "kept the run's temp folder (--keep-temp): ${given}" "${work}/f/base3.log"


##	G: each helper on its own. A pass or a skip removes the folder it made, a
##	failure keeps it and says so unless it left nothing in it, a read-only
##	folder inside doesn't stop the removal, and a folder it was handed, one
##	without its mark, or one swapped for a link is left. A link inside is
##	removed as a link. Bash and Python here, PowerShell in remove.ps1.
mkdir -p "${work}/g/outside"; echo x >"${work}/g/outside/file"
##	fChild <lang> <base> <code> [<given folder>]: the helper, then the code, in a
##	child whose temp dir is a fresh base. The code finds the folder in d.
fChild(){
	local -r lang="${1}" base="${2}" code="${3}" given="${4:-}"
	local -a run=(env -u SILKTERM_TEST_DIR -u HOST_TEMP TMPDIR="${base}" TEMP="${base}" TMP="${base}" OUTSIDE="${work}/g/outside")
	[[ -z "${given}" ]] || run+=(SILKTERM_TEST_DIR="${given}")
	mkdir -p "${base}"
	case "${lang}" in
		bash)   run+=(bash -c 'set -euo pipefail; source "${1}"; fTestDir_Use; d="${SILKTERM_TEST_DIR}"; eval "${2}"' _ "${tests}/_testdir.bash" "${code}") ;;
		python) run+=(python3 -c 'import os, sys; sys.path.insert(0, sys.argv[1]); import _testdir; d = str(_testdir.use()); exec(sys.argv[2])' "${tests}" "${code}") ;;
	esac
	"${run[@]}" >"${base}.log" 2>&1 || true
}
declare -A gCode=(
	[bash/pass]='echo x >"${d}/file"; exit 0'
	[python/pass]='open(d + "/file", "w").write("x"); _testdir.end(0)'
	[bash/fail]='echo x >"${d}/file"; exit 1'
	[python/fail]='open(d + "/file", "w").write("x"); _testdir.end(1); sys.exit(1)'
	[bash/skip]='echo x >"${d}/file"; exit 3'
	[python/skip]='open(d + "/file", "w").write("x"); _testdir.end(3); sys.exit(3)'
	[bash/empty]='exit 1'
	[python/empty]='_testdir.end(1); sys.exit(1)'
	[bash/locked]='mkdir -p "${d}/ro/sub"; echo x >"${d}/ro/sub/file"; chmod 555 "${d}/ro"; exit 0'
	[python/locked]='os.makedirs(d + "/ro/sub"); open(d + "/ro/sub/file", "w").write("x"); os.chmod(d + "/ro", 0o555); _testdir.end(0)'
	[bash/trap]='work="$(mktemp -d)"; trap '"'"'rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"'"'"' EXIT; exit 0'
	[bash/marker]='echo other >"${d}/.test_silkterm_owner"; exit 0'
	[python/marker]='open(d + "/.test_silkterm_owner", "w").write("other\n"); _testdir.end(0)'
	[bash/swapped]='mv "${d}" "${d}-aside"; ln -s "${d}-aside" "${d}"; exit 0'
	[python/swapped]='os.rename(d, d + "-aside"); os.symlink(d + "-aside", d); _testdir.end(0)'
	[bash/inner]='ln -s "${OUTSIDE}" "${d}/link"; exit 0'
	[python/inner]='os.symlink(os.environ["OUTSIDE"], d + "/link"); _testdir.end(0)'
)
##	fSwappedLeft <base>: the link still at the run folder's name, and the folder it points to still marked.
fSwappedLeft(){
	local link; link="$(find "${1}/silkterm" -mindepth 1 -maxdepth 1 -type l -print -quit)"
	[[ -n "${link}" && "$(basename "${link}")" =~ ${stampRe} && -f "${link}-aside/.test_silkterm_owner" && ! -L "${link}-aside" ]]
}
for lang in bash python; do
	g="${work}/g/${lang}"
	fChild "${lang}" "${g}-pass" "${gCode[${lang}/pass]}"
	fCheck "the ${lang} helper removes its folder when the run passes" fNoneLeft "${g}-pass"
	fChild "${lang}" "${g}-fail" "${gCode[${lang}/fail]}"
	fCheck "keeps it when the run fails" fOneStamped "${g}-fail"
	fCheck "and says where" grep -qF "test files kept in ${g}-fail/silkterm/" "${g}-fail.log"
	fChild "${lang}" "${g}-skip" "${gCode[${lang}/skip]}"
	fCheck "removes it when the run skips, with 3" fNoneLeft "${g}-skip"
	fChild "${lang}" "${g}-empty" "${gCode[${lang}/empty]}"
	fCheck "removes it when a failed run left nothing in it but its marks" fNoneLeft "${g}-empty"
	fCheck "and doesn't say it kept it" bash -c '! grep -qF "test files kept in" "${1}"' _ "${g}-empty.log"
	fChild "${lang}" "${g}-locked" "${gCode[${lang}/locked]}"
	fCheck "removes it whole with a read-only folder inside" fNoneLeft "${g}-locked"
	if [[ "${lang}" == bash ]]; then
		fChild bash "${g}-trap" "${gCode[bash/trap]}"
		fCheck "removes it from a caller's own EXIT trap in the documented form" fNoneLeft "${g}-trap"
	fi
	fChild "${lang}" "${g}-given" "${gCode[${lang}/pass]}" "${g}-handed"
	fCheck "never removes a folder it was handed" bash -c '[[ -f "$1/file" && -z "$(ls -A "$2")" ]]' _ "${g}-handed" "${g}-given"
	fChild "${lang}" "${g}-marker" "${gCode[${lang}/marker]}"
	fCheck "leaves a folder without this run's mark" fOneStamped "${g}-marker"
	fCheck "and says why" grep -qE '^test run folder: left .* in place: ' "${g}-marker.log"
	fChild "${lang}" "${g}-swapped" "${gCode[${lang}/swapped]}"
	fCheck "leaves a link put in place of the folder, and what it points to" fSwappedLeft "${g}-swapped"
	fChild "${lang}" "${g}-inner" "${gCode[${lang}/inner]}"
	fCheck "removes a link inside as a link" fNoneLeft "${g}-inner"
	fCheck "and what it points to stays" test -f "${work}/g/outside/file"
done
if command -v pwsh >/dev/null; then
	mkdir "${work}/g/pwsh"
	rc=0; env -u SILKTERM_TEST_DIR -u HOST_TEMP TMPDIR="${work}/g/pwsh" TEMP="${work}/g/pwsh" TMP="${work}/g/pwsh" pwsh -NoProfile -NonInteractive -File "${meDir}/remove.ps1" >"${work}/g-pwsh.log" 2>&1 || rc=$?
	fCheck "the PowerShell helper passes remove.ps1" test "${rc}" -eq 0
	((rc == 0)) || fShowTail "${work}/g-pwsh.log"
	fCheck "which removes its own folder too" fNoneLeft "${work}/g/pwsh"
else
	echo "  skip the PowerShell helper's removal: no pwsh here"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260930 JC: Created.
##		- 20261002 JC: A run removes the folder it made when it passes.
##		- 20261025 JC: A script that binds sockets under its run folder keeps them short.
##		- 20261031: cicd.bash's run folder is in HOST_TEMP/silkterm, and goes failed or not.
##		- 20261031: Each case that sets TMPDIR sets TEMP and TMP with it, as the pipeline does.
##		- 20261101: A lone run's folder is HOST_TEMP/silkterm/YYYYmmDD-HHMMSS_NNN, with a run.pid.
