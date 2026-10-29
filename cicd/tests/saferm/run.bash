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

## The pipeline's own removals all go through it.
fBare(){ grep -nE '(^|[;&|{([:space:]])rm +(-[A-Za-z]*[rR]|--recursive)' "${@}" | grep -vE '^[0-9]+:[[:space:]]*##?' || true; }
fCheck "no bare recursive rm in cicd.bash or the sweep" test -z "$(fBare "${repoRoot}/cicd/cicd.bash" "${repoRoot}/cicd/utility/include/sweep.bash")"
fCheck "and one in the guard" test "$(fBare "${lib}" | wc -l)" -eq 1

## The PowerShell copy, which cicd-win.ps1 goes through, and its own test.
fBareWin(){ grep -niE 'Remove-Item.*-Recurse' "${@}" | grep -vE '^[0-9]+:[[:space:]]*##' || true; }
fCheck "no bare Remove-Item -Recurse in cicd-win.ps1" test -z "$(fBareWin "${repoRoot}/cicd/cicd-win.ps1")"
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
