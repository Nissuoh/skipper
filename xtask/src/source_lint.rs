// SPDX-License-Identifier: MIT
//! `cargo xtask source-lint` (Stage 1 Task 33; LOCK-2, LOCK-3, ACC-3, MC-12, MC-75, MC-79, MC-273, S1G-11, MR-56):
//! the rules about who may call what, which a compiler cannot state per file.
//!
//! The lint reads every `.rs` file below `crates/<crate>/src` through the scanner that `provider-name-check` and
//! `lang-lint` share (`rust_tokens`, security M15): comments and the content of strings are never code, a folder
//! named `tests`, a file named `tests.rs`, a file that starts with a test-only `#![cfg(...)]` and every item that
//! carries a test-only `#[cfg(...)]` are not read. A `cfg` is test-only when it holds in test builds and nowhere
//! else (`test`, `all(test, ..)`); `any(test, windows)` and `any(test, feature = "fixtures")` hold in a product build
//! too, so such code is read like any other. A finding is one line, `file:line rule text`, with `file` below the
//! workspace root and `/` as separator.
//!
//! The rules match identifiers, not whole paths (security M15), so a builder type, a member of a family of names or
//! a `use ... as` rename does not get past them. A function definition (`fn from_click`) is no use of its name.
//! - `spawn-lint` (LOCK-2, MC-111): the identifiers `Command`, `CommandBuilder`, `spawn_command`, `WinExec` and every
//!   name that starts with `CreateProcess` or `ShellExecute` start a process, in the session code of the core, the
//!   Windows start code of the confine crate and the stand-in only.
//! - `door-lint` (ACC-3, S1G-11): a file access outside `FileDoor`. The identifiers `OpenOptions`, `hard_link`,
//!   `soft_link`, `symlink`, `symlink_*` and every name that starts with `CreateFile`, `DeleteFile`, `MoveFile`,
//!   `CopyFile` or `ReplaceFile` or fits `Create*Link*`, the calls `File::open`, `File::create`, `File::create_new`
//!   and `File::options`, and the functions of `std::fs` that read, look at, make, change or remove a file or a
//!   folder (`fs::read`, `fs::read_to_string`, `fs::read_dir`, `fs::metadata`, `fs::symlink_metadata`,
//!   `fs::canonicalize`, `fs::read_link`, `fs::exists`, `fs::write`, `fs::copy`, `fs::rename`, `fs::set_permissions`,
//!   `fs::remove_*`, `fs::create_dir*`; also named in a `use std::fs::{..}` group). The files in the table are the
//!   exceptions.
//! - `cred-lint` (ACC-3): a string literal that holds a credential file name of the door (`.credentials.json`, `.env`,
//!   `*.env`, `.env.*`).
//! - `person-lint` (MC-79, LOCK-3, security M16): the doors of the person's click open only in the command bus of the
//!   window: the identifiers `from_ui_click` (`PersonToken`), `from_ui_dialog` (`Confirmed`), `from_person_start` and
//!   `from_start_click` (`TeamRunGrant`), `from_person_apply` (`ReapplyGrant`, the person's Apply, CC-8), `from_person`
//!   (`ClickToken` from the person's token, Task 34b H2), and
//!   `from_click` (`ClickToken`, two named exceptions: the Stage 0 probe window and the account-safety simulation).
//!   The person's typing has its own door, `from_panel` (`PersonInput`, Task 34b H1), with three named exceptions: the
//!   terminal sync of the window, the Stage 0 probe window and the account-safety simulation (MC-12 (a)).
//!   `TeamRunGrant::for_tests` and `ReapplyGrant::for_tests` only under `cfg(test)` or a test feature
//!   (see [`TEST_FEATURES`]); a `cfg` that also holds in a product build (`any(feature = "fixtures", windows)`) does
//!   not count.
//! - `account-lint` (MC-273): a field `account` of a `RoleDef` is written (`x.account = ..` or `RoleDef { account: .. }`)
//!   only in the person's forms, in the definition's own loader and, for a role the Lead defines inside the person's
//!   envelope, in `add_for_lead` of `catalog.rs` (lead first, MC-396). A field assigned through a method
//!   (`account.take()`) is not seen. The match is by token: a field `account` of another type (a service's settings,
//!   an order, a form's draft) is a finding too, and each such place is a named exception.
//! - `protocol-lint` (MC-12, MC-162): `ProtocolLine::new` takes a string literal and nothing else.
//! - `prompt-lint` (MC-12): `StartInputs { prompt: .. }` other than `None` only in the service path of `derive.rs` (a
//!   function whose name contains `service`) and in its role start path `role_launch` (lead first, MC-388: the
//!   compile-time protocol line). Only the struct literal is seen, not a later assignment.
//! - `command-lint` (MC-242): `CoreCommand::SetSeenMark` and `UiCommand::DismissBriefing` are built only in the command
//!   bus and the briefing view model. A variant in pattern position is not a use: a match arm (`.. =>`, `.. | ..`,
//!   `.. if ..`), a `let` pattern (`if let .. = x`) and the pattern argument of `matches!` are handlers, and the
//!   core's own handler names the variant too.
//! - A `use` item that renames `fs`, `File`, `PersonToken`, `Confirmed`, `TeamRunGrant`, `ReapplyGrant`, `ClickToken`,
//!   `ProtocolLine`, `StartInputs`, `RoleDef`, `CoreCommand` or `UiCommand` (`use x::T as U`) is a finding of the rule of that name
//!   (text `T as`). The names that a rule matches by identifier alone show at the import itself.
//!
//! Known gaps, because a token stream has no types: a method call on a value is not seen (`path.exists()`,
//! `path.is_dir()`, `path.metadata()`, `path.canonicalize()`, `path.read_dir()`, `entry.metadata()` on a `Path` or a
//! `DirEntry`; also `x.account.take()`), nor is a type alias (`type F = std::fs::File;`) or a name that a macro
//! builds. A typed check (clippy's `disallowed-methods`) can close the first gap later and adds to this lint without
//! replacing it.
//!
//! Ruling (plan Task 33): an own token-based lint instead of clippy's `disallowed-methods`: clippy's list has no per-file
//! allowance, does not see string literals (`cred-lint`) and is not one of the pinned build steps. Cost if wrong: a
//! second check with clippy can be added without removing this one.
//!
//! Every exception is a line of [`ALLOWED`] with its reason (`cargo xtask source-lint --allowances` lists them). A
//! `temporary:` reason names code that predates the lint and has to move behind the door (or go) in the task named
//! there; the rest are exceptions by design.

