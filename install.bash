#!/usr/bin/env bash
# shellcheck disable=SC2317  ## unreachable-after-exit false positives in dispatchers

##	- Purpose: One-liner installer for a single-binary GitHub release. Detects the
##	  OS and CPU, works out which release asset that is, verifies its sha256
##	  against the release's checksums file, and installs it. Idempotent: states
##	  its plan, asks before touching anything, and does nothing when the
##	  installed binary is already current.
##	- Reusable: everything project-specific lives in the settings block below.
##	- Syntax:
##	  bash <(curl -fsSL https://raw.githubusercontent.com/yottacore/silkterm/main/install.bash) [options]
##	- Options: --release stable|dev, --target user|system, --yes, --version, --help.
##	  The OS, the CPU architecture and the asset name are all detected.
##	- Needs: bash >= 3.2 (the macOS system bash; tested on 5 only), curl or
##	  wget, and one of sha256sum / shasum / openssl.
##	- History:
##	  - 20260723 JC: Created.
##	  - 20260806 JC: Made project-agnostic; dropped --arch for autodetection;
##	                 added --version; targets bash 3.2.
##	  - 20260924 JC: --release or --target with no value says so rather than
##	                 exiting silently; the tag lookup no longer needs the API's
##	                 pretty-printed layout.
##	  - 20260925 JC: Picks the highest version from the release list and skips
##	                 drafts; an API error no longer reads as "no full release";
##	                 a re-run puts back a missing launcher.

##	Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


##	•••••••••••••••••••  Per-project settings - edit only these  ••••••••••••••••••

installerVersion="1.3.0"
ownerRepo="yottacore/silkterm"
appName="SilkTerm"
exeName="silkterm"
appComment="Smooth-scrolling GPU terminal with split panes"

##	Release asset names. {exe} {version} {os} {arch} {ext} are substituted; {ext}
##	is ".exe" on Windows and empty elsewhere. {os} is linux/macos/freebsd/windows,
##	{arch} is x86_64/arm64 - match whatever the release actually publishes.
assetPattern="{exe}-{version}-{os}-{arch}{ext}"
sumsPattern="{exe}-{version}-sha256sums.txt"

##	Freedesktop launcher (Linux only). 0 for a non-GUI program.
menuEntry=1
desktopGenericName="Terminal"
desktopIcon="utilities-terminal"
desktopCategories="System;TerminalEmulator;"
desktopKeywords="terminal;shell;prompt;command;"

##	••••••••••••••••••••••••  End per-project settings  ••••••••••••••••••••••••••

apiBase="https://api.github.com/repos/${ownerRepo}"
dlBase="https://github.com/${ownerRepo}/releases/download"
rawBase="https://raw.githubusercontent.com/${ownerRepo}/main"


##	Output helpers

##	fFail "message" ["hint" ...] - one error line, then any hints, then exit.
function fFail() {
	local first="$1"; shift
	echo "Error: ${first}" >&2
	while [ "$#" -gt 0 ]; do echo "  ${1}" >&2; shift; done
	echo >&2
	exit 1
}

function fHelp() {
	cat <<EOF
${appName} installer ${installerVersion}

Downloads the newest ${appName} release from GitHub, checks its sha256, and
installs it. It prints what it is about to do and asks first, and it does
nothing at all when the installed copy is already current.

Usage:
  bash <(curl -fsSL ${rawBase}/install.bash) [options]

Options:
  --release stable|dev   stable (default): newest full release
                         dev:              newest release, pre-releases included
  --target  user|system  user (default):   \$HOME (no root needed)
                         system:           /usr/local (uses sudo)
  --yes, -y              skip the confirmation prompt
  --version              print this installer's version and exit
  --help, -h             this text

The operating system, the CPU architecture and the matching release asset are
all detected - there is nothing to pass for them.

EOF
}

