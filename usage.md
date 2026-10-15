<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Using SilkTerm

Where the config lives, the command line, shell integration, and opening scripts and folders on Windows.

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Configuration](#configuration)
- [Command line](#command-line)
	- [Information](#information)
	- [Window options](#window-options)
	- [Control](#control)
	- [Tabs and panes](#tabs-and-panes)
	- [Window, tab or pane](#window-tab-or-pane)
- [Shell integration](#shell-integration)
- [Opening scripts and folders on Windows](#opening-scripts-and-folders-on-windows)

<!-- /TOC -->

## Configuration

On first run SilkTerm writes a commented config file with all defaults, in the place each platform keeps settings:

| OS      | Config                                            | Wallpaper folder
| :------ | :------------------------------------------------ | :-----------------------------------
| Linux   | `$XDG_CONFIG_HOME/silkterm/` (or `~/.config/...`) | beside the config
| Windows | `%APPDATA%\silkterm\`                             | `%LOCALAPPDATA%\silkterm\wallpaper\`
| macOS   | `~/Library/Application Support/silkterm/`         | beside the config

Setting `XDG_CONFIG_HOME` overrides the platform default everywhere, and `--config PATH` overrides everything - an alternate config keeps its own wallpaper and history beside itself rather than sharing the defaults.

Windows splits the two because settings are worth roaming between machines and a 60 MiB wallpaper pack is not. A pack already sitting beside the config still works; nothing has to be moved.

If making changes directly (rather than through Settings), you can apply them immediately with the "Reload config" menu item. Settings reads the file each time it opens, so it also shows what another SilkTerm window saved.

Settings names are meant to read plainly, but a few of them - scrim, contrast mask, automask mix - are particular to SilkTerm. [glossary.md](glossary.md) defines those.

To start over from the shipped defaults, run `silkterm --reset-config`. The old file is kept alongside as `config.shcl.bak` rather than deleted.

When an update converts the file to a newer format, the file as it was is kept alongside, named for the local time it was converted and the format it had, such as `config_backup_20261003-142233_format-v2.shcl`. Every older version is kept, and a file named with `--config` keeps its own name in front. A line the new format can't hold stays in the file as written, and SilkTerm puts up a notice saying how many there were and where the old file is. If the file can't be converted where it is, because it isn't UTF-8 text or the new format can't read it, SilkTerm writes a new one from the defaults with every setting it can still read, and the notice says how many it couldn't copy to the new file. A file already in the current format with lines that aren't UTF-8 text is written again without those lines, and kept alongside the same way. A notice names the lines and where the old file is.

Drop a few images into a `wallpaper` folder next to the config and SilkTerm picks one each launch, favoring whatever it hasn't shown lately. Naming a wallpaper in the config, or passing one on the command line, takes precedence. The [wallpaper pack](install.md#wallpaper-pack) is a ready-made folder to start from.

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
| :------------------------- | :-----------------------------------------------------
| `--help`, `-h`             | The version line and the option list.
| `--syntax`                 | The option list on its own.
| `--about`                  | Version, build and renderer details, for a bug report.
| `--donate`                 | How to support SilkTerm.
| `--version`, `--ver`, `-v` | Name, version and build on one line, for scripts.

### Window options

These go before any tab or pane.

| Option                                | What it does
| :------------------------------------ | :-------------------------------------------------
| `--columns N`, `--rows N`             | Starting size in cells.
| `--pixel-width N`, `--pixel-height N` | Starting size in pixels, in place of cells.
| `--background-opacity F`              | How solid the window is, from 0 to 1.
| `--hide-windowframe[=BOOL]`           | Start without the window manager's frame.
| `--hide-menu[=BOOL]`                  | Start with the menu bar hidden. Ignored on macOS.
| `--fullscreen[=BOOL]`                 | Start fullscreen.
| `--config PATH`                       | Use another config file.
| `--reset-config`                      | Move the config aside and start from the defaults.

A BOOL can be `yes` or `no`, `true` or `false`, or `1` or `0`. Leaving it off means yes.

### Control

Run these from a shell inside a SilkTerm window. They act on that window, then exit.

| Option               | What it does
| :------------------- | :-------------------------------------------------
| `--wallpaper [PATH]` | Change the wallpaper now. With no path, show none.
| `--reload-settings`  | Read the config file again and apply it.

### Tabs and panes

| Option                                | What it does
| :------------------------------------ | :----------------------------------------------------------------------------------------
| `--new-tab[=HANDLE]`                  | Make a tab, which becomes the current one. The handle is a name for later options to use.
| `--tab=ID`                            | Go back to a tab already made, by its handle, or `0` or `main` for the first.
| `--new-pane[=HANDLE]`                 | Make a pane by splitting the current one, or the one `--splits` names.
| `--pane=ID`                           | Go back to a pane already made.
| `--splits=ID`                         | With `--new-pane`, which pane to split.
| `--down`, `--up`, `--left`, `--right` | Where the new pane goes.
| `--size=N`, `--size=N%`               | The new pane's size in the split direction, in cells or percent.

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

A new tab, split or window starts in the directory the current pane is in. For most shells that needs nothing set up - SilkTerm reads the shell process's own directory.

PowerShell is the exception: `Set-Location` moves PowerShell's own idea of where it is and leaves the process where it was launched, so there is nothing to read and a new pane would start in the launch directory.

SilkTerm handles that one for you. A few seconds after launch it adds a small directory-reporting block to each PowerShell profile - and it will not touch a profile that already reports (a Windows Terminal setup, oh-my-posh, anything else), will not rewrite what is there (it appends, after saving a copy beside it), will not change your prompt, and will not put the block back if you delete it. A shell whose execution policy would refuse to load the profile is left alone and said so, rather than being handed a file it cannot read. Clear "Update PowerShell profiles" on the Shell tab of Settings, or set `shell.integration: false`, to switch it off before it ever runs.

Same story for a shell running behind `ssh` or in a container - that one is yours to set up, since the shell you are typing at is not the process SilkTerm started.

[shell-integration.md](shell-integration.md) covers all of it, including the snippets for bash, zsh and fish.

## Opening scripts and folders on Windows

The Shell tab of Settings has an "Open with SilkTerm" group, one row per kind of file:

- Batch files: `.bat` and `.cmd`.

- PowerShell scripts: `.ps1`, with PowerShell 7 if it is installed and Windows PowerShell if not.

- VBScript files: `.vbs`, through the console script host, so their messages print in the pane.

- Folder menu: an "Open in SilkTerm" entry for folders and drives. On Windows 11 it is under "Show more options".

Register makes a double-click run that kind of file in SilkTerm. The pane stays open after the script ends, so its output and exit status can be read. It is for your account only and needs no administrator rights. The arrow at the end of the row puts back whatever was there before. Register again after moving SilkTerm.

What it does not cover:

- A console program started any other way, such as `cmd` from the Run box or a double-clicked `.exe`, still opens where it did. SilkTerm is not a replacement for Windows' default terminal setting.

- A file type you picked an app for under "Open with" keeps that app, because Windows keeps that choice out of reach of other programs. Settings says so, and choosing SilkTerm there yourself is what changes it.

- A script run as administrator still opens in the Windows console.
