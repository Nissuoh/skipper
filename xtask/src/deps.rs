// SPDX-License-Identifier: MIT
//! `cargo xtask deps-check`: the dependency checks of G10 (no HTTP, TLS or network crate and no tokio in
//! `skipper.exe`; blocked and to-be-inspected crates, MC-120; no TLS feature of `bollard`; no HTTP
//! crate below `rmcp`; no `persistence` feature of `eframe`, L7) and the licence file THIRD-PARTY-LICENSES (MC-121), plan Task 17.
//! SP-6 (closing review, gate lens finding 1) makes that file the one `docs/design/third-party-notice-list.md` section 2 plans:
//! the seven parts, with the licence, copyright and NOTICE texts of every shipped package read from cargo's registry, the font
//! texts, the block of the embedded `puff.c` code, the data attributions and the runtime notices; `dist` stops, naming the
//! package, while a text is missing ([`build_licence_file`]). The same holds for the extra blocks of the table `REQUIRED_BLOCKS`
//! (notice list 5.2), also for a package that ships a licence text of its own.
//! HUB-13 (MC-337 item 1, MC-356 "Dependency") adds: `skipper.exe` has no HTTP or TLS crate and no crypto crate besides
//! the `sha2` it has already (and the crates `sha2` itself needs: `digest`, `block-buffer`, `crypto-common`, ...); the sign-in
//! helper and its network crate (`skipper-gateway`, `skipper-net`) have no HTTP, TLS, crypto or async crate either (S1G-4:
//! DPAPI, CNG and WinHTTP come through `windows-sys` features, which add no package).
//! RL-05 (IMP-13, GI-1; MC-483) adds three `cargo tree` rows for the import helper, which reads outside files in a box with no
//! network: `reqwest`, `hyper` and `tokio` must not be found below `skipper-import`.
//!
//! The graph comes from `cargo metadata --locked --offline --format-version 1 --filter-platform
//! x86_64-pc-windows-gnu`: `Graph::packages` are the packages of the resolve graph; `Graph::edges` are
//! the normal (not dev, not build) dependencies, `(from, to, active features of `to`)`, because only
//! normal dependencies end up inside a binary. The raw outputs of the `cargo tree` calls are printed
//! to stdout, so `skipper-cap run --log` keeps them as evidence.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::catalog::{ANTHROPIC_LINE, EPOCH_LINE};

/// The package whose binary must stay free of HTTP, TLS and tokio (bin `skipper` of `skipper-app`).
const MAIN_PACKAGE: &str = "skipper-app";
const TARGET: &str = "x86_64-pc-windows-gnu";

/// The HTTP and TLS crates of G10 and plan Task 17 Step 3.
const HTTP_TLS: [&str; 15] = [
    "reqwest",
    "hyper",
    "hyper-util",
    "h2",
    "rustls",
    "tokio-rustls",
    "hyper-rustls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "ureq",
    "curl",
    "curl-sys",
    "isahc",
    "attohttpc",
];

/// The crypto crates that `skipper.exe` must not link (MC-356 "Dependency"): the one crypto crate it has is `sha2` (the board's
/// and the manifest's SHA-256), with the small crates `sha2` pulls in (`digest`, `block-buffer`, `crypto-common`,
/// `generic-array`, `typenum`, `cpufeatures`), which are not listed. Everything that seals, signs, derives or hashes beyond
/// that is the gateway's work through Windows (DPAPI, CNG), never a crate.
const CRYPTO: [&str; 34] = [
    "ring",
    "aws-lc-rs",
    "aws-lc-sys",
    "aes",
    "aes-gcm",
    "aes-gcm-siv",
    "aes-siv",
    "chacha20",
    "chacha20poly1305",
    "xchacha20poly1305",
    "poly1305",
    "salsa20",
    "rsa",
    "p256",
    "p384",
    "p521",
    "k256",
    "ecdsa",
    "elliptic-curve",
    "ed25519",
    "ed25519-dalek",
    "curve25519-dalek",
    "x25519-dalek",
    "hmac",
    "hkdf",
    "pbkdf2",
    "argon2",
    "scrypt",
    "sha1",
    "sha3",
    "md-5",
    "blake2",
    "blake3",
    "orion",
];

/// More crypto crates, found by the start of the name (`sodiumoxide`, `libsodium-sys`, `webpki-roots`, `rustls-*`, `ripemd160`).
const CRYPTO_PREFIXES: [&str; 6] = ["sodiumoxide", "libsodium", "webpki", "ripemd", "whirlpool", "snow"];

/// True for a crypto crate that `skipper.exe` must not link: the names of [`CRYPTO`] and the prefixes of
/// [`CRYPTO_PREFIXES`]; `sha2` and the crates it needs are none.
fn is_crypto_crate(name: &str) -> bool {
    CRYPTO.contains(&name) || CRYPTO_PREFIXES.iter().any(|prefix| name == *prefix || name.starts_with(&format!("{prefix}-")))
}

/// The crates of the sign-in helper's side: the helper and its network crate (MC-337 item 1).
const NET_PACKAGES: [&str; 2] = ["skipper-gateway", "skipper-net"];

/// Async runtimes: none below the helper (a thread per call, MC-337 item 5a).
const ASYNC_RUNTIMES: [&str; 3] = ["tokio", "async-std", "smol"];

/// Blocked crates (G10, MC-120): they fail the check. `webbrowser` is no malware, but the open command of the window runs the
/// system's own URL handler through `session::host` (Task 36b) and a crate that starts a browser by itself stays out of the lock.
const BLOCKED: [&str; 7] = ["proc-macro1", "proc-macro-en", "aovine", "arone", "aronenao", "tinymember", "webbrowser"];

/// Crate versions flagged for inspection (G10, MC-120): reported, a person reads their source on the pin day.
const INSPECT: [(&str, &str); 3] = [("append-only-vec", "0.1.9"), ("arrayref", "0.3.10"), ("internment", "0.8.7")];

#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub licence: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Graph {
    pub packages: Vec<Package>,
    /// `(from, to, active features of to)`, normal dependencies only.
    pub edges: Vec<(usize, usize, Vec<String>)>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum DepFinding {
    HttpOrTlsInMainBinary(String),
    TokioInMainBinary,
    Blocked(String),
    Inspect(String, String),
    TlsFeature(String, String),
    HttpInRmcp(String),
    /// eframe's `persistence` feature is on: egui memory would be written to `%APPDATA%` outside the door (L7).
    PersistenceFeature,
    /// A crypto crate other than `sha2` (and what it needs) is linked into the main binary (MC-356 "Dependency").
    CryptoInMainBinary(String),
    /// An HTTP, TLS, crypto or async crate below the sign-in helper or its network crate: `(root package, crate)`.
    ForbiddenBelowNetPackage(String, String),
    /// `xtask` with its default features depends on the network crate: the push gate's scanner must link none (MODEL-15,
    /// R13); the edge exists only behind the feature `catalog-ci`.
    NetCrateInXtask,
}

fn adjacency(g: &Graph) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); g.packages.len()];
    for (from, to, _) in &g.edges {
        if *from < adj.len() && *to < adj.len() {
            adj[*from].push(*to);
        }
    }
    adj
}

/// Every package index reachable from the roots (the roots included), sorted.
fn reachable(adj: &[Vec<usize>], roots: &[usize]) -> Vec<usize> {
    let mut seen = vec![false; adj.len()];
    let mut stack: Vec<usize> = roots.to_vec();
    let mut out: Vec<usize> = Vec::new();
    while let Some(i) = stack.pop() {
        if i >= seen.len() || seen[i] {
            continue;
        }
        seen[i] = true;
        out.push(i);
        stack.extend(adj[i].iter().copied());
    }
    out.sort_unstable();
    out
}

fn indices_named(g: &Graph, name: &str) -> Vec<usize> {
    g.packages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == name)
        .map(|(i, _)| i)
        .collect()
}

fn push_unique(findings: &mut Vec<DepFinding>, finding: DepFinding) {
    if !findings.contains(&finding) {
        findings.push(finding);
    }
}

fn is_tls_feature(feature: &str) -> bool {
    let f = feature.to_ascii_lowercase();
    f.contains("ssl") || f.contains("tls") || matches!(f.as_str(), "aws-lc-rs" | "ring" | "webpki")
}

/// The checks of G10 on a resolved graph. `main` is the package of the main binary; a graph without
/// that package simply has nothing reachable from it.
pub fn check_graph(g: &Graph, main: &str) -> Vec<DepFinding> {
    let mut findings: Vec<DepFinding> = Vec::new();
    let adj = adjacency(g);

    // 1. Nothing HTTP, TLS or tokio below the main binary.
    for i in reachable(&adj, &indices_named(g, main)) {
        let name = g.packages[i].name.as_str();
        if HTTP_TLS.contains(&name) {
            push_unique(&mut findings, DepFinding::HttpOrTlsInMainBinary(name.to_string()));
        }
        if name == "tokio" {
            push_unique(&mut findings, DepFinding::TokioInMainBinary);
        }
        if is_crypto_crate(name) {
            push_unique(&mut findings, DepFinding::CryptoInMainBinary(name.to_string()));
        }
    }

    // 1b. Nothing HTTP, TLS, crypto or async below the sign-in helper or its network crate (MC-337 item 1; `sha2` is allowed).
    for root in NET_PACKAGES {
        for i in reachable(&adj, &indices_named(g, root)) {
            let name = g.packages[i].name.as_str();
            if HTTP_TLS.contains(&name) || ASYNC_RUNTIMES.contains(&name) || is_crypto_crate(name) {
                push_unique(&mut findings, DepFinding::ForbiddenBelowNetPackage(root.to_string(), name.to_string()));
            }
        }
    }

    // 2. Blocked crates and crates flagged for inspection, anywhere in the graph.
    for p in &g.packages {
        if BLOCKED.contains(&p.name.as_str()) {
            push_unique(&mut findings, DepFinding::Blocked(p.name.clone()));
        }
        if INSPECT.iter().any(|(n, v)| p.name == *n && p.version == *v) {
            push_unique(&mut findings, DepFinding::Inspect(p.name.clone(), p.version.clone()));
        }
    }

    // 3. bollard without any TLS feature (G8, PRB-65).
    for (i, p) in g.packages.iter().enumerate() {
        if p.name != "bollard" {
            continue;
        }
        for (_, to, features) in &g.edges {
            if *to != i {
                continue;
            }
            for feature in features {
                if is_tls_feature(feature) {
                    push_unique(&mut findings, DepFinding::TlsFeature(p.name.clone(), feature.clone()));
                }
            }
        }
    }

    // 4. No HTTP crate below rmcp (PRB-65: only `server` and `transport-io`).
    for i in reachable(&adj, &indices_named(g, "rmcp")) {
        let name = g.packages[i].name.as_str();
        if HTTP_TLS.contains(&name) {
            push_unique(&mut findings, DepFinding::HttpInRmcp(name.to_string()));
        }
    }

    // 5. eframe without its `persistence` feature (L7): UI state goes through the door, never `std::fs` of eframe.
    for i in indices_named(g, "eframe") {
        for (_, to, features) in &g.edges {
            if *to == i && features.iter().any(|feature| feature == "persistence") {
                push_unique(&mut findings, DepFinding::PersistenceFeature);
            }
        }
    }

    // 6. No edge from `xtask` (default features) to the network crate (MODEL-15, R13): `cargo metadata` resolves the default
    // features, so an optional dependency behind `catalog-ci` shows no edge here; a non-optional one does.
    let net_crates = indices_named(g, "skipper-net");
    for from in indices_named(g, "xtask") {
        if g.edges.iter().any(|(a, b, _)| *a == from && net_crates.contains(b)) {
            push_unique(&mut findings, DepFinding::NetCrateInXtask);
        }
    }
    findings
}

