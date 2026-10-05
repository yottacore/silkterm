<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Reducing resources

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Measure first](#measure-first)
	- [A smaller scrim](#a-smaller-scrim)
	- [The wallpaper at window size](#the-wallpaper-at-window-size)
	- [Block compression for the wallpaper](#block-compression-for-the-wallpaper)
	- [The dialogs' kept GPU context](#the-dialogs-kept-gpu-context)
	- [Software rendering](#software-rendering)
	- [The Resource use group](#the-resource-use-group)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)

<!-- /TOC -->

## Summary

One SilkTerm window holds about 330 MB of graphics memory, and many windows add up fast. Most of it is a few full-window textures that are bigger than they need to be. This doc covers making them smaller, a way to run without the graphics card at all, and a clearer place in Settings for what a window gives back.

What a window already gives back while unused is in the [Releasing resources](20260930-151334_releasing-resources.md) design doc. This doc is about what a window costs while it is in use.

## Specification

- The scrim, the wallpaper and the dialogs' kept context each cost a fraction of what they do now, with no visible change at the same settings.

- The wallpaper is held at the size it is drawn at, and is prepared again in the background after the window is resized.

- "Always use software rendering" makes SilkTerm draw on the CPU. Off by default.

- When the graphics card cannot make a device, SilkTerm falls back to software rendering rather than failing.

- The idle release and software rendering rows sit together in a "Resource use" group. A row that cannot work on the running platform is grayed with a tip that says why.

## Goals

- Many windows open at once on a card with little memory.

- No visible change. Legibility comes first, since the scrim is what makes text readable over the wallpaper.

- A window always opens, whatever the card can give it.

### Non-goals

- Lossless compression of anything on the GPU. The drivers already compress render targets on their own, and it saves bandwidth, not memory.

- Moving textures to regular memory. Anything drawn every frame has to be on the card.

## Design

### Measure first

- The 330 MB figure is a reading, and the split below is an estimate from the texture code. Nothing gets changed until it is measured.

- Read one real window's use from the driver at a few sizes, with the scrim on and off, the wallpaper on and off, and Settings opened once.

- The estimate, for a 2560x1440 window:

	| Part                             | Estimate
	| :------------------------------- | :-----------------------------
	| Scrim, five full-window textures | About 150 MB
	| Swapchain                        | 30 to 45 MB
	| Wallpaper, up to 4096 a side     | 15 to 64 MB
	| Dialogs' kept context            | About 52 MiB, once per process
	| Driver's own cost per process    | Often 50 to 100 MB on NVIDIA

- Regular memory gets the same look: scrollback, the minimap's store, the glyph caches.

### A smaller scrim

- The scrim holds five textures the size of the window, each four 16-bit floats, so 8 bytes a pixel and 40 in all.

- Most of them use one channel:
	- The text and cursor coverage layers use only alpha. One byte a pixel should do.
	- The two blur layers hold either blurred coverage or a distance. One 16-bit float each should do. Eight bits may band in a dark, soft halo, so test both.
	- The background color map uses all four channels. Its precision is worth testing too.

- From 40 bytes a pixel to about 10 to 16, before any change in size.

- The blur passes could run at half size, which is a quarter of the pixels. The blur already steps more than one pixel per tap at larger radii, so the loss may not show.
	- If it shows, Max silk keeps full size and the lower profiles use half size.
	- If it never shows, every profile uses half size.

- The text coverage is drawn by the text renderer, which writes color. A one-channel target needs the coverage pass to write white, and the shaders to read the red channel.

- Proof is a capture diff at the same settings, in all four themes, dark and light, at a few radii. Legibility is the bar, not a pixel count.

### The wallpaper at window size

- Today the image is cut to 4096 on its long side, blurred on the CPU at that size, and uploaded whole. The GPU scales it to the window every frame.

- Prepare it at the size it is drawn at instead, by its fit. Scale, never crop, so the anchor and fit still work during a resize.

- After a resize, keep drawing the current texture, scaled. Once resizing has been still for a short wait, the worker prepares the image at the new size and swaps it in with no fade.

- The blur is in image pixels today. The new size changes that, so the blur is scaled with the image to keep today's look at the same window size.

- A blurred image needs less than the window's size. Detail finer than the blur is gone anyway, so the image can be held smaller by a factor tied to the blur, and the GPU scales it up.
	- At the shipped blur, that may cut it by four or more with no encoder at all.
	- A cap keeps a small blur from shrinking it too far.

### Block compression for the wallpaper

- GPUs read block-compressed formats directly. BC1 is half a byte a pixel and BC7 one byte, against four for plain RGBA.

- They work only for textures that do not change. A GPU cannot draw into one, so this is for the wallpaper only.

- The image to compress is the one already prepared at window size and blur.

- Pick by blur and size:
	- A heavy blur: hold it smaller and skip compression. A quarter-size image in plain RGBA is already smaller than BC1 at full size.
	- Little or no blur: BC1. The wallpaper sits dimmed behind text, so its artifacts should not show.
	- BC7 where BC1 shows banding, if a test finds any.

- Encoding runs on the wallpaper worker once per image and per resize. A pure Rust encoder is preferred.

- A plain RGBA fallback is kept for an adapter without BC support, though every one checked has it.

- Executable size is a top priority, so the encoder's cost in bytes decides as much as its quality.

### The dialogs' kept GPU context

- Settings and About draw through a GPU context built once and kept for the life of the process. It holds about 52 MiB to open Settings in 86 ms rather than 310 ms. See the [Settings dialog](20260930-145721_settings-dialog.md) design doc.

- With many windows, that is 52 MiB each, all the time, for a dialog that is rarely open.

- Options:
	- Drop it when the dialog closes, and keep it only while a dialog is open.
	- Share the main window's device instead of a second one, where the backend allows. That also saves a second driver context. On X11 the dialog's GL context cannot outlive the window's, so this needs care there.
	- Keep it, and say so.

- Moved here from the releasing resources doc's roadmap.

### Software rendering

- SilkTerm uses a software adapter only when no graphics card is found at launch. A card that is found but cannot make a device, as when its memory is full, ends the launch, and an idle rebuild retries the same card.

- "Always use software rendering", off by default. It asks wgpu for the fallback adapter: lavapipe or llvmpipe on Linux, WARP on Windows.
	- The performance profile follows it. Automatic already starts a software adapter at Low.
	- macOS has no software adapter in wgpu. The row is grayed there, with a tip.
	- The X11 transparency path uses its own GL context, and libGL picks its driver once per process. Whether a launch can steer it to Mesa's software driver needs checking. If not, the row is grayed under Transparency on X11.
	- A change takes effect at the next device build, which a rebuild can trigger, rather than only at relaunch.

- The fallback: when a device cannot be made, at launch or at a rebuild, try the software adapter once before giving up.
	- A window that fell back stays on it until the next rebuild, which tries the card first again.
	- The title or Help > About says software rendering is in use, the same way it does now.
	- This is also a way out for the blank window under GPU load, where `vkCreateDevice` was seen failing.

### The Resource use group

- "Free resources when idle" and its two waits move into a group named "Resource use", with "Always use software rendering" beside them.

- "Free resources when idle" gets a warning mark. Its tip says it matters most on a card with little memory, or next to GPU-heavy programs, with many windows open that are not all in view. Turn it off only if the graphics driver has trouble with it.

- A setting that stops a window from giving memory back gets a warning mark where it does so. Today that is Transparency on Windows, since a window in view with Transparency on is never let go. The mark shows only in the Windows build.

- A row that cannot work on the running platform is grayed with a tip saying why, using the existing grayed-row tips. A row is hidden only in a build that can never use it, as with the Windows-only file association rows.

- Tips stay short, per the UI style guide.

## Alternative ideas

### Rejected

- Lossless compression on the GPU. No graphics API lets a program hold a texture in less space losslessly.

- An encoder with a C toolchain, such as `intel_tex_2`. A pure Rust one builds for every target with no extra setup.

- Block compression for the scrim or swapchain. A GPU cannot render into a compressed format.

## Research findings

- Block compression support, checked on b23 with Mesa 26.1.2:
	- lavapipe reports `textureCompressionBC`.
	- llvmpipe's GLES has `GL_EXT_texture_compression_s3tc_srgb`, `_rgtc` and `_bptc`, the three wgpu wants before it turns BC on for GLES.
	- WARP: D3D12 requires the BC formats on every device, WARP included.
	- Metal: wgpu turns BC on where Metal reports it. Not checked on b26.

- Pure Rust encoders: `texpresso` does BC1 to BC5, and `rusty_dds` does BC1 to BC7. Neither is measured here for speed, quality or binary size.

- The scrim's blur takes 25 taps a pass and spaces them more than a pixel apart once the radius passes about 4 pixels.

- The wallpaper's default blur is a sigma of 10, in pixels of the image after the 4096 cut.

## Roadmap

1. Measure.

2. Shrink the scrim and hold the wallpaper at window size. Settle the dialogs' context.

3. "Always use software rendering" and the fallback on device failure.

4. The Resource use group and its warning marks.

5. Block compression for the wallpaper, once the size cut has been measured.

## Related backlog issues

- "Measure graphics and regular memory per window" (ID 2026100418225501)

- "The scrim's textures are bigger than they need to be" (ID 2026100418225502)

- "Hold the wallpaper at the size it is drawn at" (ID 2026100418225503)

- "Always use software rendering, and fall back to it when the card cannot make a device" (ID 2026100418225504)

- "The dialogs' kept GPU context costs every process about 52 MiB" (ID 2026100418225505)

- "Settings: a Resource use group, with warning marks" (ID 2026100418225506)

- "Block compression for the wallpaper" (ID 2026100418225507)

- "The window doesn't paint while the GPU is busy or short on memory, and stays blank after the load ends" (ID 2026100312470535)

- "Release the GPU device after a long idle." (Opened 20260905-181131). Its open point, "figure out a way to reduce CPU and memory usage", moved here.
