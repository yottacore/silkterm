#!/usr/bin/env bash

##	Purpose:
##		- The one way a pipeline script removes a folder tree, per Safe removal in
##		  the CICD rules. Source this. It defines functions and runs nothing.
##		- fSafeRm <path>: only somewhere under a folder named tmp, never that
##		  folder itself, or under this project's own folder in HOST_TEMP, since
##		  the macOS and Windows temp folders aren't named tmp. With HOST_TEMP
##		  set, never a whole folder right under it either, where other runs and
##		  sessions keep theirs.
##		- fSafeRm_Under <parent> <path>: only somewhere under the one parent
##		  named, for a folder outside a temp root, such as the target dir.
##		- Neither takes HOST_TEMP itself or this project's folder in it. Where
##		  HOST_TEMP is unset, that's TMPDIR or /tmp, as in Temp files.
##		- Both take one absolute path and refuse an empty one, "..", "//", a
##		  newline, a link, a path through a link, or one that isn't there. They
##		  stay on one filesystem, and say nothing when the removal went. A
##		  read-only folder inside is opened first, so it can't stop the removal.
##	History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT

##	Resolved once, since macOS's temp folder is behind the /var link.
## A caller under set -e with no realpath on PATH, like the scroll verdict test, must still load.
## fSafeRm needs realpath anyway, so it refuses every path there.
__safeRmHost="$(realpath -m -- "${HOST_TEMP:-${TMPDIR:-/tmp}}" 2>/dev/null)" || __safeRmHost=""
__safeRmOwn="${__safeRmHost%/}/${EXE_NAME:-silkterm}"
##	Without HOST_TEMP the root is /tmp, and the short socket folders are right in it.
__safeRmHostSet="${HOST_TEMP:+1}"

fSafeRm(){ fSafeRm_Do "" "${@}"; }

fSafeRm_Under(){
	local -r parent="${1:-}"
	if [[ -z "${parent}" || "${parent}" != /?* ]]; then printf 'safe-rm: refused, no parent: "%s"\n' "${parent}" >&2; return 1; fi
	shift
	fSafeRm_Do "${parent%/}" "${@}"
}

fSafeRm_Do(){
	local -r parent="${1}"
	shift
	local p="${1:-}" real=""
	if [[ "${#}" -ne 1 || -z "${p}" || "${p}" != /* || "${p}" == *..* || "${p}" == *//* || "${p}" == *$'\n'* ]]; then
		printf 'safe-rm: refused, bad path: "%s"\n' "${p}" >&2; return 1
	fi
	p="${p%/}"
	if [[ -n "${parent}" ]]; then
		if [[ "${p}" != "${parent}"/?* ]]; then printf 'safe-rm: refused, not under %s: %s\n' "${parent}" "${p}" >&2; return 1; fi
	elif ! [[ "${p}" =~ ^/(.+/)?tmp/[^/]+(/[^/]+)*$ || "${p}" == "${__safeRmOwn}"/?* ]]; then
		printf 'safe-rm: refused, not under a tmp folder: %s\n' "${p}" >&2; return 1
	fi
	if [[ "${p}" == "${__safeRmHost}" || "${p}" == "${__safeRmOwn}" || ( -z "${parent}" && -n "${__safeRmHostSet}" && "${p%/*}" == "${__safeRmHost%/}" ) ]]; then
		printf 'safe-rm: refused, a whole project temp folder: %s\n' "${p}" >&2; return 1
	fi
	if [[ -L "${p}" ]]; then printf 'safe-rm: refused, a link: %s\n' "${p}" >&2; return 1; fi
	real="$(realpath -e -- "${p}" 2>/dev/null)" || { printf 'safe-rm: refused, not there: %s\n' "${p}" >&2; return 1; }
	if [[ "${real}" != "${p}" ]]; then printf 'safe-rm: refused, goes through a link: %s\n' "${p}" >&2; return 1; fi
	## A folder a test made read-only would stop rm partway, after it took the
	## run.pid the stale sweep goes by. Each one is opened before find reads it.
	find "${p}" -xdev -type d ! -perm -u=rwx -exec chmod u+rwx -- {} \; 2>/dev/null || true
	rm -rf --one-file-system -- "${p}"
}

##	History:
##		- 20261028 JC: Created.
##		- 20261031: HOST_TEMP itself and a whole project folder in it are refused.
##		- 20261106: A read-only folder inside is opened first, so the tree goes whole.