use std::fs;
use std::path::Path;

use crate::rust_tokens::{ident_at, is_ident, is_punct, is_test_file, skip_test_item, str_at, tokenize, Token};

const SPAWN: &str = "spawn-lint";
const DOOR: &str = "door-lint";
const CRED: &str = "cred-lint";
const PERSON: &str = "person-lint";
const ACCOUNT: &str = "account-lint";
const PROTOCOL: &str = "protocol-lint";
const PROMPT: &str = "prompt-lint";
const COMMAND: &str = "command-lint";

/// Cargo features that only a test build turns on. `TeamRunGrant::for_tests` may stand under one of them (plan
/// addendum Task 33: "only under `cfg(test)` or the test feature"; the name is the builder's choice, so both
/// names that the workspace uses for test support are accepted).
const TEST_FEATURES: [&str; 2] = ["test-support", "fixtures"];

/// The identifiers that open the doors of the person's click and typing (the definitions are no use, see [`findings`]).
const PERSON_NAMES: [&str; 8] = [
    "from_ui_click",
    "from_ui_dialog",
    "from_click",
    "from_person_start",
    "from_start_click",
    "from_person",
    "from_panel",
    "from_person_apply",
];

/// The names whose `use .. as ..` rename hides a rule, with that rule. A name that a rule matches by identifier alone
/// is not listed: its rename shows at the name itself.
const RENAMED: [(&str, &str); 12] = [
    ("fs", DOOR),
    ("File", DOOR),
    ("PersonToken", PERSON),
    ("Confirmed", PERSON),
    ("TeamRunGrant", PERSON),
    ("ReapplyGrant", PERSON),
    ("ClickToken", PERSON),
    ("ProtocolLine", PROTOCOL),
    ("StartInputs", PROMPT),
    ("RoleDef", ACCOUNT),
    ("CoreCommand", COMMAND),
    ("UiCommand", COMMAND),
];

/// The door of the person's click: the only file that builds a `PersonToken`, a `Confirmed`, a `TeamRunGrant` and a
/// `ReapplyGrant`.
const BUS: &str = "crates/skipper-app/src/ui/bus.rs";

/// One exception to one rule.
struct Allow {
    rule: &'static str,
    /// A file below the workspace root, or a folder with a trailing `/**`.
    path: &'static str,
    /// Only for findings with one of these texts; empty means every finding of the rule.
    texts: &'static [&'static str],
    /// Only inside a function whose name contains this.
    in_fn: Option<&'static str>,
    reason: &'static str,
}

const fn allow(rule: &'static str, path: &'static str, reason: &'static str) -> Allow {
    Allow { rule, path, texts: &[], in_fn: None, reason }
}

/// An exception for the findings with one of `texts` only.
const fn allow_texts(rule: &'static str, path: &'static str, texts: &'static [&'static str], reason: &'static str) -> Allow {
    Allow { rule, path, texts, in_fn: None, reason }
}

