#!/usr/bin/env bash

#  shellcheck disable=2016  ## 'Expressions don't expand in single quotes, use double quotes for that.' They are meant for the stub or the inner shell.

##	- Purpose:
##		A wine run of the Windows build once left file types and menu entries on
##		the desktop, pointing into a prefix that was later rebuilt. This runs the
##		wine launcher from a scratch copy of the repo, with wine and everything it
##		would kill stubbed, and checks every wine call has the menu builder off.
##	- Test ID: Eqqc5Me
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "${meDir}/../../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-wine.XXXXXX")"
trap 'rm -rf "${work}"' EXIT

## The launcher finds its repo from its own path, so a copy stages under work.
mkdir -p "${work}/repo/utility" "${work}/stubs"
cp "${repo}/utility/run-windows-build-via-wine.bash" "${work}/repo/utility/"
: >"${work}/fake.exe"

## pkill and pgrep are stubbed too: the launcher kills any keep-alive it finds,
## and a real run on this box must not be touched.
for name in wine wineboot; do
	printf '#!/bin/sh\necho "%s ${WINEDLLOVERRIDES-unset}" >>"%s/calls"\n' "${name}" "${work}" >"${work}/stubs/${name}"
done
for name in pkill pgrep x86_64-w64-mingw32-gcc; do printf '#!/bin/sh\nexit 0\n' >"${work}/stubs/${name}"; done
chmod +x "${work}/stubs/"*

fRun(){ env PATH="${work}/stubs:${PATH}" "${@}" bash "${work}/repo/utility/run-windows-build-via-wine.bash" --attach --exe "${work}/fake.exe" >/dev/null 2>&1; }

fRun env -u WINEDLLOVERRIDES
fCheck "a new prefix is booted" grep -q '^wineboot ' "${work}/calls"
fCheck "and the app is run" grep -q '^wine ' "${work}/calls"
fCheck "every wine call has the menu builder off" bash -c '! grep -v " winemenubuilder.exe=d$" "$1"' _ "${work}/calls"

: >"${work}/calls"
fRun env WINEDLLOVERRIDES="mshtml=d"
fCheck "an override already set is kept" grep -q '^wine mshtml=d;winemenubuilder.exe=d$' "${work}/calls"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260924 JC: Created.
