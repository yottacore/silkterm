#!/usr/bin/env bash

##	- Purpose:
##		cicd.bash gates on py-lint.py, so the lint has to fail on a finding, pass
##		a clean script, and leave alone what ruff.toml leaves out. The same for
##		its tab check, which lets spaces line up after the tabs and passes the
##		one script indented with spaces. Each class of the style guide's Python
##		rules fails, and so does a type error only mypy sees, while two scripts
##		with one file name still pass together. Run on scripts written here,
##		never tracked, so the pipeline's own lint of the repository never sees
##		them.
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

printf 'def main() -> None:\n\tprint(1)\n\n\nmain()\n' >"${work}/clean.py"
## An unused import, in ruff's default set.
printf 'import os\n' >"${work}/finding.py"
## A long line, which ruff.toml leaves out. Until 20261005 this was `l` as a
## name, which is checked now.
printf 'print("%s")\n' "$(printf 'x%.0s' {1..120})" >"${work}/ignored.py"
printf 'if True:\n    print(1)\n' >"${work}/spaces.py"
## Tabs, then spaces to line up a continuation, and a docstring indented with spaces.
printf 'def main() -> None:\n\t"""Text.\n\n    More text.\n\t"""\n\tvalue = max(1,\n\t            2)\n\tprint(value)\n' >"${work}/aligned.py"
## One script per class the style guide's Python section names, each with its rule.
printf 'def main(name):\n\treturn name\n' >"${work}/untyped.py"
printf 'def fMain() -> None:\n\tpass\n' >"${work}/camel.py"
printf 'def main(name: str) -> str:\n\treturn "%%s!" %% name\n' >"${work}/percent.py"
printf 'import os\n\n\ndef main(name: str) -> str:\n\treturn os.path.join("a", name)\n' >"${work}/ospath.py"
printf 'def main(name: str) -> str:\n\tstream = open(name)\n\treturn stream.read()\n' >"${work}/unclosed.py"
printf 'import subprocess\n\n\ndef main() -> None:\n\tsubprocess.run(["true"])\n' >"${work}/nocheck.py"
printf 'def main(seen: list[int] = []) -> list[int]:\n\treturn seen\n' >"${work}/mutable.py"
printf 'def main() -> None:\n\ttry:\n\t\tprint(1)\n\texcept Exception:\n\t\tpass\n' >"${work}/blind.py"
## The same catch, marked as meant.
printf 'def main() -> None:\n\ttry:\n\t\tprint(1)\n\texcept Exception:  # noqa: BLE001\n\t\tpass\n' >"${work}/blindok.py"
## Clean to ruff, wrong to mypy.
printf 'def main() -> int:\n\treturn "one"\n' >"${work}/typeerror.py"
## mypy names a script by its file, so two run.py files in one run would clash.
mkdir -p "${work}/one" "${work}/two"
cp "${work}/clean.py" "${work}/one/run.py"
cp "${work}/clean.py" "${work}/two/run.py"
cp "${work}/clean.py" "${work}/with-dash.py"

## stdout only: with no mypy the lint says so on stderr, and still passes a clean script.
fLint(){ rc=0; out="$(python3 "${lint}" "$@" 2>/dev/null)" || rc=$?; }
haveMypy=0
{ command -v mypy || [[ -x "${HOME}/.local/bin/mypy" ]]; } >/dev/null && haveMypy=1

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
for pair in untyped.py:ANN001 camel.py:N802 percent.py:UP031 ospath.py:PTH118 unclosed.py:SIM115 nocheck.py:PLW1510 mutable.py:B006 blind.py:BLE001; do
	fLint "${work}/${pair%%:*}"
	fCheck "${pair%%:*} fails with ${pair#*:}" grep -qF " ${pair#*:} " <<<"${out}"
done
fLint "${work}/blindok.py"
fCheck "a catch-all marked with noqa passes" test "${rc}" -eq 0 -a -z "${out}"
fLint "${root}/cicd/utility/n8output-random-unicode.py"
fCheck "the shared helper ruff.toml leaves out passes" test "${rc}" -eq 0 -a -z "${out}"
if ((haveMypy)); then
	fLint "${work}/typeerror.py"
	fCheck "a type error only mypy sees fails" test "${rc}" -eq 1
	fCheck "and is named with its line" grep -qF "typeerror.py:2: error:" <<<"${out}"
	fLint "${work}/untyped.py"
	fCheck "mypy also refuses a def with no types" grep -qF "[no-untyped-def]" <<<"${out}"
	fLint "${work}/one/run.py" "${work}/two/run.py" "${work}/with-dash.py"
	fCheck "two scripts named run.py, and a name with a dash, pass together" test "${rc}" -eq 0 -a -z "${out}"
	fLint "${work}/clean.py" "${work}/one/run.py" "${work}/typeerror.py" "${work}/two/run.py"
	fCheck "and a type error among them still fails" grep -qF "typeerror.py:2: error:" <<<"${out}"
else
	echo "  skip mypy cases (no mypy)"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261004 JC: Created.
##		- 20261005 JC: The style guide's Python rules, and mypy.
##		- 20261005 JC: except Exception.
