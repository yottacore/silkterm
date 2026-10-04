<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Style guide

The canonical style reference for SilkTerm. It covers prose, comments, naming, Rust, Bash, PowerShell and Python conventions, formatting, and commit messages. When something here conflicts with a language's own well-established idioms or its enforced formatter, the idiom and the formatter win - see [Formatting](#formatting).

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

- No narration that restates the next line.

- No decorative flowerboxing.

## File headers and licensing

- Every source file carries an SPDX identifier and a copyright line at the top:

	~~~rust
	// SPDX-License-Identifier: GPL-2.0-or-later
	// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]
	~~~

- The project itself is licensed GPL-2.0-or-later.

- Standalone helper and utility scripts are usually MIT, regardless of the project license. Give those their own MIT header.

## Naming

- Use meaningful, searchable names. It should be easy to read and to search-and-replace `upperBound`; a bare `ub` is not.
	- But a name doesn't need to be long or globally unique - it needs to be clear and easy to locate. Short conventional names are fine where they read cleanly.

- Single-letter loop counters and iterators (`for i in ...`) are fine when that is the idiomatic choice.

- Follow the language's canonical case and word-order conventions (snake_case in Rust).

## Rust

Edition 2024. The guiding aim is code that is consistent within and across files: the same error strategy, the same naming, the same module layout throughout.

### Errors

- Errors are values. Return `Result<T, E>` and propagate with `?`.

- No `panic!`, `unwrap()`, or `expect()` outside tests, examples, or provably-unreachable cases. When a case really is unreachable, justify it with a short comment.

- Prefer `thiserror` for library-style error types and `anyhow` for application-level error handling.

### Ownership and borrowing

- Borrow first. Do not use `.clone()` to satisfy the borrow checker - restructure, borrow, or take a reference instead.

- If a clone is really needed, add a comment saying why.

- Avoid gratuitous `Rc<RefCell<...>>`.

- Prefer `&str` over `String` and slices over `Vec` in arguments. Return owned types.

### Control flow

- Return early to keep the happy path at minimum indentation. Use guard clauses.

- `let ... else { return ... }` for "extract or bail".

- `?` to propagate instead of nesting `match` or `if let`.

- No `else` after a `return`.

- Collapse nested `if let` with `let`-else or, where it reads well, let-chains (`if let ... && ...`).

- Prefer flat combinators (`map`, `and_then`, `unwrap_or_else`) on `Option` and `Result` when they read cleanly. Fall back to `match` for genuine multi-arm logic.

### Types and abstraction

- Model mutually-exclusive states with enums and exhaustive `match`, not boolean flags. Avoid a catch-all `_` arm unless it is truly needed.

- Use the type system to make invalid states unrepresentable where it is cheap - newtypes, typestate.

- Traits and generics for abstraction; `dyn` only for heterogeneity. Compose; do not reach for inheritance-shaped designs.

- Derive rather than hand-roll (`Debug`, `Clone`, `PartialEq`, and so on). Derive `Debug` on all public types.

### Iterators

- Prefer iterator chains over manual loops while they stay readable.

- Break to a plain `for` loop when a chain would need more than about three combinators, or when clarity suffers.

### Documentation

- Document public items with `///`.

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

- Indent with tabs, like the rest of the repo, not PEP 8's four spaces. Spaces may follow the tabs to line up a continuation. One older script uses spaces and is left as it is.

- Scripts must pass ruff with the rules in `ruff.toml`.

## Formatting

- Rust is formatted by `rustfmt`. Run it and let its output win - do not hand-format against it.

- The project sets `rustfmt` to hard tabs at width four (`rustfmt.toml`). Tabs indent; spaces align. This is the one deliberate deviation from `rustfmt` defaults; everything else follows the defaults.

- Protect intentional hand-formatted data tables (a color matrix, a layout table) from reflow with `#[rustfmt::skip]` rather than fighting the tool.

- Code is expected to pass `clippy`. The build gate runs `clippy -D warnings`. Writing to the stricter `clippy::pedantic` bar is encouraged.

- Scripts with an enforced linter (Bash under `shellcheck`, PowerShell under PSScriptAnalyzer, Python under ruff) must pass it.

## Commit messages

- Keep them brief and high-level - a short summary of what changed.

- Put real detail in the issue, the pull request, or the code, not in a long enumerated commit body.