/// Every exception of every rule, each with its reason. Nothing else is allowed: a new line needs a reason.
const ALLOWED: &[Allow] = &[
    // spawn-lint: the one spawn path (LOCK-2, MC-111)
    allow(SPAWN, "crates/skipper-core/src/session/**", "the core's one spawn path for sessions and its detection of the program (MC-111)"),
    allow(SPAWN, "crates/skipper-confine/src/conpty.rs", "starts a session on a pseudo console (CreateProcessW), the Windows end of the spawn path"),
    allow(SPAWN, "crates/skipper-confine/src/appcontainer.rs", "starts a program in an app container, the Windows end of the spawn path"),
    allow(SPAWN, "crates/skipper-confine/src/token.rs", "starts a program under a restricted token, the Windows end of the spawn path"),
    allow(SPAWN, "crates/skipper-standin/**", "the stand-in is a test double that starts its own child processes"),
    allow(SPAWN, "crates/skipper-app/src/probe/isolation.rs", "temporary: Stage 0 probe that starts its own helper outside the box; goes with the probe code"),
    allow(SPAWN, "crates/skipper-app/src/win/relay.rs", "the uninstall's temporary copy of the installed program: one start of that copy with an argument list and no shell, because a running program cannot delete its own folder (RL-36, Task 43); and, at the end of that copy, one start of a short-lived program of the Windows system folder (`waitfor.exe`, found from `SystemRoot`, an argument list, no shell, no window, no standard handles) that inherits the handle which deletes the copy's file, because the copy cannot be the last to close the handle of the file it still runs from (UN-1)"),
    allow(SPAWN, "crates/skipper-app/src/win/processes.rs", "the hand-off of the update (UPD-0a1, design 7.7): starts only `explorer.exe` of the Windows system folder (found from `SystemRoot`, an argument list, no shell, no window) with the one shortcut file `skipper-apply.lnk` that Skipper wrote into its own update store, so that the running shell starts `skipper.exe` of the install folder with one fixed list of arguments outside the app job; and, after an apply, one start of that `skipper.exe` with no argument; from a click or a staged update, never with free text; and waits for a process by its id"),
    // door-lint: the door itself and the tools that sit beside it
    allow(DOOR, "crates/skipper-core/src/door.rs", "the file door itself"),
    allow(DOOR, "crates/skipper-app/src/win/relay.rs", "the uninstall's temporary copy: copies the running program into the person's temporary folder and opens the copy with delete-on-close, so that Windows removes it when the last handle closes; at the end of the copy it opens the running program once for reading, to compare its file number with the handle it was given (UN-1); it touches no other file (RL-36, Task 43)"),
    allow_texts(
        DOOR,
        "crates/skipper-app/src/win/recycle.rs",
        &["fs::symlink_metadata"],
        "the Recycle Bin move of a removed account's folder (ACC-RM): looks at the folder itself, never through a link, twice: before the move (a folder that is not there needs no move) and after it (the folder must be gone from its place, or the move failed); it reads, writes and deletes nothing, and the move itself is the shell's, which only recycles",
    ),
    allow(DOOR, "crates/skipper-confine/src/grants.rs", "grants: the build box's own folders and access lists"),
    allow(DOOR, "crates/skipper-confine/src/rolebox.rs", "role boxes: the folders it makes for a role"),
    allow(DOOR, "crates/skipper-confine/src/cleanup.rs", "clean-up: removes what a role box left"),
    allow(DOOR, "crates/skipper-confine/src/acl.rs", "resolves grant paths for the access list"),
    allow(DOOR, "crates/skipper-hook/src/main.rs", "the Stage 0 log mode (one append to the log file it is given) and the probe-only modes --log and --note-file (Task 46a)"),
    allow(DOOR, "crates/skipper-hook/src/relay_probe.rs", "probe-only mode --probe-relay: reads `answer` and `wait_ms`, appends `relay.log` (Task 50a)"),
    allow_texts(DOOR, "crates/skipper-hook/src/client.rs", &["OpenOptions"], "opens Skipper's own named pipe, not a file"),
    allow_texts(DOOR, "crates/skipper-app/src/ui/instance.rs", &["OpenOptions"], "opens Skipper's own named pipe (the bring-to-front line of a second start), not a file"),
    allow(DOOR, "crates/skipper-standin/**", "the stand-in is a test double with its own marker and log files"),
    allow(DOOR, "crates/skipper-core/src/lang.rs", "carry of Task 10 (R33-C1): reads the shipped language folder with std::fs; the interface of Task 10 has no door"),
    allow(DOOR, "crates/skipper-core/src/forms.rs", "looks at the account folder itself with read_link, never through it; Task 15 fix round 1b"),
    allow_texts(
        DOOR,
        "crates/skipper-core/src/git.rs",
        &["fs::symlink_metadata"],
        "two uses of a look at an entry itself, never through a link: (1) link checks of a role clone: the door lets the system follow links and judges the target, so it cannot say whether `.git` is a junction (R26-2); (2) `is_absent` of the project scan of the code folder: asks whether the file system says for sure that nothing stands at an entry (`.git`, `HEAD`, `objects`, `refs`), a link that leads nowhere counting as something, which the door's follow-and-judge look cannot say either (TT45-1)",
    ),
    allow(DOOR, "crates/skipper-confine/src/secret.rs", "makes the keys folder with its access list; reads no file"),
    allow(DOOR, "crates/skipper-gateway/src/vault.rs", "the sign-in helper is the one writer of the vault folder, which the core's door denies (MC-332 items 2, 3 and 5)"),
    allow(DOOR, "crates/skipper-gateway/src/acl.rs", "makes the vault folder with its access list and looks at it without following a link (MC-332 item 1)"),
    allow(DOOR, "crates/skipper-gateway/src/link.rs", "opens the core's named pipe with CreateFileW and SQOS identification so the pipe server cannot impersonate the gateway (MC-337 item 5a)"),
    allow(DOOR, "crates/skipper-gateway/src/data_fetch.rs", "the sign-in helper keeps the day ledger of the optional data source (requests counted per UTC day) in a file of the vault folder, which the core's door denies; the file holds no secret (MODEL-13A)"),
    // door-lint, temporary: code of Stages 0 and 1 that predates the lint
    allow(DOOR, "crates/skipper-app/src/app.rs", "temporary: Stage 0 window; makes the probe folders (move behind the door in Task 36)"),
    allow(DOOR, "crates/skipper-app/src/panels.rs", "temporary: Stage 0 window; makes the kit folder of a plain panel (move behind the door in Task 36)"),
    allow(DOOR, "crates/skipper-app/src/signals.rs", "temporary: Stage 0 window; reads the tail of the hook marker log (replaced by the core's hook handling, Task 21)"),
    allow(DOOR, "crates/skipper-app/src/probe/**", "temporary: Stage 0 probe sub-commands; they run on a build machine, never for a role (go with the probe code)"),
    allow(DOOR, "crates/skipper-confine/src/conpty.rs", "opens the ConPTY libraries with a share mode that pins them; no content of the person's files"),
    allow(DOOR, "crates/skipper-confine/src/appcontainer.rs", "opens the NUL device as the standard input of a box"),
    Allow {
        rule: DOOR,
        path: "crates/skipper-confine/src/token.rs",
        texts: &["File::open"],
        in_fn: Some("run"),
        reason: "opens the NUL device as the standard input of a weak (T1b) command, as the box's runner does; no file content (T1B-PIPE)",
    },
    allow(DOOR, "crates/skipper-confine/src/junction.rs", "makes and removes a junction folder for the probe and the tests; no file content"),
    allow(DOOR, "crates/skipper-core/src/attach.rs", "temporary: makes the kit folder after the door has judged Write for it (the door has no folder step in this path; Task 24 replaces the kit)"),
    allow(DOOR, "crates/skipper-core/src/layout.rs", "removes the old layout file after the door has judged Write for it; the door has no delete (Task 37b)"),
    allow(DOOR, "crates/skipper-core/src/sim.rs", "the account-safety simulation builds and scans its own scenario root, outside every real folder"),
    allow(DOOR, "crates/skipper-mcp/src/main.rs", "temporary: the Stage 0 stub appends one log line to the file it is given (Task 23 replaces it)"),
    // cred-lint: where a credential name may be written down (ACC-3)
    allow(CRED, "crates/skipper-core/src/door.rs", "the door's deny rule for credential names"),
    allow(CRED, "crates/skipper-core/src/canary.rs", "the canary credential of the account-safety simulation"),
    allow(CRED, "crates/skipper-core/src/sim.rs", "the account-safety simulation"),
    allow(CRED, "crates/skipper-hook/src/guard.rs", "the hook's probe names for secrets files a Grep filter can select (PB-F04)"),
    allow(CRED, "crates/skipper-provider/**", "the provider knows the files of its own program"),
    allow(CRED, "crates/skipper-standin/**", "the stand-in's read-config mode reads a canary credential file on purpose"),
    // person-lint: the doors of the person's click (MC-79, security M16)
    allow_texts(
        PERSON,
        BUS,
        &["from_ui_click", "from_ui_dialog", "from_person_start", "from_start_click", "from_person", "from_person_apply"],
        "the command bus is the one place that turns a click or a dialog answer into a token or a grant",
    ),
    allow_texts(PERSON, "crates/skipper-app/src/panels.rs", &["from_click"], "Stage 0 probe window: the stop button of a panel"),
    allow_texts(PERSON, "crates/skipper-core/src/sim.rs", &["from_click"], "the account-safety simulation stops its own scenario"),
    // person-lint: the person's typing (MC-12 (a), Task 34b H1)
    Allow {
        rule: PERSON,
        path: "crates/skipper-app/src/app.rs",
        texts: &["from_panel"],
        in_fn: Some("sync_terminals"),
        reason: "the window's terminal sync hands what the person typed in a focused terminal view to that role's session",
    },
    allow_texts(PERSON, "crates/skipper-app/src/panels.rs", &["from_panel"], "Stage 0 probe window: what the person typed in a focused panel"),
    allow_texts(PERSON, "crates/skipper-core/src/sim.rs", &["from_panel"], "the account-safety simulation types its canary text into its own stand-in sessions"),
    // account-lint: accounts are never moved (MC-273)
    allow(ACCOUNT, "crates/skipper-core/src/forms.rs", "the person's forms set the account of a role"),
    allow(ACCOUNT, "crates/skipper-core/src/defs/role.rs", "the loader of a role definition reads the account from the file the person wrote"),
    allow(ACCOUNT, "crates/skipper-app/src/vm/settings.rs", "the person's settings form sets a role's account (Task 39); MC-273 allows the person's forms"),
    // prompt-lint: the one service start path (MC-12)
    Allow {
        rule: PROMPT,
        path: "crates/skipper-core/src/derive.rs",
        texts: &[],
        in_fn: Some("service"),
        reason: "the service start path (`service_launch`, `write_service_files`) is the one place that passes a protocol line as the prompt",
    },
    // prompt-lint: the role start path of lead first (MC-388, LF-14)
    Allow {
        rule: PROMPT,
        path: "crates/skipper-core/src/derive.rs",
        texts: &[],
        in_fn: Some("role_launch"),
        reason: "the role start path passes the compile-time protocol line LEAD_OPEN_LINE, or LEAD_OPEN_LINE_DEEP (the same line with `ultrathink` in front, only for a new session, MODEL-7), as the prompt of a new session the Lead or Skipper opened (MC-388, LF-14)",
    },
    // account-lint: a role the Lead defines inside the envelope (MC-396, LF-15)
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-core/src/catalog.rs",
        texts: &[],
        in_fn: Some("add_for_lead"),
        reason: "a role the Lead defines inside the person's envelope takes the account the person set in the envelope (MC-396.2, LF-15)",
    },
    // account-lint: the person's own new account goes to the roles that have none (49g)
    Allow {
        rule: ACCOUNT,
        path: BUS,
        texts: &[".account ="],
        in_fn: Some("give_new_account"),
        reason: "the person's own new account is given to the roles that have none, sub-roles skipped (49g)",
    },
    // account-lint: the person's save of the settings window (Task 39, Task 48f)
    Allow {
        rule: ACCOUNT,
        path: BUS,
        texts: &[".account ="],
        in_fn: Some("apply_to_defs"),
        reason: "the person's settings save sets or clears a role's account in the copy of the definitions it saves with the person's token; `widens_a_cap` runs it on a copy that is never saved (MC-273)",
    },
    Allow {
        rule: ACCOUNT,
        path: BUS,
        texts: &[".account ="],
        in_fn: Some("apply_briefing_setting"),
        reason: "the person's briefing settings save sets the account the briefing session runs on, an entry of `[services.briefing]` and not a role's account (MC-247 item 6)",
    },
    // account-lint: fields named `account` of other types (the token match cannot tell them apart), and the fixtures (MC-273)
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-app/src/model/fixtures.rs",
        texts: &[".account ="],
        in_fn: Some("role_def"),
        reason: "the fixtures (cargo feature `fixtures`, which no build of skipper.exe turns on, 6.1) build the sample's role definitions in memory for the preview and the tests (Task PV4); nothing is saved",
    },
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-app/src/runtime/service.rs",
        texts: &[".account ="],
        in_fn: Some("request_service"),
        reason: "the runtime's book of a service kind notes the account its newest run started on (`[services.<kind>] account`) for the Terminals row; a service run's account, not a role's (Task 49d)",
    },
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-app/src/ui/forms.rs",
        texts: &[".account ="],
        in_fn: Some("drop_draft"),
        reason: "drops the draft of the account form (`FormsState::account`, an `AccountDraft`) when that form is done; not a role's account (Task 41d)",
    },
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-app/src/ui/forms.rs",
        texts: &[".account ="],
        in_fn: Some("leave"),
        reason: "forgets the draft of the account form (`FormsState::account`) when the last form closes; not a role's account (Task 41d)",
    },
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-app/src/ui/forms.rs",
        texts: &[".account ="],
        in_fn: Some("role_form"),
        reason: "the person's role form puts the account the person picked in its list into the form (`RoleForm::account`, G-03, Task 41d); the core's forms make the role from it, and MC-273 allows the person's forms",
    },
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-core/src/core/mod.rs",
        texts: &[".account ="],
        in_fn: Some("after_briefing_switch"),
        reason: "the person's switch of the live summary copies the lead's account into an empty `[services.briefing] account` in the same act, an entry of the service settings and not a role's account (ruling MR-11, MC-247 item 6)",
    },
    Allow {
        rule: ACCOUNT,
        path: "crates/skipper-core/src/defs/workspace.rs",
        texts: &[".account ="],
        in_fn: Some("read_services"),
        reason: "the loader of the workspace definition reads `[services.<kind>] account` from the file the person wrote; a service's account, not a role's (Task 48c, MC-247 item 6)",
    },
    // command-lint: only the person's page sets the seen mark
    allow(COMMAND, BUS, "the command bus maps the briefing's dismiss command to the core's seen mark"),
    allow(COMMAND, "crates/skipper-app/src/vm/briefing.rs", "the briefing view model builds the dismiss command"),
    // door-lint: the import helper (the unpack of MAINT-5, the normalizer of IMP-5a), at the end of the list
    allow(DOOR, "crates/skipper-import/src/lib.rs", "import helper (unpack and normalizer); runs only in the box with no network, reads one source and writes below the run folder it is given"),
    allow(DOOR, "crates/skipper-import/src/archive.rs", "import helper (unpack and normalizer); runs only in the box with no network, reads one source and writes below the run folder it is given"),
];

