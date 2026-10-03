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
