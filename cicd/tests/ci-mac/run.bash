#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted fWhyFailed reads logDir, and shellcheck can't see into an eval.
#  shellcheck disable=2329  ## 'Never invoked.' The cargo stand-in is called by the lifted fWhyFailed.

##	- Purpose:
##		ci-mac.bash names every test that failed, even when a test's own output
##		splits "test <name> ..." from its FAILED, and runs only those again.
##		Lifts fFailedNames and fWhyFailed out of ci-mac.bash as they stand and
##		feeds them logs in the shapes cargo test writes, with cargo stood in for.
##	- Test ID: desrb
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
script="$(cd "${meDir}/../.." && pwd)/ci-mac.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-cimac.XXXXXX")"
fEnd(){ local -r rc=$?; fTestDir_Rm "${work}" || true; fTestDir_End "${rc}"; }
trap fEnd EXIT

fLift(){ sed -n "/^${1}(){\$/,/^}\$/p" "${script}"; }
lifted="$(fLift fFailedNames; fLift fWhyFailed)"
eval "${lifted}"
cargo(){ echo "cargo ${*}"; }

## One thread: a test's output and its panic end up between "test <name> ..." and FAILED.
cat > "${work}/split.log" <<'E'
running 3 tests
test tests::ok_one ... measured: fine
ok
test tests::plain_fail ...
thread 'tests::plain_fail' (3755396) panicked at src/lib.rs:5:31:
plain boom
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED
test tests::split_fail ... measured: partial more

thread 'tests::split_fail' (3755397) panicked at src/lib.rs:4:79:
assertion `left == right` failed: kept cost too much
  left: 1
 right: 2
FAILED

failures:

failures:
    tests::plain_fail
    tests::split_fail

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--lib`
E

## Here there are 2 threads and 2 binaries, each with its own list, and one name on its result line.
cat > "${work}/two.log" <<'E'
running 2 tests

thread 'tests::plain_fail' (3758555) panicked at src/lib.rs:5:31:
plain boom
test tests::plain_fail ... FAILED
test tests::ok_one ... ok

failures:

failures:
    tests::plain_fail

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 1 test
noise
    indented
test other_bin_fail ... FAILED

failures:

failures:
    other_bin_fail

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
E

## Captured output, whose first failures block has an indented line that isn't a name.
cat > "${work}/captured.log" <<'E'
running 1 test
other_bin_fail --- FAILED

failures:

---- other_bin_fail stdout ----
noise
    indented

failures:
    other_bin_fail

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
E

## A binary that died, with no list at all.
printf 'running 1 test\ntest tests::crash ... error: test failed\n  process didn'"'"'t exit successfully (signal: 11, SIGSEGV)\n' > "${work}/crash.log"

fNames(){ [[ "$(fFailedNames "${work}/${1}.log" | tr '\n' ' ')" == "${2}" ]]; }
fCheck "both split tests are named"                  fNames split "tests::plain_fail tests::split_fail "
fCheck "every binary's list counts, each name once"  fNames two "tests::plain_fail other_bin_fail "
fCheck "captured output isn't read as a name"        fNames captured "other_bin_fail "

logDir="${work}"; cp "${work}/split.log" "${work}/test.log"
out="$(fWhyFailed)"
fCheck "the report names them"           grep -qx 'tests::split_fail' <<<"${out}"
fCheck "with the panic text"             grep -qF 'kept cost too much' <<<"${out}"
fCheck "and runs just those again, exact" grep -qxF 'cargo test --locked --no-fail-fast -- --test-threads=1 --nocapture --exact tests::plain_fail tests::split_fail' <<<"${out}"

cp "${work}/crash.log" "${work}/test.log"
out="$(fWhyFailed)"
fCheck "no name found shows the end of the log" grep -qF 'signal: 11' <<<"${out}"
fNoRerun(){ ! grep -q '^cargo' <<<"${out}"; }
fCheck "and runs nothing again" fNoRerun

fCheck "a failed binary doesn't stop the suite" grep -qF 'cargo test --locked --no-fail-fast -- --nocapture >' "${script}"

if ((failures)); then echo "${failures} check(s) failed"; exit 1; fi
echo "PASS desrb Mac CI names its failed tests"

##	History:
##		- 20261102 JC: Created.
