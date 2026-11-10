<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Unicode, fonts and emoji

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [Shaping with fallback](#shaping-with-fallback)
	- [Fitting glyphs to the grid](#fitting-glyphs-to-the-grid)
	- [Picking the family](#picking-the-family)
	- [Color emoji](#color-emoji)
	- [Engine limits](#engine-limits)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)
- [Copyright and license](#copyright-and-license)

<!-- /TOC -->

## Summary

A terminal is a grid of cells, and fonts don't know that. SilkTerm picks one monospace family for every weight, falls back glyph by glyph to other installed fonts for anything that family lacks, and fits every such glyph to the cells the terminal gave it. Color emoji are painted by SilkTerm itself, because the text library can't read the current color font format.

## Specification

- Every character draws. A character the terminal font lacks comes from another installed font. If no installed font has a character, the cell shows whatever the system's last resort draws, and the fix is to install a font that has it.

- Text stays on the grid. Every character from any font is drawn in the cells the terminal gave it, which is one cell for a narrow character and 2 for a wide one.

- Regular, bold, and italic come from one monospace family, so a bold word is exactly as wide as the regular one.

- Color emoji draw in color. `text.color_emoji` turns that off.

- A character Unicode presents as text, such as a check mark or the copyright sign, draws in the cell's text color, never in an emoji font's own colors.

- A glyph from another font is drawn on the same baseline as the text beside it.

- The font is found from one search order, the same on every platform:
	- `font.family`, a comma-separated list. While "Use system font" is on, it's the desktop's monospace family followed by the built-in list, or the built-in list alone where the desktop doesn't name one;
	- then the desktop's monospace family;
	- then a built-in list;
	- then whatever the generic monospace name finds, only if nothing above is installed.

- Every family in the built-in list has a real bold face.

- The font size is automatic too until it's set, and follows the desktop's fixed-width size. Ctrl+Minus, Ctrl+Plus, and Ctrl+0 change it a pixel at a time. On macOS it's Command with the same keys.

- A cell takes at most 9 combining marks.

- A control character, a tab included, draws as a single blank cell.

- A pane is always at least 2 columns wide, so a wide character always fits.

## Goals

- CJK, emoji, math symbols, and right-to-left scripts draw instead of empty boxes, with the grid intact.

- The same build and config pick the same font on every platform.

- No setting for fallback fonts. Other terminals and editors don't have one, and there isn't anything to tune.

### Non-goals

- Proportional text. Every character has to fit the grid.

- Matching weights across fonts. Marks from 2 different fonts can differ a little in weight, and which fonts are involved depends on the machine.

- A bundled font. The installed fonts are used.

## Design

### Shaping with fallback

Pane text is shaped with per-glyph font fallback (`Shaping::Advanced`), so a character the terminal font lacks comes from another installed font while the monospace alignment stays. Measuring the cell uses the plain path (`Shaping::Basic`), because it only needs the terminal font. An earlier version of the text library hung on real output with fallback on. The version in use has a bounded fallback loop and was stress-tested.

If the terminal font doesn't have a character, the character goes through `fill_glyph`, which shapes it on its own and hands the pane a buffer to place. That causes 2 problems that nothing else in the renderer has to handle.

- The fallback face has its own baseline. Its glyph is shifted onto the baseline of the text beside it.

- A fallback emoji face can contain a character that Unicode presents as text. That character isn't painted by the emoji face, so it takes the cell's color. Real emoji are unaffected.

A color emoji face rasterizes to an empty image through the text library. So an empty raster is taken as "this face draws nothing", and the glyph is shaped again through the generic monospace chain, which picks a face that does rasterize. The color path below then paints the real emoji.

Each fallback glyph is shaped once per pane, keyed by character, bold, and italic, and tinted per cell. That took fallback work from about 16% of CPU on a full screen to 0.2%.

### Fitting glyphs to the grid

- The cell width is the text's real rendered pitch, without rounding. Everything placed on the grid, from the cursor and cell backgrounds to fallback glyphs, is positioned by multiplying that width by the column. A rounded width drifted by a fraction of a pixel per column, so the cursor was drawn further past the text the longer the line got.

- A character uses the shared row layout only when the font's own width for it matches the number of columns the terminal gave it. A monospace font is free to include a double-width character at single width, and the default font does that for 53 of them, including several common emoji and fullwidth punctuation. When one of them was laid out from the font, it took one column where the grid gave it 2, and everything after it on the row drew a column to the left. A character whose widths disagree is drawn on its own and fitted to its box, the same way as characters missing from the font. So those emoji are drawn in color too.

- A glyph drawn on its own is scaled and centered in its cell box, so an over-wide fallback can't spill onto its neighbor. It's clipped to the pane's content.

- A control character, a tab included, draws as a one-cell space so the row stays on the grid.

### Picking the family

One monospace family is pinned for every weight, because the shaper picks the best face per query and would otherwise let a bold run end up in a different family than the regular run beside it. Bold text uses the boldest weight the pinned family has, so it can't fall back to a proportional bold font.

The search order in the Specification is the same on every platform. Until 2026-10-09 the "Use system font" switch put the desktop's family ahead of `font.family`. Now the switch makes `font.family` itself the desktop's family followed by the built-in list, and a list set by hand is set aside until the switch goes off, so there isn't anything left to reorder. An earlier version dropped `font.family` entirely while following the desktop font. The same build and the same config then picked different fonts depending on the platform, and a configured list could be silently ignored. Every list is now always walked. A family that isn't installed falls through to the next one, and the configured list still works as a fallback.

Platforms differ only in what they report, rather than in the rules applied to it. Windows has a system font size but doesn't have a monospace family, so the automatic family there is the built-in list, without a special case. The same goes for a desktop without any font setting, which is why the rule checks what was detected rather than which platform is running.

A size the desktop reports in points becomes logical pixels at 96 per inch, except on macOS. A Mac point already is a logical pixel, so it's used as is. The Mac's fixed-width size comes from AppKit, which returns its default when the size hasn't been set.

On Linux the desktop's own settings store is read first, and the other one fills in the gaps. That's xfconf on Xfce and gsettings elsewhere. gsettings returns values on any box with GNOME's schemas installed, including an Xfce box, and a key that was never set comes back as the schema default. So reading it first on Xfce gave Cantarell 11 and Monospace 11, whatever the desktop was set to.

The built-in list is last for a reason. The generic monospace query after it picks more or less at random from the installed fonts, and the font it picks might not have a bold face. That throws bold runs into an arbitrary, often proportional, fallback whose advances can't be snapped to the cell grid. Every entry in the built-in list has a real bold face. They're Monaspace Argon, Fira Code, JetBrains Mono, Cascadia Mono, Consolas, Ubuntu Mono, SF Mono, Menlo, and Courier New. When that list changes, the outgoing value is recorded, so an existing config that still has the old list word for word is refreshed on the next launch. A list that the user edited is left alone.

The automatic size follows the desktop's size on Windows too. A `font.size` set by hand overrides it. The size hotkeys change the size for the window, since all panes share one set of text metrics.

### Color emoji

The text library reads only the older color glyph table (COLR v0), and every current color emoji font has only the newer one (COLRv1). So SilkTerm paints color glyphs itself, in `coloremoji.rs`. It walks the paint graph and renders it through a small 2D back end, which handles transforms, clip and layer stacks, solid, linear, radial, and sweep fills, and Porter-Duff and blend compositing. The result is stored in the renderer's color atlas as a per-cell image fitted to the cell box. Characters without a color glyph aren't affected and take the monochrome fallback path.

The color glyph cache keeps images by glyph and pixel size. It only discards images that no recent frame has used, and if everything in it is still in use, it grows. Its size is bounded by what fits on screen. An earlier cache emptied itself completely when full, and threw away images the frame being drawn still needed.

The text and color glyph caches are cleared together, and a missing raster is skipped rather than stopping the frame.

### Engine limits

- The engine keeps up to 9 combining marks per cell. That change is in the engine fork until upstream releases it. See the [terminal engine](20260930-151451_alacritty-fork.md) design doc.

- A pane is at least 2 columns wide, which is the engine's documented minimum, so a wide character always has room.

- An RTL override character is kept in a window title, because refusing it would also refuse the joiners that tie multi-part emoji together.

## Alternative ideas

### Rejected

- Settings for fallback fonts. There isn't anything a person would tune.

- Batching fallback glyphs into one draw. It isn't worth it for 1.6% of a frame. See the [Speed](20260930-150643_speed.md) design doc.

- "Use system font" as inert on Windows. Only the family half is inert. The size half works.

### Superseded

- The plain shaping path for pane text. Per-glyph fallback replaced it. Backlog: "Some unicode glyphs don't render".

- The desktop's monospace family, else generic monospace, as the whole default. The one search order and the built-in list replaced it. Backlog: "Use system monospace font by default" and "The font fallback stack is only partly implemented".

- Pinning the monospace advance on the text buffer. The measured, unrounded cell width replaced it. Backlog: "There are weird spacing issues with the cursor".

- Emoji as monochrome outlines. They are painted in color now. Backlog: "Graphical emoji render as monochrome outlines instead of color".

- A heavy check mark beside a light cross in the prompt. It's the light pair now. Backlog: "The '✘' an '✓' on the git prompt look weird in powershell".

## Research findings

- The default font has 53 double-width characters at single width.

- A screen filled with fallback symbols is cheaper than ordinary text, 10.9% of a core against 24.7%, because each leaves a blank placeholder in the shaped row.

- Other terminals show the same emoji fonts in color because their rasterizers read COLRv1.

- gsettings returns schema defaults on an Xfce box, so it has to be read second there.

## Roadmap

- The fallback order hasn't been checked on Windows with a fresh config. Backlog: "The font fallback stack is only partly implemented".

- A font size per pane. All panes in a window share one set of text metrics, so it needs a renderer per pane. Backlog: "Hotkeys to increase and decrease font size".

## Related backlog issues

- "Some unicode glyphs don't render, most likely due to inadequate font coverage rather than a bug" (closed 20260629-214404)

- "Use system monospace font by default" (closed 20260629-214404)

- "There are weird spacing issues with the cursor" (closed 20260629-214404)

- "Graphical emoji render as monochrome outlines instead of color" (closed 20260727-014507)

- "The font fallback stack is only partly implemented, and resolves differently per platform for the same build" (Opened 20260727-014507)

- "Crash: a screen filled with distinct emoji aborts the terminal" (closed 20260728-074118)

- "Bug: Editing a line at any point on the prompt, that has one or more emojis in it" (Opened 20260802-002500)

- "Windows: font, scrolling and virtual-workspace problems" (Opened 20260721-130036)

- "The '✘' an '✓' on the git prompt look weird in powershell"

- "When the terminal is completely is full of text, it's slows noticeably" (Opened 20260708-191010)

- "Hotkeys to increase and decrease font size" (Opened 20260722-100516)

- "Font size should be able to be increased, even when using system font" (Opened 20260722-100516)

- "Windows fonts look too small even at 100% scale" (canceled 20260817-120024)

## Copyright and license

> Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]\
> Licensed under the [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/) (CC BY 4.0).\
> This document is provided as is, without warranty of any kind.
