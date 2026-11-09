// SPDX-License-Identifier: MIT
//! `cargo xtask supply` (Stage 1 Task 42; MC-118, MC-119, MC-130): the supply-chain checks of the project, run offline
//! with three pinned tools that live outside Skipper's own lock:
//!
//! - `cargo deny --locked --offline check --config supply-chain/deny.toml licenses bans sources` (cargo-deny 0.20.2):
//!   the licence allow-list, the bans (no duplicate of an HTTP or TLS crate, no C TLS stack) and the sources (crates.io
//!   only);
//! - `cargo audit --no-fetch --db <copy of the RustSec database> --file Cargo.lock` (cargo-audit 0.22.2): the copy must
//!   be a git checkout whose newest commit is at most 7 days old (MC-119);
//! - `cargo vet --locked --frozen` (cargo-vet 0.10.2): every registry package of the lock is audited, imported or
//!   exempted (`supply-chain/config.toml`). An exemption is not an audit: today all of them are the initial pin set,
//!   not audited, with the 7-day wait waived until version 1. A pass therefore says how much it covered: the last line
//!   is `supply: ok (vet: 0 fully audited, 488 exempted)`, with the counts of cargo vet's own summary.
//!
//! The tools are built on the build machine (`cargo install --locked --offline --root %LOCALAPPDATA%\skipper-build\tools`,
//! after the fetch phase on the main PC) and found in `<root>\bin`; the database copy is
//! `%LOCALAPPDATA%\skipper-build\advisory-db`. `--tools <root>` / `SKIPPER_TOOLS_DIR` and `--db <dir>` /
//! `SKIPPER_ADVISORY_DB` name other places. This program never touches the network: every child gets
//! `CARGO_NET_OFFLINE=true`, the flags above, and the only thing it runs besides the three tools is `git log -1` on the
//! database copy.
//!
//! It fails closed. A missing tool, a tool of another version, a missing, undated or stale database, a missing
//! `imports.lock`, a tool that exits non-zero, and a tool whose output lacks the line that proves it did its work are
//! each a finding; there is no silent pass. The exit code of a tool is the truth, the parsing only names the finding;
//! the same holds for the counts of a vet pass: a summary line in a shape this program does not know is printed as it
//! stands, and never turns a failure into a pass.
//!
//! Scope: `cargo-deny` (`[graph] targets` in deny.toml) and `cargo-vet` (`--cargo-arg=--filter-platform=...`) judge the
//! `x86_64-pc-windows-gnu` graph, like `deps-check`: the Linux-only tree of Task 3 is in the lock but never downloaded,
//! and `cargo metadata --offline` cannot read a package that is not on disk. `--all-targets` drops the vet filter once a
//! full fetch exists. The summary says which scope ran.
//!
//! Judge path: a small, plain module. Everything that parses or decides is a pure function (tests in
//! `xtask/tests/supply.rs`); the I/O is the thin `run_cli` at the end.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The pinned versions (Task 1 section B, owner decision 31.10.2026: the plan's tools are approved).
pub const DENY_VERSION: &str = "0.20.2";
pub const AUDIT_VERSION: &str = "0.22.2";
pub const VET_VERSION: &str = "0.10.2";
/// MC-119: the advisory database copy is at most 7 days old.
pub const MAX_DB_AGE_SECS: u64 = 7 * 24 * 60 * 60;
/// The graph that ships (the same as `deps-check`).
pub const WINDOWS_TARGET: &str = "x86_64-pc-windows-gnu";
/// How much of a failing tool's own output is kept in the evidence.
const MAX_RAW_LINES: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Deny,
    Audit,
    Vet,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Deny, Tool::Audit, Tool::Vet];

    /// The cargo sub-command (`cargo <name>`).
    pub fn name(self) -> &'static str {
        match self {
            Tool::Deny => "deny",
            Tool::Audit => "audit",
            Tool::Vet => "vet",
        }
    }

    pub fn pinned(self) -> &'static str {
        match self {
            Tool::Deny => DENY_VERSION,
            Tool::Audit => AUDIT_VERSION,
            Tool::Vet => VET_VERSION,
        }
    }

    /// The crate and the file `cargo install --root <tools>` leaves in `<tools>\bin`.
    pub fn crate_name(self) -> &'static str {
        match self {
            Tool::Deny => "cargo-deny",
            Tool::Audit => "cargo-audit",
            Tool::Vet => "cargo-vet",
        }
    }

    pub fn file_name(self) -> String {
        format!("{}{}", self.crate_name(), std::env::consts::EXE_SUFFIX)
    }
}

