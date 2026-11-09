# How the code arrives

Status as of 0.1.0-test.6 (2026-11-09): this repository holds the documents, the project rules and the check tool (`xtask/`). The Rust source code of the app is not here yet. This page says how it will arrive, so that nobody has to guess. The plan may change; the order below is (maintainer's call).

## What this means for what you read

Many pages describe the app in the present tense. Until the crate that holds a part is public, such a statement is the maintainer's claim and cannot be checked by anyone else. Do not rely on it as a tested fact. This is also why every page says "design statement" where no probe has settled a protection claim.

## The planned order

The code is added crate by crate, each step as its own pull request under the size limit of [the working agreement](github-workflow.md) (400 changed lines, one topic). A crate that is too large for one pull request is split into several, and each step must build and pass its own tests on its own. The expected order, from the base to the top:

1. The shared base: core types, the board and the knowledge base (`crates/skipper-core`).
2. The checks that guard the rules, so that later steps are held to them (`xtask/` already holds the first ones).
3. The parts that open files, start programs and apply the protective box (`crates/skipper-confine` and the process code).
4. The parts with network code, each in its own helper program: the sign-in helper (`skipper-gateway`) and the update helper (`skipper-update`).
5. The provider layer and the app itself (`crates/skipper-provider`, `crates/skipper-app`).
6. The design document and the probe sheets (`docs/design/`, `docs/probes/`), added with the crate they describe.

## How each step is reviewed

Each pull request follows [CONTRIBUTING.md](../CONTRIBUTING.md): one issue first, a sign-off, no private data. The maintainer runs the leak scan on the added paths. The maintainer reviews it, and the checks that exist at that time are run by hand until the first workflow exists. A step that changes what a document says about the code also updates that document in the same pull request.

## When it is done

When the last crate is public, every statement in the documents can be read against the code. The README banner and the "What is built today" table are then updated, and the first release follows the [release guide](release-guide.md).
