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

<!-- /TOC -->

## Summary

A terminal is often left open for days, many at a time. SilkTerm is built so that a window nobody is looking at costs nothing: it draws no frames, its cursor stops, and, if asked, it gives its GPU device back until it is used again. Memory it no longer needs goes back to the system, and nothing a program sends can make it grow without limit.

What a window costs while in use is in the [Reducing resources](20261004-182255_reduce-resources.md) design doc.

## Specification

- A window with focus and on screen draws only when something changes. With the cursor parked it costs a fraction of a percent of a core.

- An unfocused window's cursors park, so it draws no frames at all while nothing changes.

- A minimized window, a covered window where the desktop reports it, and a tab that is not shown draw nothing. They never stop reading their programs' output, and they catch up in one cut when shown.

- "Free resources when idle" on the Window tab, on by default, lets an unused window give its GPU device back.
	- A window counts as unused with no input, no focus change and no output from any pane while it can be seen.
	- It lets go after "Minutes when minimized", 1 by default, if minimized, after "Minutes when hidden", 30 by default, if covered, and after "Minutes otherwise", 240 by default, if only unfocused.
	- On Windows with Transparency on, a window in view never lets go, since nothing would be left on screen without its device. It waits until it is minimized.
	- It takes the device back on any sign of life: a key, a click, the pointer entering, focus, being shown, a shell printing while it can be seen, or the desktop asking for a repaint.
	- It never lets go while a dialog is open or a hardware rating is due or running.

- The window title says "(resource conservation mode)" while released, "(restoring resources ...)" until the wallpaper is back, then "(resources restored)" for five seconds.

- After a return from a text console on Linux, every window rebuilds its GPU device, at once and again three seconds later.

- Large image buffers go back to the operating system once freed.

- A program cannot grow SilkTerm's memory without limit. Window titles are cut to 2 KiB, and the title stack takes at most 4096 entries.

## Goals

- Many windows open for days, at no cost while unused.

- No visible difference when a window comes back into use.

- A window that is on screen never gives anything up, since someone may be reading it.

### Non-goals

- Lowering the cursor's frame rate. 30 frames a second is the floor for smooth motion. Parking the cursor removes the cost instead.

- Releasing a window that is only behind others under a compositing desktop, which never reports it as covered.

## Design

### Fewer frames

- The idle cost started at about a tenth of a CPU core and a fifth of a mid-range GPU for one window with nothing running. A pulsing cursor kept a 30 frames a second loop alive, and every frame rebuilt the whole scene, two full text passes plus the whole scrim pipeline, to move one small rectangle.

- A frame whose text did not change now reuses the prepared text and the scrim's halo. See the [Speed](20260930-150643_speed.md) design doc.

- A parked cursor draws no frames. Every pane other than the focused pane of the active window is parked, so an unfocused window's panes all park and no frames flow. See the [Smooth cursor](20260930-145124_smooth-cursor.md) design doc.

- A fully covered window waits instead of drawing, and catches up in one frame when it comes back. Not every window manager reports this, so nothing else depends on it.

- A minimized window and a hidden tab freeze their rendering, never their reading. Coming back is an instant cut: the scroll detectors start over rather than easing what arrived, or the bounce class of bug comes back. A tab that took 2000 lines while hidden arrives at the bottom with no motion.

- Idle panes touch no memory per frame, so the OS can page them out.

- Each freeze is behind its own source constant, `FREEZE_MINIMIZED` in `app.rs` and `FREEZE_UNFOCUSED_BLINK` in `pane.rs`, so a surprise side effect rolls back one line.

### Letting the GPU go on a long idle

- On by default. A window that has sat unused lets its GPU device go, with everything uploaded to it, and takes it back the moment it is used again. The shells run on and the grid keeps up. Only drawing stops. The case is many windows open for days, each holding a device, a swapchain, two glyph atlases, the scrim's textures and a wallpaper the whole time.

- Unused means no input, no focus change and no output from any pane while the window can be seen. Output into a hidden window does not count, or a program printing in a minimized window would keep the device for good. There are two waits, both in minutes on the Window tab. A shorter one is for a window that is minimized, or covered where the desktop reports it. A longer one is for a window that is only unfocused, since that one may be on a second screen being read. A window with focus and on screen never lets go.

- It comes back on any sign of life: a key, a click, the pointer entering, focus, a hidden window being shown, a shell printing, or the desktop asking for a repaint. Output into a hidden window does not bring it back. That waits for the reveal, the way the frozen-window rule already works. A released hidden window is owed its device at the reveal, so a desktop that says nothing about showing it again cannot leave old text up.

- It is kept off while a dialog is open, since on X11 the dialog's context cannot outlive the terminal's, and while a hardware rating is owed or running.

- What is kept is what a rebuild starts from: the wgpu instance, and on X11 the GL framebuffer config the window was made with. The instance rather than a fresh one, because a GL instance's teardown ends an EGL display the glutin context may share, and because on the other backends the adapter list it keeps is the slow part of a cold start.

- The dialogs' warm context keeps its instance and adapter the same way and lets only its device go. On NVIDIA, every Vulkan instance destroyed left two descriptors open.

- The fonts and metrics stay, since layout and input still need them. Gone with the device: the rasterized glyphs, the shaped chrome, the scrim's textures, the minimap's texture and the wallpaper, which is decoded again on the way back, as after a console switch.

- The release unbinds the GL context before destroying it. Otherwise the window keeps its old GLX surface, and NVIDIA refuses a second one. A GLX error is claimed before winit can keep it, and logged as not fatal. winit's IME focus calls expect its one error slot to be empty, and a stale GLX error there was taking whole windows down at the next focus change.

- Nothing a frame calls may read `self.gpu` while it is released.

