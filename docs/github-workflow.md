# Working on GitHub

**Status as of 0.1.0-test.6 (2026-11-09):** Skipper is in active development and not released, and contributions are not open yet (see [CONTRIBUTING.md](../CONTRIBUTING.md)). Until that file says "open", only the maintainer and the people the owner has invited can open pull requests; everyone else can read, comment and open issues. This page is the working agreement of the repository. It binds the invited people now and everyone once contributions open. (maintainer's call)

This page says how work on this repository goes, the way an open-source project or a company team runs it: an issue, a branch, a small pull request, green checks, a review, a squash merge. It is written for people and for AI coding tools alike. An AI tool that works in this repository reads this page and [AGENTS.md](../AGENTS.md) first and follows both.

## How to read this page

- **MUST** is a rule without an exception: a pull request that breaks a MUST is not merged. **SHOULD** is a rule that holds unless you have a reason, and then you give the reason in the pull request.
- This page never weakens another rule. [CONTRIBUTING.md](../CONTRIBUTING.md) says what may be submitted, [GOVERNANCE.md](../GOVERNANCE.md) says who decides, [SECURITY.md](../SECURITY.md) says how to report a vulnerability, and the [release guide](release-guide.md) says how a release is made. Where two texts differ, the stricter one holds, and the difference is reported in an issue so that it gets fixed.
- The words are those of the [glossary](glossary.md): an "issue" is a GitHub issue, and a unit of work on Skipper's board is a task. "The maintainer" is the person named in GOVERNANCE.md, not the Skipper role of the same name.
- A decision of this page is a (maintainer's call) unless it is marked (owner decision): anyone may challenge a maintainer's call with a reason in an issue, and only the owner changes an owner decision.

## The short version

1. Open an issue, or pick one.
2. Make a branch named `type/short-topic` from an up-to-date `main`.
3. Commit small, with `git commit -s`.
4. Open one small pull request with one topic (within the size limit, rule 4) that says `Closes #n` and fills the template. Split bigger work into several pull requests.
5. Get every check green.
6. The maintainer reviews. You answer every comment.
7. The maintainer squash-merges. The branch is deleted.

## The rules

### 1. Every change starts from an issue

- **MUST** Every change starts from a GitHub issue: a bug report, a piece of design feedback, or an issue that the maintainer opened for a piece of work. Use the issue forms; blank issues are off. Only a fix of a typo or of a broken link needs no issue; anything that changes a meaning does.
- **MUST** Search the open and the closed issues first, and keep to one topic per issue.
- **MUST** A change of the design starts as design feedback and ends in a pull request of its own (CONTRIBUTING rule 3).
- **SHOULD** Say in a comment that you want to take an issue, and wait for the maintainer's answer before you start more than a few lines of work, so that two people do not do the same work.

### 2. Branches

- **MUST** Never commit to `main` and never push to it. Every change is made on a branch. Someone without write access works in their own fork.
- **MUST** Name the branch `type/short-topic`: lower case, ASCII, words joined by hyphens. The type is one of `fix`, `feat`, `docs`, `test`, `refactor` or `chore`. For example `fix/sign-in-browser`, `docs/release-guide-links` or `feat/export-csv`.
- **MUST** The name holds neither "claude" nor "anthropic" in any letter case (CONTRIBUTING rule 11), and no personal data.
- **SHOULD** Start from an up-to-date `main`, keep one branch per issue and keep it short-lived. When `main` has moved on, rebase the branch instead of merging `main` into it.
- **MUST** Never reuse the name of a merged branch.

### 3. Commits

- **MUST** Every commit carries a `Signed-off-by` line (`git commit -s`, CONTRIBUTING rule 2). It is the Developer Certificate of Origin, not a cryptographic signature. The name and address in it are the contributor's choice and are public (the no-reply address is recommended); the ban on e-mail addresses in files (AGENTS.md rule 6 and the leak scan) covers file content, not this trailer.
- **MUST** The message is English. Its first line is one imperative sentence that says what the commit does, for example "Give the README a hero banner" or "Let the update gate tell a readiness check from a service that runs". It **SHOULD** fit in about 72 characters and has no full stop at the end.
- **SHOULD** A body, after a blank line, says why the change is needed and what you checked. Wrap it at about 72 characters.
- **MUST** The only trailer is `Signed-off-by`. No `Co-authored-by`, no session link, no `Reviewed-by` or similar line; the project's commit check (`xtask`) refuses them, for everyone including the maintainer and tools; it is run by hand until it runs on every pull request. Today it still compares the trailer with the maintainer's own identity, so the maintainer reviews a contributor's trailer by hand until the check is changed to accept the documented shape. Git reads a last paragraph that consists only of `Word: text` lines as trailers, so end the body with an ordinary sentence.
- **MUST** No credentials, tokens or personal data in a message (CONTRIBUTING rule 8).
- **SHOULD** One logical change per commit. Where behaviour changes, put the test in a commit before the fix.

### 4. Pull requests

- **MUST** Large pull requests are not allowed. Work is done in several small pull requests, so that the maintainer can accept some and reject others. This holds for every author, the maintainer included, and for every AI tool. (owner decision)
- **MUST** One topic per pull request (CONTRIBUTING rule 3): a pull request holds one change that can be accepted or rejected on its own, never a mix of topics. A thing you find on the way becomes a new issue.
- **MUST** The size limit is 400 changed lines per pull request, tests included, counting the lines added and the lines removed, in code and in documents. `Cargo.lock` and other generated files are not counted. The maintainer may allow a one-off exception, in writing in the issue before the work starts. This page sets the number; other texts (the task form, the code arrival page) repeat it and must match it, and a text that differs is reported in an issue (see "How to read this page").
- **MUST** A larger change is split into several pull requests that each build and pass every check on their own. They are opened in sequence, or stacked (each one based on the one before), and every one of them links the issue: `Refs #n`, and `Closes #n` in the last one.
- **MUST** Refactors, renames, formatting and behaviour changes go in separate pull requests, never together. A fix, a refactor and a reformat are three pull requests.
- **MUST** The description links its issue: `Closes #n` when the merge finishes the issue, `Refs #n` when it does only a part. A typo fix without an issue says so.
- **MUST** The title has the form of a commit's first line. Fill in every section of the template, never delete one, and tick a box only when it is true.
- **MUST** A pull request that touches a safeguard, a workflow or judge code (rule 5) says so in its description (for a safeguard, in the Safeguards section of the template), and names the rule it follows (CONTRIBUTING rules 7 and 10).
- **MUST** A pull request that is too big or mixes topics is not reviewed. The maintainer closes it or asks the author to split it, and answers with this sentence: "Thank you for your work; this pull request is too big or mixes topics, so please split it into smaller pull requests with one topic each (rule 4 of docs/github-workflow.md), and I will gladly review them."
- **MUST** An AI tool splits its work the same way and never opens one large pull request. It plans the parts before it starts, and every part is its own branch and its own pull request (rule 13).
- **MUST** Until the first workflow exists, no check runs on its own: the author runs the checks by hand and says in the description which ones were run, and the maintainer checks the rest by hand. Do not claim a check that was not run.
- **SHOULD** The limit is a ceiling, not a target: aim for a pull request that can be reviewed in one sitting.
- **SHOULD** Open a pull request as a draft while the work goes on, and mark it ready only when every check is green.

### 5. Checks

- **MUST** Every check is green before you ask for a review, and nothing is merged while one is red (GOVERNANCE.md). The checks are listed in section 2 of the [release guide](release-guide.md). Run the ones that apply to your change and say in the pull request which you ran and what they printed; the maintainer runs the rest on the build machine before merging.
- **MUST** Never skip, disable, weaken or work around a check or a test to get green. That means no `#[ignore]`, no deleted or loosened assertion, no edited expected value that fits a wrong result, no new lint allowance without a stated reason, no relaxed pin. A red lock test or a red check tool fails the build, and there is no override (CONTRIBUTING rule 3).
- **MUST** A change of judge code is named in the description. Judge code is `xtask/`, `tools/` (it follows with the code; documents first), `scan/`, `supply-chain/`, `.cargo/`, `rust-toolchain.toml`, `Cargo.lock` and every `Cargo.toml`. A reviewer who did not write the change must read its diff and accept it, as the leak-scan row of the release guide says.
- **MUST** A red test that your change did not cause is reported in an issue. It is not hidden.
- **SHOULD** Where code exists, a change of behaviour comes with its tests (CONTRIBUTING rule 3).

### 6. Review

- **MUST** Every pull request that someone other than the maintainer opens is read by the maintainer and merged only after the maintainer has approved it on GitHub. That is at least one maintainer's approval.
- **MUST** An author never merges their own pull request without a review by someone else. Someone without write access cannot merge at all. The one exception is the sole maintainer, as the fifth point of this rule says.
- **MUST** Every review comment is answered: it is fixed, or it is declined with a reason. A review thread is resolved by the reviewer when satisfied. No thread is open at the merge, and an author does not resolve a thread to hide an open point.
- **MUST** The maintainer reads again what was pushed after an approval, before the merge. An approval covers the commits it was given for.
- **MUST** While there is exactly one maintainer, nobody else can approve the maintainer's own pull request, so the repository does not demand a second approval there. The ruleset says this with required approvals set to 0 (rule 7). That pull request still goes through the same checks, and the maintainer reads the whole diff on the pull request page before the squash. Once GOVERNANCE.md names a co-maintainer, a maintainer never merges their own pull request without another maintainer's approval.
- **MUST** A review by an AI tool is advice. It is not an approval, and the maintainer decides (rule 13).
- **SHOULD** Reviewers give reasons and point to the line. Authors answer calmly. Disagree once, with reasons, and then accept the decision (code of conduct).
- **SHOULD** Do not force-push a branch while someone reviews it, unless you say so in a comment.

### 7. Merging

- **MUST** Squash and merge is the only method. `main` has a linear history, one pull request makes one commit, and there are no merge commits.
- **MUST** The squashed commit follows rule 3: an imperative first line, a body that says why, the author's `Signed-off-by` line, or every author's line when there are several, and no other trailer. The maintainer checks the text on the merge screen, because GitHub proposes its own.
- **MUST** No force-push to `main`, no deletion of `main` and no direct push, for everyone, the owner included. The ruleset of the repository requires a pull request and a linear history, and forbids force-push and deletion. It sets required approvals to 0 while there is exactly one maintainer and to 1 once a co-maintainer exists. The review by a code owner (see `.github/CODEOWNERS`) is required only once a co-maintainer exists; until then CODEOWNERS is informational, because an author cannot approve their own pull request and the requirement would block the sole maintainer. It has no bypass list: the sole maintainer merges their own pull request through the normal merge button, which the value 0 allows, and notes in that pull request that no second reviewer existed (the one allowed deviation, rule 6). Only the maintainer has write access; invited collaborators work from forks. The ruleset is set up and ticked off in the checklist "Before the repository goes public" of the [release guide](release-guide.md). Until it is in place, this rule binds by this page alone. The one exception was the first upload of the documents, which was a single reviewed seed commit made once; everything after it follows these rules, the size limit of rule 4 included.
- **MUST** Only the maintainer merges, and the branch is deleted after the merge.
- **MUST** Stale and rejected pull requests: a pull request with no answer from its author for 30 days after a review comment is closed, and a closed pull request always carries a reason in a comment. The author may reopen it or open a new one when ready.

### 8. Labels and triage

- **MUST** Follow the labels and the rules of triage in CONTRIBUTING rule 9. They are not repeated here. The issue forms add `needs-triage` and the type label by themselves, and the labels belong to the maintainer: do not create, rename or delete labels.
- **SHOULD** Answer a `needs-info` question. After 30 days of silence the maintainer closes the issue, and it can be reopened when the information arrives.
- **SHOULD** Be patient. Replies are best effort, there is no fixed deadline, and nobody is pinged over and over (SUPPORT.md, code of conduct).

### 9. Versions, releases and the changelog

- **MUST** Versions follow semantic versioning, `major.minor.patch`, and a test build adds a part such as `-test.3`. The version is set in one place, `[workspace.package]` of the root `Cargo.toml`. An ordinary pull request does not change it and does not change a status line "as of <version>"; the release does (release guide, section 1).
- **MUST** Every change that a person can notice adds an entry to the [changelog](CHANGELOG.md), in the section of the current version, in the same pull request. The entry says what a person notices first.
- **MUST** Only the maintainer makes tags and releases, after the checks of the release guide. A release is an immutable release, published only on this repository's Releases page, with the SHA-256 checksum of every file in its notes (release guide, section 5).
- **MUST** The words "Claude" and "Anthropic" never stand in a tag, a release title or a release file name, and "official" is not placed next to Skipper (CONTRIBUTING rule 11). Nothing is published to crates.io or another registry.
- **MUST** A wrong release is never edited or deleted. A fixed version is published, and its notes say what was wrong (release guide).

### 10. Security and private data

- **MUST** A vulnerability is never reported in a public issue, pull request, discussion or commit message. Use GitHub's private vulnerability reporting, as SECURITY.md describes (if the button is missing, SECURITY.md has the fallback), and publish no details before a fix exists. **SHOULD** Do not open a pull request for it until the maintainer asks.
- **MUST** No credentials, tokens, session transcripts or personal data anywhere: not in code, tests, issues, pull requests, commit messages, screenshots or recordings. Test data comes from dummy sessions only (CONTRIBUTING rule 8).
- **MUST** A secret that was committed by mistake counts as leaked. Revoke it first and tell the maintainer privately. A later commit that deletes it does not help, because the history keeps it.
- **MUST** Never go around GitHub's push protection or the project's scan. A refusal is a finding to fix, not an obstacle.
- **MUST** A problem in Claude Code itself goes to Anthropic first (SECURITY.md, "Problems in Claude Code").

### 11. Dependencies and GitHub Actions

- **MUST** Follow CONTRIBUTING rule 3 (the dependency point) and rule 10 (workflows). They are not repeated here.
- **MUST** A new dependency, or a new version of one, comes with a reason in the description and passes the supply-chain checks. Versions are exact pins, a new version waits 7 days, there is no git source, and every package keeps `publish = false`.
- **MUST** A workflow is added or changed only as CONTRIBUTING rule 10 allows, and the pull request names the rule it follows.
- **MUST** Everything is Rust: no Python, JavaScript, TypeScript, Node or shell-script tooling, also not for a helper script. Helper scripts are Rust, in `xtask` (CONTRIBUTING rule 12).
- **SHOULD** A pull request that a bot opens for a dependency is read and checked like any other. It is never merged without review.

### 12. Documentation moves with the change

- **MUST** A pull request that changes what Skipper does updates the documents that describe it, in the same pull request: the README table "What is built today", the user guide, the install guide, the privacy page, the glossary for a new word, the design with its decision marks (CONTRIBUTING rule 6), and the changelog (rule 9).
- **MUST** The tests that pin sentences of the public documents (`crates/skipper-core/tests/public_docs.rs`, added with the code) are updated in the same pull request when a pinned sentence changes. A pin is never loosened or deleted to get green.
- **MUST** New text is English and uses the glossary's words (CONTRIBUTING rules 1 and 5), and it is written in your own words (CONTRIBUTING rule 4).

### 13. Work with AI tools

AI-assisted work is allowed. These rules hold for every tool and every way of using one.

- **MUST** The human who submits is responsible for every line, as if they had written it by hand. The `Signed-off-by` line is that person's own statement, and a tool cannot give it.
- **MUST** The submitter understands and has tested every line before it is submitted. Output that nobody has read and run is not submitted.
- **MUST** Say in the pull request description that AI help was used: the template line "AI assistance used: yes/no" and, for yes, which tool and for what. The disclosure stands in the description, not in a commit trailer (rule 3).
- **MUST** The origin rules apply to what a tool writes: no copied code, text or wording from other projects (CONTRIBUTING rule 4).
- **MUST** An AI tool that works in this repository reads [AGENTS.md](../AGENTS.md) at the start and follows it and this page.
- **MUST** An AI tool never pushes to `main`, merges, tags, publishes a release or changes a repository setting. It opens a pull request, posts a comment, a review or an approval in a person's name only after that person has read the exact text and said yes. This is the third hard lock of the design (nothing reaches the main branch or the internet without a person's yes) in the form of a working rule.
- **MUST** No mass-produced or automated posts that no person has read before sending. They are closed without review (code of conduct).
- **MUST** An AI-made change is as small as any other: the limits of rule 4 hold for it in full, and a tool splits big work into several pull requests and never opens one large one.
- **SHOULD** The tool asks the human when it is unsure instead of guessing.

## Changes to this page

A change is a pull request with a reason, like any other, and it never weakens CONTRIBUTING.md, SECURITY.md or GOVERNANCE.md. A change that decides who may merge or approve also changes GOVERNANCE.md and needs the owner. An AI tool changes this page, AGENTS.md and `.github/copilot-instructions.md` only when a person tells it to in an issue or in the pull request.
