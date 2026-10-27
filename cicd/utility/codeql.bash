#!/usr/bin/env bash

##	- Purpose:
##		- CodeQL's Rust queries over the code a release binary runs: ours and every
##		  crate it links for x86_64 Linux. Part of cicd.bash's leak checks, run in
##		  the container image, which has the pinned CodeQL bundle.
##		- CodeQL reports only on source inside the database's root, so a copy of the
##		  workspace and its vendored crates goes under the target dir first, and
##		  the crates go in as source, not just their signatures. Build scripts,
##		  proc macros and dev or build dependencies are scanned too, since cargo
##		  needs them, but their results are dropped by path (2026101714015789).
##		- Any result left fails the run, unless cicd/codeql-suppressions.txt names
##		  it with the backlog item that fixes it. The SARIF goes to OUT_DIR.
##	- Syntax: cicd/utility/codeql.bash <out dir> <stamp>
##	- Exit: 0 clean; 1 a result, or no CodeQL here.
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


set -Eeuo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/../.." && pwd)"
cd "${root}"
# shellcheck source=cicd/config.bash
source "${root}/cicd/config.bash"
# shellcheck source=cicd/utility/include/echo.bash
source "${here}/include/echo.bash"
# shellcheck source=cicd/utility/include/jobs.bash
source "${here}/include/jobs.bash"

outDir="${1:?out dir}"; stamp="${2:?stamp}"
command -v codeql >/dev/null 2>&1 || fDie "codeql isn't installed here; cicd.bash runs this in the container image"
fJobs_LowPriority; fJobs hard
target="${CODEQL_TARGET:-x86_64-unknown-linux-gnu}"
work="${CARGO_TARGET_DIR:-${root}/target}/codeql"
src="${work}/src"; db="${work}/db"
raw="${work}/results.sarif"; sarif="${outDir}/codeql_${stamp}.sarif"
mkdir -p "${outDir}" || fDie "could not make ${outDir}"

## The last run's copy goes first, so a crate dropped from the lock can't linger.
## Only a folder this script marked as its own.
if [[ -e "${work}" ]]; then
	[[ -f "${work}/.silkterm-codeql" ]] || fDie "${work} is there but not ours; leaving it"
	rm -rf -- "${work}"
fi
mkdir -p "${src}" && : >"${work}/.silkterm-codeql"
cp -a Cargo.toml Cargo.lock rust-toolchain.toml .cargo source knobs "${src}/"

start="${SECONDS}"
fEcho_Clean "vendoring the dependencies ..."
## cargo vendor prints the source replacement, so the scan's own cargo runs read
## the copies offline. --quiet would drop that too.
(cd "${src}" && cargo vendor --locked --versioned-dirs vendor >>.cargo/config.toml 2>"${work}/vendor.log") \
	|| { cat "${work}/vendor.log" >&2; fDie "cargo vendor failed"; }
## What a release binary links for the target, as vendor's folder names.
(cd "${src}" && cargo tree --offline --locked -e normal --target "${target}" -p "${EXE_NAME}" --prefix none -f '{p}') \
	| awk '$3 !~ /^\(\// && !/\(proc-macro\)/ { sub(/^v/, "", $2); print $1 "-" $2 }' | sort -u >"${work}/release-crates.txt"
[[ -s "${work}/release-crates.txt" ]] || fDie "cargo tree named no crates"

## Only our crates are handed over as files. A vendored crate's own manifest would
## load as a workspace of its own and fail offline; through ours, each comes in
## as source, with its macros expanded, and its paths stay inside the root.
printf 'paths:\n  - source\n  - knobs\n' >"${work}/config.yml"
fEcho_Clean "CodeQL database ($(wc -l <"${work}/release-crates.txt") crates in the release binary) ..."
CARGO_NET_OFFLINE=true codeql database create "${db}" --language=rust --build-mode=none --source-root="${src}" \
	--codescanning-config="${work}/config.yml" --threads="${CICD_MAX_JOBS}" --quiet \
	-O "rust.cargo_target=${target}" -O rust.cargo_features=default -O rust.extract_dependencies_as_source=true \
	-O "rust.cargo_target_dir=${work}/cargo" >"${work}/create.log" 2>&1 \
	|| { tail -n 40 "${work}/create.log" >&2; fDie "CodeQL database create failed (log in ${work}/create.log)"; }
## The extractor reports a workspace it couldn't load and goes on without macros
## or types, and the database still builds, so the analysis would run on half a
## picture.
if grep -F "failed to load workspace for ${src}/Cargo.toml" "${work}/create.log" >&2; then
	fDie "CodeQL couldn't load the workspace (log in ${work}/create.log)"
fi
fEcho_Clean "CodeQL ${CODEQL_SUITE} ..."
codeql database analyze "${db}" "${CODEQL_SUITE}" --format=sarif-latest --output="${raw}" \
	--threads="${CICD_MAX_JOBS}" --quiet || fDie "CodeQL analyze failed"

## Results in our crates and the release crates are kept, the rest dropped, and
## the kept ones not suppressed are what fails the run.
rc=0
python3 - "${raw}" "${sarif}" "${work}/release-crates.txt" "${root}/${CODEQL_SUPPRESSIONS}" <<'EOF' || rc=$?
import fnmatch, json, sys
raw, out, crates, supp = sys.argv[1:5]
release = {line.strip() for line in open(crates) if line.strip()}
rules = []
for line in open(supp):
	line = line.strip()
	if line and not line.startswith("#"):
		rule, glob, item = line.split()
		rules.append((rule, glob, item))
doc = json.load(open(raw))
failed = 0
for run in doc["runs"]:
	kept = []
	for res in run.get("results", []):
		loc = res["locations"][0]["physicalLocation"]
		path = loc["artifactLocation"]["uri"]
		parts = path.split("/")
		if parts[0] == "vendor" and (len(parts) < 2 or parts[1] not in release):
			continue
		if parts[0] not in ("source", "knobs", "vendor"):
			continue
		kept.append(res)
		line = loc.get("region", {}).get("startLine", 0)
		hit = next((r for r in rules if r[0] == res["ruleId"] and fnmatch.fnmatch(path, r[1])), None)
		tag = f"known, {hit[2]}" if hit else "NEW"
		failed += hit is None
		print(f"    {tag}: {res['ruleId']} {path}:{line}: {res['message']['text'][:160]}")
	run["results"] = kept
json.dump(doc, open(out, "w"), indent=1)
sys.exit(1 if failed else 0)
EOF
((rc == 0)) || fDie "CodeQL found something new (above; SARIF in ${sarif}). Fix it, or file it and add it to ${CODEQL_SUPPRESSIONS}"
fEcho "OK: no new CodeQL results ($((SECONDS - start))s)"


##	History:
##		- 20261022 JC: Created.
##		- 20261026 JC: Jobs and priority from include/jobs.bash.
