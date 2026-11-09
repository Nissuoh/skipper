# Instructions for AI coding tools

Read `AGENTS.md` at the root of the repository before you change anything, and follow it. It holds the build and test commands, the hard rules and a map of the code.

Also follow `docs/github-workflow.md` (issue, branch, small pull request, green checks, review, squash merge) and `CONTRIBUTING.md`.

In short: work from an issue, on a branch named `type/short-topic`. Keep every pull request small: one topic, within the size limit of rule 4 of `docs/github-workflow.md`, and split larger work into several pull requests, because you must never open one large pull request. Add tests. Never weaken a check or a safeguard. Never commit secrets or real data. Copy nothing from other projects (no code, text, images or wording); write your own. Never write "Claude" or "Anthropic" in a file, package or branch name. Write English only. The project is Rust only: add no Python, JavaScript, TypeScript, Node or shell-script file or tool. Sign off every commit (`git commit -s`) and add no other trailer. Say in the pull request that AI help was used. Do not push, merge, tag, release or post on anyone's behalf unless a person told you to. If you are unsure, ask the person.