// ---------------------------------------------------------------------------------------------
// Command lines (one place, so a test can pin them)
// ---------------------------------------------------------------------------------------------

/// Arguments after `cargo`: licences, bans and sources, offline, with the project's own configuration.
pub fn deny_args(config: &str) -> Vec<String> {
    ["deny", "--locked", "--offline", "check", "--config", config, "licenses", "bans", "sources"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Arguments after `cargo`: no fetch, the copy of the advisory database, the project's lock.
pub fn audit_args(db: &str, lock: &str) -> Vec<String> {
    ["audit", "--no-fetch", "--db", db, "--file", lock].iter().map(|s| s.to_string()).collect()
}

/// Arguments after `cargo`: no network, no fetch of imported audits; the Windows graph unless `all_targets`.
pub fn vet_args(all_targets: bool) -> Vec<String> {
    let mut args: Vec<String> = ["vet", "--locked", "--frozen"].iter().map(|s| s.to_string()).collect();
    if !all_targets {
        args.push(format!("--cargo-arg=--filter-platform={WINDOWS_TARGET}"));
    }
    args
}

/// Arguments after `cargo`: the version line of a tool.
pub fn version_args(tool: Tool) -> Vec<String> {
    vec![tool.name().to_string(), "--version".to_string()]
}

// ---------------------------------------------------------------------------------------------
// Pure judgements
// ---------------------------------------------------------------------------------------------

/// True when a token of the `--version` output is the pinned version (`cargo-deny 0.20.2`, `cargo-audit-audit 0.22.2`).
pub fn version_matches(output: &str, pinned: &str) -> bool {
    strip_ansi(output).split_whitespace().any(|token| token.trim_start_matches('v') == pinned)
}

/// MC-119: the database copy is judged by the date of its newest commit.
pub fn db_age_verdict(commit_unix: u64, now_unix: u64) -> Result<(), String> {
    if commit_unix > now_unix.saturating_add(24 * 60 * 60) {
        return Err(String::from(
            "the newest commit of the advisory database is dated after today (clock or copy wrong): its age cannot be trusted",
        ));
    }
    let age = now_unix.saturating_sub(commit_unix);
    if age > MAX_DB_AGE_SECS {
        return Err(format!(
            "the advisory database is {} days old; at most {} are allowed (MC-119): copy a fresh one in the fetch phase",
            age / 86_400,
            MAX_DB_AGE_SECS / 86_400
        ));
    }
    Ok(())
}

/// `cargo vet --frozen` needs `imports.lock` as soon as `config.toml` imports audits; the networked fetch phase writes it.
pub fn imports_lock_problem(config_text: &str, imports_lock_exists: bool) -> Option<String> {
    let imports = config_text.lines().any(|l| l.trim_start().starts_with("[imports."));
    if imports && !imports_lock_exists {
        return Some(String::from(
            "vet: supply-chain/imports.lock is missing; run `cargo vet` once in the fetch phase (with network, in the box) and commit the file",
        ));
    }
    None
}

/// Removes ANSI colour sequences (a tool may colour its output although `NO_COLOR` is set).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The first few non-empty lines of an output, for a finding that cannot name its cause.
fn first_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).take(count).collect();
    if lines.is_empty() { String::from("(no output)") } else { lines.join(" | ") }
}