##	Lowercase, the 3.2 way - \${x,,} is bash 4.
function fLower() { echo "$1" | tr '[:upper:]' '[:lower:]'; }


##	Network + hashing (curl preferred, wget accepted so minimal images work)

##	fGet <url> <outfile> - quiet fetch. Returns non-zero on any HTTP or
##	transport error rather than writing a "404: not found" page to the file.
function fGet() {
	local url="$1" out="$2"
	if [ "${dlTool}" = "curl" ]; then
		curl -fsSL "${httpsOnly_curl[@]}" -o "${out}" "${url}"
	else
		wget -qO "${out}" "${httpsOnly_wget[@]}" "${url}"
	fi
}

##	fApi <url> <outfile> - an API call, carrying the optional token. Only the API
##	is rate-limited per IP; release downloads are not, so they stay anonymous.
##	The body is kept whatever the status, since GitHub says why in it, and
##	anything else worth showing goes to <outfile>.err. Fails on any non-2xx.
##	Https on the first request and on every redirect after it. Without this a
##	redirect can walk the download down to plain http, which is where the
##	checksum stops meaning anything.
httpsOnly_curl=(--proto '=https' --proto-redir '=https')
httpsOnly_wget=(--https-only)

##	The token goes in a file rather than on the command line, where 'ps' shows it
##	to every account on the box. Written 0600 inside a directory only we can read,
##	and removed by the EXIT trap. Set up once, from fMain, before the first API
##	call.
authDir=""
authFile=""
function fAuthInit() {
	[ -n "${apiToken}" ] || return 0
	authDir="$(mktemp -d 2>/dev/null)" || return 1
	chmod 700 "${authDir}" 2>/dev/null
	authFile="${authDir}/auth"
	##	The two tools read their own config format, and only curl's takes
	##	quotes - wget's rc keeps everything after the '=' verbatim.
	if [ "${dlTool}" = "curl" ]; then
		( umask 077; printf 'header = "Authorization: Bearer %s"\n' "${apiToken}" >"${authFile}" )
	else
		( umask 077; printf 'header = Authorization: Bearer %s\n' "${apiToken}" >"${authFile}" )
	fi
}

function fApi() {
	local url="$1" out="$2" code="" rc=0
	if [ "${dlTool}" = "curl" ]; then
		if [ -n "${authFile}" ]; then
			code="$(curl -sSL "${httpsOnly_curl[@]}" --config "${authFile}" -o "${out}" -w '%{http_code}' "${url}" 2>"${out}.err")" || rc=$?
		else
			code="$(curl -sSL "${httpsOnly_curl[@]}" -o "${out}" -w '%{http_code}' "${url}" 2>"${out}.err")" || rc=$?
		fi
		[ "${rc}" = "0" ] || return 1
		case "${code}" in 2??) return 0 ;; esac
		echo "HTTP ${code}" >>"${out}.err"
		return 1
	fi
	##	wget takes no header file, but it reads one out of a config file. -nv is
	##	quiet on success and still names a failure.
	if [ -n "${authFile}" ]; then
		WGETRC="${authFile}" wget -nv --content-on-error -O "${out}" "${httpsOnly_wget[@]}" "${url}" 2>"${out}.err"
	else
		wget -nv --content-on-error -O "${out}" "${httpsOnly_wget[@]}" "${url}" 2>"${out}.err"
	fi
}

##	Same, but shows progress - the release binary is the only big download.
function fGetShown() {
	local url="$1" out="$2"
	if [ "${dlTool}" = "curl" ]; then
		curl -fSL "${httpsOnly_curl[@]}" --progress-bar -o "${out}" "${url}"
	else
		wget -q "${httpsOnly_wget[@]}" --show-progress -O "${out}" "${url}"
	fi
}

##	The release signing key, as one allowed_signers line. Empty until a key is
##	generated (see cicd/config.bash), and then the checksums file is only trusted
##	when it carries a good signature by this key - which is what turns the check
##	below from "the download was not corrupted" into "this came from the author".
releaseSignPubkey=""
releaseSignIdentity="releases@silkterm"
releaseSignNamespace="silkterm-release"

