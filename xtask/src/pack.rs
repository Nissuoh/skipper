// SPDX-License-Identifier: MIT
//! The update pack of Stage 1 (Task 49k; MC-275, MC-274, TG-5): what `cargo xtask dist --stage 1` ships besides the portable
//! folder, written without any product code (`xtask.exe` links none, MC-430).
//!
//! **The pack** is the file `skipper-<version>.pack`: the whole program folder in one file that `skipper-update` downloads and
//! that the program checks again before it swaps anything. The format is `SKPK1`: the five bytes `SKPK1`, a `u32` count of
//! entries, for each entry a `u16` length of the name, the name (UTF-8, `/` between the parts), a `u64` length of the body and
//! the 32 bytes of its SHA-256, and then the bodies in the order of the entries; every number is little-endian. The reader is
//! `skipper_core::update::read_pack_with`. This module is the writer of the release step, so it keeps its own copy of the
//! format and of the rule for names; `xtask/tests/pack.rs` and `crates/skipper-core/tests/update.rs` hold the same bytes and the
//! same lists of good and bad names, and the first compares the file set with the core's constants by reading their text.
//!
//! **The file set** is what a swap replaces: the six executables at the top and the three folders `data`, `lang` and `licenses`.
//! Nothing else ships in the pack, and a name outside that set stops the build (the program would refuse such a pack, so the
//! release step refuses it first). The entries are sorted by name and carry no time stamp, so two builds of one commit give the
//! same bytes (PRB-63).
//!
//! **`SHA256SUMS.txt`** lists the files at the top of the folder (the executables and the pack) by their plain names, in the
//! shape `sha256sum` writes. The update helper reads only plain names and refuses a list with a folder in a name, so the files
//! inside the three folders are not listed there: the pack's own header holds their hashes and the program checks each one.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The first bytes of a pack (`skipper_core::update::PACK_MAGIC`).
pub const PACK_MAGIC: &[u8; 5] = b"SKPK1";

/// The executables at the top of `dist/stage1/`, in the order they are listed. The sign-in helper and the update helper are the
/// only ones with network code (MC-337 item 1, MC-274); the program's own list is `skipper_core::update::PROGRAM_EXECUTABLES`.
pub const EXECUTABLES: [&str; 6] = [
    "skipper.exe",
    "skipper-gateway.exe",
    "skipper-hook.exe",
    "skipper-import.exe",
    "skipper-mcp.exe",
    "skipper-update.exe",
];

/// The folders of `dist/stage1/` (`skipper_core::update::PROGRAM_FOLDERS`): the starter team and the shipped mechanisms, the
/// language files and the licences.
pub const FOLDERS: [&str; 3] = ["data", "lang", "licenses"];

/// The program itself: a pack without it is no program.
pub const MAIN_PROGRAM: &str = "skipper.exe";

/// The checksum file of a release (the name the update helper asks for).
pub const SUMS_FILE: &str = "SHA256SUMS.txt";

/// Where the language fragments are in the repository, and where they go in the program folder.
const LANG_SOURCE: &str = "crates/skipper-app/lang";

/// Where the starter team is in the repository; it goes to `data/starter` in the program folder.
const STARTER_SOURCE: &str = "crates/skipper-core/data/starter";

/// The name of the licence text inside the folder `licenses`.
const LICENCE_FILE: &str = "licenses/THIRD-PARTY-LICENSES";

/// Skipper's own licence text: the file `LICENSE` at the top of the source folder goes to `licenses/LICENSE` (SP-6), so every
/// copy of the program carries its own licence beside the third-party file.
const OWN_LICENCE_SOURCE: &str = "LICENSE";
const OWN_LICENCE_FILE: &str = "licenses/LICENSE";

/// The longest name of an entry, in bytes (`skipper_core::update::MAX_NAME_BYTES`).
const MAX_NAME_BYTES: usize = 200;

/// The base names Windows treats as devices, whatever extension follows.
const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(bytes));
    out
}

/// The lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha256(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

// ---- names -------------------------------------------------------------------------------------------------------------------

/// A plain part of a path: not empty, not `.` or `..`, no folder separator, drive or stream colon, wildcard or control
/// character, no space in front and no space or dot at the end (Windows would drop them and name another file), no device name.
pub fn is_safe_part(part: &str) -> bool {
    if part.is_empty() || part == "." || part == ".." || part.starts_with(' ') || part.ends_with([' ', '.']) {
        return false;
    }
    if part.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return false;
    }
    let stem = part.split('.').next().unwrap_or_default().to_ascii_lowercase();
    !DEVICE_NAMES.contains(&stem.as_str())
}