/// `error[rejected]: failed to satisfy license requirements` -> one finding per such line, with the crate when the
/// label under it names one (`┌─ registry+...#name@1.2.3:7:12`).
pub fn parse_deny(output: &str, exit_ok: bool) -> Vec<String> {
    let text = strip_ansi(output);
    let lines: Vec<&str> = text.lines().collect();
    let mut findings = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("error[") else { continue };
        let Some((code, message)) = rest.split_once("]:") else { continue };
        let mut finding = format!("deny: {} ({})", message.trim(), code.trim());
        if let Some(krate) = lines[i + 1..].iter().take(4).find_map(|l| crate_of_label(l)) {
            finding.push(' ');
            finding.push_str(&krate);
        }
        findings.push(finding);
    }
    if findings.is_empty() && !exit_ok {
        findings.push(format!("deny: the check failed without a readable finding: {}", first_lines(&text, 3)));
    }
    findings
}

/// `┌─ registry+https://github.com/rust-lang/crates.io-index#foo@1.0.0:7:12` -> `foo@1.0.0`.
fn crate_of_label(line: &str) -> Option<String> {
    let t = line.trim_start();
    if !(t.starts_with("┌─") || t.starts_with("╭─") || t.starts_with("-->")) {
        return None;
    }
    let after = t.split_once('#')?.1;
    let id = after.split(':').next()?;
    if id.contains('@') { Some(id.to_string()) } else { None }
}

/// What `cargo audit` said: the lines that make the check red, and the ones that are only noted.
#[derive(Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub findings: Vec<String>,
    pub notes: Vec<String>,
}

/// An advisory block (`Crate:`, `Version:`, optional `Warning:`, `Title:`, `ID:`) is a finding, or a note when it
/// carries a `Warning:` kind (unmaintained, unsound, yanked). The line `Loaded N security advisories` proves that the
/// database was read: without it, or with N = 0, the check is red.
pub fn parse_audit(output: &str, exit_ok: bool) -> AuditReport {
    let text = strip_ansi(output);
    let mut findings = Vec::new();
    let mut notes = Vec::new();
    let (mut krate, mut version, mut kind, mut title) = (String::new(), String::new(), String::new(), String::new());
    let mut loaded: Option<u64> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(n) = loaded_count(t) {
            loaded = Some(n);
        } else if let Some(v) = field(t, "Crate:") {
            krate = v.to_string();
            version.clear();
            kind.clear();
            title.clear();
        } else if let Some(v) = field(t, "Version:") {
            version = v.to_string();
        } else if let Some(v) = field(t, "Warning:") {
            kind = v.to_string();
        } else if let Some(v) = field(t, "Title:") {
            title = v.to_string();
        } else if let Some(id) = field(t, "ID:") {
            if kind.is_empty() {
                findings.push(format!("audit: {id} {krate} {version}: {title}"));
            } else {
                notes.push(format!("audit: warning ({kind}) {id} {krate} {version}"));
            }
        }
    }
    match loaded {
        None => findings.push(String::from(
            "audit: the output has no 'Loaded N security advisories' line; the advisory database was not read",
        )),
        Some(0) => findings.push(String::from("audit: the advisory database holds no advisory")),
        Some(_) => {}
    }
    if findings.is_empty() && !exit_ok {
        findings.push(format!("audit: the check failed without a readable finding: {}", first_lines(&text, 3)));
    }
    AuditReport { findings, notes }
}

/// `Key:     value` -> `value` (the label must be followed by a space or the end).
fn field<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(label)?;
    let value = rest.trim();
    if value.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') { Some(value) } else { None }
}

/// `Loaded 1042 security advisories (from ...)` -> 1042.
fn loaded_count(line: &str) -> Option<u64> {
    let rest = line.strip_prefix("Loaded ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !rest[digits.len()..].trim_start().starts_with("security advisor") {
        return None;
    }
    digits.parse().ok()
}

