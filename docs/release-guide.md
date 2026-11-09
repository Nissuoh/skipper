# Release guide

Status as of 0.1.0-test.6 (2026-11-09): for maintainers. Nothing is published yet. This guide lists the steps that the code and the design support, so that the first release follows them. The owner decided on 2026-11-09 that the documents are published first, then the source in small reviewed pull requests, and that an outside review is asked for after that; when the first release is published is the owner's decision (see [GOVERNANCE.md](../GOVERNANCE.md) and section 17.6 of the design document, which follows with the code).

The other pages: [install guide](install.md), [probe guide](probe-guide.md), [tested Claude Code versions](tested-versions.md), [changelog](CHANGELOG.md).

## Rules that bind every release

- **One source.** Builds are published only on the Releases page of this repository. Nothing goes to crates.io or another registry, and every package sets `publish = false`.
- **One build for everyone.** Once the source is in `main`, a test build is the same code as `main`, built the same way, from the public source only. Test builds are not distributed; only releases on the Releases page are. There is no separate build for the maintainer.
- **Names.** "Claude" and "Anthropic", in any letter case and inside longer words, never appear in a release title, a tag, a tag text or the name of a release file. No text places the word "official" next to Skipper. What lives outside the repository (the repository description, the topics, the release title and the release notes written on GitHub) is checked by hand against these rules before it is set (design section 17.5, MC-163 item 4).
- **Publication, and before the first release** (design section 17.6; owner decision, 2026-11-09): the documents are published first in the fresh repository `Nissuoh/skipper`, then the source in small reviewed pull requests, and an outside review is asked for after that, in writing (design section 9.3). At the visibility change the cleanliness review is done: a full scan of the published files and their whole history, a review of the publishing account's profile for unintended links, and GitHub secret scanning and push protection switched on. Before the first release, as a recommendation that the owner decides: the product has been stable in real use for 14 days; a stranger got along with the README alone; the name has been checked as a trademark; the written inquiry has been answered; and the pages of [policy-sources.md](policy-sources.md) have been read again.
- **Only through the capped launcher.** Builds and checks run through `skipper-cap` on the build machine of the design (section 16.5), which has no personal sign-ins. `cargo xtask` refuses to run outside it.

## Before the repository goes public

The settings below are made on github.com and are not files, so no check in this repository can prove them. Do them in this order. Write the date next to each one when it is done and verified, and announce no link before every line has a date.

1. [ ] Private vulnerability reporting is on. Verify in a logged-out window that "Report a vulnerability" shows on the Security tab. Date: ____
2. [ ] Secret scanning and push protection are on; the dependency graph, Dependabot alerts and security updates are on. Date: ____
3. [ ] The ruleset on `main` requires a pull request and a linear history, and forbids force-push and deletion. Required approvals: 0 while there is one maintainer, 1 once a co-maintainer exists. The code-owner review is not required while there is one maintainer (CODEOWNERS is informational then); switch it on when a co-maintainer exists. There is no bypass list. Verify that the maintainer can merge their own pull request. Add the required status checks once a workflow exists. Date: ____
4. [ ] Only squash merge is allowed, with the pull request title and description as the message. Merge commits and rebase merging are off, and head branches are deleted after the merge. Date: ____
5. [ ] Only the maintainer has write access. Invited collaborators have triage or read access and work from forks. Pull request creation is limited to collaborators if the plan offers that option. Date: ____
6. [ ] Actions are disabled or restricted, and the default workflow token is read-only. Date: ____
7. [ ] Discussions, the wiki and projects are off. Blank issues are off (`.github/ISSUE_TEMPLATE/config.yml`). Date: ____
8. [ ] Every label of CONTRIBUTING.md rule 9 exists, so the issue forms can apply them. Date: ____
9. [ ] Tag protection and immutable releases are on. The Releases page is the only trusted source. Date: ____
10. [ ] The repository description, the topics and the social preview say that Skipper is independent and not affiliated with, endorsed by or sponsored by Anthropic. The same line is in the release notes template and in the descriptions of the community spaces. Date: ____
11. [ ] The owner account uses two-factor sign-in with a hardware key, the recovery codes are stored offline, and a recovery route is planned. Date: ____
12. [ ] After the repository is public: click every link in the README and in `config.yml`, and check that the community invite link does not expire and that the Discord moderators are named and bound by the code of conduct. Date: ____

## 1. Prepare the repository

