// SPDX-License-Identifier: MIT
//! `cargo xtask pe-check [<file>...]`: the output check of MC-114 with an own PE reader
//! (standard library only; the native build path has no `objdump`, and a PE crate would be a new
//! download; plan Task 17, ruling NR-18).
//!
//! `parse_pe` reads the DOS header, the PE header, the optional header (DllCharacteristics) and the
//! names in the import directory and in the delay-load import directory (a DLL that is loaded on first
//! use is an import all the same). `check_pe` finds non-system DLL imports, a missing hardening
//! characteristic and an oversized file. Without arguments, `run_cli` checks every `.exe` and `.dll`
//! under `dist/stage0/`, so no shell wildcard is needed (NR-21).
//!
//! Every read is bounds-checked, and anything the reader cannot follow is an error (the file is then
//! reported as "not checkable", which fails the check): it never guesses and never panics.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA
pub const HIGH_ENTROPY_VA: u16 = 0x0020;
/// IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE
pub const DYNAMIC_BASE: u16 = 0x0040;
/// IMAGE_DLLCHARACTERISTICS_NX_COMPAT
pub const NX_COMPAT: u16 = 0x0100;

/// Size limit of one shipped file: 64 MiB (Lead's call, 2026-11-08; the spec names none). It was 40 MiB until 2026-11-08, when the release
/// `skipper.exe` of the 1.0 scope (41,970,688 bytes) grew 27,648 bytes past it and `cargo xtask dist --stage 1` refused the folder.
/// The Lead raised the cap rather than cut the program: the release profile keeps the symbols on purpose, so that a crash report
/// carries a readable backtrace. 64 MiB still catches what a size cap is for here, an accidental debug-info or unoptimised build,
/// which is several times larger than the release program.
const SIZE_LIMIT: u64 = 64 << 20;

/// Windows system DLLs of a clean Windows 10 and 11 (builder's call).
const SYSTEM_DLLS: &str = include_str!("system-dlls.txt");

/// The magic number of an optional header in PE32+ (64-bit) format.
const PE32_PLUS_MAGIC: u16 = 0x20B;
/// Offsets inside a PE32+ optional header.
const OPT_DLL_CHARACTERISTICS: usize = 70;
const OPT_NUMBER_OF_RVA_AND_SIZES: usize = 108;
const OPT_DATA_DIRECTORIES: usize = 112; // the table of data directories: 8 bytes each (RVA, then size)
const OPT_IMPORT_DIRECTORY: usize = OPT_DATA_DIRECTORIES + 8; // data directory no. 1
const OPT_DELAY_IMPORT_DIRECTORY: usize = OPT_DATA_DIRECTORIES + 13 * 8; // data directory no. 13
const DIRECTORY_ENTRY_SIZE: u64 = 8;
const IMPORT_DESCRIPTOR_SIZE: usize = 20;
/// `IMAGE_DELAYLOAD_DESCRIPTOR`: attributes, DLL name RVA, module handle RVA, import address table RVA,
/// import name table RVA, bound table RVA, unload table RVA, time stamp (4 bytes each).
const DELAY_DESCRIPTOR_SIZE: usize = 32;
/// Bit 0 of the attributes of a delay-load descriptor: its fields are RVAs (not virtual addresses).
const DELAY_ATTRIBUTE_RVA: u32 = 1;
const SECTION_HEADER_SIZE: usize = 40;
const MAX_IMPORTS: usize = 4096;

#[derive(Clone, PartialEq, Debug)]
pub struct PeFacts {
    pub dlls: Vec<String>,
    /// The DLLs of the delay-load import directory (loaded on first use, not at start).
    pub delay_dlls: Vec<String>,
    pub dynamic_base: bool,
    pub nx_compat: bool,
    pub high_entropy_va: bool,
    pub size: u64,
}