/// One place that breaks a rule, before the exceptions are applied.
#[derive(Clone, PartialEq, Debug)]
struct Finding {
    line: usize,
    rule: &'static str,
    text: String,
    /// The innermost function that holds the place.
    in_fn: Option<String>,
}

pub(crate) fn path_matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_suffix("/**") {
        Some(folder) => path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/')),
        None => path == pattern,
    }
}

fn is_allowed(path: &str, finding: &Finding) -> bool {
    ALLOWED.iter().any(|a| {
        a.rule == finding.rule
            && path_matches(a.path, path)
            && (a.texts.is_empty() || a.texts.contains(&finding.text.as_str()))
            && a.in_fn.is_none_or(|part| finding.in_fn.as_deref().is_some_and(|name| name.contains(part)))
    })
}

/// The exceptions as report lines `rule path [in *fn part*] - reason`.
pub fn allowances() -> Vec<String> {
    ALLOWED
        .iter()
        .map(|a| {
            let place = a.in_fn.map(|part| format!(" in *{part}*")).unwrap_or_default();
            format!("{} {}{place} - {}", a.rule, a.path, a.reason)
        })
        .collect()
}

/// The identifiers that start a process, wherever and however they are written.
fn is_spawn_name(name: &str) -> bool {
    matches!(name, "Command" | "CommandBuilder" | "spawn_command" | "WinExec")
        || name.starts_with("CreateProcess")
        || name.starts_with("ShellExecute")
}

