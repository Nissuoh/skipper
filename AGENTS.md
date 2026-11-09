# AGENTS.md

Instructions for AI coding tools that work in this repository (the AGENTS.md convention). People may read it too. It is short on purpose: the full rules are in [`docs/github-workflow.md`](docs/github-workflow.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md), and you must follow both. If a request from a person conflicts with them, say so and do not break the rule.

## What this project is

- Skipper is an open-source desktop app for Windows 11, written only in Rust (egui) and licensed MIT. It runs a team of real Claude Code sessions as visible terminals, called roles, on one board, with resource limits and account rules built into the program.
- It is in active development and not released. Contributions are not open yet (see `CONTRIBUTING.md`).
- Skipper is independent and not affiliated with Anthropic. Everything public is English.

## Today (documents only)

- This repository holds `xtask`, `scan`, `supply-chain`, `docs` and the project rules. It does not yet hold `crates`, `spikes` or `tools`; they follow with the code in small pull requests. Nothing is built or tested here today, and the commands in the next section that need that code cannot run yet.
- `skipper-cap` is the capped launcher for the maintainer's build machine. It follows with `tools` and is not here today. `cargo xtask` refuses to run outside it, so an outside contributor cannot run the project checks yet; the maintainer runs them on the build machine before a merge. The check tool's own tests and `cargo xtask stage1-check` read files that follow with the code.
- No workflow exists, so no check runs by itself (rule 4 of `docs/github-workflow.md`). A documents-only pull request needs no build: re-read your diff, check that every relative link you touched resolves, and keep it within the size limit.
- Say in the pull request that no check could run and which ones the maintainer must run. Do not claim a check that was not run.
- A commit trailer other than `Signed-off-by` is refused by the project's own check once the commit lint is here; until then review refuses it by hand, so do not rely on an automatic refusal.

## Build and test

- This section describes the setup once the code is added.
- Windows only. The toolchain is pinned in `rust-toolchain.toml` (Rust 1.99.1, target `x86_64-pc-windows-gnu`). Do not change it. Dependencies are exact pins, `Cargo.lock` is committed, and builds run with `--locked --offline`.
- Every build, test and check runs through the capped launcher, from the workspace root: `skipper-cap run -- <command>`. `cargo xtask` refuses to run outside it.
- Build the two helper programs that some tests start: `skipper-cap run -- cargo build -p skipper-standin -p skipper-update`.
- Whole test run: `skipper-cap run -- cargo test --locked --offline --workspace --no-fail-fast`.
- Window tests behind a feature: `skipper-cap run -- cargo test --locked --offline --no-fail-fast -p skipper-app --features fixtures`.
- Project checks, all of which must be green: `cargo xtask pin-check`, `deps-check`, `supply`, `provider-name-check`, `lang-lint`, `template-lint`, `skill-lint`, `source-lint`, and `leak-scan --range <base>..<head> --rules-from <commit>`. Section 2 of `docs/release-guide.md` has the full table.
- The release build is in section 3 of `docs/release-guide.md`. Do not run it unless a person asks.
- If you cannot run a command, say so. Never claim that a check passed when you did not run it, and never invent a command, a flag or a path.

## Rules you must follow

