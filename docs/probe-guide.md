# Probe guide

Status as of 0.1.0-test.6 (2026-11-09): for maintainers. No probe has an answer that counts; every probe of Stage 0 and Stage 1 is open. This guide says what a probe is, where the probe files are, how to run one and how an answer is recorded, as the code and the files in `docs/probes` have it (they follow with the code; documents first).

The other pages: [release guide](release-guide.md), [tested Claude Code versions](tested-versions.md), [privacy](privacy.md).

## What a probe is

A probe is a check that can only be settled on a real Windows machine, often with a real Claude Code. Statements in the design and in [SECURITY.md](../SECURITY.md) that are marked "probable" or "unverified" wait for one. Where a probe contradicts a maintainer's call, the probe wins and the call is reopened.

A probe answers yes or no. A no is a valid answer. Do not run a probe again to get a yes. A feature whose probe has not passed stays off, or shows "not verified", or takes the fallback that the probe's row names.

## Where the files are

| File | What it is |
|---|---|
| `docs/probes/stage-0/checklist.md` | The real-Windows checklist of Stage 0: one item per check, a result line under each, and the signature at the end (it follows with the code; documents first). |
| `docs/probes/stage-0/results.md` | The Stage 0 answer, built from the signed checklist (it follows with the code; documents first). |
| `docs/probes/stage-1/checklist.md` | The checks of Stage 1, one `### <ID>` item each, with what to run, what to look at, what is expected, what evidence to keep and the fallback (it follows with the code; documents first). |
| `docs/probes/stage-1/results.md` | One row for each item of that checklist, and the signature (it follows with the code; documents first). |
| `docs/probes/stage-1/*.md` (the other five) | The step lists of the longer probes: `cc-facts.md`, `early-helpers.md`, `early-duties.md`, `permission-relay.md` and `lead-first.md` (they follow with the code; documents first). |
| `docs/probes/stage-1/evidence` | The evidence files of the answers (they follow with the code; documents first). |
| `crates/skipper-core/data/probe-facts.toml` | The facts that ship with the program. Every fact reads "not run" until a signed answer is copied in (it follows with the code; documents first). |

## Running a probe

- Probes that need Windows run with the build under test, from `dist/stage1`, on Windows 11. A run on the build machine never replaces an item that needs a real PC.
- A probe of a feature that is off in every shipped build needs a probe build. The Windows Hello layer of the vault (PRB-96) is built but off: the entry of PRB-96 in `docs/probes/stage-1/checklist.md` (it follows with the code; documents first) says how to make a probe version with the one flag, `HELLO_LAYER_BUILT`, set to `true`. The flag is set in the shipped source only in the change that records a passed PRB-96, and SECURITY.md changes in the same change.
- The program has probe commands for the hand steps: `skipper.exe probe simulate`, `isolation`, `attach`, `dll`, `rollback`, `claude-hash`, `orphans`, `helpers-check`, `permission-relay`, `kit-make`, `duties-check` and `synced-manifest`, and `probe window` and `probe soak`. Each probe sheet says which ones it needs and with which flags. The lines of a probe show in the Skipper window under "Probe output" (with "Copy all") and in `%LOCALAPPDATA%\skipper-spike\out`.
- A sheet with a real Claude Code uses an account of your own, a folder only for the probe and dummy data. Never use real project content.
- Follow the sheet step by step. If you see anything other than what a step says, stop and ask. Do not guess and do not sign.
- Never end a program by its name. A Claude Code session ends with `/exit`.
- Skipper itself never types into a session, so the sheets that need typing say who types.

## Evidence

- A file name starts with the item id in lower case, a dash and a word, such as `prb-20-isolation.txt`. It lies in `docs/probes/stage-1/evidence` (it follows with the code; documents first), and its extension is one of `txt`, `md`, `log`, `csv`, `json` or `png`.
- A file holds the printed result lines or the numbers that the item asks for, copied as printed.
- A file never holds conversation content, a token, an environment value, a PC name, a user name, an account address or an expanded profile path. Write `%USERPROFILE%`, `%LOCALAPPDATA%` and `%APPDATA%` instead.
- A screenshot comes only from a dummy session or an empty project.

## Recording an answer

Stage 1 has a checker that reads the checklist and the results:

```
cargo xtask stage1-check
cargo xtask stage1-check --record <ID> --result yes|no [--evidence <names>] [--value key=value]... [--fallback <text>]
cargo xtask stage1-check --record <ID> --result skip --reason <text> [--evidence <names>]
cargo xtask stage1-check --sign <name> --date <YYYY-MM-DD>
cargo xtask stage1-check --facts
```

- The plain run says `ok`, `ok - fallback used`, `open` or `no`, and lists the items that decide it. `ok - fallback used` is never a plain yes.
- `--record` writes a row in `results.md` (it follows with the code; documents first) and refuses an answer that the checker would call open: a yes needs every evidence file that the item names and a good value for every key it carries.
- A probe that was not run is no answer. It blocks Stage 1 until it is run, or until it is skipped with a reason. Only an item that has a fallback can be skipped. An item that says `Fallback: none` is a gate: a no there is a no.
- `--sign` signs the results. `--facts` then copies the signed values into `probe-facts.toml` (it follows with the code; documents first), as `passed` for a yes and `failed` for a no, with the signature as the source. It writes nothing for an open item and nothing at all while the results are unsigned.

Stage 0 has its own checker: `cargo xtask stage0-check --checklist <file> [--template]` reads the checklist, and with `--results <file>` it turns the signed checklist into the decision of Stage 0.

Both checkers run through `skipper-cap` and refuse a file that has been changed in shape, so that an item cannot be dropped to reach `ok`.

## After an answer

1. Change the statements that the probe settled. In [SECURITY.md](../SECURITY.md) a "probable" or "unverified" becomes a plain statement, or a limit that stays. A limit is never removed without its test.
2. Update the README's table "What is built today" and the page of [tested Claude Code versions](tested-versions.md). A test of the repository fails while the page says that no probe has passed and one has.
3. If a probe finds a security problem in Claude Code itself, report it to Anthropic first and record only "reported to Anthropic" until Anthropic has had time to address it. SECURITY.md describes the route.

## The check that Skipper makes itself

At the first start after Claude Code changed its version, Skipper may run one short session of it as a probe on your own PC. It never replaces a signed answer. [Tested Claude Code versions](tested-versions.md) says when it runs.