/// Whether `name` may be an entry of the pack: one of the executables at the top, or a file at least one folder deep inside one
/// of the three folders, every part plain, exactly as the files ship (lower case).
pub fn is_pack_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || !name.split('/').all(is_safe_part) {
        return false;
    }
    match name.split_once('/') {
        None => EXECUTABLES.contains(&name),
        Some((top, _)) => FOLDERS.contains(&top),
    }
}

/// The name of the pack of this version.
pub fn pack_file_name(version: &str) -> String {
    format!("skipper-{version}.pack")
}

/// The entries at the top of `dist/stage1/` for this version: the executables, the three folders, the pack and the checksum
/// file. Nothing else is there (a tool of the build machine does not ship).
pub fn top_entries(version: &str) -> Vec<String> {
    let mut names: Vec<String> = EXECUTABLES.iter().map(|name| (*name).to_owned()).collect();
    names.extend(FOLDERS.iter().map(|name| (*name).to_owned()));
    names.push(pack_file_name(version));
    names.push(SUMS_FILE.to_owned());
    names
}

// ---- the pack ----------------------------------------------------------------------------------------------------------------

/// A pack of these files, in this order (`skipper_core::update::write_pack`).
pub fn write_pack(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&u32::try_from(files.len()).unwrap_or(u32::MAX).to_le_bytes());
    for (name, body) in files {
        out.extend_from_slice(&u16::try_from(name.len()).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&u64::try_from(body.len()).unwrap_or(u64::MAX).to_le_bytes());
        out.extend_from_slice(&sha256(body));
    }
    for (_, body) in files {
        out.extend_from_slice(body);
    }
    out
}

/// Sorts `files` by name and checks them as the program will: every name a pack name, no name twice (without regard to letter
/// case), no file that is also a folder of another entry, and the program itself among them.
pub fn sort_and_check(files: &mut [(String, Vec<u8>)]) -> Result<(), String> {
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (name, _) in files.iter() {
        if !is_pack_name(name) {
            return Err(format!(
                "{name}: not a file of the program (only the executables at the top and files inside {} may ship)",
                FOLDERS.join(", ")
            ));
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!("{name}: the name is there twice (names are compared without regard to letter case)"));
        }
    }
    for name in &seen {
        for (at, _) in name.match_indices('/') {
            if seen.contains(&name[..at]) {
                return Err(format!("{}: a file and also a folder of another entry", &name[..at]));
            }
        }
    }
    if !files.iter().any(|(name, _)| name == MAIN_PROGRAM) {
        return Err(format!("{MAIN_PROGRAM} is not among the files (a pack without the program is no update)"));
    }
    Ok(())
}

/// The pack of `files`, sorted and checked ([`sort_and_check`]); the same files in any order give the same bytes.
pub fn build_pack(mut files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    sort_and_check(&mut files)?;
    Ok(write_pack(&files))
}

/// `SHA256SUMS.txt` over these files: `<64 hex digits>  <name>` per file, sorted by name, LF line ends. A name that is no plain
/// file name (it has a folder in it) is refused, and so is a name twice: the update helper reads such a list as a bad one.
pub fn sums_text(files: &[(&str, &[u8])]) -> Result<String, String> {
    let mut lines: Vec<(&str, String)> = Vec::new();
    for (name, body) in files {
        if !is_safe_part(name) {
            return Err(format!("{name}: {SUMS_FILE} lists plain file names only"));
        }
        lines.push((*name, sha256_hex(body)));
    }
    lines.sort();
    if lines.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(format!("{SUMS_FILE}: a name is listed twice"));
    }
    let mut text = String::new();
    for (name, hash) in &lines {
        text.push_str(&format!("{hash}  {name}\n"));
    }
    Ok(text)
}

/// The version of the workspace (`[workspace.package] version` in the text of the root `Cargo.toml`), if it can stand in a file
/// name: it starts with a digit and holds only letters, digits, `.`, `-` and `+`.
pub fn workspace_version(manifest: &str) -> Result<String, String> {
    let table = manifest.parse::<toml::Table>().map_err(|e| format!("Cargo.toml: {e}"))?;
    let version = table
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_string())?;
    let plain = !version.is_empty()
        && version.len() <= 64
        && version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if plain {
        Ok(version.to_owned())
    } else {
        Err(format!("Cargo.toml: the version {version:?} cannot stand in a file name"))
    }
}

// ---- the folder --------------------------------------------------------------------------------------------------------------

/// What [`assemble`] wrote.
#[derive(Debug, PartialEq, Eq)]
pub struct Assembled {
    /// The files in the executables and the three folders (the pack and the checksum file are not counted).
    pub files: usize,
    /// The file name of the pack and its size in bytes.
    pub pack: String,
    pub pack_bytes: usize,
}

