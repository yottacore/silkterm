<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Performance profiles

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Profiles sit over the settings](#profiles-sit-over-the-settings)
	- [Remote (temporary)](#remote-temporary)
	- [In the dialog](#in-the-dialog)
	- [The first rating is measured](#the-first-rating-is-measured)
	- [The hardware fingerprint](#the-hardware-fingerprint)
	- [Writing the rating](#writing-the-rating)
	- [The watch](#the-watch)
- [Alternative ideas](#alternative-ideas)
	- [Unconsidered](#unconsidered)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)

<!-- /TOC -->

## Summary

One setting decides how much the look may cost, so a slow machine is a choice on one tab rather than a dozen switches on four. SilkTerm times the machine once, picks the richest profile it can draw at full speed, and writes that down against a fingerprint of the hardware. A remote screen gets a plain profile for the session. The user's own settings are never touched by any of it.

## Specification

- Five profiles, in the order they cost: Custom, Max silk, High, Low and Standard terminal.
	- Max silk is every effect at its shipped setting.
	- High shortens the ease-in, ease-out and single-screen stretches of a scroll, and gives the text halo a cheaper shape with a shorter reach.
	- Low also drops the halo and the cursor animation, and leans on the outline instead. It keeps the wallpaper and smooth scrolling.
	- Standard terminal is a plain terminal: no smooth scrolling, no wallpaper, no halo, no outline, no animation.
	- Custom governs nothing, so every setting is the user's own.

- No profile draws an outline wider than one pixel.

- Remote (temporary) is Standard terminal for a remote screen. It is never written to the file and lasts only the session.

- "Choose automatically" is on by default. A new machine, or one whose hardware changed, is timed once at launch under a banner, and the result is written to the settings file.

- A remote screen is never timed. It gets Remote (temporary) for the session, and the local rating stays as it was.

- A graphics adapter with no card behind it gets Low without timing.

- While running, if frames keep missing the display's refresh, the profile steps down one level until SilkTerm restarts. It never steps below Low, and it never writes that step down.

- A profile sits on top of the user's settings. The file and the dialog keep the user's own values, and Custom brings them all back.

- In Settings, a row a profile governs shows the profile's value. Editing it switches the profile to Custom, keeps what is on screen, and turns "Choose automatically" off.

- Picking a profile in the dropdown turns "Choose automatically" off, except Remote (temporary).

- "Check for hardware change" and "Re-test next run" sit under Performance. The second clears itself once the next launch has started a rating.

- A test the display stalls, such as with the monitor asleep, saves nothing, and the next launch tests again.

## Goals

- A slow machine looks as good as it can without dropping frames, with no setup.

- A fast machine is never slowed down by a bad reading.

- A person's own settings survive every profile change, and are always one pick away.

- One stall never changes every later window.

### Non-goals

- Tuning each effect by measurement. The profiles are a short ladder, not an optimizer.

- Reading the display's power state. It is platform-specific and misses other kinds of stall.

## Design

### Profiles sit over the settings

A profile sits on top of the stored settings rather than in them. The file and the dialog keep the user's own values. When settings go live, the profile overwrites the fields it governs and keeps the originals beside them, and every write path puts them back before anything reaches the file. Choosing Custom is a profile that governs nothing, so it restores everything.

A setting therefore has two layers: the file's and the profile's. `settings()` answers the profile's.

The cheaper profiles keep the same share of the scrim radius, 5 px against the shipped 8, so they look like the same halo built with fewer taps. Low keeps the wallpaper, which is decoded once and costs nothing per frame.

### Remote (temporary)

Remote (temporary) is Standard terminal under another name, and is never written to the file. It is put on for a remote screen at launch, and taken off again at the next launch unless that one is remote too. It can also be switched by hand, from the Profile dropdown or from "Temporary remote display mode" on the View menu, and either way it lasts the session. The stored profile waits underneath it.

- Any display naming a host is remote, localhost included, so X forwarding over ssh counts.

- A remote screen is not rated and nothing is written for it. Every frame is encoded and shipped over a network, so the graphics card says nothing about what the person sees, and a benchmark on it would only flatter the machine.

- Remote is left out of the hardware fingerprint.

### In the dialog

- The Performance section leads the Silk tab, first in the dialog, with text readability and the scrolling feel under it. Those are the two sections it governs most of, so the switch and its effects are on one screen. Wallpaper and cursor rows stay on their own tabs.

- The governed rows show the profile's values rather than the user's own, and their flyover says so. That is display only, so Apply writes the user's values underneath.

- A governed row still takes input, and changing one takes the profile to Custom. The rows used to be grayed, which meant every change started with a trip to the Silk tab to find a dropdown, and the flyover could only say where that tab was. Changing a setting is a clear enough statement that the profile is no longer wanted, so it is read as one: the values on screen become the user's own, the profile becomes Custom and "Choose automatically" goes off. The values on screen are what is kept, not the older ones the profile had been hiding, because the edit was made against what could be seen. Picking Custom from the dropdown is still the other way in, and that one does bring the older values back. A pick says "my settings", and an edit says "this, but with that changed".

- While a profile is showing, a governed row offers no revert arrow, since what it shows is not a value the user set.

- Remote (temporary) is no exception to the edit rule. It governs, so an edit under it drops the session override and the stored profile with it, or the new value would be covered up by one or the other.

- The Profile dropdown stays live while automatic is on, and naming one switches automatic off. It used to be grayed, so taking the machine's choice back meant finding the switch first and then the dropdown, with the dropdown showing the answer being argued with the whole time. Naming a profile is the clearest statement there is that the choice is no longer the machine's. Without that, a pick made with automatic still on would be overwritten at the next launch with nothing on screen to say why. Remote (temporary) is the exception: it lasts the session only and says nothing about what the machine should settle on, so it leaves the switch as it was.

- "Check for hardware change" and "Re-test next run" are grayed while the profile is not chosen automatically, since neither does anything then.

### The first rating is measured

Automatic is the default, and the first pick is measured rather than guessed. Naming the adapter was not enough: an integrated chip is not a slow one, so it started at Max silk and stayed there.

- A remote screen gets the Remote profile, and an adapter with no card behind it goes to Low, untimed, before anything renders.

- Anything else is timed. The window comes up whole, the wallpaper appears, and then a banner takes the window while three rungs are measured in turn: Max silk, High, Low. Each is put live and given up to about a second of full-rate frames. The first whose median frame period fits the display's refresh budget is the answer. The window keeps drawing underneath the banner, dimmed, because what is being timed is worth seeing. It takes no input, because a keystroke would change the measurement. Standard terminal is what is left when Low misses.

- A rung several times past the budget ends the run outright, since no profile below it changes the per-pixel work by that much, and that is also the case that would otherwise take longest to measure.

- A run the display stalls gives no rating. A monitor asleep paces every frame at about one a second, whatever is drawn. The run once read that as a hopeless machine and saved Standard terminal, which has no wallpaper, for every launch after. Simply not saving on a stall would test a truly slow machine at every launch. So when a rung runs more than four times over its budget, Standard terminal is timed once. A slow machine draws that well enough and gets Standard terminal. If Standard terminal stalls as well, the display is what is pacing the frames. Then nothing is saved, the session goes back to the profile it had, the banner says the display was not drawing at its usual rate, and the next launch tests again. A machine too slow even for that is accepted as testing at every launch.

### The hardware fingerprint

What the machine is gets hashed: the processor and its usable core count, the graphics adapter, and installed memory to the nearest GiB. That hash is what the profile is written down against. The parts that need no adapter are read on a worker at launch, since nothing before the first frame wants them.

- A different hash is a different machine and gets rated again. The same hash leaves the profile where it was left.

- "Check for hardware change" switches the check off for a machine already rated, and "Re-test next run" asks for one more rating regardless.

- The rating version is part of the hash, so a change to how ratings are taken rates every machine again once.

### Writing the rating

- The rating is written into the settings file line by line, so a file with a line that cannot be read still keeps it, and the rest of the file is left as it was. A file that reads clean but has nowhere a line can go, such as one with no Performance section, gets what a save from the Settings dialog would write.

- Either way, the write is refused if any other setting would load differently at the next launch. That is judged on the text the next launch would leave, after every rewrite it makes before it reads the file: the wallpaper heading repair, the conversion of a file from before the nested layout, the move of `shell.default` into the shell list, and the renames and refreshes. Adding missing settings is left out, since it only adds lines the program owns and runs whether or not a rating was written.

- Those steps read no quotes or indents now, so nothing known can make the check refuse. It stays for the next step that does, and a test hands it one.

- A rating is also refused where the next launch would not keep it. That launch writes a file from before the nested layout afresh and carries no rating over, so the rating goes in one launch later.

- When the rating cannot be kept, because the file is open in another program or cannot be written, the banner says so before it comes down, and the test runs again at the next launch.

### The watch

The display is still watched after the rating. When the median frame over a window of eased frames runs half again past the refresh period, the profile steps down one rung until SilkTerm restarts.

- The refresh period is that of the monitor the window is on now, read again four times a second, so a window dragged to another monitor is judged by that one. Frames paced under the old one are dropped rather than counted, and a new budget starts the window over.

- Nothing is written, and the next launch starts from the rated profile again.

- The watch stops at Low. Low keeps the wallpaper, which costs nothing per frame, and Standard terminal turns off the eased frames being measured, so a step there could never be checked again.

- Only a window with focus is counted. A frame several times past the budget is not counted at all, because a monitor asleep under the NVIDIA driver paces a GL client at 1 fps, and an idle gap is not a frame either. Eases more than 30 seconds apart start a new window, so a verdict comes from one sitting.

- It never steps back up within a session, because a lighter profile renders less, so a fast run under it says nothing about the heavier one. A hand pick or a measured rating lifts the step, and a hand pick with automatic off stays put.

This reverses an earlier rule, that the step was written down. A written step made one window's misreading every later window's setting, with no way back while automatic was on. It took a 60 Hz desktop with a discrete card down to Standard terminal overnight, and the wallpaper with it. Ratings written before the change were redone once, since a written step cannot be told apart from a measured answer.

## Alternative ideas

### Unconsidered

- A visible alert around a setting that a profile change moved. Backlog: "When other settings are changed automatically based on a user action to a different setting" (Opened 20260919-154614).

### Rejected

- Naming the adapter to pick the profile. An integrated chip is not a slow one.

- A separate rating record in the data directory. It is a second copy of state the settings file already has, and a hand-cleared `rated_hardware` would stop forcing a new rating, which the template comment promises.

- Retrying a write the busy check deferred. It costs a scan of every process's open files on a timer, for a case the banner now explains.

- Writing the step down and asking for a rating at the next launch. That is a banner after every hiccup.

- Writing the step down but stopping at Low. One stall would still change every later window.

- Reading the X11 display power state. It is platform-specific, and it misses every other kind of stall, such as a suspend in mid-ease or a card taken by another program.

### Superseded

- Graying the rows a profile governs, and the Profile dropdown while automatic is on. Both stay live now. Backlog: "Need to autodetect slow environments" and "Settings | Silk: Allow "Profile" to be selected even when "Choose automatically" is enabled".

- Low with no wallpaper, then Low with a 2 px outline. Low keeps the wallpaper and draws a 1 px outline. Backlog: "Performance settings" and "Retune the scrim defaults for the steeper exponential falloff".

- Starting a new machine at Max silk, or Low under software rendering, and stepping down on missed frames, written to the file. The first pick is timed now, and a step is never written.

- A rating stored against the adapter name alone. The fingerprint covers the processor, cores, adapter and memory.

- "Check again next run". It is "Re-test next run".

## Research findings

- A monitor asleep under the NVIDIA driver paces a GL client at one frame a second.

- A discrete card at 3440 by 1440 on Windows answered the top rung, the first rating taken on real hardware there.

- The "+candy" speed row, shipped settings with the automatic profile pinned off, measured 125.6 MiB against 167.7 published.

## Roadmap

- Blur quality is not part of a profile yet. A profile could drive it once the setting exists. See the Text scrim design doc.

- The profile has not been watched on a display that is not 60 Hz, or on two real monitors at different rates.

## Related backlog issues

- "Need to autodetect slow environments" (closed 20260903-213000)

- "Automatic performance detection is not sensitive enough" (Opened 20260904-163124)

- "Remote display detection: a "Remote (temporary)" performance profile" (Opened 20260905-094509)

- "Performance settings." (Opened 20260905-094509)

- "Settings dialog: gather the performance-related sections onto one "Silk" tab" (Opened 20260904-082000)

- "Settings | Silk: Allow "Profile" to be selected even when "Choose automatically" is enabled" (Opened 20260906-084810)

- "Extension to the idea of "Silk: Allow 'Profile' to be selected even when 'Choose automatically' is enabled" (Opened 20260919-155433)

- "Performance test happens at every startup"

- "A performance test run while the monitor is asleep can save a rating that is too low" (Opened 20260910-215844)

- "A performance test can still run at every launch on a settings file with no Performance section" (Opened 20260911-001526)

- "After a window moves to a monitor with a lower refresh rate, the performance profile can step down" (Opened 20260914-124200)

- "Over ssh with X forwarding, a performance rating can be saved for the forwarded screen" (Opened 20260914-124200)

- "Wallpaper vanishes instead of falling back, and a profile round trip does not bring it back" (Opened 20260905-113000)

- "Wallpaper disappears from the background." (Opened 20260910-071431)

- "When other settings are changed automatically based on a user action to a different setting" (Opened 20260919-154614)