/// `foo:1.0.0 missing ["safe-to-deploy"]` -> one finding per unvetted package. `Vetting Succeeded` must be there for a pass.
pub fn parse_vet(output: &str, exit_ok: bool) -> Vec<String> {
    let text = strip_ansi(output);
    let mut findings = Vec::new();
    let mut succeeded = false;
    let mut failed = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Vetting Succeeded") {
            succeeded = true;
        } else if t.starts_with("Vetting Failed") {
            failed = true;
        } else if let Some(finding) = unvetted_line(t) {
            findings.push(finding);
        }
    }
    if findings.is_empty() {
        if failed || !exit_ok {
            findings.push(format!("vet: the check failed without a readable finding: {}", first_lines(&text, 3)));
        } else if !succeeded {
            findings.push(String::from("vet: the output has no 'Vetting Succeeded' line; the result is unknown"));
        }
    } else if exit_ok && !succeeded {
        findings.push(String::from("vet: the exit code is 0 although packages are unvetted; the result is unknown"));
    }
    findings
}

fn unvetted_line(line: &str) -> Option<String> {
    let (token, rest) = line.split_once(' ')?;
    let (name, version) = token.split_once(':')?;
    if name.is_empty() || !version.chars().next()?.is_ascii_digit() {
        return None;
    }
    let criteria = rest.trim().strip_prefix("missing")?.trim();
    let criteria = criteria.trim_start_matches('[').trim_end_matches(']').replace('"', "");
    Some(format!("vet: {name} {version} is neither audited, imported nor exempted (missing {})", criteria.trim()))
}

/// What a `Vetting Succeeded (...)` line says about how much of the lock is audited, as the words after `supply: ok`:
/// `vet: 0 fully audited, 488 exempted` (cargo vet names partially audited packages on a line of their own, and leaves a
/// zero category out). A line in any other shape (a bare `Vetting Succeeded`, a word or count this program does not
/// know, a label twice, text after the bracket) comes back raw, so the evidence is never dropped. `None` when there is no
/// such line. Only the words of a pass: a failure is judged by `parse_vet` and the exit code, never by this.
pub fn vet_coverage(output: &str) -> Option<String> {
    let text = strip_ansi(output);
    let line = text.lines().map(str::trim).find(|l| l.starts_with("Vetting Succeeded"))?;
    Some(match vet_counts(line) {
        Some((fully, partly, exempted)) => {
            let mut parts = vec![format!("{fully} fully audited")];
            if partly > 0 {
                parts.push(format!("{partly} partially audited"));
            }
            parts.push(format!("{exempted} exempted"));
            format!("vet: {}", parts.join(", "))
        }
        None => format!("vet summary not recognised: {line}"),
    })
}

/// `Vetting Succeeded (7 fully audited, 2 partially audited, 479 exempted)` -> (7, 2, 479). A category that is left out
/// is 0; anything this does not know, or a label twice, is `None`.
fn vet_counts(line: &str) -> Option<(u64, u64, u64)> {
    let inner = line.strip_prefix("Vetting Succeeded")?.trim().strip_prefix('(')?.strip_suffix(')')?;
    let (mut fully, mut partly, mut exempted): (Option<u64>, Option<u64>, Option<u64>) = (None, None, None);
    for part in inner.split(',') {
        let (number, label) = part.trim().split_once(' ')?;
        let number: u64 = number.parse().ok()?;
        let slot = match label.trim() {
            "fully audited" => &mut fully,
            "partially audited" => &mut partly,
            "exempted" => &mut exempted,
            _ => return None,
        };
        if slot.replace(number).is_some() {
            return None;
        }
    }
    Some((fully.unwrap_or(0), partly.unwrap_or(0), exempted.unwrap_or(0)))
}

