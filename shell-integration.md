<!-- markdownlint-disable MD007 -- Indent count -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Shell integration

SilkTerm opens a new tab, split or window in the directory the current pane is in. To do that it has to know where that pane's shell is, and for most shells it can simply ask the operating system - it reads the shell process's own working directory, and nothing needs setting up.

Some shells never tell the operating system where they are. PowerShell is an example. `Set-Location` moves PowerShell's own idea of where it is and leaves the process itself in the directory it was launched in, so there is nothing to read. The same applies to any session where the shell you are typing at is not the process SilkTerm started - an `ssh` session, a container, a REPL that keeps its own location.

The fix is the same one every terminal uses: have the shell say where it is, in a short escape sequence. SilkTerm listens for two spellings and takes either.

- **OSC 7**, a `file://` URL - what the unix shells emit, and what GNOME Terminal, WezTerm, kitty and others read.

- **OSC 9;9**, a plain path - the ConEmu spelling that Windows Terminal documents. A PowerShell profile already set up for Windows Terminal works here unchanged.

What the shell reports wins over what the operating system can see. A reported directory that no longer exists on this machine is ignored, and the operating system's answer stands instead - which is also what rejects a path reported from the far side of an `ssh`, since an OSC 7 URL naming another machine is never believed.

## PowerShell: SilkTerm does this for you

A few seconds after launch, SilkTerm looks for the PowerShells you have installed and adds the block below to each one's profile. You don't have to do anything, and it happens once.

What it will not do:

- **Touch a profile that already reports.** Its own marker, or any other OSC 7 / OSC 9;9 already in the file - a Windows Terminal setup, oh-my-posh, anything - means somebody has this in hand, and the file is left exactly as it is.

- **Rewrite what is there.** The block is appended, after a copy of the profile is saved beside it as `Microsoft.PowerShell_profile.ps1.silkterm-backup`. Everything above and below the two markers stays exactly as you wrote it.

- **Put it back.** Deleting the block is how you switch it off. Nothing restores it - a note beside the config records which profiles were written to, so an empty one is left alone.

- **Replace a prompt you chose.** If your prompt is still the one PowerShell comes with, the block swaps in a git-aware one (below). If it is anything else, including oh-my-posh, starship or a `prompt` function of your own, it is left alone: on PowerShell 6+ the prompt is not touched at all, and on Windows PowerShell 5.1, which has no other hook, yours is wrapped rather than replaced.

- **Write a file the shell would refuse to read.** If PowerShell's execution policy blocks script files, the block would only turn every launch into a red execution-policy error, so the profile is left alone and a line says which shell and why. Windows PowerShell 5.1 is commonly in that state; `Get-ExecutionPolicy` shows it, and `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` is the usual fix - your call to make, not SilkTerm's.

One thing it *will* do: keep the block itself up to date. It gains things over time - the git-aware prompt is one - and only the text between the two markers is ever rewritten. If you want to change what the block does, copy it out below the markers and edit that copy, or your edits will be replaced on a later launch.

To switch the whole thing off before it ever runs, set `shell.integration: false` in the config, or clear "Update PowerShell profiles" on the Shell tab of Settings.

### The prompt

Once the block is in place, a stock prompt reads:

~~~text
[PS 7.6] 09:41:22 you@yourbox:~/projects/silkterm [ github.com:you/silkterm.git:dev ✓✘ ]
>
~~~

Left to right: which PowerShell this is, because two of them look alike at a prompt; the time the prompt was drawn; who and where; and the directory, with your home folder shortened to `~`.

The part in brackets only appears inside a git working tree, and the two marks at the end of it are a check or a cross:

- The first says everything is committed. A modified file or an untracked one turns it red.

- The second says the branch is level with its upstream - nothing to push, nothing to pull. A branch with no upstream at all counts as not level.

