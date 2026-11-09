// SPDX-License-Identifier: MIT
//! `cargo xtask pin-check` (plan Task 2 Steps 11 and 13; G6, G7, G8; MC-118, MC-120).
//!
//! Reads `supply-chain/pins.toml`, `rust-toolchain.toml`, `Cargo.lock` and `rustc -vV` (run through `%RUSTC%`
//! inside the build box) and enforces: the 7-day rule for the toolchain, the tools and every direct crate; the
//! SHA-256 pins; the lock file's registry packages (their hash, the age of their last change and the pinned
//! versions); no git source in the lock file; and a complete `[conpty]` table when any of its values is set. The
//! lock is judged by its registry packages only (Stage 1 Task 2; MC-118, S1G-3): a path package or a path edge
//! brings no third-party code, so a new workspace crate does not restart the wait. The date is the system
//! clock's UTC date, computed with the standard library only (days from civil).

use std::fmt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// How many days a release must have existed before it may be used (G7).
const WAIT_DAYS: u32 = 7;

/// The one registry a build takes third-party code from (G6), as cargo writes it into `Cargo.lock`.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// A calendar date in the proleptic Gregorian calendar.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Date {
    pub y: u16,
    pub m: u8,
    pub d: u8,
}

impl Date {
    /// `Date::parse("2026-10-21")`; `None` for any other shape or an impossible day.
    pub fn parse(s: &str) -> Option<Date> {
        let s = s.trim();
        if s.len() != 10 || !s.is_ascii() {
            return None;
        }
        let b = s.as_bytes();
        if b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        if !b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
            return None;
        }
        let y: u16 = s[0..4].parse().ok()?;
        let m: u8 = s[5..7].parse().ok()?;
        let d: u8 = s[8..10].parse().ok()?;
        if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
            return None;
        }
        Some(Date { y, m, d })
    }

    /// The date `days` days later.
    pub fn plus_days(self, days: u32) -> Date {
        let serial = days_from_civil(i64::from(self.y), i64::from(self.m), i64::from(self.d)) + i64::from(days);
        let (y, m, d) = civil_from_days(serial);
        Date { y: y as u16, m: m as u8, d: d as u8 }
    }

    /// The UTC date of a Unix time in seconds.
    pub fn from_epoch_secs(secs: u64) -> Date {
        let (y, m, d) = civil_from_days((secs / 86_400) as i64);
        Date { y: y as u16, m: m as u8, d: d as u8 }
    }

    /// The system clock's UTC date.
    pub fn today() -> Date {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Date::from_epoch_secs(secs)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

fn is_leap(y: u16) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A direct crate dependency: the exact version and the day it was released.
#[derive(Clone, PartialEq, Debug)]
pub struct CratePin {
    pub name: String,
    pub version: String,
    pub released: Date,
}

impl CratePin {
    pub fn new(name: &str, version: &str, released: Date) -> CratePin {
        CratePin { name: name.to_string(), version: version.to_string(), released }
    }
}

/// The pinned Rust toolchain.
#[derive(Clone, PartialEq, Debug)]
pub struct ToolchainPin {
    pub channel: String,
    pub host: String,
    pub released: Date,
}

impl ToolchainPin {
    pub fn new(channel: &str, host: &str, released: Date) -> ToolchainPin {
        ToolchainPin { channel: channel.to_string(), host: host.to_string(), released }
    }
}

/// A downloaded tool, pinned by the SHA-256 of its archive and, when set, of its extracted executable.
#[derive(Clone, PartialEq, Debug)]
pub struct ToolPin {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub exe_sha256: Option<String>,
    pub released: Date,
}

impl ToolPin {
    pub fn new(name: &str, version: &str, sha256: &str, released: Date) -> ToolPin {
        ToolPin {
            name: name.to_string(),
            version: version.to_string(),
            sha256: sha256.to_string(),
            exe_sha256: None,
            released,
        }
    }
}

/// A package of `Cargo.lock` that comes from a registry: the third-party code of the build, named by its checksum.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct RegistryPkg {
    pub name: String,
    pub version: String,
    pub checksum: String,
}

/// One package whose 7-day wait the owner waived (`[[waived]]` in `pins.toml`, NB-13). It counts only while
/// `Cargo.lock` holds this very name, version and checksum (see `Pins::waived_entry`).
#[derive(Clone, PartialEq, Debug)]
pub struct WaivedPkg {
    pub name: String,
    pub version: String,
    pub checksum: String,
    pub reason: String,
    pub date: Date,
}

/// Everything `supply-chain/pins.toml` pins, except the `[conpty]` table (see `check_conpty`).
#[derive(Clone, PartialEq, Debug)]
pub struct Pins {
    pub toolchain: ToolchainPin,
    pub tools: Vec<ToolPin>,
    pub crates: Vec<CratePin>,
    /// The day the registry packages of `Cargo.lock` last changed (`[lock] resolved`).
    pub lock_resolved: Option<Date>,
    /// The SHA-256 of the registry packages of `Cargo.lock` (`[lock] registry_sha256`, see `registry_sha256`).
    pub registry_sha256: Option<String>,
    /// The owner's waiver of the 7-day wait for this registry set (`[lock] seven_day_waiver`, NB-13): the age
    /// checks are skipped and `pin-check` prints the waiver; every other check stays. It counts only together
    /// with `seven_day_waiver_registry_sha256` (see `Pins::waiver_for`).
    pub seven_day_waiver: Option<String>,
    /// The `registry_sha256` the waiver was given for (`[lock] seven_day_waiver_registry_sha256`).
    /// A waiver without it, or with the hash of another registry set, counts for nothing.
    pub seven_day_waiver_registry_sha256: Option<String>,
    /// The per-package waivers (`[[waived]]`): they outlive a change of the registry set, as long as their
    /// checksum is still in the lock.
    pub waived: Vec<WaivedPkg>,
}

/// Why a pin check failed.
#[derive(Clone, PartialEq, Debug)]
pub enum PinError {
    /// Released less than 7 days before use; `earliest` is the first day it may be used.
    TooNew { name: String, earliest: Date },
    /// No valid SHA-256 (64 hex digits) is pinned for this tool.
    NoChecksum { tool: String },
    /// `rust-toolchain.toml` or `rustc -vV` does not show the pinned toolchain.
    ToolchainMismatch { what: String },
    /// The registry packages of `Cargo.lock` were last changed less than 7 days ago; `earliest` is the first day a
    /// build may use them.
    LockTooYoung { earliest: Date },
    /// The registry packages of `Cargo.lock` differ from the recorded `registry_sha256`.
    RegistryChanged { recorded: String, found: String },
    /// A direct pin is absent from `Cargo.lock` or locked at another version.
    LockMismatch { name: String, pinned: String, locked: String },
    /// `Cargo.lock` has a git source (crates.io only, G6).
    GitSource { name: String },
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PinError::TooNew { name, earliest } => {
                write!(f, "{name} is too new: the 7-day rule allows it from {earliest}")
            }
            PinError::NoChecksum { tool } => {
                write!(f, "{tool}: no valid SHA-256 is pinned (64 hex digits are needed)")
            }
            PinError::ToolchainMismatch { what } => write!(f, "toolchain mismatch: {what}"),
            PinError::LockTooYoung { earliest } => {
                write!(
                    f,
                    "the registry packages of Cargo.lock changed less than 7 days ago: the first build is allowed from {earliest}"
                )
            }
            PinError::RegistryChanged { recorded, found } => write!(
                f,
                "the registry packages of Cargo.lock differ from supply-chain/pins.toml (recorded {recorded}, found {found}): record the new set as [lock] registry_sha256 with its resolved date"
            ),
            PinError::LockMismatch { name, pinned, locked } => {
                write!(f, "{name}: pinned {pinned}, but Cargo.lock holds {locked}")
            }
            PinError::GitSource { name } => write!(f, "{name}: Cargo.lock has a git source (crates.io only)"),
        }
    }
}

fn str_field<'a>(t: &'a toml::Table, key: &str) -> Option<&'a str> {
    t.get(key).and_then(|v| v.as_str())
}

/// A date written as a TOML date (`released = 2026-10-15`) or as a string.
fn date_field(t: &toml::Table, key: &str) -> Option<Date> {
    match t.get(key)? {
        toml::Value::Datetime(dt) => {
            let text = dt.to_string();
            Date::parse(text.get(..10)?)
        }
        toml::Value::String(s) => Date::parse(s),
        _ => None,
    }
}