##	Verify the checksums file against the pinned key. Everything else is covered
##	by the checksums, so this one signature covers the whole release.
function fVerifySignature() {
	local dir="$1" sums="$2" tag="$3"
	if [ -z "${releaseSignPubkey}" ]; then
		echo "Note: this release is not signed; the download is checked against its checksums only."
		return 0
	fi
	command -v ssh-keygen >/dev/null 2>&1 \
		|| fFail "ssh-keygen not found, and this release is signed" \
			"Install OpenSSH (openssh-client) and re-run."
	fGet "${dlBase}/${tag}/${sums}.sig" "${dir}/${sums}.sig" \
		|| fFail "release ${tag} carries no signature (${sums}.sig)" \
			"This installer only accepts signed releases." \
			"Release page: https://github.com/${ownerRepo}/releases/tag/${tag}"
	printf '%s %s\n' "${releaseSignIdentity}" "${releaseSignPubkey}" > "${dir}/allowed_signers"
	ssh-keygen -Y verify -f "${dir}/allowed_signers" -I "${releaseSignIdentity}" \
		-n "${releaseSignNamespace}" -s "${dir}/${sums}.sig" < "${dir}/${sums}" >/dev/null 2>&1 \
		|| fFail "the release signature does not verify - NOT installing" \
			"The checksums file was not signed by the release key." \
			"Do not use this download; report it."
	echo "Signature OK."
}

function fSha256() {
	if [ -n "${shaTool}" ]; then
		case "${shaTool}" in
			sha256sum) sha256sum "$1" | awk '{print $1}' ;;
			shasum)    shasum -a 256 "$1" | awk '{print $1}' ;;
			openssl)   openssl dgst -sha256 "$1" | awk '{print $NF}' ;;
		esac
	fi
}

##	One line per release in a GitHub API list: tag, draft or -, pre or full.
##	Without jq: the body goes onto one line (JSON strings hold no raw newline),
##	then each "tag_name" starts a new one. A release's "draft" and "prerelease"
##	come after its tag and before the next release's, so each line carries its
##	own. An escaped quote in a release's notes cannot match, since the pattern
##	needs a bare quote after the name.
function fReleaseRows() {
	tr -d '\r\n' | sed 's/"tag_name"/\
"tag_name"/g' | awk '/^"tag_name"/ {
		tag = $0
		sub(/^"tag_name"[ \t]*:[ \t]*"/, "", tag); sub(/".*/, "", tag)
		draft = ($0 ~ /"draft"[ \t]*:[ \t]*true/) ? "draft" : "-"
		pre = ($0 ~ /"prerelease"[ \t]*:[ \t]*true/) ? "pre" : "full"
		if (tag != "") print tag, draft, pre
	}'
}

##	fFieldCmp <a> <b> - prints -1, 0 or 1 for one dotted field. Numbers compare
##	as numbers and sort below words. Two words with the same letters and a
##	trailing number compare by that number, so beta10 is above beta3.
function fFieldCmp() {
	local a="$1" b="$2" num='^[0-9]+$' word='^([^0-9]*)([0-9]+)$'
	if [[ $a =~ $num ]] && [[ $b =~ $num ]]; then
		if [ "$a" -gt "$b" ]; then echo 1; elif [ "$a" -lt "$b" ]; then echo -1; else echo 0; fi
		return 0
	fi
	if [[ $a =~ $num ]]; then echo -1; return 0; fi
	if [[ $b =~ $num ]]; then echo 1; return 0; fi
	local aStem="" aNum="" bStem="" bNum=""
	if [[ $a =~ $word ]]; then aStem="${BASH_REMATCH[1]}"; aNum="${BASH_REMATCH[2]}"; fi
	if [[ $b =~ $word ]]; then bStem="${BASH_REMATCH[1]}"; bNum="${BASH_REMATCH[2]}"; fi
	if [ -n "${aNum}" ] && [ -n "${bNum}" ] && [ "${aStem}" = "${bStem}" ]; then
		fFieldCmp "${aNum}" "${bNum}"
		return 0
	fi
	if [[ $a < $b ]]; then echo -1; elif [[ $a > $b ]]; then echo 1; else echo 0; fi
}