- On X11 there is no visible side effect by design. The window keeps showing its last frame, and taking the device back is about 25 ms.

- On Windows with Transparency off the window keeps its last frame too. With Transparency on it draws through a composition visual and has no surface of its own, so letting the device go leaves it black. That window is let go only while minimized.

- A minimized window on Windows reports a size of nothing, and a restore stops answering minimized a moment before the real size comes back. A window with no size counts as hidden, so nothing is drawn or rebuilt in that gap. A rebuild there took 1x1 as the window's size, and the console host's reflow at two columns lost the screen. Coming back into view is a sign of life by itself, since Windows sends no occlusion events and its repaint can come inside the gap.

- The window title says so. "(resource conservation mode)" while released, "(restoring resources ...)" until the wallpaper is back, since the device itself returns too fast to see, then "(resources restored)" for five seconds. Any rebuild shows it, a return from a text console included, and it goes on a `--title` too, since it is news about the window rather than part of its name.

### After a text console

On Linux, a switch to a text console and back wipes the contents of the textures the renderer samples every frame, such as the glyph atlas and the wallpaper, while the GL context survives. The driver hides the purge from any readback, so the damage cannot be detected.

So the switch is detected instead. A watcher notes the console the window started on, through `/sys/class/tty/tty0/active`. When the value returns to it after being elsewhere, the window lets its whole device go and builds it again, the same way the idle release does. It does it a second time three seconds later, after the X server has set the mode. With a dialog open it keeps the older partial rebuild of the glyphs and the wallpaper, since the dialog's context cannot outlive the window's.

`~/silk_vramdbg.on` turns on a log of the probes and heals in `~/silk_vramdbg.txt`, live, with no relaunch. The log is capped at 4 MB, and clean probes are not logged.

### Memory

- glibc lets its mmap threshold rise with each large buffer freed. After that, a wallpaper's decode is carved out of the worker thread's arena and stays resident there once freed, and `malloc_trim` never shrinks an arena that is not the main one. Every window kept the first decode's 50 MB for life, and each rebuild kept 40 MB more. The threshold is pinned at 4 MB at startup, so an image buffer comes from the OS and goes back to it. Launch memory dropped by about 60 MB with a wallpaper. `MALLOC_ARENA_MAX=1` is the way to check a suspected arena leak.

- A wallpaper is cut down to 4096 pixels a side before the RGBA copy, and its decode is capped at 512 MiB. See the [Wallpaper](20260930-150052_wallpaper.md) design doc.

- The scrim's full-screen textures are made only when needed.

- The minimap's cache is about 5 MB per pane at the default scrollback, and is freed while the map is off.

- The dialogs' GPU context is kept for the life of the process, about 52 MiB, to open Settings in a quarter of the time. See the [Settings dialog](20260930-145721_settings-dialog.md) design doc.

- When the glyph atlas fills during a long, varied session, the atlas is trimmed on the failure path too, so the next frame prepares again with room.

- The color glyph cache discards only images no recent frame touched.

- Window titles are cut to 2 KiB, in the engine fork and in SilkTerm, and the title stack takes at most 4096 entries, 8 MiB at most. The parser's buffer for its longest sequence was left alone.

- On Windows, four handles per pane are closed in the engine fork. The stock engine leaked about 200 for every 50 panes opened.

## Alternative ideas

### Rejected

- Dropping the uploads and keeping the device. The device and its context are the fixed cost the feature exists to remove, and the uploads are the smaller half.

- Disabling the idle release under transparency. The X11 GL path survives the teardown, since the ARGB visual belongs to the window and a new context on the kept config binds to it.

- A lower idle cursor frame rate. Backlog: "Epic 1n6fydv: Reduce CPU and GPU resource usage", item 2.3.

- Freezing inactive windows unless they have output. Folded into parking unfocused panes, since a visible window with output should keep drawing.

- Detecting a console switch by reading back a test texture. Three rounds of probes read back intact while the screen was black. Backlog: "Severe - VT bug".

### Superseded

- Rebuilding only the glyphs and the wallpaper after a console switch. Each thing added to the device since was one more a switch could leave spoiled. The whole device is rebuilt now. Backlog: "After a crash in VSCodium required switching to VT-1".

- "Temporarily free resources when idle" as the row's label. It is "Free resources when idle".

- Any output restarting the idle clock. Only output while the window can be seen does. Backlog: "The feature that is supposed to release the GPU after a timeout, doesn't seem to be doing anything".

- The release shipped off by default. It is on by default since before RC1. Backlog: "Free resources when idle: on by default".

## Research findings

- About 3 ms to let the device go and about 25 ms to take it back, with no CPU at all while released. Measured on Linux under software GL only.

- On Windows with an RTX 2060, about 0.1 to 0.2 s to let the device go and 0.8 to 1.2 s to take it back, plus the wallpaper. Measured on Vulkan and on the composited DX12 path.

- Idle went from 26.4% of a core to 14.5% once unchanged frames reused their text, and a parked cursor costs about 0.2% against about 14% with the pulse running. A minimized window with busy output went from about 83% of a core to about 0%.

- NVIDIA's video memory purge is hidden from any readback, so a texture that was wiped reads back intact.

## Roadmap

- The GPU release has not been measured under the NVIDIA driver on Linux, on Wayland or on macOS. What a released window shows on Wayland and macOS is not known.

- The warm dialog context's 52 MiB moved to the [Reducing resources](20261004-182255_reduce-resources.md) design doc.

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

- "When switching virtual desktops (on regular non-VM GPU-acellerated Linux), Silkterm sometimes won't repaint"

- "Terminal is sometimes completely black after coming back from a long session" (Opened 20260630-110459)

- "Epic 1n6fydv: Reduce CPU and GPU resource usage" (closed 20260731-111951)