impl Pins {
    /// Parses `supply-chain/pins.toml`. A crate with an empty `version` or without `released` is "not pinned
    /// yet" and is reported in the error text, so `pin-check` fails until every pin is set (G8).
    pub fn from_toml(text: &str) -> Result<Pins, String> {
        let root: toml::Table = toml::from_str(text).map_err(|_| "pins.toml is not valid TOML".to_string())?;
        let mut problems: Vec<String> = Vec::new();

        let toolchain = match root.get("toolchain").and_then(|v| v.as_table()) {
            Some(t) => {
                let channel = str_field(t, "channel").unwrap_or("");
                let host = str_field(t, "host").unwrap_or("");
                if channel.is_empty() || host.is_empty() {
                    problems.push("[toolchain] needs channel and host".to_string());
                }
                match date_field(t, "released") {
                    Some(released) => Some(ToolchainPin::new(channel, host, released)),
                    None => {
                        problems.push("[toolchain] released is missing or not a date".to_string());
                        None
                    }
                }
            }
            None => {
                problems.push("[toolchain] is missing".to_string());
                None
            }
        };

        let mut tools = Vec::new();
        if let Some(tt) = root.get("tool").and_then(|v| v.as_table()) {
            for (name, v) in tt {
                let Some(t) = v.as_table() else {
                    problems.push(format!("[tool.{name}] is not a table"));
                    continue;
                };
                let version = str_field(t, "version").unwrap_or("");
                let sha256 = str_field(t, "sha256").unwrap_or("");
                let exe_sha256 = str_field(t, "exe_sha256").unwrap_or("");
                match date_field(t, "released") {
                    Some(released) => {
                        let mut pin = ToolPin::new(name, version, sha256, released);
                        if !exe_sha256.is_empty() {
                            pin.exe_sha256 = Some(exe_sha256.to_string());
                        }
                        tools.push(pin);
                    }
                    None => problems.push(format!("[tool.{name}] released is missing or not a date")),
                }
            }
        }

        let mut crates = Vec::new();
        let mut unpinned: Vec<String> = Vec::new();
        if let Some(ct) = root.get("crate").and_then(|v| v.as_table()) {
            for (name, v) in ct {
                let table = v.as_table();
                let version = table.and_then(|t| str_field(t, "version")).unwrap_or("");
                let released = table.and_then(|t| date_field(t, "released"));
                match (version.is_empty(), released) {
                    (true, _) => unpinned.push(format!("{name} (version not set)")),
                    (false, None) => unpinned.push(format!("{name} (release date not set)")),
                    (false, Some(released)) => crates.push(CratePin::new(name, version, released)),
                }
            }
        }
        if !unpinned.is_empty() {
            problems.push(format!("not pinned yet: {}", unpinned.join(", ")));
        }

        let lock = root.get("lock").and_then(|v| v.as_table());
        let lock_resolved = lock.and_then(|t| date_field(t, "resolved"));
        if lock.is_some_and(|t| t.contains_key("resolved")) && lock_resolved.is_none() {
            problems.push("[lock] resolved is not a date".to_string());
        }
        let registry_sha256 = lock
            .and_then(|t| str_field(t, "registry_sha256"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let seven_day_waiver = lock
            .and_then(|t| str_field(t, "seven_day_waiver"))
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string);
        let seven_day_waiver_registry_sha256 = lock
            .and_then(|t| str_field(t, "seven_day_waiver_registry_sha256"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut waived = Vec::new();
        match root.get("waived") {
            None => {}
            Some(toml::Value::Array(entries)) => {
                for (i, entry) in entries.iter().enumerate() {
                    match entry.as_table().map(parse_waived) {
                        Some(Ok(w)) => waived.push(w),
                        Some(Err(why)) => problems.push(format!("[[waived]] {why}")),
                        None => problems.push(format!("[[waived]] entry {} is not a table", i + 1)),
                    }
                }
            }
            Some(_) => problems.push("waived must be written as [[waived]] entries".to_string()),
        }

        match (toolchain, problems.is_empty()) {
            (Some(toolchain), true) => Ok(Pins {
                toolchain,
                tools,
                crates,
                lock_resolved,
                registry_sha256,
                seven_day_waiver,
                seven_day_waiver_registry_sha256,
                waived,
            }),
            _ => Err(problems.join("; ")),
        }
    }

    /// The owner's waiver text, but only when it names the registry set that is being checked:
    /// `seven_day_waiver_registry_sha256` holds the `registry_sha256` the waiver was given for. A waiver
    /// without that hash, or with the hash of another registry set, counts for nothing, so the 7-day rule comes
    /// back with every later change of the registry packages, as NB-13 says. A change of path packages leaves
    /// the hash, and so the waiver, as it is.
    pub fn waiver_for(&self, lock_bytes: &[u8]) -> Option<&str> {
        let waiver = self.seven_day_waiver.as_deref()?;
        let named = self.seven_day_waiver_registry_sha256.as_deref()?;
        let found = registry_sha256(std::str::from_utf8(lock_bytes).ok()?).ok()?;
        found.eq_ignore_ascii_case(named.trim()).then_some(waiver)
    }

    /// The `[[waived]]` entry that covers this package: same name, version and checksum.
    pub fn waived_entry(&self, pkg: &RegistryPkg) -> Option<&WaivedPkg> {
        self.waived.iter().find(|w| {
            w.name == pkg.name && w.version == pkg.version && w.checksum.eq_ignore_ascii_case(&pkg.checksum)
        })
    }

    /// The first day a pinned package may be used: its release plus the wait. `None` for a package that is no
    /// direct pin, whose release date is not known.
    fn earliest_use(&self, pkg: &RegistryPkg) -> Option<Date> {
        let pin = self.crates.iter().find(|c| c.name == pkg.name && c.version == pkg.version)?;
        Some(pin.released.plus_days(WAIT_DAYS))
    }
}

/// One `[[waived]]` table: name, version, a 64-hex checksum, a reason and a date; the error names what is wrong.
fn parse_waived(t: &toml::Table) -> Result<WaivedPkg, String> {
    let name = str_field(t, "name").unwrap_or("");
    let version = str_field(t, "version").unwrap_or("");
    let checksum = str_field(t, "checksum").unwrap_or("");
    let reason = str_field(t, "reason").unwrap_or("");
    let label = if name.is_empty() { "entry without a name".to_string() } else { name.to_string() };
    if name.is_empty() || version.is_empty() {
        return Err(format!("{label}: name and version are needed"));
    }
    if !is_hex64(checksum) {
        return Err(format!("{label}: checksum must be 64 hex digits"));
    }
    if reason.trim().is_empty() {
        return Err(format!("{label}: reason is missing"));
    }
    let Some(date) = date_field(t, "date") else {
        return Err(format!("{label}: date is missing or not a date"));
    };
    Ok(WaivedPkg {
        name: name.to_string(),
        version: version.to_string(),
        checksum: checksum.to_string(),
        reason: reason.to_string(),
        date,
    })
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A crate may be used 7 days after its release.
pub fn check_crate(p: &CratePin, today: Date) -> Result<(), PinError> {
    let earliest = p.released.plus_days(WAIT_DAYS);
    if earliest > today {
        return Err(PinError::TooNew { name: p.name.clone(), earliest });
    }
    Ok(())
}

/// A tool needs a 64-hex SHA-256 (and one for its executable when set) and the 7-day wait.
pub fn check_tool(p: &ToolPin, today: Date) -> Result<(), PinError> {
    if !is_hex64(&p.sha256) {
        return Err(PinError::NoChecksum { tool: p.name.clone() });
    }
    if let Some(exe) = &p.exe_sha256 {
        if !exe.is_empty() && !is_hex64(exe) {
            return Err(PinError::NoChecksum { tool: format!("{} (executable)", p.name) });
        }
    }
    let earliest = p.released.plus_days(WAIT_DAYS);
    if earliest > today {
        return Err(PinError::TooNew { name: p.name.clone(), earliest });
    }
    Ok(())
}

/// The value of a `key: value` line of `rustc -vV`.
fn vv_value<'a>(rustc_vv: &'a str, key: &str) -> Option<&'a str> {
    rustc_vv
        .lines()
        .find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix(": ")))
        .map(str::trim)
}

fn mismatch(what: String) -> PinError {
    PinError::ToolchainMismatch { what }
}

/// `rust-toolchain.toml` and `rustc -vV` must show the pinned channel and host, and the toolchain must be 7 days old.
pub fn check_toolchain(
    p: &ToolchainPin,
    toolchain_toml: &str,
    rustc_vv: &str,
    today: Date,
) -> Result<(), PinError> {
    let file_channel = toml::from_str::<toml::Table>(toolchain_toml).ok().and_then(|t| {
        t.get("toolchain")?
            .as_table()?
            .get("channel")?
            .as_str()
            .map(str::to_string)
    });
    match file_channel.as_deref() {
        Some(c) if c == p.channel => {}
        Some(c) => {
            return Err(mismatch(format!("rust-toolchain.toml says channel {c}, the pin is {}", p.channel)));
        }
        None => return Err(mismatch("rust-toolchain.toml has no [toolchain] channel".to_string())),
    }
    match vv_value(rustc_vv, "release") {
        Some(r) if r == p.channel => {}
        Some(r) => return Err(mismatch(format!("rustc reports release {r}, the pin is {}", p.channel))),
        None => return Err(mismatch("rustc -vV shows no release line".to_string())),
    }
    match vv_value(rustc_vv, "host") {
        Some(h) if h == p.host => {}
        Some(h) => return Err(mismatch(format!("rustc reports host {h}, the pin is {}", p.host))),
        None => return Err(mismatch("rustc -vV shows no host line".to_string())),
    }
    let earliest = p.released.plus_days(WAIT_DAYS);
    if earliest > today {
        return Err(PinError::TooNew { name: format!("rust {}", p.channel), earliest });
    }
    Ok(())
}

struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn locked_packages(lock_text: &str) -> Option<Vec<LockedPackage>> {
    let doc: toml::Table = toml::from_str(lock_text).ok()?;
    let mut out = Vec::new();
    if let Some(packages) = doc.get("package").and_then(|v| v.as_array()) {
        for package in packages {
            out.push(LockedPackage {
                name: package.get("name")?.as_str()?.to_string(),
                version: package.get("version")?.as_str()?.to_string(),
                source: package.get("source").and_then(|s| s.as_str()).map(str::to_string),
                checksum: package.get("checksum").and_then(|s| s.as_str()).map(str::to_string),
            });
        }
    }
    Some(out)
}

/// The registry packages of a `Cargo.lock` (a `source` of exactly `CRATES_IO`), sorted by name and version: the
/// only third-party code a build can hold. Path packages (no `source`) are left out, and so are git packages,
/// which `check_lock` refuses on their own (G6). Any other source (another `registry+` index, a `sparse+` one),
/// or a registry package without a checksum, is an error naming the package, so nothing third-party stays
/// outside the set.
pub fn registry_set(lock_text: &str) -> Result<Vec<RegistryPkg>, String> {
    let packages = locked_packages(lock_text).ok_or_else(|| "Cargo.lock is not readable".to_string())?;
    let mut set = Vec::new();
    for package in packages {
        let Some(source) = package.source.as_deref() else {
            continue;
        };
        if source.starts_with("git") {
            continue;
        }
        if source != CRATES_IO {
            return Err(format!("{} {} from the source {source}", package.name, package.version));
        }
        let Some(checksum) = package.checksum.filter(|c| !c.is_empty()) else {
            return Err(format!("{} {} without a checksum", package.name, package.version));
        };
        set.push(RegistryPkg { name: package.name, version: package.version, checksum });
    }
    set.sort();
    Ok(set)
}

/// The SHA-256 over the lines `name<TAB>version<TAB>checksum<LF>` of `registry_set`: the same for any order of
/// the packages in the file and for any path package, different for a new, removed or re-checksummed one.
pub fn registry_sha256(lock_text: &str) -> Result<String, String> {
    let mut lines = String::new();
    for p in registry_set(lock_text)? {
        lines.push_str(&format!("{}\t{}\t{}\n", p.name, p.version, p.checksum));
    }
    Ok(sha256_hex(lines.as_bytes()))
}

/// The packages of the lock that are direct pins released less than 7 days before `on`, whether or not a
/// `[[waived]]` entry covers them. Only a direct pin has a known release date.
fn young_pinned(lock_text: &str, pins: &Pins, on: Date) -> Vec<RegistryPkg> {
    let Ok(set) = registry_set(lock_text) else {
        return Vec::new();
    };
    set.into_iter()
        .filter(|pkg| {
            pins.crates.iter().any(|c| c.name == pkg.name && c.version == pkg.version && check_crate(c, on).is_err())
        })
        .collect()
}

/// The packages of the lock that were released less than 7 days before `on` and that no `[[waived]]` entry
/// covers: an entry counts only while its checksum is the one in the lock. Empty for an unreadable lock
/// (`check_lock` refuses that on its own).
pub fn young_packages(lock_text: &str, pins: &Pins, on: Date) -> Vec<RegistryPkg> {
    let mut young = young_pinned(lock_text, pins, on);
    young.retain(|pkg| pins.waived_entry(pkg).is_none());
    young
}

/// One `TooNew` for each of `young_packages`, with the day the package may be used.
fn too_new_errors(lock_text: &str, pins: &Pins, on: Date) -> Vec<PinError> {
    young_packages(lock_text, pins, on)
        .into_iter()
        .filter_map(|pkg| pins.earliest_use(&pkg).map(|earliest| PinError::TooNew { name: pkg.name, earliest }))
        .collect()
}

/// The registry packages of the lock must match the recorded `registry_sha256`, have last changed 7 days ago,
/// hold every direct pin at its pinned version (the pin released 7 days before they were resolved, unless a
/// `[[waived]]` entry covers it) and the lock must contain no git source. Path packages and path edges do not
/// count. With the owner's waiver (NB-13), when it names this very registry set (`Pins::waiver_for`), the two
/// age checks are skipped; the others stay.
pub fn check_lock(pins: &Pins, lock_bytes: &[u8], today: Date) -> Result<(), PinError> {
    let waived = pins.waiver_for(lock_bytes).is_some();
    let Some(recorded) = pins.registry_sha256.as_deref() else {
        return Err(PinError::NoChecksum { tool: "Cargo.lock ([lock] registry_sha256)".to_string() });
    };
    let Some(resolved) = pins.lock_resolved else {
        return Err(PinError::NoChecksum { tool: "Cargo.lock ([lock] resolved)".to_string() });
    };
    let refused = |what: &str| PinError::LockMismatch {
        name: "Cargo.lock".to_string(),
        pinned: "a lock file of crates.io packages with checksums".to_string(),
        locked: what.to_string(),
    };
    let text = std::str::from_utf8(lock_bytes).map_err(|_| refused("an unreadable file"))?;
    let packages = locked_packages(text).ok_or_else(|| refused("an unreadable file"))?;
    let found = registry_sha256(text).map_err(|why| refused(&why))?;
    if !found.eq_ignore_ascii_case(recorded.trim()) {
        return Err(PinError::RegistryChanged { recorded: recorded.trim().to_string(), found });
    }
    let earliest = resolved.plus_days(WAIT_DAYS);
    if earliest > today && !waived {
        return Err(PinError::LockTooYoung { earliest });
    }
    for pin in &pins.crates {
        let versions: Vec<&str> =
            packages.iter().filter(|p| p.name == pin.name).map(|p| p.version.as_str()).collect();
        if !versions.contains(&pin.version.as_str()) {
            let locked = if versions.is_empty() { "absent".to_string() } else { versions.join(", ") };
            return Err(PinError::LockMismatch { name: pin.name.clone(), pinned: pin.version.clone(), locked });
        }
    }
    if !waived {
        if let Some(too_new) = too_new_errors(text, pins, resolved).into_iter().next() {
            return Err(too_new);
        }
    }
    for package in &packages {
        if package.source.as_deref().is_some_and(|s| s.starts_with("git")) {
            return Err(PinError::GitSource { name: package.name.clone() });
        }
    }
    Ok(())
}

/// The `[conpty]` table: when any value is set, both checksums must be 64 hex digits (empty strings count as
/// not set, as in `parse_bundle_pins` of Task 12).
pub fn check_conpty(pins_toml: &str) -> Result<(), PinError> {
    let Ok(root) = toml::from_str::<toml::Table>(pins_toml) else {
        return Err(PinError::NoChecksum { tool: "conpty (pins.toml unreadable)".to_string() });
    };
    let Some(table) = root.get("conpty").and_then(|v| v.as_table()) else {
        return Ok(());
    };
    let any_set = ["conpty_dll_sha256", "openconsole_exe_sha256", "version"]
        .iter()
        .any(|key| str_field(table, key).is_some_and(|v| !v.is_empty()));
    if !any_set {
        return Ok(());
    }
    for key in ["conpty_dll_sha256", "openconsole_exe_sha256"] {
        if !str_field(table, key).is_some_and(is_hex64) {
            return Err(PinError::NoChecksum { tool: format!("conpty {key}") });
        }
    }
    Ok(())
}

/// The line `pin-check` prints about the owner's waiver: its text when it names this registry set, a notice when
/// the waiver names another one (or none), nothing when there is no waiver.
fn waiver_note(pins: &Pins, lock_bytes: &[u8]) -> Option<String> {
    match (pins.seven_day_waiver.as_deref(), pins.waiver_for(lock_bytes)) {
        (_, Some(waiver)) => Some(format!("7-day wait waived for this lock: {waiver}")),
        (Some(_), None) => Some("waiver ignored: it names another set of registry packages".to_string()),
        (None, None) => None,
    }
}

