<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# UI/UX style guide

How SilkTerm's own interface is meant to look and behave. It covers the menu bar, the right-click menu, the Settings dialog, the About box, and the chrome around the panes. It says nothing about the terminal grid itself, which is the program's output rather than its interface.

This was written by reading what is already built, then tidying the rules until they stop contradicting each other. Where the code and this file disagree, the file is the intent. See [Known deviations](#known-deviations) for the ones that are known and still open.

For prose, comments, naming and Rust conventions, see [`style-guide.md`](../style-guide.md). That guide governs what gets written about the program; this one governs what the program shows.

## Contents

- [Principles](#principles)

- [Words](#words)

- [Menus](#menus)

- [Settings dialog](#settings-dialog)

- [Buttons and prompts](#buttons-and-prompts)

- [Flyover help](#flyover-help)

- [Layout and measurement](#layout-and-measurement)

- [Color roles](#color-roles)

- [Keyboard](#keyboard)

- [Known deviations](#known-deviations)

## Principles

- A terminal is a tool someone stares at all day. Chrome should be quiet, hold still, and stay out of the way of the text.

- Nothing in the interface may block the first frame. Anything slow runs on a worker and appears when it is ready.

- Prefer deriving a piece of state over storing it. If the file on disk or the thing on screen already implies the answer, compute it.

- A control explains itself by its label wherever it can. Flyover help is for the ones that cannot.

- Never surprise. A control that looks standard behaves the standard way, and a familiar keystroke does the familiar thing or is passed to the shell untouched.

## Words

- Sentence case everywhere. Menu titles, menu items, tab names, headings, labels, buttons. Only proper nouns and product names keep their capitals ("PowerShell 7", "Nushell", "SilkTerm").

- No trailing colon on a label that stands in its own column, because the layout already separates it from its control. A colon is right where a label and its value share one line: the About box's `Renderer: ...`, the tab flyover's table, the menu bar's `Copy on:` lead-in.

- No terminating punctuation on a label or a button caption. The About box's `Support SilkTerm!` is the one exception, and it is deliberate: an ask for money reads badly flattened into a filing label.

- An item that opens a further dialog ends in a single ellipsis character, no space before it: `Settings…`, `About…`, `Save as…`. An item that only asks for confirmation does not.

- Units go on the end of the label, separated by a space, with no brackets: `Opacity %`, `Blur px`, `Rate s`. The value beside the control shows the number alone.

- Keyboard shortcuts shown in a menu go in parentheses at the end of the item, spelled with `+` between every part and no spaces: `Copy (Ctrl+Shift+C)`, `Fullscreen (F11)`.
	- On macOS a row with a Command chord shows that chord instead, with the modifiers in Apple's order: `Copy (Command+C)`, `Fullscreen (Control+Command+F)`. The system menu bar draws the chord itself, so its labels have none.

- Say what a thing is, not what the code calls it. "File or folder", not "Path". "Visibility", not "Alpha". "Handle" and "Track", not "Thumb" and "Trough".

- Avoid jargon that only a developer would recognize. Two exceptions:
	- A term specific to SilkTerm that has no plainer name. Define it in [`glossary.md`](../glossary.md).
	- An option that picks a named technique, where the name is the only accurate label for it. The scrim's `Distance field` and `Half-normal` are of this kind. These go in the glossary too.

## Menus

- Two menus draw from the same entry list and the same renderer: the menu bar's dropdowns, and the right-click menu over a pane.

- The menu bar is File, Edit, View, Tabs, Panes, Help. A new action goes in the menu whose noun it acts on.

- On macOS the same menus go in the system menu bar, after an app menu holding About, Settings…, Hide and Quit. There is no in-window bar there at all, so View has no Menu bar row and `--hide-menu` does nothing. File gains New window. Help is left off, since About was its only row. Each row with an Apple standard shortcut takes its Command chord.
	- A Window menu comes last, with Minimize, Zoom, Show previous tab, Show next tab and Bring all to front. macOS adds the list of open windows under them. Each window is its own process, so that list holds only the one window and its dialogs.

- The right side of the menu bar has the focused pane's two auto-copy checkboxes, so their state is visible without opening anything. It is the only thing on the bar that is not a menu. When the window narrows it sheds its lead-in, then its words, then itself, rather than overlapping the titles.

- On macOS the two auto-copy switches are the Copy on select and Copy on output rows in Edit, checked to follow the focused pane. The system menu bar has no place for a control that is not a menu.

- The right-click menu is the pane's own menu. It is a selection from the bar, not a copy of it: the actions worth reaching without traveling, plus items that only make sense at the pointer, such as the two link actions that appear only when the click was on a link. It has one window-chrome row, Menu bar, because with the bar hidden nothing else can bring it back.

- On macOS the right-click menu has no Menu bar row, since there is no in-window bar to bring back.

- Order within a menu: the most-used action first, related actions adjacent, destructive actions last in their group.

- A separator groups; it does not decorate. Every separator must have a reason a reader could name.

- A toggle names the thing itself and draws a check mark while that thing is on. `Window frame`, not `Hide window frame`. Checked always means present or active, so a column of checkmarks reads one way down. A caption never changes to describe the other state.

- A submenu is for one homogeneous list that would otherwise swamp its parent. The installed shells are the only case today. It opens beside its parent row, never over it.

- Accelerator letters are unique within one menu, and the first match wins, so a letter spent early is spent. A row that cannot get a distinct letter goes without one rather than stealing one.

- Nothing in a menu is disabled and left visible unless its absence would be more confusing than its being gray.

## Settings dialog

The dialog is declared in `settings_ui.shcl`, which is the file to edit when adding or moving a row.

### Tabs

- Tabs run left to right along the top, on the gutter strip, standing on the line that divides that strip from the rows below.

- A tab holds one subject. Seven is about the ceiling; past that, the subject is probably two subjects.

- A tab's first heading may repeat the tab's own name, in which case it draws nothing. It stays in the declarations because a heading is also what assigns the rows under it to a tab.

### Groups and sub-groups

- A group is a titled section within a tab, separated by a rule and by clear space above it. The heading belongs to what follows it, so the space above a heading is larger than the space below it.

- A sub-group is not declared. It is a row followed by rows at a greater indent. The leader is a real control, not a title.

- Only labels indent. Every control stays in its column, whatever the depth.
	- A shared line of toggles is the one exception, under Rows.

- A sub-group's leader is usually the switch that decides whether its members do anything. Members stay live and indented when it is off, so they can be set up first and come back as they were. A switch whose members get values of the program's own while it is on is a group of automatic settings instead, below.

### Rows

- One row edits one setting, and its label names that setting in plain words.

- Two rows may share a line where neither earns one of its own and the two belong together. The upper one keeps the label column, and its label has to name both halves. The lower one is declared `beside`, takes the right half of the control column, and has its own label only if its control does not say what it is. One revert control at the end of the line answers for both.
	- A run of toggles may share one line the same way, as the four Tab text switches do. That line leaves the label and control columns. It starts where the first label would, each label sits right before its own box, the first one's too, and a wider gap comes before the next label. Both gaps grow with the interface font, so a large one keeps each label clear of its box. Where the line shares its group with other rows, as the Silk tab's hardware check line does, its first box stays in the control column under theirs and only the toggles after it pack. Each toggle keeps its own flyover, and the first one's label names only itself, since the rest name themselves. The one revert control at the end of the line puts back all of them.

- Row kinds are: heading, toggle, slider, color, text, radio, dropdown, pair, hotkey, buttons, and shells. The last two are one-offs. A `buttons` row holds no value and acts on the row above it; `shells` is the Shell tab's grid, one declared row that draws a line per stored shell. A new kind needs a reason no existing kind covers.
	- A row or a whole group that only applies to one platform is declared with `windows: true` and left out of every other build. Rows are not grayed for that, since a control that can never work there is noise.
	- A row that cannot work on the desktop it is running on is grayed instead, with a flyover saying why. "Minutes when hidden" is grayed on Wayland, which never tells a window it is hidden.

- A slider has a number field beside it, and the field is the way to enter an exact value.
	- A slider whose range spans orders of magnitude, such as the two idle waits, has a log track, so each doubling gets the same travel.
	- A row with no % and no limit of its own takes a typed number past the slider's end, up to a cap set per row. The handle waits at the end and the field shows the number. Below the slider's start still clamps.
	- Every slider's field is one width, wide enough for the longest number any slider can show in the interface font, so the sliders all end in one column. At an ordinary font that is the floor, and nothing moves.

- A color row has a chip and a hex field, and they are two separate stops. The chip opens the picker; the hex field takes a value that is already known.

- A fraction stored as 0..1 is shown as a whole percent. The file keeps the decimal.

- Every row that holds a value has a revert control at the right edge, which puts the default back. A heading, a `buttons` row and the shells grid hold no single value, so none of them has one.
	- The Windows file-type rows are the exception. Each is a `buttons` row whose Register writes the registry at once, and its revert control puts back what Register replaced. The arrow is lit while the registry names SilkTerm, and its flyover says what it puts back.

- Every row must actually write what it edits. A row whose setting is never persisted is worse than no row, because the change appears to take and then vanishes at the next launch.

- Why a row is grayed out beats what it does, so a row grayed by the machine says so in its flyover in place of its usual text. A row grayed by another setting says nothing extra, because the switch that did it is the row above. A row set by the performance profile is not grayed at all - it takes input, and its flyover says that it is showing the profile's value and that changing it switches the profile to Custom.

- A setting the program can work out for itself is automatic until a value is set, as [Settings with an automatic value](design_docs/20261008-180516_automatic_settings.md) describes.
	- It is never grayed for being automatic. Its box shows the value in use, in a lighter italic, with a small "A" mark at the right end of the box.
	- A value typed, dragged or stepped to is set by hand. The mark turns into an x that puts it back to automatic, and emptying the box does the same. So does the revert control, since the default is automatic.
	- A switch over a group of them is read off the group and holds nothing. It is on while all are automatic, off while none is, and shows a dash while some are. A click on it from a dash makes all of them automatic, and turning it off keeps what each one shows. It has no revert control, since each member has its own. "Remember last size" over Columns and Rows is one.
	- A switch over one setting is pointless, so that setting takes the mark itself. Family and Size on the Text tab follow the desktop's font that way.
	- The theme colors follow the theme the same way, Open command the desktop's opener, and File or folder the usual folder or the built-in picture. A color's mark sits in its hex box, beside the chip that opens the picker. Picking a theme puts every color back to automatic under it.
	- File or folder edits whichever of the two its box shows, and its x puts back only that one. Its revert control puts back both.

- A row that may not work on every desktop has a warning mark after its label: a small triangle in the label's color, never red. What the row depends on goes at the end of the row's flyover, which shows over the mark like the rest of the row.
	- A row that keeps a window from giving its graphics memory back gets one too, as Transparency does on Windows. So does "Free resources when idle", since some graphics drivers have trouble with it.
	- A mark says only what applies where it is shown. Transparency's memory note is in the Windows build alone, since only there is a window in view with Transparency on never let go.

### The color picker

- A chip opens a box over the panel, modal the way a prompt is: a saturation and brightness square, a hue strip down its right side, six value boxes, and Cancel and OK at the bottom right with OK the default.

- The box is titled with the row's own label, so it is plain which color is being chosen.

- The value boxes are Red %, Green %, Blue %, Brightness %, Saturation % and Hex, top to bottom. There is no hue box: the strip is the hue control.

- Changes show on the row behind the box as they are made. Cancel puts back what the row held when it opened.

- The marker on the square is a ring in whichever of black or white can be seen on the color under it. The strip's marker is black and white together, since every point on it is a full-strength color.

- Arrows adjust whatever holds focus, by the same step a number box takes. The square and the strip are also draggable, which is the faster way and not the only way.

## Buttons and prompts

- Footer buttons sit at the bottom right, in the order Cancel, Apply, OK, left to right. OK is the default and is marked as such.

- The footer stands well clear of the rows above it: about twice the gap between two ordinary rows. It is the one place a stray click is expensive, so it must not read as another row.

- Cancel discards every change. Apply commits without closing. OK commits and closes.

- A button caption is a verb or a standard word, never a sentence.

- A prompt asking for text is a small box with one line of instruction, an entry field with its existing value selected, and Cancel and OK at the bottom right.

- The instruction is an imperative naming what to type, with no terminating period: `Enter a name for the new theme`. Two prompts doing the same job word it the same way.

- A confirmation names what is about to happen and quotes the thing by name: `Really delete theme "Matrix"?`

- OK is the default in every prompt, including a confirmation to delete. Nothing SilkTerm deletes is unrecoverable enough to justify making the reader reach for the other button every time.

- A red mark is only for removal. Nothing else in the interface is red.

## Flyover help

There are four of them: a Settings row, a menu item, a link or button in the About box, and a tab in the strip. They share the rest delay, the wrapping and the placement rules, and nothing else. Each is drawn by its own caller, in its own font.

- One rest delay for every tip in the program. A menu that answered faster than the tab strip would read as a different kind of thing.

- A row's tip shows wherever the pointer rests on that row: its label, a warning mark, each option's label and every part of its control. The revert control is the one part with a tip of its own. A pair, such as Visibility's Scrim and Outline on the Cursor tab, is two settings, so each half answers for its own. A packed line of toggles is separate settings too, and each keeps its own tip.

- An automatic setting's tip ends, after a blank line, with one line on its state: `Automatic. Change it to set your own value.` or `Set by hand. Automatic would be: 17.` The x that puts it back says `Back to automatic`.

- Any other row that holds a value ends its tip with `Current value: ...` and `Default value: ...` on 2 lines, after a blank line where text comes before them, but only while the two differ. They are written the way the row writes them: the number in its box, On or Off, the option's own words, a color's hex. An automatic setting gets neither, since its state line already says what automatic would be.

- Only controls whose label does not already say what they do get a tip. The test is the tip itself: if it restates the label in other words, delete it and fix the label. A dialog of rendering settings will legitimately need one on most of its rows, because a name cannot say what a falloff curve or an easing time does to the picture.

- A tip that explains a control is prose: one to three complete sentences, each ending in a period. Two is usually enough, and a third has to answer the obvious follow-on question rather than pad.

- The tab strip's tip is the exception, and it is not prose at all. It reports facts about a tab as an aligned `Key: value` table, in the terminal font, because spaces align nothing in a proportional one.

- A tip's box stands off whatever it hangs from. In the main window that is a warmed lift off the menu color, since the shipped menu background is the inactive tab's own; in the dialogs it is the button shade against the panel. None of it is separately editable: the tip follows the menu or the panel color it is derived from.

- Text wraps to the panel, so a longer sentence or a larger interface font cannot push it off an edge.

- Placement depends on what is being described. A tip for a row goes under the control and flips above it near the bottom edge. A tip for a menu row goes beside the popup, because a box under the row would cover the rows being chosen between.

- A tip never has an action, a link, or anything the pointer has to reach.

## Layout and measurement

- Every measurement is in DIP, a CSS pixel at 1/96 inch, multiplied by the display's scale factor on its way to the screen. Nothing is scaled twice.

- Where that conversion happens differs by surface, and both are right for what they are.
	- The Settings dialog solves its whole layout in DIP and converts once at the boundary. It has a real layout pass, and a stray conversion inside it would be scaled again on the way out.
	- The main window's chrome and the About box convert each constant where it is used. Neither has a layout pass to convert at the end of, and inventing a second coordinate space for a handful of numbers would cost more than it saved.

- Measurements are floors, not fixed sizes. Content that needs more room gets it: a wide label pushes the panel wider, a taller interface font makes rows taller. A number set too small loses to the content; one set too large gives a roomier dialog. Neither can break the layout.

- Chrome sizes off the interface font, not off a constant. Changing the desktop font size must move everything together.
	- A column that holds text is measured in the interface font, with its declared width as the floor: the revert control, the Shell tab's date and Active columns, and the color picker's labels and value boxes. Where the text can't be known ahead, as with a shell's name, the column grows in step with the UI line height above the 11 pt default's, the same as the gap beside a label.

- Text is centered on its visible ink box, not on its line box. Curated single-line labels center on ascender-to-baseline; anything that may have descenders, such as a path, centers on ascent plus descent.

- A focused boxed control draws exactly one outline.

- A pixel-valued setting steps in whole pixels. Only line height keeps decimals.

- The dialog opens at the size its tallest tab wants, or at what the screen leaves, whichever is smaller. The Shell tab is left out of that, since its list grows with each shell and scrolls instead. The Keys tab is left out too, since its list of hotkeys is long and scrolls. The screen's share is the work area, which is what a window can occupy once the taskbar and any docks have taken theirs, less the frame the window manager puts around it. A monitor's full height is not that, and using it is how the footer buttons end up behind a taskbar.

- It can be resized. A resize that passes within a few pixels of the default size settles on it, and the size it is left at is used again for the rest of the session. Nothing about it is written to the config: a new run opens at the default size again.

- Too short and the rows scroll; too narrow and they scroll sideways. The tab strip and the footer buttons stay out of the vertical scroll, so there is always a way out of the dialog and always a way to another tab.

- Controls in the middle of a row are variable width and take whatever the window's width leaves them. What is fixed is what lines up: labels and the left edge of every control align on the left, and the revert control, a slider's number field and a text control's own right edge align on the right.

## Color roles

Twelve colors are editable, and each has one job. All twelve are on the Themes tab. Ten belong to the theme and ship as a dark and light pair; the scrollbar's two do not, and stay neutral whatever the theme, so a saved theme leaves them out.

Terminal:

- `background`, `foreground`, `cursor`.

Chrome:

- `dialog_background`, `dialog_foreground`: the pop-out dialogs.

- `menu_background`, `menu_foreground`: the menu bar and its dropdowns.

- `gutter`: chrome that holds no interactive element, such as the strip the dialog's tabs sit on. Recessed against the panel in both modes.

Attention:

- `highlight`: marks several things at once, including the live pane's ring, slider handles, revert icons and the default button. It stays calm because it is everywhere.

- `focus`: marks the one element the keyboard is on. More vivid than `highlight`, and well away from it in hue.

Scrollbar, outside the theme:

- `scrollbar_thumb`, `scrollbar_trough`: shown in the dialog as Scrollbar handle and Track, at the end of the palette. Their row says they are not part of the theme, since everything above them is. The minimap's marker and its own bar take them too, so they still do something with the scrollbar switched off.

Rules that go with them:

- Do not collapse `highlight` and `focus` back into one color. They answer different questions.

- Chrome defaults are neutral and shared by every built-in theme. A theme may override them, but a terminal palette that repainted the menu bar would fight the desktop.

- Any two colors drawn on each other need a contrast check, not a taste check. A harmonious pair of hues can share a luminance and become unreadable.

## Keyboard

- The shell gets the keystroke unless the interface has a specific claim on it. When in doubt, it goes to the shell.

- Program shortcuts take Ctrl+Shift where the plain Ctrl form belongs to the shell. Plain Ctrl is used only where nothing sensible would want it.
	- Ctrl+Shift+C copy, Ctrl+Shift+V paste.
	- Ctrl+Shift+T new tab, Ctrl+Shift+W or Ctrl+F4 close tab, Ctrl+Shift+N new window.
	- Ctrl+PageUp and Ctrl+PageDown walk the tabs; add Shift to move the tab along.
	- Ctrl+Plus, Ctrl+Minus and Ctrl+0 size the font for this session.
	- Ctrl+, opens Settings. F11 is fullscreen.

- Panes take Alt+Shift chords and Alt+arrows, as in Windows Terminal.
	- Alt+Shift+Plus splits right, Alt+Shift+Minus splits down, and Alt+Shift+W closes the pane.
	- Alt+arrows move to the pane that way. Of two the same distance off, the one the last move came from wins, then the top or left one.

- On macOS the program's chords are Command ones, Apple's standard shortcut where an action has one, as a key and on the menu row. No Ctrl chord is the program's there, so every one goes to the shell.
	- Command+N new window, Command+T new tab, Command+W close tab.
	- Command+C copy, Command+V paste.
	- Command+Shift+[ and Command+Shift+] walk the tabs. So do Command+PageUp and Command+PageDown, and Shift with those two moves the tab along.
	- Command+Plus, Command+Minus and Command+0 size the font.
	- Command+, opens Settings, Control+Command+F is fullscreen, Command+Q quits. Command+H and Option+Command+H hide, and Command+M minimizes.
	- Command+D splits right and Command+Shift+D splits down, and Command+Option+arrows move between panes, as in iTerm2. Option+Command+W closes the pane, since Command+W closes the tab.
	- Command+click opens a link, and Command held at a press selects a block.
	- Ctrl+click is the right-click, so it opens the right-click menu wherever a right-click does.
	- Nothing typed with Command held reaches the shell, so no Command chord takes a key from it.
	- With no in-window bar, Option plus a letter always goes to the shell.

- Alt plus a menu title's first letter opens that menu. The Menu key opens the right-click menu on the focused pane.
	- Only Alt alone does it, and the title underlines show only then. With Shift, Ctrl or Super held too, the letter is some other chord or goes to the shell.

- Every hotkey above can be changed or turned off under `keys:` in the config file, apart from Alt plus a menu title's letter. A menu row shows the chord its hotkey answers to first, so a change shows there too. The Settings dialog's Keys tab lists every one and changes them the same way.

- In an open menu, arrows move, Right enters a submenu, Left leaves one or steps to the next dropdown, Enter picks, Escape closes, and a letter picks the row with it.

- Inside a dialog, Tab and Shift+Tab move focus, Ctrl+Tab and Ctrl+PgUp/PgDn change tab, Enter is OK and Escape is Cancel. That holds with a field open: Enter closes it and takes OK, Escape cancels. Neither takes a second press.
	- A row on the Keys tab waits for a new chord after Enter, Space or a click on its box, and every key goes to it until one comes. Escape leaves the row as it was, and Backspace or Delete on its own turns the hotkey off. A key that would stop typing at the shell is refused, with what it needs held. A chord another hotkey had is said on both rows, the way the launch says it about the file.
	- On macOS Command+Shift+[ and ] change tab, and so do Command+PgUp/PgDn. Ctrl+Tab moves focus like Tab, since Command+Tab belongs to the system.
	- A text box on macOS takes the Mac's own keys: Command+C, X, V and A, Option to move or erase by words, Command+Left and Right for either end, and Command+Backspace to erase to the start.

- A tab being renamed is a text box too, with the same keys as one in a dialog. Its right-click menu has Cut, Copy, Paste, Delete and Select all. While it is up, Copy, Paste and Paste Selection on any menu act on the name.

- A text, color or number box opens with its value selected as soon as focus arrives, by key or by click, so typing replaces it.

- In a number box, Up and Down step the value by a hundredth of its range, or a tenth with Shift held, whether the box is open or merely focused. Left and Right move the caret while it is open, and step the value while it is not.
	- On a log slider the step is that share of the track, so it is the same ratio anywhere along it, and never less than 1 for a whole number.
	- A step never takes a value further past the slider's end than it already is. Up stops at the end, and Down from a typed number steps down from that number.

- Every action reachable by mouse should be reachable by keyboard, and the reverse does not have to hold. Direct manipulation is the standing exception: dragging a divider, reordering a tab or a shell, dragging the minimap marker, and renaming a tab in place have no keyboard equivalent today.

- A key that reaches a dialog never also reaches the terminal underneath.

## Known deviations

Things the built interface does differently from the rules above. Each is a small work item rather than a design question.

- `Paste Selection` keeps a capital S so that the accelerator has a letter to take. Documented as an exception, but a better fix would free a letter elsewhere.

- `Copy on select` sits at the bottom of the Cursor tab, which is not where its subject is. It was asked for there and a test pins it, so it stays until that changes.

- On macOS the auto-copy switches show their state only in the Edit menu, not at a glance as the in-window bar shows them elsewhere. The bar's dimming of them while the window is in the background has no counterpart there either.

- The right-click menu no longer offers Fullscreen, Window frame or Bare window, so with the menu bar hidden they are reachable only by F11 or by putting the bar back. Acceptable while Menu bar stays on that menu, but worth another look if the bar is ever hidden by default.

- `Gaussian [ugly]` says out loud that it is the worse option, which no other control does. It is the baseline the other three scrim functions are compared against, and the label was asked for.

- The About box pads its `Key: value` lines with extra spaces, which line nothing up in a proportional font. Cosmetic, and shared with the text `--about` prints.

- The Silk tab makes eight, one past the ceiling above. Its subject is what the look costs, which is a stretch over three sections: the profile, text readability and the scrolling feel. It was put first because the profile governs most of what is under it. Emptying those sections out left the Text tab holding only the font, and the Movement tab holding only the wheel, the scrollbar and the minimap.

- Foreground and Cursor on the Themes tab still gray out while "Text colors from wallpaper" is on. It is the last switch over rows it gives values of its own, and it has not moved to automatic values yet.

- The performance profile still lays its values over the rows it sets, rather than being a presets dropdown read off them as the automatic settings design has it.

- The Keys tab makes nine. No other tab's subject takes in the hotkeys. The nine tabs are now the widest thing in the dialog, so they set the panel's width on every tab.
