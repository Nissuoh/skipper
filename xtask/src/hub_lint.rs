// SPDX-License-Identifier: MIT
//! `cargo xtask hub-lint` (HUB-13; MC-332 item 5, MC-337 items 1 and 5, MC-356 "Dependency", MC-358): the rules that keep
//! the vault and the gateway where the spec puts them. They run on the token stream of the shared scanner
//! (`rust_tokens`), exactly like `source-lint`: comments and the content of strings are never code, test code is not
//! read (a folder `tests`, a file `tests.rs`, items with a test-only `cfg`), and a finding is one line,
//! `file:line rule text`, with `file` below the workspace root and `/` as separator. A plain `source-lint` run also runs
//! these rules, so the gate needs no new step.
//!
//! - `net-lint` (MC-337 item 1, MC-356; H20):
//!   - Never allowed anywhere: the identifiers `SECURITY_FLAG_IGNORE_*` and `WINHTTP_OPTION_SECURITY_FLAGS` (no TLS error
//!     is ever ignored), the pre-TLS-1.2 protocol flags (`WINHTTP_FLAG_SECURE_PROTOCOL_SSL2`, `_SSL3`, `_TLS1`, `_TLS1_1`,
//!     `_ALL`) and the redirect policies that follow a redirect (`WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS`,
//!     `..._DISALLOW_HTTPS_TO_HTTP`; redirects are off, MC-336 item 2). A flag that is written as a number is not seen.
//!   - Network code only in `skipper-net`, `skipper-gateway` and (until HUB-2 moves its `http.rs`) `skipper-update`: the
//!     identifiers that start with `WinHttp`, `WINHTTP_`, `InternetOpen`, `InternetConnect`, `InternetRead`,
//!     `InternetWrite`, `HttpOpenRequest`, `HttpSendRequest`, `URLDownloadTo`, `URLOpen`, `WSA`, `GetAddrInfo`,
//!     `getaddrinfo`, `gethostbyname`, and `TcpStream`, `TcpListener`, `UdpSocket`, `ToSocketAddrs`,
//!     `to_socket_addrs`. The address value types of `std::net` (`Ipv4Addr`, `IpAddr`) are no network code.
//!   - The manifests: the `windows-sys` features `Win32_Networking_WinHttp` and `Win32_Networking_WinInet` only in those
//!     three crates (the root manifest included).
//! - `cred-lint` for the vault folder (MC-332 item 5): the folder literal stays in `skipper-core/src/hub` (the constants
//!     `VAULT_FOLDER`, `VAULT_META_FILE`, ...) and in the gateway. Elsewhere a string literal that is a path with a
//!     `vault` segment (`vault\entries`), a `vault.toml` segment or `skipper-vault`, and a call `join("vault")` or
//!     `push("vault")` is a finding. A plain `"vault"` is no finding (it is the name of a settings table).
//! - `secret-lint` (MC-356 `Secret` / `SecretText`, MC-337 item 5):
//!   - No `format!`-family macro (`format`, `format_args`, `print`, `println`, `eprint`, `eprintln`, `write`, `writeln`,
//!     `panic`, `unreachable`, `todo`, `unimplemented`, `dbg`, `trace`, `debug`, `info`, `warn`) names a value of
//!     type `Secret` or `SecretText`: a path `Secret::..` in its arguments, a name that the file declares with such a type
//!     (`name: &Secret`, `let name = SecretText::new(..)`; a field counts), also written inline in the format string
//!     (`{name}`). `name.len()` and `name.is_empty()` are no use of the value.
//!   - No `impl Display` for either type, no derived `Debug` or `Serialize` on either, no derived `Clone` or `Copy` on
//!     `SecretText`, no `impl Serialize`, `Clone` or `Copy` for it, and no derived `Serialize` on a type that holds a
//!     `SecretText` - except in `skipper-core/src/hub/wire.rs`, the one wire writer.
//!
//! Known gaps, because a token stream has no types: a secret that a function returns and a caller formats
//! (`format!("{}", get())`), a name that two types share in one file (the lint then reads both as the secret), a `Clone`
//! that `.clone()` calls through a wrapper, a type alias (`type S = Secret;`). A typed check can close them later and adds
//! to this lint without replacing it.
//!
//! Every exception is a line of [`ALLOWED`] with its reason (`cargo xtask hub-lint --allowances` lists them).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::rust_tokens::{ident_at, is_ident, is_punct, is_test_file, str_at, tokenize, Token};
use crate::source_lint::{line_of, path_matches, skip_group, test_regions};

