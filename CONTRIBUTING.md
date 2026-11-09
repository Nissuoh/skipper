# Contributing to Skipper

## Status: contributions are not open yet

As of 0.1.0-test.6 (2026-11-09), Skipper is in active development and not released. The repository holds the documents and the project rules; the source code, and with it the design document, is added step by step, in small reviewed pull requests (documents first; see [docs/code-arrival.md](docs/code-arrival.md)). Contributions are not open, and pull requests from anyone who was not invited will not be reviewed. From the day the repository becomes public until this section says "open", GitHub's pull request setting stays at "Collaborators only": only people the owner has invited can open a pull request, while everyone can still read, comment and open issues. (maintainer's call)

Contributions open when all of these hold (maintainer's call):

- the repository is public;
- Stage 0 has ended with a "yes" (design section 19.1);
- the Cargo workspace builds from a clean checkout with the pinned toolchain;
- the project's checks, including the check of the sign-off line of rule 2, run on every pull request;
- this section says "open".

Until then, the rules below describe how contributing is planned to work, so you can judge in advance whether you want to take part. Who decides what is described in [`GOVERNANCE.md`](GOVERNANCE.md), and where questions go is described in [`SUPPORT.md`](SUPPORT.md).

Please read these first:

- `README.md` – what Skipper is and is not.
- `docs/glossary.md` – the words the project uses.
- `docs/design/2026-10-20-skipper-design.md` (it follows with the code; documents first) – the design, including every decision and how it is marked.
- [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) – how people treat each other here.
- [`GOVERNANCE.md`](GOVERNANCE.md) – who decides, and what happens when the maintainer is away.
- [`docs/github-workflow.md`](docs/github-workflow.md) – how work on GitHub goes, step by step, from the issue to the merge, with the rules for work with AI tools. People and AI tools follow it, and AI tools also read [`AGENTS.md`](AGENTS.md).

## The rules

### 1. English only (owner decision)

Everything that others see or help build is English: code, comments, documentation, issues, pull requests, reviews and commit messages. The interface texts live in language files; German appears only in the German language file. No user-facing text is written into the code.

### 2. Sign off your commits (maintainer's call)

There is no contributor agreement and no copyright assignment. You keep your copyright, and your contribution is licensed under the project's MIT licence; the copyright notice in [LICENSE](LICENSE), "Nissuoh and the Skipper contributors", covers you and every other contributor. Instead, every commit carries a sign-off line that certifies the Developer Certificate of Origin, version 1.1 (see developercertificate.org for the full text):

```
Signed-off-by: Your Name <your address>
```

`git commit -s` adds the line from your git settings. By signing off you state that you wrote the change or have the right to submit it under the project's licence, and that you understand the contribution and the name and address in the sign-off are public and permanent. Sign off with your name and any address you accept as public; the no-reply address that GitHub offers in your e-mail settings is recommended. The ban on e-mail addresses in files (AGENTS.md rule 6 and the leak scan) applies to the content of files, not to the `Signed-off-by` trailer. The trailer rule applies to everyone, the maintainer and every tool included, and `Signed-off-by` is the only trailer allowed. When a pull request is squashed, the commit keeps your sign-off; with several authors, each author's sign-off is kept. Commits without a sign-off are not merged. The check is the commit check of the project's check tool (`xtask`). Until it runs on every pull request, and until it is changed to accept any address you accept as public, the maintainer checks the trailer by hand.

### 3. Small, focused pull requests (owner decision)

Large pull requests are not allowed. A pull request has one topic and stays within the size limit of rule 4 of [`docs/github-workflow.md`](docs/github-workflow.md), so that each one can be accepted or rejected on its own. A pull request that is too big is closed with a request to split it.

How a change travels: an issue first; then a branch named `type/short-topic`; then one small pull request that links the issue; then review; then a squash merge. The steps are in the workflow page. The project's checks are run by hand until the first workflow exists, and the pull request says which ones were run.

- One purpose per pull request. Do not mix a fix, a refactor and a reformat.
- Say what changes and why, and point to the part of the design it follows. If the design has to change first, change the design in its own pull request.
- Where code exists, add or update its tests. A red lock test or a red check tool fails the build, and there is no override.
- A new dependency needs a reason, and it must pass the supply-chain rules of the design (section 17). In Stage 1 the main program has no network or TLS library. A new crate version is used only after 7 days (design section 17.1), also when someone else proposes it, except for the owner waiver on the current pins (see `SECURITY.md`). Dependabot is used for alerts and security updates only; its version-update pull requests are off, and there is no `.github/dependabot.yml`. GitHub's wait setting does not cover security updates, so the maintainer holds those by hand.

What is rejected without review: a pull request that is too big or mixes topics (this rule); one with a commit that has no sign-off (rule 2); one that copies code or wording from other projects (rule 4); one that weakens a safeguard (rule 7); one that contains real data or secrets (rule 8).

### 4. No copied code (owner decision)

Skipper is not a fork and contains no code, graphics, names or terms taken from other projects. General ideas may be reused, written in your own words and your own code. Do not paste in code, text, images or interface wording from other projects, including other tools that orchestrate coding sessions, and do not use their vocabulary. You are responsible for knowing where everything you submit comes from, including text produced with the help of a tool.

Exceptions, each under its own licence and each with its notice kept with it:

- `crates/skipper-import/src/inflate.rs` (it follows with the code; documents first), the DEFLATE reader of the import helper, is a Rust port of `puff.c` from zlib's `contrib/puff` (by Mark Adler, under the zlib licence). It is not a clean-room rewrite. The zlib licence allows the port, but it requires that the notice stays in every source distribution and that an altered version is plainly marked, so the header of that file keeps the zlib notice as the original has it, with the contact line of its author (the at sign of the address is written as the word "at"), says which version was ported and says that this is not the original software. Do not remove or shorten that header.
- `xtask/data/cc-src/*/schema.json` (it follows with the code; documents first) is the public settings schema of the SchemaStore project, under the Apache License 2.0. One line of it was changed, and the change is marked; the licence text (`LICENSE-schema-Apache-2.0.txt`), the notice (`NOTICE-schema.txt`) and `SOURCES.md` stand in the same folder, and none of them may be removed.
- The folder `supply-chain/notices/` holds the licence and notice texts of third-party packages that ship in a build: the notices of the statically linked mingw-w64 runtime and, in `upstream/`, the licence texts of packages that bring none of their own. They are texts, not Skipper's code; each names its source on its first line, and the only change is that an e-mail address on a copyright line is left out, with a marker line in its place.
- A further exception needs the maintainer's decision first, a licence that lets an MIT program ship it, and its notice kept with it.

### 5. Use the glossary's words (maintainer's call)

The terms in `docs/glossary.md` are used exactly, in code, documentation and interface texts. A few that are easy to get wrong:

- "Issue" means a GitHub issue and nothing else. A unit of work on the board is a task.
- "Stage" is a roadmap stage, "tier" is a sandbox level, and "level" on its own is an approval level.
- A team member is a role, not an "agent" or a "worker".
- The crate for the core is `skipper-core`, because `core` collides with Rust's own crate.

If a word you need is missing, propose it in the pull request that needs it.

### 6. How design decisions are marked

Every decision in the design document carries one mark:

- `(owner decision)` – decided by the project owner. Only the owner changes it.
- `(maintainer's call)` – decided by the maintainer; the owner can review it. An issue may propose a change with a reason.
- `(open – owner decides)` – a question that is not settled. Do not build as if it were.
- `(left to the builder)` – a detail the implementer may choose.

Today one person is both the project owner and the maintainer (see [`GOVERNANCE.md`](GOVERNANCE.md)). In these marks, "owner" means the project owner, not the owner of a task on the board. The marks still say how firmly a decision is fixed and who may change it.

Two more words matter. "Unverified" means nobody has proven the statement. "Needs a probe" means it can only be settled by a test on a real Windows machine; where a probe contradicts a maintainer's call, the probe wins and the call is reopened. Labels such as `LOCK-3`, `OPEN-01` or `T1` belong to the design document and mean nothing outside it.

Use the same marks in your own design text.

### 7. Never weaken the safeguards (owner decision)

The four hard locks are fixed in code and are not settings: no account is put at risk (Skipper never reads, stores or sends Claude credentials or session tokens), nothing runs invisibly, nothing reaches the main branch or the internet without a person's click in Skipper's own window, and the PC keeps a reserve of memory and cores (section 5 of the design; [SECURITY.md](SECURITY.md), "Words used here", says the same in plain words). No contribution may add a flag, file, variable or template that turns one off. The account-safety rules of section 9.2 change only by a pull request that gives a reason and is approved by the owner. The eight rules that enforce the first hard lock change only by an owner-approved change of the design itself, and every other rule falls only after Anthropic's written answer (section 9.3) and the owner's yes. A change that touches a safeguard says so in its description.

Three more rules bind everyone, the maintainer included. (owner decision)

- There is one Skipper. Once the source is public, the maintainer will use the same public source and the same releases as everyone else. There is no private, owner-only, paid or feature-gated build and no hidden switch. The source is uploaded in small reviewed pull requests; from then on there is no code that is not in this repository.
- Skipper never reads, stores or asks for a password or other sign-in data of your Claude account. The sign-in is Claude Code's own.
- No document and no code claims that automatic management of several accounts works until a release has it. It is in development.

### 8. Keep real data out

- No credentials, tokens, session transcripts or personal data in code, tests, issues or pull requests.
- Test data, screenshots and recordings come only from dummy sessions or an empty demo workspace.
- Report security problems privately, as `SECURITY.md` describes, never in a public issue.

### 9. Issues, labels and triage (maintainer's call)

There is one maintainer, so issues are sorted by a few fixed rules.

- **Labels.** The full list, with colours, is in `.github/labels.yml`, which the maintainer uses as the checklist for creating them. Type: `bug`, `design` (design feedback), `question`, `documentation`. Status: `needs-triage` (new, not read yet), `needs-info` (waiting for the reporter), `blocked-on-probe` (only a probe on real Windows can settle it), `decided` (answered by a design change or a recorded decision), `duplicate`, `wontfix`. Stage: `stage-0` to `stage-3`, the roadmap stage an issue belongs to. Two more, `good first issue` and `help wanted`, are set by the maintainer on small, well described issues once contributions are open. The issue forms add `needs-triage` and their type label by themselves.
- **Which form.** "Design feedback" is for a mistake, a gap, a risk or a question about the design documents. "Task or documentation change" is for a change to a document or a small piece of work, with its scope. "Bug report" is for a released build; no build is released yet, so reports sent before the first release may be closed. Questions that are not about the design go where `SUPPORT.md` says.
- **Reading.** New issues are read about once a week. Replies are best effort, and there is no fixed deadline.
- **Missing information.** An issue labelled `needs-info` is closed by hand, at the weekly reading, once it has waited 30 days without an answer. It can be reopened when the information arrives.
- **Design feedback** ends either in a change to the design, made by a pull request that links the issue, or in a short note that says what was decided and where; the issue then carries `decided`. A question sent with the "Design feedback" form arrives labelled `design` and is relabelled `question` at triage. A question about an `(owner decision)` is answered with its reason; the decision changes only if the owner changes it.
- **Bug reports** sent before a build is published are closed with a pointer to "Design feedback". A bug in Claude Code itself is closed with a pointer to Anthropic (see [`SUPPORT.md`](SUPPORT.md)).
- **Duplicates and off-topic posts** are closed with a link to the issue that covers them, or with a short reason. Mass-produced or automated posts that no person has read before sending are closed without review, as the code of conduct says.

### 10. GitHub Actions workflows (maintainer's call)

The project has one workflow file, `.github/workflows/catalog.yml`, the daily job for the model catalogue; it is added to the repository with the code (documents first). It does nothing until the owner sets the repository variable that its first lines name, and it follows the rules below. Every workflow must follow the workflow rules of the design (section 17.2), which were written before the first one. In short:

- Apart from that file, no workflow is added before the repository is public and the owner has said yes to hosted checks.
- Every action is pinned to a full-length commit SHA, with its version in a comment. Tags and branch names are not used. Today no action is used at all (see the last point).
- The workflow token starts read-only (`permissions: contents: read` at the top) and is widened only for the job that needs it, with a reason.
- No `pull_request_target` or `workflow_run` job checks out, builds or runs code from a pull request.
- Values that a pull request, issue or commit can set, such as titles, branch names and commit messages, never appear inside `${{ }}` in a `run:` step; they are passed through `env:`.
- Steps call the pinned toolchain and `cargo xtask`; logic is not written into the workflow file. Only GitHub-hosted runners are used, and no secret is used in a job that runs code from a pull request.
- Runner-hosted JavaScript actions, for example for checking out the repository, are not used (owner decision, 2026-10-21). Workflows use `run:` steps with plain commands only, so the design's allow-list of third-party actions stays empty.

A pull request that adds or changes a workflow says so in its description and names the rule it follows.

### 11. Names, logos and package registries (maintainer's call)

- "Claude" and "Anthropic", in any letter case and also inside longer words, never appear in the name of a Skipper package, library, binary, release, branch or tag, in a release title or an annotated tag text, in the name of any file (the name check reads every file name, not only those of images and videos, so there is no file called `CLAUDE.md`; `AGENTS.md` is the one instruction file for AI tools), or in a product string such as the window title, and never in Skipper's logo. Plain text may say that Skipper "runs with Claude Code" and is "not affiliated with Anthropic". The word "official" is not placed next to Skipper. The design's name check enforces this (section 17.5).
- Skipper's own images, icons, screenshots, recordings and test fixtures contain no Claude or Anthropic logo, mascot, look-alike colours, Claude Code banner or ASCII art. Recorded test fixtures are synthetic, or cut down to the shortest strings that the test needs.
- Skipper is distributed only through this repository's Releases page. Every package sets `publish = false`, directly or inherited from the workspace, and nothing is published to crates.io or another registry, also no placeholder crate that only reserves a name (crates.io's policy forbids name squatting).

