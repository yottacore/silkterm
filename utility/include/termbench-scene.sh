#!/bin/dash
#  shellcheck disable=2086  ## $BENCH_ARGS is a multi-argument string and has to split.
#                              $LABEL must NOT: it can contain a space, and splitting it
#                              silently killed every run of the first campaign.
# Runs inside the terminal under test.
#
# While it waits for the go file it reports its own grid, which is what lets one
# fitter size any terminal to the same grid without knowing that terminal's geometry
# options or cell metrics.

##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

cd "${REPO_DIR}" || exit 1

# The terminal was started on a throwaway account. The measuring tool keeps its run
# history under the real one, so it gets that back; the terminal is already running.
if [ -n "${BENCH_REAL_HOME:-}" ]; then
	HOME="${BENCH_REAL_HOME}"
	if [ -n "${BENCH_REAL_XDG_DATA_HOME:-}" ]; then
		export XDG_DATA_HOME="${BENCH_REAL_XDG_DATA_HOME}"
	else
		unset XDG_DATA_HOME
	fi
	export HOME
fi

while [ ! -f "${GO_FILE}" ]; do
	stty size > "${SIZE_FILE}.tmp" 2>/dev/null && mv "${SIZE_FILE}.tmp" "${SIZE_FILE}"
	sleep 0.2
done

# stdout has to stay on the tty: the tool stops its clock on the terminal's own reply,
# so redirecting it would measure a pipe instead. The report goes out via --out.
python3 utility/include/termbench.py ${BENCH_ARGS} --label "${LABEL}" --out "${OUT_FILE}" 2>"${OUT_FILE}.err"
echo "exit=$?" > "${OUT_FILE}.done"

##	History:
##		- 20260730 JC: Created.
