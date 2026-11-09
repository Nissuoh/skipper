// SPDX-License-Identifier: MIT
//! `cargo xtask leak-scan` (plan Task 3; MC-122 to MC-125, ACC-13, MC-163, G16 to G18, G22; rulings PR-2, PR-3).
//!
//! ```text
//! cargo xtask leak-scan (--staged | --range <a>..<b> | --range <b>) [--config <dir>] [--allow <file> | --rules-from <commit>] [--accept-judge-change <commit>] [--repo <dir>]
//! ```
//!
//! Exit 0 with `leak-scan: clean`, or exit 1 with one line per finding. Banned terms are shown by number, never
//! by text. Exit 2 means the scan refused to run (settings folder unset, missing or inside the repository;
//! planted `.gitleaks.toml` or `.gitleaksignore`; an untracked or changed `.gitattributes`; gitleaks missing;
//! usage error).
//!
//! Imported content never goes into a push (IMP-13, MC-494, RL-05): a Markdown file whose `+++` header has a `source`
//! key is a page the import made (`ImportedPage`), and a path with a `.files` folder below a `knowledge` folder is a file
//! of such a page (`ImportedFile`). Neither is admitted by the allow list; both are looked for in every added patch,
//! every changed path and every file of the end state, so a page that a later commit removes is still found in the range.
//! The name in the header of a patch may be quoted (a letter above 0x7F, a quote or a backslash in it) or end in a TAB (a
//! space in it); both are read as the `.md` they are. Two hand-made cases stay outside the patch scan and are found only
//! while the page is at the tip of the range, by the end state: a commit that only adds a `source` line to the header of a
//! page that already exists (its `-U0` patch has no first line `+++`), and a page saved as UTF-16 (its patch is not the text of
//! the page). The import writes UTF-8 pages with the whole header, so both need an edit by hand; such a page that a later
//! commit of the same range removes again is not found.
//!
//! Secret files and languages the project does not use (GATE-3): the rule is this scan's, whatever GitHub's own rulesets and push
//! protection do or do not offer for the repository. A committed path
//! whose name marks a secret (`.env`, `.env.*`, `*.env`, `.envrc`, `*.pem`, `*.key`, `*.pfx`, `*.p12`, `*.keystore`,
//! `*.jks`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`, `credentials*.json`, `.mcp.json`, `secrets.*` and `secret.*`
//! other than Rust source) is a `SecretFile`, and no allow-list entry admits it: the `.gitignore` lines for the same names
//! are no protection against `git add -f`. `.env.example` is the one `.env` file that may go in, only while it lists names
//! and no value, and an `.npmrc` or `.pypirc` only while it holds no token (`SecretContent`, found in the added lines of
//! every commit and in the end state). A source file of a language the project does not use (Python, JavaScript,
//! TypeScript), a Node package or lock file, or anything below a `node_modules` folder is a `BannedLanguage` unless
//! `scan/allowed-paths.txt` names its exact path, no wildcard, with a comment line of at least three words directly above the
//! entry that says why. These rules look at paths (the changed paths of every commit, the staged paths and every file at the
//! tip of the range), never at branch or tag names. The judge rules below apply: the change of this scan is itself a change
//! of judge code.
//!
//! The pure checks (`scan_text`, `scan_commit`, `scan_names`, `scan_paths`, `scan_manifest`, `check_scanner`) are tested in
//! this file; the git layer only reads. Every `git log` and `git show` runs with `--no-use-mailmap`, because git
//! maps identities through a working-tree `.mailmap` that the build box could write (PR-2). Every git process,
//! and gitleaks with the git it starts, runs with `GIT_ATTR_SOURCE` set to the empty tree, and the own diffs use
//! `--text`: a `.gitattributes` line such as `* -diff` or `binary` must not turn a patch into "Binary files
//! differ" and so hide its content from the scan. The allow list and
//! the gitleaks pin are read from `HEAD` whenever `HEAD` holds them, so a build step cannot widen the list or
//! swap the pin by editing the working tree (NR-14); only while no commit holds a file is the working-tree copy
//! used. The scan of a push reads both from the commit named by `--rules-from` and from nothing else (H4 of the audit
//! of 2026-10-23): the commits that are pushed must not approve their own push by widening the allow list; a `--range`
//! scan without `--rules-from` refuses to run, with or without `--allow`. Refs under
//! `refs/replace/` and a grafts file are refused, and every git process runs with replacement objects off (M9).
//! A range in which any commit changes judge code or judge input (`xtask/`, `tools/`, `scan/`, `supply-chain/`,
//! `.cargo/`, `rust-toolchain.toml`, `Cargo.lock`, any `Cargo.toml`) is a finding unless `--accept-judge-change` names
//! the tip of the range; then the scan prints a note and passes (B1 of the security amendments). The merge commit of
//! GitHub's merge button passes only in its exact shape: GitHub as committer, the subject of a pull-request merge, two
//! parents, and no path that differs from both parents (G1).
//! `--repo <dir>` points the scan at another repository (the scanner of a push runs from a clean export
//! and scans the real repository, G17); without it the current folder is the repository.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const USAGE: &str = "usage: cargo xtask leak-scan (--staged | --range <a>..<b> | --range <b>) [--config <dir>] [--allow <file> | --rules-from <commit>] [--accept-judge-change <commit>] [--repo <dir>]";
const ALLOW_PATH: &str = "scan/allowed-paths.txt";
const PINS_PATH: &str = "supply-chain/pins.toml";
/// The empty tree object (SHA-1 repositories). `GIT_ATTR_SOURCE` set to it makes git read its attribute rules
/// from a tree that holds none, instead of from the working tree. Needs git 2.40 or newer.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// The oldest git that honours `GIT_ATTR_SOURCE`; an older one ignores the variable without a word.
const MIN_GIT_VERSION: (u32, u32) = (2, 40);
/// File name extensions of images and videos; `**` patterns never admit them (MC-124).
const MEDIA_EXTENSIONS: [&str; 11] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "mp4", "webm", "mov"];
/// Trailer names that are findings wherever they stand in a commit message (G16).
const KNOWN_TRAILERS: [&str; 10] = [
    "signed-off-by",
    "co-authored-by",
    "claude-session",
    "reviewed-by",
    "acked-by",
    "tested-by",
    "reported-by",
    "suggested-by",
    "helped-by",
    "change-id",
];

// ---------------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Term {
    /// Lower-case text.
    text: String,
    /// `word:<term>` in `terms.txt`: matches whole words only.
    word: bool,
}

/// One line of `terms.txt`: blank lines and `#` comments are skipped; `word:<term>` matches whole words only.
fn parse_term(line: &str) -> Option<Term> {
    let line = line.trim().trim_start_matches('\u{feff}').trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (word, body) = match line.strip_prefix("word:") {
        Some(rest) => (true, rest.trim()),
        None => (false, line),
    };
    if body.is_empty() {
        return None;
    }
    Some(Term { text: body.to_lowercase(), word })
}

/// The entries of an allow list: one pattern per line, blank lines and `#` comments skipped.
fn parse_allow(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// The entries of an allow list that carry a reason (GATE-3): the line directly above the entry is a comment of at least
/// three words. A file of a language the project does not use passes only with its exact path and such a comment, so the
/// list says why; a bare path is not enough, and a comment above a whole block of entries holds for the first one only.
fn parse_reasoned(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut reason_above = false;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            reason_above = false;
        } else if let Some(comment) = line.strip_prefix('#') {
            reason_above = comment.split_whitespace().count() >= 3;
        } else {
            if reason_above {
                out.push(line.to_string());
            }
            reason_above = false;
        }
    }
    out
}

/// The banned terms, the allow list and the project identity.
#[derive(Clone, Debug)]
pub struct ScanConfig {
    terms: Vec<Term>,
    allow: Vec<String>,
    /// The entries of the allow list that carry a reason (`parse_reasoned`).
    reasoned: Vec<String>,
    name: String,
    noreply: String,
}

impl ScanConfig {
    /// Reads `terms.txt` and `identity.toml` (`name`, `noreply`) from the scan settings folder; the allow list
    /// comes in as text. Error messages never repeat file content.
    pub fn load(config_dir: &Path, allow_text: &str) -> io::Result<Self> {
        let terms_text = std::fs::read_to_string(config_dir.join("terms.txt"))?;
        let identity_text = std::fs::read_to_string(config_dir.join("identity.toml"))?;
        let identity: toml::Table = toml::from_str(&identity_text)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "identity.toml is not valid TOML"))?;
        let text_of = |key: &str| identity.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let name = text_of("name");
        let noreply = text_of("noreply");
        if name.is_empty() || noreply.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "identity.toml needs name and noreply"));
        }
        let terms: Vec<Term> = terms_text.lines().filter_map(parse_term).collect();
        if terms.is_empty() {
            // A list of comments only would make every term check pass vacuously (M12).
            return Err(io::Error::new(io::ErrorKind::InvalidData, "terms.txt holds no terms"));
        }
        Ok(ScanConfig {
            terms,
            allow: parse_allow(allow_text),
            reasoned: parse_reasoned(allow_text),
            name,
            noreply,
        })
    }

    /// A configuration for tests (also used by `stage0`): terms in `terms.txt` form, allow-list patterns, the
    /// no-reply address; the identity name is `P`. Not `#[cfg(test)]`-gated on purpose. The patterns come without
    /// comment lines, so every one of them counts as explained (`parse_reasoned`).
    pub fn for_test(terms: &[&str], allow: &[&str], noreply: &str) -> Self {
        ScanConfig {
            terms: terms.iter().filter_map(|t| parse_term(t)).collect(),
            allow: allow.iter().map(|a| a.to_string()).collect(),
            reasoned: allow.iter().map(|a| a.to_string()).collect(),
            name: "P".to_string(),
            noreply: noreply.to_string(),
        }
    }

    fn is_noreply(&self, address: &str) -> bool {
        !self.noreply.is_empty() && address.eq_ignore_ascii_case(&self.noreply)
    }

    /// The numbers (positions in the term list) of every term found in `text`.
    fn matched_terms(&self, text: &str) -> Vec<usize> {
        let lower = text.to_lowercase();
        self.terms
            .iter()
            .enumerate()
            .filter(|(_, term)| term_in(&lower, term))
            .map(|(n, _)| n)
            .collect()
    }

    /// Whether the allow list admits a committed path. `**` patterns never admit an image or video, nor an
    /// attributes file; such a file passes only with its exact path listed (MC-124).
    pub fn path_allowed(&self, path: &str) -> bool {
        let exact_only = is_media(path) || is_attributes_file(path);
        self.allow.iter().any(|pattern| {
            let wild = pattern.contains('*') || pattern.contains('?');
            if exact_only {
                !wild && pattern == path
            } else if wild {
                glob_match(pattern.as_bytes(), path.as_bytes())
            } else {
                pattern == path
            }
        })
    }

    /// Whether the allow list names exactly this path, without a wildcard. A binary file passes only so (I1).
    fn path_listed_exactly(&self, path: &str) -> bool {
        self.allow
            .iter()
            .any(|pattern| !(pattern.contains('*') || pattern.contains('?')) && pattern == path)
    }

    /// Whether the allow list lets a file of a language the project does not use through (GATE-3): only an entry that
    /// names exactly this path, without a wildcard, and carries a reason (`parse_reasoned`). No `**` pattern admits such a
    /// file. No entry at all admits a secret file (`secret_name_rule`).
    fn banned_language_allowed(&self, path: &str) -> bool {
        self.reasoned
            .iter()
            .any(|entry| !(entry.contains('*') || entry.contains('?')) && entry == path)
    }
}

