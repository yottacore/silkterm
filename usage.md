<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Using SilkTerm

This page explains where the config is, what a tab says, panes in tmux, the command line, shell integration, programs over ssh, and opening scripts and folders on Windows.

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Configuration](#configuration)
- [Tab text](#tab-text)
- [Panes in tmux](#panes-in-tmux)
- [Command line](#command-line)
	- [Information](#information)
	- [Window options](#window-options)
	- [Control](#control)
	- [Tabs and panes](#tabs-and-panes)
	- [Window, tab or pane](#window-tab-or-pane)
- [Shell integration](#shell-integration)
- [Programs over ssh](#programs-over-ssh)
- [Opening scripts and folders on Windows](#opening-scripts-and-folders-on-windows)

<!-- /TOC -->

## Configuration

On first run SilkTerm writes a commented config file with all defaults, in the place each platform keeps settings:

| OS      | Config                                            | Wallpaper folder
| :------ | :------------------------------------------------ | :-----------------------------------
| Linux   | `$XDG_CONFIG_HOME/silkterm/` (or `~/.config/...`) | beside the config
| Windows | `%APPDATA%\silkterm\`                             | `%LOCALAPPDATA%\silkterm\wallpaper\`
| macOS   | `~/Library/Application Support/silkterm/`         | beside the config

Setting `XDG_CONFIG_HOME` overrides the platform default everywhere, and `--config PATH` overrides everything. An alternate config keeps its own wallpaper and history beside itself rather than sharing the defaults.

On Windows the 2 are split, because settings are worth roaming between machines, but a 60 MiB wallpaper pack isn't. A pack that's already beside the config still works, and nothing has to be moved.

If making changes directly (rather than through Settings), you can apply them immediately with the "Reload config" menu item. Settings reads the file each time it opens, so it also shows what another SilkTerm window saved.

Settings names are meant to read plainly, but a few of them are particular to SilkTerm, such as scrim, contrast mask, and automask mix. [glossary.md](glossary.md) defines those.

To start over from the default settings, run `silkterm --reset-config`. The old file is kept alongside as `config.shcl.bak` rather than deleted.

When an update converts the file to a newer format, the file as it was is kept alongside, named for the local time it was converted and the format it had, such as `config_backup_20261003-142233_format-v2.shcl`. Every older version is kept, and a file named with `--config` keeps its own name in front. A line that the new format can't represent stays in the file as written, and SilkTerm puts up a notice saying how many there were and where the old file is. If the file can't be converted where it is, because it isn't UTF-8 text or the new format can't read it, SilkTerm writes a new one from the defaults with every setting it can still read, and the notice says how many it couldn't copy to the new file. A file already in the current format with lines that aren't UTF-8 text is written again without those lines, and kept alongside the same way. A notice names the lines and where the old file is.

Drop a few images into a `wallpaper` folder next to the config and SilkTerm picks one each launch, and favors whatever it hasn't shown lately. Naming a wallpaper in the config, or passing one on the command line, takes precedence. The [wallpaper pack](install.md#wallpaper-pack) is a ready-made folder to start from.

## Tab text

What each tab says comes from a template. Edit it under Tab text on the Window tab of Settings, or as `window.tab_text` in the config. A macro between percent signs is replaced by its value:

| Macro                 | Value
| :-------------------- | :----------------------------------------------------
| `%USER_DEFINED%`      | A name typed on the tab, or set with `--title`
| `%PROGRAM_DEFINED%`   | The title the running program sets
| `%SHELL%`             | The shell's name, as the Shell tab lists it
| `%PROGRAM%`           | The program the shell is running
| `%LAST_PROGRAM%`      | The program it ran last, once it's back at its prompt
| `%CURRENT_DIRECTORY%` | The shell's current directory

The rest of it:

- `a || b` shows `a`, or `b` if `a` is empty.

- `{ }` groups parts without showing the braces, and `[ ]` groups them and shows the brackets.

- A group is left out, text and all, when all of its macros are empty.

- Text between 2 parts shows only when both sides show something, so a separator doesn't hang off the end when the part after it is missing. A choice without a macro in it, such as the `-` in `{[%PROGRAM%] || -}`, counts as text here. To keep text with one macro, put the 2 in a group: `{(%SHELL%)}`.

- Spaces at either end of the template, a group, or a choice don't count, and a run of spaces shows as one. Tabs and line breaks are ignored.

- A backslash makes the next character plain text, such as `\%` or `\{`.

- Macro names aren't case-sensitive.

The default reads `Bash [vim] ~/src/` while vim runs, `Bash [last: vim] ~/src/` once it's done, and `Bash - ~/src/` before anything has run:

~~~text
%USER_DEFINED% || %PROGRAM_DEFINED% || { %SHELL% {[{ %PROGRAM% || last: %LAST_PROGRAM% }] || -} %CURRENT_DIRECTORY% }
~~~

On a narrow tab the parts give way in a set order. A program's own title goes first, then the shell's name gets shorter, the program's name is cut, the directory is shortened, the program goes, then the directory, and what's left is the shortest form of the shell's name. A name typed on the tab never shortens. A tab that doesn't have anything else to show names its shell.

A template that can't be read, such as one with a `{` and no `}`, is kept as typed, and tabs use the default until it's fixed. The row's flyover in Settings says what's wrong, and so does a message at launch.

A config from before the template had 4 check boxes for this: `window.tab_shows_title`, `tab_shows_shell`, `tab_shows_program`, and `tab_shows_directory`. The first launch of a newer SilkTerm swaps them for a template that says the same.

## Panes in tmux

SilkTerm can start a pane in a tmux session of its own, so what runs in it outlives the window. If the desktop crashes, the session is still there, and it can be attached to again from SilkTerm, another terminal, or over ssh. It needs tmux 3.2 or later, so it isn't available on Windows. Where tmux can't be found, the rows below are grayed in Settings and the menu row is left out.

- "Always run in new tmux session", under Tmux on the Program tab of Settings, starts each split in a new session. A new tab, and the window's first pane, still start the plain shell.

- "New tab in tmux" on the Tabs menu opens a tab whose pane starts in a new session, whether the switch is on or not.

- The pane runs the same shell it would have otherwise, in the directory it would have started in. A split made from a pane in tmux starts where tmux says that pane is now.

The session's name comes from a template, "Session name template" under the switch, or `shell.tmux.session_name` in the config. It works like the tab text template, with 3 macros:

| Macro        | Value
| :----------- | :-----------------------------------
| `%YYYYmmDD%` | The date
| `%HHMMSS%`   | The time
| `%HHMMSSNN%` | The time, to a hundredth of a second

The default is `%YYYYmmDD%-%HHMMSSNN%`, such as `20261017-17332704`. tmux won't keep a `.` or `:` in a name, and treats `#` as the start of one of its own formats, so all 3 become `_`. A name tmux already has gets `-2`, `-3`, and so on after it.

Closing a tab, a pane, or the window does what "When closed" says to each session SilkTerm started in it:

- Detach leaves the session running, to attach to later.

- End kills the session, and everything running in it.

- Prompt, the default, asks each time. The question has a "Don't ask me next time" box, which saves the answer given as the new setting.

End only ever kills the session SilkTerm started for that pane, found by the name it was given. If the session was renamed, or something else was switched to inside the pane, that one is left alone. A pane whose tmux exits on its own, through a detach inside it or the session ending, closes without asking.

In the config:

~~~shcl
shell:
	tmux:
		always_new_session: true
		session_name: "work-%HHMMSS%"
		on_close: detach  ## detach, end or prompt
~~~

## Command line

`silkterm --help` lists every option, and `silkterm --syntax` prints the list on its own. Order matters. Window options come first, then each tab or pane, with its own options after it.

A few examples:

~~~sh
silkterm --columns 120 --rows 40 --shell fish
silkterm --new-pane --right --size=40% --shell htop
silkterm --directory ~/src --new-tab=logs --directory /var/log --new-pane --down --shell top
~~~

The first opens a window 120 cells wide and 40 high, running fish. The second splits the first tab and puts htop on the right, in 40% of the width. The third starts the first tab in `~/src`, then opens a second tab named `logs` in `/var/log`, with `top` in a pane under it.

### Information

These print something and exit. No window opens, and they can go anywhere on the line.

| Option                     | What it does
| :------------------------- | :------------------------------------------------------
| `--help`, `-h`             | The version line and the option list.
| `--syntax`                 | The option list on its own.
| `--about`                  | Version, build, and renderer details, for a bug report.
| `--donate`                 | How to support SilkTerm.
| `--version`, `--ver`, `-v` | Name, version, and build on one line, for scripts.

### Window options

These go before any tab or pane.

| Option                                | What it does
| :------------------------------------ | :------------------------------------------------------------
| `--columns N`, `--rows N`             | Starting size in cells.
| `--pixel-width N`, `--pixel-height N` | Starting size in pixels, in place of cells.
| `--background-opacity F`              | How solid the window is, from 0 to 1.
| `--hide-windowframe[=BOOL]`           | Start without the window manager's frame.
| `--hide-menu[=BOOL]`                  | Start with the menu bar hidden. Ignored on macOS.
| `--fullscreen[=BOOL]`                 | Start fullscreen.
| `--persist[=BOOL]`                    | Remember the size as usual, with a size or font option given.
| `--config PATH`                       | Use another config file.
| `--reset-config`                      | Move the config aside and start from the defaults.
| `--software-rendering[=BOOL]`         | Draw on the CPU for this run only. Not on macOS.

A BOOL can be `yes` or `no`, `true` or `false`, or `1` or `0`. Leaving it off means yes.

`--software-rendering` does what Always use software rendering in Settings does, for that run, and saves nothing. `--software-rendering=no` draws on the graphics card for the run even with the setting on. macOS doesn't have a software renderer, so there it says so and uses the card.

A window opened with `--columns`, `--rows`, `--pixel-width`, `--pixel-height`, `--font-size`, or `--font-name` doesn't remember its size, font zoom, or maximized state, even after a resize. It does again once each of those is changed in Settings, where the pixel sizes count as Columns and Rows. With `--persist` it remembers them as usual from the start.

### Control

Run these from a shell inside a SilkTerm window. They act on that window, then exit.

| Option               | What it does
| :------------------- | :-------------------------------------------------
| `--wallpaper [PATH]` | Change the wallpaper now. With no path, show none.
| `--reload-settings`  | Read the config file again and apply it.

### Tabs and panes

| Option                                | What it does
| :------------------------------------ | :----------------------------------------------------------------------------------
| `--new-tab[=HANDLE]`                  | Make a tab for the options after it. The handle is a name for later options to use.
| `--tab=ID`                            | Go back to a tab already made, by its handle, or `0` or `main` for the first.
| `--new-pane[=HANDLE]`                 | Make a pane by splitting the current one, or the one `--splits` names.
| `--pane=ID`                           | Go back to a pane already made.
| `--splits=ID`                         | With `--new-pane`, which pane to split.
| `--down`, `--up`, `--left`, `--right` | Where the new pane goes.
| `--size=N`, `--size=N%`               | The new pane's size in the split direction, in cells or percent.

A window opens on its first tab, and each tab on its first pane, however many the line makes. Making a tab or pane, or going back to one, only says where the options after it go.

### Window, tab or pane

These apply to whatever came last on the line: the window, a tab, or a pane. A tab takes what the window set, a pane takes what its tab set, and the closest one wins.

| Option                                                     | What it does
| :--------------------------------------------------------- | :-----------------------------------------------------------------------------------------
| `--title TEXT`                                             | Window or tab title. A pane doesn't use it yet.
| `--shell COMMAND`                                          | The command to run, such as `fish` or `'bash --norc'`.
| `--directory PATH`, `--dir PATH`                           | Where that shell starts. `~` and `$VARS` work.
| `--keep-open[=BOOL]`                                       | Keep the pane after its command exits, showing how it ended.
| `--open FILE [ARGS...]`                                    | Run FILE from its own folder. The rest of the line is its arguments.
| `--font-name NAME`, `--font-size N`                        | Font family and size.
| `--background-color #rrggbb`, `--foreground-color #rrggbb` | Background and text color.
| `--wallpaper-file PATH`                                    | The wallpaper. With no path, none. Also `--background-image`.
| `--wallpaper-stretch[=BOOL]`                               | Stretch the wallpaper to fill the window. Also `--background-image-stretch`.
| `--wallpaper-zoom[=BOOL]`                                  | Zoom the wallpaper to cover the window, keeping its shape. Also `--background-image-zoom`.
| `--wallpaper-opacity F`                                    | How much the wallpaper shows, from 0 to 1. Also `--background-image-opacity`.

## Shell integration

A new tab, split, or window starts in the directory the current pane is in. For most shells that doesn't need any setup, because SilkTerm reads the shell process's own directory.

PowerShell is the exception. `Set-Location` changes PowerShell's own idea of where it is, but leaves the process in the directory it was launched in. So there isn't anything to read, and a new pane would start in the launch directory.

SilkTerm handles that one for you. A few seconds after launch it adds a small directory-reporting block to each PowerShell profile. It won't touch a profile that already reports, such as a Windows Terminal setup or oh-my-posh. It appends the block after saving a copy of the profile beside it, rather than rewriting what's there. It won't change your prompt, and it won't put the block back if you delete it. A shell whose execution policy would refuse to load the profile is left alone and a notice says so, rather than being handed a file it can't read. Clear "Update PowerShell profiles" on the Shell tab of Settings, or set `shell.integration: false`, to switch it off before it ever runs.

A shell running behind `ssh` or in a container is yours to set up, since the shell you're typing at isn't the process SilkTerm started.

[shell-integration.md](shell-integration.md) explains all of it, and has the snippets for bash, zsh, and fish.

## Programs over ssh

SilkTerm tells programs what kind of terminal they are in through `TERM`. Each pane gets `TERM=xterm-256color` and `COLORTERM=truecolor`. `ssh` sends the same `TERM` to the other host, and every host has the xterm-256color entry, so `nano`, `less`, and other full-screen programs work there without any setup first.

SilkTerm's terminal engine comes from Alacritty, and the alacritty entry describes a few extras that xterm-256color doesn't, such as curly underlines. To use it, set `shell.term: alacritty` in the config. Every new pane then gets that instead, here and over ssh. A host that has never had the alacritty entry, such as a stock Mac, then says "'alacritty': unknown terminal type." Copy the entry to that host. It goes in your home folder there, so it doesn't need admin rights:

~~~sh
infocmp -x alacritty | ssh HOST 'cat > /tmp/alacritty.ti && tic -x -o ~/.terminfo /tmp/alacritty.ti && rm /tmp/alacritty.ti'
~~~

On macOS `sudo` keeps `HOME`, so a program run with `sudo` finds it too. Linux usually sets `HOME` to root's, so there root needs its own copy.

A program in the pane you're using can set the clipboard, such as tmux or an editor over ssh. On Wayland only the window with the keyboard may set it, so a copy a program makes while the SilkTerm window doesn't have focus waits, and goes to the clipboard when the window gets focus back. If it made more than one meanwhile, the last one wins. X11, macOS, and Windows take it at once.

## Opening scripts and folders on Windows

The Shell tab of Settings has an "Open with SilkTerm" group, one row per kind of file:

- Batch files: `.bat` and `.cmd`.

- PowerShell scripts: `.ps1`, with PowerShell 7 if it's installed and Windows PowerShell if not.

- VBScript files: `.vbs`, through the console script host, so their messages print in the pane.

- Folder menu: an "Open in SilkTerm" entry for folders and drives. On Windows 11 it's under "Show more options".

Register makes a double-click run that kind of file in SilkTerm. The pane stays open after the script ends, so its output and exit status can be read. It's for your account only, and doesn't need administrator rights. The arrow at the end of the row puts back whatever was there before. Register again after moving SilkTerm.

What it doesn't change:

- A console program started any other way, such as `cmd` from the Run box or a double-clicked `.exe`, still opens where it did. SilkTerm isn't a replacement for Windows' default terminal setting.

- A file type you picked an app for under "Open with" keeps that app, because Windows keeps that choice out of reach of other programs. Settings says so, and choosing SilkTerm there yourself is what changes it.

- A script run as administrator still opens in the Windows console.
