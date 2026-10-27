#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted code reads them, and shellcheck cannot see into an eval.

##	- Purpose:
##		What a run removes of what earlier runs left, and what it never touches.
##		Stale folders and build dirs are planted next to fresh ones, and next to
##		things the pipeline did not make: another run's folder, a link, a folder
##		nobody wrote down, a scratch dir under target/. Only the stale ones the
##		pipeline made may go.
##	- Test ID: EtAiwUl
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
engine="${root}/cicd/cicd.bash"
# shellcheck source=cicd/utility/include/sweep.bash
source "${root}/cicd/utility/include/sweep.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }
fNot(){ ! "${@}"; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-cleanup.XXXXXX")"
fEnd(){ local -r rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## A: the test run folders failed runs kept. Eight written down, oldest first.
## 2 has another run's mark, 3 is gone already, 4 was swapped for a link, and 99
## was never written down.
base="${work}/a"; ledger="${work}/a.ledger"; mkdir -p "${base}" "${work}/elsewhere"
fPlant(){ local -r dir="${base}/test_silkterm_2026101${1}-00000000"; mkdir -m 700 "${dir}"; echo "tok-${1}" >"${dir}/.test_silkterm_owner"; echo x >"${dir}/out.txt"; fSweep_Record "${ledger}" "${dir}" "tok-${1}"; }
for n in 1 2 3 4 5 6 7 8; do fPlant "${n}"; done
fDir(){ echo "${base}/test_silkterm_2026101${1}-00000000"; }
echo "tok-other" >"$(fDir 2)/.test_silkterm_owner"
rm -rf "$(fDir 3)"
mv "$(fDir 4)" "${work}/elsewhere/four"; ln -s "${work}/elsewhere/four" "$(fDir 4)"
mkdir "${base}/test_silkterm_20261099-00000000"; echo tok-99 >"${base}/test_silkterm_20261099-00000000/.test_silkterm_owner"
## Not in a subshell, so the counts reach the summary.
fSweep_Ledger "${ledger}" 3 "old test run folders" 2>"${work}/a.said"; said="$(cat "${work}/a.said")"
fCheck "the newest three still there are kept" test -f "$(fDir 6)/out.txt" -a -f "$(fDir 7)/out.txt" -a -f "$(fDir 8)/out.txt"
fCheck "older ones with this pipeline's mark go" test ! -e "$(fDir 1)" -a ! -e "$(fDir 5)"
fCheck "one with another run's mark stays" test -f "$(fDir 2)/out.txt"
fCheck "and the run says why" grep -qF "left $(fDir 2) in place: another run's owner mark" <<<"${said}"
fCheck "a link put in its place stays, and what it points to" test -L "$(fDir 4)" -a -f "${work}/elsewhere/four/out.txt"
fCheck "a folder nobody wrote down stays" test -d "${base}/test_silkterm_20261099-00000000"
fCheck "the ledger keeps only what it kept" test "$(cut -f2 "${ledger}" | paste -sd' ')" = "tok-6 tok-7 tok-8"
fCheck "and the summary counts what went" grep -qE '^cleanup: removed 2 old test run folders; (about [0-9]+ MB|under 1 MB) freed$' <<<"$(fSweep_Summary)"
fSweep_Ledger "${ledger}" 3 "old test run folders" 2>/dev/null
fCheck "a second sweep removes nothing more" test -d "$(fDir 6)" -a -d "$(fDir 7)" -a -d "$(fDir 8)" -a -d "$(fDir 2)"
## A folder this user does not own. /proc/1 stands in, since only root can
## make one, and nothing can remove it, should the check ever slip.
printf '/proc/1\ttok-root\n' >"${work}/root.ledger"
fCheck "a folder this user does not own is never taken" grep -q "not this user's" <<<"$(fSweep_Ledger "${work}/root.ledger" 0 x 2>&1)"

## B: a folder made the way the run folders are, with a mark only its token fits.
made=""; token=""
fSweep_MakeDir "${work}/b" run made token
fCheck "a run folder is made, 0700, named for the time" bash -c '[[ "$(basename "$1")" =~ ^run_[0-9]{8}-[0-9]{8}$ && "$(stat -c %a "$1")" == 700 ]]' _ "${made}"
fCheck "its own token fits its mark" test -z "$(fSweep_NotOurs "${made}" "${token}")"
fCheck "another does not" test -n "$(fSweep_NotOurs "${made}" "${token}x")"
fCheck "and no token does not" test -n "$(fSweep_NotOurs "${made}" "")"

## C: a toolchain change. Cargo's dirs on the list go; a dir on the list cargo
## did not make, a link, and a scratch dir off the list stay.
t="${work}/c"
fCargo(){ mkdir -p "${1}/.fingerprint" "${1}/deps"; echo x >"${1}/deps/libx.rlib"; }
fTag(){ mkdir -p "${1}"; echo "Signature: 8a477f597d28d172789f06886806bc55" >"${1}/CACHEDIR.TAG"; }
fCargo "${t}/debug"; fCargo "${t}/release"; fTag "${t}/lint"; fCargo "${t}/lint/debug"
fCargo "${t}/x86_64-pc-windows-gnu/release"; mkdir -p "${t}/notcargo"; echo x >"${t}/notcargo/keep"
fTag "${t}/darwin"; fCargo "${work}/c-elsewhere"; ln -s "${work}/c-elsewhere" "${t}/profiling"
list=("${t}/debug" "${t}/release" "${t}/lint" "${t}/x86_64-pc-windows-gnu/release" "${t}/notcargo" "${t}/profiling")
fSweep_Toolchain "${t}" key-1 "${list[@]}"
fCheck "with no toolchain on record nothing goes" test -d "${t}/debug" -a -d "${t}/release" -a -d "${t}/lint"
fCheck "and this one is written down" test "$(cat "${t}/.cicd-toolchain")" = key-1
fSweep_Toolchain "${t}" key-1 "${list[@]}"
fCheck "the same toolchain again removes nothing" test -d "${t}/debug" -a -d "${t}/x86_64-pc-windows-gnu/release"
fSweep_Toolchain "${t}" "" "${list[@]}"
fCheck "no toolchain known removes nothing either" test -d "${t}/debug" -a "$(cat "${t}/.cicd-toolchain")" = key-1
fSweep_Toolchain "${t}" key-2 "${list[@]}"
fCheck "a new toolchain takes cargo's dirs on the list" test ! -e "${t}/debug" -a ! -e "${t}/release" -a ! -e "${t}/lint" -a ! -e "${t}/x86_64-pc-windows-gnu/release"
fCheck "but not a dir on the list cargo did not make" test -f "${t}/notcargo/keep"
fCheck "nor a link, nor what it points to" test -L "${t}/profiling" -a -f "${work}/c-elsewhere/deps/libx.rlib"
fCheck "nor a scratch dir off the list" test -f "${t}/darwin/CACHEDIR.TAG"
fCheck "and the new toolchain is written down" test "$(cat "${t}/.cicd-toolchain")" = key-2

## D: a build dir a run wrote down that the next one does not name, as a cross
## target dropped from the config.
t="${work}/d"
fCargo "${t}/debug"; fCargo "${t}/aarch64-unknown-linux-gnu/release"; fCargo "${t}/mmap-bench"
fSweep_OldBuildDirs "${t}" "${t}/debug" "${t}/aarch64-unknown-linux-gnu/release"
fCheck "the first run removes nothing, and writes its list down" test -d "${t}/aarch64-unknown-linux-gnu/release" -a "$(paste -sd' ' "${t}/.cicd-build-dirs")" = "debug aarch64-unknown-linux-gnu/release"
echo "../c-elsewhere" >>"${t}/.cicd-build-dirs"
fSweep_OldBuildDirs "${t}" "${t}/debug"
fCheck "a dir the next run does not name goes, with its empty parent" test ! -e "${t}/aarch64-unknown-linux-gnu"
fCheck "the one it names stays" test -d "${t}/debug/.fingerprint"
fCheck "a scratch dir nobody listed stays" test -d "${t}/mmap-bench/.fingerprint"
fCheck "a listed path out of the target dir is never followed" test -f "${work}/c-elsewhere/deps/libx.rlib"
fCheck "and the list is now this run's" test "$(cat "${t}/.cicd-build-dirs")" = debug

## E: incremental caches. Ages count back from the newest in the same profile.
t="${work}/e"
fInc(){ mkdir -p "${1}"; touch -d "${2}" "${1}"; }
fCargo "${t}/debug"
fInc "${t}/debug/incremental/silkterm-0aaa" "now"
fInc "${t}/debug/incremental/knobs-1bbb" "3 days ago"
fInc "${t}/debug/incremental/silkterm-2ccc" "10 days ago"
fInc "${t}/debug/incremental/not.a.cache" "30 days ago"
fTag "${t}/lint"; fCargo "${t}/lint/x86_64-pc-windows-gnu/debug"
fInc "${t}/lint/x86_64-pc-windows-gnu/debug/incremental/silkterm-3ddd" "20 days ago"
fInc "${t}/lint/x86_64-pc-windows-gnu/debug/incremental/silkterm-4eee" "40 days ago"
mkdir -p "${t}/notcargo/incremental"; fInc "${t}/notcargo/incremental/silkterm-5fff" "50 days ago"
fSweep_Incremental 7 "${t}/debug" "${t}/lint" "${t}/notcargo"
fCheck "a cache unused for a week before the newest goes" test ! -e "${t}/debug/incremental/silkterm-2ccc"
fCheck "fresher ones stay" test -d "${t}/debug/incremental/silkterm-0aaa" -a -d "${t}/debug/incremental/knobs-1bbb"
fCheck "a name rustc does not use stays" test -d "${t}/debug/incremental/not.a.cache"
fCheck "a profile left alone keeps its newest, however old" test -d "${t}/lint/x86_64-pc-windows-gnu/debug/incremental/silkterm-3ddd"
fCheck "and loses the one a week older than that, nested in a target dir of its own" test ! -e "${t}/lint/x86_64-pc-windows-gnu/debug/incremental/silkterm-4eee"
fCheck "nothing goes in a dir cargo did not make" test -d "${t}/notcargo/incremental/silkterm-5fff"

## F: the pipeline. A run stopped at the prompt has made nothing to sweep, the
## builds are swept where they run, and the run ends saying what went.
repo="${work}/repo"; mkdir -p "${repo}"
tar -C "${root}" --exclude=cicd/artifacts -cf - cicd rust-toolchain.toml source/Cargo.toml | tar -xf - -C "${repo}"
mkdir "${work}/f-base"
( cd "${repo}" && env -u SILKTERM_TEST_DIR TMPDIR="${work}/f-base" GIT_CONFIG_GLOBAL=/dev/null bash cicd/cicd.bash --no-sync </dev/null >"${work}/f.log" 2>&1 ) || true
fCheck "a run stopped at its prompt leaves no test folder" test -z "$(ls -A "${work}/f-base")"
fCheck "the build dirs are swept first thing where the builds run" bash -c 'sed -n "/^fBuildTestRelease(){$/,/^}$/p" "$1" | sed -n 2p | grep -qx "[[:space:]]*fSweepBuilds"' _ "${engine}"
fCheck "the run says what went before it is done" bash -c 'grep -B3 "^fSection \"\${APP_NAME} CI/CD: done.\"" "$1" | grep -qF "fSweep_Summary"' _ "${engine}"
fCheck "the config names its build dirs" bash -c 'source "$1" && ((${#CLEANUP_BUILD_DIRS[@]} >= 4))' _ "${root}/cicd/config.bash"

## H: the nightly's build dirs, as the pipeline sweeps them. A change of the stable
## toolchain leaves them, and a nightly bump takes the old one as no longer built.
eval "$(sed -n '/^fBuildDirs(){/,/^}/p' "${engine}")"
eval "$(sed -n '/^fSweepBuilds(){/,/^}/p' "${engine}")"
rustc(){ echo "${FAKE_RUSTC}"; }
TARGET_DIR="${work}/h"; allCrossTargets=(); CLEANUP_BUILD_DIRS=(debug lint)
fCargo "${TARGET_DIR}/debug"; fTag "${TARGET_DIR}/lint"
fTag "${TARGET_DIR}/asan-2026-01-01"; fCargo "${TARGET_DIR}/asan-2026-01-01/x86_64-unknown-linux-gnu/debug"
CLEANUP_NIGHTLY_DIRS=(asan-2026-01-01); FAKE_RUSTC="rustc 1.0"
fSweepBuilds
FAKE_RUSTC="rustc 1.1"
fSweepBuilds
fCheck "a new stable toolchain takes its own build dirs" test ! -e "${TARGET_DIR}/debug" -a ! -e "${TARGET_DIR}/lint"
fCheck "and leaves the nightly's" test -d "${TARGET_DIR}/asan-2026-01-01/x86_64-unknown-linux-gnu/debug/.fingerprint"
fTag "${TARGET_DIR}/asan-2026-02-02"; CLEANUP_NIGHTLY_DIRS=(asan-2026-02-02)
fSweepBuilds
fCheck "a nightly bump takes the old nightly's dir" test ! -e "${TARGET_DIR}/asan-2026-01-01"
fCheck "and keeps the new one" test -f "${TARGET_DIR}/asan-2026-02-02/CACHEDIR.TAG"
fCheck "the config names the ASan dir for the nightly" bash -c 'source "$1" && [[ " ${CLEANUP_NIGHTLY_DIRS[*]} " == *" ${ASAN_DIR} "* && "${ASAN_DIR}" == asan-* ]]' _ "${root}/cicd/config.bash"
unset -f rustc

## H: Zig caches, emptied whole once another Zig filled them or once over the cap,
## never part of one. 1 MB is the cap here.
zc="${work}/h"; mkdir -p "${zc}"
fZig(){ mkdir -p "${zc}/${1}/o/ab" "${zc}/${1}/h"; echo x >"${zc}/${1}/o/ab/obj"; [[ -z "${2:-}" ]] || echo "${2}" >"${zc}/${1}/.cicd-zig-version"; }
fZig fresh; fSweep_ZigCache "${zc}/fresh" 0.16.0 1
fCheck "a zig cache with no stamp is stamped with this zig, and kept" bash -c '[[ "$(cat "$1/.cicd-zig-version")" == 0.16.0 && -f "$1/o/ab/obj" ]]' _ "${zc}/fresh"
fZig same 0.16.0; fSweep_ZigCache "${zc}/same" 0.16.0 1
fCheck "one this zig filled stays" test -f "${zc}/same/o/ab/obj"
fZig older 0.15.1; fSweep_ZigCache "${zc}/older" 0.16.0 1
fCheck "one another zig filled goes whole" test ! -e "${zc}/older"
fZig big 0.16.0; head -c 1100000 /dev/zero >"${zc}/big/o/ab/blob"; fSweep_ZigCache "${zc}/big" 0.16.0 1
fCheck "one over the cap goes whole" test ! -e "${zc}/big"
## A sparse file takes no blocks, as zeros on ZFS with compression, so the
## cap reads the same on any filesystem only by apparent size.
fZig sparse 0.16.0; truncate -s 1100000 "${zc}/sparse/o/ab/blob"; fSweep_ZigCache "${zc}/sparse" 0.16.0 1
fCheck "the cap goes by apparent size, not blocks on disk" test ! -e "${zc}/sparse"
fZig linked 0.15.1; ln -s "${zc}/linked" "${zc}/link"; fSweep_ZigCache "${zc}/link" 0.16.0 1
fCheck "a link is never followed" test -f "${zc}/linked/o/ab/obj" -a -L "${zc}/link"
fZig nover 0.15.1; fSweep_ZigCache "${zc}/nover" "" 1
fCheck "with no zig to ask, nothing goes" test -f "${zc}/nover/o/ab/obj"
fCheck "and the summary counts them" grep -qF '3 zig caches' <<<"$(fSweep_Summary)"
fCheck "the run sweeps them where the builds ran, before its summary" \
	bash -c 'awk "/fSweep_ZigCache/ {z = NR} /fEcho_Clean \"\\\$\\(fSweep_Summary\\)\"/ {s = NR} END {exit !(z && s && z < s)}" "$1" && grep -qF "if ((! container)) && command -v zig" "$1"' _ "${engine}"
fCheck "the cap is 2 GB in the config" bash -c 'source "$1" && [[ "${CLEANUP_CACHE_CAP_MB}" == 2048 ]]' _ "${root}/cicd/config.bash"

## G: the PowerShell side, for cicd-win.ps1's test folders on Windows.
if command -v pwsh >/dev/null 2>&1; then
	rc=0; pwsh -NoProfile -NonInteractive -File "${meDir}/win.ps1" || rc=$?
	fCheck "the PowerShell test folder sweep passes win.ps1" test "${rc}" -eq 0
else
	echo "  skip the PowerShell half: pwsh not found"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261019 JC: Created.
##		- 20261019 JC: The nightly's build dirs.
##		- 20261025 JC: Zig caches.
##		- 20261026 JC: The Zig cap by apparent size.