fn describe(f: &DepFinding) -> String {
    match f {
        DepFinding::HttpOrTlsInMainBinary(name) => format!("{name} (HTTP or TLS crate) is linked into the main binary (G10)"),
        DepFinding::TokioInMainBinary => "tokio is linked into the main binary (G10)".to_string(),
        DepFinding::Blocked(name) => format!("{name} is a blocked crate (MC-120)"),
        DepFinding::Inspect(name, version) => {
            format!("{name} {version} is flagged for inspection (MC-120): read its source on the pin day")
        }
        DepFinding::TlsFeature(name, feature) => format!("{name} has the TLS feature {feature} (G8)"),
        DepFinding::HttpInRmcp(name) => format!("{name} (HTTP crate) is below rmcp (PRB-65)"),
        DepFinding::PersistenceFeature => "eframe has the feature persistence: it would write outside the door (L7)".to_string(),
        DepFinding::CryptoInMainBinary(name) => {
            format!("{name} (crypto crate) is linked into the main binary; only sha2 is allowed (MC-356)")
        }
        DepFinding::NetCrateInXtask => {
            "xtask depends on skipper-net with its default features; only the feature catalog-ci may (MODEL-15, R13)".to_string()
        }
        DepFinding::ForbiddenBelowNetPackage(root, name) => {
            format!("{name} (HTTP, TLS, crypto or async crate) is below {root}; the sign-in helper uses WinHTTP, DPAPI and CNG through windows-sys (MC-337)")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The graph from `cargo metadata`
// ---------------------------------------------------------------------------------------------

/// The cargo that started xtask (cargo sets CARGO for `cargo run`), else `cargo` on the PATH.
pub fn cargo_program() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

/// Builds the graph from the JSON of `cargo metadata --format-version 1`.
pub fn graph_from_metadata(json: &[u8]) -> Result<Graph, String> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| format!("cargo metadata output is not JSON: {e}"))?;
    let package_list = value["packages"].as_array().ok_or("cargo metadata has no package list")?;
    let nodes = value["resolve"]["nodes"].as_array().ok_or("cargo metadata has no resolve graph")?;

    let mut info: HashMap<&str, (String, String, Option<String>)> = HashMap::new();
    for p in package_list {
        if let (Some(id), Some(name), Some(version)) = (p["id"].as_str(), p["name"].as_str(), p["version"].as_str()) {
            info.insert(id, (name.to_string(), version.to_string(), p["license"].as_str().map(str::to_string)));
        }
    }

    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut packages: Vec<Package> = Vec::new();
    let mut node_features: HashMap<&str, Vec<String>> = HashMap::new();
    for node in nodes {
        let Some(id) = node["id"].as_str() else { continue };
        let Some((name, version, licence)) = info.get(id) else { continue };
        index.insert(id, packages.len());
        packages.push(Package { name: name.clone(), version: version.clone(), licence: licence.clone() });
        let features: Vec<String> = node["features"]
            .as_array()
            .map(|a| a.iter().filter_map(|f| f.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        node_features.insert(id, features);
    }

    let mut edges: Vec<(usize, usize, Vec<String>)> = Vec::new();
    for node in nodes {
        let Some(id) = node["id"].as_str() else { continue };
        let Some(&from) = index.get(id) else { continue };
        let Some(deps) = node["deps"].as_array() else { continue };
        for dep in deps {
            let Some(to_id) = dep["pkg"].as_str() else { continue };
            let Some(&to) = index.get(to_id) else { continue };
            let normal = match dep["dep_kinds"].as_array() {
                Some(kinds) if !kinds.is_empty() => kinds.iter().any(|k| k["kind"].is_null()),
                _ => true,
            };
            if normal {
                edges.push((from, to, node_features.get(to_id).cloned().unwrap_or_default()));
            }
        }
    }
    Ok(Graph { packages, edges })
}

/// The `workspace_root` of the JSON of `cargo metadata`, if it has one (the folder of the checkout on the
/// build machine, with forward slashes).
pub fn workspace_root_from_metadata(json: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(json).ok()?;
    value["workspace_root"].as_str().map(str::to_string)
}

/// Runs `cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-gnu`; its output.
fn run_metadata() -> Result<Vec<u8>, String> {
    let out = Command::new(cargo_program())
        .args(["metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", TARGET])
        .output()
        .map_err(|e| format!("cannot start cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

/// Runs `cargo metadata` ([`run_metadata`]); returns the graph and the workspace root.
fn load_metadata() -> Result<(Graph, Option<String>), String> {
    let stdout = run_metadata()?;
    Ok((graph_from_metadata(&stdout)?, workspace_root_from_metadata(&stdout)))
}

/// Runs `cargo metadata` ([`run_metadata`]); returns the graph and, for every package, its folder, link and authors: what
/// THIRD-PARTY-LICENSES needs ([`build_licence_file`]).
pub fn load_graph_and_infos() -> Result<(Graph, Vec<PackageInfo>), String> {
    let stdout = run_metadata()?;
    Ok((graph_from_metadata(&stdout)?, package_infos_from_metadata(&stdout)?))
}

// ---------------------------------------------------------------------------------------------
// No path of the build machine in the evidence
// ---------------------------------------------------------------------------------------------

/// What the checkout folder is called in the evidence.
const REPO_MARK: &str = "<repo>";
/// What any other absolute drive path is called in the evidence.
const LOCAL_MARK: &str = "<local path>";

/// `text` with `needle` replaced by `with`, ignoring ASCII letter case.
fn replace_ci(text: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    // ASCII lower-casing keeps every byte offset, so the found offsets are valid in `text` too.
    let hay = text.to_ascii_lowercase();
    let pin = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0usize;
    while let Some(found) = hay[from..].find(&pin) {
        let start = from + found;
        out.push_str(&text[from..start]);
        out.push_str(with);
        from = start + pin.len();
    }
    out.push_str(&text[from..]);
    out
}

/// Every drive path (`X:\...` or `X:/...`, not part of a longer word) becomes `<local path>`, up to the
/// closing parenthesis, quote or line end: cargo prints a path inside parentheses, and a path may hold spaces.
fn mask_drive_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let starts_path = bytes[i].is_ascii_alphabetic()
            && bytes.get(i + 1) == Some(&b':')
            && matches!(bytes.get(i + 2), Some(b'\\') | Some(b'/'))
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric());
        if !starts_path {
            i += 1;
            continue;
        }
        out.push_str(&text[copied..i]);
        out.push_str(LOCAL_MARK);
        let mut end = i;
        while end < bytes.len() && !matches!(bytes[end], b')' | b'"' | b'\'' | b'\n' | b'\r') {
            end += 1;
        }
        i = end;
        copied = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// Keeps the folder of the build machine out of the evidence files (G18): the workspace roots become
/// `<repo>` (in both slash spellings, ignoring letter case; `cargo metadata` writes forward slashes,
/// `cargo tree` backslashes), and any other absolute drive path becomes `<local path>`.
pub fn mask_local_paths(text: &str, roots: &[&str]) -> String {
    let mut masked = text.to_string();
    for root in roots {
        let root = root.trim_end_matches(|c: char| c == '\\' || c == '/');
        if root.is_empty() {
            continue;
        }
        let backslashes = root.replace('/', "\\");
        let slashes = root.replace('\\', "/");
        masked = replace_ci(&masked, &format!("\\\\?\\{backslashes}"), REPO_MARK);
        masked = replace_ci(&masked, &backslashes, REPO_MARK);
        masked = replace_ci(&masked, &slashes, REPO_MARK);
    }
    mask_drive_paths(&masked)
}

/// The packages reachable from the named roots (roots included), sorted by name and version.
pub fn closure(g: &Graph, roots: &[&str]) -> Vec<Package> {
    let adj = adjacency(g);
    let mut start: Vec<usize> = Vec::new();
    for root in roots {
        start.extend(indices_named(g, root));
    }
    let mut packages: Vec<Package> = reachable(&adj, &start).into_iter().map(|i| g.packages[i].clone()).collect();
    packages.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    packages.dedup_by(|a, b| a.name == b.name && a.version == b.version);
    packages
}

// ---------------------------------------------------------------------------------------------
// THIRD-PARTY-LICENSES (MC-121; docs/design/third-party-notice-list.md; SP-6)
// ---------------------------------------------------------------------------------------------
//
// The file is made in two steps. `build_licence_file` reads what the build machine holds: the licence, copyright and NOTICE
// files of every shipped package from cargo's registry (the folders come from `cargo metadata`), the texts that a published crate
// does not ship from `supply-chain/notices/upstream/<package>/`, the font texts of `epaint_default_fonts`, the schema notice and
// the block of the embedded code. `licence_document` is the pure half: those inputs in, the seven parts out, or every problem
// found. A shipped package without a licence text is a problem that names it, and nothing partial is ever written.

/// The folder of the texts that a published crate does not ship (notice list 5.2 and 5.3): `<package>/<file>`, the first line of
/// each file `Source: <where>`, then the text exactly as upstream has it.
const UPSTREAM_DIR: &str = "supply-chain/notices/upstream";

/// The crate that puts the fonts into the program (notice list 5.1).
const FONT_CRATE: &str = "epaint_default_fonts";

/// The licence ids that the fonts crate names besides its code licence: they are the font licences of part 4, not groups of part 2.
const FONT_IDS: [&str; 3] = ["OFL-1.1", "Ubuntu-font-1.0", "LicenseRef-UFL-1.0"];

/// One row of the font table of part 4 (notice list 5.1).
struct FontRow {
    font: &'static str,
    built_in: &'static str,
    licence: &'static str,
    /// The file of the crate that holds the licence text of the font, copied whole.
    text_file: &'static str,
}

const FONT_ROWS: [FontRow; 5] = [
    FontRow {
        font: "Hack-Regular",
        built_in: "built in with the default fonts of eframe",
        licence: "MIT (Source Foundry Authors) and the Bitstream Vera licence (Bitstream, Inc.); the DejaVu changes are public domain",
        text_file: "Hack-Regular.txt",
    },
    FontRow {
        font: "Ubuntu-Light",
        built_in: "built in with the default fonts of eframe",
        licence: "Ubuntu Font Licence 1.0 (Canonical Ltd.)",
        text_file: "UFL.txt",
    },
    FontRow {
        font: "egui-icons",
        built_in: "built in with the default fonts of eframe",
        licence: "MIT (John Slegers)",
        text_file: "emoji-icon-font-mit-license.txt",
    },
    FontRow {
        font: "emoji-icon-font",
        built_in: "only with the crate feature monochrome_emoji_fonts",
        licence: "MIT (John Slegers)",
        text_file: "emoji-icon-font-mit-license.txt",
    },
    FontRow {
        font: "NotoEmoji-Regular",
        built_in: "only with the crate feature monochrome_emoji_fonts",
        licence: "SIL Open Font Licence 1.1 (Google Inc.)",
        text_file: "OFL.txt",
    },
];

/// The decoder of the import helper follows `puff.c`; the file that holds it (notice list 5.2): the block of part 3 stands
/// beside it, and the build stops while the file is gone.
const PUFF_FILE: &str = "crates/skipper-import/src/inflate.rs";
const PUFF_COPYRIGHT: &str = "Copyright (C) 2002-2013 Mark Adler, all rights reserved";
/// The zlib licence as `puff.c` states it (without the author's address).
const PUFF_LICENCE: [&str; 15] = [
    "This software is provided 'as-is', without any express or implied",
    "warranty.  In no event will the author be held liable for any damages",
    "arising from the use of this software.",
    "",
    "Permission is granted to anyone to use this software for any purpose,",
    "including commercial applications, and to alter it and redistribute it",
    "freely, subject to the following restrictions:",
    "",
    "1. The origin of this software must not be misrepresented; you must not",
    "   claim that you wrote the original software. If you use this software",
    "   in a product, an acknowledgment in the product documentation would be",
    "   appreciated but is not required.",
    "2. Altered source versions must be plainly marked as such, and must not be",
    "   misrepresented as being the original software.",
    "3. This notice may not be removed or altered from any source distribution.",
];

/// A text that a package of the shipped programs needs besides its main licence text and that no licence field shows (notice list
/// 5.2, "Ships? yes"). `build_licence_file` stops, naming the package and the block, while the file is not in
/// `supply-chain/notices/upstream/<package>/`; a package that ships a main licence text of its own is no exception. The files of
/// these blocks stand in part 3 and are never taken for the main text of the package.
struct RequiredBlock {
    package: &'static str,
    /// What the block is, as the message names it.
    block: &'static str,
    /// The file of the upstream folder that holds it (first line `Source: <where>`); compared without regard to case.
    file: &'static str,
}

/// The blocks that `dist` insists on. Only these rows of notice list 5.2 are enforced; the doc names each file. The rows of
/// `read-fonts` and `libm` stay until the Lead rules that the glyph table and libm are not built for Windows (notice list 7,
/// point 2): that ruling deletes the row.
const REQUIRED_BLOCKS: [RequiredBlock; 6] = [
    RequiredBlock {
        package: "accesskit",
        block: "the Chromium BSD-3-Clause block (the Chromium authors' copyright line, the conditions and the disclaimer)",
        file: "BLOCK-Chromium-BSD-3-Clause.txt",
    },
    RequiredBlock {
        package: "accesskit_consumer",
        block: "the Chromium BSD-3-Clause block (the Chromium authors' copyright line, the conditions and the disclaimer)",
        file: "BLOCK-Chromium-BSD-3-Clause.txt",
    },
    RequiredBlock {
        package: "accesskit_windows",
        block: "the Chromium BSD-3-Clause block (the Chromium authors' copyright line, the conditions and the disclaimer)",
        file: "BLOCK-Chromium-BSD-3-Clause.txt",
    },
    RequiredBlock {
        package: "futures-channel",
        block: "the Vyukov BSD-2-Clause block of src/mpsc/queue.rs (the copyright line, the two conditions and the disclaimer)",
        file: "BLOCK-Vyukov-BSD-2-Clause.txt",
    },
    RequiredBlock {
        package: "read-fonts",
        block: "the Adobe BSD-3-Clause block of the glyph list table (the copyright line, the conditions and the disclaimer)",
        file: "BLOCK-Adobe-BSD-3-Clause.txt",
    },
    RequiredBlock {
        package: "libm",
        block: "the libm notices that the crate's own LICENSE.txt lacks (at least the FreeBSD BSD-2-Clause block)",
        file: "BLOCK-libm-notices.txt",
    },
];

/// The folder of the settings catalogue sources, and the two files of the schema notice in each version folder (notice list 4.3).
const CC_SRC: &str = "xtask/data/cc-src";
const SCHEMA_NOTICE: &str = "NOTICE-schema.txt";
const SCHEMA_LICENCE: &str = "LICENSE-schema-Apache-2.0.txt";

/// The address of the Epoch AI table that the catalogue job fetches (`EPOCH_URL` of `catalog_net.rs`, which a test compares).
const EPOCH_DATA_URL: &str = "https://epoch.ai/data/benchmark_data/benchmarks_runs.csv";

/// The largest text file of a package that is read; a bigger one stops the build rather than being cut.
const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

/// The licence of a package whose manifest names none: its files are printed as they are.
const UNSPECIFIED: &str = "Licence stated in the package files only";

/// How much Skipper prefers a licence when an expression offers a choice, best first (notice list section 3).
const PREFERENCE: [&str; 12] = [
    "MIT",
    "Apache-2.0",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "BSL-1.0",
    "0BSD",
    "CC0-1.0",
    "Unlicense",
    "MPL-2.0",
];
/// The rank of a licence that is not in the list, and of a copyleft licence (never recorded).
const OTHER_RANK: u32 = 50;
const COPYLEFT_RANK: u32 = 1000;

// ---- the inputs of the pure half ----

/// One text file of a package: its name, its text (LF line ends, no byte order mark) and, for a text of the upstream folder, where
/// it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct TextFile {
    pub name: String,
    pub text: String,
    pub source: Option<String>,
}

/// One shipped package as THIRD-PARTY-LICENSES needs it.
#[derive(Clone, Debug)]
pub struct Component {
    pub name: String,
    pub version: String,
    /// The `license` field of the manifest, an SPDX expression.
    pub licence: Option<String>,
    /// A package of this workspace: Skipper's own LICENSE covers it, no third-party text is needed.
    pub own: bool,
    /// The repository (or home page) of the package.
    pub link: Option<String>,
    /// The names of the package authors (no addresses).
    pub authors: Vec<String>,
    /// The licence texts of the package folder, then the texts of the upstream folder.
    pub licence_files: Vec<TextFile>,
    pub notice_files: Vec<TextFile>,
    /// The font licence texts found in the crate folder (read for the fonts crate only).
    pub font_files: Vec<TextFile>,
}

/// A block of third-party code that no licence field shows (part 3).
#[derive(Clone, Debug)]
pub struct EmbeddedBlock {
    pub title: String,
    /// The file of this repository that holds the code.
    pub file: String,
    /// What the code is and how it was changed.
    pub note: String,
    /// The licence id.
    pub licence: String,
    pub copyright: String,
    pub text: String,
}

/// The SchemaStore notice lines and the Apache-2.0 text that stand beside the committed schema (part 6).
#[derive(Clone, Debug)]
pub struct SchemaNotice {
    pub notice: String,
    pub licence: String,
}

/// Everything `licence_document` needs.
pub struct LicenceInput {
    /// The version of Skipper, named in part 0.
    pub version: String,
    pub components: Vec<Component>,
    pub embedded: Vec<EmbeddedBlock>,
    pub schema: SchemaNotice,
    /// The attribution lines of the model catalogue.
    pub catalogue_lines: Vec<String>,
    /// (title, notice) of the statically linked runtime (part 7).
    pub runtime: Vec<(String, String)>,
}

// ---- SPDX expressions ----

fn licence_tokens(expression: &str) -> Vec<String> {
    let spaced = expression.replace('/', " OR ").replace('(', " ( ").replace(')', " ) ");
    spaced.split_whitespace().map(str::to_string).collect()
}

#[derive(Clone, Debug, PartialEq)]
enum Spdx {
    Id(String),
    Or(Vec<Spdx>),
    And(Vec<Spdx>),
}

fn spdx_or(t: &[String], pos: &mut usize) -> Spdx {
    let mut parts = vec![spdx_and(t, pos)];
    while *pos < t.len() && t[*pos].eq_ignore_ascii_case("or") {
        *pos += 1;
        parts.push(spdx_and(t, pos));
    }
    if parts.len() == 1 { parts.remove(0) } else { Spdx::Or(parts) }
}

fn spdx_and(t: &[String], pos: &mut usize) -> Spdx {
    let mut parts = vec![spdx_atom(t, pos)];
    while *pos < t.len() && t[*pos].eq_ignore_ascii_case("and") {
        *pos += 1;
        parts.push(spdx_atom(t, pos));
    }
    if parts.len() == 1 { parts.remove(0) } else { Spdx::And(parts) }
}

/// An id, a parenthesised expression, or (at the end of the input or at a stray `)`) the empty id, which `ranked_options` refuses.
fn spdx_atom(t: &[String], pos: &mut usize) -> Spdx {
    if *pos >= t.len() || t[*pos] == ")" {
        return Spdx::Id(String::new());
    }
    if t[*pos] == "(" {
        *pos += 1;
        let inner = spdx_or(t, pos);
        if *pos < t.len() && t[*pos] == ")" {
            *pos += 1;
        }
        return inner;
    }
    let id = t[*pos].clone();
    *pos += 1;
    // "<licence> WITH <exception>": the exception text stands in the licence file of the package.
    if *pos + 1 < t.len() && t[*pos].eq_ignore_ascii_case("with") {
        *pos += 2;
    }
    Spdx::Id(id)
}

/// Every set of licences that satisfies the expression: an OR gives alternatives, an AND combines.
fn options(e: &Spdx) -> Vec<Vec<String>> {
    match e {
        Spdx::Id(id) => vec![vec![id.clone()]],
        Spdx::Or(parts) => parts.iter().flat_map(options).collect(),
        Spdx::And(parts) => {
            let mut acc: Vec<Vec<String>> = vec![Vec::new()];
            for part in parts {
                let mut next: Vec<Vec<String>> = Vec::new();
                for base in &acc {
                    for choice in options(part) {
                        let mut set = base.clone();
                        for id in choice {
                            if !set.contains(&id) {
                                set.push(id);
                            }
                        }
                        next.push(set);
                    }
                }
                acc = next;
            }
            acc
        }
    }
}

fn rank_of(id: &str) -> u32 {
    if let Some(at) = PREFERENCE.iter().position(|p| *p == id) {
        return at as u32;
    }
    let upper = id.to_ascii_uppercase();
    if upper.starts_with("GPL") || upper.starts_with("LGPL") || upper.starts_with("AGPL") {
        return COPYLEFT_RANK;
    }
    OTHER_RANK
}

fn set_rank(set: &[String]) -> (u32, usize) {
    (set.iter().map(|id| rank_of(id)).max().unwrap_or(0), set.len())
}

/// The sets of licences that a package may be recorded under, best first (notice list section 3): every OR alternative, the
/// copyleft ones left out, the most preferred first (MIT, then Apache-2.0, ...). For the fonts crate the font licences are left
/// out of the sets: part 4 carries them.
fn ranked_options(expression: &str, font_crate: bool) -> Result<Vec<Vec<String>>, String> {
    let tokens = licence_tokens(expression);
    let mut pos = 0usize;
    let parsed = spdx_or(&tokens, &mut pos);
    if tokens.is_empty() || pos != tokens.len() {
        return Err(format!("the licence expression {expression:?} cannot be read"));
    }
    let mut sets = options(&parsed);
    if sets.iter().any(|set| set.iter().any(|id| id.is_empty())) {
        return Err(format!("the licence expression {expression:?} cannot be read"));
    }
    if font_crate {
        for set in sets.iter_mut() {
            set.retain(|id| !FONT_IDS.contains(&id.as_str()));
        }
    }
    sets.retain(|set| !set.iter().any(|id| rank_of(id) >= COPYLEFT_RANK));
    if sets.is_empty() {
        return Err(format!("the licence expression {expression:?} offers only a copyleft licence, which Skipper does not ship"));
    }
    sets.sort_by_key(|set| set_rank(set));
    Ok(sets)
}

// ---- the texts ----

/// The text with every run of white space as one space and in lower case: what two wrappings of one text have in common.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

/// The licences whose own words a text holds.
fn detect(text: &str) -> Vec<&'static str> {
    let words = flat(text);
    let has = |needle: &str| words.contains(needle);
    let mut found: Vec<&'static str> = Vec::new();
    if has("permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files") {
        found.push("MIT");
    }
    if has("apache license") && has("version 2.0") {
        found.push("Apache-2.0");
    }
    if has("altered source versions must be plainly marked as such") {
        found.push("Zlib");
    }
    if has("redistribution and use in source and binary forms") {
        if has("neither the name of") {
            found.push("BSD-3-Clause");
        } else {
            found.push("BSD-2-Clause");
        }
    }
    if has("permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted") {
        if has("provided that the above copyright notice") {
            found.push("ISC");
        } else {
            found.push("0BSD");
        }
    }
    if has("unicode license v3") {
        found.push("Unicode-3.0");
    }
    if has("sil open font license") {
        found.push("OFL-1.1");
    }
    if has("ubuntu font licence") {
        found.push("Ubuntu-font-1.0");
    }
    if has("boost software license") {
        found.push("BSL-1.0");
    }
    if has("cc0 1.0 universal") {
        found.push("CC0-1.0");
    }
    if has("free and unencumbered software released into the public domain") {
        found.push("Unlicense");
    }
    if has("mozilla public license") && has("version 2.0") {
        found.push("MPL-2.0");
    }
    found
}

/// A line that names a holder: it starts with "Copyright", "(c)" or the copyright sign and has a year. The template lines of a
/// licence ("Copyright [yyyy] [name of copyright owner]") and the words of its sentences are none.
fn is_copyright_line(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    let starts = lower.starts_with("copyright") || lower.starts_with("(c)") || trimmed.starts_with('\u{a9}');
    starts
        && trimmed.len() <= 300
        && trimmed.chars().any(|c| c.is_ascii_digit())
        && !lower.contains("yyyy")
        && !lower.contains("[year]")
        && !lower.contains("<year>")
}

/// The copyright lines of a text, trimmed, each once, in the order of the text.
fn copyright_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if is_copyright_line(trimmed) && !out.iter().any(|seen| seen == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// The text without its copyright lines, without trailing blanks and with at most one blank line between paragraphs: the part
/// that two packages under one licence have in common.
fn body_of(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0usize;
    for line in text.lines() {
        let kept = line.trim_end();
        if is_copyright_line(kept.trim()) {
            continue;
        }
        if kept.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() && blank > 0 {
            out.push('\n');
        }
        blank = 0;
        out.push_str(kept);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Where the copyright of a package is named when its texts hold no line: the manifest's authors, else nobody.
fn fallback_lines(c: &Component) -> Vec<String> {
    if c.authors.is_empty() {
        vec!["no copyright line published".to_string()]
    } else {
        vec![format!("Copyright by the package authors (from the manifest): {}", c.authors.join(", "))]
    }
}

/// Whether `file` of the package `package` is the file of a required block: a text of the upstream folder with the fixed name.
fn is_required_block(package: &str, file: &TextFile) -> bool {
    file.source.is_some() && REQUIRED_BLOCKS.iter().any(|row| row.package == package && row.file.eq_ignore_ascii_case(&file.name))
}

/// One problem per package of the shipped programs whose required block (`REQUIRED_BLOCKS`) is not among its upstream texts.
fn missing_blocks(packages: &[&Component]) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    for c in packages {
        for row in REQUIRED_BLOCKS.iter().filter(|row| row.package == c.name) {
            let present = c.licence_files.iter().any(|f| f.source.is_some() && f.name.eq_ignore_ascii_case(row.file));
            if !present {
                problems.push(format!(
                    "{} {}: {} is missing. Put it into {UPSTREAM_DIR}/{}/{} with the first line \"Source: <where>\", then the text exactly as \
                     upstream has it (part 3 of THIRD-PARTY-LICENSES carries it, also for a package that ships a licence text of its own).",
                    c.name, c.version, row.block, c.name, row.file
                ));
            }
        }
    }
    problems
}

/// The files among `eligible` (indexes into `files`) that carry the text of licence `id`: those whose words say so; with
/// `lenient`, when there are none, the files whose words say nothing about any licence.
fn pick_files(files: &[TextFile], eligible: &[usize], id: &str, lenient: bool) -> Vec<usize> {
    let matching: Vec<usize> =
        eligible.iter().copied().filter(|&i| detect(&files[i].text).iter().any(|d| *d == id)).collect();
    if !matching.is_empty() || !lenient {
        return matching;
    }
    eligible.iter().copied().filter(|&i| detect(&files[i].text).is_empty()).collect()
}

/// The licences Skipper records for a component and the files that carry their texts (indexes into `licence_files`), or why
/// there is no way to write the file. A choice offered by the manifest is made by preference, but only among the licences for
/// which the package has a text: first by texts that say which licence they are, then also by texts that say nothing.
fn choose(c: &Component) -> Result<Vec<(String, Vec<usize>)>, String> {
    // The file of a required block is never the main text of a package: a block that names no licence would otherwise pass the
    // lenient pass below as the licence text and hide that the real text is missing.
    let all: Vec<usize> =
        (0..c.licence_files.len()).filter(|&i| !is_required_block(&c.name, &c.licence_files[i])).collect();
    let Some(expression) = c.licence.as_deref() else {
        if all.is_empty() {
            return Err(format!(
                "no licence in the manifest and no licence text. Put the upstream text into {UPSTREAM_DIR}/{}/ with the first line \"Source: <where>\".",
                c.name
            ));
        }
        return Ok(vec![(UNSPECIFIED.to_string(), all)]);
    };
    let sets = ranked_options(expression, c.name == FONT_CRATE)?;
    for lenient in [false, true] {
        for set in &sets {
            let mut picked: Vec<(String, Vec<usize>)> = Vec::new();
            let mut complete = true;
            for id in set {
                let found = pick_files(&c.licence_files, &all, id, lenient);
                if found.is_empty() {
                    complete = false;
                    break;
                }
                picked.push((id.clone(), found));
            }
            if complete {
                return Ok(picked);
            }
        }
    }
    let wanted: Vec<&str> = sets[0].iter().map(String::as_str).collect();
    let seen: Vec<&str> = c.licence_files.iter().map(|f| f.name.as_str()).collect();
    Err(format!(
        "no licence text for {} (licence expression {expression}; text files found: {}). Put the upstream text into {UPSTREAM_DIR}/{}/ with the first line \"Source: <where>\".",
        wanted.join(" + "),
        if seen.is_empty() { "none".to_string() } else { seen.join(", ") },
        c.name
    ))
}

// ---- the document ----

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn heading(out: &mut String, title: &str, mark: char) {
    out.push_str(title);
    out.push('\n');
    out.push_str(&mark.to_string().repeat(title.chars().count()));
    out.push_str("\n\n");
}

/// One component of a group, with the lines it is credited by in one text.
struct Member {
    name: String,
    version: String,
    source: Option<String>,
    lines: Vec<String>,
}

/// One wording of a licence text and the components that use it.
struct Variant {
    key: String,
    body: String,
    members: Vec<Member>,
}

/// The components that Skipper records under one licence.
struct Group {
    id: String,
    members: BTreeSet<String>,
    variants: Vec<Variant>,
}

/// Part 4: the font table and the licence texts of the fonts crate, or the statement that no font is bundled. A missing text is a
/// problem.
fn fonts_part(font_crate: Option<&Component>, problems: &mut Vec<String>) -> String {
    let mut out = String::new();
    heading(&mut out, "Part 4 - Fonts", '-');
    let Some(krate) = font_crate else {
        out.push_str("No font is bundled in this release (no font crate is part of the shipped programs).\n\n");
        return out;
    };
    out.push_str(&format!(
        "{} {} puts the fonts below into the program (the default fonts of eframe turn the crate on). Skipper's own licence does not \
         cover them: each font keeps its own licence, and the text of each is copied here unchanged.\n\n",
        krate.name, krate.version
    ));
    for row in FONT_ROWS.iter() {
        out.push_str(&format!("  {} - {} - {}\n", row.font, row.built_in, row.licence));
    }
    out.push('\n');
    let mut shown: Vec<&str> = Vec::new();
    for row in FONT_ROWS.iter() {
        if shown.contains(&row.text_file) {
            continue;
        }
        shown.push(row.text_file);
        let users: Vec<&str> = FONT_ROWS.iter().filter(|r| r.text_file == row.text_file).map(|r| r.font).collect();
        match krate.font_files.iter().find(|f| f.name.eq_ignore_ascii_case(row.text_file)) {
            Some(file) => {
                heading(&mut out, &format!("4.{} {} (licence text of {})", shown.len(), row.text_file, users.join(", ")), '~');
                out.push_str(file.text.trim_end());
                out.push_str("\n\n");
            }
            None => problems.push(format!(
                "{} {}: the font licence text {} is missing in the crate folder; a font must not ship without its licence text",
                krate.name, krate.version, row.text_file
            )),
        }
    }
    out
}

/// The attribution block of the Epoch AI data (notice list 4.4; CC BY 4.0 section 3(a)).
fn epoch_block() -> String {
    let mut out = String::new();
    out.push_str("The benchmark priors of the catalogue are derived from data of Epoch AI:\n\n");
    out.push_str("  Creator:  Epoch AI\n");
    out.push_str("  Title:    \"Capabilities & benchmarking\"\n");
    out.push_str(&format!("  Data:     {EPOCH_DATA_URL}\n"));
    out.push_str("  Licence:  CC BY 4.0, https://creativecommons.org/licenses/by/4.0/\n");
    out.push_str("  Changes:  Skipper reduced the data to per-model scores.\n");
    out.push_str("  Warranty: the data is offered as is, with no warranty of any kind (CC BY 4.0, section 5).\n\n");
    out.push_str(
        "Rows that Epoch AI took over from other projects may be under the licence of those projects; this credit does not make \
         them CC BY 4.0.\n\n",
    );
    out
}

/// THIRD-PARTY-LICENSES from its inputs: the seven parts of `docs/design/third-party-notice-list.md` section 2, or every problem
/// found (a package without a licence text, an expression nobody can read, a copyleft-only package, a missing font text, a
/// package of `REQUIRED_BLOCKS` without its block). The same components in any order give the same bytes.
pub fn licence_document(input: &LicenceInput) -> Result<String, Vec<String>> {
    let mut problems: Vec<String> = Vec::new();

    let mut comps: Vec<&Component> = input.components.iter().collect();
    comps.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    comps.dedup_by(|a, b| a.name == b.name && a.version == b.version);
    let own: Vec<&Component> = comps.iter().copied().filter(|c| c.own).collect();
    let third: Vec<&Component> = comps.iter().copied().filter(|c| !c.own).collect();

    let mut chosen: Vec<Vec<(String, Vec<usize>)>> = Vec::new();
    for c in &third {
        match choose(c) {
            Ok(list) => chosen.push(list),
            Err(why) => {
                problems.push(format!("{} {}: {why}", c.name, c.version));
                chosen.push(Vec::new());
            }
        }
    }
    problems.extend(missing_blocks(&third));
    let fonts = fonts_part(third.iter().copied().find(|c| c.name == FONT_CRATE), &mut problems);
    if !problems.is_empty() {
        return Err(problems);
    }

    // Part 2 first: the groups by licence used, each text once.
    let mut groups: BTreeMap<(u32, String), Group> = BTreeMap::new();
    for (c, list) in third.iter().zip(&chosen) {
        for (id, indexes) in list {
            let group = groups
                .entry((rank_of(id), id.clone()))
                .or_insert_with(|| Group { id: id.clone(), members: BTreeSet::new(), variants: Vec::new() });
            group.members.insert(format!("{} {}", c.name, c.version));
            for &index in indexes {
                let file = &c.licence_files[index];
                let body = body_of(&file.text);
                let key = flat(&body);
                let mut lines = copyright_lines(&file.text);
                if lines.is_empty() {
                    lines = fallback_lines(c);
                }
                let at = match group.variants.iter().position(|v| v.key == key) {
                    Some(at) => at,
                    None => {
                        group.variants.push(Variant { key, body, members: Vec::new() });
                        group.variants.len() - 1
                    }
                };
                let variant = &mut group.variants[at];
                match variant.members.iter_mut().find(|m| m.name == c.name && m.version == c.version) {
                    Some(member) => {
                        for line in lines {
                            if !member.lines.contains(&line) {
                                member.lines.push(line);
                            }
                        }
                    }
                    None => variant.members.push(Member {
                        name: c.name.clone(),
                        version: c.version.clone(),
                        source: file.source.clone(),
                        lines,
                    }),
                }
            }
        }
    }

    let mut out = String::new();
    heading(&mut out, "THIRD-PARTY-LICENSES", '=');

    heading(&mut out, "Part 0 - About this file", '-');
    out.push_str(&format!(
        "Skipper {} includes the third-party components below. Where a component offers a choice of licences, Skipper uses it under \
         the licence named here.\n\n",
        input.version
    ));
    out.push_str(
        "Skipper's own packages are under the MIT licence of the file LICENSE next to this one. That licence does not cover the \
         components, the fonts and the data named here.\n\n",
    );
    out.push_str(
        "The parts: 1 index of the components, 2 the licences used (each text once, with the copyright lines of its components), \
         3 embedded third-party code, 4 fonts, 5 NOTICE texts, 6 data and schemas, 7 runtime.\n\n",
    );

    heading(&mut out, "Part 1 - Index", '-');
    out.push_str(&format!("{} third-party component{}.\n\n", third.len(), plural(third.len())));
    for (c, list) in third.iter().zip(&chosen) {
        let mut used = list.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(" + ");
        if c.name == FONT_CRATE {
            used.push_str(" + the font licences of part 4");
        }
        let link = c.link.clone().unwrap_or_else(|| format!("https://crates.io/crates/{}/{}", c.name, c.version));
        out.push_str(&format!("{} {} - {} - {}\n", c.name, c.version, used, link));
    }
    out.push('\n');
    if !own.is_empty() {
        let names: Vec<&str> = own.iter().map(|c| c.name.as_str()).collect();
        out.push_str(&format!("Packages of Skipper itself ({}): {} - MIT, see the file LICENSE.\n\n", own.len(), names.join(", ")));
    }

    heading(&mut out, "Part 2 - Licences used", '-');
    if groups.is_empty() {
        out.push_str("No third-party component needs a licence text.\n\n");
    }
    for (n, group) in groups.values().enumerate() {
        let count = group.members.len();
        heading(&mut out, &format!("2.{} {} ({} component{})", n + 1, group.id, count, plural(count)), '~');
        let several = group.variants.len() > 1;
        for (i, variant) in group.variants.iter().enumerate() {
            if several {
                out.push_str(&format!("Text {} of {}\n\n", i + 1, group.variants.len()));
            }
            out.push_str("Components and their copyright lines:\n\n");
            for member in &variant.members {
                let from = match &member.source {
                    Some(source) => format!(" (text from {source})"),
                    None => String::new(),
                };
                out.push_str(&format!("  {} {}{}\n", member.name, member.version, from));
                for line in &member.lines {
                    out.push_str(&format!("    {line}\n"));
                }
            }
            out.push_str("\nLicence text:\n\n");
            out.push_str(&variant.body);
            out.push_str("\n\n");
        }
    }

    heading(&mut out, "Part 3 - Embedded third-party code", '-');
    let mut embedded = 0usize;
    for block in &input.embedded {
        embedded += 1;
        heading(&mut out, &format!("3.{embedded} {} - {}", block.title, block.licence), '~');
        out.push_str(&format!("In: {}\n{}\n\n{}\n\n{}\n\n", block.file, block.note, block.copyright, block.text.trim_end()));
    }
    for (c, list) in third.iter().zip(&chosen) {
        let used: Vec<usize> = list.iter().flat_map(|(_, indexes)| indexes.iter().copied()).collect();
        for (i, file) in c.licence_files.iter().enumerate() {
            let Some(source) = &file.source else { continue };
            if used.contains(&i) {
                continue;
            }
            embedded += 1;
            heading(&mut out, &format!("3.{embedded} {} {} - {}", c.name, c.version, file.name), '~');
            out.push_str(&format!("Source: {source}\n\n{}\n\n", file.text.trim_end()));
        }
    }
    if embedded == 0 {
        out.push_str("None.\n\n");
    }

    out.push_str(&fonts);

    heading(&mut out, "Part 5 - NOTICE texts", '-');
    let mut notices = 0usize;
    for c in &third {
        for file in &c.notice_files {
            notices += 1;
            heading(&mut out, &format!("{} {} - {}", c.name, c.version, file.name), '~');
            out.push_str(file.text.trim_end());
            out.push_str("\n\n");
        }
    }
    if notices == 0 {
        out.push_str("No package of this release ships a NOTICE file. ");
    }
    out.push_str("The NOTICE lines of the SchemaStore settings schema are in part 6.\n\n");

    heading(&mut out, "Part 6 - Data and schemas", '-');
    heading(&mut out, "6.1 Model catalogue", '~');
    out.push_str("The attribution lines of the model catalogue, as the Models page shows them:\n\n");
    for line in &input.catalogue_lines {
        out.push_str(&format!("  {line}\n"));
    }
    out.push('\n');
    out.push_str(&epoch_block());
    heading(&mut out, "6.2 SchemaStore settings schema (Apache-2.0)", '~');
    out.push_str(input.schema.notice.trim_end());
    out.push_str("\n\nLicence text of the schema:\n\n");
    out.push_str(input.schema.licence.trim_end());
    out.push_str("\n\n");

    heading(&mut out, "Part 7 - Runtime (statically linked)", '-');
    for (title, notice) in &input.runtime {
        out.push_str(title);
        out.push('\n');
        out.push_str(notice.trim_end());
        out.push_str("\n\n");
    }

    Ok(format!("{}\n", out.trim_end()))
}

// ---- reading the build machine ----

/// A package of `cargo metadata`: where its sources are and what its manifest says about its authors and its home.
#[derive(Clone, Debug)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    /// A package of this workspace (`cargo metadata` names no `source` for it).
    pub own: bool,
    /// The folder of the package (the folder of its manifest).
    pub dir: Option<PathBuf>,
    pub link: Option<String>,
    pub authors: Vec<String>,
}

/// An author as the manifest writes it ("Name <address>") without the address: no address of a person goes into the file.
fn author_name(raw: &str) -> String {
    match raw.find('<') {
        Some(at) => raw[..at].trim().to_string(),
        None => raw.trim().to_string(),
    }
}

/// The packages of the JSON of `cargo metadata --format-version 1`, with folder, link and authors.
pub fn package_infos_from_metadata(json: &[u8]) -> Result<Vec<PackageInfo>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| format!("cargo metadata output is not JSON: {e}"))?;
    let list = value["packages"].as_array().ok_or("cargo metadata has no package list")?;
    let mut out: Vec<PackageInfo> = Vec::new();
    for p in list {
        let (Some(name), Some(version)) = (p["name"].as_str(), p["version"].as_str()) else { continue };
        let dir = p["manifest_path"].as_str().and_then(|manifest| Path::new(manifest).parent()).map(Path::to_path_buf);
        let link = p["repository"]
            .as_str()
            .or_else(|| p["homepage"].as_str())
            .filter(|link| !link.trim().is_empty())
            .map(str::to_string);
        let authors: Vec<String> = p["authors"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(author_name)).filter(|n| !n.is_empty()).collect())
            .unwrap_or_default();
        out.push(PackageInfo { name: name.to_string(), version: version.to_string(), own: p["source"].is_null(), dir, link, authors });
    }
    Ok(out)
}

/// The entries of a folder as (name, path, is a folder), sorted by name; links and other things that are neither a file nor a
/// folder are left out. `label` names the folder in a message (no path of this machine goes into one).
fn list_dir(dir: &Path, label: &str) -> Result<Vec<(String, PathBuf, bool)>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{label}: cannot read the folder: {e}"))?;
    let mut out: Vec<(String, PathBuf, bool)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{label}: cannot read the folder: {e}"))?;
        let Ok(name) = entry.file_name().into_string() else { continue };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_file() {
            out.push((name, entry.path(), false));
        } else if kind.is_dir() {
            out.push((name, entry.path(), true));
        }
    }
    out.sort();
    Ok(out)
}

