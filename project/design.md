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
	- [Language / Stack Decision](#language--stack-decision)
	- [Logical code organization](#logical-code-organization)
	- [API (alacritty_terminal)](#api-alacritty_terminal)
	- [Smooth scrolling](#smooth-scrolling)
	- [Minimap](#minimap)
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
	- [A prompt is offered to bash, never installed (2026-08-30)](#a-prompt-is-offered-to-bash-never-installed-2026-08-30)
	- [Opening files from Explorer on Windows (2026-09-30)](#opening-files-from-explorer-on-windows-2026-09-30)
	- [One tip system, four places that draw it (2026-08-30)](#one-tip-system-four-places-that-draw-it-2026-08-30)
	- [Render Loop Sketch](#render-loop-sketch)
	- [Speed](#speed)
	- [The About box says how long the session has been up (2026-09-20)](#the-about-box-says-how-long-the-session-has-been-up-2026-09-20)
	- [A character handed to the window is typing (2026-09-09)](#a-character-handed-to-the-window-is-typing-2026-09-09)
	- [What untrusted input may not do (2026-09-09)](#what-untrusted-input-may-not-do-2026-09-09)
	- [The fuzzer (2026-09-09)](#the-fuzzer-2026-09-09)
	- [Environment](#environment)
	- [Startup and slow external resources](#startup-and-slow-external-resources)
	- [Releasing resources](#releasing-resources)
	- [Configuration format](#configuration-format)
	- [Variables in a setting (2026-08-30)](#variables-in-a-setting-2026-08-30)
	- [Command-line options](#command-line-options)
- [Delivery (CI/CD, branches, releases)](#delivery-cicd-branches-releases)
	- [A tab can be named by hand, and the window title follows the tab (2026-08-30)](#a-tab-can-be-named-by-hand-and-the-window-title-follows-the-tab-2026-08-30)
	- [A terminal running with administrator or root rights says so (2026-09-09)](#a-terminal-running-with-administrator-or-root-rights-says-so-2026-09-09)
	- [Tabs report what they are running, and where (2026-08-21)](#tabs-report-what-they-are-running-and-where-2026-08-21)
	- [A tab is as wide as its own label needs (2026-08-23)](#a-tab-is-as-wide-as-its-own-label-needs-2026-08-23)
	- [PowerShell gets the same prompt bash does (2026-08-21, reworked 2026-08-30)](#powershell-gets-the-same-prompt-bash-does-2026-08-21-reworked-2026-08-30)

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

- [Minimap](design_docs/20260930-150325_minimap.md)

- [Themes and text color](design_docs/20260930-150458_themes.md)

- [Speed](design_docs/20260930-150643_speed.md)

- [Unicode, fonts and emoji](design_docs/20260930-150813_unicode-and-emoji.md)

- [Split panes](design_docs/20260930-150948_split-panes.md)

- [Double-click and selection](design_docs/20260930-151047_double-click-selection.md)

- [Performance profiles](design_docs/20260930-151204_performance-profiles.md)

- [Releasing resources](design_docs/20260930-151334_releasing-resources.md)

- [The terminal engine and patched crates](design_docs/20260930-151451_alacritty-fork.md)

## Architecture

### Language / Stack Decision

Rust plus the `alacritty_terminal` crate, not a fork of the Alacritty application. The crate brings the PTY, the parser and the grid, and SilkTerm builds only the renderer. Three crates are carried on small patched branches, one per published release. Why, what is used and how the patches are carried: [The terminal engine and patched crates](design_docs/20260930-151451_alacritty-fork.md).

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

Frame loop: a PTY read or a user event marks the app dirty or starts an animation. `about_to_wait` renders when something is dirty or animating, and otherwise waits. A render advances the scroll easing, snaps the grid to the nearest whole line, and redraws each pane from current model state. Frames are driven from `about_to_wait` rather than redraw requests, because `request_redraw` is unreliable under X11/Compiz here. Timed wakes, such as a parked cursor's resume or a minimap compose that owes another, are read after the frame, since drawing is what sets them.

### API (alacritty_terminal)

The engine knows only whole lines (`display_offset`), so smooth scrolling lives entirely in the renderer. The calls SilkTerm relies on, and how the terminal is shared with the reader thread, are in the [terminal engine](design_docs/20260930-151451_alacritty-fork.md) and [Speed](design_docs/20260930-150643_speed.md) design docs.

### Smooth scrolling

The engine keeps whole lines, and the renderer draws a fractional offset over them. The wheel, the scrollbar, new output and full-screen programs all ease by moving that offset. New output runs through the output chase, a speed curve made of five named segments, one per setting. Full-screen programs are read from a scroll ledger in the engine fork, with row fingerprints as the fallback.

Full design: [Smooth scrolling](design_docs/20260930-144720_smooth-scrolling.md).

### Minimap

A column beside each pane showing the whole scroll buffer in miniature. The buffer always maps linearly onto the column and never slides, and the map stops where the eased text has reached. Lines are colored strokes weighted by how much each character inks. Full design: [Minimap](design_docs/20260930-150325_minimap.md).

### Text readability scrim

A soft halo in the background color behind every glyph, plus an optional crisp outline, so text stays readable over a busy wallpaper or a see-through window. The halo's shape and its fade are separate settings, and Strength thickens it into a plate.

Full design: [Text scrim](design_docs/20260930-145304_scrim.md).

### Themes and text color

Four built-in themes, each a dark and a light palette, plus saved themes stored whole. A minimum contrast floor in Oklab moves text that is too close to its cell's background, the block cursor's plate has to carry the text on it, and dark text on a light background is corrected to the weight an sRGB blend would give. Full design: [Themes and text color](design_docs/20260930-150458_themes.md).

### Wallpaper

A faint, blurred picture behind the text, from a named image, a rotating folder or the built-in one, prepared on a worker so it never delays the window. Visibility means the same amount of the picture's contrast in dark and light mode, and the text colors can come from the picture. The see-through window is covered there too. Full design: [Wallpaper and see-through windows](design_docs/20260930-150052_wallpaper.md).

### Performance profiles

One setting decides how much the look may cost: Max silk, High, Low, Standard terminal or Custom, plus Remote (temporary) for a remote screen. A profile sits over the user's settings and never changes them. The first pick is timed and written against a hardware fingerprint, and a session steps down on missed frames without writing it. Full design: [Performance profiles](design_docs/20260930-151204_performance-profiles.md).

### Fonts, Unicode and emoji

One monospace family is pinned for every weight, found from one search order on every platform. Anything that family lacks falls back glyph by glyph and is fitted to its cells, and color emoji are painted in-house. Full design: [Unicode, fonts and emoji](design_docs/20260930-150813_unicode-and-emoji.md).

### Hyperlinks

- URLs in the output are clickable. A link must carry a scheme from a fixed list - http, https, ftp, ftps, sftp, ssh, file, mailto - rather than being guessed from shape. That keeps false positives near zero, since output is full of words with colons and slashes in them. It is also the whole of the security story: a scheme outside the list is not a link, so it can never be handed to the desktop. Bare `www.` prefixes and bare file paths were considered and left out for the same reason.

- Punctuation is trimmed the way a reader would. A full stop or comma after a URL belongs to the sentence, and so does a closing bracket the URL is sitting inside. One the URL itself opened is part of it. A URL that wraps across rows is one link, found from either half.

- Hovering underlines, Ctrl+click opens, and Command+click on macOS. The underline appears on a plain hover with no modifier, since a link the user cannot see is a link they will not try. Opening needs Ctrl so it can never be confused with selecting. The press arms and the release opens, so a slipped press can be dragged off to cancel. A right-click on a link puts "Open link" and "Copy link" at the top of the menu, and only there. On macOS Ctrl+click is the right-click, so it opens the menu with the link rows.

- An app that is watching the mouse itself owns the pointer, so nothing underlines over it - holding Shift asks for the local behavior instead, the same bypass selection already uses. The right-click menu continues to win over such an app, as all our chrome does.

- Links open through the desktop's own handler by default, with a configurable program to override it. Deciding what a URL means is the desktop's job, not a terminal's.

### Double-click and selection

A double-click takes a shape it can name first, such as a path, URL or git remote, then the inside of a matched pair, then a word. A drag held past the pane's edge scrolls and keeps selecting. Copy has four routes, each with its own rule. Full design: [Double-click and selection](design_docs/20260930-151047_double-click-selection.md).

### Measurements and display scaling

Every measurement in the interface is written once in DIP, a ninety-sixth of an inch, and turned into real pixels only when it is drawn. A pop-out dialog converts once, at its window's edge. The main window's chrome converts at each measurement, since it shares its space with the terminal grid. `SILK_SCALE` overrides the scale factor for testing. The full rules are in the [Settings dialog](design_docs/20260930-145721_settings-dialog.md) design doc.

### The Settings dialog

A pop-out window with eight tabs, declared in the compiled-in `settings_ui.shcl`, drawn by SilkTerm and driven fully from the keyboard. Groups, sub-groups, the color picker, Apply and OK, and display scaling are all in the [Settings dialog](design_docs/20260930-145721_settings-dialog.md) design doc.

### The shell list and how it is filled

- The shells a new tab can be started with are one list, stored in the config as `shells.<key>` with a title, a command, an active flag, a comment, and the date a scan last found the program installed. File order is list order, which is also menu order. It is a plain part of the config, so it can be hand-edited, and the Settings dialog's "Shell" tab is an editor for something that already works - the list came first on purpose.

- **The list names the default shell: its first switched-on entry.** There was a separate `shell.default` setting saying the same thing, and two places claiming to name one shell can only ever disagree; one rule that is visible in the list is worth more than a second field. A config that carried the old setting has that entry moved to the top of the list, once, and the line removed - the value was the user's own statement of which shell they meant, so it is carried rather than dropped. Finding which entry it names is the same identity question the scan asks, not a string compare: the old setting was routinely a bare name where the scan had already stored the full path to the same file, and comparing the two as text put a SECOND copy of the user's default shell at the top of their own list, where the top is what "default shell" now means. An initial population is led by the shell the user logs in with, which is what makes the default right without their having said anything.

- Finding installed shells is background work that starts a few seconds after the wallpaper is really on screen - not merely the window. Both are off-thread and both are slow in the same way, so overlapping them puts a stall in the one moment anybody is looking: the gap between the window appearing and the picture arriving in it. A wallpaper that never answers (a share on a dead mount) cannot hold the scan off forever - there is a deadline past which it runs anyway, since a terminal with no shells in its menu is worse than a terminal with no picture behind its text. It stats every directory on PATH and, on Windows, reads the registry - any of which can be a mount or a hive that answers slowly - so none of it may sit between launch and the first frame. It runs on its own thread and the result is folded in when it arrives, the same shape the wallpaper pipeline uses.

- **What a scan may do to the list is deliberately lopsided.** It may add a shell it found, and it may switch off one whose program has gone - keeping the entry, its title, its flags and its place, since a shell that is merely uninstalled is not a shell the user stopped wanting. It may never switch one back on, and never rewrite a command line. A scan cannot tell a program that came back from a switch somebody turned off on purpose, and the cost of guessing wrong runs one way: quietly re-enabling something the user disabled is worse than leaving them one tick to undo.

- **A save of the list works from the file, not from what the window loaded.** Each window reads the list when it starts, and nothing watches the file, so another window may have saved since. A window that was behind used to take out only the entries it knew about and write its own list back after them. A shell another window had just found stayed on top of the whole list and became the default shell. Now the file is the third side of every save. What another window added stays, at the end, and what it removed stays gone. An entry this window did not change keeps the file's copy. The window's own order is written only when it moved something itself.

- Two shells count as one entry when they run the same program with the same arguments. Which program that is has to be resolved rather than compared as text, because the same shell is written several ways (`bash`, `/bin/bash`) and, on Windows, three environments ship a program called `bash` and they are not the same shell. Where a stored entry resolves nowhere at all, a bare name match is enough - that is what lets a reinstall re-arm the disabled entry it belongs to instead of sitting beside it as a second copy.

- **The order a fresh list arrives in is stated outright, in one place, rather than falling out of the sequence the looking happens to run in.** Each find is put in a group and the whole set is sorted once at the end. On unix the user's own login shell leads - nothing may sort above it, since the top of the list is what "default shell" means - then the modern cross-platform shells, the language REPLs, and the rest of the POSIX family. Windows has no user shell, so it is stated instead: PowerShell 7, the modern shells, the WSL distributions, the three POSIX-environment bashes, PyCmd, the language REPLs, Windows Cmd, and last the two Windows PowerShell 5 entries - the ones you reach for when something needs them rather than the one you open a terminal to get. Groups that hold shells of equal standing sort alphabetically inside themselves; groups that hold one shell built several ways keep a curated order, which is why MSYS2's full bash is offered above the mini one Git comes with.

- The login shell's twin sits directly below it and starts without reading its startup files. Each shell spells that its own way (`--norc`, `--no-rcs`, `--no-config`, `-NoProfile`), so the flag is per shell and the twin only exists where there is one. Only the login shell gets a twin; every shell having one would double a list nobody asked to be long. It arrives switched OFF: it is what you reach for when your own rc file is the thing you are debugging, not a second copy of your shell in the menu every day. `cmd.exe` is deliberately not on that table even though it has such a flag (`/d`, no AutoRun): it is what Windows reports as the command processor, so it is the login shell on every Windows box, and a second "Command Prompt" in everyone's menu costs more than a rarely-set AutoRun key is worth.

- WSL distributions are read from the registry, never by asking `wsl.exe`. A WSL2 distribution lives in a virtual disk, and listing what is installed must not be the thing that boots a virtual machine - that would be slow, surprising, and arguably a security problem for the user. Each distribution is offered whole, running its own default shell; anyone who wants a particular shell inside one edits the entry to say so. Its generation is part of its name (`WSL2; Ubuntu`), because that is the whole difference between two rows that would otherwise read identically, and the WSL2 ones are offered above the WSL1 ones. Both are offered where both exist: a WSL1 distribution is installed and usable, and hiding one because a newer-generation one sits beside it is not a call a scan gets to make. The generation is a bit in the distribution's registry flags - the `Version` value beside it is the registration format's version and reads 2 for a WSL1 distribution just as happily.

- **The Shell tab is the one place allowed to write the list, and a scan that arrives while it is open is folded into it rather than fought with.** Everywhere else the dialog carries the live list through untouched on Apply: a dialog that opened before a scan arrived would otherwise write back the empty list it copied then, emptying the menu for the rest of the session while the file on disk still had every shell in it. The tab needs to write it, so instead the scan is folded into both of the dialog's copies - the edited one, so the user sees what turned up, and the baseline, so the fold does not read as an edit they made. Because a scan only ever appends and switches off, folding it into work already done cannot undo any of it.

- The grid edits every field in the row rather than through a popup: it costs fewer clicks and reuses the field machinery the dialog already has. Four columns are fixed-width and the command takes whatever width is left, since it is the one value that is routinely too long to read at a glance. "Last seen" is read-only - it is the program's own note about the entry, and it is what makes a switched-off shell explicable. The command is required, which is enforced in the two places it can be broken: emptying the field leaves the stored command standing, and an entry that never got one is dropped on the way out of the dialog rather than written as a shell that names nothing to run.

- **Reordering is a mouse gesture on a grip, not four buttons.** Every line carries a drag handle at its left edge and the list reorders under the pointer as it travels, rather than on release - the line being dragged is the line that is seen to move, which is the whole reason to prefer a grip over arrows. It costs four Tab stops per line, and that is the trade taken knowingly: reordering has no keyboard equivalent now. The grip is therefore not a stop at all, since a focus ring on a control that Space cannot work would be worse than no ring.

- **Remove sits between the command and the date, and is drawn in red.** It is the one control in the dialog that destroys something, so it is deliberately kept off the right-hand edge that a pointer travels down on its way to the checkboxes. The red is chrome rather than a theme color: "this deletes something" is a fixed meaning, and a theme whose accent happened to be red would say it about every control at once.

- **How "where is this shell now" is answered has two halves: what the OS can see, and what the shell says - and the second wins.** Unix reads the link at `/proc/<pid>/cwd`. Windows has no equivalent and no API that reports another process's directory, so it is read out of the shell's own process memory, where SetCurrentDirectory keeps it; the result is checked for still being a directory first, so a layout that ever moved would degrade to "don't know" rather than to a wrong directory. Neither can see a shell that keeps its own idea of where it is - PowerShell's `Set-Location` never tells the OS - which is why the shell is also given a way to say so directly, in the escape sequence every terminal reads for this (OSC 7, and the ConEmu OSC 9;9 spelling that Windows Terminal documents). A report is preferred to the OS answer because it comes from the one that knows; a report naming a directory that is not here, or a machine that is not this one, is dropped and the OS answer stands.

- **The snippet is put into PowerShell profiles automatically, and what that licenses is deliberately narrow.** Asking every user to paste a block into a file before new tabs open in the right place is a poor trade when the block can be put there for them - but writing to somebody else's shell profile has to earn it. So: only a profile that reports nothing at all is touched (our marker or anyone else's OSC 7 / OSC 9;9 means it is in hand); it is appended to, never rewritten, after a copy is saved beside it; the marker makes a second launch a no-op; deleting the block is how it is switched off, and nothing puts it back; the prompt is not replaced, only wrapped where there is no other hook; and a shell whose execution policy would refuse to load the profile is left alone with a line saying why, because a file the shell cannot read is worse than no file. One setting switches the whole thing off before it starts.

- **The listening is done by wrapping the PTY, not by forking the VT parser.** The sequences arrive as bytes and the parser we use handles neither, so the obvious route was a fork of it. The route taken instead is that the PTY is an interface rather than a concrete type: a wrapper sits in front of the real one and scans what it reads on the way past, leaving the byte stream untouched. It costs a single pass looking for one byte, and it means no second fork to carry.

- **Where a shell starts is answered by four things in a fixed order, and the setting is the last of them.** A `--directory` on the command line wins outright; failing that a new tab, pane or window inherits the directory of the pane it came from; a SilkTerm that a shell launched keeps the directory that shell was in; and only what is left over - a launch from the desktop, a menu or a shortcut, where the inherited directory is an accident of whoever started us - reads `shell.startup_directory`. Its default is the home variable somebody on that platform would type (`$HOME`, `%USERPROFILE%`), because a setting whose default is a blank box says nothing about what may be put in it.

- The test for "did a shell launch us" is whether standard input is a terminal, and it is the same question on both platforms. Asking about parent processes would be more direct and costs a process-table walk on Windows, which is not something to put on the path to the first frame. Measured there: launched from a console the standard handles are the console's, launched the way Explorer and the Start menu do it they are null. A release build owns no console of its own either way, so the window-handle and attach-to-parent answers are both wrong for this question.

- Removing an entry asks first, and moving one does not. Doing the opposite undoes a move; nothing undoes a removal.

### What a pane's shell inherits

- A pane's shell inherits the environment SilkTerm itself was launched with. That is deliberate for anything the user set - an activated virtualenv, a PATH they added, a variable they exported before starting the terminal - and it is what makes a terminal opened from a shell behave as a continuation of that shell.

- It is wrong for the bookkeeping a shell keeps for itself. PowerShell 7 prepends its own module directories to the module search path that every version of PowerShell shares, so a Windows PowerShell 5.1 pane opened anywhere below one resolves PSReadLine to PowerShell 7's copy rather than its own and is not allowed to load it - the pane then starts with an error and no line editing. The execution-policy variable is the same shape: one shell sets it and everything that shell starts inherits it, so a pane can run under a policy nobody chose for it.

- So a short list of shell-private variables is put back to what a freshly launched program would see, read from the machine at startup, and everything else is passed through untouched. Among the options it was decided that a narrow list is the only one that holds up: replacing the whole environment would discard the user's own exports, which is the one thing inheriting exists for, and editing the polluted value in place - dropping the entries that belong to the other shell - would depend on where that shell happens to be installed.

- This is not a defect in SilkTerm, and the fix is not a workaround for one. The same thing happens to a command prompt launched from PowerShell 7 with nothing of ours involved. But a pane should start the way it would from the desktop, and the terminal is the only place that can decide that once for every shell it opens.

- The list is not split by platform. PowerShell runs on Linux and macOS too and mutates the same variable there, so two installs side by side collide the same way; and the launching shell's `cd -` target is stale on every platform, since a pane opens somewhere else. What is deliberately left out is the class people reach for first - an activated virtualenv or conda environment - which a user wants a pane to keep, and which could not be removed honestly in any case, because the matching PATH edits would stay behind and leave the pane half-activated.

- Unix constrains the list in a way Windows does not. There is no call that says what a freshly launched program would see - that answer is composed by PAM, the session manager and the login shell between them and is never recorded - so the unix arm can only DROP a variable, never restore it. A name may therefore join the list only if a desktop session never sets it. That holds for all three today, and it is the rule to check before adding a fourth.

### A prompt is offered to bash, never installed (2026-08-30)

- SilkTerm includes x9ps1-git, a git-aware bash prompt, and hands it to the bash panes it starts. It shows the branch, whether the tree is clean, and how far ahead or behind its tracking branch it is. It was on by default until 2026-09-16, and is off now.

- Among the ways to deliver it, it was decided to set `PROMPT_COMMAND` in the pane's environment. bash picks that up as a shell variable, and the user's own rc files run afterwards. Nothing is written into anyone's `.bashrc`, there is nothing to uninstall, and it cannot follow the user into a shell SilkTerm did not start.

- It was meant to give way to a prompt set in `.bashrc`, and never did. `PROMPT_COMMAND` sets `PS1` before every prompt, so it replaces one from the rc files. Only a `PROMPT_COMMAND` of the user's own wins.
	- Yielding to any `PS1` was decided against. Debian's `/etc/bash.bashrc` and its default `.bashrc` both set one, so the prompt would never show there.
	- Instead, when it is on it always wins, and it is off by default. The Settings row names it and says where it comes from, so turning it on is a choice made knowing it replaces the rc's prompt.

- The alternative considered was the PowerShell approach: append a block to the rc file. That was rejected here because the PowerShell case has no other option - PowerShell cannot report its directory any other way - while bash has one that touches nothing. A prompt is also a matter of taste in a way a directory report is not, so the reversible answer wins.

- The script is written beside the config the first time a bash pane opens, and rewritten whenever it differs from the compiled-in copy, so an updated SilkTerm carries an updated prompt. The pane runs it through `$BASH`, which is bash's own path - no dependency on `PATH` and no execute bit needed.

- x9ps1-git is a separate MIT project of the same author. The in-repo copy is a vendored copy of its `bin/x9ps1-git`, and will go stale on its own if nobody looks - the version it carries is in its own header.

- PowerShell gets the same prompt, ported rather than shared, and delivered the other way. See below for why the two halves cannot use one mechanism.

### Opening files from Explorer on Windows (2026-09-30)

- SilkTerm opens `.bat`, `.cmd`, `.ps1` and `.vbs` files, and folders, through per-user file associations. It is not Windows' default terminal.

- Being the default terminal is ruled out for good. Windows starts the console program first and then hands its live session to the terminal through COM, and only Windows Terminal's console host can do the handing. So it needs Windows Terminal installed, a COM server inside SilkTerm, and a second way into the pty backend. Associations cover what a person actually double-clicks, for a small part of the work.

- Registering overrides only the `open` verb of each type's program ID, its label and its command, in `HKCU\Software\Classes`. Windows reads HKCU over HKLM a value at a time, so the type keeps its icon, its other verbs and its description. It also adds a `SilkTerm.<ext>` entry under Open with, which is the only way in when the user picked an app for the type there.

- What each value held before is saved under `HKCU\Software\SilkTerm\Associations`, with the value we wrote. Putting back restores only a value still holding ours, so a change somebody made since is left alone. Registering again puts back and saves over, so the path follows the build.

- The command is `silkterm --keep-open --open "%1" %*`. `--open` picks the host from the file type and takes the rest of the line as the file's arguments. A batch file is started as itself, since CreateProcess hands it to `cmd.exe` quoted the way cmd wants. A `.ps1` uses the same execution-policy step as Windows' own "Run with PowerShell".

- A dogfood build registers the launcher's link beside its versions folder rather than the versioned copy, which gets renamed.

- The Settings rows act at once and hold no config value, since the registry is the state. They exist only in the Windows build.

### One tip system, four places that draw it (2026-08-30)

- Flyover help comes up in four places: a Settings row, a link in the About box, a tab in the strip, and a menu item. Two renderers and two fonts are involved, so the drawing was never going to be shared.

- What is shared is everything else, and it lives in `tip.rs`: how long the pointer rests before a tip appears, how the text is broken to fit a width, and where the box goes relative to what it describes. A tip that answered faster in one place than another would read as a different kind of thing, which is the reason the delay in particular is one number.

- There are two placement rules, not one, and which applies is a property of what is being described. A Settings row's tip goes under the control, flipping above it when there is no room - a footer button's tip clamped into the bottom edge would sit on the buttons it explains. A menu row's tip goes beside the popup instead, because a box under the row would cover the rows the reader is choosing between.

- The tab's tip goes away after 30 seconds, and comes back only once the pointer has left the tab and returned (2026-09-22). The pointer is often left on the strip after picking a tab, and the tip then covered the top of the pane for as long as it sat there. `window.tab_tip_max_s` sets the time, and 0 keeps it up. The other tips close when the pointer moves on, which it does almost at once in a menu or a dialog.

- A menu row gets a tip only when its label does not already say what it does. Copy and New tab explain themselves; Paste Selection, Read-only and Bare window do not. A tip on every row is noise a reader learns to skip past, which costs the ones that matter.

- A tip's box is its own color, derived from the chrome it hangs off rather than taken from it (2026-09-20). The shipped menu background is the inactive tab's own bytes, so a tab tip filled with it drew as a tab that had grown downward, and a menu row's tip had the same fault against the popup beside it. Both now fill with the menu background lifted by the step the strip puts between an inactive and an active tab, then warmed, since every tab color leans faintly blue. The border and the text come off the same two colors. The dialogs already did this with the button shade against their panel.
	- Not two more colors on the Themes tab, which is what the request suggested. Twelve colors are editable and each has one job, and the chrome already answers this kind of question with derived shades - menu hover, border and separator are all shades of the menu color, so a custom menu color stays coherent. A tip that could be set apart from its own menu would be one more pair to keep readable.
	- The benchmark banner draws in the same overlay pass and keeps the menu colors. It is a modal notice over a dimmed window, not flyover help.

### Render Loop Sketch

- Frame: advance lerp -> cross-boundary check -> sync crate offset -> translate render -> draw cells (+overscan rows).

- Need: glyph atlas (rasterize font once, cache cells), cell metrics (width/height in px), vsync via wgpu surface.

### Speed

Output reaches the window as one notice at a time, drawing never blocks reading, and a frame whose text did not change does no text work. Throughput is measured by a benchmark anyone can run, and published in the README. Full design: [Speed](design_docs/20260930-150643_speed.md).

### The About box says how long the session has been up (2026-09-20)

- The reading is taken as the box opens rather than ticking while it is on screen. A dialog redrawing once a second to move a number nobody is watching costs a wake a second for as long as it is up, and the answer is only interesting to the nearest minute. Close it and open it again for a fresh one.

- The clock runs from a mark set at the top of `main`, not from the first time anything asks. Starting it on first read would have reported how long the About box had been reachable.

### A character handed to the window is typing (2026-09-09)

- Windows lets a program give a window a character outright instead of pressing a key for it. The touch keyboard does this for characters the layout has no key for, and so do text expanders and some accessibility tools. It arrives as a key the layout cannot name, carrying only the text it stands for.

- Every reader of a key event looks at which key it was, so an unnamed one reached nothing at all - typed text, hotkeys, menu accelerators, the Settings dialog's fields and the tab rename box alike. The key is filled in from the text once, where the event arrives, rather than in each of those places.

- Nothing else produces an unnamed key that carries text, so there is no second meaning to weigh. A key the layout did name is left alone: its text is a representation of the key rather than typing, and Enter carrying a carriage return is the case that would go wrong.

- Modifiers still apply, so an injected `c` while Control is held sends the control code, the same as typing it would. Windows composes the character without consulting the layout or the modifiers, so an argument exists for ignoring them here - but a character behaving differently from the same character typed is the worse surprise, and nobody has reported the other way round.

### What untrusted input may not do (2026-09-09)

Almost everything a terminal handles came from somewhere else. Bytes arriving from a pane's program may have traveled a long way first - a log file, a build server, a remote host over ssh - and the program printing them need not be the one that wrote them. So the rule is that nothing a pane shows may change what the terminal does, and these surfaces get held to it explicitly.

- The terminal must not type on a program's behalf. A few sequences ask the terminal a question, and the answer goes back down the pty where the shell reads it as if it had been typed. An answer that could carry a line ending would submit itself, and one that could carry the program's own text would let the program choose the command. So every reply is a fixed shape built from a number, and the terminal answers no question whose answer would be somebody else's text. There is no way to read back a window title, and a request to read the clipboard is ignored rather than answered.

	- Pinned by `a_program_cannot_make_the_terminal_type` in `term.rs`, which drives a real terminal through generated escape sequences and then asks it every question it knows how to ask.

- A link must carry a scheme from the list, or it is not a link. That is what stops `javascript:` and its relatives from reaching the desktop's handler, and it is why detection is by scheme rather than by shape. The URL then goes to the platform's own opener as one plain argument, never through a shell.

	- Pinned by `a_hostile_scheme_never_becomes_a_link` and `a_url_reaches_the_opener_exactly_as_it_was_printed` in `links.rs`.

- A paste must not be able to step outside its brackets. When the application has asked for bracketed paste, an escape character in the payload would end the bracket early and everything after it would arrive as keystrokes - which is how a paste runs a command nobody typed. Escapes are removed. Unbracketed, the application cannot tell a pasted byte from a typed one, so a line break has to be the single carriage return the Enter key sends.

	- Pinned by `a_bracketed_paste_cannot_be_closed_from_inside` and the fuzz target beside it in `pane.rs`.

- A window title is text and nothing else. The desktop draws it on a task bar, in a window list and in an alt-tab switcher, all of which treat it as plain text. Control characters are removed where the title arrives, once, rather than at each of the places it is later shown. A right-to-left override survives, which does let a title read back to front - that is the same reordering any file name can ask for, and refusing it would also refuse the joiners that hold an emoji together.

	- Pinned by `a_title_arrives_as_plain_text` in `tabtitle.rs`.

- A reported directory is not automatically a directory. A shell says where it is with an escape sequence, and the answer both names the tab and decides where the next pane starts. A payload carrying a control character is refused, since it would reach the tab strip and the title. Whether the path is absolute is asked separately, at the one gate on what becomes a working directory - a relative path resolves against wherever the terminal itself was started, somewhere nobody can see, and a posix path on Windows names a real directory in the wrong filesystem.

	- Pinned by `a_reported_directory_carrying_a_control_character_is_refused` in `cwd.rs` and `a_pane_only_ever_starts_in_an_absolute_directory` in `term.rs`.

- A program may set the clipboard, but only from the pane in use (2026-09-15). This is the one place output changes something outside the pane, and it is allowed because tmux, editors over ssh and muffer's auto-copy all copy this way. A pane in the background or in another tab is ignored, so a log scrolling past in one cannot swap out what the next paste holds. A store over a megabyte is dropped whole rather than clipped. Reading the clipboard is still refused, and text a program put there goes through the paste rule above on its way back.

	- Pinned by `a_program_can_set_the_clipboard_but_never_read_it` in `term.rs`.

### The fuzzer (2026-09-09)

Ten targets, each sitting beside the code it hammers in a `mod fuzz` inside that module's tests. They are ordinary tests, so a plain `cargo test` runs every one at a fraction of a second, and the pipeline sets a budget per target for a real soak. The engine is `source/src/fuzz.rs`; the corpus and its notes are under `cicd/tests/fuzz-corpus/`.

- Generated by grammar, not guided by coverage. Every surface here is grammar-shaped - escape sequences, config lines, URLs, image chunk headers - and a generator that knows the grammar reaches deep states in a few hundred cases where bit flips need millions. Coverage-guided fuzzing would also mean a nightly toolchain and sanitizers, so it would run on one platform out of four and not in the ordinary test run at all.

- Bit flips run as well, over a small corpus of realistic inputs. A well-formed generator never emits a truncated multi-byte character or a chunk header that lies about its own length, and those are where a parser's edges are.

- One seed is the whole reproduction recipe. Every case is a pure function of a u64, and a failure reports the seed to re-run it with. Seeds run in order from zero, so a longer budget only ever adds cases - nothing a short run covered is dropped by a long one.

- Library code is in scope, which is most of the point. The escape-sequence target drives a real terminal from the engine crate, so it fuzzes the parser and the grid model rather than only our own code around them; the config target goes through the config-format crate the same way.

- What a target asserts is a property, not an expected output. A parser that returns nothing is a fine answer; a parser that returns a span running past the end of the row it was given is not. That is what makes a random case worth generating at all.

Three defects came out of building it, all fixed with it: a program could put control characters into the window title, a reported directory could too, and a reported directory that was relative would have started a pane somewhere nobody chose. Each also has a plain unit test, which is the cheaper place to keep it.

### Environment

- Target: Debian. The primary dev/reference environment is X11 (Compiz), but one Linux binary runs native on both X11 and Wayland. winit selects the backend at runtime, and X11/Wayland/GL are all loaded on demand. Windows and macOS are targets too, all with x86_64 and ARM64 variants.

	- Per-pixel transparency takes a different path on each platform: a hand-made glutin GL context on X11, the plain surface on Wayland, and DX12 composition on Windows. See the [Wallpaper](design_docs/20260930-150052_wallpaper.md) design doc.

	- Wayland coverage: smooth scrolling is identical on both engines. The scroll regression harness runs its scenes a second time under a headless `cage` kiosk (`run.bash --wayland`). Per-pixel transparency and dialog stacking on Wayland are not yet exercised (follow-ups).

- Pixel-precise input: touchpad gives true pixel deltas; notched mouse wheel snaps to lines (clamp/accumulate notch deltas into smooth target).

### Startup and slow external resources

- Nothing on the path from launch to the first frame may read an external resource that isn't needed to draw that frame. The wallpaper is the whole of that category today, and all of it runs on a worker thread. See the [Wallpaper](design_docs/20260930-150052_wallpaper.md) design doc.

- The config file itself is a deliberate exception. Window size, font metrics and theme all come from it, and the window is held hidden until it can open at its final size. Reading it later would only trade a small local read for a visible resize flash.

- The same shape is intended for shell discovery when that arrives: draw first, scan for installed shells afterwards, fold in what was found.

### Releasing resources

A window nobody is looking at draws no frames, and a minimized window or hidden tab freezes its rendering but never its reading. Optionally, an unused window gives its GPU device back and takes it again on any sign of life. After a return from a text console, the whole device is rebuilt. Full design: [Releasing resources](design_docs/20260930-151334_releasing-resources.md).

### Configuration format

- The user config uses SHCL (the sister project), replacing TOML. The file is `config.shcl`. The reference parser is a single zero-dependency crate, so dropping `toml`, `toml_edit` and `serde` made the shipped binary smaller rather than larger.

- The deciding property is forgiveness. A malformed line yields a diagnostic and is skipped, so one bad value costs only its own setting. Strict TOML could instead fail the whole document and sink every setting to its default. Forgiveness let two workarounds be deleted outright: a retry loop that blanked offending lines and reparsed, and a rewrite pass for leading-dot floats, which are valid here.

- Values are typed by the reader, not the file, so there is nothing to get wrong in the syntax and a value is stored back exactly as written.

- shcl 3.0 reads a backslash outside double quotes as itself, where 2.x read it as an escape, and 2.x wrote Windows paths that way (2026-09-24). A file whose footer has no `Format` line is taken as 2.x, and the launch has shcl rewrite it to read the same before any other step parses it. The new footer carries that line, so it happens once. A footer somebody rewrote keeps their wording, and shcl's own stamp goes under it.

- With `remember_size` on, the size written down is an ordinary window's. A fullscreen or maximized window is not a size to come back to, so neither is remembered: unfullscreening would otherwise leave every later launch opening at the size of the screen. The window's own columns and rows stay as they were, and a resize by hand still replaces them.

- Maximized is remembered on its own, under `remember_maximized`, which is off by default. A window closed maximized opens maximized, with the remembered size still under it, so un-maximizing goes back to that size. A size or fullscreen asked for on the command line wins over it. The state is read once a resize has held, not in the resize event, since the window manager may set it after the resize it caused.

- A number given on the command line is held to the range of the setting it stands for, the same range the file's copy of that setting is held to. A count of rows or columns is also held to what the graphics device can draw, since the window is a texture and a refusal there ends the launch rather than the setting.

### Variables in a setting (2026-08-30)

- A setting that names a path or a program is text SilkTerm reads. No shell ever sees it, so nothing else would expand a variable written there.

- Three spellings are read, all of them on every platform: `$NAME` and `${NAME}` from bash, `%NAME%` from cmd, `$env:NAME` and `${env:NAME}` from PowerShell. A leading `~` works too. Which shell somebody prefers should not decide whether their config file works, and a file carried between machines keeps working.

- A few names that mean one thing under two spellings are paired: HOME with USERPROFILE, USER with USERNAME, TMPDIR with TEMP and TMP. Only names with a real counterpart are listed, since a guess would be worse than an empty expansion that can be seen.

- An unset name expands to nothing, the way a shell does it.

- A command is the exception. It is split into arguments first, then only the program name is expanded; every argument after it goes through exactly as written.
	- The program name is expanded because nothing else would. SilkTerm starts it directly, so there is no shell in between to read a `%ProgramFiles%`. Splitting first also keeps a variable holding `C:\Program Files\...` as one argument.
	- The arguments are not expanded because the program itself reads them, and they already mean something to it. `cmd /k prompt $P$G` sets a cmd prompt, `bash -c 'echo $FOO'` wants bash's own `$FOO`, and a `%` in a unix path is a percent sign. Substituting any of those hands the program a word nobody typed, with no way to write a literal.
	- Every word was expanded until 20260916, which is what broke those three. Reading only the local platform's spelling was tried first and dropped the same day: it fixed the cross-platform collisions, left the local ones, and cost a carried config for nothing.

- `shell.startup_directory` defaults to the home variable in the platform's own spelling: `%USERPROFILE%` on Windows, `$HOME` elsewhere. Both are read on both platforms, so a file carried to another machine still finds home. It was `~` for part of 20260916.

### Command-line options

- Most options describe a window to open: a hierarchy of tabs and panes built with create/select verbs, with look and behavior cascading window -> tab -> pane.

- A second, much smaller family only prints something and exits. `--help`, `--syntax`, `--about`, `--donate` and `--version` never open a window, never read a config, and never touch a layout.

- Those flags are accepted in any position. The rest of the grammar cares a great deal about order, but answering a request for the help with a complaint about where the flag was written would be absurd.

- Output written for a person is padded with a blank line above and below, so it sits clear of the shell prompts either side of it. `--version` is the exception, and it exists to be captured by a script, so it stays a single flush line. `--ver` and `-v` are the same flag.

- Every build carries a build number, because a version cannot identify one. Two dogfood builds of the same release share a version, so a report of "it still does this on beta3" names something that could be any of a dozen binaries. The number is whole minutes elapsed since 2000 began, written in Crockford base 32 in lowercase: five characters until 2063, it sorts in the order the builds were made, and it decodes back to the minute one was built. Crockford's alphabet leaves out i, l, o and u, so nothing read off a screen and typed into a bug report can come back as a different character.
	- It appears in `--version`, in `--about`, in Help > About, and in the release notes.
	- The pipeline pins one number for a whole run, so the four cross builds of a release report the same build rather than four made minutes apart. Without that the release notes could not name one.
	- Unchanged sources keep the number they had. The binary did not change, so neither should what it calls itself.
	- The release notes take the number out of the artifact being published rather than computing it again, so the notes cannot name a build nobody can download.

- Where a shell starts is decided by four things, most deliberate first: `--directory` on the command line, then the directory inherited from the pane a new tab/pane/window came from, then the directory SilkTerm itself was launched from (only when a shell launched it), then the `shell.startup_directory` setting. `--directory` cascades window -> tab -> pane exactly as `--shell` does, so the flag that names a shell and the flag that says where it starts behave alike.

- `--about` reports what a bug report needs: version, build number, which of the cross builds this is, and the GPU the renderer picked. It asks the graphics stack for an adapter but never builds a device, which is the expensive half. A box with no usable adapter loses three lines and still prints the rest.

- On Windows a release build owns no console of its own, so printing has to join the one that launched it. This happens only on the paths that print and exit; a terminal window that held a console would die with the shell that started it.

- The contract on saving is that a user's comments and blank-line grouping survive. Layout may be tidied, meaning indentation and quotes that are not needed, but a value is never rewritten. The shipped template is deliberately spelled the way a save would spell it, so the first save is a no-op rather than a reflow of the file we just wrote.
	- So a save must never change what the next launch loads. The renames and refreshes a launch applies before it reads the file decide only on what a save keeps: a value as it reads, in whichever quotes; a setting's place in its blocks as the file format reads it, with comment lines left out; and a commented line where a save would put it. Before this, a single-quoted old default font list was left alone until the first save put double quotes around it, and a setting under a commented-out heading was renamed only once a save had moved the heading.
	- Rejected: a save that writes every untouched line back as it was typed. That is a second writer beside the format's own, with placement rules of its own to get right. Also rejected: refusing a save that would move a value, since the person saving can neither see why nor fix it.
	- Adding missing settings at launch keeps the same promise (2026-09-18). A setting typed deeper than its block needs still loads, until an active line is added above it and takes it for a child. Such a line is moved out to its block's own depth, which a save would do anyway. Where that is not enough, or a line would become unreadable, nothing is added and the launch says why.

- A save writes through a temp file and a rename, never in place, so a crash mid-save cannot leave a truncated config. If Windows takes the old file off its name and then cannot put the new one there, the new settings are written at the name directly, rather than leaving no file. If loading had to drop a line it could not place, the save is refused rather than quietly deleting it. One changed setting is not worth a line someone wrote.
	- A refused save is said on screen, since stderr reaches nobody on Windows and nobody who started SilkTerm from a menu (2026-09-18). Windows shows its own message box. Elsewhere a small window drawn like About stands in for one, since Linux has no message box every desktop carries. It names the file and the lines, and says changes are used now but not kept.
	- Saves nobody asked for, such as a resize, a menu switch or shells found at launch, are said once a session for each file, or every resize would raise it again. An OK or Apply in Settings is answered every time, and OK then closes, since trying again cannot help. A file open in another program still keeps Settings open, because trying again can.
	- Editing only the lines a save changes, the way the rating does, would let most saves through. That was held for shcl 3.0, which did not change it: a line it cannot place still counts as lost, so the save is still refused (2026-09-24).
	- Saves go through shcl's line-keeping save since 2026-09-25, so a line nobody changed keeps its quotes and indent. shcl still writes the whole file where it cannot keep the lines, and it always does for a file with a dropped line, so the refusal stays.

- Settings opens on the file as it is now, not on what the window loaded (2026-09-22). Several windows share one file, and one that saved after this window loaded would otherwise not show here. The file then gets only what was edited in the dialog, and the window takes everything that differs from what it runs, so OK is also when another window's change arrives.
	- The file is read when the dialog opens, not watched. Opening Settings changes nothing on screen, and Cancel leaves the window as it was.
	- The session's own choices are folded in first, the way Reload config folds them: the command line's font and colors, a wallpaper named for the session, the Remote profile.

- The template's sections follow the Settings dialog's tabs, in the same order, so a person who has learned one has learned the other. That order reaches a new file only. An existing config keeps whatever order it has, since the machinery that adds new settings places them but never moves what is already there.

- The file is organized as nested blocks, tab-indented, mirroring how the settings relate: `wallpaper` holds its children, with `rotate` and `contrast_mask` nested inside it. A setting can also be written as a single dotted line (`wallpaper.opacity: 0.1`) and reads identically. The block form is just the canonical spelling.

	- Each setting stands in its own blank-line-delimited section, comments directly above it: a short title, a description, and a range line where one applies. A commented-out line shows the built-in default and carries a `## Default` marker.

	- The in-place add/refresh passes stay line-oriented. They resolve each line's full path from the indentation around it, so a new setting is inserted inside the right block, beside its siblings.

- No migration from the old TOML configs: a fresh file is generated with defaults.

- When the flat naming gave way to nested blocks, an old config converts wholesale rather than being rewritten in place. The old file is kept alongside as a backup, a fresh current-format file is written, and every value the user had set carries over to its new place. Rewriting flat lines into blocks would have shredded the old file's comments. This way settings survive and the file's documentation is current.

- A file an earlier build converted with its image written onto the `wallpaper:` heading is repaired where it is at launch. The value moves to `image:`, unless the file already names an image there, which is the later choice and is kept while the heading's value is dropped. No other line changes, but the file is written back with LF line endings, as a conversion writes it. It is not converted again, because that needs a backup name, and that build had used most of them converting the same file at every launch. Values that build dropped are not read back from the backups, since a backup is older than anything saved after it.

- A config carries the commented default lines it was first given, so when a default changes those lines start describing the old behavior. Such a line is refreshed to the current default. The file may be corrected about what the program does on its own, but never about a value that was set by hand. A line the user activated, or annotated, is therefore left alone.

- Starting over is a rename rather than a delete. `--reset-config` moves the file aside and lets the next launch write a fresh one, so the previous settings stay recoverable.

- Some defaults are better inferred from the config directory than stated in the file. A folder of wallpapers in the expected place is taken as wanting them rotated, and nothing is written back. That, the folder's default, what shows when nothing is named, command-line wallpapers and the XMP layout and look tags are in the [Wallpaper](design_docs/20260930-150052_wallpaper.md) design doc.

## Delivery (CI/CD, branches, releases)

Guiding constraint: GitHub is dumb git hosting plus optional release storage, nothing more. No hosted CI, no Actions, as few third-party tools as possible; the whole pipeline runs locally (`cicd/cicd.bash`).

- Merge gate: `cicd.bash --gate` (fmt check, clippy with warnings as errors, tests) runs as the `pre-push` hook for pushes to main. This is the local stand-in for a hosted CI workflow. Pushes to dev and feature branches are not gated, since a branch is tested before it merges into dev. The gate reads the commit being pushed, from a throwaway worktree checked out at that commit, rather than the working tree: the two are routinely different here, and a fix still sitting uncommitted would otherwise carry the push. Cargo writes to the usual output directory, so only this crate is rebuilt there, not its dependencies.

- Version-bump guard: the same `pre-push` hook blocks a push to main unless its `source/Cargo.toml` version is a strict increase over the version already on main, by full semver precedence including prerelease ordering. So a release merge can't ship the same-or-lower version. It also requires the README Release badge to match that version, the same check `release.bash` makes, just earlier. It skips on the first main push and on branch deletes, and is overridable with `--no-verify` / `SKIP_GATE=1`.

- Branch flow: feature branches merge `--no-ff` into `dev` (the integration target). `main` is release-only: merging dev into main cuts a release.

- Releases: `cicd/utility/release.bash` tags the merge `v<version>` and can push the tag and attach the artifacts to a GitHub Release as plain uploads. The version comes from `source/Cargo.toml` alone. The tag is read from it and the build stamps from it, so they can never disagree. Version and README badge get bumped on dev before the release merge; nothing is ever committed directly on main.

- Build matrix (buildable from this Linux x86_64 box): Linux x86_64 (native) and, via `cargo-zigbuild` + `zig`, Linux ARM64, Windows x86_64 (mingw), Windows ARM64. macOS and BSD are deferred, since cross-building them needs an Apple SDK (osxcross, license-gated) or a FreeBSD sysroot, neither present here. The debug build is what the tests and profiler run against, and the optimized release builds feed packaging and dogfooding. ARM64 targets are on by default, since zig cross-builds aren't emulated and so are not much slower, and they drop out with `--no-arm`.

- Windows x86_64 toolchain for releases: the gnu (mingw) build is the canonical shipped Windows x86_64 binary, and the msvc build is deliberately left out of the Linux-cut release. The gnu build cross-builds from this Linux box in the same run as everything else, and is self-contained. msvc can only be built on Windows, since it needs `link.exe`, so folding it in would mean a Windows->Linux binary hand-off. Since the msvc build was made crt-static it no longer offers end users anything gnu doesn't. Its remaining edges, PDB/WinDbg debugging and a standard ABI, are dev-side only. `cicd-win.ps1` still builds msvc on Windows for local dogfooding and debugging. Two things would reopen this: Authenticode code-signing, whose natural home is Windows and would pull Windows-installer finalization onto that box; or evidence that mingw binaries trip antivirus reputation heuristics enough to matter. `makensis` itself is host-agnostic, so building the Windows installers on Linux is a non-issue independent of this choice.

- Packaging (pipeline stage 6, when `--quick` is not passed): built from the stage-5 release binaries, never rebuilt. Linux -> `.deb` (cargo-deb) and `.rpm` (cargo-generate-rpm) per arch, driven by `[package.metadata.deb]` / `[package.metadata.generate-rpm]` in `source/Cargo.toml`. Windows -> one self-contained NSIS installer `.exe` per arch (`cicd/packaging/windows/installer.nsi.in` + `makensis`). It upgrades an existing install in place by running the old uninstaller first, and needs no bundled runtime because the binary links only system DLLs. RPM versions can't contain `-`, so `1.0.0-beta1` is emitted as `1.0.0~beta1`. AppImage/Flatpak and the deferred macOS `.dmg` / BSD packages are future work.

- Dogfood delivery: every build the pipeline makes is installed under a fixed name in a synced app dir for the platform it targets (`.../exec/app/{linux,mswin,macos}/`), with the icon and a sidecar naming the build beside it. That is the only place a launcher looks. It replaces an older arrangement where each box also read the build host's clone over the network, which meant a probe, three bounded waits and a whole class of "why did the shortcut hang" that no longer exists. The cost is that a build reaches another box at Dropbox's pace rather than immediately, which for dogfooding is not a cost.

- One launcher, three platforms: `utility/n8runterm.ps1` is the whole implementation, and `runterm` (bash, for Linux and macOS) and `runterm.cmd` do nothing but find it and pass the arguments on. The two previous launchers were the same program written twice, and they had drifted apart in ways that only showed when one box behaved differently from the other. PowerShell 7 is now a requirement for launching a dogfood build on any platform, which is the price of having one implementation to keep right.

- Where copies live: a versions folder beside a `silkterm` symlink pointing at the newest. Anything that names the program - a name on PATH, a menu entry, an icon - points at the symlink and so never needs rewriting when a build arrives. The folder is GFS-rotated on every launch: newest and oldest always, then the last couple, then the newest of each ended day, week, month and year, capped per tier so a box that builds daily still keeps something from months back. At most ten copies, at least five, and it stops at 1 GB in between. A copy that is running is never deleted, so a window open on an old build keeps its binary.

- A menu entry runs the launcher, never the terminal. A shortcut naming a build pins that build forever, which is how the entry went stale before. The launcher writes and refreshes the entry itself, from the paths it already knows, so it cannot drift from where things actually are. Its icon comes from beside the symlink - a `.png` on Linux, the symlinked binary's own icon on Windows.

- Artifact naming (stable; download links depend on it): `<exe>-<version>-<os-arch>[.exe]` for binaries, `<exe>-<version>-<os-arch>.{deb,rpm}` and `<exe>-<version>-<os-arch>-setup.exe` for packages, plus `<exe>-<version>-sha256sums.txt` (covers binaries and packages), all collected into `cicd/artifacts/release/`.

- Pinning: `rust-toolchain.toml` pins rustc/clippy/rustfmt and the cross targets. The cargo-installed helpers (cargo-deny, cargo-zigbuild, cargo-deb, cargo-generate-rpm) and makensis are pinned in `cicd/tool-pins.txt`, which both pipelines read, with a non-gating drift warning. Dependency freshness is a periodic local `cargo update` pass, and cargo-deny advisories flag anything urgent in every run.

- Reproducible builds (2026-09-24): one commit built in two folders gives the same bytes for all four published targets. The path map keeps the folder out, the build number comes from the commit's time on a clean tree, and the Windows linkers are told not to write the link time into the PE header: `--no-insert-timestamp` for mingw, `-Brepro` for zig, which refuses the other. `cicd/utility/repro-check.bash` is the check. The msvc build that `cicd-win.ps1` makes for Windows boxes is not published and still carries its link time.

- README badges: static shields only (release, license, minimum Rust). No CI badge, since there is no hosted workflow to point one at.

- Wallpaper gallery on GitHub Pages: `docs/` is served from main, and holds one self-contained page - a thumbnail grid whose tiles open the wallpaper full size in place, with prev/next paging, a filter box and per-image provenance. A README cannot do this: GitHub renders no scripting and strips image maps, so a single contact sheet has no clickable tiles and there is no way to page through anything. It stretches the guiding constraint above and does so knowingly - Pages here is branch-served static files with no Actions workflow, GitHub builds nothing, and if it were switched off tomorrow the only casualty would be one README link. The page carries thumbnails only (about 1.4 MiB) and fetches each full image from the pack already in the repository, so the 60 MiB of wallpapers is never stored twice. Both it and the README contact sheet come out of `cicd/utility/wallpaper-gallery.bash`, which is deliberately one entry point: two rendered artifacts from one pack go stale together or not at all.

### A tab can be named by hand, and the window title follows the tab (2026-08-30)

Double-clicking a tab renames it in place. The strip has always drawn what the shell is doing, which is right most of the time and wrong when several tabs are running the same thing in the same tree.

- The edit starts with what the tab already says, all of it selected, so typing replaces it and any other key edits it. Enter or Tab keeps the change, Escape drops it, and a click anywhere else keeps it. Selection, Home and End, and paste all work; a pasted newline becomes a space, since a tab is one line high.
	- 20261002: The box edits as a Settings text box does, with the same keys on each platform: copy, cut, paste and select all, moving and erasing by words, and Shift to select. A click places the caret, a drag selects, a double-click takes a word and a third click the whole name. Right-click opens Cut, Copy, Paste, Delete and Select all, and a middle-click pastes the primary selection.
	- 20261002: Opening a menu leaves the rename up. Copy, Paste and Paste Selection there act on the name, as Edit > Copy and Paste on the macOS menu bar do. Any other pick keeps the change first, as a click elsewhere does.

- Committing a blank title, or one that matches what the tab would have said on its own, puts it back to naming the shell. Those are the two ways out of a hand-typed title, and neither needs a control of its own.

- Titles need not be unique. Two tabs called the same thing is a thing people do on purpose.

- The rename is keyed by the tab's position, so opening or closing a tab ends it - committed on an open, dropped on a close.

The window title is now assembled in one place, and always starts with the application name. A dogfood build says which one it is, since the pool holds several and they are otherwise indistinguishable in the taskbar.

- After the name comes, in order: a title typed on the tab, else the title the running program asked for, else what the tab says about the shell. So a program that renames the window (an editor, a build tool) reaches the title bar without touching the tab, and a hand-typed tab title outranks it.

- A title that is only the name or path of a program is passed over, so the next source down answers - normally the tab's own label. A Windows console names a new window after the program it starts, so a shell that sets no title of its own arrives carrying its own image path. Measured for cmd and for pwsh on two machines.

- A console that names the program and then the command it is running keeps the command, since that half says something. The name has to be a full path for that to apply, which is what a console writes; a bare name on the left would eat the file in vim's "build.bat - VIM".

- The extensions that count are `.exe`, `.bat` and `.cmd`. Not `.com`: far more titles end in a hostname or a directory than in one of the three DOS-era programs that still use it.

- A tab carrying a blank title lets the program's title through, and with neither the title is just the application name - blank means "defer", not "show nothing". A blank typed into the rename box goes back to automatic naming instead, so a blank tab title comes from an empty `--title` on the command line.

- A `--title` given on the command line is the whole answer, verbatim. It is an explicit request for exactly that string. The one thing it cannot drop is the rights below.

Which of the three parts a tab names is now the user's to choose, and so is whether the window title falls back to the tab at all (2026-09-20). Four switches on the Window tab, all on as shipped.

- A part that is switched off is dropped before the ladder of shortenings is built, not cut out of the finished label. A tab with the directory off therefore has no path rung to give up, and the rungs it does have are measured against what will really be drawn.

- Switching all three off leaves the tab naming its shell. A tab with no text cannot be told from the one beside it, so there is a floor under this and the shell's name is it.

- The tab's flyover still names all three whatever the switches say. That is the point of turning one off: the text is out of the strip, not out of reach.

- Turning the window title's switch off drops both of the tab's answers, the name typed on it and the text it works out for itself, since both are the tab talking. A title the running program asked for still comes through, and a `--title` is untouched.

A title the running program asks for can name the tab too (2026-09-20), which is a fifth switch beside the four above, also on as shipped.

- The order is the window title's, one step down: a name typed on the tab wins, then the program's title, then the text the tab works out for itself. The two were already a pair - the window title falls back to the tab - so having them disagree about what a program said would only have been confusing.

- It reads the program's title through exactly the same filter the window title does. A Windows console's own decoration comes off it, and a title that is only the name or path of a program says nothing on either.

- The title sits above the tab's ladder of shortenings rather than inside it. A program's title has no shorter forms of its own, so folding it in would have thrown away every rung longer than whatever the program happened to say, and a narrow tab would jump straight from a full title to the shell's initials. Above the ladder, a tab too narrow for the title falls through the forms it works out for itself, and the floor is still the shell's name.

- On by default because the window title already prefers a program's title by default, and a tab that disagreed with the title bar above it would read as a bug. The switch is there for a shell that retitles on every prompt, which turns a strip of useful labels into a row of the same `user@host` text.

- The tab's flyover carries a "Program title" line when there is one, whatever the switch says, the same as the parts above.

### A terminal running with administrator or root rights says so (2026-09-09)

The window title starts with "Administrator: " on Windows and "Root: " elsewhere. Windows already spells its own elevated console title bars that way, so that word is kept rather than invented.

- No flag turns it off, `--title` included. The absence of the word has to mean something, and a marker a flag can remove means nothing.

- It goes on the window title only. Tab titles are left alone, and so are the Settings and About windows: neither is a taskbar entry of its own, and the terminal window they belong to already says it.

- The rights are the terminal process's own, read once on the way to the first window. A pane that elevates itself afterwards, `sudo -s` for instance, is not covered - the title would be claiming something about the wrong process. On unix the test is the effective user id, so a setuid binary reports the rights it actually holds rather than the account that started it.

- An elevated Windows console writes the same word in front of the first title it sends, which was measured over a pseudoconsole on two machines. That is the title naming the program the console started, so both halves say nothing: the word comes off, and the program name is then dropped by the rule above. Without that step the word hides the name and the whole path is shown, which is the original complaint with rights on top.

- A title the program sets afterwards is not decorated - measured in the same run, where a title set to "build" arrived as "build". So the word cannot arrive twice from a console. It can still be typed twice, and a title already starting with it is not given another.

- Only the exact word about to be put back is removed, and only on Windows. Nothing on unix decorates a title, so taking anything off there could only destroy somebody's own text - which means a program running as root that titles itself "Root: something" will show the word twice, and that is the right answer, because the second one is its own.

- A console speaking another language writes another word, and no list of words could cover them all. So the title is also matched against the program the pane was started with. When a Windows path to that program follows a word, the word comes off in any language. Only the file name is compared, because a console spells the folders its own way. A word that holds a path separator, or one in front of some other program, is left alone.

### Tabs report what they are running, and where (2026-08-21)

A tab used to say the application's own name on Windows and the shell's process name on unix. It now reports the shell by its FRIENDLY name - the one the Shells list carries, which is the name the user gave it - followed by what that shell is doing: the command in the foreground, or the last one it ran, or, having run nothing, the directory it is in.

- The shell a pane runs is resolved once, when the pane is spawned. Leaving it as "whatever the default shell is" let the answer change under a running pane, since the background scan fills the list seconds after launch and the Shells tab reorders it - so a pane could be labeled with a shell it was not running.

- The path is shortened by an ellipsis eating the middle a directory at a time, so what is left keeps its real names. Only when that has run out do the directories above the current one drop to their initials, which on any path deeper than a couple of levels never happens - the ellipsis has already covered more ground than a column of single letters would. Two things survive every step, because they are what distinguish a location from a command - the anchor it starts from and the separator it ends with. Windows keeps its drive letter and gets no `~`, since neither shell there prints one.
	- This reverses the original order (2026-09-20), which was PyCmd's: initials first, ellipsis only where it was shorter still. It came from nemo-anywhere, which had already been through the same argument. A middle left out reads as a place with a gap in it; a column of initials reads as neither the path nor anything else, and it gives up every name at once to save a few columns.

- Tab width is two percentages of the window rather than a fixed cap, so the extra text has room on a wide display while a lone tab still reads as a tab. See the entry below for what those two now mean.

- When the tabs stop fitting, the strip shows a page at a time rather than shrinking them to nothing. The wheel over the tab bar turns the page, and switching tabs brings the new one onto it.

- A hover tip carries what the tab cannot: the shell's name, the command line behind it, whatever is running now, the whole path, and how long the tab has been open. It reads as a table - one `key: value` per line, every value starting in the same column - which is why it is the one piece of chrome drawn in the TERMINAL font rather than the interface one: the column is made of spaces, and spaces align nothing in a proportional face. A value carrying a space or a quote is quoted, so its edges are never in doubt; the quote picked is the one the value does not already contain, the same habit the config file has, so a Windows command line reads inside single quotes instead of fighting its own double ones. What is derived rather than quoted - the clock reading, and the note that no directory was reported - stays bare, since quoting those would say they were data.

### A tab is as wide as its own label needs (2026-08-23)

Tabs used to divide the bar evenly between a minimum and a maximum percentage of the window, so every tab was the same width whether it had anything to say or not. They now size themselves.

- The first percentage is the REGULAR width: what a tab is when nothing is pushing on it. It is a target, not a share - three tabs on a wide bar sit at it and leave the rest of the bar empty, rather than a couple of them stretching across the window.

- A tab whose label wants more room grows past it, up to the maximum. A crowded bar pushes every tab back below it. Everyone reaches the regular width before anyone grows past it, so a long path can never cost another tab its ordinary size, and under crowding each tab gives up the same fraction rather than the last few being starved.

- The tab in front is the exception (2026-09-20). It takes what the row can spare before any other tab grows past its ordinary width, and the maximum does not apply to it, so it spells its label out wherever there is room. The room it leaves is no use to tabs already at the cap, and it is the one being read. The tabs behind it give way for it, each dropping to whatever rung of its own ladder still fits, and none of them goes below the regular width. With the shipped 10% regular width that can leave an inactive tab saying only its shell's name; raising the regular width is what buys them a path back. Taken from nemo-anywhere, whose tab row had the same problem.

- Defaults are 10% regular and 100% maximum. The old pair (8% and 26%) made sense when the bar was divided evenly; a maximum now only says how far one tab may grow when it has the room, which is worth allowing in full for a window holding a single tab.

- The floor a tab may not shrink past is its own shortest label - a short form of the shell's name and nothing else. Tabs past that point become a page.

- What a tab says now gives way in a fixed order, rather than only the path shortening: the shell's name shortens first, then the running command's name is truncated, then the path abbreviates, then the command goes, then the path, and what is left is the shortest form of the shell's name. The path is shown alongside the command now, where before a tab running something said only what it was running.

- Short shell names are hand-picked for the shells we ship ("Windows Cmd" reads "Cmd", "PowerShell 7" reads "PS 7") and derived for anything renamed, since nothing mechanical arrives at "Cmd" from "Windows Cmd". A derived name keeps its distribution rather than its family ("WSL2; Ubuntu" reads "Ubuntu") and marks a variant with a star, so "Zsh" and "Zsh*" at least say that one of them is not the ordinary one.

### PowerShell gets the same prompt bash does (2026-08-21, reworked 2026-08-30)

The integration block sets a prompt, but only where the prompt is still the one PowerShell comes with, identified by the help link its own definition carries. Anything anybody else installed is left alone.

It began as a prompt that named the version, because two PowerShells look alike at a prompt. It now reads the same as the bash prompt described above: version, time, user, host, path, and in a git working tree the remote, the branch, and two marks for committed and level with the upstream. A PowerShell pane and a bash pane should look like the same terminal.

Two decisions came out of the port.

- It lives in the block rather than in a script beside the config, which is where the bash prompt lives. A prompt is drawn after every command, and a script would mean a process per prompt - cheap on unix, not on Windows. The block is already kept up to date in place, so it carries updates just as well as a file would.

- The block stays plain ASCII, and the check, cross and arrow are written as code points. A file with no byte-order mark is read as ANSI by Windows PowerShell 5.1, which would mangle a literal glyph on the one version that cannot be told otherwise.

- The console is put on UTF-8 at load. That is not what lets the prompt draw its glyphs, since PowerShell writes the prompt as wide characters and the code page has no say in it. What it buys is the decoding of output from `git` itself, where a branch name outside ASCII would otherwise arrive wrong.

- The second line is bare where the bash prompt puts an arrow. The `>` already says where the typing goes, and the arrow the bash version uses is a code point few fonts carry.

- The check is U+2713 rather than the U+2714 the bash prompt uses. U+2714 has an emoji presentation, so it is drawn by a color font in its own color and ignores the reverse-video the mark is set in. U+2713 is not an emoji code point at all, so it takes the color the way the cross beside it does.

Cost was the other thing the port had to answer, since three `git` calls per prompt is invisible on unix and not on Windows. The search for the working tree is done in the shell rather than by asking git, so a directory outside a repository costs no process at all, and inside one a single `git status --porcelain=v2 --branch` answers branch, clean and upstream together. The remote URL is read once per repository and remembered.

The block is also kept up to date in place from then on, between its two markers. It gains things over time, and an install that only ever appended would leave everyone who already had it on the first version forever. That edit is safe only because the region is delimited by markers we wrote - which is exactly the signal the stored shell list lacks, and why that list may still only ever be added to.