##	fListCmp <a> <b> <missing> - dotted lists, field by field. <missing> is what
##	a list that runs out first counts as: -1 for a pre-release, where fewer
##	fields sort lower, or 0 for the core, where 1.0 is 1.0.0.
function fListCmp() {
	local a="$1" b="$2" missing="$3" c=""
	while [ -n "${a}" ] || [ -n "${b}" ]; do
		if [ -z "${a}" ]; then
			if [ "${missing}" = "0" ]; then a="0"; else echo -1; return 0; fi
		fi
		if [ -z "${b}" ]; then
			if [ "${missing}" = "0" ]; then b="0"; else echo 1; return 0; fi
		fi
		c="$(fFieldCmp "${a%%.*}" "${b%%.*}")"
		[ "${c}" = "0" ] || { echo "${c}"; return 0; }
		case "${a}" in *.*) a="${a#*.}" ;; *) a="" ;; esac
		case "${b}" in *.*) b="${b#*.}" ;; *) b="" ;; esac
	done
	echo 0
}

##	fNewer <a> <b> - true when tag <a> is a higher version than <b>, in semver
##	order: 1.0.0-alpha.2 is below 1.0.0, where sort -V and git put it above.
function fNewer() {
	local a="${1#v}" b="${2#v}" aPre="" bPre="" c=""
	a="${a%%+*}"; b="${b%%+*}"
	case "${a}" in *-*) aPre="${a#*-}"; a="${a%%-*}" ;; esac
	case "${b}" in *-*) bPre="${b#*-}"; b="${b%%-*}" ;; esac
	c="$(fListCmp "${a}" "${b}" 0)"
	[ "${c}" = "0" ] || { [ "${c}" = "1" ]; return; }
	##	Same core: a release is above any of its pre-releases.
	[ -n "${aPre}" ] || { [ -n "${bPre}" ]; return; }
	[ -n "${bPre}" ] || return 1
	[ "$(fListCmp "${aPre}" "${bPre}" -1)" = "1" ]
}

##	fPickTag <stable|dev> - the highest version in the release rows on stdin,
##	skipping drafts, and pre-releases too for stable. Prints nothing if none.
function fPickTag() {
	local want="$1" tag draft pre best=""
	while read -r tag draft pre; do
		[ "${draft}" = "-" ] || continue
		[ "${want}" = "dev" ] || [ "${pre}" = "full" ] || continue
		if [ -z "${best}" ] || fNewer "${tag}" "${best}"; then best="${tag}"; fi
	done
	[ -z "${best}" ] || echo "${best}"
}

##	True when the deepest existing parent of $1 is writable by us.
function fCanWrite() {
	local dir="$1"
	while [ -n "${dir}" ] && [ "${dir}" != "/" ] && [ ! -e "${dir}" ]; do
		dir="$(dirname "${dir}")"
	done
	[ -w "${dir}" ]
}

##	Exec= is read twice: the desktop-entry string rules first, then the Exec
##	quoting rules on top. So a backslash in the path ends up as four, a quote,
##	backtick or '$' as two-plus-itself, and a literal '%' has to be doubled or it
##	reads as a field code. The whole value is quoted, which is what a space needs.
function fDesktopExec() {
	printf '%s' "$1" | sed -e 's/[\\"`$]/\\&/g' -e 's/\\/\\\\/g' -e 's/%/%%/g'
}

