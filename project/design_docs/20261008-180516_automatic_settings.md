<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->
<!-- markdownlint-disable MD055 -- Table pipe style -->

<!-- TOC ignore:true -->
# Settings with an automatic value

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Background](#background)
	- [What has to stay](#what-has-to-stay)
	- [How the performance settings have worked](#how-the-performance-settings-have-worked)
	- [What the first 2 versions of this design got wrong](#what-the-first-2-versions-of-this-design-got-wrong)
- [Specification](#specification)
	- [Switches](#switches)
	- [Rows under a checkbox](#rows-under-a-checkbox)
	- [Profiles and themes](#profiles-and-themes)
	- [Choose automatically, Remote and the step-down](#choose-automatically-remote-and-the-step-down)
	- [Text colors from wallpaper](#text-colors-from-wallpaper)
	- [A rule with no switch](#a-rule-with-no-switch)
	- [The reset arrow](#the-reset-arrow)
	- [Tips](#tips)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Words used here](#words-used-here)
	- [The knobs crate](#the-knobs-crate)
	- [The spec](#the-spec)
	- [SilkTerm's side](#silkterms-side)
	- [The files](#the-files)
	- [Old files](#old-files)
	- [Tests](#tests)
	- [Open questions](#open-questions)
- [Alternative ideas](#alternative-ideas)
	- [Unconsidered](#unconsidered)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)
- [Copyright and license](#copyright-and-license)

<!-- /TOC -->

## Summary

Status: agreed 2026-10-09, after a demo app. The third version of this design. Being built into SilkTerm.

Some settings have a value the program can work out by itself, such as the desktop's font, or the performance profile the machine test picked. Some only count while a checkbox above them is on. And some get their values from a preset, such as a performance profile or a theme.

Every switch is a plain stored checkbox. Turning one on sets aside the values under it that were set by hand, and turning it off brings them back. A change to a row under a preset is kept apart from the person's own values, and the dropdown shows the preset with a `*`. Nothing is grayed out to make a row read-only, and nothing has to be unlocked first.

The rules live in a small crate, `knobs`, with no drawing in it. SilkTerm's dialog keeps its own look and asks knobs what each row shows and what a change does.

## Background

The first version of this design was built in part on 2026-10-09. It lost values set by hand, and the second version that was written to fix it was never built. A demo app tried the model below, and it was agreed the same day.

### What has to stay

- A switch that turns a setting off, or makes it automatic, keeps the value set by hand. Turning the switch back brings that value back.

- Custom values stay remembered while a preset is in use, and come back later.

- Changing one setting takes one step, with nothing to unlock first.

- A change doesn't set off a chain of changes to the settings above it.

### How the performance settings have worked

Choose automatically picks a profile. The profile sets 17 settings: Smooth scrolling and its 5 sliders, smooth scrolling in apps, Blink, Text scrim and 4 rows under it, Outline, and on the Background tab Wallpaper, Blur and Contrast mask.

- Version 1, 2026-09-03 to 09-20:
	- Profile was grayed while Choose automatically was on.
	- The profile's rows were grayed unless the profile was Custom.
	- Rows under a switch that was off were grayed too.
	- So changing Strength % could take 3 steps first: turn off Choose automatically, pick Custom, turn on Text scrim.

- Version 2, 2026-09-20 to 10-09:
	- The rows took input. Changing one copied the profile's values over the person's own, set the profile to Custom, and turned Choose automatically off, all at once.
	- So the Custom values from before were lost on the first change made under a preset.
	- Rows under a switch that was off were still grayed.

### What the first 2 versions of this design got wrong

- The first version stored automatic as no line in the file, so going automatic deleted the value set by hand. It also took out the 2 "Use system font" checkboxes and "Remember last size".

- The second version kept a value put aside, but read every switch off the settings under it. A switch over 2 settings could then be half on, adn a "Use my changes" switch stood in for Custom. It was hard to follow in the demo, and was dropped before any of it was built.

## Specification

### Switches

A switch is a checkbox that makes the settings under it automatic, such as "Use system font" over Family. It is stored like any other setting.

- On, the settings under it use their rule. A value set by hand is set aside.

- Off, a value set aside comes back. With nothing set aside, a setting keeps the value it shows, so nothing on screen moves.

- Changing a setting under a switch that is on turns the switch off. The other settings under it keep what they show.

- When the rule has no answer, such as no desktop font, the setting uses its own value as if the switch were off.

The switches are "Use system font" over Family, "Use system font size" over Size, "Remember last size" over Columns and Rows, "Text colors from wallpaper" over Foreground and Cursor, and Choose automatically over Profile.

### Rows under a checkbox

Some rows only count while a checkbox above them is on, such as Strength % under Text scrim.

- They never gray, and they keep their value while it is off.

- Changing one turns the checkbox on, so the change shows at once. Nothing else above it changes.

### Profiles and themes

A dropdown picks a preset, or Custom. The performance profile and the theme both work this way.

- Custom is the person's own values. They are the normal lines in the file.

- While a preset is in force, the rows it sets show the preset's values.

- Changing one of those rows changes the preset in force, and leaves Custom alone. The dropdown then shows the preset with a `*`, as in `High *`, and its tip names the changed rows. The changes are saved, so they survive a restart.

- For the profile, that change also turns Choose automatically off, since a new pick at the next launch would undo it.

- Picking another entry, Custom included, drops the `*` changes. So does turning Choose automatically back on. It is the one exception to "only the reset arrow throws a value away", picked as the lesser evil. It also fits a "Save as" later.

- Custom is filled on the first run from the preset in force, so picking Custom changes nothing on screen at first.

### Choose automatically, Remote and the step-down

- Choose automatically is the profile's own switch, and its rule is the machine test's pick. A pick by hand turns it off. Turning it on sets the hand pick aside, and turning it off brings it back.

- While it is on, a display that can't keep up steps the profile down for the session, never below Low. Nothing is stored.

- Remote is a temporary profile. It is picked by hand, or at launch on a remote screen.
	- It lasts until restart and stores nothing.
	- The `*` changes of the stored pick are ignored while it is on, and kept.
	- Changing one of its rows ends it. The change goes on the stored pick as a `*` change.

### Text colors from wallpaper

It is the switch over Foreground and Cursor, with the wallpaper as their rule.

- On, the 2 colors come from the wallpaper, over the theme.

- Changing either color turns it off.

- With no wallpaper, the theme's colors are used.

### A rule with no switch

A few settings have a rule but no switch, such as Open command, whose rule is the desktop's opener. With no value set, the rule's answer is the default. The reset arrow puts it back. File or folder works the same way, with the usual folder or the built-in picture.

### The reset arrow

- On a row changed under a preset, it puts the preset's value back. With every changed row put back, the `*` goes.

- On any other row, it puts back the default, which is the rule's answer where there is one.

- On Choose automatically it also drops the `*` changes, the same as turning it on.

- It is the one way a value set by hand is thrown away.

### Tips

A row's tip is its description, a blank line, then one line on where the value comes from.

- `Automatic.`, or `Automatic. Your value, 14, is kept for later.`

- `From the High profile.`

- `Changed. The High profile's value is 50.`

- On the dropdown: `Changed here: Strength %, Text scrim. Picking another profile drops these changes.`

- A plain row keeps the `Current value` and `Default value` lines it has now.

Nothing is grayed for being automatic, set by a preset, or under a checkbox that is off. A row is grayed only for something the machine can't do, with a tip that says why.

A hand edit to the file is read the same as the dialog would have left it. A value that fails its checks is dropped. So is a `*` change for a preset that isn't the one picked.

## Goals

- Change any setting in one step, with nothing to unlock first.

- Never lose a value set by hand, except to a newer value, the reset arrow, or a new pick dropping `*` changes.

- Custom stays as it was while presets are tried.

- A change moves nothing above it, except the checkbox a row sits under, and Choose automatically for a change under a profile.

- Switches that are plain stored values, so a restart or a hand edit can't make one disagree with what the dialog shows.

- One way for every setting. A new one is a row in the dialog's spec with its relations, and a line in the field table.

### Non-goals

- A setting in 2 groups.

- Named sets of profile changes. Custom is the first such slot, and "Save as" may come later.

- Values locked from outside the settings, by the build or an environment variable. Those gray for real, with a tip saying by what.

## Design

### Words used here

| Word      | Meaning
| :-------- | :---------------------------------------------------------------------------------
| Switch    | A checkbox that makes the settings under it automatic.
| Rule      | Where an automatic value comes from: the desktop, the wallpaper, the machine test.
| Gate      | A checkbox a row only counts under. Changing the row turns it on.
| Group     | A dropdown, its presets, and the settings they set. The profile and the theme.
| Preset    | One named set of values in a group.
| Custom    | The group entry that uses the person's own values.
| Change    | A value changed while a preset is in force, shown by the `*`.
| Set aside | A value set by hand, kept while its switch is on.
| Temporary | A preset that lasts until restart and stores nothing, such as Remote.
| State     | What the program works out and remembers on one machine.

### The knobs crate

`knobs/` is a crate in the workspace with no drawing in it, and a demo app beside it that draws with egui. Only the demo depends on egui.

- The spec: every setting, its path, type and default, and its relations. A group lists its presets.

- The values: own values, `*` changes and which preset they were made on, state values, and any temporary preset.

- Reading. A setting uses its rule while its switch is on and the rule has an answer. Otherwise it uses the `*` change or the preset's value while a preset is in force, else its own value, else the default.

- Changing. One call does everything a change does to the settings around it, so the dialog never works that out itself.

- It also answers where a value comes from, the tip's line, whether the reset arrow has anything to do, and what the dropdown shows.

The rules were tried in the demo first, since it rebuilds in seconds and the dialog doesn't.

### The spec

`settings_ui.shcl` stays the one file that declares the dialog. Its rows get a few more fields:

- `gate:` names the checkbox a row only counts under.

- `auto:` and `rule:` name the row's switch and its rule. A `rule:` with no `auto:` gives the default.

- `group:` names the profile or theme a row belongs to.

- `store: state` marks a value the program works out, such as the tested profile. `kind: none` is a value with no row.

A `groups:` block lists each group's dropdown, what Custom is called, and its presets, with a value for every member. The profiles are data there now. The themes come from the program, since they depend on the mode, the desktop, and the themes saved from the dialog.

Indents are worked out from the relations, unless a row sets `indent:`. The graying block goes.

### SilkTerm's side

- A table maps each setting to its field in `Settings`, in the file's units. Each line says how to read the field into a value and how to put one back. The defaults come from `Settings::default()` through it, so no new place holds a default.

- `Settings` keeps the stored values, and every setting field is filled from them. That happens at load, after every change in the dialog, and when a rule's answer changes: the desktop's dark mode, a new wallpaper, a step-down.

- The rest of the program reads `Settings` as before.

- The program answers the rules: the desktop's font and size, the size last used on a monitor, the wallpaper's colors, the machine test's pick with any step-down, the theme's colors, and the usual wallpaper and opener.

- The dialog keeps its look and its controls. A row reads its value from `Settings`, and a change goes to knobs as a value in the file's units. The reset arrow, the tip's line and the `*` come from knobs.

The dialog was kept over the demo's widgets, since it has a lot the demo lacks: typed values past a slider's end, warning marks, rows for one platform, packed lines of checkboxes, the shells grid, hotkeys. Putting the relations in `settings_ui.shcl` was chosen over a second spec beside it, since 2 lists of the same rows drift.

### The files

`config.shcl` keeps the settings the person set, then a `kept:` block at the end for what the program saves on their behalf.

~~~shcl
text:
	scrim:
		strength: 80
kept:
	set_aside:
		font:
			family: "Fira Code"
	changes:
		profile:
			preset: high
			values:
				text:
					scrim:
						radius: 9
~~~

- Here Strength % is 80 in Custom, Family has Fira Code set aside under "Use system font", and the High profile has one `*` change.

- A group member's normal line is its Custom value, even under a switch that is on.

- The file is written in place. Only the lines that changed are touched, and comments stay.

`state.shcl` is a second file, per machine, not meant for hand edits, and safe to lose. It holds the tested profile and the hardware it was tested on, the last window size and font zoom, the sizes per monitor, and whether the window was maximized. It lives in the state folder: `XDG_STATE_HOME` on Linux, `%LOCALAPPDATA%` on Windows, Application Support on macOS. A section in the config would still be copied to other machines, and would still rewrite the config on every exit.

### Old files

- The old switch keys are read as they are, so a file from before 10-09 needs no conversion. The launch step that took them out of the file goes.

- `performance.profile: custom` loads as Custom. Under any other profile, the lines for its rows were already the person's own values kept under the profile, so they become Custom's.

- The tested hardware, the last size and the sizes per monitor move to `state.shcl` at the first launch. With Choose automatically on, `performance.profile` was the machine test's pick, so it is copied there too.

- A theme with `colors.*` lines that differ from it gets those as its `*` changes, so nothing on screen changes. The lines stay as Custom's values.

### Tests

- knobs: one test per rule, and the files round trip.

- Every row of the dialog: a change, a save and a load give the same value.

- Each profile's values in the spec match what `profile.rs` sets, before that code goes.

- Old files: a set of real config files loads to the same settings as before, except for where this design changes them on purpose.

- A step-down, Remote at launch, a dark mode flip, and a new wallpaper, each with the dialog open and closed.

### Open questions

- What Save does to a theme with `*` changes. The likely answer: Save puts them into a saved theme, and Save as makes a new theme from what shows.

- Fonts in themes (2026100913394956).

## Alternative ideas

### Unconsidered

- A "what changed" list under Profile. The tip on the dropdown names the rows for now.

- More than one Custom, named. Nobody has asked for it.

### Rejected

- Automatic stored as no line, with nothing kept. It lost the value behind every switch.

- Switches read off the settings under them, with a half-on state and a "Use my changes" switch. Stored switches are easier to follow, and can't disagree with the rows after a hand edit.

- A change under a preset that copies the preset over Custom, as in version 2 of the profiles.

- Graying a row until something above it is changed, as in version 1. Everyone reads gray as "can't touch", and the person has to find what unlocks it.

- Keeping the `*` changes across a pick. They would pile up for every preset, with no way to see them all.

- Values set aside as commented-out lines. A save that keeps comments can still move them, and a hand edit can't tell one from a note.

- State in `config.shcl`. It would be copied between machines and rewrite the config on exit.

- Moving SilkTerm onto the demo's widgets. See [SilkTerm's side](#silkterms-side).

### Superseded

- The first version of this design, 2026-10-08 to 10-09: automatic as no line. Built for the font, window size, theme colors, open command and wallpaper (2026100907341818, 2026100910295903) before the lost values were noticed.

- The second version, 2026-10-09: "Use my changes", switches read off their settings, and a half-on state. Not built.

- Settings under a master, 2026-10-08. A master switch with a stored mode and an override flag per member. Rejected at review before any of it was built.

## Research findings

- Programs that keep a value through an on and off round trip store the switch and the value apart.
	- Firefox's proxy settings store the mode on its own. Switching away from manual grays the host and port fields but keeps them.
	- Unity's render volumes have an override checkbox on every property. Cleared, the property uses the default, and the value typed in stays.
	- Print dialogs keep a page range typed in while All is picked, and typing in the field picks Pages. That is the model for a row turning its checkbox on.

- Game graphics menus usually turn the preset to Custom when one setting changes, and overwrite every setting when a preset is picked. Player forums ask for custom settings to survive a preset pick.

- Firefox about:config and VS Code settings give a changed value a reset arrow, and store only what was changed.

- Accessibility writing agrees on not disabling form controls for state, since a disabled control says neither why nor what to do. WCAG exempts only inactive controls from the contrast minimum, so a control that takes focus has to meet it.

## Roadmap

- Done: the knobs crate and its demo, 2026-10-09.

- knobs: a spec built from code, a rule that gives a default, presets from the program, a temporary preset set by the program, and saving into a file that keeps its comments.

- SilkTerm: the relations and groups in `settings_ui.shcl`, the field table, the stored values in `Settings`, `state.shcl`, and old files.

- The dialog through knobs: the switches back, the `*`, the reset arrow and the tip lines.

- Prove the old and new paths agree on old files and on each profile's values, then remove the old code in one go: the profile's shadow and values, the wallpaper colors' shadow, `config::auto` and its lint check, the graying block, and the launch step that took the switches out.

- No control is removed. This design changes how controls work, not which ones exist.

## Related backlog issues

- A setting with an automatic value can be changed on its own, with no master switch to find first (2026100816170959).

- Automatic settings, first part (2026100907341818), and theme colors, open command and wallpaper as automatic settings (2026100910295903).

- Wallpaper colors (2026100910295901) and profile presets (2026100910295902).

- The removed "Use system font" checkboxes (2026100913394948), and the "A" mark and italic style (2026100913394950).

## Copyright and license

> Copyright © 2026 Yottacore<br>
> Licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) license. No warranty.
