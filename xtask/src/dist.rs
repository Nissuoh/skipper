// SPDX-License-Identifier: MIT
//! `cargo xtask dist`: assembles `dist/stage0/` from the release build (plan Task 17, MC-117, MC-119, MC-121).
//!
//! Contents: `skipper.exe`, `skipper-hook.exe`, `skipper-mcp.exe`, `skipper-standin.exe`, `skipper-import.exe`,
//! `prb11/conpty.dll` (the dummy DLL of PRB-11), `LICENSE` (Skipper's own licence text), `THIRD-PARTY-LICENSES` (the seven
//! parts of `docs/design/third-party-notice-list.md` section 2, through `deps::build_licence_file`) and `SHA256SUMS` (written
//! last, over every other file). `dist` stops, naming the package, while a shipped package has no licence text (SP-6).
//! The release files are read from `%CARGO_TARGET_DIR%\x86_64-pc-windows-gnu\release\`. The runtime
//! notices are read at run time from `supply-chain/notices/winpthreads.txt` and
//! `supply-chain/notices/libgcc-runtime-exception.txt` (first line `Source: <where>`, then the notice
//! verbatim, byte for byte as upstream has it); `dist` stops with an error while one is missing or has no
//! `Source:` line.
//!
//! `cargo xtask dist --stage 1` (Stage 1 Tasks 43 and 49k; MC-115, MC-275, TG-5) assembles `dist/stage1/` instead: the six
//! executables of [`pack::EXECUTABLES`] (the program, the sign-in helper, the hook, the import helper, the tool server and the
//! update helper `skipper-update.exe`), the folders `lang/`, `data/` (the starter team) and `licenses/`, the update pack
//! `skipper-<version>.pack` of all of them and `SHA256SUMS.txt` over the executables and the pack. The work is `pack::assemble`
//! (tested in `xtask/tests/pack.rs`; it also puts Skipper's own `LICENSE` into `licenses/`); here the licence text is made
//! (`deps::build_licence_file`: the texts of the packages from cargo's registry, fail closed), and every executable then passes
//! `pe-check`: only the
//! sign-in helper and the update helper may import a network DLL. The per-user installer (RL-36, Task 43) is the last part of this
//! step: `skipper-setup-<version>.exe`, a byte copy of `skipper.exe` that the program takes for the installer by its name, is made
//! and listed in `SHA256SUMS.txt` (`installer::add_setup`), the folder is checked as a person will read it (`installer::check_folder`)
//! and the installer's sources are scanned for autostart, services, search paths and machine-wide places (`installer::scan_sources`).
//! No NSIS and no download of an installer tool.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use xtask::installer;
use xtask::pack;

use crate::deps;

const TARGET: &str = "x86_64-pc-windows-gnu";

/// The release files that ship in dist/stage0/ (the dummy DLL goes to prb11/).
const EXECUTABLES: [&str; 5] = ["skipper.exe", "skipper-hook.exe", "skipper-mcp.exe", "skipper-standin.exe", "skipper-import.exe"];

/// The packages whose dependency closure is listed in THIRD-PARTY-LICENSES: the package of each executable of [`EXECUTABLES`],
/// so a program that ships here also has its crates listed (`skipper-import` ships the import helper, RL-05, IMP-13, GI-10).
const LICENCE_ROOTS: [&str; 5] = ["skipper-app", "skipper-hook", "skipper-import", "skipper-mcp", "skipper-standin"];

/// The packages whose dependency closure is listed in the THIRD-PARTY-LICENSES of Stage 1: the package of each program that
/// ships in `dist/stage1/` (the stand-in is a test double and does not ship there).
const STAGE1_LICENCE_ROOTS: [&str; 6] =
    ["skipper-app", "skipper-gateway", "skipper-hook", "skipper-import", "skipper-mcp", "skipper-update"];