### 12. Rust only (owner decision)

The product, its tools, tests and build scripts are Rust only. No Python, JavaScript, TypeScript, Node or shell-script tooling. Helper scripts are Rust, in `xtask`. The allow-list scan refuses Python, JavaScript, TypeScript and Node files; review catches the rest. A rare exception needs the file's exact path on the allow-list, with a comment that says why, and the maintainer's approval in a reviewed pull request. No such exception exists today.

**Toolchain.** The compiler is pinned in `rust-toolchain.toml` (Rust 1.99.1, minimal profile, no extra components, target `x86_64-pc-windows-gnu`). The GNU target is used so that a build needs no Visual Studio installation. The project runs its own source checks in `xtask`, so `rustfmt` and `clippy` are not part of the pin; if they are added later, the components are added to that file in a pull request of their own. A new compiler version is also a pull request of its own, with a reason, and the maintainer applies the same 7-day wait after release before accepting it. Skipper targets Windows 11 only. On another system you can still read, review and improve the documents; building the program there is not supported.

### 13. Leak scan and push gate (maintainer's call)

Before anything is pushed, a scan reads every changed file and every commit message and refuses what must not be public: banned terms, e-mail addresses in file content, expanded personal paths, secrets, brand words in names and imported pages. It is `cargo xtask leak-scan`; until a hosted check runs it on every pull request, it is run by hand, and a pull request description says so (see [`docs/release-guide.md`](docs/release-guide.md), section 2).

