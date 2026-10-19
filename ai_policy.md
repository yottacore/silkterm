<!-- markdownlint-disable MD007 -- Unordered list indentation -->
<!-- markdownlint-disable MD010 -- No hard tabs -->
<!-- markdownlint-disable MD033 -- No inline html -->
<!-- markdownlint-disable MD055 -- Table pipe style [Expected: leading_and_trailing; Actual: leading_only; Missing trailing pipe] -->
<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

<!-- TOC ignore:true -->
# Human-Accountable AI: Guidelines for Software Projects

Where AI is allowed near this project, where it isn't, and who is accountable either way.

<!-- TOC ignore:true -->
## Table of contents

<!-- TOC -->

- [A note about reusing this document (please do)](#a-note-about-reusing-this-document-please-do)
- [Introduction](#introduction)
- [Problems with AI](#problems-with-ai)
	- [Cost to the FLOSS commons](#cost-to-the-floss-commons)
	- [License laundering](#license-laundering)
	- [Code quality](#code-quality)
	- [Security risks](#security-risks)
	- [Bad PR](#bad-pr)
	- [Environmental](#environmental)
	- [Economic](#economic)
	- [Ethical](#ethical)
- [Non-problems with AI](#non-problems-with-ai)
- [Good uses of AI](#good-uses-of-ai)
	- [Review and analysis](#review-and-analysis)
	- [Tests and tooling](#tests-and-tooling)
	- [Porting to other languages](#porting-to-other-languages)
	- [Porting to other operating systems](#porting-to-other-operating-systems)
	- [Tedious non-coding tasks that pay nothing](#tedious-non-coding-tasks-that-pay-nothing)
- [Rules for AI use in this project](#rules-for-ai-use-in-this-project)
	- [A person is accountable for every merged line](#a-person-is-accountable-for-every-merged-line)
	- [Self-assessment of AI speedup is not evidence](#self-assessment-of-ai-speedup-is-not-evidence)
	- [What the agent is allowed to reach](#what-the-agent-is-allowed-to-reach)
	- [What AI may do with review](#what-ai-may-do-with-review)
	- [What always needs full human review](#what-always-needs-full-human-review)
	- [What AI doesn't decide](#what-ai-doesnt-decide)
	- [Machines check first](#machines-check-first)
	- [The test suite](#the-test-suite)
	- [Contributing](#contributing)
- [Where this could change](#where-this-could-change)
- [About the author](#about-the-author)
	- [That's great and all but what about AI](#thats-great-and-all-but-what-about-ai)
	- [The use of AI in writing this document](#the-use-of-ai-in-writing-this-document)

<!-- /TOC -->

## A note about reusing this document (please do)

If you just want a clean AI guidelines document with no commentary, history, or unnecessary author background, delete the following sections. They were written to be cleanly removable:

- Introduction

- Problems with AI (and subsections)

- Non-problems with AI

- About the author (and subsection)

- This note

(Keep the copyright and license footer at the end, per terms of the open [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) license.)

## Introduction

The original inventors of computer languages borrowed heavily from linguistics, which is what LLMs happen to be built for. That's why LLMs got good at coding before most other technical work. Even if "AI" pivots to other underlying technologies, LLMs - or at least good understandings of the mechanics and structures of language - will likely have a place in programming for a long time.

But for now, at the time of writing, even the most cutting-edge frontier models can't be trusted to "vibe code" a project. LLMs can't replace programmers - they can (in theory) only make *experienced* programmers more effective - mostly by offloading the tedious, repetitive, low-risk tasks.

Whether AI can ever fully replace humans for building complex projects remains to be seen - but if/when so, it will necessarily require first being able to understand the business, user needs, what users might need but not even be aware of, what users think they need but would be contrary to project goals, the competitive landscape, legal issues like copyright and patent encroachment, project scoping and management, time and budget management, stakeholder buy-in and communication, security auditing, performance tuning, running UAT, live customer feedback sessions, devops, etc. And all of that, better than humans.

That may seem impossible now, but depending on how you measure it, the capability of AI is doubling every 4 to 7 months, at an accelerating rate. With such exponential growth, we may see a 1,000x improvement in 3 years. (If physics and resources allow it that long. Which seems likely for even a few orders of magnitude beyond that, considering that the growth has been accompanied by similar exponential gains in efficiency, real estate needed, memory budget, and per-token and per-query costs.)

But for now, it's not possible. Humans *must* be *the* critical component in the process. And may necessarily need to be so, indefinitely, at some level.

## Problems with AI

The problems that bear directly on a software project are listed first.

The three listed last may arguably be the most significant over time and space, but are generally well understood - if inconsistently acted upon at political policy levels. They are included here mostly for completeness and acknowledgment.

### Cost to the FLOSS commons

This one is aimed straight at the infrastructure a project like this one sits on.

Codeberg, the nonprofit git host, made the case in [Protecting our FLOSS commons from LLMs](https://blog.codeberg.org/protecting-our-floss-commons-from-llms.html) (July 2026). It's short, and worth reading in full whether or not their conclusions are ones you'd agree with. The argument in brief: AI crawlers are expensive to serve, the hardware to serve them on got expensive too, generated contributions cost a maintainer more to review than they cost anyone to produce, and copyleft quietly loses its teeth when code gets regenerated instead of copied.

Let's start with the crawlers: Bots walk every page of every repository, and the "needless accesses create expensive database queries that diminish the service quality for all of us", on top of real hours out of a volunteer sysadmin team.

Storage got more expensive over the same stretch: a drive they bought for EUR 700 a few years ago now costs EUR 3,700.

Nobody sends that bill to the companies running the crawlers. A large host absorbs it. A small one, an NGO, or a self-hoster might not be able to, and that narrows who can afford to host anything at all.

Then there are the PRs: "People submitting (often well-meaning) low-effort, LLM-generated contributions that require substantial amounts of time to review." That cost isn't shared either. It falls on whoever maintains the project, usually for free, and it scales with how cheap the tooling makes the submission. A model writes a plausible thousand-line pull request faster than anyone can read one.

That asymmetry is most of the reason the rules further down put the burden where they do.

### License laundering

A model trained on copyleft code can emit something very close to it with none of the license attached. Codeberg: "copyleft code is stripped of its reciprocity requirements by 'generating' it out of the training data".

Whether that holds up in court is unsettled, and probably will be for years. The question for a maintainer is more immediate. If a generated block is close enough to some GPL original that a person copying it by hand would have been obligated, then merging it puts the project somewhere it never agreed to go, and nobody in the review chain saw it happen.

Unfortunately there's currently no tooling answer to this, and even the human-driven solutions may inadequately address this risk.

### Code quality

Code quality was a serious and universal problem until recently, even on small projects. On current frontier models it still has to be managed.

The main risk is not that the code fails to work. It's that it works and quietly rots. [GitClear's analysis](https://www.gitclear.com/ai_assistant_code_quality_2025_research) of 211 million changed lines found 2024 was the first year on record where copy-pasted code exceeded moved code, with code clones up roughly fourfold. Refactoring went from about a quarter of changed lines in 2021 to under a tenth in 2024.

That is the failure mode to watch: a model asked for a fix writes a new version rather than finding the existing one. Nothing breaks. The codebase just gets worse in a way no test catches.

### Security risks

Running an agentic tool on a development machine is a risk to the developer. It reads local files, runs commands, and reaches the network. Know the risks and act accordingly.

The generated code is another risk. Veracode's [2025 report](https://www.veracode.com/resources/analyst-reports/2025-genai-code-security-report/) found that across 80 tasks and 100+ models, 45% of samples introduced an OWASP Top 10 vulnerability. Java was worst at over 70%. Cross-site scripting was missed in 86% of the cases where it applied.

Hallucinated dependencies are worse. A [study of 576,000 generated samples](https://www.usenix.org/system/files/conference/usenixsecurity25/sec25cycle1-prepub-742-spracklen.pdf) found package names that do not exist in about 5% of commercial-model output and about 22% of open-model output. Attackers register the common ones and wait. Every dependency an AI suggests must be human-verified.

And the model doesn't even have to invent the name. In August 2026, [researchers scanned](https://arstechnica.com/security/2026/08/claude-codex-and-hermes-installed-unowned-code-inside-corporate-networks/) 6,214 corporate domains for `llms.txt` files, the emerging convention for handing AI agents a machine-readable summary of a site. 120 of them pointed at package names or domains nobody had ever registered. The researchers registered a few and put beacons behind them. Within an hour a Fortune 500 network phoned home, with dozens more to follow, and the process chains traced the installs back to coding agents (Claude, Codex, and Hermes among them) that had read the files and installed whatever they named. A stale reference in somebody else's docs became code execution inside a corporate network, and nothing was hallucinated anywhere along the way.

The general lesson: an agent treats whatever it fetches as instructions, and it has a shell. So the verification rule above isn't just about dependencies the model dreams up. It covers everything the agent pulls from the network on its own, which is part of why the access rules further down give an agent the least it needs.

Used the other way around, for adversarial review, security review, and fuzz and regression suites, the same tools measurably improve a project. That asymmetry is a driving factor behind these guidelines.

### Bad PR

Any project accepting AI-generated contributions has to accept this risk.

AI is becoming a public enemy. The cause is probably mostly greed-driven and too much hype, the economics, and the sometimes shady public/private tactics used to foist data centers onto communities whose citizens pay the externalities.

(That perception may shift once desktop-class open models are good enough to work offline, which looks like a short wait rather than a long one, but perceptions can take years or decades to evolve.)

Either way, hiding the involvement of AI is not the way through any of these problems. Being transparent about its use, managing it as a tool, and accepting whatever criticism follows is the path taken here.

### Environmental

This is the hardest one to justify.

The scale is not in dispute. US data centers used about 4.4% of the country's electricity in 2023, and [Berkeley Lab](https://newscenter.lbl.gov/2025/01/15/berkeley-lab-report-evaluates-increase-in-electricity-demand-from-data-centers/) puts 2028 somewhere between 6.7% and 12%.

The argument is not that this is fine. It is that cost per unit of useful work is falling faster than the headlines suggest:

- Querying a model at GPT-3.5 quality fell from $20 to $0.07 per million tokens between late 2022 and late 2024, per the [Stanford AI Index](https://hai.stanford.edu/ai-index/2025-ai-index-report). Roughly 280x in under two years.

- Hardware cost per unit of performance is dropping about 30% a year. Energy efficiency is improving about 40% a year.

- Open-weight models have nearly caught up. The benchmark gap against closed models narrowed from 8% to 1.7% in a single year.

The part that matters most: open-weight coding models that fit in 24 to 32 GB on a consumer GPU now score around 80% on SWE-bench Verified, against roughly 90 to 95% for the best hosted models. That is already enough for the review and testing work described below.

On capability, the number usually quoted is [METR's](https://metr.org/blog/2025-03-19-measuring-ai-ability-to-complete-long-tasks/): the length of task a model finishes with 50% reliability has doubled about every seven months over the six years to 2025. Fitting only the 2024-2025 data gives a steeper curve. METR puts no single number on that, but on SWE-bench Verified alone they measured a doubling time under three months.

At a seven-month doubling interval, 3 years is about 35x compounded. At a four-month interval, 3 years is over 500x. And the interval itself keeps getting shorter. If that holds, 1,000x in 3 years is well within reach. (This is a trend, not a "prediction".)

So the position is narrower than "AI is worth it". It is that the useful capability is on track to run on a desktop with no cloud data center behind it (e.g. in a solar-powered home office), sooner rather than later.

### Economic

The companies leading this are among the largest to ever exist, and may be steering the global economy toward a cliff. If that happens, the cost will fall mostly on people who never opted in.

Time will tell whether this is the largest bubble in history or whether the anticipated returns arrive first. The railroad and dotcom booms both overbuilt too early, badly; and both left infrastructure that eventually got used at rock-bottom prices long after the bust. Late boom investors ate the loss and the public got the buildout. The presumed winners rarely survived either. For example, Google entered search years after Lycos and AltaVista, stayed private through the whole boom, and came out on top while the latter are historical footnotes.

Either way, if guidelines like the ones below were adopted broadly (narrow scope, human accountability, no AI making the decisions that matter) and especially limited to near-future desktop models - then global demand might look less like a gold rush. Maybe that's cope. Or maybe it's true.

### Ethical

These are mostly downstream of the previous two.

The clearest measured harm so far is to entry-level work. Stanford's [Canaries in the Coal Mine](https://siepr.stanford.edu/publications/working-paper/canaries-coal-mine-six-facts-about-recent-employment-effects-artificial) found a 13% relative decline in employment for workers aged 22 to 25 in the most AI-exposed occupations, including software development, while employment for more experienced workers in those same occupations held steady.

That isn't AI's fault. It's decisions made by employers with AI used as a rationale, possibly against their own long-term interests. A profession that stops training juniors runs out of seniors.

This project can't fix that. What it can do is not pretend the problem is imaginary.

## Non-problems with AI

The common objection "LLMs don't understand context" is confused at best. Contextualization isn't something bolted onto an LLM. It *is* the mechanism.

The objection usually means something narrower: *situational* context. Who you are, what you ate this morning, what's really at stake. That's a fair criticism, just poorly worded. Where LLMs exceed us is textual context: conditioning on a hundred thousand things at once, across more domains than any one person could read, let alone remember.

And "understand" belongs in quotes, because nobody can specify what understanding *is* beyond what it *does*. We grant it to other humans on inference alone, for free. (But in the end, can we really be sure *any* intelligence - yours, mine, the pilot of your next flight - is anything more than a next-word-prediction machine, running on wetware, that got good enough at not dying to mistake itself for something else?)

Whether some future hypothetical AGI is LLM-based is an open question. But LLMs have two advantages now: they can "communicate" with us, and we can literally watch them "think" in our own native language. As for coding, computer languages borrowed heavily from linguistics, which is a large part of why LLMs got good at code so early. Even if AI advances far beyond LLMs, there may still be a role for them in A) the human interface portion, and/or B) coding agents.

## Good uses of AI

What current LLMs demonstrably do well:

- Hold far more context at once than a person can. A model reads an entire repository in one pass, which makes for good "holistic" code reviews. Human working memory holds roughly four things.

- Combine ideas across fields that no single reviewer has read, or could in a lifetime.

- Look up information they weren't trained on.

What they can't do:

- Retain anything between sessions. (Without memory files and even then pretty flawed, for now.)

- Exist as one continuous mind.

The rules below are built, in part, with those limits in mind.

### Review and analysis

Verification is where LLMs are strongest, because a wrong answer is cheaper than a defect.

- Adversarial code review. Large context windows trace more code paths and hold more in mind at once than a person can. Hallucination doesn't appear to be an issue here, and models don't get bored.

- Security review: input handling, allowlists, deserialization, anything touching a file path or a URL scheme.

- Performance review. Reading hot paths for repeated work, redundant allocation, and locks held longer than they need to be.

- Sifting through linter, static analyzer, and profiler output. These produce more findings than anyone can read. Sorting the real ones from the noise, with reasons attached, is a good fit.

A model that reports a defect should file it as an issue, and a person writes the fix.

### Tests and tooling

- Regression suites, especially the pinning kind written after a bug is understood, to prove it stays fixed.

- Fuzz harnesses. Tedious to write, and easy to check.

- CI/CD and build pipelines. Tedious and error-prone (arguably even with cutting-edge dedicated products), with immediate feedback and verifiable by running.

### Porting to other languages

Porting a well-defined, documented codebase with comprehensive existing test harnesses to another language is something current frontier models can do with high fidelity, including refactoring to target-language idioms.

It is then a fully human responsibility to:

- Read and understand the generated code.

- Ensure the code conforms to house style. (Which should be at the linting and autoformatting stage but still needs eyeballs.)

- Make sure it passes all automated *and* manual unit, integration, usability, UAT, regression, performance, and security tests.

### Porting to other operating systems

Similar to the previous point. For some languages (Go, Rust, Zig, and non-compiled cross-platform scripting languages) this is a trivial non-AI task that should just be part of the CI pipeline.

But depending on what the program does, there is often OS-specific branching for functionality the language's own standard library doesn't cover. Models are usually good at "knowing" the idiomatic way to handle those cases.

OS-specific branching, if implemented, must always get human verification.

### Tedious non-coding tasks that pay nothing

Examples:

- Demo gif and video generation, including fully anonymized synthetic scenarios. Tedious for humans, and generally "not fun" for either technical or creative types.

- Benchmarking and measurements for competitive comparison charts.

- Boring "required" website setup and generation. But not for sites that *are* the product - where designers and engineers and stakeholders come together to make something good. Rather, the bare-minimum boilerplate commodity web presence even basic FLOSS products need, that nobody wants to slog through unpaid.

## Rules for AI use in this project

The short version: AI is allowed to do the tedious grunt work, project plumbing, and consistency-checking - which typically takes the most time.

But people are responsible for the goals, design, decisions, and every line of code.

People write the product code. That's the program itself, the part a user runs.

- The one exception, noted above, is porting a reference implementation to another language or operating system. The design and behavior already exist and were written by people, and the existing tests show whether the port matches. The rules for that are under [Porting to other languages](#porting-to-other-languages) and [Porting to other operating systems](#porting-to-other-operating-systems).

Other uses that don't write product code are fine. For example:

- Reviewing pull requests for trojan horses, security flaws, etc.

- Code, security, performance, and consistency reviews.

- Filing backlog issues from what those reviews find.

- Writing build, CI, and/or CD pipelines.

- Writing tests, test pipelines, and fuzz harnesses.

- Checking the holistic project for accuracy and consistency.

When a review finds a defect, it goes in as an issue or on the backlog, and a person writes the fix.

### A person is accountable for every merged line

AI is not an author and not a defense. Whoever merges a change owns it, answers questions about it, and fixes it when it breaks.

The practical test: anyone who can't explain a change doesn't merge it. For anything bigger than a bug fix, that means being able to say what changed, why this design and not another, what it assumes, what could go wrong, and which tests show it working.

### Self-assessment of AI speedup is not evidence

METR ran a [randomized trial](https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/) with experienced open-source developers on repositories they already knew well. They were 19% slower with AI tools. They believed they had been 20% faster.

That gap is the important part. Perceived productivity is not measurable by the person experiencing it, so "it's faster this way" carries no weight on its own. Where speed matters, measure it.

That trial ran on early-2025 tools, and METR now flags it as out of date. Their [February 2026 follow-up](https://metr.org/blog/2026-02-24-uplift-update/) on late-2025 tools estimates a speedup instead: about 18% for returning participants and 4% for new ones. The confidence intervals straddle zero in both cases, and the authors warn of heavy selection bias, since developers increasingly refused to participate without AI.

The numbers have moved since, but self-report still isn't evidence.

### What the agent is allowed to reach

An agent that can run commands can do anything the account running it can. So it gets the least it needs.

- It works on a branch. Nothing it does reaches main without a person merging it.

- Signing keys, registry tokens, and anything else that publishes stay off the machine it runs on. Every release step that can't be undone is done by hand.

- It doesn't see private vulnerability reports, credentials, or anyone's personal information. Whatever goes to a hosted model goes to a third party. The provider's terms have to allow contributing the result back under this project's license, and checking that is the contributor's job.

- History rewrites, force pushes, mass deletes, and anything outside the repository need a person to say so first. When it isn't clear whether something is allowed, the right move is to stop and ask, not guess.

### What AI may do with review

- Read and summarize existing code.

- Review a diff and report findings.

- Draft tests, build scripts, and CI configuration.

- Sort and explain linter, analyzer, and profiler output.

- File issues from what a review finds.

### What always needs full human review

Support code (e.g. CI, tests, etc.) and docs written by AI have to be at the same standard as if a person wrote them. Specifically:

- Any change to the security boundary. Scheme allowlists, path handling, deserialization, anything spawning a process.

- Any new dependency. Confirm it exists, is the package intended, and is actually maintained.

- Public documentation. README, release notes, etc. A model can help draft those, especially from reading human-generated code. But a person reads them before they go out. Replies to humans (e.g. human-created issues) must always be from a human.

### What AI doesn't decide

- Architecture, and anything that will be expensive to reverse. Public APIs, file formats, compatibility promises, major dependencies.

- What gets released and what gets held back.

- Anything requiring judgment about users rather than about code.

- Anything said to a person on behalf of the project. Issue replies, and above all anyone reporting a security problem. AI can help find a vulnerability. It doesn't talk to the reporter and it doesn't disclose.

Explaining a tradeoff is useful. Choosing it is not delegated.

### Machines check first

The order that works: formatter, then linter, then static analyzer and type checker, then the tests, then a person. Everything ahead of the person is cheap and never gets tired.

- Formatters are not "advisory". The formatter's output is immutable, so formatting never comes up in review. Hand-formatted data tables require the formatter's skip pragma.

- Linters and analyzers should run in CI, not just on the machine the work happened on.

- Watch for the gate being weakened instead of satisfied. A suppression comment, a disabled rule, or a loosened config is a change to the project's standards and gets reviewed as one. This is the most common way an agent makes tests "pass" when the direct route is hard.

### The test suite

A test suite is mandatory. Luckily, LLMs are very good at writing tests, as an independent effort. E.g.:

- Regression tests of the pinning kind, written after a bug is understood, so the fix can't silently come undone later. They're easy to check, too: revert the fix and watch the test fail.

- Differential testing wherever more than one implementation of the same thing exists. Run them all over the same inputs and compare the output byte for byte. But agreement between implementations proves they match, not that they are right. (A defect they all share is invisible to it.)

- Fuzzing. Tedious to write by hand, and it finds the input nobody thought of. NIST's secure development framework ([SP 800-218](https://csrc.nist.gov/pubs/sp/800/218/final)) lists it under testing executable code: "use fuzz testing tools to find issues with input handling". And Google's [OSS-Fuzz](https://google.github.io/oss-fuzz/) has found "over 10,000 vulnerabilities and 36,000 bugs across 1,000 projects" doing nothing else.

- Performance benchmarks that run on a schedule, with a threshold that fails the build.

- Security analysis on human/machine boundaries: input handling, path handling, deserialization, anything that spawns a process or opens a URL. Plus a dependency audit that runs on every build, since models sometimes suggest packages that don't exist.

Two things that are not tests but look like them:

- Coverage percentage. It measures which lines ran, not whether anything was checked. A generated test that asserts current behavior raises coverage and proves nothing, which is why those need real review.

- A model's report of a test run. Run the tests.

### Contributing

Contributions that used AI will not be automatically rejected. Three conditions:

- No AI-written product code, apart from a port as described above. Tests adn pipelines written with AI are fine.

- Say so in the pull request, along with roughly what it was used for. No process detail needed. Some projects want a trailer on every commit (libusb requires one, and the Apache guidance suggests it). Here it goes in the pull request, once. Commit messages describe the change, not the tooling used to make it. The same convention that applies to editors, formatters, and everything else.

Keep it small - reviewers' time isn't cheap.

Split mechanical changes from behavior changes, leave unrelated refactoring out, and if the agent hands back more than expected, break it up before sending it.

A pull request that takes longer to review than it took to generate is the thing maintainers everywhere are complaining about. Don't send one. Same for activity generated because it's cheap: bulk issue comments, cosmetic refactors with no purpose, a run of trivial pull requests. None of that is contribution.

## Where this could change

The position stated here is conditional. Things that would tighten it:

- Evidence that AI costs more in defects than it catches.

- Hosting terms that stop allowing AI-generated support code. A git host's rules are not something a project on it gets to negotiate.

## About the author

I'll share my backstory here, because I think it's probably very similar to anyone reading this who is considering implementing something like this as a policy for their own organization, or personal use. (If I were you though, I'd delete this and other "narrative" sections before modifying and deploying, as irrelevant to the actual policies.)

I've been programming since my early teens. Hobby, then professional, then a decades-long side-quest into all the levels of leadership (almost always with programmers in my chain of accountability and with a bit of biased "favored status"). All the while still being an enthusiastic hobby programmer. I've had a pretty standard tech career arc that covered low-level systems programming, Object-Oriented business programming, formal UI/UX design, (traditional) database design, many of the old and newer network and web paradigms, full-stack web dev, mobile development, business analysis/consulting, technical design writing, product ownership, adopted the whole agile thing, and devops with CI/CD. And just like anyone on this path, it involved a fair amount of anxiety over the sheer depth and breadth of new technologies I felt I "should" at least understand well if not also use, the FOMO, and the self-doubt over why I wasn't able to understand it all at once. Then at some point there was a slow-motion revelation - a resignation and acceptance that it's all too much - and the realization that "leaders" aren't supposed to know "everything". *Gestures broadly*. (In that respect, "giving up" meant moving forward and helping others.)

As a kid I started with BASIC, then Pascal in college, then C. (Pretty standard progression at the time.)

Then the OO indoctrination began - first with Visual Basic, some Java, then proper early C#. (And of course mandatory JavaScript, node, etc.)

I've struggled mightily to shake the reflexive urge to force OO onto non-OO languages. (Whether procedural, functional, prototypal - anything.)

Go was my first "real" modern, compiled, non-OO language. (And as you may know, boy is it non-OO.) I still struggle to avoid stapling vaguely OO-like patterns onto it.

Also somehow along the way, I became ~~the world's leading authority~~ some guy who got good at Bash. (Mostly by horribly misusing it for 20 years, using it for things it was never meant for - wondering why it performed so poorly - and cringing at my own earlier work from even a few years ago.)

### That's great and all but what about AI

Personally, I use AI to help me:

- Act as a training aid to overcome my bad OO habits, by explicitly asking the agent to identify alien OO patterns, and suggest idiomatic refactorings of the existing code.

- Deal with Rust's confusingly myriad ways of doing everything, by suggesting the one idiomatic style I chose to stick to, after searching the documented advice of experts.

	- AI also helps me understand the parts of Rust syntax I'm still having trouble hard-wiring into the brain. Which helps perpetuate that problem - but "become a master of Rust syntax" is just not on my life bucket list. Golang...maybe.

- Port code to multiple languages with bit-for-bit fidelity on input and output against a given reference implementation. (E.g., SHCL.)

- Build OS-specific packaging and installers.

- Handle most of the devops pipeline, given explicit instructions and references from previous work - including checksum-accurate reproducible builds where possible.

- Free me from the oppressive shackles of TDD. (Which I never formally adopted due to its many drawbacks, but still tend to reflexively reach for anyway, in some form or another, out of a vague sense of "obligation". [Obligation to who?])

- Do dull, tedious work like generating benchmark and demo video/GIF harnesses.

- Do adversarial code reviews. This is where AI truly shines. In fact, they're *too* good - and it's a problem converging on a bug-free candidate.

### The use of AI in writing this document

Everyone who knows me knows I love to write. Especially about highly technical subjects.

AI was used on this document for:

- **Spell-checking**. I normally use LibreOffice Writer, but it complains about every part of every URL and Markdown link, and my exceptions library is enormous by now. AI knows what to skip.

- **Fact-checking**. Several claims here were overstated at best, or flat-out wrong at worst, and got backed off or removed. And sometimes I learn new things along the way.

- **Reducing "conclusion shopping" and confirmation bias**. We humans love to shop for studies and links that support a preconceived argument. I'm no different. AI can offer a good check on that habit if asked, and could be the single best thing a solo writer can use AI for.

- **Comparing against other projects' policies**. The rules section was checked against the [Apache Software Foundation's generative tooling guidance](https://www.apache.org/legal/generative-tooling.html), GitHub's guidance on coding agents, and libusb's contribution rules for agents, and a few gaps got filled.

What AI was *not* used for:

- **Content generation**. Every questionable and/or redundant argument made here, every odd injection of unsolicited opinion and narration into what should be a straightforward "guidelines" document - is from a human. This human.

- ~~**Grammar-checking**. I prefer the organic feel of my own tedious phrasing, run-on sentences, and abruptly ending such run-on sentences where I've run out of examples but want it to *seem* like there's more, with ", etc.". If it's not tedious for me to read my own writing, it just doesn't *feel* right~~.

	- Since the second edition, this has been grammar-checked with AI.

- **Tone and appropriateness policing**. Again: not doing but probably should.

---

Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

> *This document is licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Copy it, change it, use it in a commercial project. Attribution required, and say if you changed it. None of this is legal advice, and any software this sits next to is licensed separately.*
