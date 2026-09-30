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

<!-- /TOC -->

## Summary

A double-click in SilkTerm tries to grab the thing a person meant: a whole path or URL, the text inside a pair of quotes or brackets, or else a word. A triple-click takes the whole line. A drag selects, and scrolls when held past the edge. What is selected can go to the clipboard in four ways, each with its own rule.

## Specification

- A single click places nothing and clears a selection. A drag selects from cell to cell. Ctrl held at the press selects a block instead.

- A double-click asks three questions in order, and the first that answers wins:
	- Is it a shape SilkTerm can name? URLs, file URIs, drive paths, UNC paths, absolute Unix paths, `~/` paths, git remotes and scp targets are taken whole.
	- Is it inside a pair of quotes or brackets on the same line? The contents are taken, with spaces trimmed off both ends.
	- Otherwise it takes a word, ended by the word separators.

- A path with a space in a folder name stays whole. A line number after a file name, such as `:120:5`, is left off. A trailing full stop, comma or bracket comes off.

- A triple-click takes the whole line, including the rows it wrapped onto. A fourth click goes back to one.

- A drag held past the top or bottom of the pane scrolls and keeps selecting. It starts slow and speeds up the further out and the longer it is held, up to a cap.

- A drag that strays into a neighboring pane stays in the pane it started in.

- The word separators (`selection.word_separators`) and the pairs (`selection.pairs`) are config settings. Neither is in the Settings dialog.

- Four ways to copy, each with its own rule:
	- Right-click Copy always copies.
	- Ctrl+Shift+C copies, even while the window's focus is uncertain.
	- Copy on select copies when a drag that SilkTerm itself saw ends. It sets the clipboard and, on Linux, the primary selection.
	- A program may set the clipboard with an escape sequence, but only from the pane in use.

- Copy on output puts each finished command's output on the clipboard, without the prompt. Only the focused pane of the focused window copies.

- Middle-click pastes the primary selection.

## Goals

- A double-click grabs what a reader sees as one thing, even when it has spaces or brackets in it.

- Copy works every time, from every route, and a fix to one route does not break another.

- Nothing reaches the clipboard from a window, tab or pane that is not in use.

### Non-goals

- Pairs that span lines. A double-click inside a pair works on one line only.

- Re-copying a selection a full-screen program drew itself. That highlight is the program's, and SilkTerm has no selection of its own there to copy.

## Design

### What a double-click grabs

- A double-click is two clicks in the same cell within the desktop's double-click time. The same counter gives the triple-click.

- A double-click asks three questions in order, and the first one that answers wins: is this a shape we can name, is it inside a matched pair, is it a word. Word selection was the only rule for a long time, and it cannot handle a path with a space in it, because a space is what ends a word.

- The shapes are URLs and file URIs, drive paths (`C:\...`), UNC paths, absolute Unix paths, and `~/` paths. Each has to start at an anchor a reader would recognize, with only whitespace, a quote or an opening bracket in front of it. That was preferred over "anything that is not obviously a word", which reads `and/or` as a path.

- Git remotes and scp targets are shapes as well, in the `[user@]host:path` form. This one was added because a git prompt writes the remote inside brackets beside its status marks, and the matched-pair rule then handed back the marks along with it. Narrowing the pair rule was considered and rejected, since selecting a quoted phrase whole is wanted and was asked for separately. A host needs a dot and an alphabetic ending, and the path needs a separator and a letter in its first segment, which is what keeps `build:release/x` and `notes.txt:12/34` out.

- A remote is the one shape a file extension does not end. A prompt writes the branch after the repository as `repo.git:dev`, and that whole field is what a reader sees as one thing. Stopping at `.git` would leave the branch as a dead patch that selects the brackets instead.

- Where a path ends is two rules, both picked for what they refuse. A space is crossed only when a path separator turns up within the next forty characters, so a folder name with spaces stays whole while a path followed by a sentence does not swallow it. And the run stops at a file extension, which is what leaves a `:120:5` line number behind.

- A trailing full stop, comma or bracket comes off the same way it does for a link. The two share the trimming idea but not the code, since a path may have characters a URL may not.

### Pairs and words

- A double-click inside a matched pair on one line selects what is between them, not the pair itself. The first enclosing pair in the order backtick, `"`, `'`, `{}`, `()`, `[]`, `<>` wins, so a click inside `()` takes the `()` contents even with `[]` nested inside.

- Space runs are trimmed against the delimiters. If the inside is nothing but spaces, the whole span is taken.

