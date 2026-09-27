#!/usr/bin/env bash

##	- Purpose:
##		gfs_rotate prunes the run logs and flamegraphs. A wrong role or count
##		would quietly delete old ones, so this fills a scratch folder with two
##		years of dated files, rotates it at a fixed time, and checks what is left:
##		the first file, the newest few, and the last file of each recent hour,
##		day, week and month, about thirty in all, and nothing else touched.
##	- Test ID: Er2UgY9
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cicd="$(cd "${meDir}/../.." && pwd)"
# shellcheck source=cicd/utility/include/gfs-rotate.bash
source "${cicd}/utility/include/gfs-rotate.bash"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

work="$(mktemp -d "${TMPDIR:-/tmp}/silk-rotate.XXXXXX")"
trap 'rm -rf "${work}"' EXIT
unset GFS_KEEP_FREQUENT GFS_KEEP_HOURLY GFS_KEEP_DAILY GFS_KEEP_WEEKLY GFS_KEEP_MONTHLY GFS_KEEP_YEARLY
export TZ=UTC
export GFS_NOW
GFS_NOW="$(date -d '2026-09-26 12:30:00' +%s)"

## 200 runs: 80 over the last three days, about an hour apart, and 120 spread
## over the two years before that.
dir="${work}/logs"
mkdir -p "${dir}"
declare -a stamps=()
for ((i = 1; i <= 80; i++)); do stamps+=("$(date -d "@$((GFS_NOW - i * 3300))" +%Y%m%d-%H%M%S)"); done
for ((i = 0; i < 120; i++)); do stamps+=("$(date -d "@$((GFS_NOW - 3 * 86400 - i * 527000))" +%Y%m%d-%H%M%S)"); done
for s in "${stamps[@]}"; do : >"${dir}/run_${s}.log"; done
printf '%s\n' "${stamps[@]}" >"${work}/stamps.txt"

## Neighbours the rotation must leave alone: another prefix, another extension,
## and a name with no date at all.
echo a >"${dir}/flame_20250101-000000.svg"
echo b >"${dir}/run_20250101-000000.txt"
echo c >"${dir}/notes.log"
neighbours="$(cd "${dir}" && sha256sum flame_20250101-000000.svg run_20250101-000000.txt notes.log)"

gfs_rotate "${dir}" run log >/dev/null
(cd "${dir}" && ls run_*.log) >"${work}/kept.txt"

fVerify(){ python3 - "${work}/stamps.txt" "${work}/kept.txt" "${GFS_NOW}" <<'PY'
import sys
from datetime import datetime, timezone
stamps = sorted(open(sys.argv[1]).read().split())
kept = open(sys.argv[2]).read().split()
now = datetime.fromtimestamp(int(sys.argv[3]), timezone.utc)
when = lambda s: datetime.strptime(s, "%Y%m%d-%H%M%S").replace(tzinfo=timezone.utc)
periods = {
	"hour": lambda t: t.strftime("%Y%m%d%H"),
	"day": lambda t: t.strftime("%Y%m%d"),
	"week": lambda t: t.strftime("%G%V"),
	"month": lambda t: t.strftime("%Y%m"),
	"year": lambda t: t.strftime("%Y"),
}
limits = {"frequent": 10, "hour": 4, "day": 5, "week": 4, "month": 4, "year": 2}
bad = []
roles = {}
for name in kept:
	stem = name[len("run_"):-len(".log")]
	stamp, _, role = stem.rpartition("_")
	if stamp not in stamps:
		bad.append(f"{name} was not one of the runs")
	roles.setdefault(role, []).append(stamp)
if not 20 <= len(kept) <= 30:
	bad.append(f"{len(kept)} kept, not about thirty")
if roles.get("first") != [stamps[0]]:
	bad.append(f"first is {roles.get('first')}, not the oldest run {stamps[0]}")
if roles.get("latest") != [stamps[-1]]:
	bad.append(f"latest is {roles.get('latest')}, not the newest run {stamps[-1]}")
newest = set(stamps[-10:])
keptStamps = {name[4:19] for name in kept}
if not newest <= keptStamps:
	bad.append(f"of the newest ten runs, {sorted(newest - keptStamps)} went")
for s in roles.get("frequent", []):
	if s not in newest:
		bad.append(f"{s} is frequent but not among the newest ten")
for role, key in periods.items():
	got = roles.get(role, [])
	if len(got) > limits[role]:
		bad.append(f"{len(got)} {role} files, over the {limits[role]} kept")
	for s in got:
		p = key(when(s))
		if p == key(now):
			bad.append(f"{s} is tagged {role} while that {role} is still open")
		if max(x for x in stamps if key(when(x)) == p) != s:
			bad.append(f"{s} is tagged {role} but is not the last run of it")
	ended = sorted({key(when(x)) for x in stamps if key(when(x)) != key(now)})
	if ended:
		last = max(x for x in stamps if key(when(x)) == ended[-1])
		if last not in keptStamps:
			bad.append(f"the last run of the latest ended {role}, {last}, went")
	if role != "year" and not got:
		bad.append(f"no {role} file kept")
for line in bad:
	print(f"    {line}")
sys.exit(1 if bad else 0)
PY
}
fCheck "what is kept is the first, the newest ten, and one per ended period" fVerify
fCheck "the neighbours are untouched" test "$(cd "${dir}" && sha256sum flame_20250101-000000.svg run_20250101-000000.txt notes.log)" = "${neighbours}"
out="$(gfs_rotate "${dir}" run log)"
fCheck "a second run changes nothing" test -z "${out}" -a "$(cd "${dir}" && ls run_*.log)" = "$(cat "${work}/kept.txt")"

## A run an hour later takes over the latest role.
: >"${dir}/run_$(date -d "@$((GFS_NOW + 3600))" +%Y%m%d-%H%M%S).log"
GFS_NOW=$((GFS_NOW + 3600)) gfs_rotate "${dir}" run log >/dev/null
fCheck "a new run takes the latest role" test "$(cd "${dir}" && ls ./*_latest.log | wc -l)" -eq 1 -a -e "${dir}/run_$(date -d "@$((GFS_NOW + 3600))" +%Y%m%d-%H%M%S)_latest.log"
fCheck "and the first run is still kept" test -e "${dir}/run_$(head -1 < <(sort "${work}/stamps.txt"))_first.log"

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260926 JC: Created.