/// The identifiers that open, make, move, copy, remove or link a file, wherever and however they are written.
fn is_file_name(name: &str) -> bool {
    matches!(name, "OpenOptions" | "hard_link" | "soft_link" | "symlink")
        || (name.starts_with("symlink_") && name != "symlink_metadata")
        || ["CreateFile", "DeleteFile", "MoveFile", "CopyFile", "ReplaceFile"].iter().any(|family| name.starts_with(family))
        || (name.starts_with("Create") && name.contains("Link"))
}

/// The functions of `std::fs` that read, look at, make, change or remove a file or a folder, found as `fs::<name>`.
/// (The link calls are identifiers of their own, see [`is_file_name`].)
fn is_fs_call(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "read_to_string"
            | "read_dir"
            | "metadata"
            | "symlink_metadata"
            | "canonicalize"
            | "read_link"
            | "set_permissions"
            | "exists"
            | "write"
            | "rename"
            | "copy"
            | "create_dir"
            | "create_dir_all"
    ) || name.starts_with("remove_")
}

/// The functions of `std::fs::File` that open or make a file.
fn is_file_call(name: &str) -> bool {
    matches!(name, "open" | "create" | "create_new" | "options")
}

/// The credential name that `text` holds, if any: `.credentials.json` anywhere in it, or a path segment that is a
/// credential name of the door (`.env`, `*.env`, `.env.*`), in any letter case.
fn credential_in(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.contains(".credentials.json") {
        return Some(".credentials.json".to_owned());
    }
    lower.split(['/', '\\']).find(|segment| segment.ends_with(".env") || segment.starts_with(".env.")).map(str::to_owned)
}

/// The tokens `a :: b` that start at the identifier at `i`.
fn pair_at(tokens: &[Token], i: usize) -> Option<(&str, &str)> {
    let a = ident_at(tokens, i)?;
    if !is_punct(tokens, i + 1, ':') || !is_punct(tokens, i + 2, ':') {
        return None;
    }
    Some((a, ident_at(tokens, i + 3)?))
}

/// The line of a token (0 for a number or a character literal, which no finding is made at).
pub(crate) fn line_of(token: &Token) -> usize {
    match token {
        Token::Ident(_, line) | Token::Str(_, line) | Token::Punct(_, line) => *line,
        Token::Literal => 0,
    }
}

/// True when the call after `ProtocolLine::new` (the `(` is at `open`) holds one string literal and nothing else.
fn takes_only_a_literal(tokens: &[Token], open: usize) -> bool {
    let trailing_comma = usize::from(is_punct(tokens, open + 2, ','));
    is_punct(tokens, open, '(')
        && matches!(tokens.get(open + 1), Some(Token::Str(..)))
        && is_punct(tokens, open + 2 + trailing_comma, ')')
}

/// The index after the group (`( )`, `[ ]` or `{ }`) that opens at `open`.
pub(crate) fn skip_group(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0i32;
    for at in open..tokens.len() {
        if ['(', '[', '{'].iter().any(|c| is_punct(tokens, at, *c)) {
            depth += 1;
        } else if [')', ']', '}'].iter().any(|c| is_punct(tokens, at, *c)) {
            depth -= 1;
            if depth == 0 {
                return at + 1;
            }
        }
    }
    tokens.len()
}

/// The index after the closer of the group that holds the token at `from`, or `None` when a `;` ends the statement first.
fn end_of_group(tokens: &[Token], from: usize) -> Option<usize> {
    let mut depth = 0i32;
    for at in from..tokens.len() {
        if ['(', '[', '{'].iter().any(|c| is_punct(tokens, at, *c)) {
            depth += 1;
        } else if [')', ']', '}'].iter().any(|c| is_punct(tokens, at, *c)) {
            if depth == 0 {
                return Some(at + 1);
            }
            depth -= 1;
        } else if depth == 0 && is_punct(tokens, at, ';') {
            return None;
        }
    }
    None
}