const NET: &str = "net-lint";
const CRED: &str = "cred-lint";
const SECRET: &str = "secret-lint";

/// The one wire writer of a `SecretText` (MC-337 item 5): the file that may serialize it.
const WIRE: &str = "crates/skipper-core/src/hub/wire.rs";

/// The findings of [`WIRE`] that are the writer itself.
const WIRE_TEXTS: &[&str] = &[
    "derive Serialize on a holder of SecretText",
    "derive Serialize on SecretText",
    "derive Clone on SecretText",
    "impl Serialize for SecretText",
    "impl Clone for SecretText",
];

/// The `windows-sys` features that bring WinHTTP or WinInet in.
const NET_FEATURES: [&str; 2] = ["Win32_Networking_WinHttp", "Win32_Networking_WinInet"];

/// The macros that turn their arguments into text.
const FORMAT_MACROS: [&str; 17] = [
    "format",
    "format_args",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "panic",
    "unreachable",
    "todo",
    "unimplemented",
    "dbg",
    "trace",
    "debug",
    "info",
    "warn",
];

/// One exception to one rule.
struct Allow {
    rule: &'static str,
    /// A file below the workspace root, or a folder with a trailing `/**`.
    path: &'static str,
    /// Only for findings with one of these texts; empty means every finding of the rule that can be allowed.
    texts: &'static [&'static str],
    reason: &'static str,
}

/// Every exception of every rule, each with its reason. Nothing else is allowed: a new line needs a reason. The findings
/// that no exception can cover (the never-allowed names of `net-lint`) are not in the table.
const ALLOWED: &[Allow] = &[
    Allow {
        rule: NET,
        path: "crates/skipper-net/**",
        texts: &[],
        reason: "the shared network crate: HTTPS through WinHTTP (MC-337 item 1)",
    },
    Allow {
        rule: NET,
        path: "crates/skipper-gateway/**",
        texts: &[],
        reason: "the sign-in helper is the one Skipper process that holds secrets and reaches the network (MC-337 item 1)",
    },
    Allow {
        rule: NET,
        path: "crates/skipper-update/**",
        texts: &[],
        reason: "temporary: the update helper (MC-274) has its own WinHTTP code until HUB-2 moves `http.rs` into skipper-net",
    },
    // SI-2-0 (control r2 N3): the loopback mode of the test stand-in, one file and no more. The Stage 1 pack and dist do not
    // carry the stand-in (`xtask/src/pack.rs` EXECUTABLES); the Stage 0 dist still lists it (`xtask/src/dist.rs`), where
    // `pe-check` counts `ws2_32` (what `std::net` links) as a system DLL and refuses only the WinHTTP-class DLLs.
    Allow {
        rule: NET,
        path: "crates/skipper-standin/src/modes/listen_loopback.rs",
        texts: &[],
        reason: "test stand-in, not in the Stage 1 pack, loopback only (SI-2-0)",
    },
    Allow {
        rule: CRED,
        path: "crates/skipper-core/src/hub/**",
        texts: &[],
        reason: "the hub module names the vault folder once, in constants (MC-332 item 5)",
    },
    Allow { rule: CRED, path: "crates/skipper-gateway/**", texts: &[], reason: "the gateway owns the vault folder (MC-332 item 5)" },
    Allow {
        rule: SECRET,
        path: WIRE,
        texts: WIRE_TEXTS,
        reason: "the one wire writer of a SecretText: it passes the pasted sign-in address core -> gateway once (MC-337 item 5)",
    },
];