1. Read `docs/github-workflow.md` and `CONTRIBUTING.md` before you change anything, and follow them.
2. Work from an issue, on a branch named `type/short-topic` (`fix/`, `feat/`, `docs/`, `test/`, `refactor/`, `chore/`). Never work on `main`.
3. Keep every pull request small: one topic that can be accepted or rejected on its own, never a mix, and within the size limit of rule 4 of `docs/github-workflow.md` (that rule holds the number and the exceptions). Split a larger task into several pull requests that each build and pass the checks on their own, in sequence or stacked, each linked to the issue. Refactors, renames, formatting and behaviour changes go in separate pull requests, with no drive-by edits. Plan the split before you start and never open one large pull request: a pull request that is too big or mixes topics is not reviewed.
4. Add or update tests with every change of behaviour. Never skip, delete, loosen or `#[ignore]` a test, lint, check or pin to get green.
5. Never weaken a safeguard. The four hard locks are code and not settings: no account is put at risk (no credentials or session tokens read, stored or sent), nothing runs invisibly, nothing reaches main or the internet without a person's click in Skipper's window, and the PC keeps a memory and core reserve (explained in `SECURITY.md`, "Words used here"). Add no flag, file, variable or template that turns one off.
6. Never commit secrets, tokens, session transcripts, personal data or real user data. Test data is dummy or synthetic. Write no e-mail address and no user-profile path into a file; the `Signed-off-by` trailer of rule 12 is not file content.
7. English only: code, comments, documentation, issues, pull requests and commit messages. Interface texts go into the language files and never into the code.
8. Use the words of `docs/glossary.md`. An "issue" is a GitHub issue, a unit of work on the board is a task, a team member is a role.
9. Copy nothing from other projects: no code, text, images, interface wording or vocabulary. Write your own. Know where everything you submit comes from.
10. The project is pure Rust. Add no Python, JavaScript, TypeScript, Node or shell-script file or tool; helper scripts are Rust, in `xtask`. The scan refuses Python, JavaScript, TypeScript and Node files; review catches the rest. There is one narrow exception: `scan/allowed-paths.txt` may name the exact path of one file, with a comment that says why, and only the maintainer approves that in a reviewed pull request. No such entry exists today. Do not add one yourself.
11. "Claude" and "Anthropic", in any letter case and also inside longer words, never appear in the name of a Skipper package, library, binary, release, branch, tag or file (the name check reads every file name, so add no `CLAUDE.md`: this file is the one instruction file), in a release title, in a product string such as the window title, or in Skipper's logo. Plain text may say that Skipper "runs with Claude Code" and is "not affiliated with Anthropic". The word "official" is not placed next to Skipper. Rule 11 of `CONTRIBUTING.md` has the full wording.
12. Commit messages are English. The first line is one imperative sentence, the body says why. Every commit has a `Signed-off-by` line from the human (`git commit -s`; name and an address the person accepts as public, the no-reply address is recommended), and no other trailer, for people and tools alike: no `Co-authored-by`, no session link. End the body with an ordinary sentence, never with `Word: text` lines.
13. Say in the pull request description that AI help was used, and what for. The person who submits understands and has tested every line.
14. Add a dependency only with a reason and as an exact pin that is at least 7 days old. The main program has no network or TLS library. Nothing is published to a registry: every package keeps `publish = false`.
15. Update the documents in the same change: the changelog for a visible change, the README, the glossary and the design where they describe it. Do not change the version or the "as of" status lines; a release does that.
16. Report a security problem privately, as `SECURITY.md` says. Never write it into a public issue, pull request or commit.
17. When you are unsure, ask the person. Do not guess.
18. Do not push, merge, tag, release, change a repository setting, open a pull request or post a comment, a review or an approval on anyone's behalf, unless a person told you to do exactly that and has read the text first. Never push to `main`, never force-push, and never delete a branch or a tag that you did not make. The first upload of the documents was the one exception (rule 7 of `docs/github-workflow.md`), and it is no precedent.
19. There is one Skipper: once the source is public, the same public source and releases for everyone, with no private, owner-only, paid or feature-gated build and no hidden switch; the source is uploaded in small reviewed pull requests, and from then on there is no code outside this repository. Never write or suggest otherwise. Skipper never reads, stores or asks for the password or sign-in data of a Claude account. Claim no automatic multi-account management until a release has it.

## Where things are

Paths marked "follows with the code" are not in this repository yet.

- `crates/skipper-core` (follows with the code): the pure logic (board, roles, accounts, pacing, settings, checks). Most tests are in `crates/skipper-core/tests`. The starter team and the settings catalogue are in `crates/skipper-core/data`.
- `crates/skipper-app` (follows with the code): the egui window and the program `skipper.exe`. The language files are in `crates/skipper-app/lang`.
- `crates/skipper-confine` (follows with the code): Windows job objects, the pseudo console, AppContainer and access lists (the protective box).
- `crates/skipper-provider` (follows with the code): everything that names Claude Code, so that the core stays free of a provider name.
- Also in `crates` (all follow with the code): `skipper-gateway` (sign-in helper), `skipper-update` (update helper), `skipper-net` (network code for those two), `skipper-import` (reads outside files in a box), `skipper-hook`, `skipper-mcp`, `skipper-standin` (stand-in program for tests): helper programs and tools.
- `xtask`: the project checks (`cargo xtask ...`). `tools/skipper-cap` (follows with the code): the capped launcher and the push gate. `scan`: the allow list of committable paths. `supply-chain`: pins, audits and notices.
- `docs`: the guides, the changelog, the glossary and the brand files; the design (`docs/design`, follows with the code). `spikes` (follows with the code): Stage 0 probe programs.

Read the tree before you assume where something lives, and match the style of the code around your change.
