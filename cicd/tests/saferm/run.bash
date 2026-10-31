#!/usr/bin/env bash
#  shellcheck disable=2016  ## 'expressions don't expand in single quotes.' The bash -c scripts take their own arguments.

##	- Purpose:
##		The guard every recursive removal in the pipeline goes through. A wrong
##		answer here deletes something that was never ours, so this plants
##		folders, links and odd paths beside each other and checks only the ones
##		it may take go. The cases outside a tmp folder run under the target dir,
##		whose real path has none.
##	- Test ID: dehwp
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
repoRoot="$(cd "${meDir}/../../.." && pwd)"
lib="${repoRoot}/cicd/utility/include/saferm.bash"
# shellcheck source=cicd/utility/include/saferm.bash
source "${lib}"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fRefused(){ ! "${@}" 2>/dev/null; }

work="$(cd -P "$(mktemp -d "${TMPDIR:-/tmp}/silk-saferm.XXXXXX")" && pwd -P)"
target="${CARGO_TARGET_DIR:-${repoRoot}/target}"
mkdir -p "${target}"
target="$(cd -P "${target}" && pwd -P)"
out=""
fEnd(){
	local -r rc=$?
	fSafeRm "${work}" || true
	[[ -z "${out}" ]] || fSafeRm_Under "${target}" "${out}" || true
	fTestDir_End "${rc}"
}
trap fEnd EXIT

## Under a tmp folder.
mkdir -p "${work}/a/b" "${work}/keep"; echo x >"${work}/a/b/f"; echo k >"${work}/keep/f"
said="$(fSafeRm "${work}/a" 2>&1)"
fCheck "a folder under a tmp folder goes" test ! -e "${work}/a"
fCheck "and nothing is printed" test -z "${said}"
fCheck "a trailing slash is the same folder" bash -c 'mkdir "$2/s" && source "$1" && fSafeRm "$2/s/" && [[ ! -e "$2/s" ]]' _ "${lib}" "${work}"
ln -s "${work}/keep" "${work}/lnk"
fCheck "a link is refused" fRefused fSafeRm "${work}/lnk"
fCheck "and it and what it points at stay" test -L "${work}/lnk" -a -f "${work}/keep/f"
fCheck "a path through a link is refused" fRefused fSafeRm "${work}/lnk/f"
fCheck "and what it reaches stays" test -f "${work}/keep/f"
mkdir -p "${work}/c"
fCheck "a path with .. is refused" fRefused fSafeRm "${work}/c/../keep"
fCheck "a path with // is refused" fRefused fSafeRm "${work}//keep"
fCheck "a path with a newline is refused" fRefused fSafeRm "${work}/keep"$'\n'
fCheck "an empty path is refused" fRefused fSafeRm ""
fCheck "no path is refused" fRefused fSafeRm
fCheck "a relative path is refused" fRefused fSafeRm "tmp/x"
fCheck "two paths are refused" fRefused fSafeRm "${work}/c" "${work}/keep"
fCheck "and neither goes" test -d "${work}/c" -a -f "${work}/keep/f"
fCheck "a path that isn't there is refused" fRefused fSafeRm "${work}/nope"
fCheck "the tmp folder itself is refused" fRefused fSafeRm /tmp
fCheck "so is the root" fRefused fSafeRm /