#[derive(Clone, PartialEq, Debug)]
pub enum PeError {
    NotPe,
    NotPe32Plus,
    Truncated(&'static str),
    /// A structure that the reader does not follow (for example a delay-load descriptor in the old
    /// address form): reported, never guessed at.
    Malformed(&'static str),
}

#[derive(Clone, PartialEq, Debug)]
pub enum PeFinding {
    NonSystemDll(String),
    NonSystemDelayDll(String),
    MissingCharacteristic(&'static str),
    TooLarge(u64),
    /// A network DLL (see [`NETWORK_DLLS`]) in a file that may not import one (MC-337 item 1, HUB-13).
    NetworkDll(String),
}

struct Section {
    virtual_address: u32,
    raw_size: u32,
    raw_pointer: u32,
}

fn slice_at<'a>(bytes: &'a [u8], offset: usize, len: usize, what: &'static str) -> Result<&'a [u8], PeError> {
    let end = offset.checked_add(len).ok_or(PeError::Truncated(what))?;
    bytes.get(offset..end).ok_or(PeError::Truncated(what))
}

fn u16_at(bytes: &[u8], offset: usize, what: &'static str) -> Result<u16, PeError> {
    let s = slice_at(bytes, offset, 2, what)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(bytes: &[u8], offset: usize, what: &'static str) -> Result<u32, PeError> {
    let s = slice_at(bytes, offset, 4, what)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rva_to_offset(sections: &[Section], rva: u32) -> Option<usize> {
    for s in sections {
        if rva >= s.virtual_address {
            let relative = rva - s.virtual_address;
            if relative < s.raw_size {
                return Some(relative as usize + s.raw_pointer as usize);
            }
        }
    }
    None
}

fn c_string_at(bytes: &[u8], offset: usize, what: &'static str) -> Result<String, PeError> {
    let tail = bytes.get(offset..).ok_or(PeError::Truncated(what))?;
    let window = &tail[..tail.len().min(512)];
    let end = window.iter().position(|&b| b == 0).ok_or(PeError::Truncated(what))?;
    Ok(String::from_utf8_lossy(&window[..end]).to_string())
}

/// DOS header, PE header, optional header (DllCharacteristics), import directory names and delay-load
/// import directory names; size = bytes.len().
pub fn parse_pe(bytes: &[u8]) -> Result<PeFacts, PeError> {
    if bytes.len() < 64 {
        return Err(PeError::Truncated("dos header"));
    }
    if &bytes[0..2] != b"MZ" {
        return Err(PeError::NotPe);
    }
    let pe_offset = u32_at(bytes, 0x3C, "dos header")? as usize;
    if slice_at(bytes, pe_offset, 4, "pe header")? != b"PE\0\0" {
        return Err(PeError::NotPe);
    }
    let coff = pe_offset + 4;
    let section_count = u16_at(bytes, coff + 2, "coff header")? as usize;
    let optional_size = u16_at(bytes, coff + 16, "coff header")? as usize;
    let optional = coff + 20;
    if u16_at(bytes, optional, "optional header")? != PE32_PLUS_MAGIC {
        return Err(PeError::NotPe32Plus);
    }
    if optional_size < OPT_NUMBER_OF_RVA_AND_SIZES + 4 {
        return Err(PeError::Truncated("optional header"));
    }
    let characteristics = u16_at(bytes, optional + OPT_DLL_CHARACTERISTICS, "optional header")?;
    let directory_count = u32_at(bytes, optional + OPT_NUMBER_OF_RVA_AND_SIZES, "optional header")? as usize;
    // The declared data directories must fit in the optional header. Otherwise the import list could be
    // skipped silently, and a file that hides its imports would pass.
    let needed = OPT_DATA_DIRECTORIES as u64 + directory_count as u64 * DIRECTORY_ENTRY_SIZE;
    if needed > optional_size as u64 {
        return Err(PeError::Truncated("optional header"));
    }

    let mut sections: Vec<Section> = Vec::new();
    let table = optional + optional_size;
    for i in 0..section_count {
        let at = table + i * SECTION_HEADER_SIZE;
        sections.push(Section {
            virtual_address: u32_at(bytes, at + 12, "section table")?,
            raw_size: u32_at(bytes, at + 16, "section table")?,
            raw_pointer: u32_at(bytes, at + 20, "section table")?,
        });
    }

    let mut dlls: Vec<String> = Vec::new();
    if directory_count > 1 {
        let import_rva = u32_at(bytes, optional + OPT_IMPORT_DIRECTORY, "data directory")?;
        if import_rva != 0 {
            let mut at = rva_to_offset(&sections, import_rva).ok_or(PeError::Truncated("import directory"))?;
            loop {
                let descriptor = slice_at(bytes, at, IMPORT_DESCRIPTOR_SIZE, "import directory")?;
                if descriptor.iter().all(|&b| b == 0) {
                    break;
                }
                let name_rva = u32::from_le_bytes([descriptor[12], descriptor[13], descriptor[14], descriptor[15]]);
                if name_rva == 0 {
                    break;
                }
                let name_at = rva_to_offset(&sections, name_rva).ok_or(PeError::Truncated("import name"))?;
                dlls.push(c_string_at(bytes, name_at, "import name")?);
                at += IMPORT_DESCRIPTOR_SIZE;
                if dlls.len() > MAX_IMPORTS {
                    return Err(PeError::Truncated("import directory"));
                }
            }
        }
    }

    // The delay-load import directory (data directory no. 13): DLLs that are loaded on first use.
    let mut delay_dlls: Vec<String> = Vec::new();
    if directory_count > 13 {
        let delay_rva = u32_at(bytes, optional + OPT_DELAY_IMPORT_DIRECTORY, "data directory")?;
        if delay_rva != 0 {
            let mut at = rva_to_offset(&sections, delay_rva).ok_or(PeError::Truncated("delay import directory"))?;
            loop {
                let descriptor = slice_at(bytes, at, DELAY_DESCRIPTOR_SIZE, "delay import directory")?;
                if descriptor.iter().all(|&b| b == 0) {
                    break;
                }
                let attributes = u32::from_le_bytes([descriptor[0], descriptor[1], descriptor[2], descriptor[3]]);
                if attributes & DELAY_ATTRIBUTE_RVA == 0 {
                    return Err(PeError::Malformed("delay import descriptor in the old address form"));
                }
                let name_rva = u32::from_le_bytes([descriptor[4], descriptor[5], descriptor[6], descriptor[7]]);
                if name_rva == 0 {
                    return Err(PeError::Malformed("delay import descriptor without a name"));
                }
                let name_at = rva_to_offset(&sections, name_rva).ok_or(PeError::Truncated("delay import name"))?;
                delay_dlls.push(c_string_at(bytes, name_at, "delay import name")?);
                at += DELAY_DESCRIPTOR_SIZE;
                if delay_dlls.len() > MAX_IMPORTS {
                    return Err(PeError::Truncated("delay import directory"));
                }
            }
        }
    }

    Ok(PeFacts {
        dlls,
        delay_dlls,
        dynamic_base: characteristics & DYNAMIC_BASE != 0,
        nx_compat: characteristics & NX_COMPAT != 0,
        high_entropy_va: characteristics & HIGH_ENTROPY_VA != 0,
        size: bytes.len() as u64,
    })
}

/// A DLL name in lower case without the `.dll` ending.
fn normalise(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    lower.strip_suffix(".dll").unwrap_or(&lower).to_string()
}

/// The system DLL list: names without `.dll`, in lower case; an entry ending in `*` is a prefix.
pub fn system_dll_set() -> BTreeSet<String> {
    SYSTEM_DLLS
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(normalise)
        .collect()
}

/// The shape of an API-set name without `.dll`: `api-ms-win-<contract>-l<n>-<n>-<n>`, where the contract
/// is one or more parts of lower-case letters and digits (`api-ms-win-core-synch-l1-2-0`). Nothing else
/// passes: no path characters, no dots, no empty parts.
fn is_api_set(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("api-ms-win-") else { return false };
    let parts: Vec<&str> = rest.split('-').collect();
    if parts.len() < 4 {
        return false;
    }
    let (contract, version) = parts.split_at(parts.len() - 3);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let level = version[0].strip_prefix('l').is_some_and(digits);
    let simple = |s: &&str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    level && digits(version[1]) && digits(version[2]) && contract.iter().all(simple)
}

fn is_system(name: &str, system: &BTreeSet<String>) -> bool {
    let n = normalise(name);
    if system.contains(&n) {
        return true;
    }
    system.iter().any(|entry| match entry.strip_suffix('*') {
        // `api-ms-win-*` means the API-set names, not every name that starts that way
        Some("api-ms-win-") => is_api_set(&n),
        // any other wildcard: the rest of the name is plain letters, digits, `-` and `_`
        Some(prefix) => {
            n.strip_prefix(prefix).is_some_and(|rest| rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        }
        None => false,
    })
}

/// Non-system DLL imports (loaded at start or on first use), a missing DYNAMIC_BASE, NX_COMPAT or
/// HIGH_ENTROPY_VA, and a size over the limit.
pub fn check_pe(f: &PeFacts, system: &BTreeSet<String>, size_limit: u64) -> Vec<PeFinding> {
    let mut findings = Vec::new();
    for dll in &f.dlls {
        if !is_system(dll, system) {
            findings.push(PeFinding::NonSystemDll(dll.clone()));
        }
    }
    for dll in &f.delay_dlls {
        if !is_system(dll, system) {
            findings.push(PeFinding::NonSystemDelayDll(dll.clone()));
        }
    }
    if !f.dynamic_base {
        findings.push(PeFinding::MissingCharacteristic("DYNAMIC_BASE"));
    }
    if !f.nx_compat {
        findings.push(PeFinding::MissingCharacteristic("NX_COMPAT"));
    }
    if !f.high_entropy_va {
        findings.push(PeFinding::MissingCharacteristic("HIGH_ENTROPY_VA"));
    }
    if f.size > size_limit {
        findings.push(PeFinding::TooLarge(f.size));
    }
    findings
}

/// The DLLs of Windows' network stack, without `.dll` (MC-337 item 1: `skipper.exe` imports no network DLL; HUB-13): WinHTTP,
/// WinInet, the URL monikers, the old and the Microsoft Winsock layers and the HTTP server API. `ws2_32` is not here: it is
/// on the system list because the standard library may link it whenever a program touches `std::net`, so a PE import cannot
/// tell a socket from a start-up call; the source rule `net-lint` of `hub-lint` covers the code instead.
const NETWORK_DLLS: [&str; 6] = ["winhttp", "wininet", "urlmon", "wsock32", "mswsock", "httpapi"];

/// The shipped files that may import a network DLL: the sign-in helper and the update helper, the only Skipper programs
/// with network code (MC-337 item 1, MC-274).
const NETWORK_FILES: [&str; 2] = ["skipper-gateway.exe", "skipper-update.exe"];

/// True when the file with this name (no folder) may import a network DLL.
pub fn may_use_network(file_name: &str) -> bool {
    NETWORK_FILES.iter().any(|name| file_name.eq_ignore_ascii_case(name))
}

/// [`check_pe`] for the file called `file_name`: in the two helpers the network DLLs count as system DLLs, in every other file
/// each import of one is a [`PeFinding::NetworkDll`] (and no second, "non-system", finding for the same DLL).
pub fn check_pe_named(file_name: &str, f: &PeFacts, system: &BTreeSet<String>, size_limit: u64) -> Vec<PeFinding> {
    let network: BTreeSet<String> = NETWORK_DLLS.iter().map(|name| (*name).to_string()).collect();
    if may_use_network(file_name) {
        let mut extended = system.clone();
        extended.extend(network);
        return check_pe(f, &extended, size_limit);
    }
    let mut findings: Vec<PeFinding> = check_pe(f, system, size_limit)
        .into_iter()
        .filter(|finding| match finding {
            PeFinding::NonSystemDll(name) | PeFinding::NonSystemDelayDll(name) => !network.contains(&normalise(name)),
            _ => true,
        })
        .collect();
    for dll in f.dlls.iter().chain(f.delay_dlls.iter()) {
        let name = normalise(dll);
        if network.contains(&name) {
            let finding = PeFinding::NetworkDll(name);
            if !findings.contains(&finding) {
                findings.push(finding);
            }
        }
    }
    findings
}

fn describe(f: &PeFinding) -> String {
    match f {
        PeFinding::NonSystemDll(name) => format!("non-system DLL import {name}"),
        PeFinding::NonSystemDelayDll(name) => format!("non-system delay-load DLL import {name}"),
        PeFinding::MissingCharacteristic(name) => format!("missing characteristic {name}"),
        PeFinding::TooLarge(size) => format!("{size} bytes is over the size limit"),
        PeFinding::NetworkDll(name) => {
            format!("network DLL import {name}: only skipper-gateway.exe and skipper-update.exe may import it (MC-337 item 1)")
        }
    }
}

fn collect_binaries(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_binaries(&path, out)?;
        } else if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Every `.exe` and `.dll` under dist/stage0/, in a fixed order.
fn dist_files() -> Result<Vec<PathBuf>, String> {
    let root = Path::new("dist").join("stage0");
    if !root.is_dir() {
        return Err("dist/stage0 does not exist (run cargo xtask dist first)".to_string());
    }
    let mut files = Vec::new();
    collect_binaries(&root, &mut files).map_err(|e| format!("cannot read dist/stage0: {e}"))?;
    files.sort();
    if files.is_empty() {
        return Err("no .exe or .dll under dist/stage0".to_string());
    }
    Ok(files)
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// `pe-check [<file>...]` prints one line per file (size, characteristics, imports) and then
/// `pe-check: ok` (exit 0) or the findings (exit 1).
pub fn run_cli(args: &[String]) -> u8 {
    let files: Vec<PathBuf> = if args.is_empty() {
        match dist_files() {
            Ok(files) => files,
            Err(message) => {
                println!("pe-check: {message}");
                return 1;
            }
        }
    } else {
        args.iter().map(PathBuf::from).collect()
    };
    let system = system_dll_set();
    let mut findings = 0usize;
    for file in &files {
        let name = shown(file);
        let bytes = match fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("pe-check: {name}: cannot read ({e})");
                findings += 1;
                continue;
            }
        };
        match parse_pe(&bytes) {
            Ok(facts) => {
                let delay = if facts.delay_dlls.is_empty() { "none".to_string() } else { facts.delay_dlls.join(", ") };
                println!(
                    "pe-check: {name}: {} bytes, DYNAMIC_BASE={} NX_COMPAT={} HIGH_ENTROPY_VA={}, imports: {}, delay-load imports: {delay}",
                    facts.size,
                    facts.dynamic_base,
                    facts.nx_compat,
                    facts.high_entropy_va,
                    facts.dlls.join(", ")
                );
                let base_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                for finding in check_pe_named(base_name, &facts, &system, SIZE_LIMIT) {
                    println!("pe-check: {name}: finding: {}", describe(&finding));
                    findings += 1;
                }
            }
            Err(e) => {
                println!("pe-check: {name}: not checkable ({e:?})");
                findings += 1;
            }
        }
    }
    if findings == 0 {
        println!("pe-check: ok");
        0
    } else {
        println!("pe-check: {findings} finding(s)");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: u64 = 40 << 20;

    const SECTION_RVA: u32 = 0x1000;
    const RAW_POINTER: usize = 0x200;
    const COFF: usize = 0x44;
    const OPTIONAL: usize = COFF + 20;
    const ALL: u16 = DYNAMIC_BASE | NX_COMPAT | HIGH_ENTROPY_VA;

    fn put16(image: &mut [u8], at: usize, value: u16) {
        image[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(image: &mut [u8], at: usize, value: u32) {
        image[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// The content of the one section: the import descriptors, the delay-load descriptors (when there are
    /// delay-load names; their attributes field is `delay_attributes`), then the names. Returns the bytes,
    /// the RVA of the import directory and the RVA of the delay-load directory (0 without delay imports).
    fn section_with(dlls: &[&str], delay_dlls: &[&str], delay_attributes: u32) -> (Vec<u8>, u32, u32) {
        let import_len = (dlls.len() + 1) * IMPORT_DESCRIPTOR_SIZE;
        let delay_len = if delay_dlls.is_empty() { 0 } else { (delay_dlls.len() + 1) * DELAY_DESCRIPTOR_SIZE };
        let mut section = vec![0u8; import_len + delay_len];
        let mut names: Vec<u8> = Vec::new();
        for (i, dll) in dlls.iter().enumerate() {
            let name_rva = SECTION_RVA + (import_len + delay_len + names.len()) as u32;
            put32(&mut section, i * IMPORT_DESCRIPTOR_SIZE + 12, name_rva);
            names.extend_from_slice(dll.as_bytes());
            names.push(0);
        }
        for (i, dll) in delay_dlls.iter().enumerate() {
            let at = import_len + i * DELAY_DESCRIPTOR_SIZE;
            let name_rva = SECTION_RVA + (import_len + delay_len + names.len()) as u32;
            put32(&mut section, at, delay_attributes);
            put32(&mut section, at + 4, name_rva);
            names.extend_from_slice(dll.as_bytes());
            names.push(0);
        }
        section.extend_from_slice(&names);
        let delay_rva = if delay_dlls.is_empty() { 0 } else { SECTION_RVA + import_len as u32 };
        (section, SECTION_RVA, delay_rva)
    }

    /// A minimal PE32+ image built in memory (no binary fixture in the repository): DOS header, PE
    /// header, an optional header with the given DllCharacteristics and 16 data directories (the import
    /// directory at `import_rva`, the delay-load directory at `delay_rva`; 0 = none), one `.idata`
    /// section holding `section`.
    fn image(section: &[u8], import_rva: u32, delay_rva: u32, characteristics: u16) -> Vec<u8> {
        let mut image = vec![0u8; RAW_POINTER];
        image[0..2].copy_from_slice(b"MZ");
        put32(&mut image, 0x3C, 0x40);
        image[0x40..0x44].copy_from_slice(b"PE\0\0");
        put16(&mut image, COFF, 0x8664); // machine: x86-64
        put16(&mut image, COFF + 2, 1); // one section
        put16(&mut image, COFF + 16, 240); // optional header size
        put16(&mut image, COFF + 18, 0x0022);
        put16(&mut image, OPTIONAL, PE32_PLUS_MAGIC);
        put16(&mut image, OPTIONAL + OPT_DLL_CHARACTERISTICS, characteristics);
        put32(&mut image, OPTIONAL + OPT_NUMBER_OF_RVA_AND_SIZES, 16);
        put32(&mut image, OPTIONAL + OPT_IMPORT_DIRECTORY, import_rva);
        put32(&mut image, OPTIONAL + OPT_IMPORT_DIRECTORY + 4, section.len() as u32);
        put32(&mut image, OPTIONAL + OPT_DELAY_IMPORT_DIRECTORY, delay_rva);
        put32(&mut image, OPTIONAL + OPT_DELAY_IMPORT_DIRECTORY + 4, if delay_rva == 0 { 0 } else { 0x40 });
        let header = OPTIONAL + 240;
        image[header..header + 8].copy_from_slice(b".idata\0\0");
        put32(&mut image, header + 8, section.len() as u32); // virtual size
        put32(&mut image, header + 12, SECTION_RVA); // virtual address
        put32(&mut image, header + 16, section.len() as u32); // raw size
        put32(&mut image, header + 20, RAW_POINTER as u32); // raw pointer
        image.extend_from_slice(section);
        image
    }

    fn fake_pe_with(dlls: &[&str], delay_dlls: &[&str], characteristics: u16) -> Vec<u8> {
        let (section, import_rva, delay_rva) = section_with(dlls, delay_dlls, 1);
        image(&section, import_rva, delay_rva, characteristics)
    }

    /// An image with an import directory and no delay-load directory.
    fn fake_pe(dlls: &[&str], characteristics: u16) -> Vec<u8> {
        fake_pe_with(dlls, &[], characteristics)
    }

    fn sys() -> BTreeSet<String> {
        system_dll_set()
    }

    fn facts_with(dll: &str) -> PeFacts {
        facts_with2(dll, "kernel32.dll")
    }

    fn facts_with2(a: &str, b: &str) -> PeFacts {
        PeFacts {
            dlls: vec![a.to_string(), b.to_string()],
            delay_dlls: Vec::new(),
            dynamic_base: true,
            nx_compat: true,
            high_entropy_va: true,
            size: 1000,
        }
    }

    fn big(size: u64) -> PeFacts {
        PeFacts {
            dlls: vec!["KERNEL32.dll".to_string()],
            delay_dlls: Vec::new(),
            dynamic_base: true,
            nx_compat: true,
            high_entropy_va: true,
            size,
        }
    }

    #[test]
    fn parses_dll_names_and_flags() {
        let f = parse_pe(&fake_pe(&["KERNEL32.dll"], DYNAMIC_BASE | NX_COMPAT | HIGH_ENTROPY_VA)).unwrap();
        assert!(f.dlls.contains(&"KERNEL32.dll".into()));
        assert!(f.dynamic_base && f.nx_compat && f.high_entropy_va);
    }

    #[test]
    fn parses_its_own_test_binary() {
        let f = parse_pe(&std::fs::read(std::env::current_exe().unwrap()).unwrap()).unwrap();
        assert!(f.dlls.iter().any(|d| d.eq_ignore_ascii_case("kernel32.dll")) && f.dynamic_base && f.nx_compat);
    }

    #[test]
    fn rejects_non_pe_and_truncated_input() {
        assert_eq!(parse_pe(b"MZ"), Err(PeError::Truncated("dos header")));
        assert_eq!(parse_pe(&[0u8; 512]), Err(PeError::NotPe));
    }

    #[test]
    fn rejects_mingw_runtime_dlls() {
        for d in ["libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"] {
            assert!(check_pe(&facts_with(d), &sys(), LIMIT).contains(&PeFinding::NonSystemDll(d.into())));
        }
    }

    #[test]
    fn requires_all_three_characteristics() {
        assert!(
            check_pe(&parse_pe(&fake_pe(&["KERNEL32.dll"], DYNAMIC_BASE | NX_COMPAT)).unwrap(), &sys(), LIMIT)
                .contains(&PeFinding::MissingCharacteristic("HIGH_ENTROPY_VA"))
        );
    }

    #[test]
    fn dll_match_ignores_case_and_accepts_api_sets() {
        assert!(check_pe(&facts_with2("kernel32.DLL", "api-ms-win-core-synch-l1-2-0.dll"), &sys(), LIMIT).is_empty());
    }

    #[test]
    fn size_over_limit_flagged() {
        assert!(matches!(check_pe(&big(41 << 20), &sys(), 40 << 20)[..], [PeFinding::TooLarge(_)]));
    }

    // ---- SZ-1: the shipped size limit is 64 MiB ----
    //
    // `cargo xtask dist --stage 1` refused the 1.0 `skipper.exe` on 08.11. 12:50 (`pe-check: ... 41970688 bytes is over the size
    // limit`): the program had grown 27,648 bytes past the old 40 MiB (41,943,040 bytes). The tests above pass their own limit;
    // these use the constant that `run_cli` really hands to `check_pe_named`.

    /// The size of `skipper.exe` that the check refused on 08.11.
    const SKIPPER_EXE_SIZE: u64 = 41_970_688;

    #[test]
    fn the_shipped_size_limit_is_64_mib() {
        assert_eq!(SIZE_LIMIT, 64 << 20);
    }

    #[test]
    fn a_program_of_41970688_bytes_passes_the_shipped_limit() {
        assert_eq!(check_pe_named("skipper.exe", &big(SKIPPER_EXE_SIZE), &sys(), SIZE_LIMIT), Vec::<PeFinding>::new());
        // the same size as a whole image that `parse_pe` measures: valid headers, then zero padding up to that length
        let mut image = fake_pe(&["KERNEL32.dll"], ALL);
        image.resize(SKIPPER_EXE_SIZE as usize, 0);
        let facts = parse_pe(&image).unwrap();
        assert_eq!(facts.size, SKIPPER_EXE_SIZE);
        assert_eq!(check_pe_named("skipper.exe", &facts, &sys(), SIZE_LIMIT), Vec::<PeFinding>::new());
    }

    #[test]
    fn a_file_over_the_shipped_limit_is_refused_and_one_at_the_limit_is_not() {
        assert_eq!(check_pe_named("skipper.exe", &big(SIZE_LIMIT), &sys(), SIZE_LIMIT), Vec::<PeFinding>::new());
        assert_eq!(
            check_pe_named("skipper.exe", &big(SIZE_LIMIT + 1), &sys(), SIZE_LIMIT),
            vec![PeFinding::TooLarge(SIZE_LIMIT + 1)]
        );
        // a build with debug information or without optimisation is several times larger than the release program
        assert_eq!(
            check_pe_named("skipper.exe", &big(4 * SKIPPER_EXE_SIZE), &sys(), SIZE_LIMIT),
            vec![PeFinding::TooLarge(4 * SKIPPER_EXE_SIZE)]
        );
    }

    // ---- review R5, I6: delay-load imports ----

    #[test]
    fn delay_load_imports_are_read() {
        let f = parse_pe(&fake_pe_with(&["KERNEL32.dll"], &["libwinpthread-1.dll", "dwmapi.dll"], ALL)).unwrap();
        assert_eq!(f.dlls, vec!["KERNEL32.dll".to_string()]);
        assert_eq!(f.delay_dlls, vec!["libwinpthread-1.dll".to_string(), "dwmapi.dll".to_string()]);
    }

    #[test]
    fn an_image_without_a_delay_load_directory_has_no_delay_imports() {
        assert!(parse_pe(&fake_pe(&["KERNEL32.dll"], ALL)).unwrap().delay_dlls.is_empty());
    }

    #[test]
    fn a_non_system_delay_load_import_is_a_finding() {
        let f = parse_pe(&fake_pe_with(&["KERNEL32.dll"], &["libwinpthread-1.dll"], ALL)).unwrap();
        assert_eq!(check_pe(&f, &sys(), LIMIT), vec![PeFinding::NonSystemDelayDll("libwinpthread-1.dll".into())]);
    }

    #[test]
    fn system_delay_load_imports_pass_whatever_their_case() {
        let f = parse_pe(&fake_pe_with(&["KERNEL32.dll"], &["DWMAPI.dll", "api-ms-win-core-synch-l1-2-0.DLL"], ALL)).unwrap();
        assert!(check_pe(&f, &sys(), LIMIT).is_empty());
    }

    #[test]
    fn delay_descriptors_in_the_old_address_form_are_an_error() {
        // attributes 0: the fields are addresses, not RVAs; the reader does not guess
        let (section, import_rva, delay_rva) = section_with(&["KERNEL32.dll"], &["dwmapi.dll"], 0);
        let image = image(&section, import_rva, delay_rva, ALL);
        assert!(matches!(parse_pe(&image), Err(PeError::Malformed(_))));
    }

    // ---- review R5, M15: a damaged image is an error, never a panic ----

    fn damaged(change: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut image = fake_pe_with(&["KERNEL32.dll"], &["dwmapi.dll"], ALL);
        change(&mut image);
        image
    }

    #[test]
    fn the_image_these_tests_damage_is_valid() {
        let f = parse_pe(&damaged(|_| {})).unwrap();
        assert_eq!((f.dlls.len(), f.delay_dlls.len()), (1, 1));
    }

    #[test]
    fn a_pe_header_offset_beyond_the_file_is_an_error() {
        assert_eq!(parse_pe(&damaged(|i| put32(i, 0x3C, 0xFFFF_FF00))), Err(PeError::Truncated("pe header")));
        assert_eq!(parse_pe(&damaged(|i| put32(i, 0x3C, u32::MAX))), Err(PeError::Truncated("pe header")));
        // an offset into the DOS header itself finds no PE signature
        assert_eq!(parse_pe(&damaged(|i| put32(i, 0x3C, 0))), Err(PeError::NotPe));
    }

    #[test]
    fn a_section_count_of_65535_is_an_error() {
        assert!(matches!(parse_pe(&damaged(|i| put16(i, COFF + 2, 0xFFFF))), Err(PeError::Truncated(_))));
    }

    #[test]
    fn a_pe32_image_is_not_pe32_plus() {
        assert_eq!(parse_pe(&damaged(|i| put16(i, OPTIONAL, 0x10B))), Err(PeError::NotPe32Plus));
    }

    #[test]
    fn an_optional_header_that_ends_before_the_directory_count_is_an_error() {
        assert!(matches!(parse_pe(&damaged(|i| put16(i, COFF + 16, 100))), Err(PeError::Truncated(_))));
        assert!(matches!(parse_pe(&damaged(|i| put16(i, COFF + 16, 0))), Err(PeError::Truncated(_))));
    }

    #[test]
    fn data_directories_that_do_not_fit_in_the_optional_header_are_an_error() {
        // 16 directories are declared, but the header has room for one: the import list must not be
        // skipped silently
        assert!(matches!(parse_pe(&damaged(|i| put16(i, COFF + 16, 120))), Err(PeError::Truncated(_))));
        assert!(matches!(
            parse_pe(&damaged(|i| put32(i, OPTIONAL + OPT_NUMBER_OF_RVA_AND_SIZES, u32::MAX))),
            Err(PeError::Truncated(_))
        ));
    }

    #[test]
    fn a_directory_rva_outside_every_section_is_an_error() {
        let bad_import = damaged(|i| put32(i, OPTIONAL + OPT_IMPORT_DIRECTORY, 0x9000_0000));
        assert!(matches!(parse_pe(&bad_import), Err(PeError::Truncated(_))));
        let bad_delay = damaged(|i| put32(i, OPTIONAL + OPT_DELAY_IMPORT_DIRECTORY, 0x9000_0000));
        assert!(matches!(parse_pe(&bad_delay), Err(PeError::Truncated(_))));
    }

    #[test]
    fn a_name_rva_outside_every_section_is_an_error() {
        // the first import descriptor's name field
        let bad_import = damaged(|i| put32(i, RAW_POINTER + 12, 0x9000_0000));
        assert!(matches!(parse_pe(&bad_import), Err(PeError::Truncated(_))));
        // the first delay-load descriptor's name field (after two import descriptors)
        let at = RAW_POINTER + 2 * IMPORT_DESCRIPTOR_SIZE + 4;
        let bad_delay = damaged(|i| put32(i, at, 0x9000_0000));
        assert!(matches!(parse_pe(&bad_delay), Err(PeError::Truncated(_))));
    }

    #[test]
    fn an_unterminated_or_overlong_name_is_an_error() {
        // the NUL that closes the last name is cut off
        assert!(matches!(parse_pe(&damaged(|i| { i.pop(); })), Err(PeError::Truncated(_))));
        let long = "a".repeat(600);
        assert!(matches!(parse_pe(&fake_pe(&[long.as_str()], ALL)), Err(PeError::Truncated(_))));
        assert!(matches!(parse_pe(&fake_pe_with(&[], &[long.as_str()], ALL)), Err(PeError::Truncated(_))));
    }

    #[test]
    fn a_descriptor_list_without_an_end_is_an_error() {
        const COUNT: usize = 5000; // more than the reader accepts
        let name_rva = SECTION_RVA + (COUNT * IMPORT_DESCRIPTOR_SIZE) as u32;
        let mut section = vec![0u8; COUNT * IMPORT_DESCRIPTOR_SIZE];
        for n in 0..COUNT {
            put32(&mut section, n * IMPORT_DESCRIPTOR_SIZE + 12, name_rva);
        }
        section.extend_from_slice(b"kernel32.dll\0");
        assert!(matches!(parse_pe(&image(&section, SECTION_RVA, 0, ALL)), Err(PeError::Truncated(_))));

        let name_rva = SECTION_RVA + (COUNT * DELAY_DESCRIPTOR_SIZE) as u32;
        let mut section = vec![0u8; COUNT * DELAY_DESCRIPTOR_SIZE];
        for n in 0..COUNT {
            put32(&mut section, n * DELAY_DESCRIPTOR_SIZE, 1);
            put32(&mut section, n * DELAY_DESCRIPTOR_SIZE + 4, name_rva);
        }
        section.extend_from_slice(b"kernel32.dll\0");
        assert!(matches!(parse_pe(&image(&section, 0, SECTION_RVA, ALL)), Err(PeError::Truncated(_))));
    }

    #[test]
    fn every_cut_and_every_damaged_byte_is_handled_without_a_panic() {
        let valid = damaged(|_| {});
        for len in 0..valid.len() {
            let _ = parse_pe(&valid[..len]);
        }
        for at in 0..valid.len() {
            for value in [0x00u8, 0x7F, 0x80, 0xFF] {
                let mut image = valid.clone();
                image[at] = value;
                let _ = parse_pe(&image);
            }
        }
    }

    // ---- review R5, M14: the api-ms-win-* wildcard ----

    #[test]
    fn api_set_names_must_have_the_api_set_shape() {
        let s = sys();
        for good in ["api-ms-win-core-synch-l1-2-0.dll", "API-MS-WIN-CRT-RUNTIME-L1-1-0.DLL", "api-ms-win-core-path-l1-1-0"] {
            assert!(is_system(good, &s), "{good}");
        }
        for bad in [
            "api-ms-win-..\\x.dll",
            "api-ms-win-x/y-l1-1-0.dll",
            "api-ms-win-evil.dll",
            "api-ms-win-core-synch-l1-2.dll",
            "api-ms-win-core-synch-l1-2-0.dll.dll",
            "api-ms-win-core-synch-l1-2-0.exe",
            "api-ms-win-.dll",
            "api-ms-win--l1-1-0.dll",
            "api-ms-win-core-synch-lx-2-0.dll",
            "ext-ms-win-core-synch-l1-2-0.dll",
        ] {
            assert!(!is_system(bad, &s), "{bad}");
        }
    }

    // ---- PE-1: `combase` and `uiautomationcore` are system DLLs (probe PRB-63, 07.11.) ----
    //
    // The real release build of skipper.exe and skipper-gateway.exe imports `combase.dll` (COM: `CoTaskMemFree` through
    // windows-sys, `CoIncrementMTAUsage` and the WinRT helpers through windows-core), and skipper.exe imports
    // `uiautomationcore.dll` (the screen-reader bridge of eframe's `accesskit` feature). Both are System32 DLLs of every clean
    // Windows 10 and 11, so the list must accept them, as normal and as delay-load imports, and nothing else may change.

    #[test]
    fn combase_and_uiautomationcore_are_on_the_system_list() {
        let s = sys();
        assert!(s.contains("combase") && s.contains("uiautomationcore"));
        for name in [
            "combase.dll",
            "COMBASE.DLL",
            "ComBase.dll",
            "combase",
            "uiautomationcore.dll",
            "UIAutomationCore.dll",
            "UIAUTOMATIONCORE.DLL",
        ] {
            assert!(is_system(name, &s), "{name}");
        }
    }

    #[test]
    fn the_two_dlls_pass_as_normal_imports_whatever_their_case() {
        let f = parse_pe(&fake_pe(&["KERNEL32.dll", "ComBase.dll", "UIAutomationCore.dll"], ALL)).unwrap();
        assert_eq!(f.dlls.len(), 3);
        assert!(check_pe(&f, &sys(), LIMIT).is_empty());
    }

    #[test]
    fn the_two_dlls_pass_as_delay_load_imports_too() {
        let f = parse_pe(&fake_pe_with(&["KERNEL32.dll"], &["COMBASE.dll", "uiautomationcore.DLL"], ALL)).unwrap();
        assert_eq!(f.delay_dlls.len(), 2);
        assert!(check_pe(&f, &sys(), LIMIT).is_empty());
    }

    #[test]
    fn next_to_the_two_dlls_a_mingw_runtime_dll_is_still_refused() {
        for bad in ["libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"] {
            // as a normal import
            let f = parse_pe(&fake_pe(&["KERNEL32.dll", "combase.dll", bad, "uiautomationcore.dll"], ALL)).unwrap();
            assert_eq!(check_pe(&f, &sys(), LIMIT), vec![PeFinding::NonSystemDll(bad.into())], "{bad}");
            // as a delay-load import
            let f = parse_pe(&fake_pe_with(&["KERNEL32.dll", "combase.dll"], &["uiautomationcore.dll", bad], ALL)).unwrap();
            assert_eq!(check_pe(&f, &sys(), LIMIT), vec![PeFinding::NonSystemDelayDll(bad.into())], "{bad}");
        }
    }

    #[test]
    fn next_to_the_two_dlls_an_unknown_dll_is_still_refused() {
        let f = parse_pe(&fake_pe_with(&["KERNEL32.dll", "combase.dll", "evil.dll"], &["uiautomationcore.dll", "other.dll"], ALL)).unwrap();
        assert_eq!(
            check_pe(&f, &sys(), LIMIT),
            vec![PeFinding::NonSystemDll("evil.dll".into()), PeFinding::NonSystemDelayDll("other.dll".into())]
        );
    }

    #[test]
    fn a_name_that_only_looks_like_one_of_the_two_dlls_is_refused() {
        let s = sys();
        for bad in [
            "combase2.dll",
            "combase32.dll",
            "combase-1.dll",
            "xcombase.dll",
            "libcombase.dll",
            "combase.dll.dll",
            "combase.exe",
            "..\\combase.dll",
            "uiautomationcore2.dll",
            "uiautomationcorex.dll",
            "libuiautomationcore.dll",
            "uiautomationcore.dll.dll",
            "uiautomationcore.exe",
            "sub/uiautomationcore.dll",
        ] {
            assert!(!is_system(bad, &s), "{bad}");
        }
    }

    #[test]
    fn the_system_list_holds_no_lib_name() {
        // the rule in the head of system-dlls.txt: a mingw runtime DLL is never a system DLL
        assert!(sys().iter().all(|entry| !entry.starts_with("lib")));
    }

    #[test]
    fn combase_does_not_open_the_network_rule() {
        // skipper.exe may still not import a network DLL, and the two helpers may import WinHTTP next to combase
        assert_eq!(
            check_pe_named("skipper.exe", &facts_with2("combase.dll", "winhttp.dll"), &sys(), LIMIT),
            vec![PeFinding::NetworkDll("winhttp".into())]
        );
        for helper in ["skipper-gateway.exe", "skipper-update.exe"] {
            assert!(check_pe_named(helper, &facts_with2("combase.dll", "winhttp.dll"), &sys(), LIMIT).is_empty(), "{helper}");
        }
    }

    // The import lists of the six programs as `pe-check` printed them for the real release build (probe PRB-63, 07.11.). Before
    // PE-1 the lists of skipper.exe and skipper-gateway.exe each had findings.
    const SKIPPER_IMPORTS: &[&str] = &[
        "KERNEL32.dll",
        "msvcrt.dll",
        "ntdll.dll",
        "USER32.dll",
        "imm32.dll",
        "uxtheme.dll",
        "kernel32.dll",
        "setupapi.dll",
        "gdi32.dll",
        "user32.dll",
        "ntdll.dll",
        "oleaut32.dll",
        "advapi32.dll",
        "dxgi.dll",
        "opengl32.dll",
        "ole32.dll",
        "combase.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
        "uiautomationcore.dll",
        "dwmapi.dll",
        "iphlpapi.dll",
        "ws2_32.dll",
        "bcrypt.dll",
        "crypt32.dll",
        "bcryptprimitives.dll",
        "userenv.dll",
        "psapi.dll",
        "wtsapi32.dll",
        "shell32.dll",
    ];
    const GATEWAY_IMPORTS: &[&str] = &[
        "KERNEL32.dll",
        "msvcrt.dll",
        "kernel32.dll",
        "kernel32.dll",
        "bcryptprimitives.dll",
        "ntdll.dll",
        "user32.dll",
        "advapi32.dll",
        "winhttp.dll",
        "ws2_32.dll",
        "credui.dll",
        "bcrypt.dll",
        "crypt32.dll",
        "combase.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
    ];
    const HOOK_IMPORTS: &[&str] = &["api-ms-win-core-synch-l1-2-0.dll", "KERNEL32.dll", "msvcrt.dll", "ntdll.dll"];
    const IMPORT_IMPORTS: &[&str] = &[
        "KERNEL32.dll",
        "msvcrt.dll",
        "kernel32.dll",
        "kernel32.dll",
        "ntdll.dll",
        "bcryptprimitives.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
    ];
    const MCP_IMPORTS: &[&str] = &[
        "KERNEL32.dll",
        "msvcrt.dll",
        "kernel32.dll",
        "kernel32.dll",
        "bcryptprimitives.dll",
        "ntdll.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
    ];
    const UPDATE_IMPORTS: &[&str] = &[
        "KERNEL32.dll",
        "msvcrt.dll",
        "kernel32.dll",
        "kernel32.dll",
        "ntdll.dll",
        "winhttp.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
    ];

    #[test]
    fn the_import_lists_of_the_real_release_build_pass() {
        let programs: [(&str, &[&str]); 6] = [
            ("skipper.exe", SKIPPER_IMPORTS),
            ("skipper-gateway.exe", GATEWAY_IMPORTS),
            ("skipper-hook.exe", HOOK_IMPORTS),
            ("skipper-import.exe", IMPORT_IMPORTS),
            ("skipper-mcp.exe", MCP_IMPORTS),
            ("skipper-update.exe", UPDATE_IMPORTS),
        ];
        let system = sys();
        for (file, imports) in programs {
            let f = parse_pe(&fake_pe(imports, ALL)).unwrap();
            assert_eq!(check_pe_named(file, &f, &system, LIMIT), Vec::<PeFinding>::new(), "{file}");
        }
    }
}

/// The network DLL rule of HUB-13 (MC-337 item 1): built on `PeFacts` directly, so no image is needed.
#[cfg(test)]
mod network_tests {
    use super::*;

    fn facts(dlls: &[&str], delay_dlls: &[&str]) -> PeFacts {
        PeFacts {
            dlls: dlls.iter().map(|d| (*d).to_string()).collect(),
            delay_dlls: delay_dlls.iter().map(|d| (*d).to_string()).collect(),
            dynamic_base: true,
            nx_compat: true,
            high_entropy_va: true,
            size: 1000,
        }
    }

    const LIMIT: u64 = 40 << 20;

    #[test]
    fn skipper_exe_with_winhttp_has_one_network_finding_and_no_second_one() {
        let found = check_pe_named("skipper.exe", &facts(&["KERNEL32.dll", "WINHTTP.dll"], &[]), &system_dll_set(), LIMIT);
        assert_eq!(found, vec![PeFinding::NetworkDll("winhttp".to_string())]);
    }

    #[test]
    fn a_delay_load_network_import_counts_too() {
        let found = check_pe_named("skipper.exe", &facts(&["KERNEL32.dll"], &["wininet.dll"]), &system_dll_set(), LIMIT);
        assert_eq!(found, vec![PeFinding::NetworkDll("wininet".to_string())]);
    }

    #[test]
    fn the_same_dll_in_both_directories_is_reported_once() {
        let found = check_pe_named("skipper-hook.exe", &facts(&["winhttp.dll"], &["WinHttp.DLL"]), &system_dll_set(), LIMIT);
        assert_eq!(found, vec![PeFinding::NetworkDll("winhttp".to_string())]);
    }

    #[test]
    fn the_two_helpers_may_import_winhttp() {
        for name in ["skipper-gateway.exe", "skipper-update.exe", "Skipper-Gateway.EXE"] {
            let found = check_pe_named(name, &facts(&["kernel32.dll", "winhttp.dll", "crypt32.dll"], &[]), &system_dll_set(), LIMIT);
            assert_eq!(found, Vec::<PeFinding>::new(), "{name}");
        }
    }

    #[test]
    fn a_helper_still_fails_on_a_dll_that_is_no_system_dll() {
        let found = check_pe_named("skipper-gateway.exe", &facts(&["libcurl-4.dll"], &[]), &system_dll_set(), LIMIT);
        assert_eq!(found, vec![PeFinding::NonSystemDll("libcurl-4.dll".to_string())]);
    }

    #[test]
    fn a_file_without_a_network_dll_is_judged_as_before() {
        let clean = facts(&["kernel32.dll", "ws2_32.dll", "bcryptprimitives.dll"], &[]);
        assert_eq!(check_pe_named("skipper.exe", &clean, &system_dll_set(), LIMIT), check_pe(&clean, &system_dll_set(), LIMIT));
        assert!(check_pe_named("skipper.exe", &clean, &system_dll_set(), LIMIT).is_empty());
        let other = facts(&["kernel32.dll", "evil.dll"], &[]);
        assert_eq!(
            check_pe_named("skipper.exe", &other, &system_dll_set(), LIMIT),
            vec![PeFinding::NonSystemDll("evil.dll".to_string())]
        );
    }

    #[test]
    fn every_network_dll_of_the_list_is_found_in_skipper_exe() {
        for dll in NETWORK_DLLS {
            let file = format!("{dll}.dll");
            let found = check_pe_named("skipper.exe", &facts(&[file.as_str()], &[]), &system_dll_set(), LIMIT);
            assert_eq!(found, vec![PeFinding::NetworkDll(dll.to_string())], "{dll}");
        }
    }

    #[test]
    fn the_other_findings_stay_with_a_network_finding() {
        let mut bad = facts(&["winhttp.dll"], &[]);
        bad.nx_compat = false;
        let found = check_pe_named("skipper.exe", &bad, &system_dll_set(), LIMIT);
        assert!(found.contains(&PeFinding::NetworkDll("winhttp".to_string())));
        assert!(found.contains(&PeFinding::MissingCharacteristic("NX_COMPAT")));
    }

    #[test]
    fn the_finding_names_the_dll_and_the_two_helpers() {
        let text = describe(&PeFinding::NetworkDll("winhttp".to_string()));
        assert!(text.contains("winhttp") && text.contains("skipper-gateway.exe") && text.contains("skipper-update.exe"));
    }

    #[test]
    fn the_list_of_network_dlls_holds_no_system_dll_the_program_needs() {
        let system = system_dll_set();
        // ws2_32 stays allowed (see NETWORK_DLLS); the listed DLLs are not on the system list at all.
        assert!(system.contains("ws2_32"));
        for dll in NETWORK_DLLS {
            assert!(!system.contains(dll), "{dll}");
        }
    }
}
