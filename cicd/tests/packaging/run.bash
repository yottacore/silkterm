#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted function reads them, and shellcheck cannot see into an eval.

##	- Purpose:
##		CARGO_TARGET_DIR may be set, and may be absolute. A stage that assumes
##		'target' builds and then looks for its binary where it was never put -
##		the Windows installer step did exactly that, warned, and went on, so a
##		release could go out with no installers in it.
##		build_packages() is lifted out of cicd.bash and run against the real
##		template and makensis, once with each shape of target directory.
##	- Test ID: EqAwQq9
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
realRoot="$(cd "${meDir}/../../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

if ! command -v makensis >/dev/null 2>&1; then
	echo "  skip installer step (no makensis)"
else
	work="$(mktemp -d)"
	trap 'rm -rf "${work}"' EXIT

	## A stand-in repository holding nothing but the template and icon the step reads.
	fakeRoot="${work}/repo"
	mkdir -p "${fakeRoot}/cicd/packaging/windows" "${fakeRoot}/source/assets"
	cp "${realRoot}/cicd/packaging/windows/installer.nsi.in" "${fakeRoot}/cicd/packaging/windows/"
	cp "${realRoot}/source/assets/icon.ico" "${fakeRoot}/source/assets/"

	## The step's world, as cicd.bash sets it up around the call.
	PACKAGE_ENABLE=1
	EXE_NAME="silkterm"
	NSIS_TEMPLATE="cicd/packaging/windows/installer.nsi.in"
	RELEASE_ARTIFACT_DIR="cicd/artifacts/release"   ## only ever printed
	ver="0.0.1-beta2"
	root="${fakeRoot}"
	write_sums(){ :; }
	fEcho(){ echo "    $*"; }
	fEcho_Clean(){ echo "    $*"; }
	eval "$(sed -n '/^build_packages(){/,/^}/p' "${realRoot}/cicd/cicd.bash")"

	## $1 is the binary path as stage 5 recorded it - relative to the repository
	## with no CARGO_TARGET_DIR, absolute with one.
	fRunStep() {
		local bin="$1" out="$2"
		local abs="${bin}"
		[[ "${abs}" = /* ]] || abs="${fakeRoot}/${abs}"
		mkdir -p "$(dirname "${abs}")"
		printf 'MZ stand-in\n' > "${abs}"
		art_dir="${out}"
		mkdir -p "${art_dir}"
		built_arts=("windows-x86_64|${bin}")
		build_packages > "${work}/step.log" 2>&1 || true
	}

	setup="${work}/art-rel/silkterm-${ver}-windows-x86_64-setup.exe"
	fRunStep "target/x86_64-pc-windows-gnu/release/silkterm.exe" "${work}/art-rel"
	fCheck "a relative target dir still makes the installer" test -f "${setup}"

	## It carries the program's icon, not the stock one, and a version block.
	fHasIcon(){ python3 - "$1" "${realRoot}/source/assets/icon.ico" <<'PY'
import struct, sys
exe, ico = (open(p, "rb").read() for p in sys.argv[1:3])
size, off = struct.unpack_from("<II", ico, 6 + 16 * 2 + 8)
sys.exit(0 if ico[off:off + size] in exe else 1)
PY
	}
	fCheck "and it carries the program's icon" fHasIcon "${setup}"
	fCheck "and a version block" python3 "${realRoot}/cicd/utility/pe-resources.py" --require icon,version "${setup}"
	fCheck "naming the release, pre-release tag and all" \
		python3 -c 'import sys; sys.exit(sys.argv[1].encode("utf-16-le") not in open(sys.argv[2], "rb").read())' "${ver}" "${setup}"

	fRunStep "${work}/elsewhere/x86_64-pc-windows-gnu/release/silkterm.exe" "${work}/art-abs"
	fCheck "an absolute target dir makes it too" \
		test -f "${work}/art-abs/silkterm-${ver}-windows-x86_64-setup.exe"
	if [[ ! -f "${work}/art-abs/silkterm-${ver}-windows-x86_64-setup.exe" ]]; then
		sed -n '1,20p' "${work}/step.log" | sed 's/^/    /'
	fi
fi

## The Linux packages' icons are the program's own, the images inside icon.ico,
## so the two cannot drift apart.
fIconsMatch(){ python3 - "${realRoot}" <<'PY'
import struct, sys
root = sys.argv[1]
ico = open(f"{root}/source/assets/icon.ico", "rb").read()
bad = 0
for i in range(struct.unpack_from("<H", ico, 4)[0]):
	size, off = struct.unpack_from("<II", ico, 6 + 16 * i + 8)
	side = ico[6 + 16 * i] or 256
	try:
		png = open(f"{root}/cicd/packaging/linux/icons/{side}x{side}/silkterm.png", "rb").read()
	except OSError:
		png = b""
	if png != ico[off:off + size]:
		print(f"    {side}x{side} differs from icon.ico")
		bad += 1
sys.exit(1 if bad else 0)
PY
}
fCheck "the Linux icons are the ones in icon.ico" fIconsMatch

## The Windows pipeline looks for the same binaries and has the same rule.
if command -v pwsh >/dev/null 2>&1; then
	rc=0
	pwsh -NoProfile -File "${meDir}/target-dir.ps1" || rc=$?
	fCheck "cicd-win.ps1 resolves the target directory the same way" test "${rc}" -eq 0
else
	echo "  skip cicd-win.ps1 target dir (no pwsh)"
fi

if ((failures)); then echo "${failures} failed"; exit 1; fi
echo "all passed"

##	History:
##		- 20260917 JC: Created.
##		- 20260925 JC: The installer's icon and version block, and the Linux icons.