- A bracket's partner is searched at most 200 rows away, once, at the click. An unmatched bracket is selected on its own.

- Outside any pair, the engine's semantic word selection runs, with `selection.word_separators` as its escape characters. The default leaves out `:`, so a drive path and a URL select whole.

- A control character, a tab included, draws as a one-cell space, so a word selection on tab-indented output lines up with what is on screen.

### Selecting past the edge of the screen

- A drag held past the top or bottom of its pane scrolls the view that way and keeps selecting, so a selection can run further than what fits on screen. A pointer outside the pane is pulled to the nearest edge cell rather than ignored, which also means a drag that strays into a neighboring pane still belongs to the one it started in.

- The speed is the larger of two answers: how far past the edge the pointer is, and how long it has been held there. Distance alone is the obvious rule and the one that feels right, but a maximized window has its top edge against the top of the screen, so there is nowhere left to push the pointer. That window could only ever creep. The hold reaches the same top speed in two seconds.

- It creeps rather than standing still right at the edge, since picking up one more line is the common case and a drag that starts fast overshoots it. The top speed is capped, so a pointer flung off the screen does not cross the whole buffer before the button comes up.

### Four ways to copy

Copy kept breaking, and the reason is that there are four ways to copy, each with its own gate, and every fix used to reach only one of them.

- **Right-click Copy** has no gate.

- **Ctrl+Shift+C** copies even while the window's focus flag reads false. It types nothing, so it cannot be the stray arrow key the focus gate exists to stop.

- **Copy on select** runs only after a drag SilkTerm saw itself. A program that tracks the mouse takes the drag, so SilkTerm has nothing to copy, and such a program copies for itself through the next route. It sets both the clipboard and the primary selection. `shell.copy_on_select` keeps it on across launches.

- **A program's clipboard request** (OSC 52) is honored only from the pane in use. Other panes and tabs are ignored. On Linux it also sets the primary selection.

On X11, owning the selection has one more trap. When another program takes the selection, a stale clear can arrive for a copy that has already been replaced, and the stock clipboard crate then dropped the newer text. SilkTerm carries a patched `x11-clipboard` that keeps the value. See the Alacritty design doc for how patched crates are carried.

### Copy on output

- When on, the focused pane's output goes to the clipboard as each command finishes, as plain text with colors and control codes removed. A command with no output leaves the clipboard alone.

- The prompt rows are left out. The rows a multi-line prompt draws above its input line are learned from the previous command. A row has to carry more than one bare word before it counts as prompt.

- Only the focused pane of the active tab in the focused window copies, so a background window cannot leak output. A pending capture is canceled when its window, tab or pane stops being the one in use.

- On Windows a console has no foreground process group, so a command is a live child process of the shell while it runs.

- Copy on select and copy on output are independent, and both can be on. A new pane takes its tab's setting.

### Paste

- A paste is not the clipboard's bytes. The line endings and bracketed paste are worked out in `paste_payload`, so a multi-line paste works on Windows and bracketed paste cannot be closed from inside the pasted text.

- Middle-click pastes the primary selection, bracketed when the program has turned that on.

## Alternative ideas

### Rejected

- Treating anything that is not obviously a word as a path. It reads `and/or` as a path.

- Narrowing the pair rule to fix the git prompt. Selecting a quoted phrase whole is wanted.

- Asking the X server who owns the selection before writing. It hung the event loop. Backlog: "A clipboard write can leave SilkTerm owning the selection with nothing behind it".

- Re-copying a selection a mouse-tracking program drew. The program offers its text only once.

### Superseded

- Pair first, then word. Shape now comes before pair. Backlog: "When double-clicking to select stuff backwards and forwards to defined delimiters".

- Copy on select and copy on output as one-or-the-other, one pane at a time, turning themselves off on a pane change. They are independent now. Backlog: "Copy on select and copy on output, as two checkboxes on the menu bar".

- Flat `word_separators` and `selection_pairs` keys. They are `selection.word_separators` and `selection.pairs`.

## Research findings

- Under a stock `x11-clipboard`, 19 of 900 rounds lost a copy. With the patch, 0 of 900.

- A drag held past the edge picked up 133 and 127 lines in three seconds at 24 rows, and costs no extra CPU once pinned.

## Roadmap

- A double-click on a Windows drive path has not been rechecked on Windows against a fresh config. Backlog: "Double-clicking a Windows path leaves off the drive letter".

- A PowerShell background job reads as a command still running, so copy on output waits until it ends. Windows gives no way to tell a background child from a foreground one.

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