/// One place that breaks a rule, before the exceptions are applied.
#[derive(Clone, PartialEq, Debug)]
struct Finding {
    line: usize,
    rule: &'static str,
    text: String,
    /// True for a finding that no exception can cover.
    never: bool,
}

/// A hit at a token, before it becomes a [`Finding`].
struct Hit {
    at: usize,
    rule: &'static str,
    text: String,
    never: bool,
}

fn hit(at: usize, rule: &'static str, text: impl Into<String>) -> Hit {
    Hit { at, rule, text: text.into(), never: false }
}

fn is_allowed(path: &str, finding: &Finding) -> bool {
    !finding.never
        && ALLOWED.iter().any(|a| {
            a.rule == finding.rule
                && path_matches(a.path, path)
                && (a.texts.is_empty() || a.texts.contains(&finding.text.as_str()))
        })
}

/// The exceptions as report lines `rule path - reason`.
pub fn allowances() -> Vec<String> {
    ALLOWED.iter().map(|a| format!("{} {} - {}", a.rule, a.path, a.reason)).collect()
}

// ---------------------------------------------------------------------------------------------
// net-lint
// ---------------------------------------------------------------------------------------------

/// The names that no code may use (MC-337 item 1, MC-336 item 2): ignoring a TLS error, a protocol older than TLS 1.2, a
/// redirect that is followed.
fn is_never_name(name: &str) -> bool {
    name.starts_with("SECURITY_FLAG_IGNORE_")
        || matches!(
            name,
            "WINHTTP_OPTION_SECURITY_FLAGS"
                | "WINHTTP_FLAG_SECURE_PROTOCOL_SSL2"
                | "WINHTTP_FLAG_SECURE_PROTOCOL_SSL3"
                | "WINHTTP_FLAG_SECURE_PROTOCOL_TLS1"
                | "WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_1"
                | "WINHTTP_FLAG_SECURE_PROTOCOL_ALL"
                | "WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS"
                | "WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP"
        )
}

