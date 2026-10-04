#!/usr/bin/env bash

##	- Purpose:
##		cicd.bash gates on py-lint.py, so the lint has to fail on a finding, pass
##		a clean script, and leave alone what ruff.toml leaves out. The same for
##		its tab check, which lets spaces line up after the tabs and passes the
##		one script indented with spaces. Run on scripts written here, never
##		tracked, so the pipeline's own lint of the repository never sees them.
##	- Test ID: Erm6IFE
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
lint="${root}/cicd/utility/py-lint.py"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-pylint.XXXXXX")"
trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

printf 'def main():\n\tprint(1)\n\n\nmain()\n' >"${work}/clean.py"
## An unused import, in ruff's default set.
printf 'import os\n' >"${work}/finding.py"
## `l` as a name, which ruff.toml leaves out for now.
printf 'l = 1\nprint(l)\n' >"${work}/ignored.py"
printf 'if True:\n    print(1)\n' >"${work}/spaces.py"
## Tabs, then spaces to line up a continuation, and a docstring indented with spaces.
printf 'def main():\n\t"""Text.\n\n    More text.\n\t"""\n\tvalue = max(1,\n\t            2)\n\tprint(value)\n' >"${work}/aligned.py"

fLint(){ rc=0; out="$(python3 "${lint}" "$@" 2>&1)" || rc=$?; }

fLint "${work}/clean.py"
if ((rc == 2)); then
	echo "  skip Python lint (no ruff)"
	exit 0
fi
fCheck "a clean script passes" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/finding.py"
fCheck "a finding fails" test "${rc}" -eq 1
fCheck "and is named with its line and rule" grep -qF "finding.py:1:8: F401" <<<"${out}"
fLint "${work}/clean.py" "${work}/finding.py"
fCheck "one bad script among several still fails" test "${rc}" -eq 1
fLint "${work}/ignored.py"
fCheck "a rule ruff.toml leaves out is not reported" test "${rc}" -eq 0 -a -z "${out}"
fLint "${work}/spaces.py"
fCheck "a block indented with spaces fails" test "${rc}" -eq 1
fCheck "and is named with its line" grep -qF "spaces.py:2: indentation uses spaces" <<<"${out}"
fLint "${work}/aligned.py"
fCheck "spaces after tabs, and docstring text, pass" test "${rc}" -eq 0 -a -z "${out}"
fLint "${root}/cicd/tests/scroll/analyze.py"
fCheck "the one script in spaces passes" test "${rc}" -eq 0 -a -z "${out}"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261004 JC: Created.
