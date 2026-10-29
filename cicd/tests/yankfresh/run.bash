#!/usr/bin/env bash

##	- Purpose:
##		The advisory scan sees a yank made after a box's cargo index cache was
##		filled. cargo-deny reads that cache as it is, so config.bash's
##		INDEX_REFRESH_CMD runs first. In a copy of the cache, one locked crate's
##		entry is made stale (marked yanked, with a dead etag): the scan alone must
##		fail on it, and after the refresh it must pass. A refresh with no network
##		must fail. Exit 3 when cargo-deny isn't installed or the index can't be
##		reached.
##	- Test ID: defme
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
root="$(cd "${meDir}/../../.." && pwd)"
# shellcheck source=cicd/config.bash
source "${root}/cicd/config.bash"

if ! cargo deny --version >/dev/null 2>&1; then echo "  skip: no cargo-deny"; exit 3; fi
if ! command -v python3 >/dev/null 2>&1; then echo "  skip: no python3"; exit 3; fi

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-yankfresh.XXXXXX")"
fEnd(){ local -r rc=$?; [[ "${work}" == "${TMPDIR:-/tmp}"/silk-yankfresh.* && ! -L "${work}" ]] && rm -rf -- "${work}"; fTestDir_End "${rc}"; }
trap fEnd EXIT

## A cargo home of our own: a copy of the index cache, which gets planted on,
## and links to the downloads and the advisory db, which don't.
realHome="${CARGO_HOME:-${HOME}/.cargo}"
index="$(find "${realHome}/registry/index" -maxdepth 1 -name 'index.crates.io-*' | head -n 1)"
[[ -n "${index}" && -d "${index}/.cache" ]] || { echo "  skip: no crates.io index cache in ${realHome}"; exit 3; }
export CARGO_HOME="${work}/cargo"
mkdir -p "${CARGO_HOME}/registry"
cp -a "${realHome}/registry/index" "${CARGO_HOME}/registry/"
for sub in registry/cache registry/src git advisory-dbs; do
	[[ -e "${realHome}/${sub}" ]] && ln -s "${realHome}/${sub}" "${CARGO_HOME}/${sub}"
done
entry="${CARGO_HOME}/registry/index/$(basename "${index}")/.cache/cl/ip/clipboard-win"
[[ -f "${entry}" ]] || { echo "  skip: clipboard-win not in the index cache"; exit 3; }
locked="$(grep -A1 '^name = "clipboard-win"$' "${root}/Cargo.lock" | sed -n 's/^version = "\(.*\)"$/\1/p')"
[[ -n "${locked}" ]] || { echo "  FAIL clipboard-win not in Cargo.lock"; exit 1; }

## Marks the locked version yanked and blanks the etag, so the server sends the
## real entry on the next refresh instead of "not modified".
python3 -I - "${entry}" "${locked}" <<'EOF'
import sys
path, vers = sys.argv[1], sys.argv[2]
blob = open(path, 'rb').read()
start = blob.index(b'"', 5); end = blob.index(b'"', start + 1)
blob = blob[:start + 1] + b'0' * (end - start - 1) + blob[end:]
parts = blob.split(b'\0')
hits = 0
for i, part in enumerate(parts):
	if part.startswith(b'{') and b'"vers":"%s"' % vers.encode() in part:
		parts[i] = part.replace(b'"yanked":false', b'"yanked":true'); hits += 1
if not hits: sys.exit("no entry for " + vers)
open(path, 'wb').write(b'\0'.join(parts))
EOF

## fScan: the scan as the pipeline runs it, on the db already here.
fScan(){ (cd "${root}" && "${ADVISORY_CMD[@]}" --disable-fetch) >"${work}/out.txt" 2>&1; }
fRefresh(){ (cd "${root}" && "${INDEX_REFRESH_CMD[@]}") >"${work}/refresh.txt" 2>&1; }

fStaleFails(){ ! fScan && grep -qF "clipboard-win ${locked}" "${work}/out.txt" && grep -qF 'yanked' "${work}/out.txt"; }
fCheck "a stale yank in the cache fails the scan alone" fStaleFails

lockBefore="$(cksum <"${root}/Cargo.lock")"
fNoNetFails(){ ! CARGO_HTTP_PROXY=http://127.0.0.1:9 CARGO_NET_RETRY=0 CARGO_NET_OFFLINE=true fRefresh; }
fCheck "the refresh fails with no network, offline set or not" fNoNetFails

if ! fRefresh; then tail -5 "${work}/refresh.txt"; echo "  skip: can't reach the index"; exit 3; fi
fFreshPasses(){ fScan || { tail -20 "${work}/out.txt"; return 1; }; }
fCheck "after the refresh the scan reads the live entry and passes" fFreshPasses
fLockKept(){ [[ "$(cksum <"${root}/Cargo.lock")" == "${lockBefore}" ]]; }
fCheck "and the lock is untouched" fLockKept

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20261026 JC: Created.