## Outside any tmp folder.
if [[ "/${target}/" == */tmp/* ]]; then
	echo "  skip the cases outside a tmp folder: the target dir is under one (${target})"
else
	out="$(mktemp -d "${target}/silk-saferm.XXXXXX")"
	mkdir -p "${out}/tmp/r1" "${out}/tmp/r2" "${out}/keep/sub" "${out}/keepnot/x" "${out}/ht/silkterm/r1" "${out}/ht/other/r1"
	echo k >"${out}/keep/f"
	fCheck "a folder outside a tmp folder is refused" fRefused fSafeRm "${out}/keep"
	fCheck "and stays" test -f "${out}/keep/f"
	fCheck "the tmp folder itself is refused" fRefused fSafeRm "${out}/tmp"
	fCheck "a folder in it goes" fSafeRm "${out}/tmp/r1"
	fCheck "and only that one" test ! -e "${out}/tmp/r1" -a -d "${out}/tmp/r2"
	ln -s "${out}/keep" "${out}/tmp/via"
	fCheck "a tmp path through a link to outside is refused" fRefused fSafeRm "${out}/tmp/via/sub"
	fCheck "and what it reaches stays" test -d "${out}/keep/sub"
	## HOST_TEMP's project folder counts too, as on macOS and Windows.
	fCheck "under HOST_TEMP's project folder goes" bash -c 'HOST_TEMP="$2/ht" source "$1" && fSafeRm "$2/ht/silkterm/r1" && [[ ! -e "$2/ht/silkterm/r1" ]]' _ "${lib}" "${out}"
	fCheck "that folder itself is refused" fRefused bash -c 'HOST_TEMP="$2/ht" source "$1" && fSafeRm "$2/ht/silkterm"' _ "${lib}" "${out}"
	fCheck "another project's is refused" fRefused bash -c 'HOST_TEMP="$2/ht" source "$1" && fSafeRm "$2/ht/other/r1"' _ "${lib}" "${out}"
	fCheck "and both stay" test -d "${out}/ht/silkterm" -a -d "${out}/ht/other/r1"
	## Under one named parent.
	fCheck "under the named parent goes" fSafeRm_Under "${out}/keep" "${out}/keep/sub"
	fCheck "and only that" test ! -e "${out}/keep/sub" -a -f "${out}/keep/f"
	fCheck "the parent itself is refused" fRefused fSafeRm_Under "${out}/keep" "${out}/keep"
	fCheck "a name that only starts like it is refused" fRefused fSafeRm_Under "${out}/keep" "${out}/keepnot/x"
	fCheck "another folder is refused" fRefused fSafeRm_Under "${out}/keep" "${out}/tmp/r2"
	fCheck "no parent is refused" fRefused fSafeRm_Under "" "${out}/keep"
	fCheck "a relative parent is refused" fRefused fSafeRm_Under "keep" "${out}/keep"
	fCheck "and all of them stay" test -f "${out}/keep/f" -a -d "${out}/keepnot/x" -a -d "${out}/tmp/r2"
fi

## HOST_TEMP itself and a whole project folder in it, here under a tmp folder,
## where the tmp rule alone would take them. Real folders, since a path that
## isn't there gets refused anyway.
ht="${work}/tmp/ht"
mkdir -p "${ht}/silkterm/r1" "${ht}/other/r1" "${ht}/loose" "${ht}/sock"
ln -s "${ht}" "${work}/tmp/htl"
fHt(){ HOST_TEMP="${ht}" bash -c 'source "$1"; shift; "$@"' _ "${lib}" "${@}"; }
fHtLink(){ HOST_TEMP="${work}/tmp/htl/" bash -c 'source "$1"; shift; "$@"' _ "${lib}" "${@}"; }
fNoHt(){ env -u HOST_TEMP TMPDIR="${ht}" bash -c 'source "$1"; shift; "$@"' _ "${lib}" "${@}"; }
fCheck "HOST_TEMP itself is refused" fRefused fHt fSafeRm "${ht}"
fCheck "so is this project's folder in it" fRefused fHt fSafeRm "${ht}/silkterm"
fCheck "and another project's" fRefused fHt fSafeRm "${ht}/other"
fCheck "with a trailing slash too" fRefused fHt fSafeRm "${ht}/other/"
fCheck "and it says why" grep -qF "refused, a whole project temp folder: ${ht}/other" <<<"$(fHt fSafeRm "${ht}/other" 2>&1)"
fCheck "a HOST_TEMP behind a link is resolved first" fRefused fHtLink fSafeRm "${ht}/other"
fCheck "and all of them stay" test -d "${ht}/silkterm/r1" -a -d "${ht}/other/r1"
fCheck "a run folder in another project's folder goes" fHt fSafeRm "${ht}/other/r1"
fCheck "and one in this project's" fHt fSafeRm "${ht}/silkterm/r1"
fCheck "and only those" test ! -e "${ht}/other/r1" -a ! -e "${ht}/silkterm/r1" -a -d "${ht}/other" -a -d "${ht}/silkterm"
fCheck "the one-parent form won't take HOST_TEMP" fRefused fHt fSafeRm_Under "${work}/tmp" "${ht}"
fCheck "or this project's folder" fRefused fHt fSafeRm_Under "${ht}" "${ht}/silkterm"
fCheck "but takes a folder made right in it, as the test run folders are" fHt fSafeRm_Under "${ht}" "${ht}/loose"
fCheck "with no HOST_TEMP, TMPDIR stands in" fRefused fNoHt fSafeRm "${ht}"
fCheck "and this project's folder in it is refused" fRefused fNoHt fSafeRm "${ht}/silkterm"
fCheck "while a folder right in it goes, as the socket folders in /tmp do" fNoHt fSafeRm "${ht}/sock"
fCheck "and HOST_TEMP's folders are still there" test -d "${ht}/silkterm" -a -d "${ht}/other" -a ! -e "${ht}/loose" -a ! -e "${ht}/sock"

## The release artifacts folder, cleared by cicd.bash's own lines, only under
## cicd/artifacts in the repo it was given.
clear="$(sed -n '/^\t\tif \[\[ -e "\${artDir}" || -L "\${artDir}" \]\]; then$/,/^\t\tfi$/p' "${repoRoot}/cicd/cicd.bash")"
fCheck "the clear is found in cicd.bash" test "$(grep -c 'fSafeRm_Under' <<<"${clear}")" -eq 1
# shellcheck disable=2034,2329  ## the lifted lines read them
fClear(){ (
	local root="${1}" RELEASE_ARTIFACT_DIR="${2}" artDir="${1}/${2}"
	fDie(){ echo "DIE: ${*}"; exit 1; }
	eval "${clear}"
) >/dev/null 2>&1; }
mkdir -p "${work}/repo/cicd/artifacts/release" "${work}/repo/art"
echo a >"${work}/repo/cicd/artifacts/release/f"; echo b >"${work}/repo/art/f"; echo c >"${work}/repo/cicd/artifacts/lint"
fCheck "the release folder is cleared" fClear "${work}/repo" cicd/artifacts/release
fCheck "and its neighbour stays" test ! -e "${work}/repo/cicd/artifacts/release" -a -f "${work}/repo/cicd/artifacts/lint"
fCheck "one outside cicd/artifacts stops the run" fRefused fClear "${work}/repo" art
fCheck "and stays" test -f "${work}/repo/art/f"
fCheck "so does cicd/artifacts itself" fRefused fClear "${work}/repo" cicd/artifacts
fCheck "and it stays" test -f "${work}/repo/cicd/artifacts/lint"

## Every recursive removal in a bash file in cicd/ goes through it: the
## pipeline, the tests and the utilities. A bash file is a .bash or .sh, or one
## whose first line names bash or sh. The tracked ones, as the lint takes them.
fBare(){
	grep -HnE "(^|[;&|{(\`'\"[:space:]])(command +|\\\\)?rm( +-[A-Za-z]+)* +(-[A-Za-z]*[rR]|--recursive)" "${@}" /dev/null \
		| grep -vE '^[^:]+:[0-9]+:[[:space:]]*#' || true
}
mapfile -t shFiles < <(cd "${repoRoot}" && git ls-files -z cicd | xargs -0 awk -v root="${repoRoot}/" \
	'FNR == 1 { if (FILENAME ~ /\.(bash|sh)$/ || $0 ~ /^#!.*[\/ ](ba)?sh([ \t]|$)/) print root FILENAME; nextfile }')
fCheck "the bash files are found" test "${#shFiles[@]}" -gt 60
said="$(fBare "${shFiles[@]}" | grep -vF "${lib}:" || true)"
fCheck "no bare recursive rm in any of them" test -z "${said}"
[[ -z "${said}" ]] || printf '    %s\n' "${said//$'\n'/$'\n'    }"
fCheck "and one in the guard" test "$(fBare "${lib}" | wc -l)" -eq 1
## Built from r, so this file's own lines don't match.
r="rm"
fCheck "the check finds the forms it looks for" test "$(fBare <(printf '%s\n' "${r} -rf a" "x; ${r} -f -r a" "trap '${r} -Rf a' EXIT" "\\${r} --recursive a" "command ${r} -fr a" "## ${r} -rf a" "docker run --${r} a" "${r} -f a") | wc -l)" -eq 5

## The same for every tracked Python file in cicd/, by name or first line, and
## the Python copy of the guard and its own test.
fBarePy(){ grep -HnE '\brmtree\b' "${@}" /dev/null | grep -vE '^[^:]+:[0-9]+:[[:space:]]*#' || true; }
mapfile -t pyFiles < <(cd "${repoRoot}" && git ls-files -z cicd | xargs -0 awk -v root="${repoRoot}/" \
	'FNR == 1 { if (FILENAME ~ /\.py$/ || $0 ~ /^#!.*[\/ ]python[0-9.]*([ \t]|$)/) print root FILENAME; nextfile }')
fCheck "the Python files are found" test "${#pyFiles[@]}" -gt 20
said="$(fBarePy "${pyFiles[@]}")"
fCheck "no bare rmtree in any of them" test -z "${said}"
[[ -z "${said}" ]] || printf '    %s\n' "${said//$'\n'/$'\n'    }"
r="rmtree"
fCheck "the Python check finds the forms it looks for" test "$(fBarePy <(printf '%s\n' "shutil.${r}(a)" "from shutil import ${r}" "	${r}(a, ignore_errors=True)" "## ${r}(a)" "	# ${r}(a)" "x.${r}s(a)") | wc -l)" -eq 3
fCheck "the Python guard's test (depj1) passes" python3 -I "${meDir}/run.py"

## A test's own folders, through fTestDir_Rm, under the run's test folder only.
mkdir -p "${work}/td/run/w1/sub" "${work}/td/other"; echo k >"${work}/td/other/f"
ln -s "${work}/td/run" "${work}/td/via"
fTdRm(){ ( SILKTERM_TEST_DIR="${work}/td/${1}"; fTestDir_Rm "${2}" ); }
fCheck "a folder in the test folder goes" fTdRm run "${work}/td/run/w1"
fCheck "and only that" test ! -e "${work}/td/run/w1" -a -d "${work}/td/run"
mkdir -p "${work}/td/run/w2"
fCheck "so does one named by a test folder behind a link" fTdRm via "${work}/td/via/w2"
fCheck "and it's gone" test ! -e "${work}/td/run/w2"
fCheck "a path that isn't there is left alone" fTdRm run "${work}/td/run/nope"
fCheck "the test folder itself is refused" fRefused fTdRm run "${work}/td/run"
fCheck "one outside it is refused" fRefused fTdRm run "${work}/td/other"
fCheck "and stays" test -f "${work}/td/other/f"
ln -s "${work}/td/other" "${work}/td/run/lnk"
fCheck "a link in it is refused" fRefused fTdRm run "${work}/td/run/lnk"
fCheck "and what it points at stays" test -f "${work}/td/other/f"
fCheck "with no test folder it's refused" fRefused bash -c 'unset SILKTERM_TEST_DIR; source "$1"; fTestDir_Rm "$2"' _ "${meDir}/../_testdir.bash" "${work}/td/other"
fCheck "and still stays" test -f "${work}/td/other/f"

## The PowerShell copy, which cicd-win.ps1 goes through, and its own test.
fBareWin(){ grep -HniE 'Remove-Item.*-Recurse' "${@}" | grep -vE '^[^:]+:[0-9]+:[[:space:]]*##' || true; }
fCheck "no bare Remove-Item -Recurse in cicd-win.ps1" test -z "$(fBareWin "${repoRoot}/cicd/cicd-win.ps1")"
## Nor in any other script: the test scripts, install.ps1, and the launcher the
## Windows scenarios get. A short name or a recursive .NET delete counts too.
## The tracked ones, as the PowerShell lint takes them.
mapfile -t psFiles < <(git -C "${repoRoot}" ls-files '*.ps1' | sed "s|^|${repoRoot}/|")
fCheck "the PowerShell scripts are found" test "${#psFiles[@]}" -gt 20
psFiles+=(/dev/null)
fBareAny(){
	grep -HniE '\b(remove-item|ri|rm|del|erase|rd|rmdir)\b[^#]*[[:space:]]-r(ecurse)?\b|\]::Delete\([^)]*,[[:space:]]*\$true' "${@}" \
		| grep -vE '^[^:]+:[0-9]+:[[:space:]]*##' || true
}
said="$(fBareWin "${psFiles[@]}" "${repoRoot}/cicd/tests/wingui/run.bash"; fBareAny "${psFiles[@]}")"
fCheck "or in the other PowerShell scripts" test -z "${said}"
[[ -z "${said}" ]] || printf '    %s\n' "${said//$'\n'/$'\n'    }"
if command -v pwsh >/dev/null 2>&1; then
	fCheck "the PowerShell guard's test (dehyk) passes" pwsh -NoProfile -NonInteractive -File "${meDir}/win.ps1"
else
	echo "  skip the PowerShell guard (no pwsh)"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261028 JC: Created.
##		- 20261028: The PowerShell copy, and cicd-win.ps1 through it.
##		- 20261031: Every PowerShell script, the test scripts and install.ps1 too.
##		- 20261031: Every bash file in cicd/, and the tests' own fTestDir_Rm.
##		- 20261031: HOST_TEMP itself and a whole project folder in it.
##		- 20261031: The Python copy, and every Python file in cicd/ through it.