1. Set the version in `[workspace.package]` of the root `Cargo.toml`. Every crate inherits it.
2. Update every status line "as of <version> (date)". The README holds the one full status banner; every other file keeps a short line that points to it. The files are: the README, SECURITY.md, SUPPORT.md, CONTRIBUTING.md, GOVERNANCE.md, CODE_OF_CONDUCT.md, the glossary, the working agreement `docs/github-workflow.md` and every user document in `docs/`. A test, `crates/skipper-core/tests/public_docs.rs` (it follows with the code; documents first), fails when one of them lacks the version.
3. Add a section `## <version>` to the [changelog](CHANGELOG.md). Update the README's table "What is built today" and the page of [tested Claude Code versions](tested-versions.md).
4. While no release exists, the README says "There is nothing to install yet" in its status line and "There is nothing to download yet" in "Where to get Skipper". Change both, and the rest of "Where to get Skipper", in the same change that publishes the release, and not before.
5. The update check needs a release source. The `repository` key in `crates/skipper-update/Cargo.toml` (it follows with the code; documents first) is empty until the first release is published. It takes `https://github.com/<owner>/<repo>`. The key also decides where a build looks for updates: with the key empty, every build, a test build or a plain version, looks into the person's test folder (the folder the person chose under "Test folder" in the settings) and asks nobody over the network; with the key set, a build uses the release list and reads no folder. Filling the key therefore switches every build from the folder to the release list.

## 2. Run the checks

Through `skipper-cap run -- <command>`, from the workspace root:

| Command | What it checks |
|---|---|
| `cargo xtask pin-check` | Exact version pins, the 7-day wait for every new version, the checksums of the pinned files, no git source in `Cargo.lock`. |
| `cargo xtask deps-check` | No HTTP, TLS or async crate in `skipper.exe` or in the sign-in helper, none of the HTTP crates below the import helper, no blocked crate, and the third-party licence text. |
| `cargo xtask supply` | `cargo-deny`, `cargo-audit` and `cargo-vet` at their pinned versions, offline. It fails when a tool or the advisory database copy is missing or too old. |
| `cargo xtask provider-name-check`, `lang-lint`, `template-lint`, `skill-lint` | The core names no provider, the language files agree, the starter team and the skills follow their rules. |
| `cargo xtask source-lint` | Who may start a process, open a file or use the network, by file. It also runs the vault and network rules. |
| `cargo xtask leak-scan --range <base>..<head> --rules-from <commit>` | Banned terms, e-mail addresses, expanded profile paths, secrets, brand words in names, imported pages. If any commit of the range changes judge code or judge input (`xtask/`, `tools/` (it follows with the code; documents first), `scan/`, `supply-chain/`, `.cargo/`, `rust-toolchain.toml`, `Cargo.lock` or any `Cargo.toml`), it also reports a judge-change finding and fails, unless `--accept-judge-change <tip>` names the full id of the tip of the range (`<head>`), which a reviewer who did not write the change adds after reading its diff. The scan's settings live outside the repository. `<commit>` is the full id of the reviewed commit whose allow list and pin you trust: for a push, the tip of the remote's main (run `git fetch` first, then `git rev-parse origin/main`; the remote-tracking ref is only as fresh as the last fetch, and with an old one the scan judges by an older main); before the first publication, when the remote has no main yet, the reviewed head the owner signed off; never a commit of the range being judged, unless it is that reviewed head. |
| `cargo test --locked --offline --workspace --no-fail-fast` | The whole test run, with no feature turned on. Build the two programs that some tests start first (`cargo build -p skipper-standin -p skipper-update`, also through `skipper-cap run`); those tests fail without them. Many window tests of `skipper-app` compile to zero tests in this run, which is why the next row exists. |
| `cargo test --locked --offline --no-fail-fast -p skipper-app --features fixtures` | The window tests that need the `fixtures` feature (they carry `#![cfg(feature = "fixtures")]`, some together with `windows`). Look for `running N tests` with N above zero in those files. Keep the row above as it is: `shipped_no_fixtures.rs` (it follows with the code; documents first) runs only without the feature. |

All of them must be green. There is no override.

## 3. Build the release files

