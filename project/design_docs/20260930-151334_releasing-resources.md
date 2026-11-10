<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Releasing resources

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Fewer frames](#fewer-frames)
	- [Letting the GPU go on a long idle](#letting-the-gpu-go-on-a-long-idle)
	- [After a text console](#after-a-text-console)
	- [Memory](#memory)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)
- [Copyright and license](#copyright-and-license)

<!-- /TOC -->

## Summary

A terminal is often left open for days, many at a time. SilkTerm is built so that a window that nobody is looking at doesn't cost anything. It doesn't draw any frames, its cursor stops, and if that's turned on, it gives its GPU device back until it's used again. Memory it no longer needs is returned to the system, and a program can't make it grow without limit.

What a window costs while it's in use is described in the [Reducing resources](20261004-182255_reduce-resources.md) design doc.

## Specification

- A window with focus and on screen draws only when something changes. With the cursor parked it costs a fraction of a percent of a core.

- An unfocused window's cursors park, so it doesn't draw any frames while nothing changes.

- A minimized window, a covered window where the desktop reports it, and a tab that isn't shown don't draw anything. They never stop reading their programs' output, and they catch up in one cut when shown.

- "Free resources when idle" on the Window tab, on by default, lets an unused window give its GPU device back.
	- A window counts as unused when there isn't any input, focus change, or output from any pane while it can be seen. A pane in a tab in the back can't be seen.
	- It lets go after "Minutes when hidden", 1 by default, if minimized or covered, and after "Minutes otherwise", 240 by default, if only unfocused.
	- On Windows with Transparency on, a window in view never lets go, because nothing would be left on screen without its device. It waits until it's minimized.
	- Any sign of life takes the device back. That means a key, a click, the pointer moving over it, a change of focus, or a shell printing in the tab in front while it can be seen. Being shown again counts only for a window that would be blank without its device.
	- When the desktop asks a released window for a repaint, it takes the device back only to draw that frame, and lets it go again once the frame with the wallpaper is up. The wait isn't started over.
	- It never lets go while a dialog is open or a hardware rating is due or running.

- The window title says "(resource conservation mode)" while released, "(restoring resources ...)" until the wallpaper is back, then "(resources restored)" for 5 seconds.

- After a return from a text console on Linux, every window rebuilds its GPU device, at once and again 3 seconds later.

- Large image buffers go back to the operating system once freed.

- A program can't grow SilkTerm's memory without limit. Window titles are cut to 2 KiB, and the title stack takes at most 4096 entries.

## Goals

- Many windows open for days, at no cost while unused.

- No visible difference when a window comes back into use.

- A window that is on screen never gives anything up, because someone might be reading it.

### Non-goals

- Lowering the cursor's frame rate. Smooth motion needs at least 30 frames a second. Parking the cursor removes the cost instead.

- Releasing a window that is only behind others under a compositing desktop, which never reports it as covered.

## Design

### Fewer frames

- The idle cost started at about a tenth of a CPU core and a fifth of a mid-range GPU for one window without anything running. A pulsing cursor kept a 30 frames a second loop running, and every frame rebuilt the whole scene to move one small rectangle. That was 2 full text passes plus the whole scrim pipeline.

- A frame whose text didn't change now reuses the prepared text and the scrim's halo. See the [Speed](20260930-150643_speed.md) design doc.

- A parked cursor doesn't draw any frames. Every pane other than the focused pane of the active window is parked, so an unfocused window's panes all park and no frames are drawn. See the [Smooth cursor](20260930-145124_smooth-cursor.md) design doc.

- A fully covered window waits instead of drawing, and catches up in one frame when it comes back. Not every window manager reports this, so nothing else depends on it.

- A minimized window and a hidden tab freeze their rendering but never their reading. Coming back is an instant cut. The scroll detectors start over rather than easing the new output, or the bounce class of bug comes back. A tab that got 2000 lines while hidden shows up at the bottom without any motion.

- Idle panes don't touch any memory per frame, so the OS can page them out.

- Each freeze is behind its own source constant, `FREEZE_MINIMIZED` in `app.rs` and `FREEZE_UNFOCUSED_BLINK` in `pane.rs`, so a surprise side effect can be rolled back by changing one line.

### Letting the GPU go on a long idle

- It's on by default. A window that has been unused lets its GPU device go, with everything uploaded to it, and takes it back the moment it's used again. The shells keep running and the grid keeps up. Only drawing stops. The case it's for is many windows open for days, each with a device, a swapchain, 2 glyph atlases, the scrim's textures, and a wallpaper the whole time.

- Unused means there isn't any input, focus change, or output from any pane while the window can be seen. Output into a hidden window doesn't count, or a program printing in a minimized window would keep the device for good. Output from a tab in the back doesn't count either, and doesn't bring a released window back, because it doesn't change anything on screen. There are 2 waits, both in minutes on the Window tab. A shorter one is for a window that is minimized, or covered where the desktop reports it. A longer one is for a window that is only unfocused, because that one might be on a second screen being read. A window with focus and on screen never lets go.

- Any sign of life brings it back, which means a key, a click, the pointer moving over it, a change of focus, or a shell printing. Output into a hidden window doesn't bring it back. The wake waits for the reveal, the way the frozen-window rule already works. Output in the tab in front triggers a wake at the reveal, so the window shows what changed once it can be seen. A tab in the back is out of sight before and after, so its output doesn't trigger anything.
	- Focus counts only as a change. On X11, switching back to the desktop that has an unfocused window sends it a focus-lost event, even though it never had focus (2026111010491300).
	- Being shown again isn't a sign of life by itself, unless the window would be blank without its device, as on Windows with Transparency on.
	- X11 doesn't keep a window's pixels while it's on another desktop, so switching back to that desktop triggers a repaint request. Without a device the window would come back black. So a repaint takes the device back for one frame. The device is rebuilt, the window draws and waits for the wallpaper, and then the device is let go again. That takes about a tenth of a second on llvmpipe plus the wallpaper's decode. The wait isn't started over, because there isn't any sign that someone is there. A sign of life in that time makes it a whole wake, and so does output in the tab in front from while it was hidden (2026111010491300).
	- The alternative was to keep a copy of the last frame in memory and put it back with X11's plain 2D drawing, without any device. It was turned down (JC, 20261110).
	- Only a pointer that moves counts, by the rule the look after a move uses. An enter event doesn't. On X11, when any other program releases a pointer grab, such as a menu, a window drag, or a screen locker, the window under a resting pointer gets an enter event. Counting that kept the wait from running out on a desktop, and took a released device back when nobody was there (2026110609500001).

- It's kept off while a dialog is open, because on X11 the dialog's context can't outlive the terminal's, and while a hardware rating is due or running.

- What's kept is what a rebuild starts from, which is the wgpu instance, and on X11 the GL framebuffer config the window was made with. The instance is kept rather than made fresh, because a GL instance's teardown ends an EGL display the glutin context might share, and because on the other backends the adapter list in it is the slow part of a cold start.

- The dialogs' warm context keeps its instance and adapter the same way and lets only its device go. On NVIDIA, every Vulkan instance destroyed left 2 descriptors open.

- The fonts and metrics stay, because layout and input still need them. The rasterized glyphs, the shaped chrome, the scrim's textures, and the wallpaper go with the device. The wallpaper is decoded again on the way back, as after a console switch.

- A small copy of the wallpaper is kept through the release. It's 160 pixels on its long edge, which is about 56 KiB at 16:9. The rebuilt window draws it stretched and smoothed from its first frame until the real one is prepared again. Before that, a window taking its device back showed the bare background for 1.3 to 3 seconds at 2560x1440, and longer for a bigger photo.

- The release unbinds the GL context before destroying it. Otherwise the window keeps its old GLX surface, and NVIDIA refuses a second one. A GLX error is claimed before winit can keep it, and logged as not fatal. winit's IME focus calls expect its one error slot to be empty, and a stale GLX error there was taking whole windows down at the next focus change.

- Code that a frame calls must not read `self.gpu` while the device is released.

- On X11 there isn't any visible side effect, by design. The window keeps showing its last frame, and taking the device back takes about 25 ms.

- On Windows with Transparency off the window keeps its last frame too. With Transparency on it draws through a composition visual and doesn't have a surface of its own, so letting the device go leaves it black. That window is let go only while minimized.

- A minimized window on Windows reports a size of zero, and during a restore it stops reporting minimized a moment before the real size comes back. A window without any size counts as hidden, so nothing is drawn or rebuilt in that gap. A rebuild there took 1x1 as the window's size, and the console host's reflow at 2 columns lost the screen. Coming back into view is a sign of life by itself, because Windows doesn't send occlusion events and its repaint can come inside the gap.

- The window title shows the state. It's "(resource conservation mode)" while released, "(restoring resources ...)" until the wallpaper is back, because the device itself is restored too fast to see, and then "(resources restored)" for 5 seconds. Any rebuild shows it, including a return from a text console, and it's added to a `--title` too, because it's news about the window rather than part of its name.

### After a text console

On Linux, a switch to a text console and back wipes the contents of the textures the renderer samples every frame, such as the glyph atlas and the wallpaper, while the GL context survives. The driver hides the purge from any readback, so the damage can't be detected.

So the switch is detected instead. A watcher notes the console the window started on, through `/sys/class/tty/tty0/active`. When the value changes back to it after being elsewhere, the window lets its whole device go and builds it again, the same way the idle release does. It does that again 3 seconds later, after the X server has set the mode. With a dialog open it uses the older partial rebuild of the glyphs and the wallpaper, because the dialog's context can't outlive the window's.

`~/silk_vramdbg.on` turns on a log of the probes and heals in `~/silk_vramdbg.txt`, live, without a relaunch. The log is capped at 4 MB, and clean probes aren't logged.

### Memory

- glibc lets its mmap threshold rise with each large buffer freed. After that, a wallpaper's decode is carved out of the worker thread's arena and stays resident there once freed, and `malloc_trim` never shrinks an arena that isn't the main one. Every window kept the first decode's 50 MB for life, and each rebuild kept 40 MB more. The threshold is pinned at 4 MB at startup, so an image buffer comes from the OS and goes back to it. Launch memory dropped by about 60 MB with a wallpaper. `MALLOC_ARENA_MAX=1` is the way to check a suspected arena leak.

- A wallpaper is cut down to 4096 pixels a side before the RGBA copy, and its decode is capped at 512 MiB. See the [Wallpaper](20260930-150052_wallpaper.md) design doc.
	- Since 2026-10-05 it's then stored at the size the window draws it at. See the [Reducing resources](20261004-182255_reduce-resources.md#the-wallpaper-at-window-size) design doc.

- The scrim's full-screen textures are made only when needed.

- The dialogs' GPU context is about 52 MiB, and it's kept for the life of the process so Settings opens in a quarter of the time. See the [Settings dialog](20260930-145721_settings-dialog.md) design doc.
	- On 2026-10-04 it measured about 200 MiB of graphics memory and 16 MiB of regular memory. See [Measure first](20261004-182255_reduce-resources.md#measure-first) in the Reducing resources design doc.

- When the glyph atlas fills during a long, varied session, the atlas is trimmed on the failure path too, so the next frame prepares again with room.

- The color glyph cache only discards images that no recent frame used.

- Window titles are cut to 2 KiB, in the engine fork and in SilkTerm, and the title stack takes at most 4096 entries, which is 8 MiB at most. The parser's buffer for its longest sequence was left alone.

- On Windows, 4 handles per pane are closed in the engine fork. The stock engine leaked about 200 for every 50 panes opened.

## Alternative ideas

### Rejected

- Dropping the uploads and keeping the device. The device and its context are the fixed cost the feature exists to remove, and the uploads are the smaller half.

- Disabling the idle release under transparency. The X11 GL path survives the teardown, since the ARGB visual belongs to the window and a new context on the kept config binds to it.

- A lower idle cursor frame rate. Backlog: "Epic 1n6fydv: Reduce CPU and GPU resource usage", item 2.3.

- Freezing inactive windows unless they have output. It was folded into parking unfocused panes, because a visible window with output has to keep drawing.

- Detecting a console switch by reading back a test texture. Probes read back intact over 3 tries while the screen was black. Backlog: "Severe - VT bug".

### Superseded

- Rebuilding only the glyphs and the wallpaper after a console switch. Each thing added to the device since then was one more thing a switch could leave spoiled. The whole device is rebuilt now. Backlog: "After a crash in VSCodium required switching to VT-1".

- "Temporarily free resources when idle" as the row's label. It's "Free resources when idle".

- Any output restarting the idle clock. Only output while the window can be seen does. Backlog: "The feature that is supposed to release the GPU after a timeout, doesn't seem to be doing anything".

- The release was off by default at first. It's been on by default since before RC1. Backlog: "Free resources when idle: on by default".

## Research findings

- Letting the device go takes about 3 ms and taking it back about 25 ms, without any CPU use while released. That was measured on Linux under software GL only.

- On Windows with an RTX 2060, letting the device go takes about 0.1 to 0.2 s and taking it back 0.8 to 1.2 s, plus the wallpaper. That was measured on Vulkan and on the composited DX12 path.

- Idle went from 26.4% of a core to 14.5% once unchanged frames reused their text, and a parked cursor costs about 0.2% against about 14% with the pulse running. A minimized window with busy output went from about 83% of a core to about 0%.

- NVIDIA's video memory purge is hidden from any readback, so a texture that was wiped reads back intact.

## Roadmap

- The GPU release hasn't been measured under the NVIDIA driver on Linux, on Wayland, or on macOS. It isn't known what a released window shows on Wayland and macOS.
	- 20261005: Under sway, on Wayland and through Xwayland, a released window keeps its last frame. On an X server without a compositor a minimized window doesn't have anything to show, and a restore shows the empty window until the first frame. macOS is still not known.

- The warm dialog context's 52 MiB is now tracked in the [Reducing resources](20261004-182255_reduce-resources.md) design doc.

- `~/silk_vramdbg.txt` is at its cap, and has to be moved aside before the next console switch can be logged.

## Related backlog issues

- "Release the GPU device after a long idle." (Opened 20260905-181131)

- "The feature that is supposed to release the GPU after a timeout, doesn't seem to be doing anything" (Opened 20260917-164802)

- "Two terminals running for around 24 to 48 hours, disappeared the moment they got focus" (Opened 20260918-110145)

- "Show in window title, if GPU and CPU savings are in effect" (Opened 20260918-112508)

- "After a crash in VSCodium required switching to VT-1" (Opened 20260917-164802)

- "Free resources when idle: on by default" (Opened 20261003-124705)

- "With Free resources when idle on, a window on screen but not focused is let go, goes black, and stays black until typed into" (Opened 20261003-191345)

- "After waking from Free resources when idle, the window sometimes shows only the prompt's last character until typed into" (Opened 20261003-191345)

- "Severe - VT bug" (Opened 20260722-100516)

- "Keep the old picture on screen while a let-go window takes the card back" (Opened 20261005-135818)

- "When switching virtual desktops (on regular non-VM GPU-acellerated Linux), Silkterm sometimes won't repaint"

- "Terminal is sometimes completely black after coming back from a long session" (Opened 20260630-110459)

- "Epic 1n6fydv: Reduce CPU and GPU resource usage" (closed 20260731-111951)

## Copyright and license

> Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]\
> Licensed under the [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/) (CC BY 4.0).\
> This document is provided as is, without warranty of any kind.