/// The identifiers of network code: WinHTTP and WinInet, Winsock set-up, name lookup and the socket types of `std::net`.
fn is_network_name(name: &str) -> bool {
    matches!(name, "TcpStream" | "TcpListener" | "UdpSocket" | "ToSocketAddrs" | "to_socket_addrs" | "getaddrinfo" | "gethostbyname")
        || [
            "WinHttp",
            "WINHTTP_",
            "InternetOpen",
            "InternetConnect",
            "InternetRead",
            "InternetWrite",
            "HttpOpenRequest",
            "HttpSendRequest",
            "URLDownloadTo",
            "URLOpen",
            "WSA",
            "GetAddrInfo",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn net_hits(tokens: &[Token], skipped: &[bool]) -> Vec<Hit> {
    let mut out = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Token::Ident(name, _) = token else { continue };
        if skipped[i] {
            continue;
        }
        if is_never_name(name) {
            out.push(Hit { at: i, rule: NET, text: name.clone(), never: true });
        } else if is_network_name(name) && !(i > 0 && is_ident(tokens, i - 1, "fn")) {
            out.push(hit(i, NET, name.clone()));
        }
    }
    out
}

/// The report lines of one manifest (`Cargo.toml`): the `windows-sys` features of WinHTTP and WinInet outside the
/// crates that may use them. Comments are not read.
pub fn check_manifest(path: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        for feature in NET_FEATURES {
            if code.contains(feature) {
                let finding = Finding { line: number + 1, rule: NET, text: feature.to_owned(), never: false };
                if !is_allowed(path, &finding) {
                    out.push(format!("{path}:{} {NET} {feature}", finding.line));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// cred-lint for the vault folder
// ---------------------------------------------------------------------------------------------

/// What a string literal says about the vault folder: a path with a `vault` segment (`...\vault\entries`), the metadata
/// file `vault.toml`, or the entropy prefix `skipper-vault`, in any letter case.
fn vault_text(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.contains("skipper-vault") {
        return Some("skipper-vault");
    }
    let segments: Vec<&str> = lower.split(['/', '\\']).filter(|segment| !segment.is_empty()).collect();
    if segments.contains(&"vault.toml") {
        return Some("vault.toml");
    }
    if segments.len() >= 2 && segments.contains(&"vault") {
        return Some("vault folder");
    }
    None
}

fn vault_hits(tokens: &[Token], skipped: &[bool]) -> Vec<Hit> {
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        if skipped[i] {
            continue;
        }
        if let Some((text, _)) = str_at(tokens, i) {
            if let Some(what) = vault_text(text) {
                out.push(hit(i, CRED, what));
            }
        }
        // `root.join("vault")`: the folder name as a path step (a plain literal is no finding, `[vault]` is a table).
        if (is_ident(tokens, i, "join") || is_ident(tokens, i, "push")) && is_punct(tokens, i + 1, '(') {
            if let Some((text, _)) = str_at(tokens, i + 2) {
                if text.eq_ignore_ascii_case("vault") && is_punct(tokens, i + 3, ')') {
                    out.push(hit(i + 2, CRED, "join(\"vault\")"));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// secret-lint
// ---------------------------------------------------------------------------------------------

fn is_secret_type(name: &str) -> bool {
    name == "Secret" || name == "SecretText"
}

/// The opening bracket (`(`, `[`, `{` or the `<` of generics) of the group that holds the token before `at`, or `None` when a
/// `;` ends the statement first. Groups that close before `at` are skipped.
fn enclosing_opener(tokens: &[Token], at: usize) -> Option<char> {
    let mut depth = 0i32;
    for p in (0..at).rev() {
        match &tokens[p] {
            Token::Punct(')' | ']' | '}', _) => depth += 1,
            Token::Punct('>', _) if p > 0 && !is_punct(tokens, p - 1, '-') && !is_punct(tokens, p - 1, '=') => depth += 1,
            Token::Punct('(', _) | Token::Punct('[', _) | Token::Punct('{', _) | Token::Punct('<', _) => {
                if depth == 0 {
                    return match &tokens[p] {
                        Token::Punct(c, _) => Some(*c),
                        _ => None,
                    };
                }
                depth -= 1;
            }
            Token::Punct(';', _) if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// True when the identifier at `at` (a path `a::b::Name` ends there) stands where a type stands: after the `:` of a field
/// or a parameter, a `<`, a `&`, an opening bracket or `mut` / `dyn`, or after the `,` of a list of generic arguments or of a
/// tuple (`HashMap<String, Secret>`, `(u8, SecretText)`), not after the `,` of the variants of an enum.
fn is_type_position(tokens: &[Token], at: usize) -> bool {
    let mut start = at;
    while start >= 3 && is_punct(tokens, start - 1, ':') && is_punct(tokens, start - 2, ':') && ident_at(tokens, start - 3).is_some() {
        start -= 3;
    }
    if start == 0 {
        return false;
    }
    match tokens.get(start - 1) {
        Some(Token::Punct(':', _)) => start < 2 || !is_punct(tokens, start - 2, ':'),
        Some(Token::Punct('<' | '&' | '(' | '[', _)) => true,
        Some(Token::Punct(',', _)) => matches!(enclosing_opener(tokens, start - 1), Some('<' | '(')),
        Some(Token::Ident(word, _)) => matches!(word.as_str(), "mut" | "dyn" | "impl"),
        _ => false,
    }
}

/// The names that the file declares with a secret type: `name: <type that holds Secret or SecretText>` (a field, a
/// parameter, a typed `let`) and `let [mut] name = [path::]Secret::..`.
fn tracked_names(tokens: &[Token], skipped: &[bool]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for i in 0..tokens.len() {
        if skipped[i] {
            continue;
        }
        let Some(name) = ident_at(tokens, i) else { continue };
        if is_punct(tokens, i + 1, ':') && !is_punct(tokens, i + 2, ':') && !(i > 0 && is_punct(tokens, i - 1, ':')) {
            // The type: up to the end of the field, parameter or statement, with `< >` kept together.
            let mut angle = 0i32;
            let mut at = i + 2;
            while at < tokens.len() && at < i + 2 + 24 {
                match &tokens[at] {
                    Token::Punct('<', _) => angle += 1,
                    Token::Punct('>', _) if !is_punct(tokens, at - 1, '-') => angle -= 1,
                    Token::Punct(',', _) if angle <= 0 => break,
                    Token::Punct(')' | '=' | ';' | '{' | '}' | '|', _) => break,
                    Token::Ident(word, _) if is_secret_type(word) && is_type_position(tokens, at) => {
                        names.insert(name.to_owned());
                        break;
                    }
                    _ => {}
                }
                at += 1;
            }
        }
        if name == "let" {
            let mut at = i + 1;
            if is_ident(tokens, at, "mut") {
                at += 1;
            }
            let Some(bound) = ident_at(tokens, at) else { continue };
            if is_punct(tokens, at + 1, '=') && !is_punct(tokens, at + 2, '=') {
                let mut path = at + 2;
                while let Some(segment) = ident_at(tokens, path) {
                    if !(is_punct(tokens, path + 1, ':') && is_punct(tokens, path + 2, ':')) {
                        break;
                    }
                    if is_secret_type(segment) {
                        names.insert(bound.to_owned());
                        break;
                    }
                    path += 3;
                }
            }
        }
    }
    names
}

/// The names written as `{name}` or `{name:?}` in a format string (`{{` is a brace, not an argument).
fn inline_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if chars.get(i + 1) == Some(&'{') {
                i += 2;
                continue;
            }
            let mut j = i + 1;
            let mut name = String::new();
            while j < chars.len() && (chars[j] == '_' || chars[j].is_alphanumeric()) {
                name.push(chars[j]);
                j += 1;
            }
            if !name.is_empty() {
                out.push(name);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// `name.len()` and `name.is_empty()` say nothing about the value.
fn is_size_call(tokens: &[Token], at: usize) -> bool {
    is_punct(tokens, at + 1, '.') && matches!(ident_at(tokens, at + 2), Some("len" | "is_empty"))
}

fn format_hits(tokens: &[Token], skipped: &[bool]) -> Vec<Hit> {
    let tracked = tracked_names(tokens, skipped);
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        if skipped[i] {
            continue;
        }
        let Some(mac) = ident_at(tokens, i) else { continue };
        if !FORMAT_MACROS.contains(&mac) || !is_punct(tokens, i + 1, '!') {
            continue;
        }
        if !(is_punct(tokens, i + 2, '(') || is_punct(tokens, i + 2, '[') || is_punct(tokens, i + 2, '{')) {
            continue;
        }
        let end = skip_group(tokens, i + 2).min(tokens.len());
        for k in (i + 3)..end {
            match &tokens[k] {
                Token::Ident(name, _) => {
                    // `name = value` and `name: value` label something; the value is judged on its own.
                    let label = (is_punct(tokens, k + 1, '=') && !is_punct(tokens, k + 2, '=') && !is_punct(tokens, k + 2, '>'))
                        || (is_punct(tokens, k + 1, ':') && !is_punct(tokens, k + 2, ':'));
                    if tracked.contains(name.as_str()) && !label && !is_size_call(tokens, k) {
                        out.push(hit(k, SECRET, format!("{mac}! {name}")));
                    }
                    // A call or a constant of the type (`Secret::new(..)`, also `wire::SecretText::new(..)`); an enum variant of
                    // that name (`Refusal::Secret`) has no `::` after it.
                    if is_secret_type(name) && is_punct(tokens, k + 1, ':') && is_punct(tokens, k + 2, ':') {
                        out.push(hit(k, SECRET, format!("{mac}! {name}::")));
                    }
                }
                Token::Str(text, _) => {
                    for name in inline_names(text) {
                        if tracked.contains(&name) {
                            out.push(hit(k, SECRET, format!("{mac}! {{{name}}}")));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The index after the generics `< .. >` that open at `open`, or `open` when there are none.
fn skip_angles(tokens: &[Token], open: usize) -> usize {
    if !is_punct(tokens, open, '<') {
        return open;
    }
    let mut depth = 0i32;
    for at in open..tokens.len() {
        if is_punct(tokens, at, '<') {
            depth += 1;
        } else if is_punct(tokens, at, '>') && !(at > 0 && is_punct(tokens, at - 1, '-')) {
            depth -= 1;
            if depth == 0 {
                return at + 1;
            }
        }
    }
    tokens.len()
}

/// The last identifier of the path that starts at `at` (`a::b::C` gives `C`), and the index after the path.
fn path_end(tokens: &[Token], mut at: usize) -> Option<(String, usize)> {
    let mut last = ident_at(tokens, at)?.to_owned();
    at += 1;
    while is_punct(tokens, at, ':') && is_punct(tokens, at + 1, ':') {
        match ident_at(tokens, at + 2) {
            Some(next) => {
                last = next.to_owned();
                at += 3;
            }
            None => break,
        }
    }
    Some((last, at))
}

/// Hits for `impl <Trait> for Secret` and `impl <Trait> for SecretText`.
fn impl_hits(tokens: &[Token], skipped: &[bool]) -> Vec<Hit> {
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        if skipped[i] || !is_ident(tokens, i, "impl") {
            continue;
        }
        let start = skip_angles(tokens, i + 1);
        let Some((tr, after_trait)) = path_end(tokens, start) else { continue };
        let after_args = skip_angles(tokens, after_trait);
        if !is_ident(tokens, after_args, "for") {
            continue;
        }
        let Some((target, _)) = path_end(tokens, after_args + 1) else { continue };
        if !is_secret_type(&target) {
            continue;
        }
        match tr.as_str() {
            "Display" => out.push(Hit { at: i, rule: SECRET, text: format!("impl Display for {target}"), never: true }),
            "Serialize" => out.push(hit(i, SECRET, format!("impl Serialize for {target}"))),
            "Clone" | "Copy" if target == "SecretText" => out.push(hit(i, SECRET, format!("impl {tr} for {target}"))),
            _ => {}
        }
    }
    out
}

/// The names in the derive attribute that opens at `open` (the `(` after `derive`).
fn derive_names(tokens: &[Token], open: usize) -> Vec<String> {
    let end = skip_group(tokens, open).min(tokens.len());
    ((open + 1)..end).filter_map(|at| ident_at(tokens, at).map(str::to_owned)).collect()
}

/// Hits for the derives on `Secret` and `SecretText` and for a derived `Serialize` on a type that holds a `SecretText`.
fn derive_hits(tokens: &[Token], skipped: &[bool]) -> Vec<Hit> {
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        if skipped[i] || !(is_punct(tokens, i, '#') && is_punct(tokens, i + 1, '[') && is_ident(tokens, i + 2, "derive")) {
            continue;
        }
        if !is_punct(tokens, i + 3, '(') {
            continue;
        }
        let derives = derive_names(tokens, i + 3);
        // The item: after this attribute, any other attributes and the visibility.
        let mut at = skip_group(tokens, i + 1);
        while is_punct(tokens, at, '#') && is_punct(tokens, at + 1, '[') {
            at = skip_group(tokens, at + 1);
        }
        if is_ident(tokens, at, "pub") {
            at += 1;
            if is_punct(tokens, at, '(') {
                at = skip_group(tokens, at);
            }
        }
        if !(is_ident(tokens, at, "struct") || is_ident(tokens, at, "enum") || is_ident(tokens, at, "union")) {
            continue;
        }
        let Some(name) = ident_at(tokens, at + 1) else { continue };
        if is_secret_type(name) {
            let forbidden: &[&str] = if name == "SecretText" { &["Debug", "Serialize", "Clone", "Copy"] } else { &["Debug", "Serialize"] };
            for derived in &derives {
                if forbidden.contains(&derived.as_str()) {
                    out.push(hit(at + 1, SECRET, format!("derive {derived} on {name}")));
                }
            }
        } else if derives.iter().any(|d| d == "Serialize") {
            // A holder: a field or a variant payload of type `SecretText`.
            let mut body = at + 2;
            while body < tokens.len() && !(is_punct(tokens, body, '{') || is_punct(tokens, body, '(') || is_punct(tokens, body, ';')) {
                body += 1;
            }
            if body < tokens.len() && !is_punct(tokens, body, ';') {
                let end = skip_group(tokens, body).min(tokens.len());
                let holds = (body..end).any(|k| is_ident(tokens, k, "SecretText") && is_type_position(tokens, k));
                if holds {
                    out.push(hit(at + 1, SECRET, "derive Serialize on a holder of SecretText"));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The files
// ---------------------------------------------------------------------------------------------

/// Every place of `src` that breaks a rule, outside comments and test code, before the exceptions are applied, in the
/// order of the lines.
fn findings(src: &str) -> Vec<Finding> {
    let tokens = tokenize(src);
    if is_test_file(&tokens) {
        return Vec::new();
    }
    let skipped = test_regions(&tokens);
    let mut hits = net_hits(&tokens, &skipped);
    hits.extend(vault_hits(&tokens, &skipped));
    hits.extend(format_hits(&tokens, &skipped));
    hits.extend(impl_hits(&tokens, &skipped));
    hits.extend(derive_hits(&tokens, &skipped));
    let mut out: Vec<Finding> = hits
        .into_iter()
        .map(|h| Finding { line: line_of(&tokens[h.at]), rule: h.rule, text: h.text, never: h.never })
        .collect();
    out.sort_by(|a, b| (a.line, &a.text).cmp(&(b.line, &b.text)));
    out.dedup();
    out
}

/// The report lines of one Rust file: its findings that no exception covers, as `path:line rule text`.
pub fn check_file(path: &str, src: &str) -> Vec<String> {
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
            let relative = relative_path(root, &path);
            report.extend(check_file(&relative, &text));
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The report lines of the tree below `root`: the root manifest, and per crate its manifest and every
/// `crates/<crate>/src/**/*.rs`, in name order. An error when `root` has no `crates` folder (the command was started
/// outside the workspace).
pub fn check_tree(root: &Path) -> Result<Vec<String>, String> {
    let crates = root.join("crates");
    let mut members: Vec<_> =
        fs::read_dir(&crates).map_err(|e| format!("cannot read {}: {e}", crates.display()))?.filter_map(Result::ok).collect();
    members.sort_by_key(|entry| entry.file_name());
    let mut report = Vec::new();
    let manifests = std::iter::once(root.join("Cargo.toml")).chain(members.iter().map(|member| member.path().join("Cargo.toml")));
    for manifest in manifests {
        if manifest.is_file() {
            let text = fs::read_to_string(&manifest).map_err(|e| format!("cannot read {}: {e}", manifest.display()))?;
            report.extend(check_manifest(&relative_path(root, &manifest), &text));
        }
    }
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
            eprintln!("usage: cargo xtask hub-lint [--allowances]");
            return 2;
        }
    }
    match check_tree(Path::new(".")) {
        Ok(report) if report.is_empty() => {
            println!("hub-lint: ok");
            0
        }
        Ok(report) => {
            for line in &report {
                println!("{line}");
            }
            1
        }
        Err(why) => {
            println!("hub-lint: {why} (run it from the workspace root)");
            1
        }
    }
}
