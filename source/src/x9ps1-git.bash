#!/bin/bash

# shellcheck disable=2004  ## Inappropriate complaining of "$/${} is unnecessary on arithmetic variables."
# shellcheck disable=2028  ## echo may not expand escape sequences. Use printf. (This is bad advice for ANSI escape sequences.)
# shellcheck disable=2034  ## Unused variables.
# shellcheck disable=2119  ## Disable confusing and inapplicable warning about function's $1 meaning script's $1.
# shellcheck disable=2154  ## Foo is referenced but not assigned.
# shellcheck disable=2155  ## Disable check to 'Declare and assign separately to avoid masking return values'.
# shellcheck disable=2120  ## OK with declaring variables that accept arguments, without calling with arguments (this is 'overloading').

##
##	WARNING: This script is maintained on github. Don't edit directly. https://github.com/jim-collier/x9ps1-git
##

##	Purpose:
##		This script can be set up to constantly keep the command prompt updated.
##		Normally it behaves much like standards PS1 prompts, but if it detects that
##		that it is in a git project folder, the prompt will show you the latest status
##		after every command.
##
##	Installation instructions:
##
##		1) Move this to some location in your path."
##
##		2) Add this, verbatim, to the bottom of your ~./bashrc:"
##		   PROMPT_COMMAND='PS1=`x9ps1-git`'
##
##		3) Exit your current terminal and open a new one."
##
##	Usage:
##
##		- If you want to revert to a typical linux terminal prompt at any time temporarily,
##		  set X9PS1_STANDARD=1.
##
##	Note:
##		If you run this script in a way not outlined above, and it produces garbage on the CLI,
##		just close the terminal and open up a new one.

##	Copyright and license:
##		x9ps1-git v1.0.1 build 1n3g16s,
##		Copyright © 2011-2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
##		Licensed under The MIT License (MIT). Full text at:
##			https://mit-license.org/
##		SPDX-License-Identifier: MIT

##	History at bottom of script (maintained here in addition to source control)


## Text from git goes into PS1, which bash expands at every prompt. A branch named '$(cmd)' is legal, and would run.
## Root sees a '$' in a name as '#', since '\$' is the prompt's own uid escape; that is display only.
function fPromptSafe() {
	local val="${1//[[:cntrl:]]/}"
	val="${val//\\/\\\\}"
	val="${val//\$/\\\$}"
	val="${val//\`/\\\`}"
	printf '%s' "${val}"
}