When the branch is ahead or behind, the counts follow the marks, such as `↑2↓1`. The name in front of the branch is the remote the branch tracks, else `origin`, else the first remote there is. A repository with no remote shows the branch alone.

When the brackets are there the typing moves to a second line, since the first line is long by then. Outside a repository the prompt is one line. Either way it ends in the usual `>`.

The same prompt appears on Windows PowerShell 5.1 (`[PS 5.1] ...`) and on PowerShell 7 wherever it runs, macOS and Linux included. The block puts the console on the UTF-8 code page, so a branch name with a non-ASCII character in it reads correctly.

It costs one `git` call per prompt, a few more the first time a branch is seen, and none at all outside a working tree - the search for a `.git` folder is done in the shell.

`X9PS1_STANDARD=1` in a session puts an ordinary `PS C:\path>` prompt back, the same way it does for the bash prompt below. To take the whole thing back, define your own `prompt` **below** the block; anything of yours is detected on the next launch and left alone from then on.

The look is a port of [x9ps1-git](https://github.com/jim-collier/x9ps1-git), the bash prompt described further down, so a PowerShell pane and a bash pane read the same. It is one block rather than a script on disk because a script would mean starting a process for every prompt.

This is the block, if you would rather paste it in yourself (`notepad $PROFILE`, creating the file if it is not there):

~~~powershell
# >>> SilkTerm shell integration >>>
# Reports this shell's directory to the terminal, so a new tab, pane or window
# opens where this shell is. PowerShell keeps its location to itself, so there
# is nothing for a terminal to read unless the shell says so. Nothing is drawn
# on screen, and a terminal that does not understand the sequence ignores it.
# It also sets a git-aware prompt - but only when the prompt is still the stock
# one, so your own prompt is never replaced.
# Delete this block to switch it off - SilkTerm will not put it back. It does
# keep the block itself up to date, so an edit made INSIDE the two markers is
# replaced on a later launch - copy it out below them to make it yours.
if ($Host.Name -eq 'ConsoleHost' -and -not [Console]::IsOutputRedirected) {
	function global:__SilkTermReportDir {
		$dir = $ExecutionContext.SessionState.Path.CurrentLocation.ProviderPath
		if ($dir) { Write-Host -NoNewline ("{0}]9;9;`"{1}`"{0}\" -f [char]27, $dir) }
	}
	# Is this still the prompt PowerShell ships? Its own help link is the marker:
	# a prompt anybody has written, or that oh-my-posh or starship installed,
	# will not carry it. Only the stock one is replaced.
	$__SilkTermStock = (-not $function:prompt) -or ($function:prompt.ToString() -match 'LinkID=225750')

	# Everything below builds the prompt. Worked out once, at load: none of it
	# changes while the session runs, and the prompt is drawn after every command.
	$global:__SilkTermHasGit = [bool](Get-Command git -CommandType Application -ErrorAction SilentlyContinue)
	$global:__SilkTermRemotes = @{}
	# The console goes to UTF-8 so that a branch name with a non-ASCII character
	# in it decodes, and so nothing downstream has to guess. The prompt itself
	# is written as wide characters and reaches the screen either way.
	try { [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { }
	# Code points rather than literal glyphs, because 5.1 reads a file with no
	# byte-order mark as ANSI. The light pair: a light check beside the HEAVY cross
	# read as two different weights, which is what they are.
	$global:__SilkTermGlyphs = @{ Yes = [string][char]0x2713; No = [string][char]0x2717; Up = [string][char]0x2191; Down = [string][char]0x2193 }
	# Root gets a different decorator, the way a unix prompt does.
	$global:__SilkTermAdmin = $false
	try {
		if ($PSVersionTable.PSVersion.Major -lt 6 -or $IsWindows) {
			$id = [Security.Principal.WindowsIdentity]::GetCurrent()
			$global:__SilkTermAdmin = (New-Object Security.Principal.WindowsPrincipal $id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
		}
		else { $global:__SilkTermAdmin = [Environment]::UserName -eq 'root' }
	}
	catch { }
	# A machine you are logged into by mistake should look wrong immediately, so
	# the host name can be colored per machine. Set $SilkTermHostColor ABOVE this
	# block, since a line added here is replaced on a later launch. For example:
	#   if ([Environment]::MachineName -eq 'myserver') { $SilkTermHostColor = '1;31' }
	$global:__SilkTermHostColor = if ($global:SilkTermHostColor) { $global:SilkTermHostColor } else { '1;37' }

	function global:__SilkTermPaint {
		param([string]$Code, [string]$Text)
		"$([char]27)[${Code}m$Text$([char]27)[0m"
	}

	# The working tree this directory is in, or $null. Walking up for a .git
	# entry costs a few file tests; asking git costs a process, on every prompt,
	# in every directory that is not a repository.
	function global:__SilkTermRepoRoot {
		param([string]$Start)
		$dir = $Start
		while ($dir) {
			if (Test-Path -LiteralPath (Join-Path $dir '.git')) { return $dir }
			$parent = Split-Path -Parent $dir
			if (-not $parent -or $parent -eq $dir) { return $null }
			$dir = $parent
		}
		return $null
	}

	# Branch, clean, and in step with the upstream, from one call. The v2 format
	# answers all three: the branch header lines start with #, and any other line
	# is a change - a modified file, or an untracked one.
	function global:__SilkTermGitState {
		param([string]$Root)
		$prev = $ErrorActionPreference
		$ErrorActionPreference = 'Continue'
		$lines = & git status --porcelain=v2 --branch 2>$null
		$ErrorActionPreference = $prev
		if (-not $lines) { return $null }
		$branch = ''
		$ab = $null
		$clean = $true
		foreach ($line in $lines) {
			if ($line.StartsWith('# branch.head ')) { $branch = ($line -split ' ', 3)[2] }
			elseif ($line.StartsWith('# branch.ab ')) { $ab = ($line -split ' ', 3)[2] }
			elseif (-not $line.StartsWith('#')) { $clean = $false }
		}
		# No branch.ab line means no upstream, so nothing to be level with.
		$ahead = 0
		$behind = 0
		$level = $false
		if ($ab -match '^\+(\d+) -(\d+)$') {
			$ahead = [int]$Matches[1]
			$behind = [int]$Matches[2]
			$level = ($ahead -eq 0 -and $behind -eq 0)
		}
		# The remote the branch tracks, else origin, else the first one. None is
		# fine too. Asked once per branch rather than at every prompt, and again
		# once it gains an upstream, since that can name a different remote.
		$key = "$Root`n$branch`n$($null -ne $ab)"
		if (-not $global:__SilkTermRemotes.ContainsKey($key)) {
			$prev = $ErrorActionPreference
			$ErrorActionPreference = 'Continue'
			$remote = $null
			if ($branch -ne '(detached)') { $remote = (& git config --get "branch.$branch.remote" 2>$null) -as [string] }
			if (-not $remote -or $remote -eq '.') {
				# A remote's name is case-sensitive to git, so Origin is not origin.
				$names = @(& git remote 2>$null)
				$remote = if ($names -ccontains 'origin') { 'origin' } else { $names | Select-Object -First 1 }
			}
			$url = $null
			if ($remote) { $url = (& git config --get "remote.$remote.url" 2>$null) -as [string] }
			$ErrorActionPreference = $prev
			if ($url) {
				# ssh remotes read git@host:owner/repo - the part before the @ is
				# the same on every line and says nothing.
				$at = $url.IndexOf('@')
				if ($at -ge 0) { $url = $url.Substring($at + 1) }
			}
			$global:__SilkTermRemotes[$key] = $url
		}
		@{ Branch = $branch; Clean = $clean; Synced = $level; Ahead = $ahead; Behind = $behind; Remote = $global:__SilkTermRemotes[$key] }
	}

	function global:__SilkTermPrompt {
		$dec = if ($global:__SilkTermAdmin) { '#' } else { '>' }
		$dec = $dec * ($NestedPromptLevel + 1)
		$dir = "$($ExecutionContext.SessionState.Path.CurrentLocation)"
		# An ordinary prompt, for anyone who wants one back for a session.
		if ($env:X9PS1_STANDARD -eq '1') { return "PS $dir$dec " }
		$home_ = $HOME
		if ($home_ -and $dir.StartsWith($home_, [StringComparison]::OrdinalIgnoreCase)) {
			$dir = '~' + $dir.Substring($home_.Length)
		}
		$v = $PSVersionTable.PSVersion
		# The version, because two PowerShells look alike at a prompt.
		$out = __SilkTermPaint '2;37' "[PS $($v.Major).$($v.Minor)]"
		$out += ' ' + (__SilkTermPaint '2;36' (Get-Date -Format 'HH:mm:ss'))
		$out += ' ' + (__SilkTermPaint '0;32' ([Environment]::UserName))
		$out += (__SilkTermPaint '2;37' '@')
		$out += (__SilkTermPaint $global:__SilkTermHostColor ([Environment]::MachineName))
		$out += (__SilkTermPaint '2;37' ':')
		$out += (__SilkTermPaint '0;37' $dir)
		$git = $null
		if ($global:__SilkTermHasGit -and $PWD.Provider.Name -eq 'FileSystem') {
			$root = __SilkTermRepoRoot $PWD.ProviderPath
			if ($root) { $git = __SilkTermGitState $root }
		}
		if ($git) {
			$mark = { param($ok) __SilkTermPaint $(if ($ok) { '7;32' } else { '7;31' }) $(if ($ok) { $global:__SilkTermGlyphs.Yes } else { $global:__SilkTermGlyphs.No }) }
			$out += ' ' + (__SilkTermPaint '2;37' '[') + ' '
			if ($git.Remote) { $out += (__SilkTermPaint '0;35' $git.Remote) + (__SilkTermPaint '2;37' ':') }
			$out += (__SilkTermPaint '1;36' $git.Branch) + ' '
			# Two marks: everything committed, and level with the upstream.
			$out += (& $mark $git.Clean) + (& $mark $git.Synced)
			if ($git.Ahead -or $git.Behind) {
				$counts = ''
				if ($git.Ahead) { $counts += $global:__SilkTermGlyphs.Up + $git.Ahead }
				if ($git.Behind) { $counts += $global:__SilkTermGlyphs.Down + $git.Behind }
				$out += ' ' + (__SilkTermPaint '1;32' $counts)
			}
			$out += ' ' + (__SilkTermPaint '2;37' ']')
			# The first line is long in a repository, so the typing starts on its own.
			$out += "`n"
			$sep = ''
		}
		else { $sep = ' ' }
		$out + $sep + (__SilkTermPaint '2;37' $dec) + ' '
	}
	if ($null -ne $ExecutionContext.SessionState.InvokeCommand.PSObject.Properties['LocationChangedAction']) {
		# PowerShell 6+ can be told about the location itself, which leaves the
		# prompt alone - oh-my-posh, starship and a hand-written prompt all keep
		# working, and anything already using this hook is called first. A second
		# load, such as `. $PROFILE` again, finds its own handler there and keeps
		# the one from before it, or every change would report twice.
		$__SilkTermHook = $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction
		if (-not ($__SilkTermHook -and [object]::ReferenceEquals($__SilkTermHook, $global:__SilkTermOwnHook))) {
			$global:__SilkTermPrevLocation = $__SilkTermHook
		}
		# The hook holds a delegate, which & cannot call.
		$ExecutionContext.SessionState.InvokeCommand.LocationChangedAction = {
			if ($global:__SilkTermPrevLocation) { $global:__SilkTermPrevLocation.Invoke($args[0], $args[1]) }
			__SilkTermReportDir
		}
		$global:__SilkTermOwnHook = $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction
		if ($__SilkTermStock) { function global:prompt { __SilkTermPrompt } }
	}
	else {
		# Windows PowerShell 5.1 has no such hook, so wrap whatever prompt is in
		# place rather than replacing it. A second load finds its own wrapper in
		# place, and wrapping that calls itself until the stack runs out.
		if (-not ($function:prompt -and $function:prompt.ToString() -match '__SilkTermPrevPrompt')) {
			$global:__SilkTermPrevPrompt = if ($__SilkTermStock) { $null } else { $function:prompt }
		}
		function global:prompt {
			__SilkTermReportDir
			if ($global:__SilkTermPrevPrompt) { & $global:__SilkTermPrevPrompt }
			else { __SilkTermPrompt }
		}
	}
	__SilkTermReportDir
}
# <<< SilkTerm shell integration <<<
~~~

The guard on the first line is what keeps it out of your way: it reports only in an interactive console session whose output is not redirected, so `pwsh -File build.ps1 > log.txt` gets a clean log rather than escape sequences in it.

## bash

Nothing to set up - bash moves its own process, so SilkTerm can ask the operating system. Add this only if you want a pane behind `ssh`, `sudo -i` or a container to report as well:

~~~bash
# ~/.bashrc
PROMPT_COMMAND='printf "\033]7;file://%s%s\033\\" "$HOSTNAME" "$PWD"'
~~~

Many distributions already do this for you - Debian and Fedora ship `/etc/profile.d/vte.sh`, which emits the same sequence. Check with `echo "$PROMPT_COMMAND"` before adding a second one.

Paths with characters that need escaping in a URL are supposed to be percent-encoded. SilkTerm reads them either way, so the one-liner above is enough here; encode if you also use a terminal that is stricter about it.

### A git-aware bash prompt

A separate thing, and the only other setup SilkTerm does for a shell. It is off until "Use git-aware Bash prompt" is checked on the Shell tab of Settings, or `shell.bash_prompt: true` is set in the config. Then a bash pane gets a prompt that shows the branch you are on, whether the working tree is clean, and how far ahead or behind its tracking branch it is - updated after every command, and out of the way in a directory that is not a git project.

It is not an install:

- Nothing is written into `.bashrc` or any other file of yours. The prompt is handed to the pane as `PROMPT_COMMAND` in its environment.

- It sets `PS1` before every prompt, so it replaces a `PS1` set in `.bashrc`. If you set `PROMPT_COMMAND` yourself - directly, or through starship, oh-my-posh or `/etc/profile.d/vte.sh` - yours runs instead, and you will not see this one.

- It reaches bash panes only, and only ones SilkTerm started. A shell you `ssh` into, or a `sudo -i`, keeps whatever prompt it has.

- `X9PS1_STANDARD=1` in a pane puts the ordinary Debian-style prompt back for that session.

Clear the checkbox again, or set `shell.bash_prompt: false`, to switch it off. The script itself is written beside the config as `x9ps1-git`, and is a copy of [x9ps1-git](https://github.com/jim-collier/x9ps1-git) (MIT) - usable on its own from a `PATH` directory if you want it in every terminal rather than this one.

## zsh

Same story as bash - nothing needed locally:

~~~zsh
# ~/.zshrc
precmd() { printf "\033]7;file://%s%s\033\\" "$HOST" "$PWD" }
~~~

## fish

Nothing to do - fish emits OSC 7 on its own.

## cmd.exe

Nothing to do - `cd` moves the process itself, so the operating system can answer.

## Checking it works

`cd` somewhere, then split the pane (or open a new tab). The new pane should start in the same directory. If it starts where the old pane was *launched* instead, the sequence is not reaching us - check that the block is in the profile that shell actually loads (`$PROFILE` prints its path), and that nothing later in the file replaces the prompt on 5.1.
