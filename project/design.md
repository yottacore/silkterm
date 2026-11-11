<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# SilkTerm design

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Goal](#goal)
- [Feature designs](#feature-designs)
- [Architecture](#architecture)
	- [Language / stack decision](#language--stack-decision)
	- [Logical code organization](#logical-code-organization)
	- [API (alacritty_terminal)](#api-alacritty_terminal)
	- [Smooth scrolling](#smooth-scrolling)
	- [Text readability scrim](#text-readability-scrim)
	- [Themes and text color](#themes-and-text-color)
	- [Wallpaper](#wallpaper)
	- [Performance profiles](#performance-profiles)
	- [Fonts, Unicode and emoji](#fonts-unicode-and-emoji)
	- [Hyperlinks](#hyperlinks)
	- [Double-click and selection](#double-click-and-selection)
	- [Measurements and display scaling](#measurements-and-display-scaling)
	- [The Settings dialog](#the-settings-dialog)
	- [The shell list and how it is filled](#the-shell-list-and-how-it-is-filled)
	- [What a pane's shell inherits](#what-a-panes-shell-inherits)
	- [One tip system, 4 places that draw it (2026-08-30)](#one-tip-system-4-places-that-draw-it-2026-08-30)
	- [Render loop sketch](#render-loop-sketch)
	- [Speed](#speed)
	- [The About box says how long the session has been up (2026-09-20)](#the-about-box-says-how-long-the-session-has-been-up-2026-09-20)
	- [A character handed to the window is typing (2026-09-09)](#a-character-handed-to-the-window-is-typing-2026-09-09)
	- [What untrusted input may not do (2026-09-09)](#what-untrusted-input-may-not-do-2026-09-09)
	- [The fuzzer (2026-09-09)](#the-fuzzer-2026-09-09)
	- [Environment](#environment)
	- [Startup and slow external resources](#startup-and-slow-external-resources)
	- [Releasing resources](#releasing-resources)
	- [Configuration format](#configuration-format)
	- [Windows find each other (2026-10-13)](#windows-find-each-other-2026-10-13)
	- [Variables in a setting (2026-08-30)](#variables-in-a-setting-2026-08-30)
	- [Command-line options](#command-line-options)
- [Delivery (CI/CD, branches, releases)](#delivery-cicd-branches-releases)
	- [A tab can be named by hand, and the window title follows the tab (2026-08-30)](#a-tab-can-be-named-by-hand-and-the-window-title-follows-the-tab-2026-08-30)
	- [A terminal running with administrator or root rights says so (2026-09-09)](#a-terminal-running-with-administrator-or-root-rights-says-so-2026-09-09)
	- [Tabs report what they are running, and where (2026-08-21)](#tabs-report-what-they-are-running-and-where-2026-08-21)
	- [A tab is as wide as its own label needs (2026-08-23)](#a-tab-is-as-wide-as-its-own-label-needs-2026-08-23)
- [Copyright and license](#copyright-and-license)

<!-- /TOC -->

## Goal

GUI terminal emulator for Debian/X11/Compiz with pixel-by-pixel smooth scrolling, both:

- Animated easing on output (new text appears).

- Smooth scrollback navigation with wheel.

No existing Linux terminal does animated smooth-scroll on output. (Verified: WezTerm, kitty, foot, Alacritty, GNOME Terminal, Konsole all snap to cell rows.)

## Feature designs

Each of these has its own design doc, which is the source of truth for that feature.

- [Smooth scrolling](design_docs/20260930-144720_smooth-scrolling.md)

- [Smooth cursor](design_docs/20260930-145124_smooth-cursor.md)

- [Text scrim](design_docs/20260930-145304_scrim.md)

- [Settings dialog](design_docs/20260930-145721_settings-dialog.md)

- [Wallpaper and see-through windows](design_docs/20260930-150052_wallpaper.md)

- [Themes and text color](design_docs/20260930-150458_themes.md)

- [Speed](design_docs/20260930-150643_speed.md)

- [Unicode, fonts and emoji](design_docs/20260930-150813_unicode-and-emoji.md)

- [Split panes](design_docs/20260930-150948_split-panes.md)

- [Double-click and selection](design_docs/20260930-151047_double-click-selection.md)

- [Performance profiles](design_docs/20260930-151204_performance-profiles.md)

- [Releasing resources](design_docs/20260930-151334_releasing-resources.md)

- [Reducing resources](design_docs/20261004-182255_reduce-resources.md)

- [The terminal engine and patched crates](design_docs/20260930-151451_alacritty-fork.md)

## Architecture

### Language / stack decision

Rust plus the `alacritty_terminal` crate, not a fork of the Alacritty application. The crate brings the PTY, the parser and the grid, and SilkTerm builds only the renderer. There are 3 crates on small patched branches, one per published release. The reasons, what's used, and how the patches are maintained are in [The terminal engine and patched crates](design_docs/20260930-151451_alacritty-fork.md).

- Renderer: `wgpu`. Glyph atlas plus cell draw.

### Logical code organization

SilkTerm implements an event-loop-driven renderer over a retained terminal model - closer to a game's update/render loop than to a widget framework. Three logical roles:

- Model (the only source of truth). Each pane embeds an `alacritty_terminal::Term`: the integer character grid, scrollback, cursor, and the full VT/ANSI parser. A per-pane background thread reads the child process's PTY, feeds the bytes into that `Term`, and wakes the UI thread. Global tunables live in one swappable `Settings` (an atomic `Arc`) that every layer reads. Nothing else caches grid contents.

- View (rebuilt every frame, pulled - never pushed). There is no retained widget tree. Each frame every visible pane snapshots its grid into draw data: styled text runs for the glyph renderer, plus solid quads for cell backgrounds, the cursor, and the selection. The GPU renderers draw that. Smooth scroll is a view-only idea layered on top: the model only knows whole lines, the renderer interpolates a fractional offset between them. Chrome (menu bar, tab bar, context menus, dialogs) is drawn the same immediate-mode way.

- Controller (event routing). winit delivers all input to one `ApplicationHandler`. Keystrokes become PTY bytes for the focused pane (or drive an open menu/dialog instead); the mouse drives selection, focus, divider-drag, pane reorder, and menus. Input never edits the grid directly - it goes to the child, the child replies, and the model updates on the next PTY read.

The spine of the program is a single ownership tree:

~~~text
App  (winit ApplicationHandler)
+- State                      Main window
|  +- Gfx                     GPU backend: native wgpu surface, a glutin GL context
|  |                            on X11, or a composited DX12 surface on Windows
|  |                            (the two per-pixel-transparency paths)
|  +- renderers               Text (glyphon) + rects + bg image + scrim
|  \- Tabs                    The tab list + active index
|     \- PaneManager          One per tab: a binary split tree (Node::Split / Leaf)
|        \- Pane              A leaf: layout rect, selection, per-pane state
|           \- TermInstance   Alacritty Term + its PTY-reader thread
\- DialogWin?                 Optional pop-out window (Settings / About),
                                self-contained with its own Gfx + text renderer
~~~

So a window is a list of tabs, a tab is a split tree of panes, a pane wraps one terminal; pop-out dialogs are independent sibling windows.

Frame loop: a PTY read or a user event marks the app dirty or starts an animation. `about_to_wait` renders when something is dirty or animating, and otherwise waits. A render advances the scroll easing, snaps the grid to the nearest whole line, and redraws each pane from current model state. Frames are driven from `about_to_wait` rather than redraw requests, because `request_redraw` is unreliable under X11/Compiz here. Timed wakes, such as a parked cursor's resume, are read after the frame, since drawing is what sets them.

### API (alacritty_terminal)

The engine knows only whole lines (`display_offset`), so smooth scrolling is done entirely in the renderer. The calls SilkTerm relies on, and how the terminal is shared with the reader thread, are in the [terminal engine](design_docs/20260930-151451_alacritty-fork.md) and [Speed](design_docs/20260930-150643_speed.md) design docs.

### Smooth scrolling

The engine stores whole lines, and the renderer draws a fractional offset over them. The wheel, the scrollbar, new output, and full-screen programs all ease by moving that offset. New output runs through the output chase, a speed curve made of 5 named segments, one per setting. Full-screen programs are read from a scroll ledger in the engine fork, with row fingerprints as the fallback.

Full design: [Smooth scrolling](design_docs/20260930-144720_smooth-scrolling.md).

### Text readability scrim

A soft halo in the background color is drawn behind every glyph, with an optional crisp outline, so text stays readable over a busy wallpaper or a see-through window. The halo's shape and its fade are separate settings, and Strength thickens it into a plate.

Full design: [Text scrim](design_docs/20260930-145304_scrim.md).

### Themes and text color

There are 4 built-in themes, each with a dark and a light palette, plus saved themes stored whole. A minimum contrast floor in Oklab moves text that is too close to its cell's background, the block cursor's plate has to keep the text on it readable, and dark text on a light background is corrected to look as heavy as light text on dark. Full design: [Themes and text color](design_docs/20260930-150458_themes.md).

### Wallpaper

The wallpaper is a faint, blurred picture behind the text, from a named image or the built-in one, and it's prepared on a worker thread so it never delays the window. Visibility means the same amount of the picture's contrast in dark and light mode. The design doc also describes the see-through window. Full design: [Wallpaper and see-through windows](design_docs/20260930-150052_wallpaper.md).

### Performance profiles

A single setting sets how much the look may cost. The choices are Max silk, High, Low, Standard terminal, or Custom, plus Remote (temporary) for a remote screen. A profile is applied over the user's settings and never changes them. The first pick is timed and written against a hardware fingerprint, and a session steps down on missed frames without writing it. Full design: [Performance profiles](design_docs/20260930-151204_performance-profiles.md).

### Fonts, Unicode and emoji

A single monospace family is pinned for every weight, found from one search order on every platform. Anything that family lacks falls back glyph by glyph and is fitted to its cells, and color emoji are painted in-house. Full design: [Unicode, fonts and emoji](design_docs/20260930-150813_unicode-and-emoji.md).

### Hyperlinks

- URLs in the output are clickable. A link must have a scheme from a fixed list - http, https, ftp, ftps, sftp, ssh, file, mailto - rather than being guessed from what it looks like. That keeps false positives near zero, since output is full of words with colons and slashes in them. It's also the whole of the security story, because a scheme outside the list isn't a link, so it can never be handed to the desktop. Bare `www.` prefixes and bare file paths were considered and left out for the same reason.

- Punctuation is trimmed the way a reader would. A full stop or comma after a URL belongs to the sentence, and so does a closing bracket the URL is sitting inside. One the URL itself opened is part of it. A URL that wraps across rows is one link, found from either half.

- Hovering underlines, Ctrl+click opens, and Command+click on macOS. The underline appears on a plain hover with no modifier, since a link the user can't see is a link they won't try. Opening needs Ctrl so it can never be confused with selecting. The press arms and the release opens, so a slipped press can be dragged off to cancel. A right-click on a link puts "Open link" and "Copy link" at the top of the menu, and only there. On macOS Ctrl+click is the right-click, so it opens the menu with the link rows.

- An app that is watching the mouse itself owns the pointer, so nothing underlines over it. Holding Shift gets the local behavior instead, the same bypass selection already uses. The right-click menu still wins over such an app, as all our chrome does.

- Links open through the desktop's own handler by default, with a configurable program to override it. Deciding what a URL means is the desktop's job rather than a terminal's.

### Double-click and selection

A double-click takes something it can name first, such as a path, URL or git remote, then the inside of a matched pair, then a word. A drag held past the pane's edge scrolls and keeps selecting. Copy has 4 routes, each with its own rule. Full design: [Double-click and selection](design_docs/20260930-151047_double-click-selection.md).

### Measurements and display scaling

Every measurement in the interface is written once in DIP, 1/96 of an inch, and turned into real pixels only when it is drawn. A pop-out dialog converts once, at its window's edge. The main window's chrome converts at each measurement, since it shares its space with the terminal grid. A width that text is fitted to, such as where a notice or a menu tip wraps, follows the interface font as drawn rather than the scale factor. The font size is rounded to a whole pixel at each scale, so a line then breaks at the same words at any scale. `SILK_SCALE` overrides the scale factor for testing. The full rules are in the [Settings dialog](design_docs/20260930-145721_settings-dialog.md) design doc.

### The Settings dialog

The Settings dialog is a pop-out window with its tabs in a list down the left side, with Themes, Background, Text, and Cursor under Appearance. It's declared in the compiled-in `settings_ui.shcl`, drawn by SilkTerm, and driven fully from the keyboard. Groups, sub-groups, the color picker, Apply and OK, and display scaling are all in the [Settings dialog](design_docs/20260930-145721_settings-dialog.md) design doc.

A setting the program can work out for itself is stored as a value set by hand or as no line at all, and no line means automatic. A single table in config.rs lists them with their rules, one function reads them, and a switch over a group of them is read off the group rather than stored. The design is in [Settings with an automatic value](design_docs/20261008-180516_automatic_settings.md). The font family and size, the window size, the theme colors, the link open command, and the wallpaper's image and folder are the ones moved so far.

### The shell list and how it is filled

- The shells a new tab can be started with are one list, stored in the config as `shells.<key>` with a title, a command, an active flag, a comment, and the date a scan last found the program installed. File order is list order, which is also menu order. It's a plain part of the config, so it can be edited by hand. The Settings dialog's "Shell" tab is an editor for something that already works, since the list came first on purpose.

- **The list names the default shell: its first switched-on entry.** There was a separate `shell.default` setting that said the same thing. A second place that names the default can only ever disagree with the list, so one rule that's visible in the list is worth more than a second field. A config that had the old setting has that entry moved to the top of the list, once, and the line removed. The value was the user's own statement of which shell they meant, so it's kept rather than dropped. Finding which entry it names uses the same identity test as the scan, rather than a string compare. The old setting was routinely a bare name where the scan had already stored the full path to the same file, and comparing them as text put a second copy of the user's default shell at the top of their own list, where the top is what "default shell" now means. A new list starts with the shell the user logs in with, so the default is right without the user saying anything.

- Finding installed shells is background work that starts a few seconds after the wallpaper itself is on screen, rather than only the window. Both are off-thread and both are slow in the same way, so overlapping them puts a stall in the one moment anybody is looking, which is the gap between the window appearing and the picture showing up in it. A wallpaper that never loads, such as one on a dead mount, can't hold the scan off forever. Past a deadline it runs anyway, since a terminal without any shells in its menu is worse than one without a picture behind its text. The scan stats every directory on PATH and, on Windows, reads the registry. Any of those can be a mount or a hive that responds slowly, so none of it may run between launch and the first frame. It runs on its own thread, and the result is folded in when it's ready, the same way the wallpaper pipeline works.

- **What a scan may do to the list is deliberately lopsided.** It may add a shell it found, and it may switch off one whose program is gone. The entry keeps its title, its flags, and its place, since a shell that was only uninstalled isn't one the user stopped wanting. It may never switch one back on, and never rewrite a command line. A scan can't tell a program that came back from a switch somebody turned off on purpose. A wrong guess costs more one way than the other, since quietly re-enabling something the user disabled is worse than leaving them one checkbox to tick.

- **A save of the list works from the file rather than from what the window loaded.** Each window reads the list when it starts, and nothing watches the file, so another window may have saved since. A window that was behind used to take out only the entries it knew about and write its own list back after them. A shell another window had just found stayed on top of the whole list and became the default shell. Now every save merges 3 ways, with the file as the third. What another window added stays, at the end, and what it removed stays gone. For an entry this window didn't change, the file's copy is used. The window's own order is written only when it moved something itself.

- Shells count as one entry when they run the same program with the same arguments. Which program that is has to be resolved rather than compared as text, because the same shell is written several ways (`bash`, `/bin/bash`), and on Windows, 3 environments install a program called `bash` that aren't the same shell. Where a stored entry doesn't resolve to anything, a bare name match is enough. That lets a reinstall re-arm its disabled entry, instead of the new find being listed beside it as a second copy.

- A file found 2 ways, such as through a link, is one shell, offered at the shorter path. When 2 finds under one name are different files, they're told apart by version. Where both report the same version, the shorter path is offered, in the place of the one found first, so a login shell found twice keeps the top. Where the versions differ, each name gets its version, such as "Bash 3.2.57" and "Bash 5.2.37". A shell that doesn't respond to `--version` within 2 seconds keeps its plain name. Only shells that share a name are asked, so most scans don't start anything. This applies to new finds only. A saved list keeps any pair it has, since a scan only adds and switches off.

- **The order a fresh list arrives in is stated outright, in one place, rather than falling out of whatever order the search happens to run in.** Each find is put in a group and the whole set is sorted once at the end. On unix the user's own login shell comes first, and nothing may sort above it, since the top of the list is what "default shell" means. Then come the modern cross-platform shells, the language REPLs, and the rest of the POSIX family. Windows doesn't have a user shell, so the order is set instead: PowerShell 7, the modern shells, the WSL distributions, the 3 POSIX-environment bashes, PyCmd, the language REPLs, Windows Cmd, and last the 2 Windows PowerShell 5 entries. Those are for when something needs them, rather than what somebody opens a terminal to get. Groups of shells of equal standing sort alphabetically. Groups of one shell built several ways keep a set order, which is why MSYS2's full bash is offered above the mini one that comes with Git.

- The login shell's twin is listed directly below it and starts without reading its startup files. Each shell writes that flag its own way (`--norc`, `--no-rcs`, `--no-config`, `-NoProfile`), so the flag is per shell and the twin only exists where there is one. Only the login shell gets a twin. A twin for every shell would double a list nobody asked to be long. It's added switched off, because it's for when the user's own rc file is what's being debugged, rather than a second copy of their shell in the menu every day. `cmd.exe` is left off that table on purpose, even though it has such a flag, `/d` for no AutoRun. It's what Windows reports as the command processor, so it's the login shell on every Windows box, and a second "Command Prompt" in everyone's menu costs more than a rarely-set AutoRun key is worth.

- WSL distributions are read from the registry, never by asking `wsl.exe`. A WSL2 distribution is stored in a virtual disk, and listing what's installed must not boot a virtual machine. That would be slow and surprising, and arguably a security problem for the user. Each distribution is offered whole, running its own default shell. Anyone who wants a particular shell inside one can edit the entry to say so. Its generation is part of its name (`WSL2; Ubuntu`), because that's the whole difference between 2 rows that would otherwise read identically, and the WSL2 ones are offered above the WSL1 ones. Both are offered where both exist, since a WSL1 distribution is installed and usable, and hiding one because a newer-generation one is installed beside it isn't the scan's call to make. The generation is a bit in the distribution's registry flags. The `Version` value beside it is the registration format's version, and it's 2 for a WSL1 distribution too.

- **The Shell tab is the one place allowed to write the list, and a scan that finishes while it's open is folded into it.** Everywhere else the dialog passes the live list through untouched on Apply, because a dialog that opened before a scan finished would otherwise write back the empty list it copied then, emptying the menu for the rest of the session while the file on disk still had every shell in it. The tab needs to write it, so instead the scan is folded into both of the dialog's copies. It goes into the edited one so the user sees what turned up, and into the baseline so the fold doesn't read as an edit they made. Because a scan only ever appends and switches off, folding it into work already done can't undo any of it.

- Every field is edited in its grid row rather than in a popup, since that takes fewer clicks and reuses the field code the dialog already has. There are 4 fixed-width columns, and the command takes whatever width is left, since it's the one value that's routinely too long to read at a glance. "Last seen" is read-only. It's the program's own note about the entry, and it explains why a shell was switched off. The command is required, and that's enforced in the 2 places it can be broken. Emptying the field leaves the stored command standing, and an entry that never got one is dropped on the way out of the dialog, rather than written as a shell that doesn't name anything to run.
	- The fixed columns still follow the interface font. "Last seen" and Active are measured, and the name column and the gaps between columns grow with the UI line height above the default's (2026100818102267).

- **Reordering is a mouse gesture on a grip rather than 4 buttons.** Every line has a drag handle at its left edge, and the list reorders under the pointer as it moves rather than on release. The line being dragged is the line that's seen to move, which is the reason to prefer a grip over arrows. It costs 4 Tab stops per line, and that trade was made knowingly, since reordering doesn't have a keyboard equivalent now. So the grip isn't a stop at all, since a focus ring on a control that Space can't work would be worse than no ring.

- **Remove is between the command and the date, and is drawn in red.** It's the one control in the dialog that destroys something, so it's kept off the right-hand edge on purpose, since the pointer moves down that edge on its way to the checkboxes. The red is a chrome color rather than a theme color, because "this deletes something" is a fixed meaning, and a theme whose accent happened to be red would say it about every control at once.

- **How "where is this shell now" is answered has 2 halves, what the OS can see and what the shell says, and the second wins.** On Linux the link at `/proc/<pid>/cwd` is read, and on macOS, which doesn't have /proc, `proc_pidinfo` is called. Windows doesn't have an equivalent or any API that reports another process's directory, so it's read out of the shell's own process memory, where SetCurrentDirectory stores it. The result is checked to still be a directory first, so if that memory layout ever changed, the answer would be "don't know" rather than a wrong directory. Neither can see a shell that tracks its own idea of where it is. PowerShell's `Set-Location` never tells the OS, which is why the shell is also given a way to say so directly, with the escape sequence every terminal reads for this. That's OSC 7, and the ConEmu OSC 9;9 form that Windows Terminal documents. A report wins over the OS answer because it comes from the shell itself. A report that names a directory that isn't here, or a machine that isn't this one, is dropped, and the OS answer stands.

- **The snippet is put into PowerShell profiles automatically, and what that allows is kept narrow.** Asking every user to paste a block into a file before new tabs open in the right place is a poor trade when the block can be put there for them. But a write to somebody else's shell profile needs limits. So the only profile touched is one that doesn't report anything yet. SilkTerm's marker, or anyone else's OSC 7 or OSC 9;9, means it's handled already. A copy is saved beside the profile, and then the block is appended, never rewritten. The marker makes a second launch a no-op. Deleting the block switches it off, and nothing puts it back. The prompt isn't replaced, only wrapped where there isn't another hook. A shell whose execution policy would refuse to load the profile is left alone, with a line saying why, because a file the shell can't read is worse than no file. A single setting turns the whole thing off before it starts.

- **The listening is done by wrapping the PTY rather than by forking the VT parser.** The sequences arrive as bytes, and the parser doesn't handle either of them, so the obvious route was to fork it. Instead, the PTY is used through an interface rather than a concrete type. A wrapper in front of the real one scans what it reads and leaves the byte stream untouched. It costs one pass looking for a single byte, and there's no second fork to maintain.

- **Where a shell starts is decided by 4 things in a fixed order, and the setting is the last of them.** A `--directory` on the command line wins outright. Failing that, a new tab, pane, or window inherits the directory of the pane it came from. A SilkTerm that a shell launched starts in the directory that shell was in. Only what's left over reads `shell.startup_directory`. That's a launch from the desktop, a menu, or a shortcut, where the inherited directory is an accident of whatever started SilkTerm. Its default is the home variable somebody on that platform would type (`$HOME`, `%USERPROFILE%`), because a blank default doesn't say anything about what may go in it.

- The test for "did a shell launch us" is whether standard input is a terminal, and it's the same test on both platforms. Asking about parent processes would be more direct, but it costs a process-table walk on Windows, which doesn't belong on the path to the first frame. On Windows, when launched from a console the standard handles are the console's, and when launched the way Explorer and the Start menu do it they're null. A release build doesn't own a console either way, so the window-handle and attach-to-parent tests are both wrong for this.

- Removing an entry asks first, but moving one doesn't. A move can be undone by doing the opposite, but there's no undo for a removal.

### What a pane's shell inherits

- A pane's shell inherits the environment SilkTerm itself was launched with. That's intended for anything the user set, such as an activated virtualenv, a PATH they added, or a variable they exported before starting the terminal. It's what makes a terminal opened from a shell behave as a continuation of that shell.

- It's wrong for the bookkeeping a shell does for itself. PowerShell 7 prepends its own module directories to the module search path that every version of PowerShell shares, so a Windows PowerShell 5.1 pane opened anywhere below one resolves PSReadLine to PowerShell 7's copy rather than its own and isn't allowed to load it, so the pane starts with an error and without line editing. The execution-policy variable has the same problem. A shell sets it, and everything that shell starts inherits it, so a pane can run under a policy nobody chose for it.

- So a short list of shell-private variables is put back to what a freshly launched program would see, read from the machine at startup, and everything else is passed through untouched. Of the options, a narrow list is the only one that works. Replacing the whole environment would discard the user's own exports, which is the one thing inheriting exists for, and editing the polluted value in place, by dropping the entries that belong to the other shell, would depend on where that shell happens to be installed.

- This isn't a defect in SilkTerm, and the fix isn't a workaround for one. The same thing happens to a command prompt launched from PowerShell 7 with nothing of ours involved. But a pane should start the way it would from the desktop, and the terminal is the only place where that can be set once for every shell it opens.

- The list isn't split by platform. PowerShell runs on Linux and macOS too and changes the same variable there, so 2 installs side by side collide the same way. The launching shell's `cd -` target is stale on every platform too, since a pane opens somewhere else. The first thing people think of is an activated virtualenv or conda environment, and that's left out on purpose. A user wants a pane to keep it, and it couldn't be removed cleanly in any case, because the matching PATH edits would stay behind and leave the pane half-activated.

- Unix constrains the list in a way Windows doesn't. There's no call that says what a freshly launched program would see. That's composed by PAM, the session manager, and the login shell between them, and it's never recorded, so the unix arm can only drop a variable, never restore it. A name may therefore join the list only if a desktop session never sets it. That's true for all 3 today, and it's the rule to check before adding a fourth.

### One tip system, 4 places that draw it (2026-08-30)

- Flyover help comes up in 4 places: a Settings row, a link in the About box, a tab in the strip, and a menu item. There are 2 renderers and 2 fonts involved, so the drawing was never going to be shared.

- Everything else is shared, and it's in `tip.rs`. The shared parts are how long the pointer rests before a tip appears, how the text is broken to fit a width, and where the box goes relative to what it describes. A tip that appeared faster in one place than another would read as a different kind of thing, which is why the delay in particular is one number.

- There are 2 placement rules, and which one applies depends on what's being described. A Settings row's tip goes under the control, or above it when there isn't room. A footer button's tip clamped to the bottom edge would cover the buttons it explains. A menu row's tip goes beside the popup instead, because a box under the row would cover the rows the reader is choosing between.

- The tab's tip goes away after 30 seconds, and comes back only once the pointer has left the tab and returned (2026-09-22). The pointer is often left on the strip after picking a tab, and the tip then covered the top of the pane for as long as it stayed there. `window.tab_tip_max_s` sets the time, and 0 keeps it up. The other tips close when the pointer moves on, which it does almost at once in a menu or a dialog.

- A menu row gets a tip only when its label doesn't already say what it does. Copy and New tab explain themselves; Paste selection, Read-only, and Bare window don't. A tip on every row is noise that a reader learns to skip, and then the ones that matter get skipped too.

- A tip's box is its own color, derived from the chrome it hangs off rather than taken from it (2026-09-20). The default menu background is exactly the inactive tab's color, so a tab tip filled with it drew as a tab that had grown downward, and a menu row's tip had the same fault against the popup beside it. Both now fill with the menu background lifted by the step the strip puts between an inactive and an active tab, then warmed, since every tab color leans faintly blue. The border and the text are taken from the same 2 colors. The dialogs already did this with the button shade against their panel.
	- Adding 2 more colors to the Themes tab was suggested, but it wasn't done. There are 12 editable colors, each with one job, and the chrome already handles this kind of question with derived shades. Menu hover, border, and separator are all shades of the menu color, so a custom menu color stays coherent. A tip that could be set apart from its own menu would be one more pair to keep readable.
	- The benchmark banner draws with the tips, over the menus, and uses the menu colors. It's a modal notice over a dimmed window rather than flyover help.

- A tip draws last, over everything else in its window, menus and an open dropdown list included (2026-10-13). It has a text renderer of its own, drawn after the menus' one, because a renderer draws all of its text in one go. A list that shared the tip's renderer put its text on top of the tip's box. An open dropdown or field menu also takes every click until it closes, so nothing behind it shows a tip.

- A window without the focus doesn't show any tips, and a tip that's up goes away when the focus leaves (2026-10-13). When the focus comes back, tips wait for the pointer to move. A tip that popped up under a resting pointer the moment the window came back would seem to come out of nowhere.

### Render loop sketch

- Frame: advance lerp -> cross-boundary check -> sync crate offset -> translate render -> draw cells (+overscan rows).

- Need: glyph atlas (rasterize font once, cache cells), cell metrics (width/height in px), vsync via wgpu surface.

### Speed

Output is passed to the window as one notice at a time, drawing never blocks reading, and a frame whose text didn't change doesn't do any text work. Throughput is measured by a benchmark anyone can run, and published in [comparison.md](../comparison.md). Full design: [Speed](design_docs/20260930-150643_speed.md).

### The About box says how long the session has been up (2026-09-20)

- The reading is taken as the box opens rather than ticking while it is on screen. A dialog redrawing once a second to move a number nobody is watching costs a wake a second for as long as it is up, and the number only matters to the nearest minute. Close it and open it again for a fresh one.

- The clock runs from a mark set at the top of `main`, rather than from the first time anything asks. Starting it on first read would have reported how long the About box had been reachable.

### A character handed to the window is typing (2026-09-09)

- Windows lets a program give a window a character outright instead of pressing a key for it. The touch keyboard does this for characters the layout has no key for, and so do text expanders and some accessibility tools. It's delivered as a key the layout can't name, with only the text it stands for.

- Every reader of a key event looks at which key it was, so an unnamed one did nothing at all. That included typed text, hotkeys, menu accelerators, the Settings dialog's fields, and the tab rename box. The key is filled in from the text once, where the event arrives, rather than in each of those places.

- Nothing else produces an unnamed key that has text, so there is no second meaning to weigh. A key the layout did name is left alone, since its text represents the key rather than typing. Enter, with its carriage return, is the case that would go wrong.

- Modifiers still apply, so an injected `c` while Control is held sends the control code, the same as typing it would. Windows composes the character without consulting the layout or the modifiers, so there's an argument for ignoring them here. But a character behaving differently from the same character typed is the worse surprise, and nobody has reported the other way around.

### What untrusted input may not do (2026-09-09)

Almost everything a terminal handles came from somewhere else. Bytes from a pane's program may have come from a log file, a build server, or a remote host over ssh, and the program printing them need not be the one that wrote them. So the rule is that nothing a pane shows may change what the terminal does, and each case below follows that rule.

- The terminal must not type on a program's behalf. A few sequences ask the terminal a question, and the answer goes back down the pty where the shell reads it as if it had been typed. An answer that could contain a line ending would submit itself, and one that could contain the program's own text would let the program choose the command. So every reply has a fixed format built from a number, and the terminal doesn't answer any question whose answer would be somebody else's text. There is no way to read back a window title, and a request to read the clipboard is ignored rather than answered.

	- Pinned by `a_program_cannot_make_the_terminal_type` in `term.rs`, which drives a real terminal through generated escape sequences and then asks it every question it knows how to ask.

- A link must have a scheme from the list, or it isn't a link. That's what stops `javascript:` and its relatives from getting to the desktop's handler, and it's why detection is by scheme rather than by what a link looks like. The URL then goes to the platform's own opener as one plain argument, never through a shell.

	- Pinned by `a_hostile_scheme_never_becomes_a_link` and `a_url_reaches_the_opener_exactly_as_it_was_printed` in `links.rs`.

- A paste must not be able to step outside its brackets. When the application has asked for bracketed paste, an escape character in the payload would end the bracket early and everything after it would arrive as keystrokes. That's how a paste can run a command nobody typed. Escapes are removed. Unbracketed, the application can't tell a pasted byte from a typed one, so a line break has to be the single carriage return the Enter key sends.

	- Pinned by `a_bracketed_paste_cannot_be_closed_from_inside` and the fuzz target beside it in `pane.rs`.

- A window title is text and nothing else. The desktop draws it on a task bar, in a window list, and in an alt-tab switcher, all of which treat it as plain text. Control characters are removed where the title arrives, once, rather than at each of the places it is later shown. A right-to-left override survives, which does let a title read back to front. That's the same reordering any file name can ask for, and refusing it would also refuse the joiners an emoji is built with.

	- Pinned by `a_title_arrives_as_plain_text` in `tabtitle.rs`.

- A reported directory isn't automatically a directory. A shell says where it is with an escape sequence, and that path both names the tab and sets where the next pane starts. A payload with a control character is refused, since it would end up in the tab strip and the title. Whether the path is absolute is checked separately, at the one gate on what becomes a working directory. A relative path resolves against wherever the terminal itself was started, somewhere nobody can see, and a posix path on Windows names a real directory in the wrong filesystem.

	- Pinned by `a_reported_directory_with_a_control_character_is_refused` in `cwd.rs` and `a_pane_only_ever_starts_in_an_absolute_directory` in `term.rs`.

- A program may set the clipboard, but only from the pane in use (2026-09-15). This is the one place output changes something outside the pane, and it is allowed because tmux, editors over ssh, and muffer's auto-copy all copy this way. A pane in the background or in another tab is ignored, so a log scrolling past in one can't swap out what the next paste contains. A store over a megabyte is dropped whole rather than clipped. Reading the clipboard is still refused, and text a program put there goes through the paste rule above on its way back.

	- Pinned by `a_program_can_set_the_clipboard_but_never_read_it` in `term.rs`.

### The fuzzer (2026-09-09)

There are 10 targets, each in a `mod fuzz` inside the tests of the module it exercises. They're ordinary tests, so a plain `cargo test` runs each one for a fraction of a second, and the pipeline sets a budget per target for a real soak. The engine is `source/src/fuzz.rs`, and the corpus and its notes are under `cicd/tests/fuzz-corpus/`.

- The cases are generated by grammar rather than guided by coverage. Every input here has a grammar, such as escape sequences, config lines, URLs, and image chunk headers, and a generator that knows the grammar gets to deep states in a few hundred cases where bit flips need millions. Coverage-guided fuzzing would also mean a nightly toolchain and sanitizers, so it would run on only 1 of the 4 platforms, and not in the ordinary test run at all.

- Bit flips run as well, over a small corpus of realistic inputs. A well-formed generator never emits a truncated multi-byte character or a chunk header that lies about its own length, and those are where a parser's edges are.

- A single seed is all it takes to reproduce a case. Every case is a pure function of a u64, and a failure reports the seed to re-run it with. Seeds run in order from zero, so a longer budget only ever adds cases. A long run doesn't drop anything a short run tried.

- Library code is in scope, which is most of the point. The escape-sequence target drives a real terminal from the engine crate, so it fuzzes the parser and the grid model rather than only our own code around them. The config target goes through the config-format crate the same way.

- What a target asserts is a property rather than an expected output. A parser that returns nothing is fine, but one that returns a span past the end of the row it was given isn't. That's what makes a random case worth generating.

The fuzzer found 3 defects while it was being built, and all of them were fixed with it. A program could put control characters into the window title, a reported directory could too, and a reported directory that was relative would have started a pane somewhere nobody chose. Each also has a plain unit test, which is cheaper to keep.

### Environment

- Target: Debian. The primary dev/reference environment is X11 (Compiz), but one Linux binary runs native on both X11 and Wayland. winit selects the backend at runtime, and X11/Wayland/GL are all loaded on demand. Windows and macOS are targets too, all with x86_64 and ARM64 variants.

	- On Wayland the clipboard and the primary selection are the compositor's, used over the window's own connection, and SilkTerm doesn't open any X connection (2026-10-15). Only an X11 window uses the X11 selections, Xwayland included. Wayland lets a client set a selection only while it has the keyboard, so there, a store a program makes while the window doesn't have focus is saved, the last one per selection, and set when the window gets focus back. A copy a person makes meanwhile, such as in a dialog, replaces the saved one (2026101607585700).

		- Pinned by `cicd/tests/wlclip/run.bash`.

	- Per-pixel transparency takes a different path on each platform: a hand-made glutin GL context on X11, the plain surface on Wayland, and DX12 composition on Windows. See the [Wallpaper](design_docs/20260930-150052_wallpaper.md) design doc.

	- Wayland coverage: smooth scrolling is identical on both engines. The scroll regression harness runs its scenes a second time under a headless `cage` kiosk (`run.bash --wayland`). Per-pixel transparency and dialog stacking on Wayland aren't tested yet (follow-ups).

- Pixel-precise input: touchpad gives true pixel deltas; notched mouse wheel snaps to lines (clamp/accumulate notch deltas into smooth target).

### Startup and slow external resources

- Nothing on the path from launch to the first frame may read an external resource that isn't needed to draw that frame. The wallpaper is the whole of that category today, and all of it runs on a worker thread. See the [Wallpaper](design_docs/20260930-150052_wallpaper.md) design doc.

- The config file itself is a deliberate exception. Window size, font metrics, and theme all come from it, and the window stays hidden until it can open at its final size. Reading it later would only trade a small local read for a visible resize flash.

- Shell discovery follows the same pattern. The window draws first, the scan for installed shells starts a few seconds after the wallpaper is on screen, and what it finds is folded into the shell list. See [The shell list and how it is filled](#the-shell-list-and-how-it-is-filled).

### Releasing resources

A window nobody is looking at doesn't draw any frames, and a minimized window or hidden tab freezes its rendering but never its reading. By default, an unused window gives its GPU device back and takes it again on any sign of life. After a return from a text console, the whole device is rebuilt. Full design: [Releasing resources](design_docs/20260930-151334_releasing-resources.md).

What a window costs while in use is planned in [Reducing resources](design_docs/20261004-182255_reduce-resources.md): a smaller scrim, the wallpaper kept at window size, software rendering on request or when the card can't make a device, and a Resource use group in Settings.

### Configuration format

- The user config uses SHCL, from a sister project, in place of TOML. The file is `config.shcl`. The reference parser is a single zero-dependency crate, so dropping `toml`, `toml_edit` and `serde` made the released binary smaller rather than larger.

- The deciding property is forgiveness. A malformed line yields a diagnostic and is skipped, so one bad value costs only its own setting. Strict TOML could instead fail the whole document and reset every setting to its default. That forgiveness also let 2 workarounds be deleted outright: a retry loop that blanked offending lines and reparsed, and a rewrite pass for leading-dot floats, which are valid here.

- Values are typed by the reader rather than the file, so there is nothing to get wrong in the syntax and a value is stored back exactly as written.

- shcl 3.0 reads a backslash outside double quotes as itself, where 2.x read it as an escape, and 2.x wrote Windows paths that way (2026-09-24). A file whose footer doesn't have a `Format` line is taken as 2.x, and the launch has shcl rewrite it to read the same before any other step parses it. The new footer has that line, so it happens once. A footer somebody rewrote keeps their wording, and shcl's own stamp goes under it.

- Any write that moves the file to a newer format first saves the file as it was beside it, as `config_backup_YYYYmmDD-HHMMSS_format-v<N>.shcl`, where N is the format it had. A file without a `Format` line is format 2, since that is how the conversion reads it (2026-10-03).
	- Every older version is kept, never one per format (2026-10-03). The stamp is local time, like the other file names the project stamps. A file named by `--config` uses its own name, less any `.shcl`, in place of `config`.
	- The copy is made in the writer every settings write goes through, so a save that converts a file a launch had to leave alone makes one too. The write is refused when the copy can't be made.
	- A copy is never replaced. It is written under a name of its own and then linked into place, which fails rather than replace anything. A second copy in the same second takes `_2` before the `.shcl`, and so on. A name that already has the same bytes is the same copy, from another window converting at that moment, so 2 windows converting at once leave one whole copy and never a part of one.
		- A conversion can take over a second on a busy box, so windows converting at once can still stamp different seconds. A new copy then settles on the earliest copy of the same format with the same bytes, whatever its stamp, and is dropped if that is another one. If it is its own, the later copies with those bytes are dropped instead, since a window with a later stamp may have written first (2026-10-25).
	- A 2.x line with a list in brackets doesn't have a 3.0 form, so it's kept as written and doesn't set anything. The launch says so on the terminal, and once the window is on screen, in the notice window a refused save uses, with the count and the copy's name. The writer says it, so a save that converts a file a busy launch left alone says it the same way. A conversion that keeps every setting doesn't say anything (2026-10-03).
	- A file from before the performance profiles, such as one v1.0.0-beta3 saved, drew exactly what it set. If a row a profile governs is set there to anything but Max silk's value, the conversion writes `profile: custom` and `automatic: false`, so the look is the one the user chose rather than one a profile lays over it (2026-11-01). A file without any such row stays automatic. A file with any `performance` line, active or commented, is from after the profiles and is left to say what it says.
	- The pre-nesting conversion below is a layout change rather than a format one, and still keeps its `.bak`.
	- A file shcl can't read or can't migrate is written new from the template, with every setting that still reads (2026-10-03). Can't read means not UTF-8. Can't migrate means shcl's `migrate` doesn't add a `Format` line, as for a raw block that never closes. Every line shcl can't read and every setting with nowhere to go counts as lost, and is reported the way a list in brackets is. `upgrade` in config.rs is the one place that chooses, so a later shcl that converts whole files can take it over there.
	- A current file with lines that aren't UTF-8 is written again without them at launch, and the file as it was is kept under the same backup name, with its own format number (2026-10-03). It keeps its own layout as long as every other line reads as it did beside the bad ones. A block heading that doesn't decode would hand its lines to the block above, so such a file is written new from the template with every setting that still reads, and so is one where nothing is left that sets anything, which gives the defaults. The launch names the lines on the terminal and in a "Settings file rewritten" notice, with the copy's name. A launch that finds the file busy leaves it, reads what the rewrite would write, and the next write to the file does the rewrite and says it the same way. This replaced leaving the file alone and refusing every save, which kept a file that could never be saved.
		- The rewrite is at launch rather than the first save, since the launch already does every other format rewrite, and says it once with the window up. A first save is often a resize or the rating, which nobody connects with the file.

- While the size is automatic, with neither `window.columns` nor `window.rows` set by hand, the size written down is an ordinary window's. A fullscreen or maximized window isn't a size to come back to, so neither is remembered, because unfullscreening would otherwise leave every later launch opening at the size of the screen. The window's own columns and rows stay as they were, and a resize by hand still replaces them.

- Maximized is remembered on its own, under `remember_maximized`, which is off by default. A window closed maximized opens maximized, with the remembered size still under it, so un-maximizing goes back to that size. A size or fullscreen asked for on the command line wins over it. The state is read once a resize has settled, rather than in the resize event, since the window manager may set it after the resize it caused.
	- On X11 the maximized state, and a fullscreen from the command line, are written on the window before it is shown. A window manager ignores a request for a window it doesn't manage yet, and one sent after showing it showed the window at its restored size first.

- The font zoom is remembered with the size, as px on the font size. Zooming keeps the window's pixel size and changes its grid, so both are saved, and the next launch opens at the same pixel size rather than the old grid drawn bigger. A font size on the command line wins, and its zoom isn't saved.
	- With `resize_on_zoom` on, which is off by default, a zoom resizes the window instead, so each pane keeps its columns and rows. The minimap column is fixed in pixels, so it's the panes' grids that are kept and not the window's. The window grows only out to the right and bottom edges of the work area on its monitor, and past them the grid gets smaller as it does with the setting off. Zooming back to where a run of zooms started puts back the size the window had then. A maximized or fullscreen window keeps its size. So does a snapped one on Windows, the only system that says, and a Wayland compositor refuses a tiled window's resize itself. The size saved is the one the zoom asked for, in whole cells of the window's grid.

- With `remember_per_monitor` on as well, each monitor keeps its own size and zoom under `window.monitors`. The last size set anywhere is still kept, and a monitor with no entry of its own opens at it. A monitor is named for its resolution, its scale and its physical size in mm where that is cheap to read: from the X server on X11, the EDID Windows keeps in the registry, and `CGDisplayScreenSize` on macOS. Wayland gives a program no physical size, so there it is resolution and scale. Readings under a cm apart on either side count as one monitor, since the X server gives an EDID's size either to the mm or in whole cm, and which one it gives can change after a power save. The first entry in the file that matches is the one used.
	- A window moved to another monitor takes that monitor's size once it has been still for a moment with no mouse button down, so never in the middle of a drag. A size the user set after the move began wins over it, and one set before the move is saved for the monitor it was set on.
	- The window's own resize, and the one the system makes for a new scale, aren't saved as the user's. The move that the window's own resize can cause isn't taken as a new move, or a window on the edge of 2 monitors could swap between their sizes. A move while the monitors change still counts. Waking from power save, the window manager can herd a window onto the monitor that came back first and put it back a moment later.
	- A window the window manager moved because the monitor it was on went dark keeps its size and takes no other. xfwm4 puts such a window back where it was once its monitor is lit again, but forgets where once the window resizes itself, and that left it across 2 monitors (2026-11-01). It still notes the monitor it's on, so a size the user sets there is kept for that one.
		- On X11 a monitor whose output reads disconnected is dark, though the server can go on laying it out for hours. An HDMI monitor asleep can do that. GTK, and so xfwm4, drop such a monitor and herd its windows off it, and counted as lit, that herd looked like the user's move (2026-11-06).
		- A monitor still laid out at the same corner, at another size, was turned, rescaled or given another mode, and isn't dark. On X11 the window hears nothing of a scale change, so the user's next drag off that monitor read as a herd and took no size (2026-11-09).
	- Such a window also notes where it was, and at what size. Once that monitor is back, the window manager gets 2 s to put the window back, and if it hasn't, the window puts itself back (2026-11-01). A window the user moved or resized in between stays as they left it. While the monitor is dark the window looks for it every second, and every 5 s after a minute. The Per monitor row's tip warns that this can fight the window manager.
	- A size given on the command line stays through a move until the user resizes the window.
	- A scale change keeps the grid, unless a monitor's own size is about to take over. Left to the system the window keeps its size on the desktop, and a font's cells don't grow in step with the scale, so the grid moved a few cells each time. That's how a Wayland window, which learns its scale after it opens, came up at 78x23 for 80x24. Where a kept size will take over, the window is left to the system until then, since on sway a second resize of the window's own can come back late and undo the take. Either way the panes are laid out for the size the window is about to be, so a program inside is never told a grid half or twice the size for a moment (2026-10-14).
	- On X11 the server is asked which monitor the window overlaps most, rather than winit, whose list may predate a resolution change. A window that overlaps no monitor, as when its monitor has gone dark, is on none and keeps its size.
	- A window not shown yet has no place of its own, so the launch takes the monitor under the pointer. That is where xfwm4 opens a window that asks for no position, and where winit takes the window's scale from. Going by where the hidden window sat sized it for the top-left monitor, and it changed size a moment after it showed elsewhere.
	- Wayland tells a window neither where it is nor that it moved. It looks once right after it opens, on a scale change, and when the pointer comes back after a drag.
		- A move to an output at the same scale tells it nothing at all. So it also looks as the user resizes it or zooms the font, and the size is kept for the output it's on at that moment, with no take. Looking right before the save instead would give a size set right before a move to the new output (2026-10-15).

- A window opened with a size or a font on the command line saves nothing of its size: not the size, the font zoom, or maximized, for its monitor or as the last size anywhere, even after the user resizes it (2026-10-16). The options are for that one window, and a size set around them was the wrong size for the next launch without them. The ones that count are `--columns`, `--rows`, `--pixel-width`, `--pixel-height`, `--font-size` and `--font-name`, given for the window.
	- It still opens at a kept size where one applies, fitted to the font it has, and still takes a monitor's size after a move.
	- Changing the same value in Settings hands it back, and once each one has been, the window saves as usual. A font goes back with a change or its reset arrow. Columns and rows are one size there, so changing either hands back both, and the pixel options with them. Turning Remember last size off keeps the size it showed, so that alone doesn't count.
	- `--persist` has the window save as usual from the start. A size set at a font from the command line is then kept for the font a launch without it has: the same columns and rows if they fit the screen at that font, else the ones that keep the window's size.
	- Changes in Settings to anything else are saved as usual.

- A number given on the command line is limited to the range of the setting it stands for, the same range the file's copy of that setting is limited to. A count of rows or columns is also limited to what the graphics device can draw, since the window is a texture and a refusal there ends the launch rather than the setting.

### Windows find each other (2026-10-13)

- Every window is its own process. The windows on one settings file can find each other and send each other a message.

- A settings file gets a key, a hash of its real path, lowercased on Windows. The windows on one file meet in a folder named `silkterm-<key>` in the runtime folder (`XDG_RUNTIME_DIR`, else the temp folder). Each leaves an empty file there named by its pid.
	- A message names the key, and a window on another key ignores it. A recycled pid can sit behind an old marker until that is cleared.
	- A window that crashed leaves its marker. The next message that finds nothing listening behind one deletes it.
	- On a shared `/tmp` the folder has to be this user's own, and private, or the window listens alone.

- The endpoint is a Unix socket on Linux and macOS, and a named pipe on Windows. It's the one `--reload-settings` and `--wallpaper` already use.
	- A pipe is used rather than AF_UNIX on Windows. Both come with the system and need nothing linked beyond the `windows-sys` bindings already in use, but a pipe has no file to leave behind. It goes with its process, so a pipe that isn't there always means a window that's gone. A new instance is always waiting, made before the last one is connected, so the name never vanishes between 2 clients.

- Every read and write of the settings or state file takes a lock file in the meeting folder (`std::fs::File::lock`), so 2 saves can't interleave. A save waits up to 2 seconds for the lock and then goes ahead without it, rather than freeze the window behind one stopped in a debugger.

### Variables in a setting (2026-08-30)

- A setting that names a path or a program is text SilkTerm reads. No shell ever sees it, so nothing else would expand a variable written there.

- The spellings of 3 shells are read, all of them on every platform: `$NAME` and `${NAME}` from bash, `%NAME%` from cmd, `$env:NAME` and `${env:NAME}` from PowerShell. A leading `~` works too. Which shell somebody prefers shouldn't decide whether their config file works, and a file copied between machines keeps working.

- A few names that mean one thing under 2 spellings are paired: HOME with USERPROFILE, USER with USERNAME, TMPDIR with TEMP and TMP. Only names with a real counterpart are listed, since a guess would be worse than an empty expansion that can be seen.

- An unset name expands to nothing, the way a shell does it.

- A command is the exception. It is split into arguments first, then only the program name is expanded. Every argument after it goes through exactly as written.
	- The program name is expanded because nothing else would. SilkTerm starts it directly, so there is no shell in between to read a `%ProgramFiles%`. Splitting first also keeps a variable that contains `C:\Program Files\...` as one argument.
	- The arguments aren't expanded because the program itself reads them, and they already mean something to it. `cmd /k prompt $P$G` sets a cmd prompt, `bash -c 'echo $FOO'` wants bash's own `$FOO`, and a `%` in a unix path is a percent sign. Substituting any of those hands the program a word nobody typed, with no way to write a literal.
	- Every word was expanded until 20260916, which is what broke those 3. Reading only the local platform's spelling was tried first and dropped the same day. It fixed the cross-platform collisions but left the local ones, and it broke a config moved between machines for nothing.

- `shell.startup_directory` defaults to the home variable in the platform's own spelling: `%USERPROFILE%` on Windows, `$HOME` elsewhere. Both are read on both platforms, so a file copied to another machine still finds home. It was `~` for part of 20260916.

### Command-line options

- Most options describe a window to open: a hierarchy of tabs and panes built with create/select verbs, with look and behavior cascading window -> tab -> pane.

- The create and select verbs only say where the options after them go. A window opens on its first tab, and each tab on its first pane, however many the line makes (2026-11-03). The first tab is the one a plain launch has, and the one the window's options describe. Putting the last tab made in front, as Windows Terminal does, was the other choice. Panes would then have to follow, or a tab and its panes would answer the question differently.

- A second, much smaller family only prints something and exits. `--help`, `--syntax`, `--about`, `--donate` and `--version` never open a window, never read a config, and never touch a layout.

- Those flags are accepted in any position. The rest of the grammar cares a great deal about order, but answering a request for the help with a complaint about where the flag was written would be absurd.

- Output written for a person is padded with a blank line above and below, so it sits clear of the shell prompts on either side of it. `--version` is the exception, and it exists to be captured by a script, so it stays flush: the version line, then the copyright line under it. `--ver` and `-v` are the same flag.

- Every build has a build number, because a version can't identify one. Dogfood builds of the same release share a version, so a report of "it still does this on beta3" names something that could be any of a dozen binaries. The number is whole minutes elapsed since 2000 began, written in Crockford base 32 in lowercase. It's 5 characters until 2063, it sorts in the order the builds were made, and it decodes back to the minute it was built. Crockford's alphabet leaves out i, l, o, and u, so nothing read off a screen and typed into a bug report can come back as a different character.
	- It appears in `--version`, in `--about`, in Help > About, and in the release notes.
	- The pipeline pins one number for a whole run, so the 4 cross builds of a release report the same build rather than 4 made minutes apart. Without that the release notes couldn't name one. It is the commit's time, from a dirty tree too, so a rebuild of a commit gets the same number.
	- A non-release build shows ` (YYYY-mm-DD)` after the number. Only the build instant is baked in, and the date is worked out when the number is shown, so it is the viewer's local date. A release build shows the number alone. cicd marks a run as a release when it is on a clean `main`, the only state `release.bash` accepts, through `SILK_RELEASE=1`.
	- Unchanged sources keep the number they had. The binary didn't change, so neither should what it calls itself.
	- The release notes take the number out of the artifact being published rather than computing it again, so the notes can't name a build nobody can download.

- Where a shell starts is decided by 4 things, most deliberate first: `--directory` on the command line, then the directory inherited from the pane a new tab, pane, or window came from, then the directory SilkTerm itself was launched from if a shell launched it, then the `shell.startup_directory` setting. `--directory` cascades window -> tab -> pane exactly as `--shell` does, so the flag that names a shell and the flag that says where it starts behave alike.

- `--about` opens with an "About SilkTerm" heading and the same 2 lines as `--version` under it, then reports what a bug report needs: version, build number, which of the cross builds this is, and the GPU the renderer picked. It asks the graphics stack for an adapter but never builds a device, which is the expensive half. A box with no usable adapter loses 3 lines and still prints the rest.

- On Windows a release build owns no console of its own, so printing has to join the one that launched it. This happens only on the paths that print and exit. A terminal window attached to a console would die with the shell that started it.

- The contract on saving is that a user's comments and blank-line grouping survive. Layout may be tidied, meaning indentation and quotes that aren't needed, but a value is never rewritten. The default template is deliberately written the way a save would write it, so the first save is a no-op rather than a reflow of the file SilkTerm just wrote.
	- So a save must never change what the next launch loads. The renames and refreshes a launch applies before it reads the file decide only on what a save keeps: a value as it reads, in whichever quotes; a setting's place in its blocks as the file format reads it, with comment lines left out; and a commented line where a save would put it. Before this, a single-quoted old default font list was left alone until the first save put double quotes around it, and a setting under a commented-out heading was renamed only once a save had moved the heading.
	- Rejected: a save that writes every untouched line back as it was typed. That is a second writer beside the format's own, with placement rules of its own to get right. Also rejected: refusing a save that would move a value, since the person saving can neither see why nor fix it.
	- Adding missing settings at launch keeps the same promise (2026-09-18). A setting typed deeper than its block needs still loads, until an active line is added above it and takes it for a child. Such a line is moved out to its block's own depth, which a save would do anyway. Where that isn't enough, or a line would become unreadable, nothing is added and the launch says why.

- A save writes through a temp file and a rename, never in place, so a crash mid-save can't leave a truncated config. If Windows takes the old file off its name and then can't put the new one there, the new settings are written at the name directly, rather than leaving no file. If loading had to drop a line it couldn't place, the save is refused rather than quietly deleting it. A single changed setting isn't worth losing a line someone wrote.
	- A refused save is reported on screen, since stderr isn't seen by anyone on Windows, or by anyone who started SilkTerm from a menu (2026-09-18). Windows shows its own message box. Elsewhere a small window drawn like About stands in for one, since Linux doesn't have a message box every desktop has. It names the file and the lines, and says changes are used now but not kept.
	- Saves nobody asked for, such as a resize, a menu switch or shells found at launch, are reported once a session for each file, or every resize would raise it again. A refused OK or Apply in Settings is reported every time, and OK then closes, since trying again can't help. A file open in another program still keeps Settings open, because trying again can.
	- Editing only the lines a save changes, the way the rating does, would let most saves through. That was put off until shcl 3.0, which didn't change it. A line it can't place still counts as lost, so the save is still refused (2026-09-24).
	- Saves go through shcl's line-keeping save since 2026-09-25, so a line nobody changed keeps its quotes and indent. shcl still writes the whole file where it can't keep the lines, and it always does for a file with a dropped line, so the refusal stays.

- Settings opens on the file as it is now, rather than on what the window loaded (2026-09-22). Several windows share one file, and one that saved after this window loaded would otherwise not show here. The file then gets only what was edited in the dialog, and the window takes everything that differs from what it runs, so OK is also when another window's change is picked up.
	- The file is read when the dialog opens rather than watched. Opening Settings changes nothing on screen, and Cancel leaves the window as it was.
	- The session's own choices are folded in first, the way Reload config folds them: the command line's font and colors, a wallpaper named for the session, the Remote profile.

- The template's sections follow the Settings dialog's tabs, in the same order, so a person who has learned one has learned the other. That order applies to a new file only. An existing config keeps whatever order it has, since the code that adds new settings places them but never moves what is already there.

- The file is organized as nested blocks, tab-indented, which mirrors how the settings relate: `wallpaper` contains its children, with `rotate` and `contrast_mask` nested inside it. A setting can also be written as a single dotted line (`wallpaper.opacity: 0.1`) and reads identically. The block form is the canonical one.

	- Each setting is in its own blank-line-delimited section, comments directly above it: a short title, a description, and a range line where one applies. A commented-out line shows the built-in default and has a `## Default` marker.

	- The in-place add/refresh passes stay line-oriented. They resolve each line's full path from the indentation around it, so a new setting is inserted inside the right block, beside its siblings.

- Old TOML configs aren't migrated. A fresh file is generated with defaults.

- When the flat naming gave way to nested blocks, an old config converts wholesale rather than being rewritten in place. The old file is kept alongside as a backup, a fresh current-format file is written, and every value the user had set moves over to its new place. Rewriting flat lines into blocks would have mangled the old file's comments. This way settings survive and the file's documentation is current.

- A file an earlier build converted with its image written onto the `wallpaper:` heading is repaired where it is at launch. The value moves to `image:`, unless the file already names an image there, which is the later choice and is kept while the heading's value is dropped. No other line changes, but the file is written back with LF line endings, as a conversion writes it. It isn't converted again, because that needs a backup name, and that build had used most of them converting the same file at every launch. Values that build dropped aren't read back from the backups, since a backup is older than anything saved after it.

- A config has the commented default lines it was first given, so when a default changes those lines start describing the old behavior. Such a line is refreshed to the current default. The file may be corrected about what the program does on its own, but never about a value that was set by hand. A line the user activated, or annotated, is therefore left alone.

- Starting over is a rename rather than a delete. `--reset-config` moves the file aside and lets the next launch write a fresh one, so the previous settings stay recoverable.

- Some defaults are better inferred from the config directory than stated in the file. A folder of wallpapers in the expected place is taken as wanting them rotated, and nothing is written back. That, the folder's default, what shows when nothing is named, command-line wallpapers and the XMP layout and look tags are in the [Wallpaper](design_docs/20260930-150052_wallpaper.md) design doc.

## Delivery (CI/CD, branches, releases)

Guiding constraint: GitHub is dumb git hosting plus optional release storage, nothing more. No hosted CI, no Actions, as few third-party tools as possible; the whole pipeline runs locally (`cicd/cicd.bash`).

- Standard runs (2026-10-26, from the CICD reset): `--backlog-run-quick [<area>...]` is the fast check to run before a commit, in about 4 minutes: the format check, clippy for Linux, the tests, the script lints and the docs checks, or only the areas named. `--backlog-run-full` and `--merge-gate` run every check of the default run, the other boxes too, with the format checked rather than rewritten. None of the 3 syncs, dogfoods, records the demo, publishes, or prompts. `--gate` is another name for the quick run, and `--ci` for the merge gate. `--keep-temp` keeps a run's temp folders and containers and prints where. `--list` prints the steps and test IDs the other flags would run, and runs nothing.

	- The default run takes about an hour (2026-10-28). `--long`, or `--slow`, is everything, the steps that take hours too: it adds the TSan run and CodeQL, about half an hour each, and the Miri run, which takes hours. It works with `--backlog-run-full` and `--merge-gate`, and not with `--quick` or `--backlog-run-quick`.
	- Every part of a full run has an option that turns it off (2026-11-03), so a test batch can go on past one that fails: `--no-rust-tests`, `--no-clippy`, `--no-script-lint`, `--no-scrub`, `--no-deny`, `--no-script-tests`, `--no-docs`, `--no-scroll`, `--no-wingui`, `--no-release` and `--no-cleanup`, next to the older ones such as `--no-asan` and `--no-remote`. `--no-release` also turns off what reads the release binaries (the packages, the ARM64 smoke run, dogfood, and the demo), so nothing runs on an earlier run's build. The debug build has no such option. If it fails the code doesn't compile, and every later build would fail the same way. `cicd-win.ps1` takes the ones for the parts it has, as `-NoRustTests`, `-NoClippy`, `-NoScriptLint`, `-NoDeny`, `-NoScriptTests`, `-NoRelease` and `-NoCleanup` (2026-11-04). Under `-Wsl` each goes to the Linux half as its `cicd.bash` flag, and so do `-NoScrub`, `-NoDocs`, `-NoScroll` and `-NoWingui`, which only it has a part for.

- Merge gate: `cicd.bash --merge-gate`, every check of the default run, runs as the `pre-push` hook for pushes to main (2026-10-28). This is the local stand-in for a hosted CI workflow. Pushes to dev and feature branches aren't gated, since a branch is tested before it merges into dev. The gate reads the commit being pushed, from a throwaway worktree checked out at that commit, rather than the working tree, since the 2 are routinely different here, and a fix still uncommitted would otherwise get the push through. Cargo writes to the usual output directory, or `target/container` when docker is here, so only this crate is rebuilt there, rather than its dependencies.

	- A push whose changes are all docs and art no build or test reads skips the gate and the version guard below, so docs can go straight to main. Those paths are listed in one place, `utility/git-hooks/no-gate-paths`, which the hook reads from the commit being pushed. A change to that list is always gated. A file a build or a test reads, such as `usage.md`, `README.md` or `comparison.md`, stays gated.

- Version-bump guard: the same `pre-push` hook blocks a push to main unless its `source/Cargo.toml` version is a strict increase over the version already on main, by full semver precedence including prerelease ordering. So a release merge can't publish the same or a lower version. It also requires the README Release badge to match that version, the same check `release.bash` makes, only earlier. It skips on the first main push, on branch deletes and on a push of docs alone, and is overridable with `--no-verify` / `SKIP_GATE=1`.

- Branch flow: feature branches merge `--no-ff` into `dev` (the integration target). `main` is release-only: merging dev into main cuts a release.

- Releases: `cicd/utility/release.bash` tags the merge `v<version>` and can push the tag and attach the artifacts to a GitHub Release as plain uploads. The version comes from `source/Cargo.toml` alone. The tag is read from it and the build stamps from it, so they can never disagree. Version and README badge get bumped on dev before the release merge. Nothing is ever committed directly on main. The release notes list the downloads in a table, OS by architecture, built from the files being uploaded (`cicd/utility/release-notes.bash`). There is no macOS row, since macOS is sold through an app store.

- Machine use: a step gets half the CPUs, or a quarter when the 1-minute load is over half the CPU count or the step is hard on the box (the sanitizer runs, the leak checks, CodeQL, the fuzz soak). `cicd/utility/include/jobs.bash` works the count out before each step, for the pipeline and its helper scripts alike. The run goes under `nice -n 19` and `ionice -c 3`, a container gets `--cpus` and `--cpu-shares 256` as well, and `cicd-win.ps1` runs at `BelowNormal`, or `Idle` for a hard step, judging busy by the CPU's load percentage, since Windows keeps no load average.

	- Memory: each step has a cap with no swap, `CICD_MEM_CAP` (a docker size such as `24g`, or `none`), so a step that runs away is killed alone rather than the desktop. The default is a third of RAM, from 4g to 32g, and 48g for CodeQL, which peaked at 32 GB. A container gets `--memory`, `--memory-swap` at the same size and `--oom-score-adj 1000`. Where a systemd user manager runs, the pipeline runs itself again in a user scope with `MemoryMax`, `MemorySwapMax=0` and an OOM score of 1000, which applies to the steps on this side. A step killed at the cap exits 137. The code is in `cicd/utility/include/memcap.bash`.

- Build matrix (buildable from this Linux x86_64 box): Linux x86_64 (native) and, via `cargo-zigbuild` + `zig`, Linux ARM64, Windows x86_64 (mingw), Windows ARM64. macOS and BSD are deferred, since cross-building them needs an Apple SDK (osxcross, license-gated) or a FreeBSD sysroot, neither present here. The debug build is what the tests and profiler run against, and the optimized release builds feed packaging and dogfooding. ARM64 targets are on by default, since zig cross-builds aren't emulated and so aren't much slower, and they drop out with `--no-arm`.

- Windows x86_64 toolchain for releases: the gnu (mingw) build is the canonical released Windows x86_64 binary, and the msvc build is deliberately left out of the Linux-cut release. The gnu build cross-builds from this Linux box in the same run as everything else, and is self-contained. msvc can only be built on Windows, since it needs `link.exe`, so folding it in would mean a Windows->Linux binary hand-off. Since the msvc build was made crt-static it no longer offers end users anything gnu doesn't. Its remaining edges, PDB/WinDbg debugging and a standard ABI, are dev-side only. `cicd-win.ps1` still builds msvc on Windows for local dogfooding and debugging. There are 2 things that would reopen this: Authenticode code-signing, whose natural home is Windows and would pull Windows-installer finalization onto that box; or evidence that mingw binaries trip antivirus reputation heuristics enough to matter. `makensis` itself is host-agnostic, so building the Windows installers on Linux is a non-issue independent of this choice.

- Packaging (pipeline stage 6, when `--quick` isn't passed): built from the stage-5 release binaries, never rebuilt. Linux -> `.deb` (cargo-deb) and `.rpm` (cargo-generate-rpm) per arch, driven by `[package.metadata.deb]` / `[package.metadata.generate-rpm]` in `source/Cargo.toml`. Windows -> one self-contained NSIS installer `.exe` per arch (`cicd/packaging/windows/installer.nsi.in` + `makensis`). It upgrades an existing install in place by running the old uninstaller first, and needs no bundled runtime because the binary links only system DLLs. RPM versions can't contain `-`, so `1.0.0-beta1` is emitted as `1.0.0~beta1`. AppImage/Flatpak and the deferred macOS `.dmg` / BSD packages are future work.

- Dogfood delivery: every build the pipeline makes is installed under a fixed name in a synced app dir for the platform it targets (`.../exec/app/{linux,mswin,macos}/`), with the icon and a sidecar naming the build beside it. That is the only place a launcher looks. It replaces an older arrangement where each box also read the build host's clone over the network, which meant a probe, 3 bounded waits, and a whole class of "why did the shortcut hang" that no longer exists. The cost is that a build is copied to another box at Dropbox's pace rather than right away, which doesn't matter for dogfooding.

- There's one launcher for 3 platforms. `utility/n8runterm.ps1` is the whole implementation, and `runterm` (bash, for Linux and macOS) and `runterm.cmd` do nothing but find it and pass the arguments on. The 2 previous launchers were the same program written twice, and they had drifted apart in ways that only showed when one box behaved differently from the other. PowerShell 7 is now a requirement for launching a dogfood build on any platform, which is the price of having one implementation to keep right.

- Where copies live: a versions folder beside a `silkterm` symlink pointing at the newest. Anything that names the program, such as a name on PATH, a menu entry, or an icon, points at the symlink and so never needs rewriting when a build arrives. The folder is GFS-rotated on every launch: newest and oldest always, then the last couple, then the newest of each ended day, week, month, and year, capped per tier so a box that builds daily still keeps something from months back. There are at most 10 copies and at least 5, and in between it stops at 1 GB. A copy that is running is never deleted, so a window open on an old build keeps its binary.

- A menu entry runs the launcher, never the terminal. A shortcut naming a build pins that build forever, which is how the entry went stale before. The launcher writes and refreshes the entry itself, from the paths it already knows, so it can't drift from where things are. Its icon comes from beside the symlink, which is a `.png` on Linux, the symlinked binary's own icon on Windows.

- Artifact naming (stable; download links depend on it): `<exe>-<version>-<os-arch>[.exe]` for binaries, `<exe>-<version>-<os-arch>.{deb,rpm}` and `<exe>-<version>-<os-arch>-setup.exe` for packages, plus `<exe>-<version>-sha256sums.txt` (for binaries and packages), all collected into `cicd/artifacts/release/`.

- Pinning: `rust-toolchain.toml` pins rustc/clippy/rustfmt and the cross targets. The cargo-installed helpers (cargo-deny, cargo-zigbuild, cargo-deb, cargo-generate-rpm) and makensis are pinned in `cicd/tool-pins.txt`, which both pipelines read, with a non-gating drift warning. Dependency freshness is a periodic local `cargo update` pass. A known advisory against a locked dependency fails every full run (`cargo deny check advisories`). The scan looks at what goes into a release binary for some release target, so our dev-dependencies and crates only other platforms use are left out. cargo-deny can't leave out build-dependencies, so build scripts and proc macros are still scanned. An advisory whose fix has to wait is ignored in `deny.toml` by name, with the backlog item that tracks it.

- Leak checks under AddressSanitizer (2026-10-19): every full run builds and runs the test suite again with ASan and LSan, through `cicd/utility/asan.bash`. Sanitizers need a nightly, so `rust-toolchain.toml` dates one in a comment under the channel, and only the container image installs it. A run on the host does only this step in the image, so the nightly is never installed on the box. `-Zbuild-std` instruments std as well as every dependency, and the explicit `--target` keeps the flags off build scripts and proc macros, which the leak checks leave out anyway. The build dir is named for the nightly (`target/asan-<date>`), so a bump leaves the old one for the cleanup.
	- A canary crate that leaks on purpose (`cicd/tests/sanitizer-canary`) goes first, under the same flags and suppressions. If its leak isn't reported by name, the step stops, so a setting or a suppression that hides every leak can't pass quietly.
	- LSan reports at exit, so one leak fails a whole test binary, and a test that runs a child copy of the binary sees its child fail too. Rerun the binary with `--test-threads=1` and a test name to find which.
	- `cicd/lsan-suppressions.txt` lists what is let through, each with the backlog item that fixes it. A test that can't mean anything under ASan, such as one that measures peak memory, is ignored under `cfg(silk_asan)` with the reason.
	- It adds about 15 minutes to a full run here, against under 2 for the plain suite. `--no-asan` skips it, and so does `--quick`.

- Data races under ThreadSanitizer (2026-10-23): the program starts threads, so a `--long` run also builds and runs the test suite under TSan, through `cicd/utility/tsan.bash`, after the leak checks. Same nightly, same image, and the same `-Zbuild-std` and `--target`, which matter more here: an uninstrumented std makes TSan report races that aren't there. ASan and TSan can't share a build, so it has its own dir, `target/tsan-<date>`.
	- The canary crate also races on purpose. The step stops unless TSan reports that race by name.
	- `cicd/tsan-suppressions.txt` lists what is let through, each with its backlog item. A test that can't mean anything under TSan is ignored under `cfg(silk_tsan)` with the reason.
	- The first run found only races inside Mesa's software Vulkan, a system library TSan can't see into, and those are let through by library name.
	- It adds about 30 minutes to a full run here, at a load of about 45, against about 15 for the ASan run. That put the default run at about 90 minutes, so it moved to `--long` on 2026-10-28. `--no-tsan` skips it there.

- Undefined behavior under Miri (2026-11-02): a `--leakcheck` or `--long` run also runs the test suite under Miri, through `cicd/utility/miri.bash`, after the TSan run. It finds undefined behavior in `unsafe` code, ours and the dependencies', and memory still allocated at exit. Same nightly, with its `miri` component, in the same image. The build and Miri's sysroot share one dir, `target/miri-<date>`. Isolation is off, since the tests read the clock and the test folder.
	- It takes hours, since Miri interprets each test binary on one core. A test over 60 s of CPU time under Miri is ignored there as too slow, and so is one that calls into C or starts a process, each with its reason (2026102320591313). What runs is the fast part.
	- The plan says it's very slow before the run starts, so Ctrl+C can stop it there. `--no-miri` leaves it out.
	- The canary goes first, each of its tests alone, since a report stops the whole test binary. The step stops unless Miri reports the race as a data race and the forgotten Vec as leaked memory.

- Heap growth (2026-10-19): `cicd.bash` runs the leak checks in stage 3, after the ASan run. A full run has them since 2026-10-22, and `--quick` since 2026-11-01, since this test takes under a minute. `--no-leakcheck` drops them, `--leakcheck` adds the slow ones too, and `--backlog-run-quick` refuses `--leakcheck`. The first is a heap-growth test, `source/src/heap_growth.rs`, run in-process with dhat as the allocator. Each of the main actions runs 20 rounds to fill its caches, then 20 more, and the heap in use at round 40 may be at most 4 KiB or 10 blocks over round 20. That catches what the sanitizers can't, such as a cache in a static or a list that only grows, since that memory is still reachable at exit.
	- The actions are a tab's engine opened, fed and closed, output into a full scrollback with a frame per read, the minimap folding new output, text shaping with fallback and color glyphs, the config read at launch and a value saved, and the Settings dialog's spec parsed. The third-party crates under them run as they do in the program.
	- A planted leak is one of the actions, and the test fails unless it's caught, so readings that stopped counting can't pass. A growth that already has a backlog item is reported with its ID and doesn't fail the test.
	- dhat counts every thread, so the test gets a run of its own, with one thread and its module as the filter. The `dhat-heap` feature puts dhat under the counting allocator in test builds only, so a release build is the same either way. Only the silkterm crate changes with it, so it builds in the debug dir.
	- The report goes to `cicd/artifacts/leakcheck/`, rotated like the flamegraphs, with the dhat profile beside it when the test fails. It takes about 30 seconds here.

- Soak test (2026-11-01): the leak check for what the heap-growth test can't reach in-process, `cicd/tests/soak/run.bash`, run after it and before CodeQL. It drives the debug build on a private Xvfb under xfwm4, with heaptrack watching. Each round opens a tab and a split and types into both, opens and closes the Settings dialog and a second window, and closes the tab again. After 6 rounds and 6 more, the live heap at round 12 may be at most 256 KiB over round 6. heaptrack sees every allocation in the process, the graphics driver and the other C libraries included.
	- It runs in the container image, which pins heaptrack, so a run on the host does only this step there, like the ASan run, and warns and skips it without docker.
	- Every shell is a test script, given by `--shell` and the test config, never the user's shell or its config. Each one notes its pid with what it got, so a hotkey that did nothing fails the test rather than reading as a flat heap.
	- `--plant` preloads a shim that leaks 64 KiB each time the program starts a child, and the test must fail with it.
	- It takes about 2.5 minutes, so `--quick` leaves it out unless `--leakcheck` asks. The summary and heaptrack's data go to `cicd/artifacts/leakcheck/` as `soak_<stamp>`, rotated like the heap reports. The data is about 10 MB a run.

- CodeQL (2026-10-22): the second leak check, `cicd/utility/codeql.bash`, off unless `--leakcheck`, `--codeql` or `--long` asks, since it takes about an hour and about 19 GB here (65 minutes on 2026-11-03). It runs CodeQL's `rust-security-and-quality` queries in the container image, which has the CodeQL bundle at a pinned version, so nothing goes to a hosted service. CodeQL reports only on source inside the database's root, so the workspace is copied under the target dir with its dependencies vendored beside it, and they're extracted as source. Results count in our crates and in the crates a release binary links for x86_64 Linux. Those in build scripts, proc macros, the crates only proc macros use, and dev-dependencies are dropped by path.
	- It stays out of the default run, though the 2026-10-24 CICD reset called for it there. That run takes about an hour without it, and under the same reset a step past the hour goes to `--long` (2026-11-01).
	- Any result left fails the step. `cicd/codeql-suppressions.txt` lets one through by rule and path, with the backlog item that fixes it, and for a flow, optionally by the values its message names, such as its source, by name or by the file it starts in. The SARIF goes to `cicd/artifacts/leakcheck/`.
	- Rust has no traced build in CodeQL. Its extractor runs rust-analyzer, which needs a toolchain of its own with the std source, so the image pins that too. When it can't load the workspace it goes on without types or macros and gives wrong results, so the step fails on that instead.

- The pipeline in a container (2026-10-10): `cicd.bash --container` runs stages 1 to 6 in an image built from `cicd/container/Dockerfile`, Debian trixie with every tool at its pin, and this box takes over at the private runner. The image's tag hashes the recipe and the pinned versions, so a bump makes a new image and the old one is dropped. The run inside gets the tree at its own path, its own target dir (`target/container`) and a named volume for the crate cache, and nothing else of this box: no home, keys, fonts or live config. What that buys is a test run that can't read the user's config or write the box's caches, and a toolchain that doesn't drift with the box. What it costs is a second set of build output, and a container that can't reach the card or the Windows boxes, so the GPU, dogfood, demo and remote stages stay outside. The other boxes weren't given one. vm925w runs with its hypervisor off, so no containers there, and a Linux container on the Mac builds nothing for macOS.
	- A test that skips there for want of a tool fails the run instead (2026-11-06), since the image is meant to have every tool the tests use. The Wayland window tests had skipped for want of wtype in every container run for a week before that, unseen. A skip for any other reason, such as no render node, stays a skip, and so does a missing tool on the host.

- Reproducible builds (2026-09-24): one commit built in 2 folders gives the same bytes for all 4 published targets. The path map keeps the folder out, the build number comes from the commit's time on a clean tree, and the Windows linkers are told not to write the link time into the PE header: `--no-insert-timestamp` for mingw, `-Brepro` for zig, which refuses the other. `cicd/utility/repro-check.bash` is the check. The msvc build that `cicd-win.ps1` makes for Windows boxes isn't published and still has its link time.

- README badges: static shields only (release, license, minimum Rust). No CI badge, since there is no hosted workflow to point one at.

- Wallpaper gallery on GitHub Pages: `docs/` is served from main, and has one self-contained page, a thumbnail grid whose tiles open the wallpaper full size in place, with prev/next paging, a filter box and per-image provenance. A README can't do this, because GitHub renders no scripting and strips image maps, so a single contact sheet has no clickable tiles and there is no way to page through anything. It stretches the guiding constraint above, on purpose. Pages here is branch-served static files with no Actions workflow, GitHub builds nothing, and if it were switched off tomorrow the only casualty would be one README link. The page has thumbnails only (about 1.4 MiB) and fetches each full image from the pack already in the repository, so the 60 MiB of wallpapers is never stored twice. Both it and the README contact sheet come out of `cicd/utility/wallpaper-gallery.bash`, which is the one entry point on purpose, so the 2 rendered artifacts from one pack go stale together or not at all.

### A tab can be named by hand, and the window title follows the tab (2026-08-30)

Double-clicking a tab renames it in place. The strip has always drawn what the shell is doing, which is right most of the time and wrong when several tabs are running the same thing in the same tree.

- The edit starts with what the tab already says, all of it selected, so typing replaces it and any other key edits it. Enter or Tab keeps the change, Escape drops it, and a click anywhere else keeps it. Selection, Home and End, and paste all work. A pasted newline becomes a space, since a tab is one line high.
	- 20261002: The box edits as a Settings text box does, with the same keys on each platform: copy, cut, paste and select all, moving and erasing by words, and Shift to select. A click places the caret, a drag selects, a double-click takes a word and a third click the whole name. Right-click opens Cut, Copy, Paste, Delete and Select all, and a middle-click pastes the primary selection.
	- 20261002: Opening a menu leaves the rename up. Copy, Paste and Paste selection there act on the name, as Edit > Copy and Paste on the macOS menu bar do. Any other pick keeps the change first, as a click elsewhere does.

- Committing a blank title, or one that matches what the tab would have said on its own, puts it back to naming the shell. Those are the 2 ways out of a hand-typed title, and neither needs a control of its own.

- Titles need not be unique. People give 2 tabs the same name on purpose.

- The rename is keyed by the tab's position, so opening or closing a tab ends it, which commits it on an open and drops it on a close.

The window title is now assembled in one place, and always starts with the application name. A dogfood build says which one it is, since the pool has several and they are otherwise indistinguishable in the taskbar.

- After the name comes, in order: a title typed on the tab, else the title the running program asked for, else what the tab says about the shell. So a program that renames the window, such as an editor or a build tool, gets to the title bar without touching the tab, and a hand-typed tab title outranks it.

- A title that is only the name or path of a program is passed over, so the next source down is used, normally the tab's own label. A Windows console names a new window after the program it starts, so a shell that sets no title of its own comes with its own image path as the title. This was checked for cmd and for pwsh on 2 machines.

- A console that names the program and then the command it is running keeps the command, since that half says something. The name has to be a full path for that to apply, which is what a console writes. A bare name on the left would eat the file name in vim's "build.bat - VIM".

- The extensions that count are `.exe`, `.bat` and `.cmd`. `.com` isn't one, since far more titles end in a hostname or a directory than in one of the 3 DOS-era programs that still use it.

- A tab with a blank title lets the program's title through, and with neither the title is the application name. Blank means "defer" rather than "show nothing". A blank typed into the rename box goes back to automatic naming instead, so a blank tab title comes from an empty `--title` on the command line.

- A `--title` given on the command line is the whole answer, verbatim. It's an explicit request for exactly that string. The one thing it can't drop is the rights marker below.

Which of the 3 parts a tab names is now the user's to choose, and so is whether the window title falls back to the tab at all (2026-09-20). There are 4 switches on the Window tab, all on by default.

- A part that is switched off is dropped before the ladder of shortenings is built, rather than cut out of the finished label. A tab with the directory off therefore has no path rung to give up, and the rungs it does have are measured against what will be drawn.

- Switching all 3 off leaves the tab naming its shell. A tab with no text can't be told from the one beside it, so the shell's name is the floor.

- The tab's flyover still names all 3 whatever the switches say. The point of turning one off is to take the text off the strip while keeping it in reach.

- Turning the window title's switch off drops both of the tab's answers, the name typed on it and the text it works out for itself, since both are the tab talking. A title the running program asked for still comes through, and a `--title` is untouched.

A title the running program asks for can name the tab too (2026-09-20), which is a fifth switch beside the 4 above, also on by default.

- The order is the window title's, one step down: a name typed on the tab wins, then the program's title, then the text the tab works out for itself. The window title already fell back to the tab, so having the 2 disagree about what a program said would only have been confusing.

- It reads the program's title through exactly the same filter the window title does. A Windows console's own decoration comes off it, and a title that is only the name or path of a program doesn't say anything on either.

- The title sits above the tab's ladder of shortenings rather than inside it. A program's title has no shorter forms of its own, so folding it in would have thrown away every rung longer than whatever the program happened to say, and a narrow tab would jump straight from a full title to the shell's initials. Above the ladder, a tab too narrow for the title falls through the forms it works out for itself, and the floor is still the shell's name.

- It's on by default because the window title already prefers a program's title by default, and a tab that disagreed with the title bar above it would read as a bug. The switch is there for a shell that retitles on every prompt, which turns a strip of useful labels into a row of the same `user@host` text.

- The tab's flyover has a "Program title" line when there is one, whatever the switch says, the same as the parts above.

What a tab says is a template now (2026-10-17), in place of the 4 switches. It's one text row under Tab text, and `window.tab_text` in the file. The macros are the parts the switches chose between, plus a name typed on the tab: `%USER_DEFINED%`, `%PROGRAM_DEFINED%`, `%SHELL%`, `%PROGRAM%`, `%LAST_PROGRAM%` and `%CURRENT_DIRECTORY%`. [usage.md](../usage.md#tab-text) has the grammar.

- The default template reads exactly as the 4 switches did with all of them on, and an older file's switches become the template that reads as they did. A test checks both against the old code over every mix of switches.

- Text between 2 parts shows only when both sides show something. The switches' dash, between a shell and a path with nothing running, needs that rule. In `{ %SHELL% {[...] || -} %CURRENT_DIRECTORY% }` the `-` is a fallback with no macro, which counts as text, so it goes when either neighbor is missing. Text before the first part or after the last goes with its group.

- The template says what is on the tab, and the ladder of shortenings still sets how short it gets. Each rung is the template filled in with the parts at that rung's length, so the order the parts give way in is the same whatever the template says. A program's own title goes first, as it did, and a typed name never shortens.

- A template that can't be read is kept as typed, so there is something to fix, and tabs use the default meanwhile. Showing the broken text on every tab would make every tab unreadable over one typo. The Settings row's flyover and the launch both say what is wrong.

- The parser knows nothing about tabs. It takes its list of macro names from the caller, so another template, such as a session name, can use it.

### A terminal running with administrator or root rights says so (2026-09-09)

The window title starts with "Administrator: " on Windows and "Root: " elsewhere. Windows already writes its own elevated console title bars that way, so that word is kept rather than invented.

- No flag turns it off, `--title` included. The absence of the word has to mean something, and a marker a flag can remove means nothing.

- It goes on the window title only. Tab titles are left alone, and so are the Settings and About windows: neither is a taskbar entry of its own, and the terminal window they belong to already says it.

- The rights are the terminal process's own, read once on the way to the first window. A pane that elevates itself afterwards, `sudo -s` for instance, isn't marked, since the title would be claiming something about the wrong process. On unix the test is the effective user id, so a setuid binary reports the rights it has rather than the account that started it.

- An elevated Windows console writes the same word in front of the first title it sends, as measured over a pseudoconsole on 2 machines. That's the title naming the program the console started, so neither half says anything. The word comes off, and the program name is then dropped by the rule above. Without that step the word hides the name and the whole path is shown, which is the original complaint with rights on top.

- A title the program sets afterwards isn't decorated. In the same run, a title set to "build" came through as "build". So the word can't come twice from a console. It can still be typed twice, and a title already starting with it isn't given another.

- Only the exact word about to be put back is removed, and only on Windows. Nothing on unix decorates a title, so taking anything off there could only destroy somebody's own text. That means a program running as root that titles itself "Root: something" will show the word twice, and that is the right answer, because the second one is its own.

- A console speaking another language writes another word, and no list of words could include them all. So the title is also matched against the program the pane was started with. When a Windows path to that program follows a word, the word comes off in any language. Only the file name is compared, because a console writes the folders its own way. A word that contains a path separator, or one in front of some other program, is left alone.

### Tabs report what they are running, and where (2026-08-21)

A tab used to say the application's own name on Windows and the shell's process name on unix. It now reports the shell by its friendly name, which is the name the user gave it in the Shells list, followed by what that shell is doing: the command in the foreground, or the last one it ran, or, having run nothing, the directory it is in.

- The command in the foreground is the program on screen. In a pipeline that is the last stage still running, so `cat file | less` and `git log | less` both read less. The first stage names the process group, but it has often exited already. A program's own children don't count, so vim stays vim while it runs `:!make`. Windows has no process group and names a live child of the shell, as before. cmd doesn't wait for a program with no console, so that child can end after the prompt is back, with nothing printed. The child named is watched, and its end counts the same as output.

- The shell a pane runs is resolved once, when the pane is spawned. Leaving it as "whatever the default shell is" let the answer change under a running pane, since the background scan fills the list seconds after launch and the Shells tab reorders it, so a pane could be labeled with a shell it wasn't running.

- The path is shortened by an ellipsis eating the middle a directory at a time, so what is left keeps its real names. Only when that has run out do the directories above the current one drop to their initials, which on any path deeper than a couple of levels never happens, since the ellipsis has already saved more room than a column of single letters would. The anchor a path starts from and the separator it ends with survive every step, because they're what distinguish a location from a command. Windows keeps its drive letter and gets no `~`, since neither shell there prints one.
	- This reverses the original order (2026-09-20), which was PyCmd's: initials first, ellipsis only where it was shorter still. It came from nemo-anywhere, which had already been through the same argument. A middle left out reads as a place with a gap in it. A column of initials doesn't read as the path or anything else, and it gives up every name at once to save a few columns.

- Tab width is 2 percentages of the window rather than a fixed cap, so the extra text has room on a wide display while a lone tab still reads as a tab. See the entry below for what those 2 now mean.

- When the tabs stop fitting, the strip shows a page at a time rather than shrinking them to nothing. The wheel over the tab bar turns the page, and switching tabs brings the new one onto it.

- A hover tip shows what the tab can't: the shell's name, the command line behind it, whatever is running now, the whole path, and how long the tab has been open. It reads as a table, with one `key: value` per line and every value starting in the same column. That's why it's the one piece of chrome drawn in the terminal font rather than the interface one, since the column is made of spaces, and spaces align nothing in a proportional face. A value with a space or a quote is quoted, so its edges are never in doubt. The quote picked is the one the value doesn't already contain, the same habit the config file has, so a Windows command line reads inside single quotes instead of fighting its own double ones. What's derived rather than quoted, such as the clock reading and the note that no directory was reported, stays bare, since quoting those would say they were data.

### A tab is as wide as its own label needs (2026-08-23)

Tabs used to divide the bar evenly between a minimum and a maximum percentage of the window, so every tab was the same width whether it had anything to say or not. They now size themselves.

- The first percentage is the regular width, which is what a tab is when nothing is pushing on it. It's a target rather than a share, so 3 tabs on a wide bar stay at it and leave the rest of the bar empty, rather than a couple of them stretching across the window.

- A tab whose label wants more room grows past it, up to the maximum. A crowded bar pushes every tab back below it. Every tab gets to the regular width before any grows past it, so a long path can never cost another tab its ordinary size, and under crowding each tab gives up the same fraction rather than the last few being starved.

- The tab in front is the exception (2026-09-20). It takes what the row can spare before any other tab grows past its ordinary width, and the maximum doesn't apply to it, so it shows its full label wherever there's room. The room it leaves is no use to tabs already at the cap, and it is the one being read. The tabs behind it give way for it, each dropping to whatever rung of its own ladder still fits, and none of them goes below the regular width. With the default 10% regular width that can leave an inactive tab saying only its shell's name. Raising the regular width gives them room for a path again. This was taken from nemo-anywhere, whose tab row had the same problem.

- Defaults are 10% regular and 100% maximum. The old pair (8% and 26%) made sense when the bar was divided evenly. A maximum now only says how far one tab may grow when it has the room, which is worth allowing in full for a window with a single tab.

- The floor a tab may not shrink past is its own shortest label, which is a short form of the shell's name and nothing else. Tabs past that point become a page.

- What a tab says now gives way in a fixed order, rather than only the path shortening: the shell's name shortens first, then the running command's name is truncated, then the path abbreviates, then the command goes, then the path, and what is left is the shortest form of the shell's name. The path is shown alongside the command now, where before a tab running something said only what it was running.

- Short shell names are hand-picked for the built-in shells ("Windows Cmd" reads "Cmd", "PowerShell 7" reads "PS 7") and derived for anything renamed, since nothing mechanical gets to "Cmd" from "Windows Cmd". A derived name keeps its distribution rather than its family ("WSL2; Ubuntu" reads "Ubuntu") and marks a variant with a star, so "Zsh" and "Zsh*" at least say that one of them isn't the ordinary one.

## Copyright and license

> Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]\
> Licensed under the [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/) (CC BY 4.0).\
> This document is provided as is, without warranty of any kind.
