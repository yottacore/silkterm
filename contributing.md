<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Contributing

First off, thanks for taking the time to contribute!

All types of contributions are encouraged and valued. See the [Table of Contents](#table-of-contents) for the different ways to help and the details on each. Please read the relevant section before making your contribution - it makes things easier for the maintainer and smoother for everyone.

> If you like the project but do not have time to contribute, that is fine. There are other easy ways to support it and show your appreciation:
> - Star the project
> - Post about it
> - Refer to it in your own project's readme
> - Mention it to friends and colleagues

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [Code of conduct](#code-of-conduct)
- [I have a question](#i-have-a-question)
- [I want to contribute](#i-want-to-contribute)
	- [Reporting bugs](#reporting-bugs)
	- [Suggesting enhancements](#suggesting-enhancements)
	- [Your first code contribution](#your-first-code-contribution)
- [Style guides](#style-guides)

<!-- /TOC -->

## Code of conduct

This project and everyone in it is governed by the [Code of Conduct](code_of_conduct.md). By participating, you are expected to uphold it. Report unacceptable behavior to silktermⒶyottacore.com.

## I have a question

Before asking, search the existing [issues](https://github.com/yottacore/silkterm/issues) - your question may already be answered. It is also worth a quick web search first.

If you still need clarification:

- Open an [issue](https://github.com/yottacore/silkterm/issues/new).

- Provide as much context as you can about what you are running into.

- Provide project and platform versions where they seem relevant.

## I want to contribute

> ### Legal notice
> When contributing to this project, you must agree that you have the necessary rights to the content, and that it may be provided under the project license. Work done with AI tools is covered by the [AI policy](ai_policy.md).

### Reporting bugs

<!-- TOC ignore:true -->
#### Before submitting a bug report

A good bug report should not leave others chasing you for more information. Please investigate, gather what you can, and describe the issue in detail.

- Make sure you are on the latest version.

- Confirm it is really a bug and not a local misconfiguration or an incompatible environment. Re-read the [documentation](README.md); for support questions see [I have a question](#i-have-a-question).

- Check the [bug tracker](https://github.com/yottacore/silkterm/issues?q=label%3Abug) to see if it has already been reported.

- Search the web to see if others outside this repo have discussed it.

- Collect the relevant details:
	- OS, platform, and version (Windows, Linux, macOS; x86, ARM).
	- Environment versions.
	- Your input and the resulting output, where useful.
	- Whether you can reliably reproduce it, and whether older versions also reproduce it.
	- Clean, from-scratch reproduction steps.

<!-- TOC ignore:true -->
#### How do I submit a good bug report?

> Never report security issues, vulnerabilities, or bugs that include sensitive information in the public tracker. Send those by email to silktermⒶyottacore.com instead.

Bugs and errors are tracked as GitHub issues. When you hit one:

- Open an [issue](https://github.com/yottacore/silkterm/issues/new).

- Explain the behavior you expected and the behavior you actually saw.

- Give as much context as you can, and describe the reproduction steps someone else can follow. Isolate the problem into a reduced test case where possible.

- Include the details you collected above.

Once filed, the issue will be triaged, someone will try to reproduce it from your steps, and it will be labeled and worked from there. Issues without reproduction steps may be paused until they can be reproduced.

### Suggesting enhancements

This section covers submitting an enhancement suggestion for SilkTerm, from brand-new features to small improvements. Following it helps the maintainer and the community understand your idea and find related ones.

<!-- TOC ignore:true -->
#### Before submitting an enhancement

- Make sure you are on the latest version.

- Read the [documentation](README.md) to see whether the functionality already exists, perhaps via configuration.

- Search the [issues](https://github.com/yottacore/silkterm/issues) to see whether it has already been suggested. If so, comment on the existing issue instead of opening a new one.

- Consider whether it fits the scope and aims of the project. Make the case for why it would be useful to most users, not just a small subset.

<!-- TOC ignore:true -->
#### How do I submit a good enhancement suggestion?

Enhancement suggestions are tracked as [GitHub issues](https://github.com/yottacore/silkterm/issues).

- Use a clear and descriptive title.

- Give a step-by-step description of the suggested enhancement.

- Describe the current behavior and explain the behavior you would like instead, and why. Note any alternatives you considered.

- Explain why the enhancement would be useful to most SilkTerm users. Point to other projects that solved it well, if any.

### Your first code contribution

- Install the per-platform prerequisites: [prerequisites.md](prerequisites.md).

- Build and run per [build.md](build.md). On Linux, `cargo run --release` is enough to get going.

- Read the [style guide](style-guide.md) before writing code - it is the canonical reference for naming, comments, Rust conventions, and formatting.

## Style guides

The style reference is [style-guide.md](style-guide.md), for code, scripts and commit messages.

<!-- TOC ignore:true -->
### Commit messages

Keep them short. Put the details in the issue or pull request. See [commit messages](style-guide.md#commit-messages) in the style guide.