/// Glob match: `**` matches anything including `/`, `*` matches anything except `/`, `?` one character
/// except `/`; everything else matches itself, case-sensitively.
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            if rest.first() == Some(&b'*') {
                let mut rest = rest;
                while rest.first() == Some(&b'*') {
                    rest = &rest[1..];
                }
                (0..=text.len()).any(|i| glob_match(rest, &text[i..]))
            } else {
                let limit = text.iter().position(|&c| c == b'/').unwrap_or(text.len());
                (0..=limit).any(|i| glob_match(rest, &text[i..]))
            }
        }
        Some((b'?', rest)) => text.first().is_some_and(|&c| c != b'/') && glob_match(rest, &text[1..]),
        Some((&c, rest)) => text.first() == Some(&c) && glob_match(rest, &text[1..]),
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `lower` is the lower-cased text. A `word:` term must stand between non-word characters.
fn term_in(lower: &str, term: &Term) -> bool {
    if !term.word {
        return lower.contains(&term.text);
    }
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&term.text) {
        let start = from + pos;
        let end = start + term.text.len();
        let before_ok = lower[..start].chars().next_back().is_none_or(|c| !is_word_char(c));
        let after_ok = lower[end..].chars().next().is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            return true;
        }
        from = start + lower[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

// ---------------------------------------------------------------------------------------------------------
// Findings and data
// ---------------------------------------------------------------------------------------------------------

/// One thing the scan objects to. Banned terms appear by number only.
#[derive(Debug, PartialEq)]
pub enum Finding {
    /// Banned term number `n` (position in `terms.txt`, counted from 0) found at `at`.
    Term { n: usize, at: String },
    /// An e-mail address other than the project's no-reply address.
    Email { at: String },
    /// A commit trailer other than the single sign-off.
    Trailer { at: String },
    MissingSignOff { commit: String },
    /// Author or committer is not the project identity.
    Identity { commit: String },
    /// A committed path the allow list does not admit.
    Path { path: String },
    /// An expanded user-profile path.
    ProfilePath { at: String },
    /// A gitleaks hit, redacted.
    Secret { rule: String, at: String },
    /// A brand word in a branch, tag or file name, in a Cargo name or in an annotated tag text (MC-163).
    Brand { at: String },
    /// A Cargo manifest that does not set `publish = false` (G22).
    Publishable { manifest: String },
    /// The gitleaks executable differs from the pinned SHA-256.
    ScannerChanged,
    /// A binary file at the end state whose exact path the allow list does not name (I1).
    Binary { path: String },
    /// A file at the end state that could not be read; the scan fails closed (M2).
    Unreadable { path: String },
    /// A Markdown file whose header has a `source` key: a page the import made from another service (IMP-13, MC-494). The
    /// content is the person's own and private; it never goes into a push.
    ImportedPage { at: String },
    /// A file below `knowledge/.files/`: an image or an attachment of an imported page (IMP-13, MC-494).
    ImportedFile { path: String },
    /// A range that changes judge code or judge input without the person's acceptance (B1).
    JudgeChange { rules: String },
    /// A committed path whose name marks a secret: an `.env` file, a key or certificate file, a credentials file, a
    /// secrets file. `rule` says so in plain words. No allow-list entry admits it (GATE-3).
    SecretFile { path: String, rule: &'static str },
    /// A file that holds what its name allows only without: an `.env.example` with a value, an `.npmrc` with an auth
    /// token, a `.pypirc` with a password. Found in the added lines of a patch and in the end state (GATE-3).
    SecretContent { rule: &'static str, at: String },
    /// A file of a language the project does not use (Python, JavaScript, TypeScript, Node packages) whose exact path the
    /// allow list does not name with a reason (GATE-3).
    BannedLanguage { path: String, rule: &'static str },
}

/// What the scan needs to know of one commit (or of the staged changes).
#[derive(Clone, Debug, Default)]
pub struct CommitData {
    pub id: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub message: String,
    /// The patch; only added lines are scanned.
    pub diff: String,
    pub paths: Vec<String>,
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------------------------------------
// Text checks
// ---------------------------------------------------------------------------------------------------------

fn is_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

fn is_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-'))
        && labels
            .last()
            .is_some_and(|l| l.len() >= 2 && l.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Every string shaped as a local part, the at sign and a dotted domain.
fn email_candidates(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at_pos, _) in text.match_indices('@') {
        let before = &text[..at_pos];
        let local_len: usize = before.chars().rev().take_while(|c| is_local_char(*c)).map(char::len_utf8).sum();
        let local = &before[before.len() - local_len..];
        if local.is_empty() {
            continue;
        }
        let after = &text[at_pos + 1..];
        let domain_len: usize = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .map(char::len_utf8)
            .sum();
        let domain = after[..domain_len].trim_end_matches(['.', '-']);
        if is_domain(domain) {
            found.push(format!("{local}@{domain}"));
        }
    }
    found
}

fn is_sep(b: u8) -> bool {
    b == b'\\' || b == b'/'
}

fn skip_seps(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_sep(b[i]) {
        i += 1;
    }
    i
}

fn eq_ci(b: &[u8], i: usize, word: &str) -> bool {
    b.len() >= i + word.len() && b[i..i + word.len()].eq_ignore_ascii_case(word.as_bytes())
}

fn name_follows(b: &[u8], i: usize) -> bool {
    b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c >= 0x80)
}

fn is_path_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b'/' | b'\\' | b'~' | b'%' | b'$')
}

/// `word`, one or more separators and the start of a name, beginning at `i`.
fn word_then_name(b: &[u8], i: usize, word: &str) -> bool {
    if !eq_ci(b, i, word) {
        return false;
    }
    let k = skip_seps(b, i + word.len());
    k > i + word.len() && name_follows(b, k)
}

/// An expanded user-profile path. Recognised shapes: a drive letter, the `Users` folder and a name joined by
/// backslashes or slashes (doubled backslashes too), also with the colon written `%3A` as in a file URL; the same
/// below an administrative share (a drive letter and a dollar sign after a host name); the form that Git Bash,
/// MSYS, WSL and Cygwin print, with the drive letter as the first folder and an optional `mnt` or `cygdrive`
/// folder before it; a `home` or `Users` folder below a WSL share (the host name `wsl` with a dollar sign, or
/// `wsl.localhost`, then the name of the distribution); a file URL that goes straight to a `home` or `Users` folder;
/// and a Unix home root (`home`, `Users`) at the start of a path followed by a name.
fn has_profile_path(text: &str) -> bool {
    let b = text.as_bytes();
    for i in 0..b.len() {
        let word_start = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if b[i].is_ascii_alphabetic() && word_start {
            let drive_end = if b.get(i + 1) == Some(&b':') {
                Some(i + 2)
            } else if eq_ci(b, i + 1, "%3a") {
                Some(i + 4)
            } else {
                None
            };
            if let Some(end) = drive_end {
                let j = skip_seps(b, end);
                if j > end && word_then_name(b, j, "users") {
                    return true;
                }
            }
            if b.get(i + 1) == Some(&b'$') && i > 0 && is_sep(b[i - 1]) {
                let j = skip_seps(b, i + 2);
                if j > i + 2 && word_then_name(b, j, "users") {
                    return true;
                }
            }
        }
        if i > 0 && is_sep(b[i - 1]) {
            // The first host name is written in two parts, so that a plain text scan for the share name does not flag this source.
            for host in [concat!("wsl", "$"), "wsl.localhost"] {
                if !eq_ci(b, i, host) {
                    continue;
                }
                let distro = skip_seps(b, i + host.len());
                if distro == i + host.len() {
                    continue;
                }
                let distro_end = distro + b[distro..].iter().take_while(|c| !is_sep(**c)).count();
                let root = skip_seps(b, distro_end);
                if root > distro_end && (word_then_name(b, root, "home") || word_then_name(b, root, "users")) {
                    return true;
                }
            }
        }
        if word_start && eq_ci(b, i, "file:") {
            let j = skip_seps(b, i + 5);
            if j >= i + 7 && (word_then_name(b, j, "home") || word_then_name(b, j, "users")) {
                return true;
            }
        }
        if b[i] == b'/' && (i == 0 || !is_path_char(b[i - 1])) {
            for root in ["home", "users"] {
                if eq_ci(b, i + 1, root)
                    && b.get(i + 1 + root.len()) == Some(&b'/')
                    && name_follows(b, i + 2 + root.len())
                {
                    return true;
                }
            }
            let mut k = i + 1;
            for prefix in ["mnt/", "cygdrive/"] {
                if eq_ci(b, k, prefix) {
                    k += prefix.len();
                    break;
                }
            }
            if b.get(k).is_some_and(|c| c.is_ascii_alphabetic())
                && b.get(k + 1) == Some(&b'/')
                && word_then_name(b, k + 2, "users")
            {
                return true;
            }
        }
    }
    false
}

/// A secret that shows by its first characters and a run of token characters after them. This short own list
/// stands behind gitleaks, which reads only the patches of files (I5): it covers commit messages, tag texts and
/// names, and it still sees a file whose patch gitleaks could not read. A mention of a prefix alone is no
/// finding; the run must be long enough.
struct TokenShape {
    rule: &'static str,
    prefix: &'static str,
    /// How many token characters must follow the prefix.
    min_tail: usize,
    /// Which bytes count as token characters.
    in_class: fn(u8) -> bool,
    /// The prefix must not stand inside a longer word.
    word_start: bool,
}

fn is_alnum_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_alnum_dash_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-'
}

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn is_token_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

fn is_upper_digit_byte(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

const TOKEN_SHAPES: &[TokenShape] = &[
    TokenShape { rule: "own-ai-api-key", prefix: "sk-ant-", min_tail: 20, in_class: is_token_byte, word_start: false },
    TokenShape { rule: "own-github-token", prefix: "ghp_", min_tail: 36, in_class: is_alnum_byte, word_start: false },
    TokenShape { rule: "own-github-token", prefix: "gho_", min_tail: 36, in_class: is_alnum_byte, word_start: false },
    TokenShape { rule: "own-github-token", prefix: "ghu_", min_tail: 36, in_class: is_alnum_byte, word_start: false },
    TokenShape { rule: "own-github-token", prefix: "ghs_", min_tail: 36, in_class: is_alnum_byte, word_start: false },
    TokenShape { rule: "own-github-token", prefix: "ghr_", min_tail: 36, in_class: is_alnum_byte, word_start: false },
    TokenShape { rule: "own-github-token", prefix: "github_pat_", min_tail: 22, in_class: is_word_byte, word_start: false },
    TokenShape { rule: "own-aws-key-id", prefix: "AKIA", min_tail: 16, in_class: is_upper_digit_byte, word_start: true },
    TokenShape { rule: "own-aws-key-id", prefix: "ASIA", min_tail: 16, in_class: is_upper_digit_byte, word_start: true },
    TokenShape { rule: "own-slack-token", prefix: "xoxb-", min_tail: 10, in_class: is_alnum_dash_byte, word_start: false },
    TokenShape { rule: "own-slack-token", prefix: "xoxa-", min_tail: 10, in_class: is_alnum_dash_byte, word_start: false },
    TokenShape { rule: "own-slack-token", prefix: "xoxp-", min_tail: 10, in_class: is_alnum_dash_byte, word_start: false },
    TokenShape { rule: "own-slack-token", prefix: "xoxr-", min_tail: 10, in_class: is_alnum_dash_byte, word_start: false },
    TokenShape { rule: "own-slack-token", prefix: "xoxs-", min_tail: 10, in_class: is_alnum_dash_byte, word_start: false },
    TokenShape { rule: "own-google-key", prefix: "AIza", min_tail: 35, in_class: is_token_byte, word_start: true },
];

/// Whether `text` holds the prefix of `shape` followed by a long enough run of token characters.
fn has_prefixed_token(text: &str, shape: &TokenShape) -> bool {
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(pos) = text[from..].find(shape.prefix) {
        let start = from + pos;
        let tail_from = start + shape.prefix.len();
        let tail = b[tail_from..].iter().take_while(|c| (shape.in_class)(**c)).count();
        let word_ok = !shape.word_start || start == 0 || !b[start - 1].is_ascii_alphanumeric();
        if word_ok && tail >= shape.min_tail {
            return true;
        }
        // Every prefix starts with an ASCII character, so the next byte starts a character.
        from = start + 1;
    }
    false
}

/// A PEM block header for a private key: five dashes, `BEGIN`, capital letters and spaces that end in
/// `PRIVATE KEY`, five dashes.
fn has_private_key_block(text: &str) -> bool {
    const BEGIN: &str = "-----BEGIN ";
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(pos) = text[from..].find(BEGIN) {
        let start = from + pos;
        let label_from = start + BEGIN.len();
        let label_len = b[label_from..].iter().take_while(|c| c.is_ascii_uppercase() || **c == b' ').count();
        let label = &b[label_from..label_from + label_len];
        if label.ends_with(b"PRIVATE KEY") && b[label_from + label_len..].starts_with(b"-----") {
            return true;
        }
        from = start + 1;
    }
    false
}

/// The rule name of every secret shape found in `text`, each once.
fn token_rules(text: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for shape in TOKEN_SHAPES {
        if !out.contains(&shape.rule) && has_prefixed_token(text, shape) {
            out.push(shape.rule);
        }
    }
    if has_private_key_block(text) {
        out.push("own-private-key");
    }
    out
}

/// Banned terms (by number), e-mail addresses other than the no-reply address, profile paths and secret-shaped
/// tokens in `text`. Each term, each distinct address, the profile path and each secret rule are reported once
/// per call, at `at`. Zero bytes are looked past: UTF-16 text read byte by byte has one between its letters.
pub fn scan_text(cfg: &ScanConfig, text: &str, at: &str) -> Vec<Finding> {
    let mut out = scan_plain_text(cfg, text, at);
    if text.contains('\0') {
        let squeezed = text.replace('\0', "");
        for finding in scan_plain_text(cfg, &squeezed, at) {
            if !out.contains(&finding) {
                out.push(finding);
            }
        }
    }
    out
}

fn scan_plain_text(cfg: &ScanConfig, text: &str, at: &str) -> Vec<Finding> {
    let mut out: Vec<Finding> = cfg
        .matched_terms(text)
        .into_iter()
        .map(|n| Finding::Term { n, at: at.to_string() })
        .collect();
    let mut seen = BTreeSet::new();
    for address in email_candidates(text) {
        if cfg.is_noreply(&address) {
            continue;
        }
        if seen.insert(address.to_ascii_lowercase()) {
            out.push(Finding::Email { at: at.to_string() });
        }
    }
    if has_profile_path(text) {
        out.push(Finding::ProfilePath { at: at.to_string() });
    }
    for rule in token_rules(text) {
        out.push(Finding::Secret { rule: rule.to_string(), at: at.to_string() });
    }
    if is_markdown_place(at) && has_imported_header(text) {
        out.push(Finding::ImportedPage { at: at.to_string() });
    }
    out
}

/// Whether `at`, the place of a scanned text, names a Markdown file: a path (`docs/x.md`) or a commit and a path
/// (`01234567:docs/x.md`), in any letter case (a case-insensitive file system keeps `X.MD` as it is written). The place of a
/// patch is read from its header lines, and git writes more than the name there: a name with a byte above 0x7F, a quote, a
/// backslash or a control character in quotes (`+++ "b/docs/\303\274x.md"`, unless `core.quotePath` is off, and the quote
/// and the backslash are quoted either way), and a TAB after the name when it holds a space (`+++ b/docs/my notes.md<TAB>`).
/// A closing quote and a TAB at the end are not part of the name, so they do not hide the `.md`.
fn is_markdown_place(at: &str) -> bool {
    at.trim_end_matches(['\t', '"']).to_ascii_lowercase().ends_with(".md")
}

/// Whether a line of a page header is the key `source`: the bare or a quoted spelling, blanks allowed around the `=`. A
/// longer key (`source_url`, `sources`), a comment and a value that only holds the word are none.
fn is_source_key_line(line: &str) -> bool {
    ["source", "\"source\"", "'source'"]
        .iter()
        .any(|key| line.strip_prefix(*key).is_some_and(|rest| rest.trim_start().starts_with('=')))
}

/// Whether `text` is a knowledge page that the import made: a first line `+++` (a byte order mark and CRLF line ends are
/// fine, as the page reader takes them), then header lines, one of them the key `source`, before the closing `+++` line
/// (`knowledge::page::parse_page_with`; the import writes `source = "<where>"`, `render_page_with`). A header that is never
/// closed is no page for the reader, and is still found here: the scan errs towards refusing. xtask links no product crate,
/// so the shape is read again here; the tests hold a page as the store writes it.
fn has_imported_header(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("+++") {
        return false;
    }
    for line in lines {
        let line = line.trim_end();
        if line == "+++" {
            return false;
        }
        if is_source_key_line(line.trim_start()) {
            return true;
        }
    }
    false
}

/// Whether a path has a folder `.files` below a folder `knowledge`, with either slash and in any letter case: where the
/// import stores the images and attachments of its pages (`knowledge::files::FILES_DIR`, a file named by its SHA-256).
fn is_imported_file_path(path: &str) -> bool {
    let mut in_knowledge = false;
    for segment in path.split(['/', '\\']) {
        if in_knowledge && segment.eq_ignore_ascii_case(".files") {
            return true;
        }
        if segment.eq_ignore_ascii_case("knowledge") {
            in_knowledge = true;
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------------------
// Names and manifests
// ---------------------------------------------------------------------------------------------------------

/// "claude" or "anthropic" in any letter case, also inside longer words.
pub fn brand_word_in(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("claude") || lower.contains("anthropic")
}

fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn is_media(name: &str) -> bool {
    last_segment(name)
        .rsplit_once('.')
        .is_some_and(|(_, ext)| MEDIA_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// An attributes file of git, in any folder and any letter case (a case-insensitive file system reads
/// `.GitAttributes` as well).
fn is_attributes_file(name: &str) -> bool {
    last_segment(name).eq_ignore_ascii_case(".gitattributes")
}

/// A name gets the same text checks as file content: terms, e-mail addresses, profile paths and token shapes
/// (I5, M3), and a brand word. A path below `knowledge/.files/` is a finding of its own: the file of an imported page
/// (IMP-13, MC-494); the allow list does not admit it, because `docs/**` would admit a stored name without an extension.
fn scan_name(cfg: &ScanConfig, name: &str) -> Vec<Finding> {
    let mut out = scan_text(cfg, name, name);
    if brand_word_in(name) {
        out.push(Finding::Brand { at: name.to_string() });
    }
    if is_imported_file_path(name) {
        out.push(Finding::ImportedFile { path: name.to_string() });
    }
    out
}

/// Branch, tag and file names: banned terms, e-mail addresses, profile paths, token shapes, and a brand word in
/// any of them. A plain name does not say whether it is a branch or a file, so file names of every type are
/// checked, not only images and videos (plan Task 3 Step 3, stricter than MC-163).
pub fn scan_names(cfg: &ScanConfig, names: &[String]) -> Vec<Finding> {
    names.iter().flat_map(|n| scan_name(cfg, n)).collect()
}

// ---------------------------------------------------------------------------------------------------------
// Secret files and languages the project does not use (GATE-3)
// ---------------------------------------------------------------------------------------------------------

/// The one `.env` file that may be committed, and only while it lists names and holds no value.
const ENV_EXAMPLE: &str = ".env.example";
/// Extensions of files that hold key material or a key store. Any dot-separated part of a name after its first counts, so a
/// backup such as `server.pem.bak` is a key file too, and `key.rs` is none.
const KEY_EXTENSIONS: [&str; 6] = ["pem", "key", "pfx", "p12", "keystore", "jks"];
/// The starts of the names of SSH private keys (`id_rsa`, `id_rsa.pub`, `id_ed25519.bak`).
const SSH_KEY_PREFIXES: [&str; 4] = ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];
/// Source files of Python.
const PYTHON_EXTENSIONS: [&str; 3] = ["py", "pyw", "pyi"];
/// Source files of JavaScript.
const JAVASCRIPT_EXTENSIONS: [&str; 4] = ["js", "mjs", "cjs", "jsx"];
/// Source files of TypeScript.
const TYPESCRIPT_EXTENSIONS: [&str; 4] = ["ts", "tsx", "mts", "cts"];
/// Package and lock files of the Node tools.
const NODE_PACKAGE_FILES: [&str; 5] = ["package.json", "package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml"];

/// The last segment of a path as a Windows checkout reads it: `\` is a separator as well, letters in any case, and the dots,
/// blanks, tabs and quotes at the end of the name dropped (the place of a patch may end in a TAB or a quote,
/// `is_markdown_place`).
fn file_name_key(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.trim_end_matches(['.', ' ', '\t', '"']).to_ascii_lowercase()
}

/// The rule, in plain words, that a path breaks by its name alone, if any (GATE-3): an `.env` file (`.env`, `.env.*`,
/// `*.env`, `.envrc`; `.env.example` is looked at by its content instead), a key, certificate or key store file (`*.pem`,
/// `*.key`, `*.pfx`, `*.p12`, `*.keystore`, `*.jks`, `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`), a credentials file
/// (`credentials*.json`, `*credentials.json`), the MCP server file `.mcp.json`, and `secrets.*` or `secret.*` other than
/// Rust source. This is the list of the secrets block of the `.gitignore`, which `git add -f` goes past, plus the rest of
/// the row. No allow-list entry admits such a path.
fn secret_name_rule(path: &str) -> Option<&'static str> {
    let name = file_name_key(path);
    if name == ENV_EXAMPLE {
        return None;
    }
    if name == ".env" || name == ".envrc" || name.starts_with(".env.") || name.ends_with(".env") {
        return Some("an .env file holds environment values and secrets");
    }
    if name.split('.').skip(1).any(|part| KEY_EXTENSIONS.contains(&part))
        || SSH_KEY_PREFIXES.iter().any(|prefix| name.starts_with(*prefix))
    {
        return Some("a key, certificate or key store file holds secret key material");
    }
    let bare = name.trim_start_matches('.');
    if (bare.starts_with("credentials") && bare.ends_with(".json")) || name.ends_with("credentials.json") {
        return Some("a credentials file holds sign-in data");
    }
    if name == ".mcp.json" {
        return Some("an MCP server file can hold tokens");
    }
    // Rust source is the project's own language and its content is scanned like any other (`secret.rs`, `secrets.rs`).
    let stem = bare.split_once('.').map(|(stem, _)| stem);
    if matches!(stem, Some("secrets" | "secret")) && !name.ends_with(".rs") {
        return Some("a secrets file holds secrets");
    }
    None
}

/// The rule, in plain words, that a path breaks by the language it is written in, if any (GATE-3): Python, JavaScript and
/// TypeScript source, the package and lock files of the Node tools, and anything below a `node_modules` folder. Whether
/// the allow list lets the path through is `ScanConfig::banned_language_allowed`.
fn banned_language_rule(path: &str) -> Option<&'static str> {
    let normal = path.replace('\\', "/");
    if normal.split('/').any(|segment| segment.trim_end_matches(['.', ' ']).eq_ignore_ascii_case("node_modules")) {
        return Some("a node_modules folder holds JavaScript packages");
    }
    let name = file_name_key(path);
    if NODE_PACKAGE_FILES.contains(&name.as_str()) {
        return Some("a Node package file belongs to JavaScript");
    }
    let (_, extension) = name.rsplit_once('.')?;
    if PYTHON_EXTENSIONS.contains(&extension) {
        Some("a Python source file")
    } else if JAVASCRIPT_EXTENSIONS.contains(&extension) {
        Some("a JavaScript source file")
    } else if TYPESCRIPT_EXTENSIONS.contains(&extension) {
        Some("a TypeScript source file")
    } else {
        None
    }
}

/// What a committed path breaks by its name (GATE-3): a secret file, never admitted; a file of a language the project does
/// not use, admitted only by an exact allow-list entry with a reason. A branch or a tag name is no path: this is not part
/// of `scan_name`.
fn scan_path_rules(cfg: &ScanConfig, path: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if let Some(rule) = secret_name_rule(path) {
        out.push(Finding::SecretFile { path: path.to_string(), rule });
    }
    if let Some(rule) = banned_language_rule(path).filter(|_| !cfg.banned_language_allowed(path)) {
        out.push(Finding::BannedLanguage { path: path.to_string(), rule });
    }
    out
}

/// The path rules over file names, for the files at the tip of a range (GATE-3).
pub fn scan_paths(cfg: &ScanConfig, paths: &[String]) -> Vec<Finding> {
    paths.iter().flat_map(|p| scan_path_rules(cfg, p)).collect()
}

/// The text after the first `#` that follows a blank is a comment (a `#` right after the `=` is part of the value).
fn without_inline_comment(value: &str) -> &str {
    let mut blank_before = false;
    for (at, c) in value.char_indices() {
        if c == '#' && blank_before {
            return &value[..at];
        }
        blank_before = c.is_whitespace();
    }
    value
}

/// Whether one line of an `.env.example` lists a name and no value: blank, a comment, `NAME`, or `NAME=` followed by
/// nothing, a pair of quotes or a comment; `export` in front is fine. Anything else, a value or text that is no
/// assignment at all, is not.
fn env_example_line_lists_a_name_only(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return true;
    }
    let line = line.strip_prefix("export ").map_or(line, str::trim_start);
    let name_len = line.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(*c, '_' | '.' | '-')).count();
    if name_len == 0 {
        return false;
    }
    // The name holds ASCII characters only, so the count of characters is the count of bytes.
    let rest = line[name_len..].trim();
    match rest.strip_prefix('=') {
        None => rest.is_empty(),
        Some(value) => {
            let value = without_inline_comment(value).trim();
            value.is_empty() || value == "\"\"" || value == "''"
        }
    }
}

/// Whether an `.env.example` holds a value or anything but names, blank lines and comments (GATE-3). A text with zero
/// bytes (a binary file, UTF-16 read byte by byte) holds one: no line of it lists a name.
fn env_example_has_values(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    !text.lines().all(env_example_line_lists_a_name_only)
}

/// Whether the text after the `=` of a line is a literal value: not empty, not a pair of quotes, and not only a reference
/// to an environment variable (`${NAME}`), which a token file may hold in place of the token.
fn holds_a_literal_value(value: &str) -> bool {
    let value = value.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
        .trim();
    if unquoted.is_empty() {
        return false;
    }
    let reference = unquoted
        .strip_prefix("${")
        .and_then(|v| v.strip_suffix('}'))
        .is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    !reference
}

/// Whether an `.npmrc` sets an auth token, auth or password to a literal value (`//host/:_authToken=...`, `_auth=...`,
/// `//host/:_password=...`). A line that refers to an environment variable (`${NPM_TOKEN}`) or sets no value is fine.
fn npmrc_has_token(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            return false;
        }
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        let key = key.trim().to_ascii_lowercase();
        (key.ends_with("_authtoken") || key.ends_with("_auth") || key.ends_with("_password")) && holds_a_literal_value(value)
    })
}

/// Whether a `.pypirc` sets a `password` or a `token` to a literal value (`password = ...` or `password: ...`).
fn pypirc_has_secret(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            return false;
        }
        let Some(at) = line.find(['=', ':']) else {
            return false;
        };
        let key = line[..at].trim().to_ascii_lowercase();
        matches!(key.as_str(), "password" | "token") && holds_a_literal_value(&line[at + 1..])
    })
}

/// The rule, in plain words, that a file breaks by its content when its name is allowed only without it (GATE-3): an
/// `.env.example` with a value, an `.npmrc` with an auth token, a `.pypirc` with a password. `text` is the whole file at
/// the end state, or the added lines of a patch.
fn content_rule(path: &str, text: &str) -> Option<&'static str> {
    match file_name_key(path).as_str() {
        ENV_EXAMPLE if env_example_has_values(text) => Some("an .env.example may list names only, and this one holds a value"),
        ".npmrc" if npmrc_has_token(text) => Some("an .npmrc holds an auth token"),
        ".pypirc" if pypirc_has_secret(text) => Some("a .pypirc holds a password or token"),
        _ => None,
    }
}

fn name_of(table: Option<&toml::Value>) -> Option<&str> {
    table?.get("name")?.as_str()
}

fn publish_blocked(package: &toml::Value, root_sets_publish_false: bool) -> bool {
    match package.get("publish") {
        Some(toml::Value::Boolean(false)) => true,
        Some(toml::Value::Array(registries)) => registries.is_empty(),
        Some(other) => other.get("workspace").and_then(|w| w.as_bool()) == Some(true) && root_sets_publish_false,
        None => false,
    }
}

/// Whether a root manifest sets `publish = false` in `[workspace.package]`.
fn workspace_root_blocks_publish(root_toml: &str) -> bool {
    toml::from_str::<toml::Table>(root_toml)
        .ok()
        .and_then(|t| t.get("workspace")?.get("package")?.get("publish")?.as_bool())
        == Some(false)
}

/// A Cargo manifest: `Brand` for a brand word in a `[package]`, `[lib]` or `[[bin]]` name, and `Publishable`
/// unless the manifest sets `publish = false`, or `publish.workspace = true` while the root manifest sets
/// `publish = false` (G22). A manifest without a `[package]` table (a virtual workspace root) is a workspace
/// only: it is `Publishable` only when it has a `[workspace]` table whose `[workspace.package]` lacks
/// `publish = false` (plan Task 3 Step 3).
pub fn scan_manifest(_cfg: &ScanConfig, path: &str, toml_text: &str, root_sets_publish_false: bool) -> Vec<Finding> {
    let Ok(doc) = toml::from_str::<toml::Table>(toml_text) else {
        return vec![Finding::Publishable { manifest: path.to_string() }];
    };
    let mut out = Vec::new();
    let package = doc.get("package");
    let mut names: Vec<&str> = Vec::new();
    names.extend(name_of(package));
    names.extend(name_of(doc.get("lib")));
    if let Some(bins) = doc.get("bin").and_then(|b| b.as_array()) {
        for bin in bins {
            names.extend(bin.get("name").and_then(|n| n.as_str()));
        }
    }
    if names.iter().any(|n| brand_word_in(n)) {
        out.push(Finding::Brand { at: format!("{path} (Cargo name)") });
    }
    if let Some(package) = package {
        if !publish_blocked(package, root_sets_publish_false) {
            out.push(Finding::Publishable { manifest: path.to_string() });
        }
    } else if doc.contains_key("workspace") && !workspace_root_blocks_publish(toml_text) {
        out.push(Finding::Publishable { manifest: path.to_string() });
    }
    out
}

// ---------------------------------------------------------------------------------------------------------
// Commits
// ---------------------------------------------------------------------------------------------------------

fn is_trailer_syntax(line: &str) -> bool {
    let Some((token, rest)) = line.split_once(':') else {
        return false;
    };
    token.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && (rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t'))
}

fn known_trailer(line: &str) -> bool {
    line.split_once(':')
        .is_some_and(|(token, _)| KNOWN_TRAILERS.contains(&token.trim().to_ascii_lowercase().as_str()))
}

/// Trailer lines of a message: every `Token: value` line of the last paragraph (when that is not the subject
/// paragraph) and every line anywhere after the subject that starts with a known trailer name.
fn trailer_lines(message: &str) -> Vec<&str> {
    let lines: Vec<&str> = message.lines().collect();
    let Some(end) = lines.iter().rposition(|l| !l.trim().is_empty()) else {
        return Vec::new();
    };
    let mut start = end;
    while start > 0 && !lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let line = line.trim();
        let in_last_paragraph = start > 0 && i >= start && i <= end;
        if (in_last_paragraph && is_trailer_syntax(line)) || known_trailer(line) {
            out.push(line);
        }
    }
    out
}

/// `Signed-off-by: <configured name> <<configured no-reply address>>`.
fn is_valid_sign_off(cfg: &ScanConfig, line: &str) -> bool {
    let Some((token, rest)) = line.split_once(':') else {
        return false;
    };
    if !token.trim().eq_ignore_ascii_case("signed-off-by") {
        return false;
    }
    let Some((name, address)) = rest.trim().rsplit_once('<') else {
        return false;
    };
    let Some(address) = address.trim_end().strip_suffix('>') else {
        return false;
    };
    name.trim() == cfg.name && cfg.is_noreply(address.trim())
}

fn scan_message(cfg: &ScanConfig, message: &str, short: &str) -> Vec<Finding> {
    let at = format!("{short}:message");
    let mut out = scan_text(cfg, message, &at);
    let mut valid = 0usize;
    let mut other = 0usize;
    for line in trailer_lines(message) {
        if is_valid_sign_off(cfg, line) {
            valid += 1;
        } else {
            other += 1;
        }
    }
    if valid == 0 {
        out.push(Finding::MissingSignOff { commit: short.to_string() });
    }
    for _ in 0..(valid.saturating_sub(1) + other) {
        out.push(Finding::Trailer { at: at.clone() });
    }
    out
}

fn flush_added(cfg: &ScanConfig, short: &str, path: Option<&str>, added: &mut String, out: &mut Vec<Finding>) {
    if added.is_empty() {
        return;
    }
    let at = format!("{short}:{}", path.unwrap_or("diff"));
    out.extend(scan_text(cfg, added, &at));
    // A file whose name is allowed only without a value or a token (GATE-3): found in what a commit adds, so a value
    // that a later commit of the range takes out again is still found.
    if let Some(rule) = path.and_then(|p| content_rule(p, added)) {
        out.push(Finding::SecretContent { rule, at });
    }
    added.clear();
}

/// Scans the added lines of a patch, file by file. Removed and context lines are not scanned: a commit that
/// removes a leak must itself be committable (the commit that added it is flagged by its own scan).
fn scan_diff(cfg: &ScanConfig, diff: &str, short: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    let mut added = String::new();
    let mut in_header = false;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush_added(cfg, short, path.as_deref(), &mut added, &mut out);
            in_header = true;
            path = rest.rsplit_once(" b/").map(|(_, p)| p.to_string());
            continue;
        }
        if in_header {
            if let Some(rest) = line.strip_prefix("+++ ") {
                path = Some(rest.strip_prefix("b/").unwrap_or(rest).to_string());
            } else if line.starts_with("@@") {
                in_header = false;
            }
            continue;
        }
        if let Some(text) = line.strip_prefix('+') {
            added.push_str(text);
            added.push('\n');
        }
    }
    flush_added(cfg, short, path.as_deref(), &mut added, &mut out);
    out
}

fn identity_ok(cfg: &ScanConfig, name: &str, email: &str) -> bool {
    name == cfg.name && cfg.is_noreply(email)
}

/// What a commit changes: the added lines of its patch, and every changed path against the allow list and the name rules.
fn scan_changes(cfg: &ScanConfig, c: &CommitData, short: &str) -> Vec<Finding> {
    let mut out = scan_diff(cfg, &c.diff, short);
    for path in &c.paths {
        if !cfg.path_allowed(path) {
            out.push(Finding::Path { path: path.clone() });
        }
        out.extend(scan_name(cfg, path));
        out.extend(scan_path_rules(cfg, path));
    }
    out
}

fn scan_commit_inner(cfg: &ScanConfig, c: &CommitData, with_message: bool) -> Vec<Finding> {
    let short = short_id(&c.id);
    let mut out = Vec::new();
    if !identity_ok(cfg, &c.author_name, &c.author_email) || !identity_ok(cfg, &c.committer_name, &c.committer_email) {
        out.push(Finding::Identity { commit: short.clone() });
    }
    if with_message {
        out.extend(scan_message(cfg, &c.message, &short));
    }
    out.extend(scan_changes(cfg, c, &short));
    out
}

/// One commit: author and committer identity, the message (terms, addresses, profile paths, exactly one
/// sign-off and no other trailer), the added lines of the patch, and every changed path against the allow list.
pub fn scan_commit(cfg: &ScanConfig, c: &CommitData) -> Vec<Finding> {
    scan_commit_inner(cfg, c, true)
}

/// GitHub's committer name on the merge commits of its merge button.
const GITHUB_COMMITTER_NAME: &str = "GitHub";

/// GitHub's committer address, put together at run time: no address but the project's own stands in this file, which
/// the scan reads like any other.
fn github_committer_email() -> String {
    ["noreply", "github.com"].join("@")
}

