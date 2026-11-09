# Tested Claude Code versions

Status as of 0.1.0-test.6 (2026-11-09): Skipper is not released. No probe has passed, so no version of Claude Code has been proven with Skipper on a real Windows machine. This page says what is known instead, and where each number comes from. It is read from the code and its data files, and a test of the repository checks the three numbers below against them.

The other pages: [user guide](user-guide.md), [install guide](install.md), [probe guide](probe-guide.md), [changelog](CHANGELOG.md).

## The versions

| What | Version | Where it is read from |
|---|---|---|
| Lowest accepted | 2.1.295 | `MIN_VERSION` in `crates/skipper-provider/src/version.rs` (it follows with the code; documents first) |
| Settings list checked up to | 2.1.318 | `checked_up_to` in `crates/skipper-core/data/cc/catalogue.toml`, with the extracts in `xtask/data/cc-src/2.1.318` (they follow with the code; documents first) |
| Proven by a probe | none | `crates/skipper-core/data/probe-facts.toml` (it follows with the code; documents first): every fact reads "not run" |

### Lowest accepted: 2.1.295

2.1.295 is the first version that knows the hook setting `onFailure: "block"`. Skipper's checks of a role's tool calls run as a hook, and Skipper needs a call to be blocked when that hook cannot run. An older version would let it through, so Skipper refuses it. One more thing that Skipper relies on comes earlier and is covered by the same floor: no sign-in prompt at setup for an account that is signed in (2.1.290). Auto mode as the starting mode (2.1.283) is not relied on in 1.0: Skipper switches auto mode off in every role (design MC-503). The header shows "needs Claude Code 2.1.295 or newer", and a version text that Skipper cannot read is refused too. No role starts then.

There is no upper limit. A newer version is accepted.

### Settings list checked up to 2.1.318

Skipper keeps a catalogue of Claude Code's settings, with a help text for each. On 2026-11-05 the project read the public documentation pages of Claude Code 2.1.318 (settings, environment variables, command line) and its public settings schema, and set the catalogue against them. This is a check of documents. It is not a run of Claude Code.

When the installed version is newer than that, the Claude Code settings page says "Claude Code <installed> is installed. The list of settings was checked up to version <checked>, so newer settings may be missing." Settings that came with a version you have not seen before are grouped under "New in Claude Code <version>".

### Proven by a probe: none

The probes that need a real Windows machine and a real Claude Code have not been answered. `probe-facts.toml` (it follows with the code; documents first) ships every fact as "not run". A feature that depends on a fact that has not passed stays off or shows "not verified". The [probe guide](probe-guide.md) says how a fact changes.

## A check on your own PC

At the first start that you make after the installed Claude Code changed its version, Skipper may run one short session of it in a probe folder. It sends no prompt and types nothing. It runs for at most 15 seconds, only under your own click, and only on an account that no role and no sign-in uses. It records what it saw for that exact version in a local file, and never runs again for the same version.

That check is switched on by the probe PRB-105 of the project. While that probe is not answered, the check is skipped and the rows stay marked as documented.

## Updating Claude Code

Skipper never installs or updates Claude Code. A role in the protective box is started with Claude Code's own updater switched off, so that it cannot replace its own program folder. Update Claude Code yourself, outside Skipper, with no role running.

## For maintainers: adding a version

1. Put the extracts of the new version into a new folder `xtask/data/cc-src/<version>`, the way `SOURCES.md` in the existing folder describes (they follow with the code; documents first): facts only, no prose, and the schema with its notice.
2. Run the catalogue diff of the project for that version over that folder; the tool follows with the code (documents first). It changes the catalogue file in the one way that cannot hurt: a new key becomes a row that nobody curated, a key that the documentation drops gets an end version, and a curated row is not touched.
3. Run the probe sheets for the version, as the [probe guide](probe-guide.md) says. Only a signed probe moves a fact to passed.
4. Update the numbers on this page, the [changelog](CHANGELOG.md) and the README.

The workflow `.github/workflows/catalog.yml` (added with the code) has a job that fetches the pages of Claude Code's documentation every day once the owner has switched it on; the catalogue diff that reads them follows with the code. The job only reads.
