#!/usr/bin/env bash
#  shellcheck disable=2034  ## 'variable appears unused.' The lifted function reads them, and shellcheck cannot see into an eval.

##	- Purpose:
##		CARGO_TARGET_DIR may be set, and may be absolute. A stage that assumes
##		'target' builds and then looks for its binary where it was never put -
##		the Windows installer step did exactly that, warned, and went on, so a
##		release could go out with no installers in it.
##		fBuildPackages() is lifted out of cicd.bash and run against the real
##		template and makensis, once with each shape of target directory.
##		The release collection and the Linux packages run too, on stand-in
##		binaries and a stand-in cargo, for the names and checksums download
##		links depend on. And the cross-build flags that keep the Windows
##		builds reproducible.
##	- Test ID: EqAwQq9
##	- History: At bottom of file.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	SPDX-License-Identifier: GPL-2.0-or-later

set -euo pipefail
meDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=cicd/tests/_testdir.bash
source "${meDir}/../_testdir.bash"; fTestDir_Use
realRoot="$(cd "${meDir}/../../.." && pwd)"

failures=0
fCheck(){ local -r what="${1}"; shift; if "${@}"; then echo "  ok   ${what}"; else echo "  FAIL ${what}"; failures=$((failures + 1)); fi; }

if ! command -v makensis >/dev/null 2>&1; then
	echo "  skip installer step (no makensis)"