- **Allow-list.** The scan refuses every path that is not on the list in [`scan/allowed-paths.txt`](scan/allowed-paths.txt). Most entries are a path or a directory pattern. Binary and media files pass only with their exact path. Files of languages the project does not use (rule 12) pass only with their exact path and a comment that says why. Names that mark a secret are refused whatever the list says.
- **Today.** Some entries name directories that follow with the code (`crates/`, `tools/`, `spikes/`) and others are exact paths of files that follow with the code (icons, test fixtures). They admit nothing now, because those files are not in the repository. Narrowing them to the exact directories that arrive is a follow-up change to the scan's own files and is made when the first code pull request needs it.
- **Whose rules apply.** The rules are read from the tip of main, never from the commits being judged, so a pull request cannot loosen the check that judges it.
- **Asking for a path.** Open a pull request that adds the path, with the reason in its description. Nothing is added in passing with other work.
- **Judge code.** The allow-list, the scan and the other files that decide what passes (`xtask/`, `tools/` (it follows with the code; documents first), `scan/`, `supply-chain/`, `.cargo/`, `rust-toolchain.toml`, `Cargo.lock`, every `Cargo.toml`) are judge code. A change to them needs a reviewer who did not write it and the maintainer's explicit acceptance, as [`docs/github-workflow.md`](docs/github-workflow.md) says.
- **What outsiders cannot run.** The list of private banned terms and the maintainer's identity files are kept outside the repository, so that one check cannot be reproduced by contributors. Everything else is in this repository; running it needs the capped launcher, which follows with the code. The maintainer runs the full scan before merging, and a pull request that fails it is returned with the finding.
- **Push gate.** The maintainer's pushes to this repository go through a launcher that runs the scan first and refuses the push on any finding. It is a convenience for one person, not a rule for contributors, who will work from forks once contributions open.

## Conduct

Everyone who takes part follows the [code of conduct](CODE_OF_CONDUCT.md). Conduct problems are reported privately, as that file describes, never in a public issue.
