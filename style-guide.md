<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Style guide

The style reference for SilkTerm's code, scripts and commit messages. Where it conflicts with a language's own idioms or its formatter, those win. See [Formatting](#formatting).

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Comments](#comments)
- [File headers and licensing](#file-headers-and-licensing)
- [Naming](#naming)
- [Rust](#rust)
	- [Errors](#errors)
	- [Ownership and borrowing](#ownership-and-borrowing)
	- [Control flow](#control-flow)
	- [Types and abstraction](#types-and-abstraction)
	- [Iterators](#iterators)
	- [Documentation](#documentation)
- [Bash](#bash)
- [PowerShell](#powershell)
- [Python](#python)
- [Formatting](#formatting)
- [Commit messages](#commit-messages)

<!-- /TOC -->

## Comments

- Explain *why*, not *what*.

- No banner or flowerbox comments. The docs gate fails on a banner rule in Rust code.

## File headers and licensing

- Every source file starts with an SPDX identifier and a copyright line:

	~~~rust
	// SPDX-License-Identifier: GPL-2.0-or-later
	// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
	~~~

- The project itself is licensed GPL-2.0-or-later.

- Standalone helper and utility scripts are usually MIT, regardless of the project license. Give those their own MIT header.

## Naming

- Use meaningful, searchable names. `upperBound` is easy to find and replace by hand, and a bare `ub` is not.
	- A name doesn't need to be long or globally unique. It needs to be clear and easy to find. Short conventional names are fine.

- Single-letter loop counters and iterators (`for i in ...`) are fine when that is the idiomatic choice.

- Follow the language's canonical case and word-order conventions (snake_case in Rust).

## Rust

Edition 2024. Code should look the same from one file to the next, with one way of handling errors and one naming scheme.

### Errors

- Errors are values. Return `Result<T, E>` and propagate with `?`.

- No `panic!`, `unwrap()` or `expect()` outside tests, except where a case can't happen. Then say why in a short comment. Clippy refuses `unwrap()` and `expect()` there.

- Prefer `thiserror` for library-style error types and `anyhow` for application-level error handling.

### Ownership and borrowing

- Borrow first. Don't use `.clone()` just to get past the borrow checker. Restructure or take a reference instead.

- If a clone is really needed, add a comment saying why.

- Avoid gratuitous `Rc<RefCell<...>>`.

- Prefer `&str` over `String` and slices over `Vec` in arguments. Return owned types.

### Control flow

- Return early to keep the happy path at minimum indentation. Use guard clauses.

- `let ... else { return ... }` for "extract or bail".

- `?` to propagate instead of nesting `match` or `if let`.

- No `else` after a `return`.

- Collapse nested `if let` with `let`-else or, where it reads well, let-chains (`if let ... && ...`).

- Prefer flat combinators like `map` and `and_then` on `Option` and `Result` when they read cleanly. Use `match` when there are really several arms.

### Types and abstraction

- Model mutually-exclusive states with enums and exhaustive `match`, not boolean flags. Avoid a catch-all `_` arm unless it is truly needed.

- Where it is cheap, use newtypes or typestate so invalid states can't be built.

- Use traits and generics for abstraction, and `dyn` only when the types really differ. Prefer composition to class hierarchies.

- Derive `Debug`, `Clone`, `PartialEq` and the rest rather than writing them by hand. Every public type derives `Debug`.

### Iterators

- Prefer iterator chains over manual loops while they stay readable.

- Break to a plain `for` loop when a chain would need more than about three combinators, or when clarity suffers.

### Documentation

- Document public items with `///`. A comment on anything declared `pub` is a `///` doc, never a plain `//`. A name that already says all there is to say needs no doc, since one would only repeat it.

- A file that opens with a comment about itself uses `//!` for it.

- A lint reason goes in the attribute's `reason = "..."`, not in a comment above it.

- Name things fully; no cryptic abbreviations.

## Bash

- Scripts end in `.bash`.

- Functions are fCamelCase, such as `fDie` or `fTestDir_Use`. Variables are camelCase. Settings read from a config file, and environment variables, are UPPER_SNAKE_CASE.

- Use `[[ ]]` for tests, not `[ ]`.

- Brace and quote expansions: `"${name}"`, not `$name`.

- Indent with tabs. There is no formatter, but scripts must pass `shellcheck` at warning level.

## PowerShell

- Functions are fCamelCase, the same as in Bash, rather than PowerShell's Verb-Noun. This is a choice for this project only. `PSUseApprovedVerbs` is off in `cicd/PSScriptAnalyzerSettings.psd1` for that reason, and each other rule turned off there says why.

- Scripts must pass PSScriptAnalyzer at warning level with those settings.

- Indent with tabs. Spaces may follow the tabs to line up a continuation.

## Python

- Names follow PEP 8: snake_case for functions and variables.

- Every function signature has type hints.

- Strings are built with f-strings, not `%` or `.format()`. Paths go through `pathlib`, not `os.path`.

- A file a script opens is closed by a `with` block, and every `subprocess.run` says `check=` one way or the other.

- An `except` names the errors it expects. Where catching everything is the point, the line says so with `# noqa: BLE001` and a short reason.

- Indent with tabs, like the rest of the repo, not PEP 8's four spaces. Spaces may follow the tabs to line up a continuation. One older script uses spaces and is left as it is.

- Scripts must pass ruff with the rules in `ruff.toml`, and mypy with `mypy.ini`.

## Formatting

- Rust is formatted by `rustfmt`. Run it and let its output win over hand formatting.

- The project sets `rustfmt` to hard tabs at width four (`rustfmt.toml`). Tabs indent; spaces align. This is the one deliberate deviation from `rustfmt` defaults; everything else follows the defaults.

- Protect intentional hand-formatted data tables (a color matrix, a layout table) from reflow with `#[rustfmt::skip]` rather than fighting the tool.

- Code is expected to pass `clippy`. The build gate runs `clippy -D warnings`. Writing to the stricter `clippy::pedantic` bar is encouraged.

- Bash, PowerShell and Python scripts must pass shellcheck, PSScriptAnalyzer and ruff.

## Commit messages

- Keep them short. A one-line summary of what changed is enough.

- Put the details in the issue or pull request.