/// The lines about the packages a `[[waived]]` entry carried through the age check of `check_lock`, which judges
/// every pin on the day the registry packages last changed (`resolved`): young on that day, and covered.
fn waived_package_notes(pins: &Pins, lock_text: &str) -> Vec<String> {
    let Some(resolved) = pins.lock_resolved else {
        return Vec::new();
    };
    young_pinned(lock_text, pins, resolved)
        .iter()
        .filter_map(|pkg| pins.waived_entry(pkg))
        .map(|w| format!("7-day wait waived for {} {}: {}", w.name, w.version, w.reason))
        .collect()
}

/// All checks of `pin-check`: one line per problem (empty when everything holds) and the lines about waivers,
/// if any.
fn run_checks() -> (Vec<String>, Vec<String>) {
    let pins_text = match std::fs::read_to_string("supply-chain/pins.toml") {
        Ok(text) => text,
        Err(e) => return (vec![format!("cannot read supply-chain/pins.toml: {e}")], Vec::new()),
    };
    let pins = match Pins::from_toml(&pins_text) {
        Ok(pins) => pins,
        Err(e) => return (vec![e], Vec::new()),
    };
    let lock_bytes = match std::fs::read("Cargo.lock") {
        Ok(bytes) => bytes,
        Err(e) => return (vec![format!("cannot read Cargo.lock: {e}")], Vec::new()),
    };
    let mut notes: Vec<String> = waiver_note(&pins, &lock_bytes).into_iter().collect();
    let waived = pins.waiver_for(&lock_bytes).is_some();
    let toolchain_toml = match std::fs::read_to_string("rust-toolchain.toml") {
        Ok(text) => text,
        Err(e) => return (vec![format!("cannot read rust-toolchain.toml: {e}")], notes),
    };
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let rustc_vv = match Command::new(&rustc).arg("-vV").output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => {
            return (vec![format!("rustc -vV failed with exit code {}", out.status.code().unwrap_or(-1))], notes);
        }
        Err(e) => return (vec![format!("cannot run rustc -vV: {e}")], notes),
    };
    let today = Date::today();

    let mut problems: Vec<String> = Vec::new();
    let mut note = |result: Result<(), PinError>| {
        if let Err(e) = result {
            problems.push(e.to_string());
        }
    };
    note(check_toolchain(&pins.toolchain, &toolchain_toml, &rustc_vv, today));
    for tool in &pins.tools {
        note(check_tool(tool, today));
    }
    if let (false, Ok(lock_text)) = (waived, std::str::from_utf8(&lock_bytes)) {
        for error in too_new_errors(lock_text, &pins, today) {
            note(Err(error));
        }
        notes.extend(waived_package_notes(&pins, lock_text));
    }
    note(check_lock(&pins, &lock_bytes, today));
    note(check_conpty(&pins_text));
    (problems, notes)
}

