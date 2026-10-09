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
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Words used here](#words-used-here)
	- [What is stored](#what-is-stored)
	- [The table](#the-table)
	- [Reading a setting](#reading-a-setting)
	- [Changing a setting](#changing-a-setting)
	- [The group control](#the-group-control)
	- [The settings store](#the-settings-store)
	- [The settings screen](#the-settings-screen)
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

Some settings have a value the program can work out by itself, from a preset or from a rule such as the window size. The usual way to offer that is a master switch, "Automatic layout" or a presets dropdown, that disables the group of settings under it. To change one value, a person first has to find the switch, turn it off, and then every value under it comes back at once, with whatever was stored there last time.

Here there is no master state at all. Each setting that can be automatic is automatic on its own, by storing nothing. Changing it stores a value, and only that one. The group's switch or presets dropdown is worked out from the settings under it, so it always tells the truth and never has to be kept in step.

This is how CSS `auto`, the "Automatic" entries in macOS and Windows settings, and the reset arrows in Firefox and VS Code already work. It needs one table and one function, and the settings store needs nothing new.

## Specification

- An auto setting is one whose value the program can supply. It is stored as either a value set by hand, or nothing.
	- Nothing means automatic. The setting shows and uses what its rule or preset gives right now, and it keeps following that rule as the program runs.
	- A value means set by hand. It is used as is.

- Changing an auto setting stores the new value. No other setting is touched.

- Every auto setting has a way back to automatic, on the setting itself. Which control that is depends on the kind of value, see [The settings screen](#the-settings-screen).

- Nothing is grayed out or disabled on account of being automatic. An automatic setting looks like any other, shows the value in use, and has a small mark saying it is automatic.

- A group of auto settings may have a group control. It is never stored. It is read from the members every time it is drawn:
	- An automatic switch is on when every member is automatic, off when none is, and in the mixed state when some are.
	- A presets dropdown shows the preset whose values every member has right now, else Custom. Custom is a state the dropdown reports, not an entry anyone picks.

- Using the group control changes every member in one step:
	- Switch to on, or a mixed switch clicked: every member goes to automatic.
	- Switch to off: every member stores the value it is showing right now, so nothing visible changes.
	- A preset picked: every member stores that preset's value.

- A hand edit to the settings store is read the same way. A line with a value sets it; no line means automatic.

- The flyover tip of every input control, and of its label, has:
	- The setting's description, if it has one.
	- A blank line, when anything follows it.
	- For an auto setting, one line on its state: `Automatic. Change it to set your own value.` or `Set by hand. Automatic would be: ...`.
	- `Current value: ...` and `Default value: ...`, shown only when they differ. For an automatic setting the current value is what the rule gives right now.

## Goals

- Change one setting in one step, with nothing to hunt for first.

- Never bring back a pile of old values nobody asked for.

- A group control that can't disagree with the settings under it, after a restart or a hand edit to the store.

- One way for every such setting, so a new one needs a row in a table and no code of its own.

- Nothing new in the settings store. A store that keeps only values set by hand already does all of this.

### Non-goals

- A group inside another group. A group control is never itself a member of a group.

- A setting in 2 groups.

- Remembering a hand-set value while a setting is automatic. Going automatic throws the value away, and going manual starts from what is showing. That is what keeps old values from coming back.

- Values locked from outside the settings, by the build or an environment variable. Those are disabled for real, with a tip saying by what.

- Settings that only count while a feature is on, such as the detail choices under "Show tooltips". Those are not automatic, they are unused. They stay enabled and indented under their switch, and come back as they were when the feature is turned on again.

## Design

### Words used here

| Word          | Meaning
| :---          | :---
| Auto setting  | A setting that can be automatic.
| Rule          | How the program works out an auto setting's value: a preset, a computation, or a plain fixed value.
| Automatic     | The state of an auto setting with nothing stored. It uses its rule.
| Set by hand   | The state with a value stored. It uses that value.
| Group         | Auto settings that share a group control.
| Group control | A switch or presets dropdown that reads and sets a whole group. Never stored.
| Mixed         | The switch state when some members are automatic and some set by hand. Most toolkits have one, often drawn as a dash.

### What is stored

- For each auto setting, its hand-set value, or nothing.

- For a group, nothing.

- That is all. There is no mode, no flag per setting, and nothing to clear.

### The table

One table lists every auto setting. Each row has:

- The setting's key.

- Its rule. For a group with presets, the preset values. Otherwise a function, since most rules need the program's state.

- Its group, if any.

A second small table lists each group: its name, whether its control is a switch or a presets dropdown, and the presets with their values.

Both sit with the types and defaults of every other setting, so there is one place to look. A setting's default, in the usual sense, is automatic.

### Reading a setting

One function answers 2 questions for an auto setting: which value it uses, and whether it is automatic.

~~~text
value(setting)     = stored value if one is stored, else rule(setting)
automatic(setting) = nothing is stored
~~~

Nothing reads an auto setting's stored value directly, outside this function. A check in the lint stage refuses code that does.

### Changing a setting

| What happens                       | Each member stores               | Group control then shows
| :---                               | :---                             | :---
| One member is changed              | that one stores its new value    | off, mixed, or Custom
| One member is put back to automatic| that one stores nothing          | on, mixed, or a preset if all match
| Switch turned on                   | nothing                          | on
| Mixed switch clicked               | nothing                          | on
| Switch turned off                  | the value it shows right now     | off
| A preset picked                    | that preset's value              | the preset

A mixed switch goes to automatic on a click, since that is the state a person reaching for an "Automatic" switch wants, and the other way is one more click.

### The group control

- It is drawn from the members and never stored. After a restart or a hand edit to the store it is right by construction.

- A presets dropdown lists the presets and, when nothing matches, a Custom entry that is selected and can't be picked. Picking a preset is always allowed, even the one whose values happen to match, and does the same thing.

- A switch that has only one member is pointless. Put the automatic mark on the setting and skip the group.

- A group of settings whose rule is a plain "off" is usually not a group of auto settings at all, see the last non-goal.

### The settings store

- A store that keeps only values set by hand, with no line for a default, needs nothing new. Automatic is the missing line. Going back to automatic deletes the line.

- A store that writes every key needs a marker for nothing. Use one word for all of them, and never a value that could be real. Keep it out of the public docs until there is such a store.

- A hand edit while the program runs is read the same as a change in the screen. The screen redraws the group from the function.

- A value in the store that fails the setting's checks is treated as every other bad value, and the setting falls back to automatic.

### The settings screen

- The control for an auto setting, by kind of value:
	- A choice from a list: a dropdown with `Automatic` as its first entry. Where it helps, the entry says what it gives right now, as in `Automatic (Compact)`.
	- On or off: a 3-entry dropdown, `Automatic`, `On`, `Off`. A checkbox can't show automatic.
	- A number or text: the usual field, with a small clear icon inside it that shows only while set by hand, tip `Back to automatic`. While automatic the field shows the rule's value in a lighter or italic style, the way a placeholder does, but the text is still readable and the field still takes focus.
	- A color or a file: as for text, with the icon beside the picker.

- An automatic setting whose rule changes while the screen is open redraws at once, as does the group control.

- A screen reader is told the setting is automatic, through the control's accessible description.

- The group control sits above its members. It needs no tip beyond its description, since what it shows is always what the members are.

- The tip is built when it opens, so it never shows values from before a change.

### Tests

- The function, for a stored value, nothing, and every kind of rule.

- Every row of the table under [Changing a setting](#changing-a-setting), with the group control checked after each.

- A restart: the group control reads the same from the store.

- Hand edits to the store, including a bad value, a deleted line, and a line for a setting with a preset group.

- The rule changing at run time while a member is automatic: the value in use follows, and a hand-set member dosn't.

- The table: every setting named in a group exists, is in one group, and every preset names every member of its group.

### Open questions

- None right now.

## Alternative ideas

### Unconsidered

- Keeping the last hand-set value in the store, on a commented line or a side key, so going automatic and back is lossless. It brings the "old pile" problem back by the side door, and nobody has asked for it.

- A "what Custom changed" view on a presets group, listing the members that differ from the nearest preset. Cheap to add later from the same function.

### Rejected

- A master setting with a group mode and an override flag per member, with the members grayed but still editable. It is the [superseded](#superseded) design. It needs 2 stored things beside each master, a rule for clearing stale flags, special cases for reload and for inverted switches, and an on-then-off trick to un-gray a group. Everything it stores is derivable from "which members have a value".

- Graying an editable control. Everyone reads gray as "can't touch", and a focusable control is active, so it has to meet the normal contrast rules anyway.

- Disabling the group under a master switch, as most programs do. It is the problem being solved.

- A per-member "Automatic" checkbox beside each control. It doubles the controls on the screen. The in-control forms above say the same with less.

### Superseded

- [Settings under a master](rejected/20261008-171206_settings_under_a_master.md), 2026-10-08, same day. Rejected at review for the reasons above before any of it was built.

## Research findings

- Per-setting automatic with no master state is the pattern in CSS `auto`, in the "Automatic" entries of macOS System Settings and Windows "Automatic (recommended)" dropdowns, and in Firefox about:config and VS Code settings, where a changed value gets a mark and a reset arrow and the store keeps only what was changed.

- A derived presets dropdown that shows Custom is how game graphics menus and HandBrake work. The complaints on record are that people don't notice one slider threw the preset away, and that Custom doesn't say what changed. The automatic mark on each member answers the first, and the "what changed" view in Unconsidered would answer the second.

- A mode plus per-member flags, the rejected design, is closest to Visual Studio's per-setting "Inherit from parent or project defaults" checkbox, which is widely misunderstood.

- "Enable project specific settings" in Eclipse is the disabled-group pattern. Turning it on brings back the whole set of old values, which is the pile this design avoids.

- Accessibility writing agrees on not disabling form controls for state, since a disabled control says neither why nor what to do. WCAG exempts only inactive controls from the contrast minimum, so a control that takes focus has to meet it.

## Roadmap

- Build the table, the function and the controls for each kind of value.

- Move every existing automatic setting onto it in one go, so there is never a mix of old and new. Leave the "only counts while on" groups as they are.

- Add the lint check that refuses a direct read of an auto setting.

## Related backlog issues

- A setting with an automatic value can be changed on its own, with no master switch to find first (2026100816170959). Queued.

## Copyright and license

> Copyright © 2026 Yottacore<br>
> Licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) license. No warranty.