/// The text of a file as printed: lossy UTF-8, no byte order mark, LF line ends.
fn clean_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text: &str = text.strip_prefix('\u{feff}').unwrap_or(&text);
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn read_text(path: &Path) -> Result<String, String> {
    let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_TEXT_BYTES {
        return Err(format!("the file is larger than {MAX_TEXT_BYTES} bytes"));
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    Ok(clean_text(&bytes))
}

fn is_licence_name(lower: &str) -> bool {
    lower.starts_with("licen") || lower.starts_with("copying") || lower.starts_with("unlicense")
}

/// The licence files and the NOTICE files of a package folder: the files at its top whose names start with `licen`, `copying`,
/// `unlicense` or `notice` (any case), the files of a folder `licenses` at its top, and a `COPYRIGHT` file when it holds a
/// licence text. Nothing deeper is read.
fn read_package_texts(label: &str, dir: &Path) -> Result<(Vec<TextFile>, Vec<TextFile>), String> {
    let mut licences: Vec<TextFile> = Vec::new();
    let mut notices: Vec<TextFile> = Vec::new();
    for (name, path, is_dir) in list_dir(dir, label)? {
        let lower = name.to_ascii_lowercase();
        if is_dir {
            if matches!(lower.as_str(), "licenses" | "license" | "licences" | "licence") {
                for (inner, inner_path, inner_dir) in list_dir(&path, label)? {
                    if inner_dir {
                        continue;
                    }
                    let text = read_text(&inner_path).map_err(|e| format!("{label}: {name}/{inner}: {e}"))?;
                    licences.push(TextFile { name: format!("{name}/{inner}"), text, source: None });
                }
            }
            continue;
        }
        let licence_name = is_licence_name(&lower);
        let notice_name = lower.starts_with("notice");
        let copyright_name = lower.starts_with("copyright");
        if !(licence_name || notice_name || copyright_name) {
            continue;
        }
        let text = read_text(&path).map_err(|e| format!("{label}: {name}: {e}"))?;
        if notice_name {
            notices.push(TextFile { name, text, source: None });
        } else if licence_name || !detect(&text).is_empty() {
            licences.push(TextFile { name, text, source: None });
        }
    }
    Ok((licences, notices))
}

/// A supplied text: the first line `Source: <where>`, then the text as upstream has it. The source and the text.
fn split_source(raw: &str) -> Result<(String, String), &'static str> {
    let (first, rest) = raw.split_once('\n').unwrap_or((raw, ""));
    let Some(source) = first.strip_prefix("Source:") else {
        return Err("the first line must be `Source: <where>`");
    };
    let source = source.trim();
    if source.is_empty() {
        return Err("the first line must be `Source: <where>`");
    }
    if rest.trim().is_empty() {
        return Err("no text follows the Source line");
    }
    Ok((source.to_string(), rest.to_string()))
}