/// (title in THIRD-PARTY-LICENSES, notice file) of the statically linked mingw-w64 runtime (MC-117).
const NOTICES: [(&str, &str); 2] = [
    ("winpthreads (mingw-w64)", "supply-chain/notices/winpthreads.txt"),
    ("libgcc runtime library exception (GCC)", "supply-chain/notices/libgcc-runtime-exception.txt"),
];

/// What is wrong with a notice file, if anything: the first line must be `Source: <where>` and the
/// verbatim notice must follow.
fn notice_problem(text: &str) -> Option<&'static str> {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    match first.strip_prefix("Source:") {
        Some(rest) if !rest.trim().is_empty() => {}
        _ => return Some("the first line must be `Source: <where>`"),
    }
    if lines.all(|l| l.trim().is_empty()) {
        return Some("no notice text follows the Source line");
    }
    None
}

fn read_notices() -> Result<Vec<(String, String)>, String> {
    read_notices_from(Path::new("."))
}

/// The notices of `NOTICES`, read from below `root` (the workspace root; `dist` runs there).
fn read_notices_from(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut notices = Vec::new();
    for (title, path) in NOTICES {
        let raw = fs::read_to_string(root.join(path))
            .map_err(|_| format!("{path} is missing (it holds the upstream notice; plan Task 17 Step 3, MC-117)"))?;
        let text = raw.replace("\r\n", "\n");
        if let Some(problem) = notice_problem(&text) {
            return Err(format!("{path}: {problem}"));
        }
        notices.push((title.to_string(), text));
    }
    Ok(notices)
}

/// THIRD-PARTY-LICENSES for the programs whose packages are `roots`: the shipped packages from `cargo metadata`, their licence,
/// copyright and NOTICE texts from cargo's registry, and the other parts of the file ([`deps::build_licence_file`]). `root` is
/// the workspace root (`dist` runs there). The error names every package whose text is missing; no file is made then.
fn licence_file(root: &Path, version: &str, roots: &[&str], notices: &[(String, String)]) -> Result<String, String> {
    let (graph, infos) = deps::load_graph_and_infos()?;
    let packages = deps::closure(&graph, roots);
    deps::build_licence_file(root, version, &packages, &infos, notices)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let raw: &[u8] = digest.as_ref();
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// `SHA256SUMS`: `<hash>  <relative path>` per file (like `sha256sum`), sorted by path, LF line ends.
fn sha256sums(root: &Path) -> Result<String, String> {
    let mut files: Vec<String> = Vec::new();
    collect_files(root, root, &mut files).map_err(|e| format!("cannot list {}: {e}", root.display()))?;
    files.retain(|f| f != "SHA256SUMS");
    files.sort();
    let mut text = String::new();
    for relative in &files {
        let bytes = fs::read(root.join(relative)).map_err(|e| format!("cannot read {relative}: {e}"))?;
        text.push_str(&format!("{}  {relative}\n", sha256_hex(&bytes)));
    }
    Ok(text)
}

fn dist() -> Result<usize, String> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target"));
    let release = target_dir.join(TARGET).join("release");
    let out = Path::new("dist").join("stage0");

    // Check every input before anything is replaced.
    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::new();
    for name in EXECUTABLES {
        copies.push((release.join(name), out.join(name)));
    }
    copies.push((release.join("conpty.dll"), out.join("prb11").join("conpty.dll")));
    for (source, _) in &copies {
        if !source.is_file() {
            let name = source.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            return Err(format!("{name} is missing in the release folder of {TARGET} (build the release files first)"));
        }
    }
    let notices = read_notices()?;
    let root = Path::new(".");
    let manifest = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let version = pack::workspace_version(&manifest)?;
    let own_licence = fs::read_to_string(root.join("LICENSE")).map_err(|e| format!("cannot read LICENSE (Skipper's own licence text): {e}"))?;
    let licences = licence_file(root, &version, &LICENCE_ROOTS, &notices)?;

    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| format!("cannot clear dist/stage0: {e}"))?;
    }
    fs::create_dir_all(out.join("prb11")).map_err(|e| format!("cannot create dist/stage0: {e}"))?;
    for (source, destination) in &copies {
        fs::copy(source, destination).map_err(|e| format!("cannot copy {}: {e}", destination.display()))?;
    }
    fs::write(out.join("LICENSE"), own_licence).map_err(|e| format!("cannot write LICENSE: {e}"))?;
    fs::write(out.join("THIRD-PARTY-LICENSES"), licences).map_err(|e| format!("cannot write THIRD-PARTY-LICENSES: {e}"))?;
    let sums = sha256sums(&out)?;
    fs::write(out.join("SHA256SUMS"), sums).map_err(|e| format!("cannot write SHA256SUMS: {e}"))?;
    Ok(copies.len() + 2) // the copied files, LICENSE and THIRD-PARTY-LICENSES; SHA256SUMS is counted separately
}