1. Build the release programs for `x86_64-pc-windows-gnu` with `--release --locked`, so that the `release` folder of that target holds the six programs: `skipper.exe`, `skipper-gateway.exe`, `skipper-hook.exe`, `skipper-import.exe`, `skipper-mcp.exe` and `skipper-update.exe`.
2. Run `cargo xtask dist --stage 1`. It makes the folder `dist/stage1`:
   - the six programs, and the folders `data` (the starter team), `lang` and `licenses` (it holds `LICENSE`, Skipper's own licence, and `THIRD-PARTY-LICENSES`, the licence texts, copyright lines and notices of every third-party package that ships, in the layout of the notice list, which follows with the code);
   - the pack `skipper-<version>.pack`, which holds all of those and which the program checks again before it swaps anything;
   - the setup file `skipper-setup-<version>.exe`, a copy of `skipper.exe` that the program takes for the installer by its name;
   - `SHA256SUMS.txt`, which lists the programs, the pack and the setup.
3. The same step checks every program with `pe-check` (only the sign-in helper and the update helper may import a network library, every program must have its hardening flags, and no program may be larger than 64 MiB, which catches an accidental debug-info or unoptimised build; the limit was 40 MiB until 2026-11-08), checks that the folder holds exactly this set, and reads the installer's sources for start-up entries, services, scheduled tasks, machine-wide places and requests for administrator rights. It stops with the reason if anything is wrong. `cargo xtask installer` runs the installer part alone.

   The licence file is made in the same step and fails closed. While a shipped package has no licence text, `dist` stops, names the package and writes no file. A published crate often ships no text; its text is then committed under `supply-chain/notices/upstream/<package>/`, one file per text, the first line `Source: <where>`, then the text exactly as upstream has it (the notice list, section 5.3, says which texts are there and where they came from). `dist` also stops, naming the package and the file, while a required extra block of the notice list (section 5.2: the Chromium, Vyukov, Adobe and libm blocks) is missing from that folder. After every change of `Cargo.lock`, run this step again and read the packages it names: a new package without a text needs its text committed before the release.
4. Two builds of one commit are meant to give the same bytes. The probe PRB-63 compares them; it counts only once its answer and its evidence file are in the repository.

## 4. Check it on a real PC

Do not publish a build that has only been checked on the build machine.

1. Compare the SHA-256 of every file in `dist/stage1` with `SHA256SUMS.txt` (`certutil -hashfile <file> SHA256`).
2. In a fresh user profile, install with the setup, start Skipper, then uninstall. Nothing of Skipper's but the data should be left. The [install guide](install.md) says what to expect.
3. Run the probes that the release depends on and record them, as the [probe guide](probe-guide.md) says. A probe that failed takes its fallback and is never run again to get a yes.

## 5. Push and publish

1. Push only through `skipper-cap push`, on the main PC and not on the build machine, which has no sign-ins. It scans first and then pushes exactly the commit it scanned. The scan is never judged by the commits that are being pushed, so the command needs three things besides the remote and the branch: `--scanner <xtask.exe>`, the scanner program that was built on the build machine, `--sha256 <hex>`, that program's checksum, and `--from <commit>`, the tip of the remote's `main` (the push fetches the remote first and refuses any other commit). The rules of the scan (the allow list and the pin of the secret scanner) come from that commit and from nothing else. The scanner must be recorded for that commit first: the step that brings it over from the build machine runs `skipper-cap scanner-add <remote> <commit> <xtask.exe> --sha256 <hex>`, which accepts it only if the file has the given checksum, its receipt names the same commit and checksum, and the commit is the tip of the remote's `main`. A push whose scanner is not recorded, or whose `--from` is not that tip, is refused. A push that changes the scanner or other judge code stops at the scan unless the person adds `--accept-judge-change <the commit that is pushed>` after reading the diff.
2. On GitHub, create the release as an **immutable release** on the tag `<version>` (a leading `v` is fine), with a title and notes that follow the rules above. Add `skipper-setup-<version>.exe`, `skipper-<version>.pack` and `SHA256SUMS.txt`, and the files of the portable folder as the owner decides. List the SHA-256 checksum of every file in the notes.
3. The update check reads the release list by itself while the person's setting "Automatically check for updates" is on (it is on by default), when Skipper starts and every 6 hours, and when the person clicks "Check now". It offers a release only if its tag is a plain `major.minor.patch` version (a part such as `-test.1` makes it no version) and it is neither a draft nor a pre-release. So a test build is never offered through the release list. The check downloads `SHA256SUMS.txt` and `skipper-<version>.pack` by these exact names, and the pack only after the person's click. The test folder that a build without a release source reads has the same layout: `skipper-<version>.pack` and `SHA256SUMS.txt` side by side. A folder build must carry its checksum file too, or nothing is offered from that folder, and only a version higher than the one that runs is ever offered.
4. The model and settings catalogues are not part of a release. They come from a separate rolling release called `catalog`. The workflow `.github/workflows/catalog.yml` (added with the code) builds it, and stays off until the owner switches it on.
5. After publishing, download the files from the release page and repeat the checksum comparison of section 4. Then update the README's "Where to get Skipper".

## If a release is wrong

A published immutable release cannot be changed. Publish a fixed version instead, and say in its notes what was wrong. Never delete a release to hide a problem, and report a security problem as [SECURITY.md](../SECURITY.md) describes.