/// `pin-check`: exit 0 with `pin-check: ok`, exit 1 with one line per problem, exit 2 on usage errors.
pub fn run_cli(args: &[String]) -> u8 {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask pin-check");
        return 2;
    }
    let (problems, notes) = run_checks();
    for line in notes {
        println!("pin-check: {line}");
    }
    if problems.is_empty() {
        println!("pin-check: ok");
        0
    } else {
        for problem in problems {
            println!("pin-check: {problem}");
        }
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn egui_0_37_too_new_on_2026_10_21() {
        let p = CratePin::new("egui", "0.37.0", d("2026-10-15"));
        assert_eq!(
            check_crate(&p, d("2026-10-21")),
            Err(PinError::TooNew { name: "egui".into(), earliest: d("2026-10-22") })
        );
    }

    #[test]
    fn rmcp_3_6_0_allowed_on_2026_10_27() {
        assert_eq!(check_crate(&CratePin::new("rmcp", "3.6.0", d("2026-10-20")), d("2026-10-27")), Ok(()));
    }

    const TOML_FILE: &str = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"minimal\"\ntargets = [\"x86_64-pc-windows-gnu\"]\n";
    const VV_GNU: &str = "rustc 1.99.1\nhost: x86_64-pc-windows-gnu\nrelease: 1.99.1\n";

    #[test]
    fn toolchain_1_99_1_too_new_on_2026_10_21() {
        let t = ToolchainPin::new("1.99.1", "x86_64-pc-windows-gnu", d("2026-10-15"));
        assert_eq!(
            check_toolchain(&t, TOML_FILE, VV_GNU, d("2026-10-21")),
            Err(PinError::TooNew { name: "rust 1.99.1".into(), earliest: d("2026-10-22") })
        );
        assert_eq!(check_toolchain(&t, TOML_FILE, VV_GNU, d("2026-10-22")), Ok(()));
    }

    #[test]
    fn toolchain_must_match_file_and_rustc() {
        let t = ToolchainPin::new("1.99.1", "x86_64-pc-windows-gnu", d("2026-10-15"));
        assert!(matches!(
            check_toolchain(&t, TOML_FILE, &VV_GNU.replace("windows-gnu", "windows-msvc"), d("2026-11-03")),
            Err(PinError::ToolchainMismatch { .. })
        ));
        assert!(matches!(
            check_toolchain(&t, &TOML_FILE.replace("1.99.1", "1.99.0"), VV_GNU, d("2026-11-03")),
            Err(PinError::ToolchainMismatch { .. })
        ));
    }

    #[test]
    fn toolchain_release_line_must_match() {
        let t = ToolchainPin::new("1.99.1", "x86_64-pc-windows-gnu", d("2026-10-15"));
        let other = "rustc 1.99.0\nhost: x86_64-pc-windows-gnu\nrelease: 1.99.0\n";
        assert!(matches!(
            check_toolchain(&t, TOML_FILE, other, d("2026-11-03")),
            Err(PinError::ToolchainMismatch { .. })
        ));
        assert!(matches!(
            check_toolchain(&t, "not toml [", VV_GNU, d("2026-11-03")),
            Err(PinError::ToolchainMismatch { .. })
        ));
    }

    #[test]
    fn tool_without_sha256_rejected() {
        assert!(matches!(
            check_tool(&ToolPin::new("gitleaks", "8.30.1", "", d("2026-03-21")), d("2026-10-21")),
            Err(PinError::NoChecksum { .. })
        ));
    }

    #[test]
    fn tool_with_sha256_accepted() {
        assert_eq!(
            check_tool(&ToolPin::new("gitleaks", "8.30.1", &"a".repeat(64), d("2026-03-21")), d("2026-10-21")),
            Ok(())
        );
    }

    #[test]
    fn tool_sha256_must_be_hex_of_full_length() {
        let short = ToolPin::new("gitleaks", "8.30.1", &"a".repeat(63), d("2026-03-21"));
        assert!(matches!(check_tool(&short, d("2026-10-21")), Err(PinError::NoChecksum { .. })));
        let not_hex = ToolPin::new("gitleaks", "8.30.1", &"g".repeat(64), d("2026-03-21"));
        assert!(matches!(check_tool(&not_hex, d("2026-10-21")), Err(PinError::NoChecksum { .. })));
    }

    #[test]
    fn tool_exe_sha256_checked_when_set() {
        let mut p = ToolPin::new("gitleaks", "8.30.1", &"a".repeat(64), d("2026-03-21"));
        p.exe_sha256 = Some("xyz".to_string());
        assert!(matches!(check_tool(&p, d("2026-10-21")), Err(PinError::NoChecksum { .. })));
        p.exe_sha256 = Some("b".repeat(64));
        assert_eq!(check_tool(&p, d("2026-10-21")), Ok(()));
        p.exe_sha256 = None;
        assert_eq!(check_tool(&p, d("2026-10-21")), Ok(()));
    }

    #[test]
    fn tool_too_new_is_refused() {
        let p = ToolPin::new("gitleaks", "8.30.1", &"a".repeat(64), d("2026-10-20"));
        assert_eq!(
            check_tool(&p, d("2026-10-21")),
            Err(PinError::TooNew { name: "gitleaks".into(), earliest: d("2026-10-27") })
        );
    }

    const REGISTRY: &str = CRATES_IO;
    const LOCK: &[u8] = b"version = 4\n\n[[package]]\nname = \"egui\"\nversion = \"0.37.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aa\"\n\n[[package]]\nname = \"portable-pty\"\nversion = \"0.9.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"bb\"\n";

    /// The `registry_sha256` of a lock text, as `pins.toml` records it.
    fn registry_hash_of(lock: &[u8]) -> String {
        registry_sha256(std::str::from_utf8(lock).unwrap()).unwrap()
    }

    /// A lock text from (name, version, source) rows; a registry package gets a checksum made from its name.
    fn lock_text(packages: &[(&str, &str, &str)]) -> String {
        let mut text = String::from("version = 4\n");
        for (name, version, source) in packages {
            text.push_str(&format!("\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"));
            if !source.is_empty() {
                text.push_str(&format!("source = \"{source}\"\n"));
            }
            if source.starts_with("registry+") {
                text.push_str(&format!("checksum = \"sum-{name}-{version}\"\n"));
            }
        }
        text
    }

    fn pins_with_lock(resolved: Date, registry_sha256: String) -> Pins {
        Pins {
            toolchain: ToolchainPin::new("1.99.1", "x86_64-pc-windows-gnu", d("2026-10-15")),
            tools: vec![],
            crates: vec![
                CratePin::new("egui", "0.37.0", d("2026-10-15")),
                CratePin::new("portable-pty", "0.9.0", d("2025-02-11")),
            ],
            lock_resolved: Some(resolved),
            registry_sha256: Some(registry_sha256),
            seven_day_waiver: None,
            seven_day_waiver_registry_sha256: None,
            waived: vec![],
        }
    }

    #[test]
    fn young_lock_and_young_pins_pass_with_the_owners_waiver() {
        // NB-13: resolved and built on the pin day, 2026-10-22; egui was released only 7 days before.
        let mut pins = pins_with_lock(d("2026-10-22"), registry_hash_of(LOCK));
        pins.seven_day_waiver = Some("owner, 2026-10-22: Stage 0 starts without the 7-day wait (NB-13)".to_string());
        pins.seven_day_waiver_registry_sha256 = Some(registry_hash_of(LOCK));
        assert_eq!(check_lock(&pins, LOCK, d("2026-10-22")), Ok(()));
    }

    #[test]
    fn waiver_keeps_the_hash_and_version_checks() {
        let mut pins = pins_with_lock(d("2026-10-22"), registry_hash_of(LOCK));
        pins.seven_day_waiver = Some("owner".to_string());
        pins.seven_day_waiver_registry_sha256 = Some(registry_hash_of(LOCK));
        let other = lock_text(&[("egui", "0.37.0", REGISTRY)]);
        assert!(matches!(
            check_lock(&pins, other.as_bytes(), d("2026-10-22")),
            Err(PinError::RegistryChanged { .. })
        ));
        pins.crates.push(CratePin::new("uuid", "1.28.0", d("2026-10-10")));
        assert!(matches!(check_lock(&pins, LOCK, d("2026-10-22")), Err(PinError::LockMismatch { .. })));
    }

    #[test]
    fn waiver_is_read_from_the_lock_table() {
        let text = PINS_TOML.replace("[lock]\n", "[lock]\nseven_day_waiver = \"owner\"\n");
        assert_eq!(Pins::from_toml(&text).unwrap().seven_day_waiver.as_deref(), Some("owner"));
        assert_eq!(Pins::from_toml(PINS_TOML).unwrap().seven_day_waiver, None);
    }

    #[test]
    fn build_refused_within_7_days_of_lock_resolution() {
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(LOCK));
        assert_eq!(check_lock(&pins, LOCK, d("2026-11-02")), Err(PinError::LockTooYoung { earliest: d("2026-11-03") }));
        assert_eq!(check_lock(&pins, LOCK, d("2026-11-03")), Ok(()));
    }

    #[test]
    fn changed_registry_set_rejected() {
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(LOCK));
        let other = lock_text(&[("egui", "0.37.0", REGISTRY), ("portable-pty", "0.9.1", REGISTRY)]);
        let found = registry_sha256(&other).unwrap();
        assert_eq!(
            check_lock(&pins, other.as_bytes(), d("2026-11-10")),
            Err(PinError::RegistryChanged { recorded: registry_hash_of(LOCK), found })
        );
    }

    #[test]
    fn locked_version_must_equal_pin() {
        let lock = lock_text(&[("egui", "0.37.0", REGISTRY), ("portable-pty", "0.9.1", REGISTRY)]);
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(lock.as_bytes()));
        assert_eq!(
            check_lock(&pins, lock.as_bytes(), d("2026-11-10")),
            Err(PinError::LockMismatch {
                name: "portable-pty".into(),
                pinned: "0.9.0".into(),
                locked: "0.9.1".into()
            })
        );
    }

    #[test]
    fn a_second_locked_version_of_another_crate_version_is_fine() {
        let lock = lock_text(&[
            ("egui", "0.37.0", REGISTRY),
            ("portable-pty", "0.9.0", REGISTRY),
            ("portable-pty", "0.8.0", REGISTRY),
        ]);
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(lock.as_bytes()));
        assert_eq!(check_lock(&pins, lock.as_bytes(), d("2026-11-10")), Ok(()));
    }

    #[test]
    fn pinned_crate_absent_from_lock_is_a_mismatch() {
        let lock = lock_text(&[("egui", "0.37.0", REGISTRY)]);
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(lock.as_bytes()));
        assert_eq!(
            check_lock(&pins, lock.as_bytes(), d("2026-11-10")),
            Err(PinError::LockMismatch {
                name: "portable-pty".into(),
                pinned: "0.9.0".into(),
                locked: "absent".into()
            })
        );
    }

    #[test]
    fn git_source_in_lock_rejected() {
        let lock = lock_text(&[
            ("egui", "0.37.0", REGISTRY),
            ("portable-pty", "0.9.0", REGISTRY),
            ("sample-dep", "1.0.0", "git+https://example.org/sample-dep#0123456789abcdef"),
        ]);
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(lock.as_bytes()));
        assert_eq!(
            check_lock(&pins, lock.as_bytes(), d("2026-11-10")),
            Err(PinError::GitSource { name: "sample-dep".into() })
        );
    }

    #[test]
    fn pin_released_after_lock_resolution_minus_7_days_is_too_new() {
        let lock = lock_text(&[("egui", "0.37.0", REGISTRY), ("portable-pty", "0.9.0", REGISTRY)]);
        // egui was released 2026-10-15, so a lock resolved on 2026-10-20 is too early for it.
        let pins = pins_with_lock(d("2026-10-20"), registry_hash_of(lock.as_bytes()));
        assert_eq!(
            check_lock(&pins, lock.as_bytes(), d("2026-11-10")),
            Err(PinError::TooNew { name: "egui".into(), earliest: d("2026-10-22") })
        );
    }

    #[test]
    fn lock_without_recorded_hash_or_date_is_refused() {
        let mut pins = pins_with_lock(d("2026-10-27"), registry_hash_of(LOCK));
        pins.registry_sha256 = None;
        assert!(matches!(check_lock(&pins, LOCK, d("2026-11-10")), Err(PinError::NoChecksum { .. })));
        let mut pins = pins_with_lock(d("2026-10-27"), registry_hash_of(LOCK));
        pins.lock_resolved = None;
        assert!(matches!(check_lock(&pins, LOCK, d("2026-11-10")), Err(PinError::NoChecksum { .. })));
    }

    #[test]
    fn unreadable_lock_is_refused() {
        let junk = b"this is [ not toml";
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(LOCK));
        assert!(matches!(check_lock(&pins, junk, d("2026-11-10")), Err(PinError::LockMismatch { .. })));
    }

    #[test]
    fn dates_parse_and_print() {
        assert_eq!(Date::parse("2026-10-21"), Some(Date { y: 2026, m: 10, d: 21 }));
        assert_eq!(d("2026-10-21").to_string(), "2026-10-21");
        assert_eq!(Date::parse("2026-13-01"), None);
        assert_eq!(Date::parse("2026-02-30"), None);
        assert_eq!(Date::parse("2027-02-29"), None);
        assert_eq!(Date::parse("2028-02-29"), Some(Date { y: 2028, m: 2, d: 29 }));
        assert_eq!(Date::parse("26-10-21"), None);
        assert_eq!(Date::parse("2026/10/21"), None);
    }

    #[test]
    fn dates_order_by_year_month_day() {
        assert!(d("2026-10-21") < d("2026-10-22"));
        assert!(d("2026-10-31") < d("2026-11-01"));
        assert!(d("2026-12-31") < d("2027-01-01"));
    }

    #[test]
    fn plus_days_crosses_months_years_and_leap_days() {
        assert_eq!(d("2026-10-21").plus_days(7), d("2026-10-28"));
        assert_eq!(d("2026-10-27").plus_days(7), d("2026-11-03"));
        assert_eq!(d("2026-12-28").plus_days(7), d("2027-01-04"));
        assert_eq!(d("2028-02-28").plus_days(1), d("2028-02-29"));
        assert_eq!(d("2027-02-28").plus_days(1), d("2027-03-01"));
        assert_eq!(d("2026-10-21").plus_days(0), d("2026-10-21"));
    }

    #[test]
    fn date_from_epoch_seconds() {
        assert_eq!(Date::from_epoch_secs(0), d("1970-01-01"));
        assert_eq!(Date::from_epoch_secs(1_792_540_800), d("2026-10-21"));
        assert_eq!(Date::from_epoch_secs(1_792_540_800 + 86_399), d("2026-10-21"));
        assert_eq!(Date::from_epoch_secs(1_792_540_800 + 86_400), d("2026-10-22"));
    }

    const PINS_TOML: &str = r#"