##	fPathNote <dir> <file> - how to run it when <dir> is not on PATH.
function fPathNote() {
	case ":${PATH}:" in *":${1}:"*) return 0 ;; esac
	echo
	echo "Note: ${1} is not on your PATH, so '${exeName}' won't be found by name yet."
	echo "  Add it with:  echo 'export PATH=\"${1}:\$PATH\"' >> ~/.profile"
	echo "  Until then, run it in full:  ${2}"
}

##	Scratch space. Global on purpose: the EXIT trap fires after fMain has
##	returned, so a local would be out of scope by then (and `set -u` turns that
##	into a failed exit status on an otherwise perfect install).
tmpDir=""
function fCleanup() {
	[ -z "${tmpDir}" ]  || rm -rf "${tmpDir}"
	[ -z "${authDir}" ] || rm -rf "${authDir}"
}

##	0 = yes, 1 = no, 2 = could not ask at all.
##	The terminal comes FIRST because the `curl ... | bash` form leaves the script
##	itself sitting on stdin - reading the answer from there would eat the script.
##	Falling back to stdin is what keeps a piped `echo y | ...` working.
function fConfirm() {
	local answer=""
	printf "%s [y/N]: " "$1"
	if { : </dev/tty; } 2>/dev/null; then
		read -r answer </dev/tty || { echo; return 2; }
	else
		read -r answer || { echo; return 2; }
	fi
	case "${answer}" in y|Y|yes|Yes|YES) return 0 ;; *) return 1 ;; esac
}