function fMain() {

	## Early exit if user overrides with "standard prompt" environment variable
	if [[ "${X9PS1_STANDARD}" == "1" ]]; then
		## Standard Debian-style prompt
		local -i color_prompt=0
		{ [[ -x /usr/bin/tput ]] && tput setaf 1 >&/dev/null; }  &&  color_prompt=1
		if ((color_prompt)); then
			echo "${debian_chroot:+($debian_chroot)}\[\033[01;32m\]\u@\h\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ "
		else
			echo "${debian_chroot:+($debian_chroot)}\u@\h:\w\$ "
		fi
		return 0
	fi

	## Quick options
	local -ri addArrowToEnd=1
	local -ri addNewlineToEnd=0

	## PS1 elements
	local -r UserName="\u"
	local -r HostName="\h"
	local -r WorkingDir="\w"
	local -r UsrOrRootDecorator="\\\$"
	local -r Time24="\t"
	local -r Time12="\@"
	local -r Newline="\\n"
	local -r EscBegin="\\["
	local -r EscEnd="\\]"
	local -r ColorBegin="${EscBegin}\\e["
	local -r ColorEnd="m${EscEnd}"
	local -r colorRESET="${ColorBegin}0${ColorEnd}"

	## Unicode symbols
#	local -r unicodeXmark="\\xe2\\x9c\\x98"      ## ✘
#	local -r unicodeCheckmark="\\xe2\\x9c\\x94"  ## ✔
	local -r unicodeXmark="✘"
	local -r unicodeCheckmark="✔"
	local -r unicodeUp="↑"
	local -r unicodeDown="↓"

	## Styles and background colors (sorf of mutually exclusive)
	local -r styleRegular="    0"
	local -r styleLight="      1"
	local -r styleDim="        2"
	local -r styleBold="       1"
	local -r styleItalic="     3"
	local -r styleUnderline="  4"
	local -r styleBlink="      5"
	local -r styleReverse="    7"
#	local styleLightReverse=" 10"
	local -r bgRed="          41"
	local -r bgGreen="        42"
	local -r bgYellow="       43"
	local -r bgBlue="         44"
	local -r bgPurple="       45"
	local -r bgCyan="         46"
	local -r bgWhite="        47"

	## Colors
	local -r fgBlack="        30"
	local -r fgRed="          31"
	local -r fgGreen="        32"
	local -r fgYellow="       33"
	local -r fgBlue="         34"
	local -r fgPurple="       35"
	local -r fgCyan="         36"
	local -r fgWhite="        37"

	## Specific element colors "${STYLE} ${COLOR}"; Spaces are OK for visual formatting, will be removed later.
	local colorUsername="         ${styleRegular}     ; ${fgGreen}   "
	local colorAt="               ${styleDim}         ; ${fgWhite}   "
	local colorColon="            ${styleDim}         ; ${fgWhite}   "
	local colorPath="             ${styleRegular}     ; ${fgWhite}   "
	local colorDecorator="        ${styleDim}         ; ${fgWhite}   "
	local colorRepo="             ${styleRegular}     ; ${fgPurple}  "
	local colorBranch="           ${styleLight}       ; ${fgCyan}    "
	local colorMiscDelim="        ${styleDim}         ; ${fgWhite}   "
	local colorTime="             ${styleDim}         ; ${fgCyan}    "
	local colorAttn="             ${styleLight}       ; ${fgGreen}   "


	## Unique hostname colors, with default; feel free to add, change, delete
	local colorHostname=""
	case "${HOSTNAME}" in
#		"myserver")  colorHostname="${styleBold}   ; ${fgRed}"      ;;
		*)           colorHostname="${styleLight}  ; ${fgWhite} "   ;;
	esac


	## Repo status
	local colorStatus_Yes="       ${styleReverse}     ; ${fgGreen}   "
	local colorStatus_Not="       ${styleReverse}     ; ${fgRed}     "

	## Clean up and properly format
	local -r colorUsername="${ColorBegin}${colorUsername// /}${ColorEnd}"
	local -r colorAt="${ColorBegin}${colorAt// /}${ColorEnd}"
	local -r colorHostname="${ColorBegin}${colorHostname// /}${ColorEnd}"
	local -r colorColon="${ColorBegin}${colorColon// /}${ColorEnd}"