/// `dist --stage 1`: assembles `dist/stage1/` (see the head of this file) and runs `pe-check` over its executables. The reason
/// of a failure, or the line that says what was made.
fn dist_stage1() -> Result<String, String> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target"));
    let release = target_dir.join(TARGET).join("release");
    let root = Path::new(".");
    let manifest = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let version = pack::workspace_version(&manifest)?;
    let notices = read_notices()?;
    let licences = licence_file(root, &version, &STAGE1_LICENCE_ROOTS, &notices)?;
    let out = Path::new("dist").join("stage1");
    let done = pack::assemble(&release, root, &licences, &version, &out)?;
    let executables: Vec<String> = pack::EXECUTABLES.iter().map(|name| out.join(name).to_string_lossy().into_owned()).collect();
    if crate::pe::run_cli(&executables) != 0 {
        return Err("pe-check found problems in an executable of dist/stage1 (see the lines above); do not ship the folder".to_string());
    }
    // The per-user installer (RL-36, Task 43): the setup file, the check of the folder as a person reads it, the scan of the sources.
    let setup = installer::add_setup(&out, &version)?;
    installer::check_folder(&out, &version)?;
    let findings = installer::scan_sources(root);
    if !findings.is_empty() {
        return Err(format!("the installer's sources name what an installer must never touch: {}", findings.join("; ")));
    }
    Ok(format!(
        "dist/stage1 ready ({} files, {} of {} bytes, the installer {} and {})",
        done.files,
        done.pack,
        done.pack_bytes,
        setup,
        pack::SUMS_FILE
    ))
}