/// The first line of the merge commit of a pull request: `Merge pull request #<number> from <owner>/<branch>`, and
/// nothing else on that line. What follows the first line (the title of the pull request) is not looked at here.
fn github_merge_subject_ok(message: &str) -> bool {
    let Some(subject) = message.lines().next() else {
        return false;
    };
    let Some(rest) = subject.strip_prefix("Merge pull request #") else {
        return false;
    };
    let Some((number, source)) = rest.split_once(" from ") else {
        return false;
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let Some((owner, branch)) = source.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !branch.is_empty()
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        && !branch.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether a commit is the merge commit of GitHub's merge button in its exact shape (G1): GitHub as the committer, the
/// subject of a pull-request merge, two parents, and a result that holds nothing the parents do not. The last is read
/// from the trees: no path may differ from both parents, so every path of the merge is, as a whole, the first parent's
/// or the second's, and nothing was edited inside the merge (an "evil merge"). A merge that mixes both sides inside one
/// file does not pass: it is an ordinary commit then, and it stops at the committer rule; such a merge is made locally
/// and pushed through the gate.
fn is_github_merge(repo: &Repo, c: &CommitData) -> Result<bool, String> {
    if c.committer_name != GITHUB_COMMITTER_NAME
        || c.committer_email != github_committer_email()
        || !github_merge_subject_ok(&c.message)
    {
        return Ok(false);
    }
    let parents = repo.parents(&c.id)?;
    let [first, second] = parents.as_slice() else {
        return Ok(false);
    };
    let from_first = repo.differing_paths(&c.id, first)?;
    let from_second = repo.differing_paths(&c.id, second)?;
    Ok(from_first.is_disjoint(&from_second))
}

/// A merge commit in the exact shape of GitHub's merge button (`is_github_merge`). Its committer is GitHub, not the
/// project identity, and its message carries no sign-off of its own; the rest of the rules stand: the author is the
/// project identity, the message is scanned like any other, a trailer is a finding (the one sign-off aside), and
/// whatever the merge changes against its first parent is scanned like the changes of any commit.
fn scan_github_merge(cfg: &ScanConfig, c: &CommitData) -> Vec<Finding> {
    let short = short_id(&c.id);
    let at = format!("{short}:message");
    let mut out = Vec::new();
    if !identity_ok(cfg, &c.author_name, &c.author_email) {
        out.push(Finding::Identity { commit: short.clone() });
    }
    out.extend(scan_text(cfg, &c.message, &at));
    let (mut valid, mut other) = (0usize, 0usize);
    for line in c.message.lines().skip(1).map(str::trim).filter(|l| known_trailer(l)) {
        if is_valid_sign_off(cfg, line) {
            valid += 1;
        } else {
            other += 1;
        }
    }
    for _ in 0..(valid.saturating_sub(1) + other) {
        out.push(Finding::Trailer { at: at.clone() });
    }
    out.extend(scan_changes(cfg, c, &short));
    out
}

/// The end state of one file: text checks, the content rule of a file that may not hold a value or a token (GATE-3), plus
/// the manifest checks for a `Cargo.toml`.
fn scan_end_state(cfg: &ScanConfig, path: &str, text: &str, root_blocks_publish: bool) -> Vec<Finding> {
    let mut out = scan_text(cfg, text, path);
    if let Some(rule) = content_rule(path, text) {
        out.push(Finding::SecretContent { rule, at: path.to_string() });
    }
    if last_segment(path) == "Cargo.toml" {
        out.extend(scan_manifest(cfg, path, text, root_blocks_publish));
    }
    out
}

/// What the scan makes of the bytes of one file.
enum Content {
    /// Text: UTF-8 (invalid bytes replaced) without a zero byte, or UTF-16 that decoded cleanly.
    Text(String),
    /// A zero byte and no clean UTF-16 reading: a binary file.
    Binary,
}

/// UTF-16 text, with a byte-order mark or without one. Without a mark, the text must be mostly Latin letters,
/// so that one byte of nearly every pair is zero and the byte order shows. `None` when the bytes are not clean
/// UTF-16 text (an odd length, a lone surrogate, a control character other than tab, line breaks and form feed).
fn decode_utf16(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        return None;
    }
    let (little_endian, body) = match (bytes[0], bytes[1]) {
        (0xFF, 0xFE) => (true, &bytes[2..]),
        (0xFE, 0xFF) => (false, &bytes[2..]),
        _ => {
            let pairs = bytes.len() / 2;
            let high_zero = bytes.chunks_exact(2).filter(|p| p[1] == 0).count();
            let low_zero = bytes.chunks_exact(2).filter(|p| p[0] == 0).count();
            if high_zero * 2 >= pairs && high_zero > low_zero * 3 {
                (true, bytes)
            } else if low_zero * 2 >= pairs && low_zero > high_zero * 3 {
                (false, bytes)
            } else {
                return None;
            }
        }
    };
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|p| if little_endian { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) })
        .collect();
    let text = String::from_utf16(&units).ok()?;
    if text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{c}')) {
        return None;
    }
    Some(text)
}

/// Git's own binary test is a zero byte. A file with one is text only when it is clean UTF-16.
fn classify(bytes: &[u8]) -> Content {
    if !bytes.contains(&0) {
        return Content::Text(String::from_utf8_lossy(bytes).into_owned());
    }
    match decode_utf16(bytes) {
        Some(text) => Content::Text(text),
        None => Content::Binary,
    }
}

/// One file of the end state, from its bytes (I1). Text goes through `scan_end_state`. A binary file is still
/// searched for readable runs, and it passes only when the allow list names its exact path: a PNG renamed to
/// `.dat` is no media file by its extension, and `docs/**` must not admit it unseen.
fn scan_file(cfg: &ScanConfig, path: &str, bytes: &[u8], root_blocks_publish: bool) -> Vec<Finding> {
    match classify(bytes) {
        Content::Text(text) => scan_end_state(cfg, path, &text, root_blocks_publish),
        Content::Binary => {
            let text = String::from_utf8_lossy(bytes);
            let mut out = scan_text(cfg, &text, path);
            if let Some(rule) = content_rule(path, &text) {
                out.push(Finding::SecretContent { rule, at: path.to_string() });
            }
            if !cfg.path_listed_exactly(path) {
                out.push(Finding::Binary { path: path.to_string() });
            }
            out
        }
    }
}

/// `Ok` only when the executable's SHA-256 equals the pinned value (checked before every gitleaks run).
pub fn check_scanner(exe: &[u8], pinned_sha256: &str) -> Result<(), Finding> {
    let pinned = pinned_sha256.trim();
    if is_hex64(pinned) && sha256_hex(exe).eq_ignore_ascii_case(pinned) {
        Ok(())
    } else {
        Err(Finding::ScannerChanged)
    }
}

// ---------------------------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------------------------

/// The scan settings folder must be set, exist and lie outside the repository.
fn check_config_dir(dir: Option<&Path>, repo: &Path) -> Result<PathBuf, String> {
    let dir = dir.ok_or("the scan settings folder is not set (SKIPPER_SCAN_CONFIG or --config)")?;
    if !dir.is_dir() {
        return Err("the scan settings folder does not exist".to_string());
    }
    let canonical = dir.canonicalize().map_err(|_| "the scan settings folder cannot be resolved".to_string())?;
    let repo_canonical = repo.canonicalize().map_err(|_| "the repository folder cannot be resolved".to_string())?;
    if canonical.starts_with(&repo_canonical) {
        return Err("the scan settings folder is inside the repository".to_string());
    }
    Ok(canonical)
}

/// gitleaks loads `.gitleaks.toml` and `.gitleaksignore` from the working tree on its own, and the box can
/// write both: the scan refuses to run when either exists (PR-2).
fn check_no_planted_config(repo: &Path) -> Result<(), String> {
    for name in [".gitleaks.toml", ".gitleaksignore"] {
        if repo.join(name).symlink_metadata().is_ok() {
            return Err(format!("{name} exists in the working tree; remove it, the scan refuses planted scanner settings"));
        }
    }
    if std::env::var_os("GITLEAKS_CONFIG").is_some() || std::env::var_os("GITLEAKS_CONFIG_TOML").is_some() {
        return Err("a gitleaks configuration is set in the environment".to_string());
    }
    Ok(())
}

/// The committed text when `HEAD` holds the file; the working-tree text only while no commit holds it.
fn prefer_committed(head: Option<String>, working: Option<String>) -> Option<String> {
    head.or(working)
}

/// `[tool.gitleaks] exe_sha256` of `pins.toml`; `None` when missing or empty.
fn pinned_exe_sha256(pins_text: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(pins_text).ok()?;
    let value = table.get("tool")?.get("gitleaks")?.get("exe_sha256")?.as_str()?.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// A range given to `--range`: no option look-alikes, no whitespace or control characters.
fn validate_range(spec: &str) -> Result<(), String> {
    if spec.is_empty() || spec.len() > 200 || spec.starts_with('-') || spec.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("the --range value is not a plain revision or revision range".to_string());
    }
    Ok(())
}

/// The newest revision of a range: `a..b` and `a...b` give `b`; `b` gives `b`; `a..` gives `HEAD`.
fn range_tip(spec: &str) -> String {
    let tip = match spec.rfind("..") {
        Some(i) => &spec[i + 2..],
        None => spec,
    };
    if tip.is_empty() { "HEAD".to_string() } else { tip.to_string() }
}

// ---------------------------------------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------------------------------------

/// Replaces every banned term in `s` by `<term N>`, so no output line ever shows a term's text.
fn mask_terms(cfg: &ScanConfig, s: &str) -> String {
    let hidden = || "<name hidden>".to_string();
    let lower = s.to_lowercase();
    if lower.len() != s.len() {
        return if cfg.matched_terms(s).is_empty() { s.to_string() } else { hidden() };
    }
    let mut hits: Vec<(usize, usize, usize)> = Vec::new();
    for (n, term) in cfg.terms.iter().enumerate() {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&term.text) {
            let start = from + pos;
            let end = start + term.text.len();
            hits.push((start, end, n));
            from = end;
        }
    }
    hits.sort();
    let mut out = String::new();
    let mut cursor = 0;
    for (start, end, n) in hits {
        if start < cursor {
            continue;
        }
        let (Some(before), true) = (s.get(cursor..start), s.is_char_boundary(end)) else {
            return hidden();
        };
        out.push_str(before);
        out.push_str(&format!("<term {}>", n + 1));
        cursor = end;
    }
    match s.get(cursor..) {
        Some(rest) => out.push_str(rest),
        None => return hidden(),
    }
    out
}

fn render(cfg: &ScanConfig, finding: &Finding) -> String {
    let m = |s: &str| mask_terms(cfg, s);
    match finding {
        Finding::Term { n, at } => format!("banned term #{} found at {}", n + 1, m(at)),
        Finding::Email { at } => format!("e-mail address other than the project no-reply address at {}", m(at)),
        Finding::Trailer { at } => format!("commit trailer other than the single sign-off at {}", m(at)),
        Finding::MissingSignOff { commit } => format!("commit {} has no valid sign-off", m(commit)),
        Finding::Identity { commit } => format!("author or committer of {} is not the project identity", m(commit)),
        Finding::Path { path } => format!("path not on the allow list: {}", m(path)),
        Finding::ProfilePath { at } => format!("expanded user-profile path at {}", m(at)),
        Finding::Secret { rule, at } => format!("secret (rule {}) at {}", m(rule), m(at)),
        Finding::Brand { at } => format!("brand word in a name at {}", m(at)),
        Finding::Publishable { manifest } => format!("manifest without publish = false: {}", m(manifest)),
        Finding::ScannerChanged => "the gitleaks executable differs from the pinned SHA-256".to_string(),
        Finding::JudgeChange { rules } => format!(
            "range changes judge code or judge input ({rules}); a reviewer who did not write the change must read its diff, then --accept-judge-change <range tip> lets it pass (B1)"
        ),
        Finding::Binary { path } => format!("binary file without an exact allow-list entry: {}", m(path)),
        Finding::Unreadable { path } => format!("file could not be read: {}", m(path)),
        Finding::ImportedPage { at } => {
            format!("imported page (its header has a source line); imported content never goes into a push: {}", m(at))
        }
        Finding::ImportedFile { path } => {
            format!("imported file (below knowledge/.files); imported content never goes into a push: {}", m(path))
        }
        Finding::SecretFile { path, rule } => format!(
            "secret file refused, {rule}; a file like this never goes into a push and no allow-list entry admits it: {}",
            m(path)
        ),
        Finding::SecretContent { rule, at } => {
            format!("secret in a file, {rule}; take the value out before the push: {}", m(at))
        }
        Finding::BannedLanguage { path, rule } => format!(
            "file of a language the project does not use, {rule}; it passes only with its exact path in scan/allowed-paths.txt and a comment line with the reason right above that entry: {}",
            m(path)
        ),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Mode {
    Staged,
    Range(String),
}

#[derive(Debug, PartialEq)]
struct Options {
    mode: Mode,
    config: Option<PathBuf>,
    allow: Option<PathBuf>,
    rules_from: Option<String>,
    accept_judge_change: Option<String>,
    repo: PathBuf,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut mode: Option<Mode> = None;
    let mut config = None;
    let mut allow = None;
    let mut rules_from = None;
    let mut accept_judge_change = None;
    let mut repo = PathBuf::from(".");
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--staged" => {
                if mode.is_some() {
                    return Err("use exactly one of --staged and --range".to_string());
                }
                mode = Some(Mode::Staged);
            }
            "--range" => {
                if mode.is_some() {
                    return Err("use exactly one of --staged and --range".to_string());
                }
                let value = it.next().ok_or("--range needs a value")?;
                mode = Some(Mode::Range(value.clone()));
            }
            "--config" => config = Some(PathBuf::from(it.next().ok_or("--config needs a folder")?)),
            "--allow" => allow = Some(PathBuf::from(it.next().ok_or("--allow needs a file")?)),
            "--rules-from" => {
                let value = it.next().ok_or("--rules-from needs a commit id")?;
                if !is_full_commit_id(value) {
                    return Err("--rules-from needs a full commit id (40 or 64 hex digits)".to_string());
                }
                rules_from = Some(value.to_ascii_lowercase());
            }
            "--accept-judge-change" => {
                let value = it.next().ok_or("--accept-judge-change needs a commit id")?;
                if !is_full_commit_id(value) {
                    return Err("--accept-judge-change needs a full commit id (40 or 64 hex digits)".to_string());
                }
                accept_judge_change = Some(value.to_ascii_lowercase());
            }
            "--repo" => repo = PathBuf::from(it.next().ok_or("--repo needs a folder")?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let mode = mode.ok_or("give --staged or --range <a>..<b>")?;
    if allow.is_some() && rules_from.is_some() {
        return Err("use either --allow or --rules-from, not both".to_string());
    }
    if accept_judge_change.is_some() && mode == Mode::Staged {
        return Err("--accept-judge-change goes with --range: it names the tip of a range".to_string());
    }
    Ok(Options { mode, config, allow, rules_from, accept_judge_change, repo })
}

// ---------------------------------------------------------------------------------------------------------
// Judges (B1)
// ---------------------------------------------------------------------------------------------------------

/// The folders at the root of the repository whose files are judge code or judge input: the scanner and the other
/// tasks of the build (`xtask`, `tools`), what they read their rules from (`scan`, `supply-chain`), and the build's
/// own configuration (`.cargo`). The rule is the name that a finding shows.
const JUDGE_FOLDERS: [(&str, &str); 5] =
    [("xtask", "xtask/**"), ("tools", "tools/**"), ("scan", "scan/**"), ("supply-chain", "supply-chain/**"), (".cargo", ".cargo/**")];

/// Which judge rule a committed path falls under, if any (B1): a file of a judge folder; `rust-toolchain.toml` and
/// `Cargo.lock` at the root; a `Cargo.toml` at any depth. A path is read the way a Windows checkout reads it: letters
/// in any case, `\` as a separator, and the dots and spaces at the end of a name dropped.
fn judge_rule_of(path: &str) -> Option<&'static str> {
    let normal = path.replace('\\', "/");
    let segments: Vec<String> = normal.split('/').map(|s| s.trim_end_matches(['.', ' ']).to_ascii_lowercase()).collect();
    let (first, last) = (segments.first()?, segments.last()?);
    if let Some((_, rule)) = JUDGE_FOLDERS.iter().copied().find(|(folder, _)| first == folder) {
        return Some(rule);
    }
    if segments.len() == 1 && first == "rust-toolchain.toml" {
        return Some("rust-toolchain.toml");
    }
    if segments.len() == 1 && first == "cargo.lock" {
        return Some("Cargo.lock");
    }
    if last == "cargo.toml" {
        return Some("Cargo.toml");
    }
    None
}

/// The judge rules that the changed paths touch, each once, in the order of their names.
fn judge_rules_changed(paths: &[String]) -> Vec<&'static str> {
    let found: BTreeSet<&'static str> = paths.iter().filter_map(|p| judge_rule_of(p)).collect();
    found.into_iter().collect()
}

/// A change to a judge goes through only when the person names the tip of the range (B1): the leak scan, `pin-check`
/// and `deps-check` judge a push, so a push that edits them would otherwise approve its own edit. Every commit of the
/// range counts, not the end state: a change that is undone inside the range was still pushed, and a deleted judge file
/// is a change. A merge counts with its changes against each parent. Returns the findings and, when the acceptance
/// let the change pass, a note for the log.
fn judge_change(repo: &Repo, spec: &str, accepted: Option<&str>) -> Result<(Vec<Finding>, Option<String>), String> {
    validate_range(spec)?;
    let names = repo.git(["log", "--no-use-mailmap", "--no-renames", "-m", "-z", "--name-only", "--format=", spec])?;
    let rules = judge_rules_changed(&split_nul(&names));
    if rules.is_empty() {
        return Ok((Vec::new(), None));
    }
    let rules = rules.join(", ");
    let tip = repo.resolve_tip(&range_tip(spec))?;
    if accepted.is_some_and(|id| id.eq_ignore_ascii_case(&tip)) {
        let note = format!("the range changes judge code or judge input ({rules}); accepted for the range tip {}", short_id(&tip));
        return Ok((Vec::new(), Some(note)));
    }
    Ok((vec![Finding::JudgeChange { rules }], None))
}

fn is_full_commit_id(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The allow list and `pins.toml` as text. With `--rules-from <commit>` both come from that commit and from nothing
/// else (H4): a push judges the commits it sends, so their own allow list and pin must not decide, and a missing
/// file never falls back to `HEAD` or the working tree. A range scan needs it, and an `--allow` file does not stand in
/// for it (the pin would still come from `HEAD`): without it the scan refuses, so a launcher that does not pass the flag
/// cannot make the scan judge a push by the rules of the commits it sends (M7 and N2 of the reviews of the push gate).
/// A scan of the staged changes keeps the old order: `--allow`, else `HEAD`, else the working tree.
fn rule_texts(repo: &Repo, opts: &Options) -> Result<(String, String), String> {
    if matches!(opts.mode, Mode::Range(_)) && opts.rules_from.is_none() {
        return Err("--range needs --rules-from <commit>: a range is never judged by its own rules".to_string());
    }
    if let Some(commit) = &opts.rules_from {
        if repo.resolve_tip(commit)? != *commit {
            return Err("--rules-from does not name a commit".to_string());
        }
        let allow = repo
            .blob(&format!("{commit}:{ALLOW_PATH}"))
            .ok_or("scan/allowed-paths.txt is not in the rules commit")?;
        let pins = repo.blob(&format!("{commit}:{PINS_PATH}")).ok_or("supply-chain/pins.toml is not in the rules commit")?;
        return Ok((allow, pins));
    }
    let allow_text = match &opts.allow {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("cannot read the allow list: {e}"))?,
        None => repo
            .read_preferring_committed(ALLOW_PATH)
            .ok_or("scan/allowed-paths.txt is in neither HEAD nor the working tree")?,
    };
    let pins_text = repo.read_preferring_committed(PINS_PATH).ok_or("supply-chain/pins.toml was not found")?;
    Ok((allow_text, pins_text))
}

// ---------------------------------------------------------------------------------------------------------
// git (read only) and gitleaks
// ---------------------------------------------------------------------------------------------------------

/// The environment of every git process of the scan, and of gitleaks, whose own `git log` inherits it: git
/// reads its attribute rules from the empty tree instead of the working tree, and skips the system-wide
/// attributes file. So a `* -diff` or `binary` line in any `.gitattributes`, tracked or not, cannot turn a patch
/// into "Binary files differ". Git 2.40 is the minimum because it is the first release
/// with `GIT_ATTR_SOURCE`: an older git ignores the variable silently, and no config setting takes the
/// working-tree `.gitattributes` out of force. The own diffs add `--text` as a second guard; it also covers
/// `.git/info/attributes`, which the variable does not reach. Replacement objects are off as well (M9 of the audit of
/// 2026-10-23): a ref under `refs/replace/` makes `git log` and `git show` print another commit than the one the push
/// sends, so a secret could hide behind a clean look-alike.
fn scan_git_env() -> [(&'static str, &'static str); 3] {
    [("GIT_ATTR_SOURCE", EMPTY_TREE), ("GIT_ATTR_NOSYSTEM", "1"), ("GIT_NO_REPLACE_OBJECTS", "1")]
}

/// `(major, minor)` of the text `git --version` prints, e.g. `git version 2.55.0.windows.3`.
fn parse_git_version(text: &str) -> Option<(u32, u32)> {
    let version = text.split_whitespace().nth(2)?;
    let mut parts = version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// The bytes without carriage returns, to compare a working-tree file with the index across line-ending settings.
fn without_cr(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|b| *b != b'\r').collect()
}

struct TagInfo {
    name: String,
    /// The message of an annotated tag.
    message: Option<String>,
}

struct Repo {
    dir: PathBuf,
}

fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .map(|s| String::from_utf8_lossy(s).trim_start_matches('\n').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

impl Repo {
    fn git<I, S>(&self, args: I) -> Result<Vec<u8>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .envs(scan_git_env())
            .output()
            .map_err(|e| format!("cannot start git: {e}"))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(format!("git exited with code {}", output.status.code().unwrap_or(-1)))
        }
    }

    fn git_text<I, S>(&self, args: I) -> Result<String, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.git(args).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A tracked file's text from `HEAD`, else from the working tree.
    fn read_preferring_committed(&self, rel: &str) -> Option<String> {
        let spec = format!("HEAD:{rel}");
        let head = self.git_text(["show", spec.as_str()]).ok();
        let working = std::fs::read_to_string(self.dir.join(rel)).ok();
        prefer_committed(head, working)
    }

    /// Refuses a repository that holds replacement refs or a grafts file (M9). The git processes of the scan switch
    /// replacement objects off anyway (`scan_git_env`); the refusal makes the attempt visible instead of silent. The
    /// message names no ref, because a ref name may hold a banned term.
    fn check_no_replacements(&self) -> Result<(), String> {
        let refs = self.git_text(["for-each-ref", "refs/replace/"])?;
        if !refs.trim().is_empty() {
            return Err("refs/replace/ holds replacement refs; remove them (git replace -d), the scan refuses a history that git would swap".to_string());
        }
        let grafts = self.git_text(["rev-parse", "--git-path", "info/grafts"])?;
        if self.dir.join(grafts.trim()).symlink_metadata().is_ok() {
            return Err("info/grafts exists; remove it, the scan refuses a history that git would swap".to_string());
        }
        Ok(())
    }

    /// A blob by object name, as text (invalid UTF-8 is replaced, never skipped).
    fn blob(&self, object: &str) -> Option<String> {
        self.git(["cat-file", "blob", object]).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// A blob by object name, as raw bytes; an error when git cannot read it.
    fn blob_bytes(&self, object: &str) -> Result<Vec<u8>, String> {
        self.git(["cat-file", "blob", object])
    }

    /// Refuses to scan while git could read attribute rules that the scan did not choose.
    /// Every git process of the scan reads its rules from the empty tree, but git before 2.40 ignores
    /// that without a word, `.git/info/attributes` is not covered by it, and a `.gitattributes` that is
    /// untracked or differs from the index is a rules file that nobody has scanned. A committed or staged one
    /// is checked like any path: only the exact entry of the allow list admits it. The messages name no path,
    /// because a path may hold a banned term.
    fn check_attribute_rules(&self) -> Result<(), String> {
        let version = Command::new("git")
            .arg("--version")
            .output()
            .map_err(|e| format!("cannot start git: {e}"))?;
        match parse_git_version(&String::from_utf8_lossy(&version.stdout)) {
            Some(found) if found >= MIN_GIT_VERSION => {}
            _ => {
                return Err("git 2.40 or newer is needed: an older git ignores GIT_ATTR_SOURCE, so attribute rules could hide content from the scan".to_string());
            }
        }
        let info = self.git_text(["rev-parse", "--git-path", "info/attributes"])?;
        match std::fs::read(self.dir.join(info.trim())) {
            Ok(rules) if rules.iter().any(|b| !b.is_ascii_whitespace()) => {
                return Err(".git/info/attributes holds rules; remove them, the scan refuses attribute rules it did not choose".to_string());
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot read .git/info/attributes: {e}")),
        }
        let untracked = split_nul(&self.git(["ls-files", "--others", "-z"])?);
        if untracked.iter().any(|path| is_attributes_file(path)) {
            return Err("an untracked .gitattributes exists in the working tree; remove it or add it to the commit".to_string());
        }
        for path in split_nul(&self.git(["ls-files", "--cached", "-z"])?) {
            if !is_attributes_file(&path) {
                continue;
            }
            let spec = format!(":0:{path}");
            let indexed = self.git(["cat-file", "blob", spec.as_str()])?;
            match std::fs::read(self.dir.join(&path)) {
                Ok(working) => {
                    if without_cr(&working) != without_cr(&indexed) {
                        return Err("a .gitattributes in the working tree differs from the index; stage or restore it".to_string());
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("cannot read a .gitattributes of the working tree: {e}")),
            }
        }
        Ok(())
    }

    fn rev_ids(&self, spec: &str) -> Result<Vec<String>, String> {
        let text = self.git_text(["log", "--no-use-mailmap", "--format=%H", spec])?;
        Ok(text.lines().map(|l| l.trim().to_ascii_lowercase()).filter(|l| !l.is_empty()).collect())
    }

    /// The parents of a commit, first parent first.
    fn parents(&self, id: &str) -> Result<Vec<String>, String> {
        let text = self.git_text(["log", "-1", "--no-use-mailmap", "--format=%P", id])?;
        Ok(text.split_whitespace().map(str::to_ascii_lowercase).collect())
    }

    /// The paths where the trees of two commits differ (any change, also of mode, type or a submodule's commit).
    fn differing_paths(&self, a: &str, b: &str) -> Result<BTreeSet<String>, String> {
        let names = self.git(["diff-tree", "-r", "--no-renames", "--name-only", "-z", a, b])?;
        Ok(split_nul(&names).into_iter().collect())
    }

    fn commit_data(&self, id: &str) -> Result<CommitData, String> {
        let meta = self.git_text(["log", "-1", "--no-use-mailmap", "--format=%an%x00%ae%x00%cn%x00%ce%x00%B", id])?;
        let mut fields = meta.splitn(5, '\0');
        let mut next = || fields.next().unwrap_or("").to_string();
        let author_name = next();
        let author_email = next();
        let committer_name = next();
        let committer_email = next();
        let message = next().trim_end().to_string();
        // `core.quotePath=false`: a name with a letter above 0x7F stays as it is in the header lines, so the place of a patch
        // is the real path (`is_markdown_place` also takes a quoted name, which git still writes for a quote or a backslash)
        let diff = self.git_text([
            "-c", "core.quotePath=false", "show", "--no-use-mailmap", "--no-color", "--no-ext-diff", "--no-textconv", "--text",
            "--no-renames", "-U0", "-m", "--first-parent", "--format=", id,
        ])?;
        let names = self.git([
            "show", "--no-use-mailmap", "--no-renames", "-z", "--name-only", "--diff-filter=ACMRT", "-m",
            "--first-parent", "--format=", id,
        ])?;
        Ok(CommitData {
            id: id.to_string(),
            author_name,
            author_email,
            committer_name,
            committer_email,
            message,
            diff,
            paths: split_nul(&names),
        })
    }

    /// The staged changes as commit data (identity from the repository's own git settings, no message yet) and
    /// the staged paths.
    fn staged(&self) -> Result<(CommitData, Vec<String>), String> {
        let diff = self.git_text([
            "-c", "core.quotePath=false", "diff", "--cached", "--no-color", "--no-ext-diff", "--no-textconv", "--text",
            "--no-renames", "-U0",
        ])?;
        let names = self.git(["diff", "--cached", "--name-only", "-z", "--no-renames", "--diff-filter=ACMRT"])?;
        let paths = split_nul(&names);
        let name = self.git_text(["config", "--get", "user.name"]).unwrap_or_default().trim().to_string();
        let email = self.git_text(["config", "--get", "user.email"]).unwrap_or_default().trim().to_string();
        let data = CommitData {
            id: "staged".to_string(),
            author_name: name.clone(),
            author_email: email.clone(),
            committer_name: name,
            committer_email: email,
            message: String::new(),
            diff,
            paths: paths.clone(),
        };
        Ok((data, paths))
    }

    fn resolve_tip(&self, tip: &str) -> Result<String, String> {
        let spec = format!("{tip}^{{commit}}");
        let text = self.git_text(["rev-parse", "--verify", spec.as_str()])?;
        Ok(text.trim().to_string())
    }

    fn tip_files(&self, tip: &str) -> Result<Vec<String>, String> {
        Ok(split_nul(&self.git(["ls-tree", "-r", "-z", "--name-only", tip])?))
    }

    /// Every tag that points at one of `ids`, with its message when it is annotated.
    fn tags_in(&self, ids: &BTreeSet<String>) -> Result<Vec<TagInfo>, String> {
        let raw = self.git_text([
            "for-each-ref",
            "refs/tags",
            "--format=%(refname:short)%00%(objecttype)%00%(*objectname)%00%(objectname)%00%(contents)%01",
        ])?;
        let mut out = Vec::new();
        for record in raw.split('\u{1}') {
            let record = record.trim_start_matches('\n');
            if record.trim().is_empty() {
                continue;
            }
            let mut fields = record.splitn(5, '\0');
            let name = fields.next().unwrap_or("");
            let kind = fields.next().unwrap_or("");
            let peeled = fields.next().unwrap_or("");
            let direct = fields.next().unwrap_or("");
            let contents = fields.next().unwrap_or("");
            let target = if peeled.is_empty() { direct } else { peeled };
            if !ids.contains(&target.to_ascii_lowercase()) {
                continue;
            }
            let message = if kind == "tag" { Some(contents.trim().to_string()) } else { None };
            out.push(TagInfo { name: name.to_string(), message });
        }
        Ok(out)
    }
}

/// Turns the gitleaks report and exit code into findings. A non-zero exit without a readable hit, or a report
/// that is not a JSON list, is a finding too (the scan fails closed). Only rule, file, line and commit are
/// used; the matched text never leaves the report.
fn gitleaks_findings(report: Option<&[u8]>, exit_code: i32) -> Vec<Finding> {
    let mut out = Vec::new();
    if let Some(bytes) = report {
        let trimmed = String::from_utf8_lossy(bytes);
        if !trimmed.trim().is_empty() {
            match serde_json::from_slice::<serde_json::Value>(bytes) {
                Ok(serde_json::Value::Array(items)) => {
                    for item in items {
                        let rule = item.get("RuleID").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        let file = item.get("File").and_then(|v| v.as_str()).unwrap_or("?");
                        let line = item.get("StartLine").and_then(|v| v.as_u64()).unwrap_or(0);
                        let commit = item.get("Commit").and_then(|v| v.as_str()).unwrap_or("");
                        let at = if commit.is_empty() {
                            format!("{file}:{line}")
                        } else {
                            format!("{}:{file}:{line}", short_id(commit))
                        };
                        out.push(Finding::Secret { rule, at });
                    }
                }
                _ => out.push(Finding::Secret {
                    rule: "gitleaks-report-unreadable".to_string(),
                    at: "report".to_string(),
                }),
            }
        }
    }
    if out.is_empty() && exit_code != 0 {
        out.push(Finding::Secret { rule: "gitleaks-failed".to_string(), at: format!("exit code {exit_code}") });
    }
    out
}

/// The gitleaks command line, built but not started (kept apart from `run_gitleaks` so a test can read the
/// arguments). `--ignore-gitleaks-allow` makes an inline allow comment count for nothing: the box or an agent
/// could write such a comment into any file, the same hole as a planted `.gitleaksignore`.
fn gitleaks_command(exe: &Path, repo_dir: &Path, mode: &Mode, empty: &Path, report: &Path) -> Command {
    let mut cmd = Command::new(exe);
    // gitleaks reads history through its own `git log -p`, which inherits this environment (C1).
    cmd.envs(scan_git_env());
    cmd.arg("git")
        .arg("--redact")
        .arg("--no-banner")
        .arg("--ignore-gitleaks-allow")
        .arg("--gitleaks-ignore-path")
        .arg(empty)
        .arg("--report-format")
        .arg("json")
        .arg("--report-path")
        .arg(report);
    match mode {
        Mode::Staged => {
            cmd.arg("--pre-commit").arg("--staged");
        }
        Mode::Range(spec) => {
            cmd.arg(format!("--log-opts={spec}"));
        }
    }
    cmd.arg(repo_dir);
    cmd
}

/// The folder handed to `--gitleaks-ignore-path`: `gitleaks-empty` under `temp`, refused unless it is empty. The
/// box can write under `%TEMP%`, and a `.gitleaksignore` planted there would silence gitleaks (PR-2).
fn empty_ignore_dir(temp: &Path) -> Result<PathBuf, String> {
    let empty = temp.join("gitleaks-empty");
    std::fs::create_dir_all(&empty).map_err(|e| format!("cannot create the empty ignore folder: {e}"))?;
    let mut entries = std::fs::read_dir(&empty).map_err(|e| format!("cannot read the empty ignore folder: {e}"))?;
    if entries.next().is_some() {
        return Err(format!("{} is not empty; the scan refuses planted scanner settings", empty.display()));
    }
    Ok(empty)
}

/// Runs gitleaks (already checked against its pin) on the staged changes or the range. The ignore-file folder
/// is an empty folder under `%TEMP%`; the report goes to a file under `%TEMP%` and is read and removed here.
fn run_gitleaks(exe: &Path, repo: &Repo, mode: &Mode) -> Result<Vec<Finding>, String> {
    let temp = std::env::temp_dir();
    let empty = empty_ignore_dir(&temp)?;
    let report = temp.join(format!("gitleaks-report-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&report);
    let output = gitleaks_command(exe, &repo.dir, mode, &empty, &report)
        .output()
        .map_err(|e| format!("cannot start gitleaks: {e}"))?;
    let bytes = std::fs::read(&report).ok();
    let _ = std::fs::remove_file(&report);
    Ok(gitleaks_findings(bytes.as_deref(), output.status.code().unwrap_or(-1)))
}

/// One file of the end state by object name. A file that cannot be read is a finding, never a silent skip (M2).
fn scan_stored_file(cfg: &ScanConfig, repo: &Repo, object: &str, path: &str, root_blocks_publish: bool) -> Vec<Finding> {
    match repo.blob_bytes(object) {
        Ok(bytes) => scan_file(cfg, path, &bytes, root_blocks_publish),
        Err(_) => vec![Finding::Unreadable { path: path.to_string() }],
    }
}

/// The own checks of Step 3 on the staged changes or on every commit of the range. They start with the
/// refusals about attribute rules (`check_attribute_rules`) and replacement objects (`check_no_replacements`), so
/// nothing is scanned through rules the scan did not choose or through objects git would swap.
fn own_checks(cfg: &ScanConfig, repo: &Repo, mode: &Mode) -> Result<Vec<Finding>, String> {
    repo.check_attribute_rules()?;
    repo.check_no_replacements()?;
    let mut out = Vec::new();
    match mode {
        Mode::Staged => {
            let (data, paths) = repo.staged()?;
            out.extend(scan_commit_inner(cfg, &data, false));
            let branch = repo.git_text(["branch", "--show-current"])?.trim().to_string();
            if !branch.is_empty() {
                out.extend(scan_names(cfg, &[branch]));
            }
            let root_blocks = repo.blob(":0:Cargo.toml").is_some_and(|t| workspace_root_blocks_publish(&t));
            for path in &paths {
                out.extend(scan_stored_file(cfg, repo, &format!(":0:{path}"), path, root_blocks));
            }
        }
        Mode::Range(spec) => {
            validate_range(spec)?;
            let ids = repo.rev_ids(spec)?;
            let idset: BTreeSet<String> = ids.iter().cloned().collect();
            for id in &ids {
                let data = repo.commit_data(id)?;
                // GitHub's merge button writes its merge commit with GitHub as committer; only in its exact shape
                // does that pass (G1).
                if is_github_merge(repo, &data)? {
                    out.extend(scan_github_merge(cfg, &data));
                } else {
                    out.extend(scan_commit(cfg, &data));
                }
            }
            for tag in repo.tags_in(&idset)? {
                out.extend(scan_name(cfg, &tag.name));
                if let Some(message) = &tag.message {
                    let at = format!("tag {}", tag.name);
                    out.extend(scan_text(cfg, message, &at));
                    if brand_word_in(message) {
                        out.push(Finding::Brand { at: format!("{at} (message)") });
                    }
                }
            }
            let tip = repo.resolve_tip(&range_tip(spec))?;
            let root_blocks = repo
                .blob(&format!("{tip}:Cargo.toml"))
                .is_some_and(|t| workspace_root_blocks_publish(&t));
            let files = repo.tip_files(&tip)?;
            out.extend(scan_names(cfg, &files));
            out.extend(scan_paths(cfg, &files));
            for path in &files {
                out.extend(scan_stored_file(cfg, repo, &format!("{tip}:{path}"), path, root_blocks));
            }
        }
    }
    Ok(out)
}

/// The whole scan; `Err` means "refused to run", `Ok` holds one rendered line per finding.
fn run(opts: &Options) -> Result<Vec<String>, String> {
    let repo = Repo { dir: opts.repo.clone() };
    let from_env = std::env::var_os("SKIPPER_SCAN_CONFIG").map(PathBuf::from);
    let config_dir = check_config_dir(opts.config.as_deref().or(from_env.as_deref()), &repo.dir)?;
    check_no_planted_config(&repo.dir)?;
    let (allow_text, pins_text) = rule_texts(&repo, opts)?;
    let cfg = ScanConfig::load(&config_dir, &allow_text).map_err(|e| format!("cannot load the scan settings: {e}"))?;

    let pinned = pinned_exe_sha256(&pins_text).ok_or("[tool.gitleaks] exe_sha256 is not set in supply-chain/pins.toml")?;
    let exe_path = std::env::var_os("SKIPPER_GITLEAKS").map(PathBuf::from).ok_or("SKIPPER_GITLEAKS is not set")?;
    let exe_bytes = std::fs::read(&exe_path).map_err(|e| format!("cannot read the gitleaks executable: {e}"))?;

    let mut findings: Vec<Finding> = Vec::new();
    // The own checks go first: they hold the refusals about attribute rules, and gitleaks must not run before.
    findings.extend(own_checks(&cfg, &repo, &opts.mode)?);
    // A range that changes a judge passes only with the person's acceptance of its tip (B1). The note goes to the log
    // by itself: a finding line would fail the scan.
    if let Mode::Range(spec) = &opts.mode {
        let (found, note) = judge_change(&repo, spec, opts.accept_judge_change.as_deref())?;
        findings.extend(found);
        if let Some(note) = note {
            println!("leak-scan: note: {note}");
        }
    }
    match check_scanner(&exe_bytes, &pinned) {
        Ok(()) => findings.extend(run_gitleaks(&exe_path, &repo, &opts.mode)?),
        Err(finding) => findings.push(finding),
    }

    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for finding in &findings {
        let line = render(&cfg, finding);
        if seen.insert(line.clone()) {
            lines.push(line);
        }
    }
    Ok(lines)
}

/// `leak-scan`: exit 0 and `leak-scan: clean`; exit 1 and one line per finding; exit 2 when it refuses to run
/// or the arguments are wrong.
pub fn run_cli(args: &[String]) -> u8 {
    let opts = match parse_args(args) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("leak-scan: {message}");
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match run(&opts) {
        Ok(lines) if lines.is_empty() => {
            println!("leak-scan: clean");
            0
        }
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            1
        }
        Err(message) => {
            println!("leak-scan: refused: {message}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(user: &str, host: &str) -> String {
        [user, host].join("@")
    }

    fn noreply() -> String {
        addr("1+project", "users.noreply.github.com")
    }

    fn cfg() -> ScanConfig {
        ScanConfig::for_test(&["acme-internal", "word:zq"], &["docs/**", "crates/**"], &noreply())
    }

    fn clean_commit() -> CommitData {
        CommitData {
            id: "0123456789abcdef0123456789abcdef01234567".to_string(),
            author_name: "P".to_string(),
            author_email: noreply(),
            committer_name: "P".to_string(),
            committer_email: noreply(),
            message: format!("feat: x\n\nSigned-off-by: P <{}>", noreply()),
            diff: String::new(),
            paths: vec!["crates/x/src/lib.rs".to_string()],
        }
    }

    fn commit_with_message(message: &str) -> CommitData {
        CommitData { message: message.to_string(), ..clean_commit() }
    }

    fn commit_with_paths(paths: &[&str]) -> CommitData {
        CommitData { paths: paths.iter().map(|p| p.to_string()).collect(), ..clean_commit() }
    }

    fn commit_with_diff(diff: &str) -> CommitData {
        CommitData { diff: diff.to_string(), ..clean_commit() }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("skipper-xtask-leak-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- the test cases of the plan (Task 3 Step 1) ---

    #[test]
    fn term_in_added_line_is_found() {
        assert!(matches!(scan_text(&cfg(), "+ see ACME-Internal notes", "f")[..], [Finding::Term { n: 0, .. }]));
    }

    #[test]
    fn word_flagged_term_matches_whole_word_only() {
        assert!(scan_text(&cfg(), "checksum = \"9fzq81\"", "Cargo.lock").is_empty());
        assert_eq!(scan_text(&cfg(), "the zq tool", "f").len(), 1);
    }

    #[test]
    fn term_in_file_and_branch_name_found() {
        assert_eq!(scan_names(&cfg(), &["docs/acme-internal.md".into(), "feature/zq".into()]).len(), 2);
    }

    #[test]
    fn email_other_than_noreply_stops() {
        assert!(matches!(scan_text(&cfg(), &addr("someone", "example.org"), "f")[..], [Finding::Email { .. }]));
    }

    #[test]
    fn noreply_address_passes() {
        assert!(scan_text(&cfg(), &addr("1+project", "users.noreply.github.com"), "f").is_empty());
    }

    #[test]
    fn only_sign_off_trailer_allowed() {
        let c = commit_with_message(&format!(
            "feat: x\n\nSigned-off-by: P <{}>\nCo-Authored-By: T <{}>",
            addr("1+project", "users.noreply.github.com"),
            addr("bot", "example.org")
        ));
        let f = scan_commit(&cfg(), &c);
        assert!(f.iter().any(|x| matches!(x, Finding::Trailer { .. })) && f.iter().any(|x| matches!(x, Finding::Email { .. })));
    }

    #[test]
    fn missing_sign_off_stops() {
        assert!(scan_commit(&cfg(), &commit_with_message("feat: x"))
            .iter()
            .any(|x| matches!(x, Finding::MissingSignOff { .. })));
    }

    #[test]
    fn author_or_committer_not_noreply_stops() {
        let mut c = clean_commit();
        c.committer_email = addr("someone", "example.org");
        assert!(scan_commit(&cfg(), &c).iter().any(|x| matches!(x, Finding::Identity { .. })));
        let mut c = clean_commit();
        c.author_email = addr("someone", "example.org");
        assert!(scan_commit(&cfg(), &c).iter().any(|x| matches!(x, Finding::Identity { .. })));
    }

    #[test]
    fn path_outside_allow_list_stops() {
        assert!(matches!(scan_commit(&cfg(), &commit_with_paths(&["private/x.txt"]))[..], [Finding::Path { .. }]));
    }

    #[test]
    fn image_needs_exact_allow_entry() {
        assert!(matches!(scan_commit(&cfg(), &commit_with_paths(&["docs/shot.png"]))[..], [Finding::Path { .. }])); // docs/** does not admit images
        let c = ScanConfig::for_test(&["acme-internal"], &["docs/**", "docs/shot.png"], &addr("1+project", "users.noreply.github.com"));
        assert!(scan_commit(&c, &commit_with_paths(&["docs/shot.png"])).is_empty());
    }

    #[test]
    fn windows_profile_path_stops() {
        let p = ["C:", "Users", "sample-user", "AppData"].join("\\");
        assert!(matches!(scan_text(&cfg(), &p, "f")[..], [Finding::ProfilePath { .. }]));
    }

    #[test]
    fn clean_commit_passes() {
        assert!(scan_commit(&cfg(), &clean_commit()).is_empty());
    }

    #[test]
    fn changed_scanner_stops() {
        assert_eq!(check_scanner(b"other", &sha256_hex(b"gitleaks")), Err(Finding::ScannerChanged));
        assert!(check_scanner(b"gitleaks", &sha256_hex(b"gitleaks")).is_ok());
    }

    #[test]
    fn allow_list_comes_from_head_when_head_exists() {
        let head = "docs/**\n".to_string();
        let working = "docs/**\nprivate/**\n".to_string();
        let chosen = prefer_committed(Some(head), Some(working.clone())).unwrap();
        let entries = parse_allow(&chosen);
        let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
        let c = ScanConfig::for_test(&[], &refs, &noreply());
        assert!(matches!(scan_commit(&c, &commit_with_paths(&["private/x.txt"]))[..], [Finding::Path { .. }]));
        // A HEAD without scan/allowed-paths.txt uses the working-tree list.
        let chosen = prefer_committed(None, Some(working)).unwrap();
        let entries = parse_allow(&chosen);
        let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
        let c = ScanConfig::for_test(&[], &refs, &noreply());
        assert!(scan_commit(&c, &commit_with_paths(&["private/x.txt"])).is_empty());
        assert_eq!(prefer_committed(None, None), None);
    }

    #[test]
    fn planted_scanner_config_refused() {
        let repo = temp_dir("planted");
        assert!(check_no_planted_config(&repo).is_ok());
        std::fs::write(repo.join(".gitleaks.toml"), "title = \"x\"\n").unwrap();
        assert!(check_no_planted_config(&repo).is_err());
        std::fs::remove_file(repo.join(".gitleaks.toml")).unwrap();
        std::fs::write(repo.join(".gitleaksignore"), "x\n").unwrap();
        assert!(check_no_planted_config(&repo).is_err());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn brand_word_matches_any_case_inside_words() {
        assert!(brand_word_in("my-Claude-helper") && brand_word_in("xANTHROPICx") && !brand_word_in("skipper-hook"));
    }

    #[test]
    fn brand_word_in_package_name_stops() {
        assert!(scan_manifest(&cfg(), "crates/x/Cargo.toml", "[package]\nname = \"claude-x\"\npublish.workspace = true\n", true)
            .iter()
            .any(|f| matches!(f, Finding::Brand { .. })));
    }

    #[test]
    fn brand_word_in_tag_branch_and_image_name_stops() {
        assert_eq!(
            scan_names(&cfg(), &["v0.1-anthropic".into(), "feature/claude-ui".into(), "docs/claude-logo.png".into()])
                .iter()
                .filter(|f| matches!(f, Finding::Brand { .. }))
                .count(),
            3
        );
    }

    #[test]
    fn brand_word_in_plain_text_passes() {
        assert!(scan_text(&cfg(), "Skipper runs with Claude Code and is not affiliated with Anthropic.", "README.md").is_empty());
    }

    #[test]
    fn manifest_without_publish_false_stops() {
        assert!(matches!(
            scan_manifest(&cfg(), "crates/x/Cargo.toml", "[package]\nname = \"x\"\n", true)[..],
            [Finding::Publishable { .. }]
        ));
    }

    #[test]
    fn manifest_inherits_publish_false_only_from_root() {
        let m = "[package]\nname = \"x\"\npublish.workspace = true\n";
        assert!(scan_manifest(&cfg(), "crates/x/Cargo.toml", m, true).is_empty());
        assert!(matches!(scan_manifest(&cfg(), "crates/x/Cargo.toml", m, false)[..], [Finding::Publishable { .. }]));
    }

    #[test]
    fn root_manifest_is_checked_for_workspace_publish_false() {
        assert!(scan_manifest(&cfg(), "Cargo.toml", "[workspace]\nmembers = [\"crates/x\"]\n\n[workspace.package]\npublish = false\n", true).is_empty());
        assert!(matches!(scan_manifest(&cfg(), "Cargo.toml", "[workspace]\nmembers = [\"crates/x\"]\n", false)[..], [Finding::Publishable { .. }]));
    }

    // --- further cases ---

    #[test]
    fn manifest_with_its_own_publish_false_passes() {
        assert!(scan_manifest(&cfg(), "spikes/b/Cargo.toml", "[package]\nname = \"b\"\npublish = false\n", false).is_empty());
        assert!(scan_manifest(&cfg(), "spikes/b/Cargo.toml", "[package]\nname = \"b\"\npublish = []\n", false).is_empty());
        assert!(matches!(
            scan_manifest(&cfg(), "spikes/b/Cargo.toml", "[package]\nname = \"b\"\npublish = true\n", true)[..],
            [Finding::Publishable { .. }]
        ));
    }

    #[test]
    fn virtual_workspace_manifest_publishes_nothing() {
        let root = "[workspace]\nmembers = [\"crates/x\"]\n\n[workspace.package]\npublish = false\n";
        assert!(scan_manifest(&cfg(), "Cargo.toml", root, false).is_empty());
        assert!(workspace_root_blocks_publish(root));
        assert!(!workspace_root_blocks_publish("[workspace]\nmembers = []\n"));
        assert!(!workspace_root_blocks_publish("not toml ["));
    }

    #[test]
    fn brand_word_in_lib_and_bin_names_stops() {
        let m = "[package]\nname = \"x\"\npublish = false\n\n[lib]\nname = \"anthropic_x\"\n";
        assert!(scan_manifest(&cfg(), "crates/x/Cargo.toml", m, true).iter().any(|f| matches!(f, Finding::Brand { .. })));
        let m = "[package]\nname = \"x\"\npublish = false\n\n[[bin]]\nname = \"claude-x\"\npath = \"src/main.rs\"\n";
        assert!(scan_manifest(&cfg(), "crates/x/Cargo.toml", m, true).iter().any(|f| matches!(f, Finding::Brand { .. })));
    }

    #[test]
    fn unreadable_manifest_is_a_finding() {
        assert!(matches!(
            scan_manifest(&cfg(), "crates/x/Cargo.toml", "[package\n", true)[..],
            [Finding::Publishable { .. }]
        ));
    }

    #[test]
    fn every_file_name_is_brand_checked() {
        // Plan Task 3 Step 3: a plain name does not say whether it is a branch or a file, so file names of every
        // type are checked, not only images and videos (stricter than MC-163).
        assert_eq!(scan_names(&cfg(), &["docs/claude-notes.md".into()]), vec![Finding::Brand { at: "docs/claude-notes.md".into() }]);
        let c = commit_with_paths(&["docs/anthropic-notes.md"]);
        assert_eq!(scan_commit(&cfg(), &c), vec![Finding::Brand { at: "docs/anthropic-notes.md".into() }]);
    }

    #[test]
    fn planted_file_in_the_empty_ignore_folder_refused() {
        // PR-2: the box can write under %TEMP%, so a .gitleaksignore planted in the folder handed to
        // --gitleaks-ignore-path would silence gitleaks.
        let base = std::env::temp_dir().join(format!("xtask-leak-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let empty = empty_ignore_dir(&base).unwrap();
        assert!(std::fs::read_dir(&empty).unwrap().next().is_none());
        std::fs::write(empty.join(".gitleaksignore"), b"x\n").unwrap();
        assert!(empty_ignore_dir(&base).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn terms_are_case_insensitive_and_numbered_from_zero() {
        let f = scan_text(&cfg(), "ACME-INTERNAL and Zq", "f");
        assert_eq!(f, vec![Finding::Term { n: 0, at: "f".into() }, Finding::Term { n: 1, at: "f".into() }]);
    }

    #[test]
    fn each_term_is_reported_once_per_call() {
        let f = scan_text(&cfg(), "acme-internal acme-internal acme-internal", "f");
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn terms_file_skips_comments_and_blank_lines() {
        let t = parse_term("  word:Zq  ").unwrap();
        assert!(t.word && t.text == "zq");
        assert!(parse_term("# note").is_none());
        assert!(parse_term("   ").is_none());
        assert!(parse_term("word:").is_none());
        assert!(!parse_term("plain").unwrap().word);
    }

    #[test]
    fn word_term_at_start_end_and_between_punctuation() {
        assert_eq!(scan_text(&cfg(), "zq", "f").len(), 1);
        assert_eq!(scan_text(&cfg(), "(zq)", "f").len(), 1);
        assert_eq!(scan_text(&cfg(), "zq_tool", "f").len(), 0);
        assert_eq!(scan_text(&cfg(), "xzq zq", "f").len(), 1);
    }

    #[test]
    fn email_shapes() {
        let shapes = [
            format!("see {} now", addr("a.b", "c.de")),
            format!("x <{}>", addr("p", "q.org")),
            format!("mail: {}.", addr("p+tag", "sub.example.co.uk")),
        ];
        for text in &shapes {
            assert_eq!(scan_text(&cfg(), text, "f").len(), 1, "{text}");
        }
        for text in ["@scope/pkg", "serde@1.0.0", "a@b", "user @ host.com", "plain text"] {
            assert!(scan_text(&cfg(), text, "f").is_empty(), "{text}");
        }
    }

    #[test]
    fn same_address_twice_is_one_finding_and_noreply_matches_any_case() {
        let a = addr("someone", "example.org");
        assert_eq!(scan_text(&cfg(), &format!("{a} {a}"), "f").len(), 1);
        assert!(scan_text(&cfg(), &noreply().to_uppercase(), "f").is_empty());
    }

    #[test]
    fn profile_path_shapes() {
        let hits = [
            ["C:", "Users", "sample-user"].join("\\"),
            ["C:", "Users", "sample-user"].join("\\\\"),
            ["c:", "users", "sample-user", "x"].join("/"),
            ["", "home", "sample-user", "x"].join("/"),
            format!("see {}", ["", "Users", "sample-user"].join("/")),
            format!("\\\\?\\{}", ["D:", "Users", "sample-user"].join("\\")),
        ];
        for text in &hits {
            assert!(has_profile_path(text), "{text}");
        }
        let misses = [
            "%USERPROFILE%\\AppData".to_string(),
            "%LOCALAPPDATA%\\skipper-spike".to_string(),
            ["crates", "skipper-app", "src", "home", "mod.rs"].join("/"),
            ["https://example.org", "home", "page"].join("/"),
            ["C:", "Users"].join("\\"),
            "C:\\Program Files\\x".to_string(),
        ];
        for text in &misses {
            assert!(!has_profile_path(text), "{text}");
        }
    }

    #[test]
    fn trailer_rules() {
        let n = noreply();
        let two = commit_with_message(&format!("feat: x\n\nSigned-off-by: P <{n}>\nSigned-off-by: P <{n}>"));
        assert!(scan_commit(&cfg(), &two).iter().any(|f| matches!(f, Finding::Trailer { .. })));
        let wrong_name = commit_with_message(&format!("feat: x\n\nSigned-off-by: Q <{n}>"));
        let f = scan_commit(&cfg(), &wrong_name);
        assert!(f.iter().any(|f| matches!(f, Finding::Trailer { .. })));
        assert!(f.iter().any(|f| matches!(f, Finding::MissingSignOff { .. })));
        let session = commit_with_message(&format!("feat: x\n\nSigned-off-by: P <{n}>\nClaude-Session: https://example.org/s"));
        assert!(scan_commit(&cfg(), &session).iter().any(|f| matches!(f, Finding::Trailer { .. })));
        let hidden = commit_with_message(&format!("feat: x\n\nCo-Authored-By: T <{n}>\n\nmore text\n\nSigned-off-by: P <{n}>"));
        assert!(scan_commit(&cfg(), &hidden).iter().any(|f| matches!(f, Finding::Trailer { .. })));
        let plain_body = commit_with_message(&format!("feat: x\n\nA body line.\n\nSigned-off-by: P <{n}>\n"));
        assert!(scan_commit(&cfg(), &plain_body).is_empty());
    }

    #[test]
    fn crlf_message_with_valid_sign_off_passes() {
        let n = noreply();
        let c = commit_with_message(&format!("feat: x\r\n\r\nSigned-off-by: P <{n}>\r\n"));
        assert!(scan_commit(&cfg(), &c).is_empty());
    }

    #[test]
    fn sign_off_address_must_be_the_noreply_address() {
        let c = commit_with_message(&format!("feat: x\n\nSigned-off-by: P <{}>", addr("someone", "example.org")));
        let f = scan_commit(&cfg(), &c);
        assert!(f.iter().any(|f| matches!(f, Finding::MissingSignOff { .. })));
        assert!(f.iter().any(|f| matches!(f, Finding::Email { .. })));
    }

    #[test]
    fn only_added_lines_of_the_diff_are_scanned() {
        let diff = "diff --git a/docs/f.md b/docs/f.md\n--- a/docs/f.md\n+++ b/docs/f.md\n@@ -1 +1 @@\n-removed ACME-Internal line\n+clean line\n";
        assert!(scan_commit(&cfg(), &commit_with_diff(diff)).is_empty());
        let diff = "diff --git a/docs/f.md b/docs/f.md\n--- a/docs/f.md\n+++ b/docs/f.md\n@@ -0,0 +1 @@\n+ see ACME-Internal notes\n";
        let f = scan_commit(&cfg(), &commit_with_diff(diff));
        assert!(matches!(f[..], [Finding::Term { n: 0, .. }]));
        if let Finding::Term { at, .. } = &f[0] {
            assert!(at.ends_with(":docs/f.md"), "{at}");
        }
    }

    #[test]
    fn diff_headers_are_not_scanned_as_added_lines() {
        let diff = "diff --git a/docs/f.md b/docs/f.md\nnew file mode 100644\n--- /dev/null\n+++ b/docs/f.md\n@@ -0,0 +1 @@\n+fine\n";
        assert!(scan_diff(&cfg(), diff, "abc").is_empty());
        let two = "diff --git a/docs/a.md b/docs/a.md\n--- a/docs/a.md\n+++ b/docs/a.md\n@@ -0,0 +1 @@\n+fine\ndiff --git a/docs/b.md b/docs/b.md\n--- a/docs/b.md\n+++ b/docs/b.md\n@@ -0,0 +1 @@\n+zq\n";
        let f = scan_diff(&cfg(), two, "abc");
        assert_eq!(f.len(), 1);
        assert!(matches!(&f[0], Finding::Term { at, .. } if at == "abc:docs/b.md"));
    }

    #[test]
    fn profile_path_and_email_in_diff_are_found() {
        let path = ["C:", "Users", "sample-user"].join("\\");
        let diff = format!(
            "diff --git a/docs/f.md b/docs/f.md\n--- a/docs/f.md\n+++ b/docs/f.md\n@@ -0,0 +1,2 @@\n+{path}\n+{}\n",
            addr("someone", "example.org")
        );
        let f = scan_commit(&cfg(), &commit_with_diff(&diff));
        assert!(f.iter().any(|x| matches!(x, Finding::ProfilePath { .. })));
        assert!(f.iter().any(|x| matches!(x, Finding::Email { .. })));
    }

    #[test]
    fn term_in_changed_file_name_is_found() {
        let f = scan_commit(&cfg(), &commit_with_paths(&["docs/acme-internal.md"]));
        assert!(matches!(f[..], [Finding::Term { n: 0, .. }]));
    }

    #[test]
    fn allow_list_patterns() {
        let c = ScanConfig::for_test(&[], &["README.md", "docs/**", "Cargo.toml", ".github/**", "docs/ok.png"], &noreply());
        assert!(c.path_allowed("README.md"));
        assert!(c.path_allowed("docs/a/b/c.md"));
        assert!(c.path_allowed(".github/ISSUE_TEMPLATE/config.yml"));
        assert!(c.path_allowed("Cargo.toml"));
        assert!(!c.path_allowed("crates/x/Cargo.toml"));
        assert!(!c.path_allowed("docs"));
        assert!(!c.path_allowed("Docs/a.md"));
        assert!(!c.path_allowed("src/README.md"));
        assert!(c.path_allowed("docs/ok.png"));
        assert!(!c.path_allowed("docs/other.png"));
        assert!(!c.path_allowed("docs/SHOT.PNG"));
        for ext in MEDIA_EXTENSIONS {
            assert!(!c.path_allowed(&format!("docs/x.{ext}")), "{ext}");
        }
    }

    #[test]
    fn single_star_does_not_cross_folders_and_never_admits_media() {
        let c = ScanConfig::for_test(&[], &["docs/*.md", "docs/*", "a?c.txt"], &noreply());
        assert!(c.path_allowed("docs/a.md"));
        assert!(!c.path_allowed("docs/x/a.md"));
        assert!(!c.path_allowed("docs/x.png"));
        assert!(c.path_allowed("abc.txt"));
        assert!(!c.path_allowed("a/c.txt"));
    }

    #[test]
    fn shipped_allow_list_has_the_plan_entries() {
        let text = include_str!("../../scan/allowed-paths.txt");
        let entries = parse_allow(text);
        // 22 plan entries, then the exact paths of 96 binary and media files (comment and blank lines are skipped).
        assert_eq!(entries.len(), 118);
        let plan = [
            "README.md", "LICENSE", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md", "GOVERNANCE.md", "SUPPORT.md", "AGENTS.md",
            ".gitignore", ".gitattributes", ".github/**", "docs/**", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
            ".cargo/config.toml", "tools/**", "scan/**", "supply-chain/**", "crates/**", "spikes/**", "xtask/**",
        ];
        assert_eq!(plan.len(), 22);
        for expected in plan {
            assert!(entries.iter().any(|e| e == expected), "{expected}");
        }
        // The 96 exact paths, copied byte for byte from scan/allowed-paths.txt: 5 icon pixel files, 1 update
        // fixture pack, 2 import fixture images, 11 brand vector files, 69 brand picture files (27 logo PNG
        // files, the Windows icon, 3 favicon files, 19 social images and 19 social vector sources), and 8 vector
        // pictures of the README (the hero banner in a dark and a light version, and six card icons; row GH-3).
        let exact = [
            "crates/skipper-app/assets/icon/skipper-16.rgba",
            "crates/skipper-app/assets/icon/skipper-20.rgba",
            "crates/skipper-app/assets/icon/skipper-24.rgba",
            "crates/skipper-app/assets/icon/skipper-32.rgba",
            "crates/skipper-app/assets/icon/skipper-256.rgba",
            "crates/skipper-import/tests/fixtures/markdown/vault/assets/logo.png",
            "crates/skipper-import/tests/fixtures/notion_zip/export/Notes table 5e6f708192a3b4c5d6e7f8091a2b3c4d/First note 6f708192a3b4c5d6e7f8091a2b3c4d5e/shot.png",
            "crates/skipper-update/tests/fixtures/releases/skipper-0.2.0.pack",
            "docs/brand/skipper-icon.svg",
            "docs/brand/skipper-icon-16.svg",
            "docs/brand/skipper-icon-20.svg",
            "docs/brand/skipper-icon-24.svg",
            "docs/brand/skipper-icon-32.svg",
            "docs/brand/skipper-icon-mono.svg",
            "docs/brand/skipper-icon-square.svg",
            "docs/brand/skipper-mark-dark.svg",
            "docs/brand/skipper-mark-light.svg",
            "docs/brand/skipper-mark-mono-dark.svg",
            "docs/brand/skipper-mark-mono-light.svg",
            "docs/brand/favicon/apple-touch-icon.png",
            "docs/brand/favicon/favicon.ico",
            "docs/brand/favicon/favicon.svg",
            "docs/brand/png/skipper-icon-1024.png",
            "docs/brand/png/skipper-icon-128.png",
            "docs/brand/png/skipper-icon-16.png",
            "docs/brand/png/skipper-icon-180.png",
            "docs/brand/png/skipper-icon-192.png",
            "docs/brand/png/skipper-icon-20.png",
            "docs/brand/png/skipper-icon-2048.png",
            "docs/brand/png/skipper-icon-24.png",
            "docs/brand/png/skipper-icon-256.png",
            "docs/brand/png/skipper-icon-32.png",
            "docs/brand/png/skipper-icon-40.png",
            "docs/brand/png/skipper-icon-48.png",
            "docs/brand/png/skipper-icon-512.png",
            "docs/brand/png/skipper-icon-64.png",
            "docs/brand/png/skipper-icon-96.png",
            "docs/brand/png/skipper-mark-dark-1024.png",
            "docs/brand/png/skipper-mark-dark-128.png",
            "docs/brand/png/skipper-mark-dark-2048.png",
            "docs/brand/png/skipper-mark-dark-256.png",
            "docs/brand/png/skipper-mark-dark-512.png",
            "docs/brand/png/skipper-mark-dark-64.png",
            "docs/brand/png/skipper-mark-light-1024.png",
            "docs/brand/png/skipper-mark-light-128.png",
            "docs/brand/png/skipper-mark-light-2048.png",
            "docs/brand/png/skipper-mark-light-256.png",
            "docs/brand/png/skipper-mark-light-512.png",
            "docs/brand/png/skipper-mark-light-64.png",
            "docs/brand/skipper.ico",
            "docs/brand/social/avatar-1024.png",
            "docs/brand/social/avatar-2048.png",
            "docs/brand/social/discord-banner-1920x1080-a.png",
            "docs/brand/social/discord-banner-1920x1080-b.png",
            "docs/brand/social/discord-banner-1920x1080-c.png",
            "docs/brand/social/discord-banner-960x540-a.png",
            "docs/brand/social/discord-banner-960x540-b.png",
            "docs/brand/social/discord-banner-960x540-c.png",
            "docs/brand/social/discord-icon-512.png",
            "docs/brand/social/github-social-preview-1280x640.png",
            "docs/brand/social/linkedin-banner-1584x396.png",
            "docs/brand/social/og-image-1200x630.png",
            "docs/brand/social/src/avatar-1024.svg",
            "docs/brand/social/src/avatar-2048.svg",
            "docs/brand/social/src/discord-banner-1920x1080-a.svg",
            "docs/brand/social/src/discord-banner-1920x1080-b.svg",
            "docs/brand/social/src/discord-banner-1920x1080-c.svg",
            "docs/brand/social/src/discord-banner-960x540-a.svg",
            "docs/brand/social/src/discord-banner-960x540-b.svg",
            "docs/brand/social/src/discord-banner-960x540-c.svg",
            "docs/brand/social/src/discord-icon-512.svg",
            "docs/brand/social/src/github-social-preview-1280x640.svg",
            "docs/brand/social/src/linkedin-banner-1584x396.svg",
            "docs/brand/social/src/og-image-1200x630.svg",
            "docs/brand/social/src/x-avatar-400.svg",
            "docs/brand/social/src/x-header-1500x500-a.svg",
            "docs/brand/social/src/x-header-1500x500-b.svg",
            "docs/brand/social/src/x-header-1500x500-c.svg",
            "docs/brand/social/src/x-header-3000x1000-a.svg",
            "docs/brand/social/src/x-header-3000x1000-b.svg",
            "docs/brand/social/src/x-header-3000x1000-c.svg",
            "docs/brand/social/x-avatar-400.png",
            "docs/brand/social/x-header-1500x500-a.png",
            "docs/brand/social/x-header-1500x500-b.png",
            "docs/brand/social/x-header-1500x500-c.png",
            "docs/brand/social/x-header-3000x1000-a.png",
            "docs/brand/social/x-header-3000x1000-b.png",
            "docs/brand/social/x-header-3000x1000-c.png",
            "docs/brand/readme/hero-dark.svg",
            "docs/brand/readme/hero-light.svg",
            "docs/brand/readme/icon-account.svg",
            "docs/brand/readme/icon-approve.svg",
            "docs/brand/readme/icon-board.svg",
            "docs/brand/readme/icon-limits.svg",
            "docs/brand/readme/icon-native.svg",
            "docs/brand/readme/icon-terminal.svg",
        ];
        assert_eq!(exact.len(), 96);
        assert_eq!(entries.len(), plan.len() + exact.len());
        for path in exact {
            // Present exactly once, and never a pattern: a wildcard here would admit more than the one file.
            assert_eq!(entries.iter().filter(|e| e.as_str() == path).count(), 1, "{path}");
            assert!(!path.contains('*') && !path.contains('?'), "{path}");
        }
        let refs: Vec<&str> = entries.iter().map(String::as_str).collect();
        let c = ScanConfig::for_test(&[], &refs, &noreply());
        assert!(c.path_allowed("xtask/src/leak.rs"));
        assert!(c.path_allowed("tools/skipper-cap/main.rs"));
        assert!(c.path_allowed("docs/probes/stage-0/checklist.md"));
        assert!(!c.path_allowed("private/x.txt"));
        assert!(!c.path_allowed("docs/shot.png"));
        assert!(!c.path_allowed("Cargo.toml.bak"));
        // Every one of the 96 passes the per-commit path check and the exact-path rule for binary files.
        for path in exact {
            assert!(c.path_allowed(path), "{path}");
            assert!(c.path_listed_exactly(path), "{path}");
        }
        // Without the 96 entries, the wide plan entries admit none of them as a binary file, and none of the
        // images (an image passes the path check only with its exact path).
        let plan_only = ScanConfig::for_test(&[], &plan, &noreply());
        for path in exact {
            assert!(!plan_only.path_listed_exactly(path), "{path}");
            if is_media(path) {
                assert!(!plan_only.path_allowed(path), "{path}");
            }
        }
        // Near misses of exact entries stay refused. A media file with no exact entry fails the path check, and
        // so does a changed letter case of an exact media entry. `.rgba` is no media extension, so `crates/**`
        // still admits the name in the path check; a binary file there is held back by the exact-path rule.
        assert!(!c.path_allowed("docs/brand/skipper-icon-48.svg"));
        assert!(!c.path_allowed("docs/brand/skipper-icon.SVG"));
        assert!(!c.path_allowed("docs/brand/png/skipper-icon-8.png"));
        assert!(!c.path_allowed("docs/brand/png/skipper-icon-16.PNG"));
        assert!(!c.path_allowed("docs/brand/social/_vorschau/preview-round-avatars.png"));
        assert!(!c.path_allowed("docs/brand/readme/hero-dark.SVG"));
        assert!(!c.path_allowed("docs/brand/readme/hero-sepia.svg"));
        assert!(!c.path_allowed("crates/skipper-import/tests/fixtures/markdown/vault/assets/logo2.png"));
        assert!(!c.path_listed_exactly("crates/skipper-app/assets/icon/skipper-48.rgba"));
        assert!(!c.path_listed_exactly("crates/skipper-app/assets/icon/skipper-16.rgba.bak"));
        assert!(!c.path_listed_exactly("crates/skipper-app/assets/icon/Skipper-16.rgba"));
    }

    #[test]
    fn config_loads_terms_and_identity_from_a_folder() {
        let dir = temp_dir("load");
        std::fs::write(dir.join("terms.txt"), "# comment\nsecret-one\n\nword:zq\n").unwrap();
        std::fs::write(dir.join("identity.toml"), format!("name = \"P\"\nnoreply = \"{}\"\n", noreply())).unwrap();
        let c = ScanConfig::load(&dir, "docs/**\n# comment\n\ncrates/**\n").unwrap();
        assert_eq!(c.terms.len(), 2);
        assert_eq!(c.allow, vec!["docs/**".to_string(), "crates/**".to_string()]);
        assert_eq!(c.name, "P");
        assert_eq!(scan_text(&c, "has Secret-One in it", "f").len(), 1);
        assert!(scan_commit(&c, &clean_commit()).is_empty());
        std::fs::write(dir.join("identity.toml"), "name = \"P\"\n").unwrap();
        assert!(ScanConfig::load(&dir, "").is_err());
        std::fs::write(dir.join("identity.toml"), "not toml [").unwrap();
        assert!(ScanConfig::load(&dir, "").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_folder_must_exist_outside_the_repository() {
        let repo = temp_dir("cfgrepo");
        let inside = repo.join("settings");
        std::fs::create_dir_all(&inside).unwrap();
        assert!(check_config_dir(Some(inside.as_path()), &repo).is_err());
        assert!(check_config_dir(None, &repo).is_err());
        assert!(check_config_dir(Some(repo.join("missing").as_path()), &repo).is_err());
        let outside = temp_dir("cfgoutside");
        assert!(check_config_dir(Some(outside.as_path()), &repo).is_ok());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn scanner_pin_must_be_a_full_hex_digest() {
        assert_eq!(check_scanner(b"x", ""), Err(Finding::ScannerChanged));
        assert_eq!(check_scanner(b"x", "abc"), Err(Finding::ScannerChanged));
        assert!(check_scanner(b"gitleaks", &sha256_hex(b"gitleaks").to_uppercase()).is_ok());
    }

    #[test]
    fn scanner_pin_is_read_from_the_pins_file() {
        let sha = "a".repeat(64);
        let ok = format!("[tool.gitleaks]\nexe_sha256 = \"{sha}\"\n");
        assert_eq!(pinned_exe_sha256(&ok), Some(sha));
        assert_eq!(pinned_exe_sha256("[tool.gitleaks]\nexe_sha256 = \"\"\n"), None);
        assert_eq!(pinned_exe_sha256("[tool.gitleaks]\n"), None);
        assert_eq!(pinned_exe_sha256("not toml ["), None);
    }

    #[test]
    fn range_arguments() {
        assert_eq!(range_tip("a..b"), "b");
        assert_eq!(range_tip("a...b"), "b");
        assert_eq!(range_tip("b"), "b");
        assert_eq!(range_tip("a.."), "HEAD");
        assert!(validate_range("origin/stage-0-probe..0123456789abcdef").is_ok());
        assert!(validate_range("0123456789abcdef").is_ok());
        assert!(validate_range("--output=x").is_err());
        assert!(validate_range("a b").is_err());
        assert!(validate_range("").is_err());
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_line_forms() {
        let o = parse_args(&args(&["--staged"])).unwrap();
        assert_eq!(o.mode, Mode::Staged);
        assert_eq!(o.repo, PathBuf::from("."));
        let o = parse_args(&args(&["--range", "a..b", "--config", "c", "--allow", "l.txt", "--repo", "r"])).unwrap();
        assert_eq!(o.mode, Mode::Range("a..b".to_string()));
        assert_eq!(o.config, Some(PathBuf::from("c")));
        assert_eq!(o.allow, Some(PathBuf::from("l.txt")));
        assert_eq!(o.repo, PathBuf::from("r"));
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["--staged", "--range", "a"])).is_err());
        assert!(parse_args(&args(&["--range"])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
        assert_eq!(run_cli(&args(&["--bogus"])), 2);
    }

    #[test]
    fn gitleaks_report_becomes_secret_findings() {
        let report = br#"[{"RuleID":"generic-api-key","File":"docs/a.md","StartLine":3,"Commit":"0123456789abcdef","Secret":"REDACTED"},{"RuleID":"other","File":"b.txt","StartLine":1}]"#;
        let f = gitleaks_findings(Some(report), 1);
        assert_eq!(
            f,
            vec![
                Finding::Secret { rule: "generic-api-key".into(), at: "01234567:docs/a.md:3".into() },
                Finding::Secret { rule: "other".into(), at: "b.txt:1".into() },
            ]
        );
    }

    #[test]
    fn gitleaks_clean_and_failing_runs() {
        assert!(gitleaks_findings(None, 0).is_empty());
        assert!(gitleaks_findings(Some(b"[]"), 0).is_empty());
        assert!(gitleaks_findings(Some(b""), 0).is_empty());
        assert!(matches!(gitleaks_findings(Some(b"[]"), 1)[..], [Finding::Secret { .. }]));
        assert!(matches!(gitleaks_findings(None, 126)[..], [Finding::Secret { .. }]));
        assert!(matches!(gitleaks_findings(Some(b"garbage"), 0)[..], [Finding::Secret { .. }]));
    }

    #[test]
    fn output_shows_terms_by_number_only() {
        let c = cfg();
        let f = Finding::Term { n: 0, at: "docs/acme-internal.md".into() };
        let line = render(&c, &f);
        assert!(!line.to_lowercase().contains("acme-internal"), "{line}");
        assert!(line.contains("term #1") && line.contains("<term 1>"), "{line}");
        let line = render(&c, &Finding::Path { path: "private/ACME-INTERNAL/x".into() });
        assert!(!line.to_lowercase().contains("acme-internal"), "{line}");
        let line = render(&c, &Finding::Brand { at: "feature/zq-claude".into() });
        assert!(line.contains("<term 2>"), "{line}");
        assert_eq!(mask_terms(&c, "nothing here"), "nothing here");
    }

    #[test]
    fn every_finding_renders_one_line() {
        let c = cfg();
        let all = [
            Finding::Term { n: 0, at: "f".into() },
            Finding::Email { at: "f".into() },
            Finding::Trailer { at: "f".into() },
            Finding::MissingSignOff { commit: "abc".into() },
            Finding::Identity { commit: "abc".into() },
            Finding::Path { path: "p".into() },
            Finding::ProfilePath { at: "f".into() },
            Finding::Secret { rule: "r".into(), at: "f".into() },
            Finding::Brand { at: "f".into() },
            Finding::Publishable { manifest: "m".into() },
            Finding::ScannerChanged,
            Finding::Binary { path: "p".into() },
            Finding::Unreadable { path: "p".into() },
            Finding::JudgeChange { rules: "xtask/**".into() },
            Finding::SecretFile { path: "p".into(), rule: "r" },
            Finding::SecretContent { rule: "r", at: "f".into() },
            Finding::BannedLanguage { path: "p".into(), rule: "r" },
        ];
        for f in &all {
            let line = render(&c, f);
            assert!(!line.is_empty() && !line.contains('\n'), "{f:?}");
        }
    }

    #[test]
    fn split_nul_drops_empty_entries() {
        assert_eq!(split_nul(b"a/b\0c\0"), vec!["a/b".to_string(), "c".to_string()]);
        assert!(split_nul(b"").is_empty());
    }

    #[test]
    fn gitleaks_ignores_inline_allow_comments_in_both_modes() {
        let repo_dir = Path::new("D:\\repo");
        for mode in [Mode::Staged, Mode::Range("a..b".to_string())] {
            let cmd = gitleaks_command(
                Path::new("gitleaks.exe"),
                repo_dir,
                &mode,
                Path::new("D:\\empty"),
                Path::new("D:\\report.json"),
            );
            let args: Vec<&OsStr> = cmd.get_args().collect();
            assert_eq!(args.first().copied(), Some(OsStr::new("git")));
            assert!(args.contains(&OsStr::new("--ignore-gitleaks-allow")), "{args:?}");
            assert!(args.contains(&OsStr::new("--redact")), "{args:?}");
            assert_eq!(args.last().copied(), Some(repo_dir.as_os_str()));
        }
    }

    // --- regressions of an earlier review: these failed before the fixes ---

    /// A made-up GitHub-style token, put together at run time so that no file of the repository holds
    /// token-shaped text (G18).
    fn fake_github_token() -> String {
        format!("{}{}{}", "gh", "p_", "aB3dE5gH7j".repeat(4))
    }

    /// A made-up key in the shape of an Anthropic API key, put together at run time.
    fn fake_anthropic_key() -> String {
        format!("{}{}{}", "sk-", "ant-", "Zx9Qw7Er5T".repeat(3))
    }

    fn has_secret(findings: &[Finding]) -> bool {
        findings.iter().any(|f| matches!(f, Finding::Secret { .. }))
    }

    /// Runs `git` in `dir` with a fixed identity and without the system and user configuration; panics when git
    /// fails. Returns the trimmed standard output.
    fn run_git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("-c")
            .arg("user.name=P")
            .arg("-c")
            .arg(format!("user.email={}", noreply()))
            .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", std::env::temp_dir().join("skipper-xtask-leak-no-global-config"))
            .output()
            .expect("git starts");
        assert!(output.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// An empty throwaway repository.
    fn init_repo(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        run_git(&dir, &["init", "-q"]);
        dir
    }

    fn write_file(dir: &Path, rel: &str, bytes: &[u8]) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    /// Writes `rel`, stages it and commits it with the project identity and a valid sign-off; returns the commit id.
    fn commit_file(dir: &Path, rel: &str, bytes: &[u8]) -> String {
        write_file(dir, rel, bytes);
        run_git(dir, &["add", "--", rel]);
        let message = format!("test: add a file\n\nSigned-off-by: P <{}>", noreply());
        run_git(dir, &["commit", "-q", "-m", message.as_str()]);
        run_git(dir, &["rev-parse", "HEAD"])
    }

    /// The configuration of the throwaway repositories: the root `.gitattributes` and `docs/**` are allowed.
    fn repo_cfg() -> ScanConfig {
        ScanConfig::for_test(&["acme-internal", "word:zq"], &["docs/**", ".gitattributes"], &noreply())
    }

    /// The own checks over every commit reachable from `HEAD` and the files at its tip.
    fn findings_of(dir: &Path, cfg: &ScanConfig) -> Vec<Finding> {
        own_checks(cfg, &Repo { dir: dir.to_path_buf() }, &Mode::Range("HEAD".to_string())).unwrap()
    }

    #[test]
    fn clean_throwaway_repository_has_no_findings() {
        let dir = init_repo("clean-repo");
        commit_file(&dir, "docs/a.md", b"fine\n");
        let findings = findings_of(&dir, &repo_cfg());
        assert!(findings.is_empty(), "{findings:?}");
    }

    // C1: attribute rules must not hide content from the scan.

    #[test]
    fn gitattributes_binary_line_does_not_hide_a_secret() {
        // An untracked `.gitattributes` with `* -diff` turned every patch into "Binary files differ".
        let dir = init_repo("attr-binary");
        write_file(&dir, ".gitattributes", b"* -diff\n");
        let token = fake_github_token();
        let id = commit_file(&dir, "docs/notes.md", format!("token = {token}\n").as_bytes());
        let data = Repo { dir: dir.clone() }.commit_data(&id).unwrap();
        assert!(data.diff.contains(&token), "the patch hides the added line: {}", data.diff);
    }

    #[test]
    fn nested_committed_gitattributes_does_not_hide_a_secret() {
        let dir = init_repo("attr-nested");
        commit_file(&dir, "docs/.gitattributes", b"*.md binary\n");
        let token = fake_github_token();
        let id = commit_file(&dir, "docs/notes.md", format!("token = {token}\n").as_bytes());
        let data = Repo { dir: dir.clone() }.commit_data(&id).unwrap();
        assert!(data.diff.contains(&token), "the patch hides the added line: {}", data.diff);
    }

    #[test]
    fn gitattributes_does_not_hide_a_staged_secret() {
        let dir = init_repo("attr-staged");
        write_file(&dir, ".gitattributes", b"* binary\n");
        let token = fake_github_token();
        write_file(&dir, "docs/notes.md", format!("token = {token}\n").as_bytes());
        run_git(&dir, &["add", "--", "docs/notes.md"]);
        let (data, _paths) = Repo { dir: dir.clone() }.staged().unwrap();
        assert!(data.diff.contains(&token), "the staged patch hides the added line: {}", data.diff);
    }

    #[test]
    fn info_attributes_file_does_not_hide_a_secret() {
        let dir = init_repo("attr-info");
        write_file(&dir, ".git/info/attributes", b"* -diff\n");
        let token = fake_github_token();
        let id = commit_file(&dir, "docs/notes.md", format!("token = {token}\n").as_bytes());
        let data = Repo { dir: dir.clone() }.commit_data(&id).unwrap();
        assert!(data.diff.contains(&token), "the patch hides the added line: {}", data.diff);
    }

    #[test]
    fn user_attributes_file_does_not_hide_a_secret() {
        let dir = init_repo("attr-user");
        let rules_dir = temp_dir("attr-user-rules");
        write_file(&rules_dir, "attributes", b"* -diff\n");
        let rules = rules_dir.join("attributes").to_string_lossy().replace('\\', "/");
        run_git(&dir, &["config", "core.attributesFile", rules.as_str()]);
        let token = fake_github_token();
        let id = commit_file(&dir, "docs/notes.md", format!("token = {token}\n").as_bytes());
        let data = Repo { dir: dir.clone() }.commit_data(&id).unwrap();
        assert!(data.diff.contains(&token), "the patch hides the added line: {}", data.diff);
    }

    #[test]
    fn gitleaks_runs_with_attribute_rules_switched_off() {
        // gitleaks reads history through its own `git log -p`; the child git inherits this environment.
        for mode in [Mode::Staged, Mode::Range("a..b".to_string())] {
            let cmd = gitleaks_command(
                Path::new("gitleaks.exe"),
                Path::new("D:\\repo"),
                &mode,
                Path::new("D:\\empty"),
                Path::new("D:\\report.json"),
            );
            let source = cmd
                .get_envs()
                .find(|(key, _)| *key == OsStr::new("GIT_ATTR_SOURCE"))
                .and_then(|(_, value)| value);
            assert_eq!(source, Some(OsStr::new("4b825dc642cb6eb9a060e54bf8d69288fbee4904")), "{mode:?}");
        }
    }

    #[test]
    fn untracked_gitattributes_is_refused() {
        let dir = init_repo("attr-untracked");
        commit_file(&dir, "docs/a.md", b"fine\n");
        write_file(&dir, ".gitattributes", b"* -diff\n");
        for mode in [Mode::Staged, Mode::Range("HEAD".to_string())] {
            assert!(own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &mode).is_err(), "{mode:?}");
        }
    }

    #[test]
    fn changed_gitattributes_in_the_working_tree_is_refused() {
        let dir = init_repo("attr-changed");
        commit_file(&dir, ".gitattributes", b"* text=auto\n");
        write_file(&dir, ".gitattributes", b"* -diff\n");
        for mode in [Mode::Staged, Mode::Range("HEAD".to_string())] {
            assert!(own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &mode).is_err(), "{mode:?}");
        }
    }

    #[test]
    fn committed_root_gitattributes_is_accepted() {
        let dir = init_repo("attr-root");
        commit_file(&dir, ".gitattributes", b"* text=auto\n");
        commit_file(&dir, "docs/a.md", b"fine\n");
        let findings = findings_of(&dir, &repo_cfg());
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn gitattributes_below_a_wildcard_entry_is_not_admitted() {
        let c = repo_cfg();
        assert!(c.path_allowed(".gitattributes"));
        assert!(!c.path_allowed("docs/.gitattributes"), "docs/** must not admit an attributes file");
        assert!(matches!(
            scan_commit(&c, &commit_with_paths(&["docs/.gitattributes"]))[..],
            [Finding::Path { .. }]
        ));
    }

    // I1: UTF-16 text and binary files.

    fn utf16_bytes(text: &str, little_endian: bool, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            let mark: [u8; 2] = if little_endian { [0xFF, 0xFE] } else { [0xFE, 0xFF] };
            out.extend_from_slice(&mark);
        }
        for unit in text.encode_utf16() {
            let pair: [u8; 2] = if little_endian { unit.to_le_bytes() } else { unit.to_be_bytes() };
            out.extend_from_slice(&pair);
        }
        out
    }

    fn utf16_sample() -> String {
        format!("saved in {}\nsee ACME-Internal notes\n", ["C:", "Users", "sample-user", "AppData"].join("\\"))
    }

    fn assert_utf16_is_scanned(tag: &str, bytes: &[u8]) {
        let dir = init_repo(tag);
        commit_file(&dir, "docs/notes.txt", bytes);
        let findings = findings_of(&dir, &repo_cfg());
        assert!(findings.iter().any(|f| matches!(f, Finding::ProfilePath { .. })), "profile path not found: {findings:?}");
        assert!(findings.iter().any(|f| matches!(f, Finding::Term { n: 0, .. })), "banned term not found: {findings:?}");
    }

    #[test]
    fn utf16le_text_with_bom_is_decoded_and_scanned() {
        assert_utf16_is_scanned("utf16-le-bom", &utf16_bytes(&utf16_sample(), true, true));
    }

    #[test]
    fn utf16le_text_without_bom_is_decoded_and_scanned() {
        assert_utf16_is_scanned("utf16-le", &utf16_bytes(&utf16_sample(), true, false));
    }

    #[test]
    fn utf16be_text_with_bom_is_decoded_and_scanned() {
        assert_utf16_is_scanned("utf16-be-bom", &utf16_bytes(&utf16_sample(), false, true));
    }

    #[test]
    fn utf16be_text_without_bom_is_decoded_and_scanned() {
        assert_utf16_is_scanned("utf16-be", &utf16_bytes(&utf16_sample(), false, false));
    }

    #[test]
    fn text_with_zero_bytes_between_letters_is_still_scanned() {
        let path = ["C:", "Users", "sample-user"].join("\\");
        let spaced: String = path.chars().flat_map(|c| [c, '\0']).collect();
        assert!(scan_text(&cfg(), &spaced, "f").iter().any(|f| matches!(f, Finding::ProfilePath { .. })));
    }

    const PNG_HEAD: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x06\0\0\0\x1f\xf3\xff";

    #[test]
    fn renamed_binary_under_a_wildcard_entry_is_reported() {
        // A PNG renamed to `.dat` is no media file by its extension, so `docs/**` admitted it unseen.
        let dir = init_repo("binary-dat");
        commit_file(&dir, "docs/shot.dat", PNG_HEAD);
        let findings = findings_of(&dir, &repo_cfg());
        assert!(!findings.is_empty(), "a binary file under docs/** went through unseen");
    }

    #[test]
    fn binary_file_with_an_exact_allow_entry_passes() {
        let dir = init_repo("binary-listed");
        commit_file(&dir, "docs/shot.dat", PNG_HEAD);
        let cfg = ScanConfig::for_test(
            &["acme-internal", "word:zq"],
            &["docs/**", ".gitattributes", "docs/shot.dat"],
            &noreply(),
        );
        let findings = findings_of(&dir, &cfg);
        assert!(findings.is_empty(), "{findings:?}");
    }

    // I5 and M3: messages, tag texts, branch names and file names get the same text checks as file content.

    #[test]
    fn secret_in_a_commit_message_is_found() {
        let message = format!("feat: x\n\nerror log: {}\n\nSigned-off-by: P <{}>", fake_github_token(), noreply());
        assert!(has_secret(&scan_commit(&cfg(), &commit_with_message(&message))));
    }

    #[test]
    fn secret_in_an_annotated_tag_message_is_found() {
        let dir = init_repo("tag-secret");
        commit_file(&dir, "docs/a.md", b"fine\n");
        let message = format!("release {}", fake_github_token());
        run_git(&dir, &["tag", "-a", "v0.1", "-m", message.as_str()]);
        let findings = findings_of(&dir, &repo_cfg());
        assert!(has_secret(&findings), "{findings:?}");
    }

    #[test]
    fn secret_in_a_branch_or_file_name_is_found() {
        let token = fake_github_token();
        assert!(has_secret(&scan_names(&cfg(), &[format!("feature/{token}")])));
        let file = format!("docs/{token}.txt");
        assert!(has_secret(&scan_commit(&cfg(), &commit_with_paths(&[file.as_str()]))));
    }

    #[test]
    fn address_in_a_branch_or_file_name_is_found() {
        let a = addr("someone", "example.org");
        let in_branch = scan_names(&cfg(), &[format!("feature/{a}")]);
        assert!(in_branch.iter().any(|f| matches!(f, Finding::Email { .. })), "{in_branch:?}");
        let file = format!("docs/{a}.md");
        let in_file = scan_commit(&cfg(), &commit_with_paths(&[file.as_str()]));
        assert!(in_file.iter().any(|f| matches!(f, Finding::Email { .. })), "{in_file:?}");
    }

    #[test]
    fn token_shapes_are_found_in_any_text() {
        let pem = format!("-----BEGIN {}-----\nMIIEvQIBADANBgkq", ["RSA ", "PRIVATE KEY"].concat());
        let samples = [
            ("github", format!("token = {}", fake_github_token())),
            ("anthropic", format!("key: {}", fake_anthropic_key())),
            ("aws", format!("id {}{}{}", "AK", "IA", "IOSFODNN7EXAMPLE")),
            ("slack", format!("hook {}{}{}", "xo", "xb-", "1234567890-abcdefghij")),
            ("private key", pem),
        ];
        for (label, text) in &samples {
            assert!(has_secret(&scan_text(&cfg(), text, "f")), "{label}");
        }
    }

    #[test]
    fn token_prefixes_without_a_token_tail_pass() {
        for text in [
            "tokens start with ghp_ and sk-ant- here",
            "an AKIA prefix",
            "xoxb- tokens",
            "-----BEGIN CERTIFICATE-----",
            "AKIAfoo",
        ] {
            assert!(scan_text(&cfg(), text, "f").is_empty(), "{text}");
        }
    }

    // I7: the profile-path forms that Git Bash, MSYS, Cygwin, WSL and file URLs print.

    #[test]
    fn profile_paths_from_git_bash_wsl_unc_and_file_urls_stop() {
        let hits = [
            ["", "c", "Users", "someone", "x"].join("/"),
            format!("cd {}", ["", "c", "users", "someone"].join("/")),
            ["", "mnt", "c", "Users", "someone"].join("/"),
            ["", "cygdrive", "d", "Users", "someone"].join("/"),
            [concat!("\\\\wsl", "$"), "Ubuntu", "home", "someone"].join("\\"),
            ["\\\\wsl.localhost", "Ubuntu", "home", "someone"].join("\\"),
            ["file:", "", "", "home", "someone"].join("/"),
            ["file:", "", "", "C:", "Users", "someone"].join("/"),
            ["file:", "", "", "c%3A", "Users", "someone"].join("/"),
            ["\\\\host", "c$", "Users", "someone"].join("\\"),
            format!("copy {}", ["\\\\host", "D$", "users", "someone", "x"].join("\\")),
        ];
        let missed: Vec<&String> = hits.iter().filter(|text| !has_profile_path(text.as_str())).collect();
        assert!(missed.is_empty(), "not recognised: {missed:?}");
    }

    #[test]
    fn profile_path_look_alikes_still_pass() {
        let misses = [
            ["", "c", "Users"].join("/"),
            ["", "mnt", "c", "Users", "<name>"].join("/"),
            ["", "mnt", "c", "Program Files", "x"].join("/"),
            ["", "c", "Windows", "System32"].join("/"),
            ["crates", "app", "src", "c", "users", "mod.rs"].join("/"),
            [concat!("\\\\wsl", "$"), "Ubuntu", "etc", "hosts"].join("\\"),
            ["\\\\host", "c$", "Windows"].join("\\"),
            "see https://example.org/file.txt".to_string(),
            "the profile: /tmp/x".to_string(),
        ];
        let flagged: Vec<&String> = misses.iter().filter(|text| has_profile_path(text.as_str())).collect();
        assert!(flagged.is_empty(), "wrongly flagged: {flagged:?}");
    }

    // M12 and M2.

    #[test]
    fn empty_term_list_is_refused() {
        let dir = temp_dir("no-terms");
        std::fs::write(dir.join("terms.txt"), "# only a comment\n\n").unwrap();
        std::fs::write(dir.join("identity.toml"), format!("name = \"P\"\nnoreply = \"{}\"\n", noreply())).unwrap();
        assert!(ScanConfig::load(&dir, "docs/**\n").is_err(), "a term list without terms makes every term check vacuous");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_file_at_the_tip_is_reported() {
        let dir = init_repo("unreadable");
        commit_file(&dir, "docs/a.md", b"first\n");
        commit_file(&dir, "docs/b.md", b"second\n");
        // Remove the loose object of the first file; the range covers only the second commit, so only the
        // end-state scan has to read it.
        let blob = run_git(&dir, &["rev-parse", "HEAD:docs/a.md"]);
        let object = dir.join(".git").join("objects").join(&blob[..2]).join(&blob[2..]);
        let mut permissions = std::fs::metadata(&object).unwrap().permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&object, permissions).unwrap();
        std::fs::remove_file(&object).unwrap();
        let mode = Mode::Range("HEAD~1..HEAD".to_string());
        let findings = own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &mode).unwrap();
        assert!(!findings.is_empty(), "a file that cannot be read was skipped without a word");
    }

    // --- the helpers that came with the fixes ---

    #[test]
    fn git_version_text_is_parsed() {
        assert_eq!(parse_git_version("git version 2.55.0.windows.3"), Some((2, 55)));
        assert_eq!(parse_git_version("git version 2.39.5 (Apple Git-154)"), Some((2, 39)));
        assert_eq!(parse_git_version("git version 2.40.0\n"), Some((2, 40)));
        assert_eq!(parse_git_version("not git"), None);
        assert_eq!(parse_git_version(""), None);
        assert!(parse_git_version("git version 2.39.5").unwrap() < MIN_GIT_VERSION);
        assert!(parse_git_version("git version 2.40.0").unwrap() >= MIN_GIT_VERSION);
        assert!(parse_git_version("git version 3.0.0").unwrap() >= MIN_GIT_VERSION);
    }

    #[test]
    fn zero_bytes_make_a_file_binary_unless_it_is_clean_utf16() {
        assert!(matches!(classify(b"plain text\n"), Content::Text(t) if t == "plain text\n"));
        assert!(matches!(classify(PNG_HEAD), Content::Binary));
        assert!(matches!(classify(b"ab\0cd"), Content::Binary));
        assert!(matches!(classify(&[0, 0, 0, 0]), Content::Binary));
        assert!(matches!(classify(&utf16_bytes("hello\n", true, true)), Content::Text(t) if t == "hello\n"));
        assert!(matches!(classify(&utf16_bytes("hello\n", false, false)), Content::Text(t) if t == "hello\n"));
        // A byte-order mark followed by a lone surrogate is no clean UTF-16.
        assert!(matches!(classify(&[0xFF, 0xFE, 0x00, 0xD8]), Content::Binary));
    }

    #[test]
    fn binary_file_is_searched_for_readable_runs() {
        let mut bytes = PNG_HEAD.to_vec();
        bytes.extend_from_slice(b"\0see ACME-Internal notes\0");
        let found = scan_file(&repo_cfg(), "docs/shot.dat", &bytes, false);
        assert!(found.iter().any(|f| matches!(f, Finding::Term { n: 0, .. })), "{found:?}");
        assert!(found.iter().any(|f| matches!(f, Finding::Binary { .. })), "{found:?}");
    }


    // --- RL-05 (IMP-13, MC-494): the release scan refuses imported content ---

    /// The rule name of every finding: the name of its variant in kebab case (`ImportedPage` reads `imported-page`).
    fn rules_of(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .map(|finding| {
                let debug = format!("{finding:?}");
                let name: String = debug.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                let mut rule = String::new();
                for (i, c) in name.chars().enumerate() {
                    if c.is_ascii_uppercase() && i > 0 {
                        rule.push('-');
                    }
                    rule.push(c.to_ascii_lowercase());
                }
                rule
            })
            .collect()
    }

    /// A page as the import writes it (`render_page_with` of the knowledge store): the known keys, `parent` and `source`,
    /// the closing line, then the text.
    const IMPORTED_PAGE: &str = "+++\nid = \"0190f3a2-7b1c-7d2e-8a4f-5b6c7d8e9f01\"\ntitle = \"Plan\"\nowner = \"person\"\ncreated = \"2026-11-01\"\nupdated = \"2026-11-01\"\ntags = []\nread_only_for_roles = false\nparent = \"0190f3a2-7b1c-7d2e-8a4f-5b6c7d8e9f00\"\nsource = \"notion:page:ab\"\n+++\nThe text of the page.\n";

    #[test]
    fn an_imported_page_header_is_a_finding() {
        assert_eq!(
            rules_of(&scan_text(&cfg(), "+++\ntitle = \"x\"\nsource = \"notion:page:ab\"\n+++\n", "docs/x.md")),
            vec!["imported-page"]
        );
        assert!(scan_text(&cfg(), "+++\ntitle = \"x\"\n+++\n", "docs/x.md").is_empty());
    }

    #[test]
    fn a_knowledge_files_path_is_a_finding() {
        assert_eq!(rules_of(&scan_names(&cfg(), &["docs/knowledge/.files/ab.png".into()])), vec!["imported-file"]);
        assert!(scan_names(&cfg(), &["assets/.files/ab.png".into()]).is_empty());
    }

    #[test]
    fn an_imported_page_is_found_as_the_import_writes_it_and_as_a_hand_edit_may_leave_it() {
        let found = |text: &str| rules_of(&scan_text(&cfg(), text, "docs/knowledge/plan-ab12cd34.md"));
        assert_eq!(found(IMPORTED_PAGE), vec!["imported-page"]);
        assert_eq!(found(&IMPORTED_PAGE.replace('\n', "\r\n")), vec!["imported-page"]);
        assert_eq!(found(&format!("\u{feff}{IMPORTED_PAGE}")), vec!["imported-page"]);
        for line in ["source=\"x\"", "  source   =  \"x\"", "\"source\" = \"x\"", "'source' = 'x'", "source = 7"] {
            assert_eq!(found(&format!("+++\ntitle = \"x\"\n{line}\n+++\nbody\n")), vec!["imported-page"], "{line}");
        }
    }

    #[test]
    fn text_that_is_not_an_imported_page_header_is_no_finding() {
        let clean = |text: &str| scan_text(&cfg(), text, "docs/knowledge/plan-ab12cd34.md").is_empty();
        // a page of the person's own, a `source` key below the closing line, other keys, and no header at all
        assert!(clean("+++\ntitle = \"x\"\n+++\nsource = \"notion:page:ab\"\n"));
        assert!(clean("+++\ntitle = \"x\"\nsource_url = \"x\"\nsources = []\nresource = 1\n# source = \"x\"\n+++\nbody\n"));
        assert!(clean("# Notes\n\n+++\nsource = \"x\"\n+++\n"));
        assert!(clean("source = \"notion:page:ab\"\n"));
        // only the text of a Markdown file is a page: a message or a tag text that quotes a header is none
        assert!(scan_text(&cfg(), IMPORTED_PAGE, "01234567:message").is_empty());
        assert!(scan_text(&cfg(), IMPORTED_PAGE, "tag v1").is_empty());
        // the place of a finding is a path, a commit and a path, or a path in capitals (a case-insensitive file system)
        assert_eq!(rules_of(&scan_text(&cfg(), IMPORTED_PAGE, "01234567:docs/x.md")), vec!["imported-page"]);
        assert_eq!(rules_of(&scan_text(&cfg(), IMPORTED_PAGE, "docs/X.MD")), vec!["imported-page"]);
    }

    #[test]
    fn only_a_files_folder_below_a_knowledge_folder_is_imported_content() {
        let found = |path: &str| rules_of(&scan_names(&cfg(), &[path.to_string()]));
        for path in [
            "knowledge/.files/ab",
            "x/knowledge/.files/ab.png",
            "x/Knowledge/.FILES/ab",
            "x\\knowledge\\.files\\ab",
            "knowledge/notes/.files/ab",
        ] {
            assert_eq!(found(path), vec!["imported-file"], "{path}");
        }
        for path in [".files/ab", "knowledge/files/ab", "knowledge/notes.md", "assets/.files/knowledge", "knowledge.files/ab", "x/.files/knowledge/ab"] {
            assert!(found(path).is_empty(), "{path}");
        }
    }

    #[test]
    fn a_commit_that_adds_an_imported_page_or_file_is_refused() {
        let added: String = IMPORTED_PAGE.lines().map(|line| format!("+{line}\n")).collect();
        let diff = format!("diff --git a/docs/x.md b/docs/x.md\nnew file mode 100644\n--- /dev/null\n+++ b/docs/x.md\n@@ -0,0 +1,12 @@\n{added}");
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_diff(&diff))), vec!["imported-page"]);
        // `docs/**` admits the path of a file without an extension, so only this rule stops it
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_paths(&["docs/knowledge/.files/ab"]))), vec!["imported-file"]);
    }

    /// A new-file patch of `IMPORTED_PAGE` with the given header lines, as `git show -U0` writes it.
    fn new_page_patch(diff_line: &str, plus_line: &str) -> String {
        let added: String = IMPORTED_PAGE.lines().map(|line| format!("+{line}\n")).collect();
        format!("{diff_line}\nnew file mode 100644\n--- /dev/null\n{plus_line}\n@@ -0,0 +1,12 @@\n{added}")
    }

    #[test]
    fn an_imported_page_is_found_in_a_patch_whose_file_name_git_wrote_quoted() {
        // `core.quotePath` (on by default) makes git write a name with a byte above 0x7F in quotes and octal escapes:
        // the place of the patch then ends in a closing quote and not in `.md`
        let name = "docs/knowledge/\\303\\274bersicht-ab12cd34.md";
        let diff = new_page_patch(&format!("diff --git \"a/{name}\" \"b/{name}\""), &format!("+++ \"b/{name}\""));
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_diff(&diff))), vec!["imported-page"]);
        // a quoted name of another kind of file is still no page
        let other = "docs/\\303\\274bersicht.txt";
        let diff = new_page_patch(&format!("diff --git \"a/{other}\" \"b/{other}\""), &format!("+++ \"b/{other}\""));
        assert!(scan_commit(&cfg(), &commit_with_diff(&diff)).is_empty());
        // and so is a quoted name that only has `.md` inside it (a file named `x.md"`)
        let inner = "docs/x.md\\\"";
        let diff = new_page_patch(&format!("diff --git \"a/{inner}\" \"b/{inner}\""), &format!("+++ \"b/{inner}\""));
        assert!(scan_commit(&cfg(), &commit_with_diff(&diff)).is_empty());
    }

    #[test]
    fn an_imported_page_is_found_in_a_patch_whose_file_name_git_ended_with_a_tab() {
        // git adds a TAB after the name on the `---` and `+++` lines when the name holds a space
        let diff = new_page_patch("diff --git a/docs/my notes.md b/docs/my notes.md", "+++ b/docs/my notes.md\t");
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_diff(&diff))), vec!["imported-page"]);
        let diff = new_page_patch("diff --git a/docs/my notes.txt b/docs/my notes.txt", "+++ b/docs/my notes.txt\t");
        assert!(scan_commit(&cfg(), &commit_with_diff(&diff)).is_empty());
    }

    #[test]
    fn a_range_finds_an_imported_page_whose_name_git_quotes_or_ends_with_a_tab_even_when_a_later_commit_removes_it() {
        // a German title gives a name with a letter above 0x7F (`knowledge::page::file_name_for`); a name with a space gets a TAB
        for (tag, name) in [("umlaut", "docs/knowledge/\u{fc}bersicht-ab12cd34.md"), ("space", "docs/my notes.md")] {
            let dir = init_repo(&format!("imported-name-{tag}"));
            commit_file(&dir, "docs/a.md", b"fine\n");
            commit_file(&dir, name, IMPORTED_PAGE.as_bytes());
            run_git(&dir, &["rm", "-q", "--", name]);
            let message = format!("test: remove it\n\nSigned-off-by: P <{}>", noreply());
            run_git(&dir, &["commit", "-q", "-m", message.as_str()]);
            // the tip no longer has the page: only the patch of the commit that added it can tell
            let rules = rules_of(&findings_of(&dir, &repo_cfg()));
            assert!(rules.iter().any(|r| r == "imported-page"), "{tag}: {rules:?}");
            // the removal alone has nothing of it
            let mode = Mode::Range("HEAD~1..HEAD".to_string());
            let findings = own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &mode).unwrap();
            assert!(findings.is_empty(), "{tag}: {findings:?}");
        }
    }

    #[test]
    fn a_range_with_an_imported_page_or_file_is_refused_even_when_the_tip_has_them_no_more() {
        let dir = init_repo("imported-gone");
        commit_file(&dir, "docs/a.md", b"fine\n");
        commit_file(&dir, "docs/knowledge/plan-ab12cd34.md", IMPORTED_PAGE.as_bytes());
        commit_file(&dir, &format!("docs/knowledge/.files/{}", "ab".repeat(32)), b"private file bytes\n");
        // in the tree: the end state and the commits both say so
        let rules = rules_of(&findings_of(&dir, &repo_cfg()));
        assert!(rules.iter().any(|r| r == "imported-page") && rules.iter().any(|r| r == "imported-file"), "{rules:?}");
        // removed again: the commits that added them are still in the range
        run_git(&dir, &["rm", "-q", "-r", "--", "docs/knowledge"]);
        let message = format!("test: remove them\n\nSigned-off-by: P <{}>", noreply());
        run_git(&dir, &["commit", "-q", "-m", message.as_str()]);
        let rules = rules_of(&findings_of(&dir, &repo_cfg()));
        assert!(rules.iter().any(|r| r == "imported-page") && rules.iter().any(|r| r == "imported-file"), "{rules:?}");
        // the range of the removal alone has nothing of them
        let mode = Mode::Range("HEAD~1..HEAD".to_string());
        let findings = own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &mode).unwrap();
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn an_imported_page_saved_as_utf16_is_found_at_the_end_state() {
        for (little_endian, bom) in [(true, true), (false, true), (true, false), (false, false)] {
            let found = scan_file(&repo_cfg(), "docs/p.md", &utf16_bytes(IMPORTED_PAGE, little_endian, bom), false);
            assert_eq!(rules_of(&found), vec!["imported-page"], "little endian {little_endian}, byte order mark {bom}");
        }
    }

    #[test]
    fn the_imported_content_lines_name_the_place_and_never_show_a_term() {
        let page = render(&cfg(), &Finding::ImportedPage { at: "docs/acme-internal/x.md".to_string() });
        assert!(page.contains("imported page") && page.contains("<term 1>"), "{page}");
        assert!(!page.to_lowercase().contains("acme-internal"), "{page}");
        let file = render(&cfg(), &Finding::ImportedFile { path: "knowledge/.files/ab".to_string() });
        assert!(file.contains("imported file") && file.contains("knowledge/.files/ab"), "{file}");
    }

    // --- the audit of 2026-10-23: replacement objects (M9) and the rules of a push (H4) ---

    #[test]
    fn replace_ref_does_not_hide_a_secret() {
        // `git replace` makes git show the scan another commit than the one the push sends: the original is in the
        // pack, the replacement is what `git log` and `git show` print.
        let dir = init_repo("replace-hides");
        let root = commit_file(&dir, "docs/a.md", b"fine\n");
        let token = fake_github_token();
        let leaking = commit_file(&dir, "docs/notes.md", format!("token = {token}\n").as_bytes());
        let tree = run_git(&dir, &["rev-parse", format!("{root}^{{tree}}").as_str()]);
        let clean = run_git(&dir, &["commit-tree", tree.as_str(), "-p", root.as_str(), "-m", "clean"]);
        run_git(&dir, &["replace", leaking.as_str(), clean.as_str()]);
        let data = Repo { dir: dir.clone() }.commit_data(&leaking).unwrap();
        assert!(data.diff.contains(&token), "the patch shows the replacement, not the commit: {}", data.diff);
    }

    #[test]
    fn replace_refs_are_refused() {
        let dir = init_repo("replace-refused");
        let first = commit_file(&dir, "docs/a.md", b"a\n");
        let second = commit_file(&dir, "docs/b.md", b"b\n");
        let range = Mode::Range("HEAD".to_string());
        assert!(own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &range).is_ok());
        run_git(&dir, &["replace", second.as_str(), first.as_str()]);
        for mode in [Mode::Staged, Mode::Range("HEAD".to_string())] {
            assert!(own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &mode).is_err(), "{mode:?}");
        }
        run_git(&dir, &["replace", "-d", second.as_str()]);
        assert!(own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &range).is_ok());
        write_file(&dir, ".git/info/grafts", format!("{second} {first}\n").as_bytes());
        assert!(own_checks(&repo_cfg(), &Repo { dir: dir.clone() }, &range).is_err(), "a grafts file");
    }

    #[test]
    fn gitleaks_runs_with_replacement_objects_switched_off() {
        // gitleaks reads history through its own `git log -p`, which inherits this environment.
        for mode in [Mode::Staged, Mode::Range("a..b".to_string())] {
            let cmd = gitleaks_command(
                Path::new("gitleaks.exe"),
                Path::new("D:\\repo"),
                &mode,
                Path::new("D:\\empty"),
                Path::new("D:\\report.json"),
            );
            let replace = cmd
                .get_envs()
                .find(|(key, _)| *key == OsStr::new("GIT_NO_REPLACE_OBJECTS"))
                .and_then(|(_, value)| value);
            assert_eq!(replace, Some(OsStr::new("1")), "{mode:?}");
        }
    }

    const PINNED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// The scan settings folder of a test: one term and the project identity.
    fn scan_settings_dir(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        write_file(&dir, "terms.txt", b"acme-internal\n");
        write_file(&dir, "identity.toml", format!("name = \"P\"\nnoreply = \"{}\"\n", noreply()).as_bytes());
        dir
    }

    fn options_with(range: &str, rules_from: Option<&str>, repo: &Path) -> Options {
        Options {
            mode: Mode::Range(range.to_string()),
            config: None,
            allow: None,
            rules_from: rules_from.map(str::to_string),
            accept_judge_change: None,
            repo: repo.to_path_buf(),
        }
    }

    #[test]
    fn push_mode_reads_allow_list_from_the_trusted_commit() {
        // The commits that are pushed widen the allow list to everything and add a path only the wide list admits:
        // they must not approve their own push.
        let dir = init_repo("rules-from");
        commit_file(&dir, "scan/allowed-paths.txt", b"docs/**\nscan/**\nsupply-chain/**\n");
        let base = commit_file(&dir, "supply-chain/pins.toml", format!("[tool.gitleaks]\nexe_sha256 = \"{PINNED}\"\n").as_bytes());
        commit_file(&dir, "scan/allowed-paths.txt", b"**\n");
        let tip = commit_file(&dir, "private/x.txt", b"x\n");
        let range = format!("{base}..{tip}");
        let repo = Repo { dir: dir.clone() };
        let settings = scan_settings_dir("rules-from-config");

        // Without a trusted commit the allow list is the one of HEAD, written by the pushed commits. (Only a staged
        // scan may still read it so; a range scan refuses, see `a_range_scan_without_rules_from_refuses_to_run`.)
        let staged = Options {
            mode: Mode::Staged,
            config: None,
            allow: None,
            rules_from: None,
            accept_judge_change: None,
            repo: dir.clone(),
        };
        let (own_allow, _) = rule_texts(&repo, &staged).unwrap();
        assert!(own_allow.lines().any(|l| l.trim() == "**"));
        let wide = ScanConfig::load(&settings, &own_allow).unwrap();
        let admitted = own_checks(&wide, &repo, &Mode::Range(range.clone())).unwrap();
        assert!(admitted.iter().all(|f| !matches!(f, Finding::Path { .. })), "{admitted:?}");

        // From the trusted commit it is the narrow list, and the pin is the one of that commit.
        let (allow, pins) = rule_texts(&repo, &options_with(&range, Some(&base), &dir)).unwrap();
        assert!(allow.lines().any(|l| l.trim() == "docs/**") && !allow.lines().any(|l| l.trim() == "**"), "{allow}");
        assert_eq!(pinned_exe_sha256(&pins).as_deref(), Some(PINNED));
        let narrow = ScanConfig::load(&settings, &allow).unwrap();
        let stopped = own_checks(&narrow, &repo, &Mode::Range(range)).unwrap();
        assert!(stopped.iter().any(|f| matches!(f, Finding::Path { path } if path == "private/x.txt")), "{stopped:?}");
    }

    #[test]
    fn the_trusted_commit_must_hold_both_files_and_nothing_falls_back_to_the_pushed_tree() {
        let dir = init_repo("rules-from-missing");
        let none = commit_file(&dir, "docs/a.md", b"a\n");
        let allow_only = commit_file(&dir, "scan/allowed-paths.txt", b"docs/**\n");
        commit_file(&dir, "supply-chain/pins.toml", format!("[tool.gitleaks]\nexe_sha256 = \"{PINNED}\"\n").as_bytes());
        let repo = Repo { dir: dir.clone() };
        let from = |id: &str| rule_texts(&repo, &options_with("HEAD", Some(id), &dir));
        // HEAD holds both files, the trusted commits do not: no fallback to HEAD.
        assert!(from(&none).is_err());
        assert!(from(&allow_only).is_err(), "pins.toml is only in HEAD");
        // A working-tree file does not count either.
        write_file(&dir, "supply-chain/pins.toml", b"[tool.gitleaks]\n");
        assert!(from(&allow_only).is_err());
        // The id of a tree, and an id that names nothing, are no commits.
        let tree = run_git(&dir, &["rev-parse", "HEAD^{tree}"]);
        assert!(from(&tree).is_err());
        assert!(from("0123456789abcdef0123456789abcdef01234567").is_err());
    }

    #[test]
    fn a_range_scan_without_rules_from_refuses_to_run() {
        // A launcher that does not pass --rules-from, next to this scanner, must not judge a push by the allow list and
        // the pin of the commits it sends: H4 allows no fallback, and a mismatch of gate and scanner fails closed.
        let dir = init_repo("rules-flag-missing");
        commit_file(&dir, "scan/allowed-paths.txt", b"docs/**\n");
        let tip = commit_file(&dir, "supply-chain/pins.toml", format!("[tool.gitleaks]\nexe_sha256 = \"{PINNED}\"\n").as_bytes());
        let repo = Repo { dir: dir.clone() };
        assert!(rule_texts(&repo, &options_with(&format!("{tip}..{tip}"), None, &dir)).is_err());
        assert!(rule_texts(&repo, &options_with(&tip, None, &dir)).is_err());
        // A range scan with an --allow file but no trusted commit would read the pin from HEAD (N2 of the re-review).
        let list = dir.join("own-list.txt");
        std::fs::write(&list, "docs/**\n").unwrap();
        let own = Options { allow: Some(list), ..options_with(&tip, None, &dir) };
        assert!(rule_texts(&repo, &own).is_err(), "--allow does not stand in for --rules-from");
        // A scan of the staged changes still reads HEAD.
        let staged = Options {
            mode: Mode::Staged,
            config: None,
            allow: None,
            rules_from: None,
            accept_judge_change: None,
            repo: dir.clone(),
        };
        assert!(rule_texts(&repo, &staged).is_ok());
        // With the trusted commit it runs.
        assert!(rule_texts(&repo, &options_with(&format!("{tip}..{tip}"), Some(&tip), &dir)).is_ok());
    }

    #[test]
    fn rules_from_takes_a_full_commit_id_and_not_next_to_allow() {
        let id = "0123456789abcdef0123456789abcdef01234567";
        let o = parse_args(&args(&["--range", "a..b", "--rules-from", id])).unwrap();
        assert_eq!(o.rules_from.as_deref(), Some(id));
        assert_eq!(parse_args(&args(&["--staged"])).unwrap().rules_from, None);
        for bad in ["HEAD", "0123abcd", "origin/main", "0123456789abcdef0123456789abcdef0123456z"] {
            assert!(parse_args(&args(&["--range", "a..b", "--rules-from", bad])).is_err(), "{bad}");
        }
        assert!(parse_args(&args(&["--range", "a..b", "--rules-from"])).is_err());
        assert!(parse_args(&args(&["--range", "a..b", "--rules-from", id, "--allow", "l.txt"])).is_err());
    }

    // --- security amendment B1: a change to a judge needs the person's named acceptance ---

    #[test]
    fn judge_code_and_judge_input_are_found_by_path() {
        let rules = |paths: &[&str]| judge_rules_changed(&paths.iter().map(|p| p.to_string()).collect::<Vec<String>>());
        assert!(rules(&["docs/a.md", "crates/skipper-core/src/lib.rs", "README.md"]).is_empty());
        assert_eq!(rules(&["xtask/src/leak.rs"]), vec!["xtask/**"]);
        assert_eq!(rules(&["tools/skipper-cap/main.rs"]), vec!["tools/**"]);
        assert_eq!(rules(&["scan/allowed-paths.txt"]), vec!["scan/**"]);
        assert_eq!(rules(&["supply-chain/pins.toml"]), vec!["supply-chain/**"]);
        assert_eq!(rules(&[".cargo/config.toml"]), vec![".cargo/**"]);
        assert_eq!(rules(&["rust-toolchain.toml"]), vec!["rust-toolchain.toml"]);
        assert_eq!(rules(&["Cargo.lock"]), vec!["Cargo.lock"]);
        assert_eq!(rules(&["Cargo.toml"]), vec!["Cargo.toml"]);
        assert_eq!(rules(&["crates/skipper-core/Cargo.toml"]), vec!["Cargo.toml"]);
        // A checkout on Windows ignores letter case, so a path in another case is the same file there.
        assert_eq!(rules(&["XTask/src/x.rs"]), vec!["xtask/**"]);
        // Several paths: one entry per rule, in a fixed order.
        assert_eq!(rules(&["xtask/a.rs", "xtask/b.rs", "Cargo.lock", "scan/x"]), vec!["Cargo.lock", "scan/**", "xtask/**"]);
        // Look-alikes are no judge paths.
        let look_alikes = ["docs/xtask/notes.md", "my-tools/x", "crates/x/src/cargo_toml.rs", "docs/scan.md", "tools.md", "crates/x/Cargo.toml.bak"];
        assert!(rules(&look_alikes).is_empty());
    }

    #[test]
    fn a_range_that_changes_a_judge_is_a_finding_unless_its_tip_is_accepted() {
        let dir = init_repo("judge-change");
        commit_file(&dir, "scan/allowed-paths.txt", b"docs/**\n");
        let base = commit_file(&dir, "docs/a.md", b"a\n");
        let repo = Repo { dir: dir.clone() };
        // A range of documents is not touched.
        let docs = commit_file(&dir, "docs/b.md", b"b\n");
        assert_eq!(judge_change(&repo, &format!("{base}..{docs}"), None).unwrap(), (Vec::new(), None));
        // A judge changes: a finding, no acceptance.
        let judge = commit_file(&dir, "xtask/src/x.rs", b"fn x() {}\n");
        let range = format!("{base}..{judge}");
        let (found, note) = judge_change(&repo, &range, None).unwrap();
        assert!(matches!(&found[..], [Finding::JudgeChange { rules }] if rules == "xtask/**"), "{found:?}");
        assert_eq!(note, None);
        // The person names the range tip: it passes, and says so.
        let (found, note) = judge_change(&repo, &range, Some(&judge)).unwrap();
        assert!(found.is_empty(), "{found:?}");
        assert!(note.as_deref().is_some_and(|n| n.contains("xtask/**")), "{note:?}");
        // Another commit than the tip does not count.
        let (found, _) = judge_change(&repo, &range, Some(&docs)).unwrap();
        assert!(matches!(&found[..], [Finding::JudgeChange { .. }]), "{found:?}");
    }

    #[test]
    fn a_deleted_judge_file_and_a_judge_change_that_is_undone_inside_the_range_count() {
        let dir = init_repo("judge-delete");
        commit_file(&dir, "scan/allowed-paths.txt", b"docs/**\n");
        let base = commit_file(&dir, "docs/a.md", b"a\n");
        let repo = Repo { dir: dir.clone() };
        run_git(&dir, &["rm", "-q", "--", "scan/allowed-paths.txt"]);
        run_git(&dir, &["commit", "-q", "-m", "drop the list"]);
        let deleted = run_git(&dir, &["rev-parse", "HEAD"]);
        let (found, _) = judge_change(&repo, &format!("{base}..{deleted}"), None).unwrap();
        assert!(matches!(&found[..], [Finding::JudgeChange { rules }] if rules == "scan/**"), "{found:?}");

        let other = init_repo("judge-undone");
        let start = commit_file(&other, "docs/a.md", b"a\n");
        commit_file(&other, "xtask/x.rs", b"fn x() {}\n");
        run_git(&other, &["rm", "-q", "--", "xtask/x.rs"]);
        run_git(&other, &["commit", "-q", "-m", "undo"]);
        let tip = run_git(&other, &["rev-parse", "HEAD"]);
        // The tip holds no trace of the change, but a commit of the range made it.
        let (found, _) = judge_change(&Repo { dir: other.clone() }, &format!("{start}..{tip}"), None).unwrap();
        assert!(matches!(&found[..], [Finding::JudgeChange { .. }]), "{found:?}");
    }

    #[test]
    fn accept_judge_change_takes_a_full_commit_id_and_only_with_a_range() {
        let id = "0123456789abcdef0123456789abcdef01234567";
        let o = parse_args(&args(&["--range", "a..b", "--accept-judge-change", id])).unwrap();
        assert_eq!(o.accept_judge_change.as_deref(), Some(id));
        assert_eq!(parse_args(&args(&["--range", "a..b"])).unwrap().accept_judge_change, None);
        for bad in ["HEAD", "0123abcd", "origin/main", "0123456789abcdef0123456789abcdef0123456z"] {
            assert!(parse_args(&args(&["--range", "a..b", "--accept-judge-change", bad])).is_err(), "{bad}");
        }
        assert!(parse_args(&args(&["--range", "a..b", "--accept-judge-change"])).is_err());
        assert!(parse_args(&args(&["--staged", "--accept-judge-change", id])).is_err());
    }

    // --- security amendment G1: GitHub's merge commit passes only in its exact shape ---

    #[test]
    fn the_subject_of_a_github_merge_is_the_pull_request_form_and_nothing_else() {
        for ok in [
            "Merge pull request #5 from owner/topic",
            "Merge pull request #123 from some-owner/feature/x-y.z\n\nAdd the thing",
        ] {
            assert!(github_merge_subject_ok(ok), "{ok:?}");
        }
        for bad in [
            "",
            "Merge branch 'topic'",
            "merge pull request #5 from owner/topic",
            "Merge pull request # from owner/topic",
            "Merge pull request #5a from owner/topic",
            "Merge pull request #5 from owner",
            "Merge pull request #5 from /topic",
            "Merge pull request #5 from owner/",
            "Merge pull request #5 from own er/topic",
            "Merge pull request #5 from owner/top ic",
            "Merge pull request #5 from owner/topic and more",
            "Add a file\n\nMerge pull request #5 from owner/topic",
        ] {
            assert!(!github_merge_subject_ok(bad), "{bad:?}");
        }
    }

    /// Runs git with GitHub's own committer identity, as its merge button does.
    fn run_git_as_github(dir: &Path, args: &[&str]) -> String {
        let github = ["noreply", "github.com"].join("@");
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=P", "-c", format!("user.email={}", noreply()).as_str(), "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", std::env::temp_dir().join("skipper-xtask-leak-no-global-config"))
            .env("GIT_COMMITTER_NAME", "GitHub")
            .env("GIT_COMMITTER_EMAIL", github)
            .output()
            .expect("git starts");
        assert!(output.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A repository where a branch was merged with `--no-ff`: returns the folder and the commit before the branch.
    /// `message` is one paragraph per entry; `as_github` picks GitHub's committer identity; `extra_change` edits a
    /// file inside the merge commit (an "evil merge").
    fn repo_with_merged_branch(tag: &str, message: &[&str], as_github: bool, extra_change: bool) -> (PathBuf, String) {
        let dir = init_repo(tag);
        let root = commit_file(&dir, "docs/a.md", b"a\n");
        let main_branch = run_git(&dir, &["branch", "--show-current"]);
        run_git(&dir, &["checkout", "-q", "-b", "topic"]);
        commit_file(&dir, "docs/b.md", b"b\n");
        run_git(&dir, &["checkout", "-q", main_branch.as_str()]);
        let git = |args: &[&str]| if as_github { run_git_as_github(&dir, args) } else { run_git(&dir, args) };
        let mut paragraphs: Vec<&str> = Vec::new();
        for p in message {
            paragraphs.extend(["-m", *p]);
        }
        if extra_change {
            git(&["merge", "--no-ff", "--no-commit", "-q", "topic"]);
            write_file(&dir, "docs/a.md", b"a, edited inside the merge\n");
            git(&["add", "--", "docs/a.md"]);
            let mut commit = vec!["commit", "-q"];
            commit.extend(&paragraphs);
            git(&commit);
        } else {
            let mut merge = vec!["merge", "--no-ff", "-q"];
            merge.extend(&paragraphs);
            merge.push("topic");
            git(&merge);
        }
        (dir, root)
    }

    fn findings_of_merge(dir: &Path, root: &str) -> Vec<Finding> {
        own_checks(&repo_cfg(), &Repo { dir: dir.to_path_buf() }, &Mode::Range(format!("{root}..HEAD"))).unwrap()
    }

    #[test]
    fn github_merge_commit_in_its_exact_shape_passes_and_one_with_an_extra_change_stops() {
        let subject = "Merge pull request #5 from owner/topic";
        // The exact shape: two parents, GitHub as committer, the pull-request subject, nothing but the branch.
        let (dir, root) = repo_with_merged_branch("gh-merge-exact", &[subject, "Add b"], true, false);
        let found = findings_of_merge(&dir, &root);
        assert!(found.is_empty(), "{found:?}");

        // A change inside the merge commit that the branch does not hold.
        let (dir, root) = repo_with_merged_branch("gh-merge-evil", &[subject, "Add b"], true, true);
        assert!(!findings_of_merge(&dir, &root).is_empty(), "an evil merge passed");

        // Another subject.
        let (dir, root) = repo_with_merged_branch("gh-merge-subject", &["Merge branch 'topic'"], true, false);
        assert!(!findings_of_merge(&dir, &root).is_empty(), "another subject passed");

        // The same message from the project identity is an ordinary commit: it needs its sign-off.
        let (dir, root) = repo_with_merged_branch("gh-merge-own", &[subject, "Add b"], false, false);
        let found = findings_of_merge(&dir, &root);
        assert!(found.iter().any(|f| matches!(f, Finding::MissingSignOff { .. })), "{found:?}");

        // GitHub's shape does not excuse a trailer in the message.
        let (dir, root) = repo_with_merged_branch("gh-merge-trailer", &[subject, "Co-Authored-By: someone"], true, false);
        assert!(!findings_of_merge(&dir, &root).is_empty(), "a trailer passed");
    }

    // --- GATE-3: the gate refuses secret files and languages the project does not use ---
    //
    // The rule lives in this scan, not in GitHub's own rulesets or push protection, so
    // it holds wherever the repository is hosted. The tests below name every rule by the kebab-case name of its finding
    // (`rules_of`), so they hold the behaviour and not the shape of the finding types.

    /// A scan configuration from the text of an allow list, read the way `run` reads it from the rules commit
    /// (comment lines included: a banned-language path needs the reason in the line above its entry).
    fn cfg_with_allow_text(tag: &str, allow_text: &str) -> ScanConfig {
        let dir = temp_dir(tag);
        std::fs::write(dir.join("terms.txt"), "acme-internal\nword:zq\n").unwrap();
        std::fs::write(dir.join("identity.toml"), format!("name = \"P\"\nnoreply = \"{}\"\n", noreply())).unwrap();
        ScanConfig::load(&dir, allow_text).unwrap()
    }

    /// The rule names of the findings that one commit with these changed paths gets from `cfg`.
    fn rules_for_paths(cfg: &ScanConfig, paths: &[&str]) -> Vec<String> {
        rules_of(&scan_commit(cfg, &commit_with_paths(paths)))
    }

    /// The rule names of the findings that the end state of one file gets (its path and its bytes).
    fn rules_for_file(path: &str, bytes: &[u8]) -> Vec<String> {
        rules_of(&scan_file(&cfg(), path, bytes, false))
    }

    /// A new-file patch of `path` that adds `lines`, as `git show -U0` writes it.
    fn added_lines_patch(path: &str, lines: &[&str]) -> String {
        let added: String = lines.iter().map(|line| format!("+{line}\n")).collect();
        format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{added}",
            lines.len()
        )
    }

    #[test]
    fn a_file_whose_name_marks_a_secret_is_refused_even_below_a_wide_allow_entry() {
        // `crates/**` admits every one of these paths, so only the name rule can stop them.
        let names = [
            ".env",
            ".env.local",
            ".env.production",
            ".env.example.local",
            "prod.env",
            ".envrc",
            "server.pem",
            "tls.key",
            "store.pfx",
            "store.p12",
            "release.keystore",
            "release.jks",
            "id_rsa",
            "id_rsa.pub",
            "id_ed25519",
            "id_ed25519.bak",
            "id_ecdsa",
            "id_dsa",
            "credentials.json",
            "credentials-prod.json",
            ".credentials.json",
            "gcp-credentials.json",
            "secrets.json",
            "secrets.yaml",
            "secret.toml",
            ".mcp.json",
            "server.pem.bak",
        ];
        for name in names {
            let path = format!("crates/x/{name}");
            assert_eq!(rules_for_paths(&cfg(), &[path.as_str()]), vec!["secret-file"], "{path}");
        }
    }

    #[test]
    fn a_secret_name_is_read_the_way_a_windows_checkout_reads_it() {
        // letter case, a backslash as separator, and dots or blanks at the end of a name
        for path in [".ENV", "crates/x/.Env.Local", "crates/x/ID_RSA", "crates/x/Server.PEM", "crates/x/SECRETS.JSON", "crates\\x\\.env", "crates/x/.env.", "crates/x/.env "] {
            assert!(rules_for_paths(&cfg(), &[path]).iter().any(|r| r == "secret-file"), "{path}");
        }
    }

    #[test]
    fn a_secret_file_at_the_root_is_refused_twice_over() {
        // not on the allow list, and a secret name
        let rules = rules_for_paths(&cfg(), &[".env"]);
        assert!(rules.iter().any(|r| r == "path") && rules.iter().any(|r| r == "secret-file"), "{rules:?}");
    }

    #[test]
    fn names_that_only_look_like_a_secret_file_pass() {
        for path in [
            "crates/x/src/secret.rs",
            "crates/x/src/secrets.rs",
            "crates/x/src/key.rs",
            "crates/x/src/pem.rs",
            "crates/x/src/keystore.rs",
            "crates/x/src/secretary.rs",
            "crates/x/src/credentials.rs",
            "crates/x/src/credentials_store.rs",
            "crates/x/src/environment.rs",
            "crates/x/src/envelope.rs",
            "docs/env.md",
            "docs/.environment.md",
            "docs/dotenv.md",
            "docs/keys.md",
            "docs/secrets-policy.md",
            "docs/credentials-policy.md",
            "docs/.env.example",
        ] {
            assert!(scan_commit(&cfg(), &commit_with_paths(&[path])).is_empty(), "{path}");
        }
    }

    #[test]
    fn branch_and_tag_names_are_not_judged_as_files() {
        // The name rules are path rules: a branch or tag name that holds such a word is no secret file.
        let names = ["feature/secrets.rs-fix".to_string(), "fix/id_rsa-handling".to_string(), "topic/package.json-notes".to_string()];
        assert!(scan_names(&cfg(), &names).is_empty());
    }

    #[test]
    fn a_secret_file_is_found_in_every_commit_of_a_range_and_in_the_staged_changes() {
        let cfg = repo_cfg();
        // Added and removed again inside the range: the pushed history still holds it.
        let dir = init_repo("secret-history");
        commit_file(&dir, "docs/a.md", b"fine\n");
        commit_file(&dir, "docs/.env.local", b"API_KEY=abc\n");
        run_git(&dir, &["rm", "-q", "--", "docs/.env.local"]);
        run_git(&dir, &["commit", "-q", "-m", "drop it"]);
        let found = rules_of(&findings_of(&dir, &cfg));
        assert!(found.iter().any(|r| r == "secret-file"), "{found:?}");

        // A secret file at the tip, added with `git add -f` past a `.gitignore` line.
        let dir = init_repo("secret-tip");
        write_file(&dir, ".gitignore", b".env\n");
        commit_file(&dir, "docs/a.md", b"fine\n");
        write_file(&dir, "docs/.env", b"API_KEY=abc\n");
        run_git(&dir, &["add", "-f", "--", "docs/.env"]);
        run_git(&dir, &["commit", "-q", "-m", "forced"]);
        let found = rules_of(&findings_of(&dir, &cfg));
        assert!(found.iter().any(|r| r == "secret-file"), "{found:?}");

        // Staged, not yet committed.
        let dir = init_repo("secret-staged");
        write_file(&dir, "docs/id_rsa", b"x\n");
        run_git(&dir, &["add", "-f", "--", "docs/id_rsa"]);
        let found = rules_of(&own_checks(&cfg, &Repo { dir: dir.clone() }, &Mode::Staged).unwrap());
        assert!(found.iter().any(|r| r == "secret-file"), "{found:?}");
    }

    #[test]
    fn an_env_example_may_list_names_and_no_values() {
        let names_only = "# Copy to a local env file and fill in the values.\nAPI_KEY=\nDATABASE_URL=\"\"\nexport TOKEN=\nPLAIN_NAME\nEMPTY_QUOTES=''\nWITH_NOTE= # fill in\nQUOTED_NOTE=\"\" # fill in\n\n";
        assert!(rules_for_file("crates/x/.env.example", names_only.as_bytes()).is_empty());
        assert!(rules_for_file("crates/x/.env.example", format!("\u{feff}{}", names_only.replace('\n', "\r\n")).as_bytes()).is_empty());
        assert!(rules_for_file("crates/x/.env.example", b"").is_empty());
        for line in [
            "API_KEY=abc123",
            "API_KEY=\"abc123\"",
            "API_KEY='abc'",
            "API_KEY=#abc",
            "API_KEY=${OTHER}",
            "export API_KEY=x",
            "API_KEY = x",
            "API_KEY: x",
            "just some words",
            "API_KEY=''x",
        ] {
            let text = format!("# names\nFIRST=\n{line}\nLAST=\n");
            assert_eq!(rules_for_file("crates/x/.env.example", text.as_bytes()), vec!["secret-content"], "{line}");
        }
        // The name is read in any letter case and in any folder.
        assert_eq!(rules_for_file(".ENV.EXAMPLE", b"A=1\n"), vec!["secret-content"]);
        // A binary file of that name is no list of names.
        assert!(!rules_for_file("crates/x/.env.example", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").is_empty());
    }

    #[test]
    fn an_env_example_with_a_value_is_found_in_the_patch_even_when_a_later_commit_removes_it() {
        let with_value = added_lines_patch("docs/.env.example", &["# names", "API_KEY=abc123"]);
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_diff(&with_value))), vec!["secret-content"]);
        let names_only = added_lines_patch("docs/.env.example", &["# names", "API_KEY="]);
        assert!(scan_commit(&cfg(), &commit_with_diff(&names_only)).is_empty());

        let dir = init_repo("env-example-history");
        commit_file(&dir, "docs/.env.example", b"API_KEY=abc123\n");
        commit_file(&dir, "docs/.env.example", b"API_KEY=\n");
        let found = rules_of(&findings_of(&dir, &repo_cfg()));
        assert_eq!(found, vec!["secret-content"], "{found:?}");
        // Names only from the start: clean.
        let dir = init_repo("env-example-clean");
        commit_file(&dir, "docs/.env.example", b"# names\nAPI_KEY=\n");
        let found = findings_of(&dir, &repo_cfg());
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_npmrc_or_a_pypirc_with_a_token_is_refused() {
        let found = |path: &str, text: &str| rules_for_file(path, text.as_bytes());
        for text in [
            "//registry.example.org/:_authToken=abc123def\n",
            "registry=https://registry.example.org/\n//registry.example.org/:_authToken=literal\n",
            "_auth = YWJjZGVm\n",
            "//registry.example.org/:_password=\"x\"\n",
            "  //registry.example.org/:_AUTHTOKEN = abc\n",
        ] {
            assert_eq!(found("crates/x/.npmrc", text), vec!["secret-content"], "{text}");
            assert_eq!(found(".NPMRC", text), vec!["secret-content"], "{text}");
        }
        for text in [
            "registry=https://registry.example.org/\n",
            "//registry.example.org/:_authToken=${NPM_TOKEN}\n",
            "//registry.example.org/:_authToken=\n",
            "; _authToken=abc\n# _auth=abc\n",
            "",
        ] {
            assert!(found("crates/x/.npmrc", text).is_empty(), "{text}");
        }
        for text in [
            "[pypi]\nusername = __token__\npassword = xxxxxxxxxxxx\n",
            "[pypi]\nPassword: xxxxxxxxxxxx\n",
            "[pypi]\ntoken = xxxxxxxxxxxx\n",
        ] {
            assert_eq!(found("crates/x/.pypirc", text), vec!["secret-content"], "{text}");
        }
        for text in ["[pypi]\nusername = __token__\n", "[pypi]\npassword =\n", "# password = x\n; token = y\n", ""] {
            assert!(found("crates/x/.pypirc", text).is_empty(), "{text}");
        }
        // The same files in a patch.
        let patch = added_lines_patch("crates/x/.npmrc", &["registry=https://registry.example.org/", "//registry.example.org/:_authToken=abc123def"]);
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_diff(&patch))), vec!["secret-content"]);
        let patch = added_lines_patch("crates/x/.pypirc", &["[pypi]", "password = xxxxxxxxxxxx"]);
        assert_eq!(rules_of(&scan_commit(&cfg(), &commit_with_diff(&patch))), vec!["secret-content"]);
        // Another file that holds the same line is not one of the two.
        assert!(rules_for_file("crates/x/notes.txt", b"password = xxxxxxxxxxxx\n").is_empty());
    }

    #[test]
    fn a_file_of_a_language_the_project_does_not_use_is_refused_below_a_wide_allow_entry() {
        // `crates/**` and `docs/**` admit every one of these paths, so only the language rule can stop them.
        for path in [
            "crates/x/build.py",
            "crates/x/build.pyw",
            "crates/x/build.pyi",
            "docs/site.js",
            "crates/x/a.mjs",
            "crates/x/a.cjs",
            "crates/x/a.jsx",
            "crates/x/a.ts",
            "crates/x/a.d.ts",
            "crates/x/a.tsx",
            "crates/x/a.mts",
            "crates/x/a.cts",
            "crates/x/package.json",
            "crates/x/package-lock.json",
            "crates/x/npm-shrinkwrap.json",
            "docs/yarn.lock",
            "docs/pnpm-lock.yaml",
            "docs/node_modules/left/readme.md",
            "crates/x/node_modules/left/index.rs",
            "crates/x/BUILD.PY",
            "crates/x/NODE_MODULES/y.txt",
            "crates\\x\\build.py",
        ] {
            assert_eq!(rules_for_paths(&cfg(), &[path]).iter().filter(|r| *r == "banned-language").count(), 1, "{path}");
        }
    }

    #[test]
    fn names_that_only_look_like_another_language_pass() {
        for path in [
            "crates/x/src/lib.rs",
            "crates/x/src/py.rs",
            "crates/x/src/js.rs",
            "crates/x/src/ts.rs",
            "docs/python.md",
            "docs/javascript.md",
            "docs/package.md",
            "docs/package.json.md",
            "docs/my-package.json",
            "docs/node_modules.md",
            "docs/not_node_modules/x.md",
            "crates/x/src/node.rs",
            "crates/x/Cargo.toml",
            "docs/data.json",
        ] {
            assert!(scan_commit(&cfg(), &commit_with_paths(&[path])).is_empty(), "{path}");
        }
    }

    #[test]
    fn a_banned_language_file_passes_only_with_its_exact_path_and_a_reason_above_it() {
        let allow = [
            "docs/**",
            "crates/**",
            "# The installer test drives a script of the host shell because no Rust tool exists for it.",
            "crates/x/helper.py",
            "crates/x/no-reason.py",
            "# too short",
            "crates/x/short-reason.py",
            "",
            "# A page of the docs site needs this script because the site tool has no Rust form.",
            "crates/x/wild/*.js",
            "crates/y/**",
            "# The shared reason of a block holds only for the entry right below it.",
            "crates/x/first.py",
            "crates/x/second.py",
        ]
        .join("\n");
        let cfg = cfg_with_allow_text("banned-reason", &allow);
        let banned = |path: &str| rules_for_paths(&cfg, &[path]).iter().any(|r| r == "banned-language");
        // the exact path with a comment of three words or more directly above it
        assert!(!banned("crates/x/helper.py"));
        assert!(!banned("crates/x/first.py"));
        // an exact entry under another entry, a comment that gives no reason, and no entry at all
        assert!(banned("crates/x/no-reason.py"));
        assert!(banned("crates/x/short-reason.py"));
        assert!(banned("crates/x/second.py"));
        assert!(banned("crates/x/unlisted.py"));
        // a wildcard entry never admits such a file, with or without a reason
        assert!(banned("crates/x/wild/a.js"));
        assert!(banned("crates/y/z.py"));
        // the entry has to name the path as it is: another letter case is another path
        assert!(banned("crates/x/HELPER.PY"));
    }

    #[test]
    fn an_allow_entry_never_admits_a_secret_file() {
        let allow = [
            "docs/**",
            "# Kept only as a sample because a test reads it back from the disk.",
            "docs/.env",
            "# Kept only as a sample because a test reads it back from the disk.",
            "docs/server.pem",
            "# Kept only as a sample because a test reads it back from the disk.",
            "docs/secrets.json",
        ]
        .join("\n");
        let cfg = cfg_with_allow_text("secret-listed", &allow);
        for path in ["docs/.env", "docs/server.pem", "docs/secrets.json"] {
            assert!(rules_for_paths(&cfg, &[path]).iter().any(|r| r == "secret-file"), "{path}");
        }
    }

    #[test]
    fn a_listed_banned_language_file_passes_the_scan_of_a_range_and_an_unlisted_one_does_not() {
        let dir = init_repo("banned-range");
        commit_file(&dir, "docs/a.md", b"fine\n");
        commit_file(&dir, "docs/tool.py", b"print(1)\n");
        let found = rules_of(&findings_of(&dir, &repo_cfg()));
        assert!(found.iter().any(|r| r == "banned-language"), "{found:?}");
        let listed = cfg_with_allow_text(
            "banned-range-listed",
            "docs/**\n.gitattributes\n# Kept for the demo because the tool has no Rust form yet.\ndocs/tool.py\n",
        );
        let found = findings_of(&dir, &listed);
        assert!(found.is_empty(), "{found:?}");
        // Removed again in a later commit: the pushed history still held it.
        run_git(&dir, &["rm", "-q", "--", "docs/tool.py"]);
        run_git(&dir, &["commit", "-q", "-m", "drop it"]);
        let found = rules_of(&findings_of(&dir, &repo_cfg()));
        assert!(found.iter().any(|r| r == "banned-language"), "{found:?}");
    }

    #[test]
    fn the_shipped_allow_list_admits_no_secret_file_and_no_banned_language_file_without_a_reason() {
        let text = include_str!("../../scan/allowed-paths.txt");
        let cfg = cfg_with_allow_text("shipped-gate3", text);
        for entry in parse_allow(text) {
            if entry.contains('*') || entry.contains('?') {
                continue;
            }
            let rules = rules_for_paths(&cfg, &[entry.as_str()]);
            assert!(!rules.iter().any(|r| r == "secret-file" || r == "banned-language"), "{entry}: {rules:?}");
        }
    }

    #[test]
    fn the_refusal_lines_name_the_file_and_the_rule_in_plain_words() {
        let c = cfg();
        let line = render(&c, &scan_paths(&c, &["crates/x/.env".to_string()])[0]);
        assert!(line.contains("secret file refused") && line.contains(".env file") && line.contains("crates/x/.env"), "{line}");
        assert!(line.contains("no allow-list entry admits it"), "{line}");
        let line = render(&c, &scan_paths(&c, &["crates/x/server.pem".to_string()])[0]);
        assert!(line.contains("key, certificate or key store") && line.contains("crates/x/server.pem"), "{line}");
        let line = render(&c, &scan_paths(&c, &["crates/x/build.py".to_string()])[0]);
        assert!(line.contains("language the project does not use") && line.contains("a Python source file"), "{line}");
        assert!(line.contains("scan/allowed-paths.txt") && line.contains("reason") && line.contains("crates/x/build.py"), "{line}");
        let found = scan_file(&c, "crates/x/.env.example", b"API_KEY=abc123\n", false);
        let line = render(&c, &found[0]);
        assert!(line.contains("names only") && line.contains("holds a value") && line.contains("crates/x/.env.example"), "{line}");
        let found = scan_file(&c, "crates/x/.npmrc", b"//registry.example.org/:_authToken=abc123def\n", false);
        assert!(render(&c, &found[0]).contains("auth token"), "{found:?}");
        // The value that makes the refusal never stands in the line.
        assert!(!render(&c, &found[0]).contains("abc123def"));
        // A banned term in a path is shown by number, as everywhere else.
        let line = render(&c, &Finding::SecretFile { path: "crates/x/acme-internal/.env".into(), rule: "r" });
        assert!(!line.to_lowercase().contains("acme-internal") && line.contains("<term 1>"), "{line}");
    }

    #[test]
    fn only_a_comment_of_three_words_or_more_directly_above_an_entry_is_its_reason() {
        let text = "# Not a reason for the next block.\n\na.py\n# Two words\nb.py\n# This one gives a reason.\nc.py\nd.py\n  # An indented comment with words.\n  e.py  \n\u{feff}# A comment after a mark of the byte order.\nf.py\n#\ng.py\n";
        assert_eq!(parse_reasoned(text), vec!["c.py", "e.py", "f.py"]);
        assert!(parse_reasoned("").is_empty());
        // The list that for_test builds has no comments, so every entry counts.
        let c = ScanConfig::for_test(&[], &["tools/x.py"], &noreply());
        assert!(c.banned_language_allowed("tools/x.py") && !c.banned_language_allowed("tools/y.py"));
    }

    #[test]
    fn the_gitignore_lists_every_secret_name_the_gate_refuses() {
        let text = include_str!("../../.gitignore");
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for pattern in [
            ".env",
            ".env.*",
            "!.env.example",
            "*.env",
            ".envrc",
            ".credentials.json",
            "credentials*.json",
            ".mcp.json",
            "*.pem",
            "*.key",
            "*.pfx",
            "*.p12",
            "*.keystore",
            "*.jks",
            "id_rsa*",
            "id_dsa*",
            "id_ecdsa*",
            "id_ed25519*",
            "secrets.*",
            "!secrets.rs",
            "secret.*",
            "!secret.rs",
            ".npmrc",
            ".pypirc",
        ] {
            assert!(lines.contains(&pattern), "the .gitignore has no line {pattern}");
        }
    }
}