#	local -r colorPath="${colorRESET}"
	local -r colorPath="${ColorBegin}${colorPath// /}${ColorEnd}"
	local -r colorDecorator="${ColorBegin}${colorDecorator// /}${ColorEnd}"
	local -r colorRepo="${ColorBegin}${colorRepo// /}${ColorEnd}"
	local -r colorBranch="${ColorBegin}${colorBranch// /}${ColorEnd}"
	local -r colorMiscDelim="${ColorBegin}${colorMiscDelim// /}${ColorEnd}"
	local -r colorTime="${ColorBegin}${colorTime// /}${ColorEnd}"
	local -r colorStatus_Yes="${ColorBegin}${colorStatus_Yes// /}${ColorEnd}"
	local -r colorStatus_Not="${ColorBegin}${colorStatus_Not// /}${ColorEnd}"
	local -r colorAttn="${ColorBegin}${colorAttn// /}${ColorEnd}"

	## Used later
	local encodedPartial_Repo=""
	local encodedPartial_Branch=""
	local encodedPartial_GitStatus=""
	local final_Git=""

	## Generate fully encoded primitives
	local -r final_Time="${colorTime}${Time24}${colorRESET}"
	local -r final_Username="${colorUsername}${UserName}${colorRESET}"
	local -r final_Hostname="${colorHostname}${HostName}${colorRESET}"
	local -r final_delim_At="${colorAt}@${colorRESET}"
	local -r final_delim_Colon="${colorColon}:${colorRESET}"
	local -r final_delim_Bullet="${colorMiscDelim}•${colorRESET}"
	local -r final_Path="${colorPath}${WorkingDir}${colorRESET}"
	local -r final_UserOrRootDecorator="${colorDecorator}${UsrOrRootDecorator}${colorRESET}"
	local -r final_BracketL="${colorMiscDelim}[${colorRESET}"
	local -r final_BracketR="${colorMiscDelim}]${colorRESET}"

	## To save time, see if git is even installed.
	if [[ -n "$(which git 2>/dev/null || true)" ]]; then

		## One call answers the branch, whether it's committed, and where it stands against its upstream.
		## The porcelain form is the same in every language, where the plain status text is translated.
		local -r gitStatus="$(git status --porcelain=v2 --branch 2>/dev/null || true)"
		if [[ -n "${gitStatus}" ]]; then

			local gitBranch=""  gitAheadBehind=""  line=""
			local -i isCommitted=1  isInSyncWithUpstream=0  gitAhead=0  gitBehind=0
			while IFS= read -r line; do
				case "${line}" in
					"# branch.head "*)  gitBranch="${line#"# branch.head "}"     ;;
					"# branch.ab "*)    gitAheadBehind="${line#"# branch.ab "}"  ;;
					"#"*)               ;;
					*)                  isCommitted=0                           ;;
				esac
			done <<< "${gitStatus}"

			## No 'branch.ab' line means no upstream, so there's nothing to be level with.
			if [[ "${gitAheadBehind}" =~ ^\+([0-9]+)\ -([0-9]+)$ ]]; then
				gitAhead="${BASH_REMATCH[1]}"
				gitBehind="${BASH_REMATCH[2]}"
				isInSyncWithUpstream=$((gitAhead == 0 && gitBehind == 0))
			fi

			## The remote the branch tracks, else origin, else the first one. None is fine too.
			local gitRemote=""  gitRemotes=""  gitRepo=""
			if [[ "${gitBranch}" != "(detached)" ]]; then
				gitRemote="$(git config --get "branch.${gitBranch}.remote" 2>/dev/null || true)"
			fi
			if [[ -z "${gitRemote}" || "${gitRemote}" == "." ]]; then
				gitRemotes="$(git remote 2>/dev/null || true)"
				if [[ $'\n'"${gitRemotes}"$'\n' == *$'\n'origin$'\n'* ]]; then gitRemote="origin"
				else                                                            gitRemote="${gitRemotes%%$'\n'*}"
				fi
			fi
			if [[ -n "${gitRemote}" ]]; then
				gitRepo="$(git config --get "remote.${gitRemote}.url" 2>/dev/null || true)"
				gitRepo="${gitRepo#*@}"  ## An ssh remote starts with 'git@' on every line, so it says nothing.
			fi

			## Encoded repo and branch
			[[ -z "${gitRepo}" ]] || encodedPartial_Repo="${colorRepo}$(fPromptSafe "${gitRepo}")${colorRESET}"
			[[ -z "${gitBranch}" ]] || encodedPartial_Branch="${colorBranch}$(fPromptSafe "${gitBranch}")${colorRESET}"

			## Encoded status
			if ((isCommitted));          then encodedPartial_GitStatus="${colorStatus_Yes}${unicodeCheckmark}"
			else                              encodedPartial_GitStatus="${colorStatus_Not}${unicodeXmark}"
			fi
			if ((isInSyncWithUpstream)); then encodedPartial_GitStatus="${encodedPartial_GitStatus}${colorStatus_Yes}${unicodeCheckmark}"
			else                              encodedPartial_GitStatus="${encodedPartial_GitStatus}${colorStatus_Not}${unicodeXmark}"
			fi
			encodedPartial_GitStatus="${encodedPartial_GitStatus}${colorRESET}"
			if ((gitAhead || gitBehind)); then
				encodedPartial_GitStatus="${encodedPartial_GitStatus} ${colorAttn}"
				((gitAhead == 0))  || encodedPartial_GitStatus="${encodedPartial_GitStatus}${unicodeUp}${gitAhead}"
				((gitBehind == 0)) || encodedPartial_GitStatus="${encodedPartial_GitStatus}${unicodeDown}${gitBehind}"
				encodedPartial_GitStatus="${encodedPartial_GitStatus}${colorRESET}"
			fi

			## Assemble git part of final encoded prompt
			local -a tmpArray=()
			tmpArray+=(" ")
			tmpArray+=("${final_BracketL}")
			tmpArray+=(" ")
			[[ -n "${encodedPartial_Repo}" ]] && tmpArray+=("${encodedPartial_Repo}")
			if [[ -n "${encodedPartial_Branch}"    ]]; then
				[[ -n "${encodedPartial_Repo}" ]] && tmpArray+=("${final_delim_Colon}")
				tmpArray+=("${encodedPartial_Branch}")
			fi
			if [[ -n "${encodedPartial_GitStatus}" ]]; then
				[[ -n "${encodedPartial_Repo}${encodedPartial_Branch}" ]] && tmpArray+=(" ")  ## tmpArray+=("${final_delim_Bullet}")
				tmpArray+=("${encodedPartial_GitStatus}")
			fi
			tmpArray+=(" ")
			tmpArray+=("${final_BracketR}")
			local tmpStr=""
			for (( i=0; i < ${#tmpArray[@]}; i++ )); do tmpStr="${tmpStr}${tmpArray[i]}"; done
			local -r final_Git="${tmpStr}"  ## Lock previously devined final_Git

		fi
	fi

	####
	#### Build final output string

	local -a tmpArray=()
	tmpArray+=("${final_Time}")
	tmpArray+=(" ")
	tmpArray+=("${final_Username}")
	tmpArray+=("${final_delim_At}")
	tmpArray+=("${final_Hostname}")
	tmpArray+=("${final_delim_Colon}")
	tmpArray+=("${final_Path}")
	if [[ -n "${final_Git}" ]]; then
		tmpArray+=("${final_Git}")
		((addArrowToEnd))  &&  tmpArray+=("\n${colorAttn}🡆${colorRESET}")
	fi
	if ((addNewlineToEnd)); then
		tmpArray+=("${Newline}")
	else
		tmpArray+=(" ")
	fi
	tmpArray+=("${final_UserOrRootDecorator}")
	tmpArray+=(" ")

	local tmpStr=""
	for (( i=0; i < ${#tmpArray[@]}; i++ )); do tmpStr="${tmpStr}${tmpArray[i]}"; done

	## Final return value (only do one or the other)
	echo "${tmpStr}"

}

set -e
set -E

fMain

set +eE


##	History at bottom of script (maintained here in addition to source control)
##		- 20201001 JC: Created.
##		- 20220501 JC: Fixed instruction #2.
##		- 20220502 JC:
##			- Project no longer needs to be in a path with 'git' in it. (That was weird.)
##			- Improved readability for custom hostname formatting.
##		- 20230920 JC: Added easy option to add newline to end.
##		- 20260508 JC:
##			- Set $addNewlineToEnd=0. I'll probably soon realize why it was =1.
##			- Added $addArrowToEnd to easily add or remove the post-git status arrow.
##		- 20260621 JC:
##			- Chanced license from GPLv3 to MIT.
##			- Added X9PS1_STANDARD flag.
##		- 20260915 JC: Branch and remote names are escaped before going into PS1. A name holding $(...) or backticks ran as a command at every prompt.
##		- 20260917 JC: Git part shows with any remote or none, reads the porcelain status, and shows ahead/behind counts.