/// True when the path that ends at `after` stands where a pattern stands, so that it handles the variant and does not
/// build it: a match arm (`Path { .. } =>`, `Path(..) | ..`, `Path if ..`, also inside `Some( )` or a tuple), a `let`
/// pattern (`if let Path { .. } = x`).
fn follows_pattern(tokens: &[Token], mut after: usize) -> bool {
    if is_punct(tokens, after, '{') || is_punct(tokens, after, '(') {
        after = skip_group(tokens, after);
    }
    // The rest of a tuple or slice pattern: `(Path { .. }, _) =>`.
    if is_punct(tokens, after, ',') {
        match end_of_group(tokens, after) {
            Some(end) => after = end,
            None => return false,
        }
    }
    while is_punct(tokens, after, ')') || is_punct(tokens, after, ']') {
        after += 1;
    }
    let arrow = is_punct(tokens, after, '=') && is_punct(tokens, after + 1, '>');
    let binding = is_punct(tokens, after, '=') && !is_punct(tokens, after + 1, '=') && !is_punct(tokens, after + 1, '>');
    let alternative = is_punct(tokens, after, '|') && !is_punct(tokens, after + 1, '|');
    arrow || binding || alternative || is_ident(tokens, after, "if")
}

/// True when the token at `at` stands in the pattern argument of `matches!(value, pattern)`: an enclosing parenthesis
/// that follows `matches !` has a comma before `at` at its own level.
fn in_matches_pattern(tokens: &[Token], at: usize) -> bool {
    let mut depth = 0i32;
    let mut comma_at_this_level = false;
    for back in (0..at).rev() {
        match &tokens[back] {
            Token::Punct(')' | ']' | '}', _) => depth += 1,
            Token::Punct('(' | '[' | '{', _) if depth > 0 => depth -= 1,
            Token::Punct('(', _) => {
                if back >= 2 && is_punct(tokens, back - 1, '!') && is_ident(tokens, back - 2, "matches") {
                    return comma_at_this_level;
                }
                comma_at_this_level = false;
            }
            Token::Punct('[', _) => comma_at_this_level = false,
            // A block that opens around the token, or a statement that ends before it: no argument of a macro that
            // opens further out.
            Token::Punct('{', _) => return false,
            Token::Punct(';', _) if depth == 0 => return false,
            Token::Punct(',', _) if depth == 0 => comma_at_this_level = true,
            _ => {}
        }
    }
    false
}

/// True when the path that starts at `at` and ends at `after` stands in pattern position (see [`follows_pattern`] and
/// [`in_matches_pattern`]).
fn is_pattern(tokens: &[Token], at: usize, after: usize) -> bool {
    in_matches_pattern(tokens, at) || follows_pattern(tokens, after)
}

/// True when the name at `i` is followed by `{` in a place where a struct literal can stand, not where an item
/// opens its body (`struct X {`, `impl X {`, `impl T for X {`, `-> X {`). A `=> X {` is the body of a match arm, and
/// a literal.
fn starts_a_literal(tokens: &[Token], i: usize) -> bool {
    let after_item_word = i > 0 && ["struct", "enum", "impl", "for", "trait"].iter().any(|w| is_ident(tokens, i - 1, w));
    let after_arrow = i > 1 && is_punct(tokens, i - 1, '>') && is_punct(tokens, i - 2, '-');
    is_punct(tokens, i + 1, '{') && !after_item_word && !after_arrow
}

/// The fields of the struct literal whose `{` is at `open`, as (index of the name, index of the first token of the
/// value, index after the value). A shorthand field (`name,`) has an empty value; a `..base` has no name and is left out.
fn literal_fields(tokens: &[Token], open: usize) -> Vec<(usize, usize, usize)> {
    let mut fields = Vec::new();
    let mut j = open + 1;
    while j < tokens.len() && !is_punct(tokens, j, '}') {
        let start = j;
        let mut depth = 0i32;
        while j < tokens.len() {
            if depth == 0 && (is_punct(tokens, j, ',') || is_punct(tokens, j, '}')) {
                break;
            }
            if ['(', '[', '{'].iter().any(|c| is_punct(tokens, j, *c)) {
                depth += 1;
            } else if [')', ']', '}'].iter().any(|c| is_punct(tokens, j, *c)) {
                depth -= 1;
            }
            j += 1;
        }
        if ident_at(tokens, start).is_some() {
            if is_punct(tokens, start + 1, ':') && !is_punct(tokens, start + 2, ':') {
                fields.push((start, start + 2, j));
            } else if j == start + 1 {
                fields.push((start, j, j));
            }
        }
        if is_punct(tokens, j, ',') {
            j += 1;
        }
    }
    fields
}

/// Per token: the name of the innermost function whose body holds it.
fn enclosing_fns(tokens: &[Token]) -> Vec<Option<String>> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut open: Vec<(String, i32)> = Vec::new();
    let mut pending: Option<String> = None;
    let mut braces = 0i32;
    let mut groups = 0i32;
    for (i, token) in tokens.iter().enumerate() {
        out.push(open.last().map(|(name, _)| name.clone()));
        match token {
            Token::Ident(word, _) if word == "fn" => {
                if let Some(name) = ident_at(tokens, i + 1) {
                    pending = Some(name.to_owned());
                    groups = 0;
                }
            }
            Token::Punct('(' | '[', _) => groups += 1,
            Token::Punct(')' | ']', _) => groups -= 1,
            // A declaration without a body: `fn name(..);`.
            Token::Punct(';', _) if groups == 0 => pending = None,
            Token::Punct('{', _) => {
                braces += 1;
                if groups == 0 {
                    if let Some(name) = pending.take() {
                        open.push((name, braces));
                    }
                }
            }
            Token::Punct('}', _) => {
                if open.last().is_some_and(|(_, depth)| *depth == braces) {
                    open.pop();
                }
                braces -= 1;
            }
            _ => {}
        }
    }
    out
}

/// Per token: inside an item or list element that carries a test-only `cfg`, as the shared scanner reads it
/// (`rust_tokens::skip_test_item`). Every rule skips these.
pub(crate) fn test_regions(tokens: &[Token]) -> Vec<bool> {
    let mut skipped = vec![false; tokens.len()];
    let mut i = 0;
    while i < tokens.len() {
        if let Some(end) = skip_test_item(tokens, i) {
            let end = end.min(tokens.len());
            skipped[i..end].fill(true);
            i = end;
        } else {
            i += 1;
        }
    }
    skipped
}

