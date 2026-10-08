<!-- markdownlint-disable MD007 -- Indent count -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Glossary

Words SilkTerm uses in its settings, its menus, and its own documentation that are either specific to this program or easy to misread. Written for someone using SilkTerm, and for a developer who has just opened the source for the first time.

Terminal jargon that any terminal shares is left out unless SilkTerm gives it a particular meaning.

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Alt screen](#alt-screen)
- [Anchor](#anchor)
- [Automask mix](#automask-mix)
- [Bare window](#bare-window)
- [Build number](#build-number)
- [Cell](#cell)
- [Color picker](#color-picker)
- [Contrast mask](#contrast-mask)
- [Copy on output](#copy-on-output)
- [Copy on select](#copy-on-select)
- [DIP](#dip)
- [Dogfood build](#dogfood-build)
- [Ease-in, ramp-up, ramp-down, ease-out](#ease-in-ramp-up-ramp-down-ease-out)
- [Falloff](#falloff)
- [Fit](#fit)
- [Flyover help](#flyover-help)
- [Focus](#focus)
- [Function](#function)
- [Glyph](#glyph)
- [Group](#group)
- [Gutter](#gutter)
- [Highlight](#highlight)
- [Keep open](#keep-open)
- [Layout tags](#layout-tags)
- [Look tags](#look-tags)
- [Max silk](#max-silk)
- [Minimap](#minimap)
- [Minimum contrast](#minimum-contrast)
- [Output easing](#output-easing)
- [Pane](#pane)
- [Palette](#palette)
- [Performance profile](#performance-profile)
- [PTY](#pty)
- [Read-only](#read-only)
- [Revert](#revert)
- [Scrim](#scrim)
- [Scrollback](#scrollback)
- [Shell integration](#shell-integration)
- [Single-screen speed](#single-screen-speed)
- [Softness](#softness)
- [Sub-group](#sub-group)
- [Tab strip](#tab-strip)
- [Text colors from the wallpaper](#text-colors-from-the-wallpaper)
- [Text outline](#text-outline)
- [Theme and mode](#theme-and-mode)
- [Wallpaper](#wallpaper)
- [Wheel lines](#wheel-lines)

<!-- /TOC -->

## Alt screen

A second, scrollback-free screen that a full-screen program switches to while it runs, so the shell history underneath is untouched when it exits. `less`, `vim` and `nano` all use it. SilkTerm scrolls differently on it, because there is no history to ease through.

## Anchor

Which part of a wallpaper image stays in frame when the image and the window are different shapes and the image is zoomed rather than stretched. An image can have its own anchor as an embedded tag.

## Automask mix

How much of the contrast masking is left to the picture itself rather than to the Strength setting. See *contrast mask*.

## Bare window

A view mode that hides the window frame, the menu bar and the tab strip together, leaving only panes. Toggling it back restores whichever of the three were showing before.

## Build number

A short code baked into every build, printed by `--version` and shown in Help > About. It counts whole minutes since the year 2000 and sorts in build order. Two builds of the same release version are told apart by it.

## Cell

One character position in the terminal grid. The grid is a fixed number of cells wide and tall, and every measurement of the text - the cursor, the scroll offset, a selection - is in cells rather than pixels.

## Color picker

The box a color chip opens on the Themes tab. A square sets saturation and brightness, the strip beside it sets the hue, and six boxes read and take the same color as percentages and as a hex value. Changes show on the row behind it as they are made, and Cancel puts the old color back.

## Contrast mask

A wallpaper treatment that quiets the image only where it would fight the text, leaving the rest of it alone. Its three settings are Size (how large an area each point is judged against), Strength (how far it may push the image down) and Automask mix.

The term itself comes from the early days of photography, and involves applying a blurred and inverted image as a luminosity mask, to reduce contrast.

## Copy on output

A per-pane switch. Everything the running program prints is copied to the clipboard as it appears.

## Copy on select

A per-pane switch. Releasing the mouse over a selection copies it, with no further keystroke.

## DIP

Device-independent pixel: a CSS pixel, one ninety-sixth of an inch. Every measurement in SilkTerm's chrome is written in DIP and multiplied by the display's scale factor at the last moment, so a setting means the same physical size on a normal display and a high-DPI one.

## Dogfood build

A copy of a release build that SilkTerm's own developer runs day to day, installed by the pipeline into a small pool of dated copies. The launcher runs the newest one it can find.

## Ease-in, ramp-up, ramp-down, ease-out

The four shape controls for smooth scrolling, in the order one burst of output unfolds: the view leaves a standstill, picks up speed, brakes as the output runs out, and settles on the final line. All four read the same direction, higher being faster or crisper.

## Falloff

The curve the text scrim fades along as it moves away from the glyphs. Separate from *function*, which decides the halo's shape rather than its fade.

## Fit

What to do when a wallpaper image and the window are different shapes. Stretch distorts the image to fill the window; Zoom keeps its proportions and crops. An image with its own Fit tag overrides the setting.

## Flyover help

The small box of explanation that appears when the pointer rests on a control. One rest delay is shared by every tip in the program, so nothing answers faster than anything else. Only controls whose label does not already explain them have one.

## Focus

The theme color that marks the one element the keyboard is on. Deliberately separate from *highlight*, and more vivid.

## Function

How the text scrim's halo shape is computed from the glyphs. Four methods are offered; they differ in quality and in cost.

## Glyph

The drawn shape of one character. One character can need more than one glyph, and a glyph can be wider than one cell, which is why fallback fonts and emoji get their own handling.

## Group

A titled section within a tab of the Settings dialog, separated by a rule and clear space.

## Gutter

Chrome that holds no interactive element - the strip the Settings dialog's tabs stand on. It is a theme color of its own, recessed against the panel in both dark and light mode.

## Highlight

The theme color that marks several things at once: the live pane's ring, slider handles, revert icons, the default button. It stays calm because it is everywhere. See *focus*.

## Keep open

A launch option that holds a pane open after its shell exits, saying how it ended and waiting for a key, instead of closing straight away.

## Layout tags

Fit and Anchor stored inside a wallpaper image file as XMP metadata, so an image has its own best placement instead of relying on a single global setting. *Look tags* are the same idea for Opacity and Blur.

## Look tags

Opacity and Blur stored inside a wallpaper image file. When SilkTerm is set to honor them, they replace the two sliders for that image, so a tagged pack looks the way it was tuned to look.

## Max silk

The performance profile with every effect at its shipped setting: smooth scrolling at the default feel, the wallpaper, the scrim, the cursor animation. The other profiles take things away from it. See *performance profile*.

## Minimap

A column beside the text showing the whole scrollback in miniature, as colored strokes rather than readable characters. The lit band marks what is on screen; drag it to scroll, or click elsewhere in the column to go there. It takes the space it uses, so switching it off gives those columns back to the terminal.

## Minimum contrast

A floor on how close text may come to the background it sits on. Text below the floor is lightened on a dark background and darkened on a light one. Text set to exactly the background color is left hidden, since that is how a program hides text on purpose.

## Output easing

Scrolling that eases when a program prints, rather than jumping a line at a time. The view chases the newest line rather than snapping to it, and the speed of the chase is what the four ease and ramp settings shape.

## Pane

One shell running in one rectangle of the window. A tab starts with one pane and splits into more; each pane has its own shell, its own scrollback and its own working directory.

## Palette

The set of colors a theme defines: the terminal background, foreground and cursor, the two attention colors, the chrome colors, and the sixteen standard ANSI colors.

## Performance profile

One setting on the Silk tab that decides how much the look may cost. Custom uses your own values. Max silk, High, Low and Standard terminal each set the scrolling feel, the scrim, the cursor animation and the wallpaper for you, and the rows they set show the profile's values while one is chosen. Your own values stay in the file underneath, so choosing Custom puts them back. Those rows still take input: change one and the profile becomes Custom, keeping the values that were on screen. With "Choose automatically" on, a new graphics adapter starts at Max silk, or Low under software rendering, and the profile steps down one level whenever the display cannot keep up with a scroll. A step goes no lower than Low and lasts until SilkTerm restarts. Picking a profile by hand switches "Choose automatically" off, since the pick would otherwise be overwritten at the next launch; Remote (temporary) is the exception, because it lasts this session only.

## PTY

Pseudo-terminal: the pair of file handles a terminal and its shell talk through. The shell believes it is talking to hardware; the terminal reads what it writes and turns it into a grid of cells.

## Read-only

A per-pane switch that stops keystrokes reaching the shell. The pane still shows output and can still be scrolled and copied from.

## Revert

The small arrow at the right edge of a Settings row. It puts the shipped default back for that one setting.

## Scrim

A soft halo drawn behind text so it stays readable over a busy wallpaper. Its radius, softness, shape and fade curve are all settable, and it is separate from the crisp *text outline* drawn hard against the glyphs.

## Scrollback

The lines that have scrolled off the top of the screen and are kept for reading back. It has a fixed depth; once it is full, the oldest line is dropped for every new one.

## Shell integration

A small block SilkTerm can add to a shell's own startup file so the shell reports where it is. That is what lets a new tab or split open in the directory the pane was actually sitting in, rather than where the pane started.

## Single-screen speed

The top speed the view will ease at while the output still fits on one screen. Longer bursts are governed by the ramp settings instead.

## Softness

How gradually the text scrim fades out. Low softness makes a hard-edged halo; high softness makes a diffuse one.

## Sub-group

A run of Settings rows that belong to the row above them, shown by indenting their labels while every control stays in its column. The leading row is usually the switch that decides whether the rest do anything.

## Tab strip

The row of tabs under the menu bar. Each tab is as wide as its own label needs, and the tab in front takes whatever the row can spare, so it spells its label out while the others fall back to a shorter form. When they no longer fit, the strip pages rather than shrinking them to nothing.

## Text colors from the wallpaper

A switch on the Themes tab that takes the text and cursor colors from the picture behind them rather than from the theme. The text goes as far from the picture's brightest areas as the screen allows, in a hue complementary to the picture's own, and the cursor follows it a third of the way around the color circle. While it is on, the Foreground and Cursor rows gray out and keep showing the colors that come back when it goes off. Nothing picked this way is saved: it follows whatever picture is up.

## Text outline

A crisp one-to-four pixel outline drawn hard against the glyphs, on top of the scrim. Cheaper and harder-edged than the scrim, and useful with it or without it.

## Theme and mode

A theme is a pair of palettes, one dark and one light. The mode picks which of the pair is in use: Dark, Light, or System, which follows the desktop.

Four themes ship: SilkTerm, Matrix, Retro Amber and Pastel.

Change a color and the Theme dropdown reads `[unsaved]`, since what is on screen is no longer the theme it was. Save folds the change into the theme and the name comes back. Picking the theme again instead puts its own colors back.

## Wallpaper

A background image drawn behind the terminal text, dimmed and blurred to taste. A folder can be given instead of a file, in which case a different image is chosen each time.

## Wheel lines

How many lines the view travels per notch of the mouse wheel.