[toolchain]
channel = "1.99.1"
host = "x86_64-pc-windows-gnu"
released = 2026-10-15

[tool.gitleaks]
version = "8.30.1"
asset = "gitleaks_8.30.1_windows_x64.zip"
size = 8438883
sha256 = "d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e"
exe_sha256 = ""
released = 2026-03-21

[crate.egui]
version = "0.37.0"
released = 2026-10-15

[crate.portable-pty]
version = "0.9.0"
released = 2025-02-11

[lock]
resolved = 2026-10-27
registry_sha256 = "0000000000000000000000000000000000000000000000000000000000000000"

[conpty]
conpty_dll_sha256 = ""
openconsole_exe_sha256 = ""
version = ""
"#;

    #[test]
    fn pins_file_parses_into_pins() {
        let pins = Pins::from_toml(PINS_TOML).unwrap();
        assert_eq!(pins.toolchain, ToolchainPin::new("1.99.1", "x86_64-pc-windows-gnu", d("2026-10-15")));
        assert_eq!(pins.tools.len(), 1);
        assert_eq!(pins.tools[0].name, "gitleaks");
        assert_eq!(pins.tools[0].exe_sha256, None);
        assert_eq!(pins.tools[0].released, d("2026-03-21"));
        assert_eq!(pins.crates.len(), 2);
        assert!(pins.crates.contains(&CratePin::new("egui", "0.37.0", d("2026-10-15"))));
        assert_eq!(pins.lock_resolved, Some(d("2026-10-27")));
        assert_eq!(pins.registry_sha256.as_deref(), Some("0".repeat(64).as_str()));
    }

    #[test]
    fn exe_sha256_set_becomes_some() {
        let text = PINS_TOML.replace("exe_sha256 = \"\"", &format!("exe_sha256 = \"{}\"", "c".repeat(64)));
        let pins = Pins::from_toml(&text).unwrap();
        assert_eq!(pins.tools[0].exe_sha256, Some("c".repeat(64)));
    }

    #[test]
    fn lock_table_may_be_empty() {
        let text = PINS_TOML.replace("resolved = 2026-10-27\n", "").replace("registry_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n", "");
        let pins = Pins::from_toml(&text).unwrap();
        assert_eq!(pins.lock_resolved, None);
        assert_eq!(pins.registry_sha256, None);
    }

    #[test]
    fn crate_without_version_or_release_date_is_reported() {
        let text = format!("{PINS_TOML}\n[crate.serde]\nversion = \"\"\n\n[crate.uuid]\nversion = \"1.28.0\"\n");
        let err = Pins::from_toml(&text).unwrap_err();
        assert!(err.contains("serde (version not set)"), "{err}");
        assert!(err.contains("uuid (release date not set)"), "{err}");
    }

    #[test]
    fn invalid_toml_is_reported() {
        assert!(Pins::from_toml("[toolchain\n").is_err());
    }

    #[test]
    fn missing_toolchain_table_is_reported() {
        assert!(Pins::from_toml("[lock]\n").unwrap_err().contains("[toolchain]"));
    }

    #[test]
    fn conpty_all_empty_is_fine() {
        assert_eq!(check_conpty(PINS_TOML), Ok(()));
        assert_eq!(check_conpty("[toolchain]\nchannel = \"1.99.1\"\n"), Ok(()));
    }

    #[test]
    fn conpty_with_one_value_needs_both_checksums() {
        let one = "[conpty]\nconpty_dll_sha256 = \"{h}\"\nopenconsole_exe_sha256 = \"\"\nversion = \"\"\n".replace("{h}", &"a".repeat(64));
        assert!(matches!(check_conpty(&one), Err(PinError::NoChecksum { .. })));
        let version_only = "[conpty]\nconpty_dll_sha256 = \"\"\nopenconsole_exe_sha256 = \"\"\nversion = \"1.0\"\n";
        assert!(matches!(check_conpty(version_only), Err(PinError::NoChecksum { .. })));
        let short = format!(
            "[conpty]\nconpty_dll_sha256 = \"{}\"\nopenconsole_exe_sha256 = \"{}\"\nversion = \"1.0\"\n",
            "a".repeat(64),
            "b".repeat(10)
        );
        assert!(matches!(check_conpty(&short), Err(PinError::NoChecksum { .. })));
    }

    #[test]
    fn conpty_with_both_checksums_passes() {
        let both = format!(
            "[conpty]\nconpty_dll_sha256 = \"{}\"\nopenconsole_exe_sha256 = \"{}\"\nversion = \"1.0\"\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert_eq!(check_conpty(&both), Ok(()));
    }

    #[test]
    fn rustc_vv_lines_are_read_by_key() {
        assert_eq!(vv_value(VV_GNU, "release"), Some("1.99.1"));
        assert_eq!(vv_value(VV_GNU, "host"), Some("x86_64-pc-windows-gnu"));
        assert_eq!(vv_value(VV_GNU, "commit-hash"), None);
    }

    #[test]
    fn sha256_hex_matches_a_known_digest() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn pin_errors_print_readable_lines() {
        let e = PinError::TooNew { name: "egui".into(), earliest: d("2026-10-22") };
        assert_eq!(e.to_string(), "egui is too new: the 7-day rule allows it from 2026-10-22");
        assert!(PinError::RegistryChanged { recorded: "a".into(), found: "b".into() }.to_string().contains("Cargo.lock"));
    }

    #[test]
    fn run_cli_rejects_arguments() {
        assert_eq!(run_cli(&["--bogus".to_string()]), 2);
    }

    // --- the waiver belongs to one lock file ---

    /// `PINS_TOML` with its `[lock]` table replaced: the resolved date, the registry hash and the extra lines given
    /// (the waiver lines).
    fn pins_with_lock_lines(resolved: &str, registry_sha256: &str, extra_lines: &str) -> Pins {
        let old = format!("resolved = 2026-10-27\nregistry_sha256 = \"{}\"\n", "0".repeat(64));
        assert!(PINS_TOML.contains(old.as_str()), "the test table changed");
        let new = format!("resolved = {resolved}\nregistry_sha256 = \"{registry_sha256}\"\n{extra_lines}");
        Pins::from_toml(&PINS_TOML.replace(old.as_str(), new.as_str())).unwrap()
    }

    fn waiver_lines(waiver_registry_sha256: &str) -> String {
        format!("seven_day_waiver = \"owner\"\nseven_day_waiver_registry_sha256 = \"{waiver_registry_sha256}\"\n")
    }

    #[test]
    fn waiver_for_another_lock_hash_leaves_the_age_checks_on() {
        let pins = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), &waiver_lines(&"b".repeat(64)));
        assert_eq!(
            check_lock(&pins, LOCK, d("2026-10-22")),
            Err(PinError::LockTooYoung { earliest: d("2026-10-29") })
        );
    }

    #[test]
    fn waiver_does_not_carry_over_to_a_new_lock() {
        // The lock is resolved again and `sha256` and `resolved` are updated; the old waiver line stays behind.
        let old_hash = registry_hash_of(LOCK);
        let new_lock = lock_text(&[
            ("egui", "0.37.0", REGISTRY),
            ("portable-pty", "0.9.0", REGISTRY),
            ("sample-dep", "1.0.0", REGISTRY),
        ]);
        let pins = pins_with_lock_lines("2026-10-22", &registry_hash_of(new_lock.as_bytes()), &waiver_lines(&old_hash));
        assert_eq!(
            check_lock(&pins, new_lock.as_bytes(), d("2026-10-22")),
            Err(PinError::LockTooYoung { earliest: d("2026-10-29") })
        );
    }

    #[test]
    fn waiver_without_a_lock_hash_is_ignored() {
        let pins = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), "seven_day_waiver = \"owner\"\n");
        assert_eq!(
            check_lock(&pins, LOCK, d("2026-10-22")),
            Err(PinError::LockTooYoung { earliest: d("2026-10-29") })
        );
    }

    #[test]
    fn per_pin_age_check_returns_when_the_waiver_names_another_lock() {
        // egui was released on 2026-10-15; a lock resolved on 2026-10-20 is too early for it.
        let pins = pins_with_lock_lines("2026-10-20", &registry_hash_of(LOCK), &waiver_lines(&"b".repeat(64)));
        assert_eq!(
            check_lock(&pins, LOCK, d("2026-11-10")),
            Err(PinError::TooNew { name: "egui".into(), earliest: d("2026-10-22") })
        );
    }

    #[test]
    fn waiver_that_names_this_lock_still_works() {
        let waiver = waiver_lines(&registry_hash_of(LOCK));
        let pins = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), &waiver);
        assert_eq!(check_lock(&pins, LOCK, d("2026-10-22")), Ok(()));
        let pins = pins_with_lock_lines("2026-10-20", &registry_hash_of(LOCK), &waiver);
        assert_eq!(check_lock(&pins, LOCK, d("2026-11-10")), Ok(()));
    }

    #[test]
    fn shipped_pins_file_ties_its_waiver_to_its_registry_set() {
        let root: toml::Table = toml::from_str(include_str!("../../supply-chain/pins.toml")).unwrap();
        let lock = root.get("lock").and_then(|v| v.as_table()).expect("pins.toml has a [lock] table");
        if lock.contains_key("seven_day_waiver") {
            assert_eq!(
                lock.get("seven_day_waiver_registry_sha256").and_then(|v| v.as_str()),
                lock.get("registry_sha256").and_then(|v| v.as_str()),
                "the waiver must name the registry hash it was given for"
            );
        }
    }

    #[test]
    fn waiver_hash_is_read_from_the_lock_table_and_compared_ignoring_case() {
        let waiver = waiver_lines(&registry_hash_of(LOCK).to_uppercase());
        let pins = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), &waiver);
        assert_eq!(pins.waiver_for(LOCK), Some("owner"));
        assert_eq!(pins.waiver_for(b"other"), None);
        // A hash without a waiver text is no waiver.
        let pins = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), "seven_day_waiver_registry_sha256 = \"abc\"\n");
        assert_eq!(pins.waiver_for(LOCK), None);
    }

    #[test]
    fn waiver_note_says_what_happened_to_the_waiver() {
        let named = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), &waiver_lines(&registry_hash_of(LOCK)));
        assert!(waiver_note(&named, LOCK).unwrap().contains("waived for this lock"));
        let other = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), &waiver_lines(&"b".repeat(64)));
        assert_eq!(waiver_note(&other, LOCK).as_deref(), Some("waiver ignored: it names another set of registry packages"));
        let none = pins_with_lock_lines("2026-10-22", &registry_hash_of(LOCK), "");
        assert_eq!(waiver_note(&none, LOCK), None);
    }

    // --- Stage 1 Task 2: the lock is judged by its registry packages (MC-118, G7, NB-13, S1G-3) ---

    /// A cut-down copy of the Stage 0 lock: the registry packages these tests name, with their real versions and
    /// checksums, and one path package.
    const STAGE0_LOCK: &str = r#"version = 4