/// The lines of the verdict: `supply: ok (vet: ...)` with the coverage of the vet pass (F13: a bare `ok` reads as "every
/// library audited"), or every finding and a closing line; `coverage` is not looked at when there is a finding. The exit
/// code follows.
pub fn summarize(findings: &[String], coverage: Option<&str>) -> (Vec<String>, u8) {
    if findings.is_empty() {
        let line = match coverage {
            Some(coverage) => format!("supply: ok ({coverage})"),
            None => String::from("supply: ok (vet coverage not reported)"),
        };
        return (vec![line], 0);
    }
    let mut lines: Vec<String> = findings.iter().map(|f| format!("supply: {f}")).collect();
    let word = if findings.len() == 1 { "finding" } else { "findings" };
    lines.push(format!("supply: failed ({} {word})", findings.len()));
    (lines, 1)
}

/// Replaces the given paths (both slash forms, any case, also with the `\\?\` prefix) by their marker, so no path of
/// this machine reaches the evidence (G18).
pub fn mask_paths(text: &str, roots: &[(String, &str)]) -> String {
    let mut roots: Vec<&(String, &str)> = roots.iter().filter(|(p, _)| !p.trim_matches(['\\', '/']).is_empty()).collect();
    roots.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    let mut masked = text.to_string();
    for (path, marker) in roots {
        let trimmed = path.trim_end_matches(['\\', '/']);
        let back = trimmed.replace('/', "\\");
        let forward = trimmed.replace('\\', "/");
        for needle in [format!("\\\\?\\{back}"), back, forward] {
            masked = replace_ignore_ascii_case(&masked, &needle, marker);
        }
    }
    masked
}

fn replace_ignore_ascii_case(text: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    // ASCII lowercasing keeps every byte offset, so offsets found in the copy are valid in the original.
    let haystack = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(at) = haystack[from..].find(&needle) {
        out.push_str(&text[from..from + at]);
        out.push_str(with);
        from += at + needle.len();
    }
    out.push_str(&text[from..]);
    out
}

// ---------------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------------

/// What the process environment says (a value of its own, so a test can hand in another).
#[derive(Default)]
pub struct Env {
    pub local_app_data: Option<PathBuf>,
    pub tools_dir: Option<PathBuf>,
    pub advisory_db: Option<PathBuf>,
}

impl Env {
    fn from_process() -> Env {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        Env { local_app_data: var("LOCALAPPDATA"), tools_dir: var("SKIPPER_TOOLS_DIR"), advisory_db: var("SKIPPER_ADVISORY_DB") }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub tools_root: PathBuf,
    pub db: PathBuf,
    pub all_targets: bool,
}

pub fn parse_options(args: &[String], env: &Env) -> Result<Options, String> {
    let mut tools: Option<PathBuf> = None;
    let mut db: Option<PathBuf> = None;
    let mut all_targets = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--tools" => tools = Some(PathBuf::from(it.next().ok_or("--tools needs a folder")?)),
            "--db" => db = Some(PathBuf::from(it.next().ok_or("--db needs a folder")?)),
            "--all-targets" => all_targets = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let default = |sub: &str| env.local_app_data.as_ref().map(|base| base.join("skipper-build").join(sub));
    let tools_root = tools
        .or_else(|| env.tools_dir.clone())
        .or_else(|| default("tools"))
        .ok_or("no tools folder: LOCALAPPDATA is not set; give --tools <folder> or SKIPPER_TOOLS_DIR")?;
    let db = db
        .or_else(|| env.advisory_db.clone())
        .or_else(|| default("advisory-db"))
        .ok_or("no advisory database: LOCALAPPDATA is not set; give --db <folder> or SKIPPER_ADVISORY_DB")?;
    Ok(Options { tools_root, db, all_targets })
}

// ---------------------------------------------------------------------------------------------
// The thin I/O part
// ---------------------------------------------------------------------------------------------

/// The repository root: the nearest folder above `start` that holds `Cargo.lock` and `supply-chain/`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|dir| dir.join("Cargo.lock").is_file() && dir.join("supply-chain").is_dir()).map(Path::to_path_buf)
}