function fMain() {

	echo

	##	Detect the platform first so --help can describe it, but defer any
	##	"unsupported" failure until after --help/--version have had their say.
	local osName osToken="" archToken="" exeExt="" osProblem=""
	osName="$(uname -s 2>/dev/null || echo unknown)"
	case "$(fLower "${osName}")" in
		linux)                       osToken="linux" ;;
		darwin)                      osToken="macos" ;;
		freebsd)                     osToken="freebsd" ;;
		openbsd)                     osToken="openbsd" ;;
		netbsd)                      osToken="netbsd" ;;
		dragonfly)                   osToken="dragonfly" ;;
		mingw*|msys*|cygwin*)        osToken="windows"; exeExt=".exe" ;;
		*)                           osProblem="unrecognized operating system: ${osName}" ;;
	esac
	case "$(fLower "$(uname -m 2>/dev/null || echo unknown)")" in
		x86_64|amd64|x64)            archToken="x86_64" ;;
		aarch64|arm64)               archToken="arm64" ;;
		i386|i486|i586|i686)         osProblem="32-bit x86 is not supported" ;;
		arm*)                        osProblem="32-bit ARM is not supported" ;;
		*)                           osProblem="unrecognized CPU architecture: $(uname -m 2>/dev/null)" ;;
	esac

	##	Parse arguments
	local release="stable" target="user" assumeYes=0
	while [ "$#" -gt 0 ]; do case "$1" in
		--release)   [ "$#" -ge 2 ] || fFail "--release needs a value: stable or dev"; release="$2"; shift 2 ;;
		--release=*) release="${1#*=}"; shift ;;
		--target)    [ "$#" -ge 2 ] || fFail "--target needs a value: user or system"; target="$2"; shift 2 ;;
		--target=*)  target="${1#*=}"; shift ;;
		--yes|-y)    assumeYes=1; shift ;;
		--version)   echo "${appName} installer ${installerVersion}"; echo; exit 0 ;;
		-h|--help)   fHelp; exit 0 ;;
		*)           fFail "unknown option: $1" "Run with --help to see the options." ;;
	esac; done
	case "${release}" in stable|dev) : ;; *) fFail "--release must be stable or dev (got '${release}')" ;; esac
	case "${target}" in user|system) : ;; *) fFail "--target must be user or system (got '${target}')" ;; esac

	[ -z "${osProblem}" ] || fFail "${osProblem}" \
		"No ${appName} build is published for this platform." \
		"Building from source: https://github.com/${ownerRepo}#build-it-yourself"
	if [ "${osToken}" = "windows" ]; then
		fFail "this is the Windows shell environment (${osName})" \
			"Use the PowerShell installer instead - it also sets up the Start Menu entry and PATH:" \
			"  irm ${rawBase}/install.ps1 | iex"
	fi

	##	Tools. curl/wget only need to exist for one of them; a hash tool is
	##	non-negotiable, since an unverified binary must never be installed.
	local dlTool="" shaTool=""
	if command -v curl >/dev/null 2>&1; then dlTool="curl"
	elif command -v wget >/dev/null 2>&1; then dlTool="wget"
	else fFail "neither curl nor wget is installed" "Install one of them and re-run (for example: sudo apt install curl)."
	fi
	if command -v sha256sum >/dev/null 2>&1; then shaTool="sha256sum"
	elif command -v shasum >/dev/null 2>&1; then shaTool="shasum"
	elif command -v openssl >/dev/null 2>&1; then shaTool="openssl"
	else fFail "no sha256 tool found (looked for sha256sum, shasum, openssl)" "The download can't be verified without one, so nothing will be installed."
	fi

	##	An API token is optional, and only lifts the unauthenticated rate limit.
	local apiToken="${GITHUB_TOKEN:-}"

	##	From here on there is something to clean up. A signal is turned into an
	##	ordinary exit so the EXIT trap gets its turn; bash skips it otherwise.
	trap fCleanup EXIT
	trap 'exit 130' HUP INT TERM
	fAuthInit || fFail "could not create a temporary directory"

	tmpDir="$(mktemp -d 2>/dev/null)" || fFail "could not create a temporary directory"

	##	Resolve the release tag: the highest version in the release list, drafts
	##	skipped. Stable wants a full release, and takes the newest pre-release
	##	only when the list holds none, which is what makes a project with only
	##	betas installable. A failed call is an error, never "no release".
	echo "Looking up the newest ${release} release of ${appName} ..."
	local tag="" listFile="${tmpDir}/releases.json"
	if ! fApi "${apiBase}/releases?per_page=100" "${listFile}"; then
		if grep -qi 'rate limit' "${listFile}" 2>/dev/null; then
			fFail "GitHub's API rate limit is exhausted for this IP" \
				"Wait an hour, or set GITHUB_TOKEN to a personal access token and re-run."
		fi
		fFail "could not read the release list from github.com/${ownerRepo}" \
			"Check your network or HTTPS_PROXY, and that the repository still exists." \
			"Detail: $(paste -sd ' ' - 2>/dev/null <"${listFile}.err")"
	fi
	tag="$(fReleaseRows <"${listFile}" | fPickTag "${release}")"
	if [ -z "${tag}" ] && [ "${release}" = "stable" ]; then
		tag="$(fReleaseRows <"${listFile}" | fPickTag dev)"
		if [ -n "${tag}" ]; then
			echo "No full release published yet; using the newest pre-release instead."
			release="dev"
		fi
	fi
	[ -n "${tag}" ] || fFail "github.com/${ownerRepo} has no release published yet" \
		"Building from source: https://github.com/${ownerRepo}#build-it-yourself"
	local version="${tag#v}"

	##	Work out the asset name for this platform
	local asset sums
	asset="${assetPattern}"
	##	'|' as the delimiter, so a value carrying a '/' cannot break the script.
	asset="$(echo "${asset}" | sed -e "s|{exe}|${exeName}|g" -e "s|{version}|${version}|g" \
		-e "s|{os}|${osToken}|g" -e "s|{arch}|${archToken}|g" -e "s|{ext}|${exeExt}|g")"
	sums="$(echo "${sumsPattern}" | sed -e "s|{exe}|${exeName}|g" -e "s|{version}|${version}|g")"

	##	Pull the checksums first: it is small, it says which platforms this
	##	release actually carries, and its hash lets an already-current install
	##	finish without downloading the binary at all.
	fGet "${dlBase}/${tag}/${sums}" "${tmpDir}/${sums}" 2>/dev/null \
		|| fFail "release ${tag} has no checksums file (${sums})" \
			"Nothing can be verified without it, so nothing will be installed." \
			"Release page: https://github.com/${ownerRepo}/releases/tag/${tag}"

	fVerifySignature "${tmpDir}" "${sums}" "${tag}"

	local wantSha
	wantSha="$(awk -v want="${asset}" '{ name = $2; sub(/^\*/, "", name); if (name == want) { print $1; exit } }' "${tmpDir}/${sums}")"
	if [ -z "${wantSha}" ]; then
		echo >&2
		echo "Error: release ${tag} has no build for ${osToken}-${archToken}." >&2
		echo "  Expected asset: ${asset}" >&2
		echo "  What it does carry:" >&2
		awk '{ name = $2; sub(/^\*/, "", name); print "    " name }' "${tmpDir}/${sums}" >&2
		echo "  Building from source: https://github.com/${ownerRepo}#build-it-yourself" >&2
		echo >&2
		exit 1
	fi

	##	Destination
	local destDir destFile appDir="" sudoCmd=""
	if [ "${target}" = "user" ]; then
		destDir="${HOME}/.local/bin"
		appDir="${HOME}/.local/share/applications"
	else
		destDir="/usr/local/bin"
		appDir="/usr/local/share/applications"
		if [ "$(id -u)" != "0" ]; then
			command -v sudo >/dev/null 2>&1 \
				|| fFail "a system install needs root, and sudo is not installed" \
					"Re-run as root, or use --target user to install under \$HOME instead."
			sudoCmd="sudo"
		fi
	fi
	[ "${osToken}" = "linux" ] && [ "${menuEntry}" = "1" ] || appDir=""
	destFile="${destDir}/${exeName}"

	##	Already current? Then only a missing launcher is left to do, and with
	##	nothing missing, say so and stop - no prompt, no download. An existing
	##	launcher is left as it is, since it may have been edited by hand.
	local needBinary=1 needLauncher=0
	if [ -f "${destFile}" ] && [ "$(fSha256 "${destFile}")" = "${wantSha}" ]; then needBinary=0; fi
	if [ -n "${appDir}" ] && { [ "${needBinary}" = "1" ] || [ ! -e "${appDir}/${exeName}.desktop" ]; }; then
		needLauncher=1
	fi
	if [ "${needBinary}" = "0" ] && [ "${needLauncher}" = "0" ]; then
		echo
		echo "Already up to date: ${destFile} is ${tag}. Nothing to do."
		fPathNote "${destDir}" "${destFile}"
		echo
		exit 0
	fi

	##	Catch a permission problem now, rather than after a 10MB download.
	if [ "${needBinary}" = "1" ] && [ -z "${sudoCmd}" ] && ! fCanWrite "${destDir}"; then
		if [ "${target}" = "user" ]; then
			fFail "no permission to write to ${destDir}" \
				"Check who owns it: ls -ld ${destDir}"
		else
			fFail "no permission to write to ${destDir}" \
				"Re-run under sudo, or use --target user to install under \$HOME instead."
		fi
	fi

	##	The plan
	echo
	echo "Plan:"
	if [ "${needBinary}" = "1" ]; then
		echo "  Program:  ${appName} ${tag} (${release})"
		echo "  Platform: ${osToken}-${archToken}"
		echo "  Download: ${dlBase}/${tag}/${asset}"
		echo "  Verify:   sha256 against ${sums}"
		echo "  Install:  ${destFile}"
	else
		echo "  Program:  ${destFile} is already ${tag}"
	fi
	[ "${needLauncher}" = "0" ] || echo "  Launcher: ${appDir}/${exeName}.desktop"
	[ -z "${sudoCmd}" ] || echo "  Elevation: the install steps run under sudo"
	echo
	if [ "${assumeYes}" != "1" ]; then
		local answered=0
		fConfirm "Proceed?" || answered=$?
		if [ "${answered}" = "2" ]; then
			fFail "there is no terminal here to ask for confirmation" \
				"Re-run with --yes to install without being asked."
		fi
		if [ "${answered}" != "0" ]; then
			echo "Aborted - nothing was touched."; echo; exit 0
		fi
		echo
	fi

	if [ "${needBinary}" = "1" ]; then
		##	Download + verify
		echo "Downloading ${asset} ..."
		fGetShown "${dlBase}/${tag}/${asset}" "${tmpDir}/${asset}" \
			|| fFail "download failed" \
				"The release lists this asset, so this is most likely a network problem." \
				"URL: ${dlBase}/${tag}/${asset}"
		local haveSha; haveSha="$(fSha256 "${tmpDir}/${asset}")"
		if [ "${haveSha}" != "${wantSha}" ]; then
			fFail "checksum mismatch - NOT installing" \
				"expected ${wantSha}" \
				"got      ${haveSha}" \
				"The download was corrupted or tampered with. Try again; if it repeats, report it."
		fi
		echo "Checksum OK."

		##	Install. Land beside the target and rename into place: a rename is atomic,
		##	and it replaces a binary that is currently RUNNING (a straight copy over
		##	one fails with ETXTBSY).
		echo
		echo "Installing ..."
		local staged="${destDir}/.${exeName}.new.$$"
		${sudoCmd} mkdir -p "${destDir}" || fFail "could not create ${destDir}"
		${sudoCmd} cp "${tmpDir}/${asset}" "${staged}" || fFail "could not write to ${destDir}"
		${sudoCmd} chmod 0755 "${staged}"
		${sudoCmd} mv -f "${staged}" "${destFile}" || {
			${sudoCmd} rm -f "${staged}"
			fFail "could not replace ${destFile}"
		}
	fi

	##	Desktop launcher
	if [ "${needLauncher}" = "1" ]; then
		{
			echo "[Desktop Entry]"
			echo "Type=Application"
			echo "Name=${appName}"
			echo "GenericName=${desktopGenericName}"
			echo "Comment=${appComment}"
			echo "Exec=\"$(fDesktopExec "${destFile}")\""
			echo "Icon=${desktopIcon}"
			echo "Terminal=false"
			echo "Categories=${desktopCategories}"
			echo "Keywords=${desktopKeywords}"
			echo "StartupNotify=true"
		} > "${tmpDir}/${exeName}.desktop"
		if ${sudoCmd} mkdir -p "${appDir}" 2>/dev/null \
			&& ${sudoCmd} cp "${tmpDir}/${exeName}.desktop" "${appDir}/${exeName}.desktop" 2>/dev/null; then
			${sudoCmd} chmod 0644 "${appDir}/${exeName}.desktop"
		else
			echo "Note: could not write the desktop launcher to ${appDir} (the program itself installed fine)."
		fi
	fi

	if [ "${needBinary}" = "1" ]; then
		echo "Installed ${appName} ${tag} to ${destFile}"
	else
		echo "Put back what was missing for ${appName} ${tag}"
	fi
	fPathNote "${destDir}" "${destFile}"
	echo
}


##	Script entry point
set -u -e -E -o pipefail
shopt -s inherit_errexit 2>/dev/null || true
if [ "${BASH_SOURCE[0]:-}" = "${0}" ] || [ -z "${BASH_SOURCE[0]:-}" ]; then
	##	"$@" with no arguments is an unbound-variable error under `set -u` on
	##	bash 3.2 (the macOS system bash), so only pass it when there is one.
	if [ "$#" -gt 0 ]; then fMain "$@"; else fMain; fi
fi