/// What the `cfg` predicate that starts at `at` says about a build with a test feature on, and the index after it:
/// `Some((gated, after))`. `gated` is true when the predicate holds only in a test build or with one of
/// [`TEST_FEATURES`] on: `test`, `feature = "<a test feature>"`, `all(..)` with at least one such argument, `any(..)`
/// of which every argument is such. `None` when the tokens are no predicate.
fn feature_gate(tokens: &[Token], at: usize, depth: usize) -> Option<(bool, usize)> {
    if depth > 32 {
        return None;
    }
    let name = ident_at(tokens, at)?;
    let next = at + 1;
    if is_punct(tokens, next, '=') {
        let (value, _) = str_at(tokens, next + 1)?;
        return Some((name == "feature" && TEST_FEATURES.contains(&value), next + 2));
    }
    if !is_punct(tokens, next, '(') {
        return Some((name == "test", next));
    }
    let mut arguments: Vec<bool> = Vec::new();
    let mut at = next + 1;
    while !is_punct(tokens, at, ')') {
        let (gated, after) = feature_gate(tokens, at, depth + 1)?;
        arguments.push(gated);
        at = after;
        if is_punct(tokens, at, ',') {
            at += 1;
        } else if !is_punct(tokens, at, ')') {
            return None;
        }
    }
    let gated = match name {
        "all" => arguments.iter().any(|argument| *argument),
        "any" => !arguments.is_empty() && arguments.iter().all(|argument| *argument),
        _ => false,
    };
    Some((gated, at + 1))
}

/// When the attribute at `i` is `#[cfg(..)]` or `#![cfg(..)]` with a predicate that is gated (see [`feature_gate`]):
/// the index of the first token of the predicate and the index of the `)` after it.
fn gating_cfg(tokens: &[Token], i: usize) -> Option<(usize, usize)> {
    if !is_punct(tokens, i, '#') {
        return None;
    }
    let open = if is_punct(tokens, i + 1, '!') { i + 2 } else { i + 1 };
    if !(is_punct(tokens, open, '[') && is_ident(tokens, open + 1, "cfg") && is_punct(tokens, open + 2, '(')) {
        return None;
    }
    let start = open + 3;
    let (gated, after) = feature_gate(tokens, start, 0)?;
    let close = if is_punct(tokens, after, ',') { after + 1 } else { after };
    (gated && is_punct(tokens, close, ')') && is_punct(tokens, close + 1, ']')).then_some((start, close))
}

/// Per token: inside an item or list element whose `cfg` holds only in a test build or with a test feature on. Only
/// `TeamRunGrant::for_tests` may stand there. The extent of the item is the shared scanner's: it reads a view of the
/// tokens in which every such predicate is the one word `test`.
fn feature_gated(tokens: &[Token]) -> Vec<bool> {
    let mut view: Vec<Token> = Vec::with_capacity(tokens.len());
    // For every token of the view, its index in `tokens`.
    let mut origin: Vec<usize> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        if let Some((start, close)) = gating_cfg(tokens, i) {
            for at in i..start {
                view.push(tokens[at].clone());
                origin.push(at);
            }
            view.push(Token::Ident("test".to_owned(), line_of(&tokens[start])));
            origin.push(start);
            i = close;
        } else {
            view.push(tokens[i].clone());
            origin.push(i);
            i += 1;
        }
    }
    if is_test_file(&view) {
        return vec![true; tokens.len()];
    }
    let mut gated = vec![false; tokens.len()];
    let mut at = 0;
    while at < view.len() {
        if let Some(end) = skip_test_item(&view, at) {
            let end = end.min(view.len());
            let to = origin.get(end).copied().unwrap_or(tokens.len());
            gated[origin[at]..to].fill(true);
            at = end;
        } else {
            at += 1;
        }
    }
    gated
}

/// Per token: inside a `use` item, from the keyword to its `;`.
fn use_items(tokens: &[Token]) -> Vec<bool> {
    let mut inside = vec![false; tokens.len()];
    let mut i = 0;
    while i < tokens.len() {
        if is_ident(tokens, i, "use") {
            let mut depth = 0i32;
            let mut end = i;
            while end < tokens.len() {
                if is_punct(tokens, end, '{') {
                    depth += 1;
                } else if is_punct(tokens, end, '}') {
                    depth -= 1;
                } else if is_punct(tokens, end, ';') && depth <= 0 {
                    break;
                }
                end += 1;
            }
            let stop = (end + 1).min(tokens.len());
            inside[i..stop].fill(true);
            i = stop;
        } else {
            i += 1;
        }
    }
    inside
}