/// The texts that the published crate `package` does not ship, from `supply-chain/notices/upstream/<package>/` below `root`
/// (none when the folder does not exist).
fn read_upstream_texts(root: &Path, package: &str) -> Result<Vec<TextFile>, String> {
    let dir = root.join(UPSTREAM_DIR).join(package);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let label = format!("{package} ({UPSTREAM_DIR})");
    let mut out: Vec<TextFile> = Vec::new();
    for (name, path, is_dir) in list_dir(&dir, &label)? {
        if is_dir {
            continue;
        }
        let raw = read_text(&path).map_err(|e| format!("{package}: {UPSTREAM_DIR}/{package}/{name}: {e}"))?;
        let (source, text) =
            split_source(&raw).map_err(|why| format!("{package}: {UPSTREAM_DIR}/{package}/{name}: {why}"))?;
        out.push(TextFile { name, text, source: Some(source) });
    }
    Ok(out)
}

/// Finds the font text files by name below `dir`, up to three folders deep.
fn find_font_texts(dir: &Path, label: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    for (name, path, is_dir) in list_dir(dir, label)? {
        if is_dir {
            if depth > 0 {
                find_font_texts(&path, label, depth - 1, out)?;
            }
        } else if FONT_ROWS.iter().any(|row| row.text_file.eq_ignore_ascii_case(&name))
            && !out.iter().any(|(seen, _)| seen.eq_ignore_ascii_case(&name))
        {
            out.push((name, path));
        }
    }
    Ok(())
}

/// The font licence texts of the fonts crate, found by name below its folder.
fn read_font_texts(label: &str, dir: &Path) -> Result<Vec<TextFile>, String> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    find_font_texts(dir, label, 3, &mut found)?;
    let mut out: Vec<TextFile> = Vec::new();
    for (name, path) in found {
        let text = read_text(&path).map_err(|e| format!("{label}: {name}: {e}"))?;
        out.push(TextFile { name, text, source: None });
    }
    Ok(out)
}

/// The block of part 3 for the decoder of the import helper (notice list 5.2). The build stops while the file that holds the
/// decoder is gone, so a block never outlives its code unnoticed.
fn embedded_blocks(root: &Path) -> Result<Vec<EmbeddedBlock>, String> {
    if !root.join(PUFF_FILE).is_file() {
        return Err(format!(
            "{PUFF_FILE} is missing, but part 3 of THIRD-PARTY-LICENSES carries the notice of the code it follows (puff.c, zlib licence): \
             change the block in xtask/src/deps.rs together with the file"
        ));
    }
    Ok(vec![EmbeddedBlock {
        title: "puff.c (zlib contrib/puff, Mark Adler)".to_string(),
        file: PUFF_FILE.to_string(),
        note: "The DEFLATE decoder of the import helper follows the structure of puff.c, the small inflate of zlib's contrib folder. \
               It was translated to Rust and changed by the Skipper project, so it is an altered version of that code, marked as such \
               here (condition 2 of the licence below)."
            .to_string(),
        licence: "Zlib".to_string(),
        copyright: PUFF_COPYRIGHT.to_string(),
        text: PUFF_LICENCE.join("\n"),
    }])
}

/// The numbers of a version folder name, for ordering ("2.1.318" comes after "2.1.99").
fn version_key(name: &str) -> Vec<u64> {
    name.split('.').map(|part| part.parse::<u64>().unwrap_or(0)).collect()
}

/// The schema notice and the Apache-2.0 text from the newest version folder of `xtask/data/cc-src` (notice list 4.3).
fn read_schema_notice(root: &Path) -> Result<SchemaNotice, String> {
    let base = root.join(CC_SRC);
    let mut versions: Vec<(Vec<u64>, String)> = Vec::new();
    for (name, _, is_dir) in list_dir(&base, CC_SRC)? {
        if is_dir {
            versions.push((version_key(&name), name));
        }
    }
    versions.sort();
    let Some((_, newest)) = versions.pop() else {
        return Err(format!("{CC_SRC} holds no version folder: the SchemaStore notice (Apache-2.0) cannot be part 6"));
    };
    let dir = base.join(&newest);
    let notice = read_text(&dir.join(SCHEMA_NOTICE)).map_err(|e| format!("{CC_SRC}/{newest}/{SCHEMA_NOTICE}: {e}"))?;
    let licence = read_text(&dir.join(SCHEMA_LICENCE)).map_err(|e| format!("{CC_SRC}/{newest}/{SCHEMA_LICENCE}: {e}"))?;
    Ok(SchemaNotice { notice, licence })
}

/// The attribution lines of the model catalogue, read by name from the catalogue build (`xtask::catalog`).
fn catalogue_lines() -> Vec<String> {
    vec![ANTHROPIC_LINE.to_string(), EPOCH_LINE.to_string()]
}

/// One package of the closure with everything read from its folder.
fn read_component(root: &Path, package: &Package, infos: &[PackageInfo]) -> Result<Component, String> {
    let label = format!("{} {}", package.name, package.version);
    let Some(info) = infos.iter().find(|i| i.name == package.name && i.version == package.version) else {
        return Err(format!("{label}: not in the output of cargo metadata (no folder, no authors)"));
    };
    let mut component = Component {
        name: package.name.clone(),
        version: package.version.clone(),
        licence: package.licence.clone(),
        own: info.own,
        link: info.link.clone(),
        authors: info.authors.clone(),
        licence_files: Vec::new(),
        notice_files: Vec::new(),
        font_files: Vec::new(),
    };
    if info.own {
        return Ok(component);
    }
    let Some(dir) = info.dir.as_ref().filter(|d| d.is_dir()) else {
        return Err(format!("{label}: its source folder is not in cargo's registry on this machine (fetch the sources, then run dist again)"));
    };
    let (licences, notices) = read_package_texts(&label, dir)?;
    component.licence_files = licences;
    component.licence_files.extend(read_upstream_texts(root, &package.name)?);
    component.notice_files = notices;
    if package.name == FONT_CRATE {
        component.font_files = read_font_texts(&label, dir)?;
    }
    Ok(component)
}

fn failure_text(problems: &[String]) -> String {
    let mut out = format!("THIRD-PARTY-LICENSES cannot be written; {} problem{}:", problems.len(), plural(problems.len()));
    for problem in problems {
        out.push_str("\n  - ");
        out.push_str(problem);
    }
    out
}

