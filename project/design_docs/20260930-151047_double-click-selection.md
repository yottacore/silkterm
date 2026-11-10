<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Double-click and selection

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Summary](#summary)
- [Specification](#specification)
- [Goals](#goals)
	- [Non-goals](#non-goals)
- [Design](#design)
	- [What a double-click grabs](#what-a-double-click-grabs)
	- [Pairs and words](#pairs-and-words)
	- [Selecting past the edge of the screen](#selecting-past-the-edge-of-the-screen)
	- [Four ways to copy](#four-ways-to-copy)
	- [Copy on output](#copy-on-output)
	- [Paste](#paste)
- [Alternative ideas](#alternative-ideas)
	- [Rejected](#rejected)
	- [Superseded](#superseded)
- [Research findings](#research-findings)
- [Roadmap](#roadmap)
- [Related backlog issues](#related-backlog-issues)
- [Copyright and license](#copyright-and-license)

<!-- /TOC -->

## Summary

A double-click in SilkTerm tries to grab what a person meant, which is a whole path or URL, the text inside a pair of quotes or brackets, or else a word. A triple-click takes the whole line. A drag selects, and scrolls when held past the edge. A selection can be copied to the clipboard in 4 ways, each with its own rule.

## Specification

- A single click doesn't place anything and clears a selection. A drag selects from cell to cell. Holding Ctrl at the press selects a block instead, or Command on macOS.

- A double-click checks 3 things in order, and the first match wins:
	- Is it a pattern SilkTerm recognizes? URLs, file URIs, drive paths, UNC paths, absolute Unix paths, `~/` paths, git remotes, and scp targets are taken whole.
	- Is it inside a pair of quotes or brackets on the same line? The contents are taken, with spaces trimmed off both ends.
	- Otherwise it takes a word, which ends at the word separators.

- A path with a space in a folder name stays whole. A line number after a file name, such as `:120:5`, is left off. A trailing period, comma, or bracket is dropped.

- A triple-click takes the whole line, including the rows it wrapped onto. A fourth click goes back to one.

- A drag held past the top or bottom of the pane scrolls and keeps selecting. It starts slow and speeds up the further out and the longer it is held, up to a cap.

- A drag that strays into a neighboring pane stays in the pane it started in.

- The word separators (`selection.word_separators`) and the pairs (`selection.pairs`) are config settings. Neither is in the Settings dialog.

- There are 4 ways to copy, each with its own rule:
	- Right-click Copy always copies.
	- Ctrl+Shift+C, or Command+C on macOS, copies even while the window's focus is uncertain.
	- Copy on select copies when a drag that SilkTerm itself saw ends. It sets the clipboard and, on Linux, the primary selection.
	- A program can set the clipboard with an escape sequence, but only from the pane in use.

- Copy on output puts each finished command's output on the clipboard, without the prompt. Only the focused pane of the focused window copies.

- Middle-click pastes the primary selection.

## Goals

- A double-click grabs what a reader sees as one thing, even when it has spaces or brackets in it.

- Copy works every time, from every route, and a fix to one route doesn't break another.

- A window, tab, or pane that isn't in use doesn't put anything on the clipboard.

### Non-goals

- Pairs that span lines. A double-click inside a pair works on one line only.

- Re-copying a selection a full-screen program drew itself. That highlight is the program's, and SilkTerm doesn't have a selection of its own there to copy.

## Design

### What a double-click grabs

- A double-click is 2 clicks in the same cell within the desktop's double-click time. The same counter gives the triple-click.

- A double-click checks 3 things in order, and the first match wins. It checks for a known pattern, then a matched pair, and then a word. Word selection was the only rule for a long time, and it can't handle a path with a space in it, because a space is what ends a word.

- The patterns are URLs and file URIs, drive paths (`C:\...`), UNC paths, absolute Unix paths, and `~/` paths. Each has to start at an anchor a reader would recognize, with only whitespace, a quote, or an opening bracket in front of it. That rule was picked over "anything that isn't obviously a word", which would treat `and/or` as a path.

- Git remotes and scp targets are patterns as well, in the `[user@]host:path` form. They were added because a git prompt writes the remote inside brackets beside its status marks, and the matched-pair rule then returned the marks along with it. Narrowing the pair rule was considered and rejected, because selecting a quoted phrase whole was asked for separately. A host needs a dot and an alphabetic ending, and the path needs a separator and a letter in its first segment, so `build:release/x` and `notes.txt:12/34` don't match.

- A remote is the one pattern that doesn't end at a file extension. A prompt writes the branch after the repository as `repo.git:dev`, and that whole field is what a reader sees as one thing. If the match stopped at `.git`, a double-click on the branch would select the brackets instead.

- There are 2 rules for where a path ends, and both were picked for what they rule out. A space is crossed only when a path separator comes up within the next 40 characters, so a folder name with spaces stays whole, but a path followed by a sentence doesn't take in the sentence. The match also stops at a file extension, which is what leaves a `:120:5` line number behind.

- A trailing period, comma, or bracket is dropped the same way it is for a link. Paths and links use the same trimming idea but not the same code, because a path can have characters a URL can't.

### Pairs and words

- A double-click inside a matched pair on one line selects what's between them rather than the pair itself. The first enclosing pair in the order backtick, `"`, `'`, `{}`, `()`, `[]`, `<>` wins, so a click inside `()` takes the `()` contents even with `[]` nested inside.

- Space runs are trimmed against the delimiters. If the inside is nothing but spaces, the whole span is taken.

- A `'` with a letter or digit on both sides is an apostrophe, and is neither end of a pair. That rule handles `don't`, `O'Brien`, and the `b'` Python prints in front of bytes.

- A bracket's partner is searched at most 200 rows away, once, at the click. An unmatched bracket is selected on its own.

- Outside any pair, the engine's semantic word selection runs, with `selection.word_separators` as its escape characters. The default leaves out `:`, so a drive path and a URL select whole.

- A control character, a tab included, draws as a one-cell space, so a word selection on tab-indented output lines up with what is on screen.

### Selecting past the edge of the screen

- A drag held past the top or bottom of its pane scrolls the view that way and keeps selecting, so a selection can run further than what fits on screen. A pointer outside the pane is pulled to the nearest edge cell rather than ignored, which also means a drag that strays into a neighboring pane still belongs to the one it started in.

- The speed is the larger of 2 values. One is how far past the edge the pointer is, and the other is how long it has been held there. Distance alone is the obvious rule and the one that feels right, but a maximized window has its top edge against the top of the screen, so there isn't anywhere left to push the pointer. That window could only ever creep. Holding gets to the same top speed in 2 seconds.

- It creeps rather than standing still right at the edge, because picking up one more line is the common case and a drag that starts fast overshoots it. The top speed is capped, so a pointer flung off the screen doesn't cross the whole buffer before the button comes up.

### Four ways to copy

Copy kept breaking because there are 4 ways to copy, each with its own gate, and every fix used to change only one of them.

- Right-click Copy doesn't have a gate.

- Ctrl+Shift+C, or Command+C on macOS, copies even while the window's focus flag is false. It doesn't type anything, so it can't be the stray arrow key that the focus gate is there to stop.

- Copy on select runs only after a drag SilkTerm saw itself. A program that tracks the mouse takes the drag, so SilkTerm doesn't have anything to copy, and that program copies for itself through the next route. It sets both the clipboard and the primary selection. `shell.copy_on_select` keeps it on across launches.

- A program's clipboard request (OSC 52) is honored only from the pane in use. Other panes and tabs are ignored. On Linux it also sets the primary selection.

On X11, owning the selection has one more trap. When another program takes the selection, a stale clear can arrive for a copy that has already been replaced, and the stock clipboard crate then dropped the newer text. SilkTerm uses a patched `x11-clipboard` that keeps the value. See the [terminal engine](20260930-151451_alacritty-fork.md) design doc for how patched crates are managed.

### Copy on output

- When it's on, the focused pane's output is copied to the clipboard as each command finishes, as plain text with colors and control codes removed. A command without any output leaves the clipboard alone.

- The prompt rows are left out. The rows a multi-line prompt draws above its input line are learned from the previous command. A row has to have more than one bare word before it counts as prompt.

- Only the focused pane of the active tab in the focused window copies, so a background window can't leak output. A pending capture is canceled when its window, tab, or pane stops being the one in use.

- On Windows a console doesn't have a foreground process group, so a command is treated as a live child process of the shell while it runs.

- Copy on select and copy on output are independent, and both can be on. A new pane takes its tab's setting.

### Paste

- A paste isn't the raw clipboard bytes. The line endings and bracketed paste are worked out in `paste_payload`, so a multi-line paste works on Windows and bracketed paste can't be closed from inside the pasted text.

- Middle-click pastes the primary selection, bracketed when the program has turned that on.

## Alternative ideas

### Rejected

- Treating anything that isn't obviously a word as a path. It would treat `and/or` as a path.

- Narrowing the pair rule to fix the git prompt. Selecting a quoted phrase whole was asked for.

- Asking the X server who owns the selection before writing. It hung the event loop. Backlog: "A clipboard write can leave SilkTerm owning the selection with nothing behind it".

- Re-copying a selection a mouse-tracking program drew. The program offers its text only once.

### Superseded

- Pair first, then word. Pattern now comes before pair. Backlog: "When double-clicking to select stuff backwards and forwards to defined delimiters".

- Copy on select and copy on output as one-or-the-other, one pane at a time, turning themselves off on a pane change. They're independent now. Backlog: "Copy on select and copy on output, as two checkboxes on the menu bar".

- Flat `word_separators` and `selection_pairs` keys. They're `selection.word_separators` and `selection.pairs`.

## Research findings

- Under a stock `x11-clipboard`, 19 of 900 tries lost a copy. With the patch, 0 of 900 did.

- A drag held past the edge picked up 133 and 127 lines in 3 seconds at 24 rows, and didn't cost any extra CPU once pinned.

## Roadmap

- A double-click on a Windows drive path hasn't been rechecked on Windows against a fresh config. Backlog: "Double-clicking a Windows path leaves off the drive letter".

- A PowerShell background job reads as a command still running, so copy on output waits until it ends. Windows doesn't give any way to tell a background child from a foreground one.

## Related backlog issues

- "Double-clicking selects a word up to user-tweakable delimiters" (closed 20260629-214404)

- "When double-clicking to select stuff backwards and forwards to defined delimiters" (closed 20260629-214404)

- "When double-clicking to select text, if the rule about quotes and brackets is in effect" (Opened 20260628-083740)

- "Triple-click: Select the entire line - even if it's wrapped" (Opened 20260705-110255)

- "A double-click cut paths and URLs short at the first bracket or space" (Opened 20260826-123553)

- "Double-clicking a Windows path leaves off the drive letter" (Opened 20260826-123553)

- "Double-clicking on "github.com:jim-collier/silkterm.git:dev" (Opened 20260901-183000)

- "Bug in double-click to select (then Ctrl+shift+C)" (Opened 20260706-170614)

- "Selecting text all the way to the bottom of the screen - or all the way to the top - no longer auto-scrolls" (Opened 20260919-154614)

- "The copy-to-clipboard bug is back" (Opened 20260909)

- "A clipboard write can leave SilkTerm owning the selection with nothing behind it" (Opened 20260919-145949)

- "CTRL+shift+C is not working consistently" (Opened 20260905-175000)

- "Option to copy all output (`stderr` and `stdout`) to desktop clipboard automatically" (Opened 20260702-170007)

- "Copy on select and copy on output, as two checkboxes on the menu bar" (closed 20260713-013515)

- "Ability to select text by partial lines, with left mouse button"

- "Ability to select text with in a grid-aligned rectangle, with CTRL+left mouse button"

- "Copy & paste selected text to current cursor location, via middle mouse button"

## Copyright and license

> Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]\
> Licensed under the [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/) (CC BY 4.0).\
> This document is provided as is, without warranty of any kind.