else
	work="$(mktemp -d)"
	trap 'rc=$?; rm -rf "${work}"; fTestDir_End "${rc}"' EXIT

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
	fWriteSums(){ :; }
	fEcho(){ echo "    $*"; }
	fEcho_Clean(){ echo "    $*"; }
	eval "$(sed -n '/^fBuildPackages(){/,/^}/p' "${realRoot}/cicd/cicd.bash")"

	## $1 is the binary path as stage 5 recorded it - relative to the repository
	## with no CARGO_TARGET_DIR, absolute with one.
	fRunStep() {
		local bin="$1" out="$2"
		local abs="${bin}"
		[[ "${abs}" = /* ]] || abs="${fakeRoot}/${abs}"
		mkdir -p "$(dirname "${abs}")"
		printf 'MZ stand-in\n' > "${abs}"
		artDir="${out}"
		mkdir -p "${artDir}"
		builtArts=("windows-x86_64|${bin}")
		fBuildPackages > "${work}/step.log" 2>&1 || true
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

## The release collection and the Linux packages, lifted out of cicd.bash with
## fReleaseExpects and fWriteSums. The binaries are stand-ins, and so are cargo
## and the two package tools, so what is checked is the names, the calls and
## the checksums.
pkgWork="$(mktemp -d)"
trap 'rc=$?; rm -rf "${work:-}" "${pkgWork}"; fTestDir_End "${rc}"' EXIT
(
	engine="${realRoot}/cicd/cicd.bash"
	fEcho(){ echo "    $*"; }
	fEcho_Clean(){ echo "    $*"; }
	fDie(){ echo "DIE: $*"; exit 1; }
	fWriteBuiltFrom(){ :; }
	eval "$(sed -n '/^fReleaseExpects(){/,/^}/p; /^fWriteSums(){/,/^}/p; /^fBuildPackages(){/,/^}/p' "${engine}")"
	collect="$(sed -n '/^if \[\[ -n "\${RELEASE_ARTIFACT_DIR:-}" \]\]; then$/,/^fi$/p' "${engine}")"
	noArmBlock="$(sed -n '/^if ((noArm)) && declare -p CROSS_TARGETS/,/^fi$/p' "${engine}")"
	mapfile -t shippedCross < <(bash -c 'source "$1" && printf "%s\n" "${CROSS_TARGETS[@]}"' _ "${realRoot}/cicd/config.bash")

	stubs="${pkgWork}/stubs"
	mkdir -p "${stubs}"
	cat >"${stubs}/cargo" <<'STUB'
#!/usr/bin/env bash
echo "$*" >>"${STUB_LOG}"
out=""
while (($#)); do [[ "$1" == --output ]] && out="$2"; shift; done
[[ -z "${out}" ]] || printf 'package %s\n' "${out##*/}" >"${out}"
exit 0
STUB
	printf '#!/bin/sh\nexit 0\n' >"${stubs}/cargo-deb"
	printf '#!/bin/sh\nexit 0\n' >"${stubs}/cargo-generate-rpm"
	chmod +x "${stubs}/cargo" "${stubs}/cargo-deb" "${stubs}/cargo-generate-rpm"
	export STUB_LOG="${pkgWork}/cargo.log"

	## One run of stages 5 and 6 as far as these pieces go. $1 is noArm.
	fStages(){
		local t rest osarch art
		noArm="${1}"; CROSS_TARGETS=("${shippedCross[@]}"); BUILD_CROSS=1
		eval "${noArmBlock}"
		root="${pkgWork}/repo-${1}"
		mkdir -p "${root}/source" "${root}/bin"
		printf '[package]\nname = "silkterm"\nversion = "1.2.3-beta4"\n' >"${root}/source/Cargo.toml"
		printf 'native\n' >"${root}/bin/native"
		builtArts=("linux-x86_64|${root}/bin/native")
		for t in "${CROSS_TARGETS[@]}"; do
			rest="${t#*|}"; osarch="${rest%%|*}"; rest="${rest#*|}"; art="${root}/bin/${osarch}"
			[[ "${rest%%|*}" == *.exe ]] && art+=".exe"
			printf '%s\n' "${osarch}" >"${art}"
			builtArts+=("${osarch}|${art}")
		done
		EXE_NAME=silkterm; RELEASE_NATIVE_OSARCH=linux-x86_64; PACKAGE_ENABLE=1
		RELEASE_ARTIFACT_DIR="art"; VERSION_MANIFEST="source/Cargo.toml"; NSIS_TEMPLATE="no-such-template"
		eval "${collect}"
		: >"${STUB_LOG}"
		PATH="${stubs}:${PATH}" fBuildPackages
	}

	failures=0
	fStages 0 >"${pkgWork}/stages.log" 2>&1 || { sed 's/^/    /' "${pkgWork}/stages.log"; }
	pre="silkterm-1.2.3-beta4"
	fCheck "each binary is collected as <exe>-<version>-<os-arch>" test -f "${artDir}/${pre}-linux-x86_64" -a -f "${artDir}/${pre}-linux-arm64"
	fCheck "with .exe kept on the Windows ones" test -f "${artDir}/${pre}-windows-x86_64.exe" -a -f "${artDir}/${pre}-windows-arm64.exe"
	fCheck "holding the binary it names" test "$(cat "${artDir}/${pre}-windows-arm64.exe")" = "windows-arm64"
	fCheck "the checksums file is <exe>-<version>-sha256sums.txt, and they check" \
		bash -c 'cd "$1" && sha256sum --quiet -c "$2"' _ "${artDir}" "${pre}-sha256sums.txt"
	fCheck "one .deb and one .rpm per Linux arch" test "$(grep -c '^deb ' "${STUB_LOG}")" -eq 2 -a "$(grep -c '^generate-rpm ' "${STUB_LOG}")" -eq 2
	fCheck "the ARM64 ones built for aarch64" test -n "$(grep -E '^deb .*--output [^ ]*linux-arm64\.deb --target aarch64-unknown-linux-gnu$' "${STUB_LOG}")" \
		-a -n "$(grep -E '^generate-rpm .*linux-arm64\.rpm --target aarch64-unknown-linux-gnu --arch aarch64$' "${STUB_LOG}")"
	fCheck "the .rpm version has no dash" grep -qF 'version = "1.2.3~beta4"' "${STUB_LOG}"
	fCheck "and the packages are in the checksums" test "$(grep -cE "  ${pre}-linux-(x86_64|arm64)\.(deb|rpm)$" "${artDir}/${pre}-sha256sums.txt")" -eq 4
	fCheck "which still check" bash -c 'cd "$1" && sha256sum --quiet -c "$2"' _ "${artDir}" "${pre}-sha256sums.txt"
	fCheck "and cover every file there" test "$(wc -l <"${artDir}/${pre}-sha256sums.txt")" -eq "$(find "${artDir}" -type f ! -name '*sha256sums.txt' | wc -l)"

	fStages 1 >"${pkgWork}/stages.log" 2>&1 || { sed 's/^/    /' "${pkgWork}/stages.log"; }
	fCheck "--no-arm leaves x86_64 packages only" test "$(grep -c '^deb ' "${STUB_LOG}")" -eq 1 -a "$(grep -c '^generate-rpm ' "${STUB_LOG}")" -eq 1 \
		-a -z "$(grep -F arm64 "${STUB_LOG}")"
	exit "${failures}"
) || failures=$((failures + $?))

## The Windows linkers write the link time into the PE header unless told not to,
## and then the same commit built twice has two checksums.
fLinkFlag(){ python3 - "${realRoot}/.cargo/config.toml" "$1" "$2" <<'PY'
import sys, tomllib
cfg = tomllib.load(open(sys.argv[1], "rb"))
flags = cfg.get("target", {}).get(sys.argv[2], {}).get("rustflags", [])
sys.exit(0 if sys.argv[3] in flags else 1)
PY
}
fCheck "Windows x86_64 links with no timestamp" fLinkFlag x86_64-pc-windows-gnu "link-arg=-Wl,--no-insert-timestamp"
fCheck "Windows ARM64 links reproducibly" fLinkFlag aarch64-pc-windows-gnullvm "link-arg=-Wl,-Brepro"

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
##		- 20260926 JC: Release names and checksums, the Linux packages per arch,
##		               and the reproducible Windows link flags.
