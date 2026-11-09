// SPDX-License-Identifier: MIT
//! `cargo xtask installer` and the installer step of `cargo xtask dist --stage 1` (RL-36, plan Task 43, Rust-only path; MC-115, LOCK-2
//! (c), PRB-63): the per-user installer is the program itself (`skipper.exe install`, `crates/skipper-app/src/install.rs`). A release
//! ships it as one more file of `dist/stage1/`, `skipper-setup-<version>.exe`: a byte copy of `skipper.exe` that the program takes for
//! the installer by its name (`cli::launch_from`). No NSIS and no download of an installer tool; nothing is added to `Cargo.lock`.
//!
//! The step does three things, each tested in `xtask/tests/installer.rs`:
//!
//! * [`add_setup`] makes the copy from the `skipper.exe` of the folder, after it has read the pack of the folder and checked that the
//!   program inside the pack is that very file (the installer refuses a pack that belongs to another program, so a folder that does not
//!   belong together gets no setup), and writes `SHA256SUMS.txt` again over every file at the top: the executables, the pack and the setup.
//! * [`check_folder`] reads the folder as the person will: exactly the stage 1 set and the setup, the setup equal to the program, and
//!   every line of the checksum file true.
//! * [`scan_sources`] reads the sources of the installer for what an installer must never touch (the plan's script tests
//!   `no_autostart_or_admin_in_the_script` and `the_installer_lists_only_allowed_files`, in their Rust-only form): no start-up key, no
//!   service, no scheduled task, no search path or variable, no machine-wide hive or folder, no request for administrator rights. The
//!   file set of the installer is the update pack's, so it has no list of its own to check.
//!
//! `xtask` links no product code (MC-430), so the ties to the program are read from the text of its files in the tests.

use std::fs;
use std::path::Path;

use crate::pack::{self, sha256_hex, sums_text, top_entries, MAIN_PROGRAM, PACK_MAGIC, SUMS_FILE};

/// The start of the name of the installer file; the program takes a file name that starts so and ends in `.exe` for the installer
/// (`cli::is_setup_name`).
pub const SETUP_PREFIX: &str = "skipper-setup-";

/// The sources of the installer that [`scan_sources`] reads, relative to the workspace root: the logic, the run layer and the three
/// Windows files (the entry of the app list, the shell's link and message boxes, the temporary copy of the uninstall).
pub const SOURCES: [&str; 5] = [
    "crates/skipper-app/src/install.rs",
    "crates/skipper-app/src/install_run.rs",
    "crates/skipper-app/src/win/registry.rs",
    "crates/skipper-app/src/win/shell.rs",
    "crates/skipper-app/src/win/relay.rs",
];

/// The file name of the installer of a version.
pub fn setup_file_name(version: &str) -> String {
    format!("{SETUP_PREFIX}{version}.exe")
}

// ---- the pack ----------------------------------------------------------------------------------------------------------------

/// The next `n` bytes of `bytes` at `*at`; the position moves on.
fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], String> {
    let end = at.checked_add(n).ok_or_else(|| String::from("the pack is cut short"))?;
    let slice = bytes.get(*at..end).ok_or_else(|| String::from("the pack is cut short"))?;
    *at = end;
    Ok(slice)
}

/// The body of `skipper.exe` in the pack `bytes`, after the header of every entry was read and the hash of every body was checked
/// (the format is `pack.rs`'s: `SKPK1`, a `u32` count, per entry a `u16` name length, the name, a `u64` body length and the 32 bytes of
/// the SHA-256, then the bodies; little-endian).
fn program_in_pack(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut at = 0;
    if take(bytes, &mut at, PACK_MAGIC.len())? != PACK_MAGIC {
        return Err(String::from("it is not a pack (the first bytes are wrong)"));
    }
    let count = u32::from_le_bytes(take(bytes, &mut at, 4)?.try_into().map_err(|_| String::from("the pack is cut short"))?);
    let mut headers: Vec<(String, u64, [u8; 32])> = Vec::new();
    for _ in 0..count {
        let name_len = u16::from_le_bytes(take(bytes, &mut at, 2)?.try_into().map_err(|_| String::from("the pack is cut short"))?);
        let name = String::from_utf8_lossy(take(bytes, &mut at, usize::from(name_len))?).into_owned();
        let len = u64::from_le_bytes(take(bytes, &mut at, 8)?.try_into().map_err(|_| String::from("the pack is cut short"))?);
        let sha: [u8; 32] = take(bytes, &mut at, 32)?.try_into().map_err(|_| String::from("the pack is cut short"))?;
        headers.push((name, len, sha));
    }
    let mut program: Option<Vec<u8>> = None;
    for (name, len, sha) in headers {
        let size = usize::try_from(len).map_err(|_| String::from("the pack is too large"))?;
        let body = take(bytes, &mut at, size)?;
        if pack::sha256_hex(body) != sha.iter().map(|b| format!("{b:02x}")).collect::<String>() {
            return Err(format!("the hash of {name} in the pack is wrong"));
        }
        if name == MAIN_PROGRAM {
            program = Some(body.to_vec());
        }
    }
    if at != bytes.len() {
        return Err(String::from("bytes follow the last file of the pack"));
    }
    program.ok_or_else(|| format!("{MAIN_PROGRAM} is not in the pack"))
}

