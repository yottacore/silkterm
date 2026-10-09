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
	- [What the first version of this design lost](#what-the-first-version-of-this-design-lost)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Words used here](#words-used-here)
	- [What is stored](#what-is-stored)
	- [The table](#the-table)
	- [Reading a setting](#reading-a-setting)
	- [Changing a setting](#changing-a-setting)
	- [Switches](#switches)
	- [Profiles](#profiles)
	- [Rows under a switch](#rows-under-a-switch)
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

Status: draft, 2026-10-09. The second version of this design, waiting for signoff.

Some settings have a value the program can work out by itself, from a performance profile or from a rule such as the desktop's font. A person can still set any of them by hand, and a switch can put one back to automatic.

Each such setting keeps 2 things: the value in use, if it was set by hand, and a value put aside for later. Turning a switch to automatic puts the hand-set value aside. Turning it back brings that value back. Picking a profile puts aside every value set by hand under it, and one switch brings them all back.

A change only ever flows down. Changing a setting changes that setting, and a switch or profile changes the settings under it. Nothing changes a setting above it, with one exception: changing a row under a switch that is off turns that switch on.

## Background

The first version of this design was built in part on 2026-10-09. It broke one thing people expect from settings, and the Performance settings still had the problems that led to it.

### What has to stay

- A switch that turns a setting off, or makes it automatic, keeps the value set by hand. Turning the switch back brings that value back.

- Custom values stay remembered while a preset is in use, and come back later.

- Changing one setting takes one step, with nothing to unlock first.

- A change doesn't set off a chain of changes to the settings above it.

### How the performance settings have worked

Choose automatically picks a profile. The profile sets 16 settings: Smooth scrolling and its 5 sliders, Blink, Text scrim and 4 rows under it, Outline, and on the Background tab Wallpaper, Blur and Contrast mask. Some of those are switches with rows under them that only count while the switch is on, such as Strength % under Text scrim.

There have been 3 versions so far.

- Version 1, 2026-09-03 to 09-20:
	- Profile was grayed while Choose automatically was on.
	- The 16 rows were grayed unless the profile was Custom.
	- Rows under a switch that was off were grayed too.
	- So changing Strength % could take 3 steps first: turn off Choose automatically, pick Custom, turn on Text scrim.
	- The rows on the Background tab could only be unlocked from the Silk tab.

- Version 2, 2026-09-20 to 10-09:
	- Profile took input while Choose automatically was on. Picking a profile turned automatic off.
	- The 16 rows took input. Changing one made the values on screen the person's own, set the profile to Custom, and turned Choose automatically off, all at once.
	- Rows under a switch that was off were still grayed. So Strength % couldn't be changed while Text scrim was off, and changing it never turned Text scrim on. Turning Text scrim on was the change that set Custom and turned off Choose automatically.
	- That step lost the Custom values from before, since the preset's values were copied over them. Picking a preset and then Custom again did bring them back, as long as no row was changed in between.

- Version 3, the first version of this design, 2026-10-09:
	- Rows under a switch no longer gray. Strength % can be changed while Text scrim is off. That doesn't turn Text scrim on, and the value is used once it is on.
	- The Performance settings didn't move onto it. They still work as in version 2, without the graying.

### What the first version of this design lost

- It stored automatic as no line in the file. So going automatic deleted the value set by hand.

- The 2 "Use system font" checkboxes, for Family and Size, were taken out, and so was "Remember last size" for Columns and Rows. A family typed in was lost once Family went back to automatic.

- Turning a switch off and on again still brought back the rows under it. Text scrim and Strength % worked that way, and so did every other switch with rows under it.

## Specification

- An auto setting is one whose value the program can supply, from a profile or a rule. It is in one of 3 states:
	- Automatic. It uses what its rule or profile gives right now, and keeps following it as the program runs.
	- Set by hand. It uses its own value.
	- Put aside. It has a value of its own, but is automatic for now. The value comes back when its switch or the "Use my changes" switch says so.

- Changing an auto setting sets it by hand, with the new value. Any value it had put aside is dropped, since the new one replaces it.

- A switch that makes settings automatic puts their hand-set values aside when turned on, and brings them back when turned off. A setting with nothing put aside keeps the value it shows, so nothing on screen moves.

- Picking a profile by hand puts aside every hand-set value under it, so that way the screen shows exactly that profile. The "Use my changes" switch brings them back.

- Choose automatically picking a new profile, after a hardware change or when the display can't keep up, leaves the hand-set values in use. Remote ignores them while it is on, and changes nothing stored.

- Changing a row under a switch that is off turns the switch on. Nothing else above it changes.

- Nothing is grayed out on account of being automatic, set by a profile, or under a switch that is off.

- A hand edit to the settings store is read the same way as a change on the screen.

- The flyover tip of an auto setting has one line on its state, after the description:
	- Automatic: `Automatic.`, or for a profile row, `From the High profile.`
	- Set by hand: `Set by hand. Automatic would be: ...`, or `Set by hand. The High profile's value is ...`
	- Put aside: the automatic line, then `Your value, ..., is kept for later.`

## Goals

- Change one setting in one step, with nothing to unlock first.

- Never lose a value set by hand, except to a newer value or the reset arrow.

- Never change a setting above the one changed, except a switch turned on by a row under it.

- Switches that can't disagree with the settings under them, after a restart or a hand edit to the store.

- One way for every such setting, so a new one needs a row in a table and no code of its own.

### Non-goals

- A setting in 2 groups.

- A switch over a profile's rows that is itself a profile row.

- Values locked from outside the settings, by the build or an environment variable. Those are disabled for real, with a tip saying by what.

- Named saved sets of changes. "Use my changes" is one set.

## Design

### Words used here

| Word         | Meaning
| :----------- | :--------------------------------------------------------------------------------------------------------------
| Auto setting | A setting that can be automatic.
| Rule         | How the program works out an auto setting's value: a computation such as the desktop's font, or a fixed value.
| Profile row  | An auto setting whose rule is the performance profile in force.
| Automatic    | No value of its own in use. It uses its rule.
| Set by hand  | Its own value in use.
| Put aside    | Its own value kept, not in use. It uses its rule.
| Switch       | A checkbox that makes a group of auto settings automatic, or brings their values back. Never stored.
| Mixed        | The switch state when some of its settings are automatic and some set by hand. Most toolkits draw it as a dash.

### What is stored

- For each auto setting, at most one of: its value in use, or its value put aside.

- Neither means automatic with nothing kept.

- Switches are never stored. They are read from the settings under them.

- The profile is itself an auto setting. Its rule is the machine test, and Choose automatically is its switch.

### The table

One table lists every auto setting. Each row has:

- The setting's key.

- Its rule. A function, since most rules need the program's state. For a profile row, the rule is the profile in force.

- Its switch, if it has one.

Switches are a second small table: the label, and whether on means automatic, as for "Use system font", or set by hand, as for "Use my changes".

Both sit with the types and defaults of every other setting. A setting's default, in the usual sense, is automatic.

### Reading a setting

One function answers 2 questions for an auto setting: which value it uses, and which state it is in.

~~~text
value(setting) = value in use if one is stored, else rule(setting)
state(setting) = set by hand if a value in use is stored,
                 else put aside if a value put aside is stored,
                 else automatic
~~~

Nothing reads an auto setting's stored values directly, outside this function. A check in the lint stage refuses code that does.

### Changing a setting

| What happens                           | The setting then                             | Its switch then shows
| :------------------------------------- | :------------------------------------------- | :--------------------
| Changed on screen                      | set by hand, anything put aside dropped      | off, or mixed
| Reset arrow                            | automatic, nothing kept                      | on, or mixed
| Switch turned to automatic             | its hand-set value put aside                 | automatic
| Switch turned back, nothing aside      | set by hand, at the value it shows           | set by hand
| Switch turned back, a value aside      | set by hand, at that value                   | set by hand
| Mixed switch clicked                   | as turned to automatic                       | automatic
| Profile picked by hand                 | every profile row's hand-set value put aside | "Use my changes" off
| Profile picked by Choose automatically | unchanged                                    | unchanged

A mixed switch goes to automatic on a click, since that is the state a person reaching for an "Automatic" switch wants, and the other way is one more click. For "Use my changes" the same click brings every value back, since on means set by hand there.

### Switches

- A switch is read from the settings under it every time it is drawn, so after a restart or a hand edit to the store it is right by construction.

- One with one setting under it is that setting's own switch, such as "Use system font" for Family. One with more, such as "Remember last size" for Columns and Rows, shows mixed when they differ.

- Typing in Columns while "Remember last size" is on sets Columns by hand. Rows stays automatic, and the switch shows mixed. Nothing else on screen moves.

- The switches the first version took out come back: the 2 "Use system font" checkboxes, "Remember last size", and "Text colors from wallpaper".

### Profiles

- The profile is an auto setting. Automatic means Choose automatically picks it from the machine test. Picking one by hand sets it by hand, which turns Choose automatically off. Turning Choose automatically on puts the hand pick aside, and turning it off brings it back.

- There is no Custom profile. Each profile row is automatic, set by hand, or put aside on its own. The hand-set rows are the person's changes, on top of whichever profile is in force.

- The Profile dropdown shows the profile in force, and how many rows are set by hand, as in `High, 2 changed`.

- "Use my changes" sits under Profile. It is on when every row with a value of its own has it in use, off when none does, and mixed between. It is disabled when no row has a value of its own, with the tip `Nothing changed yet.`

- Picking a profile by hand puts every hand-set row aside, so the screen shows exactly that profile. Picking one with no rows set by hand puts nothing aside, and keeps what was aside before.

- A new profile from Choose automatically, after a hardware change or a display step-down, leaves the rows as they are. The hand-set rows stay in use on top of it.

- Remote ignores every hand-set row while it is on, since it is a plain terminal. It stores nothing and changes no row.

- Changing a profile row while Choose automatically is on leaves it on. The profile stays the same.

### Rows under a switch

- Some rows only count while a switch above them is on, such as Strength % under Text scrim. Those are not auto settings. Each keeps its value while the switch is off.

- They never gray. Changing one while its switch is off turns the switch on, so the change shows at once.

- If the switch is a profile row, turning it on this way sets it by hand. Nothing above it changes: the profile and Choose automatically stay as they are.

- This is the one place a change moves a setting above it. The switch is on the same tab, right above the row, so the change is in plain sight.

### The settings store

- A value in use is the setting's usual line. A store that keeps only values set by hand already reads automatic as a missing line.

- A value put aside goes under a `kept:` block, at the same path it would have outside it.

~~~shcl
text:
	scrim:
		strength: 80
kept:
	font:
		family: "Fira Code"
~~~

- Here Strength % is set by hand at 80, and Family is automatic with Fira Code put aside.

- A line in both places is read as set by hand, and the next save drops the one in `kept:`.

- A value in either place that fails the setting's checks is treated as every other bad value. It is dropped and the setting is automatic.

- A hand edit while the program runs is read the same as a change in the screen.

### The settings screen

- The control for an auto setting, by kind of value:
	- A choice from a list: a dropdown with `Automatic` as its first entry, where the setting has no switch of its own. Where it helps, the entry says what it gives right now, as in `Automatic (Compact)`.
	- On or off: a 3-entry dropdown, `Automatic`, `On`, `Off`, where it has no switch.
	- A number, text, color or file: the usual field. While automatic it shows the rule's value, and still takes focus and input.

- The reset arrow is enabled while a setting is set by hand. It goes back to automatic and keeps nothing, tip `Back to automatic`. It is how a value is thrown away for good.

- No mark, italic or lighter style says a value is automatic or put aside. The switch and the tip's state line say it.

- An automatic setting whose rule changes while the screen is open redraws at once, as do the switches.

- A screen reader is told the setting's state, through the control's accessible description.

- The tip is built when it opens, so it never shows values from before a change.

### Tests

- The function, for each of the 3 states, and every kind of rule.

- Every row of the table under [Changing a setting](#changing-a-setting), with the switch checked after each.

- A round trip for each switch: a value set by hand, the switch on and off, and the same value back.

- A profile picked by hand, then "Use my changes" turned on: every changed row back as it was.

- A new profile from Choose automatically: no row changes state. Remote: the screen shows Remote's values, and nothing stored changes.

- A row under a switch that is off, changed: the switch on, adn nothing else changed.

- A restart: every switch and the Profile dropdown read the same from the store.

- Hand edits to the store, including a bad value, a deleted line, and a line both in use and in `kept:`.

- The rule changing at run time while a setting is automatic: the value in use follows, and a hand-set setting doesn't.

- The table: every setting named under a switch exists, and is under one switch.

### Open questions

- Whether a theme and its colors work the same way as a profile and its rows, with the theme's colors as the rule and "Use my changes" for colors changed by hand.

- How "Text colors from wallpaper" fits. Foreground and Cursor already have the theme as their rule, so the wallpaper is a second rule for the same 2 settings (2026100910295901).

## Alternative ideas

### Unconsidered

- A "what changed" list under Profile, naming the rows set by hand. Cheap to add later from the same function.

- More than one saved set of changes, named. Nobody has asked for it.

### Rejected

- Automatic stored as no line, with nothing kept. It is the first version of this design, and it lost the value behind every switch.

- A Custom profile, with one set of values. Any change made while a preset was in use copied the preset over it. Changes on top of whichever profile is in force replace it.

- A change that moves the settings above it, as in version 2. Changing one row set Custom and turned Choose automatically off.

- A master setting with a stored group mode, and its members grayed until the mode is changed. It is version 1's unlock steps.

- Keeping the hand-set rows in use when a profile is picked by hand. A picked profile then wouldn't look like that profile.

- Put-aside values as commented-out lines. A save that keeps comments can still move or drop them, and a hand edit can't tell one from a note. A separate key is plain data.

- A checkbox beside every auto setting. It doubles the controls on the screen. The switches and the in-control forms above say the same with less.

- Graying an editable control. Everyone reads gray as "can't touch", and a focusable control has to meet the normal contrast rules anyway.

### Superseded

- The first version of this design, 2026-10-08 to 10-09. Automatic was stored as no line, and every switch was read from whether its settings had lines. It was built in part, for the font, window size, theme colors, open command and wallpaper (2026100907341818, 2026100910295903), before the lost values were noticed.

- Settings under a master, 2026-10-08. A master switch with a stored group mode and an override flag per member, with the members grayed but still editable. Rejected at review before any of it was built.

## Research findings

- Programs that keep a value through an on and off round trip store the switch and the value apart.
	- Firefox's proxy settings store the mode on its own. Switching away from manual grays the host and port fields but keeps them.
	- Unity's render volumes have an override checkbox on every property. Cleared, the property uses the default, and the value typed in stays. All and None buttons flip a whole group.
	- Print dialogs keep a page range typed in while All is picked, and typing in the field picks Pages. That is the model for a row turning its switch on.

- Game graphics menus usually turn the preset to Custom when one setting changes, and overwrite every setting when a preset is picked. Player forums ask for custom settings to survive a preset pick.

- Per-setting automatic with no master state is the pattern in CSS `auto`, in the "Automatic" entries of macOS System Settings and Windows "Automatic (recommended)" dropdowns, and in Firefox about:config and VS Code settings, where a changed value gets a reset arrow and the store keeps only what was changed.

- Accessibility writing agrees on not disabling form controls for state, since a disabled control says neither why nor what to do. WCAG exempts only inactive controls from the contrast minimum, so a control that takes focus has to meet it.

## Roadmap

- Add the `kept:` block and the 3-state function. The settings already moved onto the first version keep their table rows.

- Bring back the switches the first version took out, read from their settings.

- Change the launch step that took the old switches out of the file. A switch that was on puts its settings' values aside, where it used to comment them out.

- Move the Performance rows onto it: drop Custom, add "Use my changes", and make a changed row leave Profile and Choose automatically alone. A file whose profile was Custom keeps every row that differs from the default profile as set by hand, so nothing on screen changes.

- Make a change to a row under a switch that is off turn the switch on.

- Keep the lint check that refuses a direct read of an auto setting.

- Moving a setting onto this design changes how its controls work, not which ones exist. No control is removed.

## Related backlog issues

- A setting with an automatic value can be changed on its own, with no master switch to find first (2026100816170959).

- Automatic settings, first part (2026100907341818), and theme colors, open command and wallpaper as automatic settings (2026100910295903).

- Wallpaper colors (2026100910295901) and profile presets (2026100910295902).

## Copyright and license

> Copyright © 2026 Yottacore<br>
> Licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) license. No warranty.