fn cargo_program() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

struct Run {
    ok: bool,
    text: String,
}

/// Runs `cargo <args>` in the repository with the tools folder first on `PATH`, offline, without colour or prompts.
fn run_cargo(root: &Path, path: &OsString, args: &[String]) -> Result<Run, String> {
    let out = Command::new(cargo_program())
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TERM_COLOR", "never")
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot start cargo: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(Run { ok: out.status.success(), text })
}

fn path_with(first: &Path) -> Result<OsString, String> {
    let old = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(first.to_path_buf()).chain(std::env::split_paths(&old)))
        .map_err(|e| format!("cannot build PATH: {e}"))
}

/// The date of the newest commit of the database copy, from `git log -1`; the one program besides cargo that runs.
fn db_commit_time(db: &Path) -> Result<u64, String> {
    let shown = db.to_string_lossy().replace('\\', "/");
    let out = Command::new("git")
        .args(["-c", &format!("safe.directory={shown}"), "-C", &shown, "log", "-1", "--format=%ct"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot start git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "the advisory database is not a git checkout (git log failed: {}); copy it as a git bundle clone",
            first_lines(&String::from_utf8_lossy(&out.stderr), 2)
        ));
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .map_err(|_| String::from("the date of the newest commit of the advisory database is not readable"))
}

fn now_unix() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| String::from("the system clock is before 1970"))
}

/// Everything that must hold before a tool runs. Any problem ends the run: there is no partial pass.
fn preflight(root: &Path, opts: &Options, path: &OsString) -> Vec<String> {
    let mut problems = Vec::new();
    for file in ["deny.toml", "config.toml", "audits.toml"] {
        if !root.join("supply-chain").join(file).is_file() {
            problems.push(format!("supply-chain/{file} is missing"));
        }
    }
    if let Ok(config) = std::fs::read_to_string(root.join("supply-chain").join("config.toml")) {
        problems.extend(imports_lock_problem(&config, root.join("supply-chain").join("imports.lock").is_file()));
    }
    let bin = opts.tools_root.join("bin");
    for tool in Tool::ALL {
        if !bin.join(tool.file_name()).is_file() {
            problems.push(format!(
                "{}: {} is not in <tools>\\bin; build it on the build machine: cargo install --locked --offline --root <tools> {}@{}",
                tool.name(),
                tool.file_name(),
                tool.crate_name(),
                tool.pinned()
            ));
            continue;
        }
        match run_cargo(root, path, &version_args(tool)) {
            Ok(run) if run.ok && version_matches(&run.text, tool.pinned()) => {}
            Ok(run) => problems.push(format!(
                "{}: expected {} {}, found: {}",
                tool.name(),
                tool.crate_name(),
                tool.pinned(),
                first_lines(&run.text, 2)
            )),
            Err(e) => problems.push(format!("{}: {e}", tool.name())),
        }
    }
    if !opts.db.join("crates").is_dir() {
        problems.push(String::from("audit: <db> is missing or holds no `crates` folder; copy the RustSec advisory database in the fetch phase"));
    } else {
        let verdict = db_commit_time(&opts.db).and_then(|commit| now_unix().and_then(|now| db_age_verdict(commit, now)));
        if let Err(why) = verdict {
            problems.push(format!("audit: {why}"));
        }
    }
    problems
}

/// What the three tools said: the findings, the notes, the raw outputs of the tools that found something, and the
/// coverage words of the vet pass (`None` unless vet passed with a summary line).
struct Outcome {
    findings: Vec<String>,
    notes: Vec<String>,
    raw: Vec<(Tool, String)>,
    vet_coverage: Option<String>,
}