/// The program of the pack `skipper-<version>.pack` in `out`, checked, and the program `skipper.exe` of the folder; they must be one.
fn program_of_folder(out: &Path, version: &str) -> Result<Vec<u8>, String> {
    let pack_name = pack::pack_file_name(version);
    let pack_bytes = fs::read(out.join(&pack_name)).map_err(|e| format!("{pack_name} cannot be read: {e}"))?;
    let packed = program_in_pack(&pack_bytes).map_err(|why| format!("the pack {pack_name} is not usable: {why}"))?;
    let program = fs::read(out.join(MAIN_PROGRAM)).map_err(|e| format!("{MAIN_PROGRAM} cannot be read: {e}"))?;
    if packed != program {
        return Err(format!(
            "{MAIN_PROGRAM} of the folder is not the {MAIN_PROGRAM} in the pack {pack_name}: the folder does not belong together"
        ));
    }
    Ok(program)
}

// ---- the setup file ----------------------------------------------------------------------------------------------------------

/// The names of the files at the top of the folder that `SHA256SUMS.txt` lists: the executables, the pack and the setup.
fn listed_names(version: &str) -> Vec<String> {
    let mut names: Vec<String> = pack::EXECUTABLES.iter().map(|name| (*name).to_owned()).collect();
    names.push(pack::pack_file_name(version));
    names.push(setup_file_name(version));
    names.sort();
    names
}

/// Makes `skipper-setup-<version>.exe` in `out` (the folder `assemble` wrote) and lists it in `SHA256SUMS.txt`; returns the file name.
/// Stops, and makes nothing, when the pack is missing or damaged or does not carry the program of the folder. Run again it gives the
/// same bytes.
pub fn add_setup(out: &Path, version: &str) -> Result<String, String> {
    let program = program_of_folder(out, version)?;
    let name = setup_file_name(version);
    fs::write(out.join(&name), &program).map_err(|e| format!("{name} cannot be written: {e}"))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for listed in listed_names(version) {
        let bytes = fs::read(out.join(&listed)).map_err(|e| format!("{listed} cannot be read: {e}"))?;
        files.push((listed, bytes));
    }
    let view: Vec<(&str, &[u8])> = files.iter().map(|(listed, bytes)| (listed.as_str(), bytes.as_slice())).collect();
    let sums = sums_text(&view)?;
    fs::write(out.join(SUMS_FILE), sums).map_err(|e| format!("{SUMS_FILE} cannot be written: {e}"))?;
    Ok(name)
}

/// Reads the folder as the person will: exactly the stage 1 set and the setup at the top, the setup a byte copy of the program, the
/// pack carrying that program, and every line of `SHA256SUMS.txt` true and every file listed.
pub fn check_folder(out: &Path, version: &str) -> Result<(), String> {
    let setup = setup_file_name(version);
    let mut expected = top_entries(version);
    expected.push(setup.clone());
    expected.sort();
    let mut found: Vec<String> = Vec::new();
    let top = fs::read_dir(out).map_err(|e| format!("cannot read {}: {e}", out.display()))?;
    for entry in top {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", out.display()))?;
        found.push(entry.file_name().to_string_lossy().into_owned());
    }
    found.sort();
    if let Some(stray) = found.iter().find(|name| !expected.contains(name)) {
        return Err(format!("{stray}: not part of the release folder (it holds exactly the stage 1 set and {setup})"));
    }
    if let Some(missing) = expected.iter().find(|name| !found.contains(name)) {
        return Err(format!("{missing} is missing from the release folder"));
    }
    let program = program_of_folder(out, version)?;
    let copy = fs::read(out.join(&setup)).map_err(|e| format!("{setup} cannot be read: {e}"))?;
    if copy != program {
        return Err(format!("{setup} is not a byte copy of {MAIN_PROGRAM}"));
    }
    let text = fs::read_to_string(out.join(SUMS_FILE)).map_err(|e| format!("{SUMS_FILE} cannot be read: {e}"))?;
    let mut listed: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some((hash, name)) = line.split_once("  ") else {
            return Err(format!("{SUMS_FILE}: the line {line:?} is not a hash, two spaces and a name"));
        };
        listed.push((name.to_owned(), hash.to_owned()));
    }
    for name in listed_names(version) {
        let Some((_, hash)) = listed.iter().find(|(listed_name, _)| *listed_name == name) else {
            return Err(format!("{SUMS_FILE} does not list {name}"));
        };
        let bytes = fs::read(out.join(&name)).map_err(|e| format!("{name} cannot be read: {e}"))?;
        if *hash != sha256_hex(&bytes) {
            return Err(format!("{SUMS_FILE}: the hash of {name} is wrong"));
        }
    }
    if let Some((name, _)) = listed.iter().find(|(name, _)| !listed_names(version).contains(name)) {
        return Err(format!("{SUMS_FILE} lists {name}, which is not a file at the top of the folder"));
    }
    Ok(())
}