/// THIRD-PARTY-LICENSES for the shipped `packages`, read from the build machine: `root` is the workspace root, `infos` the
/// packages of `cargo metadata`, `runtime` the notices of the statically linked runtime. Fails closed: the reason names every
/// package whose text is missing, and no file is made.
pub fn build_licence_file(
    root: &Path,
    version: &str,
    packages: &[Package],
    infos: &[PackageInfo],
    runtime: &[(String, String)],
) -> Result<String, String> {
    let mut problems: Vec<String> = Vec::new();
    let mut components: Vec<Component> = Vec::new();
    for package in packages {
        match read_component(root, package, infos) {
            Ok(component) => components.push(component),
            Err(why) => problems.push(why),
        }
    }
    let embedded = match embedded_blocks(root) {
        Ok(blocks) => blocks,
        Err(why) => {
            problems.push(why);
            Vec::new()
        }
    };
    let schema = match read_schema_notice(root) {
        Ok(schema) => Some(schema),
        Err(why) => {
            problems.push(why);
            None
        }
    };
    let Some(schema) = schema else { return Err(failure_text(&problems)) };
    let input = LicenceInput {
        version: version.to_string(),
        components,
        embedded,
        schema,
        catalogue_lines: catalogue_lines(),
        runtime: runtime.to_vec(),
    };
    match licence_document(&input) {
        Ok(text) if problems.is_empty() => Ok(text),
        Ok(_) => Err(failure_text(&problems)),
        Err(more) => {
            problems.extend(more);
            Err(failure_text(&problems))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The sub-command
// ---------------------------------------------------------------------------------------------

struct TreeCall {
    args: &'static [&'static str],
    /// `Some(name)`: the call must not match (`cargo tree -i <name>`); "did not match any packages" is the clean result.
    must_not_match: Option<&'static str>,
}

/// The package a `cargo tree` row looks into: the word after `-p`, or the whole workspace when the call names none.
fn tree_package(args: &[&'static str]) -> &'static str {
    args.iter().position(|arg| *arg == "-p").and_then(|i| args.get(i + 1)).copied().unwrap_or("the workspace")
}

/// The rows of `cargo tree`. The import helper (`skipper-import`, RL-05, IMP-13, GI-1; MC-483) reads outside files inside a
/// box with no network: no HTTP client, no HTTP library and no async runtime may stand below it, and a row asks for each of
/// the three by name.
const TREE_CALLS: [TreeCall; 8] = [
    TreeCall { args: &["tree", "--locked", "--offline", "-e", "build", "--prefix", "none"], must_not_match: None },
    TreeCall {
        args: &["tree", "--locked", "--offline", "-p", "skipper-app", "--target", TARGET, "--prefix", "none"],
        must_not_match: None,
    },
    TreeCall { args: &["tree", "--locked", "--offline", "-p", "skipper-mcp", "-i", "reqwest"], must_not_match: Some("reqwest") },
    TreeCall { args: &["tree", "--locked", "--offline", "-p", "skipper-mcp", "-i", "hyper"], must_not_match: Some("hyper") },
    TreeCall { args: &["tree", "--locked", "--offline", "-p", "skipper-import", "-i", "reqwest"], must_not_match: Some("reqwest") },
    TreeCall { args: &["tree", "--locked", "--offline", "-p", "skipper-import", "-i", "hyper"], must_not_match: Some("hyper") },
    TreeCall { args: &["tree", "--locked", "--offline", "-p", "skipper-import", "-i", "tokio"], must_not_match: Some("tokio") },
    TreeCall {
        args: &["tree", "--locked", "--offline", "-p", "stage0-spike", "-e", "features", "-i", "bollard"],
        must_not_match: None,
    },
];

/// `deps-check`: prints the package count, the findings and the raw `cargo tree` outputs, then
/// `deps-check: ok` (exit 0) or `deps-check: <n> finding(s)` (exit 1).
pub fn run_cli(args: &[String]) -> u8 {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask deps-check");
        return 2;
    }
    let (graph, workspace_root) = match load_metadata() {
        Ok(loaded) => loaded,
        Err(message) => {
            println!("deps-check: {}", mask_local_paths(&message, &[]));
            return 1;
        }
    };
    // Everything printed below is kept as evidence: no path of this machine goes into it (G18).
    let roots: Vec<&str> = workspace_root.iter().map(String::as_str).collect();
    println!("deps-check: {} packages in the graph for {TARGET}", graph.packages.len());

    let mut failures = 0usize;
    for finding in check_graph(&graph, MAIN_PACKAGE) {
        match &finding {
            DepFinding::Inspect(..) => println!("deps-check: inspect: {}", describe(&finding)),
            _ => {
                println!("deps-check: finding: {}", describe(&finding));
                failures += 1;
            }
        }
    }

    for call in &TREE_CALLS {
        println!("deps-check: $ cargo {}", call.args.join(" "));
        let out = match Command::new(cargo_program()).args(call.args).output() {
            Ok(out) => out,
            Err(e) => {
                println!("deps-check: cannot start cargo tree: {}", mask_local_paths(&e.to_string(), &roots));
                failures += 1;
                continue;
            }
        };
        let stdout = mask_local_paths(&String::from_utf8_lossy(&out.stdout), &roots);
        let stderr = mask_local_paths(&String::from_utf8_lossy(&out.stderr), &roots);
        print!("{stdout}");
        if !stdout.is_empty() && !stdout.ends_with('\n') {
            println!();
        }
        print!("{stderr}");
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            println!();
        }
        match call.must_not_match {
            Some(name) => {
                let package = tree_package(call.args);
                if out.status.success() {
                    println!("deps-check: finding: {name} is in the graph of {package}");
                    failures += 1;
                } else if stderr.contains("did not match any packages") {
                    println!("deps-check: {name} is not in the graph of {package} (clean: did not match any packages)");
                } else {
                    println!("deps-check: cargo tree -p {package} -i {name} failed for another reason");
                    failures += 1;
                }
            }
            None => {
                if !out.status.success() {
                    println!("deps-check: cargo tree failed");
                    failures += 1;
                }
            }
        }
    }

    if failures == 0 {
        println!("deps-check: ok");
        0
    } else {
        println!("deps-check: {failures} finding(s)");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, licence: &str) -> Package {
        Package { name: name.to_string(), version: version.to_string(), licence: Some(licence.to_string()) }
    }

    fn index_of(name: &str, version: &str, packages: &mut Vec<Package>) -> usize {
        match packages.iter().position(|p| p.name == name) {
            Some(i) => i,
            None => {
                packages.push(pkg(name, version, "MIT"));
                packages.len() - 1
            }
        }
    }

    /// A graph from `(from, to, version of to)` triples; every package has a version.
    fn graph_triples(edges: &[(&str, &str, &str)]) -> Graph {
        let mut packages: Vec<Package> = Vec::new();
        let mut out_edges: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for (from, to, version) in edges {
            let a = index_of(from, "1.0.0", &mut packages);
            let b = index_of(to, version, &mut packages);
            out_edges.push((a, b, Vec::new()));
        }
        Graph { packages, edges: out_edges }
    }

    fn graph(edges: &[(&str, &str)]) -> Graph {
        let triples: Vec<(&str, &str, &str)> = edges.iter().map(|(a, b)| (*a, *b, "1.0.0")).collect();
        graph_triples(&triples)
    }

    fn graph_v(edges: &[(&str, &str, &str)]) -> Graph {
        graph_triples(edges)
    }

    /// A graph in which `stage0-spike` depends on `name` with these active features.
    fn graph_f(name: &str, features: &[&str]) -> Graph {
        Graph {
            packages: vec![pkg("stage0-spike", "0.1.0", "MIT"), pkg(name, "0.21.1", "Apache-2.0")],
            edges: vec![(0, 1, features.iter().map(|f| f.to_string()).collect())],
        }
    }

    /// The shape of the real graph without anything the checks object to.
    fn clean() -> Graph {
        let mut g = graph(&[
            ("skipper-app", "skipper-core"),
            ("skipper-app", "egui"),
            ("skipper-app", "serde_json"),
            ("skipper-core", "serde_json"),
            ("skipper-mcp", "rmcp"),
            ("skipper-mcp", "tokio"),
            ("rmcp", "tokio"),
            ("stage0-spike", "bollard"),
        ]);
        let bollard = g.packages.iter().position(|p| p.name == "bollard").unwrap();
        for edge in g.edges.iter_mut() {
            if edge.1 == bollard {
                edge.2 = vec!["pipe".to_string()];
            }
        }
        g
    }

    #[test]
    fn http_crate_in_main_binary_is_found() {
        assert!(check_graph(&graph(&[("skipper-app", "hyper")]), "skipper-app").contains(&DepFinding::HttpOrTlsInMainBinary("hyper".into())));
    }

    #[test]
    fn tokio_in_main_binary_is_found() {
        assert!(check_graph(&graph(&[("skipper-app", "tokio")]), "skipper-app").contains(&DepFinding::TokioInMainBinary));
    }

    #[test]
    fn blocked_crate_is_found() {
        assert!(check_graph(&graph(&[("skipper-core", "tinymember")]), "skipper-app").contains(&DepFinding::Blocked("tinymember".into())));
    }

    #[test]
    fn inspect_crate_flagged() {
        assert!(
            check_graph(&graph_v(&[("skipper-core", "arrayref", "0.3.10")]), "skipper-app")
                .contains(&DepFinding::Inspect("arrayref".into(), "0.3.10".into()))
        );
    }

    #[test]
    fn rmcp_pulling_reqwest_is_found() {
        assert!(
            check_graph(&graph(&[("skipper-mcp", "rmcp"), ("rmcp", "reqwest")]), "skipper-app")
                .contains(&DepFinding::HttpInRmcp("reqwest".into()))
        );
    }

    #[test]
    fn bollard_tls_feature_is_found() {
        assert!(check_graph(&graph_f("bollard", &["ssl"]), "skipper-app").iter().any(|f| matches!(f, DepFinding::TlsFeature(..))));
    }

    #[test]
    fn eframe_persistence_feature_is_found() {
        assert!(check_graph(&graph_f("eframe", &["persistence"]), "skipper-app").contains(&DepFinding::PersistenceFeature));
        assert!(check_graph(&graph_f("eframe", &["glow", "persistence"]), "skipper-app").contains(&DepFinding::PersistenceFeature));
    }

    #[test]
    fn eframe_without_persistence_passes() {
        assert!(check_graph(&graph_f("eframe", &["glow"]), "skipper-app").is_empty());
        assert!(check_graph(&graph_f("eframe", &[]), "skipper-app").is_empty());
    }

    #[test]
    fn a_persistence_feature_of_another_package_is_not_the_finding() {
        assert!(check_graph(&graph_f("other", &["persistence"]), "skipper-app").is_empty());
    }

    #[test]
    fn the_persistence_finding_has_a_line() {
        assert!(describe(&DepFinding::PersistenceFeature).contains("persistence"));
    }

    #[test]
    fn clean_graph_passes() {
        assert!(check_graph(&clean(), "skipper-app").is_empty());
    }

    #[test]
    fn licence_graph_comes_from_cargo_metadata() {
        let json = br#"{
            "packages": [
                {"id": "a 1.0.0", "name": "a", "version": "1.0.0", "license": "MIT"},
                {"id": "b 2.0.0", "name": "b", "version": "2.0.0", "license": null},
                {"id": "c 3.0.0", "name": "c", "version": "3.0.0", "license": "MIT"}
            ],
            "resolve": {"nodes": [
                {"id": "a 1.0.0", "features": ["x"], "deps": [
                    {"pkg": "b 2.0.0", "dep_kinds": [{"kind": null, "target": null}]},
                    {"pkg": "c 3.0.0", "dep_kinds": [{"kind": "dev", "target": null}]}
                ]},
                {"id": "b 2.0.0", "features": ["ssl", "pipe"], "deps": []},
                {"id": "c 3.0.0", "features": [], "deps": []}
            ]}
        }"#;
        let g = graph_from_metadata(json).unwrap();
        assert_eq!(g.packages.len(), 3);
        assert_eq!(g.packages[1].licence, None);
        assert_eq!(g.edges, vec![(0, 1, vec!["ssl".to_string(), "pipe".to_string()])]);
    }

    // ---- review R5, M16: no local path of the build machine in the evidence ----

    #[test]
    fn the_workspace_root_is_masked_in_every_spelling() {
        let text = "skipper-app v0.1.0 (D:\\Builds\\someone\\tree\\crates\\skipper-app)\n\
                    skipper-core v0.1.0 (D:\\Builds\\someone\\tree\\crates\\skipper-core)\n";
        for root in ["D:/Builds/someone/tree", "D:\\Builds\\someone\\tree", "d:/builds/someone/tree"] {
            let masked = mask_local_paths(text, &[root]);
            assert!(!masked.contains("someone"), "{root}: {masked}");
            assert!(masked.contains("skipper-app v0.1.0 (<repo>\\crates\\skipper-app)"), "{root}: {masked}");
            assert!(masked.contains("skipper-core v0.1.0 (<repo>\\crates\\skipper-core)"), "{root}: {masked}");
        }
    }

    #[test]
    fn other_drive_paths_are_masked_too() {
        let text = "foo v1.0.0 (E:\\someone\\.cargo\\registry\\src\\foo-1.0.0)\nbar v2.0.0 (D:/work/bar)\n";
        let masked = mask_local_paths(text, &[]);
        assert!(!masked.contains("someone") && !masked.contains("D:/work"), "{masked}");
        assert!(masked.contains("foo v1.0.0 (<local path>)") && masked.contains("bar v2.0.0 (<local path>)"), "{masked}");
    }

    #[test]
    fn text_without_a_local_path_is_unchanged() {
        let text = "serde v1.0.229\n\u{2514}\u{2500}\u{2500} serde_core v1.0.229 (*)\nrmcp v3.6.0 (proc-macro)\n\
                    error: package ID specification `reqwest` did not match any packages\n";
        assert_eq!(mask_local_paths(text, &["C:/x/y"]), text);
        assert_eq!(mask_local_paths("", &[]), "");
    }

    #[test]
    fn the_workspace_root_comes_from_cargo_metadata() {
        let json = br#"{"packages": [], "workspace_root": "D:/Builds/someone/tree", "resolve": {"nodes": []}}"#;
        assert_eq!(workspace_root_from_metadata(json).as_deref(), Some("D:/Builds/someone/tree"));
        assert_eq!(workspace_root_from_metadata(b"{}"), None);
        assert_eq!(workspace_root_from_metadata(b"not json"), None);
    }
}

/// The crypto and network-package checks of HUB-13 (MC-356 "Dependency", MC-337 item 1).
#[cfg(test)]
mod hub13_tests {
    use super::*;

    /// A graph from `(from, to)` pairs; every package has the version 1.0.0 and no feature is active.
    fn graph_of(edges: &[(&str, &str)]) -> Graph {
        let mut packages: Vec<Package> = Vec::new();
        let mut index = |name: &str| -> usize {
            match packages.iter().position(|p| p.name == name) {
                Some(i) => i,
                None => {
                    packages.push(Package { name: name.to_string(), version: "1.0.0".to_string(), licence: Some("MIT".to_string()) });
                    packages.len() - 1
                }
            }
        };
        let mut out: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for (from, to) in edges {
            let a = index(from);
            let b = index(to);
            out.push((a, b, Vec::new()));
        }
        Graph { packages, edges: out }
    }

    #[test]
    fn sha2_and_the_crates_it_needs_are_allowed_in_the_main_binary() {
        let g = graph_of(&[
            ("skipper-app", "skipper-core"),
            ("skipper-core", "sha2"),
            ("sha2", "digest"),
            ("digest", "block-buffer"),
            ("digest", "crypto-common"),
            ("crypto-common", "generic-array"),
            ("generic-array", "typenum"),
            ("sha2", "cpufeatures"),
        ]);
        assert_eq!(check_graph(&g, "skipper-app"), Vec::<DepFinding>::new());
    }

    #[test]
    fn another_crypto_crate_in_the_main_binary_is_found() {
        for name in ["ring", "aes-gcm", "chacha20poly1305", "hmac", "sha1", "argon2", "ed25519-dalek", "webpki-roots", "libsodium-sys"] {
            let g = graph_of(&[("skipper-app", "skipper-core"), ("skipper-core", name)]);
            assert_eq!(check_graph(&g, "skipper-app"), vec![DepFinding::CryptoInMainBinary(name.to_string())], "{name}");
        }
    }

    #[test]
    fn a_crypto_crate_below_a_helper_but_not_below_the_main_binary_is_no_main_finding() {
        let g = graph_of(&[("skipper-app", "skipper-core"), ("skipper-mcp", "hmac")]);
        assert_eq!(check_graph(&g, "skipper-app"), Vec::<DepFinding>::new());
    }

    #[test]
    fn the_gateway_and_the_network_crate_have_no_http_tls_crypto_or_async_crate() {
        for (root, name) in [
            ("skipper-gateway", "reqwest"),
            ("skipper-gateway", "rustls"),
            ("skipper-gateway", "ring"),
            ("skipper-gateway", "tokio"),
            ("skipper-net", "hyper"),
            ("skipper-net", "aes"),
            ("skipper-net", "async-std"),
        ] {
            let g = graph_of(&[(root, "skipper-core"), ("skipper-core", name)]);
            assert_eq!(
                check_graph(&g, "skipper-app"),
                vec![DepFinding::ForbiddenBelowNetPackage(root.to_string(), name.to_string())],
                "{root} {name}"
            );
        }
    }

    #[test]
    fn the_gateway_may_use_sha2_and_the_network_crate_may_stand_below_it() {
        let g = graph_of(&[
            ("skipper-gateway", "skipper-net"),
            ("skipper-gateway", "skipper-core"),
            ("skipper-gateway", "sha2"),
            ("skipper-core", "sha2"),
            ("sha2", "digest"),
            ("skipper-net", "windows-sys"),
        ]);
        assert_eq!(check_graph(&g, "skipper-app"), Vec::<DepFinding>::new());
    }

    #[test]
    fn xtask_with_default_features_has_no_edge_to_the_network_crate() {
        // The graph of the default build: the gateway and the update helper reach `skipper-net`, `xtask` does not.
        let clean = graph_of(&[
            ("skipper-gateway", "skipper-net"),
            ("skipper-update", "skipper-net"),
            ("xtask", "sha2"),
            ("xtask", "serde_json"),
        ]);
        assert_eq!(check_graph(&clean, "skipper-app"), Vec::<DepFinding>::new());
        // A non-optional dependency (or a default feature that turns it on) is the finding.
        let dirty = graph_of(&[("xtask", "skipper-net"), ("skipper-net", "windows-sys")]);
        assert_eq!(check_graph(&dirty, "skipper-app"), vec![DepFinding::NetCrateInXtask]);
        assert!(describe(&DepFinding::NetCrateInXtask).contains("catalog-ci"));
    }

    #[test]
    fn a_package_that_only_looks_like_a_crypto_crate_is_none() {
        for name in ["ringbuf", "rsa-lookalike", "shake", "snowball", "aesthetic", "hmac-like", "scryptic"] {
            assert!(!is_crypto_crate(name), "{name}");
        }
        for name in ["ring", "snow", "snow-protocol", "webpki", "ripemd", "sodiumoxide"] {
            assert!(is_crypto_crate(name), "{name}");
        }
    }

    #[test]
    fn the_findings_read_in_plain_words() {
        let main = describe(&DepFinding::CryptoInMainBinary("ring".to_string()));
        assert!(main.contains("ring") && main.contains("sha2"));
        let helper = describe(&DepFinding::ForbiddenBelowNetPackage("skipper-gateway".to_string(), "tokio".to_string()));
        assert!(helper.contains("tokio") && helper.contains("skipper-gateway"));
    }
}

/// The `cargo tree` rows of the import helper (RL-05, IMP-13, GI-1; MC-483): it reads outside files inside a box with no
/// network, so no HTTP crate and no async runtime may stand below it.
#[cfg(test)]
mod import_rows_tests {
    use super::*;

    #[test]
    fn skipper_import_has_no_network_crate_rows() {
        for c in ["reqwest", "hyper", "tokio"] {
            assert!(
                TREE_CALLS.iter().any(|t| t.args.contains(&"skipper-import") && t.must_not_match == Some(c)),
                "no row for {c} below skipper-import"
            );
        }
    }

    #[test]
    fn each_import_row_asks_for_the_inverted_tree_of_one_crate_offline_and_locked() {
        let rows = &TREE_CALLS;
        for c in ["reqwest", "hyper", "tokio"] {
            let row = rows
                .iter()
                .find(|t| t.args.contains(&"skipper-import") && t.must_not_match == Some(c))
                .expect("the row exists");
            assert_eq!(row.args, ["tree", "--locked", "--offline", "-p", "skipper-import", "-i", c]);
        }
    }

    #[test]
    fn the_package_of_a_row_is_the_word_after_p_so_the_report_names_the_right_graph() {
        assert_eq!(tree_package(&["tree", "-p", "skipper-import", "-i", "tokio"]), "skipper-import");
        assert_eq!(tree_package(&["tree", "--locked", "-e", "build"]), "the workspace");
        assert_eq!(tree_package(&["tree", "-p"]), "the workspace");
        let rows = &TREE_CALLS;
        for row in rows.iter().filter(|row| row.must_not_match.is_some()) {
            assert_ne!(tree_package(row.args), "the workspace", "{:?}", row.args);
        }
    }
}

/// SP-6 (closing review, gate lens finding 1): THIRD-PARTY-LICENSES in the layout of `docs/design/third-party-notice-list.md`
/// section 2, built from fixture inputs: the seven parts in order, each licence text once with the copyright lines of its
/// components, the font block, the embedded code, the NOTICE texts, the data part, the runtime part, and no file at all while a
/// shipped package has no licence text.
#[cfg(test)]
mod licence_file_tests {
    use super::*;

    const APACHE: &str = "                                 Apache License\n                           Version 2.0, January 2004\n\n   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION\n\n   1. Definitions.\n";

    /// An MIT text with these holder lines.
    fn mit_text(holders: &[&str]) -> String {
        let mut out = String::from("MIT License\n\n");
        for holder in holders {
            out.push_str(holder);
            out.push('\n');
        }
        out.push_str(
            "\nPermission is hereby granted, free of charge, to any person obtaining a copy\nof this software and associated documentation \
             files (the \"Software\"), to deal\nin the Software without restriction.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT \
             WARRANTY OF ANY KIND.\n",
        );
        out
    }

    fn file(name: &str, text: &str) -> TextFile {
        TextFile { name: name.to_string(), text: text.to_string(), source: None }
    }

    fn comp(name: &str, version: &str, licence: &str, files: Vec<TextFile>) -> Component {
        Component {
            name: name.to_string(),
            version: version.to_string(),
            licence: Some(licence.to_string()),
            own: false,
            link: None,
            authors: Vec::new(),
            licence_files: files,
            notice_files: Vec::new(),
            font_files: Vec::new(),
        }
    }

    fn block() -> EmbeddedBlock {
        EmbeddedBlock {
            title: "fixture.c (a fixture library)".to_string(),
            file: "crates/fixture/src/lib.rs".to_string(),
            note: "FIXTURE NOTE".to_string(),
            licence: "Zlib".to_string(),
            copyright: "Copyright (C) 2002 Fixture Author".to_string(),
            text: "FIXTURE ZLIB TEXT".to_string(),
        }
    }

    fn input(components: Vec<Component>) -> LicenceInput {
        LicenceInput {
            version: "0.1.0-test.1".to_string(),
            components,
            embedded: vec![block()],
            schema: SchemaNotice { notice: "SCHEMA NOTICE LINES\n".to_string(), licence: "SCHEMA LICENCE TEXT\n".to_string() },
            catalogue_lines: vec!["Model metadata: Fixture".to_string(), "Benchmark data: Fixture".to_string()],
            runtime: vec![("winpthreads (mingw-w64)".to_string(), "RUNTIME NOTICE\n".to_string())],
        }
    }

    fn document(components: Vec<Component>) -> String {
        licence_document(&input(components)).unwrap_or_else(|problems| panic!("{problems:?}"))
    }

    /// The text of part `n`: from its heading to the heading of the next part.
    fn part(text: &str, n: usize) -> &str {
        let start = text.find(&format!("Part {n} - ")).unwrap_or_else(|| panic!("part {n} is missing"));
        let end = text.find(&format!("Part {} - ", n + 1)).unwrap_or(text.len());
        &text[start..end]
    }

    fn simple() -> Vec<Component> {
        vec![comp("alpha", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2020 Alpha Authors"]))])]
    }

    // ---- the layout ----

    #[test]
    fn the_file_has_the_seven_parts_in_the_planned_order() {
        let text = document(simple());
        let mut at = 0usize;
        for heading in ["Part 0 - ", "Part 1 - ", "Part 2 - ", "Part 3 - ", "Part 4 - ", "Part 5 - ", "Part 6 - ", "Part 7 - "] {
            let found = text[at..].find(heading).unwrap_or_else(|| panic!("{heading} is missing or out of order"));
            at += found + heading.len();
        }
        assert!(text.starts_with("THIRD-PARTY-LICENSES\n"), "{text}");
        assert!(!text.contains("none bundled"), "the false fonts sentence is gone");
        assert!(text.ends_with('\n') && !text.ends_with("\n\n\n"));
    }

    #[test]
    fn part_0_names_the_version_and_records_the_choice_of_licences() {
        let text = document(simple());
        assert!(
            part(&text, 0).contains(
                "Skipper 0.1.0-test.1 includes the third-party components below. Where a component offers a choice of licences, \
                 Skipper uses it under the licence named here."
            ),
            "{}",
            part(&text, 0)
        );
    }

    #[test]
    fn the_index_has_a_row_per_component_with_the_licence_used_and_a_link() {
        let mut beta = comp("beta", "2.0.0", "BSD-3-Clause OR Apache-2.0", vec![file("LICENSE-APACHE", APACHE)]);
        beta.link = Some("https://example.org/beta".to_string());
        let text = document(vec![simple().remove(0), beta]);
        let index = part(&text, 1);
        assert!(index.contains("alpha 1.0.0 - MIT - https://crates.io/crates/alpha/1.0.0\n"), "{index}");
        assert!(index.contains("beta 2.0.0 - Apache-2.0 - https://example.org/beta\n"), "{index}");
    }

    #[test]
    fn skippers_own_packages_are_named_once_and_need_no_third_party_text() {
        let mut own = comp("skipper-core", "0.1.0-test.1", "MIT", Vec::new());
        own.own = true;
        let text = document(vec![simple().remove(0), own]);
        assert!(part(&text, 1).contains("skipper-core"), "{}", part(&text, 1));
        assert!(part(&text, 1).contains("see the file LICENSE"), "{}", part(&text, 1));
        assert!(!part(&text, 2).contains("skipper-core"), "an own package has no third-party group");
    }

    #[test]
    fn each_licence_text_stands_once_with_the_copyright_lines_of_every_component() {
        let rewrapped = "MIT License\n\nCopyright (c) 2022 Dave\n\nPermission is hereby granted, free of charge, to any person obtaining a \
                         copy of this software and associated documentation files (the \"Software\"), to deal in the Software without \
                         restriction.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND.\n";
        let text = document(vec![
            comp(
                "a",
                "1.0.0",
                "MIT OR Apache-2.0",
                vec![file("LICENSE-MIT", &mit_text(&["Copyright (c) 2018 Alice"])), file("LICENSE-APACHE", APACHE)],
            ),
            comp("b", "2.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2019 Bob", "Copyright (c) 2021 Carol"]))]),
            comp("c", "3.0.0", "Apache-2.0", vec![file("LICENSE", APACHE)]),
            comp("d", "4.0.0", "MIT", vec![file("LICENSE", rewrapped)]),
        ]);
        let groups = part(&text, 2);
        assert_eq!(groups.matches("THE SOFTWARE IS PROVIDED").count(), 1, "the MIT text stands once, wrapped or not:\n{groups}");
        assert_eq!(groups.matches("TERMS AND CONDITIONS FOR USE").count(), 1, "the Apache text stands once:\n{groups}");
        for line in ["Copyright (c) 2018 Alice", "Copyright (c) 2019 Bob", "Copyright (c) 2021 Carol", "Copyright (c) 2022 Dave"] {
            assert_eq!(groups.matches(line).count(), 1, "{line}\n{groups}");
        }
        let mit = groups.find("2.1 MIT (3 components)").expect("the MIT group comes first");
        let apache = groups.find("2.2 Apache-2.0 (1 component)").expect("the Apache group comes second");
        assert!(mit < apache);
        // The copyright lines of the MIT components stand in the group, in front of the text.
        assert!(groups[mit..apache].contains("  b 2.0.0\n    Copyright (c) 2019 Bob\n    Copyright (c) 2021 Carol\n"), "{groups}");
        // The unused alternative of a dual licence is not printed (component a uses its MIT text).
        assert!(!groups[apache..].contains("  a 1.0.0"), "{groups}");
    }

    #[test]
    fn two_different_texts_of_one_licence_are_both_kept() {
        let other = format!("{}\nAn extra paragraph that only this package has.\n", mit_text(&["Copyright (c) 2020 Other"]));
        let text = document(vec![
            comp("plain", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2020 Plain"]))]),
            comp("extra", "1.0.0", "MIT", vec![file("LICENSE", &other)]),
        ]);
        let groups = part(&text, 2);
        assert!(groups.contains("Text 1 of 2"), "{groups}");
        assert!(groups.contains("Text 2 of 2"), "{groups}");
        assert_eq!(groups.matches("An extra paragraph").count(), 1);
    }

    #[test]
    fn a_dual_licence_without_the_preferred_text_falls_back_to_the_other_one() {
        let text = document(vec![comp("only-apache", "1.0.0", "MIT OR Apache-2.0", vec![file("LICENSE-APACHE", APACHE)])]);
        assert!(part(&text, 1).contains("only-apache 1.0.0 - Apache-2.0 - "), "{}", part(&text, 1));
        assert!(part(&text, 2).contains("2.1 Apache-2.0 (1 component)"), "{}", part(&text, 2));
    }

    #[test]
    fn a_licence_stated_only_in_the_package_files_gets_its_own_group() {
        let mut odd = comp("odd", "1.0.0", "MIT", vec![file("LICENSE", "The odd licence. Do what you like.\n")]);
        odd.licence = None;
        let text = document(vec![odd]);
        assert!(part(&text, 2).contains("Licence stated in the package files only (1 component)"), "{}", part(&text, 2));
        assert!(part(&text, 2).contains("Do what you like."));
    }

    #[test]
    fn copyright_lines_come_from_the_file_then_from_the_authors_then_say_none() {
        let mail = ["a", "@", "b.example"].concat(); // built at run time: the source holds no address
        let mut with_authors = comp("authored", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&[]))]);
        with_authors.authors = vec![author_name(&format!("A Person <{mail}>")), "Second Person".to_string()];
        let nobody = comp("nobody", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&[]))]);
        let text = document(vec![with_authors, nobody]);
        let groups = part(&text, 2);
        assert!(groups.contains("Copyright by the package authors (from the manifest): A Person, Second Person"), "{groups}");
        assert!(!text.contains(&mail), "no address of an author goes into the file");
        assert!(groups.contains("  nobody 1.0.0\n    no copyright line published\n"), "{groups}");
    }

    #[test]
    fn a_text_supplied_from_upstream_names_its_source() {
        let mut needy = comp("needy", "2.0.0", "MIT", Vec::new());
        needy.licence_files.push(TextFile {
            name: "LICENSE".to_string(),
            text: mit_text(&["Copyright (c) 2023 Needy Authors"]),
            source: Some("https://example.org/needy".to_string()),
        });
        let text = document(vec![needy]);
        assert!(part(&text, 2).contains("  needy 2.0.0 (text from https://example.org/needy)\n"), "{}", part(&text, 2));
    }

    #[test]
    fn an_extra_supplied_text_that_no_licence_uses_goes_to_the_embedded_part() {
        let mut access =
            comp("accessible", "0.25.1", "MIT OR Apache-2.0", vec![file("LICENSE-MIT", &mit_text(&["Copyright (c) 2020 Access"]))]);
        access.licence_files.push(TextFile {
            name: "LICENSE.chromium".to_string(),
            text: "BSD-3-Clause text with its Chromium notice\nRedistribution and use in source and binary forms.\nNeither the name of the Chromium authors.\n"
                .to_string(),
            source: Some("https://example.org/chromium".to_string()),
        });
        let text = document(vec![access]);
        let embedded = part(&text, 3);
        assert!(embedded.contains("accessible 0.25.1 - LICENSE.chromium"), "{embedded}");
        assert!(embedded.contains("https://example.org/chromium"), "{embedded}");
        assert!(embedded.contains("Neither the name of the Chromium authors."), "{embedded}");
        assert!(!part(&text, 2).contains("Chromium"), "the BSD text is no MIT text");
    }

    // ---- failing closed ----

    #[test]
    fn a_package_without_licence_text_stops_the_file_and_is_named() {
        let problems = licence_document(&input(vec![
            comp("good", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2020 Good"]))]),
            comp("ghost", "1.2.3", "MIT OR Apache-2.0", Vec::new()),
            comp("phantom", "0.4.0", "MIT", vec![file("LICENSE-APACHE", APACHE)]),
        ]))
        .unwrap_err();
        let all = problems.join("\n");
        assert_eq!(problems.len(), 2, "{all}");
        assert!(all.contains("ghost 1.2.3") && all.contains("phantom 0.4.0"), "{all}");
        assert!(all.contains("supply-chain/notices/upstream/ghost"), "the message says where the text goes: {all}");
        assert!(all.contains("LICENSE-APACHE"), "the message says which files were seen: {all}");
        assert!(!all.contains("good 1.0.0"));
    }

    #[test]
    fn an_unreadable_or_copyleft_only_expression_stops_the_file() {
        let problems = licence_document(&input(vec![
            comp("broken", "1.0.0", "MIT OR", vec![file("LICENSE", &mit_text(&[]))]),
            comp("viral", "1.0.0", "GPL-3.0-only", vec![file("LICENSE", "GPL text")]),
        ]))
        .unwrap_err();
        let all = problems.join("\n");
        assert!(all.contains("broken 1.0.0") && all.contains("cannot be read"), "{all}");
        assert!(all.contains("viral 1.0.0") && all.contains("copyleft"), "{all}");
    }

    // ---- which licence is recorded (notice list, section 3) ----

    fn first_choice(expression: &str) -> String {
        ranked_options(expression, false).unwrap_or_else(|why| panic!("{expression}: {why}")).remove(0).join("+")
    }

    #[test]
    fn the_licence_recorded_follows_section_3_of_the_notice_list() {
        let cases = [
            ("MIT OR Apache-2.0", "MIT"),
            ("MIT/Apache-2.0", "MIT"),
            ("Unlicense OR MIT", "MIT"),
            ("Unlicense/MIT", "MIT"),
            ("0BSD OR MIT OR Apache-2.0", "MIT"),
            ("Zlib OR Apache-2.0 OR MIT", "MIT"),
            ("Apache-2.0 WITH LLVM-exception OR MIT", "MIT"),
            ("MIT", "MIT"),
            ("Apache-2.0", "Apache-2.0"),
            ("BSD-3-Clause OR Apache-2.0", "Apache-2.0"),
            ("BSD-2-Clause OR Apache-2.0", "Apache-2.0"),
            ("Apache-2.0 OR GPL-2.0-only", "Apache-2.0"),
            ("Apache-2.0 OR BSL-1.0", "Apache-2.0"),
            ("Unicode-3.0", "Unicode-3.0"),
            ("Zlib", "Zlib"),
            ("ISC", "ISC"),
            ("BSL-1.0", "BSL-1.0"),
            ("Apache-2.0 AND MIT", "Apache-2.0+MIT"),
            ("(MIT OR Apache-2.0) AND Unicode-3.0", "MIT+Unicode-3.0"),
        ];
        for (expression, expected) in cases {
            assert_eq!(first_choice(expression), expected, "{expression}");
        }
    }

    #[test]
    fn a_copyleft_branch_is_never_offered_and_a_broken_expression_is_refused() {
        assert_eq!(ranked_options("Apache-2.0 OR GPL-2.0-only", false).unwrap().len(), 1);
        assert_eq!(ranked_options("MIT OR LGPL-2.1-or-later OR Apache-2.0", false).unwrap().len(), 2);
        assert!(ranked_options("GPL-3.0-only", false).unwrap_err().contains("copyleft"));
        for broken in ["", "MIT OR", "MIT )", "AND MIT"] {
            assert!(ranked_options(broken, false).unwrap_err().contains("cannot be read"), "{broken:?}");
        }
    }

    #[test]
    fn the_fonts_crate_records_its_code_licence_and_leaves_the_font_licences_to_part_4() {
        let expression = "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0";
        assert_eq!(ranked_options(expression, true).unwrap().remove(0), vec!["MIT".to_string()]);
        assert_eq!(ranked_options(expression, false).unwrap().remove(0).join("+"), "MIT+OFL-1.1+Ubuntu-font-1.0");
    }

    #[test]
    fn what_a_text_says_it_is_is_read_from_its_words() {
        assert_eq!(detect(&mit_text(&["Copyright (c) 2020 X"])), vec!["MIT"]);
        assert_eq!(detect(APACHE), vec!["Apache-2.0"]);
        assert_eq!(detect("  Altered   source versions must be\nplainly marked as such, and so on."), vec!["Zlib"]);
        assert_eq!(detect("Redistribution and use in source and binary forms, with conditions. Neither the name of X."), vec!["BSD-3-Clause"]);
        assert_eq!(detect("Redistribution and use in source and binary forms, with conditions."), vec!["BSD-2-Clause"]);
        assert_eq!(
            detect(
                "Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby \
                 granted, provided that the above copyright notice and this permission notice appear in all copies."
            ),
            vec!["ISC"]
        );
        assert_eq!(
            detect("Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted."),
            vec!["0BSD"]
        );
        assert_eq!(detect("UNICODE LICENSE V3\n\nCOPYRIGHT AND PERMISSION NOTICE"), vec!["Unicode-3.0"]);
        assert_eq!(detect("Boost Software License - Version 1.0"), vec!["BSL-1.0"]);
        assert!(detect("Short version for non-lawyers").is_empty());
    }

    #[test]
    fn copyright_lines_name_a_holder_and_a_year_and_templates_do_not_count() {
        let text = "Copyright (c) 2018 Alice\nCopyright 2015-2020 The Bob Authors\n(c) 2019 Carol\nCopyright [yyyy] [name of copyright owner]\n\
                    Copyright (c) <year> <owner>\nThe above copyright notice shall be included.\nCOPYRIGHT AND PERMISSION NOTICE\n   \
                    Copyright \u{a9} 1991-2023 Unicode, Inc.\nCopyright (c) 2018 Alice\n";
        assert_eq!(
            copyright_lines(text),
            vec!["Copyright (c) 2018 Alice", "Copyright 2015-2020 The Bob Authors", "(c) 2019 Carol", "Copyright \u{a9} 1991-2023 Unicode, Inc."]
        );
    }

    #[test]
    fn the_body_of_a_text_has_no_copyright_line_and_no_stray_blank_lines() {
        let body = body_of("MIT License\n\nCopyright (c) 2018 Alice\n\n\nPermission is hereby granted.   \nTHE END\n");
        assert_eq!(body, "MIT License\n\nPermission is hereby granted.\nTHE END");
    }

    // ---- fonts, embedded code, NOTICE, data, runtime ----

    fn font_texts() -> Vec<TextFile> {
        ["Hack-Regular.txt", "UFL.txt", "emoji-icon-font-mit-license.txt", "OFL.txt"]
            .iter()
            .map(|name| file(name, &format!("TEXT OF {name}\nCopyright (c) 2018 A Font Holder\n")))
            .collect()
    }

    fn fonts_crate(fonts: Vec<TextFile>) -> Component {
        let mut krate = comp(
            "epaint_default_fonts",
            "0.37.0",
            "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0",
            vec![file("LICENSE-MIT", &mit_text(&["Copyright (c) 2018 Emil Ernerfeldt"]))],
        );
        krate.font_files = fonts;
        krate
    }

    #[test]
    fn part_4_carries_the_licence_text_of_every_bundled_font() {
        let text = document(vec![fonts_crate(font_texts())]);
        let fonts = part(&text, 4);
        for name in ["Hack-Regular", "Ubuntu-Light", "egui-icons", "NotoEmoji-Regular", "emoji-icon-font"] {
            assert!(fonts.contains(name), "{name}\n{fonts}");
        }
        for file_name in ["Hack-Regular.txt", "UFL.txt", "emoji-icon-font-mit-license.txt", "OFL.txt"] {
            assert!(fonts.contains(&format!("TEXT OF {file_name}\n")), "{file_name}\n{fonts}");
        }
        assert!(fonts.contains("Copyright (c) 2018 A Font Holder"), "the font texts are whole, copyright lines included");
        assert!(!fonts.contains("none bundled"));
        // The crate's own code is recorded under MIT; its font licences are not groups of part 2.
        assert!(part(&text, 1).contains("epaint_default_fonts 0.37.0 - MIT + the font licences of part 4 - "), "{}", part(&text, 1));
        assert!(!part(&text, 2).contains("OFL-1.1 ("), "{}", part(&text, 2));
    }

    #[test]
    fn a_missing_font_text_stops_the_file_and_names_the_crate_and_the_file() {
        let mut texts = font_texts();
        texts.retain(|f| f.name != "OFL.txt");
        let problems = licence_document(&input(vec![fonts_crate(texts)])).unwrap_err().join("\n");
        assert!(problems.contains("epaint_default_fonts 0.37.0") && problems.contains("OFL.txt"), "{problems}");
    }

    #[test]
    fn without_a_font_crate_part_4_says_so() {
        let text = document(simple());
        assert!(part(&text, 4).contains("No font is bundled"), "{}", part(&text, 4));
    }

    #[test]
    fn part_3_carries_the_embedded_code_with_its_licence_text() {
        let text = document(simple());
        let embedded = part(&text, 3);
        for needle in ["fixture.c (a fixture library)", "crates/fixture/src/lib.rs", "FIXTURE NOTE", "Copyright (C) 2002 Fixture Author", "FIXTURE ZLIB TEXT"] {
            assert!(embedded.contains(needle), "{needle}\n{embedded}");
        }
    }

    #[test]
    fn part_5_carries_the_notice_files_and_says_so_when_there_are_none() {
        let mut with_notice = comp("noticed", "1.0.0", "Apache-2.0", vec![file("LICENSE", APACHE)]);
        with_notice.notice_files = vec![file("NOTICE", "This product includes software of the Noticed project.\n")];
        let text = document(vec![with_notice]);
        assert!(part(&text, 5).contains("noticed 1.0.0 - NOTICE"), "{}", part(&text, 5));
        assert!(part(&text, 5).contains("This product includes software of the Noticed project."), "{}", part(&text, 5));
        assert!(part(&document(simple()), 5).contains("No package of this release ships a NOTICE file"));
    }

    #[test]
    fn part_6_has_the_catalogue_attribution_and_the_schema_notice_with_the_apache_text() {
        let mut data = input(simple());
        data.catalogue_lines = catalogue_lines();
        let text = licence_document(&data).unwrap();
        let data_part = part(&text, 6);
        // The two lines are the constants of the catalogue build, read by name (row SP-4 rewrites their text).
        assert!(data_part.contains(ANTHROPIC_LINE), "{data_part}");
        assert!(data_part.contains(EPOCH_LINE), "{data_part}");
        // The attribution block of CC BY 4.0 section 3(a): creator, title, link, licence link, changes, disclaimer.
        for needle in [
            "Epoch AI",
            "Capabilities & benchmarking",
            EPOCH_DATA_URL,
            "https://creativecommons.org/licenses/by/4.0/",
            "reduced the data to per-model scores",
            "no warranty",
        ] {
            assert!(data_part.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()), "{needle}\n{data_part}");
        }
        assert!(data_part.contains("SCHEMA NOTICE LINES") && data_part.contains("SCHEMA LICENCE TEXT"), "{data_part}");
    }

    #[test]
    fn the_epoch_link_is_the_address_the_catalogue_job_fetches() {
        let net = include_str!("catalog_net.rs");
        assert!(
            net.contains(&format!("pub const EPOCH_URL: &str = \"{EPOCH_DATA_URL}\";")),
            "catalog_net.rs and deps.rs name different Epoch addresses"
        );
    }

    #[test]
    fn part_7_is_the_runtime_part_as_before() {
        let text = document(simple());
        assert!(part(&text, 7).contains("winpthreads (mingw-w64)\nRUNTIME NOTICE\n"), "{}", part(&text, 7));
    }

    #[test]
    fn the_same_components_in_any_order_give_the_same_bytes() {
        let make = || {
            vec![
                comp("zeta", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2020 Zeta"]))]),
                comp("alpha", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2020 Alpha"]))]),
                comp("mid", "1.0.0", "Apache-2.0", vec![file("LICENSE", APACHE)]),
            ]
        };
        let forward = document(make());
        let mut reversed = make();
        reversed.reverse();
        assert_eq!(forward, document(reversed));
        // A package that appears twice (two roots reach it) is listed once.
        let mut twice = make();
        twice.push(comp("alpha", "1.0.0", "MIT", vec![file("LICENSE", &mit_text(&["Copyright (c) 2020 Alpha"]))]));
        assert_eq!(forward, document(twice));
    }

    // ---- reading the files ----

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("skipper-licences-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_licence_and_notice_files_of_a_package_folder_are_read_and_nothing_else() {
        let base = scratch("folder");
        put(&base.join("LICENSE-MIT"), "\u{feff}MIT License\r\n\r\nCopyright (c) 2020 Z\r\n");
        put(&base.join("license-apache-2.0"), APACHE);
        put(&base.join("COPYING"), "copying text");
        put(&base.join("UNLICENSE"), "unlicense text");
        put(&base.join("NOTICE"), "notice text");
        put(&base.join("README.md"), "readme");
        put(&base.join("Cargo.toml"), "[package]");
        put(&base.join("COPYRIGHT"), "Short version for non-lawyers");
        put(&base.join("LICENSES/MIT.txt"), "folder text");
        put(&base.join("src/LICENSE"), "deeper down: not read");
        let (licences, notices) = read_package_texts("pkg 1.0.0", &base).unwrap();
        let names: Vec<&str> = licences.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["COPYING", "LICENSE-MIT", "LICENSES/MIT.txt", "UNLICENSE", "license-apache-2.0"]);
        assert_eq!(licences[1].text, "MIT License\n\nCopyright (c) 2020 Z\n", "no byte order mark, LF line ends");
        assert!(licences.iter().all(|f| f.source.is_none()));
        assert_eq!(notices.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["NOTICE"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_copyright_file_counts_only_when_it_holds_a_licence_text() {
        let base = scratch("copyright");
        put(&base.join("COPYRIGHT"), &mit_text(&["Copyright (c) 2020 Holder"]));
        let (licences, _) = read_package_texts("pkg 1.0.0", &base).unwrap();
        assert_eq!(licences.len(), 1);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn an_upstream_text_needs_the_source_line_and_comes_with_its_source() {
        let root = scratch("upstream");
        put(
            &root.join("supply-chain/notices/upstream/egui/LICENSE-MIT"),
            &format!("Source: https://example.org/egui/LICENSE-MIT\n{}", mit_text(&["Copyright (c) 2019 Egui Authors"])),
        );
        let texts = read_upstream_texts(&root, "egui").unwrap();
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].source.as_deref(), Some("https://example.org/egui/LICENSE-MIT"));
        assert!(texts[0].text.starts_with("MIT License"), "{}", texts[0].text);
        assert!(read_upstream_texts(&root, "nothing-here").unwrap().is_empty());
        put(&root.join("supply-chain/notices/upstream/bad/LICENSE"), "MIT License\nno source line\n");
        let why = read_upstream_texts(&root, "bad").unwrap_err();
        assert!(why.contains("bad") && why.contains("Source:"), "{why}");
        put(&root.join("supply-chain/notices/upstream/empty/LICENSE"), "Source: https://example.org/x\n\n");
        assert!(read_upstream_texts(&root, "empty").unwrap_err().contains("no text"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_font_texts_are_found_by_name_below_the_crate_folder() {
        let base = scratch("fonts");
        for name in ["Hack-Regular.txt", "UFL.txt", "emoji-icon-font-mit-license.txt", "OFL.txt"] {
            put(&base.join("fonts").join(name), &format!("TEXT OF {name}\r\n"));
        }
        put(&base.join("fonts/Hack-Regular.ttf"), "not a text");
        put(&base.join("README.md"), "readme");
        let mut found = read_font_texts("epaint_default_fonts 0.37.0", &base).unwrap();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["Hack-Regular.txt", "OFL.txt", "UFL.txt", "emoji-icon-font-mit-license.txt"]);
        assert_eq!(found[0].text, "TEXT OF Hack-Regular.txt\n");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn package_infos_come_from_cargo_metadata() {
        let mail = ["a", "@", "b.example"].concat();
        let json = r#"{"packages": [
            {"id": "reg 1.0.0", "name": "reg", "version": "1.0.0", "license": "MIT",
             "source": "registry+https://github.com/rust-lang/crates.io-index",
             "manifest_path": "/registry/reg-1.0.0/Cargo.toml", "repository": "https://example.org/reg", "homepage": null,
             "authors": ["A Person <MAILHERE>", "Second Person", "<MAILHERE>"]},
            {"id": "mine 0.1.0", "name": "mine", "version": "0.1.0", "license": "MIT", "source": null,
             "manifest_path": "/checkout/crates/mine/Cargo.toml", "authors": []}
        ]}"#
        .replace("MAILHERE", &mail);
        let infos = package_infos_from_metadata(json.as_bytes()).unwrap();
        assert_eq!(infos.len(), 2);
        assert!(!infos[0].own && infos[1].own);
        assert_eq!(infos[0].dir, Some(PathBuf::from("/registry/reg-1.0.0")));
        assert_eq!(infos[0].link.as_deref(), Some("https://example.org/reg"));
        assert_eq!(infos[0].authors, vec!["A Person".to_string(), "Second Person".to_string()]);
        assert_eq!(infos[1].link, None);
        assert!(package_infos_from_metadata(b"not json").is_err());
        assert!(package_infos_from_metadata(b"{}").is_err());
    }

    fn package(name: &str, version: &str, licence: &str) -> Package {
        Package { name: name.to_string(), version: version.to_string(), licence: Some(licence.to_string()) }
    }

    fn info(name: &str, version: &str, dir: Option<PathBuf>) -> PackageInfo {
        PackageInfo { name: name.to_string(), version: version.to_string(), own: false, dir, link: None, authors: Vec::new() }
    }

    /// A checkout and a registry below `base`, with one package that has its text, one that needs the upstream folder, and the
    /// schema folders (an old one that must not be read and the newest one).
    fn site(base: &Path) -> (PathBuf, Vec<Package>, Vec<PackageInfo>) {
        let root = base.join("root");
        let registry = base.join("registry");
        put(&root.join("crates/skipper-import/src/inflate.rs"), "// fixture");
        put(&root.join("xtask/data/cc-src/1.0.0/NOTICE-schema.txt"), "OLD NOTICE");
        put(&root.join("xtask/data/cc-src/2.1.318/NOTICE-schema.txt"), "SCHEMA NOTICE LINES");
        put(&root.join("xtask/data/cc-src/2.1.318/LICENSE-schema-Apache-2.0.txt"), APACHE);
        put(
            &root.join("supply-chain/notices/upstream/needy/LICENSE"),
            &format!("Source: https://example.org/needy\n{}", mit_text(&["Copyright (c) 2023 Needy Authors"])),
        );
        put(&registry.join("plain-1.0.0/LICENSE-MIT"), &mit_text(&["Copyright (c) 2020 Plain Authors"]));
        put(&registry.join("plain-1.0.0/NOTICE"), "plain notice text");
        put(&registry.join("needy-2.0.0/README.md"), "no licence here");
        let packages = vec![package("plain", "1.0.0", "MIT OR Apache-2.0"), package("needy", "2.0.0", "MIT")];
        let infos =
            vec![info("plain", "1.0.0", Some(registry.join("plain-1.0.0"))), info("needy", "2.0.0", Some(registry.join("needy-2.0.0")))];
        (root, packages, infos)
    }

    #[test]
    fn the_file_is_built_from_a_checkout_and_a_registry() {
        let base = scratch("build");
        let (root, packages, infos) = site(&base);
        let runtime = [("runtime title".to_string(), "RUNTIME BODY".to_string())];
        let text = build_licence_file(&root, "0.1.0-test.1", &packages, &infos, &runtime).unwrap_or_else(|why| panic!("{why}"));
        assert!(text.contains("plain 1.0.0") && text.contains("needy 2.0.0"), "{text}");
        assert!(text.contains("Copyright (c) 2020 Plain Authors") && text.contains("Copyright (c) 2023 Needy Authors"));
        assert!(text.contains("plain notice text"), "the NOTICE file of a package is part 5");
        assert!(text.contains("SCHEMA NOTICE LINES") && !text.contains("OLD NOTICE"), "the newest schema folder is read");
        assert!(text.contains("RUNTIME BODY"));
        assert!(text.contains("Mark Adler"), "the embedded block of the import helper's decoder is part 3");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_build_stops_and_names_the_package_while_a_text_is_missing() {
        let base = scratch("closed");
        let (root, packages, infos) = site(&base);
        fs::remove_dir_all(root.join("supply-chain/notices/upstream/needy")).unwrap();
        let why = build_licence_file(&root, "0.1.0-test.1", &packages, &infos, &[]).unwrap_err();
        assert!(why.contains("needy 2.0.0") && why.contains("supply-chain/notices/upstream/needy"), "{why}");
        assert!(!why.contains("plain 1.0.0"), "{why}");

        // A package whose folder is not in the registry is named too; so is a missing embedded file and a missing schema notice.
        let mut more_packages = packages.clone();
        more_packages.push(package("ghostly", "3.0.0", "MIT"));
        more_packages.push(package("unknown-to-cargo", "1.0.0", "MIT"));
        let mut more_infos = infos.clone();
        more_infos.push(info("ghostly", "3.0.0", Some(base.join("registry/ghostly-3.0.0"))));
        fs::remove_file(root.join("crates/skipper-import/src/inflate.rs")).unwrap();
        fs::remove_dir_all(root.join("xtask/data/cc-src")).unwrap();
        let why = build_licence_file(&root, "0.1.0-test.1", &more_packages, &more_infos, &[]).unwrap_err();
        for needle in ["ghostly 3.0.0", "unknown-to-cargo 1.0.0", "inflate.rs", "cc-src"] {
            assert!(why.contains(needle), "{needle}\n{why}");
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_embedded_block_of_the_import_helper_is_puff_c_under_the_zlib_licence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace root").to_path_buf();
        let blocks = embedded_blocks(&root).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(blocks.len(), 1);
        let puff = &blocks[0];
        assert_eq!(puff.file, "crates/skipper-import/src/inflate.rs");
        assert!(root.join(&puff.file).is_file(), "the block names a file that exists");
        assert!(puff.title.contains("puff.c") && puff.licence == "Zlib", "{}", puff.title);
        assert!(puff.copyright.contains("Mark Adler"), "{}", puff.copyright);
        assert!(puff.text.contains("Altered source versions must be plainly marked as such"), "{}", puff.text);
        assert!(puff.text.contains("This notice may not be removed or altered from any source distribution."), "{}", puff.text);
        assert!(puff.note.contains("Skipper"), "the block says that the file was changed by the project: {}", puff.note);
        assert!(!puff.text.contains('@'), "no address in the text");
    }

    // ---- the extra blocks of notice list 5.2 fail closed (control SP-6 round 2, F1) ----

    /// The packages whose extra block `dist` must have, and the file of `supply-chain/notices/upstream/<package>/` that holds it.
    const BLOCK_FILES: [(&str, &str); 6] = [
        ("accesskit", "BLOCK-Chromium-BSD-3-Clause.txt"),
        ("accesskit_consumer", "BLOCK-Chromium-BSD-3-Clause.txt"),
        ("accesskit_windows", "BLOCK-Chromium-BSD-3-Clause.txt"),
        ("futures-channel", "BLOCK-Vyukov-BSD-2-Clause.txt"),
        ("read-fonts", "BLOCK-Adobe-BSD-3-Clause.txt"),
        ("libm", "BLOCK-libm-notices.txt"),
    ];

    /// A block text of the upstream folder: a BSD text with its holder line, and where it came from.
    fn block_file(name: &str) -> TextFile {
        TextFile {
            name: name.to_string(),
            text: format!(
                "Copyright (c) 2010 Block Holder of {name}\nRedistribution and use in source and binary forms, with or without \
                 modification, are permitted provided that the conditions are met.\n"
            ),
            source: Some(format!("https://example.org/blocks/{name}")),
        }
    }

    /// A package that ships its own MIT text, as the published crate of a table package does.
    fn with_own_text(name: &str) -> Component {
        comp(name, "1.2.3", "MIT OR Apache-2.0", vec![file("LICENSE-MIT", &mit_text(&["Copyright (c) 2020 Main Holder"]))])
    }

    #[test]
    fn a_package_of_the_block_table_stops_the_file_until_its_block_is_committed_even_with_its_own_text() {
        for (package, block_name) in BLOCK_FILES {
            let problems = licence_document(&input(vec![with_own_text(package)])).unwrap_err().join("\n");
            assert!(problems.contains(&format!("{package} 1.2.3")), "{package}: the package is named: {problems}");
            assert!(problems.contains(block_name), "{package}: the file of the block is named: {problems}");
            assert!(
                problems.contains(&format!("supply-chain/notices/upstream/{package}/")),
                "{package}: the message says where the block goes: {problems}"
            );

            let mut complete = with_own_text(package);
            complete.licence_files.push(block_file(block_name));
            let text = document(vec![complete]);
            let embedded = part(&text, 3);
            assert!(embedded.contains(&format!("{package} 1.2.3 - {block_name}")), "{package}\n{embedded}");
            assert!(embedded.contains(&format!("Source: https://example.org/blocks/{block_name}")), "{package}\n{embedded}");
            assert!(embedded.contains("Redistribution and use in source and binary forms"), "{package}\n{embedded}");
            assert!(!part(&text, 2).contains("Redistribution and use"), "{package}: the block is no licence group");
        }
    }

    #[test]
    fn every_missing_block_is_one_problem_and_a_package_with_its_block_is_not_named() {
        let mut done = with_own_text("accesskit");
        done.licence_files.push(block_file("BLOCK-Chromium-BSD-3-Clause.txt"));
        let problems = licence_document(&input(vec![
            with_own_text("futures-channel"),
            with_own_text("accesskit_windows"),
            done,
            with_own_text("unrelated"),
        ]))
        .unwrap_err();
        let all = problems.join("\n");
        assert_eq!(problems.len(), 2, "{all}");
        assert!(all.contains("futures-channel 1.2.3") && all.contains("accesskit_windows 1.2.3"), "{all}");
        assert!(!all.contains("accesskit 1.2.3") && !all.contains("unrelated"), "{all}");
    }

    #[test]
    fn a_block_file_is_never_taken_for_the_main_licence_text() {
        // The block names no licence, so the lenient pass would take it for the MIT text of the package.
        let mut lone = comp("accesskit", "0.21.0", "MIT OR Apache-2.0", Vec::new());
        lone.licence_files.push(TextFile {
            name: "BLOCK-Chromium-BSD-3-Clause.txt".to_string(),
            text: "A notice that names no licence.\n".to_string(),
            source: Some("https://example.org/chromium".to_string()),
        });
        let problems = licence_document(&input(vec![lone])).unwrap_err();
        let all = problems.join("\n");
        assert_eq!(problems.len(), 1, "{all}");
        assert!(all.contains("accesskit 0.21.0") && all.contains("no licence text for MIT"), "{all}");
    }

    #[test]
    fn the_build_stops_on_a_missing_block_of_a_package_that_ships_its_own_text() {
        let base = scratch("block");
        let (root, mut packages, mut infos) = site(&base);
        let folder = base.join("registry/futures-channel-0.3.34");
        put(&folder.join("LICENSE-MIT"), &mit_text(&["Copyright (c) 2016 Futures Authors"]));
        put(&folder.join("LICENSE-APACHE"), APACHE);
        packages.push(package("futures-channel", "0.3.34", "MIT OR Apache-2.0"));
        infos.push(info("futures-channel", "0.3.34", Some(folder)));

        let why = build_licence_file(&root, "0.1.0-test.1", &packages, &infos, &[]).unwrap_err();
        assert!(why.contains("futures-channel 0.3.34") && why.contains("BLOCK-Vyukov-BSD-2-Clause.txt"), "{why}");
        assert!(!why.contains("plain 1.0.0") && !why.contains("needy 2.0.0"), "{why}");

        put(
            &root.join("supply-chain/notices/upstream/futures-channel/BLOCK-Vyukov-BSD-2-Clause.txt"),
            "Source: https://example.org/futures-channel/queue.rs\nCopyright (c) 2010-2011 Dmitry Vyukov. All rights reserved.\n\
             Redistribution and use in source and binary forms, with or without modification, are permitted provided that the \
             following conditions are met.\n",
        );
        let text = build_licence_file(&root, "0.1.0-test.1", &packages, &infos, &[]).unwrap_or_else(|why| panic!("{why}"));
        let embedded = part(&text, 3);
        assert!(embedded.contains("futures-channel 0.3.34 - BLOCK-Vyukov-BSD-2-Clause.txt"), "{embedded}");
        assert!(embedded.contains("Source: https://example.org/futures-channel/queue.rs"), "{embedded}");
        assert!(embedded.contains("Dmitry Vyukov"), "{embedded}");
        assert!(part(&text, 1).contains("futures-channel 0.3.34 - MIT - "), "the main text is still the MIT text: {}", part(&text, 1));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_block_table_of_the_code_is_the_one_these_tests_pin() {
        let rows: Vec<(&str, &str)> = REQUIRED_BLOCKS.iter().map(|row| (row.package, row.file)).collect();
        assert_eq!(rows, BLOCK_FILES.to_vec());
        for row in REQUIRED_BLOCKS.iter() {
            assert!(!row.block.is_empty(), "{}: the message names the block", row.package);
            assert!(row.file.starts_with("BLOCK-") && row.file.ends_with(".txt"), "{}: {}", row.package, row.file);
        }
    }

    #[test]
    fn the_notice_list_says_exactly_which_blocks_stop_dist() {
        let doc = include_str!("../../docs/design/third-party-notice-list.md");
        for (package, block_name) in BLOCK_FILES {
            assert!(doc.contains(&format!("`{package}`")), "the notice list names {package}");
            assert!(doc.contains(block_name), "the notice list names the file {block_name}");
        }
        assert!(doc.contains("REQUIRED_BLOCKS"), "the notice list names the table of the code");
        assert!(
            !doc.contains("and the extra blocks of section 5.2, need their upstream text committed first"),
            "the old sentence claimed that every block of section 5.2 stops dist"
        );
    }
}