/// Runs the three tools.
fn run_tools(root: &Path, opts: &Options, path: &OsString) -> Outcome {
    let mut findings = Vec::new();
    let mut notes = Vec::new();
    let mut raw = Vec::new();
    let mut coverage = None;
    let sub = |name: &str| root.join(name).to_string_lossy().into_owned();
    for tool in Tool::ALL {
        let args = match tool {
            Tool::Deny => deny_args(&sub("supply-chain/deny.toml")),
            Tool::Audit => audit_args(&opts.db.to_string_lossy(), &sub("Cargo.lock")),
            Tool::Vet => vet_args(opts.all_targets),
        };
        let run = match run_cargo(root, path, &args) {
            Ok(run) => run,
            Err(e) => {
                findings.push(format!("{}: {e}", tool.name()));
                continue;
            }
        };
        let tool_findings = match tool {
            Tool::Deny => parse_deny(&run.text, run.ok),
            Tool::Audit => {
                let report = parse_audit(&run.text, run.ok);
                notes.extend(report.notes);
                report.findings
            }
            Tool::Vet => {
                let vet_findings = parse_vet(&run.text, run.ok);
                if vet_findings.is_empty() {
                    coverage = vet_coverage(&run.text);
                }
                vet_findings
            }
        };
        if !tool_findings.is_empty() {
            raw.push((tool, run.text));
        }
        findings.extend(tool_findings);
    }
    Outcome { findings, notes, raw, vet_coverage: coverage }
}

pub fn run_cli(args: &[String]) -> u8 {
    let opts = match parse_options(args, &Env::from_process()) {
        Ok(opts) => opts,
        Err(why) => {
            eprintln!("supply: {why}");
            eprintln!("usage: cargo xtask supply [--tools <folder>] [--db <folder>] [--all-targets]");
            return 2;
        }
    };
    let root = match std::env::current_dir().ok().and_then(|cwd| find_root(&cwd)) {
        Some(root) => root,
        None => {
            println!("supply: failed: no repository found (a folder with Cargo.lock and supply-chain/ above the current folder)");
            return 1;
        }
    };
    let mut roots = vec![
        (root.to_string_lossy().into_owned(), "<repo>"),
        (opts.tools_root.to_string_lossy().into_owned(), "<tools>"),
        (opts.db.to_string_lossy().into_owned(), "<db>"),
    ];
    for (var, marker) in [("LOCALAPPDATA", "<local>"), ("USERPROFILE", "<home>"), ("TEMP", "<temp>")] {
        if let Some(value) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            roots.push((value.to_string_lossy().into_owned(), marker));
        }
    }
    let say = |line: &str| println!("{}", mask_paths(line, &roots));

    let path = match path_with(&opts.tools_root.join("bin")) {
        Ok(path) => path,
        Err(why) => {
            say(&format!("supply: failed: {why}"));
            return 1;
        }
    };
    let problems = preflight(&root, &opts, &path);
    if !problems.is_empty() {
        let (lines, code) = summarize(&problems, None);
        lines.iter().for_each(|l| say(l));
        return code;
    }
    let Outcome { findings, notes, raw, vet_coverage } = run_tools(&root, &opts, &path);
    if !opts.all_targets {
        say(&format!(
            "supply: scope: cargo-deny and cargo-vet judge the {WINDOWS_TARGET} graph (the Linux-only tree of the lock is not downloaded); --all-targets after a full fetch"
        ));
    }
    notes.iter().for_each(|n| say(&format!("supply: {n}")));
    for (tool, text) in &raw {
        for line in strip_ansi(text).lines().filter(|l| !l.trim().is_empty()).take(MAX_RAW_LINES) {
            say(&format!("supply: {} | {line}", tool.name()));
        }
    }
    let (lines, code) = summarize(&findings, vet_coverage.as_deref());
    lines.iter().for_each(|l| say(l));
    code
}