/// `dist [--stage 0|1]`: prints `dist: dist/stage0 ready (<n> files)` (exit 0) or the reason (exit 1); with `--stage 1` it
/// makes `dist/stage1/` instead (exit codes as above).
pub fn run_cli(args: &[String]) -> u8 {
    let stage1 = match args {
        [] => false,
        [flag, stage] if flag == "--stage" && stage == "0" => false,
        [flag, stage] if flag == "--stage" && stage == "1" => true,
        _ => {
            eprintln!("usage: cargo xtask dist [--stage 0|1]");
            return 2;
        }
    };
    if stage1 {
        return match dist_stage1() {
            Ok(summary) => {
                println!("dist: {summary}");
                0
            }
            Err(message) => {
                println!("dist: {message}");
                1
            }
        };
    }
    match dist() {
        Ok(files) => {
            println!("dist: dist/stage0 ready ({files} files plus SHA256SUMS)");
            0
        }
        Err(message) => {
            println!("dist: {message}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licence_notice_needs_source_line_and_text() {
        assert_eq!(notice_problem("Source: upstream page\nCopyright (c) example\n"), None);
        assert!(notice_problem("Copyright (c) example\n").is_some());
        assert!(notice_problem("Source: \nCopyright (c) example\n").is_some());
        assert!(notice_problem("Source: upstream page\n\n").is_some());
        assert!(notice_problem("").is_some());
    }

    #[test]
    fn licence_sums_hash_is_lowercase_sha256() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    // ---- review R5, I7: the two runtime notice files ----

    /// The workspace root (xtask sits directly below it); the tests run with xtask as the current folder.
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace root").to_path_buf()
    }

    #[test]
    fn the_runtime_notice_files_hold_the_upstream_texts_unchanged() {
        // (file, SHA-256 of the upstream file that follows the Source line, byte for byte)
        let expected = [
            ("supply-chain/notices/winpthreads.txt", "63263614cdd29f2f93cba85e992f041b31f9fc7b4033692f31269489a8a1b177"),
            ("supply-chain/notices/libgcc-runtime-exception.txt", "9d6b43ce4d8de0c878bf16b54d8e7a10d9bd42b75178153e3af6a815bdc90f74"),
        ];
        for (path, sha) in expected {
            let full = fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(notice_problem(&full), None, "{path}");
            let (first, body) = full.split_once('\n').expect(path);
            assert!(first.starts_with("Source: https://"), "{path}: {first}");
            assert_eq!(sha256_hex(body.as_bytes()), sha, "{path}");
        }
    }

    #[test]
    fn dist_reads_both_notices_from_the_repository() {
        let notices = read_notices_from(&repo_root()).unwrap();
        assert_eq!(notices.len(), 2);
        assert!(notices[0].1.contains("mingw-w64 project"), "{}", notices[0].1);
        assert!(notices[1].1.contains("GCC RUNTIME LIBRARY EXCEPTION"), "{}", notices[1].1);
    }

    // ---- Stage 1 (Task 49k): the licence roots and the arguments ----

    #[test]
    fn every_stage1_program_has_a_licence_root_and_the_test_double_has_none() {
        for exe in pack::EXECUTABLES {
            let package = match exe.strip_suffix(".exe") {
                Some("skipper") => "skipper-app",
                Some(other) => other,
                None => exe,
            };
            assert!(STAGE1_LICENCE_ROOTS.contains(&package), "{exe}");
        }
        assert_eq!(STAGE1_LICENCE_ROOTS.len(), pack::EXECUTABLES.len());
        assert!(!STAGE1_LICENCE_ROOTS.contains(&"skipper-standin"));
    }

    #[test]
    fn dist_takes_no_argument_or_the_stage_and_nothing_else() {
        let words = |list: &[&str]| -> Vec<String> { list.iter().map(|word| (*word).to_string()).collect() };
        for bad in [vec!["--stage"], vec!["--stage", "2"], vec!["stage", "1"], vec!["--stage", "1", "x"], vec!["x"]] {
            assert_eq!(run_cli(&words(&bad)), 2, "{bad:?}");
        }
    }

    #[test]
    fn a_missing_notice_stops_dist_with_a_clear_message() {
        let empty = std::env::temp_dir().join(format!("skipper-dist-test-{}", std::process::id()));
        fs::create_dir_all(&empty).unwrap();
        let message = read_notices_from(&empty).unwrap_err();
        let _ = fs::remove_dir_all(&empty);
        assert!(message.contains("winpthreads.txt") && message.contains("missing"), "{message}");
    }

    // ---- RL-05 (IMP-13, GI-10): the import helper ships with its licences in Stage 0 as it does in Stage 1 ----

    #[test]
    fn skipper_import_is_shipped_with_its_licences() {
        assert!(EXECUTABLES.contains(&"skipper-import.exe"), "the import helper is not in the Stage 0 file list");
        assert!(LICENCE_ROOTS.contains(&"skipper-import"), "the import helper has no root in the Stage 0 licence list");
        assert!(STAGE1_LICENCE_ROOTS.contains(&"skipper-import"), "the import helper has no root in the Stage 1 licence list");
    }

    #[test]
    fn every_stage0_program_has_a_licence_root() {
        for exe in EXECUTABLES {
            let package = match exe.strip_suffix(".exe") {
                Some("skipper") => "skipper-app",
                Some(other) => other,
                None => exe,
            };
            assert!(LICENCE_ROOTS.contains(&package), "{exe}");
        }
        assert_eq!(LICENCE_ROOTS.len(), EXECUTABLES.len());
    }
}