/// Every place of `src` that breaks a rule, outside comments and test code, before the exceptions are applied, in
/// the order of the lines.
fn findings(src: &str) -> Vec<Finding> {
    let tokens = tokenize(src);
    if is_test_file(&tokens) {
        return Vec::new();
    }
    let skipped = test_regions(&tokens);
    let gated = feature_gated(&tokens);
    let in_use = use_items(&tokens);
    let fns = enclosing_fns(&tokens);
    let mut out = Vec::new();
    let mut found = |at: usize, rule: &'static str, text: String| {
        out.push(Finding { line: line_of(&tokens[at]), rule, text, in_fn: fns[at].clone() });
    };
    for i in 0..tokens.len() {
        if skipped[i] {
            continue;
        }
        match &tokens[i] {
            Token::Str(text, _) => {
                if let Some(name) = credential_in(text) {
                    found(i, CRED, name);
                }
            }
            Token::Punct('.', _) => {
                // `x.account = ..` (not `==`): the field of a role is written.
                if is_ident(&tokens, i + 1, "account") && is_punct(&tokens, i + 2, '=') && !is_punct(&tokens, i + 3, '=') {
                    found(i + 1, ACCOUNT, ".account =".to_owned());
                }
            }
            Token::Ident(name, _) => {
                // The names of a rule, wherever they stand. A definition (`fn from_click`) is no use.
                if !(i > 0 && is_ident(&tokens, i - 1, "fn")) {
                    if is_spawn_name(name) {
                        found(i, SPAWN, name.clone());
                    }
                    if is_file_name(name) {
                        found(i, DOOR, name.clone());
                    }
                    if PERSON_NAMES.contains(&name.as_str()) {
                        found(i, PERSON, name.clone());
                    }
                }
                // `use path::Name as Other;`: a rename of a name that is matched as part of a pair or of a literal.
                if in_use[i] && is_ident(&tokens, i + 1, "as") {
                    if let Some(entry) = RENAMED.iter().find(|entry| entry.0 == name.as_str()) {
                        found(i, entry.1, format!("{name} as"));
                    }
                }
                if name == "fs" && is_punct(&tokens, i + 1, ':') && is_punct(&tokens, i + 2, ':') && is_punct(&tokens, i + 3, '{') {
                    // `use std::fs::{read, write};`: the names of the group are calls of `fs`.
                    for (at, member) in group_members(&tokens, i + 3) {
                        if is_fs_call(member) {
                            found(at, DOOR, format!("fs::{member}"));
                        }
                        if member == "self" && is_ident(&tokens, at + 1, "as") {
                            found(at, DOOR, "fs as".to_owned());
                        }
                    }
                }
                if let Some((a, b)) = pair_at(&tokens, i) {
                    match (a, b) {
                        ("File", call) if is_file_call(call) => found(i, DOOR, format!("File::{call}")),
                        ("fs", call) if is_fs_call(call) => found(i, DOOR, format!("fs::{call}")),
                        ("TeamRunGrant", "for_tests") if !gated[i] => found(i, PERSON, "TeamRunGrant::for_tests".to_owned()),
                        ("ReapplyGrant", "for_tests") if !gated[i] => found(i, PERSON, "ReapplyGrant::for_tests".to_owned()),
                        ("ProtocolLine", "new") if !takes_only_a_literal(&tokens, i + 4) => {
                            found(i, PROTOCOL, "ProtocolLine::new".to_owned());
                        }
                        ("CoreCommand", "SetSeenMark") | ("UiCommand", "DismissBriefing") if !is_pattern(&tokens, i, i + 4) => {
                            found(i, COMMAND, format!("{a}::{b}"));
                        }
                        _ => {}
                    }
                }
                if name == "RoleDef" && starts_a_literal(&tokens, i) {
                    for (field, _, _) in literal_fields(&tokens, i + 1) {
                        if is_ident(&tokens, field, "account") {
                            found(field, ACCOUNT, "RoleDef { account: .. }".to_owned());
                        }
                    }
                }
                if name == "StartInputs" && starts_a_literal(&tokens, i) {
                    for (field, from, to) in literal_fields(&tokens, i + 1) {
                        let none = to == from + 1 && is_ident(&tokens, from, "None");
                        if is_ident(&tokens, field, "prompt") && !none {
                            found(field, PROMPT, "StartInputs.prompt".to_owned());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|finding| finding.line);
    out
}

/// The identifiers at the top of the group whose `{` is at `open`, with their indexes.
fn group_members(tokens: &[Token], open: usize) -> Vec<(usize, &str)> {
    let end = skip_group(tokens, open);
    let mut depth = 0i32;
    let mut members = Vec::new();
    for at in open..end {
        if is_punct(tokens, at, '{') {
            depth += 1;
        } else if is_punct(tokens, at, '}') {
            depth -= 1;
        } else if depth == 1 {
            if let Some(name) = ident_at(tokens, at) {
                members.push((at, name));
            }
        }
    }
    members
}

/// The report lines of one file: its findings that no exception covers, as `path:line rule text`.
fn check_file(path: &str, src: &str) -> Vec<String> {
    findings(src)
        .into_iter()
        .filter(|finding| !is_allowed(path, finding))
        .map(|finding| format!("{path}:{} {} {}", finding.line, finding.rule, finding.text))
        .collect()
}

/// Every `.rs` file below `dir` that is not test code, in name order, checked into `report`; `root` is cut from the paths.
fn check_folder(root: &Path, dir: &Path, report: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> =
        fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" {
                check_folder(root, &path, report)?;
            }
        } else if path.extension().is_some_and(|x| x == "rs") && name != "tests.rs" {
            let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            report.extend(check_file(&relative, &text));
        }
    }
    Ok(())
}

/// The report lines of the tree below `root`: every `crates/<crate>/src/**/*.rs`, in name order. An error when
/// `root` has no `crates` folder (the command was started outside the workspace).
pub fn check_tree(root: &Path) -> Result<Vec<String>, String> {
    let crates = root.join("crates");
    let mut members: Vec<_> =
        fs::read_dir(&crates).map_err(|e| format!("cannot read {}: {e}", crates.display()))?.filter_map(Result::ok).collect();
    members.sort_by_key(|entry| entry.file_name());
    let mut report = Vec::new();
    for member in members {
        let source = member.path().join("src");
        if source.is_dir() {
            check_folder(root, &source, &mut report)?;
        }
    }
    Ok(report)
}

pub fn run_cli(args: &[String]) -> u8 {
    match args {
        [] => {}
        [flag] if flag == "--allowances" => {
            for line in allowances() {
                println!("{line}");
            }
            return 0;
        }
        _ => {
            eprintln!("usage: cargo xtask source-lint [--allowances]");
            return 2;
        }
    }
    match check_tree(Path::new(".")) {
        Ok(report) if report.is_empty() => {
            println!("ok");
            0
        }
        Ok(report) => {
            for line in &report {
                println!("{line}");
            }
            1
        }
        Err(why) => {
            println!("source-lint: {why} (run it from the workspace root)");
            1
        }
    }
}

/// The reasons of the exceptions of the import helper (RL-05, IMP-13).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_import_helper_exceptions_name_the_whole_helper_not_only_its_unpack() {
        let lines = allowances();
        for file in ["lib.rs", "archive.rs"] {
            let head = format!("door-lint crates/skipper-import/src/{file} - ");
            let line = lines
                .iter()
                .find(|l| l.starts_with(&head))
                .unwrap_or_else(|| panic!("no door-lint exception for {file}: {lines:?}"));
            assert!(line.contains("import helper") && line.contains("normalizer"), "{line}");
            assert!(!line.contains("unpack helper") && !line.contains("check box"), "{line}");
        }
    }
}