[[package]]
name = "egui"
version = "0.37.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bec9144f49b7a28b5f395d3147e325b4fe3e77ba686eac11d1bdf0a7da3f375f"

[[package]]
name = "egui_tiles"
version = "0.18.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "c08803282261f981e84f422b9f716450caee4c2a0301278e27aab721dff99218"

[[package]]
name = "mxc-sdk"
version = "1.0.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "7c9e2d05b3953fdb34c44e4de8d8f2b447f4444534cb95294b72fb297a468f61"

[[package]]
name = "rmcp"
version = "3.6.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "ceb42e9e47443cd83a834dea57b5816d35481417bae72ade5dc5615c59122e26"

[[package]]
name = "serde"
version = "1.0.229"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"

[[package]]
name = "skipper-core"
version = "0.1.0"
dependencies = [
 "serde",
]
"#;

    const STAGE0_WAIVER: &str = "owner, 2026-10-22: Stage 0 starts without the 7-day wait (NB-13)";

    fn day(s: &str) -> Date {
        d(s)
    }

    /// The lock with one more path package: no source and no checksum, as cargo writes the workspace's own crates.
    fn add_path_package(lock: &str, name: &str) -> String {
        format!("{lock}\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
    }

    /// The lock with one more registry package; its checksum is derived from name and version.
    fn add_registry_package(lock: &str, name: &str, version: &str) -> String {
        let checksum = sha256_hex(format!("{name} {version}").as_bytes());
        format!("{lock}\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"{REGISTRY}\"\nchecksum = \"{checksum}\"\n")
    }

    /// The lock with its package tables in the opposite order.
    fn shuffle_packages(lock: &str) -> String {
        let mut parts: Vec<&str> = lock.split("[[package]]").collect();
        let head = parts.remove(0);
        parts.reverse();
        format!("{head}[[package]]{}", parts.join("[[package]]"))
    }

    /// The lock with the checksum of the named package replaced by zeros.
    fn change_checksum(lock: &str, name: &str) -> String {
        let key = "checksum = \"";
        let package = lock.find(&format!("name = \"{name}\"\n")).expect("the package is in the test lock");
        let at = package + lock[package..].find(key).expect("the package has a checksum") + key.len();
        format!("{}{}{}", &lock[..at], "0".repeat(64), &lock[at + 64..])
    }

    /// Pins for `lock` as the registry set resolved on `resolved`, without any waiver.
    fn pins_resolved(lock: &str, resolved: &str) -> Pins {
        Pins {
            toolchain: ToolchainPin::new("1.99.1", "x86_64-pc-windows-gnu", d("2026-10-15")),
            tools: vec![],
            crates: vec![
                CratePin::new("egui", "0.37.0", d("2026-10-15")),
                CratePin::new("egui_tiles", "0.18.0", d("2026-10-19")),
                CratePin::new("mxc-sdk", "1.0.1", d("2026-10-17")),
                CratePin::new("rmcp", "3.6.0", d("2026-10-20")),
                CratePin::new("serde", "1.0.229", d("2026-07-18")),
            ],
            lock_resolved: Some(d(resolved)),
            registry_sha256: Some(registry_sha256(lock).unwrap()),
            seven_day_waiver: None,
            seven_day_waiver_registry_sha256: None,
            waived: vec![],
        }
    }

    /// Pins as Stage 0 recorded them: resolved on 2026-10-22, with the owner's whole-lock waiver.
    fn pins_for(lock: &str) -> Pins {
        Pins {
            seven_day_waiver: Some(STAGE0_WAIVER.to_string()),
            seven_day_waiver_registry_sha256: Some(registry_sha256(lock).unwrap()),
            ..pins_resolved(lock, "2026-10-22")
        }
    }

    /// `pins_for(STAGE0_LOCK)` plus one `[[waived]]` entry for each package the Stage 0 waiver covered.
    fn pins_with_stage0_waived() -> Pins {
        let waived = registry_set(STAGE0_LOCK)
            .unwrap()
            .into_iter()
            .filter(|p| ["egui_tiles", "mxc-sdk", "rmcp"].contains(&p.name.as_str()))
            .map(|p| WaivedPkg {
                name: p.name,
                version: p.version,
                checksum: p.checksum,
                reason: STAGE0_WAIVER.to_string(),
                date: d("2026-10-22"),
            })
            .collect();
        Pins { waived, ..pins_for(STAGE0_LOCK) }
    }

    #[test] fn path_only_lock_change_keeps_resolution_date_and_waiver() { let l = add_path_package(STAGE0_LOCK, "skipper-provider"); let p = pins_for(STAGE0_LOCK); assert!(check_lock(&p, l.as_bytes(), day("2026-10-23")).is_ok()); assert!(p.waiver_for(l.as_bytes()).is_some()); }
    #[test] fn registry_change_needs_new_resolution_entry() { let l = add_registry_package(STAGE0_LOCK, "unicase", "2.10.0"); assert!(matches!(check_lock(&pins_for(STAGE0_LOCK), l.as_bytes(), day("2026-10-23")), Err(PinError::RegistryChanged { .. }))); }
    #[test] fn registry_change_recorded_today_is_too_young() { let l = add_registry_package(STAGE0_LOCK, "unicase", "2.10.0"); let p = pins_resolved(&l, "2026-10-23"); assert_eq!(check_lock(&p, l.as_bytes(), day("2026-10-25")), Err(PinError::LockTooYoung { earliest: day("2026-10-30") })); }
    #[test] fn waiver_binds_to_the_registry_hash() { let l = add_registry_package(STAGE0_LOCK, "unicase", "2.10.0"); assert!(pins_for(STAGE0_LOCK).waiver_for(l.as_bytes()).is_none()); }
    #[test] fn registry_hash_ignores_order_and_path_packages() { assert_eq!(registry_sha256(&shuffle_packages(STAGE0_LOCK)).unwrap(), registry_sha256(&add_path_package(STAGE0_LOCK, "x")).unwrap()); }
    #[test] fn checksum_change_counts_as_registry_change() { assert_ne!(registry_sha256(&change_checksum(STAGE0_LOCK, "serde")).unwrap(), registry_sha256(STAGE0_LOCK).unwrap()); }
    #[test] fn stage0_waiver_still_covers_egui_tiles_after_a_registry_change() { let l = add_registry_package(STAGE0_LOCK, "unicase", "2.10.0"); let young = young_packages(&l, &pins_with_stage0_waived(), day("2026-10-23")); assert!(!young.iter().any(|p| p.name == "egui_tiles")); }
    #[test] fn a_waived_entry_with_another_checksum_does_not_count() { let l = change_checksum(STAGE0_LOCK, "egui_tiles"); assert!(young_packages(&l, &pins_with_stage0_waived(), day("2026-10-23")).iter().any(|p| p.name == "egui_tiles")); }

    // --- beyond the brief's eight: the parts they rely on ---

    #[test]
    fn registry_set_lists_registry_packages_sorted_by_name_and_version() {
        let l = add_registry_package(&add_path_package(STAGE0_LOCK, "skipper-provider"), "adler2", "2.0.1");
        let found: Vec<String> =
            registry_set(&l).unwrap().into_iter().map(|p| format!("{} {}", p.name, p.version)).collect();
        assert_eq!(found, ["adler2 2.0.1", "egui 0.37.0", "egui_tiles 0.18.0", "mxc-sdk 1.0.1", "rmcp 3.6.0", "serde 1.0.229"]);
    }

    #[test]
    fn registry_package_without_checksum_or_from_another_source_is_refused() {
        let bare = format!("version = 4\n\n[[package]]\nname = \"egui\"\nversion = \"0.37.0\"\nsource = \"{REGISTRY}\"\n");
        assert!(registry_set(&bare).unwrap_err().contains("egui"));
        let sparse = "version = 4\n\n[[package]]\nname = \"egui\"\nversion = \"0.37.0\"\nsource = \"sparse+https://example.org/index/\"\nchecksum = \"aa\"\n";
        assert!(registry_set(sparse).unwrap_err().contains("egui"));
        assert!(registry_set("this is [ not toml").is_err());
    }

    #[test]
    fn waived_stage0_packages_do_not_hold_back_a_later_registry_change() {
        // A new package is recorded on 2026-10-23: the whole-lock waiver lapses, and rmcp (released 2026-10-20) is
        // still younger than 7 days on that day, but its [[waived]] entry carries it.
        let l = add_registry_package(STAGE0_LOCK, "unicase", "2.10.0");
        let carried = Pins { waived: pins_with_stage0_waived().waived, ..pins_resolved(&l, "2026-10-23") };
        assert_eq!(check_lock(&carried, l.as_bytes(), day("2026-10-30")), Ok(()));
        let bare = pins_resolved(&l, "2026-10-23");
        assert_eq!(
            check_lock(&bare, l.as_bytes(), day("2026-10-30")),
            Err(PinError::TooNew { name: "egui_tiles".into(), earliest: day("2026-10-26") })
        );
    }

    fn pins_text_with_waived_entry() -> String {
        format!(
            "{PINS_TOML}\n[[waived]]\nname = \"egui_tiles\"\nversion = \"0.18.0\"\nchecksum = \"{}\"\nreason = \"owner\"\ndate = 2026-10-22\n",
            "a".repeat(64)
        )
    }

    #[test]
    fn waived_entries_are_read_from_the_pins_file() {
        let pins = Pins::from_toml(&pins_text_with_waived_entry()).unwrap();
        let entry = WaivedPkg {
            name: "egui_tiles".into(),
            version: "0.18.0".into(),
            checksum: "a".repeat(64),
            reason: "owner".into(),
            date: d("2026-10-22"),
        };
        assert_eq!(pins.waived, vec![entry]);
        assert!(Pins::from_toml(PINS_TOML).unwrap().waived.is_empty());
    }

    #[test]
    fn waived_entry_with_a_bad_checksum_reason_or_date_is_reported() {
        let text = pins_text_with_waived_entry();
        for (old, new, word) in [
            ("a".repeat(64), "abc".to_string(), "checksum"),
            ("reason = \"owner\"".to_string(), "reason = \"\"".to_string(), "reason"),
            ("date = 2026-10-22".to_string(), "date = \"soon\"".to_string(), "date"),
        ] {
            let err = Pins::from_toml(&text.replace(&old, &new)).unwrap_err();
            assert!(err.contains("egui_tiles") && err.contains(word), "{err}");
        }
    }

    #[test]
    fn shipped_pins_file_matches_the_shipped_lock_by_its_registry_packages() {
        let pins = Pins::from_toml(include_str!("../../supply-chain/pins.toml")).unwrap();
        let lock = include_str!("../../Cargo.lock");
        assert_eq!(
            pins.registry_sha256,
            Some(registry_sha256(lock).unwrap()),
            "the registry packages of Cargo.lock changed: record the new registry_sha256 and resolved date in pins.toml"
        );
        let set = registry_set(lock).unwrap();
        for w in &pins.waived {
            let held = set.iter().any(|p| p.name == w.name && p.version == w.version && p.checksum == w.checksum);
            assert!(held, "[[waived]] {} {} names no package of Cargo.lock", w.name, w.version);
        }
    }

    // --- fix round 1 of the review of Task 2 ---

    fn lock_with_unicase() -> String {
        add_registry_package(STAGE0_LOCK, "unicase", "2.10.0")
    }

    /// The pins of a registry change recorded on 2026-10-23, with the Stage 0 `[[waived]]` entries.
    fn carried_pins(lock: &str) -> Pins {
        Pins { waived: pins_with_stage0_waived().waived, ..pins_resolved(lock, "2026-10-23") }
    }

    #[test]
    fn waived_notes_list_the_entries_check_lock_used() {
        // Resolved 2026-10-23 and built on 2026-10-30: no package is young on that day any more, but the three
        // entries are what lets check_lock pass its age check on the resolution day, so they are named.
        let l = lock_with_unicase();
        let pins = carried_pins(&l);
        assert_eq!(check_lock(&pins, l.as_bytes(), day("2026-10-30")), Ok(()));
        assert!(too_new_errors(&l, &pins, day("2026-10-30")).is_empty());
        let lines: Vec<String> = ["egui_tiles 0.18.0", "mxc-sdk 1.0.1", "rmcp 3.6.0"]
            .iter()
            .map(|p| format!("7-day wait waived for {p}: {STAGE0_WAIVER}"))
            .collect();
        assert_eq!(waived_package_notes(&pins, &l), lines);
    }

    #[test]
    fn waived_notes_name_only_covered_packages_that_were_young_on_the_resolution_day() {
        let l = lock_with_unicase();
        let carried = carried_pins(&l);
        // Nothing was young on 2026-11-01, so no entry was needed.
        let later = Pins { lock_resolved: Some(day("2026-11-01")), ..carried.clone() };
        assert!(waived_package_notes(&later, &l).is_empty());
        // No entry, no line (check_lock refuses the young packages instead).
        assert!(waived_package_notes(&pins_resolved(&l, "2026-10-23"), &l).is_empty());
        // An entry bound to another checksum covers nothing, so rmcp is not named.
        let changed = change_checksum(&l, "rmcp");
        let pins = Pins { registry_sha256: Some(registry_sha256(&changed).unwrap()), ..carried };
        let notes = waived_package_notes(&pins, &changed);
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|n| !n.contains("rmcp")));
    }

    #[test]
    fn waived_entries_never_shorten_the_wait_of_a_new_registry_set() {
        let l = lock_with_unicase();
        assert_eq!(
            check_lock(&carried_pins(&l), l.as_bytes(), day("2026-10-25")),
            Err(PinError::LockTooYoung { earliest: day("2026-10-30") })
        );
    }

    #[test]
    fn only_crates_io_is_a_registry_source() {
        for source in [
            "registry+https://example.org/index",
            "sparse+https://example.org/index/",
            "registry+https://github.com/rust-lang/crates.io-index/",
        ] {
            let why = registry_set(&lock_text(&[("egui", "0.37.0", source)])).unwrap_err();
            assert!(why.contains("egui") && why.contains(source), "{why}");
        }
    }

    #[test]
    fn check_lock_says_why_it_refuses_a_lock() {
        let pins = pins_with_lock(d("2026-10-27"), registry_hash_of(LOCK));
        let sparse = lock_text(&[("egui", "0.37.0", "sparse+https://example.org/index/")]);
        let no_sum = format!("version = 4\n\n[[package]]\nname = \"egui\"\nversion = \"0.37.0\"\nsource = \"{REGISTRY}\"\n");
        for (text, word) in [(&sparse, "sparse+https://example.org/index/"), (&no_sum, "without a checksum")] {
            match check_lock(&pins, text.as_bytes(), d("2026-11-10")) {
                Err(PinError::LockMismatch { locked, .. }) => {
                    assert!(locked.contains("egui") && locked.contains(word), "{locked}");
                }
                other => panic!("{other:?}"),
            }
        }
        // A file that is no lock at all is just unreadable.
        match check_lock(&pins, b"this is [ not toml", d("2026-11-10")) {
            Err(PinError::LockMismatch { locked, .. }) => assert_eq!(locked, "an unreadable file"),
            other => panic!("{other:?}"),
        }
    }
}