// ---- what the installer must never touch ---------------------------------------------------------------------------------------

/// Texts that name a place or a right an installer of this project must never use, compared without regard to letter case, as a part of
/// the line (the machine-wide registry hives, the start-up keys, the machine-wide folders and the request for administrator rights).
const FORBIDDEN_PARTS: [&str; 15] = [
    "hkey_local_machine",
    "hkey_classes_root",
    "hkey_users",
    "currentversion\\run",
    "runonce",
    "requireadministrator",
    "createservicew",
    "createservicea",
    "openscmanagerw",
    "openscmanagera",
    "programfiles",
    "programdata",
    "commonstartmenu",
    "commonappdata",
    "common_startmenu",
];

/// Words that are forbidden as whole words only (a program that changes a variable or starts a task, or the verb that asks for
/// administrator rights): a longer word that holds them is no finding.
const FORBIDDEN_WORDS: [&str; 3] = ["runas", "setx", "schtasks"];

/// What a line of code names that an installer must never touch; empty for a comment and for a clean line. The key `Environment` of the
/// person's hive (the place of a search path) is found by its name in quotes.
pub fn forbidden_in(line: &str) -> Vec<String> {
    if line.trim_start().starts_with("//") {
        return Vec::new();
    }
    // A path in a string may be written with doubled backslashes; both forms are the same text.
    let lower = line.replace("\\\\", "\\").to_ascii_lowercase();
    let mut found: Vec<String> = FORBIDDEN_PARTS.iter().filter(|part| lower.contains(*part)).map(|part| (*part).to_owned()).collect();
    for word in FORBIDDEN_WORDS {
        let whole = lower.match_indices(word).any(|(at, _)| {
            let before = lower[..at].chars().next_back();
            let after = lower[at + word.len()..].chars().next();
            let joined = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            !joined(before) && !joined(after)
        });
        if whole {
            found.push(word.to_owned());
        }
    }
    if lower.contains("\"environment\"") {
        found.push(String::from("the key Environment"));
    }
    found
}

/// Reads [`SOURCES`] below `root` and lists what each names that an installer must never touch, one line each (`file:line: what`). A
/// source that cannot be read is a finding too, never a pass.
pub fn scan_sources(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    for source in SOURCES {
        let Ok(text) = fs::read_to_string(root.join(source)) else {
            findings.push(format!("{source}: missing (the scan cannot read it)"));
            continue;
        };
        for (at, line) in text.lines().enumerate() {
            for what in forbidden_in(line) {
                findings.push(format!("{source}:{}: names {what}", at + 1));
            }
        }
    }
    findings
}

// ---- the command -------------------------------------------------------------------------------------------------------------

/// The step on `dist/stage1/` of the workspace the command runs in: the version from `Cargo.toml`, the setup, the check of the folder
/// and the scan of the sources. The line that says what was done.
fn run() -> Result<String, String> {
    let root = Path::new(".");
    let manifest = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let version = pack::workspace_version(&manifest)?;
    let out = root.join("dist").join("stage1");
    let name = add_setup(&out, &version)?;
    check_folder(&out, &version)?;
    let findings = scan_sources(root);
    if !findings.is_empty() {
        return Err(format!("the installer's sources name what an installer must never touch: {}", findings.join("; ")));
    }
    Ok(format!("{name} is in dist/stage1 and listed in {SUMS_FILE}"))
}

/// `installer [--stage 1]`: makes the setup file of `dist/stage1/`, checks the folder and scans the installer's sources. Prints
/// `installer: <what was done>` (exit 0) or the reason (exit 1); a wrong argument is exit 2.
pub fn run_cli(args: &[String]) -> u8 {
    match args {
        [] => {}
        [flag, stage] if flag == "--stage" && stage == "1" => {}
        _ => {
            eprintln!("usage: cargo xtask installer [--stage 1]");
            return 2;
        }
    }
    match run() {
        Ok(summary) => {
            println!("installer: {summary}");
            0
        }
        Err(message) => {
            println!("installer: {message}");
            1
        }
    }
}