/// The files below `dir` as (name in the program folder, path), sorted by name: `prefix` and the path inside `dir` with `/`
/// between the parts. With `extension`, only the files that have it. A link, a name that is not UTF-8 and anything that is
/// neither a file nor a folder stop the build.
fn tree(dir: &Path, prefix: &str, extension: Option<&str>) -> Result<Vec<(String, PathBuf)>, String> {
    fn walk(dir: &Path, prefix: &str, extension: Option<&str>, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
            let path = entry.path();
            let Ok(file_name) = entry.file_name().into_string() else {
                return Err(format!("{}: the name is not UTF-8", path.display()));
            };
            let kind = entry.file_type().map_err(|e| format!("cannot look at {}: {e}", path.display()))?;
            let name = format!("{prefix}/{file_name}");
            if kind.is_symlink() {
                return Err(format!("{}: a link cannot ship", path.display()));
            } else if kind.is_dir() {
                walk(&path, &name, extension, out)?;
            } else if kind.is_file() {
                let wanted = extension.is_none_or(|wanted| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(wanted)));
                if wanted {
                    out.push((name, path));
                }
            } else {
                return Err(format!("{}: neither a file nor a folder", path.display()));
            }
        }
        Ok(())
    }
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    walk(dir, prefix, extension, &mut found)?;
    found.sort();
    if found.is_empty() {
        return Err(format!("{}: nothing to ship in it", dir.display()));
    }
    Ok(found)
}

/// Writes `dist/stage1/` below `out`: the six executables read from `release`, the language fragments (`lang/`) and the starter
/// team (`data/starter/`) read below `source` (the workspace root), the licence text `licences` and Skipper's own `LICENSE` (read
/// from `source`; both go to `licenses/`), the pack of all of
/// them and `SHA256SUMS.txt` over the executables and the pack. Every input is read and checked before `out` is touched, so a
/// missing file leaves the old folder as it was; the pack and the checksum file are written last. Afterwards the top of the
/// folder is compared with [`top_entries`].
pub fn assemble(release: &Path, source: &Path, licences: &str, version: &str, out: &Path) -> Result<Assembled, String> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in EXECUTABLES {
        let bytes = fs::read(release.join(name))
            .map_err(|e| format!("{name} cannot be read from the release folder (build the release files first): {e}"))?;
        files.push((name.to_owned(), bytes));
    }
    let mut listed = tree(&source.join(LANG_SOURCE), "lang", Some("toml"))?;
    listed.extend(tree(&source.join(STARTER_SOURCE), "data/starter", None)?);
    for (name, path) in listed {
        let bytes = fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        files.push((name, bytes));
    }
    files.push((LICENCE_FILE.to_owned(), licences.as_bytes().to_vec()));
    let own = fs::read(source.join(OWN_LICENCE_SOURCE)).map_err(|e| {
        format!("{OWN_LICENCE_SOURCE} cannot be read from the source folder (Skipper's own licence text ships with every release): {e}")
    })?;
    files.push((OWN_LICENCE_FILE.to_owned(), own));
    sort_and_check(&mut files)?;
    let pack = write_pack(&files);
    let pack_name = pack_file_name(version);
    let mut at_top: Vec<(&str, &[u8])> =
        files.iter().filter(|(name, _)| !name.contains('/')).map(|(name, bytes)| (name.as_str(), bytes.as_slice())).collect();
    at_top.push((pack_name.as_str(), pack.as_slice()));
    let sums = sums_text(&at_top)?;

    if out.exists() {
        fs::remove_dir_all(out).map_err(|e| format!("cannot clear {}: {e}", out.display()))?;
    }
    for (name, bytes) in &files {
        let target = out.join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(&target, bytes).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    }
    fs::write(out.join(&pack_name), &pack).map_err(|e| format!("cannot write {pack_name}: {e}"))?;
    fs::write(out.join(SUMS_FILE), sums).map_err(|e| format!("cannot write {SUMS_FILE}: {e}"))?;

    let mut found: Vec<String> = Vec::new();
    let top = fs::read_dir(out).map_err(|e| format!("cannot read {}: {e}", out.display()))?;
    for entry in top {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", out.display()))?;
        found.push(entry.file_name().to_string_lossy().into_owned());
    }
    found.sort();
    let mut expected = top_entries(version);
    expected.sort();
    if found != expected {
        return Err(format!("the top of {} is {found:?}, not {expected:?}", out.display()));
    }
    Ok(Assembled { files: files.len(), pack: pack_name, pack_bytes: pack.len() })
}
