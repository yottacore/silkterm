# >>> SilkTerm shell integration >>>
# Reports this shell's directory to the terminal, so a new tab, pane, or window
# opens where this shell is. PowerShell keeps its location to itself, so there's
# nothing for a terminal to read unless the shell says so. Nothing is drawn on
# screen, and a terminal that doesn't understand the sequence ignores it.
# It also sets a git-aware prompt, but only when the prompt is still the stock
# one, so your own prompt is never replaced.
# Delete this block to switch it off, and SilkTerm won't put it back. SilkTerm
# does keep the block itself up to date, so an edit made inside the 2 markers
# is replaced on a later launch. Copy it out below them to make it yours.
if ($Host.Name -eq 'ConsoleHost' -and -not [Console]::IsOutputRedirected) {
	function global:__SilkTermReportDir {
		$dir = $ExecutionContext.SessionState.Path.CurrentLocation.ProviderPath
		if ($dir) { Write-Host -NoNewline ("{0}]9;9;`"{1}`"{0}\" -f [char]27, $dir) }
	}
	# Is this still the prompt PowerShell comes with? Its own help link is the
	# marker. A prompt anybody has written, or that oh-my-posh or starship
	# installed, won't have it. Only the stock one is replaced.
	$__SilkTermStock = (-not $function:prompt) -or ($function:prompt.ToString() -match 'LinkID=225750')

	# Everything below builds the prompt. It's worked out once, at load, since
	# none of it changes while the session runs, and the prompt is drawn after
	# every command.
	$global:__SilkTermHasGit = [bool](Get-Command git -CommandType Application -ErrorAction SilentlyContinue)
	$global:__SilkTermRemotes = @{}
	# The console goes to UTF-8 so that a branch name with a non-ASCII character
	# in it decodes, and so nothing downstream has to guess. The prompt itself
	# is written as wide characters and gets to the screen either way.
	try { [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { }
	# Code points rather than literal glyphs, because 5.1 reads a file without a
	# byte-order mark as ANSI. Both marks are the light ones, since a light check
	# beside the heavy cross reads as 2 different weights, which is what they are.
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
	# A machine you're logged into by mistake should look wrong right away, so the
	# host name can be colored per machine. Set $SilkTermHostColor above this
	# block, since a line added here is replaced on a later launch. For example:
	#   if ([Environment]::MachineName -eq 'myserver') { $SilkTermHostColor = '1;31' }
	$global:__SilkTermHostColor = if ($global:SilkTermHostColor) { $global:SilkTermHostColor } else { '1;37' }

	function global:__SilkTermPaint {
		param([string]$Code, [string]$Text)
		"$([char]27)[${Code}m$Text$([char]27)[0m"
	}

	# The working tree this directory is in, or $null. Walking up for a .git
	# entry costs a few file tests, but asking git costs a process, on every
	# prompt, in every directory that isn't a repository.
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
	# gives all 3. The branch header lines start with #, and any other line is a
	# change, such as a modified file or an untracked one.
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
		# Without a branch.ab line there's no upstream, so nothing to be level with.
		$ahead = 0
		$behind = 0
		$level = $false
		if ($ab -match '^\+(\d+) -(\d+)$') {
			$ahead = [int]$Matches[1]
			$behind = [int]$Matches[2]
			$level = ($ahead -eq 0 -and $behind -eq 0)
		}
		# The remote the branch tracks, else origin, else the first one. It's fine
		# if there isn't one. It's looked up once per branch rather than at every
		# prompt, and again once the branch gains an upstream, since that can name
		# a different remote.
		$key = "$Root`n$branch`n$($null -ne $ab)"
		if (-not $global:__SilkTermRemotes.ContainsKey($key)) {
			$prev = $ErrorActionPreference
			$ErrorActionPreference = 'Continue'
			$remote = $null
			if ($branch -ne '(detached)') { $remote = (& git config --get "branch.$branch.remote" 2>$null) -as [string] }
			if (-not $remote -or $remote -eq '.') {
				# A remote's name is case-sensitive to git, so Origin isn't origin.
				$names = @(& git remote 2>$null)
				$remote = if ($names -ccontains 'origin') { 'origin' } else { $names | Select-Object -First 1 }
			}
			$url = $null
			if ($remote) { $url = (& git config --get "remote.$remote.url" 2>$null) -as [string] }
			$ErrorActionPreference = $prev
			if ($url) {
				# An ssh remote reads git@host:owner/repo. The part before the @ is
				# the same on every line and doesn't tell anybody anything.
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
		# The version, because 2 PowerShells look alike at a prompt.
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
			# One mark for everything committed, and one for level with the upstream.
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
		# prompt alone. oh-my-posh, starship, and a hand-written prompt all keep
		# working, and anything already using this hook is called first. A second
		# load, such as `. $PROFILE` again, finds its own handler there and keeps
		# the one from before it, or every change would report twice.
		$__SilkTermHook = $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction
		if (-not ($__SilkTermHook -and [object]::ReferenceEquals($__SilkTermHook, $global:__SilkTermOwnHook))) {
			$global:__SilkTermPrevLocation = $__SilkTermHook
		}
		# The hook stores a delegate, which & can't call.
		$ExecutionContext.SessionState.InvokeCommand.LocationChangedAction = {
			if ($global:__SilkTermPrevLocation) { $global:__SilkTermPrevLocation.Invoke($args[0], $args[1]) }
			__SilkTermReportDir
		}
		$global:__SilkTermOwnHook = $ExecutionContext.SessionState.InvokeCommand.LocationChangedAction
		if ($__SilkTermStock) { function global:prompt { __SilkTermPrompt } }
	}
	else {
		# Windows PowerShell 5.1 doesn't have that hook, so wrap whatever prompt is
		# in place rather than replacing it. A second load finds its own wrapper in
		# place, and wrapping that would call itself until the stack runs out.
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
