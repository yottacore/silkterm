#!/usr/bin/env bash

##	- Purpose:
##		The advisory scan looks only at crates that go into a release binary for a
##		release target (deny.toml's [graph] table). Runs deny.toml against a small
##		local advisory db with made-up advisories: one against a dev-only crate or
##		a crate only another platform uses must pass, one against a shipped crate
##		must fail. Exit 3 when cargo-deny isn't installed.
##	- Test ID: de5jx
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"

if ! cargo deny --version >/dev/null 2>&1; then echo "  skip: no cargo-deny"; exit 3; fi

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-advscope.XXXXXX")"
fEnd(){ local -r rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## cargo-deny names its db folder after the default url plus a hash of it. If a
## new cargo-deny changes that, every scan below errors and the pass check fails.
db="${work}/db/advisory-db-3157b0e258782691"
mkdir -p "${db}/crates"
git -C "${db}" init -q

## fPlant <crate> <id>: an advisory that every version of <crate> has.
fPlant(){
	mkdir -p "${db}/crates/${1}"
	# shellcheck disable=SC2016  ## the backticks are the advisory's fence
	printf '```toml\n[advisory]\nid = "%s"\npackage = "%s"\ndate = "2099-01-01"\ncategories = ["memory-corruption"]\n\n[versions]\npatched = []\n```\n\n# Made up for a test\n\nNot real.\n' \
		"${2}" "${1}" >"${db}/crates/${1}/${2}.md"
}
fUnplant(){ rm -f "${db}/crates/${1}/"*.md; }

## The real ignore list stays out, since the db here can't match it.
fConfig(){
	sed -e '/^\[advisories\]/a db-path = "'"${work}/db"'"' -e '/^\t{ id = "RUSTSEC-/d' "${root}/deny.toml" >"${work}/${1}.toml"
}
fConfig scoped
sed -e '/^exclude-dev = /d' -e '/^targets = \[/,/^\]/d' "${work}/scoped.toml" >"${work}/everything.toml"

## fScan <config>: output in ${work}/out.txt, returns cargo-deny's code.
fScan(){
	local rc=0
	git -C "${db}" add -A >/dev/null
	git -C "${db}" -c user.name=t -c user.email=t@example.invalid commit -qm db --allow-empty >/dev/null
	touch "${db}/.git/FETCH_HEAD"
	(cd "${root}" && cargo deny check --disable-fetch --config "${work}/${1}.toml" advisories) >"${work}/out.txt" 2>&1 || rc=$?
	return "${rc}"
}
fReports(){ grep -qF "ID: ${1}" "${work}/out.txt"; }

## dhat is a dev-dependency of ours; web-sys and android-activity are pulled in
## only for wasm and Android.
fPlant dhat RUSTSEC-2099-0001
fPlant web-sys RUSTSEC-2099-0002
fPlant android-activity RUSTSEC-2099-0003

fScanPasses(){ fScan scoped || { tail -20 "${work}/out.txt"; return 1; }; }
fCheck "dev-only and other-platform crates are out of the scan" fScanPasses
## A pass means nothing unless the same db fails the old scope.
fWholeTreeSeesAll(){ ! fScan everything && fReports RUSTSEC-2099-0001 && fReports RUSTSEC-2099-0002 && fReports RUSTSEC-2099-0003; }
fCheck "and the whole tree reports all 3" fWholeTreeSeesAll

## fCaught <crate>: an advisory against it alone fails the scan.
fCaught(){ local rc=0; fPlant "${1}" RUSTSEC-2099-0009; fScan scoped || rc=$?; fUnplant "${1}"; ((rc)) && fReports RUSTSEC-2099-0009; }
fCheck "a crate in every build fails it (unicode-width)" fCaught unicode-width
fCheck "a Windows-only crate fails it (windows-core)" fCaught windows-core
fCheck "a macOS-only crate fails it (objc2-app-kit)" fCaught objc2-app-kit
## cargo-deny can't leave build-dependencies out, so this one is a known extra.
fCheck "a build-only crate still fails it (embed-resource)" fCaught embed-resource

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261019 JC: Created.
